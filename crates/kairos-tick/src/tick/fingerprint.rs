//! collector.json. 계약 시험을 통과한 claude의 지문.

use std::fs;

use kairos_agenda::pretty_json;
use kairos_core::home::write_file_atomic;
use kairos_core::{Instant, KairosHome, KairosResult, js};
use serde_json::{Value, json};

const FILE: &str = "collector.json";

pub(super) fn read_fingerprint(home: &KairosHome) -> Option<String> {
    let bytes = fs::read(home.file(FILE)).ok()?;
    let parsed = js::parse_json(&String::from_utf8_lossy(&bytes)).ok()?;
    parsed.get("fingerprint").and_then(Value::as_str).map(str::to_owned)
}

pub(super) fn write_fingerprint(home: &KairosHome, fingerprint: &str, now: Instant) -> KairosResult<()> {
    let body = json!({ "fingerprint": fingerprint, "passedAt": now });
    write_file_atomic(&home.file(FILE), &pretty_json(&body)?)
}
