---
description: kairos 항목을 세션에 맡긴다(차단 테스트를 확인한 뒤 켠 경우만)
argument-hint: <항목 ID>
allowed-tools: mcp__plugin_kairos_kairos__kairos_accept
---

kairos_accept 도구를 id = $ARGUMENTS로 한 번 불러라. 인자가 1 이상의 정수 하나가 아니면 부르지 말고 사용법을 알려라.

- 결과의 message를 적어라.
- 결과에 launch가 있으면(세션 호스트가 git) launch.command를 코드 블록으로 보여 주고, 사용자가 새 터미널에서 직접 실행해야 한다고 알려라. 네가 실행하지 마라.
- 도구가 없다고 나오면 config.toml의 [mcp] allow_actions에 "accept"를, [hosts] enabled에 "claude-code"를 넣고 차단 테스트 뒤 kairos sessions on을 다시 실행해야 한다고 알려라.
- 다른 도구는 부르지 마라.
