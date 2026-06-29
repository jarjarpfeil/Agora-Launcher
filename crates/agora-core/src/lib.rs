//! Agora launcher core — shared business logic consumed by the Tauri GUI,
//! the standalone `agora` CLI, and the in-process MCP listener.
//!
//! Constraint (plan C2/C3): this crate MUST NOT depend on `tauri`, `clap`,
//! or any MCP-protocol crate. Every operation takes a `&Ctx` (introduced
//! later). For now this crate only hosts the pure data/error modules moved
//! out of the desktop crate in Phase 1A.

pub mod ctx;
pub mod error;
pub mod loader_manifests;
pub mod models;
pub mod download;
pub mod override_sanitizer;
pub mod paths;
pub mod db;
pub mod governance;
pub mod crash_diagnostics;
pub mod registry_sync;
pub mod registry;
pub mod modrinth;
pub mod catalog;
pub mod dependency_ops;
