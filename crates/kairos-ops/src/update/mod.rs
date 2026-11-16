//! 새 판 확인. 릴리스 저장소의 최신 판을 gh로 묻고 데이터 폴더의 `update.json`에 적어 둔다.
//!
//! 확인은 판 번호만 읽는다. 내려받지 않는다. 설치는 사용자가 터미널에서 `kairos update --apply`를 실행할 때만 한다.
//! 판 번호는 `v?N.N.N` 모양만 받는다. 알림 문구와 패널 띠에 들어가므로 다른 글자를 넣지 않는다.
//! 묶음을 받아 확인하고 푸는 일은 `bundle`이 맡는다.

mod bundle;

pub use bundle::{BundleFetcher, RELEASE_WORKFLOW, asset_name};

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kairos_core::home::write_file_atomic;
use kairos_core::process::{ProcessRequest, ProcessRunner};
use kairos_core::{Instant, KairosHome, KairosResult, js};
use serde_json::{Value, json};

/// 릴리스를 올리는 저장소. 설치 스크립트의 주소와 같다.
pub const RELEASE_REPO: &str = "blan22/Kairos";
pub const UPDATE_FILE: &str = "update.json";
/// 확인에 성공한 뒤 다시 볼 때까지.
pub const UPDATE_CHECK_INTERVAL_MS: i64 = 24 * 3_600_000;
/// 확인에 실패한 뒤 다시 볼 때까지. gh 로그인이 풀린 날에도 하루를 기다리지 않는다.
pub const UPDATE_RETRY_INTERVAL_MS: i64 = 3 * 3_600_000;
/// 알림 제목. 본문은 `update_body`다.
pub const UPDATE_TITLE: &str = "kairos 업데이트";
/// 새 판을 올리는 명령. 알림, 패널 띠, CLI 안내가 같은 글을 쓴다.
pub const UPDATE_APPLY_COMMAND: &str = "kairos update --apply";

const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE_MAX_OUTPUT: usize = 4 * 1024;
const MAX_FILE_BYTES: u64 = 4 * 1024;
/// 판 번호 한 자리의 최대 길이.
const MAX_PART_DIGITS: usize = 4;

/// `주.부.수` 판 번호. 미리 보기 판(`-rc1` 같은 꼬리)은 받지 않는다. 최신 릴리스는 미리 보기 판이 아니다.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self { major, minor, patch }
    }

    /// `0.4.2`나 `v0.4.2`. 자리마다 1~4자리 숫자다. 앞뒤 공백은 뗀다.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || {
            let part = parts.next()?;
            let digits = !part.is_empty() && part.len() <= MAX_PART_DIGITS && part.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| part.parse().ok()).flatten()
        };
        let version = Self::new(next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }

    /// 지금 도는 실행 파일의 판.
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self::new(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// 알림 본문. 판 번호 말고는 고정이다.
pub fn update_body(latest: Version) -> String {
    format!("새 판 {latest}이 나왔습니다. 터미널에서 {UPDATE_APPLY_COMMAND}를 실행하세요")
}

/// 최신 릴리스의 판을 묻는다. 모르면 None이다.
pub trait ReleaseProbe: Send + Sync {
    fn latest(&self) -> Option<Version>;
}

/// gh로 최신 릴리스의 태그를 읽는다. 셸 없이 인자 배열로 띄우고, 환경은 고른 값만 넘긴다.
pub struct GhReleaseProbe {
    runner: Arc<dyn ProcessRunner>,
    gh: String,
    env: BTreeMap<String, String>,
}

impl GhReleaseProbe {
    /// `env`는 프로세스 환경이다. PATH, HOME, LANG만 넘기고 gh가 묻거나 색을 넣지 않게 한다.
    pub fn new(runner: Arc<dyn ProcessRunner>, gh: impl Into<String>, env: &BTreeMap<String, String>) -> Self {
        Self { runner, gh: gh.into(), env: gh_env(env) }
    }

    /// 최신 릴리스 태그를 묻는 인자. 고정 문자열이다.
    pub fn args() -> [String; 4] {
        ["api".to_owned(), format!("repos/{RELEASE_REPO}/releases/latest"), "--jq".to_owned(), ".tag_name".to_owned()]
    }
}

/// gh 자식의 환경. 부모 환경을 물려주지 않는다.
pub fn gh_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let read = |key: &str, fallback: &str| env.get(key).cloned().unwrap_or_else(|| fallback.to_owned());
    [
        ("PATH", read("PATH", "/usr/bin:/bin")),
        ("HOME", read("HOME", "")),
        ("LANG", read("LANG", "en_US.UTF-8")),
        ("GH_HOST", "github.com".to_owned()),
        ("GH_PROMPT_DISABLED", "1".to_owned()),
        ("GH_NO_UPDATE_NOTIFIER", "1".to_owned()),
        ("NO_COLOR", "1".to_owned()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect()
}

impl ReleaseProbe for GhReleaseProbe {
    fn latest(&self) -> Option<Version> {
        let request = ProcessRequest::new(self.gh.clone(), Self::args(), PROBE_TIMEOUT)
            .env(self.env.clone())
            .max_output(PROBE_MAX_OUTPUT);
        let output = self.runner.run(&request);
        if !output.success() {
            return None;
        }
        // 출력은 태그 한 줄이다. 다른 모양이면 모르는 것으로 본다.
        let mut lines = output.stdout.lines().filter(|line| !line.trim().is_empty());
        let version = Version::parse(lines.next()?)?;
        lines.next().is_none().then_some(version)
    }
}

/// `update.json`의 내용.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UpdateRecord {
    pub checked_at: Option<Instant>,
    /// 마지막 확인이 성공했는가.
    pub ok: bool,
    /// 마지막으로 알아낸 최신 판. 확인에 실패해도 앞 값을 지킨다.
    pub latest: Option<Version>,
    /// 알림을 보낸 판. 판마다 한 번만 알린다.
    pub notified: Option<Version>,
}

impl UpdateRecord {
    /// `current`보다 새 판이 있으면 그 판이다.
    pub fn available(&self, current: Version) -> Option<Version> {
        self.latest.filter(|latest| *latest > current)
    }

    /// 새 판이 있고 아직 알리지 않았으면 그 판이다.
    pub fn unannounced(&self, current: Version) -> Option<Version> {
        self.available(current).filter(|latest| self.notified != Some(*latest))
    }

    /// 다시 확인할 때가 됐는가. 기록이 없거나 시각이 미래면 확인한다.
    pub fn due(&self, now: Instant) -> bool {
        let interval = if self.ok { UPDATE_CHECK_INTERVAL_MS } else { UPDATE_RETRY_INTERVAL_MS };
        self.checked_at.is_none_or(|at| !(0..interval).contains(&now.ms_since(at)))
    }
}

#[derive(Clone, Debug)]
pub struct UpdateFile {
    path: PathBuf,
}

impl UpdateFile {
    pub fn new(home: &KairosHome) -> Self {
        Self { path: home.file(UPDATE_FILE) }
    }

    /// 없거나 깨졌으면 빈 기록이다. 모양이 틀린 값은 버린다.
    pub fn read(&self) -> UpdateRecord {
        let small = fs::metadata(&self.path).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_FILE_BYTES);
        let parsed = small
            .then(|| fs::read(&self.path).ok())
            .flatten()
            .and_then(|bytes| js::parse_json(&String::from_utf8_lossy(&bytes)).ok());
        let Some(Value::Object(body)) = parsed else {
            return UpdateRecord::default();
        };
        let text = |key: &str| body.get(key).and_then(Value::as_str);
        UpdateRecord {
            checked_at: text("checkedAt").and_then(Instant::parse),
            ok: body.get("ok").and_then(Value::as_bool).unwrap_or(false),
            latest: text("latest").and_then(Version::parse),
            notified: text("notified").and_then(Version::parse),
        }
    }

    fn write(&self, record: &UpdateRecord) -> KairosResult<()> {
        let version = |value: Option<Version>| value.map(|version| version.to_string());
        let body = json!({
            "checkedAt": record.checked_at.map(Instant::to_iso),
            "ok": record.ok,
            "latest": version(record.latest),
            "notified": version(record.notified),
        });
        let mut text = body.to_string();
        text.push('\n');
        write_file_atomic(&self.path, &text)
    }

    /// 때가 됐으면 확인하고 적는다. 때가 아니면 적힌 기록을 그대로 돌려준다.
    pub fn refresh(&self, now: Instant, probe: &dyn ReleaseProbe) -> KairosResult<UpdateRecord> {
        let record = self.read();
        if !record.due(now) {
            return Ok(record);
        }
        self.check_now(now, probe)
    }

    /// 지금 확인하고 적는다. 실패하면 앞의 최신 판을 지키고 `ok`만 내린다.
    pub fn check_now(&self, now: Instant, probe: &dyn ReleaseProbe) -> KairosResult<UpdateRecord> {
        let mut record = self.read();
        let latest = probe.latest();
        record.checked_at = Some(now);
        record.ok = latest.is_some();
        record.latest = latest.or(record.latest);
        self.write(&record)?;
        Ok(record)
    }

    pub fn mark_notified(&self, version: Version) -> KairosResult<()> {
        let mut record = self.read();
        record.notified = Some(version);
        self.write(&record)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kairos_core::process::ProcessOutput;

    use super::*;

    struct Fixed(Option<Version>);

    impl ReleaseProbe for Fixed {
        fn latest(&self) -> Option<Version> {
            self.0
        }
    }

    struct Gh {
        stdout: &'static str,
        code: i32,
        seen: Mutex<Vec<ProcessRequest>>,
    }

    impl ProcessRunner for Gh {
        fn run(&self, request: &ProcessRequest) -> ProcessOutput {
            self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(request.clone());
            ProcessOutput::exited(self.code, self.stdout)
        }
    }

    fn gh(stdout: &'static str, code: i32) -> Arc<Gh> {
        Arc::new(Gh { stdout, code, seen: Mutex::new(Vec::new()) })
    }

    fn at(iso: &str) -> Instant {
        Instant::parse(iso).unwrap_or(Instant::from_millis(0))
    }

    #[test]
    fn 판_번호는_세_자리_숫자만_받는다() {
        assert_eq!(Version::parse("0.4.2"), Some(Version::new(0, 4, 2)));
        assert_eq!(Version::parse(" v10.0.12\n"), Some(Version::new(10, 0, 12)));
        for bad in ["", "v", "1.2", "1.2.3.4", "1.2.3-rc1", "1..3", "+1.2.3", "1.2.x", "12345.0.0", "v 1.2.3", "１.2.3"]
        {
            assert_eq!(Version::parse(bad), None, "{bad:?}");
        }
        assert!(Version::new(0, 10, 0) > Version::new(0, 9, 9));
        assert!(Version::new(1, 0, 0) > Version::new(0, 99, 99));
        assert_eq!(Version::new(0, 5, 0).to_string(), "0.5.0");
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn gh는_고정_인자와_고른_환경으로만_띄운다() {
        let runner = gh("v0.5.0\n", 0);
        let env: BTreeMap<String, String> =
            [("PATH", "/synthetic/bin"), ("HOME", "/synthetic/home"), ("GH_TOKEN", "secret"), ("GH_HOST", "evil")]
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect();
        let probe = GhReleaseProbe::new(runner.clone(), "/synthetic/gh", &env);
        assert_eq!(probe.latest(), Some(Version::new(0, 5, 0)));
        let seen = runner.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(seen[0].program, "/synthetic/gh");
        assert_eq!(seen[0].args, ["api", "repos/blan22/Kairos/releases/latest", "--jq", ".tag_name"]);
        let child = seen[0].env.as_ref().map(|env| env.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
        assert!(!child.contains(&"GH_TOKEN".to_owned()), "{child:?}");
        assert_eq!(seen[0].env.as_ref().and_then(|env| env.get("GH_HOST")).map(String::as_str), Some("github.com"));
    }

    #[test]
    fn gh가_실패하거나_모양이_틀리면_모른다() {
        let env = BTreeMap::new();
        for (stdout, code) in
            [("v0.5.0\n", 1), ("", 0), ("v0.5.0\nv0.6.0\n", 0), ("null\n", 0), ("\u{1b}[31mv1.0.0\n", 0)]
        {
            assert_eq!(GhReleaseProbe::new(gh(stdout, code), "gh", &env).latest(), None, "{stdout:?} {code}");
        }
    }

    #[test]
    fn 확인은_성공하면_하루_실패하면_세_시간_뒤에_다시_한다() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let home = KairosHome::at(dir.path());
        let file = UpdateFile::new(&home);
        let start = at("2026-11-16T01:00:00.000Z");
        let first = file.refresh(start, &Fixed(Some(Version::new(0, 5, 0)))).unwrap_or_default();
        assert_eq!((first.ok, first.latest), (true, Some(Version::new(0, 5, 0))));
        // 하루 안에는 묻지 않는다.
        let later = file.refresh(start.plus_ms(UPDATE_CHECK_INTERVAL_MS - 1), &Fixed(None)).unwrap_or_default();
        assert!(later.ok);
        // 하루가 지나면 묻는다. 실패해도 앞 판을 지킨다.
        let failed = file.refresh(start.plus_ms(UPDATE_CHECK_INTERVAL_MS), &Fixed(None)).unwrap_or_default();
        assert_eq!((failed.ok, failed.latest), (false, Some(Version::new(0, 5, 0))));
        let retry_at = start.plus_ms(UPDATE_CHECK_INTERVAL_MS + UPDATE_RETRY_INTERVAL_MS);
        assert!(!failed.due(retry_at.minus_ms(1)));
        assert!(failed.due(retry_at));
        // 시계가 뒤로 가면 다시 본다.
        assert!(failed.due(start));
    }

    #[test]
    fn 새_판은_판마다_한_번만_알린다() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let file = UpdateFile::new(&KairosHome::at(dir.path()));
        let current = Version::new(0, 4, 2);
        let now = at("2026-11-16T01:00:00.000Z");
        let record = file.check_now(now, &Fixed(Some(Version::new(0, 4, 2)))).unwrap_or_default();
        assert_eq!(record.available(current), None);
        let record = file.check_now(now, &Fixed(Some(Version::new(0, 5, 0)))).unwrap_or_default();
        assert_eq!(record.unannounced(current), Some(Version::new(0, 5, 0)));
        file.mark_notified(Version::new(0, 5, 0)).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(file.read().unannounced(current), None);
        assert_eq!(file.read().available(current), Some(Version::new(0, 5, 0)));
        let record = file.check_now(now, &Fixed(Some(Version::new(0, 6, 0)))).unwrap_or_default();
        assert_eq!(record.unannounced(current), Some(Version::new(0, 6, 0)));
    }

    #[test]
    fn 깨진_기록은_빈_기록으로_읽는다() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let home = KairosHome::at(dir.path());
        let file = UpdateFile::new(&home);
        assert_eq!(file.read(), UpdateRecord::default());
        for text in ["", "[]", "{\"latest\": \"0.5.0; rm\"}", "{\"latest\": 5}"] {
            fs::write(home.file(UPDATE_FILE), text).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(file.read().latest, None, "{text}");
        }
        fs::write(home.file(UPDATE_FILE), "x".repeat(8 * 1024)).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(file.read(), UpdateRecord::default());
    }
}
