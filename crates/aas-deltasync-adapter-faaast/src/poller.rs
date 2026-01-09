//! FA³ST polling-based change detection.

use aas_deltasync_adapter_aas::{AasClient, AasClientConfig};
use aas_deltasync_core::{Delta, Hlc};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;

/// Configuration for the FA³ST poller.
#[derive(Debug, Clone)]
pub struct FaaastPollerConfig {
    /// Base URL of the FA³ST server (must be HTTPS)
    pub base_url: String,
    /// Polling interval
    pub poll_interval: Duration,
    /// Custom CA certificate path (for self-signed certs)
    pub ca_cert_path: Option<PathBuf>,
    /// Client certificate path (for mTLS)
    pub client_cert_path: Option<PathBuf>,
    /// Client key path (for mTLS)
    pub client_key_path: Option<PathBuf>,
    /// Bearer token for authentication
    pub bearer_token: Option<String>,
}

impl Default for FaaastPollerConfig {
    fn default() -> Self {
        Self {
            base_url: "https://localhost:8443".to_string(),
            poll_interval: Duration::from_secs(5),
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            bearer_token: None,
        }
    }
}

/// Polling-based change detector for FA³ST.
pub struct FaaastPoller {
    client: AasClient,
    config: FaaastPollerConfig,
    /// Last known state per submodel
    snapshots: HashMap<String, Value>,
}

impl FaaastPoller {
    /// Create a new FA³ST poller.
    ///
    /// # Errors
    ///
    /// Returns error if the HTTP client cannot be created.
    pub fn new(config: FaaastPollerConfig) -> Result<Self, PollerError> {
        // Validate HTTPS requirement
        if !config.base_url.starts_with("https://") {
            return Err(PollerError::HttpsRequired);
        }

        let client_config = AasClientConfig {
            base_url: config.base_url.clone(),
            timeout: config.poll_interval * 2,
            bearer_token: config.bearer_token.clone(),
        };

        let client =
            AasClient::new(client_config).map_err(|e| PollerError::ClientInit(e.to_string()))?;

        Ok(Self {
            client,
            config,
            snapshots: HashMap::new(),
        })
    }

    /// Start polling and return a channel of deltas.
    #[must_use]
    pub fn start(
        mut self,
        submodel_ids: Vec<String>,
        clock: Hlc,
    ) -> mpsc::Receiver<(String, Delta<String, Value>)> {
        let (tx, rx) = mpsc::channel(100);
        let poll_interval = self.config.poll_interval;

        tokio::spawn(async move {
            let mut clock = clock;
            loop {
                for submodel_id in &submodel_ids {
                    match self.poll_submodel(submodel_id, &mut clock).await {
                        Ok(Some(delta)) => {
                            if tx.send((submodel_id.clone(), delta)).await.is_err() {
                                tracing::warn!("Delta receiver dropped, stopping poller");
                                return;
                            }
                        }
                        Ok(None) => {
                            // No changes
                        }
                        Err(e) => {
                            tracing::error!(submodel_id, error = %e, "Poll error");
                        }
                    }
                }

                tokio::time::sleep(poll_interval).await;
            }
        });

        rx
    }

    /// Poll a single submodel for changes.
    async fn poll_submodel(
        &mut self,
        submodel_id: &str,
        clock: &mut Hlc,
    ) -> Result<Option<Delta<String, Value>>, PollerError> {
        let current = self
            .client
            .get_submodel_value(submodel_id)
            .await
            .map_err(|e| PollerError::Fetch(e.to_string()))?;

        let previous = self.snapshots.get(submodel_id);

        let delta = if let Some(prev) = previous {
            // Compute diff
            let delta = compute_diff(prev, &current, clock);
            if delta.is_empty() {
                None
            } else {
                Some(delta)
            }
        } else {
            // First poll - generate delta for entire state
            let delta = value_to_delta(&current, "", clock);
            if delta.is_empty() {
                None
            } else {
                Some(delta)
            }
        };

        // Update snapshot
        self.snapshots.insert(submodel_id.to_string(), current);

        Ok(delta)
    }
}

/// Compute a diff between two JSON values.
fn compute_diff(old: &Value, new: &Value, clock: &mut Hlc) -> Delta<String, Value> {
    let mut delta = Delta::new();
    diff_values(old, new, "", &mut delta, clock);
    delta
}

/// Recursively diff two JSON values.
fn diff_values(
    old: &Value,
    new: &Value,
    path: &str,
    delta: &mut Delta<String, Value>,
    clock: &mut Hlc,
) {
    match (old, new) {
        (Value::Object(old_obj), Value::Object(new_obj)) => {
            // Check for removed keys
            for key in old_obj.keys() {
                if !new_obj.contains_key(key) {
                    let child_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    delta.add_remove(child_path, clock.tick());
                }
            }

            // Check for added/modified keys
            for (key, new_val) in new_obj {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };

                if let Some(old_val) = old_obj.get(key) {
                    // Recurse
                    diff_values(old_val, new_val, &child_path, delta, clock);
                } else {
                    // New key
                    delta.add_insert(child_path, new_val.clone(), clock.tick());
                }
            }
        }
        (Value::Array(old_arr), Value::Array(new_arr)) => {
            // For arrays, we do element-wise comparison by index
            // Note: This is simplified; production would use stable IDs
            let max_len = old_arr.len().max(new_arr.len());
            for i in 0..max_len {
                let child_path = format!("{path}[{i}]");
                match (old_arr.get(i), new_arr.get(i)) {
                    (Some(old_val), Some(new_val)) => {
                        diff_values(old_val, new_val, &child_path, delta, clock);
                    }
                    (Some(_), None) => {
                        delta.add_remove(child_path, clock.tick());
                    }
                    (None, Some(new_val)) => {
                        delta.add_insert(child_path, new_val.clone(), clock.tick());
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {
            // Scalar comparison
            if old != new {
                delta.add_insert(path.to_string(), new.clone(), clock.tick());
            }
        }
    }
}

/// Convert a JSON value to a delta (for initial snapshot).
fn value_to_delta(value: &Value, path: &str, clock: &mut Hlc) -> Delta<String, Value> {
    let mut delta = Delta::new();
    flatten_value(value, path, &mut delta, clock);
    delta
}

/// Flatten a JSON value into delta inserts.
fn flatten_value(value: &Value, path: &str, delta: &mut Delta<String, Value>, clock: &mut Hlc) {
    match value {
        Value::Object(obj) => {
            for (key, val) in obj {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                flatten_value(val, &child_path, delta, clock);
            }
        }
        Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                let child_path = format!("{path}[{i}]");
                flatten_value(val, &child_path, delta, clock);
            }
        }
        _ => {
            if !path.is_empty() {
                delta.add_insert(path.to_string(), value.clone(), clock.tick());
            }
        }
    }
}

/// Errors that can occur with the FA³ST poller.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PollerError {
    /// HTTPS is required but HTTP URL was provided
    #[error("FA³ST requires HTTPS per AAS v3.0 specification")]
    HttpsRequired,
    /// Client initialization failed
    #[error("client init error: {0}")]
    ClientInit(String),
    /// Fetch error
    #[error("fetch error: {0}")]
    Fetch(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn https_required() {
        let config = FaaastPollerConfig {
            base_url: "http://localhost:8080".to_string(),
            ..Default::default()
        };

        let result = FaaastPoller::new(config);
        assert!(matches!(result, Err(PollerError::HttpsRequired)));
    }

    #[test]
    fn diff_scalar_change() {
        let mut clock = Hlc::new(Uuid::new_v4());

        let old = serde_json::json!({"temperature": 25});
        let new = serde_json::json!({"temperature": 30});

        let delta = compute_diff(&old, &new, &mut clock);

        assert_eq!(delta.inserts.len(), 1);
        assert_eq!(delta.inserts[0].0, "temperature");
        assert_eq!(delta.inserts[0].1, serde_json::json!(30));
    }

    #[test]
    fn diff_key_removed() {
        let mut clock = Hlc::new(Uuid::new_v4());

        let old = serde_json::json!({"a": 1, "b": 2});
        let new = serde_json::json!({"a": 1});

        let delta = compute_diff(&old, &new, &mut clock);

        assert_eq!(delta.removes.len(), 1);
        assert_eq!(delta.removes[0].0, "b");
    }

    #[test]
    fn diff_key_added() {
        let mut clock = Hlc::new(Uuid::new_v4());

        let old = serde_json::json!({"a": 1});
        let new = serde_json::json!({"a": 1, "b": 2});

        let delta = compute_diff(&old, &new, &mut clock);

        assert_eq!(delta.inserts.len(), 1);
        assert_eq!(delta.inserts[0].0, "b");
    }
}
