//! PhotoDesk's Rust core.
//!
//! **No application yet, and no `tauri` dependency** — see `Cargo.toml` for why. §14
//! gives v0.1 a document model, a graph compile, source-preservation and golden tests
//! before it gives it a window, and every one of those has to run headless.
//!
//! Layout follows §13: `photodesk/` is the document bridge, IO, cache and export;
//! `engine/` is the render core; `ai/` the provider registry, which does not exist yet.

pub mod engine;
pub mod photodesk;
