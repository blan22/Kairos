# kairos 에이전트 안내

회사 일정과 태스크를 모아 Orca, Claude Code, VS Code에 보여 주고, 사용자가 고른 일을 제한된 claude 세션에 맡기는 시스템이다. 사용자 안내는 `README.md`와 `docs/`, 보안 바닥값은 `SECURITY.md`, 호스트 계약은 `docs/surface-protocol.md`에 있다. 이 파일은 에이전트가 저장소에서 일하는 규칙이다. 하위 폴더의 `AGENTS.md`가 그 폴더에서는 이 파일보다 앞선다.

## 언어와 기록

- 답, 계획, 주석, 문서는 한국어로 쓴다. 문서 문체는 README처럼 짧은 평서문이다.
- 커밋 메시지는 영어로 쓴다. 제목은 명령형 한 줄이고, 본문은 바뀐 것을 줄마다 적는다.
- 커밋과 PR에 AI 작성 표기(`Co-Authored-By`, "Generated with" 등)를 넣지 않는다. 기술 내용만 쓴다.
- main에서 바로 커밋하지 않는다. 브랜치를 만들고, 머지는 사용자가 요청할 때만 한다.
- 계획과 실행 기록은 `.codex/`(git 무시)에 둔다.

## 구성

| 경로 | 내용 |
|---|---|
| `crates/` | Rust 작업 공간. 의존은 cli → tick, ops, sessions → agenda → ledger → policy → core 방향으로만 흐른다 |
| `crates/kairos-core` | 값 객체, 모델, 포트, 프로필(`profile`), 커넥터 읽기 동작 목록(`connector`) |
| `crates/kairos-config` | 설정 파일 읽기와 검증, 세션 지문 |
| `packages/kairos-client` | 호스트 공용 TS(`contract.ts`는 순수, `index.ts`는 Node 실행) |
| `plugins/` | 호스트 어댑터(`orca-panel`, `claude-code`, `vscode`)와 수집 훅(`collector`) |
| `storybook/` | 플러그인 UI 미리보기(orca 모듈 대역) |
| `profiles/` | 조직 프로필을 두는 자리. `*.toml`은 git이 무시한다 |
| `examples/` | 합성 값으로 쓴 설정 예시(`org.toml`은 시험 프로필과 같다) |
| `docs/` | 사용자 문서와 표시면 계약 |
| `ops/` | launchd 템플릿, 감시자(zsh), 알림 헬퍼(Swift) |
| `scripts/` | `verify.sh`, `check-personal-data.sh`, 하네스 시험, 설치(`install.sh`, `install-release.sh`)와 릴리스 묶음(`package.sh`) |

## 검증

```
scripts/verify.sh fast    # fmt 검사, cargo check, TS 시험. 끝내기 전에 돌린다(Stop 훅도 돌린다)
scripts/verify.sh full    # + clippy -D warnings, cargo test, Orca 패널, 스토리북, 개인 식별자 검사. 머지 전
cargo test -p <크레이트> [--test <파일>] <이름>   # 한 시험
```

- 근무 시간(평일 10:00~19:30 KST)에는 운영 수집 틱이 3분마다 돈다. 무거운 빌드와 전체 시험은 필요할 때만, 좁혀서 돌린다.
- 실패와 경고가 원래 있던 것이라고 가정하지 않는다. 고친 뒤 같은 범위를 다시 돌린다.
- 시험은 실제 Orca, gh, claude, 키체인, 운영 데이터 폴더를 쓰지 않는다. 가짜 실행 파일(`KAIROS_ORCA`, `KAIROS_GH`, `KAIROS_CLAUDE`, `KAIROS_GIT`, `KAIROS_SECURITY`)과 임시 `KAIROS_HOME`을 쓴다.

## 항상

- 사람이나 조직마다 다른 값(식별자, 조직, 도메인, 캘린더, 저장소, 시간대, 근무 시간)은 `kairos_core::profile::current()`에서 읽는다. 새 값은 `kairos-config`의 raw, 검사(`values.rs`), 합치기, `config show`에 함께 더한다.
- 설정 값이 argv, 정규식, 프롬프트, 경로로 가면 모양을 좁게 검사하고, 정규식에는 `regex::escape`한 값만 넣는다.
- 시각은 `Clock`, 외부 프로세스는 `ProcessRunner`, SQL은 `*Repo`로만 다룬다. 프로세스는 셸 없이 인자 배열로 띄운다.
- 계약(`CONTRACT_VERSION`)의 필드는 더하기만 한다. 없는 값은 키째 빼서(`skip_serializing_if`) v1의 바이트를 지킨다.
- 새 명령은 `commands::registry`와 `help.rs`의 표에 한 줄씩 더한다. 표에 없으면 시험이 실패한다.
- 새 동작에는 시험을 더한다. 프로세스 동작은 `crates/kairos-cli/tests`의 `Cli` 하네스로 고정한다.
- 보안 경계 파일(세션 프로필, 설정 검증, 커넥터 목록, 수집 훅, MCP 서버)을 고쳤으면 `security-reviewer` 에이전트로 검토한다.

## 하지 않음

- golden 고정값(`crates/*/tests/fixtures/golden`)을 맞추려고 고치지 않는다. 지운 TS 구현의 출력이라 다시 만들 수 없다. 출력 변화가 의도인지 사용자에게 묻는다.
- 생성 파일을 손으로 고치지 않는다: `storybook/src/stories/OrcaPanel.stories.tsx`(`npm --prefix storybook run gen`), `plugins/collector/hooks/allowed.ts`(설치기 출력과 같아야 한다), lockfile.
- 운영을 건드리지 않는다: `~/Library/Application Support/kairos`(운영 데이터), `~/Library/LaunchAgents`, `kairos install --apply|--register`, `kairos uninstall`(`--dry-run`만 된다), `kairos sessions on`, `next --seen`(노출 예산), Orca 플러그인 승인과 적재, `orca plugin shot local.kairos`, `kairos notify setup|test`(화면에 권한 창과 알림을 띄운다).
- 외부에 쓰지 않는다(push, PR, 코멘트, 메시지, 태그와 릴리스). 사용자가 요청할 때만 한다.
- 설치 스크립트를 시험할 때는 `HOME`과 `KAIROS_HOME`을 임시 폴더로 돌리고 가짜 실행 파일을 준다(`scripts/harness/install.test.mjs`). 실제 `~/.zshrc`와 운영 데이터 폴더에 쓰지 않는다.
- 공개 저장소다. 실제 사람, 조직, 저장소, 캘린더, 토큰을 코드, 시험 값, 문서, 커밋 메시지에 쓰지 않는다. 시험 값은 합성 표지(Slack ID의 `SYNTH`, `contoso.example` 같은 예약 도메인, 캘린더 ID의 `synthetic`)를 쓴다. `scripts/check-personal-data.sh`가 일반 규칙으로 막고, 막을 실제 값은 git이 무시하는 `.personal-patterns`에 둔다.
- 수집 훅에서 `$`를 넘기는 함수를 화살표 함수로 바꾸지 않는다(엔진이 모듈을 거부한다).

## 자주 하는 일

| 일 | 스킬 |
|---|---|
| 검증 | `/verify [fast|full|…]` |
| 소스 붙이기 | `/add-source <이름>` |
| 호스트 붙이기 | `/add-host <이름>` |
| 패널 화면 상태 더하기 | `/panel-scenario <이름>` |
| 머지 전 점검 | `/release-check` |

스킬 원본은 `.agents/skills/`이고 `.claude/skills`는 그 링크다. Codex 같은 다른 에이전트도 같은 절차를 읽는다.
