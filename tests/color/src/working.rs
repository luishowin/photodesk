//! The working space under test, and the chain that carries a pixel through it.
//!
//! §4's candidate is linear Display P3 at f16. The point of this module is that the
//! buffer precision is a *parameter*, so a failure can be attributed to f16 storage
//! rather than to the transforms around it — and so §2.2's stated fallback
//! ("f32 working buffers at proxy resolution") can be evaluated against evidence
//! instead of adopted on a hunch.

use photodesk::engine::colour::{LINEAR_P3, Mat3, Space};
use photodesk::engine::gamut::luma_weights;
use crate::workload::Workload;
use half::f16;

pub use photodesk::engine::gamut::{EXPORT_GAMUT_POLICY, GamutPolicy};

/// Precision of the working buffer between render passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    /// §4's candidate.
    F16,
    /// §2.2's stated fallback.
    F32,
}

impl Precision {
    /// Quantise as a store to a texture of this format followed by a load.
    fn store(self, v: [f32; 3]) -> [f32; 3] {
        match self {
            Precision::F32 => v,
            Precision::F16 => [
                f16::from_f32(v[0]).to_f32(),
                f16::from_f32(v[1]).to_f32(),
                f16::from_f32(v[2]).to_f32(),
            ],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Precision::F16 => "f16",
            Precision::F32 => "f32",
        }
    }
}

/// The chain a pixel takes: source encoding -> working space -> output encoding.
#[derive(Clone, Copy, Debug)]
pub struct Pipeline {
    pub precision: Precision,
    pub gamut: GamutPolicy,
    /// How many times the working buffer is stored and re-read before output.
    ///
    /// Not cosmetic. §7.3 fuses stages 2–9 into one pass and gives the spatial
    /// stages their own, so a single layer is four or five stores, and §7.3's
    /// six-layer bound makes the worst case around thirty.
    pub passes: u32,
    /// What each pass does. With [`Workload::Identity`] the pass count is inert —
    /// `f16 -> f32 -> f16` is idempotent — so any statement about chain depth has to
    /// be made with [`Workload::Representative`] or it is not about anything.
    pub workload: Workload,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self {
            precision: Precision::F16,
            gamut: EXPORT_GAMUT_POLICY,
            passes: 1,
            workload: Workload::Identity,
        }
    }
}

impl Pipeline {
    /// An identity chain: the right instrument for §2.2's round-trip tests, which ask
    /// whether the transforms are correct rather than what a long edit costs.
    pub fn new(precision: Precision, passes: u32) -> Self {
        Self {
            precision,
            passes,
            ..Default::default()
        }
    }

    /// A chain that actually does per-pixel work between stores.
    pub fn working_chain(precision: Precision, passes: u32) -> Self {
        Self {
            precision,
            passes,
            workload: Workload::Representative,
            ..Default::default()
        }
    }

    fn working(&self) -> &'static Space {
        &LINEAR_P3
    }

    /// Encoded source values -> linear working space, as stored.
    pub fn ingest(&self, encoded: [f32; 3], src: &Space) -> [f32; 3] {
        let lin = [
            src.transfer.to_linear(encoded[0]),
            src.transfer.to_linear(encoded[1]),
            src.transfer.to_linear(encoded[2]),
        ];
        let m: Mat3 = src.linear_to(self.working());
        self.precision.store(m.apply(lin))
    }

    /// One render pass: read the buffer, do the pass's work in f32, write it back
    /// at the buffer's precision.
    pub fn pass(&self, working: [f32; 3], i: u32) -> [f32; 3] {
        self.precision.store(self.workload.apply(working, i))
    }

    /// Working space -> encoded output values.
    ///
    /// The gamut map runs in linear destination RGB, between the matrix and the
    /// transfer curve. That position is not incidental: the destination gamut is
    /// exactly the unit cube there, and doing it after the encode would fold the
    /// curve's own shape into the geometry (§16 #11, `gamut.rs`).
    pub fn emit(&self, working: [f32; 3], dst: &Space) -> [f32; 3] {
        let m: Mat3 = self.working().linear_to(dst);
        let lin = self.gamut.map(m.apply(working), dst);
        [
            dst.transfer.from_linear(lin[0]),
            dst.transfer.from_linear(lin[1]),
            dst.transfer.from_linear(lin[2]),
        ]
    }

    /// The full chain at 8-bit in and 8-bit out, with `passes` identity stages
    /// in the middle. This is the thing §2.2's tests actually measure.
    pub fn round_trip_u8(&self, code: [u8; 3], src: &Space, dst: &Space) -> [u8; 3] {
        let encoded = [
            code[0] as f32 / 255.0,
            code[1] as f32 / 255.0,
            code[2] as f32 / 255.0,
        ];
        let mut w = self.ingest(encoded, src);
        for i in 0..self.passes {
            w = self.pass(w, i);
        }
        let out = self.emit(w, dst);
        [quantise_u8(out[0]), quantise_u8(out[1]), quantise_u8(out[2])]
    }

    /// As above but returning the encoded float, for measurements that should not
    /// be masked by 8-bit output quantisation.
    pub fn round_trip_encoded(&self, encoded: [f32; 3], src: &Space, dst: &Space) -> [f32; 3] {
        let mut w = self.ingest(encoded, src);
        for i in 0..self.passes {
            w = self.pass(w, i);
        }
        self.emit(w, dst)
    }

    /// The same chain carried out entirely in f64, with no working-buffer
    /// quantisation. This is the answer the pipeline is *trying* to compute, so
    /// the distance between the two is exactly the cost of the buffer format — not
    /// contaminated by the transforms, which both paths share.
    pub fn reference_run(&self, encoded: [f32; 3], src: &Space, dst: &Space) -> [f32; 3] {
        let lin = [
            src.transfer.to_linear(encoded[0]) as f64,
            src.transfer.to_linear(encoded[1]) as f64,
            src.transfer.to_linear(encoded[2]) as f64,
        ];
        let to_w = src.linear_to(self.working()).0;
        let mut w = [
            to_w[0][0] * lin[0] + to_w[0][1] * lin[1] + to_w[0][2] * lin[2],
            to_w[1][0] * lin[0] + to_w[1][1] * lin[1] + to_w[1][2] * lin[2],
            to_w[2][0] * lin[0] + to_w[2][1] * lin[1] + to_w[2][2] * lin[2],
        ];
        for i in 0..self.passes {
            w = self.workload.apply(w, i);
        }
        let from_w = self.working().linear_to(dst).0;
        let out = [
            from_w[0][0] * w[0] + from_w[0][1] * w[1] + from_w[0][2] * w[2],
            from_w[1][0] * w[0] + from_w[1][1] * w[1] + from_w[1][2] * w[2],
            from_w[2][0] * w[0] + from_w[2][1] * w[1] + from_w[2][2] * w[2],
        ];
        // The *same* policy, instantiated at f64. Not a second implementation of it:
        // divergence between these two paths is meant to be the cost of the buffer
        // format, and a transcribed gamut map would quietly add itself to that number.
        let out = self.gamut.map_with(out, luma_weights(dst));
        [
            dst.transfer.from_linear(out[0] as f32),
            dst.transfer.from_linear(out[1] as f32),
            dst.transfer.from_linear(out[2] as f32),
        ]
    }
}

/// Round-to-nearest, clamped. The dither in a real encoder is a separate concern
/// and would only help, so leaving it out keeps the measurement pessimistic.
pub fn quantise_u8(v: f32) -> u8 {
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}
