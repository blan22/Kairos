# 구조

크레이트의 의존은 표의 위에서 아래 방향으로만 흐른다.

| 크레이트 | 역할 |
|---|---|
| `crates/kairos-cli` | 실행 파일이자 전체를 조립하는 지점. 명령은 `Command`를 구현하고 `registry()`에 한 줄을 추가해 등록한다 |
| `crates/kairos-tick` | 틱 실행 조율. `TickRunner`, 잠금, 수집 모드, `SourceRegistry`, `Doctor` |
| `crates/kairos-ops` | 큰 응답 파일, 잠자기 판정, 유지보수, 설치 |
| `crates/kairos-sessions` | Orca 연동, run 수명 주기, 세션 프로필 |
| `crates/kairos-agenda` | 항목 엔진, 표시 게이트, 카드, 알림 |
| `crates/kairos-ledger` | 수집 기록: 저장, 멱등 처리, watermark, baseline, 종료 판정 |
| `crates/kairos-policy` | 등급, 종료, 배포 연결, 근무 시간. IO가 없다 |
| `crates/kairos-source-mcp`, `kairos-source-github` | 소스 구현 |
| `crates/kairos-store` | DB 연결, 마이그레이션, 트랜잭션 |
| `crates/kairos-config` | 설정 파일 읽기, 값 검증, 계층 병합, 세션 지문, `connections.toml` 쓰기 |
| `crates/kairos-core` | 값 객체, 모델, 포트(trait), 프로필(`profile`), 커넥터 동작 목록(`connector`), JS 호환 유틸 |

| 폴더 | 역할 |
|---|---|
| `profiles/` | 조직 프로필 |
| `packages/kairos-client` | 호스트 공용 TS 클라이언트 |
| `plugins/orca-panel`, `plugins/claude-code`, `plugins/vscode` | 호스트 어댑터 |
| `plugins/collector` | 수집 모드 훅 |
| `ops/` | launchd 템플릿, 감시자, 알림 헬퍼 |
| `docs/surface-protocol.md` | 호스트 계약 |

## 규칙

- SQL은 `*Repo`에만 쓴다. 시각은 `Clock`, 외부 프로세스는 `ProcessRunner`를 통해 다룬다.
- 소스를 추가하려면 `kairos_core::model`의 `Source`와 `McpSource` 또는 `DirectSource`를 구현하고 `SourceRegistry::standard`에 한 줄을 추가한다. MCP 소스는 호출할 읽기 동작을 `kairos_core::connector`에 추가한다.
- 사람이나 조직마다 다른 값은 상수로 두지 않고 `kairos_core::profile::current()`에서 읽는다. 테스트 빌드는 `test-profile` 기능으로 합성 프로필(`Profile::fixture`)을 쓴다.
- 수집이 끝나면 `StateBuilder`가 `state.json`을 갱신한다.

## 수집 모드 훅

수집 모드의 훅을 고치면 `claude plugin validate plugins/collector`를 돌린다.

- 엔진이 모듈을 거부하면 모듈이 로드되지 않아 틱이 `model_called`로 멈춘다. `$`를 넘기는 함수는 반드시 함수 선언이어야 한다.
- 이때 모델 요청은 `ANTHROPIC_BASE_URL`(멀티캐스트 `224.0.0.0`)로 향한다. 커널이 연결을 거절하므로 다른 로컬 사용자가 요청을 받을 수 없다.
- 배포 뒤 처음 도는 계약 테스트에서 claude가 이 주소로도 지연 없이 실패하는지 확인한다.
