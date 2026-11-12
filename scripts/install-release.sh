#!/bin/zsh -f
# 빌드 없이 kairos를 설치한다. 최신 릴리스 묶음을 받아 SHA256을 확인하고 scripts/install.sh로 넘긴다.
# 다시 실행하면 최신 판으로 올린다. 설정과 데이터는 그대로 둔다.
#
#   curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.sh | zsh
#   curl ... | zsh -s -- --yes --google <회사 메일> --slack <Slack 멤버 ID>     # 묻지 않음
#
# 인자는 묶음 안의 scripts/install.sh에 그대로 넘긴다(--yes, --org, --no-modify-path 등).
# 미리 있어야 하는 것은 Apple Silicon Mac, 로그인한 gh, Claude Code다. Rust와 Xcode는 필요 없다.
# KAIROS_RELEASE_URL을 주면 그 주소에서 받는다(사내 미러, 시험). 기본은 GitHub의 최신 릴리스다.
# SHA256 파일은 묶음과 같은 곳에서 받는다. 받는 중에 깨진 파일은 걸러내지만 올린 사람을 증명하지는 않는다.
# curl | zsh로 불리면 사용자의 ~/.zshenv 옵션이 걸려 있다. 기본 옵션으로 되돌린다.
emulate -R zsh
set -euo pipefail

asset=kairos-aarch64-apple-darwin.tar.gz
base=${KAIROS_RELEASE_URL:-https://github.com/blan22/Kairos/releases/latest/download}

fail() { print -u2 -r -- "멈춤: $1"; exit 1 }
note() { print -r -- "  $1" }

# 받다가 끊긴 스크립트가 반만 실행되지 않게 함수로 감싼다.
main() {
  [[ $(uname -s) == Darwin && $(uname -m) == arm64 ]] || fail "Apple Silicon Mac에서만 설치합니다"
  local work
  work=$(/usr/bin/mktemp -d)
  trap "/bin/rm -rf ${(q)work}" EXIT

  print -r -- "== kairos 릴리스 받기"
  # 기본 주소는 HTTPS만 허용한다. file:// 미러(시험)는 curl이 그대로 읽는다.
  local -a secure
  secure=(--proto '=https' --tlsv1.2)
  [[ $base == file://* ]] && secure=()
  /usr/bin/curl $secure -fsSL -o $work/$asset $base/$asset || fail "묶음을 받지 못했습니다: $base/$asset"
  /usr/bin/curl $secure -fsSL -o $work/$asset.sha256 $base/$asset.sha256 || fail "SHA256 파일을 받지 못했습니다"

  local expected actual
  expected=${"$(<$work/$asset.sha256)"%%[[:space:]]*}
  [[ ${#expected} == 64 && $expected != *[^0-9a-f]* ]] || fail "SHA256 파일의 모양이 맞지 않습니다"
  actual=${"$(/usr/bin/shasum -a 256 $work/$asset)"%% *}
  [[ $actual == $expected ]] || fail "SHA256이 다릅니다. 받은 파일을 쓰지 않습니다"
  note "SHA256 확인 $actual"

  /bin/mkdir $work/bundle
  /usr/bin/tar -xzf $work/$asset -C $work/bundle
  local -a roots
  roots=($work/bundle/kairos-*(N/))
  (( ${#roots} == 1 )) || fail "묶음의 모양이 맞지 않습니다"
  local root=$roots[1]
  [[ -x $root/bin/kairos && -f $root/scripts/install.sh ]] || fail "묶음에 실행 파일이나 설치 스크립트가 없습니다"
  note "kairos $(<$root/VERSION)"

  # curl | zsh로 불리면 표준 입력이 스크립트다. setup이 물을 수 있게 설치 스크립트에만 터미널을 준다.
  # 이 셸의 입력은 바꾸지 않는다. 바꾸면 끝난 뒤 이 셸이 터미널에서 다음 명령을 읽는다. 터미널이 없으면 그대로 부른다.
  local code=0
  if [[ ! -t 0 ]] && { : </dev/tty } 2>/dev/null; then
    /bin/zsh -f $root/scripts/install.sh "$@" </dev/tty || code=$?
  else
    /bin/zsh -f $root/scripts/install.sh "$@" || code=$?
  fi
  return $code
}

main "$@"
