//! Stub — to be implemented
use super::ScrapedInstitution;
use anyhow::Result;

pub fn scrape() -> Result<Vec<ScrapedInstitution>> {
    anyhow::bail!("{} scraper not yet implemented", module_path!())
}
