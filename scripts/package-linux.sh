#!/bin/sh
# Linux 릴리스 묶음을 만든다. 받는 사람은 Rust 없이 설치한다(scripts/install-release-linux.sh). macOS는 scripts/package.sh, Windows는 scripts/package.ps1이다.
#
#   scripts/package-linux.sh [--no-build]
#
# 결과는 target/dist/kairos-<arch>-unknown-linux-gnu.tar.gz와 같은 이름의 .sha256이다. arch는 지금 머신의 것이다(x86_64, aarch64).
# 묶음 안은 kairos-<판>/ 아래에 bin/, ops/systemd/, plugins/, scripts/install-linux.sh, 문서가 놓인다.
# 실행 파일의 조상에 ops/와 plugins/가 있어 kairos가 설치 자료를 찾는다.
# --no-build는 이미 빌드한 target/release를 그대로 묶는다.
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dist=$repo/target/dist

fail() { printf '멈춤: %s\n' "$1" >&2; exit 1; }
step() { printf '\n== %s\n' "$1"; }

build=1
case ${1:-} in
  "") ;;
  --no-build) build=0 ;;
  *) fail "사용법: scripts/package-linux.sh [--no-build]" ;;
esac

cd "$repo"
[ "$(uname -s)" = Linux ] || fail "Linux에서만 묶습니다"
case $(uname -m) in
  x86_64|amd64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) fail "이 CPU($(uname -m))는 묶지 않습니다" ;;
esac
asset=kairos-$arch-unknown-linux-gnu.tar.gz
version=$(awk -F'"' '/^version = /{ print $2; exit }' Cargo.toml)
case $version in
  [0-9]*.[0-9]*.[0-9]*) ;;
  *) fail "Cargo.toml에서 판 번호를 읽지 못했습니다" ;;
esac
name=kairos-$version

if [ "$build" = 1 ]; then
  step "빌드"
  cargo build --release --locked -p kairos-cli
fi

step "확인"
for bin in kairos kairos-run; do
  [ -x "target/release/$bin" ] || fail "target/release/${bin}가 없습니다. --no-build 없이 다시 실행하세요"
done
# 빈 데이터 폴더로 판만 묻는다. 운영 데이터 폴더를 읽지 않는다.
probe=$(mktemp -d)
stage=$dist/stage
trap 'rm -rf "$probe" "$stage"' EXIT
built=$(KAIROS_HOME=$probe/home target/release/kairos --version)
[ "$built" = "kairos $version" ] || fail "target/release/kairos는 '$built'입니다. 지금 소스(kairos $version)로 다시 빌드하세요"
printf '  %s\n' "$built"

step "묶기"
rm -rf "$stage"
mkdir -p "$stage/$name/bin" "$stage/$name/ops/systemd"
install -m 0755 target/release/kairos target/release/kairos-run "$stage/$name/bin/"
cp ops/systemd/*.template "$stage/$name/ops/systemd/"
# 나머지는 git이 추적하는 파일만 넣는다. node_modules와 로컬 설정이 섞이지 않는다.
git ls-files -z -- plugins examples docs scripts/install-linux.sh profiles/README.md README.md LICENSE SECURITY.md \
  | tar -cf - --null -T - | tar -xf - -C "$stage/$name"
printf '%s\n' "$version" > "$stage/$name/VERSION"
[ -x "$stage/$name/scripts/install-linux.sh" ] || fail "scripts/install-linux.sh가 묶음에 없습니다"

mkdir -p "$dist"
# 묶을 때마다 같은 순서와 소유자로 넣는다.
tar --sort=name --owner=0 --group=0 --numeric-owner -czf "$dist/$asset" -C "$stage" "$name"
sum=$(sha256sum "$dist/$asset" | cut -d' ' -f1)
printf '%s\n' "$sum" > "$dist/$asset.sha256"

printf '  %s\n' "$dist/$asset"
printf '  sha256 %s\n' "$sum"
printf '완료: %s\n' "$name"
