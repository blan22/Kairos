//! control.json. 사용자가 정한 값을 파일 하나에 둔다.

use std::fs;

use kairos_core::home::write_file_atomic;
use kairos_core::{Instant, KairosError, KairosHome, KairosResult, LocalDate, OffDates, js, string_enum};
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::pretty_json;

/// 해제할 때까지 멈춘다.
pub const INDEFINITE: &str = "indefinite";

const PAUSED_UNTIL: &str = "pausedUntil";
const OFF_DATES: &str = "offDates";
const DISPLAY_MODE: &str = "displayMode";
const FOCUS: &str = "focus";
const DISPLAY_OPENED_AT: &str = "displayOpenedAt";
const SESSION_TEST_PASSED_AT: &str = "sessionTestPassedAt";
const P1_NOTIFY_OFF: &str = "p1NotifyOff";

string_enum! {
    /// auto는 기록 1근무일 뒤 표시, on과 off는 사용자가 정한 값이다.
    pub enum DisplayMode {
        Auto => "auto",
        On => "on",
        Off => "off",
    }
}

/// 진행 중이거나 끝난 포커스. 시각은 파일의 원문이다. 패널에 그대로 나간다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusControl {
    pub started_at: String,
    /// 사용자가 고른 끝. 회의가 있으면 실제 끝은 더 이를 수 있다.
    pub until: String,
    /// kairos focus --end로 끝낸 시각.
    pub ended_at: Option<String>,
}

/// JS의 참 거짓. 파일을 읽을 때 빈 값을 키째 버리는 기준이다.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// control.json의 내용.
///
/// 키 순서가 파일 바이트에 남는다. TS는 `{ ...control, focus }`처럼 객체를 펼쳐 저장해서, 새 키는 맨 뒤로 가고 있던 키는
/// 자리를 지킨다. 구조체로 옮기면 이 순서를 잃으므로 JS 객체와 같게 동작하는 순서 있는 맵을 쥐고 값은 메서드로만 다룬다.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct KairosControl {
    fields: Map<String, Value>,
}

impl Default for KairosControl {
    fn default() -> Self {
        let mut fields = Map::new();
        fields.insert(PAUSED_UNTIL.to_owned(), Value::Null);
        fields.insert(OFF_DATES.to_owned(), Value::Array(Vec::new()));
        Self { fields }
    }
}

impl KairosControl {
    /// 읽은 값을 정리한다. 모르는 키는 버린다. 값이 없으면 키도 두지 않는다. 깨진 표시 모드는 기본인 auto로 읽는다.
    fn from_parsed(parsed: &Map<String, Value>) -> Self {
        let mut control = Self::default();
        if let Some(paused) = parsed.get(PAUSED_UNTIL).filter(|value| !value.is_null()) {
            control.fields.insert(PAUSED_UNTIL.to_owned(), paused.clone());
        }
        if let Some(off_dates) = parsed.get(OFF_DATES).filter(|value| !value.is_null()) {
            control.fields.insert(OFF_DATES.to_owned(), off_dates.clone());
        }
        let mode = parsed.get(DISPLAY_MODE).and_then(Value::as_str);
        if let Some(mode) = mode.filter(|mode| mode.parse::<DisplayMode>().is_ok()) {
            control.fields.insert(DISPLAY_MODE.to_owned(), Value::from(mode));
        }
        for key in [FOCUS, DISPLAY_OPENED_AT, SESSION_TEST_PASSED_AT] {
            if let Some(value) = parsed.get(key).filter(|value| truthy(value)) {
                control.fields.insert(key.to_owned(), value.clone());
            }
        }
        if parsed.get(P1_NOTIFY_OFF) == Some(&Value::Bool(true)) {
            control.fields.insert(P1_NOTIFY_OFF.to_owned(), Value::Bool(true));
        }
        control
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.fields.get(key).and_then(Value::as_str)
    }

    fn with(mut self, key: &str, value: Value) -> Self {
        self.fields.insert(key.to_owned(), value);
        self
    }

    fn without(mut self, key: &str) -> Self {
        self.fields.shift_remove(key);
        self
    }

    /// ISO 시각까지 표시를 멈춘다. `INDEFINITE`면 해제할 때까지. None이면 멈추지 않았다.
    pub fn paused_until(&self) -> Option<&str> {
        self.text(PAUSED_UNTIL)
    }

    #[must_use]
    pub fn with_paused_until(self, until: Option<&str>) -> Self {
        self.with(PAUSED_UNTIL, until.map_or(Value::Null, Value::from))
    }

    pub fn is_paused(&self, now: Instant) -> bool {
        match self.paused_until() {
            None => false,
            Some(INDEFINITE) => true,
            // 해석할 수 없는 값은 NaN이라 비교가 거짓이다.
            Some(until) => Instant::parse(until).is_some_and(|until| until > now),
        }
    }

    /// kairos off로 지정한 날짜의 원문.
    pub fn off_dates(&self) -> Vec<String> {
        let dates = self.fields.get(OFF_DATES).and_then(Value::as_array);
        dates.map_or_else(Vec::new, |dates| dates.iter().filter_map(|date| date.as_str().map(str::to_owned)).collect())
    }

    #[must_use]
    pub fn with_off_dates(self, dates: &[String]) -> Self {
        self.with(OFF_DATES, json!(dates))
    }

    /// 근무일 판정에 쓰는 집합. 날짜가 아닌 값은 어떤 날과도 같지 않으므로 뺀다.
    pub fn off_date_set(&self) -> OffDates {
        self.off_dates().iter().filter_map(|date| LocalDate::parse(date)).collect()
    }

    /// 없으면 auto다.
    pub fn display_mode(&self) -> DisplayMode {
        self.text(DISPLAY_MODE).and_then(|mode| mode.parse().ok()).unwrap_or(DisplayMode::Auto)
    }

    #[must_use]
    pub fn with_display_mode(self, mode: DisplayMode) -> Self {
        self.with(DISPLAY_MODE, Value::from(mode.as_str()))
    }

    /// 포커스 값이 있는가.
    pub fn has_focus(&self) -> bool {
        self.fields.get(FOCUS).is_some_and(truthy)
    }

    /// 없으면 None이다. 값은 있는데 시각 문자열이 빠지면 TS에서 SQL 바인딩이 던지던 경우라 오류로 낸다.
    pub fn focus(&self) -> KairosResult<Option<FocusControl>> {
        if !self.has_focus() {
            return Ok(None);
        }
        let field = |key: &str| self.fields.get(FOCUS).and_then(|focus| focus.get(key)).and_then(Value::as_str);
        let (Some(started_at), Some(until)) = (field("startedAt"), field("until")) else {
            return Err(KairosError::new("invalid_control"));
        };
        Ok(Some(FocusControl {
            started_at: started_at.to_owned(),
            until: until.to_owned(),
            ended_at: field("endedAt").filter(|ended| !ended.is_empty()).map(str::to_owned),
        }))
    }

    /// 새 포커스로 바꾼다. 끝낸 표시는 사라진다.
    #[must_use]
    pub fn with_focus(self, started_at: &str, until: &str) -> Self {
        self.with(FOCUS, json!({ "startedAt": started_at, "until": until }))
    }

    /// 포커스에 끝낸 시각을 적는다. 포커스가 없으면 그대로다.
    #[must_use]
    pub fn with_focus_ended(mut self, now: &str) -> Self {
        if let Some(focus) = self.fields.get_mut(FOCUS).and_then(Value::as_object_mut) {
            focus.insert("endedAt".to_owned(), Value::from(now));
        }
        self
    }

    /// auto의 표시가 처음 열린 시각이 있다. 쉬는 날을 사후에 넣어도 게이트를 되돌리지 않는다.
    pub fn display_opened(&self) -> bool {
        self.fields.get(DISPLAY_OPENED_AT).is_some_and(truthy)
    }

    #[must_use]
    pub fn with_display_opened_at(self, now: &str) -> Self {
        self.with(DISPLAY_OPENED_AT, Value::from(now))
    }

    /// 사용자가 차단 시험(S3, S4) 뒤에 kairos sessions on으로 켠 시각. 없으면 세션을 열지 않는다.
    pub fn session_test_passed_at(&self) -> Option<&str> {
        self.text(SESSION_TEST_PASSED_AT)
    }

    /// 세션을 여는 잠금. 차단 시험 전에는 시작 버튼도 수락도 없다(단계 2 완료 기준).
    pub fn sessions_allowed(&self) -> bool {
        self.fields.get(SESSION_TEST_PASSED_AT).is_some_and(truthy)
    }

    /// 켜면 키가 맨 뒤로 간다. TS가 지운 나머지에 새로 붙여 저장한다.
    #[must_use]
    pub fn with_sessions(self, passed_at: Option<&str>) -> Self {
        let cleared = self.without(SESSION_TEST_PASSED_AT);
        match passed_at {
            Some(at) => cleared.with(SESSION_TEST_PASSED_AT, Value::from(at)),
            None => cleared,
        }
    }

    /// kairos notify off로 끈 P1 Mac 알림. 없으면 켜짐이다. P0와 건강 경고는 이 값과 무관하다.
    pub fn p1_notify_on(&self) -> bool {
        self.fields.get(P1_NOTIFY_OFF) != Some(&Value::Bool(true))
    }

    /// 끄면 키가 맨 뒤로 간다. TS가 지운 나머지에 새로 붙여 저장한다.
    #[must_use]
    pub fn with_p1_notify(self, on: bool) -> Self {
        let cleared = self.without(P1_NOTIFY_OFF);
        if on { cleared } else { cleared.with(P1_NOTIFY_OFF, Value::Bool(true)) }
    }
}

/// control.json의 읽기와 쓰기.
#[derive(Clone, Copy, Debug)]
pub struct ControlFile<'a> {
    home: &'a KairosHome,
}

impl<'a> ControlFile<'a> {
    pub const fn new(home: &'a KairosHome) -> Self {
        Self { home }
    }

    /// 파일이 없거나 깨졌으면 기본값이다. 객체가 아닌 JSON도 같다.
    pub fn read(&self) -> KairosControl {
        let Ok(bytes) = fs::read(self.home.control_path()) else {
            return KairosControl::default();
        };
        match js::parse_json(&String::from_utf8_lossy(&bytes)) {
            Ok(Value::Object(parsed)) => KairosControl::from_parsed(&parsed),
            _ => KairosControl::default(),
        }
    }

    pub fn write(&self, control: &KairosControl) -> KairosResult<()> {
        self.home.ensure()?;
        write_file_atomic(&self.home.control_path(), &pretty_json(control)?)
    }
}
