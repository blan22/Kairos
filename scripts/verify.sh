#!/bin/sh
# 검증의 단일 입구. 에이전트, Stop 훅, CI가 같은 명령을 쓴다.
#
#   scripts/verify.sh fast   # fmt 검사, cargo check, TS 시험(수 초~수십 초). Stop 훅이 쓴다
#   scripts/verify.sh full   # fast + clippy -D warnings, cargo test, Orca 패널, 스토리북, 개인 식별자 검사
#   scripts/verify.sh rust|ts|panel|storybook|hygiene   # 한 묶음만
#
# 근무 시간(평일 10:00~19:30 KST)에는 full의 무거운 빌드가 운영 수집 틱을 늦출 수 있다. KAIROS_VERIFY_FORCE=1이 아니면 알리기만 한다.
set -eu
cd "$(dirname "$0")/.."

step() { printf '\n== %s\n' "$1"; }
have() { command -v "$1" >/dev/null 2>&1; }

rust_fast() {
  step "cargo fmt --check"
  cargo fmt --all -- --check
  step "cargo check"
  cargo check --workspace --all-targets --quiet
}

rust_full() {
  step "cargo fmt --check"
  cargo fmt --all -- --check
  step "cargo clippy"
  cargo clippy --workspace --all-targets --all-features --quiet -- -D warnings
  step "cargo test"
  cargo test --workspace --quiet
}

ts() {
  step "TS 시험(수집 훅, 공용 클라이언트, VS Code 모델, 하네스 훅)"
  node --test plugins/collector/test/tick.test.ts packages/kairos-client/src/*.test.ts plugins/vscode/src/*.test.ts scripts/harness/*.test.mjs
}

panel() {
  if have orca; then
    step "Orca 패널 시험"
    (cd plugins/orca-panel && orca plugin test .)
  else
    step "Orca 패널 시험 건너뜀(orca 없음)"
  fi
}

storybook() {
  step "스토리북 목록 시험"
  node --test storybook/scripts/*.test.mjs
  if [ -d storybook/node_modules ]; then
    step "스토리북 빌드"
    npm run --silent --prefix storybook build
  else
    step "스토리북 빌드 건너뜀(npm install --prefix storybook 필요)"
  fi
}

hygiene() {
  step "개인 식별자 검사"
  ./scripts/check-personal-data.sh
}

work_hours_note() {
  [ "${KAIROS_VERIFY_FORCE:-0}" = 1 ] && return 0
  day=$(TZ=Asia/Seoul date +%u)
  minutes=$(( $(TZ=Asia/Seoul date +%H | sed 's/^0//;s/^$/0/') * 60 + $(TZ=Asia/Seoul date +%M | sed 's/^0//;s/^$/0/') ))
  if [ "$day" -le 5 ] && [ "$minutes" -ge 600 ] && [ "$minutes" -lt 1170 ]; then
    echo "주의: 근무 시간이다. 무거운 빌드가 운영 수집 틱을 늦출 수 있다(docs/development.md)." >&2
  fi
}

case "${1:-fast}" in
  fast) rust_fast; ts ;;
  full) work_hours_note; rust_full; ts; panel; storybook; hygiene ;;
  rust) rust_full ;;
  ts) ts ;;
  panel) panel ;;
  storybook) storybook ;;
  hygiene) hygiene ;;
  *) echo "사용법: scripts/verify.sh [fast|full|rust|ts|panel|storybook|hygiene]" >&2; exit 2 ;;
esac
printf '\n검증 통과: %s\n' "${1:-fast}"
