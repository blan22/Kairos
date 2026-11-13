# 개발 가이드

kairos를 빌드하고 테스트하는 방법입니다. 기여 절차는 [CONTRIBUTING.md](../CONTRIBUTING.md)에, 코드 구조는 [구조](architecture.md)에 있습니다.

코딩 에이전트(Claude Code, Codex)가 따르는 규칙은 [AGENTS.md](../AGENTS.md)에 있습니다. Claude Code는 `CLAUDE.md`가 이 파일을 불러옵니다. 강제하는 규칙은 `.claude/settings.json`의 훅에, 작업 절차는 `.agents/skills/`에 있습니다.

## 검증

검증은 `scripts/verify.sh` 하나로 합니다. CI도 같은 명령을 씁니다.

```sh
scripts/verify.sh fast    # fmt 검사, cargo check, TS 테스트
scripts/verify.sh full    # fast + clippy -D warnings, cargo test, Orca 패널, 스토리북, 개인 식별자 검사
scripts/verify.sh rust|ts|panel|storybook|hygiene   # 한 범위만
```

작업 중에는 `fast`를, PR을 올리기 전에는 `full`을 돌리세요.

개별 명령으로 돌리려면 다음을 씁니다.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
node --test plugins/collector/test/tick.test.ts   # 수집 모드 훅
node --test packages/kairos-client/src/*.test.ts  # 호스트 공용 클라이언트
node --test plugins/vscode/src/*.test.ts          # VS Code 확장 모델
node --test scripts/harness/*.test.mjs            # 훅, 설치 스크립트, 저장소 구성, 개인 식별자 검사
node --test storybook/scripts/*.test.mjs          # 스토리 목록
./scripts/check-personal-data.sh                  # 개인 식별자 검사
```

Orca 패널 테스트는 `plugins/orca-panel`에서 `orca plugin test .`로 돌립니다.

> [!NOTE]
> 근무 시간에는 무거운 빌드와 테스트를 피하세요. 부하가 크면 운영 중인 수집 틱이 시간 초과됩니다.

## 테스트와 외부 프로그램

테스트는 실제 Orca, gh, claude, 알림 헬퍼를 띄우지 않습니다. 가짜 실행 파일과 임시 `KAIROS_HOME`을 씁니다.

| 대상 | 방법 |
|---|---|
| 알림 | 틱을 돌리는 테스트는 라이브러리로 실행하고 기록용 알림기를 주입합니다. 프로세스 테스트는 `KAIROS_NOTIFIER`와 `KAIROS_OPEN`을 임시 폴더 안의 경로로 돌립니다 |
| reveal | `KAIROS_ORCA`와 `KAIROS_OPEN`에 가짜 실행 파일을 줘서 실제 Orca를 앞으로 가져오지 않습니다 |
| 수락 | 가짜 `ProcessRunner`를 주입하거나 `KAIROS_ORCA`에 가짜 실행 파일을 줍니다 |
| `kairos-run` | `KAIROS_GH`에도 가짜 gh를 주고 합성 세션 토큰을 씁니다 |

픽스처는 합성 데이터만 씁니다.

알림이 실제로 나간 틱에서는 수집기가 `orca plugin open`으로 패널을 엽니다. 묶음과 보류 규칙을 그대로 따르고, 창을 앞으로 가져오거나 포커스를 빼앗지 않습니다. 다른 사이드바 탭을 보고 있었다면 kairos 패널로 바뀝니다. `kairos notify off` 상태에서는 P1 알림으로 패널이 열리지 않습니다.

## golden 픽스처

`crates/*/tests/fixtures/golden`은 삭제한 TS 구현이 마지막으로 낸 출력입니다. 다시 만들 수 없습니다. `golden` 테스트가 깨지면 픽스처를 고치기 전에, 출력이 바뀐 것이 의도한 변화인지부터 확인하세요.

운영 DB 사본으로 해시를 다시 계산하려면 다음 명령을 씁니다.

```sh
KAIROS_HASH_DB=<사본> cargo test -p kairos-ledger --test production_hash -- --ignored --nocapture
```

## 공개 저장소의 테스트 값

이 저장소는 공개 저장소입니다. 테스트 값, 픽스처, 문서 예시는 모두 합성 값으로 씁니다.

| 항목 | 합성 값 |
|---|---|
| 조직, 저장소 | `contoso`, `contoso/atlas`, `contoso/atlas-specs`(`examples/org.toml`과 `Profile::fixture`) |
| 신뢰 도메인, 메일 | `contoso.example`, `atlas.example`, `*.invalid`, `*.test` |
| Slack ID | `U0SYNTH…`, `C0SYNTH…` |
| 캘린더 ID | 로컬 부분에 `synthetic`을 넣습니다 |
| 본인 이름 | `김준서`(Notion 이름 판정 테스트가 `준서`와 `기준서`를 구분합니다) |

`scripts/check-personal-data.sh`는 실제 값처럼 보이는 형태를 막습니다. 합성 표식이 없는 Slack ID, 무작위 캘린더 ID, 예약 도메인이 아닌 메일, 사용자 홈 경로, 토큰이 여기에 해당합니다.

회사 이름이나 사람 이름처럼 형태만으로 막을 수 없는 값은 저장소에 적는 것 자체가 유출입니다. 그래서 git이 무시하는 `.personal-patterns`(한 줄에 확장 정규식 하나)에 각자 적어 둡니다. `KAIROS_PERSONAL_PATTERNS`로 다른 파일을 지정할 수도 있습니다.

데이터 폴더(`KAIROS_HOME`)를 git 작업 트리 안에 두면 kairos가 실행되지 않습니다(`home_in_git_worktree`). 테스트는 임시 폴더를 씁니다.
