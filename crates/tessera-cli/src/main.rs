//! Tessera CLI — reciprocal museum coverage optimizer.
//!
//! Usage:
//!   tessera --zip 97007 --targets omsi,pacific-science-center --budget 300
//!   tessera --lat 45.49 --lon -122.87 --targets all
//!   tessera --zip 97007 --list   # list available institutions

use clap::Parser;
use tessera_core::model::*;
use tessera_core::solve;
use tessera_core::zip::ZipCentroids;

// ---------------------------------------------------------------------------
// Embedded data
// ---------------------------------------------------------------------------

const NETWORKS_JSON: &str = include_str!("../../../data/networks.json");
const INSTITUTIONS_JSON: &str = include_str!("../../../data/institutions.json");
const MEMBERSHIPS_JSON: &str = include_str!("../../../data/memberships.json");
const ZIP_CENTROIDS_JSON: &str = include_str!("../../../data/zip-centroids-pnw.json");

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

/// Tessera — Reciprocal Museum Coverage Optimizer
///
/// Given where you live and the institutions you want access to, finds the
/// cheapest combination of museum memberships that covers them.
#[derive(Parser, Debug)]
#[command(name = "tessera", version, about, allow_negative_numbers = true)]
struct Args {
    /// ZIP code for residence location (looked up in bundled centroid table)
    #[arg(long)]
    zip: Option<String>,

    /// Latitude of residence (alternative to --zip)
    #[arg(long, requires = "lon")]
    lat: Option<f64>,

    /// Longitude of residence (alternative to --zip)
    #[arg(long, requires = "lat")]
    lon: Option<f64>,

    /// Comma-separated list of target institution IDs, or "all"
    #[arg(long, value_delimiter = ',')]
    targets: Vec<String>,

    /// Maximum budget in USD (enables max-coverage mode)
    #[arg(long)]
    budget: Option<f64>,

    /// List all available institutions and exit
    #[arg(long)]
    list: bool,

    /// Show verbose output including candidate analysis
    #[arg(long, short)]
    verbose: bool,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args = Args::parse();
    let dataset = load_dataset();
    let zips = ZipCentroids::from_json(ZIP_CENTROIDS_JSON).expect("ZIP centroid parse error");

    // --list mode
    if args.list {
        println!("Available institutions ({}):", dataset.institutions.len());
        println!();
        println!(
            "{:<35} {:<20} {:<8} {}",
            "ID", "City", "Region", "Networks"
        );
        println!("{}", "-".repeat(80));
        for inst in &dataset.institutions {
            let nets: Vec<String> = inst.participates.iter().map(|p| p.network.to_string()).collect();
            println!(
                "{:<35} {:<20} {:<8} {}",
                inst.id, inst.city, inst.region, nets.join(", ")
            );
        }
        println!();
        println!("Available memberships:");
        println!(
            "{:<35} {:<20} {:>8} {}",
            "Institution", "Tier", "Price", "Networks"
        );
        println!("{}", "-".repeat(80));
        for m in &dataset.memberships {
            let nets: Vec<String> = m.networks_unlocked.iter().map(|n| n.to_string()).collect();
            println!(
                "{:<35} {:<20} ${:>6.0} {}",
                m.institution_id, m.tier, m.price_usd, nets.join(", ")
            );
        }
        return;
    }

    // Resolve residence
    let residence = resolve_residence(&args, &zips);

    // Resolve targets
    if args.targets.is_empty() {
        eprintln!("Error: --targets is required. Use --targets all or --targets id1,id2,...");
        eprintln!("       Use --list to see available institution IDs.");
        std::process::exit(1);
    }

    let targets: Vec<&Institution> = if args.targets.len() == 1 && args.targets[0] == "all" {
        dataset.institutions.iter().collect()
    } else {
        args.targets
            .iter()
            .map(|id| {
                dataset.institution(id).unwrap_or_else(|| {
                    eprintln!("Error: unknown institution '{id}'. Use --list to see available IDs.");
                    std::process::exit(1);
                })
            })
            .collect()
    };

    let all_memberships: Vec<&Membership> = dataset.memberships.iter().collect();

    // Header
    println!("=== Tessera — Reciprocal Museum Coverage Optimizer ===");
    println!();
    if let Some(ref zip) = args.zip {
        println!("Residence: ZIP {} ({:.4}, {:.4})", zip, residence.lat, residence.lon);
    } else {
        println!("Residence: ({:.4}, {:.4})", residence.lat, residence.lon);
    }
    println!("Targets ({}):", targets.len());
    for t in &targets {
        println!("  • {} ({}, {})", t.name, t.city, t.region);
    }
    println!();

    // Compute coverage
    let candidates =
        solve::compute_candidate_coverage(residence, &targets, &all_memberships, &dataset);

    // Filter to useful candidates
    if args.verbose {
        let useful: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.covers_free.is_empty() || !c.covers_discounted.is_empty())
            .map(|(i, _)| i)
            .collect();

        println!(
            "Candidate memberships with coverage ({}/{}):",
            useful.len(),
            candidates.len()
        );
        for &i in &useful {
            let c = &candidates[i];
            let inst = dataset.institution(&c.institution_id).unwrap();
            let free_names: Vec<&str> = c
                .covers_free
                .iter()
                .map(|&ti| targets[ti].name.as_str())
                .collect();
            let disc_names: Vec<&str> = c
                .covers_discounted
                .iter()
                .filter(|ti| !c.covers_free.contains(ti))
                .map(|&ti| targets[ti].name.as_str())
                .collect();
            println!("  {} — {} (${:.0})", inst.name, c.tier, c.price_usd);
            if !free_names.is_empty() {
                println!("    free: {}", free_names.join(", "));
            }
            if !disc_names.is_empty() {
                println!("    disc: {}", disc_names.join(", "));
            }
        }
        println!();
    }

    // Check unreachable targets
    let unreachable_free: Vec<usize> = (0..targets.len())
        .filter(|ti| !candidates.iter().any(|c| c.covers_free.contains(ti)))
        .collect();

    if !unreachable_free.is_empty() {
        println!("⚠  No free reciprocal path to:");
        for &ti in &unreachable_free {
            let has_disc = candidates.iter().any(|c| c.covers_discounted.contains(&ti));
            let suffix = if has_disc {
                " (discounted access available)"
            } else {
                " (no reciprocal access found)"
            };
            println!("   • {}{}", targets[ti].name, suffix);
        }
        println!();
    }

    // --- Solve ---
    let exact = solve::solve_exact_min_cost(&candidates, targets.len());
    let greedy = solve::solve_greedy_min_cost(&candidates, targets.len());

    println!("=== Optimal memberships (exact) ===");
    print_solution(&exact, &candidates, &targets, &dataset);

    if args.verbose {
        println!();
        println!("=== Greedy solution ===");
        print_solution(&greedy, &candidates, &targets, &dataset);

        if exact.free_count() > 0 && greedy.free_count() == exact.free_count() {
            let ratio = greedy.total_cost / exact.total_cost;
            let bound = (targets.len() as f64).ln() + 1.0;
            println!(
                "Greedy/Exact ratio: {:.2} (ln(n)+1 bound: {:.2})",
                ratio, bound
            );
        }
    }

    // --- Budget mode ---
    if let Some(budget) = args.budget {
        println!();
        println!("=== Max coverage under ${budget:.0} budget ===");
        let max_cov = solve::solve_exact_max_coverage(&candidates, targets.len(), budget);
        print_solution(&max_cov, &candidates, &targets, &dataset);
    }

    // --- Arbitrage ---
    println!();
    println!("=== Arbitrage ===");
    let target_ids: Vec<&str> = targets.iter().map(|t| t.id.as_str()).collect();
    let report = solve::arbitrage_report(&candidates, &exact, &target_ids);

    println!("Per-target cheapest single membership:");
    for ta in &report.per_target {
        let name = targets[ta.target_index].name.as_str();
        let free_str = match ta.cheapest_free {
            Some((ci, price)) => {
                let c = &candidates[ci];
                format!(
                    "{} — {} (${:.0})",
                    dataset.institution(&c.institution_id).unwrap().name,
                    c.tier,
                    price
                )
            }
            None => "—".into(),
        };
        println!("  {:<35} {}", name, free_str);
    }

    if !report.marginal.is_empty() {
        println!();
        println!("Marginal contribution:");
        for mv in &report.marginal {
            let inst = dataset.institution(&mv.institution_id).unwrap();
            let excl: Vec<&str> = mv
                .exclusive_free
                .iter()
                .map(|&ti| targets[ti].name.as_str())
                .collect();
            if excl.is_empty() {
                println!(
                    "  {} — {} (${:.0}): redundant for free coverage",
                    inst.name, mv.tier, mv.price
                );
            } else {
                println!(
                    "  {} — {} (${:.0}): exclusively covers {}",
                    inst.name,
                    mv.tier,
                    mv.price,
                    excl.join(", ")
                );
            }
        }
    }

    // Disclaimer
    println!();
    println!("Verify with each institution before visiting. Data is best-effort.");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn load_dataset() -> Dataset {
    let networks: Vec<NetworkSpec> =
        serde_json::from_str(NETWORKS_JSON).expect("networks.json parse error");
    let institutions: Vec<Institution> =
        serde_json::from_str(INSTITUTIONS_JSON).expect("institutions.json parse error");
    let memberships: Vec<Membership> =
        serde_json::from_str(MEMBERSHIPS_JSON).expect("memberships.json parse error");
    Dataset {
        networks,
        institutions,
        memberships,
    }
}

fn resolve_residence(args: &Args, zips: &ZipCentroids) -> LatLon {
    if let Some(ref zip) = args.zip {
        match zips.lookup(zip) {
            Some(loc) => loc,
            None => {
                eprintln!("Error: ZIP code '{zip}' not found in centroid table.");
                eprintln!("       Use --lat/--lon instead, or try a nearby ZIP.");
                std::process::exit(1);
            }
        }
    } else if let (Some(lat), Some(lon)) = (args.lat, args.lon) {
        LatLon::new(lat, lon)
    } else {
        eprintln!("Error: provide --zip or --lat/--lon for residence location.");
        std::process::exit(1);
    }
}

fn print_solution(
    sol: &solve::SolutionSet,
    candidates: &[solve::CandidateMembership],
    targets: &[&Institution],
    dataset: &Dataset,
) {
    if sol.selected.is_empty() {
        println!("  No solution found.");
        return;
    }

    println!(
        "  Total: ${:.0} | Free: {}/{} | Discount-only: {}",
        sol.total_cost,
        sol.free_count(),
        targets.len(),
        sol.discount_only_count(),
    );
    println!("  Memberships:");
    for &si in &sol.selected {
        let c = &candidates[si];
        let inst = dataset.institution(&c.institution_id).unwrap();
        let covered: Vec<&str> = c
            .covers_free
            .iter()
            .filter(|ti| sol.covered_free.contains(ti))
            .map(|&ti| targets[ti].name.as_str())
            .collect();
        println!(
            "    {} — {} (${:.0})",
            inst.name, c.tier, c.price_usd,
        );
        if !covered.is_empty() {
            println!("      → {}", covered.join(", "));
        }
    }
    let uncovered: Vec<&str> = (0..targets.len())
        .filter(|ti| !sol.covered_free.contains(ti))
        .map(|ti| targets[ti].name.as_str())
        .collect();
    if !uncovered.is_empty() {
        println!("  Uncovered: {}", uncovered.join(", "));
    }
}
