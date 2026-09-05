//! Spike B — colour validation harness (`ARCHITECTURE.md` §2.2).
//!
//! Built to answer one question with evidence: is linear Display P3 at f16 a working
//! space this project can freeze? Kept afterwards as a permanent suite (§13), because
//! the transforms it checks are the ones every later golden-image test sits on top of.
//!
//! Spike A established there is no incumbent colour architecture to adapt — RapidRAW
//! has none at all — so §4 is built from nothing and this harness is what decides
//! whether it is built on f16.

pub mod colour;
pub mod corpus;
pub mod delta_e;
pub mod gamut;
pub mod working;
pub mod workload;
