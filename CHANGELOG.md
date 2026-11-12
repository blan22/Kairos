# 변경 기록

버전마다 사용자에게 보이는 변화를 적는다. 마이그레이션 절차가 있으면 `docs/`의 해당 문서를 가리킨다.

## 다음 버전

### 추가

- Orca 패널을 `지금`, `대기`, `세션`, `상태` 탭으로 나눴다. 탭 이름 옆에 건수를 보여 준다.
- 펼친 행과 `먼저 볼 일` 카드에 올라온 이유와 권장 조작을 보여 준다.
- 패널 검색. 제목, 사유, 출처, 그룹 이름을 탭을 가로질러 찾는다.
- `상태` 탭. 소스별 수집 상태, 열린 일의 건수, `먼저 볼 일` 카드의 횟수 예산을 보여 준다.
- `kairos later --when hour|tomorrow`. 패널에서는 `나중에` 옆의 `⋯`로 고른다.
- `kairos next --json`의 `sources`(소스별 수집 상태). 계약 버전은 그대로다.
- `kairos next --seen-ready`. 세션 소식만 확인으로 적고 카드 노출과 예산은 건드리지 않는다.

### 변경

- 패널은 `먼저 볼 일` 카드가 화면에 있을 때만 노출을 기록한다. 다른 탭과 검색 중에는 `--seen-ready`로 읽는다.
- 패널을 다시 열거나 알림을 누르면 검색을 닫고 `지금` 탭으로 돌아온다.
- 상태 표시줄 문구가 `kairos N · 긴급 M`이 됐다.

## 0.2.0

첫 공개 릴리스.

### 추가

- 알림 권한을 다른 Mac 앱처럼 macOS 권한 창으로 받는다. 터미널에서는 묻지 않는다. 전에 거절했다면 시스템 설정의 알림 화면을 연다.
- `kairos notify setup`(권한 받기), `kairos notify test`(테스트 알림).
- `kairos doctor`에 `Mac 알림` 줄과 마지막 요약(`문제 없음` 또는 확인할 항목 목록)을 추가했다.
- `kairos help`가 명령을 용도별로 묶어 한 줄 설명과 함께 보여 준다. `kairos help <명령>`, `kairos <명령> --help`, `kairos --version`도 추가했다.
- 잘못 입력한 명령에는 비슷한 이름을 제안하고, 옵션 오류 아래에는 사용법을 붙인다.
- `kairos update`. 현재 버전과 설치 방식에 맞는 업데이트 명령을 알려 준다.
- `kairos completions zsh|bash`. 셸 자동 완성 스크립트.
- `kairos uninstall --runtime`(런타임과 명령 링크까지), `--purge`(데이터 폴더와 키체인의 세션 토큰까지). 지울 대상을 먼저 보여 주고 확인을 받는다.
- 빌드 없는 설치. `scripts/install-release.sh`가 릴리스 패키지를 받아 SHA256을 확인하고 설치한다. 패키지는 `scripts/package.sh`와 릴리스 워크플로가 만든다.
- `scripts/install.sh --update`(pull 후 다시 설치. 질문 없이 진행하려면 `--yes`를 함께 준다), `--no-modify-path`, `--help`.
- 설치 스크립트 테스트(`scripts/harness/install.test.mjs`). `scripts/verify.sh fast`가 함께 돌린다.
- [문제 해결](docs/troubleshooting.md) 문서, 이슈와 PR 템플릿.

### 변경

- `scripts/install.sh`가 빠진 도구를 하나씩이 아니라 한 번에 모아 알려 준다.
- `scripts/install.sh`가 `~/.local/bin`이 PATH에 없으면 `~/.zshrc`에 한 줄을 추가할지 묻는다(`--yes`면 묻지 않고 추가한다).
- `kairos uninstall`이 끝날 때 남은 것을 지우는 옵션을 안내한다.
