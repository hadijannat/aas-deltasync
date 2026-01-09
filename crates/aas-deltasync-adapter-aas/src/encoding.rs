//! AAS Part 2 encoding utilities.
//!
//! Implements the mandatory encoding rules from AAS Part 2 HTTP/REST API:
//!
//! - Identifiers of Identifiables are base64url-encoded (no padding)
//! - idShortPath is URL-encoded (not base64url)
//!
//! # References
//!
//! - IDTA 01002-3-1: Specification of the Asset Administration Shell Part 2
//! - FAQ: https://github.com/admin-shell-io/questions-and-answers

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

/// Characters that must be percent-encoded in idShortPath.
/// Note: Square brackets `[]` are preserved for list index notation.
const IDSHORT_PATH_ESCAPE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'?')
    .add(b'{')
    .add(b'}')
    .add(b'/')
    .add(b'\\');

/// Encode an AAS identifier using base64url without padding.
///
/// Per AAS Part 2, identifiers of Identifiables must be base64url-encoded
/// when passed in API paths. The padding character `=` must NOT be used.
///
/// # Examples
///
/// ```
/// use aas_deltasync_adapter_aas::encode_id_base64url;
///
/// let encoded = encode_id_base64url("urn:example:aas:asset1");
/// assert!(!encoded.contains('='));  // No padding
/// assert!(!encoded.contains('+'));  // No standard base64 chars
/// assert!(!encoded.contains('/'));
/// ```
#[must_use]
pub fn encode_id_base64url(id: &str) -> String {
    URL_SAFE_NO_PAD.encode(id.as_bytes())
}

/// Decode a base64url-encoded AAS identifier.
///
/// # Errors
///
/// Returns error if the input is not valid base64url.
///
/// # Examples
///
/// ```
/// use aas_deltasync_adapter_aas::{encode_id_base64url, decode_id_base64url};
///
/// let original = "urn:example:aas:asset1";
/// let encoded = encode_id_base64url(original);
/// let decoded = decode_id_base64url(&encoded).unwrap();
/// assert_eq!(decoded, original);
/// ```
pub fn decode_id_base64url(encoded: &str) -> Result<String, EncodingError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|e| EncodingError::Base64Decode(e.to_string()))?;

    String::from_utf8(bytes).map_err(|e| EncodingError::Utf8Decode(e.to_string()))
}

/// URL-encode an idShortPath segment.
///
/// Per AAS Part 2, idShortPath must be URL-encoded (not base64url).
/// Square brackets `[]` are preserved for list element addressing.
///
/// # Examples
///
/// ```
/// use aas_deltasync_adapter_aas::encode_idshort_path;
///
/// // Simple path
/// let encoded = encode_idshort_path("TechnicalData.MaxTemperature");
/// assert_eq!(encoded, "TechnicalData.MaxTemperature");
///
/// // Path with spaces
/// let encoded = encode_idshort_path("My Property");
/// assert_eq!(encoded, "My%20Property");
///
/// // Path with list index (brackets preserved)
/// let encoded = encode_idshort_path("Components[0]");
/// assert_eq!(encoded, "Components[0]");
/// ```
#[must_use]
pub fn encode_idshort_path(path: &str) -> String {
    utf8_percent_encode(path, IDSHORT_PATH_ESCAPE).to_string()
}

/// Decode a URL-encoded idShortPath.
///
/// # Errors
///
/// Returns error if the input contains invalid UTF-8 sequences.
///
/// # Examples
///
/// ```
/// use aas_deltasync_adapter_aas::{encode_idshort_path, decode_idshort_path};
///
/// let original = "My Property";
/// let encoded = encode_idshort_path(original);
/// let decoded = decode_idshort_path(&encoded).unwrap();
/// assert_eq!(decoded, original);
/// ```
pub fn decode_idshort_path(encoded: &str) -> Result<String, EncodingError> {
    percent_decode_str(encoded)
        .decode_utf8()
        .map(|s| s.into_owned())
        .map_err(|e| EncodingError::Utf8Decode(e.to_string()))
}

/// Errors that can occur during encoding/decoding.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EncodingError {
    /// Base64 decoding failed
    #[error("base64 decode error: {0}")]
    Base64Decode(String),
    /// UTF-8 decoding failed
    #[error("UTF-8 decode error: {0}")]
    Utf8Decode(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden test vectors from AAS FAQ and real-world identifiers

    #[test]
    fn base64url_basic() {
        let id = "urn:example:aas:asset1";
        let encoded = encode_id_base64url(id);
        let decoded = decode_id_base64url(&encoded).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn base64url_no_padding() {
        // Identifiers of various lengths should never have padding
        for id in [
            "a",
            "ab",
            "abc",
            "abcd",
            "https://example.org/aas/12345",
            "urn:zvei:de:ZVEI:IDTA:SubmodelTemplate:DigitalNameplate:1.0",
        ] {
            let encoded = encode_id_base64url(id);
            assert!(
                !encoded.contains('='),
                "Encoded '{}' should not contain padding: {}",
                id,
                encoded
            );
        }
    }

    #[test]
    fn base64url_no_standard_chars() {
        // base64url must not contain + or /
        let id = "urn:example:with+plus/and/slashes";
        let encoded = encode_id_base64url(id);
        assert!(!encoded.contains('+'), "Should not contain +");
        assert!(!encoded.contains('/'), "Should not contain /");

        let decoded = decode_id_base64url(&encoded).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn base64url_unicode() {
        let id = "urn:example:aas:资产1";
        let encoded = encode_id_base64url(id);
        let decoded = decode_id_base64url(&encoded).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn idshort_path_simple() {
        let path = "TechnicalData.MaxTemperature";
        let encoded = encode_idshort_path(path);
        // Simple alphanumeric + dots should pass through unchanged
        assert_eq!(encoded, path);
    }

    #[test]
    fn idshort_path_with_spaces() {
        let path = "Technical Data.Max Temperature";
        let encoded = encode_idshort_path(path);
        assert!(encoded.contains("%20"), "Spaces should be encoded");
        let decoded = decode_idshort_path(&encoded).unwrap();
        assert_eq!(decoded, path);
    }

    #[test]
    fn idshort_path_with_brackets() {
        // List indices use square brackets, which should be preserved
        let path = "Components[0].SubComponents[1]";
        let encoded = encode_idshort_path(path);
        // Brackets should be preserved for list notation
        assert!(
            encoded.contains('[') && encoded.contains(']'),
            "Brackets should be preserved: {}",
            encoded
        );
    }

    #[test]
    fn idshort_path_special_chars() {
        let path = "Path/With<Special>Chars";
        let encoded = encode_idshort_path(path);
        assert!(!encoded.contains('/'), "/ should be encoded");
        assert!(!encoded.contains('<'), "< should be encoded");
        assert!(!encoded.contains('>'), "> should be encoded");

        let decoded = decode_idshort_path(&encoded).unwrap();
        assert_eq!(decoded, path);
    }

    #[test]
    fn golden_test_submodel_id() {
        // Example from AAS FAQ: encoding a typical submodel identifier
        let submodel_id = "https://admin-shell.io/zvei/nameplate/2/0/Nameplate";
        let encoded = encode_id_base64url(submodel_id);

        // Verify roundtrip
        let decoded = decode_id_base64url(&encoded).unwrap();
        assert_eq!(decoded, submodel_id);

        // Verify no forbidden characters
        assert!(!encoded.contains('='));
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn golden_test_idshort_path_nested() {
        // Nested path with various segment types
        let path = "ContactInformation.Phone[Business].AreaCode";
        let encoded = encode_idshort_path(path);
        let decoded = decode_idshort_path(&encoded).unwrap();
        assert_eq!(decoded, path);
    }
}
