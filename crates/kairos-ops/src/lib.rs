//! 운영: 큰 응답 파일, 잠자기 판정, 유지보수, 설치
//!
//! - `ToolResults` 수집 프로젝트 폴더의 큰 응답 파일을 되찾고 지운다. 보안 경계다.
//! - `StallWatch`, `SleepDetector`, `ResumeFile`, `TickClockRepo` 잠자기와 고장을 나눈다.
//! - `Maintenance`, `MaintenanceRepo` 보존 정리, 백업, 하루 한 번의 정비
//! - `Installer` 실행본 배치와 launchd 등록

mod fs_util;
mod install;
mod maintenance;
mod posix;
mod sleep;
mod tick_clock;
mod tool_results;

pub use install::collector_allowlist;
pub use install::{
    COLLECT_LABEL, InstallMode, InstallOptions, InstallPaths, InstallPlan, InstallResult, Installer, LABELS, PlanInput,
    PlistFile, UninstallOptions, UninstallResult, VERSION, WATCHDOG_LABEL, default_launch_agents_dir, find_binary,
    find_claude, is_legacy_label, legacy_labels, plan_install, render_template,
};
pub use maintenance::{
    BACKUP_KEEP, Maintenance, MaintenanceRecord, MaintenanceRepo, OPEN_TEXT_RETENTION_DAYS, backup_dir,
    maintenance_path,
};
pub use sleep::{
    BEAT_MS, ResumeFile, SLEEP_GAP_MS, SLEEP_INTERRUPTED, SleepDetector, StallWatch, TICK_GAP_MS, counts_as_failure,
    parse_sleep_count, slept_during,
};
pub use tick_clock::TickClockRepo;
pub use tool_results::{
    MAX_RECOVERED_BYTES, RecoveredResponses, ToolResults, ToolResultsSweep, collect_cwd, collector_project_dir,
    project_slug,
};
