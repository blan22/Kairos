---
name: panel-scenario
description: Orca 패널의 화면 상태(시나리오)를 더하거나 패널 UI를 고칠 때 쓴다. 점검 시나리오, 스토리, 시험, 실제 렌더 캡처를 함께 맞춘다.
argument-hint: "<시나리오 이름>"
allowed-tools: Bash(npm --prefix storybook run *), Bash(node --test *), Bash(scripts/verify.sh *)
---

1. 합성 데이터는 `plugins/orca-panel/src/fixtures.ts`의 함수로 만든다. 실제 이름, 저장소, URL을 쓰지 않는다.
2. `plugins/orca-panel/src/panel.check.ts`의 `defineCheck`에 시나리오를 더한다. 버튼을 누르는 흐름은 `panel.test.tsx`에 시험으로 둔다.
3. `npm --prefix storybook run gen`으로 스토리를 다시 만든다(스토리 파일은 손으로 고치지 않는다). 패널이 대역에 없는 Orca 컴포넌트를 쓰기 시작했으면 `storybook/src/orca/components.tsx`에 더한다.
4. `scripts/verify.sh panel`과 `scripts/verify.sh storybook`을 돌린다.
5. 실제 모양은 `npm --prefix storybook run shots`로 찍는다. Orca 창이 숨겨져 이 세션도 가려지므로 매번 사용자 확인을 받는다. 설치된 플러그인 키(`local.kairos`)로는 찍지 않는다.
6. 패널 폴더를 고치면 Orca의 개발 플러그인이 다시 승인을 기다릴 수 있다. 승인은 사용자가 한다. 끝날 때 알린다.
