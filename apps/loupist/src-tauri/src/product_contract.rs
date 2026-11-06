use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn text(path: &str) -> String {
    fs::read_to_string(root().join(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn json(path: &str) -> Value {
    serde_json::from_str(&text(path)).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn cargo_package_version(path: &str) -> String {
    text(path)
        .lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|version| version.strip_suffix('"'))
        })
        .unwrap_or_else(|| panic!("{path}: package version"))
        .to_owned()
}

fn cargo_lock_package_version(path: &str, package: &str) -> String {
    let lock = text(path);
    let package_name = format!("name = \"{package}\"");
    lock.split("[[package]]")
        .find(|entry| entry.lines().any(|line| line == package_name.as_str()))
        .and_then(|entry| {
            entry.lines().find_map(|line| {
                line.strip_prefix("version = \"")
                    .and_then(|version| version.strip_suffix('"'))
            })
        })
        .unwrap_or_else(|| panic!("{path}: {package} package version"))
        .to_owned()
}

fn assert_product_version(version: &str) {
    let valid_number = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    let (base, rc) = match version.split_once("-rc.") {
        Some((base, rc)) => (base, Some(rc)),
        None => (version, None),
    };
    let mut parts = base.split('.');
    let canonical = valid_number(parts.next().unwrap_or_default())
        && valid_number(parts.next().unwrap_or_default())
        && valid_number(parts.next().unwrap_or_default())
        && parts.next().is_none()
        && rc.is_none_or(|sequence| valid_number(sequence) && sequence != "0");
    assert!(
        canonical,
        "product version must be X.Y.Z or X.Y.Z-rc.N: {version}"
    );
}

fn assert_macos_bundle_version(version: &str) {
    let components = version.split('.').collect::<Vec<_>>();
    assert!(
        (1..=3).contains(&components.len())
            && components.iter().all(|component| {
                !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
            })
            && components.iter().any(|component| {
                component
                    .parse::<u64>()
                    .is_ok_and(|component| component > 0)
            }),
        "macOS bundle version must be a positive 1-3 component build number: {version}"
    );
}

#[test]
fn product_versions_are_consistent_and_release_compatible() {
    let product_version = cargo_package_version("apps/loupist/src-tauri/Cargo.toml");
    assert_product_version(&product_version);
    let product_base_version = product_version.split('-').next().unwrap();

    let app = json("apps/loupist/package.json");
    let package_lock = json("apps/loupist/package-lock.json");
    let tauri = json("apps/loupist/src-tauri/tauri.conf.json");
    assert_eq!(app["version"].as_str(), Some(product_version.as_str()));
    assert_eq!(
        package_lock["version"].as_str(),
        Some(product_version.as_str())
    );
    assert_eq!(
        package_lock["packages"][""]["version"].as_str(),
        Some(product_version.as_str())
    );
    assert_eq!(app["devDependencies"]["@earendil-works/pi-ai"], "1.5.0");
    assert_eq!(
        app["devDependencies"]["@earendil-works/pi-coding-agent"],
        "1.5.0"
    );
    assert_eq!(tauri["version"].as_str(), Some(product_version.as_str()));
    assert_macos_bundle_version(
        tauri["bundle"]["macOS"]["bundleVersion"]
            .as_str()
            .expect("macOS bundle version"),
    );

    let info_plist = text("apps/loupist/src-tauri/Info.plist");
    assert!(info_plist.contains("<key>CFBundleShortVersionString</key>"));
    assert!(info_plist.contains(&format!("<string>{product_base_version}</string>")));
    assert_eq!(
        cargo_lock_package_version("apps/loupist/src-tauri/Cargo.lock", "loupist-desktop"),
        product_version
    );
}

#[test]
fn korea_investment_catalog_is_read_only() {
    let manifest = json("apps/loupist/marketplace/kis-read-api-v1.json");
    assert_eq!(manifest["policy"]["orders_included"], false);
    let operations = manifest["operations"].as_array().unwrap();
    assert!(operations
        .iter()
        .all(|operation| operation["http_method"] == "GET"));
}

#[test]
fn release_workflows_pin_actions_and_identity() {
    let release = text(".github/workflows/release.yml");
    let ci = text(".github/workflows/ci.yml");
    let qualification = text(".github/workflows/release-qualification.yml");
    let promotion = text(".github/workflows/promote-release.yml");
    let setup = text(".github/actions/setup-toolchains/action.yml");
    assert!(release.contains("test \"$GITHUB_REPOSITORY\" = \"loupistai/loupist\""));
    for workflow in [&release, &ci, &qualification, &promotion, &setup] {
        for line in workflow.lines().map(str::trim) {
            let Some(action) = line.strip_prefix("uses: ") else {
                continue;
            };
            if action.starts_with("./") {
                continue;
            }
            let revision = action
                .split_once('@')
                .map(|(_, revision)| revision)
                .and_then(|revision| revision.split_whitespace().next())
                .expect("workflow action has an immutable revision");
            assert!(
                revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "workflow action is not pinned to a commit: {action}"
            );
        }
    }
    for required in [
        "toolchain: 1.97.1",
        "version: 0.11.2",
        "syft-version: v1.50.0",
    ] {
        assert!(
            release.contains(required) || ci.contains(required) || setup.contains(required),
            "workflow toolchain pin is missing: {required}"
        );
    }

    assert!(promotion.contains("environment: stable-release"));

    let stage_pi_macos = text("apps/loupist/scripts/stage-pi-macos-arm64.sh");
    let stage_pi_windows = text("apps/loupist/scripts/stage-pi-windows-x64.ps1");
    let pi_runtime = text("apps/loupist/src-tauri/src/pi.rs");
    assert!(stage_pi_macos.contains("PI_VERSION=1.5.0"));
    assert!(stage_pi_windows.contains("$piVersion = \"1.5.0\""));
    let macos_pi_archive_sha256 =
        "a9bf0d82d58d2a7d337f4ae4171b5fc9ebffe42509527a831364c4b65adf2cdf";
    let windows_pi_archive_sha256 =
        "12d58a70e80b16ba9f764950b7a42bdeb07959495c0db68396cf2988c0b71af3";
    assert!(stage_pi_macos.contains(macos_pi_archive_sha256));
    assert!(stage_pi_windows.contains(windows_pi_archive_sha256));
    assert!(pi_runtime.contains(macos_pi_archive_sha256));
    assert!(pi_runtime.contains(windows_pi_archive_sha256));
}
