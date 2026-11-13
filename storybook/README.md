# kairos Storybook

플러그인 UI를 브라우저에서 확인하는 Storybook입니다. 값은 모두 합성 데이터입니다. 실제 렌더 비교 스토리를 뺀 나머지는 실제 계정이나 Orca 없이 볼 수 있습니다. 실제 렌더 캡처는 git이 무시하므로 `npm run shots`로 먼저 찍어야 보입니다.

## 시작하기

```sh
cd storybook
npm install
npm run storybook   # http://localhost:6006
```

| 명령 | 하는 일 |
|---|---|
| `npm run storybook` | 개발 서버를 띄웁니다(http://localhost:6006) |
| `npm run build` | 정적 빌드를 만듭니다(`dist`) |
| `npm test` | 스토리 목록이 시나리오와 같은지 검사합니다 |
| `npm run gen` | 시나리오를 추가한 뒤 스토리 파일을 다시 만듭니다 |
| `npm run shots` | 실제 Orca 렌더를 캡처합니다 |

> [!NOTE]
> `npm run shots`를 실행하는 동안 Orca 창이 잠시 숨겨집니다.

## 스토리

| 분류 | 내용 |
|---|---|
| Orca 패널/시나리오 | `plugins/orca-panel/src/panel.check.ts`의 시나리오마다 실제 패널 코드(`panel.tsx`)를 그립니다. 테마와 폭을 바꾸고, 틱 이벤트와 알림 클릭 이벤트를 보낼 수 있습니다 |
| Orca 패널/실제 렌더 비교 | `npm run shots`로 찍은 실제 Orca 화면과 브라우저 mock을 나란히 비교합니다 |
| VS Code/트리 보기 | 확장의 모델이 합성 목록을 그룹, 줄, 메뉴로 바꾼 결과를 보여 줍니다 |

Claude Code 플러그인은 슬래시 명령과 MCP 도구로만 이루어져 있어서 그릴 화면이 없습니다.

## 브라우저 mock

Orca 패널은 Orca 전용 `orca` 모듈로 화면을 그립니다. Storybook의 Vite는 `orca`, `orca/jsx-runtime`, `orca/testing`을 `src/orca/`의 mock으로 연결합니다.

mock의 컴포넌트는 React로 다시 만든 것입니다. Orca의 모양을 흉내 낼 뿐 똑같지는 않습니다. 정확한 모양은 실제 렌더 캡처로 확인하세요.

- atom은 메모리에만 있고 스토리마다 초기화됩니다.
- `plugin.request`에는 시나리오의 처리기가 120ms 늦게 응답합니다. 처리기가 없으면 요청이 실패합니다.
- `shell.openExternal`은 실제로 열지 않고 콘솔에만 기록합니다.

패널 코드는 Storybook에 맞춰 고치지 않습니다. 패널이 mock에 없는 컴포넌트를 쓰기 시작하면 `src/orca/components.tsx`에 추가하세요.
