use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::{fs, io};

use kairos_core::KairosResult;
use regex::Regex;
use serde::Serialize;

use super::reader::is_real_directory;
use super::{RESULTS_DIR, SESSION_ID, ToolResults};
use crate::fs_util::remove_if_present;

/// 하네스가 쓰는 이름(측정). toolu_<id>와 토큰 상한 초과분의 mcp-<서버>-<도구>-<ms>.txt. 정리는 이 이름만 지운다.
static HARNESS_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:toolu_|mcp-)").expect("상수 정규식"));

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ToolResultsSweep {
    pub removed: usize,
    pub remaining: usize,
}

fn names_in(directory: &Path) -> KairosResult<Vec<OsString>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    Ok(entries.map(|entry| entry.map(|entry| entry.file_name())).collect::<io::Result<_>>()?)
}

fn name_matches(pattern: &Regex, name: Option<&OsStr>) -> bool {
    name.is_some_and(|name| pattern.is_match(&name.to_string_lossy()))
}

/// 수집기 세션의 하네스 파일과 세션 안의 링크만 지운다. 슬러그가 겹친 다른 폴더의 파일일 수 있어 나머지는 남겨 드러낸다.
fn is_sweepable(entry: &Path) -> bool {
    let is_results = entry.file_name() == Some(OsStr::new(RESULTS_DIR));
    let results = if is_results { Some(entry) } else { entry.parent() };
    let session = results.and_then(Path::parent).and_then(Path::file_name);
    name_matches(&SESSION_ID, session) && (is_results || name_matches(&HARNESS_FILE, entry.file_name()))
}

fn remove_if_empty(directory: &Path) {
    // 비어 있지 않거나 이미 없다.
    let _ = fs::remove_dir(directory);
}

impl ToolResults {
    /// 수집기는 세션 기록을 남기지 않는다. 짝이 되는 기록이 있으면 같은 폴더를 쓰는 대화형 세션이다.
    fn is_user_session(&self, session: &OsStr) -> bool {
        let mut record = session.to_owned();
        record.push(".jsonl");
        self.project_dir.join(record).exists()
    }

    /// 프로젝트 폴더의 세션 폴더 중 tool-results 아래 항목. 링크는 따라가지 않고 링크 자체를 항목으로 센다.
    pub fn entries(&self) -> KairosResult<Vec<PathBuf>> {
        if is_real_directory(&self.project_dir) != Some(true) {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        for session in names_in(&self.project_dir)? {
            let session_dir = self.project_dir.join(&session);
            if is_real_directory(&session_dir) != Some(true) || self.is_user_session(&session) {
                continue;
            }
            let results = session_dir.join(RESULTS_DIR);
            match is_real_directory(&results) {
                None => {}
                Some(false) => entries.push(results),
                Some(true) => entries.extend(names_in(&results)?.iter().map(|name| results.join(name))),
            }
        }
        Ok(entries)
    }

    /// tool-results 아래 파일을 지우고 빈 폴더를 정리한다. 계산한 폴더 밖은 건드리지 않는다.
    pub fn sweep(&self) -> KairosResult<ToolResultsSweep> {
        let mut removed = 0;
        for entry in self.entries()? {
            // 폴더는 만들어질 이유가 없다. 재귀 삭제하지 않고 남은 건수로 드러낸다. 지우지 못한 것도 남은 건수로 드러난다.
            let is_file = fs::symlink_metadata(&entry).is_ok_and(|metadata| !metadata.is_dir());
            if is_sweepable(&entry) && is_file && remove_if_present(&entry).is_ok() {
                removed += 1;
            }
        }
        if is_real_directory(&self.project_dir) == Some(true) {
            for session in names_in(&self.project_dir)? {
                let session_dir = self.project_dir.join(&session);
                let is_collector = SESSION_ID.is_match(&session.to_string_lossy()) && !self.is_user_session(&session);
                if !is_collector || is_real_directory(&session_dir) != Some(true) {
                    continue;
                }
                let results = session_dir.join(RESULTS_DIR);
                if is_real_directory(&results) == Some(true) {
                    remove_if_empty(&results);
                }
                remove_if_empty(&session_dir);
            }
            remove_if_empty(&self.project_dir);
        }
        Ok(ToolResultsSweep { removed, remaining: self.entries()?.len() })
    }
}
