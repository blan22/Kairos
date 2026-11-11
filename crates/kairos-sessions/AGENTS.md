# kairos-sessions 안내

이 크레이트는 보안 경계다. 세션은 사용자가 누른 일을 제한된 claude로 띄운다.

- 거부 목록, 샌드박스, `--settings` 내용은 넓히지 않는다. 바꾸면 `crates/kairos-config/src/lib.rs`의 `SESSION_FLOOR_VERSION`을 올려 모든 사용자의 세션 잠금을 다시 건다.
- 토큰(`SessionToken`)은 인자, 로그, DB, 오류 코드, Debug 출력에 넣지 않는다. 자식 환경과 0600 settings 파일로만 간다.
- 세션 프롬프트에 들어가는 외부 값은 `issue_url_matches`를 통과한 URL과 검사한 기준 브랜치뿐이다. 제목이나 본문을 넣지 않는다.
- Orca와 git 호출은 고정 인자 배열이다. 저장소 훅은 끈다(`core.hooksPath=/dev/null`).
- `tests/fixtures/session_command.json`과 `session_profile.rs`는 인자를 글자까지 고정한다. 바꾸면 그 변화를 커밋 메시지에 적는다.
- 고친 뒤 `cargo test -p kairos-sessions`와 `security-reviewer`.
