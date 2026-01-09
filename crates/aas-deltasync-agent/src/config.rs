//! Agent configuration.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

/// Agent configuration.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Agent unique identifier
    pub agent_id: Option<Uuid>,

    /// Adapter configuration
    pub adapter: AdapterConfig,

    /// Replication configuration
    pub replication: ReplicationConfig,

    /// Persistence configuration
    pub persistence: PersistenceConfig,

    /// Subscriptions to synchronize
    pub subscriptions: Vec<SubscriptionConfig>,
}

/// Adapter configuration.
#[derive(Debug, Clone)]
pub struct AdapterConfig {
    /// Adapter type: "basyx" or "faaast"
    pub adapter_type: String,

    /// AAS repository URL
    pub aas_repo_url: Option<String>,

    /// Submodel repository URL
    pub sm_repo_url: String,

    /// MQTT broker URL (for BaSyx)
    pub mqtt_broker: Option<String>,

    /// Bearer token for authentication
    pub bearer_token: Option<String>,

    /// Poll interval (for FA³ST)
    pub poll_interval: Duration,
}

/// Replication configuration.
#[derive(Debug, Clone)]
pub struct ReplicationConfig {
    /// MQTT broker URL for delta replication
    pub mqtt_broker: String,

    /// Tenant identifier
    pub tenant: String,

    /// Enable egress (push back to AAS server)
    pub enable_egress: bool,
}

/// Persistence configuration.
#[derive(Debug, Clone)]
pub struct PersistenceConfig {
    /// Persistence type: "sqlite" or "memory"
    pub store_type: String,

    /// Database path (for SQLite)
    pub db_path: PathBuf,

    /// Compaction interval
    pub compaction_interval: Duration,
}

/// Subscription configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionConfig {
    /// AAS identifier
    pub aas_id: String,

    /// Submodel identifier
    pub submodel_id: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            agent_id: None,
            adapter: AdapterConfig {
                adapter_type: "basyx".to_string(),
                aas_repo_url: Some("http://localhost:8081".to_string()),
                sm_repo_url: "http://localhost:8082".to_string(),
                mqtt_broker: Some("tcp://localhost:1883".to_string()),
                bearer_token: None,
                poll_interval: Duration::from_secs(5),
            },
            replication: ReplicationConfig {
                mqtt_broker: "tcp://localhost:1883".to_string(),
                tenant: "default".to_string(),
                enable_egress: false,
            },
            persistence: PersistenceConfig {
                store_type: "sqlite".to_string(),
                db_path: PathBuf::from("./deltasync.db"),
                compaction_interval: Duration::from_secs(3600),
            },
            subscriptions: Vec::new(),
        }
    }
}

impl AgentConfig {
    /// Load configuration from environment variables.
    ///
    /// # Environment Variables
    ///
    /// - `DELTASYNC_AGENT_ID`: Agent UUID
    /// - `DELTASYNC_ADAPTER_TYPE`: "basyx" or "faaast"
    /// - `DELTASYNC_SM_REPO_URL`: Submodel repository URL
    /// - `DELTASYNC_MQTT_BROKER`: MQTT broker URL
    /// - `DELTASYNC_TENANT`: Tenant identifier
    /// - `DELTASYNC_DB_PATH`: SQLite database path
    ///
    /// # Errors
    ///
    /// Returns error if required environment variables are missing.
    pub fn from_env() -> Result<Self> {
        let mut config = Self::default();

        if let Ok(id) = std::env::var("DELTASYNC_AGENT_ID") {
            config.agent_id = Some(Uuid::parse_str(&id).context("Invalid DELTASYNC_AGENT_ID")?);
        }

        if let Ok(adapter_type) = std::env::var("DELTASYNC_ADAPTER_TYPE") {
            config.adapter.adapter_type = adapter_type;
        }

        if let Ok(url) = std::env::var("DELTASYNC_SM_REPO_URL") {
            config.adapter.sm_repo_url = url;
        }

        if let Ok(url) = std::env::var("DELTASYNC_AAS_REPO_URL") {
            config.adapter.aas_repo_url = Some(url);
        }

        if let Ok(mqtt) = std::env::var("DELTASYNC_MQTT_BROKER") {
            config.adapter.mqtt_broker = Some(mqtt.clone());
            config.replication.mqtt_broker = mqtt;
        }

        if let Ok(tenant) = std::env::var("DELTASYNC_TENANT") {
            config.replication.tenant = tenant;
        }

        if let Ok(db_path) = std::env::var("DELTASYNC_DB_PATH") {
            config.persistence.db_path = PathBuf::from(db_path);
        }

        if let Ok(token) = std::env::var("DELTASYNC_BEARER_TOKEN") {
            config.adapter.bearer_token = Some(token);
        }

        // Parse subscriptions from JSON env var
        if let Ok(subs_json) = std::env::var("DELTASYNC_SUBSCRIPTIONS") {
            config.subscriptions =
                serde_json::from_str(&subs_json).context("Invalid DELTASYNC_SUBSCRIPTIONS JSON")?;
        }

        Ok(config)
    }
}
