#!/bin/zsh -f
# KairosNotifier.app을 만든다. 결과는 이 폴더의 KairosNotifier.app이다.
# 중간 산출물은 저장소의 target/notifier/에 둔다. 설치는 ops/를 통째로 복사하므로 ops/ 안에 두지 않는다.
# 서명은 키체인의 첫 'Apple Development' 인증서를 SHA-1로 고른다. 이름으로 고르면 codesign이 실패한다.
# 인증서가 없으면 ad-hoc으로 서명한다. 이때는 서명이 바뀔 때마다 알림 권한을 다시 물을 수 있다.
set -euo pipefail

dir=${0:A:h}
build=${dir:h:h}/target/notifier
app_name=KairosNotifier.app
target=arm64-apple-macos13.0
swiftc=/usr/bin/swiftc
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

/bin/mkdir -p $build
# 번들을 바꿔 끼울 때 rename을 쓴다. 같은 볼륨이어야 한다.
stage=$(/usr/bin/mktemp -d $build/stage.XXXXXX)
trap '/bin/rm -rf $stage' EXIT

print -r -- "== 자체 시험"
$swiftc -swift-version 6 -target $target -o $build/selftest $dir/Payload.swift $dir/Tests/main.swift
$build/selftest

print -r -- "== 컴파일"
$swiftc -swift-version 6 -O -target $target -o $build/KairosNotifier $dir/Payload.swift $dir/main.swift
$swiftc -swift-version 6 -O -o $build/make_icon $dir/make_icon.swift

# 잘못된 입력은 AppKit과 알림 센터를 건드리기 전에 2로 끝나야 한다. 번들 밖의 바이너리로 확인한다.
# thread가 빠진 payload는 kairos 쪽 키 이름이 바뀐 경우를 흉내 낸다.
expect_bad_input() {
  local rc=0
  $build/KairosNotifier "$@" 2>/dev/null || rc=$?
  if (( rc != 2 )); then
    print -u2 -r -- "실패: KairosNotifier ${(j: :)${(q)@}} 종료 코드 $rc, 2를 기대함"
    exit 1
  fi
}
expect_bad_input bogus
expect_bad_input post
expect_bad_input post '{"title":"t","body":"b","items":[1],"tier":"P0"}'
expect_bad_input post '{"title":"t","body":"b","items":[1],"tier":"P2","thread":"kairos.p0"}'
print -r -- "잘못된 입력 종료 코드 확인"

print -r -- "== 아이콘"
/bin/rm -rf $build/AppIcon.iconset
$build/make_icon $build/AppIcon.iconset
/usr/bin/iconutil -c icns $build/AppIcon.iconset -o $build/AppIcon.icns

print -r -- "== 번들"
bundle=$stage/$app_name
/bin/mkdir -p $bundle/Contents/MacOS $bundle/Contents/Resources
/usr/bin/install -m 0755 $build/KairosNotifier $bundle/Contents/MacOS/KairosNotifier
/usr/bin/install -m 0644 $dir/Info.plist $bundle/Contents/Info.plist
/usr/bin/install -m 0644 $build/AppIcon.icns $bundle/Contents/Resources/AppIcon.icns
print -n 'APPL????' > $bundle/Contents/PkgInfo
/usr/bin/plutil -lint -s $bundle/Contents/Info.plist
# Finder 정보 같은 확장 속성이 남으면 codesign이 거절한다.
/usr/bin/xattr -cr $bundle 2>/dev/null || true

print -r -- "== 서명"
identity=$(/usr/bin/security find-identity -v -p codesigning | /usr/bin/awk '/"Apple Development/ { print $2; exit }')
if [[ -z $identity ]]; then
  print -u2 -r -- "경고: Apple Development 인증서가 없어 ad-hoc으로 서명합니다. 알림 권한을 다시 물을 수 있습니다."
  identity=-
fi
/usr/bin/codesign --force --options runtime --timestamp=none --sign $identity $bundle
/usr/bin/codesign --verify --deep --strict $bundle

print -r -- "== 교체"
# 같은 파일 시스템 안의 rename이라 반쯤 만든 번들이 자리에 놓이지 않는다.
old=$build/old.$$
if [[ -e $dir/$app_name ]]; then
  /bin/mv $dir/$app_name $old
fi
if ! /bin/mv $bundle $dir/$app_name; then
  [[ -e $old ]] && /bin/mv $old $dir/$app_name
  exit 1
fi
/bin/rm -rf $old
/usr/bin/codesign --verify --deep --strict --verbose=2 $dir/$app_name
/usr/bin/codesign -dvv $dir/$app_name 2>&1 | /usr/bin/grep -E '^(Identifier|Authority|TeamIdentifier|Signature|CodeDirectory)' || true

# 개발 트리 번들은 LaunchServices에서 뺀다. 설치본과 번들 ID, 버전이 같아서 알림을 누를 때 어느 쪽이 뜰지 정해져 있지 않다.
# 개발 트리 번들이 뜨면 <번들>/../../../bin/kairos가 없는 경로로 풀린다. 설치본은 README의 setup 단계에서 등록한다.
# 빌드가 끝나고 30초 안팎 뒤 macOS가 다시 등록하는 것을 봤다. 그래서 이 단계는 최선 노력이다.
# 개발 트리 번들이 떠도 ~/.local/bin/kairos로 kairos를 찾는다.
$lsregister -u $dir/$app_name 2>/dev/null || true
print -r -- "완료: $dir/$app_name"
