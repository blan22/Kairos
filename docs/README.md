# kairos 문서

처음이라면 [핵심 개념](concepts.md)을 읽고 [설치](install.md)로 넘어가세요.

## 사용자 가이드

| 문서 | 내용 |
|---|---|
| [핵심 개념](concepts.md) | 항목, 등급, 표시 게이트, 세션 같은 낱말의 뜻 |
| [설치](install.md) | 설치, 업데이트, 제거 |
| [지원하는 OS](platforms.md) | macOS, Linux, Windows에서 다른 점과 세션을 macOS에서만 여는 이유 |
| [설정](configuration.md) | `kairos setup`, 조직 프로필, 설정 파일, 근무 일정 |
| [항목과 알림](items.md) | 항목이 열리고 닫히는 방식, 예산, Mac 알림 |
| [에이전트 세션](sessions.md) | 세션 켜기, 맡길 수 있는 일, PR 올리기 |
| [문제 해결](troubleshooting.md) | 자주 막히는 곳과 해결 명령 |

## 레퍼런스

| 문서 | 내용 |
|---|---|
| [명령](commands.md) | 모든 명령과 옵션 |
| [소스](sources.md) | 서비스별 수집 방식, 잠자기 처리 |
| [호스트 계약](surface-protocol.md) | 호스트가 kairos와 주고받는 명령과 JSON |
| [보안](../SECURITY.md) | 보안 기준, 알려진 남은 위험, 취약점 신고 |
| [변경 기록](../CHANGELOG.md) | 버전별 변경 사항 |

## 설계와 개발

| 문서 | 내용 |
|---|---|
| [구조](architecture.md) | 크레이트 구조와 의존 방향 |
| [세션 샌드박스](session-sandbox.md) | 세션을 띄우고 가두는 방법 |
| [개발 가이드](development.md) | 빌드, 검증, 테스트 규칙 |
| [기여 안내](../CONTRIBUTING.md) | 이슈, PR, 문서 쓰기 |
| [AGENTS.md](../AGENTS.md) | 코딩 에이전트가 이 저장소에서 따르는 규칙 |

## 구성 요소

| 문서 | 내용 |
|---|---|
| [Orca 패널](../plugins/orca-panel/README.md) | Orca 사이드바 패널과 상태 표시줄 |
| [Claude Code 플러그인](../plugins/claude-code/README.md) | 슬래시 명령과 MCP 서버 |
| [VS Code 확장](../plugins/vscode/README.md) | 활동 막대의 kairos 보기 |
| [KairosNotifier](../ops/notifier/README.md) | Mac 알림 헬퍼 |
| [스토리북](../storybook/README.md) | 플러그인 UI 미리보기 |
| [조직 프로필](../profiles/README.md) | 관리자에게 받은 `org.toml`을 두는 곳 |
