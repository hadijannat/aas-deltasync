//! `BaSyx` MQTT event types.

use aas_deltasync_adapter_aas::decode_id_base64url;
use serde::{Deserialize, Serialize};

/// Type of `BaSyx` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    /// Element was created
    Created,
    /// Element was updated
    Updated,
    /// Element was deleted
    Deleted,
    /// Multiple elements were patched
    Patched,
}

impl EventType {
    /// Parse event type from topic suffix.
    #[must_use]
    pub fn from_topic_suffix(suffix: &str) -> Option<Self> {
        match suffix {
            "created" => Some(Self::Created),
            "updated" => Some(Self::Updated),
            "deleted" => Some(Self::Deleted),
            "patched" => Some(Self::Patched),
            _ => None,
        }
    }
}

/// A parsed `BaSyx` MQTT event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasyxEvent {
    /// Repository ID
    pub repo_id: String,
    /// Submodel ID (decoded from base64url)
    pub submodel_id: String,
    /// Type of event
    pub event_type: EventType,
    /// Element-level details (if applicable)
    pub element: Option<ElementEvent>,
    /// Raw event payload
    pub payload: serde_json::Value,
}

/// Element-specific event details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementEvent {
    /// idShortPath of the affected element
    pub id_short_path: String,
    /// New value (if present in event)
    pub value: Option<serde_json::Value>,
}

impl BasyxEvent {
    /// Parse a `BaSyx` event from an MQTT topic and payload.
    ///
    /// # Topic Format
    ///
    /// `sm-repository/{repoId}/submodels/{submodelIdBase64}/submodelElements/{idShortPath}/{eventType}`
    ///
    /// # Errors
    ///
    /// Returns error if the topic format is invalid.
    pub fn parse(topic: &str, payload: &[u8]) -> Result<Self, EventParseError> {
        let parts: Vec<&str> = topic.split('/').collect();

        // Minimum: sm-repository/repo/submodels/smId/...
        if parts.len() < 4 || parts[0] != "sm-repository" {
            return Err(EventParseError::InvalidTopic(topic.to_string()));
        }

        let repo_id = parts[1].to_string();

        // Find submodels segment
        let submodels_idx = parts
            .iter()
            .position(|&p| p == "submodels")
            .ok_or_else(|| EventParseError::InvalidTopic(topic.to_string()))?;

        if submodels_idx + 1 >= parts.len() {
            return Err(EventParseError::InvalidTopic(topic.to_string()));
        }

        let submodel_id_encoded = parts[submodels_idx + 1];
        let submodel_id = decode_id_base64url(submodel_id_encoded)
            .map_err(|e| EventParseError::DecodeError(e.to_string()))?;

        // Parse payload
        let payload: serde_json::Value = if payload.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(payload)
                .map_err(|e| EventParseError::PayloadParse(e.to_string()))?
        };

        // Determine event type and element path
        let event_type_str = parts.last().unwrap_or(&"");
        let event_type = EventType::from_topic_suffix(event_type_str)
            .ok_or_else(|| EventParseError::UnknownEventType((*event_type_str).to_string()))?;

        // Check if this is an element-level event
        let element =
            if let Some(elements_idx) = parts.iter().position(|&p| p == "submodelElements") {
                if elements_idx + 1 < parts.len() - 1 {
                    // There's a path between submodelElements and event type
                    let path_parts = &parts[elements_idx + 1..parts.len() - 1];
                    let id_short_path = path_parts.join("/");

                    // Extract value from payload if present
                    let value = payload.get("value").cloned().or_else(|| {
                        // Some events have the value directly
                        if payload.is_object() && payload.get("modelType").is_some() {
                            payload.get("value").cloned()
                        } else if !payload.is_null() && !payload.is_object() {
                            Some(payload.clone())
                        } else {
                            None
                        }
                    });

                    Some(ElementEvent {
                        id_short_path,
                        value,
                    })
                } else {
                    None
                }
            } else {
                None
            };

        Ok(Self {
            repo_id,
            submodel_id,
            event_type,
            element,
            payload,
        })
    }
}

/// Errors that can occur parsing `BaSyx` events.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EventParseError {
    /// Topic format is invalid
    #[error("invalid topic format: {0}")]
    InvalidTopic(String),
    /// Failed to decode base64url identifier
    #[error("decode error: {0}")]
    DecodeError(String),
    /// Failed to parse payload JSON
    #[error("payload parse error: {0}")]
    PayloadParse(String),
    /// Unknown event type
    #[error("unknown event type: {0}")]
    UnknownEventType(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aas_deltasync_adapter_aas::encode_id_base64url;

    #[test]
    fn parse_element_updated_event() {
        let submodel_id = "urn:example:sm:data";
        let encoded_sm_id = encode_id_base64url(submodel_id);

        let topic = format!(
            "sm-repository/repo1/submodels/{encoded_sm_id}/submodelElements/Temperature/updated"
        );

        let payload = br#"{"value": 25.5}"#;

        let event = BasyxEvent::parse(&topic, payload).unwrap();

        assert_eq!(event.repo_id, "repo1");
        assert_eq!(event.submodel_id, submodel_id);
        assert_eq!(event.event_type, EventType::Updated);

        let element = event.element.unwrap();
        assert_eq!(element.id_short_path, "Temperature");
        assert_eq!(element.value, Some(serde_json::json!(25.5)));
    }

    #[test]
    fn parse_element_deleted_event() {
        let submodel_id = "urn:example:sm:data";
        let encoded_sm_id = encode_id_base64url(submodel_id);

        let topic = format!(
            "sm-repository/repo1/submodels/{encoded_sm_id}/submodelElements/OldProperty/deleted"
        );

        let event = BasyxEvent::parse(&topic, b"").unwrap();

        assert_eq!(event.event_type, EventType::Deleted);
        let element = event.element.unwrap();
        assert_eq!(element.id_short_path, "OldProperty");
        assert!(element.value.is_none());
    }

    #[test]
    fn parse_nested_path() {
        let submodel_id = "urn:example:sm:nested";
        let encoded_sm_id = encode_id_base64url(submodel_id);

        let topic = format!(
            "sm-repository/repo2/submodels/{encoded_sm_id}/submodelElements/Collection/SubProperty/updated"
        );

        let event = BasyxEvent::parse(&topic, b"{}").unwrap();

        let element = event.element.unwrap();
        assert_eq!(element.id_short_path, "Collection/SubProperty");
    }
}
