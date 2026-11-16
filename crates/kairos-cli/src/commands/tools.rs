//! 설치 뒤에 쓰는 도구 명령. `completions`는 셸 자동 완성 스크립트를 낸다. `update`는 `commands::update`에 있다.

use kairos_core::KairosResult;
use kairos_core::text::sanitize_text;

use crate::app::AppContext;
use crate::args::ArgParser;
use crate::command::Command;
use crate::help::{self, Group};

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
const FLAGS: [(&str, &[&str]); 2] =
    [("uninstall", &["--dry-run", "--runtime", "--purge", "--yes"]), ("update", &["--check", "--apply", "--json"])];

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
