//! collect.lock. 틱은 한 번에 하나만 돈다.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kairos_core::home::FILE_MODE;
use kairos_core::ports::Clock;
use kairos_core::{KairosHome, KairosResult, js};
use rustix::io::Errno;
use rustix::process::{Pid, test_kill_process, test_kill_process_group};
use serde_json::{Value, json};

use crate::tick::TOTAL_LIMIT_MS;

/// pid를 확인할 수 없는 잠금을 넘겨받는 시간. 전체 상한의 두 배다.
pub const LOCK_STALE_MS: i64 = TOTAL_LIMIT_MS * 2;
/// pid가 살아 있으면 잠자기에서 깬 앞 틱일 수 있어 넘겨받지 않는다. 다른 프로세스가 pid를 물려받은 경우만 이 시간 뒤에 푼다.
pub const LOCK_LIVE_PID_MS: i64 = 24 * 60 * 60 * 1000;

/// `statSync(path).mtimeMs`. Node가 계산하는 식 그대로라 소수점 아래까지 같다.
pub(crate) fn mtime_ms(path: &Path) -> io::Result<f64> {
    let metadata = fs::metadata(path)?;
    Ok(metadata.mtime() as f64 * 1000.0 + metadata.mtime_nsec() as f64 / 1_000_000.0)
}

/// `rmSync(path, { force: true })`.
fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        outcome => outcome,
    }
}

/// `process.kill(pid, 0)`이 ESRCH로 끝나는가. 살아 있거나 EPERM이거나 pid가 정수 범위 밖이면 거짓이다.
/// EPERM은 다른 사용자(root)의 프로세스가 pid를 물려받은 경우다.
fn pid_is_gone(pid: f64) -> bool {
    let number = pid as i32;
    if f64::from(number) != pid {
        return false;
    }
    let probe = match number {
        0 => return false,
        positive if positive > 0 => Pid::from_raw(positive).map(test_kill_process),
        negative => negative.checked_neg().and_then(Pid::from_raw).map(test_kill_process_group),
    };
    matches!(probe, Some(Err(Errno::SRCH)))
}

/// 잠금 파일. 살아 있는 프로세스가 쥐고 있으면 잡지 못하고, 죽었거나 오래된 잠금은 넘겨받는다.
#[derive(Clone)]
pub struct CollectLock {
    path: PathBuf,
    clock: Arc<dyn Clock>,
}

impl CollectLock {
    pub fn new(home: &KairosHome, clock: Arc<dyn Clock>) -> Self {
        Self { path: home.file("collect.lock"), clock }
    }

    fn ticket(&self) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(".takeover");
        name.into()
    }

    fn create(&self, now: i64) -> KairosResult<bool> {
        let opened = OpenOptions::new().write(true).create_new(true).mode(FILE_MODE).open(&self.path);
        match opened {
            Ok(mut file) => {
                let text = json!({ "pid": std::process::id(), "at": now }).to_string();
                file.write_all(text.as_bytes())?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// 살아 있는 프로세스가 쥐고 있으면 거짓이다.
    pub fn acquire(&self) -> KairosResult<bool> {
        let now = self.clock.now().millis();
        if self.create(now)? {
            return Ok(true);
        }
        if !self.is_stale(now) {
            return Ok(false);
        }
        self.take_over(now)
    }

    /// 오래된 잠금은 표를 쥔 한 프로세스만 지운다. 둘이 함께 오래됐다고 판정해도 뒤의 프로세스가 앞의 새 잠금을 지우지 못한다.
    /// 표는 찰나만 쥔다. 있으면 다른 프로세스가 넘겨받는 중이므로 양보한다. 죽어서 남긴 표는 치우고 이번 틱은 쉰다.
    fn take_over(&self, now: i64) -> KairosResult<bool> {
        let ticket = self.ticket();
        match OpenOptions::new().write(true).create_new(true).mode(FILE_MODE).open(&ticket) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // 그사이 표가 사라졌으면 다음 틱에 다시 본다.
                if mtime_ms(&ticket).is_ok_and(|mtime| now as f64 - mtime > LOCK_STALE_MS as f64) {
                    let _ = remove_if_present(&ticket);
                }
                return Ok(false);
            }
            Err(error) => return Err(error.into()),
        }
        let taken = self.replace_stale(now);
        let cleared = remove_if_present(&ticket);
        let taken = taken?;
        cleared?;
        Ok(taken)
    }

    /// 표를 얻은 뒤 다시 본다. 앞선 프로세스가 이미 넘겨받았을 수 있다.
    fn replace_stale(&self, now: i64) -> KairosResult<bool> {
        if !self.is_stale(now) {
            return Ok(false);
        }
        remove_if_present(&self.path)?;
        self.create(now)
    }

    fn is_stale(&self, now: i64) -> bool {
        let parsed = fs::read(&self.path).ok().and_then(|bytes| js::parse_json(&String::from_utf8_lossy(&bytes)).ok());
        match parsed {
            // 쓰는 도중이거나 깨진 파일. 오래됐을 때만 넘겨받는다.
            None | Some(Value::Null) => {
                mtime_ms(&self.path).map_or(true, |mtime| now as f64 - mtime > LOCK_STALE_MS as f64)
            }
            Some(held) => {
                let Some(pid) = held.get("pid").and_then(Value::as_f64) else {
                    return true;
                };
                let at = held.get("at").and_then(Value::as_f64);
                let overdue = at.is_some_and(|at| now as f64 - at > LOCK_LIVE_PID_MS as f64);
                pid_is_gone(pid) || overdue
            }
        }
    }

    pub fn release(&self) -> KairosResult<()> {
        Ok(remove_if_present(&self.path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairos_core::Instant;
    use kairos_core::ports::{FixedClock, SystemClock};

    const DEAD_PID: i64 = (1 << 22) + 1;

    fn lock_at(home: &KairosHome, clock: Arc<dyn Clock>) -> CollectLock {
        CollectLock::new(home, clock)
    }

    fn system_lock(home: &KairosHome) -> CollectLock {
        lock_at(home, Arc::new(SystemClock))
    }

    fn later_lock(home: &KairosHome, ms: i64) -> CollectLock {
        lock_at(home, Arc::new(FixedClock(Instant::from_millis(SystemClock.now().millis() + ms))))
    }

    fn held(home: &KairosHome, pid: i64, age_ms: i64) {
        let at = SystemClock.now().millis() - age_ms;
        fs::write(home.file("collect.lock"), json!({ "pid": pid, "at": at }).to_string()).unwrap();
    }

    fn temp_home() -> (tempfile::TempDir, KairosHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = KairosHome::at(dir.path());
        (dir, home)
    }

    #[test]
    fn 죽은_프로세스의_잠금은_넘겨받는다() {
        let (_dir, home) = temp_home();
        held(&home, DEAD_PID, 0);
        assert!(system_lock(&home).acquire().unwrap());
    }

    #[test]
    fn pid가_살아_있으면_잠자기에서_깬_앞_틱일_수_있어_시간이_지났어도_넘겨받지_않는다() {
        let (_dir, home) = temp_home();
        held(&home, i64::from(std::process::id()), 3_600_000);
        assert!(!system_lock(&home).acquire().unwrap());
        held(&home, i64::from(std::process::id()), 25 * 3_600_000);
        assert!(system_lock(&home).acquire().unwrap(), "하루가 지나면 pid를 물려받은 다른 프로세스로 본다");
    }

    /// 원본은 process.kill을 가짜로 바꿨다. pid 1은 root의 launchd라 실제 EPERM이 돌아온다.
    #[test]
    fn pid가_다른_사용자_프로세스로_재사용되면_eperm_하루가_지난_잠금만_넘겨받는다() {
        let (_dir, home) = temp_home();
        assert_eq!(test_kill_process(Pid::from_raw(1).unwrap()), Err(Errno::PERM));
        held(&home, 1, 3_600_000);
        assert!(!system_lock(&home).acquire().unwrap());
        held(&home, 1, 25 * 3_600_000);
        assert!(system_lock(&home).acquire().unwrap());
    }

    #[test]
    fn 넘겨받기_직전에_앞선_프로세스가_잡은_잠금은_지우지_않는다() {
        let (_dir, home) = temp_home();
        held(&home, i64::from(std::process::id()), 0);
        assert!(!system_lock(&home).take_over(SystemClock.now().millis()).unwrap());
        assert!(home.file("collect.lock").exists());
        assert!(!home.file("collect.lock.takeover").exists());
    }

    #[test]
    fn 넘겨받는_중인_프로세스가_있으면_양보하고_죽어서_남긴_표는_치운_뒤_다음_틱에_넘겨받는다() {
        let (_dir, home) = temp_home();
        held(&home, DEAD_PID, 0);
        fs::write(home.file("collect.lock.takeover"), "").unwrap();
        assert!(!system_lock(&home).acquire().unwrap());
        assert!(home.file("collect.lock.takeover").exists(), "방금 생긴 표는 치우지 않는다");
        assert!(!later_lock(&home, 3 * 60_000).acquire().unwrap(), "오래된 표를 치운 틱도 양보한다");
        assert!(!home.file("collect.lock.takeover").exists());
        assert!(system_lock(&home).acquire().unwrap());
    }

    #[test]
    fn 잠금_파일은_0600이고_pid와_시각을_적는다() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, home) = temp_home();
        let clock = Arc::new(FixedClock(Instant::from_millis(1234)));
        assert!(lock_at(&home, clock.clone()).acquire().unwrap());
        let path = home.file("collect.lock");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, format!(r#"{{"pid":{},"at":1234}}"#, std::process::id()));
        lock_at(&home, clock).release().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn 깨진_잠금은_오래됐을_때만_넘겨받고_pid_없는_잠금은_바로_넘겨받는다() {
        let (_dir, home) = temp_home();
        fs::write(home.file("collect.lock"), "{깨진").unwrap();
        assert!(!system_lock(&home).acquire().unwrap());
        assert!(later_lock(&home, LOCK_STALE_MS + 60_000).acquire().unwrap());
        fs::write(home.file("collect.lock"), "null").unwrap();
        assert!(!system_lock(&home).acquire().unwrap(), "JS에서 null.pid는 던지고 깨진 파일과 같아진다");
        fs::write(home.file("collect.lock"), "{}").unwrap();
        assert!(system_lock(&home).acquire().unwrap());
    }

    /// 원본은 프로세스 8개를 띄웠다. 잠금 판정은 프로세스를 구별하지 않으므로 스레드로 같은 경쟁을 만든다.
    #[test]
    fn 죽은_pid의_잠금을_여러_프로세스가_동시에_넘겨받아도_하나만_잡는다() {
        for _ in 0..30 {
            let (_dir, home) = temp_home();
            held(&home, DEAD_PID, 0);
            let barrier = Arc::new(std::sync::Barrier::new(8));
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    let (home, barrier) = (home.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        system_lock(&home).acquire().unwrap()
                    })
                })
                .collect();
            let got = workers.into_iter().map(|worker| worker.join().unwrap()).filter(|got| *got).count();
            assert_eq!(got, 1);
        }
    }
}
