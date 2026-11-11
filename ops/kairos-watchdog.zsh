#!/bin/zsh -f
# kairos 감시자. 5분마다 launchd가 부른다. claude와 Node에 의존하지 않는다.
# 외부 프로그램은 /usr/bin/sqlite3와 알림 헬퍼(KairosNotifier.app)만 쓴다. 시각과 파일 시각은 zsh 내장으로 읽는다.
# osascript는 쓰지 않는다. 그 알림은 '스크립트 편집기' 이름으로 뜨고, 누르면 Script Editor가 열리고, 권한 창을 따로 띄운다.
# 설치본 PATH가 비어 있어도 돌도록 PATH를 쓰지 않는다.

zmodload zsh/datetime zsh/stat
umask 077
export TZ=Asia/Seoul

home_dir=${KAIROS_HOME:-$HOME/Library/Application Support/kairos}
db=$home_dir/kairos.db
watch=$home_dir/watch.tsv
window_file=$home_dir/watch-window.tsv
last_file=$home_dir/watchdog.last
beat_file=$home_dir/watchdog.beat
resume_file=$home_dir/resumed.at
now=${KAIROS_WATCHDOG_NOW:-$EPOCHSECONDS}

sqlite=/usr/bin/sqlite3
# 설치본은 <rt>/ops/kairos-watchdog.zsh와 <rt>/ops/notifier/KairosNotifier.app에 함께 놓인다.
notifier=${KAIROS_WATCHDOG_NOTIFIER:-${0:A:h}/notifier/KairosNotifier.app/Contents/MacOS/KairosNotifier}

# 수집 시간과 요일은 수집이 watch-window.tsv에 적는다(설정의 schedule.collect, schedule.days).
# 파일이 없거나 모양이 틀리면 기본값(월~금 09:00~20:00)을 쓴다.
window_start=540
window_end=1200
window_days=12345
if [[ -f $window_file ]]; then
  IFS=$'\t' read -r a b c < $window_file
  if [[ $a == <-> && $b == <-> && $c =~ '^[1-7]{1,7}$' ]] && (( a < b && b <= 1440 )); then
    window_start=$a window_end=$b window_days=$c
  fi
fi

weekday=$(strftime '%u' $now)
hour=$(strftime '%H' $now)
minute=$(strftime '%M' $now)
second=$(strftime '%S' $now)
minutes_now=$(( 10#$hour * 60 + 10#$minute ))
[[ $window_days == *$weekday* ]] || exit 0
(( minutes_now >= window_start && minutes_now < window_end )) || exit 0

# 그날 수집 시작 시각. 밤사이 공백을 지연으로 세지 않기 위한 하한이다.
day_start=$(( now - ( (minutes_now - window_start) * 60 + 10#$second ) ))
today=$(strftime '%Y-%m-%d' $now)

# 잠자기 판정. 감시자는 5분마다 돈다. 간격이 7분 30초를 넘으면 Mac이 쉰 것이고 수집이 멈춘 것이 아니다.
# 깨어 있는 동안은 간격이 벌어지지 않으므로 멈춘 수집은 그대로 알린다. 수집도 잠자기를 보면 같은 파일에 쓴다.
resumed=0
if [[ -s $beat_file ]]; then
  read -r last_beat < $beat_file
  if [[ $last_beat == <-> ]] && (( now - last_beat > 450 )); then print -r -- $now > $resume_file; fi
fi
print -r -- $now > $beat_file
if [[ -s $resume_file ]]; then
  read -r resumed < $resume_file
  [[ $resumed == <-> ]] && (( resumed <= now )) || resumed=0
fi

notify() {
  local message=$1
  if [[ -n ${KAIROS_WATCHDOG_DRY:-} ]]; then
    print -r -- "알림: $message"
    return 0
  fi
  # 헬퍼가 없으면 알리지 않는다. 다음 판정에서 다시 시도한다.
  [[ -x $notifier ]] || return 127
  # 메시지는 고정 문구와 검사한 이름, 숫자뿐이다. 그래도 JSON 문자열로 감싼다.
  local body=${message//\\/\\\\}
  body=${body//\"/\\\"}
  $notifier post "{\"title\":\"kairos 점검\",\"body\":\"$body\",\"items\":[],\"tier\":\"P1\",\"thread\":\"kairos.watchdog\"}" >/dev/null 2>&1
}

problems=()
stalled=0

if [[ ! -f $db ]]; then
  problems+=("kairos 데이터베이스 없음")
else
  # 수집기가 쉬는 날(kairos off)이면 오늘은 판정하지 않는다.
  skip_today=0
  if [[ -f $watch ]]; then
    zstat -A mtime +mtime $watch 2>/dev/null
    first=$(read -r line < $watch; print -r -- $line)
    if [[ $first == $'active\t0' && $(strftime '%Y-%m-%d' ${mtime[1]:-0}) == $today ]]; then skip_today=1; fi
  fi

  if (( ! skip_today )); then
    last_run=$($sqlite -cmd '.timeout 3000' -cmd 'PRAGMA query_only = 1' $db "SELECT coalesce(strftime('%s', max(finished_at)), '') FROM source_run" 2>/dev/null)
    sqlite_status=$?
    if (( sqlite_status != 0 )); then
      problems+=("kairos 데이터베이스를 읽지 못함")
    elif [[ -z $last_run ]]; then
      # 마지막 run 행이 없는 것도 경고다.
      problems+=("kairos 수집 기록 없음")
    else
      floor=$(( last_run > day_start ? last_run : day_start ))
      floor=$(( floor > resumed ? floor : resumed ))
      age=$(( now - floor ))
      stalled=0
      (( age > 600 )) && stalled=1 && problems+=("kairos 수집기 $(( age / 60 ))분째 멈춤")

      if [[ -f $watch ]]; then
        while IFS=$'\t' read -r kind name query threshold; do
          if [[ $kind == alert ]]; then
            problems+=("kairos 점검 필요: ${name//[^A-Za-z0-9_.: -]/}")
            continue
          fi
          [[ $kind == source ]] || continue
          # 수집기 자체가 멈췄으면 소스별 지연은 같은 원인이다. 따로 적지 않는다.
          (( stalled )) && continue
          [[ $name =~ '^[a-z0-9_-]+$' && $query =~ '^[a-z0-9_-]+$' && $threshold =~ '^[0-9]+$' ]] || continue
          ok_at=$($sqlite -cmd '.timeout 3000' -cmd 'PRAGMA query_only = 1' $db "SELECT coalesce(strftime('%s', max(finished_at)), '') FROM source_run WHERE source = '$name' AND query = '$query' AND outcome = 'ok'" 2>/dev/null)
          ok_floor=$(( ${ok_at:-0} > day_start ? ${ok_at:-0} : day_start ))
          ok_floor=$(( ok_floor > resumed ? ok_floor : resumed ))
          ok_age=$(( now - ok_floor ))
          (( ok_age > threshold )) && problems+=("$name $(( ok_age / 60 ))분 지연")
        done < $watch
      fi
    fi
  fi
fi

if (( ${#problems} == 0 )); then
  # 복구되면 기록을 비운다. 다음 문제는 새 경고로 알린다.
  [[ -s $last_file ]] && : > $last_file
  exit 0
fi

# 한 소스의 여러 질의가 같은 지연을 내면 한 번만 적는다.
message=${(j:, :)${(u)problems}}
# 같은 경고는 반복하지 않는다. 분 단위 숫자만 달라진 것은 같은 경고다.
# 내용이 바뀌거나 복구 뒤 다시 생기면 다시 알린다.
key=${message//[0-9]/}
if [[ -s $last_file && $(<$last_file) == $key ]]; then
  exit 0
fi
# 알림이 나간 뒤에만 기록한다. 먼저 쓰면 실패한 알림이 재시도 없이 사라진다.
if notify $message; then
  print -r -- $key > $last_file
else
  # 표준 출력은 watchdog.log다. 코드만 남긴다.
  print -r -- "notify_failed $?"
fi
exit 0
