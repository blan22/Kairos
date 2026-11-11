use kairos_agenda::{HealthStatus, SourceHealth};
use kairos_ops::SLEEP_INTERRUPTED;

pub(super) const fn health_label(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Ok => "정상",
        HealthStatus::Delayed => "지연",
        HealthStatus::Never => "수집 기록 없음",
        HealthStatus::NotConnected => "미연결",
        HealthStatus::Quiet => "판정 안 함(수집 시간 밖)",
    }
}

/// partial의 원인마다 복구가 다르다. 저절로 풀리는 것과 사람이 볼 것을 가른다.
fn code_hint(code: &str) -> Option<&'static str> {
    Some(match code {
        "notion_read_cap" => "다음 틱에 이어 읽음",
        "notion_backlog" => "밀린 메일을 하루씩 읽는 중",
        "page_cap" => "쪽 상한. 다음 틱에 다시 읽음",
        "record_null" => "응답에 빈 항목이 낌. 나머지는 저장했고 다음 틱에 다시 읽음",
        "page_cap_stalled" => "쪽 상한으로 멈춤. 확인 필요",
        "mode_deadline" => "틱 마감. 다음 틱에 다시 읽음",
        "mode_hung" => "호출이 끝나지 않음. 계속되면 격리됨",
        "notion_body" => "본문이 비어 있음. 계속되면 격리됨",
        "notion_sender_mismatch" => "발신자 형식 변경 의심. 확인 필요",
        "large_response_missing" => "큰 응답 파일 없음. 다음 틱에 다시 읽음",
        "large_response_outside" => "큰 응답 경로 불일치. 확인 필요",
        "large_response_over_limit" => "큰 응답이 회수 상한을 넘음. 저절로 풀리지 않음",
        "mcp_response_too_large" => "응답이 토큰 상한을 넘음. 저절로 풀리지 않음",
        SLEEP_INTERRUPTED => "Mac 잠자기로 끊김. 고장이 아니고 다음 틱에 다시 읽음",
        _ => return None,
    })
}

pub(super) fn code_note(row: &SourceHealth) -> String {
    let Some(code) = row.last_error_code.as_deref() else {
        return String::new();
    };
    match code_hint(code) {
        Some(hint) => format!(" ({code}: {hint})"),
        None => format!(" ({code})"),
    }
}
