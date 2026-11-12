# 수락과 세션

## 수락

`kairos accept <id>`는 멱등이다. 패널의 시작 버튼이 이 명령을 호출한다.

- 표시 게이트가 닫혀 있으면 `gate_closed`를 돌려준다.
- 세션 잠금이 걸려 있으면 `not_eligible`과 URL만 돌려준다.
- 세션이나 Orca 터미널 안에서 호출하면 `session_caller`로 거절한다.

### 세션 잠금

세션 잠금은 차단 테스트를 하기 전에 세션이 열리는 일을 막는다. 차단 테스트는 세션을 켜기 전에 사용자가 실제 세션으로 직접 확인하는 점검이다. CLI 메시지에는 '차단 시험'으로 나온다.

- 세션이 확인 창 없이 뜨고 `worktree ps`에 잡히는지 본다.
- 세션 안에서 `printenv`로 지워야 할 환경 변수가 비어 있는지 본다.
- 메시지 전송 도구, gh 쓰기, git push 같은 외부 쓰기를 시도해 모두 막히는지 본다. 회사 저장소가 아니라 개인 테스트 저장소와 로컬 bare 원격으로 한다.

잠금은 다음과 같이 동작한다.

- 테스트를 확인한 뒤 사용자가 `kairos sessions on`으로 켠다. 끄면 시작 버튼이 사라진다.
- 상태는 `doctor`의 `세션` 줄에 나온다.
- 켤 때의 설정 지문이 `session-approval`에 남는다. 세션 자격에 영향을 주는 설정이 바뀌면 다시 잠긴다.
- 세션 보안 기준(거부 목록, 샌드박스)을 바꾸면 `kairos-config`의 `SESSION_FLOOR_VERSION`을 올려 모든 사용자의 세션을 다시 잠근다.

### 수락 순서

`BEGIN IMMEDIATE` 트랜잭션으로 run을 `claimed`로 기록하고 항목을 `accepted`로 바꾼다. 경쟁에서 진 쪽은 기존 run을 받는다. 세션은 이슈마다 하나이므로, 같은 이슈의 다른 항목이 연 세션이 있으면 그 세션으로 보낸다. 이어서 다음 순서로 Orca를 호출한다. 도중에 사용자가 끝내기나 취소를 누르면 그 결과를 돌려주고 멈춘다.

1. `orca reference find <url>`. 기록으로 연결된(`linked`) 살아 있는 에이전트가 있으면 그 세션을 이어받고(`adopted`) 끝낸다.
2. `orca worktree create --repo id:<대상 저장소> --name t-<짧은id> --no-parent --setup skip --base-branch <기준 브랜치> --reference <url>`. 기준 브랜치는 조직 프로필의 플레이북 대상에서 가져온다. `id:` 뒤의 Orca 저장소 ID는 그 대상의 저장소를 `config.toml`의 `[hosts.orca.repos]`에서 찾은 값이다. 대상이나 ID가 없으면 Orca를 호출하지 않는다.
3. `orca worktree show`로 경로와 HEAD를 확인하고 선택자를 run에 저장한다.
4. `orca terminal create --worktree <선택자> --title kairos-<짧은id> --command '<kairos-run> <uuid>' --focus`

Orca 호출에는 다음 규칙이 있다.

- `--pr`, `orca agent launch`, `worktree create --agent`, `orca terminal send`는 쓰지 않는다.
- `--focus`는 `kairos accept`의 `terminal create`에만 붙인다. 사용자가 패널에서 버튼을 누른 순간이므로 세션 창을 보여 준다.
- orca는 데이터 폴더를 작업 디렉터리로 하고, `ORCA_TERMINAL_HANDLE` 같은 호출자 값을 지운 뒤 인자 배열로 호출한다. `KAIROS_ORCA`로 실행 파일을 바꿀 수 있다.
- `terminal create`는 재시도 ID가 없으므로 다시 호출하지 않는다. 응답을 잃으면 `visualLayouts`의 탭 제목과 `worktree ps`로 찾고, 찾지 못하면 `needs_attention`으로 둔다. `kairos-run`은 run마다 한 번만 뜨므로 늦게 도착한 명령이 세션을 하나 더 만들지 못한다.

## 플레이북

### 대상

세션에 맡길 수 있는 항목은 `playbook_of`가 정한다. 다음 조건을 모두 만족해야 한다.

- 객체 조건(`playbook_for`): 열린 이슈나 PR이고, T0나 T1이 열었고, 매핑표에 있는 저장소이며, URL 형식이 맞다.
- kairos가 본 그 이슈의 사람이 쓴 글이 모두 T0나 T1의 것이다. 한 번이라도 본 T2, T3의 글은 `outsider_marks`에 남아 보존 기한과 정리 작업이 지나도 계속 막는다.
- 처음 읽을 때 최근 20개에 들지 않았던 글과 마지막 수집 뒤에 달린 글은 `kairos-run`이 세션을 띄우기 직전에 다시 확인한다.

조건에 맞지 않는 항목은 `not_eligible`과 표시용 URL만 받고 Orca를 호출하지 않는다.

제안은 이슈마다 하나이고, 플레이북이 있는 첫 번째 열린 항목에 붙는다. 그 이슈에 세션이 없어야 한다. 시작 버튼은 여기에 세션 잠금이 풀려 있어야 한다는 조건이 더 붙는다. 잠겨 있으면 목록에 `playbook`과 `scope`만 나오고 `start`는 없다.

### 종류

어떤 일을 맡길지는 항목이 열린 이유에 따라 정한다.

| 플레이북 | 대상 | 세션이 하는 일 | 프로필 |
|---|---|---|---|
| `qa_issue` | 내 담당 이슈 | 원인 조사, 수정 계획, 수정과 로컬 커밋 | 고치는 프로필 |
| `pr_review` | 리뷰를 요청받은 PR | 리뷰 의견 초안 | 읽는 프로필 |
| `reply_draft` | 답장이 필요한 이슈 | 답변 초안 | 읽는 프로필 |

- `qa_issue`는 조사와 계획을 마친 뒤 해당 worktree에서 코드를 고치고 로컬에 커밋한다.
- 읽는 프로필의 결과는 세션 안에 남는다.
- 세션이 외부에 쓰는(push, PR, 코멘트, Notion) 플레이북은 없다. 고치는 세션의 커밋은 사용자가 `PR 올리기`를 누를 때만 세션 밖에서 올라간다([PR 올리기](#pr-올리기)).
- 수락할 때 고른 값이 run의 `playbook` 열에 남고, `kairos-run`은 이 값으로 프로필과 프롬프트를 정한다.

### 제안하지 않는 항목

- Slack, Gmail, Notion, Calendar 항목. 세션이 그 소스를 읽을 방법이 없고(`--strict-mcp-config`), 본문을 저장하지 않으므로 넘겨줄 값도 없다.
- 내 PR의 CI 실패와 변경 요청, PR에 달린 글. `gh run`과 `gh api`가 막혀 있어 CI 로그와 인라인 리뷰 글을 읽을 수 없다.

## kairos-run

`kairos-run <uuid>`는 UUID만 받는다. 이슈 URL은 DB에서 읽고, 시작 버튼과 같은 조건을 다시 검사한다.

### 외부인 재확인

세션을 띄우기 전에 gh GraphQL로 그 이슈나 PR의 본문, 댓글, 리뷰, 리뷰 스레드 글의 작성자를 끝까지 읽는다.

- T0, T1이 아닌 사람의 글이 있으면 `outsider_writing`으로 실패 처리한다.
- 전부 읽었다고 확인할 수 없으면(gh 실패, 시간 초과, 개수 불일치, 등급을 정할 수 없는 작성자) `live_unverified`로 실패 처리한다.
- 실패하면 run을 실패로 내리고 항목을 되돌린다.
- 봇이 쓴 글은 세지 않는다.

통과하면 URL을 고정 프롬프트에 넣는다.

### claude 실행 옵션

claude는 `--session-id <uuid>`, `--permission-mode auto`, `--strict-mcp-config`, `--settings`, `--tools`, `--disallowedTools`로 뜬다.

| | 읽는 프로필 | 고치는 프로필 |
|---|---|---|
| `--tools` | `Bash,Read,Grep,Glob` | 여기에 `Edit,Write` 추가 |
| `--setting-sources` | 넘기지 않는다 | `user` |
| 거부 목록 | 기본 | `git commit`과 Edit, Write만 풀고 아래 Edit 거부를 추가 |

- 고치는 프로필이 `--setting-sources user`를 쓰는 이유: 프로젝트와 로컬 설정(`.claude/settings.json`, `.claude/settings.local.json`)의 훅은 샌드박스 밖에서 실행된다. 고치는 세션은 그 훅이 호출하는 작업 공간의 파일을 고칠 수 있으므로 프로젝트와 로컬 설정을 읽지 않는다.
- 사용자 설정은 남긴다. Orca 상태 보고 훅이 거기 있어서, 빼면 `worktree ps`가 세션을 보지 못한다.
- 읽는 프로필은 작업 공간을 고치지 못하므로 `--setting-sources`를 넘기지 않는다.
- 고치는 프로필에는 다음 경로의 Edit 거부가 추가된다: 데이터 폴더, 설치 폴더, 홈 폴더의 제어 경로와 셸 시작 파일, 작업 공간의 `.git`(하위 폴더에 새로 만드는 `.git` 포함), `.claude`, `.mcp.json`(하위 폴더 포함), `CLAUDE.md`, `CLAUDE.local.md`, hook 폴더, 그리고 아래 denyWrite 경로 전부.
- push, pull, remote, config, commit-tree, update-ref, gh 쓰기는 두 프로필 모두 막는다.

환경 변수는 두 단계로 지운다. `kairos-sessions`의 `CLEARED_ENV`(`profile.rs`)에 있는 변수를 `env -u`로 시작 환경에서 빼고, `--settings`로 빈 값을 한 번 더 덮어쓴다. claude가 시작한 뒤에 `~/.claude/settings.json`의 `env`를 다시 넣기 때문이다. 이 동작은 가짜 claude로만 테스트했다. 차단 테스트 때 세션 안에서 `printenv`로 확인한다.

### 세션 토큰

세션의 GitHub 권한은 읽기 전용 fine-grained 토큰(`github_pat_`) 하나뿐이다.

- 세션을 처음 켜기 전에 GitHub에서 플레이북 대상 저장소만 선택한 fine-grained 토큰을 만든다. 권한은 Contents, Issues, Pull requests, Metadata 모두 Read-only다.
- `kairos connect session-token`에 stdin으로 넘기면 키체인에 저장된다.
- 키체인에 없으면 `<KAIROS_HOME>/session-gh-token` 파일을 읽는다. 이 파일은 본인 소유의 0600 일반 파일이고 토큰 한 줄만 들어 있어야 한다.
- 토큰이 없거나 형식이나 권한이 맞지 않으면 `kairos-run`은 claude를 띄우지 않고 고정 문구로 거절한다.
- classic(`ghp_`) 토큰과 gh 로그인 토큰(`gho_`)은 쓰기 범위가 함께 들어 있으므로 받지 않는다.

토큰은 두 곳으로 전달한다. 자식 환경의 `GH_TOKEN`과 `--settings` 파일의 env다. 설정 파일은 `<KAIROS_HOME>/session-settings/<run>.json`(0700 폴더 안의 0600 파일)이고 세션이 끝나면 지운다.

- `~/.claude/settings.json`의 env에 다른 `GH_TOKEN`이 있어도 `--settings`가 읽기 전용 토큰으로 다시 고정한다.
- 다른 사용자가 ps로 인자를 볼 수 있으므로 토큰은 인자에 넣지 않는다.
- 세션의 gh는 `GH_CONFIG_DIR=/var/empty`를 보므로 키체인의 로그인 정보를 찾지 않는다.

## 샌드박스

### 기본 설정

`--settings`는 다음을 함께 넘긴다.

- Bash 샌드박스: 쓰기는 작업 공간만 허용한다(연결된 worktree면 아래에서 설명하는 공용 `.git`의 막지 않은 부분도 허용). 네트워크는 `api.github.com`만 허용한다. 샌드박스 밖 재실행은 금지한다.
- git 자격 증명 끄기.
- `permissions.blockReadsOutsideWorkingDirectories`.
- 고치는 프로필은 `permissions.blockWritesOutsideWorkingDirectories`(claude 2.1.314 이상)로 Edit, Write의 작업 공간 밖 쓰기도 막는다. 이 설정을 모르는 버전에서도 차단이 유지되도록, 홈 폴더의 제어 경로(`~/.claude`, `~/Library/LaunchAgents`, `~/.local/bin`, git 설정, `~/.ssh`)와 셸 시작 파일은 Edit 거부로 한 번 더 막는다.

샌드박스의 denyRead는 Bash에만 적용된다. Read, Grep, Glob은 위 설정과 Read 거부 규칙으로 작업 공간 밖과 자격 증명 파일(`~/.ssh`, `~/.config/gh`, `~/Library/Keychains` 등)을 막는다. 접두어 방식의 거부 목록은 보조 방어선일 뿐 경계가 아니다.

### 고치는 세션의 git 쓰기

고치는 프로필의 커밋은 Orca worktree 안에서 이루어진다. 연결된 worktree의 git 폴더는 본 저장소의 `.git` 아래(`worktrees/<이름>`과 공용 objects, refs, logs)에 있어 작업 공간 밖이다. 다만 claude 샌드박스가 연결된 worktree의 공용 `.git` 쓰기를 허용하고 그 안의 `hooks`와 `config`는 막는다. 그래서 `allowWrite`는 따로 두지 않는다.

`kairos-run`은 세션을 띄우기 전에 git을 실행하지 않는다. 작업 공간(현재 폴더)의 `.git` 포인터와 `commondir`만 읽어서(`SessionWorkspace`) 다음 경로를 denyWrite에 추가한다.

- 작업 공간의 `.git` 포인터
- 공용 폴더의 `hooks`, `config`, `config.worktree`, `info`, `modules`, `objects/info`, `shallow`, `commondir`
- 자기 관리 폴더의 `commondir`, `gitdir`, `config.worktree`
- 다른 worktree의 관리 폴더
- 작업 공간의 `.husky`, `.githooks`, `.claude`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md`
- 저장소, 전역, 시스템 git 설정(include 포함)의 `core.hooksPath`가 가리키는 폴더와 include된 설정 파일

여기에 glob을 추가한다.

- 작업 공간 이름 여섯 개(`.husky`, `.githooks`, `.claude`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md`)와 git 설정이 가리키는 작업 공간 안의 대상(hook 폴더, include된 파일). 그 경로와 하위 전체가 대상이다.
- `.git`이 폴더가 아닌 경우(연결된 worktree의 포인터 파일) `<작업 공간>/.git`과 그 하위.
- 하위 폴더에 새로 만드는 `.git`(`<작업 공간>/*/**/.git`, `<작업 공간>/*/**/.git/**`).
- 모든 폴더의 `CLAUDE.md`, `CLAUDE.local.md`(`<작업 공간>/**/CLAUDE.md` 등)와 `.claude`, `.mcp.json`(`<작업 공간>/**/.claude`, `<작업 공간>/**/.claude/**`, `<작업 공간>/**/.mcp.json`).

glob은 다음과 같이 동작한다.

- 커밋은 포인터 파일에 기록하지 않는다. 그래서 `<작업 공간>/.git`을 막아도 연결된 worktree에서 커밋할 수 있다.
- 하위 `.git` glob은 본 checkout과 연결된 worktree 모두에 붙는다. `*/`로 시작하므로 작업 공간 바로 아래의 `.git`에는 일치하지 않는다. 따라서 본 checkout의 커밋은 그대로 `.git` 폴더에 쓸 수 있다.
- glob이 아닌 denyWrite 경로에는 그 경로와 하위 전체에 대한 Edit 거부가, glob에는 같은 glob의 Edit 거부가 붙는다. Bash와 Edit, Write가 같은 범위를 막게 된다.

### 대소문자와 유니코드

macOS의 APFS는 기본으로 대소문자를 구분하지 않는다. `.GIT`이나 `claude.md`도 git과 claude가 같은 파일로 열기 때문에, glob은 글자마다 `[gG]`처럼 두 경우를 모두 받는다.

APFS는 유니코드도 접어서 비교한다. `ſ`는 s로, 켈빈 기호 K는 k로 열린다. Seatbelt 정규식은 바이트 단위로 비교하므로 s와 k 자리는 `*`로 넓힌다(`.husky`는 `.[hH][uU]*[yY]`). claude 샌드박스는 glob을 Seatbelt 정규식으로 바꿀 때 닫힌 `[..]`를 문자 클래스 그대로 넘긴다.

### 막는 이유와 한계

- 위 경로는 모두 사용자가 나중에 샌드박스 밖에서 실행하는 git이나 claude가 읽어 코드 실행으로 이어질 수 있는 곳이다.
- claude 설정과 `CLAUDE.md`는 이 세션뿐 아니라 사용자가 나중에 여는 세션도 다시 읽는다. 하위 폴더의 `.claude`와 `.mcp.json`은 그 폴더에서 연 세션의 프로젝트 설정이 된다.
- 하위 폴더에 심어 둔 `.git`의 설정은 사용자가 그 폴더에서 git을 실행할 때 동작한다.
- git 설정 값을 해석할 수 없거나 작업 공간을 확인하지 못하면 고치는 세션은 뜨지 않는다.
- `.git`이라는 이름 없이 git 폴더 구조(`HEAD`, `objects`, `refs`)만 갖춘 bare 저장소는 이름으로 막을 수 없어 위험으로 남는다. 사용자가 전역 git 설정에 `safe.bareRepository=explicit`를 두면 git이 그런 폴더를 저장소로 열지 않는다.

세션 프롬프트는 이 제약(외부 쓰기 없음, 네트워크는 `api.github.com`뿐이라 패키지 설치 불가, 인터프리터와 셸 접두어 거부)을 먼저 알려 준다. 막히면 우회하지 말고 멈춰서 남은 일을 적게 한다.

## 세션 상태

claude가 끝나면 run을 `ended`로 기록한다. ps에 잡히기 전에 실패로 끝나면 `failed`로 내리고 항목을 되돌린다.

`kairos runs`가 `worktree ps`로 상태를 맞춘다. 패널은 `next --json`의 `runs.active`가 0보다 클 때 이 명령을 호출한다. 항목이 먼저 닫혀도 세션은 계속 추적한다. 수집 틱은 호출하지 않는다.

| ps | run |
|---|---|
| working | started |
| waiting, blocked | waiting_user, 준비됨 카드 `question` |
| done | waiting_user, 준비됨 카드 `done` |
| 터미널을 만든 뒤 30초 동안 보이지 않음 | needs_attention, 카드 `attention` |
| 한 번 보인 뒤 사라지고 터미널도 없음 | ended, 카드 `ended` |
| ps 실패, 잘린 응답 | 바꾸지 않음 |

- kairos-run이 시작하지 않은 채 5분이 지난 `needs_attention`과, 터미널 단계까지 가지 못한 채 5분이 지난 `claimed`는 `failed`로 내리고 항목을 되돌린다.
- 세션이 끝났다고 항목이 끝나는 것은 아니다.
- 이미 내려간 run은 다시 끝나도 항목을 건드리지 않는다.

### 준비됨 카드

준비됨 카드는 `next --json`의 `ready` 배열에 나온다.

- 예산과 무관하고 run마다 한 번만 나온다. 패널이 `--seen`으로 그리면 다시 나오지 않는다.
- 항목이 닫혀도 세션이 살아 있으면 나온다.
- 표시 게이트가 닫혀 있으면 `failed` 카드만 나온다.
- 백그라운드에서 실패한 세션은 `failed` 카드를 한 번 내고, 항목은 다음 묶음에 다시 올라온다.

### 세션 창 이동

- 세션 창은 `kairos accept`가 터미널을 만들 때 한 번 앞으로 나온다. 사용자가 버튼을 누른 순간이고, 자동 경로는 이 호출에 도달하지 않는다.
- 그 뒤로는 `kairos move <id>`만 사용자를 세션으로 이동시킨다. paneKey로 handle을 다시 찾아 `orca terminal switch`를 호출한다. 터미널이 없으면 run을 `ended`로 기록한다.
- `kairos accept <id> --cancel`은 항목을 되돌리지만 이미 뜬 세션을 끄지는 않는다. 끝났거나 확인이 필요한 세션의 항목에는 패널에 `되돌리기`로 나온다.

## PR 올리기

고치는 세션(qa_issue)은 샌드박스 안에서 로컬 커밋까지만 하고, PR은 제안만 한다. 첫 커밋의 제목이 PR 제목이 되고, 커밋 본문(여러 개면 커밋 목록)이 PR 본문이 된다.

세션이 끝나면 패널에 `PR 올리기`가 나온다. 누르면 `kairos pr <id>`가 세션 밖에서 사용자의 git 자격 증명과 gh 로그인으로 push하고, 플레이북 대상 저장소에 draft PR을 만든다. 브랜치는 run ID로 정한 `kairos/<worktree 이름>`이다.

- 확인한 커밋 SHA를 그대로 올리고, 저장소 훅과 replace ref는 끈다(`core.hooksPath=/dev/null`, `--no-replace-objects`).
- kairos-run이 띄운 세션만 올린다. 이어받은 세션은 올리지 않는다.
- 저장소나 worktree 설정이 git이 실행하는 명령이나 원격 주소를 바꾸면 올리지 않는다. origin이 대상 저장소가 아니어도 올리지 않는다.
- 원격에서 방금 받은 base와 비교해 `.github/` 같은 CI와 리뷰 설정을 바꾼 커밋은 올리지 않는다.
- 받은 base ref는 같은 클론의 다른 세션도 쓸 수 있는 공용 refs에 있다. 그래서 `git ls-remote`가 알려 준 SHA와 다르면 올리지 않는다.
- 본인이 연 PR이 이미 있으면 push만 한다.
- 커밋하지 않은 변경은 포함하지 않는다.
- 제목의 멘션, 이슈 참조, 링크는 무력화하고, 커밋 메시지는 본문의 코드 블록 안에만 싣는다.
- 세션 안에서는 호출할 수 없다.
