#!/bin/zsh -f
# kairos를 설치한다. 처음 설치와 업그레이드가 같은 명령이다.
#
#   scripts/install.sh [--org <파일> [--sha256 <값>]]
#   scripts/install.sh --yes --google <회사 메일> --slack <Slack 멤버 ID> [--name <이름>]   # 묻지 않음
#   scripts/install.sh --update [--yes]    # 저장소를 최신으로 당긴 뒤 다시 설치한다. --yes면 묻지 않는다
#   scripts/install.sh --no-modify-path    # ~/.zshrc를 건드리지 않는다
#   scripts/install.sh --help
#
# --update, --no-modify-path, --help를 뺀 인자는 그대로 kairos setup에 넘긴다.
# 받은 저장소에서 돌리면 빌드하고, 릴리스 묶음(bin/kairos가 들어 있다)에서 돌리면 빌드를 건너뛴다.
# 1. 도구를 확인한다. 빠진 것을 한 번에 모아 설치 명령과 함께 알려 주고 멈춘다.
# 2. kairos, kairos-run, 알림 헬퍼를 빌드한다(릴리스 묶음은 건너뛴다).
# 3. ~/.local/bin/kairos를 설치본(current)에 건다. 판이 바뀌어도 다시 걸 필요가 없다.
#    ~/.local/bin이 PATH에 없으면 ~/.zshrc에 한 줄을 더할지 묻는다(--yes면 묻지 않고 더한다).
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

# 이 스크립트의 옵션을 떼고 나머지를 setup에 넘긴다.
update=0
modify_path=1
yes=0
setup_args=()
for arg in "$@"; do
  case $arg in
    -h|--help)
      # 머리말의 주석이 곧 사용법이다.
      /usr/bin/sed -n '2,/^set -euo/{/^set -euo/d;s/^# \{0,1\}//;p;}' $0
      exit 0
      ;;
    --update) update=1 ;;
    --no-modify-path) modify_path=0 ;;
    # 인용한다. 빈 인자(--sha256 "")가 사라지면 뒤 인자가 그 자리로 밀린다.
    --yes) yes=1; setup_args+=("$arg") ;;
    *) setup_args+=("$arg") ;;
  esac
done

# 릴리스 묶음에는 빌드한 실행 파일이 들어 있고 Cargo.toml이 없다.
prebuilt=0
[[ -x $repo/bin/kairos && ! -f $repo/Cargo.toml ]] && prebuilt=1

if (( update )); then
  (( prebuilt )) && fail "릴리스 설치는 설치 명령을 다시 실행하면 최신 판으로 올라갑니다(kairos update가 알려 줍니다)"
  step "업데이트: 저장소 당기기"
  [[ -z $(/usr/bin/git -C $repo status --porcelain) ]] \
    || fail "저장소에 커밋하지 않은 변경이 있습니다. 정리한 뒤 다시 실행하세요"
  /usr/bin/git -C $repo pull --ff-only || fail "당기지 못했습니다. 위 메시지를 확인하세요"
  # 처음 설치와 같은 질문을 한다(Enter는 지금 값). 묻지 않는 것은 --yes를 함께 줬을 때뿐이다.
  # --yes는 설치와 Orca 패널 승인까지 예로 답하므로 당긴 코드에 스스로 붙이지 않는다. ~/.zshrc는 건드리지 않는다.
  setup_args+=(--no-modify-path)
  # 당긴 뒤의 스크립트로 다시 시작한다. 지금 읽는 파일이 바뀌었을 수 있다.
  exec /bin/zsh -f $repo/scripts/install.sh "${(@)setup_args}"
fi

step "1/4 도구 확인"
[[ $(uname -s) == Darwin && $(uname -m) == arm64 ]] || fail "Apple Silicon Mac에서만 설치합니다. Linux는 scripts/install-linux.sh, Windows는 scripts/install.ps1을 쓰세요"
# 하나씩 멈추면 설치하고 다시 돌리기를 되풀이한다. 빠진 것을 모아 한 번에 알린다.
missing=()
if (( ! prebuilt )); then
  xcode-select -p >/dev/null 2>&1 || missing+=("Xcode Command Line Tools: xcode-select --install")
  command -v cargo >/dev/null 2>&1 || missing+=("Rust: https://rustup.rs 의 설치 명령을 실행하고 새 터미널을 여세요")
fi
command -v gh >/dev/null 2>&1 || missing+=("gh: brew install gh 뒤 gh auth login")
command -v claude >/dev/null 2>&1 || missing+=("claude: Claude Code를 설치하세요")
if (( ${#missing} )); then
  print -u2 -r -- "멈춤: 아래 ${#missing}가지를 갖춘 뒤 다시 실행하세요"
  for item in $missing; do print -u2 -r -- "  - $item"; done
  exit 1
fi
# 로그인은 setup의 GitHub 단계가 다시 본다. 여기서는 미리 알리기만 한다.
gh auth status >/dev/null 2>&1 || note "gh에 로그인되어 있지 않습니다. GitHub를 쓰려면 gh auth login을 먼저 하세요"
if (( prebuilt )); then
  note "gh, claude 확인"
else
  note "Xcode Command Line Tools, Rust, gh, claude 확인"
fi
[[ -x /Applications/Orca.app/Contents/Resources/bin/orca ]] || note "Orca가 없습니다. 패널 단계는 건너뜁니다"

step "2/4 빌드"
if (( prebuilt )); then
  kairos=$repo/bin/kairos
  note "릴리스 묶음이라 빌드하지 않습니다"
else
  cd $repo
  cargo build --release -p kairos-cli
  zsh ops/notifier/build.sh
  kairos=$repo/target/release/kairos
fi

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

# ~/.local/bin을 PATH에 더한다. zsh의 ~/.zshrc에만 쓰고, 이미 적혀 있으면 다시 쓰지 않는다.
path_line='export PATH="$HOME/.local/bin:$PATH"'
path_hint() {
  note "PATH에 ${bin_dir}가 없습니다. 셸 설정에 다음 줄을 더하세요"
  note "  $path_line"
}
if (( ! ${path[(Ie)$bin_dir]} )); then
  rc=${ZDOTDIR:-$HOME}/.zshrc
  if (( ! modify_path )) || [[ ${${SHELL:-}:t} != zsh ]]; then
    path_hint
  elif [[ -f $rc ]] && /usr/bin/grep -qF '.local/bin' $rc; then
    note "${rc}에 이미 ~/.local/bin이 있습니다. 새 터미널을 열면 kairos 명령이 잡힙니다"
  else
    add=$yes
    if (( ! yes )) && [[ -t 0 ]]; then
      print -n -r -- "  PATH에 ${bin_dir}가 없습니다. ${rc}에 한 줄을 더할까요? [Y/n] "
      read -r answer || answer=n
      [[ ${answer:l} == (|y|yes|예|네) ]] && add=1
    fi
    if (( add )); then
      print -r -- "" >> $rc
      print -r -- "# kairos: ~/.local/bin을 PATH에 더한다" >> $rc
      print -r -- $path_line >> $rc
      note "${rc}에 PATH 한 줄을 더했습니다. 새 터미널을 열면 kairos 명령이 잡힙니다"
    else
      path_hint
    fi
  fi
fi

# 설치 방식을 적어 둔다. kairos update가 읽고 맞는 명령을 알려 준다.
# 임시 파일에 쓰고 바꿔 끼운다. 그 자리에 링크가 있어도 링크 너머에 쓰지 않는다.
/bin/mkdir -p -m 700 $data
record=$(/usr/bin/mktemp $data/.install-source.XXXXXX)
if (( prebuilt )); then
  print -r -- "release" > $record
else
  print -r -- "source:$repo" > $record
fi
/bin/chmod 600 $record
/bin/mv -f $record $data/install-source

step "4/4 kairos setup"
if (( ! prebuilt )); then
  exec $kairos setup "${(@)setup_args}"
fi
# 릴리스 묶음은 임시 폴더에 풀려 있어 끝나면 지워진다. 설치하지 않았으면 kairos 명령이 남지 않는다.
code=0
$kairos setup "${(@)setup_args}" || code=$?
if [[ ! -x $target ]]; then
  print -r -- ""
  note "설치하지 않아 kairos 명령이 없습니다. 받은 묶음은 끝나면 지워집니다"
  note "설치하려면 설치 명령을 다시 실행하고 설치 질문에 y로 답하세요(--yes면 묻지 않습니다)"
  # 없는 실행 파일을 가리키는 링크를 남기지 않는다.
  if [[ -L $link && $(/usr/bin/readlink $link) == $target ]]; then
    /bin/rm -f $link
  fi
fi
exit $code
