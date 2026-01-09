//! Agent runtime orchestration.

use crate::config::{AgentConfig, SubscriptionConfig};
use crate::persistence::SqliteStore;
use crate::replication::ReplicationManager;
use aas_deltasync_adapter_aas::{AasClient, AasClientConfig};
use aas_deltasync_core::{Delta, Hlc, OrMap, Timestamp};
use aas_deltasync_proto::topics::MessageType;
use aas_deltasync_proto::{DocDelta, TopicScheme};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::time::Duration;
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
            };
            Some(AasClient::new(config).context("Failed to create AAS client")?)
        } else {
            None
        };

        // Initialize replication
        let (replication, mut eventloop) = ReplicationManager::new(
            &self.config.replication.mqtt_broker,
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

                            if msg_type != MessageType::Delta {
                                continue;
                            }

                            let doc_delta = match DocDelta::from_cbor(&publish.payload) {
                                Ok(delta) => delta,
                                Err(err) => {
                                    tracing::warn!(error = %err, "Failed to decode DocDelta");
                                    continue;
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
                                        continue;
                                    }
                                };

                            let doc_state = documents
                                .entry(doc_delta.doc_id.clone())
                                .or_insert_with(|| DocumentState::new(actor_id));
                            doc_state.apply_delta(&delta);

                            if let Ok(timestamp) = doc_delta.timestamp() {
                                persist_delta(self.store.as_ref(), &doc_delta, timestamp);
                                update_peer_progress(self.store.as_ref(), &doc_delta, timestamp);
                            }

                            if let Some(aas_client) = aas_client.as_ref() {
                                if let Some(sub) = subscriptions.get(&doc_delta.doc_id) {
                                    apply_delta_egress(
                                        aas_client,
                                        sub,
                                        &delta,
                                    )
                                    .await;
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
