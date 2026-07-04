# Data Provenance

This directory contains a **hand-curated fact table** of reciprocal museum
membership data. It is *not* a copy of any association's compiled member list.

## Sources

| Source | Type | Retrieved | Notes |
|--------|------|-----------|-------|
| ASTC directory (astc.org) | Network member list | 2026-06 | Institution names, cities, ASTC participation |
| NARM directory (nfrm.org) | Network member list | 2026-06 | Institution names, exclusion flags (**, #) |
| ACM directory (childrensmuseums.org) | Network member list | 2026-06 | Institution names, reciprocal tier |
| AZA directory (aza.org) | Network member list | 2026-06 | Zoo/aquarium names, reciprocal discount level |
| AHS directory (ahsgardening.org) | Network member list | 2026-06 | Garden names, 90-mi flag |
| Individual institution websites | Membership pricing | 2026-06 | Tier names, prices, network unlock levels |

## Dataset scope

The current dataset covers **80 institutions** in the PNW region (Oregon and
Washington) across 5 reciprocal networks (NARM, ASTC, AHS, AZA, ACM). Data was
scraped from spgfan.com directory pages and merged via `cargo xtask scrape` in
July 2026.

## Freshness disclaimer

Prices, network participation, and exclusion rules change. Always verify with
the institution before visiting. This data carries no warranty.
