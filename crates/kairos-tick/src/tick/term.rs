//! 틱 동안만 도는 두 스레드. SIGTERM을 받는 쪽과 심장박동이다.

#[cfg(unix)]
use std::panic;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use kairos_core::KairosHome;
#[cfg(unix)]
use kairos_ops::ToolResults;
use kairos_ops::{BEAT_MS, StallWatch};
#[cfg(unix)]
use signal_hook::consts::SIGTERM;
#[cfg(unix)]
use signal_hook::iterator::{Handle, Signals};

use crate::lock::CollectLock;

#[cfg(unix)]
const TERMINATED_EXIT_CODE: i32 = 143;

/// 틱이 신호에 끊겨도 되는 구간을 표시한다.
///
/// 원본의 신호 처리기는 이벤트 루프에서 돌아서 await 사이에서만 끼어든다. 그래서 claude가 돌아온 뒤 정지 표식을 쓰기 전에는
/// 죽지 않는다. 스레드의 신호 처리는 아무 때나 돌므로, 메인이 이 문을 쥔 동안 기다리게 해 같은 약속을 지킨다.
#[derive(Clone, Default)]
pub(super) struct TermGate(Arc<Mutex<()>>);

impl TermGate {
    fn lock(&self) -> MutexGuard<'_, ()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn hold(&self) -> HeldGate<'_> {
        HeldGate { gate: self, guard: Some(self.lock()) }
    }
}

/// 메인 스레드가 쥔 문.
pub(super) struct HeldGate<'a> {
    gate: &'a TermGate,
    guard: Option<MutexGuard<'a, ()>>,
}

impl HeldGate<'_> {
    /// 원본의 await 자리. 이 안에서는 신호가 틱을 끝낼 수 있다.
    pub(super) fn outside<T>(&mut self, work: impl FnOnce() -> T) -> T {
        self.guard = None;
        let value = work();
        self.guard = Some(self.gate.lock());
        value
    }
}

/// 신호 처리 스레드. finally는 신호로 죽을 때 돌지 않는다. 잠금과 메일 본문이 든 파일을 남기지 않는다.
/// Windows의 작업 스케줄러는 신호 없이 프로세스를 끝낸다. 남은 잠금과 파일은 다음 틱이 정리한다.
#[cfg(unix)]
pub(super) struct TermWatch {
    handle: Handle,
    thread: Option<JoinHandle<()>>,
}

#[cfg(not(unix))]
pub(super) struct TermWatch;

/// unix 판과 같이 틱 끝에서 놓는 자리를 지킨다. 놓을 것은 없다.
#[cfg(not(unix))]
impl Drop for TermWatch {
    fn drop(&mut self) {}
}

#[cfg(not(unix))]
impl TermWatch {
    pub(super) fn install(
        _gate: TermGate,
        _home: &KairosHome,
        _user_home: &std::path::Path,
        _lock: CollectLock,
    ) -> Option<Self> {
        None
    }
}

#[cfg(unix)]
impl TermWatch {
    /// 등록하지 못하면 None이다. 그때도 틱은 돌고 죽으면 다음 틱이 정리한다.
    pub(super) fn install(
        gate: TermGate,
        home: &KairosHome,
        user_home: &std::path::Path,
        lock: CollectLock,
    ) -> Option<Self> {
        let mut signals = Signals::new([SIGTERM]).ok()?;
        let handle = signals.handle();
        let (home, user_home) = (home.clone(), user_home.to_owned());
        let thread = thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _held = gate.lock();
                let _ = ToolResults::for_collector(&home, &user_home).sweep();
                let _ = lock.release();
                std::process::exit(TERMINATED_EXIT_CODE);
            }
        });
        Some(Self { handle, thread: Some(thread) })
    }
}

#[cfg(unix)]
impl Drop for TermWatch {
    /// 메인이 문을 놓은 뒤에 불러야 한다. 처리 중인 신호가 문에서 기다리면 끝나지 않는다.
    fn drop(&mut self) {
        self.handle.close();
        if let Some(thread) = self.thread.take()
            && let Err(payload) = thread.join()
            && !thread::panicking()
        {
            panic::resume_unwind(payload);
        }
    }
}

/// 깨어 있는 동안 자식이 멈춰도 이 박동은 계속 돈다. 공백은 프로세스가 얼어 있었다는 뜻이다.
pub(super) struct Heartbeat {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Heartbeat {
    pub(super) fn start(stall: Arc<StallWatch>) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let thread = thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(Duration::from_millis(BEAT_MS)) {
                stall.beat();
            }
        });
        Self { stop: Some(stop), thread: Some(thread) }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.stop = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    /// 원본의 신호 처리기는 동기 구간에 끼어들지 못한다. 그래서 claude가 돌아온 뒤 정지 표식을 쓰기 전에는 죽지 않는다.
    #[test]
    fn 문을_쥔_동안에는_신호_처리가_기다리고_밖에서만_지나간다() {
        let gate = TermGate::default();
        let mut held = gate.hold();
        let (passed, seen) = channel();
        let waiting = gate.clone();
        let handler = thread::spawn(move || {
            let _held = waiting.lock();
            passed.send(()).unwrap();
        });
        assert!(seen.recv_timeout(Duration::from_millis(100)).is_err(), "동기 구간에는 끼어들지 못한다");
        held.outside(|| seen.recv_timeout(Duration::from_secs(5)).unwrap());
        handler.join().unwrap();
    }

    #[test]
    fn 심장박동은_멈추면_스레드가_끝난다() {
        use kairos_core::Instant;
        use kairos_core::ports::FixedClock;
        let stall = Arc::new(StallWatch::new(Arc::new(FixedClock(Instant::from_millis(0)))));
        drop(Heartbeat::start(stall.clone()));
        assert_eq!(Arc::strong_count(&stall), 1);
    }
}
