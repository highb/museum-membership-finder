//! Geographic utilities — haversine distance, brute-force proximity checks.
//!
//! No spatial index needed at the expected N (~3–5k institutions).

use crate::model::LatLon;

/// Mean radius of the Earth in miles.
const EARTH_RADIUS_MI: f64 = 3958.8;

/// Haversine great-circle distance between two points, in miles.
pub fn haversine_mi(a: LatLon, b: LatLon) -> f64 {
    let d_lat = (b.lat - a.lat).to_radians();
    let d_lon = (b.lon - a.lon).to_radians();
    let a_lat = a.lat.to_radians();
    let b_lat = b.lat.to_radians();

    let h = (d_lat / 2.0).sin().powi(2)
        + a_lat.cos() * b_lat.cos() * (d_lon / 2.0).sin().powi(2);

    2.0 * EARTH_RADIUS_MI * h.sqrt().asin()
}

/// Returns `true` if `target` is within `radius_mi` of `origin`.
pub fn within(origin: LatLon, target: LatLon, radius_mi: f64) -> bool {
    haversine_mi(origin, target) <= radius_mi
}

#[cfg(test)]
mod tests {
    use super::*;

    // Known reference distances (approximate, verified via external tools).

    /// Aloha, OR (45.4912, -122.8720) to Newport, OR (44.6368, -124.0534)
    /// ≈ 83 miles — this is the acceptance-test case for ASTC exclusion.
    #[test]
    fn aloha_to_newport() {
        let aloha = LatLon::new(45.4912, -122.8720);
        let newport = LatLon::new(44.6368, -124.0534);
        let dist = haversine_mi(aloha, newport);
        // Should be roughly 83 mi (within 90 mi of residence → ASTC excluded)
        assert!(
            (80.0..=86.0).contains(&dist),
            "Aloha→Newport distance {dist:.1} mi not in expected range 80–86"
        );
        assert!(within(aloha, newport, 90.0), "Newport should be within 90 mi of Aloha");
    }

    /// Aloha, OR to OMSI, Portland (45.5085, -122.6665) ≈ 10–11 mi.
    #[test]
    fn aloha_to_omsi() {
        let aloha = LatLon::new(45.4912, -122.8720);
        let omsi = LatLon::new(45.5085, -122.6665);
        let dist = haversine_mi(aloha, omsi);
        assert!(
            (9.0..=13.0).contains(&dist),
            "Aloha→OMSI distance {dist:.1} mi not in expected range"
        );
    }

    /// OMSI to Oregon Coast Aquarium (Newport) ≈ 91 mi — just outside ASTC 90 mi.
    #[test]
    fn omsi_to_newport_aquarium() {
        let omsi = LatLon::new(45.5085, -122.6665);
        let newport = LatLon::new(44.6368, -124.0534);
        let dist = haversine_mi(omsi, newport);
        assert!(
            (88.0..=95.0).contains(&dist),
            "OMSI→Newport distance {dist:.1} mi not in expected range"
        );
    }

    /// Same point → 0 distance.
    #[test]
    fn same_point() {
        let p = LatLon::new(45.0, -122.0);
        assert!((haversine_mi(p, p)).abs() < 1e-10);
    }

    /// Antipodal points → ~half circumference ≈ 12,451 mi.
    #[test]
    fn antipodal() {
        let a = LatLon::new(0.0, 0.0);
        let b = LatLon::new(0.0, 180.0);
        let dist = haversine_mi(a, b);
        assert!(
            (12_400.0..=12_500.0).contains(&dist),
            "Antipodal distance {dist:.1} mi"
        );
    }
}
