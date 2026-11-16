# 세션 샌드박스

세션을 어떻게 띄우고 어떻게 가두는지 설명하는 설계 문서입니다. 세션을 켜고 쓰는 방법은 [에이전트 세션](sessions.md)에, 지켜야 하는 보안 기준은 [SECURITY.md](../SECURITY.md)에 있습니다. 세션 코드는 `crates/kairos-sessions`에 있습니다.

이 문서는 macOS의 샌드박스를 설명합니다. Linux와 Windows에서는 `kairos_core::platform::Os::supports_sessions`가 거짓이라 수락, 세션 켜기, `kairos-run`이 모두 거절합니다. Linux용 차단 목록(`SessionProfile::for_os`)은 샌드박스가 기준을 지킬 수 있게 되면 쓰려고 남겨 둔 것입니다. 다시 열 조건은 [지원하는 OS](platforms.md#세션을-macos에서만-여는-이유)에 있습니다.

## 수락 순서

`kairos accept`는 `BEGIN IMMEDIATE` 트랜잭션으로 run을 `claimed`로 기록하고 항목을 `accepted`로 바꿉니다. 경쟁에서 진 쪽은 기존 run을 받습니다. 그다음 Orca를 아래 순서로 호출합니다. 도중에 사용자가 끝내기나 취소를 누르면 그 결과를 돌려주고 멈춥니다.

1. `orca reference find <url>`. 기록으로 연결된(`linked`) 살아 있는 에이전트가 있으면 그 세션을 이어받고(`adopted`) 끝냅니다.
2. `orca worktree create --repo id:<대상 저장소> --name t-<짧은id> --no-parent --setup skip --base-branch <기준 브랜치> --reference <url>`. 기준 브랜치는 조직 프로필의 플레이북 대상에서 가져옵니다. `id:` 뒤의 Orca 저장소 ID는 그 대상의 저장소를 `config.toml`의 `[hosts.orca.repos]`에서 찾은 값입니다. 설정에 없으면 `orca repo list --json`에서 원격 주소(`github.com/<owner>/<name>`)가 같은 저장소를 고릅니다. 같은 원격의 클론이 여럿이면 폴더 이름이 저장소 이름과 같은 하나를 고릅니다. 하나로 고르지 못하면 worktree를 만들지 않고 `repo_unresolved`로 알립니다.
3. `orca worktree show`로 경로와 HEAD를 확인하고, 선택자를 run에 저장합니다.
4. `orca terminal create --worktree <선택자> --title kairos-<짧은id> --command '<kairos-run> <uuid>' --focus`

Orca를 호출할 때는 다음 규칙을 지킵니다.

- `--pr`, `orca agent launch`, `worktree create --agent`, `orca terminal send`는 쓰지 않습니다.
- `--focus`는 `kairos accept`의 `terminal create`에만 붙입니다. 사용자가 패널에서 버튼을 누른 순간이라 세션 창을 보여 줍니다.
- orca는 데이터 폴더를 작업 디렉터리로 하고, `ORCA_TERMINAL_HANDLE` 같은 호출자 값을 지운 뒤 인자 배열로 호출합니다. `KAIROS_ORCA`로 실행 파일을 바꿀 수 있습니다.
- `terminal create`는 재시도 ID가 없으므로 다시 호출하지 않습니다. 응답을 잃으면 `visualLayouts`의 탭 제목과 `worktree ps`로 찾고, 찾지 못하면 `needs_attention`으로 둡니다. `kairos-run`은 run마다 한 번만 뜨므로 늦게 도착한 명령이 세션을 하나 더 만들 수 없습니다.

## kairos-run

`kairos-run <uuid>`는 UUID만 받습니다. 이슈 URL은 DB에서 읽고, 시작 버튼과 같은 조건을 다시 검사합니다. 수락할 때 고른 플레이북은 run의 `playbook` 열에 남아 있고, `kairos-run`은 이 값으로 프로필과 프롬프트를 정합니다.

### 외부인 재확인

세션을 띄우기 직전에 사용자의 gh 로그인으로 GraphQL을 불러 그 이슈나 PR의 본문, 댓글, 리뷰, 리뷰 스레드 글과 작성자를 끝까지 읽습니다. 같은 질의로 받은 글이 세션 입력이 되므로, 확인한 뒤에 달린 글이 세션에 들어가지 않습니다.

| 결과 | 처리 |
|---|---|
| T0, T1이 아닌 사람의 글이 있음 | `outsider_writing`으로 실패 |
| 전부 읽었다고 확인할 수 없음(gh 실패, 시간 초과, 개수 불일치, 등급을 정할 수 없는 작성자) | `live_unverified`로 실패 |
| 통과 | 읽은 글을 세션 입력 파일로 쓰고, URL과 그 파일의 경로만 고정 프롬프트에 넣어 세션을 띄움. PR이면 diff도 받음 |

실패하면 run을 실패로 내리고 항목을 되돌립니다. 봇이 쓴 글은 세지 않습니다.

## claude 실행 옵션

claude는 `--session-id <uuid>`, `-n <제목>`, `--permission-mode auto`, `--strict-mcp-config`, `--settings`, `--tools`, `--disallowedTools`, `--add-dir <세션 입력 폴더>`로 뜹니다. 프로필에 따라 다른 값은 다음과 같습니다.

| | 읽는 프로필 | 고치는 프로필 |
|---|---|---|
| `--tools` | `Bash,Read,Grep,Glob` | 여기에 `Edit,Write` 추가 |
| `--setting-sources` | 넘기지 않음 | `user` |
| 거부 목록 | 기본 | `git commit`과 Edit, Write만 풀고, 아래 Edit 거부를 추가 |

고치는 프로필이 `--setting-sources user`를 쓰는 데는 이유가 있습니다. 프로젝트와 로컬 설정(`.claude/settings.json`, `.claude/settings.local.json`)의 훅은 샌드박스 밖에서 실행됩니다. 고치는 세션은 그 훅이 호출하는 작업 공간의 파일을 고칠 수 있으므로, 프로젝트와 로컬 설정을 읽지 않게 합니다.

사용자 설정은 남겨 둡니다. Orca 상태 보고 훅이 거기 있어서, 빼면 `worktree ps`가 세션을 보지 못합니다. 읽는 프로필은 작업 공간을 고치지 못하므로 `--setting-sources`를 넘기지 않습니다.

고치는 프로필에는 다음 경로의 Edit 거부가 추가됩니다.

- 데이터 폴더, 설치 폴더
- 홈 폴더의 제어 경로와 셸 시작 파일
- 작업 공간의 `.git`(하위 폴더에 새로 만드는 `.git` 포함), `.claude`, `.mcp.json`(하위 폴더 포함), `CLAUDE.md`, `CLAUDE.local.md`, hook 폴더
- 아래 [고치는 세션의 git 쓰기](#고치는-세션의-git-쓰기)의 denyWrite 경로 전부

push, pull, remote, config, commit-tree, update-ref, gh 쓰기는 두 프로필 모두 막습니다.

### 환경 변수

환경 변수는 두 단계로 지웁니다. 먼저 `kairos-sessions`의 `CLEARED_ENV`(`profile.rs`)에 있는 변수를 `env -u`로 시작 환경에서 빼고, `--settings`로 빈 값을 한 번 더 덮어씁니다. claude가 시작한 뒤에 `~/.claude/settings.json`의 `env`를 다시 넣기 때문입니다.

이 동작은 가짜 claude로만 테스트했습니다. 차단 테스트 때 세션 안에서 `printenv`로 확인하세요.

### 자격 증명과 세션 입력

세션에는 GitHub 자격 증명을 넘기지 않습니다. `GH_TOKEN`을 포함한 `CLEARED_ENV`를 모두 `env -u`로 지우고 `--settings`에서도 빈 값으로 고정합니다. 설정 파일은 `<KAIROS_HOME>/session-settings/<run>.json`(0700 폴더 안의 0600 파일)이고, 세션이 끝나면 지웁니다.

- `~/.claude/settings.json`의 env에 `GH_TOKEN`이 있어도 `--settings`가 빈 값으로 다시 덮습니다.
- 세션의 gh는 `GH_CONFIG_DIR=/var/empty`를 보므로 키체인의 로그인 정보를 찾지 않습니다. 네트워크도 없어서 어떤 자격 증명을 얻어도 GitHub에 닿지 못합니다.
- 이슈나 PR의 글은 `~/Library/Caches/kairos/session-briefs/<run>/brief.md`로 넘깁니다. run마다 새로 만든 0700 폴더 안의 0600 파일이고, 이미 있는 폴더나 링크인 위 폴더에는 쓰지 않습니다. 세션이 끝나면 지웁니다.
- 세션은 `--add-dir`로 이 run 폴더를 엽니다. 본문은 인자와 프롬프트에 넣지 않고 세션이 Read 도구로 읽게 해서, 지시가 아니라 데이터로 다루게 합니다.
- 세션 입력 폴더 전체(`session-briefs`)는 샌드박스 denyWrite와 denyRead, Edit 거부에 들어 있습니다. 세션은 자기 입력을 Read로만 읽고, 다른 run의 입력은 작업 공간 밖이라 읽지 못합니다.
- 봇의 글은 작성자 판정에서 세지 않으므로 봇의 글이라고 표시합니다. 울타리 밖 제목 줄의 이름과 경로에서는 제어 문자와 백틱을 지웁니다.

## 샌드박스

`--settings`는 다음을 함께 넘깁니다.

- **Bash 샌드박스.** 쓰기는 작업 공간만 허용합니다. 연결된 worktree라면 아래에서 설명할 공용 `.git`의 막지 않은 부분도 허용합니다. 허용하는 네트워크 호스트는 없고(`allowedDomains`가 비어 있고 `strictAllowlist`가 켜져 있습니다), GitHub 호스트는 사용자 설정이 열어도 막습니다. 샌드박스 밖 재실행은 금지합니다.
- **git 자격 증명 끄기.**
- **`permissions.blockReadsOutsideWorkingDirectories`.**
- **고치는 프로필의 `permissions.blockWritesOutsideWorkingDirectories`.** claude 2.1.314 이상에서 Edit, Write의 작업 공간 밖 쓰기도 막습니다. 이 설정을 모르는 버전에서도 차단이 유지되도록, 홈 폴더의 제어 경로(`~/.claude`, `~/Library/LaunchAgents`, `~/.local/bin`, git 설정, `~/.ssh`)와 셸 시작 파일은 Edit 거부로 한 번 더 막습니다.

샌드박스의 denyRead는 Bash에만 적용됩니다. Read, Grep, Glob은 위 설정과 Read 거부 규칙으로 작업 공간 밖과 자격 증명 파일(`~/.ssh`, `~/.config/gh`, `~/Library/Keychains` 등)을 막습니다. 접두어 방식의 거부 목록은 보조 방어선일 뿐 경계가 아닙니다.

## 고치는 세션의 git 쓰기

고치는 프로필의 커밋은 Orca worktree 안에서 이루어집니다. 연결된 worktree의 git 폴더는 본 저장소의 `.git` 아래(`worktrees/<이름>`과 공용 objects, refs, logs)에 있어서 작업 공간 밖입니다. 하지만 claude 샌드박스가 연결된 worktree의 공용 `.git` 쓰기를 허용하고, 그 안의 `hooks`와 `config`는 막습니다. 그래서 `allowWrite`는 따로 두지 않습니다.

`kairos-run`은 세션을 띄우기 전에 git을 실행하지 않습니다. 작업 공간(현재 폴더)의 `.git` 포인터와 `commondir`만 읽어서(`SessionWorkspace`) 다음 경로를 denyWrite에 추가합니다.

- 작업 공간의 `.git` 포인터
- 공용 폴더의 `hooks`, `config`, `config.worktree`, `info`, `modules`, `objects/info`, `shallow`, `commondir`
- 자기 관리 폴더의 `commondir`, `gitdir`, `config.worktree`
- 다른 worktree의 관리 폴더
- 작업 공간의 `.husky`, `.githooks`, `.claude`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md`
- 저장소, 전역, 시스템 git 설정(include 포함)의 `core.hooksPath`가 가리키는 폴더와 include된 설정 파일

여기에 glob을 더합니다.

- 작업 공간 이름 여섯 개(`.husky`, `.githooks`, `.claude`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md`)와, git 설정이 가리키는 작업 공간 안의 대상(hook 폴더, include된 파일). 그 경로와 하위 전체가 대상입니다.
- `.git`이 폴더가 아닐 때(연결된 worktree의 포인터 파일) `<작업 공간>/.git`과 그 하위
- 하위 폴더에 새로 만드는 `.git`(`<작업 공간>/*/**/.git`, `<작업 공간>/*/**/.git/**`)
- 모든 폴더의 `CLAUDE.md`, `CLAUDE.local.md`(`<작업 공간>/**/CLAUDE.md` 등)와 `.claude`, `.mcp.json`(`<작업 공간>/**/.claude`, `<작업 공간>/**/.claude/**`, `<작업 공간>/**/.mcp.json`)

이 glob들은 이렇게 동작합니다.

- 커밋은 포인터 파일에 기록하지 않습니다. 그래서 `<작업 공간>/.git`을 막아도 연결된 worktree에서 커밋할 수 있습니다.
- 하위 `.git` glob은 본 checkout과 연결된 worktree 모두에 붙습니다. `*/`로 시작하므로 작업 공간 바로 아래의 `.git`에는 일치하지 않습니다. 따라서 본 checkout의 커밋은 그대로 `.git` 폴더에 쓸 수 있습니다.
- glob이 아닌 denyWrite 경로에는 그 경로와 하위 전체에 대한 Edit 거부가, glob에는 같은 glob의 Edit 거부가 붙습니다. 그래서 Bash와 Edit, Write가 같은 범위를 막습니다.

### 대소문자와 유니코드

macOS의 APFS는 기본으로 대소문자를 구분하지 않습니다. `.GIT`이나 `claude.md`도 git과 claude가 같은 파일로 열기 때문에, glob은 글자마다 `[gG]`처럼 두 경우를 모두 받습니다.

APFS는 유니코드도 접어서 비교합니다. `ſ`는 s로, 켈빈 기호 K는 k로 열립니다. Seatbelt 정규식은 바이트 단위로 비교하므로 s와 k 자리는 `*`로 넓힙니다(`.husky`는 `.[hH][uU]*[yY]`). claude 샌드박스는 glob을 Seatbelt 정규식으로 바꿀 때 닫힌 `[..]`를 문자 클래스 그대로 넘깁니다.

### 왜 막는지, 무엇을 못 막는지

위 경로는 모두 사용자가 나중에 샌드박스 밖에서 실행하는 git이나 claude가 읽어, 코드 실행으로 이어질 수 있는 곳입니다.

- claude 설정과 `CLAUDE.md`는 이 세션뿐 아니라 사용자가 나중에 여는 세션도 다시 읽습니다. 하위 폴더의 `.claude`와 `.mcp.json`은 그 폴더에서 연 세션의 프로젝트 설정이 됩니다.
- 하위 폴더에 심어 둔 `.git`의 설정은 사용자가 그 폴더에서 git을 실행할 때 동작합니다.
- git 설정 값을 해석할 수 없거나 작업 공간을 확인하지 못하면 고치는 세션은 뜨지 않습니다.

> [!WARNING]
> `.git`이라는 이름 없이 git 폴더 구조(`HEAD`, `objects`, `refs`)만 갖춘 bare 저장소는 이름으로 막을 수 없어 위험으로 남습니다. 전역 git 설정에 `safe.bareRepository=explicit`를 두면 git이 그런 폴더를 저장소로 열지 않습니다.

## 세션 프롬프트

세션 프롬프트는 이 제약을 먼저 알려 줍니다. 외부 쓰기가 없고, 네트워크가 없어 gh와 패키지 설치가 실패하며, 인터프리터와 셸 접두어는 거부된다는 내용입니다. 고치는 세션에는 끝나면 kairos가 커밋을 올린다는 것도 알려 줍니다. 막히면 우회하지 말고 멈춰서 남은 일을 적게 합니다.

## PR 올리기의 안전장치

세션이 끝난 뒤의 올리기와 `kairos pr`은 같은 코드를 씁니다. 세션이 끝난 뒤에는 방금 정상으로 끝나 `ended`가 된 그 run만 검사하고, 커밋과 바뀐 파일 목록을 보여 준 뒤 사용자가 일회용 확인 코드를 입력해야 올립니다. 비교와 push는 데이터 폴더의 검사용 저장소(`pr-check/<worktree 이름>`)에서 `--git-dir`을 밝혀 부르므로 `safe.bareRepository=explicit`에서도 동작하고, 끝나면 지웁니다. 자동 경로의 PATH는 `/usr/bin:/bin:/usr/sbin:/sbin`과 세션 전에 정한 gh의 폴더뿐입니다. 세션 터미널의 PATH를 물려받으므로 작업 공간과 git 폴더, 세션 입력 폴더 아래의 PATH 항목을 빼고, gh는 세션을 띄우기 전에 그 폴더 밖에서 정해 둡니다. 확인한 커밋 SHA를 그대로 올리고, 저장소 훅과 replace ref를 끕니다(`core.hooksPath=/dev/null`, `--no-replace-objects`). 올리기 전에 검사하는 조건 전체는 [SECURITY.md](../SECURITY.md#pr-올리기)에 있습니다.

## 보안 기준을 바꿀 때

세션 보안 기준(거부 목록, 샌드박스)을 바꾸면 `kairos-config`의 `SESSION_FLOOR_VERSION`을 올립니다. 그러면 모든 사용자의 세션이 다시 잠기고, 각자 차단 테스트를 확인한 뒤 다시 켜야 합니다.
