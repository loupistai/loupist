#[cfg(feature = "e2e")]
use std::path::PathBuf;

use serde::Deserialize;

use crate::app::CommandError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileFilter {
    name: String,
    extensions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickFileRequest {
    title: String,
    filters: Vec<FileFilter>,
}

fn normalized_filters(
    filters: Vec<FileFilter>,
) -> Result<Vec<(String, Vec<String>)>, CommandError> {
    filters
        .into_iter()
        .map(|filter| {
            let extensions: Vec<String> = filter
                .extensions
                .into_iter()
                .map(|extension| {
                    extension
                        .trim()
                        .trim_start_matches('.')
                        .to_ascii_lowercase()
                })
                .collect();
            if extensions.is_empty()
                || extensions.iter().any(|extension| {
                    extension.is_empty()
                        || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
                })
            {
                return Err(CommandError::invalid(
                    "file filters require plain file extensions",
                ));
            }
            Ok((filter.name, extensions))
        })
        .collect()
}

/// Same e2e substitution as `loupist_choose_folder`: a debug build can return a
/// fixture path because the macOS WebDriver delegate cannot open the panel.
/// Empty and unset values still open the dialog. A release build never reads
/// the variable.
fn e2e_pick_override(raw: Option<&std::ffi::OsStr>) -> Result<Option<String>, CommandError> {
    #[cfg(feature = "e2e")]
    {
        let Some(raw) = raw.filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let path = PathBuf::from(raw);
        if !path.is_absolute() {
            return Err(CommandError::invalid(
                "e2e pick file must be an absolute path",
            ));
        }
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| CommandError::invalid("e2e pick file must be an existing file"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(CommandError::invalid(
                "e2e pick file must be an existing file",
            ));
        }
        path.to_str()
            .map(|path| Some(path.to_owned()))
            .ok_or_else(|| CommandError::invalid("e2e pick file path must be valid Unicode"))
    }
    #[cfg(not(feature = "e2e"))]
    {
        let _ = raw;
        Ok(None)
    }
}

fn e2e_picked_file() -> Result<Option<String>, CommandError> {
    #[cfg(feature = "e2e")]
    let raw = std::env::var_os("LOUPIST_E2E_PICK_FILE");
    #[cfg(not(feature = "e2e"))]
    let raw: Option<std::ffi::OsString> = None;
    e2e_pick_override(raw.as_deref())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn pick_file(request: PickFileRequest) -> Result<Option<String>, CommandError> {
    let filters = normalized_filters(request.filters)?;
    if let Some(path) = e2e_picked_file()? {
        return Ok(Some(path));
    }
    let mut dialog = rfd::AsyncFileDialog::new().set_title(request.title);
    for (name, extensions) in filters {
        dialog = dialog.add_filter(name, &extensions);
    }
    dialog
        .pick_file()
        .await
        .map(|file| {
            file.path()
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| CommandError::invalid("file path must be valid Unicode"))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_file_filter_mapping_preserves_labels_and_normalizes_extensions() {
        let request: PickFileRequest = serde_json::from_value(serde_json::json!({
            "title": "Import holdings", "filters": [
                { "name": "Holdings", "extensions": [" .CSV ", "tsv"] },
                { "name": "Text", "extensions": ["TXT"] }
            ]
        }))
        .unwrap();
        assert_eq!(
            normalized_filters(request.filters).unwrap(),
            vec![
                (
                    "Holdings".to_owned(),
                    vec!["csv".to_owned(), "tsv".to_owned()]
                ),
                ("Text".to_owned(), vec!["txt".to_owned()]),
            ]
        );
        for invalid in ["", "*", "../csv", "csv;exe"] {
            assert!(normalized_filters(vec![FileFilter {
                name: "Invalid".into(),
                extensions: vec![invalid.into()]
            }])
            .is_err());
        }
    }

    #[cfg(not(feature = "e2e"))]
    #[test]
    fn e2e_pick_file_variable_is_ignored_without_the_e2e_feature() {
        let path = std::env::temp_dir().join(format!(
            "loupist-e2e-pick-ignore-{}.csv",
            std::process::id()
        ));
        std::fs::write(
            &path,
            b"symbol,market,name,quantity\nNWHLD,US,Northwind,1\n",
        )
        .unwrap();
        let result = e2e_pick_override(Some(path.as_os_str()));
        let _ = std::fs::remove_file(&path);
        assert_eq!(result.unwrap(), None);
        assert_eq!(e2e_picked_file().unwrap(), None);
    }

    #[cfg(feature = "e2e")]
    #[test]
    fn e2e_pick_file_returns_only_an_absolute_existing_file() {
        let path =
            std::env::temp_dir().join(format!("loupist-e2e-pick-allow-{}.csv", std::process::id()));
        std::fs::write(
            &path,
            b"symbol,market,name,quantity\nNWHLD,US,Northwind,1\n",
        )
        .unwrap();
        let selected = e2e_pick_override(Some(path.as_os_str())).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(selected.as_deref(), path.to_str());
        assert_eq!(e2e_pick_override(None).unwrap(), None);
        assert_eq!(
            e2e_pick_override(Some(std::ffi::OsStr::new(""))).unwrap(),
            None
        );
        assert!(e2e_pick_override(Some(std::ffi::OsStr::new("holdings.csv"))).is_err());
        assert!(e2e_pick_override(Some(std::env::temp_dir().as_os_str())).is_err());
    }
}
