//! Acceptance test: the "Aloha 97007" scenario.
//!
//! This is the hand-derived acceptance oracle from the project brief.
//! An Aloha, OR (97007) resident wants access to 8 PNW institutions.
//! The optimizer must reproduce the hand-derived optimal answer.
//!
//! Key rules exercised:
//! - ASTC 90-mi both-must-clear: Oregon Coast Aquarium (~83 mi from Aloha)
//!   is EXCLUDED even though OMSI (~91 mi from Newport) would clear the
//!   home-institution radius.
//! - NARM 15-mi flag: Lan Su Chinese Garden is EXCLUDED for an OMSI member
//!   (~1.5 mi away).
//! - ACM discount: Children's museums get 50% off, not free.
//! - AZA discount: Zoos get ~50% off, not free.
//! - NARM no-exclusion: Portland Art Museum, Seattle Art Museum, Tacoma Art
//!   Museum, Maryhill Museum are all NARM-eligible with no exclusion.

use std::collections::BTreeSet;

use tessera_core::model::*;
use tessera_core::rules;
use tessera_core::solve;
use tessera_core::zip::ZipCentroids;

const NETWORKS_JSON: &str = include_str!("../../../data/networks.json");
const INSTITUTIONS_JSON: &str = include_str!("../../../data/institutions.json");
const MEMBERSHIPS_JSON: &str = include_str!("../../../data/memberships.json");
const ZIP_CENTROIDS_JSON: &str = include_str!("../../../data/zip-centroids-pnw.json");

fn load() -> (Dataset, ZipCentroids) {
    let dataset = Dataset {
        networks: serde_json::from_str(NETWORKS_JSON).unwrap(),
        institutions: serde_json::from_str(INSTITUTIONS_JSON).unwrap(),
        memberships: serde_json::from_str(MEMBERSHIPS_JSON).unwrap(),
    };
    let zips = ZipCentroids::from_json(ZIP_CENTROIDS_JSON).unwrap();
    (dataset, zips)
}

/// The 8 target institutions for the acceptance test.
const TARGET_IDS: &[&str] = &[
    "pacific-science-center",     // ASTC — eligible (>90 mi from Aloha + OMSI)
    "seattle-art-museum",         // NARM — eligible
    "tacoma-art-museum",          // NARM — eligible
    "maryhill-museum",            // NARM — eligible
    "oregon-coast-aquarium",      // ASTC — EXCLUDED (within 90 mi of Aloha)
    "lan-su-chinese-garden",      // NARM — EXCLUDED (15-mi flag, OMSI nearby)
    "portland-art-museum",        // NARM — eligible
    "woodland-park-zoo",          // AZA — discount only
];

// -----------------------------------------------------------------------
// Eligibility oracle
// -----------------------------------------------------------------------

/// Hand-derived eligibility for Aloha/OMSI Family Plus.
/// OMSI Family Plus unlocks ASTC + NARM.
#[test]
fn acceptance_eligibility_omsi_family_plus() {
    let (ds, zips) = load();
    let aloha = zips.lookup("97007").unwrap();

    let user = User {
        residence: aloha,
        held: vec![MembershipRef {
            institution_id: "omsi".into(),
            tier: "Family Plus".into(),
        }],
    };

    let results = rules::compute_eligibility(
        &user,
        &ds,
        &[Network::Astc, Network::Narm],
        Some("omsi"),
    );

    let eligible_ids: BTreeSet<&str> = results
        .iter()
        .map(|r| r.institution_id.as_str())
        .collect();

    // ASTC eligible: only Pacific Science Center (Seattle, ~145 mi)
    assert!(
        eligible_ids.contains("pacific-science-center"),
        "Pacific Science Center should be ASTC-eligible"
    );
    // ASTC excluded: Oregon Coast Aquarium (~83 mi from Aloha)
    let oca_astc = results.iter().find(|r| {
        r.institution_id == "oregon-coast-aquarium" && r.network == Network::Astc
    });
    assert!(oca_astc.is_none(), "Oregon Coast Aquarium should be ASTC-excluded");

    // ASTC excluded: OMSI itself (within 90 mi of residence)
    let omsi_astc = results.iter().find(|r| {
        r.institution_id == "omsi" && r.network == Network::Astc
    });
    assert!(omsi_astc.is_none(), "OMSI should be ASTC-excluded for Aloha resident");

    // NARM eligible: Portland Art Museum, Seattle Art Museum, Tacoma Art Museum,
    // Maryhill Museum, OMSI itself (NARM has no default exclusion)
    assert!(eligible_ids.contains("portland-art-museum"));
    assert!(eligible_ids.contains("seattle-art-museum"));
    assert!(eligible_ids.contains("tacoma-art-museum"));
    assert!(eligible_ids.contains("maryhill-museum"));
    assert!(eligible_ids.contains("omsi")); // OMSI via NARM (no exclusion)

    // NARM excluded: Lan Su (15 mi flag, OMSI is ~1.5 mi away)
    let lansu_narm = results.iter().find(|r| {
        r.institution_id == "lan-su-chinese-garden" && r.network == Network::Narm
    });
    assert!(lansu_narm.is_none(), "Lan Su should be NARM-excluded for OMSI member");
}

// -----------------------------------------------------------------------
// Optimizer oracle
// -----------------------------------------------------------------------

/// The hand-derived optimal answer for the 8-target Aloha scenario.
///
/// Of the 8 targets:
/// - 6 are free-coverable: Pacific Science Center (ASTC), Seattle Art Museum,
///   Tacoma Art Museum, Maryhill Museum, Lan Su Chinese Garden, Portland Art
///   Museum (NARM).
/// - Oregon Coast Aquarium: ASTC-excluded (within 90 mi of residence) and has
///   no other network → no free path.
/// - Woodland Park Zoo: AZA discount only.
///
/// Key insight the optimizer discovers: Lan Su's 15-mi NARM flag excludes it
/// for OMSI-based memberships (OMSI ~1.5 mi away), but a Tacoma Art Museum
/// membership (>15 mi) covers Lan Su via NARM. So the optimal is:
///
///   Tacoma Art Museum Household ($95) → 5 NARM targets including Lan Su
///   + OMSI Explorer ($130) → Pacific Science Center (ASTC)
///   = $225, covering 6 of 8 targets.
///
/// This beats OMSI Family Plus alone ($185 for only 5 targets — Lan Su excluded).
#[test]
fn acceptance_optimizer_min_cost() {
    let (ds, zips) = load();
    let aloha = zips.lookup("97007").unwrap();

    let targets: Vec<&Institution> = TARGET_IDS
        .iter()
        .map(|id| ds.institution(id).unwrap())
        .collect();
    let all_memberships: Vec<&Membership> = ds.memberships.iter().collect();

    let candidates =
        solve::compute_candidate_coverage(aloha, &targets, &all_memberships, &ds);
    let exact = solve::solve_exact_min_cost(&candidates, targets.len());

    // The 6 free-coverable targets:
    // 0=pacific-science-center, 1=seattle-art-museum, 2=tacoma-art-museum,
    // 3=maryhill-museum, 5=lan-su-chinese-garden, 6=portland-art-museum
    let expected_free: BTreeSet<usize> = [0, 1, 2, 3, 5, 6].iter().copied().collect();
    assert_eq!(
        exact.covered_free, expected_free,
        "Should cover exactly the 6 free-coverable targets"
    );

    // Optimal cost: Tacoma ($95) + OMSI Explorer ($130) = $225
    assert_eq!(
        exact.total_cost, 225.0,
        "Optimal cost should be $225 (Tacoma Household + OMSI Explorer)"
    );

    // Should select exactly two memberships
    assert_eq!(exact.selected.len(), 2, "Should select exactly two memberships");

    // Verify the selected memberships
    let selected_ids: BTreeSet<(&str, &str)> = exact
        .selected
        .iter()
        .map(|&i| (
            candidates[i].institution_id.as_str(),
            candidates[i].tier.as_str(),
        ))
        .collect();
    assert!(
        selected_ids.contains(&("tacoma-art-museum", "Household")),
        "Should include Tacoma Art Museum Household"
    );
    assert!(
        selected_ids.contains(&("omsi", "Explorer")),
        "Should include OMSI Explorer"
    );
}

/// Verify the greedy solver also covers all free-coverable targets
/// and respects the ln(n)+1 bound.
#[test]
fn acceptance_greedy_bound() {
    let (ds, zips) = load();
    let aloha = zips.lookup("97007").unwrap();

    let targets: Vec<&Institution> = TARGET_IDS
        .iter()
        .map(|id| ds.institution(id).unwrap())
        .collect();
    let all_memberships: Vec<&Membership> = ds.memberships.iter().collect();

    let candidates =
        solve::compute_candidate_coverage(aloha, &targets, &all_memberships, &ds);
    let exact = solve::solve_exact_min_cost(&candidates, targets.len());
    let greedy = solve::solve_greedy_min_cost(&candidates, targets.len());

    // Greedy should cover at least as many as exact
    assert!(
        greedy.free_count() >= exact.free_count(),
        "greedy covers {} but exact covers {}",
        greedy.free_count(),
        exact.free_count()
    );

    // Greedy cost within ln(n)+1 of optimal
    if exact.total_cost > 0.0 {
        let bound = (targets.len() as f64).ln() + 1.0;
        assert!(
            greedy.total_cost <= exact.total_cost * bound,
            "greedy ${} > ln(8)+1 × exact ${} = ${:.0}",
            greedy.total_cost,
            exact.total_cost,
            exact.total_cost * bound
        );
    }
}

/// Oregon Coast Aquarium is the key ASTC exclusion test case.
/// ~83 mi from Aloha (within 90 mi of residence) → excluded.
/// ~91 mi from OMSI (just outside 90 mi from home institution) → would clear.
/// But ASTC both_must_clear means BOTH must be >90 mi, so it's excluded.
#[test]
fn acceptance_oca_astc_excluded() {
    let (ds, zips) = load();
    let aloha = zips.lookup("97007").unwrap();

    let targets: Vec<&Institution> = TARGET_IDS
        .iter()
        .map(|id| ds.institution(id).unwrap())
        .collect();
    let all_memberships: Vec<&Membership> = ds.memberships.iter().collect();

    let candidates =
        solve::compute_candidate_coverage(aloha, &targets, &all_memberships, &ds);

    // Oregon Coast Aquarium is target index 4
    let oca_index = 4;
    assert_eq!(TARGET_IDS[oca_index], "oregon-coast-aquarium");

    // No candidate should cover Oregon Coast Aquarium with free admission
    // (it's only in ASTC, and ASTC-excluded)
    let any_free = candidates.iter().any(|c| c.covers_free.contains(&oca_index));
    assert!(
        !any_free,
        "Oregon Coast Aquarium should not be free-coverable for Aloha resident"
    );
}

/// Lan Su is NARM-excluded for OMSI-based memberships (15 mi flag),
/// but NARM-eligible for memberships from institutions >15 mi away.
#[test]
fn acceptance_lan_su_narm_exclusion() {
    let (ds, zips) = load();
    let aloha = zips.lookup("97007").unwrap();

    let targets: Vec<&Institution> = TARGET_IDS
        .iter()
        .map(|id| ds.institution(id).unwrap())
        .collect();
    let all_memberships: Vec<&Membership> = ds.memberships.iter().collect();

    let candidates =
        solve::compute_candidate_coverage(aloha, &targets, &all_memberships, &ds);

    // Lan Su is target index 5
    let lansu_index = 5;
    assert_eq!(TARGET_IDS[lansu_index], "lan-su-chinese-garden");

    // OMSI-based memberships should NOT cover Lan Su (15 mi flag)
    let omsi_cands: Vec<&solve::CandidateMembership> = candidates
        .iter()
        .filter(|c| c.institution_id == "omsi")
        .collect();
    for c in &omsi_cands {
        assert!(
            !c.covers_free.contains(&lansu_index),
            "OMSI {} should not cover Lan Su via NARM (15 mi flag)",
            c.tier
        );
    }

    // But Tacoma Art Museum or Seattle Art Museum membership SHOULD cover Lan Su
    // (those institutions are >15 mi from Lan Su)
    let distant_covers_lansu = candidates
        .iter()
        .filter(|c| c.institution_id != "omsi" && c.institution_id != "portland-art-museum" && c.institution_id != "lan-su-chinese-garden" && c.institution_id != "portland-childrens-museum")
        .any(|c| c.covers_free.contains(&lansu_index));
    assert!(
        distant_covers_lansu,
        "At least one distant NARM membership should cover Lan Su"
    );
}

/// Budget-constrained scenario: with $150, what's the best coverage?
#[test]
fn acceptance_budget_constrained() {
    let (ds, zips) = load();
    let aloha = zips.lookup("97007").unwrap();

    let targets: Vec<&Institution> = TARGET_IDS
        .iter()
        .map(|id| ds.institution(id).unwrap())
        .collect();
    let all_memberships: Vec<&Membership> = ds.memberships.iter().collect();

    let candidates =
        solve::compute_candidate_coverage(aloha, &targets, &all_memberships, &ds);
    let max_cov = solve::solve_exact_max_coverage(&candidates, targets.len(), 150.0);

    // With $150, can't afford OMSI Family Plus ($185).
    // Best is OMSI Explorer ($130, ASTC only → Pacific Science Center)
    // or Tacoma Art Museum ($95, NARM → 4 targets)
    // or Lan Su ($100, NARM → some targets depending on home inst)
    // Tacoma gives the most coverage for $150 budget.
    assert!(max_cov.total_cost <= 150.0);
    assert!(
        max_cov.free_count() >= 3,
        "Should cover at least 3 targets under $150"
    );
}

/// ZIP centroid lookup is correct for acceptance test.
#[test]
fn acceptance_zip_97007() {
    let zips = ZipCentroids::from_json(ZIP_CENTROIDS_JSON).unwrap();
    let aloha = zips.lookup("97007").unwrap();
    // Aloha, OR is roughly (45.47-45.49, -122.85 to -122.87)
    assert!((45.45..=45.51).contains(&aloha.lat), "lat {}", aloha.lat);
    assert!((-122.90..=-122.83).contains(&aloha.lon), "lon {}", aloha.lon);
}

/// Full CLI-equivalent end-to-end: the same query the CLI runs.
#[test]
fn acceptance_full_end_to_end() {
    let (ds, zips) = load();
    let aloha = zips.lookup("97007").unwrap();

    let targets: Vec<&Institution> = TARGET_IDS
        .iter()
        .map(|id| ds.institution(id).unwrap())
        .collect();
    let all_memberships: Vec<&Membership> = ds.memberships.iter().collect();

    let candidates =
        solve::compute_candidate_coverage(aloha, &targets, &all_memberships, &ds);

    // Exact min-cost
    let exact = solve::solve_exact_min_cost(&candidates, targets.len());

    // Snapshot assertions:
    // 1. Optimal cost is $225 (Tacoma Household + OMSI Explorer)
    assert_eq!(exact.total_cost, 225.0);

    // 2. Covers 6 of 8 targets with free admission
    assert_eq!(exact.free_count(), 6);

    // 3. Uncovered (no free path): Oregon Coast Aquarium, Woodland Park Zoo
    let uncovered: BTreeSet<usize> = (0..targets.len())
        .filter(|ti| !exact.covered_free.contains(ti))
        .collect();
    let uncovered_ids: Vec<&str> = uncovered.iter().map(|&ti| TARGET_IDS[ti]).collect();
    assert!(
        uncovered_ids.contains(&"oregon-coast-aquarium"),
        "OCA should be uncovered"
    );
    assert!(
        uncovered_ids.contains(&"woodland-park-zoo"),
        "Woodland Park Zoo should be uncovered (AZA discount, not free)"
    );
    // Lan Su IS covered — via Tacoma Art Museum NARM (>15 mi from Lan Su)
    assert!(
        !uncovered_ids.contains(&"lan-su-chinese-garden"),
        "Lan Su should be covered via distant NARM membership"
    );

    // 4. Arbitrage report
    let target_ids: Vec<&str> = TARGET_IDS.to_vec();
    let report = solve::arbitrage_report(&candidates, &exact, &target_ids);

    // Pacific Science Center: cheapest free is OMSI Explorer ($130)
    assert_eq!(
        report.per_target[0].cheapest_free.unwrap().1, 130.0,
        "Cheapest free for Pacific Sci Center should be $130 (OMSI Explorer)"
    );

    // Lan Su: cheapest free is Tacoma Art Museum ($95)
    assert_eq!(
        report.per_target[5].cheapest_free.unwrap().1, 95.0,
        "Cheapest free for Lan Su should be $95 (Tacoma Art Museum)"
    );

    // Woodland Park Zoo: no free path, but has discounted
    assert!(
        report.per_target[7].cheapest_free.is_none(),
        "Woodland Park Zoo should have no free path"
    );
    assert!(
        report.per_target[7].cheapest_discounted.is_some(),
        "Woodland Park Zoo should have discounted path"
    );
}
