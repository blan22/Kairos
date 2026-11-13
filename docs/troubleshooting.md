# 문제 해결

막혔다면 먼저 `kairos doctor`를 실행해 보세요. 확인할 항목을 마지막에 모아 보여 주고, 없으면 `문제 없음`이 나옵니다. 명령이 기억나지 않으면 `kairos help`를, 특정 명령의 사용법은 `kairos <명령> --help`를 보세요.

- [설치](#설치)
- [알림이 오지 않습니다](#알림이-오지-않습니다)
- [카드나 항목이 보이지 않습니다](#카드나-항목이-보이지-않습니다)
- [설정](#설정)
- [세션](#세션)
- [Orca 패널](#orca-패널)
- [macOS 권한 창이 계속 뜹니다](#macos-권한-창이-계속-뜹니다)
- [지우고 다시 시작하기](#지우고-다시-시작하기)
- [버그 제보](#버그-제보)

## 설치

| 메시지나 증상 | 해결 방법 |
|---|---|
| `멈춤: 아래 N가지를 갖춘 뒤 다시 실행하세요` | 목록의 도구를 설치하고 같은 명령을 다시 실행하세요. Rust를 방금 설치했다면 새 터미널을 여세요 |
| `kairos: command not found` | 새 터미널을 여세요. 그래도 없다면 `~/.local/bin`이 PATH에 없는 것입니다. `~/.zshrc`에 `export PATH="$HOME/.local/bin:$PATH"`를 추가하세요 |
| `~/.local/bin/kairos가 링크가 아닌 파일이라 그대로 둡니다` | 그 위치에 다른 파일이 있습니다. 옮기거나 지운 뒤 설치 스크립트를 다시 실행하세요 |
| `SHA256이 다릅니다` | 받은 패키지가 손상됐거나 바뀌었습니다. 다시 실행하고, 계속 반복되면 저장소에서 빌드하세요 |
| `묶음을 받지 못했습니다` | 네트워크를 확인하세요. 릴리스가 없는 버전이라면 저장소에서 빌드하세요 |
| `--update`가 `커밋하지 않은 변경`으로 멈춤 | 저장소의 변경을 커밋하거나 치운 뒤 다시 실행하세요 |
| `데이터 폴더(…)가 git 작업 트리(…) 안에 있습니다`(`--json`은 `home_in_git_worktree`) | 데이터 폴더(`KAIROS_HOME`)를 git 작업 트리 밖으로 옮기세요 |
| 릴리스 설치 뒤 `설치하지 않아 kairos 명령이 없습니다` | 설치 명령을 다시 실행하고, setup이 설치할지 물을 때 `y`로 답하세요. 받은 패키지는 임시 폴더에 있어서 남지 않습니다 |

현재 버전과 업데이트 명령은 `kairos update`가 알려 줍니다.

## 알림이 오지 않습니다

다음 순서로 확인하세요.

1. `kairos doctor`의 `Mac 알림` 줄을 봅니다.
2. 권한을 아직 받지 않았다면 `kairos notify setup`을 실행하고, 화면 오른쪽 위의 권한 창에서 허용을 누릅니다.
3. 전에 거절했다면 macOS가 권한 창을 다시 띄우지 않습니다. `kairos notify setup`이 열어 주는 시스템 설정 > 알림 > kairos에서 '알림 허용'을 켭니다.
4. `kairos notify test`로 테스트 알림을 보냅니다.

권한이 있는데도 오지 않는다면 다음을 확인하세요.

- `kairos notify`가 `off`라면 `kairos notify on`으로 켜세요.
- 근무 요일과 근무 시간 밖에는 알림을 내지 않습니다(`config.toml`의 `[schedule]`).
- 표시 모드가 auto라면 첫 수집 뒤 첫 근무일에는 기록만 하고 카드와 알림을 내지 않습니다. `kairos setup`으로 설치하면 표시를 켜 두지만, `kairos install --register`만 실행했거나 `kairos display auto`를 골랐다면 auto입니다. `kairos display`로 현재 상태를 보고 `kairos display on`으로 여세요.
- 설치 직후의 첫 수집은 알림 권한을 허용하기 전에 돌 수 있습니다. 그때 알리지 못한 일은 권한을 허용한 뒤의 틱이 다시 알립니다.
- `kairos pause` 중에는 알림을 내지 않고, `kairos focus` 중에는 쌓아 둡니다.
- macOS 집중 모드가 켜져 있으면 알림 센터에만 쌓입니다.

버전을 올린 뒤 권한을 다시 묻는 것은 알림 헬퍼가 ad-hoc으로 서명되어 있기 때문입니다. 다시 허용하면 됩니다.

## 카드나 항목이 보이지 않습니다

- 표시가 닫혀 있는 동안(auto의 첫 근무일, `kairos display off`)에는 기록만 합니다. `kairos list --would-show`는 표시됐을 항목을, `kairos list --items`는 열린 항목을 보여 줍니다.
- 상태 줄에 `수집 기록 없음`이 나오면 수집이 아직 돌지 않은 것입니다. `kairos doctor`의 소스 줄과 launchd 작업을 확인하세요. 작업이 없으면 `kairos setup`을 다시 실행해 설치하세요.
- 서비스가 `설정 필요` 상태라면 `kairos sources`가 부족한 값과 채우는 명령을 알려 줍니다.
- Mac이 잠든 동안의 틱은 고장이 아닙니다(`sleep_interrupted`).

## 설정

- `kairos config validate`가 잘못된 항목을 알려 줍니다. `kairos config show --origin`은 각 값이 어느 파일에서 왔는지 보여 줍니다.
- 설정 파일은 본인 소유이고 그룹이나 다른 사용자가 쓸 수 없을 때만 읽습니다. 권한이 넓으면 `chmod 600`으로 좁히세요.
- 설정이 깨져 있어도 `config`, `connect`, `sources`, `setup`, `uninstall`, `update`, `completions`, `help`는 동작합니다. 이 명령으로 고치거나, 지우고 다시 시작하세요.

## 세션

- `doctor`에 `session-approval`이 나오면 세션 자격에 영향을 주는 설정이 바뀌어 세션이 잠긴 것입니다. 차단 테스트를 확인한 뒤 `kairos sessions on`을 다시 실행하세요([에이전트 세션](sessions.md#세션-켜기)).
- `세션 열고 시작`이 `repo_unresolved`로 실패하면 kairos가 Orca에서 작업할 저장소를 하나로 고르지 못한 것입니다. 같은 원격의 클론을 Orca에 여러 개 등록했고 폴더 이름도 저장소 이름과 다를 때 생깁니다. `orca repo list --json`에서 쓸 저장소의 `id`를 골라 `config.toml`의 `[hosts.orca.repos]`에 적으세요([설정](configuration.md)).
- 세션 토큰이 없다고 나오면 읽기 전용 fine-grained 토큰을 만들어 `kairos connect session-token < 토큰파일`로 등록하세요([세션 토큰](sessions.md#세션-토큰)).

## Orca 패널

**버전을 올린 뒤 다시 승인을 요구합니다.** 설치본 폴더가 바뀌었기 때문입니다. `kairos setup`을 다시 실행하면 마지막 단계에서 로드와 승인을 묻습니다.

**패널이 `errored`이고 `orca plugin logs local.kairos`에 `content hash mismatch`가 있습니다.** 패널 폴더에 적재 과정이 쓴 타입 파일(`.orca`)이 남아 있는 것입니다. Orca는 승인 해시를 잴 때는 이 폴더를 빼고, 워커를 띄울 때는 넣고 잽니다. 0.4.1까지의 setup이나 손으로 실행한 `orca plugin dev load`가 이 파일을 남깁니다. 다시 적재하거나 읽어도 풀리지 않으니, 그 버전의 `plugins/orca-panel/.orca`를 지우고 패널을 다시 시작하세요.

```sh
rm -rf "$HOME/Library/Application Support/kairos/current/plugins/orca-panel/.orca"
orca plugin restart local.kairos
```

**새 일 점이 사라지지 않습니다.** 그 칩을 고르고 2초 넘게 본 뒤 다른 칩으로 옮기세요. 잠깐 지나친 화면은 본 것으로 기록하지 않습니다. 패널 권한에 플러그인 저장소가 빠져 있으면 점이 아예 없습니다. 이때는 `kairos setup`으로 다시 승인하세요.

**패널이 비어 있습니다.** 터미널에서 `kairos next`가 같은 내용을 출력하는지 확인하세요. 터미널에서도 비어 있다면 [카드나 항목이 보이지 않습니다](#카드나-항목이-보이지-않습니다)를 보세요.

## macOS 권한 창이 계속 뜹니다

`‘kairos’이(가) 데스크탑 폴더의 파일에 접근하려고 합니다`가 3분마다 뜬다면 kairos를 업데이트하세요. 수집 세션의 claude가 스킬 링크를 따라 보호 폴더를 읽던 것이 원인이었고, 지금은 스킬과 명령을 읽지 않습니다. 이미 눌러 둔 허용이나 거부는 그대로 두어도 됩니다.

## 지우고 다시 시작하기

```sh
kairos uninstall --dry-run --purge   # 지울 대상만 확인합니다
kairos uninstall --runtime           # 런타임만 지웁니다. 수집 기록과 설정은 남습니다
kairos uninstall --purge             # 전부 지웁니다
```

## 버그 제보

[이슈](https://github.com/blan22/Kairos/issues/new/choose)에 `kairos --version`, `kairos doctor`의 출력, 재현 순서를 적어 주세요. 출력에 있는 메일 주소, 조직과 저장소 이름, 경로의 사용자 이름은 가려 주세요. 보안 문제는 공개 이슈에 쓰지 말고 [SECURITY.md](../SECURITY.md)의 방법으로 알려 주세요.
