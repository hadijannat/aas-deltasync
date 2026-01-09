//! # AAS-ΔSync Protocol
//!
//! Wire protocol definitions and MQTT topic scheme for delta replication.
//!
//! ## Messages
//!
//! - `AgentHello`: Peer discovery and capability advertisement
//! - `DocDelta`: Compact delta for incremental replication
//! - `AntiEntropyRequest/Response`: State synchronization
//!
//! ## MQTT Topics
//!
//! Topic scheme: `aas-deltasync/v1/{tenant}/{doc_hash}/{message_type}`

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod messages;
pub mod topics;

pub use messages::{AgentHello, AntiEntropyRequest, AntiEntropyResponse, DocDelta};
pub use topics::TopicScheme;
