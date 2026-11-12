# Orca 패널 안내

- 테스트는 이 폴더에서 `orca plugin test .`로 돌린다. 타입은 `orca plugin types .`가 `.orca/types`에 만든다(git 무시).
- 패널(`panel.tsx`)은 Orca 컴포넌트만 쓰고 DOM을 직접 그리지 않는다. 외부 문자열은 `Text`와 `ListItem`의 title로만 그리고 Markdown, Link는 쓰지 않는다.
- 워커만 쓰는 경로와 Node API는 `paths.ts`, `runner.ts`, `service.ts`에 둔다. `config.ts`는 패널도 읽으므로 Node 모듈을 import하지 않는다.
- 이 폴더는 플러그인 폴더 밖의 모듈을 읽지 못할 수 있으므로 `packages/kairos-client`를 쓰지 않는다. 같은 규칙(셸 없는 argv, ID 정규식, 계약 버전 검사)을 `runner.ts`가 지킨다.
- 화면 상태는 `panel.check.ts`의 시나리오로 정의한다. 시나리오를 추가했다면 `npm --prefix storybook run gen`을 돌린다. 절차는 `/panel-scenario`에 있다.
- 이 폴더를 고치면 Orca의 개발 플러그인이 다시 승인 대기 상태가 될 수 있다. 승인은 사용자가 한다. 작업을 끝낼 때 사용자에게 알린다.
- `orca plugin shot`은 Orca 창을 숨긴다. 매번 사용자 확인을 받고, `local.kairos`로는 찍지 않는다.
