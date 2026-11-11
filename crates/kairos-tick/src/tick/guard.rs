//! 원본의 try/catch 자리. 소스와 포트의 실패는 틱을 멈추지 않는다.

use std::panic::{self, AssertUnwindSafe, catch_unwind};

use kairos_core::ports::{Alert, Notifier};

/// 패닉이면 None이다.
pub(super) fn caught<T>(work: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(work)).ok()
}

/// 던지는 알림을 전달 실패로 바꾼다. 패닉이 표시 없이 빠져나가면 미전달로 세지 못한다.
pub(super) struct GuardedNotifier<'a>(pub &'a dyn Notifier);

impl Notifier for GuardedNotifier<'_> {
    fn notify(&self, alert: &Alert) -> bool {
        caught(|| self.0.notify(alert)).unwrap_or(false)
    }
}

/// 기본 훅은 패닉 메시지를 stderr에 찍는다. 문자열 경계 패닉은 그 문자열을 메시지에 담으므로 외부 문자열이 launchd 로그로 샌다.
/// 메시지 없이 위치만 남긴다. 조립 지점이 한 번 부른다.
pub fn install_quiet_panic_hook() {
    panic::set_hook(Box::new(|info| {
        let place = info.location().map_or_else(String::new, |at| format!(" {}:{}", at.file(), at.line()));
        eprintln!("kairos 내부 오류{place}");
    }));
}
