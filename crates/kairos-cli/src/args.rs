//! `node:util.parseArgs`의 strict 동작. 원본 옵션에는 `short`가 없다.
//!
//! 오류 코드는 Node의 값을 그대로 쓴다. `ERR_PARSE_ARGS_`로 시작하는 코드가 JSON 오류의 usage 경로로 간다.

use std::collections::{BTreeMap, BTreeSet};

use kairos_core::{KairosError, KairosResult, js};

const UNKNOWN_OPTION: &str = "ERR_PARSE_ARGS_UNKNOWN_OPTION";
const INVALID_OPTION_VALUE: &str = "ERR_PARSE_ARGS_INVALID_OPTION_VALUE";
const UNEXPECTED_POSITIONAL: &str = "ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OptionKind {
    Flag,
    Text,
}

#[derive(Clone, Copy, Debug)]
pub struct OptionSpec {
    name: &'static str,
    kind: OptionKind,
}

/// 값을 받지 않는 옵션.
pub const fn flag(name: &'static str) -> OptionSpec {
    OptionSpec { name, kind: OptionKind::Flag }
}

/// 값이 필요한 옵션.
pub const fn text(name: &'static str) -> OptionSpec {
    OptionSpec { name, kind: OptionKind::Text }
}

/// 토큰 하나. 값이 다음 인자에서 왔으면 `inline`이 거짓이다.
struct OptionToken<'a> {
    name: &'a str,
    value: Option<&'a str>,
    inline: bool,
}

enum Token<'a> {
    Option(OptionToken<'a>),
    Positional(&'a str),
}

#[derive(Clone, Copy, Debug)]
pub struct ArgParser<'a> {
    options: &'a [OptionSpec],
    allow_positionals: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParsedArgs {
    flags: BTreeSet<String>,
    texts: BTreeMap<String, String>,
    positionals: Vec<String>,
}

impl ParsedArgs {
    pub fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.texts.get(name).map(String::as_str)
    }

    pub fn positionals(&self) -> &[String] {
        &self.positionals
    }

    pub fn first(&self) -> Option<&str> {
        self.positionals.first().map(String::as_str)
    }
}

impl<'a> ArgParser<'a> {
    /// 위치 인자를 받지 않는 파서. `parseArgs`의 기본값이다.
    pub const fn new(options: &'a [OptionSpec]) -> Self {
        Self { options, allow_positionals: false }
    }

    /// `allowPositionals: true`.
    pub const fn with_positionals(options: &'a [OptionSpec]) -> Self {
        Self { options, allow_positionals: true }
    }

    fn find(&self, name: &str) -> Option<&OptionSpec> {
        self.options.iter().find(|option| option.name == name)
    }

    fn is_text(&self, name: &str) -> bool {
        self.find(name).is_some_and(|option| option.kind == OptionKind::Text)
    }

    /// 인자를 토큰으로 나눈다. 여기서는 던지지 않는다. Node도 판정을 앞 토큰부터 뒤에서 한다.
    fn tokens<'s>(&self, args: &'s [String]) -> Vec<Token<'s>> {
        let mut tokens = Vec::with_capacity(args.len());
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            let arg = arg.as_str();
            if arg == "--" {
                tokens.extend(rest.by_ref().map(|value| Token::Positional(value)));
                break;
            }
            let Some(body) = arg.strip_prefix('-').filter(|body| !body.is_empty()) else {
                tokens.push(Token::Positional(arg));
                continue;
            };
            let Some(long) = body.strip_prefix('-') else {
                // `short`를 쓴 옵션이 없으므로 -x, -xy, -x=y는 모두 모르는 옵션 토큰이다. 값을 먹지 않는다.
                tokens.push(Token::Option(OptionToken { name: body, value: None, inline: false }));
                continue;
            };
            // Node는 3번째 글자부터 등호를 찾는다. `--=x`는 이름이 `=x`인 옵션이다.
            match long.char_indices().skip(1).find(|(_, letter)| *letter == '=') {
                Some((at, _)) => {
                    let (name, value) = (&long[..at], &long[at + 1..]);
                    tokens.push(Token::Option(OptionToken { name, value: Some(value), inline: true }));
                }
                None if self.is_text(long) => {
                    let value = rest.next().map(String::as_str);
                    tokens.push(Token::Option(OptionToken { name: long, value, inline: false }));
                }
                None => tokens.push(Token::Option(OptionToken { name: long, value: None, inline: false })),
            }
        }
        tokens
    }

    fn check(&self, token: &OptionToken<'_>) -> KairosResult<OptionKind> {
        let Some(option) = self.find(token.name) else {
            return Err(KairosError::new(UNKNOWN_OPTION));
        };
        let invalid = match option.kind {
            OptionKind::Text => token.value.is_none(),
            OptionKind::Flag => token.value.is_some(),
        };
        // 다음 인자를 값으로 먹었는데 옵션처럼 생겼으면 값을 빠뜨린 것으로 본다.
        let ambiguous =
            !token.inline && token.value.is_some_and(|value| js::utf16_len(value) > 1 && value.starts_with('-'));
        if invalid || ambiguous {
            return Err(KairosError::new(INVALID_OPTION_VALUE));
        }
        Ok(option.kind)
    }

    pub fn parse(&self, args: &[String]) -> KairosResult<ParsedArgs> {
        let mut parsed = ParsedArgs::default();
        for token in self.tokens(args) {
            match token {
                Token::Positional(_) if !self.allow_positionals => {
                    return Err(KairosError::new(UNEXPECTED_POSITIONAL));
                }
                Token::Positional(value) => parsed.positionals.push(value.to_owned()),
                Token::Option(option) => match (self.check(&option)?, option.value) {
                    (OptionKind::Text, Some(value)) => {
                        parsed.texts.insert(option.name.to_owned(), value.to_owned());
                    }
                    _ => {
                        parsed.flags.insert(option.name.to_owned());
                    }
                },
            }
        }
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPTIONS: [OptionSpec; 3] = [flag("json"), flag("seen"), text("surface")];

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn code(parser: ArgParser<'_>, values: &[&str]) -> String {
        parser.parse(&args(values)).unwrap_err().code().to_owned()
    }

    #[test]
    fn 값이_필요한_옵션은_다음_인자나_등호_뒤를_받는다() {
        let parser = ArgParser::new(&OPTIONS);
        assert_eq!(parser.parse(&args(&["--surface", "panel"])).unwrap().text("surface"), Some("panel"));
        assert_eq!(parser.parse(&args(&["--surface=panel"])).unwrap().text("surface"), Some("panel"));
        assert_eq!(parser.parse(&args(&["--surface="])).unwrap().text("surface"), Some(""));
        assert_eq!(parser.parse(&args(&["--surface=a=b"])).unwrap().text("surface"), Some("a=b"));
        assert_eq!(parser.parse(&args(&["--surface", "a", "--surface=b"])).unwrap().text("surface"), Some("b"));
    }

    #[test]
    fn 값이_빠지거나_옵션처럼_보이는_값은_오류다() {
        let parser = ArgParser::new(&OPTIONS);
        assert_eq!(code(parser, &["--surface"]), INVALID_OPTION_VALUE);
        assert_eq!(code(parser, &["--surface", "--json"]), INVALID_OPTION_VALUE);
        assert_eq!(code(parser, &["--surface", "-x"]), INVALID_OPTION_VALUE);
        assert_eq!(parser.parse(&args(&["--surface", "-"])).unwrap().text("surface"), Some("-"));
        assert_eq!(parser.parse(&args(&["--surface=--json"])).unwrap().text("surface"), Some("--json"));
    }

    #[test]
    fn 불리언에는_값을_줄_수_없다() {
        let parser = ArgParser::new(&OPTIONS);
        assert_eq!(code(parser, &["--json=1"]), INVALID_OPTION_VALUE);
        assert_eq!(code(parser, &["--json="]), INVALID_OPTION_VALUE);
        assert!(parser.parse(&args(&["--json", "--seen"])).unwrap().flag("seen"));
    }

    #[test]
    fn 모르는_옵션은_짧은_형식까지_모두_오류다() {
        let parser = ArgParser::with_positionals(&OPTIONS);
        for value in ["--bogus", "--bogus=1", "-x", "-xy", "-1", "--=x", "--=a=b", "-한", "--__proto__"] {
            assert_eq!(code(parser, &[value]), UNKNOWN_OPTION, "{value}");
        }
    }

    #[test]
    fn 위치_인자를_막으면_오류고_허용하면_받는다() {
        assert_eq!(code(ArgParser::new(&OPTIONS), &["a"]), UNEXPECTED_POSITIONAL);
        assert_eq!(code(ArgParser::new(&OPTIONS), &["--", "a"]), UNEXPECTED_POSITIONAL);
        assert_eq!(code(ArgParser::new(&OPTIONS), &["-"]), UNEXPECTED_POSITIONAL);
        let parsed = ArgParser::with_positionals(&OPTIONS).parse(&args(&["a", "-", "--json", "b"])).unwrap();
        assert_eq!(parsed.positionals(), args(&["a", "-", "b"]));
        assert!(parsed.flag("json"));
    }

    #[test]
    fn 이중_대시_뒤는_모두_위치_인자다() {
        let parsed =
            ArgParser::with_positionals(&OPTIONS).parse(&args(&["--json", "--", "--seen", "-1", "--"])).unwrap();
        assert_eq!(parsed.positionals(), args(&["--seen", "-1", "--"]));
        assert!(!parsed.flag("seen"));
    }

    #[test]
    fn 앞_토큰의_오류가_먼저다() {
        assert_eq!(code(ArgParser::new(&OPTIONS), &["a", "--bogus"]), UNEXPECTED_POSITIONAL);
        assert_eq!(code(ArgParser::new(&OPTIONS), &["--bogus", "a"]), UNKNOWN_OPTION);
        assert_eq!(code(ArgParser::new(&OPTIONS), &["--json=1", "--bogus"]), INVALID_OPTION_VALUE);
    }
}
