use std::fmt;

/// 실패의 코드. 화면과 로그에는 코드만 나간다. 메시지에 외부 문자열을 넣지 않는다.
#[derive(Debug)]
pub struct KairosError {
    code: String,
}

pub type KairosResult<T> = Result<T, KairosError>;

impl KairosError {
    pub fn new(code: impl Into<String>) -> Self {
        Self { code: code.into() }
    }

    /// `error.code`에 해당한다. CLI가 이 값을 그대로 내보낸다.
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for KairosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.code)
    }
}

impl std::error::Error for KairosError {}

impl From<std::io::Error> for KairosError {
    fn from(error: std::io::Error) -> Self {
        // Node의 errno 코드 이름과 맞춘다. 경로 같은 문자열을 끌고 오지 않는다.
        let code = match error.kind() {
            std::io::ErrorKind::NotFound => "ENOENT",
            std::io::ErrorKind::PermissionDenied => "EACCES",
            std::io::ErrorKind::AlreadyExists => "EEXIST",
            _ => "EIO",
        };
        Self::new(code)
    }
}
