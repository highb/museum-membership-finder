//! Tessera CLI — reciprocal museum coverage optimizer.
//!
//! Cycle 2: demonstrates optimizer output for a hardcoded scenario.
//! Full CLI arg parsing in Cycle 3.

use tessera_core::model::*;

use tessera_core::solve;

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

    // Scenario: Aloha, OR resident wants to visit these institutions
    let aloha = LatLon::new(45.4912, -122.8720);
    let target_ids = [
        "pacific-science-center",
        "seattle-art-museum",
        "tacoma-art-museum",
        "maryhill-museum",
        "childrens-museum-of-tacoma",
    ];

    let targets: Vec<&Institution> = target_ids
        .iter()
        .map(|id| dataset.institution(id).expect(id))
        .collect();

    let all_memberships: Vec<&Membership> = dataset.memberships.iter().collect();

    println!("=== Tessera — Reciprocal Museum Coverage Optimizer ===");
    println!();
    println!("Residence: Aloha, OR (45.4912, -122.8720)");
    println!("Targets ({}): ", targets.len());
    for t in &targets {
        println!("  • {} ({}, {})", t.name, t.city, t.region);
    }
    println!();

    // Compute coverage
    let candidates = solve::compute_candidate_coverage(aloha, &targets, &all_memberships, &dataset);

    // Filter to candidates that cover at least one target
    let useful: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.covers_free.is_empty() || !c.covers_discounted.is_empty())
        .map(|(i, _)| i)
        .collect();

    println!("Candidate memberships with coverage ({}/{}):", useful.len(), candidates.len());
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
        println!(
            "  [{:>2}] {} — {} (${:.0})",
            i, inst.name, c.tier, c.price_usd
        );
        if !free_names.is_empty() {
            println!("       free: {}", free_names.join(", "));
        }
        if !disc_names.is_empty() {
            println!("       disc: {}", disc_names.join(", "));
        }
    }
    println!();

    // --- (a) Min-cost full coverage ---
    println!("=== (a) Min-cost full free coverage ===");

    // Check which targets are reachable with free admission at all
    let unreachable_free: Vec<usize> = (0..targets.len())
        .filter(|ti| !candidates.iter().any(|c| c.covers_free.contains(ti)))
        .collect();

    if !unreachable_free.is_empty() {
        println!("  ⚠ Full free coverage is impossible. These targets have no free path:");
        for &ti in &unreachable_free {
            // Check if discounted access exists
            let has_disc = candidates.iter().any(|c| c.covers_discounted.contains(&ti));
            let suffix = if has_disc { " (discounted access available)" } else { " (no reciprocal access)" };
            println!("    • {}{}", targets[ti].name, suffix);
        }
        println!();

        // Solve for reachable subset only
        let reachable: Vec<usize> = (0..targets.len())
            .filter(|ti| candidates.iter().any(|c| c.covers_free.contains(ti)))
            .collect();
        println!("  Solving for the {} reachable targets:", reachable.len());
    }

    let exact = solve::solve_exact_min_cost(&candidates, targets.len());
    let greedy = solve::solve_greedy_min_cost(&candidates, targets.len());

    print_solution("Exact (B&B)", &exact, &candidates, &targets, &dataset);
    print_solution("Greedy", &greedy, &candidates, &targets, &dataset);

    if exact.free_count() > 0 && exact.free_count() == greedy.free_count() {
        let ratio = greedy.total_cost / exact.total_cost;
        let bound = (targets.len() as f64).ln() + 1.0;
        println!(
            "  Greedy/Exact ratio: {:.2} (ln(n)+1 bound: {:.2})",
            ratio, bound
        );
    }
    println!();

    // --- (b) Max coverage under budget ---
    let budget = 200.0;
    println!("=== (b) Max coverage under ${budget:.0} budget ===");
    let max_cov = solve::solve_exact_max_coverage(&candidates, targets.len(), budget);
    print_solution(&format!("Max coverage (≤${budget:.0})"), &max_cov, &candidates, &targets, &dataset);
    println!();

    // --- (c) Arbitrage report ---
    println!("=== (c) Arbitrage report ===");
    let target_id_strs: Vec<&str> = target_ids.to_vec();
    let report = solve::arbitrage_report(&candidates, &exact, &target_id_strs);

    println!("Per-target cheapest single membership:");
    for ta in &report.per_target {
        let target_name = targets[ta.target_index].name.as_str();
        let free_str = match ta.cheapest_free {
            Some((ci, price)) => format!(
                "{} — {} (${:.0})",
                dataset.institution(&candidates[ci].institution_id).unwrap().name,
                candidates[ci].tier,
                price
            ),
            None => "—".into(),
        };
        let disc_str = match ta.cheapest_discounted {
            Some((ci, price)) => format!(
                "{} — {} (${:.0})",
                dataset.institution(&candidates[ci].institution_id).unwrap().name,
                candidates[ci].tier,
                price
            ),
            None => "—".into(),
        };
        println!("  {target_name}");
        println!("    free: {free_str}");
        if ta.cheapest_discounted.is_some() {
            println!("    disc: {disc_str}");
        }
    }

    println!();
    println!("Marginal contribution of each selected membership:");
    for mv in &report.marginal {
        let inst = dataset.institution(&mv.institution_id).unwrap();
        let excl_free: Vec<&str> = mv
            .exclusive_free
            .iter()
            .map(|&ti| targets[ti].name.as_str())
            .collect();
        println!(
            "  {} — {} (${:.0})",
            inst.name, mv.tier, mv.price
        );
        if excl_free.is_empty() {
            println!("    → no exclusive free coverage (redundant for free targets)");
        } else {
            println!("    → exclusively covers: {}", excl_free.join(", "));
        }
    }
}

fn print_solution(
    label: &str,
    sol: &solve::SolutionSet,
    candidates: &[solve::CandidateMembership],
    targets: &[&Institution],
    dataset: &Dataset,
) {
    println!("  {label}:");
    println!(
        "    Total cost: ${:.0} | Free coverage: {}/{} | Discount-only: {}",
        sol.total_cost,
        sol.free_count(),
        targets.len(),
        sol.discount_only_count(),
    );
    println!("    Selected memberships:");
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
            "      {} — {} (${:.0}) → {}",
            inst.name,
            c.tier,
            c.price_usd,
            covered.join(", ")
        );
    }
    let uncovered: Vec<&str> = (0..targets.len())
        .filter(|ti| !sol.covered_free.contains(ti))
        .map(|ti| targets[ti].name.as_str())
        .collect();
    if !uncovered.is_empty() {
        println!("    Uncovered: {}", uncovered.join(", "));
    }
}
