//! MARP (Museum Alliance Reciprocal Program) scraper.
//!
//! Scrapes <https://sites.google.com/view/marplist/list-by-state>, a static
//! Google Sites page listing ~70 art museums organized into sections by
//! state/province. Each section has a state-name heading followed by a list
//! of museum names (some entries include a trailing city after a comma).

use super::astc::state_code;
use super::ScrapedInstitution;
use anyhow::{Context, Result};
use scraper::{Html, Selector};

const URL: &str = "https://sites.google.com/view/marplist/list-by-state";

/// Well-known city mappings for MARP museums whose entry text doesn't include
/// a city (major art museums with well-known locations).
fn known_city(name: &str, region: &str) -> &'static str {
    match (name, region) {
        ("Crocker Art Museum", _) => "Sacramento",
        ("San Francisco Museum of Modern Art", _) => "San Francisco",
        ("San Jose Museum of Art", _) => "San Jose",
        ("UC Berkeley Art Museum and Pacific Film Archive (BAMPFA)", _) => "Berkeley",
        ("Colorado Springs Fine Arts Center at Colorado College", _) => "Colorado Springs",
        ("The Bruce Museum", _) => "Greenwich",
        ("High Museum of Art", _) => "Atlanta",
        ("Museum of Contemporary Art Chicago", _) => "Chicago",
        ("The Indianapolis Museum of Art at Newfields", _) => "Indianapolis",
        ("New Orleans Museum of Art", _) => "New Orleans",
        ("Portland Museum of Art", "ME") => "Portland",
        ("The Baltimore Museum of Art", _) => "Baltimore",
        ("Walker Art Center", _) => "Minneapolis",
        ("Currier Museum of Art", _) => "Manchester",
        ("Montclair Art Museum", _) => "Montclair",
        ("New Jersey State Museum", _) => "Trenton",
        ("Newark Museum", _) => "Newark",
        ("Brooklyn Museum", _) => "Brooklyn",
        ("The Parrish Art Museum", _) => "Water Mill",
        ("Solomon R. Guggenheim Museum", _) => "New York",
        ("Cincinnati Art Museum", _) => "Cincinnati",
        ("Portland Art Museum", "OR") => "Portland",
        ("Knoxville Museum of Art", _) => "Knoxville",
        ("Amon Carter Museum of American Art", _) => "Fort Worth",
        ("Modern Art Museum of Fort Worth", _) => "Fort Worth",
        ("The Museum of Fine Arts", "TX") => "Houston",
        ("Chrysler Museum of Art", _) => "Norfolk",
        ("Virginia Museum of Contemporary Art", _) => "Virginia Beach",
        ("Milwaukee Art Museum", _) => "Milwaukee",
        ("Art Gallery of Ontario", _) => "Toronto",
        ("The Montreal Museum of Fine Arts", _) => "Montreal",
        ("National Gallery of Canada", _) => "Ottawa",
        ("Royal Ontario Museum", _) => "Toronto",
        ("The Vancouver Art Gallery", _) => "Vancouver",
        ("Worcester Museum of Art", _) => "Worcester",
        _ => "",
    }
}

/// Split a museum list entry into (name, city_hint). Many entries are just
/// "Museum Name"; some are "Museum Name, City" where the trailing segment
/// after the last comma is a short place name. We only split when the
/// trailing segment looks like a city (short, no museum-y words) so we don't
/// mis-split names like "UC Berkeley Art Museum and Pacific Film Archive
/// (BAMPFA)" which contain no comma anyway, or multi-clause names that do.
fn parse_museum_entry(text: &str) -> (String, String) {
    let text = text.trim();
    if let Some(comma_pos) = text.rfind(',') {
        let after = text[comma_pos + 1..].trim();
        let words: Vec<&str> = after.split_whitespace().collect();
        let looks_like_city = !words.is_empty()
            && words.len() <= 4
            && words[0].chars().next().is_some_and(char::is_uppercase)
            && !matches!(
                words[0].to_lowercase().as_str(),
                "the" | "and" | "of" | "for" | "at" | "in"
            )
            && !after.contains('(')
            && !after.contains("Museum")
            && !after.contains("Art")
            && !after.contains("Gallery")
            && !after.contains("Center");
        if looks_like_city {
            let name = text[..comma_pos].trim().to_string();
            return (name, after.to_string());
        }
    }
    (text.to_string(), String::new())
}

/// Canadian MARP museums mapped to their province.
fn canadian_province(name: &str) -> &'static str {
    match name {
        "Art Gallery of Ontario" | "Royal Ontario Museum" => "ON",
        "National Gallery of Canada" => "ON", // Ottawa, Ontario
        "The Montreal Museum of Fine Arts" => "QC",
        "The Vancouver Art Gallery" => "BC",
        _ => "ON",
    }
}

const KNOWN_REGION_HEADERS: &[&str] = &[
    "Alabama",
    "Alaska",
    "Arizona",
    "Arkansas",
    "California",
    "Colorado",
    "Connecticut",
    "Delaware",
    "Florida",
    "Georgia",
    "Hawaii",
    "Idaho",
    "Illinois",
    "Indiana",
    "Iowa",
    "Kansas",
    "Kentucky",
    "Louisiana",
    "Maine",
    "Maryland",
    "Massachusetts",
    "Massachussetts", // MARP page's misspelling
    "Michigan",
    "Minnesota",
    "Mississippi",
    "Missouri",
    "Montana",
    "Nebraska",
    "Nevada",
    "New Hampshire",
    "New Jersey",
    "New Mexico",
    "New York",
    "North Carolina",
    "North Dakota",
    "Ohio",
    "Oklahoma",
    "Oregon",
    "Pennsylvania",
    "Rhode Island",
    "South Carolina",
    "South Dakota",
    "Tennessee",
    "Texas",
    "Utah",
    "Vermont",
    "Virginia",
    "Washington",
    "West Virginia",
    "Wisconsin",
    "Wyoming",
    "District of Columbia",
    "Canada",
];

/// Parse the "List by State" page HTML into institutions.
pub fn parse_page(html: &str) -> Vec<ScrapedInstitution> {
    let doc = Html::parse_document(html);
    let section_sel = Selector::parse("section").unwrap();
    let li_sel = Selector::parse("li").unwrap();

    let mut results = Vec::new();

    for section in doc.select(&section_sel) {
        let full_text = section.text().collect::<String>();
        let full_text = full_text.trim();
        if full_text.is_empty() {
            continue;
        }

        let first_line = full_text.lines().next().unwrap_or("").trim();
        let state_name = if first_line == "Massachussetts" {
            "Massachusetts"
        } else {
            first_line
        };

        if !KNOWN_REGION_HEADERS.contains(&state_name) {
            continue;
        }

        let is_canada = state_name == "Canada";
        let region_code = if is_canada {
            String::new()
        } else {
            state_code(state_name).unwrap_or("").to_string()
        };

        for li in section.select(&li_sel) {
            let raw = li.text().collect::<String>();
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }

            // Strip footnote markers like trailing "*" / "**"
            let clean = raw.trim_end_matches('*').trim();

            let (name, city_hint) = parse_museum_entry(clean);
            if name.is_empty() {
                continue;
            }

            let (region, country) = if is_canada {
                (canadian_province(&name).to_string(), "CA".to_string())
            } else {
                (region_code.clone(), "US".to_string())
            };

            let city = if !city_hint.is_empty() {
                city_hint
            } else {
                known_city(&name, &region).to_string()
            };

            let mut inst = ScrapedInstitution::new(name, city, region.clone(), "marp");
            inst.country = country;

            // MARP notes a 150km (~93mi) exclusion for Ontario institutions.
            if is_canada && region == "ON" {
                inst.exclusion_radius_mi = Some(93.0);
            }

            results.push(inst);
        }
    }

    results
}

pub fn scrape() -> Result<Vec<ScrapedInstitution>> {
    let client = super::http_client()?;

    eprintln!("[marp] GET {URL}");
    let html = client.get(URL).send().context("MARP GET failed")?.text()?;

    let results = parse_page(&html);
    eprintln!("[marp] found {} institutions", results.len());

    if results.is_empty() {
        anyhow::bail!("MARP scraper found no institutions — page structure may have changed");
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_html() -> &'static str {
        r#"<html><body>
        <section>
            <h2>California</h2>
            <ul>
                <li>Crocker Art Museum</li>
                <li>The Museum of Contemporary Art, Los Angeles</li>
                <li>San Francisco Museum of Modern Art</li>
                <li>San Jose Museum of Art</li>
                <li>Skirball Cultural Center, Los Angeles</li>
                <li>UC Berkeley Art Museum and Pacific Film Archive (BAMPFA)</li>
            </ul>
        </section>
        <section>
            <h2>Colorado</h2>
            <ul>
                <li>Colorado Springs Fine Arts Center at Colorado College</li>
            </ul>
        </section>
        <section>
            <h2>New York</h2>
            <ul>
                <li>Albright-Knox Art Gallery, Buffalo</li>
                <li>Brooklyn Museum</li>
                <li>The Parrish Art Museum</li>
                <li>Solomon R. Guggenheim Museum</li>
            </ul>
        </section>
        <section>
            <h2>Canada</h2>
            <ul>
                <li>Art Gallery of Ontario**</li>
                <li>The Montreal Museum of Fine Arts</li>
                <li>National Gallery of Canada</li>
                <li>Royal Ontario Museum</li>
                <li>The Vancouver Art Gallery</li>
            </ul>
        </section>
        </body></html>"#
    }

    #[test]
    fn parse_california() {
        let results = parse_page(sample_html());
        let ca: Vec<_> = results
            .iter()
            .filter(|i| i.region == "CA" && i.country == "US")
            .collect();
        assert_eq!(ca.len(), 6);
        assert_eq!(ca[0].name, "Crocker Art Museum");
        assert_eq!(ca[0].city, "Sacramento");
    }

    #[test]
    fn parse_city_from_entry() {
        let results = parse_page(sample_html());
        let moca = results
            .iter()
            .find(|i| i.name == "The Museum of Contemporary Art")
            .unwrap();
        assert_eq!(moca.city, "Los Angeles");
        assert_eq!(moca.region, "CA");
    }

    #[test]
    fn parse_no_split_bampfa() {
        let results = parse_page(sample_html());
        let bampfa = results.iter().find(|i| i.name.contains("BAMPFA")).unwrap();
        assert!(bampfa.name.contains("UC Berkeley"));
        assert!(bampfa.name.contains("Pacific Film Archive"));
    }

    #[test]
    fn parse_colorado() {
        let results = parse_page(sample_html());
        let co: Vec<_> = results.iter().filter(|i| i.region == "CO").collect();
        assert_eq!(co.len(), 1);
        assert_eq!(co[0].city, "Colorado Springs");
    }

    #[test]
    fn parse_new_york_with_city() {
        let results = parse_page(sample_html());
        let ak = results
            .iter()
            .find(|i| i.name.contains("Albright-Knox"))
            .unwrap();
        assert_eq!(ak.city, "Buffalo");
        assert_eq!(ak.region, "NY");
    }

    #[test]
    fn parse_canada_ontario_exclusion() {
        let results = parse_page(sample_html());
        let ago = results
            .iter()
            .find(|i| i.name == "Art Gallery of Ontario")
            .unwrap();
        assert_eq!(ago.country, "CA");
        assert_eq!(ago.region, "ON");
        assert_eq!(ago.exclusion_radius_mi, Some(93.0));
        assert!(!ago.name.contains('*'));
    }

    #[test]
    fn parse_canada_non_ontario() {
        let results = parse_page(sample_html());
        let van = results
            .iter()
            .find(|i| i.name == "The Vancouver Art Gallery")
            .unwrap();
        assert_eq!(van.country, "CA");
        assert_eq!(van.region, "BC");
        assert_eq!(van.exclusion_radius_mi, None);
    }

    #[test]
    fn parse_canada_quebec() {
        let results = parse_page(sample_html());
        let mtl = results
            .iter()
            .find(|i| i.name == "The Montreal Museum of Fine Arts")
            .unwrap();
        assert_eq!(mtl.country, "CA");
        assert_eq!(mtl.region, "QC");
        assert_eq!(mtl.exclusion_radius_mi, None);
    }

    #[test]
    fn all_entries_have_network() {
        let results = parse_page(sample_html());
        for r in &results {
            assert_eq!(r.network, "marp");
        }
    }

    #[test]
    fn total_count() {
        let results = parse_page(sample_html());
        // 6 CA + 1 CO + 4 NY + 5 Canada = 16
        assert_eq!(results.len(), 16);
    }

    #[test]
    fn test_parse_museum_entry() {
        let (name, city) = parse_museum_entry("The Museum of Contemporary Art, Los Angeles");
        assert_eq!(name, "The Museum of Contemporary Art");
        assert_eq!(city, "Los Angeles");

        let (name, city) = parse_museum_entry("Crocker Art Museum");
        assert_eq!(name, "Crocker Art Museum");
        assert_eq!(city, "");

        let (name, city) =
            parse_museum_entry("UC Berkeley Art Museum and Pacific Film Archive (BAMPFA)");
        assert!(name.contains("BAMPFA"));
        assert_eq!(city, "");

        let (name, city) = parse_museum_entry("Wexner Center for the Arts, Columbus");
        assert_eq!(name, "Wexner Center for the Arts");
        assert_eq!(city, "Columbus");
    }
}
