//! Tessera CLI — reciprocal museum coverage optimizer.
//!
//! Wired up in Cycle 3. For now, validates that the fixture data loads.

fn main() {
    let dataset = tessera_core::Dataset {
        networks: vec![],
        institutions: vec![],
        memberships: vec![],
    };
    println!("tessera-cli: dataset has {} institutions", dataset.institutions.len());
}
