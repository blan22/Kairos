use std::collections::BTreeMap;

use crate::GithubConfig;

fn config_of(vars: &[(&str, &str)]) -> GithubConfig {
    let vars: BTreeMap<String, String> = vars.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    GithubConfig::from_vars(|key| vars.get(key).cloned())
}

#[test]
fn gh_경로는_kairos_gh의_값이고_없거나_비면_gh다() {
    assert_eq!(config_of(&[("KAIROS_GH", "/opt/gh")]).gh_path(), "/opt/gh");
    assert_eq!(config_of(&[("KAIROS_GH", "")]).gh_path(), "gh");
    assert_eq!(config_of(&[]).gh_path(), "gh");
}

#[test]
fn 기본_환경은_원본의_키_목록과_기본값이다() {
    let env = config_of(&[]).base_env();
    let expected = [
        ("GH_HOST", "github.com"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
        ("GH_PROMPT_DISABLED", "1"),
        ("HOME", ""),
        ("LANG", "en_US.UTF-8"),
        ("NO_COLOR", "1"),
        ("PATH", "/usr/bin:/bin"),
    ];
    assert_eq!(env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect::<Vec<_>>(), expected);
}

#[test]
fn 값이_있으면_빈_문자열이라도_그대로_넘긴다() {
    let env = config_of(&[("PATH", ""), ("HOME", "/h"), ("LANG", "")]).base_env();
    assert_eq!(env["PATH"], "");
    assert_eq!(env["HOME"], "/h");
    assert_eq!(env["LANG"], "");
}
