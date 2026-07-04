use anyhow::{Context, Result};
use clap::Args;
use std::collections::BTreeMap;
use std::path::Path;
use tessera_core::model::*;

// ---------------------------------------------------------------------------
// add-institution
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct AddInstitutionArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    city: String,
    #[arg(long)]
    region: String,
    #[arg(long)]
    lat: f64,
    #[arg(long)]
    lon: f64,
    #[arg(long)]
    provenance: String,
    /// Network participation, repeatable. Format: `astc` or `narm:excl=15` or `aza:discount=0.5`
    #[arg(long = "network", num_args = 1)]
    networks: Vec<String>,
}

fn parse_network_enum(s: &str) -> Result<Network> {
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .with_context(|| format!("unknown network '{s}'"))
}

fn parse_participation(spec: &str) -> Result<Participation> {
    // Format: network or network:key=val:key=val
    let parts: Vec<&str> = spec.split(':').collect();
    let network = parse_network_enum(parts[0])?;
    let mut admission = None;
    let mut exclusion = None;

    for &kv in &parts[1..] {
        let (k, v) = kv.split_once('=')
            .with_context(|| format!("bad key=value in '{spec}'"))?;
        match k {
            "excl" => {
                let mi: f64 = v.parse()?;
                exclusion = Some(ExclusionRule {
                    residence_radius_mi: None,
                    home_institution_radius_mi: Some(mi),
                    both_must_clear: false,
                });
            }
            "discount" => {
                let frac: f32 = v.parse()?;
                admission = Some(Admission::Discount { fraction: frac });
            }
            _ => anyhow::bail!("unknown participation key '{k}' in '{spec}'"),
        }
    }

    Ok(Participation {
        network,
        admission,
        exclusion,
        special_exhibit_restricted: false,
    })
}

pub fn add_institution(args: AddInstitutionArgs) -> Result<()> {
    let path = Path::new("data/institutions.json");
    let mut institutions: Vec<Institution> = load_json(path)?;

    let participates: Vec<Participation> = args.networks.iter()
        .map(|s| parse_participation(s))
        .collect::<Result<_>>()?;

    let inst = Institution {
        id: args.id.clone(),
        name: args.name,
        city: args.city,
        region: args.region,
        country: "US".into(),
        location: LatLon::new(args.lat, args.lon),
        website: None,
        participates,
        provenance: args.provenance,
    };

    // Idempotent: update in place or append
    if let Some(existing) = institutions.iter_mut().find(|i| i.id == args.id) {
        *existing = inst;
    } else {
        institutions.push(inst);
    }

    // Sort by id
    institutions.sort_by(|a, b| a.id.cmp(&b.id));
    write_json(path, &institutions)?;
    println!("✓ Institution '{}' written to {}", args.id, path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// add-membership
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct AddMembershipArgs {
    #[arg(long)]
    institution: String,
    #[arg(long)]
    tier: String,
    #[arg(long)]
    price: f64,
    /// Comma-separated networks
    #[arg(long)]
    networks: String,
    #[arg(long)]
    guests: Option<u8>,
}

pub fn add_membership(args: AddMembershipArgs) -> Result<()> {
    let path = Path::new("data/memberships.json");
    let mut memberships: Vec<Membership> = load_json(path)?;

    let networks_unlocked: Vec<Network> = args.networks.split(',')
        .map(|s| parse_network_enum(s.trim()))
        .collect::<Result<_>>()?;

    let mem = Membership {
        institution_id: args.institution.clone(),
        tier: args.tier.clone(),
        price_usd: args.price,
        networks_unlocked,
        guests_included: args.guests.unwrap_or(0),
    };

    // Idempotent: update in place or append
    if let Some(existing) = memberships.iter_mut()
        .find(|m| m.institution_id == args.institution && m.tier == args.tier)
    {
        *existing = mem;
    } else {
        memberships.push(mem);
    }

    // Sort by (institution_id, tier)
    memberships.sort_by(|a, b| {
        a.institution_id.cmp(&b.institution_id).then(a.tier.cmp(&b.tier))
    });
    write_json(path, &memberships)?;
    println!("✓ Membership '{}/{}' written to {}", args.institution, args.tier, path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// add-zips
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct AddZipsArgs {
    /// CSV file with columns: zip,lat,lon
    #[arg(long)]
    file: String,
    /// Region tag (target file: zip-centroids-{region}.json)
    #[arg(long)]
    region: String,
}

pub fn add_zips(args: AddZipsArgs) -> Result<()> {
    let out_path = Path::new("data").join(format!("zip-centroids-{}.json", args.region));

    // Load existing or start fresh
    let mut map: BTreeMap<String, [f64; 2]> = if out_path.exists() {
        let text = std::fs::read_to_string(&out_path)?;
        serde_json::from_str(&text)?
    } else {
        BTreeMap::new()
    };

    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_path(&args.file)
        .with_context(|| format!("opening CSV {}", args.file))?;

    let mut count = 0usize;
    for result in rdr.records() {
        let record = result?;
        if record.len() < 3 {
            anyhow::bail!("CSV row too short: {record:?}");
        }
        let zip = record[0].trim().to_string();
        let lat: f64 = record[1].trim().parse()
            .with_context(|| format!("bad lat in row: {record:?}"))?;
        let lon: f64 = record[2].trim().parse()
            .with_context(|| format!("bad lon in row: {record:?}"))?;
        map.insert(zip, [lat, lon]);
        count += 1;
    }

    write_json(&out_path, &map)?;
    println!("✓ Merged {count} ZIP centroids into {} (total: {})", out_path.display(), map.len());
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))
}

fn write_json<T: serde::Serialize>(path: &Path, data: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(data)? + "\n";
    std::fs::write(path, json)
        .with_context(|| format!("writing {}", path.display()))
}
