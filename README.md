# kairos

회사 일정과 일거리(Calendar, GitHub, Slack, Gmail, Notion)를 모아 지금 볼 일 하나를 골라 보여 주고, 사용자가 고른 일은 샌드박스에 갇힌 claude 세션에 맡기는 macOS 도구다. 표시면은 Orca 패널, Claude Code 플러그인, VS Code 확장이고 모두 같은 계약(`docs/surface-protocol.md`)으로 붙는다.

한 사람이 매일 쓰는 도구를 공개한 것이다. 설정, 계약, DB 스키마는 판마다 바뀔 수 있고 바뀌면 `docs/`에 옮기는 절차를 적는다.

- 외부 서비스는 읽기만 한다. 세션도 push, PR, 코멘트를 하지 못한다. PR은 사용자가 버튼을 누를 때만 세션 밖에서 올라간다.
- 본문, 댓글, 메일 제목은 저장하지 않는다. 알림 본문은 고정 낱말과 건수뿐이다.
- 사람과 조직마다 다른 값은 모두 설정 파일에 있다. 코드와 저장소에는 없다.
- Rust 실행 파일 두 개(`kairos`, `kairos-run`)이고 런타임이 필요 없다. 호스트가 실행하는 부분(Orca 패널, 수집 훅, VS Code 확장)만 TypeScript다.

## 요구 사항

- Apple Silicon Mac, Xcode Command Line Tools(`xcode-select --install`)
- Rust(rustup. 판은 `rust-toolchain.toml`이 받는다)
- 로그인한 gh(`gh auth login`), Claude Code
- Orca 1.4.248 이상(패널을 쓸 때만)

## 계정 없이 보기

Rust만 있으면 합성 데이터로 kairos가 고르는 모양을 볼 수 있다. 외부 서비스, gh, claude, Orca를 부르지 않고, 비어 있는 새 폴더에만 쓴다.

```
demo="$(mktemp -d)/kairos"
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- demo
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- next
KAIROS_HOME="$demo" cargo run -q -p kairos-cli --bin kairos -- list --items
```

패널 화면은 스토리북(`storybook/`)에서 합성 시나리오로 본다.

## 설치

```
git clone https://github.com/blan22/Kairos.git kairos && cd kairos
scripts/install.sh
```

스크립트가 실행 파일과 알림 헬퍼를 빌드하고 `~/.local/bin/kairos`를 건 뒤 `kairos setup`으로 넘긴다. setup은 조직 프로필, 켤 서비스, 계정 연결, 설치, 알림과 Orca 패널을 차례로 묻는다. 업그레이드도 같은 명령이다. 묻지 않고 끝까지 가는 `--yes`, 단계별 명령, 설정 파일의 모양은 [설치와 설정](docs/install.md)에 있다.

조직 프로필은 저장소에 넣지 않는다. 관리자가 따로 나눠 주고 지문(SHA256)을 알려 준다. `examples/org.toml`이 합성 값으로 쓴 예시다.

## 쓰기

```
kairos next            # 지금 볼 일 하나
kairos list            # 열린 항목
kairos later <ID>      # 나중에
kairos done <ID>       # 끝냄
kairos doctor          # 수집과 설정 상태
kairos sources         # 연동 서비스 켜고 끄기
```

수집은 launchd가 3분마다 돈다. 첫 정상 수집 뒤 하루는 기록만 하고 카드와 알림을 내지 않는다(`kairos display`로 바꾼다). 명령 전체는 [명령](docs/commands.md), 고르는 규칙과 알림 예산은 [항목과 표시](docs/items.md)에 있다.

일을 세션에 맡기려면 차단 시험을 본 뒤 `kairos sessions on`으로 켠다. 세션이 할 수 있는 일과 막힌 일은 [수락과 세션](docs/sessions.md)에 있다.

## 데이터와 권한

| 무엇 | 어디 |
|---|---|
| 수집 기록, 항목, 실행 기록 | `~/Library/Application Support/kairos`(`KAIROS_HOME`). 폴더 0700, 파일 0600 |
| 설정(`org.toml`, `config.toml`, `connections.toml`) | 같은 폴더. 본인 소유이고 남이 쓸 수 없어야 읽는다 |
| 세션용 읽기 전용 GitHub 토큰 | 키체인(`kairos connect session-token`) |
| launchd 잡 | `~/Library/LaunchAgents/dev.kairos.*.plist` |

- 수집은 Claude Code의 MCP 연결(Calendar, Slack, Gmail)과 gh로 읽는다. 부를 수 있는 동작은 `crates/kairos-core/src/connector.rs`의 읽기 목록뿐이다.
- 저장하는 것은 식별자, 시각, 등급, 앞부분 120자(링크와 주소는 가림)다. 외부 분석이나 원격 전송은 없다.
- 데이터 폴더는 git 작업 트리 밖에 둔다. 안에 두면 kairos가 거부한다. 개인 데이터가 저장소에 커밋되지 않게 하기 위해서다.
- Orca 패널은 `trusted-worker` 플러그인이다. 패널은 kairos 실행 파일을 셸 없이 부르기만 하지만, Orca의 권한 목록을 파일 접근 샌드박스로 읽으면 안 된다.
- Mac 알림에는 고정 낱말과 건수만 나온다. 잠금 화면 표시는 macOS 알림 설정에서 정한다.
- 지우기: `kairos uninstall`이 launchd 잡을 내린다. 데이터 폴더와 Orca 패널은 직접 지운다.

보안 바닥값과 남은 위험은 [SECURITY.md](SECURITY.md)에 있다.

## 문서

| 문서 | 내용 |
|---|---|
| [docs/install.md](docs/install.md) | 설치, setup, 설정 파일, 조직 프로필 |
| [docs/commands.md](docs/commands.md) | 명령 목록과 복구 명령 |
| [docs/items.md](docs/items.md) | 항목, 표시 게이트, 예산, 알림 |
| [docs/sessions.md](docs/sessions.md) | 수락, 세션 프로필, 샌드박스, PR 올리기 |
| [docs/sources.md](docs/sources.md) | 소스별 수집 방식, 잠자기 |
| [docs/architecture.md](docs/architecture.md) | 크레이트 구조와 의존 방향 |
| [docs/surface-protocol.md](docs/surface-protocol.md) | 표시면(호스트) 계약 |
| [docs/development.md](docs/development.md) | 개발과 시험 |
| [AGENTS.md](AGENTS.md) | 코딩 에이전트가 이 저장소에서 일하는 규칙 |

## 기여

이슈와 PR을 받는다. 시작 전에 [개발과 시험](docs/development.md)을 읽고, 올리기 전에 `scripts/verify.sh full`을 돌린다. 시험 값은 합성 데이터만 쓴다(`scripts/check-personal-data.sh`가 검사한다). 보안 문제는 공개 이슈 대신 [SECURITY.md](SECURITY.md)의 방법으로 알린다.

## 라이선스

MIT. [LICENSE](LICENSE)를 본다.
