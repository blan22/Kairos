use std::collections::BTreeMap;

/// gh 실행 설정. 환경 변수는 `from_env` 한 곳에서만 읽는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GithubConfig {
    gh_path: String,
    path: String,
    home: String,
    lang: String,
}

impl GithubConfig {
    /// 조립 지점에서 한 번 부른다.
    pub fn from_env() -> Self {
        Self::from_vars(|key| std::env::var_os(key).map(|value| value.to_string_lossy().into_owned()))
    }

    /// 변수 읽기를 넘겨받아 만든다. 빈 `KAIROS_GH`는 없는 것으로 치고, 다른 값의 빈 문자열은 그대로 둔다.
    pub fn from_vars(read: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            gh_path: read("KAIROS_GH").filter(|path| !path.is_empty()).unwrap_or_else(|| "gh".to_owned()),
            path: read("PATH").unwrap_or_else(|| "/usr/bin:/bin".to_owned()),
            home: read("HOME").unwrap_or_default(),
            lang: read("LANG").unwrap_or_else(|| "en_US.UTF-8".to_owned()),
        }
    }

    pub(crate) fn gh_path(&self) -> &str {
        &self.gh_path
    }

    /// 자식이 받는 환경의 전부다. 부모 환경을 물려주지 않는다.
    pub(crate) fn base_env(&self) -> BTreeMap<String, String> {
        [
            ("PATH", self.path.as_str()),
            ("HOME", self.home.as_str()),
            ("LANG", self.lang.as_str()),
            ("GH_HOST", "github.com"),
            ("GH_PROMPT_DISABLED", "1"),
            ("GH_NO_UPDATE_NOTIFIER", "1"),
            ("NO_COLOR", "1"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
    }
}
