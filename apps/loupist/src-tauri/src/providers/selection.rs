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
    /// Why the stored model, still listed, cannot run now. The selection is
    /// then Auto in its place; the stored choice applies again once it can run.
    pub stored_unavailable: Option<ModelUnavailableReason>,
}

/// The model one request runs on, decided at admission.
pub(crate) struct Admitted {
    pub choice: AutoChoice,
    /// Auto answered, for an empty choice or one that cannot run.
    pub is_default: bool,
    pub stored_unavailable: Option<ModelUnavailableReason>,
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
            stored_unavailable: None,
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
            stored_unavailable: None,
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
    /// Whether any built-in or custom AI account has a credential.
    pub fn any_account_connected(&self) -> bool {
        self.view
            .providers
            .iter()
            .any(|p| p.credential_source != CredentialSource::Missing)
            || self
                .view
                .custom_providers
                .iter()
                .any(|p| p.credential_source != CredentialSource::Missing)
    }

    pub fn selection(
        &self,
        id: &str,
        level: &str,
        options: &BTreeMap<String, String>,
    ) -> ResolvedModelSelection {
        match self.admit(id, level, options) {
            Ok(admitted) => ResolvedModelSelection {
                stored_unavailable: admitted.stored_unavailable,
                ..ResolvedModelSelection::available(admitted.choice, admitted.is_default)
            },
            Err(reason) => ResolvedModelSelection::unavailable(reason, true),
        }
    }

    /// An explicit model runs when it is offered, its account has a credential
    /// and it is shown in Chat. Otherwise Auto answers: for an empty choice with
    /// the requested controls the model supports, for an explicit one with the
    /// controls Auto declares, so a stored choice's controls never reach a model
    /// that does not declare them. Only no runnable model at all refuses.
    pub(crate) fn admit(
        &self,
        id: &str,
        level: &str,
        options: &BTreeMap<String, String>,
    ) -> Result<Admitted, ModelUnavailableReason> {
        let mut stored_unavailable = None;
        if let Some(choice) = auto::explicit_choice(&self.catalog, id, level, options) {
            match self.cannot_run(&choice.model.id) {
                None => {
                    return Ok(Admitted {
                        choice,
                        is_default: false,
                        stored_unavailable: None,
                    })
                }
                reason => stored_unavailable = reason,
            }
        }
        match self.default.clone() {
            Some(choice) => Ok(Admitted {
                choice: auto::default_controls(choice, id, level, options),
                is_default: true,
                stored_unavailable,
            }),
            None if self.any_account_connected() => Err(ModelUnavailableReason::NoEligibleModel),
            None => Err(ModelUnavailableReason::NoConnectedAccount),
        }
    }

    /// Why a model the catalog lists cannot run now: its account signed out or
    /// disconnected, or the user hid it.
    fn cannot_run(&self, id: &str) -> Option<ModelUnavailableReason> {
        let Some(model) = self.view.models.iter().find(|model| model.id == id) else {
            return Some(ModelUnavailableReason::CatalogUnavailable);
        };
        if model.credential_source == CredentialSource::Missing {
            Some(ModelUnavailableReason::AccountDisconnected)
        } else if self
            .view
            .hidden_model_profile_ids
            .iter()
            .any(|hidden| hidden == id)
        {
            Some(ModelUnavailableReason::ModelHidden)
        } else {
            None
        }
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
