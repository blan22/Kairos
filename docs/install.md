# 설치와 설정

## 설치

저장소를 받아 스크립트 하나를 돌린다. 업그레이드도 같은 명령이다. 옛 설치를 먼저 지우지 않는다.

```
git clone https://github.com/blan22/Kairos.git kairos && cd kairos
scripts/install.sh [--org <파일> [--sha256 <값>]]
```

미리 있어야 하는 것은 Apple Silicon Mac, Xcode Command Line Tools(`xcode-select --install`), Rust(rustup, 판은 `rust-toolchain.toml`이 받는다), 로그인한 gh(`gh auth login`), Claude Code다. Orca는 패널을 쓸 때만 있으면 된다. 빠진 것이 있으면 스크립트가 설치 명령을 알려 주고 멈춘다.

스크립트는 kairos, kairos-run, 알림 헬퍼를 빌드하고 `~/.local/bin/kairos`를 설치본(`current/bin/kairos`)에 건 뒤 `kairos setup`으로 넘긴다. 알림 헬퍼는 키체인의 Apple Development 인증서로 서명하고, 없으면 ad-hoc으로 서명한다. ad-hoc이면 다시 빌드할 때마다 알림 권한을 다시 물을 수 있다. `~/.local/bin`이 PATH에 없으면 더할 줄을 알려 준다.

묻지 않고 끝까지 가려면 `--yes`와 본인 식별자를 넘긴다. 사람이 할 일은 macOS 알림 권한 창의 허용 한 번이다.

```
scripts/install.sh --yes --google <회사 메일> --slack <Slack 멤버 ID> [--name <연차 일정의 이름>]
```

`--yes`는 소스를 지금 값(처음이면 기본값)으로 두고, 설치, 알림 권한, Orca 패널 적재와 승인을 예로 답한다. 패널 승인 전 Orca의 권한 문구는 그대로 보여 준다. 조직 프로필은 `--org`가 없으면 실행한 kairos가 든 저장소의 `profiles/`에 하나뿐일 때만 들이고 SHA256을 찍는다. 지금 폴더의 `profiles/`는 묻지 않고 들이지 않는다. 넘기지 않은 식별자는 비워 두고 채우는 명령을 알려 준다. 세션은 켜지 않는다.

조직 프로필(`org.toml`)은 저장소에 넣지 않는다. 회사 도메인, 그룹 캘린더 ID, 저장소 이름이 들어 있기 때문이다. 관리자가 따로 나눠 주고 SHA256 지문을 함께 알려 준다. 받은 파일은 `profiles/`에 두거나(git이 무시한다) `--org <파일> --sha256 <값>`으로 넘긴다. `examples/org.toml`은 모양을 보여 주는 합성 예시다. 조직 없이 쓰면 조직 값이 필요한 판정(신뢰 도메인, 배포 캘린더, 플레이북)은 꺼진다.

## 설정과 연동

`kairos setup`은 조직 프로필, 켤 서비스, 계정 연결, 설치, 알림과 Orca 패널을 차례로 묻는다. 조직 프로필은 SHA256을 보여 주고 `y`일 때만 들인다(`--yes`면 묻지 않고 들인다). 관리자가 지문을 알려 줬으면 `--sha256`으로 고정한다. 다시 실행해도 되고 Enter는 지금 값을 그대로 둔다. 설치는 `y`라고 답할 때만 한다.

```
kairos setup [--org <파일> [--sha256 <값>]] [--yes] [--no-install] [--google <주소>] [--slack <ID>] [--name <이름>]
```

`--google`, `--slack`, `--name`을 주면 그 값은 묻지 않고 기록한다. `--no-install`은 설치를 건너뛴다(설정만 바꿀 때).

마지막 단계는 설치본(`<데이터 폴더>/current`)이 있을 때만 돈다. 개발 트리의 번들과 패널 폴더는 권한과 승인을 따로 받기 때문에 설치본을 쓴다.

- Mac 알림: 헬퍼가 이미 권한을 받았으면(`status`가 `authorized`) 묻지 않는다. 아니면 묻고, 번들을 LaunchServices에 등록(`lsregister -f`)한 뒤 `KairosNotifier setup`으로 권한 창을 띄운다.
- Orca 패널: 묻고 `orca plugin dev load <설치본>/plugins/orca-panel`을 부른다. Orca가 승인을 받으면 그 권한 문구를 그대로 보여 주고, `y`일 때만(또는 `--yes`) Orca가 준 지문(`ap1-…`)으로 `--approve`를 부른다. 묻는 승인의 기본은 아니오다. 판이 바뀌면 폴더 경로가 바뀌어 다시 승인한다.
- 세션은 여기서 켜지 않는다. 차단 시험을 본 뒤 `kairos sessions on`을 직접 한다.

서비스는 `kairos sources`로 켜고 끈다. 목록은 서비스마다 상태(켬, 끔, 설정 필요)와 모자란 값, 채우는 명령을 보여 준다. 끈 서비스는 `connections.toml`의 `[sources]`에 남고 다음 틱부터 수집하지 않는다. 세션 지문에 들지 않아 세션은 잠기지 않는다. `config.toml`의 `[sources] disabled`에 손으로 적은 서비스는 `sources on`으로 켜지지 않는다.

```
kairos sources [--json]                # Calendar, GitHub, Slack, Gmail, Notion의 상태
kairos sources off slack notion        # 끈다
kairos sources on slack                # 켠다
```

단계를 따로 하려면 아래 명령을 쓴다. `setup`도 같은 길로 쓴다.

```
kairos config init --org examples/org.toml       # 조직 프로필을 들이고 config.toml 틀을 만든다(예시는 합성 값)
kairos connect github                            # gh 로그인 사용자를 본인으로 기록
kairos connect google <주소>                     # 개인 캘린더 ID이자 메일 주소
kairos connect slack <사용자 ID>                  # Slack 프로필의 멤버 ID
kairos connect name <이름>                       # 연차 일정과 Notion 알림의 이름
kairos connect session-token < 토큰파일           # 세션용 읽기 전용 토큰을 키체인에
kairos config validate && kairos config show --origin
kairos install --apply && kairos install --register
```

설정은 데이터 폴더의 세 파일이다. 뒤 층이 앞 층을 덮고, 보안 바닥값(`SECURITY.md`)은 어느 층에도 칸이 없다.

| 파일 | 쓰는 사람 | 내용 |
|---|---|---|
| `org.toml` | 조직 관리자가 배포(`profiles/`) | GitHub 조직, 신뢰 도메인, 그룹 캘린더, 배포 태그, Notion 워크스페이스, 플레이북 대상 |
| `config.toml` | 사용자 | 시간대(IANA), 근무 요일, 근무 시간, 점심, 묶음 시각, 수집 시간, 고정으로 끈 소스, 호스트, 세션 호스트, MCP로 열 동작, Orca 저장소 ID, git 클론 경로 |
| `connections.toml` | `kairos connect`, `sources`, `setup`만 | 본인 식별자, MCP 바인딩(서버와 도구 이름), 끈 소스 |

방해 금지 시간은 `[schedule]`로 정한다. 근무 요일(`days`) 밖과 근무 시간(`work`) 밖에는 카드와 Mac 알림을 내지 않는다. 기본은 월~금 10:00~19:30이다. 주말에도 하루 종일 받으려면 이렇게 쓴다.

```toml
[schedule]
days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
work = ["00:00", "24:00"]
collect = ["00:00", "24:00"]
lunch = []
```

`lunch = []`는 점심 보류를 끈다. `collect`는 수집이 도는 시간이라 `work`를 덮어야 한다. 감시자도 수집이 쓰는 `watch-window.tsv`로 같은 요일과 시간에만 판정한다. 공휴일과 연차는 근무 요일이어도 쉬는 날이다.

설정 파일은 본인 소유이고 그룹이나 남이 쓸 수 없어야 읽는다. 깨진 설정이면 `config`, `connect`, `sources`, `setup`, `help`를 뺀 명령이 멈춘다. 세션 자격에 닿는 값이 바뀌면 세션이 다시 잠기고 `doctor`가 알린다(`session-approval`). MCP 바인딩을 바꾸면 `kairos install --apply`를 다시 해야 수집 훅의 허용 목록(`hooks/allowed.ts`)에 반영된다.

세션 호스트는 `orca`(기본)와 `git`이다. `git`이면 `[hosts.git.repos]`의 로컬 클론에 `kairos/<이름>` 브랜치로 worktree를 만들고 띄울 명령을 돌려준다. VS Code 확장은 그 명령을 터미널로 띄우고, Claude Code는 명령을 보여 준다.

0.2에서 올리면 launchd 라벨이 `dev.kairos.*`로 바뀐다. `install --register`가 옛 라벨(`com.<이름>.kairos.collect`, `com.<이름>.kairos.watchdog`)을 내리고 지운다. 이름이 같아도 링크이거나 내용에 그 라벨과 `KAIROS_HOME` 키가 없는 plist는 남의 잡으로 보고 건드리지 않는다. 알림 헬퍼의 번들 ID도 `dev.kairos.notifier`로 바뀌어 다시 빌드하고 `KairosNotifier setup`으로 권한을 받는다. 세션은 설정 지문이 없어 잠기므로 차단 시험 뒤 `kairos sessions on`을 다시 한다.
