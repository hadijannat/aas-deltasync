//! Agent runtime orchestration.

use crate::config::{AgentConfig, SubscriptionConfig};
use crate::persistence::SqliteStore;
use crate::replication::ReplicationManager;
use aas_deltasync_adapter_aas::{AasClient, AasClientConfig};
use aas_deltasync_adapter_basyx::{BasyxEvent, BasyxSubscriber, BasyxSubscriberConfig, EventType};
use aas_deltasync_core::{Delta, Hlc, OrMap, Timestamp};
use aas_deltasync_proto::topics::MessageType;
use aas_deltasync_proto::{AntiEntropyRequest, AntiEntropyResponse, DocDelta, TopicScheme};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug)]
struct DocumentState {
    state: OrMap<String, serde_json::Value>,
    clock: Hlc,
}

impl DocumentState {
    fn new(actor_id: Uuid) -> Self {
        Self {
            state: OrMap::new(),
            clock: Hlc::new(actor_id),
        }
    }

    fn apply_delta(&mut self, delta: &Delta<String, serde_json::Value>) {
        for (_, _, timestamp) in &delta.inserts {
            self.clock.update(*timestamp);
        }
        for (_, timestamp) in &delta.removes {
            self.clock.update(*timestamp);
        }

        delta.apply_to(&mut self.state);
    }
}

/// The main agent runtime.
pub struct Agent {
    config: AgentConfig,
    clock: Hlc,
    store: Option<SqliteStore>,
}

impl Agent {
    /// Create a new agent.
    ///
    /// # Errors
    ///
    /// Returns error if initialization fails.
    pub fn new(config: AgentConfig, clock: Hlc) -> Result<Self> {
        let store = if config.persistence.store_type == "sqlite" {
            Some(
                SqliteStore::open(&config.persistence.db_path)
                    .context("Failed to open SQLite database")?,
            )
        } else {
            None
        };

        Ok(Self {
            config,
            clock,
            store,
        })
    }

    /// Run the agent's main loop.
    ///
    /// # Errors
    ///
    /// Returns error if any component fails.
    #[allow(clippy::too_many_lines)]
    pub async fn run(self) -> Result<()> {
        tracing::info!("Starting agent runtime");

        let topic_scheme = TopicScheme::new(&self.config.replication.tenant);
        let actor_id = self.clock.actor_id();

        let mut subscriptions = HashMap::<String, SubscriptionConfig>::new();
        let mut documents = HashMap::<String, DocumentState>::new();

        for sub in &self.config.subscriptions {
            let doc_id = format!("{}:{}", sub.aas_id, sub.submodel_id);
            subscriptions.insert(doc_id.clone(), sub.clone());
            documents
                .entry(doc_id)
                .or_insert_with(|| DocumentState::new(actor_id));
        }

        let aas_client = if self.config.replication.enable_egress {
            let config = AasClientConfig {
                base_url: self.config.adapter.sm_repo_url.clone(),
                timeout: Duration::from_secs(30),
                bearer_token: self.config.adapter.bearer_token.clone(),
                ca_cert_path: self.config.adapter.aas_ca_path.clone(),
                client_cert_path: self.config.adapter.aas_client_cert_path.clone(),
                client_key_path: self.config.adapter.aas_client_key_path.clone(),
            };
            Some(AasClient::new(config).context("Failed to create AAS client")?)
        } else {
            None
        };

        // Initialize replication
        let (replication, mut eventloop) = ReplicationManager::new(
            &self.config.replication.mqtt_broker,
            self.config.replication.mqtt_ca_path.as_deref(),
            &format!("aas-deltasync-{}", self.clock.actor_id()),
            topic_scheme.clone(),
        )
        .context("Failed to create replication manager")?;

        // Subscribe to document topics
        for sub in &self.config.subscriptions {
            let doc_id = format!("{}:{}", sub.aas_id, sub.submodel_id);
            let doc_hash = hash_doc_id(&doc_id);
            replication.subscribe(&doc_hash).await?;
        }

        // Initialize BaSyx subscriber if adapter type is basyx
        let basyx_rx: Option<mpsc::Receiver<Result<BasyxEvent, _>>> =
            if self.config.adapter.adapter_type == "basyx" {
                if let Some(mqtt_broker) = &self.config.adapter.mqtt_broker {
                    let basyx_config = BasyxSubscriberConfig {
                        mqtt_broker: mqtt_broker.clone(),
                        mqtt_ca_path: self.config.adapter.mqtt_ca_path.clone(),
                        client_id: format!("aas-deltasync-basyx-{actor_id}"),
                        repo_id: "sm-repo".to_string(),
                        keep_alive: Duration::from_secs(30),
                    };

                    let subscriber = BasyxSubscriber::new(basyx_config)
                        .context("Failed to create BaSyx subscriber")?;
                    subscriber
                        .subscribe()
                        .await
                        .context("Failed to subscribe to BaSyx events")?;

                    tracing::info!("BaSyx event ingestion enabled");
                    Some(subscriber.start())
                } else {
                    tracing::warn!("BaSyx adapter selected but no MQTT broker configured");
                    None
                }
            } else {
                None
            };

        // Wrap in Option for the select! loop
        let mut basyx_rx = basyx_rx;

        tracing::info!("Agent running, press Ctrl+C to stop");

        // Main event loop
        loop {
            tokio::select! {
                // Handle MQTT events
                event = eventloop.poll() => {
                    match event {
                        Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(publish))) => {
                            tracing::debug!(
                                topic = %publish.topic,
                                payload_len = publish.payload.len(),
                                "Received replication message"
                            );
                            let Some((doc_hash, msg_type)) = topic_scheme.parse(&publish.topic) else {
                                continue;
                            };

                            match msg_type {
                                MessageType::Delta => {
                                    handle_delta_message(
                                        &publish.payload,
                                        &doc_hash,
                                        actor_id,
                                        &mut documents,
                                        &subscriptions,
                                        aas_client.as_ref(),
                                        self.store.as_ref(),
                                    ).await;
                                }
                                MessageType::AntiEntropyRequest => {
                                    handle_ae_request(
                                        &publish.payload,
                                        &doc_hash,
                                        &replication,
                                        self.store.as_ref(),
                                    ).await;
                                }
                                MessageType::AntiEntropyResponse => {
                                    handle_ae_response(
                                        &publish.payload,
                                        actor_id,
                                        &mut documents,
                                        self.store.as_ref(),
                                    );
                                }
                                MessageType::Hello => {
                                    tracing::debug!(
                                        doc_hash = %doc_hash,
                                        "Received agent hello (peer discovery not yet implemented)"
                                    );
                                }
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!(error = %e, "MQTT error");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }

                // Handle BaSyx ingestion events
                Some(event_result) = async {
                    match basyx_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match event_result {
                        Ok(basyx_event) => {
                            handle_basyx_event(
                                &basyx_event,
                                actor_id,
                                &mut documents,
                                &subscriptions,
                                &replication,
                                &topic_scheme,
                                self.store.as_ref(),
                            ).await;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to parse BaSyx event");
                        }
                    }
                }

                // Handle shutdown
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Shutdown signal received");
                    break;
                }
            }
        }

        tracing::info!("Agent stopped");
        Ok(())
    }
}

fn persist_delta(store: Option<&SqliteStore>, delta: &DocDelta, timestamp: Timestamp) {
    if let Some(store) = store {
        if let Err(err) = store.save_delta(
            &delta.doc_id,
            &delta.delta_id,
            &delta.delta_payload,
            &timestamp.actor_id.to_string(),
            timestamp.physical_ms,
        ) {
            tracing::warn!(error = %err, doc_id = %delta.doc_id, "Failed to persist delta");
        }
    }
}

fn update_peer_progress(store: Option<&SqliteStore>, delta: &DocDelta, timestamp: Timestamp) {
    if let Some(store) = store {
        if let Err(err) = store.update_peer_progress(
            &timestamp.actor_id.to_string(),
            &delta.doc_id,
            &delta.delta_id,
        ) {
            tracing::warn!(error = %err, doc_id = %delta.doc_id, "Failed to update peer progress");
        }
    }
}

async fn apply_delta_egress(
    client: &AasClient,
    sub: &SubscriptionConfig,
    delta: &Delta<String, serde_json::Value>,
) {
    for (path, value, _) in &delta.inserts {
        if let Err(err) = client
            .patch_submodel_element_value(&sub.submodel_id, path, value)
            .await
        {
            tracing::warn!(
                error = %err,
                submodel_id = %sub.submodel_id,
                path,
                "Failed to apply delta insert via egress"
            );
        }
    }

    for (path, _) in &delta.removes {
        tracing::debug!(
            submodel_id = %sub.submodel_id,
            path,
            "Skipping remove in egress (no delete API wired)"
        );
    }
}

/// Hash a document ID for topic sharding.
fn hash_doc_id(doc_id: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    doc_id.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Handle a Delta message from the replication stream.
async fn handle_delta_message(
    payload: &[u8],
    doc_hash: &str,
    actor_id: Uuid,
    documents: &mut HashMap<String, DocumentState>,
    subscriptions: &HashMap<String, SubscriptionConfig>,
    aas_client: Option<&AasClient>,
    store: Option<&SqliteStore>,
) {
    let doc_delta = match DocDelta::from_cbor(payload) {
        Ok(delta) => delta,
        Err(err) => {
            tracing::warn!(error = %err, "Failed to decode DocDelta");
            return;
        }
    };

    let expected_hash = hash_doc_id(&doc_delta.doc_id);
    if doc_hash != expected_hash {
        tracing::warn!(
            doc_id = %doc_delta.doc_id,
            topic_hash = %doc_hash,
            expected_hash = %expected_hash,
            "Doc hash mismatch for delta"
        );
    }

    let delta: Delta<String, serde_json::Value> =
        match ciborium::from_reader(doc_delta.delta_payload.as_slice()) {
            Ok(delta) => delta,
            Err(err) => {
                tracing::warn!(error = %err, "Failed to decode delta payload");
                return;
            }
        };

    let doc_state = documents
        .entry(doc_delta.doc_id.clone())
        .or_insert_with(|| DocumentState::new(actor_id));
    doc_state.apply_delta(&delta);

    if let Ok(timestamp) = doc_delta.timestamp() {
        persist_delta(store, &doc_delta, timestamp);
        update_peer_progress(store, &doc_delta, timestamp);
    }

    if let Some(aas_client) = aas_client {
        if let Some(sub) = subscriptions.get(&doc_delta.doc_id) {
            apply_delta_egress(aas_client, sub, &delta).await;
        }
    }

    tracing::debug!(
        doc_id = %doc_delta.doc_id,
        inserts = delta.inserts.len(),
        removes = delta.removes.len(),
        "Applied delta from replication"
    );
}

/// Handle a `BaSyx` event by converting to delta and publishing.
async fn handle_basyx_event(
    event: &BasyxEvent,
    actor_id: Uuid,
    documents: &mut HashMap<String, DocumentState>,
    subscriptions: &HashMap<String, SubscriptionConfig>,
    replication: &ReplicationManager,
    _topic_scheme: &TopicScheme,
    store: Option<&SqliteStore>,
) {
    // Find matching subscription by submodel_id
    let doc_id = subscriptions
        .iter()
        .find(|(_, sub)| sub.submodel_id == event.submodel_id)
        .map(|(doc_id, _)| doc_id.clone());

    let Some(doc_id) = doc_id else {
        tracing::debug!(
            submodel_id = %event.submodel_id,
            "Ignoring BaSyx event for unsubscribed submodel"
        );
        return;
    };

    // Get or create document state
    let doc_state = documents
        .entry(doc_id.clone())
        .or_insert_with(|| DocumentState::new(actor_id));

    // Convert BasyxEvent to Delta
    let delta = basyx_event_to_delta(event, &mut doc_state.clock);

    if delta.is_empty() {
        return;
    }

    // Apply delta locally
    doc_state.apply_delta(&delta);

    // Serialize delta payload
    let mut delta_payload = Vec::new();
    if let Err(err) = ciborium::into_writer(&delta, &mut delta_payload) {
        tracing::warn!(error = %err, "Failed to serialize delta");
        return;
    }

    // Create and publish DocDelta
    let timestamp = doc_state.clock.current();
    let doc_delta = DocDelta::new(doc_id.clone(), timestamp, delta_payload);
    let doc_hash = hash_doc_id(&doc_id);

    if let Err(err) = replication.publish_delta(&doc_hash, &doc_delta).await {
        tracing::warn!(error = %err, "Failed to publish delta from BaSyx event");
    }

    // Persist delta
    persist_delta(store, &doc_delta, timestamp);

    tracing::debug!(
        doc_id = %doc_id,
        event_type = ?event.event_type,
        inserts = delta.inserts.len(),
        removes = delta.removes.len(),
        "Processed BaSyx event"
    );
}

/// Convert a `BaSyx` event to a CRDT delta.
fn basyx_event_to_delta(event: &BasyxEvent, clock: &mut Hlc) -> Delta<String, serde_json::Value> {
    let mut delta = Delta::new();

    let Some(element) = &event.element else {
        return delta;
    };

    // Convert idShortPath slashes to dots for CRDT key
    let path = element.id_short_path.replace('/', ".");

    match event.event_type {
        EventType::Created | EventType::Updated => {
            if let Some(value) = &element.value {
                delta.add_insert(path, value.clone(), clock.tick());
            }
        }
        EventType::Deleted => {
            delta.add_remove(path, clock.tick());
        }
        EventType::Patched => {
            // For patched events, the payload may contain multiple changes
            // For now, treat as a single update if value is present
            if let Some(value) = &element.value {
                delta.add_insert(path, value.clone(), clock.tick());
            }
        }
    }

    delta
}

/// Handle an anti-entropy request by querying persistence and responding.
async fn handle_ae_request(
    payload: &[u8],
    doc_hash: &str,
    replication: &ReplicationManager,
    store: Option<&SqliteStore>,
) {
    let request = match AntiEntropyRequest::from_cbor(payload) {
        Ok(req) => req,
        Err(err) => {
            tracing::warn!(error = %err, "Failed to decode AntiEntropyRequest");
            return;
        }
    };

    tracing::debug!(
        doc_id = %request.doc_id,
        "Processing anti-entropy request"
    );

    let Some(store) = store else {
        tracing::debug!("No persistence store, cannot respond to AE request");
        return;
    };

    // Parse the have_summary as a timestamp threshold
    let after_ts: u64 = if request.have_summary.len() >= 8 {
        u64::from_be_bytes(request.have_summary[..8].try_into().unwrap_or([0u8; 8]))
    } else {
        0
    };

    // Query persistence for deltas after the requester's timestamp
    let delta_bytes = match store.get_deltas_after(&request.doc_id, after_ts) {
        Ok(deltas) => deltas,
        Err(err) => {
            tracing::warn!(error = %err, "Failed to query deltas for AE");
            return;
        }
    };

    if delta_bytes.is_empty() {
        tracing::debug!(doc_id = %request.doc_id, "No missing deltas to send");
        return;
    }

    // Convert raw bytes to DocDeltas
    let mut deltas = Vec::new();
    for bytes in delta_bytes {
        if let Ok(doc_delta) = DocDelta::from_cbor(&bytes) {
            deltas.push(doc_delta);
        }
    }

    let response = AntiEntropyResponse::with_deltas(request.doc_id.clone(), deltas.clone());

    if let Err(err) = replication.publish_ae_response(doc_hash, &response).await {
        tracing::warn!(error = %err, "Failed to publish AE response");
    } else {
        tracing::info!(
            doc_id = %request.doc_id,
            deltas_count = deltas.len(),
            "Sent anti-entropy response"
        );
    }
}

/// Handle an anti-entropy response by applying received deltas.
fn handle_ae_response(
    payload: &[u8],
    actor_id: Uuid,
    documents: &mut HashMap<String, DocumentState>,
    store: Option<&SqliteStore>,
) {
    let response = match AntiEntropyResponse::from_cbor(payload) {
        Ok(resp) => resp,
        Err(err) => {
            tracing::warn!(error = %err, "Failed to decode AntiEntropyResponse");
            return;
        }
    };

    tracing::debug!(
        doc_id = %response.doc_id,
        deltas_count = response.deltas.len(),
        has_snapshot = response.snapshot.is_some(),
        "Processing anti-entropy response"
    );

    let doc_state = documents
        .entry(response.doc_id.clone())
        .or_insert_with(|| DocumentState::new(actor_id));

    // Apply snapshot if provided (takes precedence)
    if let Some(snapshot_bytes) = &response.snapshot {
        if let Ok(state) =
            ciborium::from_reader::<OrMap<String, serde_json::Value>, _>(snapshot_bytes.as_slice())
        {
            doc_state.state = state;
            tracing::info!(doc_id = %response.doc_id, "Applied snapshot from AE response");
        }
    }

    // Apply deltas
    let mut applied_count = 0;
    for doc_delta in &response.deltas {
        let delta: Delta<String, serde_json::Value> =
            match ciborium::from_reader(doc_delta.delta_payload.as_slice()) {
                Ok(d) => d,
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to decode delta from AE response");
                    continue;
                }
            };

        doc_state.apply_delta(&delta);
        applied_count += 1;

        // Persist the delta
        if let Ok(timestamp) = doc_delta.timestamp() {
            persist_delta(store, doc_delta, timestamp);
            update_peer_progress(store, doc_delta, timestamp);
        }
    }

    tracing::info!(
        doc_id = %response.doc_id,
        applied_count,
        "Anti-entropy sync complete"
    );
}
