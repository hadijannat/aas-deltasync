//! # AAS-ΔSync Core
//!
//! Core CRDT model, HLC timestamps, and merge semantics for AAS-ΔSync.
//!
//! This crate provides:
//! - Hybrid Logical Clock (HLC) for globally ordered timestamps
//! - CRDT primitives (LWW registers, OR-Map) adapted for AAS semantics
//! - Document model mapping AAS Submodels to CRDT structures
//! - Merge algorithms with deterministic conflict resolution

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod crdt;
pub mod document;
pub mod hlc;
pub mod merge;

pub use crdt::{Delta, LwwRegister, OrMap};
pub use document::{CrdtDocument, DocId, View};
pub use hlc::{Hlc, Timestamp};
