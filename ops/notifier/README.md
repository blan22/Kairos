# KairosNotifier

KairosNotifier는 kairos 알림을 macOS 알림 센터에 띄우는 작은 앱입니다. 알림을 누르면 `kairos reveal <id>...`를 호출해 Orca 패널에서 그 일을 바로 엽니다.

kairos는 osascript로 알림을 보내지 않습니다. osascript 알림은 '스크립트 편집기'라는 이름으로 뜨고, 누르면 Script Editor가 열리기 때문입니다. 그래서 kairos의 알림은 모두 이 앱으로 보냅니다. 감시자가 보내는 `kairos 점검` 알림도 마찬가지입니다.

| 항목 | 값 |
|---|---|
| 번들 | `ops/notifier/KairosNotifier.app` |
| 번들 ID | `dev.kairos.notifier` |
| 표시 이름 | `kairos` |
| 알림 스타일 | 배너 |

앱에는 창이 없고, Dock과 메뉴 막대에도 나타나지 않습니다(`LSUIElement`). 배너 알림은 몇 초 뒤 저절로 사라지고 알림 센터에 남습니다. 시스템 설정에서 '경고' 스타일로 바꿀 수 있습니다.

## 알림 권한 받기

보통은 직접 할 일이 없습니다. `kairos setup`의 마지막 단계와 `kairos notify setup`이 아래 두 명령을 대신 실행합니다. 터미널에서 묻지 않고 바로 macOS 권한 창을 띄웁니다.

손으로 하려면 다음 명령을 실행하세요. 경로의 `0.4.2`는 설치한 버전으로 바꿉니다.

```sh
app="$HOME/Library/Application Support/kairos/0.4.2/ops/notifier/KairosNotifier.app"
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$app"
"$app/Contents/MacOS/KairosNotifier" setup
```

`lsregister -f`는 설치본을 LaunchServices에 등록합니다. 알림을 누르면 macOS가 이 번들을 실행합니다. 다시 설치했을 때도 한 번 실행하세요.

권한 창에서 허용하면 "알림이 켜졌습니다" 알림이 뜹니다. 그 전까지 `post`는 종료 코드 3으로 끝나고, Mac 알림은 나가지 않습니다.

macOS는 한 번 응답한 앱에 권한 창을 다시 띄우지 않습니다. 전에 거절했다면 `kairos notify setup`이 시스템 설정의 알림 화면을 엽니다. 거기서 kairos의 '알림 허용'을 켜세요.

> [!NOTE]
> 설치하기 전에 개발 트리에서 실행하면 개발 트리의 번들이 따로 권한을 받습니다. 권한은 설치본 경로로 받으세요.

## 명령

| 명령 | 하는 일 |
| --- | --- |
| `KairosNotifier post '<json>'` | 알림 하나를 띄웁니다. 권한을 묻지 않습니다 |
| `KairosNotifier setup` | 권한을 묻고, 허용되면 안내 알림을 하나 띄웁니다. 최대 120초 기다립니다 |
| `KairosNotifier status` | `authorized`, `denied`, `notDetermined`, `provisional`, `ephemeral` 중 하나를 출력합니다 |
| `KairosNotifier` | 알림을 눌렀을 때 macOS가 인자 없이 실행합니다. 15초 안에 응답이 없으면 종료합니다 |

### `post` 입력

```json
{"title": "긴급", "body": "회의 임박 1건", "subtitle": "선택", "items": [123, 456], "tier": "P0", "thread": "kairos.p0"}
```

- `tier`는 `P0` 또는 `P1`입니다. P0는 Time Sensitive 알림으로, P1은 일반 알림으로 뜹니다.
- `items`는 양의 정수만 남기고 최대 20개까지 받습니다.
- 알림 ID는 `kairos.<tier>.<id-id-...>` 꼴입니다. 그래서 같은 등급으로 같은 항목들을 다시 알리면 앞의 알림을 대체합니다.

`post`는 프로세스가 시작된 뒤 3.5초 안에 끝납니다. kairos가 정한 제한 시간(5초)보다 짧습니다. 알림 센터가 제때 응답하지 않으면 종료 코드 4로 끝납니다. 시간 초과로 끝나도 알림 센터가 요청을 이미 받았을 수 있습니다. kairos는 4를 받으면 다음 틱에 다시 보냅니다.

### 종료 코드

| 코드 | 의미 | kairos의 처리 |
| --- | --- | --- |
| 0 | 게시함 | 완료 |
| 2 | 입력 오류(인자, JSON, 빈 값) | 보내지 못함 |
| 3 | 권한 없음 또는 미결정 | 보내지 못함 |
| 4 | 알림 센터가 제때 응답하지 않음(게시 여부 알 수 없음) | 실패로 처리하고 다음 틱에 다시 시도 |
| 5 | 알림 센터가 거절함 | 보내지 못함 |

## 알림을 누르면

알림을 누른 뒤 Orca 패널이 열리기까지는 다음 순서로 진행됩니다.

1. macOS가 번들을 실행합니다. 앱은 delegate를 `applicationWillFinishLaunching`에서 등록하므로 클릭 응답을 놓치지 않습니다.
2. `userInfo.items`에서 양의 정수 ID만 꺼냅니다. 10^15 미만, 최대 20개입니다. 남은 ID가 없으면 아무것도 하지 않습니다.
3. `<번들>/../../../bin/kairos reveal <id>...`를 셸 없이 실행합니다. 설치본에서는 이 경로가 `<rt>/bin/kairos`가 됩니다. 이 파일이 없으면 `~/.local/bin/kairos`를 씁니다.
4. kairos가 끝나거나 10초가 지나면 앱을 종료합니다. Orca를 앞으로 가져오는 일은 `kairos reveal`이 맡습니다.

`post`, `setup`, `status` 프로세스가 살아 있는 동안 알림을 누르면, macOS는 새 프로세스를 띄우지 않고 그 프로세스에 응답을 보냅니다. 이때도 `kairos reveal`을 실행한 뒤에 종료합니다. reveal 자식 프로세스의 출력은 늘 `/dev/null`로 보냅니다. 그래야 kairos가 자식이 끝나기를 기다리지 않습니다.

## 보안

- 실행 파일 경로는 번들 위치와 홈 폴더로만 정합니다. 알림 내용이나 defaults에서 받지 않습니다.
- 알림에는 항목 ID 숫자만 싣습니다. 클릭할 때 다시 검사하고, 숫자가 아닌 값은 버립니다.
- 인자는 `Process`로 넘깁니다. 셸과 AppleScript를 거치지 않습니다.
- Hardened Runtime으로 서명합니다. entitlement는 없습니다.

## 빌드

```sh
ops/notifier/build.sh
```

소스는 앱 본체인 `main.swift`와 입력을 검사하는 `Payload.swift`입니다. Swift 6 모드로 컴파일합니다(`-O -target arm64-apple-macos13.0`). 빌드 스크립트는 다음 순서로 진행합니다.

1. `Tests/main.swift`의 자체 테스트를 돌립니다. ID 범위와 중복, 20개 제한, kairos가 넘기는 payload 형식, 인자 해석을 확인합니다. 알림 센터는 쓰지 않습니다.
2. 앱과 아이콘 도구(`make_icon.swift`)를 컴파일합니다.
3. 번들 밖의 바이너리에 잘못된 입력을 줘서 종료 코드 2로 끝나는지 확인합니다. 모르는 모드, `thread`가 빠진 payload, 틀린 `tier`를 넣어 봅니다.
4. `make_icon.swift`로 아이콘을 그립니다.
5. `target/notifier/` 아래의 임시 폴더에서 번들을 만들고 서명합니다.
6. rename으로 `KairosNotifier.app`을 교체합니다.
7. 개발 트리의 번들을 LaunchServices에서 등록 해제합니다(`lsregister -u`).

중간 산출물은 저장소의 `target/notifier/`에 둡니다. 설치할 때 `ops/`를 통째로 복사하므로 `ops/` 안에는 결과 번들만 둡니다.

### 서명

서명에는 키체인의 첫 번째 `Apple Development` 인증서를 SHA-1로 골라 씁니다. 인증서가 없으면 ad-hoc으로 서명하고 경고를 냅니다.

서명이 바뀌면 macOS가 다른 앱으로 보고 권한을 다시 물을 수 있습니다. 늘 같은 인증서로 빌드하세요.

### 개발 트리 번들의 등록 해제

개발 트리의 번들은 설치본과 번들 ID, 버전이 같습니다. 둘 다 LaunchServices에 등록되어 있으면 알림을 눌렀을 때 어느 쪽이 실행될지 알 수 없습니다. 그래서 빌드의 마지막 단계에서 개발 트리 번들을 등록 해제합니다.

다만 빌드가 끝나고 30초쯤 지나면 macOS가 다시 등록할 수 있어서, 이 단계가 확실한 보장은 아닙니다. 개발 트리 번들이 실행되더라도 kairos는 `~/.local/bin/kairos`로 찾습니다. 확실히 하려면 [문제 해결](#문제-해결)의 명령으로 다시 등록 해제하세요.

## 문제 해결

### 알림이 뜨지 않습니다

`kairos doctor`의 `Mac 알림` 줄을 확인하고, `kairos notify test`로 테스트 알림을 보내 보세요. 권한이 없으면 `kairos notify setup`을 실행합니다. 전에 거절했다면 시스템 설정 > 알림 > kairos에서 '알림 허용'을 켜세요.

### 집중 모드에서 P0가 가려집니다

이 앱에는 Time Sensitive 자격(entitlement)이 없어서 집중 모드를 통과하지 못할 수 있습니다. 시스템 설정 > 집중 모드 > 해당 모드 > 허용된 앱에 kairos를 추가하세요.

### 아이콘이 예전 것으로 보입니다

macOS가 아이콘을 캐시하기 때문입니다. 다시 로그인하면 바뀝니다.

### 눌러도 패널이 열리지 않습니다

로그를 확인하세요.

```sh
log show --last 10m --predicate 'subsystem == "dev.kairos.notifier"'
```

`kairos 실행 파일을 찾지 못했습니다`가 보이면 `<rt>/bin/kairos`와 `~/.local/bin/kairos`가 있는지 확인하세요.

### 눌렀을 때 개발 트리의 번들이 실행됩니다

LaunchServices에 등록된 경로를 확인하세요.

```sh
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -dump | grep 'path:.*KairosNotifier.app'
```

설치본 경로만 남아 있어야 합니다. 개발 트리 경로가 있으면 `lsregister -u <경로>`로 등록 해제하고, 설치본은 `lsregister -f`로 다시 등록하세요. 개발 트리 번들을 직접 실행하면 다시 등록됩니다.
