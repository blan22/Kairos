use serde_json::{Value, json};

use super::fake::{FakeGh, FakeOptions, context, ids, nodes, query, source, spread};
use crate::GithubSource;
use crate::mention::{MentionInfo, MentionScanner, ScanFailed};
use crate::node_parser::NodeParser;
use kairos_core::model::{DirectSource, Outcome};

fn scan(text: &str) -> MentionInfo {
    MentionScanner::shared().scan(Some(&json!(text))).unwrap()
}

/// 기대값은 Node의 `mentionInfo` 실측이다.
#[test]
fn 멘션_판정은_원본과_같다() {
    let cases = [
        ("@kairos-dev", true, 1),
        ("@KAIROS-DEV", true, 1),
        ("가@kairos-dev", true, 1),
        ("😀@kairos-dev", true, 1),
        ("_@kairos-dev", false, 0),
        ("/@kairos-dev", false, 0),
        ("-@kairos-dev", true, 1),
        ("@@kairos-dev", true, 0),
        ("@kairos-dev_", false, 1),
        ("@kairos-dev/team", true, 1),
        ("@kairos-dev.", true, 1),
        ("@kairos-dev가", true, 1),
        ("@a@b", false, 1),
        ("@a/b @A/B", false, 1),
        ("@a/ @a", false, 1),
        ("@-a", false, 0),
        ("@a_b", false, 1),
        ("@a/b_c-d", false, 1),
        ("@bl\u{212A}an22", false, 1),
        ("@kairos-dev\n@b\r@c\u{2028}@d", true, 4),
        ("없음", false, 0),
        ("@", false, 0),
    ];
    for (text, mentions_me, mention_count) in cases {
        assert_eq!(scan(text), MentionInfo { mentions_me, mention_count }, "{text}");
    }
}

#[test]
fn 멘션_수는_50에서_멈춘다() {
    let text = (0..60).map(|index| format!("@u{index}")).collect::<Vec<_>>().join(" ");
    assert_eq!(scan(&text).mention_count, 50);
}

#[test]
fn 문자열이_아닌_본문은_멘션이_없다() {
    for body in [json!(1), json!(["@kairos-dev"]), json!({ "a": "@kairos-dev" })] {
        assert_eq!(MentionScanner::shared().scan(Some(&body)), Ok(MentionInfo::default()));
    }
    assert_eq!(MentionScanner::shared().scan(None), Ok(MentionInfo::default()));
}

/// 기본 한도로 실패하던 입력이다. 본문을 자르지 않으므로 맨 끝의 멘션까지 잡는다.
#[test]
fn 긴_본문의_맨_끝_멘션을_놓치지_않는다() {
    let padding = 1_100_000;
    for filler in ["x".repeat(padding), "x ".repeat(padding / 2), "@".repeat(padding), "@a ".repeat(padding / 3)] {
        let found = scan(&format!("{filler} @kairos-dev"));
        assert!(found.mentions_me);
    }
}

#[test]
fn 엔진이_판정을_끝내지_못하면_멘션_없음이_아니라_실패다() {
    let body = json!("앞쪽 글자 뒤의 @kairos-dev");
    // 두 패턴은 각자 실패한다. 한쪽만 삼켜도 멘션 여부나 멘션 수를 조용히 놓친다.
    for (self_limit, any_limit) in [(0, usize::MAX), (usize::MAX, 0), (0, 0)] {
        let scanner = MentionScanner::with_backtrack_limits(self_limit, any_limit);
        assert_eq!(scanner.scan(Some(&body)), Err(ScanFailed));
    }
}

#[test]
fn 멘션을_판정하지_못한_노드는_못_읽은_노드다() {
    let scanner = MentionScanner::with_backtrack_limits(0, 0);
    let parser = NodeParser::with_scanner(&scanner);
    assert_eq!(parser.parse(&nodes()["issue_assigned"]), None);
    let no_mention = spread(&nodes()["issue_closed"], json!({ "bodyText": "멘션 없는 본문" }));
    assert!(parser.parse(&no_mention).is_some());
}

#[test]
fn 멘션_판정_실패는_partial로_드러나고_watermark가_멈춘다() {
    let fake = FakeGh::new(FakeOptions { search: vec![("mentions:", ids(&["I_synth_c"]))], ..FakeOptions::default() });
    let scanner: &'static MentionScanner = Box::leak(Box::new(MentionScanner::with_backtrack_limits(0, 0)));
    let github: GithubSource = source(fake).with_parser(NodeParser::with_scanner(scanner));
    let results = github.collect(&context(Vec::new()));
    let mentions = query(&results, "mentions");
    assert_eq!(mentions.outcome, Outcome::Partial);
    assert_eq!(mentions.error_code.as_deref(), Some("node_unreadable"));
    assert_eq!(mentions.watermark, None);
    assert!(mentions.events.is_empty());
    assert_eq!(query(&results, "authored").outcome, Outcome::Ok);
}

/// 기대값은 Node의 `String(number)` 실측이다.
#[test]
fn 번호와_id는_js_숫자_표기로_키에_들어간다() {
    let comment = |id: Value| json!({ "databaseId": id, "createdAt": "2026-03-01T00:00:00Z", "bodyText": "합성" });
    let cases = [
        (json!(101), "gh:comment:101"),
        (json!(1.0), "gh:comment:1"),
        (json!(1.5), "gh:comment:1.5"),
        (json!(-0.0), "gh:comment:0"),
        (json!(1e21), "gh:comment:1e+21"),
        (json!(1.5e300), "gh:comment:1.5e+300"),
        (json!(1e-7), "gh:comment:1e-7"),
        (json!(123456789012345680000.0), "gh:comment:123456789012345680000"),
        (json!(9007199254740993_u64), "gh:comment:9007199254740992"),
    ];
    for (id, key) in cases {
        let node = spread(&nodes()["issue_closed"], json!({ "comments": { "nodes": [comment(id)] } }));
        let events = NodeParser::new().parse(&node).unwrap().events;
        assert_eq!(events[0].dedupe_key, key);
    }
}
