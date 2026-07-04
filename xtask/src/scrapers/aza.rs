//! AZA (Association of Zoos & Aquariums) Reciprocity Program scraper.
//!
//! Source is a PDF chart with a main data table (State, City, Zoo/Aquarium,
//! Reciprocity Level + Contact Name, Phone/Email) and an unrelated
//! explanatory sidebar that `pdftotext -layout` interleaves onto the same
//! lines. We reject anything that doesn't parse as a full row with a
//! recognized reciprocity-level prefix, which naturally filters out the
//! sidebar prose without needing an exhaustive blocklist.

use super::ScrapedInstitution;
use anyhow::Result;
use regex::Regex;

const PDF_URL: &str = "https://assets.speakcdn.com/assets/2332/reciprocity_chart.pdf";

pub fn scrape() -> Result<Vec<ScrapedInstitution>> {
    let text = super::download_pdf_text(PDF_URL, "aza")?;
    let institutions = parse_text(&text)?;
    eprintln!("[aza] parsed {} institutions", institutions.len());
    Ok(institutions)
}

const SKIP_SUBSTRINGS: &[&str] = &[
    "Always call the zoo or aquarium",
    "their current policies regarding",
    "reciprocal admissions",
    "PLEASE NOTE",
    "ALWAYS CALL AHEAD",
    "Zoo or Aquarium",
    "Updated ",
];

fn is_skip_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    SKIP_SUBSTRINGS.iter().any(|s| t.contains(s))
}

/// Recognize a State column value: a US state/territory full name, the
/// bare "DC" abbreviation used directly in this chart, or one of the
/// international section markers (CANADA, MEXICO, COLOMBIA).
fn match_state_marker(seg: &str) -> Option<(String, String)> {
    if seg.eq_ignore_ascii_case("DC") {
        return Some(("US".to_string(), "DC".to_string()));
    }
    if let Some(code) = super::state_code(seg) {
        let country = if super::is_canadian(code) { "CA" } else { "US" };
        return Some((country.to_string(), code.to_string()));
    }
    match seg.to_ascii_uppercase().as_str() {
        "CANADA" => Some(("CA".to_string(), String::new())),
        "MEXICO" => Some(("MX".to_string(), String::new())),
        "COLOMBIA" => Some(("CO".to_string(), String::new())),
        _ => None,
    }
}

/// Split a combined "Reciprocity Level + Contact Name" field into its two
/// parts. Recognizes the three documented canonical levels ("50%",
/// "100% OR 50%", "FREE TO PUBLIC"); anything else is left unclassified.
fn split_reciprocity_contact(field: &str) -> (Option<String>, String) {
    let trimmed = field.trim();
    let lower = trimmed.to_lowercase();

    if lower.starts_with("100% or 50%") {
        let rest = trimmed["100% or 50%".len()..].trim().to_string();
        return (Some("100% OR 50%".to_string()), rest);
    }
    if lower.starts_with("free to public") {
        let rest = trimmed["free to public".len()..].trim().to_string();
        return (Some("FREE TO PUBLIC".to_string()), rest);
    }
    let pct_re = Regex::new(r"^(\d+%)\s*(.*)$").unwrap();
    if let Some(caps) = pct_re.captures(trimmed) {
        return (Some(caps[1].to_string()), caps[2].trim().to_string());
    }
    (None, trimmed.to_string())
}

pub fn parse_text(text: &str) -> Result<Vec<ScrapedInstitution>> {
    let gap = Regex::new(r"\s{3,}").unwrap();
    let mut current_country = "US".to_string();
    let mut current_region = String::new();
    let mut out = Vec::new();

    for raw_line in text.lines() {
        if is_skip_line(raw_line) {
            continue;
        }
        let trimmed = raw_line.trim();
        let segs: Vec<&str> = gap
            .split(trimmed)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let mut idx = 0;
        let mut country = current_country.clone();
        let mut region = current_region.clone();
        if let Some((c, r)) = segs.first().and_then(|s| match_state_marker(s)) {
            country = c;
            region = r;
            idx = 1;
        }

        let city = match segs.get(idx) {
            Some(s) => *s,
            None => continue,
        };
        let name = match segs.get(idx + 1) {
            Some(s) => *s,
            None => continue,
        };
        let reciprocity_contact = match segs.get(idx + 2) {
            Some(s) => *s,
            None => continue,
        };

        let (level, contact) = split_reciprocity_contact(reciprocity_contact);
        // A row that doesn't carry a recognized reciprocity level is almost
        // certainly interleaved sidebar prose, not real chart data.
        let level = match level {
            Some(l) => l,
            None => continue,
        };

        current_country = country.clone();
        current_region = region.clone();

        let mut inst = ScrapedInstitution::new(name.to_string(), city.to_string(), region, "aza");
        inst.country = country;
        inst.extra.insert("reciprocity_level".to_string(), level);
        if !contact.is_empty() {
            inst.extra.insert("contact_name".to_string(), contact);
        }
        if let Some(phone_email) = segs.get(idx + 3) {
            if phone_email.contains('@') {
                inst.extra
                    .insert("email".to_string(), phone_email.to_string());
            } else {
                inst.phone = Some(phone_email.to_string());
            }
        }
        out.push(inst);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_rows_with_state_carried_forward() {
        let text = "\
Arizona       Phoenix                  Phoenix Zoo                                                    50% Donor and Member Relations   602-286-3800
              Tempe                    SEA LIFE Arizona Aquarium                                      50% Membership Dept.             arizona@sealifeus.com
              Tucson                   Reid Park Zoo                                                  50% Membership Dept.             520-881-4753
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 3);

        assert_eq!(out[0].city, "Phoenix");
        assert_eq!(out[0].region, "AZ");
        assert_eq!(out[0].country, "US");
        assert_eq!(out[0].extra.get("reciprocity_level").unwrap(), "50%");
        assert_eq!(
            out[0].extra.get("contact_name").unwrap(),
            "Donor and Member Relations"
        );
        assert_eq!(out[0].phone.as_deref(), Some("602-286-3800"));

        assert_eq!(out[1].city, "Tempe");
        assert_eq!(out[1].region, "AZ");
        assert_eq!(out[1].extra.get("email").unwrap(), "arizona@sealifeus.com");
        assert_eq!(out[1].phone, None);

        assert_eq!(out[2].city, "Tucson");
        assert_eq!(out[2].region, "AZ");
    }

    #[test]
    fn parses_100_or_50_and_free_to_public_levels() {
        let text = "\
Idaho      Boise               Zoo Boise                                                   100% OR 50% Morgan Aaron                    208-608-7765
DC            Washington               Smithsonian National Zoological Park                  FREE TO PUBLIC NZP Membership Office      202-633-2922
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].extra.get("reciprocity_level").unwrap(),
            "100% OR 50%"
        );
        assert_eq!(out[0].extra.get("contact_name").unwrap(), "Morgan Aaron");

        assert_eq!(out[1].region, "DC");
        assert_eq!(
            out[1].extra.get("reciprocity_level").unwrap(),
            "FREE TO PUBLIC"
        );
        assert_eq!(
            out[1].extra.get("contact_name").unwrap(),
            "NZP Membership Office"
        );
    }

    #[test]
    fn parses_canada_and_mexico_sections() {
        let text = "\
CANADA        Calgary -Alberta         Wilder Institute's Calgary Zoo                                 50% Guest Services               403-232-9300 x2
              Toronto                  Toronto Zoo                                                    50% Guest Relations              416-392-5929
MEXICO        Puebla                   Africam Safari Park                                   100% OR 50% Daniela Juarez                djuarez@africamsafari.com
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].country, "CA");
        assert_eq!(out[0].city, "Calgary -Alberta");
        assert_eq!(out[1].country, "CA");
        assert_eq!(out[1].city, "Toronto");
        assert_eq!(out[2].country, "MX");
        assert_eq!(
            out[2].extra.get("reciprocity_level").unwrap(),
            "100% OR 50%"
        );
    }

    #[test]
    fn skips_interleaved_sidebar_prose() {
        let text = "\
Alabama       Birmingham               Birmingham Zoo                                                 50% Patty Pendleton              205-879-0409 x232               aquarium's members.
                                                                                                                                                                       *This does not include any of the free
                                                                                                                                                                       admission zoos/aquariums in green
Alaska        Seward                   Alaska SeaLife Center                                          50% Laura Swihart                907-224-6337
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].city, "Birmingham");
        assert_eq!(out[1].city, "Seward");
    }

    #[test]
    fn skips_repeating_header_and_footer_boilerplate() {
        let text = "\
State         City                     Zoo or Aquarium                                         Reciprocity Contact Name                Phone #
Alabama       Birmingham               Birmingham Zoo                                                 50% Patty Pendleton              205-879-0409 x232
                                               PLEASE NOTE \u{2013} It is at the discretion of the participating zoos and aquariums
                                     as to whether they will be able to honor entrance benefits during this time. ALWAYS CALL AHEAD.                                                                     Updated 6/18/26
Always call the zoo or aquarium you plan to visit ahead of time to confirm
their current policies regarding reciprocal admissions.
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Birmingham Zoo");
    }
}
