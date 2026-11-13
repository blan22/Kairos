# 지원하는 OS

kairos는 macOS, Linux, Windows에서 돕니다. 수집, 카드, 알림, 호스트(Orca 패널, Claude Code 플러그인, VS Code 확장)는 세 OS에서 같게 동작합니다. [세션](concepts.md)(에이전트에 일 맡기기)은 macOS에서만 엽니다. 설치 방법은 [설치](install.md)에 있습니다.

## 한눈에 보기

| | macOS | Linux | Windows |
|---|---|---|---|
| 지원하는 머신 | Apple Silicon | x86_64, aarch64. glibc 2.35 이상 | Windows 10, 11(x86_64) |
| 수집 예약 | launchd | systemd 사용자 timer | 작업 스케줄러 |
| 알림 | 알림 헬퍼(KairosNotifier) | `notify-send` | 토스트 알림 |
| 알림을 누르면 | 패널이 열립니다 | 동작 없음 | 패널이 열립니다 |
| 세션 | 씁니다 | 열지 않습니다 | 열지 않습니다 |

## 파일이 놓이는 곳

| | macOS | Linux | Windows |
|---|---|---|---|
| 데이터 폴더(`KAIROS_HOME`) | `~/Library/Application Support/kairos` | `~/.local/share/kairos` | `%LOCALAPPDATA%\kairos` |
| `kairos` 명령 | `~/.local/bin/kairos` 링크 | `~/.local/bin/kairos` 링크 | 사용자 PATH의 `%LOCALAPPDATA%\kairos\current\bin` |
| 예약 작업 | `~/Library/LaunchAgents/dev.kairos.*.plist` | `~/.config/systemd/user/dev.kairos.*.service`, `.timer` | 작업 스케줄러의 `dev.kairos.*`. 원본은 데이터 폴더의 `tasks\` |

데이터 폴더의 구성과 권한(폴더 0700, 파일 0600)은 세 OS가 같습니다. Windows는 권한 비트가 없어서 다음 절의 규칙을 따릅니다.

## OS마다 다른 점

### Linux

- 수집은 systemd 사용자 timer가 3분마다, 감시자는 5분마다 실행합니다. 로그인한 동안만 돕니다. 로그아웃한 뒤에도 돌리려면 `loginctl enable-linger`를 직접 켜세요.
- 알림은 `notify-send`로 보냅니다. 알림을 눌러도 패널이 열리지 않습니다. 같은 항목은 Orca 패널의 알림 띠에 나옵니다.
- 잠자기는 `/sys/power/suspend_stats/success`의 횟수로 알아챕니다. 이 파일이 없으면 하트비트의 틈으로만 봅니다.
- Orca 패널은 PATH의 절대 경로 항목에서 `orca`를 찾습니다. 본인이나 root 소유이고 다른 사용자가 고칠 수 없는 파일만 씁니다.
- WSL2에서도 설치할 수 있습니다. `/etc/wsl.conf`의 `[boot]`에 `systemd=true`를 켜야 수집이 돕니다. 알림은 WSL의 데스크톱 환경에 따라 보이지 않을 수 있습니다.

### Windows

- 데이터 폴더는 `%LOCALAPPDATA%` 아래여야 합니다. Windows에서는 kairos가 폴더의 접근 권한(ACL)을 검사하지 않고, 사용자 프로필 폴더의 기본 권한에 기댑니다. 그래서 다른 곳을 `KAIROS_HOME`으로 지정하면 `home_not_private`로 거절합니다. 설치 스크립트도 같은 규칙으로 PATH와 파일을 바꾸기 전에 멈춥니다.
- 수집은 작업 스케줄러가 3분마다, 감시자는 5분마다 실행합니다. 작업은 창 없는 런처(`kairos-launch.exe`)가 띄우므로 콘솔 창이 깜박이지 않습니다.
- 알림은 토스트로 보냅니다. 보낸 이름(`dev.kairos.notifier`)과 `kairos-reveal:` 링크 처리기는 설치할 때 사용자 범위(HKCU)에 등록합니다. 관리자 권한은 필요 없습니다.
- 알림을 누르면 `kairos reveal --link`가 패널을 엽니다. 다른 프로그램도 이 링크를 열 수 있지만, 할 수 있는 일은 항목 번호로 패널을 여는 것뿐입니다.
- 잠자기 횟수를 읽지 않습니다. 하트비트의 틈으로만 봅니다.
- Orca 패널은 PATH의 절대 경로 항목에서 `orca.exe`를 찾습니다. npm으로 설치해 `orca.cmd`만 있으면 패널 열기 명령은 안내만 하고, 사이드바 아이콘으로 패널을 엽니다.
- Claude Code 플러그인의 MCP 서버는 Git Bash 같은 sh에서 뜹니다. 실행 파일의 소유자와 권한 비트는 검사하지 않고, 정해진 자리(`%LOCALAPPDATA%\kairos\current\bin\kairos.exe`)의 파일만 실행합니다.

## 세션을 macOS에서만 여는 이유

세션은 claude를 샌드박스에 가두고 [보안 기준](../SECURITY.md#세션)을 지켜야 엽니다. Linux와 Windows에서는 지금 이 기준을 지킬 수 없습니다.

- Linux의 Claude Code 샌드박스(bubblewrap)는 쓰기 금지 목록의 glob 패턴(예: `~/.config/git/**`)을 적용하지 않습니다. 세션이 git 설정 같은 파일을 고쳐 세션 밖에서 코드를 실행할 수 있습니다.
- seccomp 필터가 없으면 세션이 D-Bus로 키링과 systemd에 닿습니다.
- Windows에는 Claude Code 샌드박스가 없습니다. WSL2 안의 세션은 위 Linux와 같은 한계를 가집니다.

그래서 Linux와 Windows의 kairos는 세션을 아예 열지 않습니다.

- `kairos accept`와 `kairos sessions on`은 `sessions_unsupported`로 거절합니다.
- `kairos-run`은 `이 OS에서는 세션 경계를 지키는 샌드박스가 없어 세션을 열지 않습니다`를 출력하고 끝납니다.
- 카드의 다른 동작(나중에, 끝냄, 할 일 추가)과 수집, 알림은 그대로 씁니다.

샌드박스가 이 기준을 지킬 수 있게 되면 다시 검토합니다.
