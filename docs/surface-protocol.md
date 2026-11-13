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
| `next [--seen \| --seen-ready] --surface <호스트> --json` | 지금 볼 일 카드, 묶음, 준비됨 카드, `reveal`, `sources` | `--seen`일 때만 노출과 예산에 기록. `--seen-ready`는 준비됨 카드만 확인으로 기록 |
| `list --json` | 열린 항목 | 없음 |
| `watch --json` | 상태가 바뀌면 `{"kind":"changed"}` 한 줄을 출력한다. 첫 줄은 `{"kind":"ready"}`다 | 없음 |
| `runs --json` | 세션 상태를 맞춘다(`next`의 `runs.active > 0`일 때) | run 상태 |
| `later [--when hour\|tomorrow]`, `done [--drop]`, `keep [--until-deploy]`, `add -- <제목>` | 항목 조작 | 있음 |
| `accept <ID> [--cancel] [--from claude-code] --json` | 세션 시작과 되돌리기 | 있음 |
| `move <ID> --json` | 세션으로 이동(Orca 세션만) | 없음 |
| `pr <ID> --json` | 고치는 세션이 커밋한 브랜치를 사용자의 git, gh로 push하고 draft PR을 만든다. 응답은 `{"pr":{"status","message","url","branch"}}` | 원격 브랜치와 PR |
| `reveal <ID>...` | 알림을 눌렀을 때 호출한다. `reveal.json`을 쓰고, Orca 호스트를 켰으면 패널을 연다 | `reveal.json` |

`--surface`에는 호출한 호스트 이름(`orca`, `claude-code`, `vscode`)을 준다. `panel`은 `orca`의 옛 이름이다.

## `next`의 `sources`

소스마다 수집 상태 한 줄이다. 표시 게이트가 닫혀 있어도 온다. 소스가 없으면 키 자체가 없다. 이 필드가 없는 엔진의 응답도 같은 계약이므로 호스트는 없는 경우를 그려야 한다.

```json
{ "sources": [{ "source": "slack", "label": "Slack", "status": "delayed", "lastOkAt": "2026-11-04T01:20:00.000Z", "delayMinutes": 37 }] }
```

- `status`는 `ok`, `delayed`, `never`(수집 기록 없음), `not_connected`, `quiet`(수집 시간 아님) 가운데 하나다. 한 소스에 질의가 여럿이면 가장 나쁜 상태를 쓰고, `lastOkAt`과 `delayMinutes`는 그 상태인 질의의 값만 쓴다. 지연이면 가장 오래된 `lastOkAt`과 가장 긴 `delayMinutes`를, 그 밖에는 가장 최근의 `lastOkAt`을 쓴다. 주기가 다른 질의의 시각은 섞지 않는다.
- `lastOkAt`과 `delayMinutes`는 값이 없으면 `null`이다.
- 호스트는 `source`를 `^[a-z][a-z0-9_]{0,31}$`로, `status`를 위 다섯 값으로 검사하고 틀린 줄은 버린다. `label`은 텍스트로만 그린다.

## 항목의 `why`와 `next`의 `newest`

`--json` 응답의 항목(`list`의 줄과 카드의 줄)에는 왜 올라왔고 왜 제안했는지의 근거 `why`가 붙는다. 값은 모두 코드와 수다. 외부에서 온 글은 들어 있지 않다. 근거를 만들 수 없으면 키 자체가 없다.

```json
{
  "why": {
    "rule": "p2.assigned",
    "events": 2,
    "facts": ["issue", "assigned_me", "milestone"],
    "offer": { "repo": "contoso-synthetic/app", "workspace": "contoso-synthetic/app", "base": "origin/main", "checks": ["assigned_me", "open", "mapped", "insiders", "no_session"] }
  }
}
```

- `rule`은 항목을 연 정책 규칙 ID다(`p1.ci_failed` 같은 `등급.이름`). 긴급, 오늘, 나머지 순으로 고른 소식의 규칙이다. 없으면 키가 없다.
- `events`는 이 항목을 열거나 다시 올린 소식 수다. 항목을 연 소식도 센다. 이미 열린 항목에 접혀 들어간 소식은 세지 않는다.
- `facts`는 객체에서 읽은 사실 코드이고 순서가 화면 순서다. GitHub는 `issue`, `pull_request`, `assigned_me`, `review_requested`, `authored_me`, `draft`, `changes_requested`, `approved`, `checks_failed`, `checks_pending`, `checks_passed`, `milestone`, 일정은 `response_needed`, `attending`, `tentative`, `declined`, `all_day`, Slack은 `dm`, Gmail은 `mail`이다. 비어 있으면 키가 없다.
- `attendees`와 `startsAt`은 일정의 참석자 수와 시작 시각이다. 일정이 아니면 키가 없다.
- `offer`는 에이전트 제안이 붙은 줄에만 있다. `repo`는 대상 저장소, `workspace`와 `base`는 세션이 일할 저장소와 기준 브랜치다. `checks`는 제안이 나온 조건 코드(`assigned_me`, `review_requested`, `reply_owed`, `open`, `mapped`, `insiders`, `no_session`)다.
- 호스트는 규칙 ID와 코드를 낱말 모양으로 검사하고 자기 표에 있는 것만 그린다. 모르는 코드는 버린다. 저장소와 브랜치는 모양이 틀리면 `offer`만 버린다.

`next`의 `newest`는 열린 항목(기준선과 직접 추가한 일 제외) 가운데 가장 큰 ID다. 표시 게이트가 닫혀 있어도 온다. 호스트가 마지막으로 본 뒤 새 항목이 생겼는지 가릴 때 쓴다. 열린 항목이 없으면 키가 없다.

두 필드는 추가 필드라서 계약 버전은 그대로다. 없는 엔진의 응답도 같은 계약이므로 호스트는 없는 경우를 그려야 한다.

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
