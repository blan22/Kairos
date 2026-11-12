---
name: add-source
description: 새 수집 소스(커넥터)를 추가하거나 기존 소스의 MCP 동작을 늘릴 때 쓴다. 읽기 동작 목록, 소스 구현, 등록, 설정, 허용 목록, 테스트를 빠짐없이 바꾼다.
argument-hint: "<소스 이름>"
---

아래 순서대로 바꾼다. 하나라도 빠지면 수집이 아무 경고 없이 not_connected로 남거나 훅이 호출을 막는다.

1. 읽기 동작: `crates/kairos-core/src/connector.rs`의 `Capability`에 추가한다. 읽기 동작만 넣는다. 쓰기 동사가 든 도구는 넣지 않는다.
2. 소스: `crates/kairos-source-mcp`(MCP) 또는 새 크레이트(직접 실행)에 `Source`와 `McpSource`/`DirectSource`를 구현한다. 서버와 도구 이름은 `Capability::server()`, `tool()`로 읽는다. 본문은 저장하지 않고, 앞부분 120자는 `kairos_core::text`로 정리한다.
3. 이름: `SourceName`(kairos-core `model/enums.rs`)에 추가한다. 본인 식별자가 필요하면 `Profile.identity`, `kairos-config`의 `RawIdentity`, `values.rs` 검사, `kairos connect` 하위 명령을 함께 추가한다.
4. 소스 목록: `crates/kairos-core/src/catalog.rs`의 `CATALOG`에 한 줄을 추가한다(요약, 필수 값, 선택 값). `kairos sources`, `kairos setup`, 수집 등록이 이 표를 읽는다. 새 본인 식별자라면 `Need`에도 추가하고 `setup`의 질문에 연결한다.
5. 등록: `crates/kairos-tick/src/registry.rs`의 `standard`에 구현을 추가한다. 켜짐 여부와 필요한 값은 소스 목록이 판정한다.
6. 허용 목록: `kairos-ops/src/install/allowlist.rs`와 `plugins/collector/hooks/allowed.ts`(생성 결과 그대로)를 맞추고, 수집 훅 `tick.ts`의 페이지 넘김이 그 도구를 처리하는지 확인한다.
7. 신뢰 판정: 사람의 등급(T0~T3)은 `Profile.org`로 정한다. 정규식은 설정으로 받지 않는다.
8. 테스트: 합성 응답 픽스처(`tests/fixtures`), 파서 테스트, `kairos-tick`의 등록 테스트, 수집 훅 테스트(`node --test plugins/collector/test/tick.test.ts`), `claude plugin validate plugins/collector`.
9. 문서: `docs/sources.md`의 소스 표, `SECURITY.md`(외부 쓰기 목록), `docs/surface-protocol.md`(hello의 sources).
10. `security-reviewer` 에이전트로 검토하고 `scripts/verify.sh full`을 돌린다.
