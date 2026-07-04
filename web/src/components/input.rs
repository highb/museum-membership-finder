use leptos::prelude::*;

#[component]
pub fn InputPanel(
    /// (id, name, city+region, networks)
    institutions: Vec<(String, String, String, String)>,
    on_solve: impl Fn(String, Vec<String>, Option<f64>) + 'static + Clone,
) -> impl IntoView {
    let (zip, set_zip) = signal("97007".to_string());
    let (budget_str, set_budget_str) = signal(String::new());

    // Track which institutions are selected (indices)
    let inst_count = institutions.len();
    let (selected, set_selected) = signal::<Vec<bool>>(vec![true; inst_count]);

    let institutions = StoredValue::new(institutions);

    let toggle = move |idx: usize| {
        set_selected.update(|v| {
            if let Some(val) = v.get_mut(idx) {
                *val = !*val;
            }
        });
    };

    let select_all = move |_| {
        set_selected.set(vec![true; inst_count]);
    };

    let select_none = move |_| {
        set_selected.set(vec![false; inst_count]);
    };

    let on_solve_click = on_solve.clone();
    let handle_solve = move |_| {
        let z = zip.get();
        let sel = selected.get();
        let target_ids: Vec<String> = institutions.with_value(|insts| {
            insts
                .iter()
                .enumerate()
                .filter(|(i, _)| sel.get(*i).copied().unwrap_or(false))
                .map(|(_, inst)| inst.0.clone())
                .collect()
        });
        let budget: Option<f64> = {
            let bs = budget_str.get();
            if bs.is_empty() {
                None
            } else {
                bs.parse().ok()
            }
        };
        on_solve_click(z, target_ids, budget);
    };

    let selected_count = move || selected.get().iter().filter(|&&v| v).count();

    view! {
        <div class="card">
            <h2>"Where do you live?"</h2>
            <div class="input-row">
                <div class="zip-field">
                    <label for="zip">"ZIP Code"</label>
                    <input
                        type="text"
                        id="zip"
                        placeholder="97007"
                        maxlength="5"
                        prop:value=move || zip.get()
                        on:input=move |ev| {
                            let val = event_target_value(&ev);
                            set_zip.set(val);
                        }
                    />
                    <p class="hint">"PNW ZIPs (OR/WA) supported"</p>
                </div>
                <div class="budget-field">
                    <label for="budget">"Budget (optional)"</label>
                    <input
                        type="number"
                        id="budget"
                        placeholder="e.g. 200"
                        min="0"
                        prop:value=move || budget_str.get()
                        on:input=move |ev| {
                            let val = event_target_value(&ev);
                            set_budget_str.set(val);
                        }
                    />
                    <p class="hint">"Max $ to spend"</p>
                </div>
            </div>
        </div>

        <div class="card">
            <h2>"Where do you want to visit?"</h2>
            <div class="select-controls">
                <button on:click=select_all>"Select all"</button>
                <button on:click=select_none>"Select none"</button>
                <span style="color: #78716c; font-size: 0.85rem; margin-left: auto; align-self: center;">
                    {move || format!("{} selected", selected_count())}
                </span>
            </div>
            <div class="target-grid">
                {institutions.with_value(|insts| {
                    insts
                        .iter()
                        .enumerate()
                        .map(|(idx, inst)| {
                            let name = inst.1.clone();
                            let meta = format!("{} \u{b7} {}", inst.2, inst.3);
                            view! {
                                <label class="target-item">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || {
                                            selected.get().get(idx).copied().unwrap_or(false)
                                        }
                                        on:change=move |_| toggle(idx)
                                    />
                                    <span>
                                        <span class="name">{name}</span>
                                        <br />
                                        <span class="meta">{meta}</span>
                                    </span>
                                </label>
                            }
                        })
                        .collect::<Vec<_>>()
                })}
            </div>
        </div>

        <button
            class="btn-primary"
            on:click=handle_solve
            disabled=move || selected_count() == 0
        >
            "\u{1f50d} Find optimal memberships"
        </button>
    }
}
