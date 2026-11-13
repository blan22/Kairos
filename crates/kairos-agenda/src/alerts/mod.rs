//! P0와 P1 알림. 알림 헬퍼(KairosNotifier.app)에 JSON 한 덩어리로 넘긴다. osascript는 쓰지 않는다(누르면 Script Editor가 열린다).
//! 제목과 본문은 고정이고 건수 숫자만 바뀐다. 외부 문자열을 넣지 않는다. 항목 ID는 누르면 패널이 보여 줄 대상이다.

mod desktop;
mod notifier;
mod p0;
mod p1;

pub use desktop::{DesktopNotifier, REVEAL_LINK_PREFIX, TOAST_APP_ID, parse_reveal_link, reveal_link};
pub use notifier::{NOTIFIER_APP_REL, NativeNotifier, ORCA_BIN, ORCA_BUNDLE_ID, OrcaPanelOpener, PANEL_OPEN_ARGS};
pub use p0::{NotifyReport, P0_TITLE, P0Alerts};
pub use p1::{P1_CATCHUP_TITLE, P1_TITLE, P1Alerts, P1Line, P1NotifyInput, P1NotifyReport};
