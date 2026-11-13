# 설정

kairos의 설정은 `kairos setup` 한 번으로 거의 끝납니다. 이 문서는 setup이 묻는 내용, 설정 파일, 직접 바꿀 수 있는 값을 설명합니다.

## kairos setup

```sh
kairos setup [--org <파일> [--sha256 <값>]] [--yes] [--no-install] [--google <주소>] [--slack <ID>] [--name <이름>]
```

setup은 다섯 가지를 차례로 묻습니다.

1. 조직 프로필. SHA256을 보여 주고, `y`라고 답할 때만 가져옵니다. 관리자에게 지문을 받았다면 `--sha256`으로 고정하세요.
2. 켤 서비스
3. 계정 연결. `--google`, `--slack`, `--name`으로 넘긴 값은 묻지 않고 기록합니다.
4. 설치. `y`라고 답할 때만 합니다. 설정만 바꾸려면 `--no-install`로 건너뛰세요.
5. Orca 패널

알림 권한은 터미널에서 묻지 않고 macOS 권한 창으로 받습니다. 여러 번 실행해도 괜찮습니다. Enter를 누르면 현재 값을 그대로 둡니다. `--yes`를 주면 조직 프로필도 묻지 않고 가져옵니다.

### 설치 단계

설치하면 수집 작업을 launchd에 등록하고 바로 한 번 실행합니다. 다음 3분 틱을 기다리지 않습니다. 다만 수집 시간 밖이거나 수집이 중단된 상태라면 그 실행은 수집하지 않고 끝납니다.

이때 [표시 게이트](concepts.md#표시-게이트)를 on으로 둡니다. 그래서 새로 설치해도 첫 수집부터 카드와 알림이 나옵니다.

- 표시 모드를 고른 적이 없고 표시가 열린 적도 없을 때만 on으로 바꿉니다. 직접 고른 값은 그대로 둡니다.
- 설치하지 못하면 되돌립니다.
- 하루 동안 기록만 하고 싶다면 `kairos display auto`를 실행하세요.

표시가 켜져 있으면 첫 수집에서 알릴 일을 바로 Mac 알림으로 보냅니다. 처음 설치할 때는 알림 권한을 허용하기 전에 첫 수집이 돌 수도 있습니다. 그때 알리지 못한 일은 권한을 허용한 뒤의 틱이 다시 알립니다.

마지막 두 단계(알림 권한, Orca 패널)는 설치본(`<데이터 폴더>/current`)이 있을 때만 진행합니다. 개발 트리의 번들과 패널 폴더는 권한과 승인을 따로 받기 때문에, 설치본을 기준으로 합니다.

### Mac 알림 권한

다른 Mac 앱과 같습니다. macOS 권한 창에서 허용하면 확인 알림이 하나 오고, 그때부터 알림이 옵니다.

- 헬퍼가 이미 권한을 받았다면(`status`가 `authorized`) 아무것도 띄우지 않습니다.
- 아직 물은 적이 없다면 번들을 LaunchServices에 등록(`lsregister -f`)한 뒤 `KairosNotifier setup`으로 권한 창을 띄웁니다. 창은 화면 오른쪽 위에 뜨고 2분 동안 기다립니다.
- 전에 거절했다면 macOS가 권한 창을 다시 띄우지 않습니다. 이때는 시스템 설정의 알림 화면을 열지 묻습니다. `--yes`이거나 입력이 닫힌 실행에서는 열지 않고 안내만 합니다. 시스템 설정 > 알림 > kairos에서 '알림 허용'을 켜세요.

나중에 권한을 다시 받으려면 `kairos notify setup`을, 알림이 오는지 확인하려면 `kairos notify test`를 쓰세요. 현재 상태는 `kairos doctor`의 `Mac 알림` 줄에 나옵니다.

### Orca 패널

setup은 물어본 뒤 `orca plugin dev load <설치본>/plugins/orca-panel`을 호출합니다. Orca가 승인을 요구하면 그 권한 문구를 그대로 보여 주고, `y`라고 답할 때만(또는 `--yes`일 때) Orca가 준 지문(`ap1-…`)으로 `--approve`를 호출합니다. 승인 질문의 기본값은 '아니오'입니다.

버전이 바뀌면 설치본 폴더 경로가 바뀌므로 다시 승인해야 합니다.

### 세션

setup은 세션을 켜지 않습니다. [차단 테스트](sessions.md#세션-켜기)를 확인한 뒤 `kairos sessions on`을 직접 실행하세요.

## 조직 프로필

조직 프로필(`org.toml`)에는 회사 도메인, 그룹 캘린더 ID, 저장소 이름이 들어 있습니다. 그래서 저장소에 넣지 않고, 관리자가 SHA256 지문과 함께 따로 나눠 줍니다.

- 받은 파일은 `profiles/`에 두거나(git이 무시합니다) `--org <파일> --sha256 <값>`으로 넘기세요.
- 형식은 합성 값으로 만든 [`examples/org.toml`](../examples/org.toml)을 참고하세요.
- 조직 프로필 없이도 쓸 수 있습니다. 다만 조직 값이 필요한 판정(신뢰 도메인, 배포 캘린더, 플레이북)은 꺼집니다.

## 서비스 켜고 끄기

```sh
kairos sources [--json]                # Calendar, GitHub, Slack, Gmail, Notion의 상태
kairos sources off slack notion        # 끕니다
kairos sources on slack                # 켭니다
```

목록에는 서비스마다 상태(켬, 끔, 설정 필요)와 부족한 값, 그 값을 채우는 명령이 나옵니다.

- 끈 서비스는 `connections.toml`의 `[sources]`에 기록되고, 다음 틱부터 수집하지 않습니다.
- 서비스 목록은 세션 지문에 들어가지 않습니다. 서비스를 꺼도 세션은 잠기지 않습니다.
- `config.toml`의 `[sources] disabled`에 직접 적은 서비스는 `sources on`으로 켜지지 않습니다.

## 단계별로 설정하기

setup을 쓰지 않고 단계를 하나씩 실행할 수도 있습니다. setup도 내부에서 같은 경로를 탑니다.

```sh
kairos config init --org examples/org.toml       # 조직 프로필을 가져오고 config.toml 틀을 만듭니다(예시는 합성 값)
kairos connect github                            # gh 로그인 사용자를 본인으로 기록합니다
kairos connect google <주소>                     # 개인 캘린더 ID이자 메일 주소
kairos connect slack <사용자 ID>                  # Slack 프로필의 멤버 ID
kairos connect name <이름>                       # 연차 일정과 Notion 알림에 쓰는 이름
kairos connect session-token < 토큰파일           # 세션용 읽기 전용 토큰을 키체인에 저장합니다
kairos config validate && kairos config show --origin
kairos install --apply && kairos install --register
```

`install --register`는 수집 작업을 등록하고 바로 한 번 실행합니다(`launchctl kickstart`). 표시 모드는 바꾸지 않습니다. 표시는 setup의 설치 단계가 켭니다.

## 설정 파일

설정은 데이터 폴더의 파일 세 개로 이루어집니다. 아래 표에서 뒤에 있는 파일이 앞의 값을 덮어씁니다. 예외로 본인 식별자는 `config.toml`의 `[identity]`에 적으면 `connections.toml`의 값을 덮습니다. [SECURITY.md](../SECURITY.md)의 보안 기준은 어느 파일로도 바꿀 수 없습니다.

| 파일 | 쓰는 주체 | 내용 |
|---|---|---|
| `org.toml` | 조직 관리자가 배포(`profiles/`) | GitHub 조직, 신뢰 도메인, 그룹 캘린더, 배포 태그, Notion 워크스페이스, 플레이북 대상 |
| `config.toml` | 사용자 | 시간대(IANA), 근무 요일, 근무 시간, 점심, 묶음 시각, 수집 시간, 고정으로 끈 소스, 호스트, 세션 호스트, MCP로 열 동작, Orca 저장소 ID, git 클론 경로 |
| `connections.toml` | `kairos connect`, `sources`, `setup`만 | 본인 식별자, MCP 바인딩(서버와 도구 이름), 끈 소스 |

- 설정 파일은 본인 소유이고 그룹이나 다른 사용자가 쓸 수 없을 때만 읽습니다.
- 설정이 깨져 있으면 `config`, `connect`, `sources`, `setup`, `uninstall`, `update`, `completions`, `help`를 뺀 나머지 명령은 멈춥니다.
- 세션 자격에 영향을 주는 값이 바뀌면 세션이 다시 잠기고 `doctor`가 알려 줍니다(`session-approval`).
- MCP 바인딩을 바꿨다면 `kairos install --apply`를 다시 실행하세요. 그래야 수집 훅의 허용 목록(`hooks/allowed.ts`)에 반영됩니다.

값을 바꾼 뒤에는 `kairos config validate`로 확인하고, `kairos config show --origin`으로 각 값이 어느 파일에서 왔는지 볼 수 있습니다.

### config.toml

`kairos config init`이 만드는 틀을 줄여 옮긴 것입니다. 주석은 우리말로 다듬었고, 저장소 이름은 합성 값으로 바꿨습니다. 실제 틀은 `crates/kairos-cli/src/commands/config.rs`의 `USER_TEMPLATE`입니다. 모두 주석이라 그대로 두면 기본값으로 동작합니다.

```toml
schema = 1

# 본인 식별자는 `kairos connect`가 connections.toml에 씁니다. 여기 쓰면 그 값을 덮습니다.
# [identity]
# name = "홍길동"

[schedule]
# timezone = "Asia/Seoul"
# days = ["mon", "tue", "wed", "thu", "fri"]
# work = ["10:00", "19:30"]
# lunch = ["12:00", "13:30"]
# bundles = ["13:30", "18:00"]
# collect = ["09:00", "20:00"]

[sources]
# disabled = ["notion"]

[hosts]
# 켠 호스트. 순서가 알림을 눌렀을 때 패널을 여는 순서입니다.
# enabled = ["orca", "claude-code", "vscode"]

# Orca 저장소 ID는 컴퓨터마다 다릅니다. `orca repo list --json`의 id를 적습니다.
# [hosts.orca.repos]
# "contoso/atlas" = "00000000-0000-0000-0000-000000000000"

[mcp]
# `kairos mcp serve`가 에이전트에 여는 동작. 기본은 읽기만입니다.
# allow_actions = ["later", "done", "keep", "add", "accept"]
```

## 근무 일정과 방해 금지

`[schedule]`로 카드와 알림을 받을 시간을 정합니다. 근무 요일(`days`)과 근무 시간(`work`) 밖에는 카드와 Mac 알림을 내지 않습니다.

| 키 | 기본값 | 뜻 |
|---|---|---|
| `timezone` | `Asia/Seoul` | 시간대(IANA 이름) |
| `days` | 월~금 | 근무 요일 |
| `work` | 10:00~19:30 | 카드와 알림을 내는 시간 |
| `lunch` | 12:00~13:30 | 카드와 P1 알림을 보류하는 점심시간 |
| `bundles` | 13:30, 18:00 | P2 일을 모아 보여 주는 [묶음](concepts.md#묶음) 시각 |
| `collect` | 09:00~20:00 | 수집 틱이 도는 시간 |

주말에도 하루 종일 받으려면 다음처럼 쓰세요.

```toml
[schedule]
days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
work = ["00:00", "24:00"]
collect = ["00:00", "24:00"]
lunch = []
```

- `lunch = []`는 점심 보류를 끕니다.
- `collect`는 수집이 도는 시간이므로 `work`를 포함해야 합니다.
- 감시자도 수집이 쓰는 `watch-window.tsv`를 읽어 같은 요일과 시간에만 판정합니다.
- 공휴일과 연차는 근무 요일이어도 쉬는 날로 처리합니다.

## 세션 호스트

[세션 호스트](concepts.md#세션-호스트)는 `config.toml`의 `[sessions] host`로 고릅니다.

| 값 | 동작 |
|---|---|
| `orca`(기본값) | Orca worktree와 터미널에서 세션을 띄웁니다 |
| `git` | `[hosts.git.repos]`에 지정한 로컬 클론에 `kairos/<이름>` 브랜치로 worktree를 만들고, 세션을 띄울 명령을 돌려줍니다 |

`git`일 때 VS Code 확장은 받은 명령을 터미널에서 실행하고, Claude Code는 명령을 보여 줍니다.

## MCP로 열 동작

`kairos mcp serve`는 기본으로 읽기 도구(`kairos_next`, `kairos_list`)만 엽니다. 상태를 바꾸는 동작은 `[mcp] allow_actions`에 적은 것(`later`, `done`, `keep`, `add`, `accept`)만 추가됩니다. `accept`는 `[hosts] enabled`에 `claude-code`도 있어야 나옵니다. 자세한 내용은 [호스트 계약](surface-protocol.md#mcp)에 있습니다.
