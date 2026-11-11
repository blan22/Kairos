# 수락과 세션

`kairos accept <id>`는 멱등이다. 표시 게이트가 닫혀 있으면 `gate_closed`, 세션 잠금이 있으면 `not_eligible`과 URL만 준다. 세션이나 Orca 터미널 안에서 부르면 `session_caller`로 거절한다. 패널이 부르는 경로다.

세션 잠금은 차단 시험(S3, S4) 전에 세션이 열리는 일을 막는다. 시험을 본 뒤 사용자가 `kairos sessions on`으로 켠다. 끄면 시작 버튼이 사라진다. 상태는 `doctor`의 `세션` 줄에 나온다. 켤 때의 설정 지문이 `session-approval`에 남고, 세션 자격에 닿는 설정이 바뀌면 다시 잠긴다. 세션 바닥값(거부 목록, 샌드박스)을 바꾸면 `kairos-config`의 `SESSION_FLOOR_VERSION`을 올려 모두 다시 잠근다.

`BEGIN IMMEDIATE`로 run을 `claimed`로 쓰고 항목을 `accepted`로 바꾼다. 진 쪽은 기존 run을 받는다. 세션은 이슈마다 하나라서 같은 이슈의 다른 항목이 연 세션이 있으면 그 세션으로 보낸다. 이어서 다음 순서로 Orca를 부른다. 사이에 사용자가 끝이나 취소를 누르면 그 결과를 돌려주고 멈춘다.

1. `orca reference find <url>`. 기록으로 연결된(`linked`) 살아 있는 에이전트가 있으면 그 세션을 붙이고(`adopted`) 끝낸다.
2. `orca worktree create --repo id:<대상 저장소> --name t-<짧은id> --no-parent --setup skip --base-branch origin/fe-stage --reference <url>`
3. `orca worktree show`로 경로와 HEAD를 확인하고 선택자를 run에 저장한다.
4. `orca terminal create --worktree <선택자> --title kairos-<짧은id> --command '<kairos-run>' <uuid> --focus`

`--pr`, `orca agent launch`, `worktree create --agent`, `orca terminal send`는 쓰지 않는다. `--focus`는 `kairos accept`의 `terminal create`에만 있다. 사용자가 패널에서 누른 순간이라 세션 창을 보여 준다. orca는 데이터 폴더를 cwd로 하고 `ORCA_TERMINAL_HANDLE` 같은 호출자 값을 지운 뒤 인자 배열로 부른다. `KAIROS_ORCA`로 실행 파일을 바꿀 수 있다. `--command`에 node 절대 경로가 있는 이유는 터미널의 `node`가 worktree의 버전 고정을 따르기 때문이다.

대상은 `playbook_of`다. 객체 조건(`playbook_for`: 열린 이슈나 PR, T0나 T1이 연 것, 매핑표의 저장소, URL 정규식)에 더해 kairos가 본 그 이슈의 사람 글이 모두 T0, T1이어야 한다. 한 번 본 T2, T3 사람 글은 `outsider_marks`에 남아 보존 기한과 정비가 지나도 계속 막는다. 처음 읽을 때 최근 20개 밖에 있던 글과 마지막 수집 뒤의 글은 `kairos-run`이 띄우기 직전에 다시 본다. 그 밖의 항목은 `not_eligible`과 표시용 URL만 받고 Orca를 부르지 않는다. 제안은 이슈마다 하나이고 플레이북이 있는 첫 열린 항목에 붙는다. 세션이 없다는 조건도 더 붙는다. 시작 버튼은 제안에 세션 잠금이 풀렸다는 조건이 더 붙는다. 잠겨 있으면 목록의 `playbook`과 `scope`만 나오고 `start`는 없다.

어떤 일을 맡기는지는 항목이 열린 이유로 고른다. `qa_issue`는 고치는 프로필로 돈다. 조사와 계획 뒤 그 worktree에서 코드를 고치고 로컬에 커밋한다. 나머지는 읽는 프로필로 돌고 결과는 세션 안에 남는다. 세션이 외부에 쓰는(push, PR, 코멘트, Notion) 플레이북은 없다. 고치는 세션의 커밋은 사용자가 `PR 올리기`를 누를 때만 세션 밖에서 올라간다(아래). 수락 때 고른 값이 run의 `playbook` 열에 남고 `kairos-run`은 이 값으로 프로필과 프롬프트를 고른다.

| 플레이북 | 대상 | 세션이 하는 일 |
|---|---|---|
| `qa_issue` | 내 담당 이슈 | 원인 조사, 수정 계획, 수정과 로컬 커밋 |
| `pr_review` | 리뷰를 요청받은 PR | 리뷰 의견 초안 |
| `reply_draft` | 답장 필요가 된 이슈 | 답변 초안 |

Slack, Gmail, Notion, Calendar 항목은 제안하지 않는다. 세션이 그 소스를 읽을 길이 없고(`--strict-mcp-config`), 본문을 저장하지 않으므로 넘길 값도 없다. 내 PR의 CI 실패와 변경 요청, PR의 글도 제안하지 않는다. `gh run`과 `gh api`가 막혀 있어 CI 로그와 인라인 리뷰 글을 읽을 수 없다.

`terminal create`는 재시도 ID가 없어 다시 부르지 않는다. 응답을 잃으면 `visualLayouts`의 탭 제목과 `worktree ps`로 찾고, 못 찾으면 `needs_attention`으로 둔다. `kairos-run`은 run마다 한 번만 뜨므로 늦게 도착한 명령이 세션을 하나 더 만들지 못한다.

`kairos-run <uuid>`는 UUID만 받는다. 이슈 URL은 DB에서 읽고 시작 버튼과 같은 조건을 다시 본다. 이어서 gh GraphQL로 그 이슈나 PR의 본문, 댓글, 리뷰, 리뷰 스레드 글의 작성자를 끝까지 읽는다. T0, T1이 아닌 사람 글이 있으면 `outsider_writing`, 다 읽었다고 보일 수 없으면(gh 실패, 시간 초과, 수 불일치, 등급을 정할 수 없는 작성자) `live_unverified`로 run을 실패로 내리고 항목을 되돌린다. 통과하면 고정 프롬프트에 넣는다. 봇 글은 세지 않는다. claude는 `--session-id <uuid>`, `--permission-mode auto`, `--strict-mcp-config`, `--settings`, `--tools`, `--disallowedTools`로 뜬다. 고치는 프로필은 `--setting-sources user`를 더한다. 프로젝트와 로컬 설정(`.claude/settings.json`, `.claude/settings.local.json`)의 훅은 샌드박스 밖에서 돌고, 고치는 세션은 그 훅이 부르는 작업 공간의 파일을 고칠 수 있기 때문이다. 사용자 설정은 남는다. Orca 상태 보고 훅이 거기 있고, 없으면 `worktree ps`가 세션을 못 본다. 읽는 프로필은 작업 공간을 고치지 못하므로 `--setting-sources`를 넘기지 않는다. `--tools`는 읽는 프로필이 `Bash,Read,Grep,Glob`, 고치는 프로필이 여기에 `Edit,Write`를 더한 값이다. 고치는 프로필의 거부 목록은 읽는 프로필에서 `git commit`과 Edit, Write만 빠지고, 데이터 폴더, 설치 폴더, 홈의 제어 평면과 셸 시작 파일, 작업 공간의 `.git`(하위 폴더에 새로 만드는 `.git` 포함), `.claude`, `.mcp.json`(하위 폴더 포함), `CLAUDE.md`, `CLAUDE.local.md`, hook 폴더와 아래 denyWrite 경로마다의 Edit 거부가 붙는다. push, pull, remote, config, commit-tree, update-ref, gh 쓰기는 그대로 막힌다. `src/session.ts`의 `CLEARED_ENV`는 `env -u`로 시작 환경에서 빼고, `--settings`로 빈 값을 덮는다. claude가 `~/.claude/settings.json`의 `env`를 시작 뒤에 다시 넣기 때문이다. 이 동작은 가짜 claude로만 시험했고 S3에서 세션 안 `printenv`로 확인해야 한다.

세션의 GitHub 권한은 소유자가 `<KAIROS_HOME>/session-gh-token`에 둔 읽기 전용 fine-grained 토큰(`github_pat_`) 하나뿐이다. 파일은 본인 소유의 0600 일반 파일이고 토큰 한 줄이어야 한다. 이 파일이 없거나 형식, 권한이 맞지 않으면 `kairos-run`은 claude를 띄우지 않고 고정 문구로 거절한다. classic(`ghp_`)과 gh 로그인 토큰(`gho_`)은 쓰기 범위가 함께 있어 받지 않는다. 토큰은 자식 환경의 `GH_TOKEN`과 `<KAIROS_HOME>/session-settings/<run>.json`(0700 폴더의 0600 파일, 세션이 끝나면 지움)으로 넘기는 `--settings` 파일의 env 두 곳으로 간다. `~/.claude/settings.json`의 env에 다른 `GH_TOKEN`이 있어도 `--settings`가 읽기 전용 토큰으로 다시 고정한다. 다른 사용자가 ps로 인자를 볼 수 있어 토큰은 인자에 넣지 않는다. 세션의 gh는 `GH_CONFIG_DIR=/var/empty`를 보므로 키체인 로그인을 찾지 않는다.

`--settings`는 Bash 샌드박스(쓰기는 작업 공간뿐이고 연결된 worktree면 아래처럼 공용 `.git`의 막지 않은 곳도, 네트워크는 `api.github.com`뿐, 샌드박스 밖 재실행 금지), git 자격 증명 끄기, `permissions.blockReadsOutsideWorkingDirectories`를 함께 넘긴다. 고치는 프로필은 `permissions.blockWritesOutsideWorkingDirectories`(claude 2.1.314 이상)로 Edit, Write의 작업 공간 밖 쓰기도 막는다. 그 설정을 모르는 판에서도 남도록 홈의 제어 평면(`~/.claude`, `~/Library/LaunchAgents`, `~/.local/bin`, git 설정, `~/.ssh`)과 셸 시작 파일은 Edit 거부로 한 번 더 막는다. 샌드박스 denyRead는 Bash에만 걸리고, Read, Grep, Glob은 이 설정과 Read 거부 규칙이 작업 공간 밖과 자격 증명 파일(`~/.ssh`, `~/.config/gh`, `~/Library/Keychains` 등)을 막는다. 접두어 거부 목록은 겹일 뿐 경계가 아니다.

고치는 프로필의 커밋은 Orca worktree 안에서 일어난다. 연결된 worktree의 git 폴더는 본 저장소의 `.git` 아래(`worktrees/<이름>`과 공용 objects, refs, logs)에 있어 작업 공간 밖이지만, claude 샌드박스가 연결된 worktree의 공용 `.git` 쓰기를 열고 그 안의 `hooks`, `config`는 막는다. 그래서 `allowWrite`는 두지 않는다. `kairos-run`은 띄우기 전에 git을 부르지 않고 작업 공간(현재 폴더)의 `.git` 포인터와 `commondir`만 읽어(`SessionWorkspace`) denyWrite를 더한다. 작업 공간의 `.git` 포인터, 공용 폴더의 `hooks`, `config`, `config.worktree`, `info`, `modules`, `objects/info`, `shallow`, `commondir`, 자기 관리 폴더의 `commondir`, `gitdir`, `config.worktree`, 다른 worktree의 관리 폴더, 작업 공간의 `.husky`, `.githooks`, `.claude`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md`, 저장소와 전역, 시스템 git 설정(include까지)의 `core.hooksPath`가 가리키는 폴더와 include된 설정 파일이다. 이어서 glob을 더한다. macOS의 APFS는 기본으로 대소문자를 가리지 않아 `.GIT`, `claude.md`도 git과 claude가 같은 이름으로 열므로, glob은 글자마다 `[gG]`처럼 두 경우를 받는다. APFS는 유니코드로도 접어 `ſ`를 s로, 켈빈 기호 K를 k로 열고 Seatbelt 정규식은 바이트로 비교하므로, s와 k 자리는 `*`로 넓힌다(`.husky`는 `.[hH][uU]*[yY]`). claude 샌드박스는 glob을 Seatbelt 정규식으로 바꿀 때 닫힌 `[..]`를 글자 묶음 그대로 넘긴다. 작업 공간 이름 여섯(`.husky`, `.githooks`, `.claude`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md`)과 git 설정이 가리키는 작업 공간 안 대상(hook 폴더, include된 파일)의 그 자리와 아래, `.git`이 폴더가 아니면(연결된 worktree의 포인터 파일) `<작업 공간>/.git`과 아래, 하위 폴더에 새로 만드는 `.git`(`<작업 공간>/*/**/.git`, `<작업 공간>/*/**/.git/**`), 어느 폴더의 `CLAUDE.md`, `CLAUDE.local.md`(`<작업 공간>/**/CLAUDE.md` 등)와 `.claude`, `.mcp.json`(`<작업 공간>/**/.claude`, `<작업 공간>/**/.claude/**`, `<작업 공간>/**/.mcp.json`)이다. 포인터 파일은 커밋이 쓰지 않는다. 하위 `.git` glob은 본 checkout과 연결된 worktree 모두에 붙는다. `*/`로 시작해 작업 공간 바로 아래의 `.git`에는 맞지 않으므로 본 checkout의 커밋은 그대로 `.git` 폴더에 쓴다. glob이 아닌 denyWrite 경로는 모두 그 자리와 아래의 Edit 거부가, glob은 같은 glob의 Edit 거부가 붙어 Bash와 Edit, Write가 같은 곳을 막는다. `.git`이라는 이름 없이 git 폴더 모양(`HEAD`, `objects`, `refs`)만 둔 bare 저장소는 이름으로 막을 수 없어 남는다. 사용자가 전역 git 설정에 `safe.bareRepository=explicit`를 두면 git이 그런 폴더를 저장소로 열지 않는다. claude 설정과 `CLAUDE.md`는 이 세션과 사용자가 나중에 여는 세션이 다시 읽고(하위 폴더의 `.claude`, `.mcp.json`은 그 폴더에서 연 세션의 프로젝트 설정이 된다), 심어 둔 하위 `.git`의 설정은 사용자가 그 폴더에서 git을 부를 때 돈다. 설정 값을 풀 수 없으면 고치는 세션은 뜨지 않는다. 모두 사용자가 나중에 샌드박스 밖에서 돌리는 git이 읽어 실행으로 이어지는 곳이다. 작업 공간을 확인하지 못하면 고치는 세션은 뜨지 않는다. 세션 프롬프트는 이 한계(외부 쓰기 없음, 네트워크는 `api.github.com`뿐이라 패키지 설치 불가, 해석기와 셸 접두어 거부)를 먼저 말하고, 막히면 돌아가지 말고 멈춰 남은 일을 적게 한다.

세션을 처음 켜기 전에 GitHub에서 플레이북 대상 저장소만 고른 fine-grained 토큰을 만든다. 권한은 Contents, Issues, Pull requests, Metadata 모두 Read-only다. `kairos connect session-token`에 stdin으로 넘기면 키체인에 들어간다. 키체인에 없으면 `<KAIROS_HOME>/session-gh-token`(0600)을 본다. claude가 끝나면 run을 `ended`로 적는다. ps가 보기 전에 실패로 끝나면 `failed`로 내리고 항목을 되돌린다.

`kairos runs`가 `worktree ps`로 상태를 맞춘다. 패널은 `next --json`의 `runs.active`가 0보다 클 때 부른다. 항목이 먼저 닫혀도 세션을 계속 본다. 수집 틱은 부르지 않는다.

| ps | run |
|---|---|
| working | started |
| waiting, blocked | waiting_user, 준비됨 카드 `question` |
| done | waiting_user, 준비됨 카드 `done` |
| 터미널 뒤 30초 동안 없음 | needs_attention, 카드 `attention` |
| 본 뒤에 사라지고 터미널도 없음 | ended, 카드 `ended` |
| ps 실패, 잘린 응답 | 바꾸지 않음 |

kairos-run이 시작하지 않은 채 5분이 지난 `needs_attention`과 터미널 단계까지 못 간 채 5분이 지난 `claimed`는 `failed`로 내리고 항목을 되돌린다. 세션 종료는 항목 종료가 아니다. 이미 내려간 run은 다시 끝나도 항목을 건드리지 않는다.

준비됨 카드는 `next --json`의 `ready` 배열에 나온다. 예산 밖이고 run마다 1회다. 패널이 `--seen`으로 그리면 다시 나오지 않는다. 항목이 닫혀도 세션이 살아 있으면 나온다. 표시 게이트가 닫히면 `failed` 카드만 나온다. 뒤에서 실패한 세션은 `failed` 카드 1회와 함께 항목이 다음 묶음에 다시 오른다.

세션 창은 `kairos accept`가 터미널을 만들 때 한 번 앞으로 온다. 사용자가 누른 순간이고 자동 경로는 여기에 닿지 않는다. 그 뒤로는 `kairos move <id>`만 사용자를 세션으로 옮긴다. paneKey로 handle을 다시 찾아 `orca terminal switch`를 부른다. 터미널이 없으면 run을 `ended`로 적는다. `kairos accept <id> --cancel`은 항목을 되돌리지만 뜬 세션을 끄지는 않는다. 끝났거나 확인이 필요한 세션의 항목에는 패널에 `되돌리기`로 나온다.

고치는 세션(qa_issue)은 샌드박스 안에서 로컬 커밋까지만 하고 PR을 제안만 한다. 첫 커밋 제목이 PR 제목, 커밋 본문(여럿이면 커밋 목록)이 PR 본문이 된다. 세션이 끝나면 패널에 `PR 올리기`가 나온다. 누르면 `kairos pr <id>`가 세션 밖에서 사용자의 git 자격 증명과 gh 로그인으로 run ID로 정한 `kairos/<worktree 이름>` 브랜치로 push하고 플레이북 대상 저장소에 draft PR을 만든다. 확인한 커밋 SHA를 그대로 올리고 저장소 훅과 replace ref는 끈다(`core.hooksPath=/dev/null`, `--no-replace-objects`). kairos-run이 띄운 세션만 올리고 이어받은 세션은 올리지 않는다. 저장소나 worktree 설정이 git이 부르는 명령이나 원격 주소를 바꾸거나 origin이 대상 저장소가 아니면 올리지 않는다. 원격에서 방금 받은 base와 비교해 `.github/` 같은 CI와 리뷰 설정을 바꾼 커밋은 올리지 않는다. 받은 base ref는 같은 클론의 다른 세션도 쓸 수 있는 공용 refs에 있으므로, `git ls-remote`가 알려 준 SHA와 다르면 올리지 않는다. 본인이 연 PR이 이미 있으면 push만 한다. 커밋하지 않은 변경은 들지 않는다. 제목의 멘션, 이슈 참조, 링크는 무력화하고 커밋 글은 본문의 코드 블록에만 싣는다. 세션 안에서는 부르지 못한다.
