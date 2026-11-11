//! 알림을 누르면 헬퍼가 부르는 명령. 항목 ID를 reveal.json에 남기고 패널을 연 뒤 Orca를 앞으로 끌어온다.

use std::thread;
use std::time::{Duration, Instant};

use kairos_agenda::{ORCA_BUNDLE_ID, OrcaPanelOpener, REVEAL_MAX_ITEMS, RevealFile, RevealView};
use kairos_core::KairosResult;
use kairos_core::ports::PanelOpener;
use kairos_core::process::ProcessRequest;
use serde_json::json;

use super::shared::parse_ids;
use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag};
use crate::command::Command;
use crate::reply::Reply;

const USAGE: &str = "사용법: kairos reveal <항목 ID>... [--json]";
const OPTIONS: [OptionSpec; 1] = [flag("json")];
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);
/// `execFile`의 기본 maxBuffer.
const MAX_OUTPUT: usize = 1024 * 1024;
/// Orca가 꺼져 있으면 `plugin open`은 바로 실패한다. `open -b`로 띄운 뒤 뜰 때까지 다시 연다.
/// 헬퍼는 10초 뒤 자기만 끝내고 이 프로세스는 남는다.
const PANEL_RETRY_FOR: Duration = Duration::from_secs(8);
const PANEL_RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// `open -b`. Orca 창을 앞으로 끌어온다. 패널 명령은 포커스를 가져가지 않는다.
fn activate_orca(context: &AppContext) -> bool {
    let request = ProcessRequest::new(context.open_bin(), ["-b", ORCA_BUNDLE_ID], OPEN_TIMEOUT).max_output(MAX_OUTPUT);
    context.runner().run(&request).success()
}

/// 앞으로 끌어온 뒤에도 패널이 닫혀 있으면 정해 둔 시간 동안 다시 연다. 끌어오지 못했으면 다시 열지 않는다.
fn open_panel_after_launch(opener: &OrcaPanelOpener) -> bool {
    let deadline = Instant::now() + PANEL_RETRY_FOR;
    while Instant::now() < deadline {
        thread::sleep(PANEL_RETRY_INTERVAL);
        if opener.open() {
            return true;
        }
    }
    false
}

pub struct Reveal;

impl Command for Reveal {
    fn name(&self) -> &'static str {
        "reveal"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(USAGE)
    }

    /// 파일을 먼저 쓴다. 패널이 이미 열려 있으면 워커가 파일을 보고 바로 옮긴다. Orca 단계가 실패해도 성공으로 답한다.
    /// Orca가 꺼져 있었으면 첫 `plugin open`이 실패한다. `open -b`로 띄운 뒤 다시 연다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let ids = match parse_ids(parsed.positionals(), reply) {
            Ok(ids) => ids,
            Err(code) => return Ok(code),
        };
        if ids.len() > REVEAL_MAX_ITEMS {
            return Ok(reply.usage(&format!("항목 ID는 {REVEAL_MAX_ITEMS}개까지 받습니다")));
        }
        // 같은 ID가 두 번 오면 한 번만 둔다. 순서는 알림의 순서다.
        let mut items: Vec<i64> = Vec::with_capacity(ids.len());
        for id in ids {
            if !items.contains(&id) {
                items.push(id);
            }
        }
        let reveal = RevealView { items, at: context.now().millis() };
        RevealFile::new(context.home()).write(&reveal)?;
        // reveal.json은 모든 호스트가 본다(VS Code 확장은 파일을 지켜보고 목록을 연다). Orca는 켰을 때만 부른다.
        let orca_enabled = kairos_core::profile::current().hosts.is_enabled("orca");
        let (mut panel_opened, mut activated) = (false, false);
        if orca_enabled {
            let opener = OrcaPanelOpener::with_bin(context.runner(), context.panel_orca_bin());
            panel_opened = opener.open();
            activated = activate_orca(context);
            if !panel_opened && activated {
                panel_opened = open_panel_after_launch(&opener);
            }
        }
        if reply.json() {
            return reply.success(json!({ "reveal": reveal, "panelOpened": panel_opened, "activated": activated }));
        }
        context.console().print(&if panel_opened || !orca_enabled {
            format!("알림의 항목 {}건을 패널에 보여 줍니다", reveal.items.len())
        } else {
            "패널을 열지 못했습니다. Orca에서 kairos 패널을 여세요".to_owned()
        });
        Ok(0)
    }
}
