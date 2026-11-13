# Install Loupist (Beta)

## macOS 13 or newer on Apple Silicon

**Install Loupist (Beta)**

Loupist runs on macOS 13 or newer on Apple Silicon. Windows, Intel Macs and Linux are not supported.

1. Download [Loupist-macOS-arm64.dmg](https://github.com/loupistai/loupist/releases/latest/download/Loupist-macOS-arm64.dmg).
2. Open the file and drag Loupist to Applications.
3. Open Loupist from Applications. **macOS will refuse the first time.** This Beta is not notarized by Apple, so macOS shows a warning that it cannot verify or open the app. Click **Done** (or **Cancel**). Do not click Move to Trash.
4. Allow it once:
   - **macOS 15 or newer:** open **System Settings → Privacy & Security**, scroll down to **Security**, and click **Open Anyway** next to the line about Loupist. Confirm with **Open Anyway** and enter your login password. The button is shown for about an hour after step 3.
   - **macOS 13 or 14:** in Finder, Control-click (right-click) Loupist in Applications, choose **Open**, then click **Open** in the dialog. The **Open Anyway** button in System Settings → Privacy & Security works too.
5. From now on Loupist opens normally. Updates install inside the app and do not ask again.

If macOS says Loupist "is damaged and can't be opened", the download may be incomplete or may have been changed. Delete it, download again, and check it as described below. Do not work around that message.

**Why the warning?** Apple's notarization requires a paid Apple Developer membership, which this Beta does not have yet. The app is signed with Loupist's own certificate instead, so macOS can tell that an update comes from the same publisher, but Apple has not scanned it. You can check for yourself that your download is the file built from the public source code: see "Verify your download".

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

Publisher. After installing, the app's signature should name Loupist's certificate. The fingerprint is in `RELEASE-METADATA.json` and in `apps/loupist/release-profile.json`:

```sh
codesign -d -r- /Applications/Loupist.app
# designated => identifier "com.monarchjuno.loupist" and certificate root = H"<fingerprint>"
```

**What Beta means**

- The app is complete enough to use for real research, and it will change. Expect rough edges and report them in [Issues](https://github.com/loupistai/loupist/issues).
- It is not notarized by Apple. First launch needs the one-time step above.
- It has been tested by its maintainer on one Mac. It has not been tested on every supported macOS version.
- Your data stays in your Loupist folders and is upgraded in place by later versions, after a backup is written beside each database. Downgrading is not supported.
- A later version will switch to an Apple Developer ID signature. On that update macOS will ask once for permission to use each saved Data connection key. Choosing Always Allow settles it.
- Loupist never trades and is not a licensed adviser.

## 한국어 설치 안내

**Loupist(베타) 설치**

Loupist는 Apple Silicon Mac의 macOS 13 이상에서 동작합니다. Windows, Intel Mac, Linux는 지원하지 않습니다.

1. [Loupist-macOS-arm64.dmg](https://github.com/loupistai/loupist/releases/latest/download/Loupist-macOS-arm64.dmg)를 내려받습니다.
2. 파일을 열고 Loupist를 응용 프로그램 폴더로 끌어다 놓습니다.
3. 응용 프로그램 폴더에서 Loupist를 엽니다. **처음에는 macOS가 실행을 막습니다.** 이 베타는 Apple 공증을 받지 않았기 때문에 macOS가 앱을 확인하거나 열 수 없다는 경고를 표시합니다. **완료**(또는 **취소**)를 누르세요. "휴지통으로 이동"은 누르지 마세요.
4. 한 번만 허용합니다.
   - **macOS 15 이상:** **시스템 설정 → 개인정보 보호 및 보안**을 열고 아래쪽 **보안** 항목에서 Loupist 관련 문구 옆의 **그래도 열기**를 누릅니다. 다시 **그래도 열기**를 누르고 로그인 암호를 입력합니다. 이 버튼은 3번에서 실행을 시도한 뒤 약 1시간 동안만 보입니다.
   - **macOS 13, 14:** Finder의 응용 프로그램 폴더에서 Loupist를 Control-클릭(오른쪽 클릭)하고 **열기**를 선택한 다음, 대화상자에서 **열기**를 누릅니다. 시스템 설정 → 개인정보 보호 및 보안의 **그래도 열기** 버튼으로도 됩니다.
5. 이후에는 평소처럼 열립니다. 업데이트는 앱 안에서 설치되며 이 과정을 다시 거치지 않습니다.

macOS가 Loupist가 "손상되었기 때문에 열 수 없습니다"라고 하면 다운로드가 불완전하거나 파일이 바뀌었을 수 있습니다. 파일을 지우고 다시 내려받은 뒤 아래 방법으로 확인하세요. 이 메시지는 우회하지 마세요.

**왜 경고가 나오나요?** Apple 공증에는 유료 Apple Developer 멤버십이 필요한데, 이 베타는 아직 가입하지 않았습니다. 대신 Loupist 자체 인증서로 서명했습니다. 그래서 macOS는 업데이트가 같은 배포자에게서 왔는지는 알 수 있지만, Apple이 앱을 검사한 것은 아닙니다. 내려받은 파일이 공개된 소스 코드로 빌드된 바로 그 파일인지 직접 확인할 수 있습니다. "다운로드 확인"을 참고하세요.

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

배포자. 설치 후 앱 서명에 Loupist 인증서가 표시되어야 합니다. 지문은 `RELEASE-METADATA.json`과 `apps/loupist/release-profile.json`에 있습니다.

```sh
codesign -d -r- /Applications/Loupist.app
# designated => identifier "com.monarchjuno.loupist" and certificate root = H"<지문>"
```

**베타의 의미**

- 실제 리서치에 쓸 수 있을 만큼 갖춰져 있지만 앞으로 바뀝니다. 거친 부분이 있을 수 있으니 [Issues](https://github.com/loupistai/loupist/issues)에 알려 주세요.
- Apple 공증을 받지 않았습니다. 첫 실행 때 위의 절차가 한 번 필요합니다.
- 관리자가 Mac 한 대에서 테스트했습니다. 지원하는 모든 macOS 버전에서 테스트하지는 못했습니다.
- 데이터는 각 Loupist 폴더에 남고, 이후 버전이 데이터베이스 옆에 백업을 만든 뒤 그 자리에서 업그레이드합니다. 다운그레이드는 지원하지 않습니다.
- 이후 버전에서 Apple Developer ID 서명으로 바뀝니다. 그 업데이트 때 macOS가 저장된 데이터 연결 키마다 사용 허용을 한 번 묻습니다. "항상 허용"을 선택하면 됩니다.
- Loupist는 매매를 하지 않으며 인가받은 투자자문업자가 아닙니다.

## Updates

Loupist checks the stable signed update feed daily after a successful check, with shorter retries after failures. Settings → Updates also provides **Check for updates**. Download, installation, and restart always require user confirmation. Prereleases are never offered on the stable channel.

Compatible development databases are upgraded in place on first open, after a private backup is written beside the database. Formats older than the supported baseline, and databases from newer versions, are refused and left unchanged.
