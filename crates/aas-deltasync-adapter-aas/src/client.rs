//! HTTP client for AAS Part 2 API.
//!
//! Provides a minimal interface for interacting with AAS servers,
//! using the correct encoding rules for identifiers and paths.

use super::encoding::{encode_id_base64url, encode_idshort_path};
use reqwest::Client;
use serde_json::Value;
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
}

impl Default for AasClientConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8081".to_string(),
            timeout: Duration::from_secs(30),
            bearer_token: None,
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
    /// Returns error if the HTTP client cannot be created.
    pub fn new(config: AasClientConfig) -> Result<Self, ClientError> {
        let mut builder = Client::builder().timeout(config.timeout);

        if config.base_url.starts_with("https://") {
            // Enable rustls for HTTPS (required for FA³ST)
            builder = builder.use_rustls_tls();
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
    }

    #[test]
    fn client_creation() {
        let config = AasClientConfig::default();
        let client = AasClient::new(config);
        assert!(client.is_ok());
    }
}
