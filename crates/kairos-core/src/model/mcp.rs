use std::collections::{BTreeMap, HashMap};
use std::fmt;

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// JS 객체의 정수 키 상한. 이 값 밖의 숫자 문자열은 보통 키처럼 삽입 순서를 따른다.
const MAX_ARRAY_INDEX: u64 = 4_294_967_294;

/// `"0"`이나 `"12"`처럼 JS가 배열 첨자로 보는 키의 값.
fn array_index(key: &str) -> Option<u64> {
    let canonical =
        !key.is_empty() && key.bytes().all(|byte| byte.is_ascii_digit()) && (key == "0" || !key.starts_with('0'));
    if canonical { key.parse().ok().filter(|index| *index <= MAX_ARRAY_INDEX) } else { None }
}

/// 다시 읽지 않을 대상과 처리한 메시지 수. plan-tick JSON에 키 순서가 그대로 남는다.
/// `Object.fromEntries`처럼 같은 키는 첫 자리에 뒤의 값이 들어가고, 배열 첨자 키는 오름차순으로 앞에 온다.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkipCounts(Vec<(String, i64)>);

impl SkipCounts {
    pub fn get(&self, key: &str) -> Option<&i64> {
        self.0.iter().find(|(known, _)| known == key).map(|(_, count)| count)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, i64)> + '_ {
        self.0.iter().map(|(key, count)| (key.as_str(), *count))
    }
}

impl FromIterator<(String, i64)> for SkipCounts {
    fn from_iter<I: IntoIterator<Item = (String, i64)>>(entries: I) -> Self {
        let mut slots: HashMap<String, usize> = HashMap::new();
        let mut rows: Vec<(String, i64)> = Vec::new();
        for (key, count) in entries {
            match slots.get(&key) {
                Some(slot) => rows[*slot].1 = count,
                None => {
                    slots.insert(key.clone(), rows.len());
                    rows.push((key, count));
                }
            }
        }
        // 안정 정렬이라 문자열 키는 삽입 순서를 지킨다.
        rows.sort_by_key(|(key, _)| array_index(key).unwrap_or(u64::MAX));
        Self(rows)
    }
}

/// 키가 같은지만 본다. 순서를 보려면 `iter`를 쓴다.
impl PartialEq<BTreeMap<String, i64>> for SkipCounts {
    fn eq(&self, other: &BTreeMap<String, i64>) -> bool {
        self.0.len() == other.len() && self.0.iter().all(|(key, count)| other.get(key) == Some(count))
    }
}

impl Serialize for SkipCounts {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, count) in &self.0 {
            map.serialize_entry(key, count)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for SkipCounts {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Counts;

        impl<'de> Visitor<'de> for Counts {
            type Value = SkipCounts;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("대상별 메시지 수")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<SkipCounts, A::Error> {
                let mut rows = Vec::new();
                while let Some(entry) = map.next_entry::<String, i64>()? {
                    rows.push(entry);
                }
                Ok(rows.into_iter().collect())
            }
        }

        deserializer.deserialize_map(Counts)
    }
}

/// 검색 뒤의 후속 읽기 지시. 대상은 모드가 같은 틱의 검색 결과에서만 뽑는다.
/// 도구와 상한의 실제 허용은 모드의 상수가 정하고, 이 값은 그 안에서만 쓰인다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpExpand {
    pub tool: String,
    pub max_reads: i64,
    /// 다시 읽지 않을 대상과 처리한 메시지 수. 검색의 메시지 수가 이보다 크면 읽는다.
    pub skip: SkipCounts,
    /// 실패한 적이 있는 대상. 처음 읽는 대상 뒤로 미룬다.
    pub retry_ids: Vec<String>,
}

/// 수집 모드가 `$.mcp.call`로 실행할 호출 1건. 실제 허용 여부는 모드의 상수 허용목록이 정한다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpCall {
    pub id: String,
    pub query: String,
    pub server: String,
    pub tool: String,
    pub args: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expand: Option<McpExpand>,
}

/// 모드가 돌려준 응답 한 쪽.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResponse {
    pub id: String,
    pub is_error: bool,
    #[serde(default)]
    pub content: Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(keys: &[&str]) -> SkipCounts {
        keys.iter().enumerate().map(|(index, key)| ((*key).to_owned(), index as i64)).collect()
    }

    #[test]
    fn 키_순서는_object_from_entries의_json과_같다() {
        let text = serde_json::to_string(&counts(&["z", "a", "10", "2", "02", "4294967295", "4294967294"])).unwrap();
        assert_eq!(text, r#"{"2":3,"10":2,"4294967294":6,"z":0,"a":1,"02":4,"4294967295":5}"#);
    }

    #[test]
    fn 같은_키는_첫_자리에_뒤의_값이_들어간다() {
        assert_eq!(serde_json::to_string(&counts(&["b", "a", "b"])).unwrap(), r#"{"b":2,"a":1}"#);
    }

    #[test]
    fn json으로_돌아와도_순서를_지킨다() {
        let parsed: SkipCounts = serde_json::from_str(r#"{"z":1,"a":2}"#).unwrap();
        assert_eq!(parsed.iter().collect::<Vec<_>>(), [("z", 1), ("a", 2)]);
        assert_eq!(parsed.get("a"), Some(&2));
        assert_eq!(parsed, BTreeMap::from([("a".to_owned(), 2), ("z".to_owned(), 1)]));
    }
}
