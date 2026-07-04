//! NARM (North American Reciprocal Museum) association scraper.
//!
//! Source is a PDF laid out in two side-by-side columns per page. We shell
//! out to `pdftotext -layout` so that both columns land on the same physical
//! text line, separated by a wide run of whitespace, then split each line on
//! that gap to recover a single logical column of `City, Name[flags], Phone`
//! rows (or `Province, City, Name[flags], Phone` inside the Canada section).

use super::ScrapedInstitution;
use anyhow::Result;
use regex::Regex;

const PDF_URL: &str = "https://narmassociation.org/wp-content/uploads/2026/06/NARM_SUMMER_2026.pdf";

pub fn scrape() -> Result<Vec<ScrapedInstitution>> {
    let text = super::download_pdf_text(PDF_URL, "narm")?;
    let institutions = parse_text(&text)?;
    eprintln!("[narm] parsed {} institutions", institutions.len());
    Ok(institutions)
}

/// Which section of the document we're currently reading rows from.
#[derive(Debug, Clone)]
struct Section {
    country: String,
    region: String,
    /// Rows are `Province, City, Name, Phone` instead of `City, Name, Phone`.
    canada_row: bool,
    /// Rows are `Name, Phone` (city is fixed, taken from the header itself).
    dc_city: Option<String>,
    /// False until we've actually seen a concrete state/section header, so we
    /// don't emit anything from stray preamble text.
    ready: bool,
}

impl Section {
    fn none() -> Self {
        Self {
            country: "US".to_string(),
            region: String::new(),
            canada_row: false,
            dc_city: None,
            ready: false,
        }
    }

    /// Minimum number of commas a combined fragment needs before it can be a
    /// complete record: City,Name,Phone (2) / Province,City,Name,Phone (3) /
    /// Name,Phone for the DC block (1).
    fn comma_threshold(&self) -> usize {
        if self.canada_row {
            3
        } else if self.dc_city.is_some() {
            1
        } else {
            2
        }
    }

    fn prefix_fields(&self) -> usize {
        if self.canada_row {
            2
        } else if self.dc_city.is_some() {
            0
        } else {
            1
        }
    }
}

const SKIP_SUBSTRINGS: &[&str] = &[
    "NARM privileges",
    "narmassociation.org",
    "Preferred Services Providers",
    "©",
    "All rights reserved",
    "This list is updated",
    "organizations occur",
    "search the NARM map",
    "Members from one of",
    "member institutions listed",
    "accepted NARM identification",
    "Free/member admission",
    "Member discounts",
    "Guests are not included",
    "NOTE: Some museums",
    "'Family' benefits",
    "It is always advisable",
    "benefits available",
    "North American Reciprocal",
    "Museum (NARM)",
    "Association\u{ae} Members",
    "Summer 20",
    "mid-December",
    "mid-March",
    "mid-June",
    "mid-September",
    "Reciprocal Program",
    "membership department",
    "For questions about",
    "Restrictions",
];

fn is_skip_line(seg: &str) -> bool {
    if seg.is_empty() {
        return true;
    }
    if seg.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if seg.starts_with('•') {
        return true;
    }
    SKIP_SUBSTRINGS.iter().any(|s| seg.contains(s))
}

/// Recognize a segment that starts a new document section, returning the
/// `Section` state to use for subsequent rows. Returns `None` if `seg` is an
/// ordinary data row rather than a header.
fn match_section_header(seg: &str) -> Option<Section> {
    match seg {
        "Bermuda" => {
            return Some(Section {
                country: "BM".to_string(),
                region: String::new(),
                canada_row: false,
                dc_city: None,
                ready: true,
            });
        }
        "Canada" => {
            return Some(Section {
                country: "CA".to_string(),
                region: String::new(),
                canada_row: true,
                dc_city: None,
                ready: true,
            });
        }
        "Cayman Islands" => {
            return Some(Section {
                country: "KY".to_string(),
                region: String::new(),
                canada_row: false,
                dc_city: None,
                ready: true,
            });
        }
        "Puerto Rico" => {
            return Some(Section {
                country: "US".to_string(),
                region: "PR".to_string(),
                canada_row: false,
                dc_city: None,
                ready: true,
            });
        }
        "United States" => return Some(Section::none()),
        _ => {}
    }
    if let Some(city) = seg.strip_prefix("District of Columbia,") {
        return Some(Section {
            country: "US".to_string(),
            region: "DC".to_string(),
            canada_row: false,
            dc_city: Some(city.trim().to_string()),
            ready: true,
        });
    }
    if let Some(code) = super::state_code(seg) {
        return Some(Section {
            country: "US".to_string(),
            region: code.to_string(),
            canada_row: false,
            dc_city: None,
            ready: true,
        });
    }
    None
}

/// Strip trailing NARM restriction-flag symbols from a name and translate
/// them into the shared `ScrapedInstitution` fields.
fn apply_flags(raw_name: &str) -> (String, Vec<String>, bool, Option<f64>) {
    let trimmed = raw_name.trim();
    let flag_start = trimmed
        .rfind(|c: char| !matches!(c, '*' | '#' | '^'))
        .map(|i| i + 1)
        .unwrap_or(0);
    let name = trimmed[..flag_start].trim().to_string();
    let suffix = &trimmed[flag_start..];

    let stars = suffix.matches('*').count();
    let hashes = suffix.matches('#').count();
    let caret = suffix.contains('^');

    let mut flags = Vec::new();
    let mut special_exhibit_restricted = false;
    let mut exclusion_radius_mi = None;

    if stars >= 3 {
        special_exhibit_restricted = true;
        exclusion_radius_mi = Some(15.0);
        flags.push("***".to_string());
    } else if stars == 2 {
        exclusion_radius_mi = Some(15.0);
        flags.push("**".to_string());
    } else if stars == 1 {
        special_exhibit_restricted = true;
        flags.push("*".to_string());
    }

    if hashes >= 2 {
        flags.push("##".to_string());
    } else if hashes == 1 {
        exclusion_radius_mi = exclusion_radius_mi.or(Some(50.0));
        flags.push("#".to_string());
    }

    if caret {
        flags.push("^".to_string());
    }

    (name, flags, special_exhibit_restricted, exclusion_radius_mi)
}

fn build_institution(combined: &str, section: &Section) -> Option<ScrapedInstitution> {
    let fields: Vec<&str> = combined.split(',').map(|s| s.trim()).collect();
    let prefix_fields = section.prefix_fields();
    if fields.len() < prefix_fields + 2 {
        return None;
    }
    let phone = fields.last().unwrap().to_string();
    if phone.is_empty() {
        return None;
    }
    let name_fields = &fields[prefix_fields..fields.len() - 1];
    let raw_name = name_fields.join(", ");

    let (city, region, country) = if section.canada_row {
        (
            fields[1].to_string(),
            fields[0].to_uppercase(),
            "CA".to_string(),
        )
    } else if let Some(dc_city) = &section.dc_city {
        (dc_city.clone(), "DC".to_string(), "US".to_string())
    } else {
        (
            fields[0].to_string(),
            section.region.clone(),
            section.country.clone(),
        )
    };

    if city.is_empty() || raw_name.is_empty() {
        return None;
    }

    let (name, flags, special_exhibit_restricted, exclusion_radius_mi) = apply_flags(&raw_name);
    if name.is_empty() {
        return None;
    }

    let mut inst = ScrapedInstitution::new(name, city, region, "narm");
    inst.country = country;
    inst.phone = Some(phone);
    inst.exclusion_flags = flags;
    inst.special_exhibit_restricted = special_exhibit_restricted;
    inst.exclusion_radius_mi = exclusion_radius_mi;
    Some(inst)
}

pub fn parse_text(text: &str) -> Result<Vec<ScrapedInstitution>> {
    let gap = Regex::new(r"\s{4,}").unwrap();
    let mut section = Section::none();
    let mut pending = String::new();
    let mut out = Vec::new();

    for line in text.lines() {
        for raw_seg in gap.split(line) {
            let seg = raw_seg.trim();
            if is_skip_line(seg) {
                continue;
            }
            if let Some(new_section) = match_section_header(seg) {
                section = new_section;
                pending.clear();
                continue;
            }
            if !section.ready {
                continue;
            }

            let combined = if pending.is_empty() {
                seg.to_string()
            } else {
                format!("{pending} {seg}")
            };

            let commas = combined.matches(',').count();
            let last_field_nonempty = combined
                .rsplit(',')
                .next()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);

            if commas >= section.comma_threshold() && last_field_nonempty {
                if let Some(inst) = build_institution(&combined, &section) {
                    out.push(inst);
                }
                pending.clear();
            } else {
                pending = combined;
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_us_entries() {
        let text = "\
                                                                          United States
                                                                    Alabama
Birmingham, Birmingham Museum of Art, 205-254-2565
Birmingham, Vulcan Park and Museum, 205-203-4822
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "Birmingham Museum of Art");
        assert_eq!(out[0].city, "Birmingham");
        assert_eq!(out[0].region, "AL");
        assert_eq!(out[0].country, "US");
        assert_eq!(out[0].network, "narm");
        assert_eq!(out[0].phone.as_deref(), Some("205-254-2565"));
        assert!(out[0].exclusion_flags.is_empty());
        assert!(!out[0].special_exhibit_restricted);
    }

    #[test]
    fn strips_and_classifies_flags() {
        let text = "\
                                                                    Arizona
Phoenix, Heard Museum*, 602-252-8848
La Jolla, Museum of Contemporary Art San Diego**, 858-454-3541
Los Angeles, Academy Museum of Motion Pictures***, 323-930-3000
Berkeley, UC Berkeley Art Museum##, 510-642-0808
Jamestown, Railtown 1897 State Historic Park^, 209-984-3953
Sacramento, The California Museum*^, 916-653-7524
St. Petersburg, The Dal\u{ed} Museum#, 727-823-3767
";
        let out = parse_text(text).unwrap();
        let by_name = |n: &str| out.iter().find(|i| i.name == n).unwrap();

        let heard = by_name("Heard Museum");
        assert!(heard.special_exhibit_restricted);
        assert_eq!(heard.exclusion_radius_mi, None);
        assert_eq!(heard.exclusion_flags, vec!["*"]);

        let mcasd = by_name("Museum of Contemporary Art San Diego");
        assert!(!mcasd.special_exhibit_restricted);
        assert_eq!(mcasd.exclusion_radius_mi, Some(15.0));
        assert_eq!(mcasd.exclusion_flags, vec!["**"]);

        let academy = by_name("Academy Museum of Motion Pictures");
        assert!(academy.special_exhibit_restricted);
        assert_eq!(academy.exclusion_radius_mi, Some(15.0));
        assert_eq!(academy.exclusion_flags, vec!["***"]);

        let berkeley = by_name("UC Berkeley Art Museum");
        assert_eq!(berkeley.exclusion_flags, vec!["##"]);

        let railtown = by_name("Railtown 1897 State Historic Park");
        assert_eq!(railtown.exclusion_flags, vec!["^"]);

        let ca_museum = by_name("The California Museum");
        assert!(ca_museum.special_exhibit_restricted);
        assert_eq!(ca_museum.exclusion_flags, vec!["*", "^"]);

        let dali = by_name("The Dal\u{ed} Museum");
        assert_eq!(dali.exclusion_radius_mi, Some(50.0));
        assert_eq!(dali.exclusion_flags, vec!["#"]);
    }

    #[test]
    fn parses_canadian_province_rows() {
        let text = "\
                                            Canada
AB, Edmonton, Art Gallery of Alberta, 780-422-6223
BC, Kamloops, Kamloops Art Gallery, 250-377-2400
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].city, "Edmonton");
        assert_eq!(out[0].region, "AB");
        assert_eq!(out[0].country, "CA");
        assert_eq!(out[0].name, "Art Gallery of Alberta");
    }

    #[test]
    fn parses_bermuda_cayman_and_puerto_rico() {
        let text = "\
                                            Bermuda
Devonshire, The Masterworks Museum of Bermuda Art, 441-236-2950
                                       Cayman Islands
George Town, Cayman Islands National Museum, 011-677-345-925-7621
                                         Puerto Rico
San Juan, MUSAN | Museo de los Santos y Arte Nacional, 787-455-4216
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].country, "BM");
        assert_eq!(out[1].country, "KY");
        assert_eq!(out[2].country, "US");
        assert_eq!(out[2].region, "PR");
    }

    #[test]
    fn handles_wrapped_multiline_entries() {
        let text = "\
                                            Canada
ON, Kenora, The Muse | Lake of the Woods Museum & Douglas Family Art Centre,
 807-467-2202
ON, Kleinburg, McMichael Canadian Art Collection^, 905-893-1121
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].name,
            "The Muse | Lake of the Woods Museum & Douglas Family Art Centre"
        );
        assert_eq!(out[0].phone.as_deref(), Some("807-467-2202"));
        assert_eq!(out[0].city, "Kenora");
        assert_eq!(out[0].region, "ON");
    }

    #[test]
    fn handles_dc_special_format() {
        let text = "\
                                            United States
     District of Columbia, Washington
The American University Museum, 202-885-1300
Dumbarton House, 202-337-2288
                                                                    Florida
Anna Maria, Anna Maria Island Historical Museum, 941-778-0492
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].city, "Washington");
        assert_eq!(out[0].region, "DC");
        assert_eq!(out[0].name, "The American University Museum");
        assert_eq!(out[1].city, "Washington");
        assert_eq!(out[1].name, "Dumbarton House");
        assert_eq!(out[2].region, "FL");
        assert_eq!(out[2].city, "Anna Maria");
    }

    #[test]
    fn skips_preamble_and_footer_noise() {
        let text = "\
North American Reciprocal
Museum (NARM)
Association\u{ae} Members
Summer 2026
This list is updated quarterly in mid-December, mid-March, mid-June and
                                                                    Alabama
Birmingham, Birmingham Museum of Art, 205-254-2565
                          1          Restrictions
                                    *NARM privileges may be restricted for concerts/lectures/special exhibitions and ticketed events.
                                                                                                                                                                 https://narmassociation.org
                                                                                                                                                                 Preferred Services Providers
                                                                                                                               \u{a9}2026 North American Reciprocal Museum (NARM) Association\u{ae} All rights reserved in all media.
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Birmingham Museum of Art");
    }
}
