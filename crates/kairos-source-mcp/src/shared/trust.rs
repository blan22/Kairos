use std::sync::LazyLock;

use kairos_core::js;
use kairos_core::model::TrustTier;
use regex::Regex;

/// 표시 이름은 발신자가 정한다. 끝에 있는 주소만 실제 주소다.
static ANGLE_ADDRESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"<([^<>{ws}]+@[^<>{ws}]+)>[{ws}]*$", ws = js::WS)).expect("상수 정규식"));

/// 글자 끝의 꺾쇠 주소.
pub(crate) fn angle_address(text: &str) -> Option<&str> {
    Some(ANGLE_ADDRESS.captures(text)?.get(1)?.as_str())
}

pub(crate) fn is_trusted_address(address: Option<&str>) -> bool {
    let Some(address) = address.filter(|address| !address.is_empty()) else {
        return false;
    };
    // 설계 8절의 신뢰 도메인은 조직 프로필에서 온다. 목록 밖은 외부다. 그룹 캘린더는 ID로만 믿는다(calendar).
    kairos_core::profile::current().org.is_trusted_address(address)
}

pub(crate) fn trust_of(address: Option<&str>) -> TrustTier {
    if is_trusted_address(address) { TrustTier::T1 } else { TrustTier::T3 }
}
