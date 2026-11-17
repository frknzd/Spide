//! Spide core: design model, operations, store, metrics, derivations and exporters.
#![recursion_limit = "512"]

pub mod adr;
pub mod archunit;
pub mod course;
pub mod assess;
pub mod derive;
pub mod decisions;
pub mod detail;
pub mod diff;
pub mod evidence;
pub mod health;
pub mod home;
pub mod impact;
pub mod lucid;
pub mod metrics;
pub mod model;
pub mod ops;
pub mod overlap;
pub mod plantuml;
pub mod project;
pub mod responsibility;
pub mod pdf;
pub mod png;
pub mod rules;
pub mod scenarios;
pub mod sequence;
pub mod store;
pub mod symbols;
pub mod sync;
pub mod proposal_review;
pub mod tools;

pub use model::*;
pub use ops::{Op, OpError};
pub use store::{Store, Target};
