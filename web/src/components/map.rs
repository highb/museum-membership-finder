use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[derive(Clone, Debug)]
pub struct MapInstitution {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub networks: Vec<String>,
    /// Original index into the full institution list (for selection state).
    pub global_index: usize,
}

/// Helper: call marker.remove() to detach from the map.
fn remove_marker(marker: &JsValue) {
    if let Ok(remove_fn) = js_sys::Reflect::get(marker, &JsValue::from_str("remove")) {
        if let Some(f) = remove_fn.dyn_ref::<js_sys::Function>() {
            let _ = f.call0(marker);
        }
    }
}

/// Helper: call marker.setStyle({fillColor})
fn set_marker_color(marker: &JsValue, color: &str) {
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &JsValue::from_str("fillColor"), &JsValue::from_str(color));
    if let Ok(set_style) = js_sys::Reflect::get(marker, &JsValue::from_str("setStyle")) {
        if let Some(f) = set_style.dyn_ref::<js_sys::Function>() {
            let _ = f.call1(marker, &opts);
        }
    }
}

/// Create a single Leaflet circleMarker, bind tooltip + click handler, add to map.
fn create_marker(
    l: &JsValue,
    map: &JsValue,
    inst: &MapInstitution,
    is_selected: bool,
    on_toggle: impl Fn(usize) + 'static,
) -> Result<JsValue, JsValue> {
    let circle_marker_fn = js_sys::Reflect::get(l, &JsValue::from_str("circleMarker"))?;
    let latlng = js_sys::Array::of2(&JsValue::from_f64(inst.lat), &JsValue::from_f64(inst.lon));
    let color = if is_selected { "#16a34a" } else { "#2563eb" };

    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &JsValue::from_str("radius"), &JsValue::from_f64(8.0))?;
    js_sys::Reflect::set(&opts, &JsValue::from_str("fillColor"), &JsValue::from_str(color))?;
    js_sys::Reflect::set(&opts, &JsValue::from_str("color"), &JsValue::from_str("#fff"))?;
    js_sys::Reflect::set(&opts, &JsValue::from_str("weight"), &JsValue::from_f64(2.0))?;
    js_sys::Reflect::set(&opts, &JsValue::from_str("fillOpacity"), &JsValue::from_f64(0.8))?;

    let marker = js_sys::Reflect::apply(
        circle_marker_fn.dyn_ref::<js_sys::Function>().ok_or("L.circleMarker not fn")?,
        l,
        &js_sys::Array::of2(&latlng, &opts),
    )?;

    // Tooltip
    let tooltip_text = if inst.networks.is_empty() {
        inst.name.clone()
    } else {
        format!("{} ({})", inst.name, inst.networks.join(", "))
    };
    let bind_tooltip = js_sys::Reflect::get(&marker, &JsValue::from_str("bindTooltip"))?;
    js_sys::Reflect::apply(
        bind_tooltip.dyn_ref::<js_sys::Function>().ok_or("bindTooltip not fn")?,
        &marker,
        &js_sys::Array::of1(&JsValue::from_str(&tooltip_text)),
    )?;

    // Click handler
    let global_idx = inst.global_index;
    let on_click = js_sys::Reflect::get(&marker, &JsValue::from_str("on"))?;
    let closure = Closure::wrap(Box::new(move |_: JsValue| {
        on_toggle(global_idx);
    }) as Box<dyn Fn(JsValue)>);
    js_sys::Reflect::apply(
        on_click.dyn_ref::<js_sys::Function>().ok_or("on not fn")?,
        &marker,
        &js_sys::Array::of2(&JsValue::from_str("click"), closure.as_ref()),
    )?;
    closure.forget();

    // Add to map
    let add_to = js_sys::Reflect::get(&marker, &JsValue::from_str("addTo"))?;
    js_sys::Reflect::apply(
        add_to.dyn_ref::<js_sys::Function>().ok_or("addTo not fn")?,
        &marker,
        &js_sys::Array::of1(map),
    )?;

    Ok(marker)
}

#[component]
pub fn MapView(
    institutions: Signal<Vec<MapInstitution>>,
    selected: Signal<Vec<bool>>,
    on_toggle: impl Fn(usize) + 'static + Copy,
) -> impl IntoView {
    let map_container = NodeRef::<leptos::html::Div>::new();

    // Leaflet map object + L namespace, populated after first mount.
    // Use signals (not StoredValue) so the markers effect re-runs when the map becomes ready.
    let (map_sig, set_map_sig) = signal::<Option<JsValue>>(None);
    let (l_sig, set_l_sig) = signal::<Option<JsValue>>(None);
    // Current markers on the map: (global_index, JsValue)
    let markers_store: StoredValue<Vec<(usize, JsValue)>> = StoredValue::new(Vec::new());

    // Initialize base map once after mount
    Effect::new(move |_| {
        if map_sig.get_untracked().is_some() {
            return; // already initialized
        }
        if let Some(container) = map_container.get() {
            let container_el = container.clone();
            request_animation_frame(move || {
                match init_base_map(container_el) {
                    Ok((map, l)) => {
                        set_map_sig.set(Some(map));
                        set_l_sig.set(Some(l));
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("Map init error: {:?}", e).into());
                    }
                }
            });
        }
    });

    // Reactive effect: sync markers to institutions signal.
    // Subscribes to map_sig + l_sig so it re-runs once the map is ready,
    // and to institutions so it re-runs when filters change.
    Effect::new(move |_| {
        let insts = institutions.get();
        let sel = selected.get_untracked();

        let map = match map_sig.get() {
            Some(m) => m,
            None => return, // map not ready yet — will re-run when set
        };
        let l = match l_sig.get() {
            Some(l) => l,
            None => return,
        };

        // Remove old markers
        let old_markers = markers_store.get_value();
        for (_, marker) in old_markers.iter() {
            remove_marker(marker);
        }

        // Add new markers
        let mut new_markers: Vec<(usize, JsValue)> = Vec::with_capacity(insts.len());
        for inst in insts.iter() {
            if inst.lat == 0.0 && inst.lon == 0.0 {
                continue;
            }
            let is_selected = sel.get(inst.global_index).copied().unwrap_or(false);
            match create_marker(&l, &map, inst, is_selected, on_toggle) {
                Ok(marker) => new_markers.push((inst.global_index, marker)),
                Err(e) => {
                    web_sys::console::error_1(&format!("Marker error: {:?}", e).into());
                }
            }
        }
        markers_store.set_value(new_markers);
    });

    // Reactive effect: update marker colors when selection changes
    Effect::new(move |_| {
        let sel = selected.get();
        let markers = markers_store.get_value();
        if markers.is_empty() {
            return;
        }
        for (global_idx, marker) in markers.iter() {
            let is_sel = sel.get(*global_idx).copied().unwrap_or(false);
            let color = if is_sel { "#16a34a" } else { "#2563eb" };
            set_marker_color(marker, color);
        }
    });

    view! {
        <div node_ref=map_container class="map-container"></div>
    }
}

fn request_animation_frame(f: impl FnOnce() + 'static) {
    use wasm_bindgen::closure::Closure;
    let closure = Closure::once_into_js(f);
    if let Some(window) = web_sys::window() {
        let _ = window.request_animation_frame(closure.as_ref().unchecked_ref());
    }
}

/// Create the Leaflet map + tile layer (no markers). Returns (map, L).
fn init_base_map(container: web_sys::HtmlDivElement) -> Result<(JsValue, JsValue), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let l = js_sys::Reflect::get(&window, &JsValue::from_str("L"))?;

    let container_id = format!("map-{}", js_sys::Math::random());
    container.set_id(&container_id);

    let map_fn = js_sys::Reflect::get(&l, &JsValue::from_str("map"))?;
    let map_options = js_sys::Object::new();
    js_sys::Reflect::set(
        &map_options,
        &JsValue::from_str("center"),
        &js_sys::Array::of2(&JsValue::from_f64(46.0), &JsValue::from_f64(-122.5)),
    )?;
    js_sys::Reflect::set(
        &map_options,
        &JsValue::from_str("zoom"),
        &JsValue::from_f64(6.0),
    )?;

    let map = js_sys::Reflect::apply(
        map_fn.dyn_ref::<js_sys::Function>().ok_or("L.map not fn")?,
        &l,
        &js_sys::Array::of2(&JsValue::from_str(&container_id), &map_options),
    )?;

    // Detect dark mode
    let document = window.document().ok_or("no document")?;
    let is_dark = document
        .document_element()
        .map(|el| el.class_list().contains("dark"))
        .unwrap_or(false);

    let tile_url = if is_dark {
        "https://cartodb-basemaps-{s}.global.ssl.fastly.net/dark_all/{z}/{x}/{y}.png"
    } else {
        "https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png"
    };

    let tile_layer_fn = js_sys::Reflect::get(&l, &JsValue::from_str("tileLayer"))?;
    let tile_options = js_sys::Object::new();
    js_sys::Reflect::set(
        &tile_options,
        &JsValue::from_str("attribution"),
        &JsValue::from_str("&copy; OpenStreetMap contributors"),
    )?;

    let tile_layer = js_sys::Reflect::apply(
        tile_layer_fn.dyn_ref::<js_sys::Function>().ok_or("L.tileLayer not fn")?,
        &l,
        &js_sys::Array::of2(&JsValue::from_str(tile_url), &tile_options),
    )?;

    let add_to = js_sys::Reflect::get(&tile_layer, &JsValue::from_str("addTo"))?;
    js_sys::Reflect::apply(
        add_to.dyn_ref::<js_sys::Function>().ok_or("addTo not fn")?,
        &tile_layer,
        &js_sys::Array::of1(&map),
    )?;

    Ok((map, l))
}
