//! Tessera web UI — Leptos CSR app.
//!
//! Privacy property: the user's ZIP/location never leaves the browser.
//! All computation (geo lookup, rules engine, optimizer) runs client-side in WASM.

use leptos::prelude::*;
use wasm_bindgen::prelude::wasm_bindgen;

mod data;
mod components;

use components::app::App;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}
