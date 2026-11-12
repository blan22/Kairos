## 바꾼 것

<!-- 무엇을, 왜. 사용자에게 보이는 변화가 있으면 CHANGELOG.md의 "다음 판"에 한 줄 더한다. -->

## 확인

- [ ] `scripts/verify.sh full`이 통과한다
- [ ] 새 동작에 시험을 더했다
- [ ] 시험 값, 문서 예시, 커밋 메시지에 합성 값만 썼다(`scripts/check-personal-data.sh`)
- [ ] golden 고정값(`crates/*/tests/fixtures/golden`)을 고치지 않았다. 고쳤다면 출력이 바뀐 이유를 위에 적었다
- [ ] 보안 경계(세션 프로필, 설정 검증, 커넥터 목록, 수집 훅, MCP 서버, 설치기)를 고쳤다면 SECURITY.md의 바닥값을 넓히지 않았다
- [ ] 명령이나 설정이 바뀌었다면 `docs/`를 함께 고쳤다
