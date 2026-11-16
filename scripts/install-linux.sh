#!/bin/sh
# Linux에 kairos를 설치한다. 처음 설치와 업그레이드가 같은 명령이다. macOS는 scripts/install.sh, Windows는 scripts/install.ps1이다.
#
#   scripts/install-linux.sh [--org <파일> [--sha256 <값>]]
#   scripts/install-linux.sh --yes --google <회사 메일> --slack <Slack 멤버 ID> [--name <이름>]   # 묻지 않음
#   scripts/install-linux.sh --update [--yes]    # 저장소를 최신으로 당긴 뒤 다시 설치한다
#   scripts/install-linux.sh --help
#
# --update, --no-modify-path, --help를 뺀 인자는 그대로 kairos setup에 넘긴다.
# 받은 저장소에서 돌리면 빌드하고, 릴리스 묶음(bin/kairos가 들어 있다)에서 돌리면 빌드를 건너뛴다.
# 1. 도구를 확인한다. 빠진 것을 한 번에 모아 알려 주고 멈춘다.
# 2. kairos, kairos-run을 빌드한다(릴리스 묶음은 건너뛴다).
# 3. ~/.local/bin/kairos를 설치본(current)에 건다. 셸 설정 파일은 고치지 않고, PATH에 없으면 더할 줄만 알려 준다.
# 4. kairos setup으로 넘긴다. 설치 단계는 systemd 사용자 timer를 등록한다.
#    Linux에서는 세션(에이전트에 맡기기)을 열지 않는다. 수집, 패널, 알림은 macOS와 같다.
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
data=${KAIROS_HOME:-$HOME/.local/share/kairos}
bin_dir=$HOME/.local/bin

step() { printf '\n== %s\n' "$1"; }
fail() { printf '멈춤: %s\n' "$1" >&2; exit 1; }
note() { printf '  %s\n' "$1"; }
have() { command -v "$1" >/dev/null 2>&1; }

update=0
for arg in "$@"; do
  case $arg in
    -h|--help)
      # 머리말의 주석이 곧 사용법이다.
      sed -n '2,/^set -eu/{/^set -eu/d;s/^# \{0,1\}//;p;}' "$0"
      exit 0
      ;;
    --update) update=1 ;;
  esac
done

# 이 스크립트의 옵션을 떼고 나머지를 setup에 넘긴다. 뒤에 붙인 뒤 원래 인자를 밀어낸다. 빈 인자도 자리를 지킨다.
count=$#
for arg do
  case $arg in
    --update|--no-modify-path) ;;
    *) set -- "$@" "$arg" ;;
  esac
done
shift "$count"

prebuilt=0
if [ -x "$repo/bin/kairos" ] && [ ! -f "$repo/Cargo.toml" ]; then prebuilt=1; fi

if [ "$update" = 1 ]; then
  [ "$prebuilt" = 1 ] && fail "릴리스 설치는 설치 명령을 다시 실행하면 최신 판으로 올라갑니다(kairos update가 알려 줍니다)"
  step "업데이트: 저장소 당기기"
  [ -z "$(git -C "$repo" status --porcelain)" ] || fail "저장소에 커밋하지 않은 변경이 있습니다. 정리한 뒤 다시 실행하세요"
  git -C "$repo" pull --ff-only || fail "당기지 못했습니다. 위 메시지를 확인하세요"
  # 당긴 뒤의 스크립트로 다시 시작한다. 지금 읽는 파일이 바뀌었을 수 있다. --update는 이미 뗐다.
  exec sh "$repo/scripts/install-linux.sh" "$@"
fi

step "1/4 도구 확인"
[ "$(uname -s)" = Linux ] || fail "Linux에서만 설치합니다. macOS는 scripts/install.sh, Windows는 scripts/install.ps1을 쓰세요"
missing=""
add_missing() { missing="$missing
  - $1"; }
if [ "$prebuilt" = 0 ]; then
  have cargo || add_missing "Rust: https://rustup.rs 의 설치 명령을 실행하고 새 터미널을 여세요"
  have cc || add_missing "C 컴파일러: 배포판의 build-essential(또는 gcc)을 설치하세요"
fi
have gh || add_missing "gh: https://cli.github.com 의 안내대로 설치한 뒤 gh auth login"
have systemctl || add_missing "systemd: 수집은 systemd 사용자 timer로 돕니다. WSL은 /etc/wsl.conf의 [boot] systemd=true를 켜세요"
if [ -n "$missing" ]; then
  printf '멈춤: 아래를 갖춘 뒤 다시 실행하세요%s\n' "$missing" >&2
  exit 1
fi
gh auth status >/dev/null 2>&1 || note "gh에 로그인되어 있지 않습니다. GitHub를 쓰려면 gh auth login을 먼저 하세요"
have notify-send || note "notify-send가 없습니다. 알림을 받으려면 libnotify(notify-send)를 설치하세요"
have orca || note "orca가 PATH에 없습니다. 패널 단계는 건너뜁니다"
note "도구 확인"

step "2/4 빌드"
if [ "$prebuilt" = 1 ]; then
  kairos=$repo/bin/kairos
  note "릴리스 묶음이라 빌드하지 않습니다"
else
  (cd "$repo" && cargo build --release -p kairos-cli)
  kairos=$repo/target/release/kairos
fi

step "3/4 kairos 명령"
link=$bin_dir/kairos
target=$data/current/bin/kairos
mkdir -p "$bin_dir"
if [ -L "$link" ] || [ ! -e "$link" ]; then
  ln -sfn "$target" "$link"
  note "$link -> $target"
else
  note "${link}가 링크가 아닌 파일이라 그대로 둡니다"
fi
case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *)
    note "PATH에 ${bin_dir}가 없습니다. 셸 설정(~/.bashrc, ~/.zshrc 등)에 다음 줄을 더하세요"
    note "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    ;;
esac

# 설치 방식을 적어 둔다. kairos update가 읽고 맞는 명령을 알려 준다.
# 임시 파일에 쓰고 바꿔 끼운다. 그 자리에 링크가 있어도 링크 너머에 쓰지 않는다.
(umask 077 && mkdir -p "$data")
record=$(mktemp "$data/.install-source.XXXXXX")
if [ "$prebuilt" = 1 ]; then
  printf 'release\n' > "$record"
else
  printf 'source:%s\n' "$repo" > "$record"
fi
chmod 600 "$record"
mv -f "$record" "$data/install-source"

step "4/4 kairos setup"
if [ "$prebuilt" = 0 ]; then
  exec "$kairos" setup "$@"
fi
# 릴리스 묶음은 임시 폴더에 풀려 있어 끝나면 지워진다. 설치하지 않았으면 kairos 명령이 남지 않는다.
code=0
"$kairos" setup "$@" || code=$?
if [ "$prebuilt" = 1 ] && [ ! -x "$target" ]; then
  printf '\n'
  note "설치하지 않아 kairos 명령이 없습니다. 받은 묶음은 끝나면 지워집니다"
  note "설치하려면 설치 명령을 다시 실행하고 설치 질문에 y로 답하세요(--yes면 묻지 않습니다)"
  # 없는 실행 파일을 가리키는 링크를 남기지 않는다.
  if [ -L "$link" ] && [ "$(readlink "$link")" = "$target" ]; then rm -f "$link"; fi
fi
exit $code
