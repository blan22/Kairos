mod repo;

use std::fs::{self, DirBuilder, Permissions};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use kairos_core::home::{DIR_MODE, FILE_MODE, write_file_atomic};
use kairos_core::{Instant, KairosError, KairosHome, KairosResult, LocalDate, js};
use kairos_ledger::{Invariants, check_invariants};
use kairos_store::Connection;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::fs_util::remove_if_present;
pub use repo::{MaintenanceRepo, OPEN_TEXT_RETENTION_DAYS};

pub const BACKUP_KEEP: usize = 7;
static BACKUP_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^kairos-[0-9]{4}-[0-9]{2}-[0-9]{2}\.db$").expect("상수 정규식"));

pub fn backup_dir(home: &KairosHome) -> PathBuf {
    home.file("backup")
}

pub fn maintenance_path(home: &KairosHome) -> PathBuf {
    home.file("maintenance.json")
}

/// maintenance.json. 옛 기록에 없는 건수는 0으로 읽는다. 원본의 읽는 쪽이 `?? 0`으로 받던 값이다.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceRecord {
    /// 마지막으로 정비를 마친 KST 날짜.
    pub date: LocalDate,
    #[serde(default)]
    pub invariants: Invariants,
    #[serde(default)]
    pub events_deleted: i64,
    #[serde(default)]
    pub objects_deleted: i64,
    #[serde(default)]
    pub runs_deleted: i64,
    #[serde(default)]
    pub backup_file: Option<String>,
}

impl MaintenanceRecord {
    /// 없거나 깨졌으면 None이다.
    pub fn read(home: &KairosHome) -> Option<Self> {
        let text = fs::read_to_string(maintenance_path(home)).ok()?;
        serde_json::from_value(js::parse_json(&text).ok()?).ok()
    }

    fn write(&self, home: &KairosHome) -> KairosResult<()> {
        let text = serde_json::to_string_pretty(self).map_err(|_| KairosError::new("maintenance_unserializable"))?;
        write_file_atomic(&maintenance_path(home), &format!("{text}\n"))
    }
}

/// 하루 한 번의 정비. 백업, 보존 정리, 불변식 검사를 맡는다.
#[derive(Clone, Copy, Debug)]
pub struct Maintenance<'a> {
    connection: &'a Connection,
    home: &'a KairosHome,
}

impl<'a> Maintenance<'a> {
    pub const fn new(connection: &'a Connection, home: &'a KairosHome) -> Self {
        Self { connection, home }
    }

    /// 하루에 한 번 VACUUM INTO로 복사본을 만든다. 같은 권한(0700/0600)이고 최근 7개만 둔다.
    pub fn backup(&self, now: Instant) -> KairosResult<Option<PathBuf>> {
        let directory = backup_dir(self.home);
        DirBuilder::new().recursive(true).mode(DIR_MODE).create(&directory)?;
        fs::set_permissions(&directory, Permissions::from_mode(DIR_MODE))?;
        let target = directory.join(format!("kairos-{}.db", now.local_date()));
        let mut created = None;
        if !target.exists() {
            MaintenanceRepo::new(self.connection).vacuum_into(&target)?;
            fs::set_permissions(&target, Permissions::from_mode(FILE_MODE))?;
            created = Some(target);
        }
        remove_stale_backups(&directory)?;
        Ok(created)
    }

    /// KST 날짜가 바뀐 뒤 처음 도는 수집에서 정비한다. 백업, 보존 정리, 불변식 검사 순서다.
    pub fn run_daily(&self, now: Instant, force: bool) -> KairosResult<Option<MaintenanceRecord>> {
        let today = now.local_date();
        if !force && MaintenanceRecord::read(self.home).is_some_and(|record| record.date == today) {
            return Ok(None);
        }
        // 백업을 먼저 한다. 정리가 잘못돼도 어제까지의 사본이 남는다.
        let backup = self.backup(now)?;
        let repo = MaintenanceRepo::new(self.connection);
        repo.prune_items(now)?;
        let events_deleted = repo.prune_events(now)?;
        let objects_deleted = repo.prune_objects(now)?;
        repo.scrub_stale_open(now)?;
        repo.prune_thread_reads(now)?;
        let runs_deleted = repo.prune_runs(now)?;
        let record = MaintenanceRecord {
            date: today,
            invariants: check_invariants(self.connection)?,
            events_deleted,
            objects_deleted,
            runs_deleted,
            backup_file: backup.map(|path| path.to_string_lossy().into_owned()),
        };
        record.write(self.home)?;
        Ok(Some(record))
    }
}

fn remove_stale_backups(directory: &Path) -> KairosResult<()> {
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if BACKUP_NAME.is_match(&name) {
            names.push(name);
        }
    }
    names.sort();
    for stale in &names[..names.len().saturating_sub(BACKUP_KEEP)] {
        remove_if_present(&directory.join(stale))?;
    }
    Ok(())
}
