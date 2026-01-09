//! # AAS-ΔSync Agent
//!
//! Synchronization agent runtime for offline-first, multi-master AAS digital twins.
//!
//! ## Architecture
//!
//! The agent implements five concurrent loops:
//! 1. **Ingress**: Receives events from adapters (BaSyx MQTT, FA³ST polling)
//! 2. **Mutation**: Converts events to CRDT deltas and applies locally
//! 3. **Replication**: Publishes deltas to MQTT and handles anti-entropy
//! 4. **Egress**: Pushes converged state back to AAS server (optional)
//! 5. **Persistence**: Snapshots and compacts delta log

use aas_deltasync_core::Hlc;
use anyhow::Result;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

mod config;
mod persistence;
mod replication;
mod runtime;

pub use config::AgentConfig;
pub use runtime::Agent;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting AAS-ΔSync Agent"
    );

    // Load configuration
    let config = AgentConfig::from_env()?;

    // Create agent
    let agent_id = config.agent_id.unwrap_or_else(Uuid::new_v4);
    let clock = Hlc::new(agent_id);

    tracing::info!(%agent_id, "Agent initialized");

    let agent = Agent::new(config, clock)?;

    // Run agent
    agent.run().await?;

    Ok(())
}
