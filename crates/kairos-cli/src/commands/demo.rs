//! `kairos demo`. 계정 없이 kairos를 보는 합성 데이터 폴더를 만든다.
//!
//! 외부 서비스, Orca, gh, claude를 부르지 않는다. 비어 있는 새 폴더에만 쓰고, `KAIROS_HOME`을 직접 준 때만 돈다.
//! 기본 데이터 폴더에 합성 기록이 섞이면 실제 수집과 구분할 수 없기 때문이다.

use std::collections::BTreeSet;
use std::fs;

use kairos_agenda::{DisplayMode, ItemEngine, KairosControl, RefreshInput};
use kairos_core::home::write_file_atomic;
use kairos_core::model::{
    ActorKind, EventType, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, SourceName, SourceResult, TrustTier,
};
use kairos_core::{Instant, KairosError, KairosResult, OffDates};
use kairos_ledger::{IngestOptions, Ledger};
use kairos_store::Database;
use serde_json::json;

use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag};
use crate::command::Command;
use crate::reply::Reply;

const OPTIONS: [OptionSpec; 1] = [flag("json")];

/// 합성 조직 프로필. 시험 프로필과 같은 값이다.
const ORG: &str = include_str!("../../../../examples/org.toml");
/// 하루 종일, 매일 카드를 낸다. 데모를 밤이나 주말에 열어도 비어 보이지 않게 한다.
const CONFIG: &str = "schema = 1\n\n[schedule]\ndays = [\"mon\", \"tue\", \"wed\", \"thu\", \"fri\", \"sat\", \"sun\"]\nwork = [\"00:00\", \"24:00\"]\ncollect = [\"00:00\", \"24:00\"]\nlunch = []\n";
const CONNECTIONS: &str = "schema = 1\n\n[identity]\ngithub = \"demo-user\"\ngoogle = \"demo-user@contoso.example\"\nslack = \"U0SYNTHDEMO\"\nname = \"김준서\"\n";
const CHANNEL: &str = "C0SYNTHDEMO";
const REPO: &str = "contoso/atlas";

pub struct Demo;

impl Command for Demo {
    fn name(&self) -> &'static str {
        "demo"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: KAIROS_HOME=<비어 있는 새 폴더> kairos demo [--json]")
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        // 데이터 폴더는 공백을 떼고 비면 기본 폴더로 간다(`KairosHome::from_env`). 같은 기준으로 본다.
        let given = context.env("KAIROS_HOME").is_some_and(|value| !kairos_core::js::trim(value).is_empty());
        if !given {
            let message = "데모는 KAIROS_HOME에 비어 있는 새 폴더를 줄 때만 만듭니다. 예: KAIROS_HOME=\"$(mktemp -d)/kairos\" kairos demo";
            return Ok(reply.fail("demo_home_required", message, 2));
        }
        let home = context.home();
        // 없는 폴더만 비어 있다고 본다. 읽지 못하는 폴더는 무엇이 들었는지 모르므로 쓰지 않는다.
        let occupied = match fs::read_dir(home.root()) {
            Ok(mut entries) => entries.next().is_some(),
            Err(error) => error.kind() != std::io::ErrorKind::NotFound,
        };
        if occupied {
            let message = "데이터 폴더가 비어 있지 않습니다. 데모는 새 폴더에만 씁니다";
            return Ok(reply.fail("demo_home_not_empty", message, 1));
        }
        home.ensure()?;
        for (name, content) in [("org.toml", ORG), ("config.toml", CONFIG), ("connections.toml", CONNECTIONS)] {
            write_file_atomic(&home.file(name), content)?;
        }
        context.control_file().write(&KairosControl::default().with_display_mode(DisplayMode::On))?;
        let now = context.now();
        let database = context.open_db()?;
        seed(&database, now)?;
        let ok_sources: BTreeSet<String> =
            [SourceName::Github, SourceName::Slack].iter().map(|source| source.as_str().to_owned()).collect();
        let off_dates = OffDates::new();
        let input = RefreshInput { now, off_dates: &off_dates, ok_sources: &ok_sources, cards_held: false };
        let report = ItemEngine::new(&database).refresh(&input)?;
        drop(database);
        context.refresh_state()?;
        let root = home.root().display().to_string();
        if reply.json() {
            return reply.success(json!({ "home": root, "opened": report.opened }));
        }
        let console = context.console();
        console.print(&format!("합성 데이터로 데모 폴더를 만들었습니다: {root}"));
        console.print(&format!("열린 항목 {}건. 외부 서비스에는 접속하지 않았습니다", report.opened));
        console.print("같은 KAIROS_HOME으로 보세요:");
        for command in ["next", "list", "doctor"] {
            console.print(&format!("  KAIROS_HOME=\"{root}\" kairos {command}"));
        }
        console.print("수집을 돌리지 않으므로 상태 줄에 '수집 기록 없음'이 나옵니다");
        console.print("지우려면 폴더를 지웁니다. 수집(collect)과 세션은 이 폴더에서 돌리지 않습니다");
        Ok(0)
    }
}

/// 소스마다 기준선 실행을 먼저 남기고, 그 뒤의 사건을 넣는다. 첫 실행의 객체와 사건은 기준선이라 항목이 되지 않는다.
fn seed(database: &Database, now: Instant) -> KairosResult<()> {
    let (baseline, recent) = (now.minus_ms(10 * 60 * 1000).to_iso(), now.minus_ms(5 * 60 * 1000).to_iso());
    let ledger = Ledger::new(database);
    let ingest = |run: SourceResult, at: &str| -> KairosResult<()> {
        let when = Instant::parse(at).ok_or_else(|| KairosError::new("demo_time"))?;
        ledger.ingest(run, &IngestOptions::at(when)).map(|_| ())
    };
    for source in [SourceName::Github, SourceName::Slack] {
        ingest(run(source, &baseline), &baseline)?;
    }
    let pull = |number: i64, title: &str, mine: bool| NormalizedObject {
        text_head: Some(title.to_owned()),
        permalink: Some(format!("https://github.com/{REPO}/pull/{number}")),
        state: Some("open".to_owned()),
        authored_by_me: Some(mine),
        assigned_to_me: Some(false),
        review_requested_me: Some(!mine),
        trust_tier: Some(TrustTier::T1),
        ..NormalizedObject::new(
            format!("gh:{REPO}#{number}"),
            SourceName::Github,
            ObjectKind::PullRequest,
            number.to_string(),
        )
    };
    let issue = NormalizedObject {
        text_head: Some("합성 이슈: 결제 화면 문구 오류".to_owned()),
        permalink: Some(format!("https://github.com/{REPO}/issues/7")),
        state: Some("open".to_owned()),
        assigned_to_me: Some(true),
        authored_by_me: Some(false),
        trust_tier: Some(TrustTier::T1),
        ..NormalizedObject::new(format!("gh:{REPO}#7"), SourceName::Github, ObjectKind::Issue, "7")
    };
    let objects = vec![pull(42, "합성 PR: 장바구니 정리", false), pull(51, "합성 PR: 로그인 오류 수정", true), issue];
    let github = |id: &str, event_type: EventType, object: i64| NormalizedEvent {
        actor_id: Some("synthetic-teammate".to_owned()),
        object_key: Some(format!("gh:{REPO}#{object}")),
        trust_tier: Some(TrustTier::T1),
        ..NormalizedEvent::new(
            format!("gh:demo:{id}"),
            SourceName::Github,
            event_type,
            recent.as_str(),
            ActorKind::Human,
        )
    };
    let events = vec![
        github("review-42", EventType::GhReviewRequested, 42),
        github("changes-51", EventType::GhChangesRequested, 51),
        github("comment-7", EventType::GhComment, 7),
    ];
    ingest(SourceResult { objects, events, ..run(SourceName::Github, &recent) }, &recent)?;
    let key = format!("slack:{CHANNEL}:1700000000.000100");
    let mention = NormalizedEvent {
        actor_id: Some("U0SYNTHPEER".to_owned()),
        thread_key: Some(key.clone()),
        mentions_me: Some(true),
        mention_count: Some(1),
        channel_id: Some(CHANNEL.to_owned()),
        permalink: Some(format!("https://synthetic.example/archives/{CHANNEL}/p1700000000000100")),
        text_head: Some("합성 멘션: 오늘 배포 전에 확인 부탁드려요".to_owned()),
        trust_tier: Some(TrustTier::T1),
        ..NormalizedEvent::new(key, SourceName::Slack, EventType::SlackMention, recent.as_str(), ActorKind::Human)
    };
    ingest(SourceResult { events: vec![mention], ..run(SourceName::Slack, &recent) }, &recent)
}

/// 정상으로 끝난 한 번의 실행.
fn run(source: SourceName, at: &str) -> SourceResult {
    SourceResult {
        source,
        query: "demo".into(),
        outcome: Outcome::Ok,
        started_at: at.into(),
        finished_at: at.into(),
        pages: 1,
        expected_count: None,
        parsed_count: 0,
        identity: None,
        window_from: None,
        window_to: None,
        watermark: Some(at.into()),
        progress_at: None,
        thread_reads: Vec::new(),
        overlap_seconds: 300,
        snapshot: false,
        error_code: None,
        objects: Vec::new(),
        events: Vec::new(),
    }
}
