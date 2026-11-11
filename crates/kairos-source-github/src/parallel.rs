//! `Promise.all`의 대체. 결과는 입력 순서로 모은다. 이벤트 순서가 저장 순서가 되므로 완료 순서를 쓰면 안 된다.

use std::sync::{Mutex, PoisonError};
use std::thread;

use kairos_core::ports::CancelToken;

use crate::error::GhError;

/// 시간상 가장 먼저 난 실패. `Promise.all`은 먼저 거절된 사유로 끝나지 입력 순서로 고르지 않는다.
#[derive(Default)]
struct FirstFailure(Mutex<Option<GhError>>);

impl FirstFailure {
    fn note<T>(&self, result: Result<T, GhError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.0.lock().unwrap_or_else(PoisonError::into_inner).get_or_insert(error);
                None
            }
        }
    }

    fn take(self) -> Option<GhError> {
        self.0.into_inner().unwrap_or_else(PoisonError::into_inner)
    }
}

/// 항목마다 스레드 하나로 동시에 돌리고 입력 순서로 모은다. 패닉한 일은 None이다.
pub(crate) fn run_all<I: Sync, T: Send>(items: &[I], work: impl Fn(&I) -> T + Sync) -> Vec<Option<T>> {
    thread::scope(|scope| {
        let handles: Vec<_> = items.iter().map(|item| scope.spawn(|| work(item))).collect();
        handles.into_iter().map(|handle| handle.join().ok()).collect()
    })
}

/// 서로 다른 두 일을 동시에 돌린다. 둘 다 성공해야 성공이다. 원본과 달리 남은 일이 끝날 때까지 기다린다.
/// 호출마다 시간 제한이 있어 영원히 막히지 않는다.
pub(crate) fn try_join<A: Send, B: Send>(
    first: impl FnOnce() -> Result<A, GhError> + Send,
    second: impl FnOnce() -> Result<B, GhError> + Send,
    cancel: &CancelToken,
) -> Result<(A, B), GhError> {
    let failure = FirstFailure::default();
    let (a, b) = thread::scope(|scope| {
        let a = scope.spawn(|| failure.note(first()));
        let b = scope.spawn(|| failure.note(second()));
        (a.join().ok().flatten(), b.join().ok().flatten())
    });
    if let Some(error) = failure.take() {
        return Err(error);
    }
    a.zip(b).ok_or_else(|| GhError::unexpected(cancel))
}
