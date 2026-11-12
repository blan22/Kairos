# 바뀐 것

판마다 사용자에게 보이는 변화를 적는다. 옮기는 절차가 있으면 `docs/`의 해당 문서를 가리킨다.

## 다음 판

### 더함

- 알림 권한을 다른 Mac 앱처럼 macOS 권한 창으로 받는다. 터미널에서 묻지 않는다. 전에 거절했으면 시스템 설정의 알림 화면을 연다.
- `kairos notify setup`(권한 받기), `kairos notify test`(시험 알림).
- `kairos doctor`에 `Mac 알림` 줄과 끝의 요약(`문제 없음` 또는 확인할 것 목록).
- `kairos help`가 명령을 하는 일로 묶어 한 줄 설명과 함께 보여 준다. `kairos help <명령>`, `kairos <명령> --help`, `kairos --version`.
- 잘못 친 명령에 가까운 이름을 권하고, 옵션 오류 아래에 사용법 줄을 붙인다.
- `kairos update`. 지금 판과 설치 방식에 맞는 업데이트 명령을 알려 준다.
- `kairos completions zsh|bash`. 셸 자동 완성 스크립트.
- `kairos uninstall --runtime`(실행본과 명령 링크까지), `--purge`(데이터 폴더와 키체인의 세션 토큰까지). 지울 것을 먼저 보여 주고 묻는다.
- 빌드 없는 설치. `scripts/install-release.sh`가 릴리스 묶음을 받아 SHA256을 확인하고 설치한다. 묶음은 `scripts/package.sh`와 릴리스 워크플로가 만든다.
- `scripts/install.sh --update`(당기고 다시 설치. 묻지 않으려면 `--yes`를 함께 준다), `--no-modify-path`, `--help`.
- 설치 스크립트 시험(`scripts/harness/install.test.mjs`). `scripts/verify.sh fast`가 함께 돌린다.
- [문제 해결](docs/troubleshooting.md) 문서, 이슈와 PR 템플릿.

### 바꿈

- `scripts/install.sh`가 빠진 도구를 하나씩이 아니라 한 번에 모아 알린다.
- `scripts/install.sh`가 `~/.local/bin`이 PATH에 없으면 `~/.zshrc`에 한 줄을 더할지 묻는다(`--yes`면 묻지 않고 더한다).
- `kairos uninstall`의 끝 안내가 남은 것을 지우는 옵션을 알려 준다.

## 0.2.0

공개 첫 판.
