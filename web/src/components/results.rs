use leptos::prelude::*;

use crate::components::app::SolverOutput;
use tessera_core::solve;

#[component]
pub fn ResultsPanel(output: SolverOutput) -> impl IntoView {
    let SolverOutput {
        candidates,
        exact,
        budget_solution,
        target_names,
        target_ids,
        dataset,
        zip_display: _,
    } = output;

    let n_targets = target_names.len();

    // Unreachable targets (no free path at all)
    let unreachable: Vec<usize> = (0..n_targets)
        .filter(|ti| !candidates.iter().any(|c| c.covers_free.contains(ti)))
        .collect();

    // Discount-only targets
    let discount_only: Vec<usize> = unreachable
        .iter()
        .copied()
        .filter(|ti| candidates.iter().any(|c| c.covers_discounted.contains(ti)))
        .collect();

    let tid_strings: Vec<String> = target_ids.iter().map(|s| s.to_string()).collect();
    let tid_refs: Vec<&str> = tid_strings.iter().map(|s| s.as_str()).collect();
    let arb_report = solve::arbitrage_report(&candidates, &exact, &tid_refs);

    view! {
        <div class="results">
            // Optimal solution
            <div class="card solution-card">
                <h2>"\u{2728} Optimal Memberships"</h2>
                <SolutionDisplay
                    solution=exact.clone()
                    candidates=candidates.clone()
                    target_names=target_names.clone()
                    dataset=dataset.clone()
                    n_targets=n_targets
                />
            </div>

            // Budget solution (if requested)
            {budget_solution.map(|bs| {
                let cands = candidates.clone();
                let tnames = target_names.clone();
                let ds = dataset.clone();
                view! {
                    <div class="card solution-card warn">
                        <h2>"\u{1f4b0} Best Under Budget"</h2>
                        <SolutionDisplay
                            solution=bs
                            candidates=cands
                            target_names=tnames
                            dataset=ds
                            n_targets=n_targets
                        />
                    </div>
                }
            })}

            // Warnings
            {(!unreachable.is_empty()).then(|| {
                let items: Vec<String> = unreachable.iter().map(|&ti| {
                    let name = &target_names[ti];
                    if discount_only.contains(&ti) {
                        format!("{} (discount only)", name)
                    } else {
                        format!("{} (no reciprocal path)", name)
                    }
                }).collect();
                view! {
                    <div class="uncovered">
                        <strong>"\u{26a0}\u{fe0f} No free reciprocal path to:"</strong>
                        <ul style="margin: 0.25rem 0 0 1.2rem;">
                            {items.into_iter().map(|item| view! { <li>{item}</li> }).collect::<Vec<_>>()}
                        </ul>
                    </div>
                }
            })}

            // Arbitrage table
            <div class="card" style="margin-top: 1rem;">
                <h2>"\u{1f4ca} Per-Target Analysis"</h2>
                <table class="arb-table">
                    <thead>
                        <tr>
                            <th>"Target"</th>
                            <th>"Cheapest Free Via"</th>
                            <th>"Price"</th>
                        </tr>
                    </thead>
                    <tbody>
                        {arb_report.per_target.iter().enumerate().map(|(ti, ta)| {
                            let name = target_names[ti].clone();
                            let (via, price) = match ta.cheapest_free {
                                Some((ci, p)) => {
                                    let c = &candidates[ci];
                                    let inst = dataset.institution(&c.institution_id).unwrap();
                                    (format!("{} \u{2014} {}", inst.name, c.tier), format!("${:.0}", p))
                                }
                                None => {
                                    match ta.cheapest_discounted {
                                        Some((ci, p)) => {
                                            let c = &candidates[ci];
                                            let inst = dataset.institution(&c.institution_id).unwrap();
                                            (format!("{} (discount)", inst.name), format!("${:.0}", p))
                                        }
                                        None => ("\u{2014}".into(), "\u{2014}".into()),
                                    }
                                }
                            };
                            view! {
                                <tr>
                                    <td>{name}</td>
                                    <td>{via}</td>
                                    <td>{price}</td>
                                </tr>
                            }
                        }).collect::<Vec<_>>()}
                    </tbody>
                </table>
            </div>
        </div>
    }
}

#[component]
fn SolutionDisplay(
    solution: solve::SolutionSet,
    candidates: Vec<solve::CandidateMembership>,
    target_names: Vec<String>,
    dataset: tessera_core::model::Dataset,
    n_targets: usize,
) -> impl IntoView {
    if solution.selected.is_empty() {
        return view! {
            <p style="color: var(--warn);">"No memberships needed \u{2014} or no coverage possible."</p>
        }.into_any();
    }

    let total = solution.total_cost;
    let free_count = solution.free_count();
    let uncovered_names: Vec<String> = (0..n_targets)
        .filter(|ti| !solution.covered_free.contains(ti))
        .map(|ti| target_names[ti].clone())
        .collect();

    let picks: Vec<(String, String, f64, Vec<String>)> = solution
        .selected
        .iter()
        .map(|&si| {
            let c = &candidates[si];
            let inst = dataset.institution(&c.institution_id).unwrap();
            let covered: Vec<String> = c
                .covers_free
                .iter()
                .filter(|ti| solution.covered_free.contains(ti))
                .map(|&ti| target_names[ti].clone())
                .collect();
            (inst.name.clone(), c.tier.clone(), c.price_usd, covered)
        })
        .collect();

    view! {
        <div>
            <div class="solution-summary">
                <div class="stat">
                    <div class="value">{format!("${:.0}", total)}</div>
                    <div class="label">"Total Cost"</div>
                </div>
                <div class="stat">
                    <div class="value">{format!("{}/{}", free_count, n_targets)}</div>
                    <div class="label">"Free Coverage"</div>
                </div>
                <div class="stat">
                    <div class="value">{picks.len()}</div>
                    <div class="label">{if picks.len() == 1 { "Membership" } else { "Memberships" }}</div>
                </div>
            </div>

            {picks.into_iter().map(|(name, tier, price, covered)| {
                view! {
                    <div class="pick">
                        <div class="pick-header">
                            <div>
                                <span class="pick-name">{name.clone()}</span>
                                <span class="pick-tier">{format!(" \u{2014} {}", tier)}</span>
                            </div>
                            <span class="pick-price">{format!("${:.0}", price)}</span>
                        </div>
                        <div class="pick-covers">
                            <span class="cov-free">
                                {format!("\u{2192} {}", covered.join(", "))}
                            </span>
                        </div>
                    </div>
                }
            }).collect::<Vec<_>>()}

            {(!uncovered_names.is_empty()).then(|| {
                view! {
                    <p style="font-size: 0.85rem; color: #78716c; margin-top: 0.5rem;">
                        {format!("Not covered: {}", uncovered_names.join(", "))}
                    </p>
                }
            })}
        </div>
    }.into_any()
}
