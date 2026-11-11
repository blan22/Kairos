use std::ffi::OsStr;
use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::LazyLock;

use kairos_core::KairosResult;
use kairos_source_mcp::shared::ModeFailure;
use regex::Regex;

use super::{RESULTS_DIR, SESSION_ID};
use crate::fs_util::remove_if_present;
use crate::posix::{basename, dirname, normalize};

static RESULT_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]*$").expect("상수 정규식"));

/// 경로의 폴더가 링크가 아닌 실제 폴더인가. 없으면 None이다.
pub(super) fn is_real_directory(path: &Path) -> Option<bool> {
    fs::symlink_metadata(path).ok().map(|metadata| metadata.is_dir())
}

/// 표식이 가리킨 파일 하나. 경로는 외부 입력이다. 원본과 같은 순서로 하나라도 어긋나면 열지 않는다.
pub(super) struct ResultFile<'a> {
    pub path: &'a str,
    pub project_dir: &'a Path,
    pub max_bytes: u64,
}

impl ResultFile<'_> {
    fn results_dir(&self) -> &str {
        dirname(self.path)
    }

    fn session_dir(&self) -> &str {
        dirname(self.results_dir())
    }

    /// 글자만으로 판정한다. `..`과 `//`가 든 경로는 정규화해서 살리지 않고 거부한다.
    fn is_result_path(&self) -> bool {
        self.path.starts_with('/')
            && normalize(self.path) == self.path
            && basename(self.results_dir()) == RESULTS_DIR
            && OsStr::new(dirname(self.session_dir())) == self.project_dir.as_os_str()
            && SESSION_ID.is_match(basename(self.session_dir()))
            && RESULT_FILE.is_match(basename(self.path))
    }

    /// 연 파일이 지금 그 경로의 파일이고 기대한 폴더 안에 있는가. 검사와 사용 사이에 폴더가 바뀌면 거짓이다.
    fn is_still_in_place(&self, opened: &Metadata) -> bool {
        let check = || -> io::Result<bool> {
            let current = fs::symlink_metadata(self.path)?;
            let expected = fs::canonicalize(self.project_dir)?.join(basename(self.session_dir())).join(RESULTS_DIR);
            Ok(opened.dev() == current.dev()
                && opened.ino() == current.ino()
                && fs::canonicalize(self.results_dir())? == expected)
        };
        check().unwrap_or(false)
    }

    /// 파일을 읽는다. 바깥 `Err`는 원본에서 던지던 입출력 실패이고, 안쪽 `Err`는 응답의 고정 실패 코드다.
    pub fn read(&self, consume: bool) -> KairosResult<Result<String, ModeFailure>> {
        if !self.is_result_path() {
            return Ok(Err(ModeFailure::RecoverOutside));
        }
        for directory in [self.project_dir, Path::new(self.session_dir()), Path::new(self.results_dir())] {
            match is_real_directory(directory) {
                None => return Ok(Err(ModeFailure::RecoverMissing)),
                Some(false) => return Ok(Err(ModeFailure::RecoverOutside)),
                Some(true) => {}
            }
        }
        // O_NOFOLLOW는 마지막 조각의 링크를 막고, O_NONBLOCK은 FIFO를 여는 데서 멈추지 않게 한다.
        let opened = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(self.path);
        let mut file = match opened {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Err(ModeFailure::RecoverMissing)),
            Err(_) => return Ok(Err(ModeFailure::RecoverOutside)),
        };
        let stat = file.metadata()?;
        // 하드 링크면 밖의 파일이 이름만 안에 있는 것이다.
        if !stat.is_file() || stat.nlink() > 1 || !self.is_still_in_place(&stat) {
            return Ok(Err(ModeFailure::RecoverOutside));
        }
        if stat.size() > self.max_bytes {
            if consume {
                remove_if_present(Path::new(self.path))?;
            }
            return Ok(Err(ModeFailure::RecoverTooLarge));
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if consume {
            remove_if_present(Path::new(self.path))?;
        }
        Ok(Ok(text))
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    const SESSION: &str = "4f2a1c3e-0b6d-4e8f-9a7b-1c2d3e4f5a6b";

    #[test]
    fn 연_뒤에_경로의_파일이_바뀌면_제자리가_아니다() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let results = project.join(SESSION).join(RESULTS_DIR);
        fs::create_dir_all(&results).unwrap();
        let path = results.join("toolu_a.txt");
        fs::write(&path, "first").unwrap();
        let text = path.to_string_lossy();
        let file = ResultFile { path: &text, project_dir: &project, max_bytes: 100 };
        let opened = File::open(&path).unwrap().metadata().unwrap();
        assert!(file.is_still_in_place(&opened));

        fs::write(dir.path().join("swap"), "second").unwrap();
        fs::rename(dir.path().join("swap"), &path).unwrap();
        assert!(!file.is_still_in_place(&opened));
    }

    #[test]
    fn 연_뒤에_폴더가_다른_곳으로_바뀌면_제자리가_아니다() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let results = project.join(SESSION).join(RESULTS_DIR);
        fs::create_dir_all(&results).unwrap();
        let path = results.join("toolu_a.txt");
        fs::write(&path, "first").unwrap();
        let text = path.to_string_lossy();
        let file = ResultFile { path: &text, project_dir: &project, max_bytes: 100 };
        let opened = File::open(&path).unwrap().metadata().unwrap();

        let elsewhere = dir.path().join("elsewhere");
        fs::rename(&results, &elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &results).unwrap();
        assert!(!file.is_still_in_place(&opened), "inode가 같아도 폴더가 밖이면 거부한다");
    }

    #[test]
    fn 폴더가_사라지면_제자리가_아니다() {
        let dir = tempfile::tempdir().unwrap();
        let kept = dir.path().join("kept");
        fs::write(&kept, "x").unwrap();
        let opened = File::open(&kept).unwrap().metadata().unwrap();
        let file = ResultFile { path: "/nonexistent/x", project_dir: Path::new("/nonexistent"), max_bytes: 100 };
        assert!(!file.is_still_in_place(&opened));
    }
}
