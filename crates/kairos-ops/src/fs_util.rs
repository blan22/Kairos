use std::path::Path;
use std::{fs, io};

/// `rmSync(path, { force: true })`. 이미 없는 것은 실패가 아니다.
pub(crate) fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        outcome => outcome,
    }
}
