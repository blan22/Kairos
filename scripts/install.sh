#!/bin/zsh -f
# kairos를 빌드하고 설치한다. 처음 설치와 업그레이드가 같은 명령이다.
#
#   scripts/install.sh [--org <파일> [--sha256 <값>]]
#   scripts/install.sh --yes --google <회사 메일> --slack <Slack 멤버 ID> [--name <이름>]   # 묻지 않음
#   scripts/install.sh --help
#
# 인자는 그대로 kairos setup에 넘긴다. --yes여도 macOS 알림 권한 창의 허용은 사람이 누른다.
# 1. 도구를 확인한다. 빠진 것을 한 번에 모아 설치 명령과 함께 알려 주고 멈춘다.
# 2. kairos, kairos-run, 알림 헬퍼를 빌드한다.
# 3. ~/.local/bin/kairos를 설치본(current)에 건다. 판이 바뀌어도 다시 걸 필요가 없다.
# 4. kairos setup으로 넘긴다. 조직 프로필, 서비스, 계정, 설치, Orca 패널을 차례로 묻는다.
#    알림 권한은 터미널에서 묻지 않는다. 다른 Mac 앱처럼 macOS 권한 창이 뜨고, 허용하면 그때부터 알림이 간다.
#    다시 실행하면 Enter로 지금 값을 지키고, 설치는 새 빌드로 덮는다. 옛 설치를 먼저 지우지 않아도 된다.
set -euo pipefail

repo=${0:A:h:h}
data=${KAIROS_HOME:-$HOME/Library/Application Support/kairos}
bin_dir=$HOME/.local/bin

step() { print -r -- ""; print -r -- "== $1" }
fail() { print -u2 -r -- "멈춤: $1"; exit 1 }
note() { print -r -- "  $1" }

case ${1:-} in
  -h|--help)
    # 머리말의 주석이 곧 사용법이다.
    /usr/bin/sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p;}' $0
    exit 0
    ;;
esac

step "1/4 도구 확인"
[[ $(uname -s) == Darwin && $(uname -m) == arm64 ]] || fail "Apple Silicon Mac에서만 설치합니다"
# 하나씩 멈추면 설치하고 다시 돌리기를 되풀이한다. 빠진 것을 모아 한 번에 알린다.
missing=()
xcode-select -p >/dev/null 2>&1 || missing+=("Xcode Command Line Tools: xcode-select --install")
command -v cargo >/dev/null 2>&1 || missing+=("Rust: https://rustup.rs 의 설치 명령을 실행하고 새 터미널을 여세요")
command -v gh >/dev/null 2>&1 || missing+=("gh: brew install gh 뒤 gh auth login")
command -v claude >/dev/null 2>&1 || missing+=("claude: Claude Code를 설치하세요")
if (( ${#missing} )); then
  print -u2 -r -- "멈춤: 아래 ${#missing}가지를 갖춘 뒤 다시 실행하세요"
  for item in $missing; do print -u2 -r -- "  - $item"; done
  exit 1
fi
# 로그인은 setup의 GitHub 단계가 다시 본다. 여기서는 미리 알리기만 한다.
gh auth status >/dev/null 2>&1 || note "gh에 로그인되어 있지 않습니다. GitHub를 쓰려면 gh auth login을 먼저 하세요"
note "Xcode Command Line Tools, Rust, gh, claude 확인"
[[ -x /Applications/Orca.app/Contents/Resources/bin/orca ]] || note "Orca가 없습니다. 패널 단계는 건너뜁니다"

step "2/4 빌드"
cd $repo
cargo build --release -p kairos-cli
zsh ops/notifier/build.sh

step "3/4 kairos 명령"
link=$bin_dir/kairos
target=$data/current/bin/kairos
/bin/mkdir -p $bin_dir
if [[ -L $link || ! -e $link ]]; then
  /bin/ln -sfn $target $link
  note "$link -> $target"
else
  note "${link}가 링크가 아닌 파일이라 그대로 둡니다"
fi
if (( ! ${path[(Ie)$bin_dir]} )); then
  note "PATH에 ${bin_dir}가 없습니다. ~/.zshrc에 다음 줄을 더하세요"
  note "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

step "4/4 kairos setup"
exec $repo/target/release/kairos setup "$@"
