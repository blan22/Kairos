# 명령

```
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
kairos notify [on|off] [--json]
kairos accept <항목 ID> [--cancel] [--from claude-code] [--json]
kairos move <항목 ID> [--json]
kairos pr <항목 ID> [--json]
kairos runs [--json]
kairos doctor
kairos pause [분] | --resume
kairos off [YYYY-MM-DD] [--clear]
kairos collect [--force] | --resume | --skip-slack-until <Message_ts 또는 ISO 시각> | --reread-notion
kairos install [--dry-run [--preview-dir 폴더]] | --apply | --register [--claude 경로] [--gh 경로] [--root 폴더]
kairos uninstall [--dry-run]
kairos setup [--org <파일> [--sha256 <값>]] [--yes] [--no-install] [--google <주소>] [--slack <ID>] [--name <이름>]
kairos sources [on|off <소스>...] [--json]
kairos config validate | show [--origin] | path | init [--org <파일> [--sha256 <값>]] [--force]
kairos connect [github | google <주소> | slack <ID> | name <이름> | mcp <소스> <서버> [동작=도구]... | session-token]
kairos mcp serve
KAIROS_HOME=<비어 있는 새 폴더> kairos demo [--json]
```

`--skip-slack-until`은 Slack 질의가 `parse_error`로 멈췄을 때 쓰는 복구 명령이다. 그 시각까지 건너뛰고, 건너뛴 구간은 7일 동안 `doctor`와 `list` 첫 줄에 나온다. 쪽 상한이나 틱 마감으로 잘린 Slack 질의는 다음 틱이 읽은 마지막 건 1초 앞부터 저절로 이어 읽는다(`page_cap`). 같은 초에 쪽 예산보다 많은 메시지가 몰려 이어 읽어도 앞으로 가지 못할 때만 `page_cap_stalled` 공백으로 알리고, 그때도 이 명령으로 건너뛸 수 있다.

`--reread-notion`은 Notion 파서를 고친 뒤 형식을 몰라 저장한 메일과 격리한 메일을 다음 틱부터 다시 읽게 한다.

`plan-tick`, `page-info`, `ingest`는 수집 모드가 부르는 내부 명령이다. 직접 실행한 `kairos collect`는 `KAIROS_CLAUDE`, `KAIROS_PLUGIN_DIR`이 있어야 MCP 소스를 읽는다. plist에는 둘 다 들어 있다.

`demo`는 계정 없이 보는 합성 데이터 폴더를 만든다. `KAIROS_HOME`을 직접 준 비어 있는 폴더에만 쓰고(`demo_home_required`, `demo_home_not_empty`), 외부 서비스와 gh, claude, Orca를 부르지 않는다. 예시 조직 프로필(`examples/org.toml`), 하루 종일 매일 카드를 내는 일정, 열린 표시 게이트, GitHub과 Slack 사건 넷(P1 둘, P2 둘)을 넣는다. 수집을 돌리지 않으므로 상태 줄에 `수집 기록 없음`이 나온다.
