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
cargo test          # 24 tests: model, geo, fixture round-trip
cargo doc --open    # browse the API docs
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
