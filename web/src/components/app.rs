use leptos::prelude::*;

use crate::components::input::InputPanel;
use crate::components::results::ResultsPanel;
use crate::data;
use tessera_core::model::*;
use tessera_core::solve;

#[component]
fn DarkModeToggle() -> impl IntoView {
    // Determine initial state: localStorage > system preference > light
    let initial_dark = web_sys::window()
        .map(|w| {
            // Check localStorage first
            if let Ok(Some(storage)) = w.local_storage() {
                if let Ok(Some(val)) = storage.get_item("dark-mode") {
                    return val == "true";
                }
            }
            // Fall back to system preference
            w.match_media("(prefers-color-scheme: dark)")
                .ok()
                .flatten()
                .map(|mq| mq.matches())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    let (is_dark, set_is_dark) = signal(initial_dark);
    // Track whether user has explicitly toggled (don't write to localStorage on init)
    let (user_toggled, set_user_toggled) = signal(false);

    // Apply dark mode class to <html> element on mount and when toggled
    Effect::new(move |_| {
        let dark = is_dark.get();
        let toggled = user_toggled.get();
        if let Some(window) = web_sys::window() {
            if let Some(document) = window.document() {
                if let Some(el) = document.document_element() {
                    if dark {
                        let _ = el.class_list().add_1("dark");
                    } else {
                        let _ = el.class_list().remove_1("dark");
                    }
                }
            }
            // Only persist to localStorage after explicit user toggle
            if toggled {
                if let Ok(Some(storage)) = window.local_storage() {
                    let _ = storage.set_item("dark-mode", if dark { "true" } else { "false" });
                }
            }
        }
    });

    let toggle = move |_| {
        set_user_toggled.set(true);
        set_is_dark.update(|v| *v = !*v);
    };

    view! {
        <div class="header-badges">
            <div class="privacy-badge">
                "\u{1f512} Your location never leaves this browser"
            </div>
            <button class="dark-mode-toggle" on:click=toggle title="Toggle dark mode">
                {move || if is_dark.get() { "\u{2600}\u{fe0f}" } else { "\u{1f319}" }}
            </button>
        </div>
    }
}

/// Solver output passed from input → results.
#[derive(Clone, Debug)]
pub struct SolverOutput {
    pub candidates: Vec<solve::CandidateMembership>,
    pub exact: solve::SolutionSet,
    pub budget_solution: Option<solve::SolutionSet>,
    pub target_names: Vec<String>,
    pub target_ids: Vec<String>,
    pub dataset: Dataset,
    pub zip_display: String,
}

#[component]
pub fn App() -> impl IntoView {
    let dataset = data::load_dataset();
    let zips = data::load_zips();

    // All institutions for target selection (now with lat/lon + type)
    let institutions: Vec<(String, String, String, String, Option<String>, InstitutionType, f64, f64)> = dataset
        .institutions
        .iter()
        .map(|i| {
            let nets: Vec<String> = i.participates.iter().map(|p| p.network.to_string()).collect();
            (i.id.clone(), i.name.clone(), format!("{}, {}", i.city, i.region), nets.join(", "), i.website.clone(), i.institution_type, i.location.lat, i.location.lon)
        })
        .collect();

    let (output, set_output) = signal::<Option<SolverOutput>>(None);
    let (error_msg, set_error) = signal::<Option<String>>(None);

    // Solver callback
    let ds_for_solve = dataset.clone();
    let on_solve = move |zip: String, target_ids: Vec<String>, budget: Option<f64>| {
        set_error.set(None);
        set_output.set(None);

        // Resolve residence
        let residence = match zips.lookup(&zip) {
            Some(loc) => loc,
            None => {
                set_error.set(Some(format!("ZIP code '{}' not found. Try a PNW ZIP (e.g. 97007, 98101).", zip)));
                return;
            }
        };

        if target_ids.is_empty() {
            set_error.set(Some("Select at least one target institution.".into()));
            return;
        }

        let targets: Vec<&Institution> = target_ids
            .iter()
            .filter_map(|id| ds_for_solve.institution(id))
            .collect();

        let all_memberships: Vec<&Membership> = ds_for_solve.memberships.iter().collect();

        let candidates =
            solve::compute_candidate_coverage(residence, &targets, &all_memberships, &ds_for_solve);
        let exact = solve::solve_exact_min_cost(&candidates, targets.len());
        let budget_solution = budget.map(|b| solve::solve_exact_max_coverage(&candidates, targets.len(), b));

        let target_names: Vec<String> = targets.iter().map(|t| t.name.clone()).collect();

        set_output.set(Some(SolverOutput {
            candidates,
            exact,
            budget_solution,
            target_names,
            target_ids: target_ids.clone(),
            dataset: ds_for_solve.clone(),
            zip_display: zip.clone(),
        }));
    };

    view! {
        <div class="app">
            <header class="header">
                <h1>"\u{1f3db}\u{fe0f} Tessera"</h1>
                <p class="subtitle">
                    "Reciprocal museum coverage optimizer \u{2014} find the cheapest memberships that unlock the most free visits."
                </p>
                <DarkModeToggle />
            </header>

            <InputPanel institutions=institutions on_solve=on_solve />

            {move || error_msg.get().map(|msg| view! {
                <div class="card" style="border-left: 3px solid var(--error);">
                    <p style="color: var(--error);">{msg}</p>
                </div>
            })}

            {move || output.get().map(|out| view! {
                <ResultsPanel output=out />
            })}

            <footer class="footer">
                <p>"Verify with each institution before visiting. Data is best-effort, not official."</p>
                <p>"Built with Rust + Leptos + WASM. "
                    <a href="https://github.com/tessera" target="_blank">"Source"</a>
                    " \u{b7} MIT / Apache-2.0"
                </p>
            </footer>
        </div>
    }
}
