# Tessera — Reciprocal Museum Coverage Optimizer

A client-side, all-Rust web tool that answers: **"Given where I live and the
institutions I want access to, what is the cheapest legal combination of museum
memberships that covers them?"**

Models the U.S. reciprocal-network landscape (NARM, ASTC, AHS, ROAM, MARP, ACM,
AZA, Time Travelers), encodes per-network/per-institution exclusion rules as
Datalog ([Ascent](https://github.com/s-arash/ascent)), and solves the
membership-selection problem as weighted set-cover.

## Status

**Cycle 0 complete** — workspace, domain model, fixture data, tests passing.
See [`docs/PLAN.md`](docs/PLAN.md) for the full roadmap.

## Quick start

```bash
cargo test                # all tests (model, geo, optimizer, fixtures)
cargo doc --open          # browse the API docs
```

### Development server

The web UI is a [Leptos](https://leptos.dev/) WASM SPA built with
[Trunk](https://trunkrs.dev/). To run the dev server with live-reload:

```bash
cd web
trunk serve               # http://localhost:8000
```

Trunk watches `src/`, `style.css`, `index.html`, `../crates/tessera-core/src/`,
and `../data/` — any change triggers a WASM rebuild and browser reload.

For a one-off production build (no dev server):

```bash
cd web
trunk build --release     # outputs to web/dist/
```

## Architecture

- **Pure Rust, no hand-written JS.** Leptos (WASM SPA), Ascent (Datalog),
  pure-Rust optimizer. No C/C++ FFI.
- **Local-first, privacy-by-architecture.** User's address never leaves the
  browser. All computation is client-side.
- **Data as JSON in git.** Hand-curated fact table with per-row provenance.
  Quarterly refresh. Shipped as `rkyv` zero-copy archive.

## Project layout

```
crates/
  tessera-core/     # domain model, rules engine, optimizer (WASM-safe)
  tessera-cli/      # native CLI (Cycle 3)
  tessera-wasm/     # wasm-bindgen bindings (Cycle 4)
web/                # Leptos app (Cycle 4)
data/               # canonical JSON fixtures + provenance
docs/               # project plan and design docs
```

## License

MIT OR Apache-2.0
