//! `kairos collect`의 틱은 실제 알림 경로라서 바이너리로 돌리지 않는다. 알림 헬퍼와 패널 명령은 PATH로 막지 못한다.
//! 여기서는 같은 명령을 `AppContext`로 돌리되 알림과 패널, 프로세스 실행기를 대역으로 준다.
//! 대역 실행기는 알림과 패널 명령이 오면 시험을 깨뜨린다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::sync::{Arc, Mutex};

use kairos_agenda::{ControlFile, DisplayMode, ItemRepo, KairosControl, P0_TITLE, StateBuilder};
use kairos_cli::{AppContext, Console, registry};
use kairos_core::model::{
    ActorKind, CollectContext, DirectSource, EventType, NormalizedEvent, NormalizedObject, ObjectKind, Outcome,
    QuerySpec, Source, SourceMode, SourceName, SourceResult,
};
use kairos_core::ports::{Alert, FixedClock, Notifier, PanelOpener};
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_core::{Instant, KairosHome};
use kairos_ledger::{IngestOptions, Ledger};
use kairos_store::Database;
use kairos_tick::{SleepProbe, SourceRegistry};
use tempfile::TempDir;

const BEFORE: &str = "2026-03-01T01:00:00.000Z";
const T1: &str = "2026-03-02T01:03:00.000Z"; // 월요일 10:03 KST
const T2: &str = "2026-03-02T01:06:00.000Z";

fn at(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

/// 외부 프로세스가 오면 깨진다. 이 시험의 수집은 프로세스를 띄울 일이 없다.
#[derive(Default)]
struct NoProcess;

impl ProcessRunner for NoProcess {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        assert_ne!(request.program, "/usr/bin/osascript", "osascript 알림은 쓰지 않는다");
        assert!(request.args.first().map(String::as_str) != Some("post"), "실제 알림 헬퍼를 부르면 안 된다");
        assert!(request.args.first().map(String::as_str) != Some("plugin"), "실제 패널을 부르면 안 된다");
        ProcessOutput::ended(ProcessEnd::Missing)
    }
}

#[derive(Default)]
struct Effects {
    messages: Mutex<Vec<String>>,
    alerts: Mutex<Vec<Alert>>,
    panels: Mutex<usize>,
}

impl Notifier for Effects {
    fn notify(&self, alert: &Alert) -> bool {
        self.messages.lock().unwrap().push(alert.body.clone());
        self.alerts.lock().unwrap().push(alert.clone());
        true
    }
}

impl PanelOpener for Effects {
    fn open(&self) -> bool {
        *self.panels.lock().unwrap() += 1;
        true
    }
}

struct NoSleep;

impl SleepProbe for NoSleep {
    fn gap_ms(&self) -> i64 {
        0
    }

    fn count(&self) -> Option<i64> {
        None
    }
}

/// 정해 둔 결과를 내는 합성 달력.
struct Scripted {
    queries: Vec<QuerySpec>,
    objects: Vec<NormalizedObject>,
    events: Vec<NormalizedEvent>,
}

impl DirectSource for Scripted {
    fn collect(&self, context: &CollectContext) -> Vec<SourceResult> {
        vec![result(&context.now.to_iso(), self.objects.clone(), self.events.clone())]
    }
}

impl Source for Scripted {
    fn name(&self) -> SourceName {
        SourceName::Calendar
    }

    fn queries(&self) -> &[QuerySpec] {
        &self.queries
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::Direct(self)
    }
}

fn result(when: &str, objects: Vec<NormalizedObject>, events: Vec<NormalizedEvent>) -> SourceResult {
    SourceResult {
        source: SourceName::Calendar,
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
        objects,
        events,
    }
}

fn imminent() -> NormalizedObject {
    NormalizedObject {
        text_head: Some("$(비밀 회의 제목)".into()),
        all_day: Some(false),
        start_at: Some("2026-03-02T10:30:00+09:00".into()),
        end_at: Some("2026-03-02T11:00:00+09:00".into()),
        state: Some("confirmed".into()),
        my_response: Some("accepted".into()),
        busy: Some(true),
        ..NormalizedObject::new("gcal:imminent", SourceName::Calendar, ObjectKind::Meeting, "imminent")
    }
}

struct Lab {
    _dir: TempDir,
    home: KairosHome,
    effects: Arc<Effects>,
    console: Console,
}

impl Lab {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = KairosHome::at(dir.path());
        let db = Database::open(&home).unwrap();
        Ledger::new(&db).ingest(result(BEFORE, Vec::new(), Vec::new()), &IngestOptions::at(at(BEFORE))).unwrap();
        drop(db);
        let control = KairosControl::default().with_display_mode(DisplayMode::On);
        ControlFile::new(&home).write(&control).unwrap();
        Self { _dir: dir, home, effects: Arc::new(Effects::default()), console: Console::captured("") }
    }

    fn context(&self, now: &str, objects: Vec<NormalizedObject>, events: Vec<NormalizedEvent>) -> AppContext {
        let env = BTreeMap::from([("HOME".to_owned(), self.home.root().to_str().unwrap().to_owned())]);
        let source = Scripted { queries: vec![QuerySpec::new("tick", 180, 300)], objects, events };
        AppContext::new(self.home.clone(), Arc::new(FixedClock(at(now))), Arc::new(NoProcess), env)
            .with_sources(SourceRegistry::new(vec![Arc::new(source)]))
            .with_notifier(self.effects.clone())
            .with_panel(self.effects.clone())
            .with_sleep_probe(Arc::new(NoSleep))
            .with_console(self.console.clone())
    }

    fn collect(&self, now: &str, objects: Vec<NormalizedObject>) -> i32 {
        registry().dispatch(&self.context(now, objects, Vec::new()), &["collect".to_owned()])
    }
}

#[test]
fn collect는_고른_알림기로_p0를_한_번만_알리고_패널을_열고_건수를_찍는다() {
    let lab = Lab::new();
    assert_eq!(lab.collect(T1, vec![imminent()]), 0);
    assert_eq!(*lab.effects.messages.lock().unwrap(), ["회의 임박 1건"]);
    let alert = lab.effects.alerts.lock().unwrap()[0].clone();
    assert_eq!((alert.title.as_str(), alert.items), (P0_TITLE, vec![1]));
    assert_eq!(*lab.effects.panels.lock().unwrap(), 1);
    let out = lab.console.out();
    assert!(out.starts_with("수집 완료: 5건 기록, 실패 0건\n"), "{out}");
    assert!(out.contains("P0 알림 1건\n"), "{out}");
    assert!(!out.contains("비밀 회의 제목"));
    assert_eq!(lab.collect(T2, vec![imminent()]), 0);
    assert_eq!(lab.effects.messages.lock().unwrap().len(), 1, "다음 틱은 울리지 않는다");
    let db = Database::open(&lab.home).unwrap();
    assert!(ItemRepo::new(db.connection()).get(1).unwrap().unwrap().p0_notified_at.is_some());
    assert!(StateBuilder::read_state(&lab.home).is_some());
}

#[test]
fn collect는_시간_밖이면_수집_안_함을_찍고_0으로_끝난다() {
    let lab = Lab::new();
    assert_eq!(lab.collect("2026-03-02T13:00:00.000Z", Vec::new()), 0);
    assert_eq!(lab.console.out(), "수집 안 함: 수집 시간(근무일 09:00~20:00) 밖\n");
}

#[test]
fn collect는_정지_표식이_있으면_멈추고_1로_끝난다() {
    let lab = Lab::new();
    fs::write(lab.home.file("halt.json"), "{\"reason\":\"model_called\"}").unwrap();
    assert_eq!(lab.collect(T1, Vec::new()), 1);
    assert_eq!(lab.console.out(), "수집 안 함: 수집 중단 중: model_called\n");
    assert!(lab.effects.messages.lock().unwrap().is_empty());
}

#[test]
fn 실패한_소스는_줄로_나오고_force는_시간_밖에도_돈다() {
    let lab = Lab::new();
    let context = lab.context("2026-03-02T13:00:00.000Z", Vec::new(), vec![broken_event()]);
    let code = registry().dispatch(&context, &["collect".to_owned(), "--force".to_owned()]);
    assert_eq!(code, 0);
    let out = lab.console.out();
    assert!(out.contains("  calendar tick: contract_break "), "{out}");
    assert!(out.starts_with("수집 완료: 5건 기록, 실패 1건\n"), "{out}");
}

/// 계약을 어긴 이벤트. 소스가 결과 전체를 잃는다.
fn broken_event() -> NormalizedEvent {
    NormalizedEvent::new("gcal:bad", SourceName::Slack, EventType::CalCreated, "not-a-time", ActorKind::Human)
}

#[test]
fn 패닉은_코드_없는_throw처럼_unexpected다() {
    struct Boom;
    impl Source for Boom {
        fn name(&self) -> SourceName {
            panic!("합성 패닉")
        }
        fn queries(&self) -> &[QuerySpec] {
            &[]
        }
        fn mode(&self) -> SourceMode<'_> {
            SourceMode::NotConnected { reason: "합성" }
        }
    }
    struct Exploding;
    impl kairos_cli::Command for Exploding {
        fn name(&self) -> &'static str {
            "explode"
        }
        fn execute(&self, _context: &AppContext, _args: &[String]) -> kairos_core::KairosResult<i32> {
            let _ = Boom.name();
            Ok(0)
        }
    }
    let lab = Lab::new();
    let context = lab.context(T1, Vec::new(), Vec::new());
    let custom = kairos_cli::CommandRegistry::new().with(Exploding);
    assert_eq!(custom.dispatch(&context, &["explode".to_owned()]), 1);
    assert_eq!(lab.console.err(), "kairos explode 실패: unexpected\n");
}
