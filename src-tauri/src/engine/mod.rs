//! The render core (§13): decode, colour, GPU dispatch, shaders.
//!
//! **The colour primitives live here, not in `tests/color/`, and the move mattered.**
//! Spike B's harness was written before there was any product, so it validated its own
//! copy of the transforms against lcms2 — which proved the *harness* was right and
//! could have stayed green while the shipped transforms were wrong, for the simple
//! reason that there were none. Now that there are, the harness points at them: it
//! imports `engine::colour` and `engine::gamut` and cross-validates those. §13 always
//! said `engine/` was "decode, colour, GPU dispatch, shaders"; this is the colour.

pub mod colour;
pub mod decode;
pub mod exif;
pub mod export;
pub mod gamut;
pub mod glsl;
pub mod icc;
pub mod image;
pub mod render;
