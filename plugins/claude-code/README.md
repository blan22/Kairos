# kairos Claude Code 플러그인

Claude Code 안에서 kairos를 쓰는 플러그인입니다. `kairos mcp serve`를 MCP 서버로 연결하고, 슬래시 명령 네 개를 제공합니다.

## 설치

```sh
kairos install --apply           # current 링크를 만듭니다
claude plugin install <저장소>/plugins/claude-code
```

## 명령

| 명령 | 하는 일 | 필요한 설정 |
|---|---|---|
| `/kairos` | 지금 볼 일과 열린 항목 | 없음 |
| `/kairos-done <ID>...` | 처리 완료로 닫기 | `[mcp] allow_actions`에 `done` |
| `/kairos-later <ID>...` | 다음 묶음으로 미루기 | `[mcp] allow_actions`에 `later` |
| `/kairos-start <ID>` | 세션에 맡기기 | `allow_actions`에 `accept`, `[hosts] enabled`에 `claude-code`, `kairos sessions on` |

`/kairos`를 뺀 세 명령은 상태를 바꾸므로, 표의 설정을 켜야 쓸 수 있습니다. 설정 방법은 [설정](../../docs/configuration.md)에, 세션을 켜는 순서는 [에이전트 세션](../../docs/sessions.md)에 있습니다.

## 보안

MCP 서버는 기본으로 읽기 도구(`kairos_next`, `kairos_list`)만 엽니다. 상태를 바꾸는 도구는 `config.toml`의 `[mcp] allow_actions`로 켠 것만 나옵니다. 설정, 연동, 설치, 세션 잠금은 도구로 제공하지 않습니다.

- 세션 시작 도구는 `claude-code` 호스트까지 켜야 나옵니다.
- kairos 세션 안(`KAIROS_SESSION`)에서는 수락 명령이 거절됩니다. kairos 세션은 `--strict-mcp-config`로 뜨므로 이 서버를 읽지도 않습니다.
- 슬래시 명령의 `allowed-tools`는 그 명령이 호출할 도구 하나만 허용합니다. 다른 대화에서 모델이 상태를 바꾸는 도구를 호출하려 하면 Claude Code가 권한을 묻습니다.
- 실행 파일은 설치본의 `current/bin/kairos`만 씁니다. 자리는 macOS가 `~/Library/Application Support/kairos`, Linux가 `~/.local/share/kairos`입니다. 본인 소유이고 다른 사용자가 고칠 수 없을 때만 실행합니다.
- Windows에서는 Git Bash 같은 sh에서 `%LOCALAPPDATA%\kairos\current\bin\kairos.exe`를 실행합니다. 권한 비트가 없어 소유자와 권한은 검사하지 않습니다.
