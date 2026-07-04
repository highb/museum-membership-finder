//! Per-network scrapers for official association data sources.
//!
//! Each submodule implements `scrape() -> Result<Vec<ScrapedInstitution>>`
//! fetching directly from the association's own website/PDF.

pub mod acm;
pub mod ahs;
pub mod astc;
pub mod aza;
pub mod marp;
pub mod narm;
pub mod roam;

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Write;
use std::process::Command;

/// A single institution record as returned by a network scraper.
#[derive(Debug, Clone)]
pub struct ScrapedInstitution {
    pub name: String,
    pub city: String,
    pub region: String,  // state/province code, e.g. "OR", "WA", "ON"
    pub country: String, // ISO 3166-1 alpha-2, default "US"
    pub network: String, // lowercase network key: "acm", "narm", etc.
    pub website: Option<String>,
    pub phone: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Raw exclusion-flag symbols from source (e.g. "*", "**", "#", "+")
    pub exclusion_flags: Vec<String>,
    /// Whether special exhibits are restricted
    pub special_exhibit_restricted: bool,
    /// Exclusion radius in miles parsed from flags
    pub exclusion_radius_mi: Option<f64>,
    /// Network-specific extra fields (e.g. AZA reciprocity level, ASTC admittance policy)
    pub extra: HashMap<String, String>,
}

impl ScrapedInstitution {
    /// Create a new ScrapedInstitution with required fields, defaults for the rest.
    pub fn new(name: String, city: String, region: String, network: &str) -> Self {
        Self {
            name,
            city,
            region,
            country: "US".to_string(),
            network: network.to_string(),
            website: None,
            phone: None,
            lat: None,
            lon: None,
            exclusion_flags: Vec::new(),
            special_exhibit_restricted: false,
            exclusion_radius_mi: None,
            extra: HashMap::new(),
        }
    }
}

/// Build a shared reqwest blocking client.
pub fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent("tessera-xtask/0.1 (museum-data-ingestion)")
        .timeout(std::time::Duration::from_secs(60))
        .build()?)
}

/// Map full US state/territory/Canadian province name -> 2-letter code.
pub fn state_code(name: &str) -> Option<&'static str> {
    let n = name.trim();
    match n {
        // US States
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

/// Canadian province/territory codes, used to detect country from region.
const CA_CODES: &[&str] = &[
    "AB", "BC", "MB", "NB", "NL", "NS", "ON", "PE", "QC", "SK", "NT", "NU", "YT",
];

/// Returns true if the region code is a Canadian province/territory.
pub fn is_canadian(code: &str) -> bool {
    CA_CODES.contains(&code)
}

/// Ensure `pdftotext` (poppler-utils) is available, installing it via apt if missing.
fn ensure_pdftotext() -> Result<()> {
    if Command::new("pdftotext").arg("-v").output().is_ok() {
        return Ok(());
    }
    eprintln!("[scrapers] pdftotext not found; installing poppler-utils...");
    let status = Command::new("sudo")
        .args(["apt-get", "install", "-y", "poppler-utils"])
        .status()
        .context("failed to run apt-get install poppler-utils")?;
    if !status.success() {
        anyhow::bail!("apt-get install poppler-utils exited with status {status}");
    }
    Ok(())
}

/// Write PDF bytes to a temp file and extract text via `pdftotext -layout`.
pub fn pdf_bytes_to_text(bytes: &[u8], tag: &str) -> Result<String> {
    ensure_pdftotext()?;
    let path = std::env::temp_dir().join(format!("tessera-xtask-{tag}.pdf"));
    {
        let mut f = std::fs::File::create(&path)
            .with_context(|| format!("creating temp file {}", path.display()))?;
        f.write_all(bytes)
            .with_context(|| format!("writing temp file {}", path.display()))?;
    }
    let output = Command::new("pdftotext")
        .args([
            "-layout",
            path.to_str().context("temp path is not valid UTF-8")?,
            "-",
        ])
        .output()
        .context("running pdftotext")?;
    let _ = std::fs::remove_file(&path);
    if !output.status.success() {
        anyhow::bail!(
            "pdftotext failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Download a PDF from `url` and extract its text via `pdftotext -layout`.
pub fn download_pdf_text(url: &str, tag: &str) -> Result<String> {
    eprintln!("[{tag}] downloading {url}");
    let bytes = http_client()?
        .get(url)
        .send()
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("bad HTTP status from {url}"))?
        .bytes()
        .context("reading response body")?;
    eprintln!("[{tag}] downloaded {} bytes", bytes.len());
    let text = pdf_bytes_to_text(&bytes, tag)?;
    eprintln!("[{tag}] extracted {} chars of text", text.len());
    Ok(text)
}
