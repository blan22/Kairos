# 수집 훅 안내

Claude Code 모드 엔진이 이 훅을 모델 없이 실행한다. `$`만 쓸 수 있고 Node API는 없다.

- `$`를 넘기는 함수는 함수 선언이어야 한다. 화살표 함수로 바꾸면 엔진이 모듈을 거부하고 틱이 `model_called`로 멈춘다.
- 고친 뒤에는 `node --test plugins/collector/test/tick.test.ts`와 `claude plugin validate plugins/collector`를 돌린다.
- `hooks/allowed.ts`는 설치기(`kairos-ops/src/install/allowlist.rs`)가 바인딩에 따라 다시 쓰는 파일이다. 직접 고치지 않는다. 저장소에 있는 파일은 바인딩이 없을 때의 출력과 같아야 한다(`kairos-ops` 테스트가 확인한다).
- 허용 목록에 없는 호출은 기본으로 거부한다. 이 기본값을 바꾸지 않는다.
