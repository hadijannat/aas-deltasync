//! # FA³ST Adapter
//!
//! Polling-based change detection for FA³ST AAS servers.
//!
//! ## HTTPS Requirement
//!
//! Per FA³ST documentation: "In accordance to the specification, only HTTPS
//! is supported since AAS v3.0."
//!
//! This adapter requires TLS and supports:
//! - Custom CA bundles for self-signed certificates
//! - Client certificates (mTLS) for mutual authentication
//!
//! ## Change Detection
//!
//! Since FA³ST doesn't provide real-time events, the adapter polls
//! submodel values and computes diffs to generate CRDT deltas.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod poller;

pub use poller::{FaaastPoller, FaaastPollerConfig};
