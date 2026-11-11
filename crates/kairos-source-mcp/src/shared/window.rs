use kairos_core::Instant;
use kairos_core::model::CollectContext;

pub(crate) const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const MIN_WINDOW_DAYS: i64 = 2;
/// 이벤트 보존 기간과 같다.
pub const MAX_WINDOW_DAYS: i64 = 30;

/// 메일 검색의 창.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MailWindow {
    pub days: i64,
    pub capped: bool,
    pub from: Instant,
}

/// `Math.ceil(ms / DAY_MS)`.
pub(crate) fn ceil_days(ms: i64) -> i64 {
    (ms as f64 / DAY_MS as f64).ceil() as i64
}

/// 메일을 어디부터 읽어야 하는가. 첫 ok 전이면 첫 시도부터다. 그 전의 메일은 baseline이다. 읽은 표시가 있으면 그 뒤다.
pub(crate) fn scan_start(context: &CollectContext, query: &str) -> Option<Instant> {
    let cursor = context.cursor(query)?;
    let first_run = || cursor.first_run_at.as_deref().and_then(Instant::parse);
    let base = cursor.window_start().or_else(first_run);
    let progress = cursor.progress_at.as_deref().and_then(Instant::parse);
    match (base, progress) {
        (Some(base), Some(progress)) => Some(base.max(progress)),
        (base, progress) => base.or(progress),
    }
}

impl MailWindow {
    /// 커서부터 지금까지를 덮는 일수이고, newer_than이 일 단위라 하루 여유를 더한다.
    pub fn of(context: &CollectContext, query: &str) -> Self {
        let needed = match scan_start(context, query) {
            Some(start) => ceil_days(context.now.ms_since(start)) + 1,
            None => MIN_WINDOW_DAYS,
        };
        let days = needed.clamp(MIN_WINDOW_DAYS, MAX_WINDOW_DAYS);
        Self { days, capped: needed > MAX_WINDOW_DAYS, from: context.now.minus_ms(days * DAY_MS) }
    }
}
