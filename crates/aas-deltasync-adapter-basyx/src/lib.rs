//! # `BaSyx` Adapter
//!
//! MQTT event ingestion from Eclipse `BaSyx` AAS and Submodel Repositories.
//!
//! ## `BaSyx` MQTT Topics
//!
//! `BaSyx` publishes events on topics:
//! - `sm-repository/{repoId}/submodels/{submodelIdBase64}/submodelElements/{idShortPath}/updated`
//! - `.../created`, `.../deleted`
//! - `.../submodelElements/patched`
//!
//! The adapter subscribes to these topics and converts events to CRDT deltas.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod events;
pub mod subscriber;

pub use events::{BasyxEvent, ElementEvent, EventType};
pub use subscriber::{BasyxSubscriber, BasyxSubscriberConfig};
