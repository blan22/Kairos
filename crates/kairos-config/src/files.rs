//! 설정 파일 읽기. 설정은 T1 판정과 세션 자격을 바꾸므로 남이 바꿀 수 있는 파일은 읽지 않는다.

use std::fs::{self, File, Metadata};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::ConfigError;

/// 설정 파일 하나의 상한. 손으로 쓰는 파일이라 넉넉하다.
const MAX_BYTES: u64 = 64 * 1024;

/// 없으면 None. 링크, 일반 파일이 아닌 것, 데이터 폴더 주인의 것이 아닌 것, 그룹이나 남이 쓸 수 있는 것,
/// 하드 링크가 둘 이상인 것, 상한을 넘는 것은 거부한다. 검사한 파일과 연 파일이 다르면 거부한다.
pub(crate) fn read_owned(root: &Path, name: &'static str) -> Result<Option<String>, ConfigError> {
    let path = root.join(name);
    let linked = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        // 데이터 폴더가 아직 없거나 폴더가 아니면 설정도 없다. 그 오류는 데이터 폴더를 여는 쪽이 낸다.
        Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => {
            return Ok(None);
        }
        Err(_) => return Err(ConfigError::file(name, "config_unreadable")),
    };
    let owner = fs::metadata(root).map_err(|_| ConfigError::file(name, "config_unreadable"))?.uid();
    let unsafe_file = |metadata: &Metadata| {
        !metadata.is_file()
            || metadata.mode() & 0o022 != 0
            || metadata.uid() != owner
            || metadata.nlink() != 1
            || metadata.len() > MAX_BYTES
    };
    if linked.file_type().is_symlink() || unsafe_file(&linked) {
        return Err(ConfigError::file(name, "config_unsafe"));
    }
    let file = File::open(&path).map_err(|_| ConfigError::file(name, "config_unreadable"))?;
    let opened = file.metadata().map_err(|_| ConfigError::file(name, "config_unreadable"))?;
    if opened.dev() != linked.dev() || opened.ino() != linked.ino() || unsafe_file(&opened) {
        return Err(ConfigError::file(name, "config_unsafe"));
    }
    let mut content = String::new();
    file.take(MAX_BYTES).read_to_string(&mut content).map_err(|_| ConfigError::file(name, "config_not_utf8"))?;
    Ok(Some(content))
}
