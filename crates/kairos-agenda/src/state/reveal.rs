//! reveal.json. 알림을 누르면 `kairos reveal`이 쓰고, 패널이 `next --json`으로 읽어 그 항목을 맨 위에 보여 준다.

use std::fs;
use std::sync::LazyLock;

use kairos_core::home::write_file_atomic;
use kairos_core::{Instant, KairosError, KairosHome, KairosResult};
use regex::Regex;
use serde::{Deserialize, Serialize};

pub const REVEAL_FILE: &str = "reveal.json";
/// 한 번에 보여 줄 항목의 상한. 헬퍼도 같은 수까지만 넘긴다.
pub const REVEAL_MAX_ITEMS: usize = 20;
/// 이 시간이 지난 요청은 패널이 무시한다. 늦게 연 패널이 오래된 알림으로 화면을 바꾸지 않는다.
pub const REVEAL_WINDOW_MS: i64 = 10 * 60_000;

static ITEM_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[1-9][0-9]{0,14}$").expect("상수 정규식"));

/// 항목 ID 입력 검사. CLI의 ID 검사와 같은 규칙이다.
pub fn is_item_id(value: &str) -> bool {
    ITEM_ID.is_match(value)
}

/// 알림에서 연 항목과 그 시각(ms).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RevealView {
    pub items: Vec<i64>,
    pub at: i64,
}

impl RevealView {
    /// ID가 1개 이상 상한 이하이고 모두 양의 정수인가.
    pub fn valid(&self) -> bool {
        (1..=REVEAL_MAX_ITEMS).contains(&self.items.len()) && self.items.iter().all(|id| is_item_id(&id.to_string()))
    }
}

/// reveal.json의 읽기와 쓰기.
#[derive(Clone, Copy, Debug)]
pub struct RevealFile<'a> {
    home: &'a KairosHome,
}

impl<'a> RevealFile<'a> {
    pub const fn new(home: &'a KairosHome) -> Self {
        Self { home }
    }

    pub fn write(&self, view: &RevealView) -> KairosResult<()> {
        self.home.ensure()?;
        let text = serde_json::to_string(view).map_err(|_| KairosError::new("json_error"))?;
        write_file_atomic(&self.home.file(REVEAL_FILE), &format!("{text}\n"))
    }

    /// 없거나 깨졌거나 ID가 틀렸으면 None이다.
    pub fn read(&self) -> Option<RevealView> {
        let content = fs::read_to_string(self.home.file(REVEAL_FILE)).ok()?;
        serde_json::from_str::<RevealView>(&content).ok().filter(RevealView::valid)
    }

    /// 지금부터 10분 안에 쓴 요청만. 앞날의 시각도 버린다.
    pub fn recent(&self, now: Instant) -> Option<RevealView> {
        self.read().filter(|view| (0..=REVEAL_WINDOW_MS).contains(&(now.millis() - view.at)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 쓴_요청은_10분까지만_읽고_틀린_파일은_없는_것과_같다() {
        let dir = tempfile::tempdir().unwrap();
        let home = KairosHome::at(dir.path().join("home"));
        let file = RevealFile::new(&home);
        assert_eq!(file.read(), None);
        let view = RevealView { items: vec![3, 7], at: 1_000_000 };
        file.write(&view).unwrap();
        assert_eq!(fs::read_to_string(home.file(REVEAL_FILE)).unwrap(), "{\"items\":[3,7],\"at\":1000000}\n");
        assert_eq!(file.recent(Instant::from_millis(1_000_000 + REVEAL_WINDOW_MS)), Some(view.clone()));
        assert_eq!(file.recent(Instant::from_millis(1_000_001 + REVEAL_WINDOW_MS)), None);
        assert_eq!(file.recent(Instant::from_millis(999_999)), None, "앞날의 시각");
        for broken in ["{", "{\"items\":[],\"at\":1}", "{\"items\":[0],\"at\":1}", "{\"items\":[-1],\"at\":1}", "[]"] {
            fs::write(home.file(REVEAL_FILE), broken).unwrap();
            assert_eq!(file.read(), None, "{broken}");
        }
        let many = RevealView { items: (1..=21).collect(), at: 1 };
        assert!(!many.valid());
    }
}
