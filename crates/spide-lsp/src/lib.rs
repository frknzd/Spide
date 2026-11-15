//! Spide's language-server and linter integration: a managed stdio LSP client with position
//! encoding negotiation, default tooling per language, `doctor`, and normalisation of LSP
//! diagnostics and linter output into language-neutral evidence.

pub mod client;
pub mod defaults;
pub mod diagnostics;
pub mod position;
pub mod providers;
pub mod transport;

pub use client::{LspClient, ServerConfig};
