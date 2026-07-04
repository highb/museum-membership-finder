use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::Path;
use tessera_core::model::{Institution, Membership, NetworkSpec};

/// Load and deserialize a JSON array from `data/{name}`.
fn load_json<T: serde::de::DeserializeOwned>(name: &str) -> Result<T> {
    let path = Path::new("data").join(name);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))
}

pub fn run() -> Result<()> {
    let networks: Vec<NetworkSpec> = load_json("networks.json")?;
    let institutions: Vec<Institution> = load_json("institutions.json")?;
    let memberships: Vec<Membership> = load_json("memberships.json")?;

    let errors = validate(&networks, &institutions, &memberships);

    if errors.is_empty() {
        println!("✓ All data files valid ({} networks, {} institutions, {} memberships)",
            networks.len(), institutions.len(), memberships.len());
        Ok(())
    } else {
        for e in &errors {
            eprintln!("  ✗ {e}");
        }
        anyhow::bail!("{} validation error(s)", errors.len());
    }
}

pub fn validate(
    networks: &[NetworkSpec],
    institutions: &[Institution],
    memberships: &[Membership],
) -> Vec<String> {
    let mut errors = Vec::new();

    // Build lookup sets
    let network_set: HashSet<_> = networks.iter().map(|n| n.network).collect();
    let inst_ids: Vec<&str> = institutions.iter().map(|i| i.id.as_str()).collect();
    let inst_set: HashSet<&str> = inst_ids.iter().copied().collect();

    // No duplicate institution IDs
    if inst_ids.len() != inst_set.len() {
        let mut seen = HashSet::new();
        for id in &inst_ids {
            if !seen.insert(id) {
                errors.push(format!("duplicate institution id: {id}"));
            }
        }
    }

    // Institution checks
    for inst in institutions {
        // Provenance
        if inst.provenance.is_empty() {
            errors.push(format!("institution '{}': empty provenance", inst.id));
        }

        // Coordinates
        if inst.location.lat < -90.0 || inst.location.lat > 90.0 {
            errors.push(format!("institution '{}': lat {} out of range", inst.id, inst.location.lat));
        }
        if inst.location.lon < -180.0 || inst.location.lon > 180.0 {
            errors.push(format!("institution '{}': lon {} out of range", inst.id, inst.location.lon));
        }

        // participates → valid networks
        for p in &inst.participates {
            if !network_set.contains(&p.network) {
                errors.push(format!(
                    "institution '{}': participates in unknown network {:?}",
                    inst.id, p.network
                ));
            }
        }
    }

    // Membership checks
    let mut membership_keys = HashSet::new();
    for m in memberships {
        // institution_id references valid institution
        if !inst_set.contains(m.institution_id.as_str()) {
            errors.push(format!(
                "membership '{}/{}': unknown institution_id '{}'",
                m.institution_id, m.tier, m.institution_id
            ));
        }

        // No duplicate (institution_id, tier)
        let key = (m.institution_id.as_str(), m.tier.as_str());
        if !membership_keys.insert(key) {
            errors.push(format!("duplicate membership: {} / {}", m.institution_id, m.tier));
        }

        // networks_unlocked → valid networks
        for n in &m.networks_unlocked {
            if !network_set.contains(n) {
                errors.push(format!(
                    "membership '{}/{}': unknown network {:?} in networks_unlocked",
                    m.institution_id, m.tier, n
                ));
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_core::model::*;

    fn sample_networks() -> Vec<NetworkSpec> {
        vec![NetworkSpec {
            network: Network::Astc,
            admission: Admission::Free,
            default_exclusion: ExclusionRule::ASTC_DEFAULT,
        }]
    }

    fn sample_institutions() -> Vec<Institution> {
        vec![Institution {
            id: "test-museum".into(),
            name: "Test Museum".into(),
            city: "Portland".into(),
            region: "OR".into(),
            country: "US".into(),
            location: LatLon::new(45.5, -122.6),
            website: None,
            institution_type: InstitutionType::Specialty,
            participates: vec![Participation {
                network: Network::Astc,
                admission: None,
                exclusion: None,
                special_exhibit_restricted: false,
            }],
            provenance: "test".into(),
        }]
    }

    fn sample_memberships() -> Vec<Membership> {
        vec![Membership {
            institution_id: "test-museum".into(),
            tier: "Basic".into(),
            price_usd: 50.0,
            networks_unlocked: vec![Network::Astc],
            guests_included: 2,
        }]
    }

    #[test]
    fn valid_data_passes() {
        let errs = validate(&sample_networks(), &sample_institutions(), &sample_memberships());
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    }

    #[test]
    fn real_data_passes() {
        // This test validates the actual data/ files ship clean.
        let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let networks: Vec<NetworkSpec> = serde_json::from_str(
            &std::fs::read_to_string(data_dir.join("networks.json")).unwrap(),
        ).unwrap();
        let institutions: Vec<Institution> = serde_json::from_str(
            &std::fs::read_to_string(data_dir.join("institutions.json")).unwrap(),
        ).unwrap();
        let memberships: Vec<Membership> = serde_json::from_str(
            &std::fs::read_to_string(data_dir.join("memberships.json")).unwrap(),
        ).unwrap();
        let errs = validate(&networks, &institutions, &memberships);
        assert!(errs.is_empty(), "data/ validation errors: {errs:?}");
    }

    #[test]
    fn duplicate_institution_id_detected() {
        let nets = sample_networks();
        let mut insts = sample_institutions();
        insts.push(insts[0].clone());
        let mems = sample_memberships();
        let errs = validate(&nets, &insts, &mems);
        assert!(errs.iter().any(|e| e.contains("duplicate institution id")));
    }

    #[test]
    fn unknown_institution_in_membership() {
        let nets = sample_networks();
        let insts = sample_institutions();
        let mems = vec![Membership {
            institution_id: "nonexistent".into(),
            tier: "Basic".into(),
            price_usd: 50.0,
            networks_unlocked: vec![Network::Astc],
            guests_included: 0,
        }];
        let errs = validate(&nets, &insts, &mems);
        assert!(errs.iter().any(|e| e.contains("unknown institution_id")));
    }

    #[test]
    fn empty_provenance_detected() {
        let nets = sample_networks();
        let mut insts = sample_institutions();
        insts[0].provenance = String::new();
        let errs = validate(&nets, &insts, &sample_memberships());
        assert!(errs.iter().any(|e| e.contains("empty provenance")));
    }

    #[test]
    fn bad_coordinates_detected() {
        let nets = sample_networks();
        let mut insts = sample_institutions();
        insts[0].location = LatLon::new(91.0, -122.6);
        let errs = validate(&nets, &insts, &sample_memberships());
        assert!(errs.iter().any(|e| e.contains("lat")));
    }

    #[test]
    fn unknown_network_in_participation() {
        // Use a network not in the networks list
        let nets = sample_networks(); // only ASTC
        let mut insts = sample_institutions();
        insts[0].participates.push(Participation {
            network: Network::Narm, // not in nets
            admission: None,
            exclusion: None,
            special_exhibit_restricted: false,
        });
        let errs = validate(&nets, &insts, &sample_memberships());
        assert!(errs.iter().any(|e| e.contains("unknown network")));
    }

    #[test]
    fn duplicate_membership_detected() {
        let nets = sample_networks();
        let insts = sample_institutions();
        let mut mems = sample_memberships();
        mems.push(mems[0].clone());
        let errs = validate(&nets, &insts, &mems);
        assert!(errs.iter().any(|e| e.contains("duplicate membership")));
    }
}
