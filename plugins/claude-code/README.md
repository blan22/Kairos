# kairos Claude Code 플러그인

kairos를 Claude Code에서 본다. `kairos mcp serve`를 MCP 서버로 붙이고 슬래시 명령 네 개를 둔다.

| 명령 | 하는 일 | 필요한 설정 |
|---|---|---|
| `/kairos` | 지금 볼 일과 열린 항목 | 없음 |
| `/kairos-done <ID>...` | 처리함으로 닫기 | `[mcp] allow_actions`에 `done` |
| `/kairos-later <ID>...` | 다음 묶음으로 미루기 | `[mcp] allow_actions`에 `later` |
| `/kairos-start <ID>` | 세션에 맡기기 | `allow_actions`에 `accept`, `[hosts] enabled`에 `claude-code`, `kairos sessions on` |

## 설치

```
kairos install --apply           # current 링크를 만든다
claude plugin install <저장소>/plugins/claude-code
```

## 보안

- MCP 서버는 기본으로 읽기 도구(`kairos_next`, `kairos_list`)만 연다. 상태를 바꾸는 도구는 `config.toml`의 `[mcp] allow_actions`로 켠 것만 나온다.
- 세션 시작은 `claude-code` 호스트도 켜야 나오고, kairos 세션 안(`KAIROS_SESSION`)에서는 수락 명령이 거절한다. kairos 세션은 `--strict-mcp-config`로 떠서 이 서버를 읽지 않는다.
- 설정, 연동, 설치, 세션 잠금은 도구로 내지 않는다.
- 슬래시 명령의 `allowed-tools`는 그 명령이 부를 도구 하나만 허락한다. 모델이 다른 대화에서 상태를 바꾸는 도구를 부르면 Claude Code가 권한을 묻는다.
- 실행 파일은 `~/Library/Application Support/kairos/current/bin/kairos`만 쓰고, 본인 소유이며 다른 사용자가 고칠 수 없을 때만 띄운다.
