use super::authority::bounded_text;
use super::*;

impl AppToolExecutor {
    pub(super) fn native_authorize(&self, params: &Value) -> Result<Value, BrokerError> {
        let mut result = json!({"allowed": true});
        if params["name"] == "read" {
            if let Some(path) = params.get("path").and_then(Value::as_str) {
                if crate::commands::export_trust::tagged_file(
                    &self.state,
                    std::path::Path::new(path),
                )
                .map_err(|error| BrokerError::Execution(error.to_string()))?
                {
                    self.capture.turn_taint.mark_untrusted_text();
                    result["untrusted_text"] =
                        crate::custom_connections::taint::native_read_delimiters();
                }
            }
        }
        Ok(result)
    }

    pub(super) fn workspace_capture(&self, params: Value) -> Result<Value, BrokerError> {
        let object = exact_object(&params, &["path"], &["expected_sha256"])?;
        let path = bounded_text(object.get("path"), 1, 8 * 1024)?;
        let expected = object
            .get("expected_sha256")
            .map(|value| bounded_text(Some(value), 64, 64))
            .transpose()?;
        if expected
            .as_ref()
            .is_some_and(|hash| !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err(BrokerError::Malformed);
        }
        let profile = self
            .state
            .store
            .get_loupist(&self.loupist_id)
            .map_err(|error| BrokerError::Execution(error.to_string()))?
            .ok_or_else(|| BrokerError::Execution("Loupist does not exist".into()))?;
        let root = crate::fs::loupist_root::profile_workspace(&profile)
            .map_err(|error| BrokerError::Execution(error.message))?;
        let file = crate::fs::workspace_files::exact_text(&root, &path, expected.as_deref())
            .map_err(|error| BrokerError::Execution(error.message))?;
        let untrusted = crate::commands::export_trust::contains(
            &self.state,
            file.content_sha256
                .as_deref()
                .ok_or(BrokerError::Malformed)?,
        )
        .map_err(|error| BrokerError::Execution(error.message))?;
        Ok(json!({
            "path": file.path,
            "content": file.content,
            "content_sha256": file.content_sha256,
            "media_type": file.media_type,
            "size_bytes": file.size_bytes,
            "source_class": "user_file",
            "trust": if untrusted { "third_party_text" } else { "user_file" },
            "warnings": ["Local file snapshot. Authorship and external origin are unverified; this does not establish independent corroboration."],
        }))
    }
}
