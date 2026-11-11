//!
//! `모든 last 연결이 pageInfo를 읽는다`는 `queries.rs`에 있다.
//! `ingest 통합: ...`은 kairos-ledger가 필요해 3단계로 넘겼다.

use std::collections::HashSet;
use std::sync::Arc;

use kairos_core::model::{CursorState, DirectSource, Outcome, SourceResult};
use kairos_core::process::{ProcessEnd, ProcessOutput};
use serde_json::{Map, Value, json};

use super::fake::{
    FakeGh, FakeOptions, FnRunner, NOW, by_id, context, cursor, ids, query, search_table, source, spread,
};

fn collect(fake: &Arc<FakeGh>) -> Vec<SourceResult> {
    source(fake.clone()).collect(&context(Vec::new()))
}

fn with_search() -> FakeOptions {
    FakeOptions { search: search_table(), ..FakeOptions::default() }
}

fn external_ids(result: &SourceResult) -> Vec<&str> {
    result.objects.iter().map(|object| object.external_id.as_str()).collect()
}

#[test]
fn 다섯_질의가_설계의_검색어로_org_contoso_고정이다_updated_필터는_멘션과_닫힘에만_있다() {
    let fake = FakeGh::new(with_search());
    let results = collect(&fake);
    assert_eq!(results.len(), 5);
    let calls = fake.calls();
    let queries: Vec<&str> = calls.iter().filter(|call| call.is_search()).map(|call| call.first_field()).collect();
    assert_eq!(queries.len(), 5);
    assert!(queries.iter().all(|q| q.starts_with("q=org:contoso ")));
    assert!(queries.iter().any(|q| q.contains("review-requested:kairos-dev") && !q.contains("updated")));
    assert!(queries.iter().any(|q| q.contains("assignee:kairos-dev") && !q.contains("updated")));
    assert!(queries.iter().any(|q| q.contains("author:kairos-dev") && !q.contains("updated")));
    assert!(queries.iter().any(|q| q.contains("mentions:kairos-dev") && q.contains("updated:>=2026-02-23T01:00:00Z")));
    assert!(queries.iter().any(|q| q.contains("is:closed") && q.contains("updated:>=")));
}

#[test]
fn 토큰은_gh_auth_token_user_본인로_받아_검색과_graphql_자식_환경에만_넘긴다() {
    let fake = FakeGh::new(with_search());
    collect(&fake);
    let calls = fake.calls();
    assert_eq!(calls[0].args, ["auth", "token", "--user", "kairos-dev"]);
    assert_eq!(calls[0].env.get("GH_TOKEN"), None);
    for call in &calls[1..] {
        assert_eq!(call.env.get("GH_TOKEN").map(String::as_str), Some("synthetic-token"));
        assert!(!call.args.join(" ").contains("synthetic-token"));
    }
}

#[test]
fn 읽기만_한다_api_호출은_get_검색_rate_limit_graphql_질의뿐이고_mutation이_없다() {
    let fake = FakeGh::new(with_search());
    collect(&fake);
    for call in fake.calls().iter().filter(|call| call.args[0] == "api") {
        let kind = match call.args[1].as_str() {
            "rate_limit" => "rate".to_owned(),
            "graphql" => "graphql".to_owned(),
            _ => call.args[1..4].join(" "),
        };
        assert!(["rate", "graphql", "-X GET search/issues"].contains(&kind.as_str()), "{kind}");
        assert!(!call.args.iter().any(|arg| ["POST", "PATCH", "DELETE"].contains(&arg.as_str())));
        if let Some(input) = &call.stdin {
            assert!(!input.to_lowercase().contains("mutation"));
        }
    }
}

#[test]
fn graphql_질의는_viewer를_단언하고_updated_at을_가져오지_않는다_id는_변수로_간다() {
    let fake = FakeGh::new(with_search());
    collect(&fake);
    let calls = fake.calls();
    let graphql: Vec<_> = calls.iter().filter(|call| call.is_graphql()).collect();
    assert!(graphql.len() >= 2);
    for call in graphql {
        let query = call.query_text();
        assert!(query.contains("viewer { login }"));
        assert!(!query.contains("updatedAt"));
        assert!(!call.args.iter().any(|arg| arg.contains("PR_synth")));
    }
}

#[test]
fn 한_객체는_우선순위가_높은_질의_하나만_낸다_겹친_후보로_이벤트가_중복되지_않는다() {
    let results = collect(&FakeGh::new(with_search()));
    let keys: Vec<&str> =
        results.iter().flat_map(|result| &result.objects).map(|object| object.object_key.as_str()).collect();
    assert_eq!(keys.iter().collect::<HashSet<_>>().len(), keys.len());
    assert_eq!(external_ids(query(&results, "review_requested")), ["example-org/widget#12"]);
    assert_eq!(external_ids(query(&results, "assigned")), ["example-org/widget#13"]);
    assert_eq!(external_ids(query(&results, "authored")), ["example-org/widget#11"]);
    assert!(query(&results, "mentions").objects.is_empty());
    let mut states: Vec<_> =
        query(&results, "closed").objects.iter().map(|object| object.state.clone().unwrap()).collect();
    states.sort();
    assert_eq!(states, ["closed", "merged"]);
    let dedupe: Vec<&str> =
        results.iter().flat_map(|result| &result.events).map(|event| event.dedupe_key.as_str()).collect();
    assert_eq!(dedupe.iter().collect::<HashSet<_>>().len(), dedupe.len());
}

#[test]
fn 정상_수집_ok_신원_본인_건수_일치_열림_질의만_snapshot_watermark는_수집_시작_시각() {
    let results = collect(&FakeGh::new(with_search()));
    for result in &results {
        assert_eq!(result.outcome, Outcome::Ok);
        assert_eq!(result.identity.as_deref(), Some("kairos-dev"));
        assert_eq!(result.expected_count, Some(result.parsed_count));
        assert_eq!(result.watermark.as_deref(), Some(NOW));
        assert_eq!(result.overlap_seconds, 300);
    }
    let snapshots: Vec<&str> =
        results.iter().filter(|result| result.snapshot).map(|result| result.query.as_str()).collect();
    assert_eq!(snapshots, ["review_requested", "assigned", "authored"]);
}

#[test]
fn 커서가_있으면_멘션_검색은_watermark_빼기_overlap부터_본다() {
    let fake = FakeGh::new(with_search());
    let watermark = cursor("2026-03-02T00:00:00.000Z", None);
    source(fake.clone()).collect(&context(vec![("mentions", watermark)]));
    let calls = fake.calls();
    let q = calls.iter().filter(|call| call.is_search()).map(|call| call.first_field());
    let mentions = q.into_iter().find(|q| q.contains("mentions:")).unwrap();
    assert!(mentions.contains("updated:>=2026-03-01T23:55:00Z"), "{mentions}");
}

#[test]
fn viewer가_본인가_아니면_전부_contract_break이고_검색하지_않는다() {
    let fake = FakeGh::new(FakeOptions { viewer: Some("someone-else"), ..with_search() });
    let results = collect(&fake);
    assert!(results.iter().all(|result| {
        result.outcome == Outcome::ContractBreak
            && result.error_code.as_deref() == Some("identity_mismatch")
            && result.objects.is_empty()
    }));
    assert!(!fake.calls().iter().any(|call| call.is_search()));
}

#[test]
fn 검색_한도_여유가_없으면_검색하지_않고_rate_limited로_끝난다() {
    let fake = FakeGh::new(FakeOptions { search_remaining: Some(3), ..with_search() });
    let results = collect(&fake);
    assert!(results.iter().all(|result| result.outcome == Outcome::RateLimited));
    assert!(!fake.calls().iter().any(|call| call.is_search()));
}

#[test]
fn 질의_하나의_검색이_한도에_걸려도_다른_질의는_진행한다() {
    let results = collect(&FakeGh::new(FakeOptions { fail_search: Some("assignee:"), ..with_search() }));
    assert_eq!(query(&results, "assigned").outcome, Outcome::RateLimited);
    assert_eq!(query(&results, "authored").outcome, Outcome::Ok);
}

#[test]
fn 검색_건수가_읽은_건수보다_크면_partial이고_watermark를_주지_않는다() {
    let results = collect(&FakeGh::new(FakeOptions { total_count: Some(250), ..with_search() }));
    let authored = query(&results, "authored");
    assert_eq!(authored.outcome, Outcome::Partial);
    assert_eq!(authored.error_code.as_deref(), Some("search_truncated"));
    assert_eq!(authored.watermark, None);
}

#[test]
fn graphql이_노드를_못_읽으면_partial이다_0건처럼_ok로_내지_않는다() {
    let results = collect(&FakeGh::new(FakeOptions { null_ids: vec!["PR_synth_a"], ..with_search() }));
    assert_eq!(query(&results, "authored").outcome, Outcome::Partial);
    assert_eq!(query(&results, "authored").watermark, None);
}

#[test]
fn graphql이_실패하면_후보가_있던_질의만_실패하고_후보가_0건인_질의는_ok다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["PR_synth_a"]))],
        graphql_error: Some(""),
        ..FakeOptions::default()
    });
    // 첫 viewer 호출부터 실패하므로 전부 같은 사유로 끝난다.
    let results = collect(&fake);
    assert!(results.iter().all(|result| result.outcome == Outcome::ParseError && result.objects.is_empty()));
}

#[test]
fn 인증_실패와_gh_없음은_실패로_드러난다() {
    let auth = source(Arc::new(FnRunner(|_: &_| ProcessOutput {
        end: ProcessEnd::Exited(1),
        stdout: String::new(),
        stderr: "no oauth token found for kairos-dev".to_owned(),
    })))
    .collect(&context(Vec::new()));
    assert!(auth.iter().all(|result| result.outcome == Outcome::Auth));
    let missing =
        source(Arc::new(FnRunner(|_: &_| ProcessOutput::ended(ProcessEnd::Missing)))).collect(&context(Vec::new()));
    assert!(missing.iter().all(|result| result.outcome == Outcome::ToolMissing));
}

#[test]
fn 빈_결과는_ok다_실패와_0건을_구분한다() {
    let results = collect(&FakeGh::new(FakeOptions::default()));
    assert!(results.iter().all(|result| result.outcome == Outcome::Ok && result.parsed_count == 0));
}

fn mentions_cursor() -> CursorState {
    cursor("2026-03-02T00:00:00.000Z", Some("2026-03-01T00:00:00.000Z"))
}

fn reply(id: i64, body: &str) -> Value {
    json!({
        "databaseId": id, "createdAt": "2026-03-02T00:30:00Z", "bodyText": body,
        "url": format!("https://example.invalid/rc/{id}"), "authorAssociation": "MEMBER",
        "author": { "__typename": "User", "login": "synthetic-teammate" },
    })
}

fn node_map(id: &str, node: Value) -> Map<String, Value> {
    Map::from_iter([(id.to_owned(), node)])
}

fn mentions_of(fake: &Arc<FakeGh>) -> SourceResult {
    let results = source(fake.clone()).collect(&context(vec![("mentions", mentions_cursor())]));
    query(&results, "mentions").clone()
}

#[test]
fn 리뷰_스레드가_창_밖이면_넓게_다시_읽어_오래된_스레드의_멘션을_잡는다() {
    let cut = json!({ "reviewThreads": { "pageInfo": { "hasPreviousPage": true }, "nodes": [] } });
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["PR_synth_b"]))],
        overrides: node_map("PR_synth_b", spread(&by_id("PR_synth_b"), cut)),
        expanded: node_map(
            "PR_synth_b",
            json!({
                "id": "PR_synth_b",
                "reviewThreads": {
                    "pageInfo": { "hasPreviousPage": false },
                    "nodes": [{ "comments": {
                        "pageInfo": { "hasPreviousPage": false }, "nodes": [reply(901, "@kairos-dev 확인 부탁")],
                    } }],
                },
            }),
        ),
        ..FakeOptions::default()
    });
    let mentions = mentions_of(&fake);
    assert_eq!(mentions.outcome, Outcome::Ok);
    let event = mentions.events.iter().find(|event| event.dedupe_key == "gh:rcomment:901").unwrap();
    assert_eq!(event.mentions_me, Some(true));
    assert_eq!(fake.calls().iter().filter(|call| call.is_expanded()).count(), 1);
}

// 같은 since로 다시 읽어도 같은 결과라 멈추면 영영 풀리지 않는다. 읽은 만큼 쓰고 넘긴 사실을 고지한다.
#[test]
fn 넓게_읽어도_잘리면_넘기고_전진하며_덮지_못한_구간으로_남긴다() {
    let cut = json!({ "reviewThreads": { "pageInfo": { "hasPreviousPage": true }, "nodes": [] } });
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["PR_synth_b"]))],
        overrides: node_map("PR_synth_b", spread(&by_id("PR_synth_b"), cut.clone())),
        expanded: node_map("PR_synth_b", spread(&json!({ "id": "PR_synth_b" }), cut)),
        ..FakeOptions::default()
    });
    let mentions = mentions_of(&fake);
    assert_eq!(mentions.outcome, Outcome::Ok);
    assert_eq!(mentions.error_code.as_deref(), Some("node_quarantined"));
    assert_eq!(mentions.watermark.as_deref(), Some(NOW));
    assert_eq!(mentions.window_from.as_deref(), Some("2026-03-01T23:55:00.000Z"));
    assert_eq!(external_ids(&mentions), ["example-org/widget#12"], "읽은 객체는 그대로 낸다");
}

#[test]
fn 주말_공백_뒤_댓글이_창을_넘으면_앞쪽_댓글의_멘션을_읽는다() {
    let late = spread(&reply(910, "뒤쪽 댓글"), json!({ "createdAt": "2026-03-02T00:40:00Z" }));
    let visible = json!({ "comments": { "pageInfo": { "hasPreviousPage": true }, "nodes": [late] } });
    let earlier = json!({ "pageInfo": { "hasPreviousPage": false }, "nodes": [reply(909, "@kairos-dev 앞쪽 멘션")] });
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        overrides: node_map("I_synth_c", spread(&by_id("I_synth_c"), visible)),
        expanded: node_map("I_synth_c", json!({ "id": "I_synth_c", "comments": earlier })),
        ..FakeOptions::default()
    });
    let mentions = mentions_of(&fake);
    assert_eq!(mentions.outcome, Outcome::Ok);
    let event = mentions.events.iter().find(|event| event.dedupe_key == "gh:comment:909").unwrap();
    assert_eq!(event.mentions_me, Some(true));
}

fn cut_comments() -> Value {
    let late = spread(&reply(910, "뒤쪽 댓글"), json!({ "createdAt": "2026-03-02T00:40:00Z" }));
    json!({ "comments": { "pageInfo": { "hasPreviousPage": true }, "nodes": [late] } })
}

#[test]
fn 넓은_읽기가_부분_실패하면_ok로_넘어가지_않고_읽어_둔_댓글을_지킨다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        overrides: node_map("I_synth_c", spread(&by_id("I_synth_c"), cut_comments())),
        expanded: node_map("I_synth_c", json!({ "id": "I_synth_c", "comments": null })),
        expanded_errors: true,
        ..FakeOptions::default()
    });
    let mentions = mentions_of(&fake);
    assert_eq!(mentions.outcome, Outcome::Partial);
    assert_eq!(mentions.error_code.as_deref(), Some("node_unreadable"));
    assert_eq!(mentions.watermark, None);
    assert!(mentions.events.iter().any(|event| event.dedupe_key == "gh:comment:910"));
}

#[test]
fn 넓은_읽기의_연결이_null이면_앞의_값으로_남아_잘린_채_넘긴다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        overrides: node_map("I_synth_c", spread(&by_id("I_synth_c"), cut_comments())),
        expanded: node_map("I_synth_c", json!({ "id": "I_synth_c", "comments": null })),
        ..FakeOptions::default()
    });
    let mentions = mentions_of(&fake);
    assert_eq!(mentions.outcome, Outcome::Ok);
    assert_eq!(mentions.error_code.as_deref(), Some("node_quarantined"));
    assert_eq!(mentions.watermark.as_deref(), Some(NOW));
    assert!(mentions.events.iter().any(|event| event.dedupe_key == "gh:comment:910"));
}

#[test]
fn 창_앞쪽이_이미_본_구간이면_다시_읽지_않는다() {
    let stale = spread(&reply(911, "오래된 댓글"), json!({ "createdAt": "2026-02-01T00:00:00Z" }));
    let old = json!({ "comments": { "pageInfo": { "hasPreviousPage": true }, "nodes": [stale] } });
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        overrides: node_map("I_synth_c", spread(&by_id("I_synth_c"), old)),
        ..FakeOptions::default()
    });
    assert_eq!(mentions_of(&fake).outcome, Outcome::Ok);
    assert_eq!(fake.calls().iter().filter(|call| call.is_expanded()).count(), 0);
}

// 원본에 없는 시험. 원본 시험의 이름은 이 동작을 말하지만 본문은 viewer 호출에서 끝나 여기까지 오지 못한다.
// 노드 하나만 읽어도 응답이 상한을 넘으면 다시 읽어도 같다. 그 질의는 노드를 넘기고 전진한다.
#[test]
fn 혼자_읽어도_넘치는_노드는_그_질의만_넘기고_후보가_0건인_질의는_그대로_ok다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["PR_synth_a"]))],
        hook: Some(Box::new(|request| {
            let reads_nodes = request.stdin.as_deref().is_some_and(|input| input.contains("PR_synth_a"));
            reads_nodes.then(|| ProcessOutput::ended(ProcessEnd::OutputTooLarge))
        })),
        ..FakeOptions::default()
    });
    let results = collect(&fake);
    let mentions = query(&results, "mentions");
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));
    assert_eq!(mentions.identity.as_deref(), Some("kairos-dev"));
    assert!(mentions.objects.is_empty() && mentions.events.is_empty());
    assert_eq!(query(&results, "authored").outcome, Outcome::Ok);
    assert_eq!(query(&results, "authored").error_code, None);
}

fn assigned(id: &str, at: &str) -> Value {
    json!({ "__typename": "AssignedEvent", "id": id, "createdAt": at, "assignee": { "login": "kairos-dev" } })
}

#[test]
fn 타임라인_창이_잘리면_넓게_다시_읽어_이벤트를_찾고_watermark가_전진한다() {
    let item = assigned("AE_9", "2026-03-02T00:30:00Z");
    let hidden = assigned("AE_8", "2026-03-02T00:20:00Z");
    let timeline = json!({ "timelineItems": { "pageInfo": { "hasPreviousPage": true }, "nodes": [item] } });
    let wide = json!({
        "id": "I_synth_c",
        "timelineItems": { "pageInfo": { "hasPreviousPage": false }, "nodes": [hidden, item] },
    });
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        overrides: node_map("I_synth_c", spread(&by_id("I_synth_c"), timeline)),
        expanded: node_map("I_synth_c", wide),
        ..FakeOptions::default()
    });
    let mentions = mentions_of(&fake);
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Ok, None));
    assert!(mentions.watermark.is_some());
    assert!(mentions.events.iter().any(|event| event.dedupe_key == "gh:assign:AE_8"), "창 밖의 배정을 찾아야 한다");
}

#[test]
fn 넓게_읽어도_타임라인이_잘려_있으면_넘기고_전진한다() {
    let item = assigned("AE_9", "2026-03-02T00:30:00Z");
    let timeline = json!({ "timelineItems": { "pageInfo": { "hasPreviousPage": true }, "nodes": [item] } });
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        overrides: node_map("I_synth_c", spread(&by_id("I_synth_c"), timeline.clone())),
        expanded: node_map("I_synth_c", spread(&json!({ "id": "I_synth_c" }), timeline)),
        ..FakeOptions::default()
    });
    let mentions = mentions_of(&fake);
    assert_eq!(mentions.outcome, Outcome::Ok);
    assert_eq!(mentions.error_code.as_deref(), Some("node_quarantined"));
    assert_eq!(mentions.watermark.as_deref(), Some(NOW));
    assert!(mentions.events.iter().any(|event| event.dedupe_key == "gh:assign:AE_9"), "읽은 이벤트는 그대로 낸다");
}

// ingest 통합 시험의 앞단. 저장은 3단계에서 시험하고, 여기서는 재수집의 키가 같은지만 본다.
#[test]
fn 같은_응답을_다시_읽으면_이벤트_키와_순서가_같다() {
    let keys = |results: Vec<SourceResult>| -> Vec<String> {
        results.into_iter().flat_map(|result| result.events).map(|event| event.dedupe_key).collect()
    };
    let first = keys(collect(&FakeGh::new(with_search())));
    assert!(!first.is_empty());
    assert_eq!(keys(collect(&FakeGh::new(with_search()))), first);
}
