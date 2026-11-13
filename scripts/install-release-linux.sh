#!/bin/sh
# 빌드 없이 Linux에 kairos를 설치한다. 최신 릴리스 묶음을 받아 SHA256을 확인하고 scripts/install-linux.sh로 넘긴다.
# 다시 실행하면 최신 판으로 올린다. 설정과 데이터는 그대로 둔다.
#
#   curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release-linux.sh | sh
#   curl ... | sh -s -- --yes --google <회사 메일> --slack <Slack 멤버 ID>     # 묻지 않음
#
# 인자는 묶음 안의 scripts/install-linux.sh에 그대로 넘긴다.
# 묶음은 x86_64와 aarch64가 있다(glibc 2.35 이상). 미리 있어야 하는 것은 systemd(사용자 세션)와 로그인한 gh다.
# KAIROS_RELEASE_URL을 주면 그 주소에서 받는다(사내 미러, 시험). 기본은 GitHub의 최신 릴리스다.
# SHA256 파일은 묶음과 같은 곳에서 받는다. 받는 중에 깨진 파일은 걸러내지만 올린 사람을 증명하지는 않는다.
set -eu

base=${KAIROS_RELEASE_URL:-https://github.com/blan22/Kairos/releases/latest/download}

fail() { printf '멈춤: %s\n' "$1" >&2; exit 1; }
note() { printf '  %s\n' "$1"; }

# 받다가 끊긴 스크립트가 반만 실행되지 않게 함수로 감싼다.
main() {
  [ "$(uname -s)" = Linux ] || fail "Linux에서만 설치합니다. macOS는 install-release.sh, Windows는 install-release.ps1을 쓰세요"
  case $(uname -m) in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) fail "이 CPU($(uname -m))용 묶음이 없습니다. 저장소를 받아 scripts/install-linux.sh로 빌드하세요" ;;
  esac
  asset=kairos-$arch-unknown-linux-gnu.tar.gz
  command -v sha256sum >/dev/null 2>&1 || fail "sha256sum이 없습니다(coreutils)"
  command -v curl >/dev/null 2>&1 || fail "curl이 없습니다"
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT

  printf '== kairos 릴리스 받기\n'
  # 기본 주소는 HTTPS만 허용한다. file:// 미러(시험)는 curl이 그대로 읽는다.
  case $base in
    file://*) secure="" ;;
    *) secure="--proto =https --tlsv1.2" ;;
  esac
  # shellcheck disable=SC2086
  curl $secure -fsSL -o "$work/$asset" "$base/$asset" || fail "묶음을 받지 못했습니다: $base/$asset"
  # shellcheck disable=SC2086
  curl $secure -fsSL -o "$work/$asset.sha256" "$base/$asset.sha256" || fail "SHA256 파일을 받지 못했습니다"

  expected=$(head -c 64 "$work/$asset.sha256")
  case $expected in
    *[!0-9a-f]*|"") fail "SHA256 파일의 모양이 맞지 않습니다" ;;
  esac
  [ ${#expected} -eq 64 ] || fail "SHA256 파일의 모양이 맞지 않습니다"
  actual=$(sha256sum "$work/$asset" | cut -d' ' -f1)
  [ "$actual" = "$expected" ] || fail "SHA256이 다릅니다. 받은 파일을 쓰지 않습니다"
  note "SHA256 확인 $actual"

  mkdir "$work/bundle"
  tar -xzf "$work/$asset" -C "$work/bundle"
  root="" found=0
  for dir in "$work"/bundle/kairos-*/; do
    [ -d "$dir" ] || continue
    root=${dir%/} found=$((found + 1))
  done
  [ "$found" -eq 1 ] || fail "묶음의 모양이 맞지 않습니다"
  [ -x "$root/bin/kairos" ] && [ -f "$root/scripts/install-linux.sh" ] || fail "묶음에 실행 파일이나 설치 스크립트가 없습니다"
  note "kairos $(cat "$root/VERSION")"

  # curl | sh로 불리면 표준 입력이 스크립트다. setup이 물을 수 있게 설치 스크립트에만 터미널을 준다. 터미널이 없으면 그대로 부른다.
  code=0
  if [ ! -t 0 ] && (: </dev/tty) 2>/dev/null; then
    sh "$root/scripts/install-linux.sh" "$@" </dev/tty || code=$?
  else
    sh "$root/scripts/install-linux.sh" "$@" || code=$?
  fi
  return $code
}

main "$@"
