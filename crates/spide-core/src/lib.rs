//! Spide core: design model, operations, store, metrics, derivations and exporters.

pub mod derive;
pub mod metrics;
pub mod model;
pub mod ops;
pub mod plantuml;
pub mod png;
pub mod store;
pub mod tools;

pub use model::*;
pub use ops::{Op, OpError};
pub use store::{Store, Target};
