//! 서브커맨드. 입력은 항목 ID뿐이고 add의 제목만 예외다. 제목은 저장만 한다.
//! accept, move, runs, reveal만 Orca를 부른다. 나머지는 DB와 파일만 건드린다.

mod collect;
mod config;
mod connect;
mod demo;
mod doctor;
mod install;
mod items;
mod list;
mod mcp;
mod mode;
mod next;
mod notifier;
mod reveal;
mod sessions;
mod settings;
mod setup;
mod shared;
mod sources;
mod surface;

use crate::command::CommandRegistry;

/// 새 명령은 여기와 `help`의 표에 한 줄씩 더한다.
pub fn registry() -> CommandRegistry {
    CommandRegistry::new()
        .with(surface::Hello)
        .with(list::List)
        .with(next::Next)
        .with(surface::Watch)
        .with(reveal::Reveal)
        .with(sessions::Accept)
        .with(sessions::Move)
        .with(sessions::Pr)
        .with(sessions::Runs)
        .with(items::Later)
        .with(items::Done)
        .with(items::Keep)
        .with(items::Add)
        .with(items::Triage)
        .with(settings::Focus)
        .with(settings::Display)
        .with(sessions::Sessions)
        .with(settings::Notify)
        .with(doctor::Doctor)
        .with(settings::Pause)
        .with(settings::Off)
        .with(collect::Collect)
        .with(mode::PlanTick)
        .with(mode::PageInfo)
        .with(mode::Ingest)
        .with(setup::Setup)
        .with(config::Config)
        .with(sources::Sources)
        .with(mcp::Mcp)
        .with(connect::Connect)
        .with(install::Install)
        .with(install::Uninstall)
        .with(demo::Demo)
}
