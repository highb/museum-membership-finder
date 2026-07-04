# Tessera — Agent/Developer Guide

## Architecture

Rust workspace. All-WASM client-side app — no backend.

- `crates/tessera-core/` — domain model, Ascent (Datalog) rules engine, set-cover optimizer. Pure, no-IO, WASM-safe.
- `web/` — Leptos 0.7 CSR app. Built with Trunk (no Node/npm). Source in `web/src/`.
- `data/` — canonical JSON fixtures (institutions, memberships, networks, ZIP centroids). Embedded into WASM via `include_str!`.
- `xtask/` — CLI tooling for data validation and ingestion (`cargo xtask validate`, `cargo xtask add-institution`, etc.).

## Dev server

The web UI uses **Trunk** (`trunk serve`) for development. It watches source files, rebuilds WASM on change, and live-reloads the browser.

```bash
cd web
trunk serve       # serves on http://localhost:8000
```

Configuration is in `web/Trunk.toml`. Watched paths: `src/`, `style.css`, `index.html`, `../crates/tessera-core/src/`, `../data/`.

For a static build: `trunk build` (dev) or `trunk build --release` (optimized). Output goes to `web/dist/`.

## exe.dev VM setup

The dev server runs as a systemd service:

```
/etc/systemd/system/srv.service  →  trunk serve (port 8000)
```

Accessible at https://tessera.exe.xyz/ via the exe.dev proxy.

Useful commands:
- `systemctl status srv` — check dev server
- `journalctl -u srv -f` — tail dev server logs
- `sudo systemctl restart srv` — restart after config changes

Trunk auto-rebuilds on file changes, so you typically don't need to restart the service. Just edit and save.

## Testing

```bash
cargo test          # runs all workspace tests
cargo xtask validate  # checks data integrity
```

## Code conventions

- No hand-written JS. Leaflet interop is via `wasm-bindgen` + `js_sys`.
- Data files are JSON in `data/`, embedded at compile time.
- No backend, no accounts, no server-side storage of user location.
