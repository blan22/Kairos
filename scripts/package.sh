#!/bin/zsh -f
# 릴리스 묶음을 만든다. 받는 사람은 Rust와 Xcode 없이 설치한다(scripts/install-release.sh).
#
#   scripts/package.sh [--no-build]
#
# 결과는 target/dist/kairos-aarch64-apple-darwin.tar.gz와 같은 이름의 .sha256이다.
# 묶음 안은 kairos-<판>/ 아래에 bin/, ops/, plugins/, scripts/install.sh, 문서가 놓인다.
# 실행 파일의 조상에 ops/와 plugins/가 있어 kairos가 설치 자료를 찾는다.
# --no-build는 이미 빌드한 target/release와 ops/notifier/KairosNotifier.app을 그대로 묶는다.
# 알림 헬퍼는 키체인의 Apple Development 인증서로 서명하고, 없으면(CI) ad-hoc으로 서명한다.
# ad-hoc 서명은 판마다 달라서 올린 뒤 macOS가 알림 권한을 다시 물을 수 있다.
set -euo pipefail

repo=${0:A:h:h}
asset=kairos-aarch64-apple-darwin.tar.gz
dist=$repo/target/dist
app=ops/notifier/KairosNotifier.app

fail() { print -u2 -r -- "멈춤: $1"; exit 1 }
step() { print -r -- ""; print -r -- "== $1" }

build=1
case ${1:-} in
  "") ;;
  --no-build) build=0 ;;
  *) fail "사용법: scripts/package.sh [--no-build]" ;;
esac

cd $repo
[[ $(uname -s) == Darwin && $(uname -m) == arm64 ]] || fail "Apple Silicon Mac에서만 묶습니다"
version=$(/usr/bin/awk -F'"' '/^version = /{ print $2; exit }' Cargo.toml)
[[ $version == <->.<->.<->* ]] || fail "Cargo.toml에서 판 번호를 읽지 못했습니다"
name=kairos-$version

if (( build )); then
  step "빌드"
  cargo build --release -p kairos-cli
  zsh ops/notifier/build.sh
fi

step "확인"
for bin in kairos kairos-run; do
  [[ -x target/release/$bin ]] || fail "target/release/${bin}가 없습니다. --no-build 없이 다시 실행하세요"
done
[[ -d $app ]] || fail "${app}가 없습니다. ops/notifier/build.sh로 만드세요"
# 빈 데이터 폴더로 판만 묻는다. 운영 데이터 폴더를 읽지 않는다.
probe=$(/usr/bin/mktemp -d)
trap '/bin/rm -rf $probe' EXIT
built=$(KAIROS_HOME=$probe/home target/release/kairos --version)
[[ $built == "kairos $version" ]] \
  || fail "target/release/kairos는 '$built'입니다. 지금 소스(kairos $version)로 다시 빌드하세요"
/usr/bin/codesign --verify --deep --strict $app
print -r -- "  $built, 알림 헬퍼 서명 확인"

step "묶기"
stage=$dist/stage
/bin/rm -rf $stage
/bin/mkdir -p $stage/$name/bin $stage/$name/ops/notifier
/usr/bin/install -m 0755 target/release/kairos target/release/kairos-run $stage/$name/bin/
/bin/cp ops/*.plist.template ops/kairos-watchdog.zsh $stage/$name/ops/
# 헬퍼는 번들만 넣는다. 소스와 빌드 스크립트는 넣지 않는다.
/bin/cp -R $app $stage/$name/ops/notifier/
# 나머지는 git이 추적하는 파일만 넣는다. node_modules와 로컬 설정이 섞이지 않는다.
/usr/bin/git ls-files -z -- plugins examples docs scripts/install.sh profiles/README.md README.md LICENSE SECURITY.md \
  | /usr/bin/tar -cf - --null -T - | /usr/bin/tar -xf - -C $stage/$name
print -r -- $version > $stage/$name/VERSION
[[ -x $stage/$name/scripts/install.sh ]] || fail "scripts/install.sh가 묶음에 없습니다"
/usr/bin/codesign --verify --deep --strict $stage/$name/$app

# AppleDouble(._) 파일을 넣지 않는다.
COPYFILE_DISABLE=1 /usr/bin/tar -czf $dist/$asset -C $stage $name
sum=$(/usr/bin/shasum -a 256 $dist/$asset | /usr/bin/awk '{ print $1 }')
print -r -- $sum > $dist/$asset.sha256
/bin/rm -rf $stage

print -r -- "  $dist/$asset"
print -r -- "  sha256 $sum"
print -r -- "완료: $name"
