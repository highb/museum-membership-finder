//! Embedded dataset — compiled into the WASM binary.

use tessera_core::model::*;
use tessera_core::zip::ZipCentroids;

const NETWORKS_JSON: &str = include_str!("../../data/networks.json");
const INSTITUTIONS_JSON: &str = include_str!("../../data/institutions.json");
const MEMBERSHIPS_JSON: &str = include_str!("../../data/memberships.json");
const ZIP_CENTROIDS_JSON: &str = include_str!("../../data/zip-centroids-pnw.json");

pub fn load_dataset() -> Dataset {
    Dataset {
        networks: serde_json::from_str(NETWORKS_JSON).expect("networks.json"),
        institutions: serde_json::from_str(INSTITUTIONS_JSON).expect("institutions.json"),
        memberships: serde_json::from_str(MEMBERSHIPS_JSON).expect("memberships.json"),
    }
}

pub fn load_zips() -> ZipCentroids {
    ZipCentroids::from_json(ZIP_CENTROIDS_JSON).expect("zip-centroids.json")
}
