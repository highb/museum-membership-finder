//! Domain types for the Tessera reciprocal museum coverage optimizer.
//!
//! These types form the canonical data model. JSON files in `data/` deserialize
//! into these structures. The model is pure (no I/O, no side effects) and
//! WASM-safe.

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Network
// ---------------------------------------------------------------------------

/// A reciprocal-admission network. Each network defines its own default
/// admission type and exclusion rule; individual institutions may override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Narm,
    Astc,
    Ahs,
    Roam,
    Marp,
    Acm,
    Aza,
    TimeTravelers,
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Network::Narm => write!(f, "NARM"),
            Network::Astc => write!(f, "ASTC"),
            Network::Ahs => write!(f, "AHS"),
            Network::Roam => write!(f, "ROAM"),
            Network::Marp => write!(f, "MARP"),
            Network::Acm => write!(f, "ACM"),
            Network::Aza => write!(f, "AZA"),
            Network::TimeTravelers => write!(f, "Time Travelers"),
        }
    }
}

impl Network {
    /// All known networks, in a fixed order.
    pub const ALL: &[Network] = &[
        Network::Narm,
        Network::Astc,
        Network::Ahs,
        Network::Roam,
        Network::Marp,
        Network::Acm,
        Network::Aza,
        Network::TimeTravelers,
    ];
}

// ---------------------------------------------------------------------------
// Admission
// ---------------------------------------------------------------------------

/// What reciprocal admission gets you: free entry or a percentage discount.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Admission {
    /// Free admission.
    Free,
    /// Discounted admission. `fraction` is the discount as a decimal
    /// (e.g. 0.5 = 50% off).
    Discount { fraction: f32 },
}

impl Admission {
    /// Returns `true` if this is fully free admission.
    pub fn is_free(&self) -> bool {
        matches!(self, Admission::Free)
    }
}

// ---------------------------------------------------------------------------
// ExclusionRule
// ---------------------------------------------------------------------------

/// How a network (or an institution's flag) excludes a target based on
/// geographic proximity.
///
/// - `residence_radius_mi`: exclude if the target is within this distance of
///   the user's residence.
/// - `home_institution_radius_mi`: exclude if the target is within this
///   distance of the user's home (membership-granting) institution.
/// - `both_must_clear`: if `true`, the target must clear **both** radii to be
///   eligible (ASTC model). If `false`, each radius is evaluated independently.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExclusionRule {
    /// Exclude if target within this many miles of the user's residence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residence_radius_mi: Option<f64>,

    /// Exclude if target within this many miles of the home institution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_institution_radius_mi: Option<f64>,

    /// ASTC-style: target must clear *both* radii. If `false`, each radius
    /// is an independent exclusion check.
    #[serde(default)]
    pub both_must_clear: bool,
}

impl ExclusionRule {
    /// No exclusion — the network imposes no geographic restriction.
    pub const NONE: ExclusionRule = ExclusionRule {
        residence_radius_mi: None,
        home_institution_radius_mi: None,
        both_must_clear: false,
    };

    /// ASTC's rule: 90-mile radius from both residence and home institution;
    /// target must clear both.
    pub const ASTC_DEFAULT: ExclusionRule = ExclusionRule {
        residence_radius_mi: Some(90.0),
        home_institution_radius_mi: Some(90.0),
        both_must_clear: true,
    };
}

// ---------------------------------------------------------------------------
// NetworkSpec
// ---------------------------------------------------------------------------

/// The default rules for an entire reciprocal network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSpec {
    pub network: Network,
    /// Network-default admission type (institutions may override).
    pub admission: Admission,
    /// Network-default exclusion rule (institutions may override).
    pub default_exclusion: ExclusionRule,
}

// ---------------------------------------------------------------------------
// Participation
// ---------------------------------------------------------------------------

/// An institution's membership in a reciprocal network, possibly with
/// institution-specific overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Participation {
    pub network: Network,

    /// Override the network-default admission for this institution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<Admission>,

    /// Override the network-default exclusion rule (e.g. NARM 15/50 mi flags).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusion: Option<ExclusionRule>,

    /// True if reciprocal admission excludes special/temporary exhibitions
    /// (NARM `*` flag).
    #[serde(default)]
    pub special_exhibit_restricted: bool,
}

// ---------------------------------------------------------------------------
// LatLon
// ---------------------------------------------------------------------------

/// A geographic coordinate (WGS 84).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
}

// ---------------------------------------------------------------------------
// Institution
// ---------------------------------------------------------------------------

/// A museum, science center, garden, zoo, or historic site that participates
/// in one or more reciprocal networks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Institution {
    /// Stable, URL-safe slug (e.g. `"omsi"`, `"lan-su-chinese-garden"`).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// City.
    pub city: String,
    /// State or region.
    pub region: String,
    /// ISO 3166-1 alpha-2 country code.
    #[serde(default = "default_country")]
    pub country: String,
    /// Location.
    pub location: LatLon,
    /// Networks this institution participates in.
    pub participates: Vec<Participation>,
    /// Attribution: source name + retrieval date.
    pub provenance: String,
}

fn default_country() -> String {
    "US".to_string()
}

// ---------------------------------------------------------------------------
// Membership
// ---------------------------------------------------------------------------

/// A purchasable membership tier at an institution. Only some tiers unlock
/// reciprocal-network access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Membership {
    /// The institution this membership belongs to.
    pub institution_id: String,
    /// Tier name (e.g. `"Family Plus"`, `"Explorer"`).
    pub tier: String,
    /// Annual price in USD.
    pub price_usd: f64,
    /// Which reciprocal networks this tier unlocks.
    pub networks_unlocked: Vec<Network>,
    /// Number of guests included on the membership.
    #[serde(default)]
    pub guests_included: u8,
}

// ---------------------------------------------------------------------------
// User
// ---------------------------------------------------------------------------

/// A user's location and currently-held memberships. Never leaves the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub residence: LatLon,
    /// References to held memberships (institution_id + tier).
    #[serde(default)]
    pub held: Vec<MembershipRef>,
}

/// A reference to a specific membership tier at an institution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MembershipRef {
    pub institution_id: String,
    pub tier: String,
}

// ---------------------------------------------------------------------------
// Dataset — top-level container
// ---------------------------------------------------------------------------

/// The complete dataset: networks, institutions, and available memberships.
/// This is the shape of the canonical JSON data files combined.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dataset {
    pub networks: Vec<NetworkSpec>,
    pub institutions: Vec<Institution>,
    pub memberships: Vec<Membership>,
}

impl Dataset {
    /// Look up a network spec by network enum.
    pub fn network_spec(&self, network: Network) -> Option<&NetworkSpec> {
        self.networks.iter().find(|s| s.network == network)
    }

    /// Look up an institution by id.
    pub fn institution(&self, id: &str) -> Option<&Institution> {
        self.institutions.iter().find(|i| i.id == id)
    }

    /// All memberships for a given institution.
    pub fn memberships_for(&self, institution_id: &str) -> Vec<&Membership> {
        self.memberships
            .iter()
            .filter(|m| m.institution_id == institution_id)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_display() {
        assert_eq!(Network::Narm.to_string(), "NARM");
        assert_eq!(Network::Astc.to_string(), "ASTC");
        assert_eq!(Network::TimeTravelers.to_string(), "Time Travelers");
    }

    #[test]
    fn admission_is_free() {
        assert!(Admission::Free.is_free());
        assert!(!Admission::Discount { fraction: 0.5 }.is_free());
    }

    #[test]
    fn exclusion_rule_constants() {
        let none = ExclusionRule::NONE;
        assert!(none.residence_radius_mi.is_none());
        assert!(none.home_institution_radius_mi.is_none());
        assert!(!none.both_must_clear);

        let astc = ExclusionRule::ASTC_DEFAULT;
        assert_eq!(astc.residence_radius_mi, Some(90.0));
        assert_eq!(astc.home_institution_radius_mi, Some(90.0));
        assert!(astc.both_must_clear);
    }

    #[test]
    fn network_all_count() {
        assert_eq!(Network::ALL.len(), 8);
    }
}
