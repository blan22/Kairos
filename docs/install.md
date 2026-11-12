# 설치와 설정

## 설치

설치 방법은 두 가지다. 둘 다 처음 설치와 업그레이드에 같은 명령을 쓰고, 기존 설치를 먼저 지울 필요가 없다. 설정과 데이터는 그대로 유지된다.

### 저장소에서 빌드

```
git clone https://github.com/blan22/Kairos.git kairos && cd kairos
scripts/install.sh [--org <파일> [--sha256 <값>]]
```

미리 준비할 것은 Apple Silicon Mac, Xcode Command Line Tools(`xcode-select --install`), Rust(rustup. 버전은 `rust-toolchain.toml`이 정한다), 로그인한 gh(`gh auth login`), Claude Code다. Orca는 패널을 쓸 때만 필요하다. 빠진 것이 있으면 스크립트가 전부 모아 설치 명령과 함께 알려 주고 멈춘다. 사용법은 `scripts/install.sh --help`로 본다.

스크립트는 kairos, kairos-run, 알림 헬퍼를 빌드하고 `~/.local/bin/kairos`를 설치본(`current/bin/kairos`)에 링크한 뒤 `kairos setup`으로 넘어간다. 알림 헬퍼는 키체인의 Apple Development 인증서로 서명하고, 인증서가 없으면 ad-hoc으로 서명한다. ad-hoc 서명이면 다시 빌드할 때마다 macOS가 알림 권한을 다시 물을 수 있다.

### 릴리스 패키지(빌드 없음)

```
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.sh | zsh
```

Rust와 Xcode가 없어도 된다. gh와 Claude Code는 필요하다.

스크립트는 GitHub의 최신 릴리스에서 패키지(`kairos-aarch64-apple-darwin.tar.gz`)와 SHA256 파일을 받고, 값이 일치할 때만 압축을 풀어 패키지 안의 `scripts/install.sh`로 넘어간다. 받은 파일은 임시 폴더에 두었다가 끝나면 지운다. 따라서 setup의 설치 질문에 `y`로 답해야 `kairos` 명령이 남는다. 설치하지 않고 끝내면 스크립트가 그 사실을 알려 준다.

- 옵션은 `zsh -s -- --yes --google <회사 메일>`처럼 뒤에 붙인다.
- 실행 전에 내용을 확인하려면 스크립트를 파일로 받아 `zsh install-release.sh`로 실행한다.
- SHA256 파일은 패키지와 같은 곳에서 받는다. 전송 중에 손상된 파일은 걸러 내지만 올린 사람을 증명하지는 않는다. 패키지를 신뢰할 수 없으면 저장소에서 빌드한다.
- 패키지의 알림 헬퍼는 ad-hoc으로 서명되어 있다. 버전을 올린 뒤 macOS가 알림 권한을 다시 물을 수 있다.
- 패키지는 태그(`v<버전>`)를 올리면 `.github/workflows/release.yml`이 `scripts/package.sh`로 만든다. 로컬에서도 같은 명령으로 `target/dist/`에 만들 수 있다.
- `KAIROS_RELEASE_URL`을 지정하면 설치 스크립트가 그 주소에서 받는다(사내 미러용).

### kairos 명령과 PATH

`~/.local/bin`이 PATH에 없으면 스크립트가 `~/.zshrc`에 `export PATH="$HOME/.local/bin:$PATH"` 한 줄을 추가할지 묻는다.

- `--yes`면 묻지 않고 추가한다.
- 이미 `.local/bin`이 적혀 있으면 다시 쓰지 않는다.
- 터미널이 아니거나 zsh가 아니면 추가할 줄만 알려 준다.
- `--no-modify-path`를 주면 셸 설정을 건드리지 않는다.

새 터미널을 열면 `kairos` 명령을 쓸 수 있다.

자동 완성을 쓰려면 `~/.zshrc`의 `compinit` 뒤에 `source <(kairos completions zsh)`를 추가한다. bash는 `~/.bashrc`에 `eval "$(kairos completions bash)"`를 추가한다.

### 업데이트

`kairos update`가 현재 버전과 설치 방식에 맞는 명령을 알려 준다. kairos는 스스로 코드를 내려받지 않는다.

```
kairos update
<저장소>/scripts/install.sh --update     # 저장소에서 빌드했을 때. pull(--ff-only) 후 다시 설치한다
```

저장소에서 빌드한 경우:

- `--update`는 저장소에 커밋하지 않은 변경이 있으면 멈춘다.
- pull한 뒤에는 처음 설치할 때와 같은 질문을 한다. Enter를 누르면 현재 값을 유지하고, 설치와 Orca 패널 승인은 `y`로 답해야 진행한다.
- 질문 없이 진행하려면 `--yes`를 함께 준다. `--yes`는 설치와 패널 승인까지 자동으로 동의하므로 `--update`가 알아서 붙이지는 않는다.
- `~/.zshrc`는 건드리지 않는다.

릴리스 패키지로 설치한 경우:

- 설치 명령 한 줄을 다시 실행한다.
- 미러(`KAIROS_RELEASE_URL`)로 설치했다면 같은 주소를 지정하고 다시 실행한다. `kairos update`가 알려 주는 명령은 공개 주소 기준이다.

### 제거

```
kairos uninstall --dry-run --purge   # 지울 대상만 확인한다
kairos uninstall                     # launchd 작업만 해제한다
kairos uninstall --runtime           # + 런타임과 ~/.local/bin/kairos. 수집 기록과 설정은 남긴다
kairos uninstall --purge             # + 데이터 폴더 전체와 키체인의 세션 토큰
```

`--runtime`과 `--purge`는 지울 대상을 보여 주고 `y`라고 답할 때만 지운다. Orca 패널은 언로드하는 명령(`orca plugin dev unload <경로>`)을 알려 준다. `~/.zshrc`에 추가한 PATH 줄과 저장소 폴더는 직접 지운다.

### 질문 없이 설치

질문 없이 끝까지 진행하려면 `--yes`와 본인 식별자를 함께 넘긴다. 사람이 할 일은 macOS 알림 권한 창에서 허용을 한 번 누르는 것뿐이다.

```
scripts/install.sh --yes --google <회사 메일> --slack <Slack 멤버 ID> [--name <연차 일정의 이름>]
```

- `--yes`는 소스를 현재 값(처음이면 기본값)으로 두고, 설치와 Orca 패널 로드·승인에 자동으로 동의한다. 패널을 승인하기 전에 Orca의 권한 문구는 그대로 보여 준다.
- 알림 권한은 `--yes`가 없어도 터미널에서 묻지 않는다.
- `--org`가 없으면, 실행한 kairos가 들어 있는 저장소의 `profiles/`에 프로필이 하나뿐일 때만 그 프로필을 가져오고 SHA256을 출력한다. 현재 폴더의 `profiles/`는 묻지 않고 가져오지 않는다.
- 넘기지 않은 식별자는 비워 두고, 채우는 명령을 알려 준다.
- 세션은 켜지 않는다.

### 조직 프로필

조직 프로필(`org.toml`)은 저장소에 넣지 않는다. 회사 도메인, 그룹 캘린더 ID, 저장소 이름이 들어 있기 때문이다. 관리자가 따로 배포하고 SHA256 지문을 함께 알려 준다.

- 받은 파일은 `profiles/`에 두거나(git이 무시한다) `--org <파일> --sha256 <값>`으로 넘긴다.
- `examples/org.toml`은 형식을 보여 주는 합성 예시다.
- 조직 프로필 없이 쓰면 조직 값이 필요한 판정(신뢰 도메인, 배포 캘린더, 플레이북)은 꺼진다.

## 설정과 연동

### setup

`kairos setup`은 조직 프로필, 켤 서비스, 계정 연결, 설치, Orca 패널을 차례로 묻고, 알림 권한은 macOS 권한 창으로 받는다.

```
kairos setup [--org <파일> [--sha256 <값>]] [--yes] [--no-install] [--google <주소>] [--slack <ID>] [--name <이름>]
```

- 조직 프로필은 SHA256을 보여 주고 `y`라고 답할 때만 가져온다(`--yes`면 묻지 않고 가져온다). 관리자가 지문을 알려 줬다면 `--sha256`으로 고정한다.
- 여러 번 실행해도 된다. Enter를 누르면 현재 값을 그대로 둔다.
- 설치는 `y`라고 답할 때만 한다. `--no-install`은 설치를 건너뛴다(설정만 바꿀 때 쓴다).
- `--google`, `--slack`, `--name`으로 넘긴 값은 묻지 않고 기록한다.

마지막 단계(알림 권한, Orca 패널)는 설치본(`<데이터 폴더>/current`)이 있을 때만 진행한다. 개발 트리의 번들과 패널 폴더는 권한과 승인을 따로 받기 때문에 설치본을 기준으로 한다.

**Mac 알림**

- 다른 Mac 앱과 같다. 터미널에서 묻지 않고 macOS 권한 창이 묻는다. 허용하면 확인 알림이 하나 오고 그때부터 알림이 온다.
- 헬퍼가 이미 권한을 받았으면(`status`가 `authorized`) 아무것도 띄우지 않는다. 아직 물은 적이 없으면 번들을 LaunchServices에 등록(`lsregister -f`)한 뒤 `KairosNotifier setup`으로 권한 창을 띄운다. 창은 화면 오른쪽 위에 뜨고 2분 동안 기다린다.
- 전에 거절했다면 macOS가 권한 창을 다시 띄우지 않는다. 이때는 시스템 설정의 알림 화면을 열지 묻는다(`--yes`이거나 입력이 닫힌 실행에서는 열지 않고 안내만 한다). 시스템 설정 > 알림 > kairos에서 '알림 허용'을 켜면 된다.
- 권한을 나중에 다시 받으려면 `kairos notify setup`, 알림이 오는지 확인하려면 `kairos notify test`를 쓴다. 현재 상태는 `kairos doctor`의 `Mac 알림` 줄에 나온다.

**Orca 패널**

- 물어본 뒤 `orca plugin dev load <설치본>/plugins/orca-panel`을 호출한다. Orca가 승인을 요구하면 그 권한 문구를 그대로 보여 주고, `y`라고 답할 때만(또는 `--yes`) Orca가 준 지문(`ap1-…`)으로 `--approve`를 호출한다. 승인 질문의 기본값은 '아니오'다.
- 버전이 바뀌면 폴더 경로가 바뀌므로 다시 승인해야 한다.

**세션**

- 세션은 setup에서 켜지 않는다. 차단 테스트를 확인한 뒤 `kairos sessions on`을 직접 실행한다.

### 서비스 켜고 끄기

서비스는 `kairos sources`로 켜고 끈다. 목록에는 서비스마다 상태(켬, 끔, 설정 필요), 부족한 값, 그 값을 채우는 명령이 나온다.

```
kairos sources [--json]                # Calendar, GitHub, Slack, Gmail, Notion의 상태
kairos sources off slack notion        # 끈다
kairos sources on slack                # 켠다
```

- 끈 서비스는 `connections.toml`의 `[sources]`에 기록되고 다음 틱부터 수집하지 않는다.
- 세션 지문에 포함되지 않으므로 서비스를 꺼도 세션은 잠기지 않는다.
- `config.toml`의 `[sources] disabled`에 직접 적은 서비스는 `sources on`으로 켜지지 않는다.

### 단계별 명령

단계를 따로 실행하려면 아래 명령을 쓴다. `setup`도 내부에서 같은 경로를 탄다.

```
kairos config init --org examples/org.toml       # 조직 프로필을 가져오고 config.toml 틀을 만든다(예시는 합성 값)
kairos connect github                            # gh 로그인 사용자를 본인으로 기록
kairos connect google <주소>                     # 개인 캘린더 ID이자 메일 주소
kairos connect slack <사용자 ID>                  # Slack 프로필의 멤버 ID
kairos connect name <이름>                       # 연차 일정과 Notion 알림에 쓰는 이름
kairos connect session-token < 토큰파일           # 세션용 읽기 전용 토큰을 키체인에 저장
kairos config validate && kairos config show --origin
kairos install --apply && kairos install --register
```

### 설정 파일

설정은 데이터 폴더의 파일 세 개로 이루어진다. 뒤 계층이 앞 계층을 덮어쓴다. 보안 기준(`SECURITY.md`)은 어느 계층에서도 바꿀 수 없다.

| 파일 | 쓰는 주체 | 내용 |
|---|---|---|
| `org.toml` | 조직 관리자가 배포(`profiles/`) | GitHub 조직, 신뢰 도메인, 그룹 캘린더, 배포 태그, Notion 워크스페이스, 플레이북 대상 |
| `config.toml` | 사용자 | 시간대(IANA), 근무 요일, 근무 시간, 점심, 묶음 시각, 수집 시간, 고정으로 끈 소스, 호스트, 세션 호스트, MCP로 열 동작, Orca 저장소 ID, git 클론 경로 |
| `connections.toml` | `kairos connect`, `sources`, `setup`만 | 본인 식별자, MCP 바인딩(서버와 도구 이름), 끈 소스 |

- 설정 파일은 본인 소유이고 그룹이나 다른 사용자가 쓸 수 없을 때만 읽는다.
- 설정이 깨져 있으면 `config`, `connect`, `sources`, `setup`, `uninstall`, `update`, `completions`, `help`를 제외한 명령은 멈춘다.
- 세션 자격에 영향을 주는 값이 바뀌면 세션이 다시 잠기고 `doctor`가 알려 준다(`session-approval`).
- MCP 바인딩을 바꾸면 `kairos install --apply`를 다시 실행해야 수집 훅의 허용 목록(`hooks/allowed.ts`)에 반영된다.

### 근무 일정과 방해 금지

방해 금지 시간은 `[schedule]`로 정한다. 근무 요일(`days`)과 근무 시간(`work`) 밖에는 카드와 Mac 알림을 내지 않는다. 기본값은 월~금 10:00~19:30이다. 주말에도 하루 종일 받으려면 다음과 같이 쓴다.

```toml
[schedule]
days = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
work = ["00:00", "24:00"]
collect = ["00:00", "24:00"]
lunch = []
```

- `lunch = []`는 점심 보류를 끈다.
- `collect`는 수집이 도는 시간이므로 `work`를 포함해야 한다.
- 감시자도 수집이 쓰는 `watch-window.tsv`를 읽어 같은 요일과 시간에만 판정한다.
- 공휴일과 연차는 근무 요일이어도 쉬는 날로 처리한다.

### 세션 호스트

세션 호스트는 `orca`(기본값)와 `git` 두 가지이고 `config.toml`의 `[sessions] host`로 고른다. `git`이면 `[hosts.git.repos]`에 지정한 로컬 클론에 `kairos/<이름>` 브랜치로 worktree를 만들고, 세션을 띄울 명령을 돌려준다. VS Code 확장은 그 명령을 터미널에서 실행하고, Claude Code는 명령을 보여 준다.

## 공개 전 버전에서 업그레이드

공개 릴리스 이전의 설치(launchd 라벨이 `com.<이름>.kairos.*`인 설치)에서 업그레이드하면 다음이 바뀐다.

- launchd 라벨이 `dev.kairos.*`로 바뀐다. `install --register`가 옛 라벨(`com.<이름>.kairos.collect`, `com.<이름>.kairos.watchdog`)을 해제하고 지운다. 이름이 같더라도 링크이거나, 내용에 그 라벨과 `KAIROS_HOME` 키가 없는 plist는 다른 프로그램의 작업으로 보고 건드리지 않는다.
- 알림 헬퍼의 번들 ID가 `dev.kairos.notifier`로 바뀐다. 헬퍼를 다시 빌드하고 `kairos notify setup`으로 권한을 다시 받는다.
- 설정 지문이 없어 세션이 잠긴다. 차단 테스트를 확인한 뒤 `kairos sessions on`을 다시 실행한다.
