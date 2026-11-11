#!/bin/sh
# 공개 저장소에 실제 사람, 조직의 값이 들어오면 실패한다. CI(hygiene)와 커밋 전에 돌린다.
#
# 두 겹이다.
# 1. 일반 규칙(이 파일): 실제 값처럼 보이는 모양을 막는다. 시험 값은 합성 표지를 쓴다.
#    Slack ID는 SYNTH, FAKE, TEST, OTHER가 들고, 메일과 도메인은 example, invalid, test 같은 예약 이름을 쓴다.
# 2. 개인 규칙: 막을 실제 값(회사 이름, 사람 이름, 계정 ID)은 저장소에 적는 순간 그 자체가 유출이다.
#    git이 무시하는 `.personal-patterns`(한 줄에 확장 정규식 하나, #으로 시작하면 주석)나
#    KAIROS_PERSONAL_PATTERNS(그 파일 경로)로 받는다. 각자의 컴퓨터에만 두고 CI에는 없다.
#    대소문자를 가리지 않고, 규칙의 `-`, `.`, `_`, 공백은 서로 바꿔 쓴 꼴(foo_bar, foo.bar, FooBar)도 잡는다.
#
# 검사 자체가 실패하면(잘못된 정규식, 읽지 못한 파일 목록) 통과로 치지 않고 2로 끝낸다.
# 이 파일의 테스트는 scripts/harness/personal-data.test.mjs다.
set -eu
cd "$(dirname "$0")/.."

self='scripts/check-personal-data.sh'
# 파이프의 끝은 하위 셸에서 돌아 변수가 남지 않는다. 찾은 것과 검사 오류는 임시 파일에 모은다.
findings=$(mktemp)
errors=$(mktemp)
list=$(mktemp)
links=$(mktemp)
trap 'rm -f "$findings" "$errors" "$list" "$links"' EXIT

# 추적 파일 목록. 의존성 잠금 파일은 남의 메타데이터(저자 메일 등)라 뺀다.
# 목록을 얻지 못하거나 비면 검사할 것이 없는 것이 아니라 검사가 실패한 것이다.
if ! git ls-files -z -s -- . ':!:Cargo.lock' ':!:*package-lock.json' ":!:$self" > "$list"; then
  echo "git ls-files 실패" >> "$errors"
fi
if [ ! -s "$list" ]; then
  echo "추적 파일 목록이 비었습니다" >> "$errors"
fi
# 심볼릭 링크(.claude/skills 등)는 grep이 폴더 링크에서 오류를 내므로 파일로 읽지 않는다.
# 대신 링크 대상 문자열(실제 홈 경로가 들 수 있다)을 "경로 -> 대상" 줄로 모아 같은 규칙에 넣는다.
tab=$(printf '\t')
perl -0 -ne 'print "$1\t$2\n" if /^120000 ([0-9a-f]+) \d+\t(.*)\0\z/s' "$list" | while IFS=$tab read -r sha path; do
  if target=$(git cat-file blob "$sha"); then
    printf '%s -> %s\n' "$path" "$target" >> "$links"
  else
    echo "링크 대상을 읽지 못했습니다: $path" >> "$errors"
  fi
done

files() {
  perl -0 -ne 'print "$1\0" if /^(?!120000 )\d+ [0-9a-f]+ \d+\t(.*)\0\z/s' "$list"
  if [ -s "$links" ]; then
    printf '%s\0' "$links"
  fi
}

report() {
  # $1: 설명, 표준 입력: grep -n 결과
  found=$(cat)
  if [ -n "$found" ]; then
    printf '%s:\n%s\n\n' "$1" "$found" >> "$findings"
  fi
}

check_regex() {
  # $1: 확장 정규식. grep은 일치 없음에 1, 오류에 2를 낸다. 오류를 삼키면 검사가 조용히 꺼진다.
  status=0
  printf '' | grep -E -- "$1" > /dev/null 2>&1 || status=$?
  if [ "$status" -gt 1 ]; then
    echo "잘못된 정규식: $1" >> "$errors"
    return 1
  fi
}

scan() {
  # $1: 확장 정규식, $2: grep에 더할 옵션(선택). 일치한 줄을 낸다.
  check_regex "$1" || return 0
  # xargs는 grep이 1(일치 없음)이면 123, 2(오류)면 123이나 그 밖의 값을 낸다. 바이너리 건너뜀은 -I가 한다.
  files | xargs -0 grep -nEI ${2:-} -- "$1" 2> "$errors.grep" || true
  if [ -s "$errors.grep" ]; then
    cat "$errors.grep" >> "$errors"
  fi
  rm -f "$errors.grep"
}

leftover() {
  # 표준 입력: grep -n 결과. $1: 값 모양, $2: 허용 모양, $3: 허용 검사에 더할 grep 옵션(선택).
  # 줄 단위로 거르면 합성 표지가 있는 줄의 실제 값도 지나간다. 값마다 보고, 허용되지 않은 값이 남은 줄만 낸다.
  while IFS= read -r line; do
    content=${line#*:*:}
    rest=$(printf '%s\n' "$content" | grep -oE -- "$1" | grep -vE ${3:-} -- "$2" || true)
    if [ -n "$rest" ]; then
      printf '%s\n' "$line"
    fi
  done
}

# Slack 사용자, 채널 ID 모양. 합성 표지가 없으면 실제 ID로 본다.
slack='\b[UCDGW]0[A-Z0-9]{7,11}\b'
scan "$slack" \
  | leftover "$slack" '(SYNTH|FAKE|TEST|OTHER|EXAMPLE)' \
  | report "Slack ID처럼 보이는 값(합성 값에는 SYNTH를 넣는다)"

# 그룹 캘린더 ID. 실제 ID는 로컬 부분에 20자 넘는 무작위 글자가 있다. 합성 값은 synthetic을 넣는다.
calendar='[A-Za-z0-9._%+-]*[a-z0-9]{20,}[A-Za-z0-9._%+-]*@group\.calendar\.google\.com'
scan "$calendar" \
  | leftover "$calendar" '([Ss]ynth|[Ee]xample|[Ff]ake)' \
  | report "그룹 캘린더 ID(합성 값에는 synthetic을 넣는다)"

# 메일 주소. 예약 도메인과 알림을 보내는 서비스 주소만 둔다. 허용 모양은 주소 끝까지 맞아야 한다.
mail='[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}'
# 서비스 도메인은 알림을 보내는 주소 모양(noreply, notify 등)이나 한 글자 합성 값만 둔다.
# users.noreply.github.com 같은 주소는 로컬 부분에 실제 계정이 들어 있어 받지 않는다. 그룹 캘린더 ID는 위 캘린더 규칙이 본다.
service='(notion\.com|sentry\.io|getsentry\.com|google\.com|apple\.com|figma\.com|github\.com|anthropic\.com)'
sender='(git|no-?reply|no_reply|[a-z]+-noreply|alerts?|notify|notifications?|calendar-notification|someone|[a-z])'
allowed_mail="@([A-Za-z0-9-]+\\.)*(example|invalid|test|localhost)\$|@([A-Za-z0-9-]+\\.)*example\\.(com|org|net)\$|@(group(\\.v)?|resource)\\.calendar\\.google\\.com\$|^$sender@([a-z0-9-]+\\.)*$service\$"
scan "$mail" \
  | leftover "$mail" "$allowed_mail" -i \
  | report "예약 도메인이 아닌 메일 주소(시험 값은 example, invalid, test 도메인을 쓴다)"

# 사용자 홈 경로. 합성 이름만 둔다.
home='/Users/[A-Za-z0-9._-]+'
scan "$home" \
  | leftover "$home" '^/Users/(synth|synthetic|someone|me|x|test|example|runner|you|user|name)$' \
  | report "실제 사용자 홈처럼 보이는 경로"

# 실제 토큰 모양. 시험의 합성 토큰은 SYNTHETIC이나 x의 반복을 쓴다.
secret='(github_pat_[A-Za-z0-9_]{30,}|gh[pousr]_[A-Za-z0-9]{36}|xox[abprs]-[0-9]{6,}[A-Za-z0-9-]*|sk-ant-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY-----)'
scan "$secret" \
  | leftover "$secret" '(SYNTH|EXAMPLE|FAKE|x{8,}|X{8,}|0{8,}|abcdefgh|12345678)' \
  | report "비밀처럼 보이는 값"
# 개인 키 본문 조각. 따옴표 안이나 리터럴 \n으로 이은 한 줄 키도 잡는다. 16진수만 있는 값(해시)은 둔다.
base64='[A-Za-z0-9+/]{60,}={0,2}'
scan "$base64" \
  | leftover "$base64" '^([0-9a-f]+|[0-9A-F]+)$' \
  | report "개인 키 본문처럼 보이는 base64 값"

# 개인 규칙.
patterns_file=${KAIROS_PERSONAL_PATTERNS:-.personal-patterns}
if [ -n "${KAIROS_PERSONAL_PATTERNS:-}" ] && [ ! -f "$patterns_file" ]; then
  echo "KAIROS_PERSONAL_PATTERNS가 가리키는 파일이 없습니다: $patterns_file" >> "$errors"
fi
if [ -f "$patterns_file" ]; then
  count=0
  # CRLF와 줄 끝 공백은 규칙을 조용히 무력하게 만든다. 떼고 읽는다.
  rules=$(sed -e 's/\r$//' -e 's/[[:space:]]*$//' "$patterns_file" | grep -vE '^[[:space:]]*(#|$)' || true)
  printf '%s\n' "$rules" | while IFS= read -r rule; do
    [ -n "$rule" ] || continue
    # `-`, `\.`, `_`, 공백을 구분자 하나로 본다. 대괄호 식 안은 그대로 둔다.
    relaxed=$(printf '%s\n' "$rule" | awk '{
      out = ""; inside = 0
      for (i = 1; i <= length($0); i++) {
        c = substr($0, i, 1)
        if (inside) { out = out c; if (c == "]") inside = 0; continue }
        if (c == "[") { inside = 1; out = out c; continue }
        if (c == "\\" && substr($0, i + 1, 1) == ".") { out = out "[-._ ]?"; i++; continue }
        if (c == "-" || c == "_" || c == " ") { out = out "[-._ ]?"; continue }
        out = out c
      }
      print out
    }')
    scan "$relaxed" -i
  done | report "개인 규칙($patterns_file)에 걸린 값"
  count=$(printf '%s\n' "$rules" | grep -c . || true)
  personal="개인 규칙 ${count}개"
else
  personal="개인 규칙 없음(.personal-patterns)"
fi

if [ -s "$errors" ]; then
  cat "$errors" >&2
  echo "검사를 끝내지 못했습니다. 통과로 치지 않습니다." >&2
  exit 2
fi
if [ -s "$findings" ]; then
  cat "$findings" >&2
  echo "공개 저장소에 둘 수 없는 값이 있습니다. 합성 값으로 바꾸세요." >&2
  exit 1
fi
echo "개인 식별자 없음 (일반 규칙, $personal)"
