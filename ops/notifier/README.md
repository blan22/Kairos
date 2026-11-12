# KairosNotifier

kairos 알림을 macOS 알림 센터로 띄우는 작은 앱입니다. 알림을 누르면 `kairos reveal <id>...`를 불러 Orca 패널에서 그 일을 바로 엽니다.
osascript 알림은 '스크립트 편집기' 이름으로 뜨고 누르면 Script Editor가 열립니다. kairos는 osascript를 쓰지 않고 이 앱으로만 알립니다. 감시자도 이 앱으로 `kairos 점검` 알림을 보냅니다.

- 번들: `ops/notifier/KairosNotifier.app` (ID `dev.kairos.notifier`, 표시 이름 `kairos`)
- Dock과 메뉴 막대에 뜨지 않습니다(`LSUIElement`). 창도 없습니다.
- 알림 방식은 배너입니다. 몇 초 뒤 저절로 사라지고 알림 센터에 남습니다. 시스템 설정에서 '경고'로 바꿀 수 있습니다.

## 모드

| 명령 | 하는 일 |
| --- | --- |
| `KairosNotifier post '<json>'` | 알림 하나를 띄웁니다. 권한을 묻지 않습니다. |
| `KairosNotifier setup` | 권한을 묻고, 허용되면 안내 알림을 하나 띄웁니다. 최대 120초 기다립니다. |
| `KairosNotifier status` | `authorized`, `denied`, `notDetermined`, `provisional`, `ephemeral` 중 하나를 출력합니다. |
| `KairosNotifier` | 알림을 누르면 macOS가 인자 없이 띄웁니다. 15초 안에 응답이 없으면 끝납니다. |

`post` 입력:

```json
{"title": "긴급", "body": "회의 임박 1건", "subtitle": "선택", "items": [123, 456], "tier": "P0", "thread": "kairos.p0"}
```

- `tier`는 `P0` 또는 `P1`입니다. P0은 Time Sensitive, P1은 보통 알림입니다.
- `items`는 양의 정수만 남기고 최대 20개입니다. 알림 ID는 `kairos.<tier>.<id-id-...>`라서 같은 묶음을 다시 알리면 앞의 알림을 바꿔 끼웁니다.
- `post`는 프로세스 시작부터 3.5초 안에 끝납니다. kairos의 제한(5초)보다 짧습니다. 알림 센터가 응답하지 않으면 4로 끝납니다.
- 시간 초과 때 알림 센터가 요청을 이미 받았을 수 있습니다. kairos는 4를 받으면 다음 틱에 다시 보냅니다.

## 종료 코드

| 코드 | 뜻 | kairos의 대응 |
| --- | --- | --- |
| 0 | 게시함 | 끝 |
| 2 | 입력 오류(인자, JSON, 빈 값) | 보내지 못함 |
| 3 | 권한 없음 또는 미결정 | 보내지 못함 |
| 4 | 알림 센터가 제때 답하지 않음(게시 여부 모름) | 실패로 두고 다음 틱에 다시 시도 |
| 5 | 알림 센터가 거절함 | 보내지 못함 |

## 빌드

```sh
ops/notifier/build.sh
```

- 소스는 `main.swift`(앱)와 `Payload.swift`(입력 검사)입니다. Swift 6 모드로 컴파일합니다(`-O -target arm64-apple-macos13.0`).
- 먼저 `Tests/main.swift` 자체 시험을 돌립니다. ID 범위·중복·20개 제한, kairos가 넘기는 payload 모양, 인자 해석을 확인합니다. 알림 센터는 쓰지 않습니다.
- 번들 밖의 바이너리로 잘못된 입력(모르는 모드, thread가 빠진 payload, 틀린 tier)이 2로 끝나는지 확인합니다.
- `make_icon.swift`로 아이콘을 그립니다. 중간 산출물은 저장소의 `target/notifier/`에 둡니다. 설치는 `ops/`를 통째로 복사하므로 `ops/` 안에는 결과 번들만 둡니다.
- `target/notifier/`의 임시 폴더에서 번들을 만들고 서명한 뒤 `KairosNotifier.app`과 rename으로 바꿉니다.
- 끝으로 개발 트리 번들을 LaunchServices에서 뺍니다(`lsregister -u`). 설치본과 번들 ID, 버전이 같아서, 둘 다 등록돼 있으면 알림을 누를 때 어느 쪽이 뜰지 정해져 있지 않습니다.
- 빌드가 끝나고 30초 안팎 뒤 macOS가 다시 등록할 수 있습니다. 그래서 이 단계는 최선 노력입니다. 개발 트리 번들이 떠도 `~/.local/bin/kairos`로 kairos를 찾습니다. 확실히 하려면 문제 해결 절의 명령으로 다시 뺍니다.
- 서명은 키체인의 첫 `Apple Development` 인증서를 SHA-1로 고릅니다. 없으면 ad-hoc으로 서명하고 경고합니다.
- 서명이 바뀌면 macOS가 다른 앱으로 보고 권한을 다시 물을 수 있습니다. 같은 인증서로 빌드하세요.

## 처음 한 번: setup

`kairos setup`의 마지막 단계와 `kairos notify setup`이 아래 두 명령을 대신한다. 터미널에서 묻지 않고 바로 macOS 권한 창을 띄운다. 손으로 할 때만 아래를 쓴다.

```sh
app="$HOME/Library/Application Support/kairos/0.2.0/ops/notifier/KairosNotifier.app"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app"
"$app/Contents/MacOS/KairosNotifier" setup
```

`lsregister -f`는 설치본을 LaunchServices에 등록합니다. 알림을 누르면 macOS가 이 번들을 띄웁니다. 다시 설치한 뒤에도 한 번 실행합니다.

권한 창에서 허용하면 "알림이 켜졌습니다" 알림이 뜹니다. 그 전까지 `post`는 3으로 끝나고 Mac 알림은 나가지 않습니다.
macOS는 한 번 답한 앱에 권한 창을 다시 띄우지 않습니다. 거절한 뒤에는 `kairos notify setup`이 시스템 설정의 알림 화면을 엽니다. 거기서 kairos의 '알림 허용'을 켭니다.
설치 전 개발 트리에서 실행하면 개발 트리의 번들이 따로 권한을 받습니다. 설치본 경로로 실행하세요.

## 클릭에서 reveal까지

1. 알림을 누르면 macOS가 번들을 띄웁니다. delegate는 `applicationWillFinishLaunching`에서 걸어 응답을 잃지 않습니다.
2. `userInfo.items`에서 양의 정수 ID만 꺼냅니다(10^15 미만, 최대 20개). 비었으면 아무것도 하지 않습니다.
3. `<번들>/../../../bin/kairos reveal <id>...`를 셸 없이 실행합니다. 설치본에서는 `<rt>/bin/kairos`입니다. 없으면 `~/.local/bin/kairos`를 씁니다.
4. kairos가 끝나거나 10초가 지나면 앱이 끝납니다. Orca를 앞으로 가져오는 일은 `kairos reveal`이 합니다.

`post`, `setup`, `status` 프로세스가 살아 있는 동안 알림을 누르면 macOS는 새로 띄우지 않고 그 프로세스에 응답을 보냅니다. 이때는 `kairos reveal`을 띄운 뒤에 끝냅니다. 자식의 출력은 `/dev/null`로 보내 kairos가 자식을 기다리지 않게 합니다.

## 보안

- 실행 파일 경로는 번들 위치와 홈 폴더로만 정합니다. 알림 내용과 defaults에서 받지 않습니다.
- 알림에는 항목 ID 숫자만 싣습니다. 클릭 때 다시 검사하고, 숫자가 아닌 값은 버립니다.
- 인자는 `Process`로 넘깁니다. 셸과 AppleScript를 거치지 않습니다.
- Hardened Runtime으로 서명합니다. entitlement는 없습니다.

## 문제 해결

- 알림이 안 뜹니다: `kairos doctor`의 `Mac 알림` 줄을 보고 `kairos notify test`로 시험 알림을 보내 봅니다. 권한이 없으면 `kairos notify setup`을 실행합니다. 전에 거절했으면 시스템 설정 > 알림 > kairos에서 '알림 허용'을 켭니다.
- 집중 모드에서 P0이 가려집니다: 이 앱에는 Time Sensitive 자격(entitlement)이 없어 집중 모드를 뚫지 못할 수 있습니다. 시스템 설정 > 집중 모드 > 해당 모드 > 허용된 앱에 kairos를 넣으세요.
- 아이콘이 예전 것으로 보입니다: macOS가 아이콘을 캐시합니다. 다시 로그인하면 바뀝니다.
- 눌러도 패널이 안 열립니다: 로그를 봅니다.

  ```sh
  log show --last 10m --predicate 'subsystem == "dev.kairos.notifier"'
  ```

  `kairos 실행 파일을 찾지 못했습니다`가 보이면 `<rt>/bin/kairos`와 `~/.local/bin/kairos`를 확인합니다.
- 눌렀을 때 개발 트리의 번들이 뜹니다: LaunchServices에 등록된 경로를 봅니다.

  ```sh
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -dump | grep 'path:.*KairosNotifier.app'
  ```

  설치본 경로만 남아야 합니다. 개발 트리 경로가 있으면 `lsregister -u <경로>`로 빼고, 설치본은 `lsregister -f`로 다시 등록합니다. 개발 트리 번들을 직접 실행하면 다시 등록됩니다.
