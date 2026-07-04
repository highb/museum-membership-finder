use leptos::prelude::*;
use crate::components::map::{MapView, MapInstitution};

/// Data for a single institution row.
#[derive(Clone, Debug)]
pub struct InstRow {
    pub id: String,
    pub name: String,
    pub city: String,
    pub region: String,
    pub networks: Vec<String>,
    pub website: Option<String>,
    pub lat: f64,
    pub lon: f64,
}

#[component]
pub fn InputPanel(
    /// (id, name, city+region, networks, website, lat, lon)
    institutions: Vec<(String, String, String, String, Option<String>, f64, f64)>,
    on_solve: impl Fn(String, Vec<String>, Option<f64>) + 'static + Clone,
) -> impl IntoView {
    let (zip, set_zip) = signal("97007".to_string());
    let (budget_str, set_budget_str) = signal(String::new());

    // Parse institution tuples into structured rows
    let rows: Vec<InstRow> = institutions
        .iter()
        .map(|(id, name, city_region, nets, website, lat, lon)| {
            let parts: Vec<&str> = city_region.splitn(2, ", ").collect();
            let city = parts.first().unwrap_or(&"").to_string();
            let region = parts.get(1).unwrap_or(&"").to_string();
            let networks: Vec<String> = nets
                .split(", ")
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            InstRow { id: id.clone(), name: name.clone(), city, region, networks, website: website.clone(), lat: *lat, lon: *lon }
        })
        .collect();

    // Derive available regions and networks for filter UI
    let all_regions: Vec<String> = {
        let mut r: Vec<String> = rows.iter().map(|r| r.region.clone()).collect();
        r.sort();
        r.dedup();
        r
    };
    let all_networks: Vec<String> = {
        let mut n: Vec<String> = rows.iter().flat_map(|r| r.networks.clone()).collect();
        n.sort();
        n.dedup();
        n
    };

    let inst_count = rows.len();
    let (selected, set_selected) = signal::<Vec<bool>>(vec![false; inst_count]);
    let (search_query, set_search_query) = signal(String::new());
    let (active_region, set_active_region) = signal::<Option<String>>(None);
    let (active_network, set_active_network) = signal::<Option<String>>(None);
    let (view_mode, set_view_mode) = signal::<&str>("list"); // "list" or "map"

    let rows = StoredValue::new(rows);
    let all_regions = StoredValue::new(all_regions);
    let all_networks = StoredValue::new(all_networks);

    // Compute which indices pass the current filters
    let filtered_indices = move || {
        let query = search_query.get().to_lowercase();
        let region_filter = active_region.get();
        let network_filter = active_network.get();

        rows.with_value(|rows| {
            rows.iter()
                .enumerate()
                .filter(|(_, row)| {
                    // Text search
                    if !query.is_empty() {
                        let haystack = format!("{} {} {}", row.name, row.city, row.networks.join(" ")).to_lowercase();
                        if !haystack.contains(&query) {
                            return false;
                        }
                    }
                    // Region filter
                    if let Some(ref r) = region_filter {
                        if row.region != *r {
                            return false;
                        }
                    }
                    // Network filter
                    if let Some(ref n) = network_filter {
                        let n_upper = n.to_uppercase();
                        if !row.networks.iter().any(|net| net.to_uppercase() == n_upper) {
                            return false;
                        }
                    }
                    true
                })
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        })
    };

    // Group filtered indices by region then city
    let grouped = move || {
        let indices = filtered_indices();
        rows.with_value(|rows| {
            let mut groups: Vec<(String, Vec<(String, Vec<usize>)>)> = Vec::new();
            // Collect into region -> city -> indices
            let mut region_map: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<usize>>> =
                std::collections::BTreeMap::new();
            for &idx in &indices {
                let row = &rows[idx];
                region_map
                    .entry(row.region.clone())
                    .or_default()
                    .entry(row.city.clone())
                    .or_default()
                    .push(idx);
            }
            for (region, cities) in region_map {
                let city_groups: Vec<(String, Vec<usize>)> = cities.into_iter().collect();
                groups.push((region, city_groups));
            }
            groups
        })
    };

    let toggle = move |idx: usize| {
        set_selected.update(|v| {
            if let Some(val) = v.get_mut(idx) {
                *val = !*val;
            }
        });
    };
    
    // Create a Copy version for the map
    let toggle_copy = move |idx: usize| {
        set_selected.update(|v| {
            if let Some(val) = v.get_mut(idx) {
                *val = !*val;
            }
        });
    };

    let select_visible = move |_| {
        let vis = filtered_indices();
        set_selected.update(|v| {
            for &i in &vis {
                if let Some(val) = v.get_mut(i) {
                    *val = true;
                }
            }
        });
    };

    let deselect_visible = move |_| {
        let vis = filtered_indices();
        set_selected.update(|v| {
            for &i in &vis {
                if let Some(val) = v.get_mut(i) {
                    *val = false;
                }
            }
        });
    };

    let select_none = move |_| {
        set_selected.set(vec![false; inst_count]);
    };

    let on_solve_click = on_solve.clone();
    let handle_solve = move |_| {
        let z = zip.get();
        let sel = selected.get();
        let target_ids: Vec<String> = rows.with_value(|rows| {
            rows.iter()
                .enumerate()
                .filter(|(i, _)| sel.get(*i).copied().unwrap_or(false))
                .map(|(_, row)| row.id.clone())
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

    let total_selected = move || selected.get().iter().filter(|&&v| v).count();
    let filtered_count = move || filtered_indices().len();
    let filtered_selected = move || {
        let vis = filtered_indices();
        let sel = selected.get();
        vis.iter().filter(|&&i| sel.get(i).copied().unwrap_or(false)).count()
    };

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

            // Search bar
            <div class="search-bar">
                <input
                    type="text"
                    class="search-input"
                    placeholder="\u{1f50d} Search by name or city\u{2026}"
                    prop:value=move || search_query.get()
                    on:input=move |ev| {
                        set_search_query.set(event_target_value(&ev));
                    }
                />
            </div>

            // Filter badges
            <div class="filter-row">
                <div class="filter-group">
                    <span class="filter-label">"State:"</span>
                    <button
                        class=move || if active_region.get().is_none() { "filter-badge active" } else { "filter-badge" }
                        on:click=move |_| set_active_region.set(None)
                    >"All"</button>
                    {all_regions.with_value(|regions| {
                        regions.iter().map(|r| {
                            let r2 = r.clone();
                            let r3 = r.clone();
                            view! {
                                <button
                                    class=move || {
                                        if active_region.get().as_deref() == Some(&r2) {
                                            "filter-badge active"
                                        } else {
                                            "filter-badge"
                                        }
                                    }
                                    on:click=move |_| set_active_region.set(Some(r3.clone()))
                                >{r.clone()}</button>
                            }
                        }).collect::<Vec<_>>()
                    })}
                </div>
                <div class="filter-group">
                    <span class="filter-label">"Network:"</span>
                    <button
                        class=move || if active_network.get().is_none() { "filter-badge active" } else { "filter-badge" }
                        on:click=move |_| set_active_network.set(None)
                    >"All"</button>
                    {all_networks.with_value(|nets| {
                        nets.iter().map(|n| {
                            let n2 = n.clone();
                            let n3 = n.clone();
                            view! {
                                <button
                                    class=move || {
                                        if active_network.get().as_deref() == Some(&n2) {
                                            "filter-badge active"
                                        } else {
                                            "filter-badge"
                                        }
                                    }
                                    on:click=move |_| set_active_network.set(Some(n3.clone()))
                                >{n.clone()}</button>
                            }
                        }).collect::<Vec<_>>()
                    })}
                </div>
            </div>

            // Selection controls
            <div class="select-controls">
                <button on:click=select_visible>"Select visible"</button>
                <button on:click=deselect_visible>"Deselect visible"</button>
                <button on:click=select_none>"Clear all"</button>
                <span class="select-counts">
                    {move || {
                        let fs = filtered_selected();
                        let fc = filtered_count();
                        let ts = total_selected();
                        if fc < inst_count {
                            format!("{fs}/{fc} shown \u{b7} {ts} total selected")
                        } else {
                            format!("{ts} of {fc} selected")
                        }
                    }}
                </span>
            </div>

            // View tabs (List / Map)
            <div class="view-tabs">
                <button
                    class=move || if view_mode.get() == "list" { "view-tab active" } else { "view-tab" }
                    on:click=move |_| set_view_mode.set("list")
                >"\u{1f4cb} List"</button>
                <button
                    class=move || if view_mode.get() == "map" { "view-tab active" } else { "view-tab" }
                    on:click=move |_| set_view_mode.set("map")
                >"\u{1f5fa}\u{fe0f} Map"</button>
            </div>

            // Conditional view rendering
            {move || {
                if view_mode.get() == "list" {
                    // List view
                    view! {
                        <div class="target-list">
                            {move || {
                                let groups = grouped();
                                if groups.is_empty() {
                                    return vec![view! {
                                        <div class="empty-state">
                                            "No institutions match your filters."
                                        </div>
                                    }.into_any()];
                                }
                                groups.into_iter().map(|(region, cities)| {
                                    let region_label = match region.as_str() {
                                        "OR" => "Oregon",
                                        "WA" => "Washington",
                                        _ => &region,
                                    };
                                    view! {
                                        <div class="region-group">
                                            <div class="region-header">{region_label.to_string()}</div>
                                            {cities.into_iter().map(|(city, indices)| {
                                                view! {
                                                    <div class="city-group">
                                                        <div class="city-header">{city}</div>
                                                        {indices.into_iter().map(|idx| {
                                                            rows.with_value(|rows| {
                                                                let row = &rows[idx];
                                                                let name = row.name.clone();
                                                                let nets = row.networks.join(", ");
                                                                let website = row.website.clone();
                                                                view! {
                                                                    <label class="target-item">
                                                                        <input
                                                                            type="checkbox"
                                                                            prop:checked=move || {
                                                                                selected.get().get(idx).copied().unwrap_or(false)
                                                                            }
                                                                            on:change=move |_| toggle(idx)
                                                                        />
                                                                        <span class="target-info">
                                                                            <span class="name">{name}</span>
                                                                            <span class="target-actions">
                                                                                {website.map(|url| view! {
                                                                                    <a
                                                                                        class="website-link"
                                                                                        href=url
                                                                                        target="_blank"
                                                                                        rel="noopener noreferrer"
                                                                                        on:click=move |ev| {
                                                                                            ev.stop_propagation();
                                                                                        }
                                                                                        title="Visit website"
                                                                                    >"\u{1f517}"</a>
                                                                                })}
                                                                                <span class="net-badges">
                                                                                    {nets.split(", ").map(|n| {
                                                                                        let cls = format!("net-badge net-{}", n.to_lowercase());
                                                                                        view! { <span class=cls>{n.to_string()}</span> }
                                                                                    }).collect::<Vec<_>>()}
                                                                                </span>
                                                                            </span>
                                                                        </span>
                                                                    </label>
                                                                }
                                                            })
                                                        }).collect::<Vec<_>>()}
                                                    </div>
                                                }
                                            }).collect::<Vec<_>>()}
                                        </div>
                                    }.into_any()
                                }).collect::<Vec<_>>()
                            }}
                        </div>
                    }.into_any()
                } else {
                    // Map view — derive filtered institutions as a signal
                    let map_institutions = Signal::derive(move || {
                        let vis = filtered_indices();
                        rows.with_value(|rows| {
                            vis.iter().map(|&idx| {
                                let r = &rows[idx];
                                MapInstitution {
                                    id: r.id.clone(),
                                    name: r.name.clone(),
                                    lat: r.lat,
                                    lon: r.lon,
                                    networks: r.networks.clone(),
                                    global_index: idx,
                                }
                            }).collect()
                        })
                    });
                    view! {
                        <MapView
                            institutions=map_institutions
                            selected=selected.into()
                            on_toggle=toggle_copy
                        />
                    }.into_any()
                }
            }}
        </div>

        <button
            class="btn-primary"
            on:click=handle_solve
            disabled=move || total_selected() == 0
        >
            "\u{1f50d} Find optimal memberships"
        </button>
    }
}
