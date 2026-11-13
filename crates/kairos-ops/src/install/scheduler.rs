//! 운영체제의 스케줄러. 수집 틱(3분)과 감시자(5분)를 띄우는 파일을 만들고, 설치된 파일에서 값을 읽는다.
//!
//! | | 파일 | 등록 |
//! |---|---|---|
//! | macOS | `~/Library/LaunchAgents/<라벨>.plist` | `launchctl bootstrap` |
//! | Linux | `~/.config/systemd/user/<라벨>.service`, `.timer` | `systemctl --user enable` |
//! | Windows | `<데이터 폴더>/tasks/<라벨>.xml`, `.json` | `schtasks /Create /XML` |
//!
//! Windows의 작업은 창 없는 런처(`kairos-launch.exe`)가 `.json`의 환경과 로그 파일로 kairos를 띄운다.
//! 작업 스케줄러에는 작업별 환경 변수가 없고, 콘솔 프로그램을 바로 띄우면 3분마다 창이 깜박인다.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use kairos_core::platform::Os;
use kairos_core::{KairosError, KairosResult};
use regex::Regex;
use serde_json::{Value, json};

use super::COLLECT_LABEL;
use super::plan::{ServiceFile, read_template, render_template, render_with, xml_escape};

static PLIST_VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<key>([A-Z_]+)</key>[ \t\r\n]*<string>([^<]*)</string>").expect("상수 정규식"));
static UNIT_ENVIRONMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^Environment="([A-Z_]+)=([^"\n]*)"$"#).expect("상수 정규식"));
/// 설치된 파일 하나의 상한. 손으로 늘리지 않는 작은 파일이다.
const MAX_INSTALLED_BYTES: u64 = 64 * 1024;

/// 수집 잡에 넘기는 환경. 감시자는 `KAIROS_HOME`만 받는다.
const COLLECT_ENV: [(&str, &str); 6] = [
    ("PATH", "PATH"),
    ("LANG", "LANG"),
    ("KAIROS_HOME", "KAIROS_HOME"),
    ("KAIROS_CLAUDE", "CLAUDE"),
    ("KAIROS_GH", "GH"),
    ("KAIROS_PLUGIN_DIR", "PLUGIN_DIR"),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scheduler {
    Launchd,
    Systemd,
    TaskScheduler,
}

impl Scheduler {
    pub const fn for_os(os: Os) -> Self {
        match os {
            Os::MacOs => Self::Launchd,
            Os::Linux => Self::Systemd,
            Os::Windows => Self::TaskScheduler,
        }
    }

    /// 안내 문구의 이름.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Launchd => "launchd",
            Self::Systemd => "systemd",
            Self::TaskScheduler => "작업 스케줄러",
        }
    }

    /// 안내 문구에 쓰는 파일 이름.
    pub const fn file_noun(self) -> &'static str {
        match self {
            Self::Launchd => "plist",
            Self::Systemd => "유닛",
            Self::TaskScheduler => "작업 파일",
        }
    }

    /// 라벨 하나의 파일들. 등록에 쓰는 파일이 먼저 온다(launchd plist, systemd timer, 작업 XML).
    pub fn file_names(self, label: &str) -> Vec<String> {
        match self {
            Self::Launchd => vec![format!("{label}.plist")],
            Self::Systemd => vec![format!("{label}.timer"), format!("{label}.service")],
            Self::TaskScheduler => vec![format!("{label}.xml"), format!("{label}.json")],
        }
    }

    pub(super) fn render(
        self,
        label: &str,
        ops: &Path,
        service_dir: &Path,
        values: &BTreeMap<&str, String>,
    ) -> KairosResult<Vec<ServiceFile>> {
        let file = |name: String, content: String| ServiceFile {
            label: label.to_owned(),
            path: service_dir.join(name),
            content,
        };
        match self {
            Self::Launchd => {
                let template = read_template(&ops.join(format!("{label}.plist.template")))?;
                Ok(vec![file(format!("{label}.plist"), render_template(&template, values)?)])
            }
            Self::Systemd => {
                let mut files = Vec::with_capacity(2);
                for kind in ["timer", "service"] {
                    let template = read_template(&ops.join("systemd").join(format!("{label}.{kind}.template")))?;
                    files.push(file(format!("{label}.{kind}"), render_with(&template, values, unit_value)?));
                }
                Ok(files)
            }
            Self::TaskScheduler => {
                let launch_file = service_dir.join(format!("{label}.json"));
                let mut values = values.clone();
                values.insert("LAUNCH_FILE", launch_file.to_string_lossy().into_owned());
                let template = read_template(&ops.join("windows").join(format!("{label}.xml.template")))?;
                let task = render_with(&template, &values, task_value)?;
                Ok(vec![
                    file(format!("{label}.xml"), task),
                    file(format!("{label}.json"), launch_spec(label, &values)?),
                ])
            }
        }
    }

    /// 설치된 수집 잡 파일에 적힌 환경 값. 없거나 읽을 수 없으면 None이다.
    pub fn installed_value(self, service_dir: &Path, label: &str, key: &str) -> Option<String> {
        let name = match self {
            Self::Launchd => format!("{label}.plist"),
            Self::Systemd => format!("{label}.service"),
            Self::TaskScheduler => format!("{label}.json"),
        };
        let text = read_small(&service_dir.join(name))?;
        match self {
            Self::Launchd => PLIST_VALUE
                .captures_iter(&text)
                .find(|captures| &captures[1] == key)
                .map(|captures| xml_unescape(&captures[2])),
            Self::Systemd => UNIT_ENVIRONMENT
                .captures_iter(&text)
                .find(|captures| &captures[1] == key)
                .map(|captures| unit_unescape(&captures[2])),
            Self::TaskScheduler => {
                let spec: Value = serde_json::from_str(&text).ok()?;
                spec.get("env")?.get(key)?.as_str().map(str::to_owned)
            }
        }
    }
}

/// 링크가 아닌 작은 파일만 읽는다.
fn read_small(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_INSTALLED_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn xml_unescape(value: &str) -> String {
    value.replace("&quot;", "\"").replace("&gt;", ">").replace("&lt;", "<").replace("&amp;", "&")
}

fn unit_unescape(value: &str) -> String {
    value.replace("%%", "%")
}

/// systemd 유닛의 큰따옴표 안에 넣는 값. 지정자 `%`는 `%%`로 바꾸고, 따옴표, 역슬래시, `$`, 제어 문자가 든 값은 받지 않는다.
/// 환경 줄과 ExecStart의 해석이 달라 하나의 이스케이프로 두 자리를 모두 맞출 수 없다. 이런 경로는 설치에 쓰지 않는다.
pub(super) fn unit_value(value: &str) -> KairosResult<String> {
    if value.chars().any(|c| c.is_control() || matches!(c, '"' | '\\' | '$' | '`')) {
        return Err(KairosError::new("install_value_unsupported"));
    }
    Ok(value.replace('%', "%%"))
}

/// 작업 XML에 넣는 값. 큰따옴표로 감싼 인자에도 들어가므로 따옴표와 제어 문자가 든 값은 받지 않는다.
pub(super) fn task_value(value: &str) -> KairosResult<String> {
    if value.chars().any(|c| c.is_control() || c == '"') {
        return Err(KairosError::new("install_value_unsupported"));
    }
    Ok(xml_escape(value))
}

/// 런처가 읽는 실행 설명. 런처는 자기 옆의 kairos만 띄우고 여기서는 인자, 환경, 로그 파일만 받는다.
fn launch_spec(label: &str, values: &BTreeMap<&str, String>) -> KairosResult<String> {
    let value =
        |key: &str| values.get(key).cloned().ok_or_else(|| KairosError::new(format!("unfilled placeholder: {key}")));
    let collect = label == COLLECT_LABEL;
    let mut env = serde_json::Map::new();
    if collect {
        for (name, key) in COLLECT_ENV {
            let text = if key == "LANG" { "en_US.UTF-8".to_owned() } else { value(key)? };
            env.insert(name.to_owned(), Value::String(task_value(&text).map(|_| text)?));
        }
    } else {
        let home = value("KAIROS_HOME")?;
        env.insert("KAIROS_HOME".to_owned(), Value::String(task_value(&home).map(|_| home)?));
    }
    let (command, log) = if collect { ("collect", "collect.log") } else { ("watchdog", "watchdog.log") };
    let log_dir = value("LOG_DIR")?;
    let spec = json!({
        "schema": 1,
        "args": [command],
        "env": env,
        "log": format!("{log_dir}\\{log}"),
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&spec).map_err(|_| KairosError::new("install_launch_spec"))?))
}

/// 파일에 쓸 바이트. 작업 XML은 `schtasks /XML`이 읽는 UTF-16LE(BOM 포함)이다.
pub(super) fn encoded(file: &ServiceFile) -> Vec<u8> {
    if file.path.extension().is_some_and(|extension| extension == "xml") {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in file.content.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    } else {
        file.content.clone().into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 유닛_값은_지정자를_이스케이프하고_해석이_갈리는_문자는_거부한다() {
        assert_eq!(unit_value("/home/u/50%/kairos").unwrap(), "/home/u/50%%/kairos");
        assert_eq!(unit_value("/home/u/my data/kairos").unwrap(), "/home/u/my data/kairos");
        for bad in ["/a\"b", "/a\\b", "/a$b", "/a`b", "/a\nb"] {
            assert_eq!(unit_value(bad).unwrap_err().code(), "install_value_unsupported", "{bad:?}");
        }
        assert_eq!(unit_unescape("/home/u/50%%/kairos"), "/home/u/50%/kairos");
    }

    #[test]
    fn 작업_값은_xml로_이스케이프하고_따옴표는_거부한다() {
        assert_eq!(task_value(r"C:\Users\A&B\kairos").unwrap(), r"C:\Users\A&amp;B\kairos");
        assert!(task_value("C:\\a\"b").is_err());
    }

    #[test]
    fn 작업_xml은_bom이_있는_utf16le로_쓴다() {
        let file = ServiceFile { label: "x".into(), path: "a/x.xml".into(), content: "<a>한</a>".into() };
        let bytes = encoded(&file);
        assert_eq!(&bytes[..4], &[0xFF, 0xFE, b'<', 0]);
        assert_eq!(bytes.len(), 2 + "<a>한</a>".encode_utf16().count() * 2);
        let json = ServiceFile { label: "x".into(), path: "a/x.json".into(), content: "{}".into() };
        assert_eq!(encoded(&json), b"{}");
    }
}
