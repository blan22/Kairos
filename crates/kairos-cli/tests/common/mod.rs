//! 프로세스 시험의 도우미. 고정값은 모두 합성이다.
//!
//! 실제 바이너리를 띄운다. 자식 환경을 비우고 시작해 운영 값이 섞이지 않는다.
//! KAIROS_ORCA, KAIROS_CLAUDE, KAIROS_GH는 언제나 임시 폴더 안을 가리킨다. `find_binary`의 고정 폴더에 실제 orca와 claude가 있어
//! 값이 비면 진짜가 불린다. KAIROS_OPEN과 KAIROS_NOTIFIER도 같다. 비면 reveal이 실제 Orca를 앞으로 끌어온다. 알림 경로(`kairos collect`의 틱)는 이 하네스로 돌리지 않는다. `lib_collect.rs`가 라이브러리에서 본다.
#![allow(dead_code, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use kairos_agenda::{ControlFile, DisplayMode, Item, ItemEngine, ItemRepo, KairosControl, RefreshInput, StateBuilder};
use kairos_core::model::{
    ActorKind, EventType, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, SourceName, SourceResult, TrustTier,
};
use kairos_core::ports::{Clock, SystemClock};
use kairos_core::{Instant, KairosHome, OffDates};
use kairos_ledger::{IngestOptions, Ledger};
use kairos_store::Database;
use serde_json::{Value, json};
use tempfile::TempDir;

/// 시험 프로필(`Profile::fixture`)의 플레이북 대상.
pub const PLAYBOOK_ORG: &str = "contoso";
pub const PLAYBOOK_REPOS: [&str; 2] = ["atlas", "atlas-specs"];

pub const BIN: &str = env!("CARGO_BIN_EXE_kairos");
pub const RUN_BIN: &str = env!("CARGO_BIN_EXE_kairos-run");
pub const T0: &str = "2026-03-02T01:00:00.000Z"; // 월요일 10:00 KST
pub const T1: &str = "2026-03-02T01:03:00.000Z";
pub const BEFORE: &str = "2026-03-01T01:00:00.000Z";
pub const CHANNEL: &str = "C0SYNTH1";
pub const PANE: &str = "tab-synth:leaf-synth";

pub fn at(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

pub fn now() -> Instant {
    SystemClock.now()
}

pub fn issue_url(number: i64) -> String {
    format!("https://github.com/{PLAYBOOK_ORG}/{}/issues/{number}", PLAYBOOK_REPOS[0])
}

/// 끝난 프로세스.
#[derive(Debug)]
pub struct Done {
    pub status: i32,
    pub out: String,
    pub err: String,
}

impl Done {
    fn of(output: Output) -> Self {
        Self {
            status: output.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&output.stdout).into_owned(),
            err: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// 표준 출력의 JSON 한 덩어리.
    pub fn body(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|_| panic!("JSON이 아니다: {:?} {:?}", self.out, self.err))
    }
}

/// 임시 데이터 폴더 하나와 그 위의 kairos.
pub struct Cli {
    dir: TempDir,
    /// 데이터 폴더(`root`) 밖의 임시 폴더. 데이터 폴더 밖이어야 하는 worktree 같은 자리에 쓴다.
    outside: TempDir,
    pub root: PathBuf,
    pub env: BTreeMap<String, String>,
}

impl Cli {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let path = |name: &str| root.join(name).to_str().unwrap().to_owned();
        let env = BTreeMap::from([
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("HOME".to_owned(), root.to_str().unwrap().to_owned()),
            ("KAIROS_HOME".to_owned(), root.to_str().unwrap().to_owned()),
            ("KAIROS_ORCA".to_owned(), path("no-such-orca")),
            ("KAIROS_CLAUDE".to_owned(), path("no-such-claude")),
            ("KAIROS_GH".to_owned(), path("no-such-gh")),
            ("KAIROS_OPEN".to_owned(), path("no-such-open")),
            ("KAIROS_NOTIFIER".to_owned(), path("no-such-notifier")),
            ("KAIROS_SECURITY".to_owned(), path("no-such-security")),
        ]);
        write_fixture_config(&root);
        let outside = tempfile::tempdir().unwrap();
        Self { dir, outside, root, env }
    }

    pub fn home(&self) -> KairosHome {
        KairosHome::at(&self.root)
    }

    /// 소유자가 두는 읽기 전용 토큰 파일. kairos-run은 이 파일이 없으면 세션을 띄우지 않는다. 값은 합성이다.
    pub fn write_session_token(&self) {
        let path = self.home().file("session-gh-token");
        fs::write(&path, "github_pat_SYNTHETIC0123456789abcdef\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.env.insert(key.to_owned(), value.to_owned());
    }

    fn command(&self, binary: &str, extra: &[(&str, &str)]) -> Command {
        let mut command = Command::new(binary);
        command.env_clear().envs(&self.env).current_dir(&self.root);
        command.envs(extra.iter().copied());
        command
    }

    pub fn run(&self, args: &[&str]) -> Done {
        self.run_with(&[], args)
    }

    pub fn run_with(&self, extra: &[(&str, &str)], args: &[&str]) -> Done {
        self.refuse_real_tick(args);
        let mut command = self.command(BIN, extra);
        Done::of(command.args(args).stdin(Stdio::null()).output().unwrap())
    }

    /// 틱은 실제 osascript와 pmset을 부른다. 고정 경로라 PATH로 막지 못해서 여기서 멈춘다.
    fn refuse_real_tick(&self, args: &[&str]) {
        let flags = ["--resume", "--reread-notion", "--skip-slack-until", "--json"];
        let tick = args.first() == Some(&"collect") && !args.iter().any(|arg| flags.contains(arg));
        assert!(!tick, "틱은 lib_collect.rs에서 돌린다");
    }

    /// 표준 입력을 넘겨 부른다.
    pub fn pipe(&self, args: &[&str], input: &[u8]) -> Done {
        let mut child = self
            .command(BIN, &[])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        Done::of(child.wait_with_output().unwrap())
    }

    /// 인자 뒤에 --json을 붙인다.
    pub fn json(&self, args: &[&str]) -> Done {
        let mut all = args.to_vec();
        all.push("--json");
        self.run(&all)
    }

    pub fn db(&self) -> Database {
        Database::open(&self.home()).unwrap()
    }

    pub fn control(&self) -> KairosControl {
        ControlFile::new(&self.home()).read()
    }

    /// 세션을 켠 control이면 `kairos sessions on`처럼 설정 지문도 남긴다.
    pub fn write_control(&self, control: &KairosControl) {
        let home = self.home();
        if control.sessions_allowed() {
            kairos_agenda::SessionApproval::new(&home).record().unwrap();
        }
        ControlFile::new(&home).write(control).unwrap();
    }

    /// 파일 그대로의 control.json.
    pub fn control_json(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.home().control_path()).unwrap()).unwrap()
    }

    pub fn state(&self) -> Value {
        StateBuilder::read_state(&self.home()).unwrap()
    }

    /// 켠 표시와 세션. 수락까지 가는 시험의 바탕이다.
    pub fn open_sessions(&self) {
        let control = KairosControl::default().with_display_mode(DisplayMode::On).with_sessions(Some(T0));
        self.write_control(&control);
    }

    /// 표시를 켜고 세션은 잠근다.
    pub fn display_on(&self) {
        self.write_control(&KairosControl::default().with_display_mode(DisplayMode::On));
    }

    pub fn fake_orca(&mut self) -> FakeOrcaBin {
        let fake = FakeOrcaBin::write(&self.root);
        self.set("KAIROS_ORCA", fake.bin.to_str().unwrap());
        fake
    }

    /// kairos-run이 띄우기 직전에 읽는 gh. 조직의 사람이 연 댓글 없는 이슈 1을 답하고 받은 인자를 적는다.
    pub fn fake_gh(&mut self) -> PathBuf {
        let (bin, log) = (self.root.join("fake-gh"), self.root.join("gh-calls.log"));
        let issue = json!({ "data": { "repository": { "issueOrPullRequest": {
            "__typename": "Issue", "id": "I_synth", "number": 1,
            "authorAssociation": "MEMBER", "author": { "__typename": "User", "login": "synthetic-lead" },
            "comments": { "totalCount": 0, "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] },
        } } } });
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\ncase \"$1\" in\nauth) printf 'synthetic-token\\n' ;;\napi) cat > /dev/null; printf '%s\\n' '{issue}' ;;\n*) exit 1 ;;\nesac\n",
            log = log.display(),
        );
        write_script(&bin, &script);
        self.set("KAIROS_GH", bin.to_str().unwrap());
        log
    }

    /// 실행 파일과 나란한 설치본 구조로 복사한다. `<root>/bin/kairos`.
    pub fn install_binaries(&self) -> PathBuf {
        let bin = self.root.join("install").join("bin");
        fs::create_dir_all(&bin).unwrap();
        for (source, name) in [(BIN, "kairos"), (RUN_BIN, "kairos-run")] {
            fs::copy(source, bin.join(name)).unwrap();
        }
        bin
    }

    pub fn run_installed(&self, bin: &Path, args: &[&str]) -> Done {
        let binary = bin.join("kairos");
        let mut command = self.command(binary.to_str().unwrap(), &[]);
        Done::of(command.args(args).stdin(Stdio::null()).output().unwrap())
    }

    /// 설치 자료 폴더(`ops/`, `plugins/`, `profiles/`)와 그 안의 실행 파일. setup이 고르는 조직 프로필이 저장소의
    /// `profiles/`(git 무시)에 따라 달라지지 않게, 조직 프로필을 고르는 시험은 이 폴더의 kairos를 부른다.
    pub fn asset_bin(&self, profiles: &[(&str, &str)]) -> PathBuf {
        let assets = self.root.join("assets");
        for dir in ["ops", "plugins", "profiles", "bin"] {
            fs::create_dir_all(assets.join(dir)).unwrap();
        }
        for (name, content) in profiles {
            fs::write(assets.join("profiles").join(name), content).unwrap();
        }
        let bin = assets.join("bin");
        for (source, name) in [(BIN, "kairos"), (RUN_BIN, "kairos-run")] {
            fs::copy(source, bin.join(name)).unwrap();
        }
        bin
    }

    /// 설치 자료 폴더의 kairos에 표준 입력을 넘겨 부른다.
    pub fn pipe_installed(&self, bin: &Path, args: &[&str], input: &[u8]) -> Done {
        let binary = bin.join("kairos");
        let mut child = self
            .command(binary.to_str().unwrap(), &[])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        Done::of(child.wait_with_output().unwrap())
    }

    pub fn dir(&self) -> &Path {
        self.dir.path()
    }

    /// 데이터 폴더 밖의 임시 폴더. 실제 경로다.
    pub fn outside(&self) -> PathBuf {
        fs::canonicalize(self.outside.path()).unwrap()
    }
}

/// 합성 조직 프로필. `Profile::fixture`와 같은 값이다.
pub const EXAMPLE_ORG: &str = include_str!("../../../../examples/org.toml");

/// 시험 프로필(`Profile::fixture`)과 같은 값을 설정 파일로 쓴다. 실행 파일은 설정 파일에서 프로필을 읽는다.
pub fn write_fixture_config(root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let files = [
        ("org.toml", EXAMPLE_ORG),
        (
            "config.toml",
            "schema = 1\n[hosts.orca.repos]\n\"contoso/atlas\" = \"0000c0de-0000-4000-8000-000000000001\"\n",
        ),
        (
            "connections.toml",
            "schema = 1\n[identity]\ngithub = \"kairos-dev\"\ngoogle = \"kairos-dev@contoso.example\"\nslack = \"U0SYNTH0001\"\nname = \"김준서\"\n",
        ),
    ];
    for (name, content) in files {
        let path = root.join(name);
        fs::write(&path, content).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

pub fn write_script(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

/// 호출을 파일에 남기는 가짜 orca. 원본의 fakeOrcaBin과 같은 응답을 낸다.
pub struct FakeOrcaBin {
    pub bin: PathBuf,
    pub log: PathBuf,
}

/// 호출 기록 한 줄.
#[derive(Clone, Debug)]
pub struct OrcaCall {
    pub cwd: String,
    pub handle: String,
    pub args: Vec<String>,
}

impl FakeOrcaBin {
    fn write(root: &Path) -> Self {
        let (bin, log) = (root.join("fake-orca"), root.join("orca-calls.log"));
        let worktree = json!({
            "id": "repo-synth::/synthetic/worktrees/t-1",
            "identity": { "key": "wt-synth-key" },
            "path": "/synthetic/worktrees/t-1",
            "head": "abc123",
            "branch": "synthetic",
        });
        // 줄이 섞이지 않게 한 묶음으로 쓴다. 칸은 U+001F로 나눈다.
        let script = format!(
            r#"#!/bin/sh
line=$(printf '%s\037%s' "$(pwd -P)" "${{ORCA_TERMINAL_HANDLE-}}"; for arg in "$@"; do printf '\037%s' "$arg"; done)
printf '%s\n' "$line" >> '{log}'
case "$1 $2" in
'reference find') result='{{"matches":[],"truncated":false}}' ;;
'worktree create'|'worktree show') result='{{"worktree":{worktree}}}' ;;
'terminal create') result='{{"terminal":{{"handle":"term_made","tabId":"tab-synth","paneKey":"{PANE}"}}}}' ;;
'terminal list') result='{{"terminals":[{{"handle":"term_now","tabId":"tab-synth","leafId":"leaf-synth","connected":true}}]}}' ;;
'worktree ps') result='{{"worktrees":[],"truncated":false}}' ;;
'terminal switch') result='{{"focus":{{"handle":"term_now","tabId":"tab-synth"}}}}' ;;
*) printf '{{"ok":false,"error":{{"code":"unexpected"}}}}\n'; exit 1 ;;
esac
printf '{{"id":"req","ok":true,"result":%s,"_meta":{{}}}}\n' "$result"
"#,
            log = log.display(),
        );
        write_script(&bin, &script);
        Self { bin, log }
    }

    pub fn called(&self) -> bool {
        self.log.exists()
    }

    pub fn calls(&self) -> Vec<OrcaCall> {
        let text = fs::read_to_string(&self.log).unwrap_or_default();
        text.lines()
            .map(|line| {
                let mut parts = line.split('\u{1f}').map(str::to_owned);
                let cwd = parts.next().unwrap();
                let handle = parts.next().unwrap();
                OrcaCall { cwd, handle, args: parts.collect() }
            })
            .collect()
    }

    pub fn raw(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    pub fn count(&self, first: &str, second: &str) -> usize {
        self.calls().iter().filter(|call| call.args[0] == first && call.args[1] == second).count()
    }
}

pub fn run_result(source: SourceName, when: &str) -> SourceResult {
    SourceResult {
        source,
        query: "tick".into(),
        outcome: Outcome::Ok,
        started_at: when.into(),
        finished_at: when.into(),
        pages: 1,
        expected_count: None,
        parsed_count: 0,
        identity: None,
        window_from: None,
        window_to: None,
        watermark: Some(when.into()),
        progress_at: None,
        thread_reads: Vec::new(),
        overlap_seconds: 300,
        snapshot: false,
        error_code: None,
        objects: Vec::new(),
        events: Vec::new(),
    }
}

pub fn ingest(db: &Database, run: SourceResult, when: &str) {
    Ledger::new(db).ingest(run, &IngestOptions::at(at(when))).unwrap();
}

pub fn prime(db: &Database, source: SourceName, when: &str) {
    ingest(db, run_result(source, when), when);
}

pub fn push_objects(db: &Database, source: SourceName, when: &str, objects: Vec<NormalizedObject>) {
    ingest(db, SourceResult { objects, ..run_result(source, when) }, when);
}

pub fn push_events(db: &Database, source: SourceName, when: &str, events: Vec<NormalizedEvent>) {
    ingest(db, SourceResult { events, ..run_result(source, when) }, when);
}

pub fn issue(number: i64) -> NormalizedObject {
    NormalizedObject {
        text_head: Some(format!("합성 이슈 {number}")),
        state: Some("open".into()),
        assigned_to_me: Some(true),
        authored_by_me: Some(false),
        ..NormalizedObject::new(
            format!("gh:acme/widget#{number}"),
            SourceName::Github,
            ObjectKind::Issue,
            number.to_string(),
        )
    }
}

pub fn comment(id: &str, occurred_at: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some("synthetic-teammate".into()),
        object_key: Some("gh:acme/widget#1".into()),
        ..NormalizedEvent::new(
            format!("gh:comment:{id}"),
            SourceName::Github,
            EventType::GhComment,
            occurred_at,
            ActorKind::Human,
        )
    }
}

pub fn mention(ts: &str, occurred_at: &str) -> NormalizedEvent {
    let key = format!("slack:{CHANNEL}:{ts}");
    NormalizedEvent {
        actor_id: Some("U0SYNTH2".into()),
        thread_key: Some(key.clone()),
        mentions_me: Some(true),
        mention_count: Some(1),
        channel_id: Some(CHANNEL.into()),
        permalink: Some(format!("https://synthetic.example/archives/{CHANNEL}/p{ts}")),
        text_head: Some(format!("합성 멘션 {ts}")),
        ..NormalizedEvent::new(key, SourceName::Slack, EventType::SlackMention, occurred_at, ActorKind::Human)
    }
}

pub fn refresh(db: &Database, when: &str) {
    let (none, ok_sources) = (OffDates::new(), BTreeSet::new());
    let input = RefreshInput { now: at(when), off_dates: &none, ok_sources: &ok_sources, cards_held: false };
    ItemEngine::new(db).refresh(&input).unwrap();
}

pub fn item(db: &Database, id: i64) -> Item {
    ItemRepo::new(db.connection()).get(id).unwrap().unwrap()
}

pub fn items(db: &Database) -> Vec<Item> {
    let ids: Vec<i64> = {
        let mut statement = db.connection().prepare("SELECT id FROM items ORDER BY id").unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
    };
    ids.into_iter().map(|id| item(db, id)).collect()
}

pub fn count(db: &Database, table: &str) -> i64 {
    db.connection().query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

/// 원본의 homeWithItem. 지금 시각의 Slack 멘션으로 열린 항목 하나가 있다.
pub fn cli_with_item() -> Cli {
    let cli = Cli::new();
    let db = cli.db();
    let moment = now();
    let (before, at_now) = (moment.minus_ms(1000).to_iso(), moment.to_iso());
    prime(&db, SourceName::Slack, &before);
    let mut event = mention("1", &at_now);
    event.text_head = Some("\u{1b}[31m합성 멘션\u{1b}[0m".into());
    push_events(&db, SourceName::Slack, &at_now, vec![event]);
    refresh(&db, &at_now);
    drop(db);
    cli
}

/// 시작 버튼이 나오는 담당 이슈 하나를 연다. 원본의 seedIssue.
pub fn seed_issue(db: &Database, title: Option<&str>) {
    prime(db, SourceName::Slack, BEFORE);
    prime(db, SourceName::Github, BEFORE);
    let mut object = issue(1);
    object.permalink = Some(issue_url(1));
    object.trust_tier = Some(TrustTier::T1);
    if let Some(title) = title {
        object.text_head = Some(title.to_owned());
    }
    let mut event = comment("c1", T0);
    event.trust_tier = Some(TrustTier::T1);
    push_objects(db, SourceName::Github, T0, vec![object]);
    push_events(db, SourceName::Github, T0, vec![event]);
    refresh(db, T0);
}

/// 원본의 homeWithIssue. 표시와 세션이 켜진 폴더에 담당 이슈가 있다.
pub fn cli_with_issue() -> Cli {
    let cli = Cli::new();
    seed_issue(&cli.db(), None);
    cli.open_sessions();
    cli
}
