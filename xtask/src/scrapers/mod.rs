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

use anyhow::Result;
use std::collections::HashMap;

/// A single institution record as returned by a network scraper.
#[derive(Debug, Clone)]
pub struct ScrapedInstitution {
    pub name: String,
    pub city: String,
    pub region: String,       // state/province code, e.g. "OR", "WA", "ON"
    pub country: String,      // ISO 3166-1 alpha-2, default "US"
    pub network: String,      // lowercase network key: "acm", "narm", etc.
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
