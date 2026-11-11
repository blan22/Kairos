//! kairos의 도메인 어휘. 값 객체와 모델, 바깥 세계와 만나는 포트를 둔다.
//!
//! 이 크레이트는 다른 kairos 크레이트에 의존하지 않는다. 판단 규칙은 `kairos-policy`, 저장은 `kairos-store`다.

pub mod catalog;
pub mod connector;
pub mod error;
pub mod home;
pub mod instant;
pub mod js;
pub mod local_date;
pub mod model;
pub mod ports;
pub mod process;
pub mod profile;
pub mod text;

pub use error::{KairosError, KairosResult};
pub use home::KairosHome;
pub use instant::Instant;
pub use local_date::{LocalDate, OffDates};
pub use profile::Profile;
