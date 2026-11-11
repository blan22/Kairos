//! 'PATH의 상대 경로 항목은 실행 파일 후보가 아니다'.
//! 현재 폴더를 바꾸므로 프로세스를 따로 쓰게 파일을 나눴다. 이 파일에는 시험을 더 두지 않는다.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use kairos_ops::find_binary;

#[test]
fn path의_상대_경로_항목은_실행_파일_후보가_아니다() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().join("cwd");
    fs::create_dir(&cwd).unwrap();
    let binary = cwd.join("kairos-synthetic-binary");
    fs::write(&binary, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_current_dir(&cwd).unwrap();
    assert_eq!(find_binary("kairos-synthetic-binary", ".:/usr/bin"), None);
    let absolute = format!("{}:/usr/bin", cwd.display());
    assert_eq!(find_binary("kairos-synthetic-binary", &absolute), Some(binary.to_string_lossy().into_owned()));
}
