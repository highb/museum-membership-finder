//! ROAM (Reciprocal Organization of Associated Museums) scraper.
//!
//! Source is a single-column table PDF: `State  City  Museum  Restrictions
//! Restriction Details`. `pdftotext -layout` preserves the fixed-width column
//! positions, so we split each row on wide whitespace gaps and use the
//! indentation of wrapped continuation lines to tell a museum-name wrap from
//! a restriction-details wrap.

use super::ScrapedInstitution;
use anyhow::Result;
use regex::Regex;

const PDF_URL: &str =
    "https://drive.google.com/uc?export=download&id=1mooFWoD0u8mRcY4WWH3FnzPdhTjTvmgw";

/// Indentation (in columns) above which a continuation line is assumed to be
/// wrapped "Restriction Details" text rather than a wrapped museum name.
/// The Museum column starts around col 44 and Restriction Details around
/// col 110+, so 80 sits comfortably between them.
const DETAILS_INDENT_THRESHOLD: usize = 80;

pub fn scrape() -> Result<Vec<ScrapedInstitution>> {
    let text = super::download_pdf_text(PDF_URL, "roam")?;
    let institutions = parse_text(&text)?;
    eprintln!("[roam] parsed {} institutions", institutions.len());
    Ok(institutions)
}

// These are checked with `starts_with`, not `contains`: the legend text
// ("ROAM privileges do not apply...") also appears verbatim inside real
// Restriction Details cells, but only the page-level legend line begins
// with it at column 0.
const SKIP_PREFIXES: &[&str] = &[
    "List of ROAM Museums",
    "* ROAM privileges may be restricted",
    "+ ROAM privileges do not extend",
    "\u{2021} ROAM privileges do not apply",
    "State Full",
];

fn is_skip_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return true;
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    SKIP_PREFIXES.iter().any(|s| t.starts_with(s))
}

/// Recognize a State/Full column value, whether a clean US state / Canadian
/// province name, an international marker, or a name truncated by the fixed
/// column width (in which case any glued-on remainder, usually the start of
/// the City field, is returned too).
fn match_state_field(seg: &str) -> Option<(String, String, Option<String>)> {
    if let Some(code) = super::state_code(seg) {
        let country = if super::is_canadian(code) { "CA" } else { "US" };
        return Some((country.to_string(), code.to_string(), None));
    }
    match seg {
        "Bogota" | "Bogot\u{e1}" => return Some(("CO".to_string(), String::new(), None)),
        "England" => return Some(("GB".to_string(), String::new(), None)),
        "Mexico" | "M\u{e9}xico" => return Some(("MX".to_string(), String::new(), None)),
        "Panama" | "Panam\u{e1}" => return Some(("PA".to_string(), String::new(), None)),
        "Cayman Islands" => return Some(("KY".to_string(), String::new(), None)),
        _ => {}
    }
    // Column-width truncations, e.g. "British ColumKamloops" or
    // "New Brunsw Fredericton" where the province name overflowed into (or
    // right up against) the City column.
    const TRUNCATED: &[(&str, &str, &str)] = &[
        ("District of Co", "US", "DC"),
        ("British Colum", "CA", "BC"),
        ("New Brunsw", "CA", "NB"),
        ("Prince Edwar", "CA", "PE"),
        ("Saskatchewa", "CA", "SK"),
        ("Cayman Islan", "KY", ""),
    ];
    for (prefix, country, region) in TRUNCATED {
        if let Some(rest) = seg.strip_prefix(prefix) {
            let rest = rest.trim();
            let remainder = if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            };
            return Some((country.to_string(), region.to_string(), remainder));
        }
    }
    None
}

fn apply_restriction_flags(inst: &mut ScrapedInstitution, flags: &str) {
    for ch in flags.chars() {
        match ch {
            '*' => {
                inst.special_exhibit_restricted = true;
                inst.exclusion_flags.push("*".to_string());
            }
            '+' => {
                inst.exclusion_radius_mi = Some(25.0);
                inst.exclusion_flags.push("+".to_string());
            }
            '\u{2021}' => inst.exclusion_flags.push("\u{2021}".to_string()),
            _ => {}
        }
    }
}

fn append_name(inst: &mut ScrapedInstitution, text: &str) {
    if inst.name.is_empty() {
        inst.name = text.to_string();
    } else {
        inst.name.push(' ');
        inst.name.push_str(text);
    }
}

fn append_detail(inst: &mut ScrapedInstitution, text: &str) {
    let entry = inst
        .extra
        .entry("restriction_details".to_string())
        .or_default();
    if !entry.is_empty() {
        entry.push(' ');
    }
    entry.push_str(text);
}

pub fn parse_text(text: &str) -> Result<Vec<ScrapedInstitution>> {
    let gap = Regex::new(r"\s{3,}").unwrap();
    let mut current_country = "US".to_string();
    let mut current_region = String::new();
    let mut out: Vec<ScrapedInstitution> = Vec::new();

    for raw_line in text.lines() {
        if is_skip_line(raw_line) {
            continue;
        }
        let trimmed = raw_line.trim();
        let indent = raw_line.len() - raw_line.trim_start().len();

        if indent > 0 {
            if let Some(last) = out.last_mut() {
                if indent >= DETAILS_INDENT_THRESHOLD {
                    append_detail(last, trimmed);
                } else {
                    append_name(last, trimmed);
                }
            }
            continue;
        }

        let segs: Vec<&str> = gap
            .split(trimmed)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if segs.is_empty() {
            continue;
        }

        let mut idx = 0;
        let mut glued_city: Option<String> = None;
        if let Some((country, region, remainder)) = match_state_field(segs[0]) {
            current_country = country;
            current_region = region;
            glued_city = remainder;
            idx = 1;
        }

        let city = if let Some(g) = glued_city {
            g
        } else {
            match segs.get(idx) {
                Some(s) => {
                    idx += 1;
                    s.to_string()
                }
                None => continue,
            }
        };
        let museum = match segs.get(idx) {
            Some(s) => {
                idx += 1;
                s.to_string()
            }
            None => continue,
        };
        let rest = &segs[idx..];

        let mut inst = ScrapedInstitution::new(museum, city, current_region.clone(), "roam");
        inst.country = current_country.clone();
        if let Some(flags) = rest.first() {
            apply_restriction_flags(&mut inst, flags);
        }
        if let Some(details) = rest.get(1) {
            inst.extra
                .insert("restriction_details".to_string(), details.to_string());
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
California      Bakersfield                 Kern County Museum                                                 *+
California      Berkeley                    Center for the Arts & Religion at the Graduate Theological Union
California      Berkeley                    UC Berkeley Art Museum and Pacific Film Archive                    \u{2021}            PFA Film Tickets
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].name, "Kern County Museum");
        assert_eq!(out[0].city, "Bakersfield");
        assert_eq!(out[0].region, "CA");
        assert_eq!(out[0].country, "US");
        assert!(out[0].special_exhibit_restricted);
        assert_eq!(out[0].exclusion_radius_mi, Some(25.0));
        assert_eq!(out[0].exclusion_flags, vec!["*", "+"]);

        assert_eq!(
            out[1].name,
            "Center for the Arts & Religion at the Graduate Theological Union"
        );
        assert_eq!(out[1].region, "CA");

        assert_eq!(
            out[2].extra.get("restriction_details").unwrap(),
            "PFA Film Tickets"
        );
    }

    #[test]
    fn merges_wrapped_museum_name_continuation() {
        let text = "\
California      Long Beach                  Carolyn Campagna Kleefeld Contemporary Art Museum
                                            California State University Long Beach
California      Long Beach                  Museum of Latin American Art                                       *
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(
            out[0].name,
            "Carolyn Campagna Kleefeld Contemporary Art Museum California State University Long Beach"
        );
        assert_eq!(out[1].name, "Museum of Latin American Art");
    }

    #[test]
    fn merges_wrapped_restriction_details_continuation() {
        let text = "\
California      Napa                        diRosa Center for Contemporary Art                                 *\u{2021}           Discounted entry to public events and annual
                                                                                                                            Auction
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].extra.get("restriction_details").unwrap(),
            "Discounted entry to public events and annual Auction"
        );
    }

    #[test]
    fn handles_column_width_truncated_province_names() {
        let text = "\
Alberta         Edmonton                    Art Gallery of Alberta                                   +
British ColumKamloops                       Kamloops Art Gallery
New Brunsw Fredericton                      Beaverbrook Art Gallery                                  *
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].region, "AB");
        assert_eq!(out[0].country, "CA");

        assert_eq!(out[1].region, "BC");
        assert_eq!(out[1].city, "Kamloops");
        assert_eq!(out[1].name, "Kamloops Art Gallery");

        assert_eq!(out[2].region, "NB");
        assert_eq!(out[2].city, "Fredericton");
        assert_eq!(out[2].name, "Beaverbrook Art Gallery");
    }

    #[test]
    fn handles_district_of_columbia_truncation() {
        let text = "\
District of Co Washington                   National Gallery of Art                                      *\u{2021}           ROAM privileges do not apply to retail or
                                                                                                                      dining.
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].region, "DC");
        assert_eq!(out[0].country, "US");
        assert_eq!(out[0].city, "Washington");
        assert_eq!(
            out[0].extra.get("restriction_details").unwrap(),
            "ROAM privileges do not apply to retail or dining."
        );
    }

    #[test]
    fn handles_international_entries() {
        let text = "\
Bogota          Bogota                      MAMBO                                                    *
Bogota          Bogota                      Museo Nacional de Colombia
England         Bath                        American Museum & Gardens
Mexico          Monterrey                   Museo de Arte Contempor\u{e1}neo de Monterrey (MARCO)         *
Panam\u{e1}          Panam\u{e1}                      MAC Panam\u{e1}                                               *
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].country, "CO");
        assert_eq!(out[2].country, "GB");
        assert_eq!(out[3].country, "MX");
        assert_eq!(out[4].country, "PA");
    }

    #[test]
    fn handles_online_only_entry_with_no_city_column() {
        let text = "\
Online                                      The Hunger Museum                                                  *
Alabama         Auburn                      Jule Collins Smith Museum of Fine Art                              *
";
        let out = parse_text(text).unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].city, "Online");
        assert_eq!(out[0].name, "The Hunger Museum");
        assert_eq!(out[1].region, "AL");
    }
}
