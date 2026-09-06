//! PhotoDesk's Rust core.
//!
//! **No application yet, and no `tauri` dependency** — see `Cargo.toml` for why. §14
//! gives v0.1 a document model, a graph compile, source-preservation and golden tests
//! before it gives it a window, and every one of those has to run headless.
//!
//! Layout follows §13: `photodesk/` is the document bridge, IO, cache and export;
//! `engine/` will be the render core; `ai/` the provider registry. The two that do not
//! exist yet do not exist yet.

pub mod photodesk;
