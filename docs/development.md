# 개발과 시험

에이전트(Claude Code, Codex)가 일하는 규칙은 `AGENTS.md`(Claude Code는 `CLAUDE.md`가 가져온다), 강제되는 규칙은 `.claude/settings.json`의 훅, 절차는 `.agents/skills/`에 있다. 검증의 단일 입구는 `scripts/verify.sh fast|full`이고 CI도 같은 명령을 쓴다.

```
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
node --test plugins/collector/test/tick.test.ts   # 수집 모드 훅
node --test packages/kairos-client/src/*.test.ts  # 호스트 공용 클라이언트
node --test plugins/vscode/src/*.test.ts          # VS Code 확장 모델
./scripts/check-personal-data.sh                  # 개인 식별자 검사
```

패널 시험은 `plugins/orca-panel`에서 `orca plugin test .`로 돌린다.

`crates/*/tests/fixtures/golden`은 지운 TS 구현이 마지막으로 낸 출력이다. 다시 만들 수 없으니 `golden` 시험이 깨지면 고정값을 고치기 전에 출력이 바뀌는 것이 의도인지부터 본다. 근무 시간에는 무거운 빌드와 시험을 피한다. 부하가 크면 운영 틱이 시간 초과된다.

운영 DB 사본으로 해시를 재계산하려면 `KAIROS_HASH_DB=<사본> cargo test -p kairos-ledger --test production_hash -- --ignored --nocapture`를 쓴다.

시험은 실제 알림 헬퍼를 띄우지 않는다. 틱을 도는 시험은 라이브러리로 돌며 기록용 알림기를 넣는다. 프로세스 시험은 `KAIROS_NOTIFIER`와 `KAIROS_OPEN`을 임시 폴더 안으로 돌린다. reveal 시험은 `KAIROS_ORCA`와 `KAIROS_OPEN`에 가짜 실행 파일을 줘서 실제 Orca를 앞으로 끌어오지 않는다.

수락 시험은 가짜 `ProcessRunner`를 주입하거나 `KAIROS_ORCA`에 가짜 실행 파일을 준다. `kairos-run`을 띄우는 시험은 `KAIROS_GH`에도 가짜 gh를 주고 합성 세션 토큰을 쓴다. 실제 orca, gh, claude는 띄우지 않는다. 고정값은 합성 데이터만 쓴다.

알림이 실제로 나간 틱에서는 수집기가 `orca plugin open`으로 패널을 연다. 묶음과 보류 규칙을 그대로 따르고, 창을 앞으로 끌어오거나 포커스를 가져가지 않는다. 다른 사이드바 탭을 보고 있었다면 kairos 패널로 바뀐다. `kairos notify off`면 P1으로는 열리지 않는다.

## 공개 저장소의 시험 값

이 저장소는 공개다. 시험 값, 고정값, 문서 예시는 모두 합성이다.

| 칸 | 합성 값 |
|---|---|
| 조직, 저장소 | `contoso`, `contoso/atlas`, `contoso/atlas-specs`(`examples/org.toml`과 `Profile::fixture`) |
| 신뢰 도메인, 메일 | `contoso.example`, `atlas.example`, `*.invalid`, `*.test` |
| Slack ID | `U0SYNTH…`, `C0SYNTH…` |
| 캘린더 ID | 로컬 부분에 `synthetic`을 넣는다 |
| 본인 이름 | `김준서`(Notion 이름 판정 시험이 `준서`와 `기준서`를 가른다) |

`scripts/check-personal-data.sh`가 실제 값처럼 보이는 모양(합성 표지가 없는 Slack ID, 무작위 캘린더 ID, 예약 도메인이 아닌 메일, 사용자 홈 경로, 토큰)을 막는다. 회사 이름이나 사람 이름처럼 모양으로 막을 수 없는 값은 저장소에 적으면 그 자체가 유출이라, git이 무시하는 `.personal-patterns`(한 줄에 확장 정규식 하나)에 각자 적는다. `KAIROS_PERSONAL_PATTERNS`로 다른 파일을 줄 수도 있다.

데이터 폴더(`KAIROS_HOME`)를 git 작업 트리 안에 두면 kairos가 돌지 않는다(`home_in_git_worktree`). 시험은 임시 폴더를 쓴다.
