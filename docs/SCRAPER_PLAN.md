# Per-Network Scraper Plan

> Replace the single spgfan.com scraper with per-association official-source scrapers.
> Each scraper is independent — build and test in parallel.

## Motivation

The current `cargo xtask scrape` pulls from spgfan.com, a fan site that:
- Lags behind official rosters (e.g. Portland Children's Museum listed but permanently closed)
- Only covers NARM + ASTC, only OR/WA
- Has no exclusion flags, coordinates, or membership pricing

Official association sources provide richer, more current data.

---

## Network Source Summary

| Network | Size | Official Source | Format | Data richness |
|---------|------|----------------|--------|---------------|
| **NARM** | ~1,541 | Quarterly PDF at `narmassociation.org/wp-content/uploads/YYYY/MM/NARM_SEASON_YYYY.pdf` | PDF (tabular: city, name, phone, exclusion flags) | Name, city, state, phone, exclusion symbols (`*`, `**`, `***`, `#`, `##`, `^`) |
| **ASTC** | ~372 | Web search at `myastc.astc.org/passport-program-search` | HTML (ASP.NET paginated, 4 pages of 100) | Name, address, state, phone, website, admittance policy, proof-of-residence flag |
| **ROAM** | ~526 | PDF on Google Drive (linked from `sites.google.com/site/roammuseums`) | PDF (13-page table: state, city, name, restriction flags) | Name, city, state, restriction flags only — no URLs, phones, or coords |
| **AHS** | ~399 | JSON blob embedded in `ahsgardening.org/ahs-garden-network/` page | JSON (extracted from `js_data.locations`) | Name, lat/lon, address, phone, website, 90-mi exclusion flag, benefits |
| **ACM** | ~209 reciprocal | AJAX endpoint: `POST findachildrensmuseum.org/wp-admin/admin-ajax.php` (action=`csl_ajax_search`) | JSON (Store Locator Plus plugin) | Name, address, city, state, ZIP, lat/lon, phone, website, category="Reciprocal" |
| **AZA** | ~150+ | PDF at `assets.speakcdn.com/assets/2332/reciprocity_chart.pdf` | PDF (6-page chart: state, city, name, reciprocity level, contact, phone) | Name, city, state, reciprocity tier (50%/100%/Free), contact, phone |
| **MARP** | ~70 | Google Sites page at `sites.google.com/view/marplist` + PDF from member museums | HTML (static list) | Name, city, state — small, mostly art museums. Not accepting new members. |
| **Time Travelers** | unknown | No known central directory yet | TBD | TBD — needs manual research |

---

## Scraper Architecture

Each network gets its own module in `xtask/src/scrapers/`:

```
xtask/src/
  scrapers/
    mod.rs          # ScrapedInstitution struct, shared helpers
    narm.rs         # PDF download + parse
    astc.rs         # HTML scrape (paginated)
    roam.rs         # PDF download + parse
    ahs.rs          # JSON extraction from page
    acm.rs          # AJAX JSON endpoint
    aza.rs          # PDF download + parse
    marp.rs         # HTML scrape (static page)
  scrape.rs         # Updated: orchestrates per-network scrapers, merges
  main.rs           # CLI: `cargo xtask scrape --networks narm,astc,...`
```

### Shared types (`scrapers/mod.rs`)

```rust
pub struct ScrapedInstitution {
    pub name: String,
    pub city: String,
    pub region: String,       // state/province code
    pub country: String,      // default "US"
    pub network: Network,
    pub website: Option<String>,
    pub phone: Option<String>,
    pub location: Option<LatLon>,  // if source provides coords
    pub exclusion_flags: Vec<String>,  // raw symbols: "**", "#", etc.
    pub extra: HashMap<String, String>, // network-specific fields
}
```

### Pipeline per scraper

1. **Fetch** — download PDF/HTML/JSON from official source
2. **Parse** — extract structured `ScrapedInstitution` records
3. **Return** `Vec<ScrapedInstitution>` to orchestrator

The orchestrator (`scrape.rs`) handles:
- Dedup across networks (same institution in NARM + ROAM + AHS)
- Merge into existing `data/institutions.json` (fuzzy match by name+city)
- Backfill coordinates via bundled ZIP centroid table or geocoding
- `--dry-run` mode
- Validation pass

---

## Per-Scraper Implementation Plan

### 1. ACM — easiest, start here
**Effort: Small**
- Single POST to `findachildrensmuseum.org/wp-admin/admin-ajax.php`
- Returns all 377 museums as JSON with lat/lon, address, category
- Filter `category_names == "Reciprocal"` → ~209 institutions
- Already has coordinates — no geocoding needed
- **Deps:** `reqwest`, `serde_json`

### 2. AHS — easy, JSON embedded in page
**Effort: Small**
- GET `ahsgardening.org/ahs-garden-network/`
- Extract `js_data.locations` JSON from page source (regex or DOM parse)
- 399 gardens with lat/lon, address, phone, website, 90-mi exclusion flag
- Already has coordinates
- **Deps:** `reqwest`, `serde_json`, `regex`

### 3. ASTC — medium, paginated HTML
**Effort: Medium**
- GET `myastc.astc.org/passport-program-search` with empty search
- Parse `<ul class="list-results">` → `<li>` entries
- ASP.NET postback pagination (4 pages) — need to POST `__VIEWSTATE` etc.
- 372 institutions with address, phone, website, admittance policy
- No coordinates — need geocoding
- **Deps:** `reqwest`, `scraper`, `regex`

### 4. NARM — medium, PDF parsing
**Effort: Medium**
- Download quarterly PDF: `narmassociation.org/wp-content/uploads/YYYY/MM/NARM_SEASON_YYYY.pdf`
- URL pattern is predictable (season name + year)
- Parse tabular PDF: "State" header, then rows of "City, Name, Phone"
- Exclusion symbols inline with name: `*`, `**`, `***`, `#`, `##`, `^`
- ~1,541 institutions, no coordinates
- **Deps:** `reqwest`, `pdf-extract` or shell out to `pdftotext`

### 5. ROAM — medium, PDF parsing
**Effort: Medium**
- Download PDF from Google Drive link (changes quarterly)
- Parse 13-page table: State, City, Museum Name, Restrictions
- ~526 museums, no coordinates or URLs
- **Deps:** `reqwest`, `pdf-extract` or `pdftotext`

### 6. AZA — medium, PDF parsing
**Effort: Medium**
- Download `assets.speakcdn.com/assets/2332/reciprocity_chart.pdf`
- Parse 6-page chart: State, City, Zoo/Aquarium, Reciprocity Level, Contact, Phone
- Reciprocity level parsing: "50%", "100% OR 50%", "FREE TO PUBLIC"
- ~150 institutions, no coordinates
- **Deps:** `reqwest`, `pdf-extract` or `pdftotext`

### 7. MARP — small, static HTML
**Effort: Small**
- Scrape `sites.google.com/view/marplist`
- ~70 institutions, static list
- Not accepting new members — low churn
- **Deps:** `reqwest`, `scraper`

### 8. Time Travelers — deferred
**Effort: Unknown**
- No central directory found yet
- Defer until source identified

---

## Parallelization

These scrapers are fully independent. Recommended execution:

```
┌─────────────────────────────────────────────────────┐
│ Phase 1: JSON sources (no PDF parsing needed)       │
│                                                     │
│   ┌─────┐   ┌─────┐                                │
│   │ ACM │   │ AHS │  ← can start immediately       │
│   └─────┘   └─────┘                                │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ Phase 2: HTML sources                               │
│                                                     │
│   ┌──────┐   ┌──────┐                               │
│   │ ASTC │   │ MARP │  ← can start immediately      │
│   └──────┘   └──────┘                               │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ Phase 3: PDF sources (need pdftotext or pdf-extract)│
│                                                     │
│   ┌──────┐   ┌──────┐   ┌─────┐                    │
│   │ NARM │   │ ROAM │   │ AZA │ ← after PDF dep    │
│   └──────┘   └──────┘   └─────┘                    │
└─────────────────────────────────────────────────────┘
```

All phases can run in parallel if PDF tooling is available.
Within each phase, individual scrapers have zero dependencies on each other.

---

## Geocoding Strategy

Sources that provide coordinates: **ACM**, **AHS** (via JSON lat/lon).
Sources that don't: **NARM**, **ASTC**, **ROAM**, **AZA**, **MARP**.

For institutions without coordinates:
1. Match against existing `data/institutions.json` entries (already geocoded)
2. Look up by ZIP from bundled `data/zip-centroids-*.json`
3. Flag remaining as `NEEDS GEOCODING` with `lat: 0.0, lon: 0.0`
4. Future: batch geocode via Nominatim (free, rate-limited) or bundled city-centroid table

---

## De-duplication

Many institutions appear in multiple networks (e.g. a museum in both NARM and ROAM).
The existing `find_match()` fuzzy-matching logic handles this — a scraped entry
matches an existing institution and adds the new network participation.

Key: always merge into a single `Institution` with multiple `Participation` entries.

---

## CLI Interface

```bash
# Scrape all networks
cargo xtask scrape --networks all

# Scrape specific networks
cargo xtask scrape --networks acm,ahs,astc

# Dry run
cargo xtask scrape --networks narm --dry-run

# Limit to specific states (for testing)
cargo xtask scrape --networks narm --states or,wa
```

---

## Migration from spgfan.com scraper

1. Keep `xtask/src/scrape.rs` as-is until all replacements are tested
2. Build new scrapers in `xtask/src/scrapers/`
3. Wire new scrapers into `scrape.rs` orchestrator
4. Verify output matches/exceeds spgfan data for OR+WA
5. Remove spgfan-specific code

---

## Open Questions

1. **PDF parsing crate vs. shelling out to `pdftotext`?** `pdftotext` (poppler) is more reliable for tabular PDFs. Could add as a build dependency or optional feature.
2. **Rate limiting** — ASTC pagination needs delays. ACM/AHS single-request, no issue.
3. **NARM Cloudflare** — the NARM website's search/map is behind Cloudflare Turnstile. The quarterly PDF is the reliable path.
4. **ROAM Google Drive link** — changes quarterly. May need to scrape the ROAM homepage to find the current link.
