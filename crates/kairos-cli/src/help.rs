//! `kairos help`. 명령을 하는 일로 묶어 한 줄 설명과 함께 보여 준다.
//!
//! 설명과 묶음은 이 표에만 있다. `Command::usage`는 `--json` 오류 답의 계약이라 손대지 않는다.
//! 그 줄이 없는 명령(`doctor`, `pause` 등)의 사용법도 이 표에 둔다. 등록한 명령이 표에 없으면 시험이 실패한다.
//! 호스트와 수집이 부르는 명령은 `kairos help --all`에만 나온다.

use crate::app::Console;
use crate::command::CommandRegistry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Group {
    Daily,
    Sessions,
    Display,
    Setup,
    /// 호스트(패널, 확장)와 수집이 부른다. 사람이 직접 칠 일이 없다.
    Internal,
}

impl Group {
    const SHOWN: [Self; 5] = [Self::Daily, Self::Sessions, Self::Display, Self::Setup, Self::Internal];

    const fn title(self) -> &'static str {
        match self {
            Self::Daily => "매일 쓰는 명령",
            Self::Sessions => "세션에 맡기기",
            Self::Display => "표시와 알림",
            Self::Setup => "설치, 설정, 점검",
            Self::Internal => "호스트와 수집이 부르는 명령",
        }
    }
}

pub struct Entry {
    pub name: &'static str,
    pub group: Group,
    pub summary: &'static str,
    /// `Command::usage`가 없는 명령의 사용법 줄.
    pub usage: Option<&'static str>,
}

const fn entry(name: &'static str, group: Group, summary: &'static str) -> Entry {
    Entry { name, group, summary, usage: None }
}

const fn with_usage(name: &'static str, group: Group, summary: &'static str, usage: &'static str) -> Entry {
    Entry { name, group, summary, usage: Some(usage) }
}

/// 묶음 안의 순서가 도움말에 나오는 순서다.
pub const ENTRIES: [Entry; 36] = [
    entry("next", Group::Daily, "지금 볼 일 하나"),
    entry("list", Group::Daily, "열린 항목"),
    entry("later", Group::Daily, "나중에 다시 본다"),
    entry("done", Group::Daily, "끝냈다(--drop은 버린다)"),
    entry("keep", Group::Daily, "할 일로 남겨 둔다(--until-deploy는 배포 때 다시)"),
    entry("add", Group::Daily, "할 일을 직접 더한다"),
    entry("triage", Group::Daily, "담당 이슈, 내 PR, 리뷰 요청을 10건씩 추린다"),
    entry("focus", Group::Daily, "포커스. 그동안 알림을 쌓아 둔다"),
    entry("accept", Group::Sessions, "항목을 claude 세션에 맡긴다(--cancel은 되돌린다)"),
    entry("move", Group::Sessions, "그 항목의 세션 창으로 간다"),
    entry("pr", Group::Sessions, "세션이 커밋한 브랜치를 draft PR로 올린다"),
    entry("runs", Group::Sessions, "세션 상태"),
    entry("sessions", Group::Sessions, "세션을 여는 잠금을 켜고 끈다"),
    entry("display", Group::Display, "카드와 알림 표시를 켜고 끈다"),
    entry("notify", Group::Display, "P1 알림 켜고 끄기, Mac 알림 권한(setup)과 시험 알림(test)"),
    with_usage("pause", Group::Display, "표시를 잠시 멈춘다. 수집은 계속한다", "사용법: kairos pause [분] | --resume"),
    with_usage("off", Group::Display, "쉬는 날로 적는다", "사용법: kairos off [YYYY-MM-DD] [--clear]"),
    entry("setup", Group::Setup, "처음 설정. 조직, 서비스, 계정, 설치, 알림 권한, Orca 패널"),
    with_usage("doctor", Group::Setup, "수집과 설정 상태를 점검한다", "사용법: kairos doctor"),
    entry("sources", Group::Setup, "연동 서비스를 켜고 끈다"),
    entry("connect", Group::Setup, "본인 계정과 MCP 연결을 적는다"),
    entry("config", Group::Setup, "설정 파일을 검사하고 보여 준다"),
    with_usage(
        "install",
        Group::Setup,
        "실행본을 설치하고 스케줄러(launchd, systemd, 작업 스케줄러)에 등록한다",
        "사용법: kairos install [--dry-run [--preview-dir 폴더]] | --apply | --register [--claude 경로] [--gh 경로] [--root 폴더]",
    ),
    with_usage(
        "uninstall",
        Group::Setup,
        "스케줄러 잡을 내린다(--runtime은 실행본까지, --purge는 데이터까지 지운다)",
        "사용법: kairos uninstall [--dry-run] [--runtime | --purge] [--yes]",
    ),
    with_usage(
        "collect",
        Group::Setup,
        "수집을 한 번 돌리거나 멈춘 수집을 푼다",
        "사용법: kairos collect [--force] | --resume | --skip-slack-until <Message_ts 또는 ISO 시각> | --reread-notion",
    ),
    entry("update", Group::Setup, "지금 판과 업데이트하는 명령을 알려 준다"),
    entry("completions", Group::Setup, "셸 자동 완성 스크립트를 낸다(zsh, bash)"),
    entry("demo", Group::Setup, "계정 없이 합성 데이터로 본다"),
    entry("hello", Group::Internal, "호스트가 시작할 때 계약 판과 구성을 읽는다"),
    entry("watch", Group::Internal, "상태가 바뀌면 한 줄을 낸다"),
    entry("reveal", Group::Internal, "알림을 누르면 그 항목을 패널에 연다"),
    with_usage("plan-tick", Group::Internal, "수집 모드: 이번 틱의 질의", "사용법: kairos plan-tick"),
    with_usage("page-info", Group::Internal, "수집 모드: 쪽 정보", "사용법: kairos page-info < JSON"),
    with_usage("ingest", Group::Internal, "수집 모드: 수집 결과를 들인다", "사용법: kairos ingest < JSON"),
    entry("mcp", Group::Internal, "kairos를 MCP 도구로 낸다"),
    with_usage("watchdog", Group::Internal, "감시자: 수집이 멈췄는지 본다(Linux, Windows)", "사용법: kairos watchdog"),
];

pub fn find(name: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.name == name)
}

/// `kairos --version`.
pub fn version_line() -> String {
    format!("kairos {}", env!("CARGO_PKG_VERSION"))
}

/// 명령 전체. `all`이 아니면 내부 명령을 뺀다.
pub fn print_overview(console: &Console, registry: &CommandRegistry, all: bool) {
    console.print(&version_line());
    console.print("회사 일정과 일거리를 모아 지금 볼 일 하나를 보여 줍니다.");
    console.print("");
    console.print("사용법: kairos <명령> [옵션]");
    let names = registry.names();
    let width = names.iter().map(|name| name.len()).max().unwrap_or(0);
    for group in Group::SHOWN {
        if group == Group::Internal && !all {
            continue;
        }
        let entries: Vec<&Entry> =
            ENTRIES.iter().filter(|entry| entry.group == group && names.contains(&entry.name)).collect();
        if entries.is_empty() {
            continue;
        }
        console.print("");
        console.print(group.title());
        for entry in entries {
            console.print(&format!("  {:<width$}  {}", entry.name, entry.summary));
        }
    }
    // 표에 없는 명령도 숨기지 않는다.
    let unlisted: Vec<&str> = names.iter().copied().filter(|name| find(name).is_none()).collect();
    if all && !unlisted.is_empty() {
        console.print("");
        console.print(&format!("그 밖: {}", unlisted.join(", ")));
    }
    console.print("");
    console.print("명령 하나의 사용법: kairos help <명령> 또는 kairos <명령> --help");
    if !all {
        console.print("내부 명령까지 보기: kairos help --all");
    }
    console.print("처음이면 kairos setup, 이상하면 kairos doctor. 문서: docs/commands.md");
}

/// 명령 하나. 설명과 사용법 줄이다. 없는 명령이면 false다.
pub fn print_command(console: &Console, registry: &CommandRegistry, name: &str) -> bool {
    if registry.find(name).is_none() {
        return false;
    }
    if let Some(entry) = find(name) {
        console.print(&format!("kairos {name}: {}", entry.summary));
    }
    match usage_of(registry, name) {
        Some(usage) => console.print(usage),
        None => console.print(&format!("사용법: kairos {name}")),
    }
    true
}

/// 명령의 사용법 줄. `Command::usage`가 없으면 표의 줄이다.
fn usage_of(registry: &CommandRegistry, name: &str) -> Option<&'static str> {
    registry.find(name).and_then(|command| command.usage()).or_else(|| find(name).and_then(|entry| entry.usage))
}

/// 잘못 친 명령에 가장 가까운 이름. 두 글자 안쪽으로 다를 때만 권한다. 내부 명령은 권하지 않는다.
pub fn suggest(registry: &CommandRegistry, typed: &str) -> Option<&'static str> {
    let typed: Vec<char> = typed.chars().collect();
    if typed.is_empty() || typed.len() > 32 {
        return None;
    }
    registry
        .names()
        .into_iter()
        .filter(|name| find(name).is_none_or(|entry| entry.group != Group::Internal))
        .map(|name| (distance(&typed, &name.chars().collect::<Vec<_>>()), name))
        .filter(|(distance, name)| *distance <= 2 && *distance < name.len())
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, name)| name)
}

/// 편집 거리(넣기, 빼기, 바꾸기).
fn distance(left: &[char], right: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=right.len()).collect();
    for (i, a) in left.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, b) in right.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if a == b { diagonal } else { 1 + diagonal.min(above).min(row[j]) };
            diagonal = above;
        }
    }
    row[right.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 등록한_명령은_모두_도움말_표에_있고_사용법_줄이_있다() {
        let registry = crate::registry();
        let names = registry.names();
        for name in &names {
            let entry = find(name).unwrap_or_else(|| panic!("도움말 표에 {name}이 없다"));
            assert!(!entry.summary.is_empty(), "{name}");
            assert!(usage_of(&registry, name).is_some(), "{name}의 사용법 줄이 없다");
        }
        for entry in &ENTRIES {
            assert!(names.contains(&entry.name), "표의 {}은 등록되지 않았다", entry.name);
            let twice =
                registry.find(entry.name).and_then(|command| command.usage()).is_some() && entry.usage.is_some();
            assert!(!twice, "{}의 사용법 줄이 두 곳에 있다", entry.name);
        }
        assert_eq!(names.len(), ENTRIES.len());
    }

    #[test]
    fn 가까운_이름만_권한다() {
        let registry = crate::registry();
        assert_eq!(suggest(&registry, "nxt"), Some("next"));
        assert_eq!(suggest(&registry, "docter"), Some("doctor"));
        assert_eq!(suggest(&registry, "sorces"), Some("sources"));
        assert_eq!(suggest(&registry, "보내기"), None);
        assert_eq!(suggest(&registry, "constructor"), None);
        assert_eq!(suggest(&registry, ""), None);
        // 내부 명령은 권하지 않는다.
        assert_eq!(suggest(&registry, "ingst"), None);
    }

    #[test]
    fn 편집_거리를_센다() {
        let chars = |text: &str| text.chars().collect::<Vec<_>>();
        assert_eq!(distance(&chars("next"), &chars("next")), 0);
        assert_eq!(distance(&chars("nxt"), &chars("next")), 1);
        assert_eq!(distance(&chars("lsit"), &chars("list")), 2);
        assert_eq!(distance(&chars(""), &chars("pr")), 2);
    }
}
