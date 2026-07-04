//! Eligibility rules engine — Ascent Datalog over the domain model.
//!
//! The architecture is:
//! 1. Rust code computes geographic exclusions (haversine) and populates
//!    `excluded` facts.
//! 2. Ascent derives `eligible(user, institution, network, admission_kind)`
//!    via negation-as-absence.
//!
//! Admission kinds are encoded as `u8` for Ascent compatibility:
//! - 0 = Free
//! - 1 = Discount (the fraction is carried in a separate lookup, not in the
//!   Datalog relation, because `f32` isn't `Hash + Eq`).

use ascent::ascent;

use crate::geo;
use crate::model::*;

// ---------------------------------------------------------------------------
// Admission encoding for Ascent (must be Hash + Eq)
// ---------------------------------------------------------------------------

/// Compact admission kind for use in Ascent relations.
/// The actual discount fraction is looked up separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum AdmissionKind {
    Free = 0,
    Discount = 1,
}

impl From<&Admission> for AdmissionKind {
    fn from(a: &Admission) -> Self {
        match a {
            Admission::Free => AdmissionKind::Free,
            Admission::Discount { .. } => AdmissionKind::Discount,
        }
    }
}

// ---------------------------------------------------------------------------
// Eligibility result
// ---------------------------------------------------------------------------

/// One eligible (user, institution, network) triple with its admission type.
#[derive(Debug, Clone, PartialEq)]
pub struct EligibilityResult {
    pub institution_id: String,
    pub network: Network,
    pub admission: Admission,
    /// True if special/temporary exhibitions are restricted.
    pub special_exhibit_restricted: bool,
}

// ---------------------------------------------------------------------------
// Ascent program
// ---------------------------------------------------------------------------

ascent! {
    struct EligibilityProg;

    /// (institution_id, network_key, admission_kind)
    relation participates(String, String, AdmissionKind);

    /// (network_key) — networks the user holds via their memberships.
    relation holds_network(String);

    /// (institution_id, network_key) — pre-computed by geo logic.
    relation excluded(String, String);

    /// (institution_id, network_key, admission_kind) — derived.
    relation eligible(String, String, AdmissionKind);

    eligible(i, n, adm) <--
        participates(i, n, adm),
        holds_network(n),
        !excluded(i, n);
}

// ---------------------------------------------------------------------------
// Network key helper
// ---------------------------------------------------------------------------

fn network_key(n: Network) -> String {
    match n {
        Network::Narm => "narm".into(),
        Network::Astc => "astc".into(),
        Network::Ahs => "ahs".into(),
        Network::Roam => "roam".into(),
        Network::Marp => "marp".into(),
        Network::Acm => "acm".into(),
        Network::Aza => "aza".into(),
        Network::TimeTravelers => "time_travelers".into(),
    }
}

fn key_to_network(k: &str) -> Option<Network> {
    match k {
        "narm" => Some(Network::Narm),
        "astc" => Some(Network::Astc),
        "ahs" => Some(Network::Ahs),
        "roam" => Some(Network::Roam),
        "marp" => Some(Network::Marp),
        "acm" => Some(Network::Acm),
        "aza" => Some(Network::Aza),
        "time_travelers" => Some(Network::TimeTravelers),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Exclusion logic (Rust, not Datalog — because it needs f64 geo)
// ---------------------------------------------------------------------------

/// Determine the effective exclusion rule for a participation:
/// institution-level override > network default.
fn effective_exclusion(participation: &Participation, dataset: &Dataset) -> ExclusionRule {
    if let Some(ref excl) = participation.exclusion {
        *excl
    } else {
        dataset
            .network_spec(participation.network)
            .map(|s| s.default_exclusion)
            .unwrap_or(ExclusionRule::NONE)
    }
}

/// Determine the effective admission for a participation:
/// institution-level override > network default.
fn effective_admission(participation: &Participation, dataset: &Dataset) -> Admission {
    if let Some(ref adm) = participation.admission {
        *adm
    } else {
        dataset
            .network_spec(participation.network)
            .map(|s| s.admission)
            .unwrap_or(Admission::Free)
    }
}

/// Check whether a user is excluded from visiting `target` via `network`
/// given the exclusion rule.
///
/// `home_institution_loc`: location of the institution granting the network.
/// For ASTC `both_must_clear`: target must be >radius from BOTH residence AND
/// home institution. If EITHER is within radius, excluded.
fn is_excluded(
    user: &User,
    target_loc: LatLon,
    home_institution_loc: Option<LatLon>,
    rule: &ExclusionRule,
) -> bool {
    let res_within = rule
        .residence_radius_mi
        .map(|r| geo::within(user.residence, target_loc, r))
        .unwrap_or(false);

    let home_within = match (rule.home_institution_radius_mi, home_institution_loc) {
        (Some(r), Some(home_loc)) => geo::within(home_loc, target_loc, r),
        _ => false,
    };

    if rule.both_must_clear {
        // ASTC model: excluded if EITHER radius is violated.
        // "both must clear" means target must be outside BOTH radii.
        res_within || home_within
    } else {
        // Independent checks: each violated radius independently excludes.
        res_within || home_within
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compute the full eligibility set for a user given the dataset and the
/// networks the user holds (derived from their memberships).
///
/// Returns a list of `EligibilityResult` — one per (institution, network)
/// pair that the user can visit.
pub fn compute_eligibility(
    user: &User,
    dataset: &Dataset,
    networks_held: &[Network],
    home_institution_id: Option<&str>,
) -> Vec<EligibilityResult> {
    let home_loc = home_institution_id
        .and_then(|id| dataset.institution(id))
        .map(|inst| inst.location);

    let mut prog = EligibilityProg::default();

    // Populate holds_network
    for &net in networks_held {
        prog.holds_network.push((network_key(net),));
    }

    // Populate participates + compute excluded facts
    for inst in &dataset.institutions {
        for part in &inst.participates {
            let nk = network_key(part.network);
            let adm = effective_admission(part, dataset);
            let adm_kind = AdmissionKind::from(&adm);

            prog.participates
                .push((inst.id.clone(), nk.clone(), adm_kind));

            // Compute exclusion
            let rule = effective_exclusion(part, dataset);
            if is_excluded(user, inst.location, home_loc, &rule) {
                prog.excluded.push((inst.id.clone(), nk.clone()));
            }
        }
    }

    // Run Datalog
    prog.run();

    // Collect results, enriching with full Admission and special_exhibit_restricted
    let mut results = Vec::new();
    for (inst_id, nk, _adm_kind) in &prog.eligible {
        let network = match key_to_network(nk) {
            Some(n) => n,
            None => continue,
        };
        let inst = match dataset.institution(inst_id) {
            Some(i) => i,
            None => continue,
        };
        let part = match inst.participates.iter().find(|p| p.network == network) {
            Some(p) => p,
            None => continue,
        };
        let admission = effective_admission(part, dataset);
        results.push(EligibilityResult {
            institution_id: inst_id.clone(),
            network,
            admission,
            special_exhibit_restricted: part.special_exhibit_restricted,
        });
    }

    // Sort for deterministic output
    results.sort_by(|a, b| {
        a.institution_id
            .cmp(&b.institution_id)
            .then_with(|| format!("{}", a.network).cmp(&format!("{}", b.network)))
    });

    results
}

// ---------------------------------------------------------------------------
// Convenience: derive networks_held from memberships
// ---------------------------------------------------------------------------

/// Given a user's held membership refs, determine which networks they have
/// access to.
pub fn networks_from_memberships(user: &User, dataset: &Dataset) -> Vec<Network> {
    let mut nets: Vec<Network> = user
        .held
        .iter()
        .flat_map(|mref| {
            dataset
                .memberships
                .iter()
                .filter(|m| m.institution_id == mref.institution_id && m.tier == mref.tier)
                .flat_map(|m| m.networks_unlocked.iter().copied())
        })
        .collect();
    nets.sort_by_key(|n| *n as u8);
    nets.dedup();
    nets
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture() -> Dataset {
        let networks: Vec<NetworkSpec> =
            serde_json::from_str(include_str!("../../../data/networks.json")).unwrap();
        let institutions: Vec<Institution> =
            serde_json::from_str(include_str!("../../../data/institutions.json")).unwrap();
        let memberships: Vec<Membership> =
            serde_json::from_str(include_str!("../../../data/memberships.json")).unwrap();
        Dataset {
            networks,
            institutions,
            memberships,
        }
    }

    /// Aloha, OR resident with OMSI Family Plus (ASTC + NARM).
    /// Oregon Coast Aquarium (Newport) is ~83 mi from Aloha — within ASTC's
    /// 90 mi residence radius → EXCLUDED even though OMSI is ~91 mi from
    /// Newport (would clear the home-institution radius).
    /// This is the acceptance-test case from the brief.
    #[test]
    fn astc_both_must_clear_excludes_newport() {
        let ds = load_fixture();
        let aloha = LatLon::new(45.4912, -122.8720);
        let user = User {
            residence: aloha,
            held: vec![MembershipRef {
                institution_id: "omsi".into(),
                tier: "Family Plus".into(),
            }],
        };

        let results = compute_eligibility(
            &user,
            &ds,
            &[Network::Astc, Network::Narm],
            Some("omsi"),
        );

        // Oregon Coast Aquarium should NOT appear as ASTC-eligible
        let oca_astc = results.iter().find(|r| {
            r.institution_id == "oregon-coast-aquarium" && r.network == Network::Astc
        });
        assert!(
            oca_astc.is_none(),
            "Oregon Coast Aquarium should be ASTC-excluded for Aloha resident \
             (within 90 mi of residence)"
        );
    }

    /// OMSI itself should be ASTC-excluded for an Aloha resident holding OMSI
    /// membership — it's ~11 mi from home, well within 90 mi of both
    /// residence and home institution.
    #[test]
    fn astc_excludes_home_institution_area() {
        let ds = load_fixture();
        let aloha = LatLon::new(45.4912, -122.8720);
        let user = User {
            residence: aloha,
            held: vec![MembershipRef {
                institution_id: "omsi".into(),
                tier: "Family Plus".into(),
            }],
        };

        let results = compute_eligibility(
            &user,
            &ds,
            &[Network::Astc],
            Some("omsi"),
        );

        // OMSI should NOT appear as ASTC-eligible (within 90 mi of residence)
        let omsi_astc = results
            .iter()
            .find(|r| r.institution_id == "omsi" && r.network == Network::Astc);
        assert!(
            omsi_astc.is_none(),
            "OMSI should be ASTC-excluded for an Aloha resident"
        );
    }

    /// Pacific Science Center (Seattle, ~145 mi from Aloha) should be
    /// ASTC-eligible — clears both 90-mi radii.
    #[test]
    fn astc_eligible_when_both_clear() {
        let ds = load_fixture();
        let aloha = LatLon::new(45.4912, -122.8720);
        let user = User {
            residence: aloha,
            held: vec![MembershipRef {
                institution_id: "omsi".into(),
                tier: "Family Plus".into(),
            }],
        };

        let results = compute_eligibility(
            &user,
            &ds,
            &[Network::Astc],
            Some("omsi"),
        );

        let psc = results.iter().find(|r| {
            r.institution_id == "pacific-science-center" && r.network == Network::Astc
        });
        assert!(
            psc.is_some(),
            "Pacific Science Center should be ASTC-eligible for Aloha resident \
             (~145 mi from both Aloha and OMSI)"
        );
        assert!(psc.unwrap().admission.is_free());
    }

    /// Lan Su Chinese Garden has a NARM 15-mi exclusion from home institution.
    /// OMSI is ~1.5 mi from Lan Su → an OMSI member is NARM-excluded from Lan Su.
    #[test]
    fn narm_15mi_flag_excludes_nearby() {
        let ds = load_fixture();
        let aloha = LatLon::new(45.4912, -122.8720);
        let user = User {
            residence: aloha,
            held: vec![MembershipRef {
                institution_id: "omsi".into(),
                tier: "Family Plus".into(),
            }],
        };

        let results = compute_eligibility(
            &user,
            &ds,
            &[Network::Narm],
            Some("omsi"),
        );

        let lan_su_narm = results.iter().find(|r| {
            r.institution_id == "lan-su-chinese-garden" && r.network == Network::Narm
        });
        assert!(
            lan_su_narm.is_none(),
            "Lan Su should be NARM-excluded for an OMSI member (within 15 mi)"
        );
    }

    /// Lan Su with a *distant* home institution (Seattle Art Museum, ~145 mi
    /// from Lan Su) should be NARM-eligible.
    #[test]
    fn narm_15mi_flag_allows_distant_home() {
        let ds = load_fixture();
        // User lives in Seattle, home institution is Seattle Art Museum
        let seattle = LatLon::new(47.6073, -122.3380);
        let user = User {
            residence: seattle,
            held: vec![MembershipRef {
                institution_id: "seattle-art-museum".into(),
                tier: "Household".into(),
            }],
        };

        let results = compute_eligibility(
            &user,
            &ds,
            &[Network::Narm],
            Some("seattle-art-museum"),
        );

        let lan_su = results.iter().find(|r| {
            r.institution_id == "lan-su-chinese-garden" && r.network == Network::Narm
        });
        assert!(
            lan_su.is_some(),
            "Lan Su should be NARM-eligible for a Seattle Art Museum member \
             (home institution >15 mi away)"
        );
    }

    /// ACM admission should be Discount(0.5), not Free.
    #[test]
    fn acm_eligible_but_discounted() {
        let ds = load_fixture();
        let portland = LatLon::new(45.5152, -122.6784);
        let user = User {
            residence: portland,
            held: vec![MembershipRef {
                institution_id: "portland-childrens-museum".into(),
                tier: "Family".into(),
            }],
        };

        let results = compute_eligibility(
            &user,
            &ds,
            &[Network::Acm],
            Some("portland-childrens-museum"),
        );

        // Children's Museum of Tacoma should be ACM-eligible with discount
        let tacoma_cm = results.iter().find(|r| {
            r.institution_id == "childrens-museum-of-tacoma" && r.network == Network::Acm
        });
        assert!(
            tacoma_cm.is_some(),
            "Children's Museum of Tacoma should be ACM-eligible"
        );
        match tacoma_cm.unwrap().admission {
            Admission::Discount { fraction } => {
                assert!(
                    (fraction - 0.5).abs() < f32::EPSILON,
                    "ACM discount should be 50%, got {fraction}"
                );
            }
            Admission::Free => panic!("ACM should be Discount, not Free"),
        }
    }

    /// Portland Children's Museum should be ACM-eligible for itself (ACM has
    /// no exclusion rule) — but it's the home institution, which is fine
    /// because ACM's default_exclusion has no radius.
    #[test]
    fn acm_no_exclusion_allows_home() {
        let ds = load_fixture();
        let portland = LatLon::new(45.5152, -122.6784);
        let user = User {
            residence: portland,
            held: vec![MembershipRef {
                institution_id: "portland-childrens-museum".into(),
                tier: "Family".into(),
            }],
        };

        let results = compute_eligibility(
            &user,
            &ds,
            &[Network::Acm],
            Some("portland-childrens-museum"),
        );

        let pcm = results.iter().find(|r| {
            r.institution_id == "portland-childrens-museum" && r.network == Network::Acm
        });
        assert!(
            pcm.is_some(),
            "Portland Children's Museum should be ACM-eligible (no exclusion rule)"
        );
    }

    /// networks_from_memberships correctly derives networks from membership refs.
    #[test]
    fn test_networks_from_memberships() {
        let ds = load_fixture();
        let user = User {
            residence: LatLon::new(45.0, -122.0),
            held: vec![MembershipRef {
                institution_id: "omsi".into(),
                tier: "Family Plus".into(),
            }],
        };
        let nets = networks_from_memberships(&user, &ds);
        assert!(nets.contains(&Network::Astc));
        assert!(nets.contains(&Network::Narm));
        assert_eq!(nets.len(), 2);
    }

    /// User holding no networks gets no eligibility.
    #[test]
    fn no_networks_no_eligibility() {
        let ds = load_fixture();
        let user = User {
            residence: LatLon::new(45.0, -122.0),
            held: vec![],
        };
        let results = compute_eligibility(&user, &ds, &[], None);
        assert!(results.is_empty());
    }

    /// Full scenario: Aloha resident with OMSI Family Plus (ASTC+NARM).
    /// Count eligible institutions and verify key inclusions/exclusions.
    #[test]
    fn full_scenario_aloha_omsi() {
        let ds = load_fixture();
        let aloha = LatLon::new(45.4912, -122.8720);
        let user = User {
            residence: aloha,
            held: vec![MembershipRef {
                institution_id: "omsi".into(),
                tier: "Family Plus".into(),
            }],
        };

        let results = compute_eligibility(
            &user,
            &ds,
            &[Network::Astc, Network::Narm],
            Some("omsi"),
        );

        // Print for debugging
        for r in &results {
            let adm = if r.admission.is_free() {
                "free".to_string()
            } else if let Admission::Discount { fraction } = r.admission {
                format!("{}% off", (fraction * 100.0) as u32)
            } else {
                "?".into()
            };
            eprintln!(
                "  {} via {} ({}){}",
                r.institution_id,
                r.network,
                adm,
                if r.special_exhibit_restricted {
                    " [special exhibits restricted]"
                } else {
                    ""
                }
            );
        }

        // ASTC: Pacific Science Center + other distant ASTC institutions should be
        // eligible (>90 mi from both Aloha residence and OMSI home institution).
        // Oregon Coast Aquarium (~83 mi from residence) and nearby OMSI/Gilbert House
        // are excluded by the both_must_clear 90-mi rule.
        let astc_eligible: Vec<_> = results
            .iter()
            .filter(|r| r.network == Network::Astc)
            .collect();
        let astc_ids: Vec<&str> = astc_eligible.iter().map(|r| r.institution_id.as_str()).collect();
        assert!(
            !astc_eligible.is_empty(),
            "at least one ASTC institution should be eligible (>90mi from both residence and home), got 0"
        );
        assert!(astc_ids.contains(&"pacific-science-center"));
        // Oregon Coast Aquarium should NOT be eligible (~83 mi from Aloha)
        assert!(!astc_ids.contains(&"oregon-coast-aquarium"));
        // OMSI itself is excluded (0 mi from home)
        assert!(!astc_ids.contains(&"omsi"));

        // NARM: should include Seattle-area + Maryhill (distant), exclude
        // Lan Su (15mi flag) and Portland-area NARM with no exclusion
        // actually Portland Art Museum has no NARM exclusion override → eligible
        let narm_eligible: Vec<_> = results
            .iter()
            .filter(|r| r.network == Network::Narm)
            .collect();

        let narm_ids: Vec<&str> = narm_eligible.iter().map(|r| r.institution_id.as_str()).collect();

        // Lan Su should NOT be in NARM (15 mi exclusion, OMSI is ~1.5 mi away)
        assert!(!narm_ids.contains(&"lan-su-chinese-garden"));

        // Seattle Art Museum, Tacoma Art Museum, Maryhill should be NARM-eligible
        // (no exclusion rules, distant enough)
        assert!(narm_ids.contains(&"seattle-art-museum"));
        assert!(narm_ids.contains(&"tacoma-art-museum"));
        assert!(narm_ids.contains(&"maryhill-museum"));

        // Portland Art Museum: NARM has no default exclusion, and PAM has no
        // per-institution exclusion override → eligible
        assert!(narm_ids.contains(&"portland-art-museum"));

        // OMSI itself is in NARM too (no exclusion for NARM default)
        assert!(narm_ids.contains(&"omsi"));
    }
}
