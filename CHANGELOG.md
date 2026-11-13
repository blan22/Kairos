# 변경 기록

사용자에게 보이는 변화를 버전별로 적습니다. 형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 참고합니다. 마이그레이션 절차가 필요한 버전은 `docs/`의 해당 문서를 가리킵니다.

## [다음 버전]

### 변경

- 문서를 합니다체로 다시 쓰고 구조를 정리했습니다. [핵심 개념](docs/concepts.md), [설정](docs/configuration.md), [세션 샌드박스](docs/session-sandbox.md), [문서 목차](docs/README.md), [기여 안내](CONTRIBUTING.md)를 새로 만들었습니다. 설정 내용은 `docs/install.md`에서 `docs/configuration.md`로, 세션의 내부 동작은 `docs/sessions.md`에서 `docs/session-sandbox.md`로 옮겼습니다.

## [0.4.2] - 2026-11-13

### 수정

- 0.4.1에서도 Orca 패널 워커가 `content hash mismatch`로 뜨지 않던 문제를 고쳤습니다. Orca는 개발 폴더의 승인 해시를 잴 때 `.orca`를 빼지만, 워커를 띄울 때는 넣고 잽니다. 그래서 0.4.1처럼 타입 파일을 먼저 만들어도 해시가 맞지 않았습니다. 이제 setup이 적재와 승인 뒤에 패널의 `.orca`를 지우고 워커를 다시 띄웁니다. 이미 설치한 버전은 [문제 해결](docs/troubleshooting.md#orca-패널)의 두 명령으로 고칠 수 있습니다.
- 패널 승인을 미뤘을 때 setup이 `orca plugin dev load --approve`를 직접 실행하라고 안내하던 것을 고쳤습니다. 이제 `kairos setup`을 다시 실행하라고 안내합니다.

## [0.4.1] - 2026-11-13

### 수정

- 새 버전을 설치하고 Orca 패널을 승인하면 패널 워커가 `content hash mismatch`로 뜨지 않던 문제를 고쳤습니다. Orca가 첫 적재에서 승인한 뒤에 타입 파일(`.orca/types`)을 써서 해시가 달라졌습니다. setup이 승인 전에 이 파일을 만듭니다.
- setup의 패널 승인 안내가 아이콘을 종으로 부르던 것을 고쳤습니다.

## [0.4.0] - 2026-11-13

### 추가

- Orca 패널 위쪽에 프로바이더 칩(`전체`, `GitHub`, `Slack`, `Calendar`, `Gmail`, `Notion` 등)을 두었습니다. 칩을 고르면 그 출처의 일만 보이고, 그 아래 `지금`, `대기`, `세션` 탭이 진행 상태를 나눕니다.
- 패널을 마지막으로 본 뒤 새로 온 일을 칩, 탭, 줄에는 빨간 점으로, 상태 표시줄에는 ` ●`로 알립니다. 새 일 줄을 2초 넘게 보면 본 것으로 기록하고, 화면의 점은 그 화면을 떠날 때 지웁니다.
- 펼친 줄과 카드가 항목을 연 정책 규칙의 문장과 판단 근거(PR, 내 담당, 검사 실패, 참석 응답 전, 소식 수 등)를 보여 줍니다.
- 에이전트 제안 상자가 플레이북이 하는 일, 맡겨도 된다고 본 근거, 세션이 일할 저장소와 기준 브랜치, 단계와 하지 않는 일을 보여 줍니다.
- `kairos list --json`, `next --json`의 항목에 `why`를, `next --json`에 `newest`를 추가했습니다. 계약 버전은 그대로입니다.

### 변경

- 설치(`kairos install --register`, `kairos setup`)가 수집 작업을 등록한 뒤 바로 한 번 실행합니다. 다음 3분 틱을 기다리지 않습니다.
- 패널의 `상태` 탭을 머리말의 `수집 상태` 버튼으로 옮겼습니다. 지연된 소스가 있으면 버튼이 경고 색이 됩니다.
- 패널과 상태 표시줄 아이콘을 종에서 알림 앱과 같은 계열의 `gauge`로 바꿨습니다. 플러그인 목록에는 알림 앱 아이콘을 씁니다.
- Orca 패널이 플러그인 자체 저장소(`storage:own`) 권한을 요청합니다. 새 일 표시의 기준(항목 ID와 수)만 저장합니다. 업데이트한 뒤 Orca에서 한 번 다시 승인해 주세요.
- 펼친 줄을 테두리 대신 바탕색으로 표시해, 누를 때 아래 줄이 흔들리지 않습니다.
- `kairos setup`의 설치 단계가 표시를 켭니다. 새로 설치해도 첫 수집부터 카드와 알림이 나옵니다. 표시 모드를 직접 고른 적이 있으면 그 값을 지킵니다. 하루 동안 기록만 하려면 `kairos display auto`를 실행하세요.

### 수정

- 수집 틱이 돌 때마다 macOS가 `kairos`의 데스크탑 폴더 접근을 묻던 문제를 고쳤습니다. 수집 세션의 claude가 스킬 링크를 따라 보호 폴더를 읽던 것이 원인이었습니다. 이제 사용자 스킬과 명령을 읽지 않습니다(`--disable-slash-commands`).
- 설치된 `kairos`로 `kairos setup`을 다시 실행하고 설치에 `y`로 답하면 `install_same_root`로 멈추던 문제를 고쳤습니다. 같은 버전이면 파일을 복사하지 않고 launchd 등록만 다시 합니다.

## [0.3.0] - 2026-11-12

### 추가

- Orca 패널을 `지금`, `대기`, `세션`, `상태` 탭으로 나눴습니다. 탭 이름 옆에 건수를 보여 줍니다.
- 펼친 행과 `먼저 볼 일` 카드에 올라온 이유와 권장 조작을 보여 줍니다.
- 패널 검색. 제목, 사유, 출처, 그룹 이름을 탭을 가로질러 찾습니다.
- `상태` 탭. 소스별 수집 상태, 열린 일의 건수, `먼저 볼 일` 카드의 횟수 예산을 보여 줍니다.
- `kairos later --when hour|tomorrow`. 패널에서는 `나중에` 옆의 `⋯`로 고릅니다.
- `kairos next --json`의 `sources`(소스별 수집 상태). 계약 버전은 그대로입니다.
- `kairos next --seen-ready`. 세션 소식만 확인으로 기록하고 카드 노출과 예산은 건드리지 않습니다.

### 변경

- 패널은 `먼저 볼 일` 카드가 화면에 있을 때만 노출을 기록합니다. 다른 탭과 검색 중에는 `--seen-ready`로 읽습니다.
- 패널을 다시 열거나 알림을 누르면 검색을 닫고 `지금` 탭으로 돌아옵니다.
- 상태 표시줄 문구를 `kairos N · 긴급 M`으로 바꿨습니다.

## [0.2.0] - 2026-11-12

첫 공개 릴리스입니다.

### 추가

- 알림 권한을 다른 Mac 앱처럼 macOS 권한 창으로 받습니다. 터미널에서는 묻지 않습니다. 전에 거절했다면 시스템 설정의 알림 화면을 엽니다.
- `kairos notify setup`(권한 받기), `kairos notify test`(테스트 알림).
- `kairos doctor`에 `Mac 알림` 줄과 마지막 요약(`문제 없음` 또는 확인할 항목 목록)을 추가했습니다.
- `kairos help`가 명령을 용도별로 묶어 한 줄 설명과 함께 보여 줍니다. `kairos help <명령>`, `kairos <명령> --help`, `kairos --version`도 추가했습니다.
- 잘못 입력한 명령에는 비슷한 이름을 제안하고, 옵션 오류 아래에는 사용법을 붙입니다.
- `kairos update`. 현재 버전과 설치 방식에 맞는 업데이트 명령을 알려 줍니다.
- `kairos completions zsh|bash`. 셸 자동 완성 스크립트를 출력합니다.
- `kairos uninstall --runtime`(런타임과 명령 링크까지), `--purge`(데이터 폴더와 키체인의 세션 토큰까지). 지울 대상을 먼저 보여 주고 확인을 받습니다.
- 빌드 없는 설치. `scripts/install-release.sh`가 릴리스 패키지를 받아 SHA256을 확인하고 설치합니다. 패키지는 `scripts/package.sh`와 릴리스 워크플로가 만듭니다.
- `scripts/install.sh --update`(pull한 뒤 다시 설치합니다. 질문 없이 진행하려면 `--yes`를 함께 주세요), `--no-modify-path`, `--help`.
- 설치 스크립트 테스트(`scripts/harness/install.test.mjs`). `scripts/verify.sh fast`가 함께 돌립니다.
- [문제 해결](docs/troubleshooting.md) 문서, 이슈와 PR 템플릿.

### 변경

- `scripts/install.sh`가 빠진 도구를 하나씩이 아니라 한 번에 모아 알려 줍니다.
- `scripts/install.sh`가 `~/.local/bin`이 PATH에 없으면 `~/.zshrc`에 한 줄을 추가할지 묻습니다(`--yes`면 묻지 않고 추가합니다).
- `kairos uninstall`이 끝날 때 남은 것을 지우는 옵션을 안내합니다.

[다음 버전]: https://github.com/blan22/Kairos/compare/v0.4.2...HEAD
[0.4.2]: https://github.com/blan22/Kairos/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/blan22/Kairos/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/blan22/Kairos/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/blan22/Kairos/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/blan22/Kairos/releases/tag/v0.2.0
