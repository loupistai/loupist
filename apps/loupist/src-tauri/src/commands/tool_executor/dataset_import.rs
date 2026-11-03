use super::*;
use crate::datasets::{DatasetInput, MAX_DATASET_BYTES};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRequest {
    path: String,
    #[serde(default)]
    expected_sha256: Option<String>,
    #[serde(default)]
    input_refs: Vec<DatasetInput>,
}

impl AppToolExecutor {
    pub(super) fn dataset_import(&self, params: Value) -> Result<Value, BrokerError> {
        let request: ImportRequest = serde_json::from_value(params.clone()).map_err(|_| {
            let (field, expected) = if !params.get("path").is_some_and(Value::is_string) {
                ("dataset_import.path", "expected a string")
            } else if params.get("expected_sha256").is_some_and(|value| !value.is_null() && !value.is_string()) {
                ("dataset_import.expected_sha256", "expected a string or null")
            } else if params.get("input_refs").is_some_and(|value| serde_json::from_value::<Vec<DatasetInput>>(value.clone()).is_err()) {
                ("dataset_import.input_refs", "expected an array of {name: string, dataset_id: string, revision: integer, pointer?: string}")
            } else {
                ("dataset_import", "expected only path, expected_sha256, and input_refs fields")
            };
            BrokerError::invalid(field, expected)
        })?;
        let map = |e: crate::app::CommandError| BrokerError::Execution(e.to_string());
        if request.input_refs.len() > 16
            || request.expected_sha256.as_ref().is_some_and(|hash| {
                hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Err(BrokerError::Malformed);
        }
        let mut names = BTreeSet::new();
        // A file derived from third-party text keeps that class.
        let mut trust = crate::custom_connections::taint::TRUST_USER_FILE;
        for input in &request.input_refs {
            if input.name.is_empty()
                || input.name.len() > 64
                || !names.insert(&input.name)
                || !input
                    .name
                    .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                || input.dataset_id.is_empty()
                || input.dataset_id.len() > 256
                || !input
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || input.revision == 0
                || input
                    .pointer
                    .as_ref()
                    .is_some_and(|p| !crate::util::json_pointer::valid_json_pointer(p, 2048, true))
            {
                return Err(BrokerError::Malformed);
            }
            let (_, payload, input_metadata) = self
                .state
                .store
                .dataset(&self.loupist_id, &input.dataset_id, input.revision)
                .map_err(map)?;
            if !crate::custom_connections::taint::retained_trusted(&input_metadata) {
                trust = crate::custom_connections::taint::TRUST_THIRD_PARTY_TEXT;
            }
            if input
                .pointer
                .as_ref()
                .is_some_and(|p| payload.pointer(p).is_none())
            {
                return Err(BrokerError::Execution(
                    "Input dataset pointer is absent".into(),
                ));
            }
        }
        let profile = self
            .state
            .store
            .get_loupist(&self.loupist_id)
            .map_err(|e| BrokerError::Execution(e.to_string()))?
            .ok_or_else(|| BrokerError::Execution("Loupist does not exist".into()))?;
        let root = crate::fs::loupist_root::profile_workspace(&profile).map_err(map)?;
        let path = crate::fs::workspace_files::relative_path(&request.path).map_err(map)?;
        if path == ".loupist" || path.starts_with(".loupist/") {
            return Err(BrokerError::MethodDenied);
        }
        let bytes = crate::fs::workspace_files::exact_bytes(&root, &path, MAX_DATASET_BYTES)
            .map_err(map)?;
        let digest = sha256(&bytes);
        if crate::commands::export_trust::contains(&self.state, &digest).map_err(map)? {
            trust = crate::custom_connections::taint::TRUST_THIRD_PARTY_TEXT;
        }
        if request
            .expected_sha256
            .as_ref()
            .is_some_and(|expected| !expected.eq_ignore_ascii_case(&digest))
        {
            return Err(BrokerError::Execution(
                "Workspace file changed; inspect it before importing".into(),
            ));
        }
        let payload: Value = serde_json::from_slice(&bytes).map_err(|e| {
            BrokerError::Execution(format!("Dataset import requires UTF-8 JSON: {e}"))
        })?;
        let metadata = json!({
            "origin":"local_file", "path":path, "content_sha256":digest,
            "size_bytes":bytes.len(), "input_refs":request.input_refs,
            "lineage":"declared inputs; execution and external origin are unverified",
            "coverage":{"retained":"complete parsed local JSON value"},
            "trust": trust
        });
        let request_key = sha256(
            json!({"kind":"workspace-json/1", "path":path})
                .to_string()
                .as_bytes(),
        );
        let reference = self
            .state
            .store
            .dataset_put(&self.loupist_id, &request_key, &payload, metadata)
            .map_err(map)?;
        let description = self
            .state
            .store
            .dataset_describe(&self.loupist_id, &reference.dataset_id, reference.revision)
            .map_err(map)?;
        Ok(
            json!({"dataset_ref":reference,"path":path,"content_sha256":digest,"size_bytes":bytes.len(),
            "data_summary":description,"source_class":"local_file","warnings":["Local data snapshot. Declared inputs do not verify execution, external origin, or historical availability."]}),
        )
    }
}
