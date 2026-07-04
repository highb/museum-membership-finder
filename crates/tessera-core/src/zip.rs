//! ZIP-code centroid lookup — bundled, local, no external API calls.
//!
//! Privacy property: the user's location is resolved client-side from a
//! static table, never sent to any server.

use std::collections::HashMap;

use crate::model::LatLon;

/// A ZIP-centroid lookup table. Backed by a `HashMap<String, LatLon>`.
#[derive(Debug, Clone)]
pub struct ZipCentroids {
    entries: HashMap<String, LatLon>,
}

impl ZipCentroids {
    /// Parse a JSON object of `{ "ZIPCODE": [lat, lon], ... }`.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let raw: HashMap<String, [f64; 2]> = serde_json::from_str(json)?;
        let entries = raw
            .into_iter()
            .map(|(zip, [lat, lon])| (zip, LatLon::new(lat, lon)))
            .collect();
        Ok(ZipCentroids { entries })
    }

    /// Look up a ZIP code. Returns `None` if not in the table.
    pub fn lookup(&self, zip: &str) -> Option<LatLon> {
        self.entries.get(zip).copied()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNW_JSON: &str = include_str!("../../../data/zip-centroids-pnw.json");

    #[test]
    fn parse_pnw_centroids() {
        let zips = ZipCentroids::from_json(PNW_JSON).unwrap();
        assert!(zips.len() >= 100, "expected ≥100 PNW zips, got {}", zips.len());
    }

    #[test]
    fn lookup_aloha() {
        let zips = ZipCentroids::from_json(PNW_JSON).unwrap();
        let loc = zips.lookup("97007").expect("97007 should exist");
        // Aloha, OR is roughly (45.49, -122.87)
        assert!((loc.lat - 45.49).abs() < 0.1, "lat {}", loc.lat);
        assert!((loc.lon + 122.87).abs() < 0.1, "lon {}", loc.lon);
    }

    #[test]
    fn lookup_missing() {
        let zips = ZipCentroids::from_json(PNW_JSON).unwrap();
        assert!(zips.lookup("00000").is_none());
    }
}
