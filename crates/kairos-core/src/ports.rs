//! 바깥 세계와 만나는 포트. 구현은 생성자로 주입한다. 시험은 가짜를 넘긴다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Instant;

/// 현재 순간. 벽시계를 읽는 곳은 `SystemClock` 하나뿐이다.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        // macOS의 monotonic 시계는 잠자기 동안 멈춘다. 잠자기 판정이 공백을 봐야 하므로 벽시계를 쓴다.
        let elapsed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        Instant::from_millis(elapsed.map_or(0, |value| value.as_millis() as i64))
    }
}

/// 시험과 차등 실행이 쓰는 고정 시계.
#[derive(Clone, Copy, Debug)]
pub struct FixedClock(pub Instant);

impl Clock for FixedClock {
    fn now(&self) -> Instant {
        self.0
    }
}

/// `AbortSignal`의 대체. 복제본이 모두 같은 신호를 본다.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 알림의 급. 헬퍼가 묶음 스레드와 중단 수준을 고른다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AlertTier {
    P0,
    P1,
}

impl AlertTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
        }
    }
}

/// 알림 한 건. 제목과 본문은 고정 문구와 건수뿐이다. 외부 문자열을 넣지 않는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Alert {
    pub tier: AlertTier,
    pub title: String,
    pub body: String,
    /// 이 알림에 담긴 항목 ID. 누르면 패널이 이 항목을 보여 준다.
    pub items: Vec<i64>,
}

/// 알림을 보낸 결과. 실패해도 이미 게시됐을 수 있으면 `Unknown`이다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    Posted,
    /// 게시하지 않은 것이 확실하다. 다른 길로 보내도 두 번 뜨지 않는다.
    NotPosted,
    /// 시간 초과처럼 게시 여부를 모른다.
    Unknown,
}

/// Mac 알림. 전달했으면 참이다.
pub trait Notifier: Send + Sync {
    fn notify(&self, alert: &Alert) -> bool;

    /// 결과를 세 갈래로 낸다. 기본은 실패를 게시하지 않은 것으로 본다.
    fn deliver(&self, alert: &Alert) -> Delivery {
        if self.notify(alert) { Delivery::Posted } else { Delivery::NotPosted }
    }
}

/// Orca 패널을 연다. 열었으면 참이다.
pub trait PanelOpener: Send + Sync {
    fn open(&self) -> bool;
}
