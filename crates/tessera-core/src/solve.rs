//! Set-cover optimizer for reciprocal museum membership selection.
//!
//! The membership-selection problem is weighted set-cover (NP-hard): choose
//! the min-cost subset of candidate memberships whose union of coverage sets
//! contains all target institutions.
//!
//! The twist: a membership's coverage is *geographically dependent* on where
//! the user lives (because eligibility depends on residence + home institution
//! location). So coverage must be pre-computed per-candidate via the rules
//! engine before the optimizer runs.
//!
//! # Solvers
//!
//! - **Greedy**: O(n·m) coverage-per-dollar heuristic. Achieves ln(n)+1
//!   approximation ratio for min-cost, and is monotone-submodular for max-
//!   coverage (1 - 1/e guarantee).
//! - **Exact**: Branch-and-bound with greedy upper bound and dominance
//!   pruning. Practical for the expected instance sizes (~50 candidates,
//!   ~20 targets).
//!
//! # Objective modes
//!
//! - **(a) MinCost**: minimum total price to achieve full free coverage of
//!   the target set.
//! - **(b) MaxCoverage**: maximize the number of covered targets under a
//!   budget constraint.
//! - **(c) Arbitrage**: per-membership analysis — cheapest single membership
//!   unlocking each target, and marginal value of each membership in the
//!   chosen set.

use std::collections::BTreeSet;

use crate::model::*;
use crate::rules;

// ---------------------------------------------------------------------------
// Coverage representation
// ---------------------------------------------------------------------------

/// A candidate membership with its pre-computed coverage set.
#[derive(Debug, Clone)]
pub struct CandidateMembership {
    pub institution_id: String,
    pub tier: String,
    pub price_usd: f64,
    pub networks_unlocked: Vec<Network>,
    pub guests_included: u8,
    /// Indices into the target list that this membership covers with free admission.
    pub covers_free: BTreeSet<usize>,
    /// Indices into the target list that this membership covers with discounted admission.
    pub covers_discounted: BTreeSet<usize>,
}

/// Result of an optimization run.
#[derive(Debug, Clone)]
pub struct SolutionSet {
    /// Indices into the candidate list of the selected memberships.
    pub selected: Vec<usize>,
    /// Total cost of the selected memberships.
    pub total_cost: f64,
    /// Target indices covered with free admission.
    pub covered_free: BTreeSet<usize>,
    /// Target indices covered with discounted admission.
    pub covered_discounted: BTreeSet<usize>,
}

impl SolutionSet {
    pub fn empty() -> Self {
        SolutionSet {
            selected: Vec::new(),
            total_cost: 0.0,
            covered_free: BTreeSet::new(),
            covered_discounted: BTreeSet::new(),
        }
    }

    /// Number of targets with free coverage.
    pub fn free_count(&self) -> usize {
        self.covered_free.len()
    }

    /// Number of targets with only discounted (not free) coverage.
    pub fn discount_only_count(&self) -> usize {
        self.covered_discounted
            .difference(&self.covered_free)
            .count()
    }
}

// ---------------------------------------------------------------------------
// Arbitrage report
// ---------------------------------------------------------------------------

/// Per-target analysis: cheapest single membership that covers it.
#[derive(Debug, Clone)]
pub struct TargetArbitrage {
    pub target_index: usize,
    pub target_id: String,
    /// Cheapest single membership providing free admission, if any.
    pub cheapest_free: Option<(usize, f64)>, // (candidate_index, price)
    /// Cheapest single membership providing discounted admission, if any.
    pub cheapest_discounted: Option<(usize, f64)>,
}

/// Marginal value analysis for a membership in a selected set.
#[derive(Debug, Clone)]
pub struct MarginalValue {
    pub candidate_index: usize,
    pub institution_id: String,
    pub tier: String,
    pub price: f64,
    /// Targets that would lose free coverage if this membership were removed.
    pub exclusive_free: BTreeSet<usize>,
    /// Targets that would lose discounted coverage if this membership were removed.
    pub exclusive_discounted: BTreeSet<usize>,
}

/// Full arbitrage report.
#[derive(Debug, Clone)]
pub struct ArbitrageReport {
    pub per_target: Vec<TargetArbitrage>,
    pub marginal: Vec<MarginalValue>,
}

// ---------------------------------------------------------------------------
// Coverage computation
// ---------------------------------------------------------------------------

/// Pre-compute the coverage of each candidate membership against the target
/// set, using the rules engine.
///
/// For each candidate membership, we simulate the user holding *only* that
/// membership and compute which targets become eligible.
pub fn compute_candidate_coverage(
    user_residence: LatLon,
    targets: &[&Institution],
    candidates: &[&Membership],
    dataset: &Dataset,
) -> Vec<CandidateMembership> {
    let target_ids: Vec<&str> = targets.iter().map(|t| t.id.as_str()).collect();

    candidates
        .iter()
        .map(|m| {
            // Simulate user holding only this membership
            let sim_user = User {
                residence: user_residence,
                held: vec![MembershipRef {
                    institution_id: m.institution_id.clone(),
                    tier: m.tier.clone(),
                }],
            };
            let nets = rules::networks_from_memberships(&sim_user, dataset);
            let eligibility = rules::compute_eligibility(
                &sim_user,
                dataset,
                &nets,
                Some(&m.institution_id),
            );

            let mut covers_free = BTreeSet::new();
            let mut covers_discounted = BTreeSet::new();

            for elig in &eligibility {
                if let Some(idx) = target_ids
                    .iter()
                    .position(|tid| *tid == elig.institution_id)
                {
                    match elig.admission {
                        Admission::Free => {
                            covers_free.insert(idx);
                        }
                        Admission::Discount { .. } => {
                            covers_discounted.insert(idx);
                        }
                    }
                }
            }

            CandidateMembership {
                institution_id: m.institution_id.clone(),
                tier: m.tier.clone(),
                price_usd: m.price_usd,
                networks_unlocked: m.networks_unlocked.clone(),
                guests_included: m.guests_included,
                covers_free,
                covers_discounted,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Greedy solver
// ---------------------------------------------------------------------------

/// Greedy set-cover: repeatedly pick the candidate with the best
/// coverage-per-dollar ratio until all targets are covered or no progress
/// can be made.
///
/// For min-cost mode, this is an O(n·m) heuristic with ln(n)+1 approximation.
pub fn solve_greedy_min_cost(
    candidates: &[CandidateMembership],
    n_targets: usize,
) -> SolutionSet {
    let mut uncovered: BTreeSet<usize> = (0..n_targets).collect();
    let mut selected: Vec<usize> = Vec::new();
    let mut used: Vec<bool> = vec![false; candidates.len()];
    let mut total_cost = 0.0;
    let mut covered_free = BTreeSet::new();
    let mut covered_discounted = BTreeSet::new();

    while !uncovered.is_empty() {
        // Find the candidate with the best marginal-coverage-per-dollar
        let best = candidates
            .iter()
            .enumerate()
            .filter(|(i, _)| !used[*i])
            .filter_map(|(i, c)| {
                let marginal: BTreeSet<usize> =
                    c.covers_free.intersection(&uncovered).copied().collect();
                let gain = marginal.len();
                if gain == 0 {
                    return None;
                }
                // coverage-per-dollar (higher is better)
                let ratio = gain as f64 / c.price_usd;
                Some((i, ratio, marginal))
            })
            .max_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        match best {
            Some((idx, _, marginal)) => {
                used[idx] = true;
                selected.push(idx);
                total_cost += candidates[idx].price_usd;
                covered_free.extend(&marginal);
                covered_discounted.extend(&candidates[idx].covers_discounted);
                for t in &marginal {
                    uncovered.remove(t);
                }
            }
            None => break, // No more progress possible
        }
    }

    SolutionSet {
        selected,
        total_cost,
        covered_free,
        covered_discounted,
    }
}

/// Greedy max-coverage under a budget: repeatedly pick the best
/// coverage-per-dollar candidate that fits in the remaining budget.
pub fn solve_greedy_max_coverage(
    candidates: &[CandidateMembership],
    n_targets: usize,
    budget: f64,
) -> SolutionSet {
    let mut uncovered: BTreeSet<usize> = (0..n_targets).collect();
    let mut selected: Vec<usize> = Vec::new();
    let mut used: Vec<bool> = vec![false; candidates.len()];
    let mut total_cost = 0.0;
    let mut covered_free = BTreeSet::new();
    let mut covered_discounted = BTreeSet::new();

    loop {
        let remaining = budget - total_cost;
        let best = candidates
            .iter()
            .enumerate()
            .filter(|(i, c)| !used[*i] && c.price_usd <= remaining)
            .filter_map(|(i, c)| {
                let marginal: BTreeSet<usize> =
                    c.covers_free.intersection(&uncovered).copied().collect();
                let gain = marginal.len();
                if gain == 0 {
                    return None;
                }
                let ratio = gain as f64 / c.price_usd;
                Some((i, ratio, marginal))
            })
            .max_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

        match best {
            Some((idx, _, marginal)) => {
                used[idx] = true;
                selected.push(idx);
                total_cost += candidates[idx].price_usd;
                covered_free.extend(&marginal);
                covered_discounted.extend(&candidates[idx].covers_discounted);
                for t in &marginal {
                    uncovered.remove(t);
                }
            }
            None => break,
        }
    }

    SolutionSet {
        selected,
        total_cost,
        covered_free,
        covered_discounted,
    }
}

// ---------------------------------------------------------------------------
// Exact branch-and-bound solver
// ---------------------------------------------------------------------------

/// Exact min-cost set-cover via branch-and-bound.
///
/// Uses the greedy solution as the initial upper bound, prunes branches
/// where the current cost already exceeds the best known, and applies
/// dominance pruning (skip a candidate if another unselected candidate
/// is strictly cheaper and covers a superset).
///
/// If full free coverage is impossible, returns the cheapest solution that
/// covers the maximum number of free-coverable targets.
pub fn solve_exact_min_cost(
    candidates: &[CandidateMembership],
    n_targets: usize,
) -> SolutionSet {
    if candidates.is_empty() || n_targets == 0 {
        return SolutionSet::empty();
    }

    // Determine which targets are actually coverable
    let coverable: BTreeSet<usize> = (0..n_targets)
        .filter(|ti| candidates.iter().any(|c| c.covers_free.contains(ti)))
        .collect();

    if coverable.is_empty() {
        return SolutionSet::empty();
    }

    // Get greedy bound first
    let greedy = solve_greedy_min_cost(candidates, n_targets);

    // Use greedy as initial best if it covers all coverable targets
    let initial_best = if coverable.is_subset(&greedy.covered_free) {
        Some(greedy)
    } else {
        None
    };

    let mut best = initial_best;

    // Sort candidates by price for better pruning
    let mut sorted_indices: Vec<usize> = (0..candidates.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        candidates[a]
            .price_usd
            .partial_cmp(&candidates[b].price_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    bb_recurse(
        candidates,
        &sorted_indices,
        &coverable,
        0,
        &Vec::new(),
        0.0,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &mut best,
    );

    best.unwrap_or_else(SolutionSet::empty)
}

#[allow(clippy::too_many_arguments)]
fn bb_recurse(
    candidates: &[CandidateMembership],
    sorted_indices: &[usize],
    coverable: &BTreeSet<usize>,
    depth: usize,
    selected: &Vec<usize>,
    current_cost: f64,
    covered_free: &BTreeSet<usize>,
    covered_discounted: &BTreeSet<usize>,
    best: &mut Option<SolutionSet>,
) {
    // Check if we've covered all coverable targets
    if coverable.is_subset(covered_free) {
        let is_better = match best {
            Some(b) => current_cost < b.total_cost,
            None => true,
        };
        if is_better {
            *best = Some(SolutionSet {
                selected: selected.clone(),
                total_cost: current_cost,
                covered_free: covered_free.clone(),
                covered_discounted: covered_discounted.clone(),
            });
        }
        return;
    }

    // Prune: current cost already exceeds best
    if let Some(b) = best.as_ref() {
        if current_cost >= b.total_cost {
            return;
        }
    }

    // Prune: no more candidates
    if depth >= sorted_indices.len() {
        return;
    }

    // Lower bound: even if we could cover remaining targets for free,
    // we still need at least one more membership. Check if the cheapest
    // remaining candidate would push us over.
    let cheapest_remaining = sorted_indices[depth..]
        .iter()
        .map(|&i| candidates[i].price_usd)
        .fold(f64::INFINITY, f64::min);

    if let Some(b) = best.as_ref() {
        if current_cost + cheapest_remaining >= b.total_cost {
            // Even adding the cheapest remaining won't beat best
            // (only prune if we still have uncovered targets)
            let uncovered: BTreeSet<usize> =
                coverable.difference(covered_free).copied().collect();
            if !uncovered.is_empty() {
                // Check if any remaining candidate can cover anything uncovered
                let any_useful = sorted_indices[depth..].iter().any(|&i| {
                    candidates[i]
                        .covers_free
                        .intersection(&uncovered)
                        .next()
                        .is_some()
                });
                if !any_useful {
                    return;
                }
            }
        }
    }

    let ci = sorted_indices[depth];
    let cand = &candidates[ci];

    // Branch: include candidate ci
    let marginal_free: BTreeSet<usize> = cand
        .covers_free
        .difference(covered_free)
        .copied()
        .collect();

    if !marginal_free.is_empty() {
        let mut new_selected = selected.clone();
        new_selected.push(ci);
        let new_covered_free: BTreeSet<usize> =
            covered_free.union(&cand.covers_free).copied().collect();
        let new_covered_disc: BTreeSet<usize> = covered_discounted
            .union(&cand.covers_discounted)
            .copied()
            .collect();

        bb_recurse(
            candidates,
            sorted_indices,
            coverable,
            depth + 1,
            &new_selected,
            current_cost + cand.price_usd,
            &new_covered_free,
            &new_covered_disc,
            best,
        );
    }

    // Branch: exclude candidate ci
    bb_recurse(
        candidates,
        sorted_indices,
        coverable,
        depth + 1,
        selected,
        current_cost,
        covered_free,
        covered_discounted,
        best,
    );
}

/// Exact max-coverage under budget via branch-and-bound.
pub fn solve_exact_max_coverage(
    candidates: &[CandidateMembership],
    n_targets: usize,
    budget: f64,
) -> SolutionSet {
    if candidates.is_empty() || n_targets == 0 {
        return SolutionSet::empty();
    }

    let mut best = SolutionSet::empty();

    bb_max_recurse(
        candidates,
        n_targets,
        budget,
        0,
        &Vec::new(),
        0.0,
        &BTreeSet::new(),
        &BTreeSet::new(),
        &mut best,
    );

    best
}

#[allow(clippy::too_many_arguments)]
fn bb_max_recurse(
    candidates: &[CandidateMembership],
    n_targets: usize,
    budget: f64,
    depth: usize,
    selected: &Vec<usize>,
    current_cost: f64,
    covered_free: &BTreeSet<usize>,
    covered_discounted: &BTreeSet<usize>,
    best: &mut SolutionSet,
) {
    // Update best if current solution is better
    if covered_free.len() > best.free_count()
        || (covered_free.len() == best.free_count() && current_cost < best.total_cost)
    {
        *best = SolutionSet {
            selected: selected.clone(),
            total_cost: current_cost,
            covered_free: covered_free.clone(),
            covered_discounted: covered_discounted.clone(),
        };
    }

    if depth >= candidates.len() {
        return;
    }

    // Upper bound: even if all remaining candidates' coverage were free and
    // non-overlapping, how many could we add?
    let current_count = covered_free.len();
    let max_possible_remaining: usize = candidates[depth..]
        .iter()
        .filter(|c| c.price_usd <= budget - current_cost)
        .map(|c| c.covers_free.len())
        .sum();

    if current_count + max_possible_remaining <= best.free_count() {
        return; // Can't beat best
    }

    let cand = &candidates[depth];

    // Branch: include (if within budget)
    if current_cost + cand.price_usd <= budget {
        let new_covered_free: BTreeSet<usize> =
            covered_free.union(&cand.covers_free).copied().collect();
        let new_covered_disc: BTreeSet<usize> = covered_discounted
            .union(&cand.covers_discounted)
            .copied()
            .collect();
        let mut new_selected = selected.clone();
        new_selected.push(depth);

        bb_max_recurse(
            candidates,
            n_targets,
            budget,
            depth + 1,
            &new_selected,
            current_cost + cand.price_usd,
            &new_covered_free,
            &new_covered_disc,
            best,
        );
    }

    // Branch: exclude
    bb_max_recurse(
        candidates,
        n_targets,
        budget,
        depth + 1,
        selected,
        current_cost,
        covered_free,
        covered_discounted,
        best,
    );
}

// ---------------------------------------------------------------------------
// Arbitrage report
// ---------------------------------------------------------------------------

/// Generate an arbitrage report: per-target cheapest covering membership,
/// and marginal-value analysis of each membership in a solution set.
pub fn arbitrage_report(
    candidates: &[CandidateMembership],
    solution: &SolutionSet,
    targets: &[&str],
) -> ArbitrageReport {
    // Per-target: cheapest single membership covering it
    let per_target: Vec<TargetArbitrage> = (0..targets.len())
        .map(|ti| {
            let cheapest_free = candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| c.covers_free.contains(&ti))
                .min_by(|a, b| {
                    a.1.price_usd
                        .partial_cmp(&b.1.price_usd)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, c)| (i, c.price_usd));

            let cheapest_discounted = candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| c.covers_discounted.contains(&ti))
                .min_by(|a, b| {
                    a.1.price_usd
                        .partial_cmp(&b.1.price_usd)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, c)| (i, c.price_usd));

            TargetArbitrage {
                target_index: ti,
                target_id: targets[ti].to_string(),
                cheapest_free,
                cheapest_discounted,
            }
        })
        .collect();

    // Marginal value: for each selected membership, which targets would lose
    // coverage if it were removed?
    let marginal: Vec<MarginalValue> = solution
        .selected
        .iter()
        .map(|&si| {
            let cand = &candidates[si];
            let other_selected: Vec<usize> = solution
                .selected
                .iter()
                .copied()
                .filter(|&s| s != si)
                .collect();

            // Coverage of all OTHER selected memberships
            let other_free: BTreeSet<usize> = other_selected
                .iter()
                .flat_map(|&s| candidates[s].covers_free.iter().copied())
                .collect();
            let other_disc: BTreeSet<usize> = other_selected
                .iter()
                .flat_map(|&s| candidates[s].covers_discounted.iter().copied())
                .collect();

            // Targets exclusively covered by this membership
            let exclusive_free: BTreeSet<usize> = cand
                .covers_free
                .difference(&other_free)
                .copied()
                .filter(|t| solution.covered_free.contains(t))
                .collect();
            let exclusive_discounted: BTreeSet<usize> = cand
                .covers_discounted
                .difference(&other_disc)
                .copied()
                .filter(|t| solution.covered_discounted.contains(t))
                .collect();

            MarginalValue {
                candidate_index: si,
                institution_id: cand.institution_id.clone(),
                tier: cand.tier.clone(),
                price: cand.price_usd,
                exclusive_free,
                exclusive_discounted,
            }
        })
        .collect();

    ArbitrageReport {
        per_target,
        marginal,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a simple candidate
    fn cand(
        id: &str,
        tier: &str,
        price: f64,
        free: &[usize],
        disc: &[usize],
    ) -> CandidateMembership {
        CandidateMembership {
            institution_id: id.into(),
            tier: tier.into(),
            price_usd: price,
            networks_unlocked: vec![],
            guests_included: 0,
            covers_free: free.iter().copied().collect(),
            covers_discounted: disc.iter().copied().collect(),
        }
    }

    // -----------------------------------------------------------------------
    // Known-optimal constructed instances
    // -----------------------------------------------------------------------

    /// Trivial: one candidate covers everything.
    #[test]
    fn single_candidate_covers_all() {
        let candidates = vec![cand("a", "t", 100.0, &[0, 1, 2], &[])];
        let exact = solve_exact_min_cost(&candidates, 3);
        let greedy = solve_greedy_min_cost(&candidates, 3);

        assert_eq!(exact.selected, vec![0]);
        assert_eq!(exact.total_cost, 100.0);
        assert_eq!(exact.free_count(), 3);
        assert_eq!(greedy.selected, vec![0]);
    }

    /// Two non-overlapping candidates needed to cover all targets.
    #[test]
    fn two_disjoint_candidates() {
        let candidates = vec![
            cand("a", "t", 50.0, &[0, 1], &[]),
            cand("b", "t", 80.0, &[2, 3], &[]),
        ];
        let exact = solve_exact_min_cost(&candidates, 4);
        assert_eq!(exact.total_cost, 130.0);
        assert_eq!(exact.free_count(), 4);

        let greedy = solve_greedy_min_cost(&candidates, 4);
        assert_eq!(greedy.free_count(), 4);
    }

    /// Greedy sub-optimality: constructed example where greedy is not optimal.
    /// Targets: 0..5 (6 targets).
    /// C0: covers {0,1,2,3,4,5}, price 65
    /// C1: covers {0,1,2}, price 40
    /// C2: covers {3,4,5}, price 40
    /// C3: covers {0,3}, price 10 (best ratio initially: 2/10)
    ///
    /// Greedy picks C3 (2/10), then C1 (2/40), then C2 (2/40) = $90.
    /// Exact picks C0 = $65.
    #[test]
    fn greedy_vs_exact_suboptimality() {
        let candidates = vec![
            cand("big", "t", 65.0, &[0, 1, 2, 3, 4, 5], &[]),
            cand("left", "t", 40.0, &[0, 1, 2], &[]),
            cand("right", "t", 40.0, &[3, 4, 5], &[]),
            cand("cross", "t", 10.0, &[0, 3], &[]),
        ];
        let n = 6;

        let exact = solve_exact_min_cost(&candidates, n);
        assert_eq!(exact.total_cost, 65.0, "exact should find $65 solution");
        assert_eq!(exact.free_count(), n);

        let greedy = solve_greedy_min_cost(&candidates, n);
        assert_eq!(greedy.free_count(), n);
        // Greedy picks C3+C1+C2 = $90, which is suboptimal
        assert!(
            greedy.total_cost >= exact.total_cost,
            "greedy should not beat exact"
        );
        // But greedy should be within ln(n)+1 of optimal
        let ln_n_bound = (n as f64).ln() + 1.0;
        assert!(
            greedy.total_cost <= exact.total_cost * ln_n_bound,
            "greedy ${} should be ≤ ln({})+1 × exact ${} = ${:.0}",
            greedy.total_cost,
            n,
            exact.total_cost,
            exact.total_cost * ln_n_bound
        );
    }

    /// Overlapping candidates: exact should find the cheaper combo.
    #[test]
    fn overlapping_candidates_exact() {
        // Targets: 0, 1, 2
        // C0: {0,1} $60
        // C1: {1,2} $60
        // C2: {0,1,2} $150
        // Optimal: C0+C1 = $120, not C2 = $150
        let candidates = vec![
            cand("a", "t", 60.0, &[0, 1], &[]),
            cand("b", "t", 60.0, &[1, 2], &[]),
            cand("c", "t", 150.0, &[0, 1, 2], &[]),
        ];
        let exact = solve_exact_min_cost(&candidates, 3);
        assert_eq!(exact.total_cost, 120.0);
        assert_eq!(exact.free_count(), 3);
    }

    /// Empty inputs.
    #[test]
    fn empty_candidates() {
        let exact = solve_exact_min_cost(&[], 5);
        assert!(exact.selected.is_empty());
        assert_eq!(exact.free_count(), 0);
    }

    #[test]
    fn zero_targets() {
        let candidates = vec![cand("a", "t", 100.0, &[], &[])];
        let exact = solve_exact_min_cost(&candidates, 0);
        assert!(exact.selected.is_empty());
        assert_eq!(exact.total_cost, 0.0);
    }

    /// Impossible: no candidate covers target 2.
    #[test]
    fn impossible_coverage() {
        let candidates = vec![
            cand("a", "t", 50.0, &[0], &[]),
            cand("b", "t", 50.0, &[1], &[]),
        ];
        let exact = solve_exact_min_cost(&candidates, 3);
        // Can't cover target 2 — should return best partial
        assert!(exact.free_count() < 3);
    }

    // -----------------------------------------------------------------------
    // Max-coverage under budget
    // -----------------------------------------------------------------------

    #[test]
    fn max_coverage_budget() {
        // Budget $100. C0=$60 covers {0,1}, C1=$60 covers {2,3}.
        // Can only afford one.
        let candidates = vec![
            cand("a", "t", 60.0, &[0, 1], &[]),
            cand("b", "t", 60.0, &[2, 3], &[]),
        ];
        let exact = solve_exact_max_coverage(&candidates, 4, 100.0);
        assert_eq!(exact.free_count(), 2);
        assert!(exact.total_cost <= 100.0);

        // Budget $200: can afford both.
        let exact2 = solve_exact_max_coverage(&candidates, 4, 200.0);
        assert_eq!(exact2.free_count(), 4);
    }

    #[test]
    fn max_coverage_picks_better_value() {
        // Budget $100.
        // C0=$90 covers {0}, C1=$80 covers {0,1,2}
        // Should pick C1.
        let candidates = vec![
            cand("a", "t", 90.0, &[0], &[]),
            cand("b", "t", 80.0, &[0, 1, 2], &[]),
        ];
        let exact = solve_exact_max_coverage(&candidates, 3, 100.0);
        assert_eq!(exact.free_count(), 3);
        assert_eq!(exact.total_cost, 80.0);
    }

    // -----------------------------------------------------------------------
    // Arbitrage report
    // -----------------------------------------------------------------------

    #[test]
    fn arbitrage_cheapest_per_target() {
        let candidates = vec![
            cand("a", "t", 50.0, &[0, 1], &[]),
            cand("b", "t", 80.0, &[1, 2], &[]),
            cand("c", "t", 120.0, &[0, 1, 2], &[]),
        ];
        let solution = solve_exact_min_cost(&candidates, 3);
        let targets = vec!["t0", "t1", "t2"];
        let report = arbitrage_report(&candidates, &solution, &targets);

        // Target 0: cheapest free is C0 ($50)
        assert_eq!(report.per_target[0].cheapest_free.unwrap().1, 50.0);
        // Target 1: cheapest free is C0 ($50)
        assert_eq!(report.per_target[1].cheapest_free.unwrap().1, 50.0);
        // Target 2: cheapest free is C1 ($80)
        assert_eq!(report.per_target[2].cheapest_free.unwrap().1, 80.0);
    }

    #[test]
    fn arbitrage_marginal_value() {
        let candidates = vec![
            cand("a", "t", 50.0, &[0, 1], &[]),
            cand("b", "t", 80.0, &[1, 2], &[]),
        ];
        // Solution selects both
        let solution = SolutionSet {
            selected: vec![0, 1],
            total_cost: 130.0,
            covered_free: [0, 1, 2].iter().copied().collect(),
            covered_discounted: BTreeSet::new(),
        };
        let targets = vec!["t0", "t1", "t2"];
        let report = arbitrage_report(&candidates, &solution, &targets);

        // C0 exclusively covers target 0 (target 1 also covered by C1)
        let m0 = &report.marginal[0];
        assert_eq!(m0.candidate_index, 0);
        assert!(m0.exclusive_free.contains(&0), "C0 should exclusively cover target 0");
        assert!(!m0.exclusive_free.contains(&1), "Target 1 is also covered by C1");

        // C1 exclusively covers target 2
        let m1 = &report.marginal[1];
        assert_eq!(m1.candidate_index, 1);
        assert!(m1.exclusive_free.contains(&2), "C1 should exclusively cover target 2");
    }

    /// Discount-only coverage: candidate covers a target with discount but
    /// not free.
    #[test]
    fn discount_only_coverage_tracked() {
        let candidates = vec![
            cand("a", "t", 100.0, &[0], &[1]),  // free: {0}, disc: {1}
        ];
        let sol = solve_greedy_min_cost(&candidates, 2);
        assert_eq!(sol.free_count(), 1);
        assert_eq!(sol.discount_only_count(), 1);
    }

    // -----------------------------------------------------------------------
    // Greedy approximation bound
    // -----------------------------------------------------------------------

    /// Verify greedy ≤ ln(n)·optimal on a larger constructed instance.
    #[test]
    fn greedy_approximation_bound() {
        // 10 targets, 5 candidates with overlapping coverage
        let candidates = vec![
            cand("a", "t", 30.0, &[0, 1, 2], &[]),
            cand("b", "t", 25.0, &[2, 3, 4], &[]),
            cand("c", "t", 35.0, &[4, 5, 6, 7], &[]),
            cand("d", "t", 20.0, &[6, 7, 8, 9], &[]),
            cand("e", "t", 90.0, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9], &[]),
        ];
        let n = 10;

        let exact = solve_exact_min_cost(&candidates, n);
        let greedy = solve_greedy_min_cost(&candidates, n);

        assert_eq!(exact.free_count(), n);
        assert_eq!(greedy.free_count(), n);

        let bound = (n as f64).ln() + 1.0;
        assert!(
            greedy.total_cost <= exact.total_cost * bound,
            "greedy ${} > ln({})+1 × exact ${} = ${:.0}",
            greedy.total_cost,
            n,
            exact.total_cost,
            exact.total_cost * bound
        );
    }

    // -----------------------------------------------------------------------
    // Integration: coverage from rules engine
    // -----------------------------------------------------------------------

    #[test]
    fn coverage_computation_with_fixture() {
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

        let aloha = LatLon::new(45.4912, -122.8720);

        // Targets: a few institutions the user wants to visit
        let target_ids = ["pacific-science-center", "seattle-art-museum", "tacoma-art-museum"];
        let targets: Vec<&Institution> = target_ids
            .iter()
            .map(|id| dataset.institution(id).unwrap())
            .collect();

        // Candidates: all memberships that unlock at least one network
        let all_memberships: Vec<&Membership> = dataset.memberships.iter().collect();

        let coverage = compute_candidate_coverage(aloha, &targets, &all_memberships, &dataset);

        // OMSI Family Plus should cover Pacific Science Center (ASTC, free)
        // and Seattle Art Museum + Tacoma Art Museum (NARM, free)
        let omsi_fp = coverage
            .iter()
            .find(|c| c.institution_id == "omsi" && c.tier == "Family Plus")
            .expect("OMSI Family Plus should be in candidates");

        assert!(
            omsi_fp.covers_free.len() == 3,
            "OMSI Family Plus should cover all 3 targets, covers {:?}",
            omsi_fp.covers_free
        );

        // Run optimizer
        let exact = solve_exact_min_cost(&coverage, targets.len());
        assert_eq!(exact.free_count(), 3);
        // OMSI Family Plus at $185 should be cheaper than alternatives
        assert!(
            exact.total_cost <= 185.0,
            "Optimal cost should be ≤ $185 (OMSI FP), got ${}",
            exact.total_cost
        );
    }
}
