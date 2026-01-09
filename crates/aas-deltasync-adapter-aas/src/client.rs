//! HTTP client for AAS Part 2 API.
//!
//! Provides a minimal interface for interacting with AAS servers,
//! using the correct encoding rules for identifiers and paths.

use super::encoding::{encode_id_base64url, encode_idshort_path};
use reqwest::Client;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// AAS HTTP client configuration.
#[derive(Debug, Clone)]
pub struct AasClientConfig {
    /// Base URL of the AAS server (e.g., <http://localhost:8081>)
    pub base_url: String,
    /// Request timeout
    pub timeout: Duration,
    /// Optional bearer token for authentication
    pub bearer_token: Option<String>,
    /// Custom CA certificate path for self-signed server certs (PEM format)
    pub ca_cert_path: Option<PathBuf>,
    /// Client certificate path for mTLS authentication (PEM format)
    pub client_cert_path: Option<PathBuf>,
    /// Client private key path for mTLS authentication (PEM format)
    pub client_key_path: Option<PathBuf>,
}

impl Default for AasClientConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8081".to_string(),
            timeout: Duration::from_secs(30),
            bearer_token: None,
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
        }
    }
}

/// HTTP client for AAS Part 2 API operations.
pub struct AasClient {
    client: Client,
    config: AasClientConfig,
}

impl AasClient {
    /// Create a new AAS client.
    ///
    /// # Errors
    ///
    /// Returns error if the HTTP client cannot be created, or if TLS
    /// certificate files cannot be read or parsed.
    pub fn new(config: AasClientConfig) -> Result<Self, ClientError> {
        let mut builder = Client::builder().timeout(config.timeout);

        if config.base_url.starts_with("https://") {
            // Enable rustls for HTTPS (required for FA³ST)
            builder = builder.use_rustls_tls();

            // Load custom CA certificate if provided (for self-signed certs)
            if let Some(ca_path) = &config.ca_cert_path {
                let ca_cert = fs::read(ca_path).map_err(|e| {
                    ClientError::Init(format!(
                        "failed to read CA certificate {}: {e}",
                        ca_path.display()
                    ))
                })?;
                let cert = reqwest::Certificate::from_pem(&ca_cert).map_err(|e| {
                    ClientError::Init(format!("failed to parse CA certificate: {e}"))
                })?;
                builder = builder.add_root_certificate(cert);
                tracing::debug!(ca_path = %ca_path.display(), "Loaded custom CA certificate");
            }

            // Load client certificate and key for mTLS if both are provided
            if let (Some(cert_path), Some(key_path)) =
                (&config.client_cert_path, &config.client_key_path)
            {
                let cert_pem = fs::read(cert_path).map_err(|e| {
                    ClientError::Init(format!(
                        "failed to read client certificate {}: {e}",
                        cert_path.display()
                    ))
                })?;
                let key_pem = fs::read(key_path).map_err(|e| {
                    ClientError::Init(format!(
                        "failed to read client key {}: {e}",
                        key_path.display()
                    ))
                })?;

                // Combine cert and key into a single PEM for identity
                let mut identity_pem = cert_pem;
                identity_pem.extend_from_slice(&key_pem);

                let identity = reqwest::Identity::from_pem(&identity_pem).map_err(|e| {
                    ClientError::Init(format!("failed to create client identity: {e}"))
                })?;
                builder = builder.identity(identity);
                tracing::debug!(
                    cert_path = %cert_path.display(),
                    key_path = %key_path.display(),
                    "Loaded client certificate for mTLS"
                );
            }
        }

        let client = builder
            .build()
            .map_err(|e| ClientError::Init(e.to_string()))?;

        Ok(Self { client, config })
    }

    /// Build the authorization header if configured.
    fn auth_header(&self) -> Option<String> {
        self.config
            .bearer_token
            .as_ref()
            .map(|t| format!("Bearer {t}"))
    }

    /// Get the $value view of a submodel.
    ///
    /// # Errors
    ///
    /// Returns error on network or API errors.
    pub async fn get_submodel_value(&self, submodel_id: &str) -> Result<Value, ClientError> {
        let encoded_id = encode_id_base64url(submodel_id);
        let url = format!("{}/submodels/{}/$value", self.config.base_url, encoded_id);

        tracing::debug!(submodel_id, url, "GET submodel $value");

        let mut request = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ClientError::Request(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        response
            .json()
            .await
            .map_err(|e| ClientError::Parse(e.to_string()))
    }

    /// Get the value of a specific submodel element.
    ///
    /// # Errors
    ///
    /// Returns error on network or API errors.
    pub async fn get_submodel_element_value(
        &self,
        submodel_id: &str,
        id_short_path: &str,
    ) -> Result<Value, ClientError> {
        let encoded_sm_id = encode_id_base64url(submodel_id);
        let encoded_path = encode_idshort_path(id_short_path);
        let url = format!(
            "{}/submodels/{}/submodel-elements/{}/$value",
            self.config.base_url, encoded_sm_id, encoded_path
        );

        tracing::debug!(submodel_id, id_short_path, url, "GET element $value");

        let mut request = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ClientError::Request(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        response
            .json()
            .await
            .map_err(|e| ClientError::Parse(e.to_string()))
    }

    /// Patch the value of a specific submodel element.
    ///
    /// Uses the `$value` content modifier for minimal payload.
    ///
    /// # Errors
    ///
    /// Returns error on network or API errors.
    pub async fn patch_submodel_element_value(
        &self,
        submodel_id: &str,
        id_short_path: &str,
        value: &Value,
    ) -> Result<(), ClientError> {
        let encoded_sm_id = encode_id_base64url(submodel_id);
        let encoded_path = encode_idshort_path(id_short_path);
        let url = format!(
            "{}/submodels/{}/submodel-elements/{}/$value",
            self.config.base_url, encoded_sm_id, encoded_path
        );

        tracing::debug!(submodel_id, id_short_path, url, "PATCH element $value");

        let mut request = self
            .client
            .patch(&url)
            .header("Content-Type", "application/json")
            .json(value);

        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ClientError::Request(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        Ok(())
    }

    /// Get all submodel descriptors from a submodel repository.
    ///
    /// # Errors
    ///
    /// Returns error on network or API errors.
    pub async fn list_submodels(&self) -> Result<Vec<Value>, ClientError> {
        let url = format!("{}/submodels", self.config.base_url);

        tracing::debug!(url, "GET submodels list");

        let mut request = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            request = request.header("Authorization", auth);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ClientError::Request(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClientError::ApiError {
                status: response.status().as_u16(),
                message: response.text().await.unwrap_or_default(),
            });
        }

        // AAS API returns paginated results with "result" array
        let body: Value = response
            .json()
            .await
            .map_err(|e| ClientError::Parse(e.to_string()))?;

        if let Some(result) = body.get("result").and_then(|r| r.as_array()) {
            Ok(result.clone())
        } else if let Some(arr) = body.as_array() {
            Ok(arr.clone())
        } else {
            Ok(vec![body])
        }
    }
}

/// Errors that can occur with the AAS client.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ClientError {
    /// Client initialization failed
    #[error("client init error: {0}")]
    Init(String),
    /// HTTP request failed
    #[error("request error: {0}")]
    Request(String),
    /// API returned an error status
    #[error("API error (status {status}): {message}")]
    ApiError {
        /// HTTP status code
        status: u16,
        /// Error message from API
        message: String,
    },
    /// Response parsing failed
    #[error("parse error: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let config = AasClientConfig::default();
        assert_eq!(config.base_url, "http://localhost:8081");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert!(config.bearer_token.is_none());
        assert!(config.ca_cert_path.is_none());
        assert!(config.client_cert_path.is_none());
        assert!(config.client_key_path.is_none());
    }

    #[test]
    fn client_creation() {
        let config = AasClientConfig::default();
        let client = AasClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn config_with_tls_fields() {
        let config = AasClientConfig {
            base_url: "https://localhost:8443".to_string(),
            timeout: Duration::from_secs(30),
            bearer_token: None,
            ca_cert_path: Some(PathBuf::from("/tmp/ca.pem")),
            client_cert_path: Some(PathBuf::from("/tmp/client.pem")),
            client_key_path: Some(PathBuf::from("/tmp/client.key")),
        };

        assert!(config.ca_cert_path.is_some());
        assert!(config.client_cert_path.is_some());
        assert!(config.client_key_path.is_some());
    }

    #[test]
    fn client_creation_with_invalid_ca_fails() {
        let config = AasClientConfig {
            base_url: "https://localhost:8443".to_string(),
            ca_cert_path: Some(PathBuf::from("/nonexistent/ca.pem")),
            ..Default::default()
        };

        let result = AasClient::new(config);
        assert!(result.is_err());
        // Verify it's an Init error by checking the error message
        let err_msg = format!("{}", result.err().unwrap());
        assert!(err_msg.contains("client init error"));
    }
}
