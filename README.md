<div align="center">

<img src="plugins/orca-panel/assets/icon.png" width="96" alt="">

# kairos

**흩어진 회사 일을 모아, 지금 볼 일 하나를 골라 주는 도구(macOS, Linux, Windows)**

[![CI](https://github.com/blan22/Kairos/actions/workflows/ci.yml/badge.svg)](https://github.com/blan22/Kairos/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/blan22/Kairos)](https://github.com/blan22/Kairos/releases)
[![License](https://img.shields.io/github/license/blan22/Kairos)](LICENSE)

</div>

kairos는 Calendar, GitHub, Slack, Gmail, Notion에서 나에게 온 일을 모아 급한 순서로 정리합니다. 그리고 지금 봐야 할 일 하나를 골라 Orca 패널, Claude Code, VS Code에 보여 줍니다. 고른 일은 샌드박스에 가둔 claude 세션에 맡길 수도 있습니다.

- **읽기만 합니다.** 외부 서비스에 글을 쓰지 않고, 세션도 push나 PR, 코멘트를 할 수 없습니다. PR은 사용자가 버튼을 누를 때만 세션 밖에서 올라갑니다.
- **내용을 저장하지 않습니다.** 본문, 댓글, 메일 제목은 저장하지 않고, 알림에는 고정 문구와 건수만 나옵니다.
- **흐름을 끊지 않습니다.** 포커스 중이나 점심시간, 근무 시간 밖에 온 일은 쌓아 두었다가 한 번에 알립니다. 긴급 알림만 포커스 중에도 옵니다. 급하지 않은 일은 정해진 시각에 모아서 보여 줍니다.
- **설치가 가볍습니다.** Rust 실행 파일 두 개(`kairos`, `kairos-run`)로 동작해 별도 런타임이 필요 없습니다. 호스트가 실행하는 부분(Orca 패널, 수집 훅, VS Code 확장)만 TypeScript입니다.

> [!NOTE]
> 한 사람이 매일 쓰는 도구를 공개한 것입니다. 설정, 호스트 계약, DB 스키마는 버전마다 바뀔 수 있고, 바뀌면 `docs/`에 마이그레이션 절차를 적습니다.

## 계정 없이 체험하기

Rust만 있으면 합성 데이터로 kairos가 일을 고르는 모습을 볼 수 있습니다. 외부 서비스, gh, claude, Orca를 호출하지 않고, 비어 있는 새 폴더에만 씁니다.

```sh
demo="$(mktemp -d)/kairos"
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- demo
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- next
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- list --items
```

`next`는 수집 상태 한 줄과 지금 볼 일 하나를, `list --items`는 열린 항목을 급한 순서로 출력합니다.

```console
$ kairos next
Calendar 수집 기록 없음, GitHub 수집 기록 없음, Slack 수집 기록 없음, Gmail 수집 기록 없음, Notion 수집 기록 없음
P1 답장 필요
#4 P1 답장 필요 Slack 합성 멘션: 오늘 배포 전에 확인 부탁드려요

$ kairos list --items
#2 P1 내 PR 확인 GitHub 합성 PR: 로그인 오류 수정
#4 P1 답장 필요 Slack 합성 멘션: 오늘 배포 전에 확인 부탁드려요
#1 P2 리뷰 요청 GitHub 합성 PR: 장바구니 정리
#3 P2 답장 필요 GitHub 합성 이슈: 결제 화면 문구 오류
```

패널 화면은 [스토리북](storybook/README.md)에서 합성 시나리오로 볼 수 있습니다.

## 설치

**준비할 것**: Apple Silicon Mac, 로그인한 gh(`gh auth login`), Claude Code. Orca 패널을 쓰려면 Orca 1.4.248 이상이 필요합니다. Linux와 Windows는 아래 [Linux와 Windows](#linux와-windows)를 보세요.

릴리스 패키지로 설치하면 빌드하지 않으므로 Rust와 Xcode가 없어도 됩니다.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.sh | zsh
```

저장소에서 빌드하려면 Xcode Command Line Tools(`xcode-select --install`)와 Rust(rustup)가 더 필요합니다.

```sh
git clone https://github.com/blan22/Kairos.git kairos && cd kairos
scripts/install.sh
```

설치 스크립트는 빠진 도구를 한 번에 모아 알려 줍니다. 실행 파일과 알림 헬퍼를 준비하고 `~/.local/bin/kairos` 링크를 만든 뒤 `kairos setup`으로 넘어갑니다. setup은 조직 프로필, 켤 서비스, 계정 연결, 설치, Orca 패널을 차례로 묻습니다. 알림 권한은 다른 Mac 앱처럼 macOS 권한 창에서 허용하면 됩니다.

업그레이드도 같은 명령으로 합니다. `kairos update`가 설치 방식에 맞는 명령을 알려 줍니다. 질문 없이 설치하는 방법, PATH와 자동 완성은 [설치](docs/install.md)에, setup과 설정 파일은 [설정](docs/configuration.md)에 있습니다. 막히면 [문제 해결](docs/troubleshooting.md)을 보세요.

### Linux와 Windows

수집, 카드, 알림, 호스트는 macOS와 같게 동작합니다. [세션](docs/sessions.md)은 샌드박스가 보안 기준을 지킬 수 없어 열지 않습니다. OS마다 다른 점은 [지원하는 OS](docs/platforms.md)에 있습니다.

Linux(x86_64, aarch64)에는 systemd 사용자 세션과 로그인한 gh가 필요합니다.

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release-linux.sh | sh
```

Windows 10, 11(x86_64)에는 로그인한 gh가 필요합니다. PowerShell에서 실행하세요.

```powershell
irm https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.ps1 | iex
```

저장소에서 빌드하려면 Linux는 `scripts/install-linux.sh`, Windows는 `scripts\install.ps1`을 실행하세요.

조직 프로필은 저장소에 넣지 않습니다. 관리자가 SHA256 지문과 함께 따로 나눠 줍니다. [`examples/org.toml`](examples/org.toml)은 합성 값으로 만든 예시입니다.

## 사용법

```sh
kairos next            # 지금 볼 일 하나
kairos list            # 열린 항목
kairos later <ID>      # 나중에 다시 보기(--when hour|tomorrow로 때를 고릅니다)
kairos done <ID>       # 끝냄
kairos doctor          # 수집과 설정 상태 점검
kairos sources         # 연동 서비스 켜고 끄기
kairos notify test     # Mac 알림이 오는지 테스트(권한은 kairos notify setup)
kairos update          # 현재 버전과 업데이트 명령
kairos help            # 전체 명령과 한 줄 설명
```

수집은 launchd(Linux는 systemd, Windows는 작업 스케줄러)가 3분마다 실행하고, 설치하면 바로 한 번 실행합니다. `kairos setup`으로 설치하면 표시를 켜 두기 때문에 첫 수집부터 카드와 알림이 나옵니다. 하루 동안 기록만 하고 싶다면 `kairos display auto`로 바꾸세요.

일을 세션에 맡기려면 차단 테스트를 확인한 뒤 `kairos sessions on`으로 켭니다. 세션이 할 수 있는 일과 막힌 일은 [에이전트 세션](docs/sessions.md)에 있습니다. 세션은 macOS에서만 씁니다.

## 데이터와 권한

| 무엇 | 어디에 |
|---|---|
| 수집 기록, 항목, 실행 기록 | `~/Library/Application Support/kairos`(`KAIROS_HOME`). 폴더 0700, 파일 0600. Linux와 Windows의 자리는 [지원하는 OS](docs/platforms.md#파일이-놓이는-곳)에 있습니다 |
| 설정(`org.toml`, `config.toml`, `connections.toml`) | 같은 폴더. 본인 소유이고 다른 사용자가 쓸 수 없을 때만 읽습니다 |
| 세션용 읽기 전용 GitHub 토큰 | 키체인(`kairos connect session-token`) |
| launchd 작업 | `~/Library/LaunchAgents/dev.kairos.*.plist` |

- 수집은 Claude Code의 MCP 연결(Calendar, Slack, Gmail)과 gh로 읽습니다. 호출할 수 있는 동작은 `crates/kairos-core/src/connector.rs`의 읽기 목록에 있는 것뿐입니다.
- 저장하는 것은 식별자, 시각, 등급, 앞부분 120자(링크와 주소는 가립니다)입니다. 외부 분석 도구나 원격 전송은 없습니다.
- 사람과 조직마다 다른 값은 모두 설정 파일에 둡니다. 코드와 저장소에는 없습니다.
- 데이터 폴더는 git 작업 트리 밖에 있어야 합니다. 안에 있으면 kairos가 실행을 거부합니다. 개인 데이터가 저장소에 커밋되는 일을 막기 위해서입니다.
- Mac 알림에는 고정 문구와 건수만 나옵니다. 잠금 화면에 표시할지는 macOS 알림 설정에서 정합니다.
- `kairos uninstall`은 launchd 작업을 해제합니다. `--runtime`은 런타임과 `kairos` 명령까지, `--purge`는 데이터 폴더와 키체인의 세션 토큰까지 지웁니다. 지울 대상을 먼저 보여 주고 확인을 받습니다. Orca 패널은 언로드하는 명령을 알려 줍니다.

> [!IMPORTANT]
> Orca 패널은 `trusted-worker` 플러그인입니다. 패널은 kairos 실행 파일을 셸 없이 호출하기만 하지만, Orca의 권한 목록이 파일 접근을 막아 주는 샌드박스는 아닙니다.

보안 기준과 남은 위험은 [SECURITY.md](SECURITY.md)에 있습니다.

## 문서

전체 목차는 [docs/README.md](docs/README.md)에 있습니다.

| 처음 쓸 때 | 더 알고 싶을 때 | 개발할 때 |
|---|---|---|
| [핵심 개념](docs/concepts.md) | [항목과 알림](docs/items.md) | [기여 안내](CONTRIBUTING.md) |
| [설치](docs/install.md) | [에이전트 세션](docs/sessions.md) | [개발 가이드](docs/development.md) |
| [설정](docs/configuration.md) | [소스](docs/sources.md) | [구조](docs/architecture.md) |
| [명령](docs/commands.md) | [세션 샌드박스](docs/session-sandbox.md) | [호스트 계약](docs/surface-protocol.md) |
| [문제 해결](docs/troubleshooting.md) | [보안](SECURITY.md) | [변경 기록](CHANGELOG.md) |

## 기여

이슈와 PR을 환영합니다. 시작하기 전에 [기여 안내](CONTRIBUTING.md)를 읽어 주세요.

- 버그를 제보할 때는 `kairos --version`과 `kairos doctor`의 출력을 개인 값을 가리고 붙여 주세요.
- PR을 올리기 전에 `scripts/verify.sh full`을 돌려 주세요.
- 테스트 값은 합성 데이터만 씁니다(`scripts/check-personal-data.sh`가 검사합니다).
- 보안 문제는 공개 이슈 대신 [SECURITY.md](SECURITY.md)의 방법으로 알려 주세요.

## 라이선스

[MIT](LICENSE)
