//! Agent runtime orchestration.

use crate::config::AgentConfig;
use crate::persistence::SqliteStore;
use crate::replication::ReplicationManager;
use aas_deltasync_core::Hlc;
use aas_deltasync_proto::TopicScheme;
use anyhow::{Context, Result};

/// The main agent runtime.
pub struct Agent {
    config: AgentConfig,
    clock: Hlc,
    #[allow(dead_code)]
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
    pub async fn run(self) -> Result<()> {
        tracing::info!("Starting agent runtime");

        let topic_scheme = TopicScheme::new(&self.config.replication.tenant);

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
                            // TODO: Process incoming delta
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

/// Hash a document ID for topic sharding.
fn hash_doc_id(doc_id: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    doc_id.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
