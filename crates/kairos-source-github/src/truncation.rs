use kairos_core::Instant;
use serde_json::Value;

use crate::json::{nodes, text};

fn hides_older(connection: Option<&Value>) -> bool {
    let page_info = connection.and_then(|value| value.get("pageInfo"));
    page_info.and_then(|value| value.get("hasPreviousPage")).and_then(Value::as_bool) == Some(true)
}

/// last:N 연결의 앞쪽이 잘렸고, 보이는 가장 오래된 항목이 since 이후면 그 앞에 새 항목이 있을 수 있다.
fn may_skip_new(connection: Option<&Value>, field: &str, since: Instant) -> bool {
    if !hides_older(connection) {
        return false;
    }
    let items = nodes(connection);
    let oldest = text(items.first().and_then(|item| item.get(field))).and_then(Instant::parse);
    oldest.is_none_or(|at| at >= since)
}

/// 고정 창 밖에 새 활동이 남았을 수 있는 연결의 이름. 비어 있으면 이 노드는 온전하다.
/// 리뷰 스레드는 오래된 스레드에도 새 답글이 달리므로 시각과 무관하게 잘리면 잘린 것이다.
/// since가 None이면 첫 수집이다. 그 이전의 일은 baseline이라 잘려도 잃는 것이 없다.
pub(crate) fn truncated_connections(raw: &Value, since: Option<Instant>) -> Vec<&'static str> {
    let Some(since) = since.filter(|_| raw.is_object()) else {
        return Vec::new();
    };
    let threads = raw.get("reviewThreads");
    let mut names = Vec::new();
    if may_skip_new(raw.get("comments"), "createdAt", since) {
        names.push("comments");
    }
    if may_skip_new(raw.get("reviews"), "submittedAt", since) {
        names.push("reviews");
    }
    if may_skip_new(raw.get("timelineItems"), "createdAt", since) {
        names.push("timelineItems");
    }
    if hides_older(threads) {
        names.push("reviewThreads");
    }
    if nodes(threads).iter().any(|thread| may_skip_new(thread.get("comments"), "createdAt", since)) {
        names.push("threadComments");
    }
    names
}
