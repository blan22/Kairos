# 호스트 계약

호스트(Orca 패널, VS Code 확장, Claude Code 플러그인)가 kairos와 주고받는 규약이다. 판단은 kairos가 하고, 호스트는 화면을 그리고 버튼을 전달하는 일만 맡는다.

형식 번호는 `CONTRACT_VERSION`(현재 1)이고 응답마다 `schema`로 들어온다. 필드는 추가만 한다. 필드를 빼거나 의미를 바꾸면 번호를 올린다.

## 원칙

- 호스트는 kairos 실행 파일을 셸 없이 인자 배열로 호출한다. 실행 파일은 `<KAIROS_HOME>/current/bin/kairos`이고, 본인 소유이며 그룹이나 다른 사용자가 쓸 수 없을 때만 호출한다.
- 호스트가 만드는 인자는 고정 문자열, `^[1-9]\d{0,14}$`를 통과한 항목 ID, 사용자가 직접 입력한 할 일 제목(`--` 뒤)뿐이다. kairos가 준 `argv`는 쓰지 않는다.
- stdout 전체가 JSON 하나다. `schema`가 다르면 그리지 않는다. 종료 코드는 0 성공, 1 실행 실패, 2 사용법 오류다.
- `title`, `url`, 메시지 앞부분은 외부에서 온 문자열이다. 텍스트로만 그리고, URL은 허용한 호스트의 https만 사용자가 눌렀을 때 연다.
- 호출은 한 번에 하나만 실행하고 시간 제한을 둔다(읽기 15초, 쓰기 20초, 수락 115초).

공용 구현은 `packages/kairos-client`(TS)다. VS Code 확장은 이 패키지를 번들해서 쓴다. Orca 패널은 플러그인 폴더 밖을 읽지 못하므로 같은 규칙을 자체 코드(`plugins/orca-panel/src/runner.ts`)로 지킨다.

## 명령

| 명령 | 용도 | 상태 변경 |
|---|---|---|
| `hello --json` | 시작할 때 계약 버전, 엔진 버전, 켠 호스트, 세션 호스트, 세션 잠금, 소스 연결을 읽는다 | 없음 |
| `next [--seen] --surface <호스트> --json` | 지금 볼 일 카드, 묶음, 준비됨 카드, `reveal` | `--seen`일 때만 노출과 예산에 기록 |
| `list --json` | 열린 항목 | 없음 |
| `watch --json` | 상태가 바뀌면 `{"kind":"changed"}` 한 줄을 출력한다. 첫 줄은 `{"kind":"ready"}`다 | 없음 |
| `runs --json` | 세션 상태를 맞춘다(`next`의 `runs.active > 0`일 때) | run 상태 |
| `later`, `done [--drop]`, `keep [--until-deploy]`, `add -- <제목>` | 항목 조작 | 있음 |
| `accept <ID> [--cancel] [--from claude-code] --json` | 세션 시작과 되돌리기 | 있음 |
| `move <ID> --json` | 세션으로 이동(Orca 세션만) | 없음 |
| `pr <ID> --json` | 고치는 세션이 커밋한 브랜치를 사용자의 git, gh로 push하고 draft PR을 만든다. 응답은 `{"pr":{"status","message","url","branch"}}` | 원격 브랜치와 PR |
| `reveal <ID>...` | 알림을 눌렀을 때 호출한다. `reveal.json`을 쓰고, Orca 호스트를 켰으면 패널을 연다 | `reveal.json` |

`--surface`에는 호출한 호스트 이름(`orca`, `claude-code`, `vscode`)을 준다. `panel`은 `orca`의 옛 이름이다.

## 수락 응답의 `launch`

세션 호스트가 `git`이면 수락 응답에 세션을 띄울 명령이 함께 온다.

```json
{ "launch": { "cwd": "/…/atlas-kairos/t-1a2b3c4d", "argv": ["/…/current/bin/kairos-run", "<run id>"], "command": "cd '/…' && '/…/kairos-run' <run id>" } }
```

- 호스트는 `argv[0]`이 `/kairos-run`으로 끝나는 절대 경로이고 `argv[1]`이 run ID 형식일 때만 터미널을 연다. 여는 시점은 사용자가 버튼을 누른 그 순간이고, 셸을 거치지 않는다.
- 터미널을 열 수 없는 호스트(Claude Code)는 `command`를 보여 주고 사용자가 직접 실행한다.
- 30분 안에 세션이 뜨지 않으면 run은 실패로 내려가고 항목은 되돌아간다.

## 호출한 호스트

`accept`는 세션이나 Orca 터미널 안(`CLAUDECODE`, `ORCA_TERMINAL_HANDLE`)에서 호출하면 거절한다. `--from claude-code`는 `hosts.enabled`에 `claude-code`가 있을 때만 이 거절을 통과한다. kairos 세션 안(`KAIROS_SESSION`)에서는 어떤 경우에도 거절한다.

## MCP

`kairos mcp serve`는 같은 명령을 MCP 도구로 제공한다. 기본으로는 `kairos_next`와 `kairos_list`만 열고, `[mcp] allow_actions`로 켠 동작(`later`, `done`, `keep`, `add`, `accept`)만 추가한다. `accept`는 `claude-code` 호스트도 켜야 나온다. 설정, 연동, 설치, 세션 잠금은 도구로 제공하지 않는다.
