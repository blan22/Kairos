//! 지금 카드 1장을 고르고 패널이 읽는 JSON을 만든다(설계 6.3, 6.5, 16절).
//! 예산과 등급 판단은 여기서만 한다. 플러그인은 그리기와 버튼만 맡는다.
//!
//! 흐름
//! - next: 고르기만 한다. 예산을 쓰지 않아 몇 번을 불러도 같다.
//! - next --seen: 사용자에게 실제로 그렸을 때 부른다. 이때만 suggestions와 예산에 남는다.
//! - next --ack: 묶음과 넘어온 카드를 확인으로 내린다.
//!
//! 세션의 준비됨 카드는 ready에 따로 나온다. 예산을 쓰지 않고 run마다 1회다.
//! 대기 시각과 카드 만료를 먼저 반영하므로 위 셋 모두 짧은 쓰기 트랜잭션을 연다.
//! 보류로 가려진 카드는 내린다. 사용자가 보지 못한 시간을 밀어내기로 세지 않기 위해서다.
//!
//! 외부 문자열은 title과 url에만 있고 정제해서 낸다. argv는 고정 낱말과 항목 ID뿐이다.

mod budget_meter;
mod card_builder;
mod card_chooser;
mod focus_tracker;
mod playbook;
mod presenter;
mod views;

pub use budget_meter::{BudgetMeter, BudgetUsage};
pub use card_builder::{CardBuilder, session_of};
pub use card_chooser::{CardChooser, P1AlertMode};
pub use focus_tracker::{FocusTracker, FocusView};
pub use playbook::{Playbook, issue_url_matches, label_of, playbook_for, playbook_of, reason_label, safe_url};
pub use presenter::{ListResponse, NextOptions, NextResponse, Presenter, RunsView};
pub use views::{Card, CardAction, DeployView, Hold, HoldReason, ItemView, NextContext, ReadyCard};

/// 패널과 수락 쪽이 맞추는 JSON 형식 번호. 필드를 빼거나 뜻을 바꾸면 올린다.
pub const CONTRACT_VERSION: i64 = 1;
