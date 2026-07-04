//! Tessera CLI — reciprocal museum coverage optimizer.
//!
//! Cycle 1: prints eligibility set for a hardcoded Aloha/OMSI scenario.
//! Full CLI wiring in Cycle 3.

use tessera_core::*;
use tessera_core::rules::{compute_eligibility, networks_from_memberships};

fn main() {
    // Load fixture data
    let networks: Vec<NetworkSpec> =
        serde_json::from_str(include_str!("../../../data/networks.json")).unwrap();
    let institutions: Vec<Institution> =
        serde_json::from_str(include_str!("../../../data/institutions.json")).unwrap();
    let memberships: Vec<Membership> =
        serde_json::from_str(include_str!("../../../data/memberships.json")).unwrap();
    let dataset = Dataset {
        networks,
        institutions,
        memberships,
    };

    // Aloha, OR resident with OMSI Family Plus membership
    let user = User {
        residence: LatLon::new(45.4912, -122.8720),
        held: vec![MembershipRef {
            institution_id: "omsi".into(),
            tier: "Family Plus".into(),
        }],
    };

    let nets = networks_from_memberships(&user, &dataset);
    println!("User holds networks: {}", nets.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", "));
    println!();

    let results = compute_eligibility(&user, &dataset, &nets, Some("omsi"));

    println!("Eligible institutions ({} total):", results.len());
    println!("{:<35} {:<8} {:<12} {}", "Institution", "Network", "Admission", "Notes");
    println!("{}", "-".repeat(75));
    for r in &results {
        let adm = match r.admission {
            Admission::Free => "free".to_string(),
            Admission::Discount { fraction } => format!("{}% off", (fraction * 100.0) as u32),
        };
        let notes = if r.special_exhibit_restricted {
            "special exhibits restricted"
        } else {
            ""
        };
        let name = dataset
            .institution(&r.institution_id)
            .map(|i| i.name.as_str())
            .unwrap_or(&r.institution_id);
        println!("{:<35} {:<8} {:<12} {}", name, r.network.to_string(), adm, notes);
    }
}
