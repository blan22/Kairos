#!/bin/sh
# 실제 Orca 렌더 캡처. panel.check.ts의 시나리오마다 어두운 테마와 밝은 테마로 찍어 public/shots에 둔다.
# orca plugin shot은 시나리오를 합성 데이터로 그리고 실제 kairos를 부르지 않는다. 다만 Orca 창이 숨겨진 동안만 찍으므로
# 이 스크립트를 돌리는 동안 Orca 창(과 그 안의 터미널)이 잠시 가려진다. 사람이 직접 돌린다.
# 설치된 플러그인 키(local.kairos)로 찍지 않는다. 그 패널을 열면 실제 next --seen이 불려 노출 예산을 쓴다.
set -eu
cd "$(dirname "$0")/.."
panel=../plugins/orca-panel
out=public/shots
mkdir -p "$out"
names=$(node --input-type=module -e "import { scenarioNames } from './scripts/scenarios.mjs'; console.log(scenarioNames().join(' '))")
for name in $names; do
  for theme in dark light; do
    orca plugin shot "$panel" --scenario "$name" --theme "$theme" --width 360 --out "$out/$name-$theme.png" --json >/dev/null \
      && echo "찍음 $name $theme" || echo "실패 $name $theme" >&2
  done
done
