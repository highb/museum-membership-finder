//! ASTC (Association of Science and Technology Centers) Passport Program scraper.
//!
//! Scrapes <https://myastc.astc.org/passport-program-search> which is an ASP.NET
//! WebForms app. An empty search returns all ~372 participants, paginated
//! 100/page across 4 pages. Page 1 is triggered by POSTing the search button's
//! event target; pages 2-4 by POSTing the pager link event targets, carrying
//! forward __VIEWSTATE/__VIEWSTATEGENERATOR/__EVENTVALIDATION from the previous
//! response each time.

use super::ScrapedInstitution;
use anyhow::{Context, Result};
use regex::Regex;
use scraper::{Html, Selector};

const URL: &str = "https://myastc.astc.org/passport-program-search";
const SEARCH_TARGET: &str = "main$content$Pnlcdfdab11e7324253838d1b7687182116$ctlQuery$btnSearch";
const PAGER_PREFIX: &str =
    "main$content$Pnlcdfdab11e7324253838d1b7687182116$ctlListControl$lnkPager";

/// Map full US state/territory/Canadian province name -> 2-letter code.
pub(crate) fn state_code(name: &str) -> Option<&'static str> {
    let n = name.trim();
    match n {
        "Alabama" => Some("AL"),
        "Alaska" => Some("AK"),
        "Arizona" => Some("AZ"),
        "Arkansas" => Some("AR"),
        "California" => Some("CA"),
        "Colorado" => Some("CO"),
        "Connecticut" => Some("CT"),
        "Delaware" => Some("DE"),
        "Florida" => Some("FL"),
        "Georgia" => Some("GA"),
        "Hawaii" => Some("HI"),
        "Idaho" => Some("ID"),
        "Illinois" => Some("IL"),
        "Indiana" => Some("IN"),
        "Iowa" => Some("IA"),
        "Kansas" => Some("KS"),
        "Kentucky" => Some("KY"),
        "Louisiana" => Some("LA"),
        "Maine" => Some("ME"),
        "Maryland" => Some("MD"),
        "Massachusetts" => Some("MA"),
        "Michigan" => Some("MI"),
        "Minnesota" => Some("MN"),
        "Mississippi" => Some("MS"),
        "Missouri" => Some("MO"),
        "Montana" => Some("MT"),
        "Nebraska" => Some("NE"),
        "Nevada" => Some("NV"),
        "New Hampshire" => Some("NH"),
        "New Jersey" => Some("NJ"),
        "New Mexico" => Some("NM"),
        "New York" => Some("NY"),
        "North Carolina" => Some("NC"),
        "North Dakota" => Some("ND"),
        "Ohio" => Some("OH"),
        "Oklahoma" => Some("OK"),
        "Oregon" => Some("OR"),
        "Pennsylvania" => Some("PA"),
        "Rhode Island" => Some("RI"),
        "South Carolina" => Some("SC"),
        "South Dakota" => Some("SD"),
        "Tennessee" => Some("TN"),
        "Texas" => Some("TX"),
        "Utah" => Some("UT"),
        "Vermont" => Some("VT"),
        "Virginia" => Some("VA"),
        "Washington" => Some("WA"),
        "West Virginia" => Some("WV"),
        "Wisconsin" => Some("WI"),
        "Wyoming" => Some("WY"),
        // DC & territories
        "District of Columbia" => Some("DC"),
        "Puerto Rico" => Some("PR"),
        "Virgin Islands" | "U.S. Virgin Islands" => Some("VI"),
        "Guam" => Some("GU"),
        "American Samoa" => Some("AS"),
        "Northern Mariana Islands" => Some("MP"),
        // Canadian provinces
        "Alberta" => Some("AB"),
        "British Columbia" => Some("BC"),
        "Manitoba" => Some("MB"),
        "New Brunswick" => Some("NB"),
        "Newfoundland and Labrador" | "Newfoundland" => Some("NL"),
        "Nova Scotia" => Some("NS"),
        "Ontario" => Some("ON"),
        "Prince Edward Island" => Some("PE"),
        "Quebec" | "Québec" => Some("QC"),
        "Saskatchewan" => Some("SK"),
        "Northwest Territories" => Some("NT"),
        "Nunavut" => Some("NU"),
        "Yukon" => Some("YT"),
        _ => None,
    }
}

/// Extract a hidden input field value by name from raw page HTML.
fn hidden_field(html: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"name="{}"[^>]*value="([^"]*)""#, regex::escape(name));
    let re = Regex::new(&pattern).ok()?;
    re.captures(html).map(|c| c[1].to_string())
}

/// Build the POST form fields from the previous page's HTML, overriding
/// __EVENTTARGET to trigger the given postback (search button or pager link).
fn build_form_fields(page_html: &str, event_target: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    for name in [
        "__BROWSERID",
        "__ADMINSITE",
        "__NAVLINKID",
        "__READONLY",
        "__VIEWSTATE",
        "__VIEWSTATEGENERATOR",
        "__EVENTVALIDATION",
    ] {
        if let Some(val) = hidden_field(page_html, name) {
            fields.push((name.to_string(), val));
        }
    }
    fields.push(("__EVENTTARGET".to_string(), event_target.to_string()));
    fields.push(("__EVENTARGUMENT".to_string(), String::new()));
    fields
}

/// ASTC uses a non-standard spelling for Hawaii.
fn normalize_state_name(raw: &str) -> String {
    let s = raw.trim();
    match s {
        "Hawai'i" | "Hawai\u{2018}i" | "Hawai\u{2019}i" => "Hawaii".to_string(),
        other => other.to_string(),
    }
}

/// Country-name headers ASTC uses for international entries.
fn country_code_for_header(name: &str) -> Option<&'static str> {
    match name {
        "Australia" => Some("AU"),
        "Bermuda" => Some("BM"),
        "Canada" => Some("CA"),
        "Czech Republic" => Some("CZ"),
        "Israel" => Some("IL"),
        "Malaysia" => Some("MY"),
        "Mexico" => Some("MX"),
        "Philippines" => Some("PH"),
        "Singapore" => Some("SG"),
        "South Korea" => Some("KR"),
        "United Kingdom" => Some("GB"),
        _ => None,
    }
}

/// Extract a Canadian province code from an address line like
/// "1867 St. Laurent Boulevard, Ottawa, Ontario  K1G 5A3".
fn extract_canadian_province(address: &str) -> Option<&'static str> {
    const PROVINCES: &[&str] = &[
        "Alberta",
        "British Columbia",
        "Manitoba",
        "New Brunswick",
        "Newfoundland and Labrador",
        "Newfoundland",
        "Nova Scotia",
        "Ontario",
        "Prince Edward Island",
        "Quebec",
        "Québec",
        "Saskatchewan",
        "Northwest Territories",
        "Nunavut",
        "Yukon",
    ];
    PROVINCES
        .iter()
        .find(|prov| address.contains(*prov))
        .and_then(|prov| state_code(prov))
}

/// Extract city from an address like "800 Museum Dr, Anniston, AL 36206" —
/// the city is the second-to-last comma-separated segment.
fn extract_city(address: &str) -> String {
    let parts: Vec<&str> = address.split(',').collect();
    if parts.len() >= 2 {
        parts[parts.len() - 2].trim().to_string()
    } else {
        String::new()
    }
}

/// Parse all institution entries from one page of HTML.
pub fn parse_page(html: &str) -> Vec<ScrapedInstitution> {
    let doc = Html::parse_document(html);
    let li_sel = Selector::parse("li.list-result").unwrap();
    let h5_sel = Selector::parse("h5.pass").unwrap();
    let h3_sel = Selector::parse("h3.pass").unwrap();
    let div_sel = Selector::parse("div.passp").unwrap();
    let a_sel = Selector::parse("a").unwrap();

    let mut results = Vec::new();

    for li in doc.select(&li_sel) {
        let state_header = match li.select(&h5_sel).next() {
            Some(el) => el.text().collect::<String>().trim().to_string(),
            None => continue,
        };

        let name = match li.select(&h3_sel).next() {
            Some(el) => el.text().collect::<String>().trim().to_string(),
            None => continue,
        };
        if name.is_empty() {
            continue;
        }

        let mut address = String::new();
        let mut phone = None;
        let mut website = None;
        let mut individual_memberships = String::new();
        let mut group_memberships = String::new();
        let mut proof_of_residence = false;

        for div_el in li.select(&div_sel) {
            let text = div_el.text().collect::<String>().trim().to_string();

            if let Some(a) = div_el.select(&a_sel).next() {
                if let Some(href) = a.value().attr("href") {
                    if href.starts_with("http") {
                        website = Some(href.to_string());
                        continue;
                    }
                }
            }

            if let Some(rest) = text.strip_prefix("Individual Membership(s):") {
                individual_memberships = rest.trim().to_string();
            } else if let Some(rest) = text.strip_prefix("Group Membership(s):") {
                group_memberships = rest.trim().to_string();
            } else if let Some(rest) = text.strip_prefix("Proof of Residence Required:") {
                proof_of_residence = rest.trim().eq_ignore_ascii_case("yes");
            } else if text.trim() == "United States"
                || [
                    "Canada",
                    "Australia",
                    "Bermuda",
                    "Czech Republic",
                    "Israel",
                    "Malaysia",
                    "Philippines",
                    "Singapore",
                    "South Korea",
                    "Mexico",
                    "United Kingdom",
                ]
                .contains(&text.trim())
            {
                // country line; already implied by state_header, nothing to store
            } else if text.starts_with('(') || text.starts_with('+') {
                phone = Some(text.clone());
            } else if address.is_empty() && !text.is_empty() {
                address = text;
            }
        }

        let normalized = normalize_state_name(&state_header);
        let (region, country) = if let Some(cc) = country_code_for_header(&normalized) {
            if normalized == "Canada" {
                let prov = extract_canadian_province(&address).unwrap_or("ON");
                (prov.to_string(), "CA".to_string())
            } else {
                (String::new(), cc.to_string())
            }
        } else {
            let code = state_code(&normalized).unwrap_or("").to_string();
            (code, "US".to_string())
        };

        let city = extract_city(&address);

        let mut inst = ScrapedInstitution::new(name, city, region, "astc");
        inst.country = country;
        inst.website = website;
        inst.phone = phone;
        if proof_of_residence {
            inst.extra
                .insert("proof_of_residence".to_string(), "true".to_string());
        }
        if !individual_memberships.is_empty() {
            inst.extra
                .insert("individual_memberships".to_string(), individual_memberships);
        }
        if !group_memberships.is_empty() {
            inst.extra
                .insert("group_memberships".to_string(), group_memberships);
        }

        results.push(inst);
    }

    results
}

/// Count how many pager links exist on the page (lnkPager0, lnkPager1, ...).
fn count_pager_links(html: &str) -> usize {
    let re = Regex::new(r"lnkPager(\d+)").unwrap();
    let mut max_idx: Option<usize> = None;
    for cap in re.captures_iter(html) {
        if let Ok(n) = cap[1].parse::<usize>() {
            max_idx = Some(max_idx.map_or(n, |m: usize| m.max(n)));
        }
    }
    max_idx.map_or(0, |m| m + 1)
}

pub fn scrape() -> Result<Vec<ScrapedInstitution>> {
    let client = super::http_client()?;

    eprintln!("[astc] GET {URL}");
    let initial_html = client
        .get(URL)
        .send()
        .context("ASTC initial GET failed")?
        .text()?;

    eprintln!("[astc] POST search (page 1)");
    let fields = build_form_fields(&initial_html, SEARCH_TARGET);
    let page1_html = client
        .post(URL)
        .form(&fields)
        .send()
        .context("ASTC search POST failed")?
        .text()?;

    let mut all = parse_page(&page1_html);
    eprintln!("[astc] page 1: {} institutions", all.len());

    let num_pager_links = count_pager_links(&page1_html);
    eprintln!("[astc] found {num_pager_links} pager links");

    let mut prev_html = page1_html;
    for pager_idx in 0..num_pager_links {
        let target = format!("{PAGER_PREFIX}{pager_idx}");
        eprintln!("[astc] POST page {} (target: {target})", pager_idx + 2);

        let fields = build_form_fields(&prev_html, &target);
        let html = client
            .post(URL)
            .form(&fields)
            .send()
            .with_context(|| format!("ASTC page {} POST failed", pager_idx + 2))?
            .text()?;

        let page_results = parse_page(&html);
        eprintln!(
            "[astc] page {}: {} institutions",
            pager_idx + 2,
            page_results.len()
        );
        all.extend(page_results);
        prev_html = html;
    }

    eprintln!("[astc] total: {} institutions", all.len());
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_html() -> &'static str {
        r#"<html><body>
        <ul class="list-results">
            <li class="list-result">
                <h5 class="pass"><span><b>Alabama</b></span></h5>
                <h3 class="pass"><span><b>Anniston Museums and Gardens</b></span></h3>
                <div class="passp"><span>800 Museum Dr, Anniston, AL 36206</span></div>
                <div class="passp"><span> United States</span></div>
                <div class="passp"><span>(256) 237-6766</span></div>
                <div class="passp"><span><a href="https://www.exploreamag.org/" title="Website">https://www.exploreamag.org/</a></span></div>
                <div class="passp"><span><b>Individual Membership(s):</b> Individual Level</span></div>
                <div class="passp"><span><b>Group Membership(s):</b> Family Level, Patron Level</span></div>
                <div class="passp"><span><b>Proof of Residence Required:</b> No</span></div>
            </li>
            <li class="list-result">
                <h5 class="pass"><span><b>Alabama</b></span></h5>
                <h3 class="pass"><span><b>Exploreum Science Center</b></span></h3>
                <div class="passp"><span>65 Government Street, Mobile, AL 36602</span></div>
                <div class="passp"><span> United States</span></div>
                <div class="passp"><span>(251) 208-6893</span></div>
                <div class="passp"><span><a href="https://www.exploreum.com/">https://www.exploreum.com/</a></span></div>
                <div class="passp"><span><b>Individual Membership(s):</b></span></div>
                <div class="passp"><span><b>Group Membership(s):</b> Mini Plus, Standard Family</span></div>
                <div class="passp"><span><b>Proof of Residence Required:</b> Yes</span></div>
            </li>
            <li class="list-result">
                <h5 class="pass"><span><b>Canada</b></span></h5>
                <h3 class="pass"><span><b>Science World</b></span></h3>
                <div class="passp"><span>1455 Quebec St, Vancouver, British Columbia  V6A 3Z7</span></div>
                <div class="passp"><span> Canada</span></div>
                <div class="passp"><span>(604) 443-7440</span></div>
                <div class="passp"><span><a href="https://www.scienceworld.ca/">https://www.scienceworld.ca/</a></span></div>
                <div class="passp"><span><b>Individual Membership(s):</b></span></div>
                <div class="passp"><span><b>Group Membership(s):</b> Family</span></div>
                <div class="passp"><span><b>Proof of Residence Required:</b> No</span></div>
            </li>
            <li class="list-result">
                <h5 class="pass"><span><b>Hawai'i</b></span></h5>
                <h3 class="pass"><span><b>Imiloa Astronomy Center</b></span></h3>
                <div class="passp"><span>600 Imiloa Pl, Hilo, HI 96720</span></div>
                <div class="passp"><span> United States</span></div>
                <div class="passp"><span>(808) 932-8901</span></div>
                <div class="passp"><span><a href="https://imiloahawaii.org/">https://imiloahawaii.org/</a></span></div>
                <div class="passp"><span><b>Individual Membership(s):</b> Basic</span></div>
                <div class="passp"><span><b>Group Membership(s):</b></span></div>
                <div class="passp"><span><b>Proof of Residence Required:</b> No</span></div>
            </li>
            <li class="list-result">
                <h5 class="pass"><span><b>Australia</b></span></h5>
                <h3 class="pass"><span><b>Questacon</b></span></h3>
                <div class="passp"><span>King Edward Terrace, Canberra, ACT 02600</span></div>
                <div class="passp"><span> Australia</span></div>
                <div class="passp"><span>+61 262702800</span></div>
                <div class="passp"><span><a href="https://www.questacon.edu.au/">https://www.questacon.edu.au/</a></span></div>
                <div class="passp"><span><b>Individual Membership(s):</b></span></div>
                <div class="passp"><span><b>Group Membership(s):</b> Family</span></div>
                <div class="passp"><span><b>Proof of Residence Required:</b> No</span></div>
            </li>
        </ul>
        </body></html>"#
    }

    #[test]
    fn parse_us_entry() {
        let results = parse_page(sample_html());
        assert!(results.len() >= 2);

        let anniston = &results[0];
        assert_eq!(anniston.name, "Anniston Museums and Gardens");
        assert_eq!(anniston.region, "AL");
        assert_eq!(anniston.country, "US");
        assert_eq!(anniston.city, "Anniston");
        assert_eq!(anniston.network, "astc");
        assert_eq!(
            anniston.website.as_deref(),
            Some("https://www.exploreamag.org/")
        );
        assert_eq!(anniston.phone.as_deref(), Some("(256) 237-6766"));
        assert!(anniston.extra.get("proof_of_residence").is_none());
    }

    #[test]
    fn parse_proof_of_residence() {
        let results = parse_page(sample_html());
        let exploreum = &results[1];
        assert_eq!(exploreum.name, "Exploreum Science Center");
        assert_eq!(
            exploreum
                .extra
                .get("proof_of_residence")
                .map(|s| s.as_str()),
            Some("true")
        );
    }

    #[test]
    fn parse_canadian_entry() {
        let results = parse_page(sample_html());
        let sw = results.iter().find(|i| i.name == "Science World").unwrap();
        assert_eq!(sw.region, "BC");
        assert_eq!(sw.country, "CA");
        assert_eq!(sw.city, "Vancouver");
    }

    #[test]
    fn parse_hawaii_entry() {
        let results = parse_page(sample_html());
        let imiloa = results
            .iter()
            .find(|i| i.name == "Imiloa Astronomy Center")
            .unwrap();
        assert_eq!(imiloa.region, "HI");
        assert_eq!(imiloa.country, "US");
    }

    #[test]
    fn parse_international_entry() {
        let results = parse_page(sample_html());
        let q = results.iter().find(|i| i.name == "Questacon").unwrap();
        assert_eq!(q.country, "AU");
        assert_eq!(q.region, "");
    }

    #[test]
    fn no_exclusion_fields_set() {
        // ASTC's 90-mile exclusion is handled at the network level, not per-institution.
        let results = parse_page(sample_html());
        for r in &results {
            assert_eq!(r.exclusion_radius_mi, None);
            assert!(r.exclusion_flags.is_empty());
        }
    }

    #[test]
    fn count_results() {
        let results = parse_page(sample_html());
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_extract_city() {
        assert_eq!(
            extract_city("800 Museum Dr, Anniston, AL 36206"),
            "Anniston"
        );
        assert_eq!(
            extract_city("1867 St. Laurent Boulevard, Ottawa, Ontario  K1G 5A3"),
            "Ottawa"
        );
    }

    #[test]
    fn test_extract_canadian_province() {
        assert_eq!(
            extract_canadian_province("1455 Quebec St, Vancouver, British Columbia  V6A 3Z7"),
            Some("BC")
        );
        assert_eq!(
            extract_canadian_province("1867 St. Laurent Boulevard, Ottawa, Ontario  K1G 5A3"),
            Some("ON")
        );
        assert_eq!(
            extract_canadian_province("2903 Powerhouse Dr, Regina, Saskatchewan  S4N 0A1"),
            Some("SK")
        );
    }

    #[test]
    fn test_normalize_state_name() {
        assert_eq!(normalize_state_name("Hawai\u{2019}i"), "Hawaii");
        assert_eq!(normalize_state_name("Hawai'i"), "Hawaii");
        assert_eq!(normalize_state_name("California"), "California");
    }

    #[test]
    fn test_state_code_all_50_plus_dc() {
        let states = [
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
        ];
        for s in states {
            assert!(state_code(s).is_some(), "missing state_code for {s}");
        }
    }

    #[test]
    fn test_state_code_canadian_provinces() {
        let provinces = [
            "Alberta",
            "British Columbia",
            "Manitoba",
            "New Brunswick",
            "Newfoundland and Labrador",
            "Nova Scotia",
            "Ontario",
            "Prince Edward Island",
            "Quebec",
            "Saskatchewan",
            "Northwest Territories",
            "Nunavut",
            "Yukon",
        ];
        for p in provinces {
            assert!(state_code(p).is_some(), "missing state_code for {p}");
        }
    }

    #[test]
    fn test_state_code_unknown() {
        assert_eq!(state_code("Atlantis"), None);
    }
}
