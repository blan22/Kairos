//! 설계 6.3, 6.5의 예산 값.
//!
//! 2026-11-04 표면 변경: 상태 표시줄 건수와 패널 목록은 사용자가 스스로 여는 것이라 예산에서 뺀다.
//! 예산은 '지금 카드' 1장에만 쓴다. Mac 알림은 받지 않는다(2026-11-05 사용자 결정). 30분 합치기는 알림에도 쓴다.

use kairos_core::Instant;

/// 수집 틱이 도는 시간(현지, 자정 이후 분). 근무 시간과 다르다. launchd 일정, 감시자와 맞물린다.
/// 시각 값은 모두 프로필의 `schedule`에서 온다. 기본은 언제나(모든 요일 00:00~24:00, 점심 없음)이고 묶음은 13:30과 18:00이다.
pub fn tick_start_minutes() -> u32 {
    schedule().tick.start
}

pub fn tick_end_minutes() -> u32 {
    schedule().tick.end
}

/// 지금까지 수집이 쉬지 않고 이어진 구간의 시작. 그 앞의 공백은 수집 시간 밖이라 지연이 아니다.
/// 보통은 오늘 수집이 시작된 순간이다. 수집이 하루 종일이면 자정을 넘어 이어지므로, 전날도 수집 요일이면 그날로 내려간다.
/// 모든 요일에 하루 종일 수집하면(기본 일정) 하한이 없다.
pub fn collecting_since(at: Instant) -> Instant {
    let schedule = schedule();
    let mut day = at.local_date();
    if schedule.tick.start != 0 || schedule.tick.end != kairos_core::profile::DAY_MINUTES {
        return Instant::at_local(day, schedule.tick.start);
    }
    for _ in 0..7 {
        let before = day.add_days(-1);
        if !schedule.days.contains(before.weekday()) {
            return Instant::at_local(day, 0);
        }
        day = before;
    }
    Instant::from_millis(0)
}

fn schedule() -> &'static kairos_core::profile::Schedule {
    &kairos_core::profile::current().schedule
}

pub const P1_PER_HOUR: i64 = 1;
pub const P1_PER_DAY: i64 = 4;
/// 목표일 뿐 강제하지 않는다. P1 4건과 묶음 2회의 합이다.
pub const DAY_TARGET: i64 = 6;
pub const PUSH_LIMIT: i64 = 2;
pub const BUNDLE_LINES: usize = 7;
pub const MERGE_WINDOW_MS: i64 = 30 * 60_000;
/// P1 Mac 알림의 전달 시도 상한. 표시는 됐는데 실패로 판정돼도 같은 알림이 이 횟수를 넘지 않는다.
pub const P1_NOTIFY_TRIES: i64 = 5;
/// 이 횟수까지는 틱마다 시도한다. 그 뒤는 앞선 실패에서 `P1_RETRY_GAP_MS`를 기다린다.
pub const P1_NOTIFY_TICK_TRIES: i64 = 3;
pub const P1_RETRY_GAP_MS: i64 = 30 * 60_000;
/// 응답 없이 카드를 유지하는 시간. 지나면 밀어내기 1회다. P0는 만료되지 않는다.
pub const CARD_TTL_MS: i64 = 3_600_000;
pub const P1_DUE_MS: i64 = 90 * 60_000;
pub fn work_start_minutes() -> u32 {
    schedule().work.start
}

/// 근무 시작이 자정인 일정(기본값)의 아침. '내일 아침'과 날짜만 있는 일정이 쓴다.
pub const MORNING_FALLBACK_MINUTES: u32 = 9 * 60;

/// '내일 아침'처럼 하루를 여는 시각. 근무 시작이 있으면 그 시각이고, 하루 종일 근무면 09:00이다.
pub fn morning_minutes() -> u32 {
    match work_start_minutes() {
        0 => MORNING_FALLBACK_MINUTES,
        start => start,
    }
}

pub fn work_end_minutes() -> u32 {
    schedule().work.end
}

pub fn lunch_start_minutes() -> u32 {
    schedule().lunch.start
}

pub fn lunch_end_minutes() -> u32 {
    schedule().lunch.end
}

/// P2 묶음 시각. 오름차순이고 하나 이상이다(kairos-config가 검증한다).
pub fn bundle_slot_minutes() -> &'static [u32] {
    &schedule().bundles
}

pub const FOCUS_DEFAULT_MINUTES: i64 = 50;
pub const FOCUS_MAX_MINUTES: i64 = 90;
pub const FOCUS_MEETING_MARGIN_MS: i64 = 5 * 60_000;
/// 끝난 포커스의 쌓인 건수를 보여 주는 시간.
pub const FOCUS_NOTICE_MS: i64 = 3_600_000;

/// `fold_lines`의 결과.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FoldedLines<T> {
    pub shown: Vec<T>,
    pub more: usize,
}

/// 넘어온 줄은 접지 않는다. 앞쪽 keep개는 줄 수를 넘어도 모두 남긴다.
pub fn fold_lines<T: Clone>(lines: &[T], keep: usize) -> FoldedLines<T> {
    let limit = BUNDLE_LINES.max(keep);
    FoldedLines { shown: lines.iter().take(limit).cloned().collect(), more: lines.len().saturating_sub(limit) }
}

/// 합칠 수 있는 행. 항목 행이 구현한다.
pub trait Mergeable {
    fn channel_id(&self) -> Option<&str>;
    /// 해석할 수 없으면 None이다. 어떤 구간에도 들지 않는다.
    fn opened_at(&self) -> Option<Instant>;
}

/// 첫 항목과 같은 채널의 30분 구간을 카드 1장으로 합친다. 채널이 없으면 합치지 않는다.
pub fn merge_group<T: Mergeable>(rows: &[T]) -> Vec<&T> {
    let Some(anchor) = rows.first() else {
        return Vec::new();
    };
    let Some(channel) = anchor.channel_id() else {
        return vec![anchor];
    };
    let start = anchor.opened_at();
    rows.iter()
        .filter(|row| row.channel_id() == Some(channel))
        .filter(|row| match (row.opened_at(), start) {
            (Some(at), Some(start)) => at.ms_since(start).abs() <= MERGE_WINDOW_MS,
            _ => false,
        })
        .collect()
}
