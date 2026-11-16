# kairos-sessions 안내

이 크레이트는 보안 경계다. 세션은 사용자가 고른 일을 제한된 claude로 띄운다.

- 거부 목록, 샌드박스, `--settings` 내용의 허용 범위를 넓히지 않는다. 바꿨다면 `crates/kairos-config/src/lib.rs`의 `SESSION_FLOOR_VERSION`을 올려 모든 사용자의 세션 잠금을 다시 건다.
- 세션에 GitHub 자격 증명을 넘기지 않는다. 토큰 이름은 env -u와 settings env에서 모두 비우고, 샌드박스 네트워크 허용 호스트를 두지 않는다.
- 이슈나 PR의 글은 `LiveAuthorReader::read_brief`가 작성자 확인과 같은 질의로 읽은 것만 세션 입력 파일(`--add-dir` run 폴더의 0600 파일)로 넘긴다.
- 세션 프롬프트에 들어가는 외부 값은 `issue_url_matches`를 통과한 URL과 검사를 거친 기준 브랜치뿐이다. 제목이나 본문은 넣지 않는다. 세션 입력 파일의 경로는 kairos가 정한 값이다.
- 세션 뒤의 PR 올리기(`AfterSession`)는 이 프로세스가 `ended`로 내린 고치는 세션이고 다른 고치는 세션이 없을 때만 부른다. 올리기 전에 사용자에게 일회용 코드를 받는다(`prepare_run` 뒤 `push`). 고정 답(y)은 남은 프로세스가 터미널에 넣을 수 있다.
- PR 올리기의 비교와 push는 검사용 저장소(`pr-check`)에서 한다. 세션이 쓸 수 있는 공용 .git에서 diff나 push를 하지 않는다.
- Orca와 git은 고정 인자 배열로 호출한다. 저장소 훅은 끈다(`core.hooksPath=/dev/null`).
- `tests/fixtures/session_command.json`과 `session_profile.rs`는 인자를 글자 단위로 고정한다. 바꿨다면 그 변화를 커밋 메시지에 적는다.
- 고친 뒤에는 `cargo test -p kairos-sessions`를 돌리고 `security-reviewer`로 검토한다.
