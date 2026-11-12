//! control.json을 바꾸는 명령. `notify`는 macOS 알림 권한도 다룬다.

use kairos_agenda::{DisplayMode, FocusTracker, INDEFINITE};
use kairos_core::local_date::is_date_only;
use kairos_core::{Instant, KairosError, KairosResult};
use kairos_policy::FOCUS_DEFAULT_MINUTES;
use kairos_tick::js_number_text;
use serde_json::json;

use super::notifier::{Grant, NotifierHelper, TestPost};
use super::sources;
use crate::app::{AppContext, Console};
use crate::args::{ArgParser, OptionSpec, flag};
use crate::command::Command;
use crate::js_number::{is_integer, js_number};
use crate::reply::Reply;

const JSON_ONLY: [OptionSpec; 1] = [flag("json")];
const FOCUS_OPTIONS: [OptionSpec; 3] = [flag("json"), flag("end"), flag("status")];
const PAUSE_OPTIONS: [OptionSpec; 1] = [flag("resume")];
const OFF_OPTIONS: [OptionSpec; 1] = [flag("clear")];
/// `Date`가 나타낼 수 있는 시각의 끝. 넘으면 원본의 toISOString이 RangeError를 던진다.
const MAX_DATE_MS: f64 = 8.64e15;
/// `code`가 없는 throw.
const UNEXPECTED: &str = "unexpected";

fn valid_minutes(minutes: f64) -> bool {
    is_integer(minutes) && minutes > 0.0
}

pub struct Focus;

impl Command for Focus {
    fn name(&self) -> &'static str {
        "focus"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos focus [분] | --end | --status [--json]")
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&FOCUS_OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let minutes = parsed.first().map_or(FOCUS_DEFAULT_MINUTES as f64, js_number);
        if !valid_minutes(minutes) {
            return Ok(reply.usage("사용법: kairos focus [분] | --end | --status"));
        }
        let at = context.now();
        let (end, status) = (parsed.flag("end"), parsed.flag("status"));
        let mut control = context.control();
        if end {
            control = FocusTracker::end(control, at);
        } else if !status {
            // 끝은 90분을 넘지 않아 큰 값도 그 안으로 잘린다.
            control = FocusTracker::start(control, minutes as i64, at);
        }
        if !status {
            context.control_file().write(&control)?;
        }
        let database = context.open_db()?;
        let focus = FocusTracker::new(database.connection()).view(&control, at)?;
        drop(database);
        if reply.json() {
            return reply.success(json!({ "at": at, "focus": focus }));
        }
        let line = if focus.active {
            format!("포커스 중: {}까지", focus.until.as_deref().unwrap_or("null"))
        } else {
            focus.held_line.clone().unwrap_or_else(|| "포커스 아님".to_owned())
        };
        context.console().print(&line);
        Ok(0)
    }
}

pub struct Display;

impl Command for Display {
    fn name(&self) -> &'static str {
        "display"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos display [on|off|auto] [--json]")
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&JSON_ONLY).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        if let Some(mode) = parsed.first() {
            let Ok(mode) = mode.parse::<DisplayMode>() else {
                return Ok(reply.usage("사용법: kairos display [on|off|auto]"));
            };
            context.control_file().write(&context.control().with_display_mode(mode))?;
            context.refresh_state()?;
        }
        let at = context.now();
        let database = context.open_db()?;
        let display = context.next_context(&database, at)?.gate;
        drop(database);
        if reply.json() {
            return reply.success(json!({ "at": at, "display": display }));
        }
        context.console().print(&if display.open {
            format!("표시 켜짐 ({})", display.mode)
        } else {
            format!("표시 꺼짐 ({}): {}", display.mode, display.message.as_deref().unwrap_or("null"))
        });
        Ok(0)
    }
}

pub struct Notify;

const NOTIFY_USAGE: &str = "사용법: kairos notify [on|off|setup|test] [--json]";

impl Notify {
    /// `setup`: macOS 권한 창을 띄운다. 전에 거절했으면 창이 뜨지 않으므로 시스템 설정의 알림 화면을 연다.
    /// `test`: 고정 문구의 시험 알림 하나를 보낸다. 둘 다 화면에 무언가를 띄우므로 세션 안에서는 하지 않는다.
    fn permission(context: &AppContext, reply: Reply<'_>, mode: &str) -> KairosResult<i32> {
        if sources::session_caller(context) {
            return Ok(reply.fail(
                "session_caller",
                "세션 안에서는 알림 권한을 다루지 않습니다. 일반 터미널에서 실행하세요",
                1,
            ));
        }
        let helper = match NotifierHelper::installed(context) {
            Ok(helper) => helper,
            Err(missing) => return Ok(reply.fail("notifier_missing", missing.message(), 1)),
        };
        if mode == "test" {
            return match helper.post_test() {
                TestPost::Posted if reply.json() => reply.success(json!({ "notify": { "test": "posted" } })),
                TestPost::Posted => {
                    context.console().print(
                        "시험 알림을 보냈습니다. 보이지 않으면 집중 모드와 시스템 설정 > 알림 > kairos를 확인하세요",
                    );
                    Ok(0)
                }
                TestPost::NotAuthorized => {
                    Ok(reply.fail("notify_not_authorized", "알림 권한이 없습니다. kairos notify setup으로 받으세요", 1))
                }
                TestPost::Failed(code) => {
                    let code = code.map_or_else(|| "없음".to_owned(), |code| code.to_string());
                    Ok(reply.fail("notify_failed", &format!("시험 알림을 보내지 못했습니다(헬퍼 종료 코드 {code})"), 1))
                }
            };
        }
        // 진행 안내는 사람에게만 찍는다. --json의 표준 출력은 JSON 한 덩어리다.
        let quiet = Console::captured("");
        let grant = helper.grant(if reply.json() { &quiet } else { context.console() });
        if grant == Grant::DeniedBefore {
            let opened = helper.open_settings();
            if !reply.json() {
                context.console().print(if opened {
                    "시스템 설정의 알림 화면을 열었습니다"
                } else {
                    "시스템 설정을 열지 못했습니다. 직접 여세요"
                });
            }
        }
        if reply.json() {
            return reply.success(json!({ "notify": { "permission": helper.permission().as_str() } }));
        }
        Ok(i32::from(grant != Grant::Authorized))
    }
}

impl Command for Notify {
    fn name(&self) -> &'static str {
        "notify"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(NOTIFY_USAGE)
    }

    /// `on`, `off`는 P1 Mac 알림 켜기와 끄기다. 끈 동안 잡힌 건은 다시 켜도 울리지 않는다. P0와 건강 경고는 끄지 못한다.
    /// `setup`, `test`는 macOS 알림 권한을 받고 시험한다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&JSON_ONLY).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let mode = parsed.first();
        if let Some(mode @ ("setup" | "test")) = mode {
            return Self::permission(context, reply, mode);
        }
        if mode.is_some_and(|mode| mode != "on" && mode != "off") {
            return Ok(reply.usage("사용법: kairos notify [on|off|setup|test]"));
        }
        if let Some(mode) = mode {
            context.control_file().write(&context.control().with_p1_notify(mode != "off"))?;
        }
        let p1 = context.control().p1_notify_on();
        if reply.json() {
            return reply.success(json!({ "notify": { "p1": p1 } }));
        }
        context.console().print(if p1 { "P1 알림 켜짐" } else { "P1 알림 꺼짐. 켜기: kairos notify on" });
        Ok(0)
    }
}

pub struct Pause;

/// `new Date(Date.now() + minutes * 60_000).toISOString()`.
fn minutes_later(now: Instant, minutes: f64) -> KairosResult<String> {
    let at = now.millis() as f64 + minutes * 60_000.0;
    if at.abs() > MAX_DATE_MS {
        return Err(KairosError::new(UNEXPECTED));
    }
    Ok(Instant::from_millis(at as i64).to_iso())
}

impl Command for Pause {
    fn name(&self) -> &'static str {
        "pause"
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&PAUSE_OPTIONS).parse(args)?;
        let console = context.console();
        let control = context.control();
        let control = if parsed.flag("resume") {
            let cleared = control.with_paused_until(None);
            console.print("일시정지를 해제했습니다.");
            cleared
        } else if let Some(text) = parsed.first() {
            let minutes = js_number(text);
            if !valid_minutes(minutes) {
                console.print("사용법: kairos pause [분] | kairos pause --resume");
                return Ok(2);
            }
            let until = minutes_later(context.now(), minutes)?;
            console.print(&format!("{}분 동안 표시를 멈춥니다. 수집은 계속합니다.", js_number_text(minutes)));
            control.with_paused_until(Some(&until))
        } else {
            console.print("표시를 멈췄습니다. 수집은 계속합니다. 해제: kairos pause --resume");
            control.with_paused_until(Some(INDEFINITE))
        };
        context.control_file().write(&control)?;
        context.refresh_state()?;
        Ok(0)
    }
}

pub struct Off;

impl Command for Off {
    fn name(&self) -> &'static str {
        "off"
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&OFF_OPTIONS).parse(args)?;
        let date = parsed.first().map_or_else(|| context.now().local_date().to_string(), str::to_owned);
        if !is_date_only(&date) {
            context.console().print("사용법: kairos off [YYYY-MM-DD] [--clear]");
            return Ok(2);
        }
        let clear = parsed.flag("clear");
        let control = context.control();
        let mut dates: Vec<String> = control.off_dates().into_iter().filter(|day| *day != date).collect();
        if !clear {
            dates.push(date.clone());
            // 기본 정렬은 UTF-16 단위 순서다.
            dates.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        }
        context.control_file().write(&control.with_off_dates(&dates))?;
        context.refresh_state()?;
        context.console().print(&if clear {
            format!("{date}을 근무일로 되돌렸습니다.")
        } else {
            format!("{date}을 쉬는 날로 기록했습니다.")
        });
        Ok(0)
    }
}
