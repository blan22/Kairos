# Orca 패널 안내

- 시험은 `orca plugin test .`(이 폴더에서). 타입은 `orca plugin types .`가 `.orca/types`에 만든다(git 무시).
- 패널(`panel.tsx`)은 Orca 컴포넌트만 쓰고 DOM을 그리지 않는다. 외부 문자열은 `Text`와 `ListItem` title로만 그리고 Markdown, Link를 쓰지 않는다.
- 워커만 쓰는 경로와 Node API는 `paths.ts`, `runner.ts`, `service.ts`에 둔다. `config.ts`는 패널도 읽으므로 Node 모듈을 가져오지 않는다.
- 이 폴더는 플러그인 폴더 밖 모듈을 읽지 못할 수 있어 `packages/kairos-client`를 쓰지 않는다. 같은 규칙(셸 없는 argv, ID 정규식, 계약 판 검사)을 `runner.ts`가 지킨다.
- 화면 상태는 `panel.check.ts`의 시나리오다. 더하면 `npm --prefix storybook run gen`. 절차는 `/panel-scenario`.
- 이 폴더를 고치면 Orca의 개발 플러그인이 다시 승인을 기다릴 수 있다. 승인은 사용자가 한다. 끝날 때 알린다.
- `orca plugin shot`은 Orca 창을 숨긴다. 매번 사용자 확인을 받고, `local.kairos`로는 찍지 않는다.
