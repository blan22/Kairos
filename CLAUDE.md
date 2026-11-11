@AGENTS.md

## Claude Code

- 프로젝트 설정(`.claude/settings.json`)의 훅이 규칙 일부를 강제한다: 운영과 launchd를 바꾸는 명령, 생성 파일과 운영 폴더 편집은 막고, golden 편집과 `orca plugin shot`은 묻고, Rust 파일은 고칠 때마다 rustfmt로 맞추고, 끝낼 때 Rust나 TS를 고쳤으면 `scripts/verify.sh fast`를 돌린다. 훅이 막으면 이유를 읽고 다른 길을 찾거나 사용자에게 묻는다. 우회하지 않는다.
- Stop 훅을 잠시 끄려면 `KAIROS_HARNESS_STOP=0`(긴 탐색 작업). 끈 채로 끝내지 않는다.
- 하위 에이전트: `reviewer`(정확성), `security-reviewer`(보안 경계). 둘 다 읽기 전용이다.
- 개인 설정은 `CLAUDE.local.md`, `.claude/settings.local.json`(git 무시)에 둔다.
