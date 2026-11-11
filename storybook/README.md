# kairos Storybook

플러그인 UI를 브라우저에서 본다.

| 묶음 | 내용 |
|---|---|
| Orca 패널/시나리오 | `plugins/orca-panel/src/panel.check.ts`의 시나리오마다 실제 패널 코드(`panel.tsx`)를 그린다. 테마와 폭을 바꾸고, 틱과 알림 누름 이벤트를 보낼 수 있다 |
| Orca 패널/실제 렌더 비교 | `npm run shots`로 찍은 실제 Orca 화면과 브라우저 대역을 나란히 본다 |
| VS Code/트리 보기 | 확장의 모델이 합성 목록을 묶음, 줄, 메뉴로 바꾼 모양 |

Claude Code 플러그인은 슬래시 명령과 MCP 도구라 그릴 화면이 없다.

## 실행

```
cd storybook
npm install
npm run storybook   # http://localhost:6006
npm run build       # 정적 빌드(dist)
npm test            # 스토리 목록이 시나리오와 같은지
npm run gen         # 시나리오를 더한 뒤 스토리 파일 다시 만들기
npm run shots       # 실제 Orca 렌더 캡처(Orca 창이 잠시 숨겨진다)
```

## 브라우저 대역

Orca 패널은 Orca 전용 `orca` 모듈로 그린다. Storybook의 Vite가 `orca`, `orca/jsx-runtime`, `orca/testing`을 `src/orca/`의 대역으로 돌린다.

- 컴포넌트는 React로 다시 만들었다. 모양은 Orca를 흉내 낼 뿐 같지 않다. 정확한 모양은 실제 렌더 캡처로 본다.
- atom은 메모리에만 있고 스토리마다 비운다. `plugin.request`는 시나리오의 처리기가 답하고(120ms 지연), 처리기가 없으면 실패한다.
- `shell.openExternal`은 열지 않고 콘솔에만 적는다.
- 패널 코드는 고치지 않는다. 대역에 없는 컴포넌트를 패널이 쓰기 시작하면 `src/orca/components.tsx`에 더한다.
