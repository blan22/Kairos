use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use kairos_core::{KairosHome, KairosResult};
use kairos_ops::backup_dir;

const PRIVATE_FILES: [&str; 16] = [
    "kairos.db",
    "kairos.db-wal",
    "kairos.db-shm",
    "state.json",
    "control.json",
    "engine.json",
    "auth.json",
    "watch.tsv",
    "watch-window.tsv",
    "halt.json",
    "maintenance.json",
    "collector.json",
    "watchdog.last",
    "watchdog.beat",
    "resumed.at",
    "collect.lock",
];

pub(super) fn mode_of(path: &Path) -> KairosResult<String> {
    Ok(format!("{:03o}", fs::metadata(path)?.permissions().mode() & 0o777))
}

fn check(path: &Path, expected: &str, problems: &mut Vec<String>) -> KairosResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let mode = mode_of(path)?;
    if mode != expected {
        problems.push(format!("권한 {mode} (기대 {expected}): {}", path.display()));
    }
    Ok(())
}

/// 0600이어야 하는 파일과 0700이어야 하는 폴더 중 기대와 다른 것.
pub fn permission_problems(home: &KairosHome) -> KairosResult<Vec<String>> {
    let mut problems = Vec::new();
    check(home.root(), "700", &mut problems)?;
    for name in PRIVATE_FILES {
        check(&home.file(name), "600", &mut problems)?;
    }
    for directory in [backup_dir(home), home.file("logs")] {
        check(&directory, "700", &mut problems)?;
        if !directory.exists() {
            continue;
        }
        // readdirSync는 이름 바이트순이다.
        let mut names: Vec<_> =
            fs::read_dir(&directory)?.map(|entry| entry.map(|entry| entry.file_name())).collect::<Result<_, _>>()?;
        names.sort();
        for name in names {
            check(&directory.join(name), "600", &mut problems)?;
        }
    }
    Ok(problems)
}
