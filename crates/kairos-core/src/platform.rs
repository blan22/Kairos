//! 운영체제마다 다른 값. 백엔드는 실행 시 `Os`로 고르고, 컴파일이 갈리는 API(unix 권한, 링크)만 cfg로 나눈다.
//! 백엔드를 실행 시 고르므로 어느 호스트에서나 가짜 러너로 다른 운영체제의 동작을 시험할 수 있다.

use std::fs::{self, DirBuilder, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

impl Os {
    /// 빌드한 운영체제. macOS와 Windows가 아닌 unix는 Linux로 본다.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MacOs => "macos",
            Self::Linux => "linux",
            Self::Windows => "windows",
        }
    }

    /// PATH 항목 구분자.
    pub const fn path_list_separator(self) -> char {
        match self {
            Self::Windows => ';',
            _ => ':',
        }
    }

    /// 실행 파일 확장자. Windows만 `.exe`다.
    pub const fn exe_suffix(self) -> &'static str {
        match self {
            Self::Windows => ".exe",
            _ => "",
        }
    }

    /// 이름에 실행 파일 확장자를 붙인다.
    pub fn exe(self, name: &str) -> String {
        format!("{name}{}", self.exe_suffix())
    }

    /// 기본 데이터 폴더. 환경 변수는 보지 않는다. 호스트 어댑터(TS)도 사용자 홈만으로 같은 폴더를 찾아야 한다.
    pub fn default_data_dir(self, user_home: &Path) -> PathBuf {
        match self {
            Self::MacOs => user_home.join("Library").join("Application Support").join("kairos"),
            Self::Linux => user_home.join(".local").join("share").join("kairos"),
            Self::Windows => user_home.join("AppData").join("Local").join("kairos"),
        }
    }

    /// 에이전트 세션을 띄울 수 있는가. 세션 경계를 지키는 샌드박스가 있을 때만이다. 지금은 macOS뿐이다.
    /// - 네이티브 Windows에는 Claude Code 샌드박스가 없다.
    /// - Linux(WSL2 포함)의 샌드박스(bubblewrap)는 glob 쓰기 차단(하위 `.git`, `.claude`, `CLAUDE.md`)을 버린다.
    ///   seccomp 도구가 없으면 유닉스 소켓도 막지 않는다. 경계를 glob 없이 다시 짜기 전에는 열지 않는다.
    pub const fn supports_sessions(self) -> bool {
        matches!(self, Self::MacOs)
    }

    /// 데이터 폴더가 이 사용자만 쓰는 자리인지 소유자와 권한 비트로 확인하지 못하는 OS에서 그 까닭.
    /// Windows는 권한 목록(ACL)을 읽지 않는다. 대신 기본 ACL이 본인 전용인 `%LOCALAPPDATA%` 아래만 받는다.
    pub fn private_home_problem(self, home: &Path, user_home: &Path) -> Option<String> {
        if self != Self::Windows {
            return None;
        }
        let local = user_home.join("AppData").join("Local");
        let (home, local) = (
            std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf()),
            std::fs::canonicalize(&local).unwrap_or(local),
        );
        (!home.starts_with(&local)).then(|| {
            format!(
                "데이터 폴더({})가 {} 밖에 있습니다. Windows에서는 다른 사용자가 쓸 수 없는 자리인지 확인하지 못해 멈춥니다. KAIROS_HOME을 그 아래로 옮기세요",
                home.display(),
                local.display()
            )
        })
    }
}

/// PATH 값을 항목으로 나눈다. 빈 항목은 뺀다.
pub fn split_path_list(os: Os, value: &str) -> impl Iterator<Item = &str> {
    value.split(os.path_list_separator()).filter(|entry| !entry.is_empty())
}

/// 절대 경로인가. 문자열로 판단한다. 호스트가 다른 운영체제의 경로를 시험할 수 있게 `Path::is_absolute`를 쓰지 않는다.
/// Windows는 드라이브 문자 뒤의 구분자(`C:\`, `C:/`)나 UNC(`\\server\share`)만 절대 경로다.
pub fn is_absolute(os: Os, path: &str) -> bool {
    match os {
        Os::Windows => {
            let bytes = path.as_bytes();
            let drive = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'\\' | b'/');
            drive || path.starts_with("\\\\")
        }
        _ => path.starts_with('/'),
    }
}

/// 파일의 unix 권한 비트(하위 12비트). unix가 아니면 None이다.
pub fn mode_of(metadata: &Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(metadata.permissions().mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// 소유자 uid. unix가 아니면 None이다.
pub fn owner_of(metadata: &Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(metadata.uid())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// 하드 링크 수. unix가 아니면 None이다(Windows의 값은 안정 API가 아니다).
pub fn links_of(metadata: &Metadata) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(metadata.nlink())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// 같은 파일인지 가리는 (장치, inode). unix가 아니면 None이다.
pub fn identity_of(metadata: &Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// 다른 사용자가 고칠 수 있거나 남의 것이거나 하드 링크가 있는가. 확인할 수 없는 항목은 문제로 보지 않는다.
/// Windows는 사용자 프로필 폴더의 ACL(본인과 SYSTEM, 관리자만)에 기댄다.
pub fn foreign_or_shared(metadata: &Metadata, owner: Option<u32>, forbidden_mode: u32) -> bool {
    mode_of(metadata).is_some_and(|mode| mode & forbidden_mode != 0)
        || owner_of(metadata).zip(owner).is_some_and(|(actual, expected)| actual != expected)
        || links_of(metadata).is_some_and(|links| links != 1)
}

/// 두 메타데이터가 같은 파일인가. 확인할 수 없으면 같다고 본다.
pub fn same_file(first: &Metadata, second: &Metadata) -> bool {
    match (identity_of(first), identity_of(second)) {
        (Some(first), Some(second)) => first == second,
        _ => true,
    }
}

/// 권한 비트를 바꾼다. unix가 아니면 하지 않는다.
pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

/// 연 파일의 권한 비트를 바꾼다. 경로를 다시 찾지 않는다. unix가 아니면 하지 않는다.
pub fn set_file_mode(file: &fs::File, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (file, mode);
        Ok(())
    }
}

/// 실행할 수 있는 일반 파일인가. Windows는 확장자가 `.exe`인 파일이다.
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    match mode_of(&metadata) {
        Some(mode) => mode & 0o111 != 0,
        None => path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("exe")),
    }
}

/// 새로 만들 파일과 폴더의 권한. unix가 아니면 무시한다.
pub trait CreateMode {
    #[must_use]
    fn create_mode(&mut self, mode: u32) -> &mut Self;
}

impl CreateMode for OpenOptions {
    fn create_mode(&mut self, mode: u32) -> &mut Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            self.mode(mode)
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
            self
        }
    }
}

impl CreateMode for DirBuilder {
    fn create_mode(&mut self, mode: u32) -> &mut Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            self.mode(mode)
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
            self
        }
    }
}

/// 열 때 마지막 경로 조각의 링크를 따라가지 않는다. unix는 `O_NOFOLLOW`(링크면 실패),
/// Windows는 `FILE_FLAG_OPEN_REPARSE_POINT`(링크 자체를 연다)다. 호출자는 연 뒤 일반 파일인지 다시 본다.
pub trait NoFollow {
    #[must_use]
    fn no_follow(&mut self) -> &mut Self;
    /// `no_follow`에 더해 FIFO에서 막히지 않는다. unix만 `O_NONBLOCK`을 붙인다.
    #[must_use]
    fn no_follow_nonblocking(&mut self) -> &mut Self;
}

impl NoFollow for OpenOptions {
    fn no_follow(&mut self) -> &mut Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            self.custom_flags(libc::O_NOFOLLOW)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            self.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        }
        #[cfg(not(any(unix, windows)))]
        {
            self
        }
    }

    fn no_follow_nonblocking(&mut self) -> &mut Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            self.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        }
        #[cfg(not(unix))]
        {
            self.no_follow()
        }
    }
}

/// Windows `FILE_FLAG_OPEN_REPARSE_POINT`.
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// 파일을 가리키는 심볼릭 링크를 만든다. Windows는 개발자 모드나 관리자 권한이 없으면 실패한다.
pub fn symlink_file(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

/// 폴더를 가리키는 심볼릭 링크를 만든다. `target`이 상대 경로면 `link`의 폴더 기준이다.
pub fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, link);
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 운영체제마다_데이터_폴더가_다르다() {
        let home = Path::new("/synthetic/user");
        assert_eq!(Os::MacOs.default_data_dir(home), home.join("Library/Application Support/kairos"));
        assert_eq!(Os::Linux.default_data_dir(home), home.join(".local/share/kairos"));
        assert_eq!(Os::Windows.default_data_dir(home), home.join("AppData").join("Local").join("kairos"));
    }

    #[test]
    fn 절대_경로를_문자열로_판단한다() {
        assert!(is_absolute(Os::Linux, "/usr/bin/gh"));
        assert!(!is_absolute(Os::Linux, "bin/gh"));
        assert!(is_absolute(Os::Windows, r"C:\Program Files\GitHub CLI\gh.exe"));
        assert!(is_absolute(Os::Windows, "d:/tools/gh.exe"));
        assert!(is_absolute(Os::Windows, r"\\server\share\gh.exe"));
        assert!(!is_absolute(Os::Windows, "/usr/bin/gh"));
        assert!(!is_absolute(Os::Windows, r"C:gh.exe"));
        assert!(!is_absolute(Os::Windows, r"tools\gh.exe"));
    }

    #[test]
    fn path_목록을_운영체제의_구분자로_나눈다() {
        assert_eq!(split_path_list(Os::Linux, "/a::/b").collect::<Vec<_>>(), ["/a", "/b"]);
        assert_eq!(split_path_list(Os::Windows, r"C:\a;;C:\b").collect::<Vec<_>>(), [r"C:\a", r"C:\b"]);
        assert_eq!(Os::Windows.exe("kairos"), "kairos.exe");
        assert_eq!(Os::Linux.exe("kairos"), "kairos");
    }

    #[test]
    fn 세션은_macos에서만_연다() {
        assert!(Os::MacOs.supports_sessions());
        assert!(!Os::Linux.supports_sessions() && !Os::Windows.supports_sessions());
    }

    #[test]
    fn windows는_localappdata_아래의_데이터_폴더만_받는다() {
        let user = Path::new("/synthetic/home");
        assert_eq!(Os::Windows.private_home_problem(&user.join("AppData/Local/kairos"), user), None);
        assert!(Os::Windows.private_home_problem(Path::new("/synthetic/shared/kairos"), user).is_some());
        assert_eq!(Os::Linux.private_home_problem(Path::new("/synthetic/shared/kairos"), user), None);
    }
}
