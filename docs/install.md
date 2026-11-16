# 설치

kairos를 설치, 업데이트, 제거하는 방법입니다. 설치한 뒤의 설정은 [설정](configuration.md)에 있습니다.

## 준비할 것

| 필요한 것 | 언제 |
|---|---|
| Apple Silicon Mac | 항상 |
| 로그인한 gh(`gh auth login`) | 항상 |
| Claude Code | 항상 |
| Orca 1.4.248 이상 | Orca 패널을 쓸 때 |
| Xcode Command Line Tools(`xcode-select --install`) | 저장소에서 빌드할 때 |
| Rust(rustup) | 저장소에서 빌드할 때. 버전은 `rust-toolchain.toml`이 정합니다 |

빠진 것이 있으면 설치 스크립트가 한꺼번에 모아 설치 명령과 함께 알려 주고 멈춥니다.

이 표는 macOS 기준입니다. Linux와 Windows는 [Linux에 설치하기](#linux에-설치하기)와 [Windows에 설치하기](#windows에-설치하기)를 보세요. OS마다 다른 점은 [지원하는 OS](platforms.md)에 있습니다.

## 설치하기

방법은 두 가지입니다. 어느 쪽이든 처음 설치와 업그레이드에 같은 명령을 씁니다. 기존 설치를 먼저 지울 필요가 없고, 설정과 데이터는 그대로 남습니다.

### 릴리스 패키지로 설치

빌드하지 않으므로 Rust와 Xcode가 없어도 됩니다.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.sh | zsh
```

스크립트는 GitHub의 최신 릴리스에서 패키지(`kairos-aarch64-apple-darwin.tar.gz`)와 SHA256 파일을 받습니다. 값이 일치할 때만 압축을 풀고, 패키지 안의 `scripts/install.sh`로 넘어갑니다.

받은 파일은 임시 폴더에 두었다가 끝나면 지웁니다. 그래서 setup이 설치할지 물을 때 `y`로 답해야 `kairos` 명령이 남습니다. 설치하지 않고 끝내면 스크립트가 그 사실을 알려 줍니다.

- 옵션은 `zsh -s -- --yes --google <회사 메일>`처럼 뒤에 붙입니다.
- 실행하기 전에 내용을 보고 싶다면 스크립트를 파일로 받아 `zsh install-release.sh`로 실행하세요.
- 사내 미러에서 받으려면 `KAIROS_RELEASE_URL`에 주소를 지정합니다.

> [!WARNING]
> SHA256 파일은 패키지와 같은 릴리스에서 받습니다. 전송 중에 손상된 파일은 걸러 내지만, 누가 올렸는지는 증명하지 못합니다. 패키지를 믿기 어렵다면 저장소에서 빌드하세요.

패키지의 알림 헬퍼는 ad-hoc으로 서명되어 있어서, 버전을 올린 뒤 macOS가 알림 권한을 다시 물을 수 있습니다.

패키지는 `v<버전>` 태그를 올리면 `.github/workflows/release.yml`이 `scripts/package.sh`로 만듭니다. 로컬에서 같은 명령을 실행하면 `target/dist/`에 만들어집니다.

### 저장소에서 빌드

```sh
git clone https://github.com/blan22/Kairos.git kairos && cd kairos
scripts/install.sh [--org <파일> [--sha256 <값>]]
```

스크립트는 `kairos`, `kairos-run`, 알림 헬퍼를 빌드합니다. 그다음 `~/.local/bin/kairos`를 설치본(`current/bin/kairos`)에 링크하고 `kairos setup`으로 넘어갑니다. 사용법은 `scripts/install.sh --help`로 볼 수 있습니다.

알림 헬퍼는 키체인의 Apple Development 인증서로 서명하고, 인증서가 없으면 ad-hoc으로 서명합니다. ad-hoc 서명이면 다시 빌드할 때마다 macOS가 알림 권한을 다시 물을 수 있습니다.

### 질문 없이 설치하기

`--yes`와 본인 식별자를 함께 넘기면 끝까지 묻지 않고 진행합니다. 사람이 할 일은 macOS 알림 권한 창에서 허용을 한 번 누르는 것뿐입니다.

```sh
scripts/install.sh --yes --google <회사 메일> --slack <Slack 멤버 ID> [--name <연차 일정의 이름>]
```

`--yes`를 주면 이렇게 진행합니다.

- 소스는 현재 값으로 둡니다. 처음이면 기본값입니다.
- 설치와 Orca 패널 로드, 승인에 자동으로 동의합니다. 승인하기 전에 Orca의 권한 문구는 그대로 보여 줍니다.
- `--org`가 없으면, 실행한 kairos가 들어 있는 저장소의 `profiles/`에 프로필이 하나뿐일 때만 그 프로필을 가져오고 SHA256을 출력합니다. 현재 폴더의 `profiles/`는 묻지도, 가져오지도 않습니다.
- 넘기지 않은 식별자는 비워 두고, 채우는 명령을 알려 줍니다.
- 세션은 켜지 않습니다.

알림 권한은 `--yes`가 없어도 터미널에서 묻지 않습니다.

## Linux에 설치하기

x86_64와 aarch64를 지원합니다. 릴리스 패키지는 glibc 2.35 이상(Ubuntu 22.04, Debian 12 등)에서 돕니다. Linux에서는 [세션](sessions.md)을 열지 않습니다.

| 필요한 것 | 언제 |
|---|---|
| systemd 사용자 세션 | 항상. 수집을 systemd 사용자 timer로 돌립니다 |
| 로그인한 gh(`gh auth login`) | 항상 |
| `notify-send`(libnotify) | 알림을 받을 때 |
| Orca | Orca 패널을 쓸 때. PATH의 `orca`를 부릅니다 |
| Rust(rustup), C 컴파일러 | 저장소에서 빌드할 때 |

릴리스 패키지로 설치합니다.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release-linux.sh | sh
```

저장소에서 빌드합니다.

```sh
git clone https://github.com/blan22/Kairos.git kairos && cd kairos
scripts/install-linux.sh
```

- 옵션은 macOS와 같습니다. 릴리스 설치는 `sh -s -- --yes --google <회사 메일>`처럼 뒤에 붙입니다.
- 패키지 이름은 `kairos-<x86_64|aarch64>-unknown-linux-gnu.tar.gz`입니다. SHA256이 맞을 때만 풉니다.
- `~/.local/bin/kairos` 링크를 만듭니다. 셸 설정 파일은 고치지 않습니다. `~/.local/bin`이 PATH에 없으면 더할 줄만 알려 줍니다.
- WSL2에서는 `/etc/wsl.conf`의 `[boot]`에 `systemd=true`를 켠 뒤 설치하세요.

## Windows에 설치하기

Windows 10, 11(x86_64)을 지원합니다. Windows에서는 [세션](sessions.md)을 열지 않습니다.

| 필요한 것 | 언제 |
|---|---|
| 로그인한 gh(`winget install --id GitHub.cli` 뒤 `gh auth login`) | 항상 |
| Orca | Orca 패널을 쓸 때. PATH의 `orca`를 부릅니다 |
| Rust(rustup), Visual Studio Build Tools의 C++ 도구 | 저장소에서 빌드할 때 |

릴리스 패키지로 설치합니다. PowerShell에서 실행하세요.

```powershell
irm https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.ps1 | iex
```

옵션을 넘기려면 스크립트를 블록으로 만들어 실행합니다.

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.ps1))) --yes --google <회사 메일>
```

저장소에서 빌드합니다.

```powershell
git clone https://github.com/blan22/Kairos.git kairos; cd kairos
powershell -ExecutionPolicy Bypass -File scripts\install.ps1
```

- 패키지 이름은 `kairos-x86_64-pc-windows-msvc.zip`입니다. SHA256이 맞을 때만 풉니다.
- 데이터 폴더는 `%LOCALAPPDATA%\kairos`입니다. `KAIROS_HOME`은 `%LOCALAPPDATA%` 아래의 절대 경로만 쓸 수 있습니다. 다른 곳이면 설치 스크립트가 아무것도 바꾸기 전에 멈춥니다.
- setup이 설치를 마치면, 설치본의 `bin` 폴더가 사용자 PATH에 없을 때 더할지 묻습니다. 설치하지 않았으면 PATH를 고치지 않습니다. `--yes`면 묻지 않고 더하고, `--no-modify-path`면 건드리지 않습니다. 새 터미널부터 `kairos`를 부를 수 있습니다.
- 수집은 작업 스케줄러의 `dev.kairos.collect` 작업이 돌립니다. 알림은 토스트로 오고, 누르면 패널이 열립니다.

## PATH와 자동 완성

`~/.local/bin`이 PATH에 없으면 설치 스크립트가 `~/.zshrc`에 다음 한 줄을 추가할지 묻습니다.

```sh
export PATH="$HOME/.local/bin:$PATH"
```

- `--yes`를 주면 묻지 않고 추가합니다.
- 이미 `.local/bin`이 적혀 있으면 다시 쓰지 않습니다.
- 터미널이 아니거나 셸이 zsh가 아니면 추가할 줄만 알려 줍니다.
- `--no-modify-path`를 주면 셸 설정을 건드리지 않습니다.

새 터미널을 열면 `kairos` 명령을 쓸 수 있습니다.

자동 완성을 쓰려면 셸 설정에 한 줄을 추가하세요.

```sh
# zsh: ~/.zshrc의 compinit 뒤에
source <(kairos completions zsh)

# bash: ~/.bashrc에
eval "$(kairos completions bash)"
```

## 업데이트

`kairos update`가 현재 버전과 설치 방식에 맞는 업데이트 명령을 알려 줍니다. kairos가 스스로 코드를 내려받지는 않습니다.

**저장소에서 빌드했다면** 다음 명령을 실행하세요. `git pull --ff-only`로 받은 뒤 다시 설치합니다.

```sh
<저장소>/scripts/install.sh --update
```

- 저장소에 커밋하지 않은 변경이 있으면 멈춥니다.
- 받은 뒤에는 처음 설치할 때와 같은 질문을 합니다. Enter를 누르면 현재 값을 유지하고, 설치와 Orca 패널 승인은 `y`로 답해야 진행합니다.
- 질문 없이 진행하려면 `--yes`를 함께 주세요. `--yes`는 설치와 패널 승인까지 자동으로 동의하기 때문에 `--update`가 알아서 붙이지는 않습니다.
- `~/.zshrc`는 건드리지 않습니다.

Linux는 `scripts/install-linux.sh --update`, Windows는 `scripts\install.ps1 --update`가 같은 일을 합니다.

**릴리스 패키지로 설치했다면** 설치 명령 한 줄을 다시 실행하세요. 미러(`KAIROS_RELEASE_URL`)로 설치했다면 같은 주소를 지정하고 실행합니다. `kairos update`가 알려 주는 명령은 공개 주소 기준입니다.

## 제거

```sh
kairos uninstall --dry-run --purge   # 지울 대상만 확인합니다
kairos uninstall                     # 예약 작업(launchd, systemd, 작업 스케줄러)만 해제합니다
kairos uninstall --runtime           # 런타임과 ~/.local/bin/kairos까지 지웁니다. 수집 기록과 설정은 남습니다
kairos uninstall --purge             # 데이터 폴더 전체와 키체인의 세션 토큰까지 지웁니다
```

`--runtime`과 `--purge`는 지울 대상을 먼저 보여 주고, `y`라고 답할 때만 지웁니다. 어떤 경우에 거절하는지는 [명령](commands.md#uninstall)에 있습니다.

Orca 패널은 언로드하는 명령(`orca plugin dev unload <경로>`)만 알려 줍니다. `~/.zshrc`에 추가한 PATH 줄과 저장소 폴더는 직접 지우세요.

## 공개 전 버전에서 업그레이드

공개 릴리스 이전의 설치는 launchd 라벨이 `com.<이름>.kairos.*`입니다. 여기서 업그레이드하면 세 가지가 바뀝니다.

1. launchd 라벨이 `dev.kairos.*`로 바뀝니다. `install --register`가 옛 라벨(`com.<이름>.kairos.collect`, `com.<이름>.kairos.watchdog`)을 해제하고 지웁니다. 이름이 같더라도 링크이거나, 내용에 그 라벨과 `KAIROS_HOME` 키가 없는 plist는 다른 프로그램의 작업으로 보고 건드리지 않습니다.
2. 알림 헬퍼의 번들 ID가 `dev.kairos.notifier`로 바뀝니다. 헬퍼를 다시 빌드하고 `kairos notify setup`으로 권한을 다시 받으세요.
3. 설정 지문이 없어 세션이 잠깁니다. 차단 테스트를 확인한 뒤 `kairos sessions on`을 다시 실행하세요.
