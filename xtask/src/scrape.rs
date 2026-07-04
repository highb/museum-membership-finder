//! `cargo xtask scrape` -- fetch institution data from network directories
//! on spgfan.com and merge into data/institutions.json.

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Args;
use regex::Regex;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use tessera_core::model::*;

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

#[derive(Args)]
pub struct ScrapeArgs {
    /// Comma-separated list of state codes to scrape (default: or,wa)
    #[arg(long, default_value = "or,wa", value_delimiter = ',')]
    states: Vec<String>,

    /// Comma-separated list of networks to scrape (default: narm,astc)
    #[arg(long, default_value = "narm,astc", value_delimiter = ',')]
    networks: Vec<String>,

    /// Print what would change without writing
    #[arg(long)]
    dry_run: bool,
}

// ---------------------------------------------------------------------------
// Scraped entry
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ScrapedEntry {
    name: String,
    city: String,
    region: String,
    network: String,
    website: Option<String>,
}

// ---------------------------------------------------------------------------
// Normalisation helpers
// ---------------------------------------------------------------------------

fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn slugify(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let re = Regex::new(r"-+").unwrap();
    let s = re.replace_all(&s, "-");
    s.trim_matches('-').to_string()
}

fn network_from_str(s: &str) -> Option<Network> {
    match s.to_lowercase().as_str() {
        "narm" => Some(Network::Narm),
        "astc" => Some(Network::Astc),
        "ahs" => Some(Network::Ahs),
        "roam" => Some(Network::Roam),
        "marp" => Some(Network::Marp),
        "acm" => Some(Network::Acm),
        "aza" => Some(Network::Aza),
        "time_travelers" => Some(Network::TimeTravelers),
        _ => None,
    }
}

fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#8211;", "\u{2013}")
        .replace("&#8212;", "\u{2014}")
        .replace("&#8217;", "\u{2019}")
        .replace("&#8216;", "\u{2018}")
        .replace("&nbsp;", " ")
        .replace("&#038;", "&")
        .replace("&rsquo;", "\u{2019}")
        .replace("&lsquo;", "\u{2018}")
        .replace('\u{00a0}', " ")
}

// ---------------------------------------------------------------------------
// HTTP fetching
// ---------------------------------------------------------------------------

fn build_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("tessera-xtask/0.1 (museum-data-scraper)")
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// Fetch a URL. Returns None if it redirects (3xx), Err on real failures.
fn fetch_url(url: &str) -> Result<Option<String>> {
    eprintln!("  Fetching {url} ...");
    let client = build_client()?;
    let resp = client.get(url).send().with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    if status.is_redirection() {
        let loc = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");
        eprintln!("    -> Redirected to {loc}, skipping");
        return Ok(None);
    }
    if !status.is_success() {
        anyhow::bail!("HTTP {status} for {url}");
    }
    Ok(Some(resp.text()?))
}

fn fetch_and_parse(url: &str) -> Result<Vec<(String, String, Option<String>)>> {
    match fetch_url(url)? {
        Some(body) => parse_entries(&body),
        None => anyhow::bail!("URL redirected, not a valid source"),
    }
}

// ---------------------------------------------------------------------------
// HTML parsing
// ---------------------------------------------------------------------------

/// Extract text content from a table cell, decoding HTML entities.
fn cell_text(el: &scraper::ElementRef) -> String {
    let raw: String = el.text().collect::<Vec<_>>().join("");
    decode_html_entities(raw.trim())
}

/// Extract the href from the first <a> inside an element, if it looks like
/// an external institution URL (not a spgfan.com internal link).
fn cell_href(el: &scraper::ElementRef) -> Option<String> {
    let a_sel = Selector::parse("a[href]").unwrap();
    el.select(&a_sel).next().and_then(|a| {
        a.value().attr("href").and_then(|href| {
            let href = href.trim();
            // Skip spgfan.com internal links and empty/anchor-only hrefs
            if href.is_empty()
                || href.starts_with('#')
                || href.contains("spgfan.com")
            {
                None
            } else if href.starts_with("http://") || href.starts_with("https://") {
                Some(href.to_string())
            } else {
                None
            }
        })
    })
}

/// Parse institution entries from the `.entry-content` div.
///
/// Returns (city_state, name, website_url) tuples.
/// Primary strategy: HTML table rows with two <td> cells.
/// Fallback: tab-delimited text lines.
fn parse_entries(html: &str) -> Result<Vec<(String, String, Option<String>)>> {
    let doc = Html::parse_document(html);
    let content_sel = Selector::parse(".entry-content").unwrap();
    let entry_content = match doc.select(&content_sel).next() {
        Some(el) => el,
        None => {
            anyhow::bail!("No .entry-content element found");
        }
    };

    let mut results = Vec::new();

    // Strategy 1: HTML table rows
    let tr_sel = Selector::parse("table tr").unwrap();
    let td_sel = Selector::parse("td").unwrap();

    for row in entry_content.select(&tr_sel) {
        let cells: Vec<_> = row.select(&td_sel).collect();
        if cells.len() >= 2 {
            let city_state = cell_text(&cells[0]);
            let name = cell_text(&cells[1]);
            let url = cell_href(&cells[1]);
            if !city_state.is_empty() && !name.is_empty() {
                results.push((city_state, name, url));
            }
        }
    }

    // Strategy 2: tab-delimited text
    if results.is_empty() {
        let inner = entry_content.inner_html();
        let br_re = Regex::new(r"<br\s*/?>")?;
        let text = br_re.replace_all(&inner, "\n");
        let tag_re = Regex::new(r"<[^>]+>")?;
        let text = tag_re.replace_all(&text, "");
        let text = decode_html_entities(&text);

        let tab_re = Regex::new(r"^(.+?)\t(.+)$")?;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(caps) = tab_re.captures(line) {
                results.push((caps[1].trim().to_string(), caps[2].trim().to_string(), None));
            }
        }
    }

    Ok(results)
}

/// Parse "City, ST" into (city, state).
fn parse_city_state(s: &str) -> Option<(String, String)> {
    let re = Regex::new(r"^(.+?),?\s+([A-Z]{2})$").unwrap();
    re.captures(s)
        .map(|caps| (caps[1].trim().to_string(), caps[2].to_string()))
}

// ---------------------------------------------------------------------------
// Source scraping (with ASTC fallback)
// ---------------------------------------------------------------------------

/// URL for the main ASTC page (all states in one table).
const ASTC_MAIN_URL: &str =
    "https://spgfan.com/reciprocal-admission-programs/astc-travel-passport-program-association-of-science-technology-centers/";

fn scrape_source(
    network: &str,
    state: &str,
    expected_region: &str,
    astc_cache: &mut Option<Vec<(String, String, Option<String>)>>,
) -> (Vec<ScrapedEntry>, Vec<String>) {
    let url = format!("https://spgfan.com/{network}/{state}/");
    let mut entries = Vec::new();
    let mut warnings = Vec::new();

    // Try per-state URL first; fall back to main ASTC page if redirected
    let raw = match fetch_and_parse(&url) {
        Ok(raw) => raw,
        Err(_) if network == "astc" => {
            eprintln!("    Falling back to main ASTC page...");
            let cached = astc_cache.get_or_insert_with(|| {
                match fetch_and_parse(ASTC_MAIN_URL) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("    Failed to fetch ASTC main page: {e:#}");
                        Vec::new()
                    }
                }
            });
            cached
                .iter()
                .filter(|(cs, _, _)| {
                    parse_city_state(cs)
                        .map(|(_, r)| r.eq_ignore_ascii_case(expected_region))
                        .unwrap_or(false)
                })
                .cloned()
                .collect()
        }
        Err(e) => {
            warnings.push(format!("Failed to fetch {url}: {e:#}"));
            return (entries, warnings);
        }
    };

    if raw.is_empty() {
        warnings.push(format!("No entries found for {network}/{state}"));
    }

    for (city_state, name, url) in &raw {
        match parse_city_state(city_state) {
            Some((city, region)) => {
                if region.eq_ignore_ascii_case(expected_region) {
                    entries.push(ScrapedEntry {
                        name: name.clone(),
                        city,
                        region: region.to_uppercase(),
                        network: network.to_string(),
                        website: url.clone(),
                    });
                } else {
                    warnings.push(format!(
                        "Unexpected region {region} (expected {expected_region}) for '{name}' - skipping"
                    ));
                }
            }
            None => {
                // Skip header rows like "OREGON" or "CITY / STATE"
                if !name.contains("ATTRACTION") && !city_state.contains("CITY") {
                    warnings.push(format!(
                        "Could not parse city/state from '{city_state}' for '{name}'"
                    ));
                }
            }
        }
    }

    (entries, warnings)
}

// ---------------------------------------------------------------------------
// Matching & merging
// ---------------------------------------------------------------------------

fn find_match(entry: &ScrapedEntry, institutions: &[Institution]) -> Option<usize> {
    let norm_scraped = normalize_name(&entry.name);
    let scraped_words: HashSet<&str> = norm_scraped.split_whitespace().collect();

    // 1. Exact normalized match
    for (i, inst) in institutions.iter().enumerate() {
        if normalize_name(&inst.name) == norm_scraped {
            return Some(i);
        }
    }

    // 2. Containment match (same region & city preferred)
    let mut candidates: Vec<(usize, bool)> = Vec::new();
    for (i, inst) in institutions.iter().enumerate() {
        let norm_inst = normalize_name(&inst.name);
        let same_city = inst.city.eq_ignore_ascii_case(&entry.city)
            && inst.region.eq_ignore_ascii_case(&entry.region);

        // Only match by containment if the shorter string is a substantial
        // fraction of the longer one (>=60%), avoiding false matches like
        // "Oregon Coast Council for the Arts" -> "Oregon Coast Aquarium".
        let shorter_len = norm_scraped.len().min(norm_inst.len());
        let longer_len = norm_scraped.len().max(norm_inst.len());
        let ratio = shorter_len as f64 / longer_len as f64;
        if (norm_inst.contains(&norm_scraped) || norm_scraped.contains(&norm_inst))
            && ratio >= 0.6
        {
            candidates.push((i, same_city));
        } else if same_city {
            // 3. Word-overlap match for same city: if >=2 meaningful words
            //    are shared, it is likely the same institution.
            let inst_words: HashSet<&str> = norm_inst.split_whitespace().collect();
            let stopwords: HashSet<&str> = [
                "the", "of", "and", "in", "at", "a", "for",
                "museum", "center", "art", "arts", "gallery",
                "historical", "history", "society", "county",
                "pacific", "northwest", "northwest", "oregon",
                "washington", "coast", "columbia", "valley",
                "island", "national", "american",
            ]
            .into_iter()
            .collect();
            let meaningful: Vec<&&str> = scraped_words
                .iter()
                .filter(|w| inst_words.contains(*w) && !stopwords.contains(*w))
                .collect();
            // Require >=2 meaningful shared words AND they must be >50% of
            // the shorter name's meaningful words to avoid false positives.
            let scraped_meaningful = scraped_words
                .iter()
                .filter(|w| !stopwords.contains(*w))
                .count();
            let inst_meaningful = inst_words
                .iter()
                .filter(|w| !stopwords.contains(*w))
                .count();
            let min_meaningful = scraped_meaningful.min(inst_meaningful);
            if meaningful.len() >= 2
                && min_meaningful > 0
                && meaningful.len() * 3 >= min_meaningful * 2
            {
                candidates.push((i, true));
            }
        }
    }

    if let Some(&(i, _)) = candidates.iter().find(|(_, same)| *same) {
        return Some(i);
    }
    if let Some(&(i, _)) = candidates.first() {
        return Some(i);
    }

    None
}

fn ensure_network_participation(institution: &mut Institution, network: Network) -> bool {
    if institution.participates.iter().any(|p| p.network == network) {
        return false;
    }
    institution.participates.push(Participation {
        network,
        admission: None,
        exclusion: None,
        special_exhibit_restricted: false,
    });
    true
}

fn find_coords_for_city(
    city: &str,
    region: &str,
    institutions: &[Institution],
) -> Option<LatLon> {
    institutions
        .iter()
        .find(|i| {
            i.city.eq_ignore_ascii_case(city)
                && i.region.eq_ignore_ascii_case(region)
                && (i.location.lat != 0.0 || i.location.lon != 0.0)
        })
        .map(|i| i.location)
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

pub fn run(args: ScrapeArgs) -> Result<()> {
    let date_str = Utc::now().format("%Y-%m").to_string();
    let mut all_entries: Vec<ScrapedEntry> = Vec::new();
    let mut all_warnings: Vec<String> = Vec::new();
    let mut source_counts: Vec<(String, usize)> = Vec::new();
    let mut astc_cache: Option<Vec<(String, String, Option<String>)>> = None;

    for network in &args.networks {
        for state in &args.states {
            let expected_region = state.to_uppercase();
            let (entries, warnings) =
                scrape_source(network, state, &expected_region, &mut astc_cache);
            let label = format!("{}-{}", network.to_uppercase(), state.to_uppercase());
            source_counts.push((label, entries.len()));
            all_entries.extend(entries);
            all_warnings.extend(warnings);
        }
    }

    // Print scrape summary
    println!("\n-- Scrape summary --");
    for (label, count) in &source_counts {
        println!("  Scraped {count} from {label}");
    }
    println!();

    if all_entries.is_empty() {
        println!("No entries scraped. Nothing to merge.");
        return Ok(());
    }

    // Load existing institutions
    let inst_path = Path::new("data/institutions.json");
    let text =
        std::fs::read_to_string(inst_path).with_context(|| "reading data/institutions.json")?;
    let mut institutions: Vec<Institution> =
        serde_json::from_str(&text).with_context(|| "parsing data/institutions.json")?;

    let mut new_count = 0usize;
    let mut updated_count = 0usize;
    let existing_ids: HashSet<String> = institutions.iter().map(|i| i.id.clone()).collect();

    // Merge each scraped entry
    for entry in &all_entries {
        let network = match network_from_str(&entry.network) {
            Some(n) => n,
            None => {
                all_warnings.push(format!(
                    "Unknown network '{}' - skipping '{}'",
                    entry.network, entry.name
                ));
                continue;
            }
        };

        match find_match(entry, &institutions) {
            Some(idx) => {
                if ensure_network_participation(&mut institutions[idx], network) {
                    updated_count += 1;
                    if args.dry_run {
                        println!(
                            "  [UPDATE] '{}' <- add network {}",
                            institutions[idx].name, entry.network
                        );
                    }
                }
                // Backfill website if not already set
                if institutions[idx].website.is_none() {
                    if let Some(ref url) = entry.website {
                        institutions[idx].website = Some(url.clone());
                    }
                }
            }
            None => {
                let mut slug = slugify(&entry.name);
                if existing_ids.contains(&slug)
                    || institutions.iter().any(|i| i.id == slug)
                {
                    slug = format!("{}-{}", slug, entry.region.to_lowercase());
                }

                let location =
                    find_coords_for_city(&entry.city, &entry.region, &institutions)
                        .unwrap_or_else(|| {
                            all_warnings.push(format!(
                                "NEEDS GEOCODING: {} in {}, {}",
                                entry.name, entry.city, entry.region
                            ));
                            LatLon::new(0.0, 0.0)
                        });

                let provenance = format!(
                    "Scraped from spgfan.com/{}/{}, {}",
                    entry.network,
                    entry.region.to_lowercase(),
                    date_str
                );

                let new_inst = Institution {
                    id: slug,
                    name: entry.name.clone(),
                    city: entry.city.clone(),
                    region: entry.region.clone(),
                    country: "US".to_string(),
                    location,
                    website: entry.website.clone(),
                    participates: vec![Participation {
                        network,
                        admission: None,
                        exclusion: None,
                        special_exhibit_restricted: false,
                    }],
                    provenance,
                };

                if args.dry_run {
                    println!(
                        "  [NEW] '{}' in {}, {} ({})",
                        new_inst.name, new_inst.city, new_inst.region, entry.network
                    );
                }

                institutions.push(new_inst);
                new_count += 1;
            }
        }
    }

    // Sort: region -> city -> name
    institutions.sort_by(|a, b| {
        a.region
            .cmp(&b.region)
            .then_with(|| a.city.cmp(&b.city))
            .then_with(|| a.name.cmp(&b.name))
    });

    // Summary
    println!("-- Merge summary --");
    println!("  {new_count} new institutions added");
    println!("  {updated_count} existing updated with new networks");
    println!("  {} total institutions", institutions.len());

    if !all_warnings.is_empty() {
        println!("\n-- Warnings --");
        for w in &all_warnings {
            println!("  ! {w}");
        }
    }

    // Write
    if args.dry_run {
        println!("\n  (dry run - no files written)");
    } else {
        let json = serde_json::to_string_pretty(&institutions)?;
        let mut file = std::fs::File::create(inst_path)
            .with_context(|| "writing data/institutions.json")?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        println!("\n  Wrote data/institutions.json");

        // Run validation
        println!("\n-- Running validation --");
        crate::validate::run()?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_name() {
        assert_eq!(
            normalize_name("Portland Art Museum / NARM"),
            "portland art museum narm"
        );
        assert_eq!(normalize_name("  Foo  Bar  "), "foo bar");
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Portland Art Museum"), "portland-art-museum");
        assert_eq!(slugify("Lan Su Chinese Garden"), "lan-su-chinese-garden");
        assert_eq!(
            slugify("ScienceWorks Hands-On Museum"),
            "scienceworks-hands-on-museum"
        );
    }

    #[test]
    fn test_parse_city_state() {
        assert_eq!(
            parse_city_state("Portland, OR"),
            Some(("Portland".to_string(), "OR".to_string()))
        );
        assert_eq!(
            parse_city_state("Baker City, OR"),
            Some(("Baker City".to_string(), "OR".to_string()))
        );
        assert_eq!(parse_city_state("nope"), None);
    }

    #[test]
    fn test_parse_table_entries() {
        let html = concat!(
            "<html><body><div class=\"entry-content\">",
            "<table><tbody>",
            "<tr><td><a href=\"https://www.spgfan.com/portlandor\">Portland, OR</a></td>",
            "<td><a href=\"https://portlandartmuseum.org/\">Portland Art Museum</a></td></tr>",
            "<tr><td><a href=\"https://www.spgfan.com/eugeneor\">Eugene, OR</a></td>",
            "<td><a href=\"https://jsma.uoregon.edu/\">Jordan Schnitzer Museum of Art</a></td></tr>",
            "</tbody></table></div></body></html>",
        );
        let result = parse_entries(html).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "Portland, OR");
        assert_eq!(result[0].1, "Portland Art Museum");
        assert_eq!(result[0].2, Some("https://portlandartmuseum.org/".to_string()));
        assert_eq!(result[1].0, "Eugene, OR");
        assert_eq!(result[1].1, "Jordan Schnitzer Museum of Art");
        assert_eq!(result[1].2, Some("https://jsma.uoregon.edu/".to_string()));
    }

    #[test]
    fn test_parse_table_filters_spgfan_urls() {
        let html = concat!(
            "<html><body><div class=\"entry-content\">",
            "<table><tbody>",
            "<tr><td><a href=\"https://www.spgfan.com/portlandor\">Portland, OR</a></td>",
            "<td><a href=\"https://www.spgfan.com/some-museum\">Some Museum</a></td></tr>",
            "</tbody></table></div></body></html>",
        );
        let result = parse_entries(html).unwrap();
        assert_eq!(result.len(), 1);
        // spgfan.com link should be filtered out
        assert_eq!(result[0].2, None);
    }

    #[test]
    fn test_find_match_exact() {
        let institutions = vec![Institution {
            id: "test".into(),
            name: "Portland Art Museum".into(),
            city: "Portland".into(),
            region: "OR".into(),
            country: "US".into(),
            location: LatLon::new(0.0, 0.0),
            website: None,
            participates: vec![],
            provenance: "test".into(),
        }];
        let entry = ScrapedEntry {
            name: "Portland Art Museum".into(),
            city: "Portland".into(),
            region: "OR".into(),
            network: "narm".into(),
            website: None,
        };
        assert_eq!(find_match(&entry, &institutions), Some(0));
    }

    #[test]
    fn test_find_match_containment() {
        let institutions = vec![Institution {
            id: "test".into(),
            name: "Jordan Schnitzer Museum of Art / University of Oregon".into(),
            city: "Eugene".into(),
            region: "OR".into(),
            country: "US".into(),
            location: LatLon::new(0.0, 0.0),
            website: None,
            participates: vec![],
            provenance: "test".into(),
        }];
        let entry = ScrapedEntry {
            name: "Jordan Schnitzer Museum of Art".into(),
            city: "Eugene".into(),
            region: "OR".into(),
            network: "narm".into(),
            website: None,
        };
        assert_eq!(find_match(&entry, &institutions), Some(0));
    }

    #[test]
    fn test_find_match_word_overlap() {
        let institutions = vec![Institution {
            id: "reach-museum".into(),
            name: "REACH Museum".into(),
            city: "Richland".into(),
            region: "WA".into(),
            country: "US".into(),
            location: LatLon::new(0.0, 0.0),
            website: None,
            participates: vec![],
            provenance: "test".into(),
        }];
        let entry = ScrapedEntry {
            name: "The REACH".into(),
            city: "Richland".into(),
            region: "WA".into(),
            network: "astc".into(),
            website: None,
        };
        // "the reach" vs "reach museum" - only 1 meaningful word overlap ("reach"),
        // so this should NOT match (threshold is 2).
        assert_eq!(find_match(&entry, &institutions), None);
    }
}
