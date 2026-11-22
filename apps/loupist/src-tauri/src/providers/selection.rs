//! Read-only projection of a requested Chat or schedule model.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{
    auto::{self, AutoChoice},
    catalog::{CredentialSource, ModelCatalog, ModelCatalogView, ModelRunControl},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ModelUnavailableReason {
    NoConnectedAccount,
    NoEligibleModel,
    AccountDisconnected,
    ModelHidden,
    InvalidConfiguration,
    CatalogUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResolvedModelSelection {
    pub model: Option<ModelIdentity>,
    pub thinking_levels: Vec<String>,
    pub run_controls: Vec<ModelRunControl>,
    pub is_default: bool,
    pub unavailable_reason: Option<ModelUnavailableReason>,
    pub thinking_level: Option<String>,
    pub run_options: BTreeMap<String, String>,
}

impl ResolvedModelSelection {
    pub(crate) fn catalog_unavailable(id: &str) -> Self {
        Self::unavailable(
            ModelUnavailableReason::CatalogUnavailable,
            auto::uses_default(id),
        )
    }

    pub(crate) fn unavailable(reason: ModelUnavailableReason, is_default: bool) -> Self {
        Self {
            model: None,
            thinking_levels: vec![],
            run_controls: vec![],
            is_default,
            unavailable_reason: Some(reason),
            thinking_level: None,
            run_options: BTreeMap::new(),
        }
    }

    fn available(choice: AutoChoice, is_default: bool) -> Self {
        let model = choice.model;
        Self {
            model: Some(ModelIdentity {
                id: model.id,
                provider: model.provider,
                model: model.model,
                name: model.name,
            }),
            thinking_levels: model.thinking_levels,
            run_controls: model.run_controls,
            is_default,
            unavailable_reason: None,
            thinking_level: Some(choice.thinking_level),
            run_options: choice.run_options,
        }
    }
}

/// All reads in a list use one credential, visibility and catalog snapshot.
pub(crate) struct SelectionCatalog {
    pub view: ModelCatalogView,
    pub catalog: ModelCatalog,
    pub default: Option<AutoChoice>,
}

impl SelectionCatalog {
    pub fn selection(
        &self,
        id: &str,
        level: &str,
        options: &BTreeMap<String, String>,
    ) -> ResolvedModelSelection {
        if let Some(choice) = auto::explicit_choice(&self.catalog, id, level, options) {
            if let Some(model) = self.view.models.iter().find(|model| model.id == id) {
                if model.credential_source == CredentialSource::Missing {
                    return ResolvedModelSelection::unavailable(
                        ModelUnavailableReason::AccountDisconnected,
                        false,
                    );
                }
                if self
                    .view
                    .hidden_model_profile_ids
                    .iter()
                    .any(|hidden| hidden == id)
                {
                    return ResolvedModelSelection::unavailable(
                        ModelUnavailableReason::ModelHidden,
                        false,
                    );
                }
                return ResolvedModelSelection::available(choice, false);
            }
        }
        if let Some(choice) = self.default.clone() {
            let choice = auto::default_controls(choice, id, level, options);
            return ResolvedModelSelection::available(choice, true);
        }
        let connected = self
            .view
            .providers
            .iter()
            .any(|p| p.credential_source != CredentialSource::Missing)
            || self
                .view
                .custom_providers
                .iter()
                .any(|p| p.credential_source != CredentialSource::Missing);
        ResolvedModelSelection::unavailable(
            if connected {
                ModelUnavailableReason::NoEligibleModel
            } else {
                ModelUnavailableReason::NoConnectedAccount
            },
            true,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ModelIdentity {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DefaultModelChange {
    pub id: String,
    pub previous: Option<ModelIdentity>,
    pub current: Option<ModelIdentity>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ObservedDefault {
    pub model: Option<ModelIdentity>,
    pub pending: Option<DefaultModelChange>,
}
