# kairos

회사 일정과 할 일(Calendar, GitHub, Slack, Gmail, Notion)을 모아 지금 볼 일 하나를 골라 보여 주는 macOS 도구다. 사용자가 고른 일은 샌드박스로 제한한 claude 세션에 맡길 수 있다. 화면은 Orca 패널, Claude Code 플러그인, VS Code 확장 세 가지이고, 모두 같은 [호스트 계약](docs/surface-protocol.md)으로 kairos에 붙는다.

한 사람이 매일 쓰는 도구를 공개한 것이다. 설정, 계약, DB 스키마는 버전마다 바뀔 수 있고, 바뀌면 `docs/`에 마이그레이션 절차를 적는다.

- 외부 서비스는 읽기만 한다. 세션도 push, PR, 코멘트를 하지 못한다. PR은 사용자가 버튼을 누를 때만 세션 밖에서 올라간다.
- 본문, 댓글, 메일 제목은 저장하지 않는다. 알림 본문에는 고정 문구와 건수만 나온다.
- 사람과 조직마다 다른 값은 모두 설정 파일에 둔다. 코드와 저장소에는 없다.
- Rust 실행 파일 두 개(`kairos`, `kairos-run`)로 동작하고 별도 런타임이 필요 없다. 호스트가 실행하는 부분(Orca 패널, 수집 훅, VS Code 확장)만 TypeScript다.

## 요구 사항

- Apple Silicon Mac
- 로그인한 gh(`gh auth login`)와 Claude Code
- 저장소에서 빌드할 때만: Xcode Command Line Tools(`xcode-select --install`)와 Rust(rustup. 버전은 `rust-toolchain.toml`이 정한다)
- 패널을 쓸 때만: Orca 1.4.248 이상

## 계정 없이 체험하기

Rust만 있으면 합성 데이터로 kairos가 일을 고르는 모습을 볼 수 있다. 외부 서비스, gh, claude, Orca를 호출하지 않고, 비어 있는 새 폴더에만 쓴다.

```
demo="$(mktemp -d)/kairos"
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- demo
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- next
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- list --items
```

`next`는 수집 상태 한 줄과 지금 볼 일 하나를, `list --items`는 열린 항목을 급한 순서로 출력한다.

```
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

패널 화면은 스토리북(`storybook/`)에서 합성 시나리오로 볼 수 있다.

## 설치

저장소에서 빌드하는 방법이다.

```
git clone https://github.com/blan22/Kairos.git kairos && cd kairos
scripts/install.sh
```

빌드 없이 릴리스 패키지로 설치할 수도 있다. Rust와 Xcode가 필요 없다.

```
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.sh | zsh
```

스크립트는 빠진 도구를 한 번에 모아 알려 주고, 실행 파일과 알림 헬퍼를 준비한 뒤 `~/.local/bin/kairos` 링크를 만들고 `kairos setup`으로 넘어간다. setup은 조직 프로필, 켤 서비스, 계정 연결, 설치, Orca 패널을 차례로 묻는다. 알림 권한은 다른 Mac 앱처럼 macOS 권한 창에서 허용하면 된다.

- 업그레이드도 같은 명령으로 한다. `kairos update`가 설치 방식에 맞는 명령을 알려 준다.
- 질문 없이 끝까지 진행하는 `--yes`, PATH와 자동 완성, 단계별 명령, 설정 파일 형식은 [설치와 설정](docs/install.md)에 있다.
- 막히면 [문제 해결](docs/troubleshooting.md)을 본다.

조직 프로필은 저장소에 넣지 않는다. 관리자가 따로 배포하고 SHA256 지문을 함께 알려 준다. `examples/org.toml`은 합성 값으로 만든 예시다.

## 사용법

```
kairos next            # 지금 볼 일 하나
kairos list            # 열린 항목
kairos later <ID>      # 나중에 다시 보기(--when hour|tomorrow로 때를 고른다)
kairos done <ID>       # 끝냄
kairos doctor          # 수집과 설정 상태 점검
kairos sources         # 연동 서비스 켜고 끄기
kairos notify test     # Mac 알림이 오는지 테스트(권한은 kairos notify setup)
kairos update          # 현재 버전과 업데이트 명령
kairos help            # 전체 명령과 한 줄 설명
```

수집은 launchd가 3분마다 실행하고, 설치하면 바로 한 번 실행한다. `kairos setup`으로 설치하면 표시를 켜 두므로 첫 수집부터 카드와 알림을 낸다. 하루 동안 기록만 하려면 `kairos display auto`로 바꾼다. 전체 명령은 [명령](docs/commands.md)에, 일을 고르는 규칙과 알림 예산은 [항목과 표시](docs/items.md)에 있다.

일을 세션에 맡기려면 차단 테스트를 확인한 뒤 `kairos sessions on`으로 켠다. 세션이 할 수 있는 일과 막힌 일은 [수락과 세션](docs/sessions.md)에 있다.

## 데이터와 권한

| 무엇 | 어디 |
|---|---|
| 수집 기록, 항목, 실행 기록 | `~/Library/Application Support/kairos`(`KAIROS_HOME`). 폴더 0700, 파일 0600 |
| 설정(`org.toml`, `config.toml`, `connections.toml`) | 같은 폴더. 본인 소유이고 다른 사용자가 쓸 수 없을 때만 읽는다 |
| 세션용 읽기 전용 GitHub 토큰 | 키체인(`kairos connect session-token`) |
| launchd 작업 | `~/Library/LaunchAgents/dev.kairos.*.plist` |

- 수집은 Claude Code의 MCP 연결(Calendar, Slack, Gmail)과 gh로 읽는다. 호출할 수 있는 동작은 `crates/kairos-core/src/connector.rs`의 읽기 목록에 있는 것뿐이다.
- 저장하는 것은 식별자, 시각, 등급, 앞부분 120자(링크와 주소는 가린다)다. 외부 분석 도구나 원격 전송은 없다.
- 데이터 폴더는 git 작업 트리 밖에 둔다. 안에 두면 kairos가 실행을 거부한다. 개인 데이터가 저장소에 커밋되는 일을 막기 위해서다.
- Orca 패널은 `trusted-worker` 플러그인이다. 패널은 kairos 실행 파일을 셸 없이 호출하기만 한다. 다만 Orca의 권한 목록이 파일 접근을 막아 주는 샌드박스는 아니라는 점에 유의한다.
- Mac 알림에는 고정 문구와 건수만 나온다. 잠금 화면에 표시할지는 macOS 알림 설정에서 정한다.
- 제거: `kairos uninstall`은 launchd 작업을 해제한다. `--runtime`은 런타임과 `kairos` 명령까지, `--purge`는 데이터 폴더와 키체인의 세션 토큰까지 지운다. 지울 대상을 먼저 보여 주고 확인을 받는다. Orca 패널은 언로드하는 명령을 알려 준다.

보안 기준과 남은 위험은 [SECURITY.md](SECURITY.md)에 있다.

## 문서

| 문서 | 내용 |
|---|---|
| [docs/install.md](docs/install.md) | 설치, 업데이트, 제거, setup, 설정 파일, 조직 프로필 |
| [docs/troubleshooting.md](docs/troubleshooting.md) | 자주 막히는 지점과 해결 명령 |
| [docs/commands.md](docs/commands.md) | 명령 목록과 복구 명령 |
| [docs/items.md](docs/items.md) | 항목, 표시 게이트, 예산, 알림 |
| [docs/sessions.md](docs/sessions.md) | 수락, 세션 프로필, 샌드박스, PR 올리기 |
| [docs/sources.md](docs/sources.md) | 소스별 수집 방식, 잠자기 처리 |
| [docs/architecture.md](docs/architecture.md) | 크레이트 구조와 의존 방향 |
| [docs/surface-protocol.md](docs/surface-protocol.md) | 호스트 계약 |
| [docs/development.md](docs/development.md) | 개발과 테스트 |
| [plugins/orca-panel](plugins/orca-panel/README.md), [plugins/claude-code](plugins/claude-code/README.md), [plugins/vscode](plugins/vscode/README.md) | 호스트별 플러그인 |
| [ops/notifier](ops/notifier/README.md) | 알림 헬퍼(KairosNotifier) |
| [AGENTS.md](AGENTS.md) | 코딩 에이전트가 이 저장소에서 따르는 규칙 |
| [CHANGELOG.md](CHANGELOG.md) | 버전별 변경 사항 |

## 기여

이슈와 PR을 환영한다.

- 버그를 제보할 때는 `kairos --version`과 `kairos doctor`의 출력을 개인 값을 가리고 붙인다.
- 작업 전에 [개발과 테스트](docs/development.md)를 읽고, PR을 올리기 전에 `scripts/verify.sh full`을 돌린다.
- 테스트 값은 합성 데이터만 쓴다(`scripts/check-personal-data.sh`가 검사한다).
- 보안 문제는 공개 이슈 대신 [SECURITY.md](SECURITY.md)의 방법으로 알린다.

## 라이선스

MIT. [LICENSE](LICENSE)를 참고한다.
