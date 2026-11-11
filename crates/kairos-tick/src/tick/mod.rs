//! 수집 틱 하나. `TickRunner::run`이 소스를 모두 한 번씩 훑고 state.json을 갱신한다.

mod auth;
mod direct;
mod fingerprint;
mod guard;
mod mode_run;
mod report;
mod runner;
mod settle;
mod term;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use kairos_agenda::{EngineReport, ItemEngine, RefreshInput};
use kairos_core::{KairosHome, KairosResult};
use kairos_ops::{SleepDetector, StallWatch};
use kairos_store::Database;

pub use auth::{AUTH_CHECK_INTERVAL_MS, AuthFile, AuthRecord};
pub use guard::install_quiet_panic_hook;
pub use report::{CollectReport, CollectStatus, ModeReport, OutcomeRow};
pub use runner::TickRunner;

pub const SOURCE_LIMIT_MS: i64 = 20_000;
pub const TOTAL_LIMIT_MS: i64 = 60_000;

/// 틱의 설정. 환경 변수는 kairos-cli가 `from_env`로 한 번 읽어 넘긴다.
#[derive(Clone, Debug)]
pub struct TickConfig {
    pub home: KairosHome,
    /// claude 실행 파일. 없으면 수집 모드 소스가 tool_missing으로 드러난다.
    pub claude_bin: Option<String>,
    /// 수집 플러그인 폴더.
    pub plugin_dir: Option<String>,
    /// ~/.claude가 있는 폴더.
    pub user_home: PathBuf,
    /// 소스별 상한. 시험에서만 줄인다.
    pub source_limit_ms: i64,
    /// 프로세스 환경. claude 자식의 환경은 `ClaudeMode::env`로 이 값에서 고른다.
    pub env: BTreeMap<String, String>,
}

impl TickConfig {
    pub fn new(home: KairosHome, user_home: impl Into<PathBuf>) -> Self {
        Self {
            home,
            claude_bin: None,
            plugin_dir: None,
            user_home: user_home.into(),
            source_limit_ms: SOURCE_LIMIT_MS,
            env: BTreeMap::new(),
        }
    }

    /// `KAIROS_CLAUDE`와 `KAIROS_PLUGIN_DIR`를 `env`에서 읽는다.
    pub fn from_env(home: KairosHome, user_home: impl Into<PathBuf>, env: BTreeMap<String, String>) -> Self {
        Self {
            claude_bin: env.get("KAIROS_CLAUDE").cloned(),
            plugin_dir: env.get("KAIROS_PLUGIN_DIR").cloned(),
            env,
            ..Self::new(home, user_home)
        }
    }
}

/// 잠자기 판정의 측정 값. 없으면 심장박동과 pmset을 쓴다.
pub trait SleepProbe: Send + Sync {
    /// 심장박동의 가장 긴 공백.
    fn gap_ms(&self) -> i64;
    /// OS의 잠자기 횟수. 모르면 None이다.
    fn count(&self) -> Option<i64>;
}

/// 틱마다 쓰는 실제 측정. 공백은 벽시계로 잰다.
struct LiveSleepProbe {
    stall: Arc<StallWatch>,
    detector: SleepDetector,
}

impl SleepProbe for LiveSleepProbe {
    fn gap_ms(&self) -> i64 {
        self.stall.max_gap_ms()
    }

    fn count(&self) -> Option<i64> {
        self.detector.sleep_count()
    }
}

/// 수집 기록 뒤의 항목 갱신.
pub trait ItemRefresher: Send + Sync {
    fn refresh(&self, database: &Database, input: &RefreshInput<'_>) -> KairosResult<EngineReport>;
}

/// 운영의 항목 엔진.
#[derive(Clone, Copy, Debug, Default)]
pub struct EngineItems;

impl ItemRefresher for EngineItems {
    fn refresh(&self, database: &Database, input: &RefreshInput<'_>) -> KairosResult<EngineReport> {
        ItemEngine::new(database).refresh(input)
    }
}
