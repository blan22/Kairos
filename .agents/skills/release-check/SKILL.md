---
name: release-check
description: main에 머지하거나 사내에 배포하기 전에 쓴다. 검증, 개인 정보, 문서, 판 번호, 이전 절차를 점검표로 확인한다.
allowed-tools: Bash(scripts/verify.sh *), Bash(./scripts/check-personal-data.sh), Bash(git status *), Bash(git diff *), Bash(git log *)
---

점검표. 하나씩 확인하고 결과를 표로 보고한다.

- [ ] main이 아닌 브랜치에서 작업했고 커밋 메시지는 영어로 기술 내용만 적었다(AI 작성 표기 없음).
- [ ] `scripts/verify.sh full` 통과. 근무 시간이면 사용자와 시점을 정한다.
- [ ] `./scripts/check-personal-data.sh` 통과.
- [ ] 계약(`CONTRACT_VERSION`), DB 스키마, 설정 `schema`, `SESSION_FLOOR_VERSION` 중 바뀐 것이 있으면 README와 `docs/`에 이전 절차를 적었다.
- [ ] 보안 경계를 건드렸으면 `security-reviewer` 결과를 반영했다.
- [ ] README, AGENTS.md, SECURITY.md, `docs/surface-protocol.md`가 코드와 맞다.
- [ ] 운영 설치본(`kairos install --apply`)과 launchd 등록은 하지 않았다. 사용자가 한다.
