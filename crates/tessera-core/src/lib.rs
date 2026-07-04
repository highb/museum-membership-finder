//! # tessera-core
//!
//! Pure, no-I/O, WASM-safe domain model for the Tessera reciprocal museum
//! coverage optimizer.
//!
//! Models the U.S. reciprocal-network landscape (NARM, ASTC, AHS, ROAM, MARP,
//! ACM, AZA, Time Travelers), encodes per-network/per-institution exclusion
//! rules, and solves the membership-selection problem as weighted set-cover.

pub mod model;
pub mod geo;
pub mod rules;
pub mod solve;
pub mod zip;

pub use model::*;
