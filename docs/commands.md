# 명령

```
kairos help [<명령> | --all]
kairos --version
kairos hello [--json]
kairos list [--would-show | --items | --json [--all]]
kairos next [--seen] [--surface panel|orca|claude-code|vscode] [--ack] [--json]
kairos watch --json
kairos later <항목 ID>... [--json]
kairos done <항목 ID>... [--drop] [--json]
kairos keep <항목 ID>... [--until-deploy] [--json]
kairos add <제목> [--json]
kairos triage [keep|drop <항목 ID>...] [--json]
kairos focus [분] | --end | --status [--json]
kairos display [on|off|auto] [--json]
kairos sessions [on|off] [--json]
kairos notify [on|off|setup|test] [--json]
kairos accept <항목 ID> [--cancel] [--from claude-code] [--json]
kairos move <항목 ID> [--json]
kairos pr <항목 ID> [--json]
kairos runs [--json]
kairos doctor
kairos pause [분] | --resume
kairos off [YYYY-MM-DD] [--clear]
kairos collect [--force] | --resume | --skip-slack-until <Message_ts 또는 ISO 시각> | --reread-notion
kairos install [--dry-run [--preview-dir 폴더]] | --apply | --register [--claude 경로] [--gh 경로] [--root 폴더]
kairos uninstall [--dry-run] [--runtime | --purge] [--yes]
kairos update [--json]
kairos completions zsh|bash
kairos setup [--org <파일> [--sha256 <값>]] [--yes] [--no-install] [--google <주소>] [--slack <ID>] [--name <이름>]
kairos sources [on|off <소스>...] [--json]
kairos config validate | show [--origin] | path | init [--org <파일> [--sha256 <값>]] [--force]
kairos connect [github | google <주소> | slack <ID> | name <이름> | mcp <소스> <서버> [동작=도구]... | session-token]
kairos mcp serve
KAIROS_HOME=<비어 있는 새 폴더> kairos demo [--json]
```

## 도움말

`kairos help`는 명령을 용도별로 묶어 한 줄 설명과 함께 보여 준다. 호스트와 수집이 호출하는 명령(`hello`, `watch`, `reveal`, `plan-tick`, `page-info`, `ingest`, `mcp`)은 `--all`을 줄 때만 나온다.

- `kairos help <명령>`과 `kairos <명령> --help`는 그 명령의 설명과 사용법을 출력한다.
- 옵션을 잘못 주면 오류 코드 아래에 사용법이 함께 나온다.
- 잘못 입력한 명령에는 비슷한 이름을 제안한다.

## notify

- `notify setup`은 macOS 알림 권한 창을 띄운다. 터미널에서는 묻지 않는다. 이미 권한이 있으면 아무것도 띄우지 않는다. 전에 거절했다면 macOS가 권한 창을 다시 띄우지 않으므로 시스템 설정의 알림 화면을 연다.
- `notify test`는 고정 문구의 테스트 알림 하나를 보낸다.
- 둘 다 설치본의 헬퍼를 쓰고, 세션 안에서는 동작하지 않는다.

## doctor

`doctor`는 마지막에 확인할 항목을 모아 다시 보여 준다. 없으면 `문제 없음`이다. `Mac 알림` 줄은 알림 권한 상태이고, 권한을 아직 받지 않았으면 문제로 표시한다.

## uninstall

| 옵션 | 지우는 범위 |
|---|---|
| 없음 | launchd 작업만 해제한다 |
| `--runtime` | 런타임(`<데이터 폴더>/<버전>`, `current`, `~/.local/bin/kairos` 링크)까지. 수집 기록과 설정은 남긴다 |
| `--purge` | 데이터 폴더 전체와 키체인의 세션 토큰까지 |
| `--dry-run` | 지울 대상을 보여 주기만 한다 |

`--runtime`과 `--purge`는 지울 대상을 먼저 보여 주고 `y`라고 답할 때만 지운다(`--yes`면 묻지 않는다). 엉뚱한 폴더를 지우지 않도록 다음을 확인한다.

- 데이터 폴더가 링크이거나, 홈 폴더이거나, 그 상위 폴더이면 거절한다. `KAIROS_HOME`을 잘못 지정한 상태에서 다른 폴더를 지우는 일을 막기 위해서다.
- 런타임으로 인정하는 것은 데이터 폴더 바로 아래에 있고, 이름이 버전 번호이며, `bin/kairos`와 `bin/kairos-run`이 들어 있는 폴더뿐이다.
- `--purge`는 kairos만 쓰는 이름의 파일(`kairos.db`, `connections.toml`, `install-source`)이 하나도 없으면 거절한다.
- `~/.local/bin/kairos`는 이 데이터 폴더를 가리키는 링크일 때만 지운다.
- launchd 작업을 해제하지 못했으면 지우지 않고 멈춘다. 남은 작업이 지워진 경로를 계속 가리키기 때문이다.

그 밖의 동작은 다음과 같다.

- Orca 패널은 언로드하는 명령을 알려 주기만 한다. 승인을 사용자가 했으므로 언로드도 사용자가 한다.
- 세션 안에서는 런타임을 지우지 않는다(`--dry-run`은 된다).
- 설정이 깨져 있어도 `uninstall`은 동작한다.
- 데이터 폴더를 먼저 지우고 키체인의 토큰을 마지막에 지운다.

## update

`update`는 현재 버전, 설치 방식, 업데이트 명령을 알려 준다. 아무것도 내려받거나 바꾸지 않는다.

- 설치 방식은 설치 스크립트가 데이터 폴더의 `install-source`에 기록해 둔다(`release` 또는 `source:<저장소 경로>`).
- 저장소에서 빌드했으면 `<저장소>/scripts/install.sh --update`를, 릴리스 패키지로 설치했으면 설치 명령 한 줄을 알려 준다.
- `--json`은 `update.version`, `update.method`(`source`, `release`, `unknown`), `update.command`를 출력한다. 알려 줄 명령이 없으면 `command` 키 자체가 빠진다.
- `command`는 사람이 터미널에 붙여 넣는 셸 문자열이다. 호스트는 이 값을 실행하지 않는다.

## completions

`completions`는 셸 자동 완성 스크립트를 표준 출력으로 내보낸다. `kairos help`에 나오는 명령과 하위 명령(`on`, `off`, `setup` 등)을 완성한다. 명령 바로 다음 위치에서만 정해진 단어를 제안하고, 그 뒤로는 파일 이름을 완성한다.

- zsh: `~/.zshrc`의 `compinit` 뒤에 `source <(kairos completions zsh)`를 추가한다.
- bash: `~/.bashrc`에 `eval "$(kairos completions bash)"`를 추가한다. macOS의 bash 3.2는 `source <(...)`를 읽지 못한다.

`completions`와 `update`는 설정이 깨져 있어도 동작한다.

## collect

- `--skip-slack-until`은 Slack 질의가 `parse_error`로 멈췄을 때 쓰는 복구 명령이다. 지정한 시각까지 건너뛰고, 건너뛴 구간은 7일 동안 `doctor`와 `list`의 첫 줄에 나온다.
- 페이지 상한이나 틱 마감으로 잘린 Slack 질의는 다음 틱이 마지막으로 읽은 건의 1초 전부터 자동으로 이어 읽는다(`page_cap`). 같은 초에 페이지 예산보다 많은 메시지가 몰려 이어 읽어도 진행하지 못할 때만 `page_cap_stalled` 공백으로 알리고, 이때도 `--skip-slack-until`로 건너뛸 수 있다.
- `--reread-notion`은 Notion 파서를 고친 뒤에 쓴다. 형식을 알 수 없는 채로(`notion_unknown`) 저장한 메일과 격리한 메일을 다음 틱부터 다시 읽는다.
- `plan-tick`, `page-info`, `ingest`는 수집 모드가 호출하는 내부 명령이다.
- `kairos collect`를 직접 실행할 때는 `KAIROS_CLAUDE`와 `KAIROS_PLUGIN_DIR`이 있어야 MCP 소스를 읽는다. launchd plist에는 둘 다 들어 있다.

## demo

`demo`는 계정 없이 체험할 수 있는 합성 데이터 폴더를 만든다.

- `KAIROS_HOME`으로 직접 지정한 빈 폴더에만 쓴다(`demo_home_required`, `demo_home_not_empty`). 외부 서비스, gh, claude, Orca는 호출하지 않는다.
- 예시 조직 프로필(`examples/org.toml`), 요일과 시간에 관계없이 카드를 내는 `[schedule]` 설정, 열린 표시 게이트, GitHub과 Slack 이벤트 네 건(P1 둘, P2 둘)을 넣는다.
- 수집을 돌리지 않으므로 상태 줄에 `수집 기록 없음`이 나온다.
