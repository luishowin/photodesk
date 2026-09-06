//! Spike B — colour validation harness (`ARCHITECTURE.md` §2.2).
//!
//! Built to answer one question with evidence: is linear Display P3 at f16 a working
//! space this project can freeze? Kept afterwards as a permanent suite (§13), because
//! the transforms it checks are the ones every later golden-image test sits on top of.
//!
//! Spike A established there is no incumbent colour architecture to adapt — RapidRAW
//! has none at all — so §4 is built from nothing and this harness is what decides
//! whether it is built on f16.
//!
//! **It now tests the product rather than a copy of it.** The spaces, transfer curves,
//! matrices and gamut policy moved into `photodesk::engine` when the decode path
//! needed them; what stays here is measurement — ΔE2000, Lab, the corpus, and a model
//! of the pipeline whose precision is a parameter. Before the move this crate held its
//! own transforms and validated *those* against lcms2, which is a weaker claim than it
//! looks: it could have been green while the shipped transforms were wrong, there
//! being none. The cross-validations now point at the code that runs.

pub mod corpus;
pub mod delta_e;
pub mod lab;
pub mod working;
pub mod workload;
