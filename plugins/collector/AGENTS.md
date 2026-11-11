# 수집 훅 안내

Claude Code 모드 엔진이 이 훅을 모델 없이 돌린다. `$`만 쓸 수 있고 Node가 없다.

- `$`를 넘기는 함수는 함수 선언이어야 한다. 화살표 함수로 바꾸면 엔진이 모듈을 거부하고 틱이 `model_called`로 멈춘다.
- 고친 뒤 `node --test plugins/collector/test/tick.test.ts`와 `claude plugin validate plugins/collector`.
- `hooks/allowed.ts`는 설치기(`kairos-ops/src/install/allowlist.rs`)가 바인딩으로 다시 쓰는 파일이다. 손으로 고치지 않는다. 저장소 판은 바인딩이 없을 때의 출력과 같아야 한다(`kairos-ops` 시험).
- 허용 목록 밖의 호출은 기본 거부다. 이 기본을 바꾸지 않는다.
