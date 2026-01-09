//! # AAS Adapter
//!
//! AAS Part 2 encoding utilities and HTTP client for interacting with AAS servers.
//!
//! ## Encoding Rules (per AAS Part 2 HTTP/REST API)
//!
//! - **Identifiable IDs**: base64url-encoded WITHOUT padding
//! - **idShortPath**: URL-encoded (preserving `[]` for list indices)
//!
//! These rules are non-negotiable for interoperability with `BaSyx`, FA³ST, and other
//! AAS implementations.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod client;
pub mod encoding;

pub use client::{AasClient, AasClientConfig};
pub use encoding::{
    decode_id_base64url, decode_idshort_path, encode_id_base64url, encode_idshort_path,
};
