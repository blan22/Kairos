# 문제 해결

먼저 `kairos doctor`를 본다. 끝에 확인할 것을 모아 보여 주고, 없으면 `문제 없음`이다. 명령이 기억나지 않으면 `kairos help`, 한 명령의 사용법은 `kairos <명령> --help`다.

## 설치

| 증상 | 할 일 |
|---|---|
| `멈춤: 아래 N가지를 갖춘 뒤 다시 실행하세요` | 목록의 도구를 설치하고 같은 명령을 다시 실행한다. Rust를 방금 설치했으면 새 터미널을 연다 |
| `kairos: command not found` | 새 터미널을 연다. 그래도 없으면 `~/.local/bin`이 PATH에 없다. `~/.zshrc`에 `export PATH="$HOME/.local/bin:$PATH"`를 더한다 |
| `~/.local/bin/kairos가 링크가 아닌 파일이라 그대로 둡니다` | 그 자리에 다른 파일이 있다. 옮기거나 지운 뒤 설치 스크립트를 다시 실행한다 |
| `SHA256이 다릅니다` | 받은 묶음이 깨졌거나 바뀌었다. 다시 실행하고, 되풀이되면 저장소에서 빌드한다 |
| `묶음을 받지 못했습니다` | 네트워크를 보고, 릴리스가 없는 판이면 저장소에서 빌드한다 |
| `--update`가 `커밋하지 않은 변경`으로 멈춘다 | 저장소의 변경을 커밋하거나 치운 뒤 다시 실행한다 |
| `데이터 폴더(…)가 git 작업 트리(…) 안에 있습니다`(`--json`은 `home_in_git_worktree`) | 데이터 폴더(`KAIROS_HOME`)를 git 작업 트리 밖으로 옮긴다 |
| 릴리스 설치 뒤 `설치하지 않아 kairos 명령이 없습니다` | 설치 명령을 다시 실행하고 setup의 설치 질문에 `y`로 답한다. 받은 묶음은 임시 폴더라 남지 않는다 |

지금 판과 업데이트하는 명령은 `kairos update`가 알려 준다.

## 알림이 오지 않는다

1. `kairos doctor`의 `Mac 알림` 줄을 본다.
2. 권한을 아직 받지 않았으면 `kairos notify setup`을 실행하고 화면 오른쪽 위의 권한 창에서 허용을 누른다.
3. 전에 거절했으면 macOS가 권한 창을 다시 띄우지 않는다. `kairos notify setup`이 여는 시스템 설정 > 알림 > kairos에서 '알림 허용'을 켠다.
4. `kairos notify test`로 시험 알림을 보낸다.

권한이 있는데도 오지 않으면 아래를 본다.

- `kairos notify`가 `off`면 `kairos notify on`으로 켠다.
- 근무 요일과 근무 시간 밖에는 알림을 내지 않는다(`config.toml`의 `[schedule]`).
- 처음 설치한 뒤 첫 근무일은 기록만 하고 카드와 알림을 내지 않는다. `kairos display`로 지금 상태를 보고 `kairos display on`으로 연다.
- `kairos pause` 중에는 내지 않고, `kairos focus` 중에는 쌓아 둔다.
- macOS 집중 모드가 켜져 있으면 알림 센터에만 쌓인다.

판을 올린 뒤 권한을 다시 묻는 것은 알림 헬퍼가 ad-hoc으로 서명되었기 때문이다. 다시 허용하면 된다.

## 카드나 항목이 보이지 않는다

- 표시가 닫혀 있는 동안(첫 근무일, `kairos display off`)에는 기록만 한다. `kairos list --would-show`가 표시됐을 항목을 보여 주고, `kairos list --items`는 열린 항목을 보여 준다.
- 상태 줄에 `수집 기록 없음`이 나오면 수집이 아직 돌지 않았다. `kairos doctor`의 소스 줄과 launchd 잡을 본다. 잡이 없으면 `kairos setup`을 다시 실행해 설치한다.
- 서비스가 `설정 필요`면 `kairos sources`가 모자란 값과 채우는 명령을 알려 준다.
- Mac이 잠든 동안의 틱은 고장이 아니다(`sleep_interrupted`).

## 설정

- `kairos config validate`가 틀린 칸을 알려 준다. `kairos config show --origin`은 값이 어느 파일에서 왔는지 보여 준다.
- 설정 파일은 본인 소유이고 그룹이나 남이 쓸 수 없어야 읽는다. 권한이 넓으면 `chmod 600`으로 좁힌다.
- 설정이 깨져도 `config`, `connect`, `sources`, `setup`, `uninstall`, `update`, `completions`, `help`는 돈다. 이 명령들로 고치거나 지우고 다시 시작한다.

## 세션

- `doctor`에 `session-approval`이 나오면 세션 자격에 닿는 설정이 바뀌어 세션이 잠긴 것이다. 차단 시험을 본 뒤 `kairos sessions on`을 다시 한다([수락과 세션](sessions.md)).
- 세션 토큰이 없다고 하면 읽기 전용 fine-grained 토큰을 만들어 `kairos connect session-token < 토큰파일`로 넣는다.

## Orca 패널

- 판을 올리면 설치본 폴더가 바뀌어 Orca가 다시 승인을 받는다. `kairos setup`을 다시 실행하면 마지막 단계가 적재와 승인을 묻는다.
- 패널이 비어 있으면 `kairos next`가 터미널에서 같은 내용을 내는지 본다. 터미널에서도 비어 있으면 위의 "카드나 항목이 보이지 않는다"를 본다.

## 지우고 다시 시작하기

```
kairos uninstall --dry-run --purge   # 지울 것만 본다
kairos uninstall --runtime           # 실행본만 지운다. 수집 기록과 설정은 남는다
kairos uninstall --purge             # 전부 지운다
```

## 버그 알리기

이슈에 `kairos --version`, `kairos doctor`의 출력, 다시 일으키는 순서를 적는다. 출력의 메일 주소, 조직과 저장소 이름, 경로의 사용자 이름은 가린다. 보안 문제는 공개 이슈에 쓰지 않고 [SECURITY.md](../SECURITY.md)의 방법으로 알린다.
