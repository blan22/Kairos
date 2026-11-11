//! 본문을 저장하지 않고 멘션 여부와 사람 수만 뽑는다.
//!
//! 뒤돌아보기 때문에 fancy-regex를 쓴다. 이 엔진의 백트랙 예산은 탐색 위치마다 줄어서, 기본 한도(100만)로는
//! 평범한 긴 본문도 끝까지 못 보고 실패한다(측정: `@` 없는 1MiB ASCII 본문이 한도 초과).
//! 그렇다고 본문을 자르면 뒤쪽 멘션을 조용히 놓친다. 그래서 한도를 풀고 자르지 않는다.
//!
//! 두 패턴은 입력 길이에 선형이다. 첫 글자 `@`가 다른 시도의 몸통에 들어가지 않아 시도끼리 겹치지 않고,
//! 몸통 뒤에 실패할 조건이 없다. 측정(릴리스 빌드): 공격적 입력 64MiB에서 패턴당 3.4초 이하(약 20MB/s).
//!
//! 엔진이 그래도 오류를 내면 `ScanFailed`다. 호출자는 그 노드를 못 읽은 것으로 세어 partial을 낸다.
//! 판정 실패를 멘션 없음으로 내지 않는다.

use std::collections::HashSet;
use std::sync::LazyLock;

use fancy_regex::{Regex, RegexBuilder};
use serde_json::Value;

use crate::self_login;

const MENTION_COUNT_CAP: usize = 50;

static SHARED: LazyLock<MentionScanner> =
    LazyLock::new(|| MentionScanner::with_backtrack_limits(usize::MAX, usize::MAX));

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct MentionInfo {
    pub(crate) mentions_me: bool,
    pub(crate) mention_count: i64,
}

/// 정규식 엔진이 판정을 끝내지 못했다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ScanFailed;

/// JS의 `i`는 비ASCII 글자를 ASCII로 접지 않는다. fancy-regex는 `(?-u)`를 받지 않아 글자마다 클래스로 푼다.
fn ascii_any_case(login: &str) -> String {
    let letter = |c: char| format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase());
    login.chars().map(|c| if c.is_ascii_alphabetic() { letter(c) } else { c.to_string() }).collect()
}

#[derive(Debug)]
pub(crate) struct MentionScanner {
    self_mention: Regex,
    any_mention: Regex,
}

impl MentionScanner {
    pub(crate) fn shared() -> &'static Self {
        &SHARED
    }

    /// 패턴마다 한도를 받는다. 시험이 한쪽 패턴만 실패시킬 수 있어야 한다.
    pub(crate) fn with_backtrack_limits(self_limit: usize, any_limit: usize) -> Self {
        let build =
            |pattern: &str, limit| RegexBuilder::new(pattern).backtrack_limit(limit).build().expect("상수 정규식");
        let login = ascii_any_case(self_login());
        Self {
            // JS의 `\w`는 ASCII다.
            self_mention: build(&format!(r"(?<![A-Za-z0-9_/])@{login}(?![A-Za-z0-9_-])"), self_limit),
            any_mention: build(r"(?<![A-Za-z0-9_/@])@([A-Za-z0-9][A-Za-z0-9-]*(?:/[A-Za-z0-9_-]+)?)", any_limit),
        }
    }

    pub(crate) fn scan(&self, body: Option<&Value>) -> Result<MentionInfo, ScanFailed> {
        let text = body.and_then(Value::as_str).unwrap_or_default();
        // 두 패턴 모두 `@`가 있어야 맞는다. 대부분의 본문은 엔진을 돌리지 않고 끝난다.
        if !text.contains('@') {
            return Ok(MentionInfo::default());
        }
        let mut handles = HashSet::new();
        for captures in self.any_mention.captures_iter(text) {
            let captures = captures.map_err(|_| ScanFailed)?;
            handles.insert(captures.get(1).map_or_else(String::new, |handle| handle.as_str().to_ascii_lowercase()));
        }
        Ok(MentionInfo {
            mentions_me: self.self_mention.is_match(text).map_err(|_| ScanFailed)?,
            mention_count: handles.len().min(MENTION_COUNT_CAP) as i64,
        })
    }
}
