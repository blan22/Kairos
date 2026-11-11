use kairos_core::model::{ActorKind, EventType, NormalizedEvent, SourceName};
use kairos_core::{Instant, KairosResult, LocalDate, OffDates};
use kairos_policy::{DeployLink, DeployLinkStatus, DeployLinker, DeployStage};
use kairos_store::Connection;

use crate::event_recorder::{EventContext, EventRecorder};
use crate::repo::{CursorRepo, EventRepo, ObjectRepo, ObjectRow};

#[derive(Clone, Debug, PartialEq)]
pub struct UpcomingDeploy {
    pub object: ObjectRow,
    pub date: LocalDate,
    pub link: DeployLink,
}

/// 오늘 이후의 [배포] 일정과 연결 상태. 건수는 호출 시점에 계산한다.
pub fn upcoming_deploys(connection: &Connection, now: Instant) -> KairosResult<Vec<UpcomingDeploy>> {
    let today = now.local_date();
    let objects = ObjectRepo::new(connection);
    let candidates = objects.deploy_candidates()?;
    let mut deploys = Vec::new();
    for object in objects.live_deploys()? {
        let Some(date) = DeployLinker::date(&object.to_rule_object()) else {
            continue;
        };
        if date >= today {
            let link = DeployLinker::link(object.text_head.as_deref(), &candidates);
            deploys.push(UpcomingDeploy { object, date, link });
        }
    }
    deploys.sort_by_key(|deploy| deploy.date);
    Ok(deploys)
}

fn deploy_event(object: &ObjectRow, dedupe_key: String, event_type: EventType, now: Instant) -> NormalizedEvent {
    NormalizedEvent {
        object_key: Some(object.object_key.clone()),
        channel_id: object.channel_id.clone(),
        permalink: object.permalink.clone(),
        text_head: object.text_head.clone(),
        trust_tier: object.trust_tier,
        ..NormalizedEvent::new(dedupe_key, SourceName::Calendar, event_type, now.to_iso(), ActorKind::Bot)
    }
}

/// 단계의 이벤트 키. 배포일을 넣어 일정이 밀리면 같은 단계가 다시 나고, 같은 날짜는 한 번만 난다.
///
/// 날짜 없는 키는 이 형식 전에 저장된 행이다. 그 사건이 이번 날짜의 단계였으면 같은 사건으로 본다.
/// 사건의 보존 기간이 지나면 이 분기는 쓰이지 않는다.
fn stage_key(
    events: &EventRepo<'_>,
    object: &ObjectRow,
    stage: DeployStage,
    date: LocalDate,
    off_dates: &OffDates,
) -> KairosResult<String> {
    let undated = format!("deploy:{}:{stage}", object.external_id);
    let earlier = events.occurred_at(&undated)?;
    let same_event = earlier
        .as_deref()
        .and_then(Instant::parse)
        .is_some_and(|at| DeployLinker::stage(date, at.local_date(), off_dates) == Some(stage));
    Ok(if same_event { undated } else { format!("{undated}@{date}") })
}

/// 배포 위험과 그 해소를 이벤트로 만든다.
pub(crate) struct DeployWatch<'a> {
    connection: &'a Connection,
}

impl<'a> DeployWatch<'a> {
    pub(crate) const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    /// 배포 연결을 판정한다. Calendar와 GitHub 둘 다 한 번은 ok여야 한다. GitHub를 읽은 적이 없으면 연결 0건이 아니라 모르는 것이다.
    /// 연결이 사라졌다는 판정은 이번 실행이 GitHub ok일 때만 내린다.
    pub(crate) fn evaluate(
        &self,
        source: SourceName,
        context: &EventContext,
        off_dates: &OffDates,
    ) -> KairosResult<usize> {
        if !matches!(source, SourceName::Calendar | SourceName::Github) {
            return Ok(0);
        }
        let cursors = CursorRepo::new(self.connection);
        if !cursors.has_ok_run(SourceName::Calendar)? || !cursors.has_ok_run(SourceName::Github)? {
            return Ok(0);
        }
        let today = context.now.local_date();
        let recorder = EventRecorder::new(self.connection);
        let events = EventRepo::new(self.connection);
        let mut inserted = 0;
        for UpcomingDeploy { object, date, link } in upcoming_deploys(self.connection, context.now)? {
            let linked = link.status == DeployLinkStatus::Linked;
            if linked && let Some(stage) = DeployLinker::stage(date, today, off_dates) {
                let dedupe_key = stage_key(&events, &object, stage, date, off_dates)?;
                let event_type = match stage {
                    DeployStage::D0 => EventType::DeployRiskD0,
                    DeployStage::D1 => EventType::DeployRiskD1,
                };
                if recorder.record(&deploy_event(&object, dedupe_key, event_type, context.now), context)? {
                    inserted += 1;
                }
                continue;
            }
            if linked || source != SourceName::Github {
                continue;
            }
            if events.has_open_deploy_risk(&object.object_key)? {
                let dedupe_key = format!("deploy:{}:cleared:{}", object.external_id, context.run_id);
                let cleared = deploy_event(&object, dedupe_key, EventType::DeployCleared, context.now);
                if recorder.record(&cleared, context)? {
                    inserted += 1;
                }
            }
        }
        Ok(inserted)
    }
}
