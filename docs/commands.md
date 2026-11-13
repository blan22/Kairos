# 명령

kairos의 모든 명령과 옵션입니다. 터미널에서는 `kairos help`로 같은 목록을, `kairos help <명령>`이나 `kairos <명령> --help`로 명령 하나의 사용법을 볼 수 있습니다.

## 한눈에 보기

**매일 쓰는 명령**

```
kairos next [--seen | --seen-ready] [--surface panel|orca|claude-code|vscode] [--ack] [--json]
kairos list [--would-show | --items | --json [--all]]
kairos later <항목 ID>... [--when hour|tomorrow] [--json]
kairos done <항목 ID>... [--drop] [--json]
kairos keep <항목 ID>... [--until-deploy] [--json]
kairos add <제목> [--json]
kairos triage [keep|drop <항목 ID>...] [--json]
kairos focus [분] | --end | --status [--json]
```

**세션에 맡기기**

```
kairos accept <항목 ID> [--cancel] [--from claude-code] [--json]
kairos move <항목 ID> [--json]
kairos pr <항목 ID> [--json]
kairos runs [--json]
kairos sessions [on|off] [--json]
```

**표시와 알림**

```
kairos display [on|off|auto] [--json]
kairos notify [on|off|setup|test] [--json]
kairos pause [분] | --resume
kairos off [YYYY-MM-DD] [--clear]
```

**설치, 설정, 점검**

```
kairos setup [--org <파일> [--sha256 <값>]] [--yes] [--no-install] [--google <주소>] [--slack <ID>] [--name <이름>]
kairos doctor
kairos sources [on|off <소스>...] [--json]
kairos connect [github | google <주소> | slack <ID> | name <이름> | mcp <소스> <서버> [동작=도구]... | session-token]
kairos config validate | show [--origin] | path | init [--org <파일> [--sha256 <값>]] [--force]
kairos install [--dry-run [--preview-dir 폴더]] | --apply | --register [--claude 경로] [--gh 경로] [--root 폴더]
kairos uninstall [--dry-run] [--runtime | --purge] [--yes]
kairos collect [--force] | --resume | --skip-slack-until <Message_ts 또는 ISO 시각> | --reread-notion
kairos update [--json]
kairos completions zsh|bash
KAIROS_HOME=<비어 있는 새 폴더> kairos demo [--json]
kairos help [<명령> | --all]
kairos --version
```

**호스트와 수집이 쓰는 명령**

```
kairos hello [--json]
kairos watch --json
kairos mcp serve
```

`kairos help`는 명령을 용도별로 묶어 한 줄 설명과 함께 보여 줍니다. 호스트와 수집이 호출하는 명령(`hello`, `watch`, `reveal`, `plan-tick`, `page-info`, `ingest`, `mcp`)은 `--all`을 줄 때만 나옵니다. 옵션을 잘못 주면 오류 코드 아래에 사용법이 함께 나오고, 잘못 입력한 명령에는 비슷한 이름을 제안합니다.

호스트가 쓰는 명령과 JSON 응답은 [호스트 계약](surface-protocol.md)에 있습니다.

## next

지금 볼 일 하나를 고릅니다. 몇 번을 호출해도 예산을 쓰지 않습니다.

- `--seen`은 '지금 볼 일' 카드의 노출과 예산, 세션 소식 확인을 함께 기록합니다.
- `--seen-ready`는 세션 소식 확인만 기록합니다. 카드 노출과 예산은 건드리지 않습니다. 카드를 그리지 않는 화면이 씁니다.
- 둘을 함께 주면 `--seen`을 따릅니다. 출력은 어느 쪽이든 같습니다.

## later

항목을 나중에 다시 보도록 미룹니다.

| 옵션 | 다시 올라오는 때 |
|---|---|
| 없음 | 다음 [묶음](concepts.md#묶음) 시각 |
| `--when hour` | 1시간 뒤 |
| `--when tomorrow` | 다음 근무일의 근무 시작 시각. 쉬는 날은 건너뜁니다 |

다른 값은 사용법 오류입니다. 미룬 횟수는 어느 쪽이든 한 번으로 셉니다.

## notify

- `notify setup`은 macOS 알림 권한 창을 띄웁니다. 터미널에서는 묻지 않습니다. 이미 권한이 있으면 아무것도 띄우지 않습니다. 전에 거절했다면 macOS가 권한 창을 다시 띄우지 않으므로 시스템 설정의 알림 화면을 엽니다.
- `notify test`는 고정 문구로 된 테스트 알림 하나를 보냅니다.
- 둘 다 설치본의 헬퍼를 쓰고, 세션 안에서는 동작하지 않습니다.
- `notify off`는 P1 알림만 끕니다. 자세한 동작은 [항목과 알림](items.md#p1-알림)에 있습니다.

## doctor

수집과 설정 상태를 점검하고, 마지막에 확인할 항목을 모아 다시 보여 줍니다. 확인할 것이 없으면 `문제 없음`이 나옵니다. `Mac 알림` 줄은 알림 권한 상태이고, 권한을 아직 받지 않았으면 문제로 표시합니다.

## uninstall

| 옵션 | 지우는 범위 |
|---|---|
| 없음 | launchd 작업만 해제합니다 |
| `--runtime` | 런타임(`<데이터 폴더>/<버전>`, `current`, `~/.local/bin/kairos` 링크)까지. 수집 기록과 설정은 남깁니다 |
| `--purge` | 데이터 폴더 전체와 키체인의 세션 토큰까지 |
| `--dry-run` | 지울 대상을 보여 주기만 합니다 |

`--runtime`과 `--purge`는 지울 대상을 먼저 보여 주고 `y`라고 답할 때만 지웁니다(`--yes`면 묻지 않습니다). 엉뚱한 폴더를 지우지 않도록 다음을 확인합니다.

- 데이터 폴더가 링크이거나, 홈 폴더이거나, 그 상위 폴더이면 거절합니다. `KAIROS_HOME`을 잘못 지정한 채로 다른 폴더를 지우는 일을 막기 위해서입니다.
- 런타임으로 인정하는 것은 데이터 폴더 바로 아래에 있고, 이름이 버전 번호이며, `bin/kairos`와 `bin/kairos-run`이 들어 있는 폴더뿐입니다.
- `--purge`는 kairos만 쓰는 이름의 파일(`kairos.db`, `connections.toml`, `install-source`)이 하나도 없으면 거절합니다.
- `~/.local/bin/kairos`는 이 데이터 폴더를 가리키는 링크일 때만 지웁니다.
- launchd 작업을 해제하지 못했으면 지우지 않고 멈춥니다. 남은 작업이 지워진 경로를 계속 가리키기 때문입니다.

그 밖의 동작은 다음과 같습니다.

- Orca 패널은 언로드하는 명령을 알려 주기만 합니다. 승인을 사용자가 했으므로 언로드도 사용자가 합니다.
- 세션 안에서는 런타임을 지우지 않습니다. `--dry-run`은 됩니다.
- 설정이 깨져 있어도 동작합니다.
- 데이터 폴더를 먼저 지우고, 키체인의 토큰을 마지막에 지웁니다.

## update

현재 버전, 설치 방식, 업데이트 명령을 알려 줍니다. 아무것도 내려받거나 바꾸지 않습니다.

- 설치 방식은 설치 스크립트가 데이터 폴더의 `install-source`에 기록해 둡니다(`release` 또는 `source:<저장소 경로>`).
- 저장소에서 빌드했다면 `<저장소>/scripts/install.sh --update`를, 릴리스 패키지로 설치했다면 설치 명령 한 줄을 알려 줍니다.
- `--json`은 `update.version`, `update.method`(`source`, `release`, `unknown`), `update.command`를 출력합니다. 알려 줄 명령이 없으면 `command` 키 자체가 빠집니다.
- `command`는 사람이 터미널에 붙여 넣는 셸 문자열입니다. 호스트는 이 값을 실행하지 않습니다.

## completions

셸 자동 완성 스크립트를 표준 출력으로 내보냅니다. `kairos help`에 나오는 명령과 하위 명령(`on`, `off`, `setup` 등)을 완성합니다. 명령 바로 다음 자리에서만 정해진 단어를 제안하고, 그 뒤로는 파일 이름을 완성합니다.

```sh
# zsh: ~/.zshrc의 compinit 뒤에 추가합니다
source <(kairos completions zsh)

# bash: ~/.bashrc에 추가합니다. macOS의 bash 3.2는 source <(...)를 읽지 못합니다
eval "$(kairos completions bash)"
```

`completions`와 `update`는 설정이 깨져 있어도 동작합니다.

## collect

수집을 한 번 돌리거나, 멈춘 수집을 풉니다.

- `--skip-slack-until`은 Slack 질의가 `parse_error`로 멈췄을 때 쓰는 복구 명령입니다. 지정한 시각까지 건너뛰고, 건너뛴 구간은 7일 동안 `doctor`와 `list`의 첫 줄에 나옵니다.
- 페이지 상한이나 틱 마감으로 잘린 Slack 질의는 다음 틱이 마지막으로 읽은 건의 1초 전부터 자동으로 이어 읽습니다(`page_cap`). 같은 초에 페이지 예산보다 많은 메시지가 몰려 이어 읽어도 진행하지 못할 때만 `page_cap_stalled` 공백으로 알립니다. 이때도 `--skip-slack-until`로 건너뛸 수 있습니다.
- `--reread-notion`은 Notion 파서를 고친 뒤에 씁니다. 형식을 알 수 없는 채로(`notion_unknown`) 저장한 메일과 격리한 메일을 다음 틱부터 다시 읽습니다.
- `plan-tick`, `page-info`, `ingest`는 수집 모드가 호출하는 내부 명령입니다.
- `kairos collect`를 직접 실행하려면 `KAIROS_CLAUDE`와 `KAIROS_PLUGIN_DIR`이 있어야 MCP 소스를 읽습니다. launchd plist에는 둘 다 들어 있습니다.

## demo

계정 없이 체험할 수 있는 합성 데이터 폴더를 만듭니다.

- `KAIROS_HOME`으로 직접 지정한 빈 폴더에만 씁니다(`demo_home_required`, `demo_home_not_empty`). 외부 서비스, gh, claude, Orca는 호출하지 않습니다.
- 예시 조직 프로필(`examples/org.toml`), 요일과 시간에 관계없이 카드를 내는 `[schedule]` 설정, 열린 표시 게이트, GitHub과 Slack 이벤트 네 건(P1 둘, P2 둘)을 넣습니다.
- 수집을 돌리지 않으므로 상태 줄에 `수집 기록 없음`이 나옵니다.
