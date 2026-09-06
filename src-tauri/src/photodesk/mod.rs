//! The document → engine bridge, IO and export (§13).
//!
//! Today it is the document model and the sidecar that carries it. The engine arrives
//! with v0.1's render path; the boundary between them is already the one §13 draws.

pub mod document;
pub mod sidecar;
