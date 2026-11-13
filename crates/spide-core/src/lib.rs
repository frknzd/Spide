//! Spide core: design model, operations, store, metrics, derivations and exporters.

pub mod adr;
pub mod archunit;
pub mod assess;
pub mod derive;
pub mod detail;
pub mod diff;
pub mod drift;
pub mod home;
pub mod impact;
pub mod metrics;
pub mod model;
pub mod ops;
pub mod overlap;
pub mod plantuml;
pub mod responsibility;
pub mod png;
pub mod rules;
pub mod scenarios;
pub mod store;
pub mod tools;

pub use model::*;
pub use ops::{Op, OpError};
pub use store::{Store, Target};
