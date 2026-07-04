//! Integration test: deserialize the hand-authored JSON fixtures, validate
//! counts and field integrity, and re-serialize (round-trip).

use tessera_core::*;

const NETWORKS_JSON: &str = include_str!("../../../data/networks.json");
const INSTITUTIONS_JSON: &str = include_str!("../../../data/institutions.json");
const MEMBERSHIPS_JSON: &str = include_str!("../../../data/memberships.json");

fn load_dataset() -> Dataset {
    let networks: Vec<NetworkSpec> =
        serde_json::from_str(NETWORKS_JSON).expect("networks.json parse failed");
    let institutions: Vec<Institution> =
        serde_json::from_str(INSTITUTIONS_JSON).expect("institutions.json parse failed");
    let memberships: Vec<Membership> =
        serde_json::from_str(MEMBERSHIPS_JSON).expect("memberships.json parse failed");
    Dataset {
        networks,
        institutions,
        memberships,
    }
}

#[test]
fn fixture_deserializes() {
    let ds = load_dataset();
    assert!(!ds.networks.is_empty(), "networks should not be empty");
    assert!(!ds.institutions.is_empty(), "institutions should not be empty");
    assert!(!ds.memberships.is_empty(), "memberships should not be empty");
}

#[test]
fn fixture_network_count() {
    let ds = load_dataset();
    assert_eq!(
        ds.networks.len(),
        8,
        "expected 8 network specs (one per Network variant)"
    );
}

#[test]
fn fixture_institution_count() {
    let ds = load_dataset();
    assert!(
        ds.institutions.len() >= 12,
        "expected at least 12 fixture institutions, got {}",
        ds.institutions.len()
    );
}

#[test]
fn fixture_membership_count() {
    let ds = load_dataset();
    assert_eq!(
        ds.memberships.len(),
        10,
        "expected 10 fixture memberships"
    );
}

#[test]
fn institution_ids_unique() {
    let ds = load_dataset();
    let mut ids: Vec<&str> = ds.institutions.iter().map(|i| i.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        ds.institutions.len(),
        "institution IDs must be unique"
    );
}

#[test]
fn membership_institution_refs_valid() {
    let ds = load_dataset();
    for m in &ds.memberships {
        assert!(
            ds.institution(&m.institution_id).is_some(),
            "membership references unknown institution: {}",
            m.institution_id
        );
    }
}

#[test]
fn all_institutions_have_provenance() {
    let ds = load_dataset();
    for inst in &ds.institutions {
        assert!(
            !inst.provenance.is_empty(),
            "institution {} missing provenance",
            inst.id
        );
    }
}

#[test]
fn all_institutions_have_location() {
    let ds = load_dataset();
    for inst in &ds.institutions {
        assert!(
            inst.location.lat.abs() <= 90.0 && inst.location.lon.abs() <= 180.0,
            "institution {} has invalid coordinates: {:?}",
            inst.id,
            inst.location
        );
    }
}

#[test]
fn most_institutions_have_at_least_one_network() {
    let ds = load_dataset();
    let no_network: Vec<&str> = ds
        .institutions
        .iter()
        .filter(|i| i.participates.is_empty())
        .map(|i| i.id.as_str())
        .collect();
    // Some notable museums are tracked for reference even without network membership.
    // Ensure the majority participate in at least one network.
    assert!(
        no_network.len() < ds.institutions.len() / 4,
        "too many institutions have no network: {:?}",
        no_network
    );
}

#[test]
fn omsi_participates_in_astc_and_narm() {
    let ds = load_dataset();
    let omsi = ds.institution("omsi").expect("OMSI not found");
    let networks: Vec<Network> = omsi.participates.iter().map(|p| p.network).collect();
    assert!(networks.contains(&Network::Astc), "OMSI should be in ASTC");
    assert!(networks.contains(&Network::Narm), "OMSI should be in NARM");
}

#[test]
fn astc_exclusion_rule_correct() {
    let ds = load_dataset();
    let astc = ds.network_spec(Network::Astc).expect("ASTC spec missing");
    assert_eq!(astc.default_exclusion.residence_radius_mi, Some(90.0));
    assert_eq!(astc.default_exclusion.home_institution_radius_mi, Some(90.0));
    assert!(astc.default_exclusion.both_must_clear);
}

#[test]
fn lan_su_narm_has_15mi_exclusion() {
    let ds = load_dataset();
    let lan_su = ds.institution("lan-su-chinese-garden").expect("Lan Su not found");
    let narm_part = lan_su
        .participates
        .iter()
        .find(|p| p.network == Network::Narm)
        .expect("Lan Su should participate in NARM");
    let excl = narm_part.exclusion.as_ref().expect("Lan Su NARM should have exclusion override");
    assert_eq!(excl.home_institution_radius_mi, Some(15.0));
}

#[test]
fn acm_admission_is_discount() {
    let ds = load_dataset();
    let acm = ds.network_spec(Network::Acm).expect("ACM spec missing");
    match acm.admission {
        Admission::Discount { fraction } => {
            assert!(
                (fraction - 0.5).abs() < f32::EPSILON,
                "ACM discount should be 50%"
            );
        }
        _ => panic!("ACM admission should be Discount, not Free"),
    }
}

#[test]
fn round_trip_serde() {
    let ds = load_dataset();
    // Serialize the dataset to JSON and back
    let json = serde_json::to_string_pretty(&ds).expect("serialize failed");
    let ds2: Dataset = serde_json::from_str(&json).expect("deserialize round-trip failed");
    assert_eq!(ds2.institutions.len(), ds.institutions.len());
    assert_eq!(ds2.networks.len(), ds.networks.len());
    assert_eq!(ds2.memberships.len(), ds.memberships.len());
}

#[test]
fn dataset_lookup_helpers() {
    let ds = load_dataset();
    // network_spec
    assert!(ds.network_spec(Network::Astc).is_some());
    assert!(ds.network_spec(Network::TimeTravelers).is_some());

    // institution
    assert!(ds.institution("omsi").is_some());
    assert!(ds.institution("nonexistent").is_none());

    // memberships_for
    let omsi_memberships = ds.memberships_for("omsi");
    assert_eq!(omsi_memberships.len(), 2, "OMSI should have 2 tiers");
}

#[test]
fn all_institutions_have_explicit_type() {
    let ds = load_dataset();
    // Every institution must have a meaningful institution_type.
    // The Specialty type is valid but should be rare — if more than 10%
    // of institutions are Specialty, someone probably forgot to classify
    // newly-ingested data.
    let specialty_count = ds
        .institutions
        .iter()
        .filter(|i| i.institution_type == InstitutionType::Specialty)
        .count();
    let threshold = ds.institutions.len() / 10 + 1;
    assert!(
        specialty_count <= threshold,
        "too many institutions classified as Specialty ({specialty_count}/{} — threshold {threshold}); \
         newly ingested institutions likely need manual classification",
        ds.institutions.len()
    );

    // Every InstitutionType variant should appear at least once
    // (sanity check that the enum and data stay in sync).
    for t in InstitutionType::ALL {
        let count = ds.institutions.iter().filter(|i| i.institution_type == *t).count();
        assert!(
            count > 0,
            "no institutions of type {t:?} found — is the type still in use?"
        );
    }
}
