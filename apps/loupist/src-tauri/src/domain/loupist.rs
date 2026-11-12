use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::{require_bounded_text, require_identifier, required_option, DomainError};
use crate::providers::catalog::valid_model_profile_id;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootFilesystemIdentity {
    pub device: u64,
    pub inode: u64,
}

/// How a Loupist talks. Presentation only: it never changes evidence requirements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LoupistTone {
    /// States uncertainty first, adds caveats and explains unfamiliar terms.
    Careful,
    /// Concise research language; explains terms on first use.
    #[default]
    Balanced,
    /// Leads with the view, fewer caveats, assumes financial fluency.
    Direct,
}

/// The template a Loupist was made from (onboarding and New Loupist cards).
/// Kept so a class Loupist's watchers start on for its class and its first
/// morning message can name the connections it works best with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum LoupistTemplate {
    Portfolio,
    Semiconductors,
    Dividends,
    Technology,
    Crypto,
    Macro,
    Commodities,
    RealAssets,
    Custom,
}

impl LoupistTemplate {
    pub const ALL: [Self; 9] = [
        Self::Portfolio,
        Self::Semiconductors,
        Self::Dividends,
        Self::Technology,
        Self::Crypto,
        Self::Macro,
        Self::Commodities,
        Self::RealAssets,
        Self::Custom,
    ];

    /// The renderer's template id ("realAssets").
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portfolio => "portfolio",
            Self::Semiconductors => "semiconductors",
            Self::Dividends => "dividends",
            Self::Technology => "technology",
            Self::Crypto => "crypto",
            Self::Macro => "macro",
            Self::Commodities => "commodities",
            Self::RealAssets => "realAssets",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|template| template.as_str() == value.trim())
    }

    /// The focus the New Loupist dialog fills in for this template (its English
    /// onboarding copy); Custom has none.
    pub fn default_focus(self) -> Option<&'static str> {
        match self {
            Self::Portfolio => Some("Companies I hold and watch"),
            Self::Semiconductors => Some("Chips, memory, foundries and equipment"),
            Self::Dividends => Some("Dividend payers and payout safety"),
            Self::Technology => Some("Large US technology companies"),
            Self::Crypto => Some("Crypto networks, tokens and the companies around them"),
            Self::Macro => Some("Interest rates, inflation, currencies and central banks"),
            Self::Commodities => {
                Some("Energy, metals and farm goods: supply, demand and positioning")
            }
            Self::RealAssets => {
                Some("Property, infrastructure and land: owners, tenants and financing")
            }
            Self::Custom => None,
        }
    }

    /// The asset class an asset-class template is made for.
    pub fn asset_class(self) -> Option<crate::knowledge::assets::AssetClass> {
        use crate::knowledge::assets::AssetClass;
        match self {
            Self::Crypto => Some(AssetClass::Crypto),
            Self::Macro => Some(AssetClass::RatesFx),
            Self::Commodities => Some(AssetClass::Commodity),
            Self::RealAssets => Some(AssetClass::RealAsset),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoupistProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub focus: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default = "default_loupist_avatar")]
    pub avatar: String,
    /// Profiles written before tones existed read as Balanced.
    #[serde(default)]
    pub tone: LoupistTone,
    /// The template chosen at creation; none for imported folders, shared
    /// packs and Loupists made before templates were kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<LoupistTemplate>,
    pub workspace_root: String,
    /// Optional app-private runtime binding. Restored libraries receive a fresh
    /// runtime directory when the user reconnects their workspace.
    #[serde(deserialize_with = "required_option")]
    pub runtime_filesystem_identity: Option<RootFilesystemIdentity>,
    #[serde(deserialize_with = "required_option")]
    pub root_filesystem_identity: Option<RootFilesystemIdentity>,
    #[serde(deserialize_with = "required_option")]
    pub last_model_profile_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

fn default_loupist_avatar() -> String {
    "initial".into()
}

/// Persisted legacy IDs and the ten bundled portraits share one validation boundary.
pub const LOUPIST_AVATAR_IDS: &[&str] = &[
    "loupist-01",
    "loupist-02",
    "loupist-03",
    "loupist-04",
    "loupist-05",
    "loupist-06",
    "loupist-07",
    "loupist-08",
    "loupist-09",
    "loupist-10",
    "initial",
    "cpu",
    "coins",
    "globe",
    "line-chart",
    "landmark",
    "factory",
    "compass",
    "book-open",
    "sparkles",
];

impl LoupistProfile {
    pub fn validate(&self) -> Result<(), DomainError> {
        require_identifier(&self.id, "loupist id is empty or unsafe")?;
        let root = self.workspace_root.as_bytes();
        // A restored library may come from another OS. Preserve its absolute
        // path as reconnect metadata; local execution separately binds a real
        // directory using this machine's filesystem rules.
        let windows_absolute = self.workspace_root.starts_with("\\\\")
            || (root.len() >= 3
                && root[0].is_ascii_alphabetic()
                && root[1] == b':'
                && matches!(root[2], b'/' | b'\\'));
        let unix_absolute = self.workspace_root.starts_with('/');
        if (!unix_absolute && !windows_absolute) || self.workspace_root.contains('\0') {
            return Err(DomainError::Invalid(
                "Loupist workspace must be an absolute path",
            ));
        }
        if self
            .last_model_profile_id
            .as_deref()
            .is_some_and(|value| !valid_model_profile_id(value))
        {
            return Err(DomainError::Invalid("last model profile id is invalid"));
        }
        if self.created_at_ms < 0 || self.updated_at_ms < self.created_at_ms {
            return Err(DomainError::Invalid("loupist timestamps are invalid"));
        }
        Ok(())
    }
}

/// Read foreign Windows paths even when a disconnected library is opened on Unix.
/// Backslashes in a Unix folder name are literal characters, not separators.
pub fn workspace_folder_name(path: &str) -> &str {
    let bytes = path.as_bytes();
    let windows = path.starts_with("\\\\")
        || (bytes.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'/' | b'\\'));
    let separator = |ch| ch == '/' || (windows && ch == '\\');
    path.trim_end_matches(separator)
        .rsplit(separator)
        .find(|part| !part.is_empty())
        .unwrap_or(path)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoupistCapabilityBinding {
    pub loupist_id: String,
    pub entry_id: String,
    pub enabled: bool,
    pub granted_permissions: Vec<String>,
    pub config: std::collections::BTreeMap<String, String>,
    pub updated_at_ms: i64,
}

impl LoupistCapabilityBinding {
    pub fn validate(&self) -> Result<(), DomainError> {
        require_identifier(&self.loupist_id, "capability Loupist id is invalid")?;
        if self.entry_id.is_empty()
            || self.entry_id.len() > 96
            || !self.entry_id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte)
            })
        {
            return Err(DomainError::Invalid("capability entry id is invalid"));
        }
        if self.granted_permissions.len() > 32 || self.config.len() > 32 {
            return Err(DomainError::Invalid("capability settings are too large"));
        }
        let mut permissions = BTreeSet::new();
        for permission in &self.granted_permissions {
            require_bounded_text(permission, 256, "capability permission is invalid")?;
            if !permissions.insert(permission.as_str()) {
                return Err(DomainError::Invalid(
                    "capability permissions contain duplicates",
                ));
            }
        }
        for (key, value) in &self.config {
            require_bounded_text(key, 128, "capability config key is invalid")?;
            if value.len() > 4_096 || value.contains('\0') {
                return Err(DomainError::Invalid("capability config value is invalid"));
            }
        }
        if self.updated_at_ms < 0 {
            return Err(DomainError::Invalid("capability timestamp is invalid"));
        }
        Ok(())
    }
}

pub fn default_loupist_capability_bindings(
    loupist_id: &str,
    timestamp: i64,
) -> Vec<LoupistCapabilityBinding> {
    let catalog = crate::marketplace::bundled_catalog()
        .expect("the bundled Marketplace catalog is a build invariant");
    catalog
        .entries
        .into_iter()
        .map(|entry| {
            // A host-only connection (wallet reads) is turned on when first used.
            let enabled = entry.free_state == crate::marketplace::MarketplaceFreeState::Keyless
                && entry.id != crate::finance_data::onchain::ONCHAIN_SOURCE_ID
                && entry.setup.as_ref().is_none_or(|setup| {
                    setup.config_fields.iter().all(|field| !field.required)
                        && setup.credential_fields.iter().all(|field| !field.required)
                });
            LoupistCapabilityBinding {
                loupist_id: loupist_id.to_owned(),
                entry_id: entry.id,
                enabled,
                granted_permissions: if enabled {
                    vec!["execute".to_owned()]
                } else {
                    Vec::new()
                },
                config: std::collections::BTreeMap::new(),
                updated_at_ms: timestamp,
            }
        })
        .collect::<Vec<_>>()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionKind {
    Chat,
    ChatRuntime,
    Loupist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionPhase {
    Prepared,
    Detached,
}

/// Durable intent for canonical filesystem deletion. SQLite deliberately
/// retains this row after the target row is deleted so startup can distinguish
/// rollback (target exists) from cleanup (target is committed absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionJournal {
    pub id: String,
    pub kind: DeletionKind,
    pub loupist_id: String,
    pub target_id: String,
    #[serde(deserialize_with = "required_option")]
    pub expected_source_identity: Option<RootFilesystemIdentity>,
    pub phase: DeletionPhase,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl DeletionJournal {
    pub fn validate(&self) -> Result<(), DomainError> {
        require_identifier(&self.id, "deletion journal id is empty or unsafe")?;
        require_identifier(
            &self.loupist_id,
            "deletion journal Loupist id is empty or unsafe",
        )?;
        require_identifier(
            &self.target_id,
            "deletion journal target id is empty or unsafe",
        )?;
        if self.kind == DeletionKind::Loupist && self.target_id != self.loupist_id {
            return Err(DomainError::Invalid(
                "Loupist deletion journal target is invalid",
            ));
        }
        if self.kind != DeletionKind::Loupist && self.expected_source_identity.is_some() {
            return Err(DomainError::Invalid(
                "deletion journal source identity is invalid",
            ));
        }
        if self.created_at_ms < 0 || self.updated_at_ms < self.created_at_ms {
            return Err(DomainError::Invalid(
                "deletion journal timestamps are invalid",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod release_compatibility_tests {
    use super::*;

    #[test]
    fn the_upgrade_ladder_preserves_legacy_avatar_ids_and_missing_tone() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch(include_str!(
                "../store/tests/fixtures/baseline-store4-knowledge6.sql"
            ))
            .unwrap();
        let mut profile = crate::test_fixtures::loupist_profile("legacy-avatar");
        profile.avatar = "cpu".into();
        let mut stored = serde_json::to_value(&profile).unwrap();
        stored.as_object_mut().unwrap().remove("tone");
        let json = stored.to_string();
        connection
            .execute(
                "INSERT INTO loupist_profiles VALUES (?1, 1, ?2, NULL, NULL, ?3)",
                rusqlite::params![profile.id, profile.workspace_root, json],
            )
            .unwrap();
        let upgraded = crate::store::upgraded_copy(&connection).unwrap();
        let after: String = upgraded
            .query_row("SELECT data_json FROM loupist_profiles", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            after, json,
            "store upgrades do not rewrite this profile data"
        );
        let loaded: LoupistProfile = serde_json::from_str(&after).unwrap();
        assert_eq!(loaded.avatar, "cpu");
        assert_eq!(loaded.tone, LoupistTone::Balanced);
        loaded.validate().unwrap();
    }
}
