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

`kairos help`는 명령을 하는 일로 묶어 한 줄 설명과 함께 보여 준다. 호스트와 수집이 부르는 명령(`hello`, `watch`, `reveal`, `plan-tick`, `page-info`, `ingest`, `mcp`)은 `--all`에만 나온다. `kairos help <명령>`과 `kairos <명령> --help`는 그 명령의 설명과 사용법 줄을 낸다. 옵션을 잘못 주면 오류 코드 아래에 사용법 줄이 따라온다. 잘못 친 명령에는 가까운 이름을 권한다.

`notify setup`은 macOS 알림 권한 창을 띄운다. 터미널에서 묻지 않는다. 이미 받았으면 아무것도 띄우지 않고, 전에 거절했으면 권한 창이 다시 뜨지 않으므로 시스템 설정의 알림 화면을 연다. `notify test`는 고정 문구의 시험 알림 하나를 보낸다. 둘 다 설치본의 헬퍼를 쓰고 세션 안에서는 하지 않는다.

`doctor`는 끝에 확인할 것을 모아 다시 보여 준다. 없으면 `문제 없음`이다. `Mac 알림` 줄은 알림 권한 상태다. 권한을 아직 받지 않았으면 문제로 올린다.

`uninstall`은 launchd 잡만 내린다. `--runtime`은 실행본(`<데이터 폴더>/<판>`, `current`, `~/.local/bin/kairos` 링크)까지 지우고 수집 기록과 설정은 남긴다. `--purge`는 데이터 폴더 전체와 키체인의 세션 토큰까지 지운다. 둘 다 지울 것을 먼저 보여 주고 `y`일 때만 지운다(`--yes`면 묻지 않는다). `--dry-run`은 보여 주기만 한다. 둘 다 데이터 폴더가 링크이거나 홈 폴더이거나 그 위이면 거절한다. `KAIROS_HOME`을 잘못 준 채로 남의 폴더를 지우지 않기 위해서다. 실행본으로 보는 것은 데이터 폴더 바로 아래에서 이름이 판 번호이고 `bin/kairos`와 `bin/kairos-run`이 든 폴더뿐이다. `--purge`는 kairos만 쓰는 이름의 파일(`kairos.db`, `connections.toml`, `install-source`)이 하나도 없으면 거절한다. `~/.local/bin/kairos`는 이 데이터 폴더를 가리키는 링크일 때만 지운다. 잡을 내리지 못했으면 지우지 않고 멈춘다. 남은 잡이 지운 경로를 계속 가리키기 때문이다. Orca 패널은 내리는 명령을 알려 주기만 한다. 승인을 사용자가 했으므로 내리는 것도 사용자가 한다. 세션 안에서는 실행본을 지우지 않는다(`--dry-run`은 된다). 설정이 깨져도 `uninstall`은 돈다. 데이터 폴더를 먼저 지우고 키체인의 토큰을 마지막에 지운다.

`update`는 지금 판, 설치 방식, 업데이트하는 명령을 알려 준다. 아무것도 내려받거나 바꾸지 않는다. 설치 방식은 설치 스크립트가 데이터 폴더의 `install-source`에 적어 둔다(`release` 또는 `source:<저장소 경로>`). 저장소에서 빌드했으면 `<저장소>/scripts/install.sh --update`, 릴리스 묶음으로 설치했으면 설치 명령 한 줄이다. `--json`은 `update.version`, `update.method`(`source`, `release`, `unknown`), `update.command`를 낸다. 알려 줄 명령이 없으면 `command`는 키째 빠진다. `command`는 사람이 터미널에 붙여 넣는 셸 문자열이다. 호스트가 실행하지 않는다.

`completions`는 셸 자동 완성 스크립트를 표준 출력으로 낸다. `kairos help`에 나오는 명령과 하위 낱말(`on`, `off`, `setup` 등)을 채운다. 명령 바로 뒤 자리에서만 정해진 낱말을 내고, 그 뒤 자리는 파일 이름으로 채운다. zsh는 `~/.zshrc`의 `compinit` 뒤에 `source <(kairos completions zsh)`를, bash는 `~/.bashrc`에 `eval "$(kairos completions bash)"`를 더한다. macOS의 bash 3.2는 `source <(...)`를 읽지 못한다. `completions`와 `update`는 설정이 깨져도 돈다.

`--skip-slack-until`은 Slack 질의가 `parse_error`로 멈췄을 때 쓰는 복구 명령이다. 그 시각까지 건너뛰고, 건너뛴 구간은 7일 동안 `doctor`와 `list` 첫 줄에 나온다. 쪽 상한이나 틱 마감으로 잘린 Slack 질의는 다음 틱이 읽은 마지막 건 1초 앞부터 저절로 이어 읽는다(`page_cap`). 같은 초에 쪽 예산보다 많은 메시지가 몰려 이어 읽어도 앞으로 가지 못할 때만 `page_cap_stalled` 공백으로 알리고, 그때도 이 명령으로 건너뛸 수 있다.

`--reread-notion`은 Notion 파서를 고친 뒤 형식을 몰라 저장한 메일과 격리한 메일을 다음 틱부터 다시 읽게 한다.

`plan-tick`, `page-info`, `ingest`는 수집 모드가 부르는 내부 명령이다. 직접 실행한 `kairos collect`는 `KAIROS_CLAUDE`, `KAIROS_PLUGIN_DIR`이 있어야 MCP 소스를 읽는다. plist에는 둘 다 들어 있다.

`demo`는 계정 없이 보는 합성 데이터 폴더를 만든다. `KAIROS_HOME`을 직접 준 비어 있는 폴더에만 쓰고(`demo_home_required`, `demo_home_not_empty`), 외부 서비스와 gh, claude, Orca를 부르지 않는다. 예시 조직 프로필(`examples/org.toml`), 하루 종일 매일 카드를 내는 일정, 열린 표시 게이트, GitHub과 Slack 사건 넷(P1 둘, P2 둘)을 넣는다. 수집을 돌리지 않으므로 상태 줄에 `수집 기록 없음`이 나온다.
