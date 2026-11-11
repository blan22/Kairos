---
name: verify
description: kairos 변경을 검증한다. 작업을 끝내기 전, 커밋 전, "검증해줘", "테스트 돌려줘"에 쓴다. 범위(fast, full, 한 묶음)를 고르고 실패를 고친다.
argument-hint: "[fast|full|rust|ts|panel|storybook|hygiene]"
allowed-tools: Bash(scripts/verify.sh *), Bash(cargo test *), Bash(git status *), Bash(git diff *)
---

1. 범위를 정한다. 인자가 있으면 `$ARGUMENTS`, 없으면 바뀐 파일로 고른다.
   - Rust만 고쳤고 작은 변경이면 `cargo test -p <크레이트>` 뒤 `scripts/verify.sh fast`.
   - 여러 크레이트, 계약, 보안 경계, 머지 전이면 `scripts/verify.sh full`. 근무 시간(평일 10:00~19:30 KST)이면 사용자에게 먼저 알린다.
   - Orca 패널을 고쳤으면 `scripts/verify.sh panel`, 시나리오를 더했으면 `storybook`도.
2. 돌리고, 실패하면 고친 뒤 같은 범위를 다시 돌린다. 경고나 실패가 원래 있던 것이라고 가정하지 않는다.
3. golden 시험이 깨지면 고정값을 고치기 전에 출력 변화가 의도인지 사용자에게 묻는다.
4. 끝나면 돌린 범위, 통과한 수, 남은 실패를 한국어로 짧게 적는다.
