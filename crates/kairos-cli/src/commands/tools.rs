//! 설치 뒤에 쓰는 도구 명령. `completions`는 셸 자동 완성 스크립트를 내고 `update`는 올리는 명령을 알려 준다.
//!
//! `update`는 스스로 받거나 빌드하지 않는다. kairos는 코드를 내려받아 실행하지 않고, 설치 스크립트는 사용자가
//! 터미널에서 직접 돌린다. 설치 스크립트가 데이터 폴더에 남긴 설치 방식(`install-source`)을 읽어 맞는 명령만 찍는다.

use std::fs;
use std::path::Path;

use kairos_core::KairosResult;
use kairos_core::text::{sanitize, sanitize_text};
use serde_json::json;

use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag};
use crate::command::Command;
use crate::help::{self, Group};
use crate::reply::Reply;

const JSON_ONLY: [OptionSpec; 1] = [flag("json")];
/// 설치 스크립트가 남기는 파일. 한 줄이다. `source:<저장소 절대 경로>` 또는 `release`.
const INSTALL_SOURCE_FILE: &str = "install-source";
const SOURCE_PREFIX: &str = "source:";
const RELEASE_MARK: &str = "release";
/// 저장소 경로 길이 상한. 넘으면 모르는 값으로 본다.
const MAX_SOURCE_BYTES: u64 = 4096;
/// 릴리스 설치 명령. 다시 돌리면 최신 판으로 올린다.
const RELEASE_COMMAND: &str = "curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.sh | zsh";

/// 설치 방식.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Origin {
    /// 저장소에서 빌드했다. 값은 저장소 경로다.
    Source(String),
    Release,
    /// 기록이 없거나 읽을 수 없다.
    Unknown,
}

impl Origin {
    fn read(home: &Path) -> Self {
        let path = home.join(INSTALL_SOURCE_FILE);
        let small = fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_SOURCE_BYTES);
        let Some(text) = small.then(|| fs::read_to_string(&path).ok()).flatten() else {
            return Self::Unknown;
        };
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let line = text.lines().next().unwrap_or("").trim();
        if line == RELEASE_MARK {
            return Self::Release;
        }
        match line.strip_prefix(SOURCE_PREFIX) {
            // 절대 경로만 받는다. 그대로 찍을 수 없는 값(제어 문자, bidi 문자)은 모르는 값으로 본다. 붙여 넣을 명령이 된다.
            Some(repo) if repo.starts_with('/') && is_plain(repo) => Self::Source(repo.to_owned()),
            _ => Self::Unknown,
        }
    }

    const fn method(&self) -> &'static str {
        match self {
            Self::Source(_) => "source",
            Self::Release => "release",
            Self::Unknown => "unknown",
        }
    }
}

/// 화면에 찍는 경로의 길이 상한.
pub(super) const PLAIN_LIMIT: usize = 4096;

/// 터미널에 그대로 찍어도 되는 글인가. 제어 문자, ESC 시퀀스, bidi 문자, 줄바꿈이 없어야 한다.
pub(super) fn is_plain(text: &str) -> bool {
    sanitize_text(text, PLAIN_LIMIT) == text
}

/// 셸에 그대로 붙여 넣을 수 있게 작은따옴표로 감싼다.
pub(super) fn shell_quote(text: &str) -> String {
    let plain = text.chars().all(|letter| letter.is_ascii_alphanumeric() || "/._-+".contains(letter));
    if plain && !text.is_empty() { text.to_owned() } else { format!("'{}'", text.replace('\'', "'\\''")) }
}

pub struct Update;

impl Command for Update {
    fn name(&self) -> &'static str {
        "update"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos update [--json]")
    }

    /// 지금 판과 설치 방식에 맞는 업데이트 명령을 알려 준다. 아무것도 바꾸지 않는다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&JSON_ONLY).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let origin = Origin::read(context.home().root());
        let (label, command) = match &origin {
            Origin::Source(repo) => {
                let script = Path::new(repo).join("scripts").join("install.sh");
                let command = script.is_file().then(|| format!("{} --update", shell_quote(&script.to_string_lossy())));
                (format!("소스 빌드({})", sanitize(repo)), command)
            }
            Origin::Release => ("릴리스".to_owned(), Some(RELEASE_COMMAND.to_owned())),
            Origin::Unknown => ("기록 없음".to_owned(), None),
        };
        if reply.json() {
            let mut update = json!({ "version": env!("CARGO_PKG_VERSION"), "method": origin.method() });
            // 없는 값은 키째 뺀다. 사람이 터미널에 붙여 넣을 셸 문자열이다. 호스트가 실행하지 않는다.
            if let Some(command) = &command {
                update["command"] = json!(command);
            }
            return reply.success(json!({ "update": update }));
        }
        let console = context.console();
        console.print(&format!("지금 판: {}", help::version_line()));
        console.print(&format!("설치 방식: {label}"));
        match (&origin, command) {
            (_, Some(command)) => {
                console.print("업데이트하려면 터미널에서 다음을 실행하세요. 설정과 데이터는 그대로 둡니다");
                console.print(&format!("  {command}"));
            }
            (Origin::Source(_), None) => {
                console.print("저장소가 그 자리에 없습니다. 저장소를 다시 받아 scripts/install.sh를 실행하세요");
            }
            _ => {
                console.print("설치 방식을 적어 두기 전의 판으로 설치했습니다. 받은 저장소에서 다음을 실행하세요");
                console.print("  scripts/install.sh --update");
            }
        }
        Ok(0)
    }
}

/// 명령 바로 뒤(셋째 낱말)에 오는 정해진 낱말. 그 뒤 자리는 소스 이름, ID, 파일이라 채우지 않는다.
const WORDS: [(&str, &[&str]); 8] = [
    ("notify", &["on", "off", "setup", "test"]),
    ("display", &["on", "off", "auto"]),
    ("sessions", &["on", "off"]),
    ("sources", &["on", "off"]),
    ("config", &["validate", "show", "path", "init"]),
    ("connect", &["github", "google", "slack", "name", "mcp", "session-token"]),
    ("triage", &["keep", "drop"]),
    ("completions", &["zsh", "bash"]),
];

/// 어느 자리에나 오는 옵션.
const FLAGS: [(&str, &[&str]); 1] = [("uninstall", &["--dry-run", "--runtime", "--purge", "--yes"])];

const HELP_SUMMARY: &str = "명령 목록과 한 명령의 사용법";

/// 자동 완성에 나오는 명령. 내부 명령은 뺀다.
fn public_entries() -> impl Iterator<Item = &'static help::Entry> {
    help::ENTRIES.iter().filter(|entry| entry.group != Group::Internal)
}

/// zsh 자동 완성. `source <(kairos completions zsh)`로도, fpath의 `_kairos` 파일로도 쓴다.
/// 정해진 낱말이 없는 자리는 파일 이름으로 채운다(`--org <파일>`).
fn zsh_script() -> String {
    let mut script = String::from(
        "#compdef kairos\n# kairos completions zsh가 만든 파일이다. 손으로 고치지 않는다.\n_kairos() {\n  local -a commands\n  commands=(\n",
    );
    let described = public_entries().map(|entry| (entry.name, entry.summary)).chain([("help", HELP_SUMMARY)]);
    for (name, summary) in described {
        // `_describe`는 `이름:설명`을 받는다. 설명의 콜론과 작은따옴표를 피한다.
        let summary = summary.replace('\\', "\\\\").replace(':', "\\:").replace('\'', "'\\''");
        script.push_str(&format!("    '{name}:{summary}'\n"));
    }
    script.push_str(
        "  )\n  if (( CURRENT == 2 )); then\n    _describe -t commands 'kairos 명령' commands\n    return\n  fi\n  case ${words[2]} in\n",
    );
    for (name, flags) in FLAGS {
        script.push_str(&format!("    {name}) compadd -- {}; return ;;\n", flags.join(" ")));
    }
    script.push_str("  esac\n  if (( CURRENT == 3 )); then\n    case ${words[2]} in\n      help) _describe -t commands 'kairos 명령' commands; return ;;\n");
    for (name, words) in WORDS {
        script.push_str(&format!("      {name}) compadd -- {}; return ;;\n", words.join(" ")));
    }
    script.push_str(
        "    esac\n  fi\n  _files\n}\nif [[ ${funcstack[1]} == _kairos ]]; then\n  _kairos \"$@\"\nelse\n  compdef _kairos kairos\nfi\n",
    );
    script
}

/// bash 자동 완성. `eval \"$(kairos completions bash)\"`로 쓴다. macOS의 bash 3.2는 `source <(...)`를 읽지 못한다.
/// 정해진 낱말이 없는 자리는 bash의 기본 완성(파일 이름)에 맡긴다.
fn bash_script() -> String {
    let names: Vec<&str> = public_entries().map(|entry| entry.name).chain(["help"]).collect();
    let mut script = String::from(
        "# kairos completions bash가 만든 파일이다. 손으로 고치지 않는다.\n_kairos() {\n  local cur=${COMP_WORDS[COMP_CWORD]}\n  local words=\"\"\n  if [ \"$COMP_CWORD\" -eq 1 ]; then\n",
    );
    script.push_str(&format!("    words=\"{}\"\n", names.join(" ")));
    script.push_str("  else\n    case ${COMP_WORDS[1]} in\n");
    for (name, flags) in FLAGS {
        script.push_str(&format!("      {name}) words=\"{}\" ;;\n", flags.join(" ")));
    }
    script.push_str("    esac\n    if [ \"$COMP_CWORD\" -eq 2 ]; then\n      case ${COMP_WORDS[1]} in\n");
    script.push_str(&format!("        help) words=\"{}\" ;;\n", names.join(" ")));
    for (name, words) in WORDS {
        script.push_str(&format!("        {name}) words=\"{}\" ;;\n", words.join(" ")));
    }
    script.push_str(
        "      esac\n    fi\n  fi\n  COMPREPLY=()\n  if [ -n \"$words\" ]; then\n    COMPREPLY=($(compgen -W \"$words\" -- \"$cur\"))\n  fi\n}\ncomplete -o default -F _kairos kairos\n",
    );
    script
}

pub struct Completions;

impl Command for Completions {
    fn name(&self) -> &'static str {
        "completions"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos completions zsh|bash")
    }

    /// 셸 자동 완성 스크립트를 표준 출력에 낸다. 명령 이름과 설명은 도움말 표에서 온다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&[]).parse(args)?;
        let script = match parsed.positionals() {
            [shell] if shell == "zsh" => zsh_script(),
            [shell] if shell == "bash" => bash_script(),
            _ => {
                context.console().print("사용법: kairos completions zsh|bash");
                let console = context.console();
                console.print("zsh: ~/.zshrc에 source <(kairos completions zsh) 한 줄을 compinit 뒤에 더합니다");
                console.print("bash: ~/.bashrc에 eval \"$(kairos completions bash)\" 한 줄을 더합니다");
                return Ok(2);
            }
        };
        // 스크립트 끝에 개행이 있다. print가 하나 더 붙이지 않게 뗀다.
        context.console().print(script.trim_end_matches('\n'));
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 설치_방식은_한_줄에서_읽고_모양이_틀리면_모른다고_본다() {
        assert_eq!(Origin::parse("release\n"), Origin::Release);
        assert_eq!(Origin::parse("source:/synthetic/repo\n"), Origin::Source("/synthetic/repo".to_owned()));
        assert_eq!(Origin::parse("source:/with space/repo"), Origin::Source("/with space/repo".to_owned()));
        for bad in ["", "source:relative/repo", "source:/a\u{1b}[31m", "git:/x", "release extra"] {
            assert_eq!(Origin::parse(bad), Origin::Unknown, "{bad:?}");
        }
    }

    #[test]
    fn 붙여_넣을_명령은_필요할_때만_따옴표로_감싼다() {
        assert_eq!(shell_quote("/synthetic/repo/scripts/install.sh"), "/synthetic/repo/scripts/install.sh");
        assert_eq!(shell_quote("/with space/x"), "'/with space/x'");
        assert_eq!(shell_quote("/it's/x"), "'/it'\\''s/x'");
        assert_eq!(shell_quote("/a;rm -rf b"), "'/a;rm -rf b'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn 자동_완성은_공개_명령만_담고_정해진_낱말을_안다() {
        let registry = crate::registry();
        let public: Vec<&str> = public_entries().map(|entry| entry.name).collect();
        let (zsh, bash) = (zsh_script(), bash_script());
        let bash_commands = bash.lines().find(|line| line.trim_start().starts_with("words=\"")).unwrap_or("");
        for name in registry.names() {
            let internal = help::find(name).is_some_and(|entry| entry.group == Group::Internal);
            assert_eq!(public.contains(&name), !internal, "{name}");
            assert_eq!(zsh.contains(&format!("    '{name}:")), !internal, "{name}");
            let in_bash = bash_commands.split([' ', '"']).any(|word| word == name);
            assert_eq!(in_bash, !internal, "{name}");
        }
        for script in [&zsh, &bash] {
            assert!(script.contains("on off setup test"), "{script}");
        }
        // 낱말 표의 명령은 모두 등록돼 있다.
        for (name, _) in WORDS.iter().chain(FLAGS.iter()) {
            assert!(registry.find(name).is_some(), "{name}");
        }
        // help는 등록된 명령이 아니지만 두 셸 모두 채운다. 빈 자리는 파일 이름으로 넘긴다.
        assert!(zsh.contains("    'help:") && bash_commands.contains(" help\""), "{bash_commands}");
        assert!(zsh.contains("\n  _files\n") && bash.contains("complete -o default -F _kairos kairos"));
    }
}
