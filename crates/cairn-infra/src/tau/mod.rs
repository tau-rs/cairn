//! Adapter for the tau agent runtime over its serve-mode protocol
//! (JSON-RPC 2.0 over NDJSON on stdio). See
//! `docs/superpowers/specs/2026-06-14-tau-augmented-answer-design.md`.

pub mod client;
pub mod config;
pub mod process;
pub mod runtime;
pub mod supervisor;
pub mod wire;

pub use config::TauConfig;
pub use runtime::TauServeRuntime;
pub use supervisor::TauSidecar;
