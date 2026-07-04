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
}

#[component]
pub fn MapView(
    institutions: Vec<MapInstitution>,
    selected: Signal<Vec<bool>>,
    on_toggle: impl Fn(usize) + 'static + Copy,
) -> impl IntoView {
    let map_container = NodeRef::<leptos::html::Div>::new();
    let institutions = StoredValue::new(institutions);
    // Store marker JsValues so we can update their styles reactively
    let markers_store: StoredValue<Vec<JsValue>> = StoredValue::new(Vec::new());

    // Initialize map after mount
    Effect::new(move |_| {
        if let Some(container) = map_container.get() {
            let container_el = container.clone();

            // Give the DOM a tick to render
            request_animation_frame(move || {
                match init_leaflet_map(
                    container_el,
                    institutions.get_value(),
                    selected,
                    on_toggle,
                ) {
                    Ok(marker_list) => {
                        markers_store.set_value(marker_list);
                    }
                    Err(e) => {
                        web_sys::console::error_1(
                            &format!("Map init error: {:?}", e).into(),
                        );
                    }
                }
            });
        }
    });

    // Reactive effect: update marker colors when selection changes
    Effect::new(move |_| {
        let sel = selected.get();
        let markers = markers_store.get_value();
        if markers.is_empty() {
            return;
        }
        for (idx, marker) in markers.iter().enumerate() {
            let is_sel = sel.get(idx).copied().unwrap_or(false);
            let color = if is_sel { "#16a34a" } else { "#2563eb" };
            let opts = js_sys::Object::new();
            let _ = js_sys::Reflect::set(
                &opts,
                &JsValue::from_str("fillColor"),
                &JsValue::from_str(color),
            );
            if let Ok(set_style) =
                js_sys::Reflect::get(marker, &JsValue::from_str("setStyle"))
            {
                if let Some(f) = set_style.dyn_ref::<js_sys::Function>() {
                    let _ = f.call1(marker, &opts);
                }
            }
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

fn init_leaflet_map(
    container: web_sys::HtmlDivElement,
    institutions: Vec<MapInstitution>,
    selected: Signal<Vec<bool>>,
    on_toggle: impl Fn(usize) + 'static + Copy,
) -> Result<Vec<JsValue>, JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let l = js_sys::Reflect::get(&window, &JsValue::from_str("L"))?;

    let container_id = format!("map-{}", js_sys::Math::random());
    container.set_id(&container_id);

    // Create map: L.map(container, options)
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
        map_fn
            .dyn_ref::<js_sys::Function>()
            .ok_or("L.map not a function")?,
        &l,
        &js_sys::Array::of2(&JsValue::from_str(&container_id), &map_options),
    )?;

    // Detect dark mode
    let document = window.document().ok_or("no document")?;
    let body = document.body().ok_or("no body")?;
    let is_dark = body.class_list().contains("dark");

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
        tile_layer_fn
            .dyn_ref::<js_sys::Function>()
            .ok_or("L.tileLayer not a function")?,
        &l,
        &js_sys::Array::of2(&JsValue::from_str(tile_url), &tile_options),
    )?;

    let add_to = js_sys::Reflect::get(&tile_layer, &JsValue::from_str("addTo"))?;
    js_sys::Reflect::apply(
        add_to
            .dyn_ref::<js_sys::Function>()
            .ok_or("addTo not a function")?,
        &tile_layer,
        &js_sys::Array::of1(&map),
    )?;

    // Add markers
    let circle_marker_fn =
        js_sys::Reflect::get(&l, &JsValue::from_str("circleMarker"))?;

    let mut marker_list: Vec<JsValue> = Vec::with_capacity(institutions.len());

    for (idx, inst) in institutions.iter().enumerate() {
        // Skip institutions with no coords
        if inst.lat == 0.0 && inst.lon == 0.0 {
            continue;
        }

        let latlng = js_sys::Array::of2(
            &JsValue::from_f64(inst.lat),
            &JsValue::from_f64(inst.lon),
        );

        let is_selected = selected.get_untracked().get(idx).copied().unwrap_or(false);
        let color = if is_selected { "#16a34a" } else { "#2563eb" };

        let marker_options = js_sys::Object::new();
        js_sys::Reflect::set(
            &marker_options,
            &JsValue::from_str("radius"),
            &JsValue::from_f64(8.0),
        )?;
        js_sys::Reflect::set(
            &marker_options,
            &JsValue::from_str("fillColor"),
            &JsValue::from_str(color),
        )?;
        js_sys::Reflect::set(
            &marker_options,
            &JsValue::from_str("color"),
            &JsValue::from_str("#fff"),
        )?;
        js_sys::Reflect::set(
            &marker_options,
            &JsValue::from_str("weight"),
            &JsValue::from_f64(2.0),
        )?;
        js_sys::Reflect::set(
            &marker_options,
            &JsValue::from_str("fillOpacity"),
            &JsValue::from_f64(0.8),
        )?;

        let marker = js_sys::Reflect::apply(
            circle_marker_fn
                .dyn_ref::<js_sys::Function>()
                .ok_or("L.circleMarker not a function")?,
            &l,
            &js_sys::Array::of2(&latlng, &marker_options),
        )?;

        // Tooltip with name + networks
        let tooltip_text = if inst.networks.is_empty() {
            inst.name.clone()
        } else {
            format!("{} ({})", inst.name, inst.networks.join(", "))
        };
        let bind_tooltip =
            js_sys::Reflect::get(&marker, &JsValue::from_str("bindTooltip"))?;
        js_sys::Reflect::apply(
            bind_tooltip
                .dyn_ref::<js_sys::Function>()
                .ok_or("bindTooltip not a function")?,
            &marker,
            &js_sys::Array::of1(&JsValue::from_str(&tooltip_text)),
        )?;

        // Click handler
        let on_click =
            js_sys::Reflect::get(&marker, &JsValue::from_str("on"))?;
        let closure = Closure::wrap(Box::new(move |_: JsValue| {
            on_toggle(idx);
        }) as Box<dyn Fn(JsValue)>);

        js_sys::Reflect::apply(
            on_click
                .dyn_ref::<js_sys::Function>()
                .ok_or("on not a function")?,
            &marker,
            &js_sys::Array::of2(&JsValue::from_str("click"), closure.as_ref()),
        )?;
        closure.forget();

        // Add to map
        let add_to =
            js_sys::Reflect::get(&marker, &JsValue::from_str("addTo"))?;
        js_sys::Reflect::apply(
            add_to
                .dyn_ref::<js_sys::Function>()
                .ok_or("addTo not a function")?,
            &marker,
            &js_sys::Array::of1(&map),
        )?;

        marker_list.push(marker);
    }

    Ok(marker_list)
}
