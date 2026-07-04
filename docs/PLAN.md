---
title: "Tessera — Reciprocal Museum Coverage Optimizer"
created: 2026-07-03
modified: 2026-07-04
status: proposed
area: "[[Portfolio]]"
tags: [rust, wasm, datalog, optimization, local-first, open-source, portfolio, civic-tech]
next_action: Hand this brief to Shelly (exe.dev) to scaffold Cycle 0
repo: TBD (github.com/<you>/tessera)
license: "MIT OR Apache-2.0"
---

# Tessera — Reciprocal Museum Coverage Optimizer

> **Name.** A *tessera hospitalis* was a Roman token of reciprocal hospitality — often split in two, each party keeping half, honored across cities and generations. A membership card honored at other institutions is the same object. (Alt name if it collides with something: *Symbolon*, the Greek broken-token equivalent. Renaming is a one-liner; don't block on it.)

## Summary

A client-side, all-Rust web tool that answers: **"Given where I live and the institutions I want access to, what is the cheapest legal combination of museum memberships that covers them — and what does each unlock?"** It models the U.S. reciprocal-network landscape (NARM, ASTC, AHS, ROAM, MARP, ACM, AZA, Time Travelers), encodes the per-network / per-institution exclusion rules as Datalog, and solves the membership-selection problem as weighted set-cover.

Scope is deliberately **portfolio, not product** (see [Commercialization](#commercialization)). The return is career capital: Rust→WASM, rules-as-data (ties to Cedar / Formal Methods), combinatorial optimization, and local-first/privacy-by-architecture in one artifact. Origin: the Reciprocal Museum Memberships analysis (OMSI vs. the Lan Su rotating roster) — that hand-derived answer becomes the acceptance oracle in Cycle 3.

## Problem statement

Reciprocal-network data is siloed (each association ships its own PDF/map, refreshed on its own cadence — NARM ~quarterly, ~1,540 institutions) and the data that actually decides admission (which membership *tier* unlocks which network, guest counts, local exclusions) lives scattered across individual institutions' pages. Enthusiasts currently solve the optimization by hand in spreadsheets. Two sub-problems make it non-trivial and portfolio-worthy:

1. **Geographic eligibility.** ASTC excludes any target within 90 mi (linear) of *either* your residence *or* your home institution — **both** must clear. NARM applies per-institution 15/50-mi flags. AHS's 90-mi rule is enforced only by *some* gardens. Mixed network-level and institution-level predicates.
2. **Selection.** Choosing the min-cost set of memberships covering a target set is weighted set-cover (NP-hard), with the twist that a membership's coverage is *itself* geographically dependent on where you live.

## Architecture — locked decisions

Encoded here so they are **not reopened** during implementation.

| Concern | Decision | Why |
|---|---|---|
| Storage (canonical) | **Static `data/*.json` in git → `Vec<_>` in memory** | ~3–5k rows, ~10k edges, read-only, quarterly churn. JSON stays canonical for git-diffability. |
| Storage (shipped) | **`rkyv` zero-copy archive**, built from the JSON | Browser loads with zero-copy deserialization. |
| ~~Graph DB~~ | **No** | 2-hop bipartite, no traversal to accelerate. |
| ~~Tauri / embedded DB / Turso~~ | **No** | No desktop shell / write-path / backend needed. |
| Rules engine | **Ascent** (Datalog proc-macro) — **locked** | Compiles Datalog away into Rust. WASM-clean, no FFI. |
| Optimizer | **Pure-Rust branch-and-bound + greedy** — **locked** | Tiny instances. If ILP needed: `microlp` (pure Rust). |
| Analytical layer | **None. `Vec` + iterators.** | No DuckDB-WASM (C++), no DataFusion/Polars (too heavy). |
| Geo | **Haversine, brute force** | No spatial index at this N. |
| Geocoding | **Bundled ZIP-centroid table, local lookup** | No external geocoding — preserves privacy. |
| UI | **Leptos** (all-Rust) — **locked** | Fine-grained reactive WASM SPA, no JS framework. |
| Bundler | **Trunk. No Node/npm.** | Trunk emits WASM + wasm-bindgen JS bootstrap. |
| Automation | **`xtask` crate** | Build/deploy/ingest in Rust. |
| E2E test | **`fantoccini`** (Rust WebDriver client) | Drives a real browser from Rust. |
| Hosting | **Static: GitHub / Cloudflare Pages** | No backend to rot. |
| Dev env | **exe.dev VM** | Rust stable + `wasm32-unknown-unknown` + trunk. |

> **The maximalism ceiling.** Zero-JS in the browser isn't achievable today. The honest claim is **"no hand-written JS,"** and "Rust all the way down until it hits the browser boundary."

## Domain model (`tessera-core`)

Pure, no-I/O, WASM-safe.

```rust
enum Network { Narm, Astc, Ahs, Roam, Marp, Acm, Aza, TimeTravelers }
enum Admission { Free, Discount(f32) }

struct ExclusionRule {
    residence_radius_mi: Option<f64>,
    home_institution_radius_mi: Option<f64>,
    both_must_clear: bool,
}

struct NetworkSpec { network, admission, default_exclusion }
struct Participation { network, admission?, exclusion?, special_exhibit_restricted }
struct Institution { id, name, city, region, country, location, participates, provenance }
struct Membership { institution_id, tier, price_usd, networks_unlocked, guests_included }
struct User { residence, held }
struct Dataset { networks, institutions, memberships }
```

## Rules engine — eligibility

Datalog (Ascent). Per-network encoding:

| Network | Admission | Exclusion |
|---|---|---|
| ASTC | Free | 90 mi from residence **OR** home institution (both must clear) |
| NARM | Free | per-institution: `**` = 15 mi; `#` = 50 mi; else none |
| ROAM | Free | ~100 mi (configurable) |
| AHS | Free/disc | per-institution 90-mi flag |
| MARP | Free | per-institution if any |
| ACM | **50% off**, ≤6 people | none |
| AZA | **~50% off** | per-institution |
| Time Travelers | Free/disc | per-institution |

## Optimizer (`tessera-core::solve`)

- **Objectives:** (a) min cost for full free coverage; (b) max coverage under budget; (c) arbitrage report.
- **Solvers:** exact branch-and-bound; greedy (coverage-per-dollar) as fast baseline.

## Repo layout

```
tessera/
  Cargo.toml                 # workspace
  crates/
    tessera-core/            # model, rules (ascent), optimizer
    tessera-cli/             # native CLI over core
    tessera-wasm/            # wasm-bindgen bindings
  web/                       # Leptos app
  data/
    institutions.json
    memberships.json
    networks.json
    PROVENANCE.md
  docs/
    PLAN.md                  # this document
  ingest/                    # quarterly pipeline
  .github/workflows/         # CI
```

## Cycles

### Cycle 0 — Workspace + domain model + schema ✅
- `tessera-core` with type definitions
- JSON schema for data snapshot
- Hand-authored fixture (~12 institutions, 3+ networks) that round-trips through serde
- `cargo test` green; `cargo doc` renders the model

### Cycle 1 — Geo + eligibility rules
- Haversine + Ascent ruleset producing `eligible(user, institution, network, admission)`
- Golden tests: ASTC both-must-clear-90, NARM 15-mi flag, ACM 50%-off

### Cycle 2 — Set-cover optimizer
- Greedy + exact B&B; objective modes (a)/(b); arbitrage report (c)
- Constructed instances with known optima

### Cycle 3 — CLI end-to-end + real PNW data slice ✅
- `tessera-cli` with clap: `--zip`, `--lat/--lon`, `--targets`, `--budget`, `--list`, `-v`
- `tessera-core::zip` — bundled ZIP→lat/lon lookup (110 PNW entries), local-only
- 12 PNW institutions across 6 networks, 10 membership tiers
- Acceptance oracle (8 tests): "Aloha 97007 wants 8 PNW venues"
  - Optimal: Tacoma Art Museum Household ($95) + OMSI Explorer ($130) = $225
  - Covers 6/8 targets — key insight: Tacoma-based NARM clears Lan Su's 15-mi flag
    (OMSI-based NARM cannot, being ~1.5 mi away)
  - Uncoverable: Oregon Coast Aquarium (ASTC <90 mi), Woodland Park Zoo (AZA discount only)
- 59 tests total, all green

### Cycle 4 — WASM core + Leptos web UI ✅
- Leptos 0.7 CSR app with Trunk bundler, release build 457KB WASM
- ZIP input (bundled centroid lookup), target multiselect, optional budget
- Optimal memberships display: cost/coverage stats, pick cards, per-target table
- Warnings for unreachable (ASTC-excluded) and discount-only targets
- Privacy verified: no network requests, all computation in WASM
- Served on port 8000 via systemd (busybox httpd), accessible at tessera.exe.xyz
- No hand-written JS — Rust all the way down
- 59 tests still green

### Cycle 5 — Static deploy + portfolio embed
- GitHub Actions → Pages/CF; embeddable widget; README narrative

### Cycle 6 — (Stretch) Ingest pipeline
- `cargo xtask ingest` command; schema validation; idempotent

## Non-goals & guardrails

- **No backend, no accounts, no server-side storage of user location.** Ever.
- **Do not scrape-and-republish association member lists wholesale.** The dataset is a derived, attributed, hand-curated fact table with per-row provenance.
- **Freshness is best-effort and disclaimed in-UI.**
- Not a commercial product.

## Commercialization

**Intentionally non-commercial.** The product is career capital, not revenue. Ship under MIT/Apache-2.0, write the design-decisions README as the narrative.

## Open questions

1. **ROAM distance rule** — network-level 100 mi vs. per-institution? Model configurable.
2. **Data sourcing method** — confirm curated-fact-table approach before any ingestion.
3. **Styling** — plain CSS or `stylist`? Decide in Cycle 4.
