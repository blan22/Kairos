---
description: kairos 항목을 다음 묶음으로 미룬다
argument-hint: <항목 ID>...
allowed-tools: mcp__plugin_kairos_kairos__kairos_later
---

kairos_later 도구를 ids = [$ARGUMENTS]로 한 번 불러라. 인자가 1 이상의 정수가 아니면 부르지 말고 사용법을 알려라.
결과의 results마다 항목 ID와 message를 한 줄로 적어라. 다른 도구는 부르지 마라.
도구가 없다고 나오면 config.toml의 [mcp] allow_actions에 "later"를 넣어야 한다고 알려라.
