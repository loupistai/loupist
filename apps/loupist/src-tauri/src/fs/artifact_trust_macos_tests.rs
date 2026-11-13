//! Real-signature corpus for the release-only gate, also compiled into Mac unit tests.
use super::*;
use std::{ffi::OsStr, path::PathBuf, process::Output};

struct SignedFixture {
    root: tempfile::TempDir,
    home: PathBuf,
    keychain: PathBuf,
}

impl SignedFixture {
    fn command(&self, program: &str, args: &[&OsStr]) -> Output {
        Command::new(program)
            .args(args)
            .env("HOME", &self.home)
            .env("CFFIXED_USER_HOME", &self.home)
            .output()
            .unwrap()
    }

    fn require(&self, program: &str, args: &[&OsStr]) -> Output {
        let output = self.command(program, args);
        assert!(
            output.status.success(),
            "fixture command {program}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn new() -> Option<Self> {
        for tool in [
            "/usr/bin/codesign",
            "/usr/bin/security",
            "/usr/bin/openssl",
            "/usr/bin/clang",
        ] {
            if !Path::new(tool).is_file() {
                eprintln!("skipping real macOS signatures: {tool} is missing");
                return None;
            }
        }
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        std::fs::create_dir_all(home.join("Library/Preferences")).unwrap();
        std::fs::create_dir_all(home.join("Library/Keychains")).unwrap();
        let keychain = home.join("Library/Keychains/test.keychain-db");
        let fixture = Self {
            root,
            home,
            keychain,
        };
        fixture.require(
            "/usr/bin/security",
            &[
                "create-keychain".as_ref(),
                "-p".as_ref(),
                "fixture".as_ref(),
                fixture.keychain.as_os_str(),
            ],
        );
        fixture.require(
            "/usr/bin/security",
            &[
                "unlock-keychain".as_ref(),
                "-p".as_ref(),
                "fixture".as_ref(),
                fixture.keychain.as_os_str(),
            ],
        );
        fixture.require(
            "/usr/bin/security",
            &[
                "list-keychains".as_ref(),
                "-d".as_ref(),
                "user".as_ref(),
                "-s".as_ref(),
                fixture.keychain.as_os_str(),
            ],
        );
        Some(fixture)
    }

    fn certificate(&self, name: &str) -> String {
        let config = self.root.path().join(format!("{name}.cnf"));
        std::fs::write(&config, format!("[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN=Loupist Fixture {name}\n[ext]\nbasicConstraints=critical,CA:false\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=critical,codeSigning\n")).unwrap();
        let key = self.root.path().join(format!("{name}.key"));
        let cert = self.root.path().join(format!("{name}.pem"));
        let p12 = self.root.path().join(format!("{name}.p12"));
        self.require(
            "/usr/bin/openssl",
            &[
                "req".as_ref(),
                "-x509".as_ref(),
                "-newkey".as_ref(),
                "rsa:2048".as_ref(),
                "-sha256".as_ref(),
                "-days".as_ref(),
                "2".as_ref(),
                "-nodes".as_ref(),
                "-keyout".as_ref(),
                key.as_os_str(),
                "-out".as_ref(),
                cert.as_os_str(),
                "-config".as_ref(),
                config.as_os_str(),
            ],
        );
        self.require(
            "/usr/bin/openssl",
            &[
                "pkcs12".as_ref(),
                "-export".as_ref(),
                "-inkey".as_ref(),
                key.as_os_str(),
                "-in".as_ref(),
                cert.as_os_str(),
                "-out".as_ref(),
                p12.as_os_str(),
                "-passout".as_ref(),
                "pass:fixture".as_ref(),
            ],
        );
        self.require(
            "/usr/bin/security",
            &[
                "import".as_ref(),
                p12.as_os_str(),
                "-P".as_ref(),
                "fixture".as_ref(),
                "-x".as_ref(),
                "-T".as_ref(),
                "/usr/bin/codesign".as_ref(),
                "-t".as_ref(),
                "cert".as_ref(),
                "-f".as_ref(),
                "pkcs12".as_ref(),
                "-k".as_ref(),
                self.keychain.as_os_str(),
            ],
        );
        self.require(
            "/usr/bin/security",
            &[
                "set-key-partition-list".as_ref(),
                "-S".as_ref(),
                "apple-tool:,apple:,codesign:".as_ref(),
                "-s".as_ref(),
                "-k".as_ref(),
                "fixture".as_ref(),
                self.keychain.as_os_str(),
            ],
        );
        let output = self.require(
            "/usr/bin/openssl",
            &[
                "x509".as_ref(),
                "-in".as_ref(),
                cert.as_os_str(),
                "-noout".as_ref(),
                "-fingerprint".as_ref(),
                "-sha1".as_ref(),
            ],
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .rsplit('=')
            .next()
            .unwrap()
            .replace(':', "")
    }

    fn binary(&self, destination: &Path) {
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        let source = self.root.path().join("fixture.c");
        std::fs::write(&source, "int main(void) { return 0; }\n").unwrap();
        self.require(
            "/usr/bin/clang",
            &[source.as_os_str(), "-o".as_ref(), destination.as_os_str()],
        );
    }

    fn sign(&self, path: &Path, identity: &str) {
        self.require(
            "/usr/bin/codesign",
            &[
                "--force".as_ref(),
                "--sign".as_ref(),
                identity.as_ref(),
                path.as_os_str(),
            ],
        );
    }
}

impl Drop for SignedFixture {
    fn drop(&mut self) {
        if !self.keychain.exists() {
            return;
        }
        let output = self.command(
            "/usr/bin/security",
            &["delete-keychain".as_ref(), self.keychain.as_os_str()],
        );
        assert!(
            output.status.success(),
            "temporary fixture keychain cleanup failed"
        );
    }
}

#[test]
fn real_signatures_accept_only_the_apps_signer_and_sealed_bundle() {
    let Some(fixture) = SignedFixture::new() else {
        return;
    };
    let first = fixture.certificate("first");
    let second = fixture.certificate("second");
    let app = fixture.root.path().join("Loupist.app");
    let current = app.join("Contents/MacOS/loupist");
    let helper = app.join("Contents/Resources/worker");
    fixture.binary(&current);
    fixture.binary(&helper);
    std::fs::write(app.join("Contents/Info.plist"), "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.monarchjuno.loupist</string><key>CFBundleExecutable</key><string>loupist</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>").unwrap();
    fixture.sign(&helper, &first);
    fixture.sign(&app, &first);
    verify_macos_identity_for_app(&current, &helper).expect("same actual leaf certificate");

    let outside = fixture.root.path().join("outside");
    fixture.binary(&outside);
    fixture.sign(&outside, &first);
    assert!(
        verify_macos_identity_for_app(&current, &outside).is_err(),
        "outside bundle"
    );
    let link = app.join("Contents/Resources/link");
    std::os::unix::fs::symlink(&helper, &link).unwrap();
    fixture.sign(&app, &first);
    assert!(
        verify_macos_identity_for_app(&current, &link).is_err(),
        "linked helper"
    );
    std::fs::remove_file(&link).unwrap();

    fixture.sign(&helper, &second);
    fixture.sign(&app, &first);
    verify_codesign(&app, true).unwrap();
    assert!(
        verify_macos_identity_for_app(&current, &helper).is_err(),
        "different certificate in an otherwise valid sealed app"
    );
    fixture.sign(&helper, "-");
    fixture.sign(&app, &first);
    assert!(
        verify_macos_identity_for_app(&current, &helper).is_err(),
        "ad-hoc helper"
    );
    fixture.sign(&helper, &first);
    fixture.sign(&app, "-");
    assert!(
        verify_macos_identity_for_app(&current, &helper).is_err(),
        "ad-hoc app"
    );
    fixture.sign(&helper, "-");
    fixture.sign(&app, "-");
    assert!(
        verify_macos_identity_for_app(&current, &helper).is_err(),
        "two ad-hoc identities"
    );
    fixture.require(
        "/usr/bin/codesign",
        &["--remove-signature".as_ref(), helper.as_os_str()],
    );
    fixture.sign(&app, &first);
    assert!(
        verify_macos_identity_for_app(&current, &helper).is_err(),
        "unsigned helper"
    );
    fixture.sign(&helper, &first);
    fixture.sign(&app, &first);
    let mut bytes = std::fs::read(&helper).unwrap();
    bytes[4096] ^= 1;
    std::fs::write(&helper, bytes).unwrap();
    assert!(
        verify_macos_identity_for_app(&current, &helper).is_err(),
        "tampered helper"
    );
    fixture.sign(&app, &first);
    assert!(
        verify_macos_identity_for_app(&current, &helper).is_err(),
        "invalid helper signature even after app re-seal"
    );
    fixture.sign(&helper, &first);
    fixture.sign(&app, &first);
    verify_macos_identity_for_app(&current, &helper).unwrap();
    fixture.require(
        "/usr/bin/codesign",
        &["--remove-signature".as_ref(), current.as_os_str()],
    );
    assert!(
        verify_macos_identity_for_app(&current, &helper).is_err(),
        "unsigned running app"
    );
}
