---
name: release-check
description: main에 머지하거나 사내에 배포하기 전에 쓴다. 검증, 개인 정보, 문서, 버전 번호, 마이그레이션 절차를 점검표로 확인한다.
allowed-tools: Bash(scripts/verify.sh *), Bash(./scripts/check-personal-data.sh), Bash(git status *), Bash(git diff *), Bash(git log *)
---

점검표다. 하나씩 확인하고 결과를 표로 보고한다.

- [ ] main이 아닌 브랜치에서 작업했고, 커밋 메시지는 영어로 기술 내용만 적었다(AI 작성 표기 없음).
- [ ] `scripts/verify.sh full`이 통과한다. 근무 시간이면 사용자와 돌릴 시점을 정한다.
- [ ] `./scripts/check-personal-data.sh`가 통과한다.
- [ ] 계약(`CONTRACT_VERSION`), DB 스키마, 설정 `schema`, `SESSION_FLOOR_VERSION` 중 바뀐 것이 있으면 README와 `docs/`에 마이그레이션 절차를 적었다.
- [ ] 보안 경계를 건드렸다면 `security-reviewer`의 검토 결과를 반영했다.
- [ ] README, AGENTS.md, SECURITY.md, `docs/surface-protocol.md`가 코드와 일치한다.
- [ ] 운영 설치(`kairos install --apply`)와 launchd 등록은 하지 않았다. 사용자가 한다.
