//! Per-element merge semantics for AAS types.
//!
//! Defines deterministic conflict resolution rules for each AAS element type.
//!
//! # Merge Rules
//!
//! | Element Type | Strategy |
//! |--------------|----------|
//! | Property | LWW register (HLC + actor tiebreaker) |
//! | Range | LWW per bound (min/max) |
//! | MultiLanguageProperty | LWW per language code |
//! | SubmodelElementCollection | OR-Map of children by idShort |
//! | SubmodelElementList | Stable element IDs (not list indices) |
//! | File/Blob | LWW pointer to content-addressed blob |

use serde::{Deserialize, Serialize};

/// The type of an AAS submodel element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ElementType {
    /// A single-valued property
    Property,
    /// A range with min/max values
    Range,
    /// A multi-language property
    MultiLanguageProperty,
    /// A reference element
    ReferenceElement,
    /// A blob (binary data)
    Blob,
    /// A file reference
    File,
    /// A submodel element collection
    SubmodelElementCollection,
    /// An ordered list of elements
    SubmodelElementList,
    /// An annotated relationship
    AnnotatedRelationshipElement,
    /// A basic event element
    BasicEventElement,
    /// An entity element
    Entity,
    /// An operation element
    Operation,
    /// A capability element
    Capability,
}

/// Merge strategy for a given element type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Last-Writer-Wins: higher timestamp wins entirely
    Lww,
    /// Per-field LWW: each field merged independently
    PerFieldLww,
    /// OR-Map: children merged as a set with add-wins
    OrMap,
    /// Content-addressed: pointer to immutable content
    ContentAddressed,
}

impl ElementType {
    /// Get the merge strategy for this element type.
    #[must_use]
    pub fn merge_strategy(&self) -> MergeStrategy {
        match self {
            // Simple value types use straightforward LWW
            ElementType::Property
            | ElementType::ReferenceElement
            | ElementType::BasicEventElement
            | ElementType::Capability => MergeStrategy::Lww,

            // Range and MultiLanguageProperty have compound values
            ElementType::Range | ElementType::MultiLanguageProperty => MergeStrategy::PerFieldLww,

            // Collections use OR-Map semantics
            ElementType::SubmodelElementCollection
            | ElementType::SubmodelElementList
            | ElementType::AnnotatedRelationshipElement
            | ElementType::Entity => MergeStrategy::OrMap,

            // Binary content uses content-addressing
            ElementType::Blob | ElementType::File => MergeStrategy::ContentAddressed,

            // Operations are structural definitions, use LWW
            ElementType::Operation => MergeStrategy::Lww,
        }
    }
}

/// Path segment for addressing within an AAS structure.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PathSegment {
    /// The idShort of the element
    pub id_short: String,
    /// Optional stable element ID for list elements
    pub element_id: Option<String>,
}

impl PathSegment {
    /// Create a new path segment.
    #[must_use]
    pub fn new(id_short: impl Into<String>) -> Self {
        Self {
            id_short: id_short.into(),
            element_id: None,
        }
    }

    /// Create a path segment for a list element.
    #[must_use]
    pub fn list_element(id_short: impl Into<String>, element_id: impl Into<String>) -> Self {
        Self {
            id_short: id_short.into(),
            element_id: Some(element_id.into()),
        }
    }
}

/// Canonical path to an element within a submodel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CanonicalPath {
    /// Path segments from submodel root
    pub segments: Vec<PathSegment>,
}

impl CanonicalPath {
    /// Create a new empty path (root).
    #[must_use]
    pub fn root() -> Self {
        Self { segments: vec![] }
    }

    /// Create a path from segments.
    #[must_use]
    pub fn from_segments(segments: Vec<PathSegment>) -> Self {
        Self { segments }
    }

    /// Append a segment to the path.
    #[must_use]
    pub fn child(&self, segment: PathSegment) -> Self {
        let mut segments = self.segments.clone();
        segments.push(segment);
        Self { segments }
    }

    /// Convert to idShortPath string format.
    #[must_use]
    pub fn to_id_short_path(&self) -> String {
        self.segments
            .iter()
            .map(|s| {
                if let Some(ref eid) = s.element_id {
                    format!("{}[{}]", s.id_short, eid)
                } else {
                    s.id_short.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Parse from idShortPath string format.
    #[must_use]
    pub fn from_id_short_path(path: &str) -> Self {
        if path.is_empty() {
            return Self::root();
        }

        let segments = path
            .split('.')
            .map(|part| {
                if let Some(bracket_start) = part.find('[') {
                    let id_short = part[..bracket_start].to_string();
                    let element_id = part[bracket_start + 1..part.len() - 1].to_string();
                    PathSegment {
                        id_short,
                        element_id: Some(element_id),
                    }
                } else {
                    PathSegment::new(part)
                }
            })
            .collect();

        Self { segments }
    }
}

impl std::fmt::Display for CanonicalPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_id_short_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_type_strategies() {
        assert_eq!(ElementType::Property.merge_strategy(), MergeStrategy::Lww);
        assert_eq!(
            ElementType::Range.merge_strategy(),
            MergeStrategy::PerFieldLww
        );
        assert_eq!(
            ElementType::SubmodelElementCollection.merge_strategy(),
            MergeStrategy::OrMap
        );
        assert_eq!(
            ElementType::Blob.merge_strategy(),
            MergeStrategy::ContentAddressed
        );
    }

    #[test]
    fn canonical_path_roundtrip() {
        let path = CanonicalPath::from_segments(vec![
            PathSegment::new("TechnicalData"),
            PathSegment::new("MaxTemperature"),
        ]);

        assert_eq!(path.to_id_short_path(), "TechnicalData.MaxTemperature");

        let parsed = CanonicalPath::from_id_short_path("TechnicalData.MaxTemperature");
        assert_eq!(path, parsed);
    }

    #[test]
    fn canonical_path_with_list_element() {
        let path = CanonicalPath::from_segments(vec![
            PathSegment::new("Components"),
            PathSegment::list_element("Items", "item-uuid-123"),
        ]);

        assert_eq!(path.to_id_short_path(), "Components.Items[item-uuid-123]");

        let parsed = CanonicalPath::from_id_short_path("Components.Items[item-uuid-123]");
        assert_eq!(path, parsed);
    }
}
