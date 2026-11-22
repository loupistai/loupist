# Install Loupist (Beta)

## macOS 13 or newer on Apple Silicon

**Install Loupist (Beta)**

Loupist runs on macOS 13 or newer on Apple Silicon. Windows, Intel Macs and Linux are not supported.

1. Download [Loupist-macOS-arm64.dmg](https://github.com/loupistai/loupist/releases/latest/download/Loupist-macOS-arm64.dmg).
2. Open the file and drag Loupist to Applications.
3. Open Loupist from Applications. Starting with 0.1.3, the release is signed with Developer ID and notarized by Apple. On first launch, you should see the standard macOS confirmation for an app downloaded from the internet. If it appears, click **Open**.

**If you downloaded 0.1.2**

That download uses Loupist’s self-signed certificate. If macOS blocks its first launch, click **OK** on macOS 13 or 14, or **Done** on macOS 15 or later. Open **System Settings → Privacy & Security**, find the message about Loupist under **Security**, and click **Open Anyway**. Confirm with your Mac login password if requested. If the button has disappeared, open the app again to bring back the prompt.

If macOS says Loupist "is damaged and can't be opened", the download may be incomplete or may have been changed. Delete it, download again, and check it as described below. Do not work around that message.

**Verify your download**

Checksum. Download `SHA256SUMS` from the same release and compare:

```sh
shasum -a 256 ~/Downloads/Loupist-macOS-arm64.dmg
```

Build provenance. With the [GitHub CLI](https://cli.github.com), check that GitHub built this exact file from this repository's release workflow:

```sh
gh attestation verify ~/Downloads/Loupist-macOS-arm64.dmg --repo loupistai/loupist \
  --signer-workflow loupistai/loupist/.github/workflows/release.yml
```

Publisher. For 0.1.3 and later Developer ID releases, look for `TeamIdentifier=FX2J28HNK6` in the signature details and compare that team ID with `macos_signing.team_id` in the same release’s `RELEASE-METADATA.json`. To check the signature and stapled notarization ticket, run:

```sh
codesign --verify --deep --strict -R '=anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists' /Applications/Loupist.app
codesign -dvv /Applications/Loupist.app
xcrun stapler validate /Applications/Loupist.app
```

**What Beta means**

- The app is complete enough to use for real research, and it will change. Expect rough edges and report them in [Issues](https://github.com/loupistai/loupist/issues).
- It has been tested by its maintainer on one Mac. It has not been tested on every supported macOS version.
- Your data stays in your Loupist folders and is upgraded in place by later versions, after a backup is written beside each database. Downgrading is not supported.
- When updating from 0.1.2, macOS asks once per saved Data connection key because the app’s signing identity changes. Enter your Mac login password and click **Always Allow**. Later updates from the same Developer ID team should retain access to that key without another prompt. If you choose **Deny**, the key stays saved. Open **Settings → Data connections → Try again** to grant access later. A saved GitHub sharing token can prompt in the same way. AI sign-ins are unaffected.
- Loupist never trades and is not a licensed adviser.

## 한국어 설치 안내

**Loupist(베타) 설치**

Loupist는 Apple Silicon Mac의 macOS 13 이상에서 동작합니다. Windows, Intel Mac, Linux는 지원하지 않습니다.

1. [Loupist-macOS-arm64.dmg](https://github.com/loupistai/loupist/releases/latest/download/Loupist-macOS-arm64.dmg)를 내려받습니다.
2. 파일을 열고 Loupist를 응용 프로그램 폴더로 끌어다 놓습니다.
3. 응용 프로그램 폴더에서 Loupist를 엽니다. 0.1.3부터 배포판은 Developer ID로 서명되며 Apple 공증을 받습니다. 처음 실행하면 인터넷에서 내려받은 앱을 열지 묻는 macOS 확인 창이 나타날 수 있습니다. 창이 나타나면 **열기**를 누르십시오.

**0.1.2를 내려받은 경우**

0.1.2는 Loupist 자체 인증서로 서명되었습니다. macOS가 첫 실행을 차단하면 macOS 13·14에서는 **확인**, macOS 15 이상에서는 **완료**를 누르십시오. **시스템 설정 → 개인정보 보호 및 보안**의 **보안** 항목에서 Loupist 옆의 **그래도 열기**를 누르십시오. 요청 시 Mac 로그인 암호를 입력하십시오. 버튼이 사라졌다면 앱을 다시 열어 경고를 표시하십시오.

macOS가 Loupist가 "손상되었기 때문에 열 수 없습니다"라고 하면 다운로드가 불완전하거나 파일이 바뀌었을 수 있습니다. 파일을 지우고 다시 내려받은 뒤 아래 방법으로 확인하십시오. 이 메시지는 우회하지 마십시오.

**다운로드 확인**

체크섬. 같은 릴리스의 `SHA256SUMS`를 내려받아 값을 비교합니다.

```sh
shasum -a 256 ~/Downloads/Loupist-macOS-arm64.dmg
```

빌드 출처. [GitHub CLI](https://cli.github.com)로, GitHub가 이 저장소의 릴리스 워크플로에서 바로 이 파일을 빌드했는지 확인합니다.

```sh
gh attestation verify ~/Downloads/Loupist-macOS-arm64.dmg --repo loupistai/loupist \
  --signer-workflow loupistai/loupist/.github/workflows/release.yml
```

배포자. Developer ID로 서명된 0.1.3 이상 배포판은 앱의 서명 정보에서 `TeamIdentifier=FX2J28HNK6`을 확인하십시오. 이 값은 해당 릴리스의 `RELEASE-METADATA.json`에 기록된 `macos_signing.team_id`와 일치합니다. 다음 명령으로 서명과 앱에 첨부된 공증 티켓을 확인할 수 있습니다.

```sh
codesign --verify --deep --strict -R '=anchor apple generic and certificate leaf[field.1.2.840.113635.100.6.1.13] exists' /Applications/Loupist.app
codesign -dvv /Applications/Loupist.app
xcrun stapler validate /Applications/Loupist.app
```

**베타의 의미**

- 실제 리서치에 쓸 수 있을 만큼 갖춰져 있지만 앞으로 바뀝니다. 거친 부분이 있을 수 있으니 [Issues](https://github.com/loupistai/loupist/issues)에 알려 주십시오.
- 관리자가 Mac 한 대에서 테스트했습니다. 지원하는 모든 macOS 버전에서 테스트하지는 못했습니다.
- 데이터는 각 Loupist 폴더에 남고, 이후 버전이 데이터베이스 옆에 백업을 만든 뒤 그 자리에서 업그레이드합니다. 다운그레이드는 지원하지 않습니다.
- 0.1.2에서 업데이트하면 앱의 서명 정보가 바뀌기 때문에 macOS가 저장된 데이터 연결 키마다 한 번씩 접근 권한을 묻습니다. Mac 로그인 암호를 입력하고 **항상 허용**을 누르십시오. 이후 같은 Developer ID 팀이 서명한 업데이트에서는 다시 묻지 않고 키를 사용할 수 있을 것으로 예상합니다. **거부**해도 키는 저장된 상태로 남습니다. 나중에 **설정 → 데이터 연결 → 다시 시도**에서 접근을 허용할 수 있습니다. 저장된 GitHub 공유 토큰에도 같은 요청이 나타날 수 있습니다. AI 로그인에는 영향이 없습니다.
- Loupist는 매매를 하지 않으며 인가받은 투자자문업자가 아닙니다.

## Updates

Loupist checks the stable signed update feed daily after a successful check, with shorter retries after failures. Settings → Updates also provides **Check for updates**. Download, installation, and restart always require user confirmation. Prereleases are never offered on the stable channel.

Settings shows update errors and the kinds of work blocking installation in the
interface language, never the host's diagnostic strings. A background check failure
says Loupist will retry later; a manual failure asks the user to check the internet
connection and try again. Download or signature-verification failure, installation
failure and a changed update offer each have their own localized guidance. Unknown
host failures use a localized message for the failed operation. If macOS is
still handling a saved-key write or removal, an installation request reports
that blocker immediately. Retry once the Keychain operation finishes.

Compatible development databases are upgraded in place on first open, after a private backup is written beside the database. Formats older than the supported baseline, and databases from newer versions, are refused and left unchanged.
