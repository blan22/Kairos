@AGENTS.md

## Claude Code

- 프로젝트 설정(`.claude/settings.json`)의 훅이 규칙 일부를 강제한다.
  - 막는다: 운영 환경과 launchd를 바꾸는 명령, 생성 파일과 운영 폴더 편집.
  - 사용자에게 묻는다: golden 편집, `orca plugin shot`.
  - 자동으로 한다: Rust 파일을 고칠 때마다 rustfmt를 적용하고, 작업을 끝낼 때 Rust나 TS를 고쳤으면 `scripts/verify.sh fast`를 돌린다.
- 훅이 막으면 이유를 읽고 다른 방법을 찾거나 사용자에게 묻는다. 우회하지 않는다.
- 긴 탐색 작업처럼 Stop 훅을 잠시 꺼야 하면 `KAIROS_HARNESS_STOP=0`을 쓴다. 끈 채로 작업을 끝내지 않는다.
- 하위 에이전트는 `reviewer`(정확성)와 `security-reviewer`(보안 경계)다. 둘 다 읽기 전용이다.
- 개인 설정은 `CLAUDE.local.md`와 `.claude/settings.local.json`(git 무시)에 둔다.
