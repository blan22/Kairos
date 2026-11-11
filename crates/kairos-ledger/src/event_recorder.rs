use std::collections::BTreeSet;

use kairos_core::model::{ActorKind, EventType, NormalizedEvent, SourceName, TrustTier};
use kairos_core::{Instant, KairosError, KairosResult};
use kairos_policy::{Classifier, ClassifyInput, RuleEvent};
use kairos_store::Connection;

use crate::repo::{EventRepo, NewEvent, ObjectRepo, OutsiderMarkRepo};

const CATCHUP_AGE_MS: i64 = 30 * 60 * 1000;
pub const EVENT_RETENTION_DAYS: i64 = 30;
pub(crate) const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const EVENT_RETENTION_MS: i64 = EVENT_RETENTION_DAYS * DAY_MS;

/// 이벤트를 저장할 실행의 맥락.
pub(crate) struct EventContext {
    pub run_id: i64,
    pub now: Instant,
    /// 이 질의가 저장하는 첫 실행이다. 이때 본 것은 모두 시작 전의 것이다.
    pub baseline_run: bool,
    /// 첫 ok 전이다. 일정 변화는 시각이 now라 사건 시각으로 나눌 수 없다.
    pub before_ok: bool,
    /// 이 시각 이전의 사건이 baseline이다. 해석할 수 없는 저장값이면 None이고 어떤 사건도 이전이 아니다.
    pub t0: Option<Instant>,
    pub deploy_linked: BTreeSet<String>,
}

/// 세션이 읽는 GitHub 글 중 T0, T1이 아닌 사람의 글. 등급이 없으면 모르는 것이라 믿지 않는다.
/// 봇 글은 세지 않는다. `playbook_of`와 같은 기준이다.
fn is_outsider_writing(event: &NormalizedEvent) -> bool {
    event.source == SourceName::Github
        && event.actor_kind == ActorKind::Human
        && !matches!(event.trust_tier, Some(TrustTier::T0 | TrustTier::T1))
}

/// 이벤트 삽입과 분류. 소스 이벤트와 Ledger가 만든 이벤트가 이 경로 하나로 저장된다.
pub(crate) struct EventRecorder<'a> {
    events: EventRepo<'a>,
    objects: ObjectRepo<'a>,
    outsiders: OutsiderMarkRepo<'a>,
}

impl<'a> EventRecorder<'a> {
    pub(crate) const fn new(connection: &'a Connection) -> Self {
        Self {
            events: EventRepo::new(connection),
            objects: ObjectRepo::new(connection),
            outsiders: OutsiderMarkRepo::new(connection),
        }
    }

    /// 새로 저장했으면 참이다. 거짓은 이미 있거나 보존 기간이 지난 사건이다.
    pub(crate) fn record(&self, event: &NormalizedEvent, context: &EventContext) -> KairosResult<bool> {
        // 세션 게이트의 증거는 보존 기한을 보기 전에 남긴다. 30일이 지난 외부 글도 세션은 읽는다.
        if let Some(object_key) = event.object_key.as_deref().filter(|_| is_outsider_writing(event)) {
            self.outsiders.mark(object_key, &context.now.to_iso())?;
        }
        let occurred = Instant::parse(&event.occurred_at);
        // GitHub는 객체의 최근 댓글을 틱마다 다시 읽는다. 보존 기간이 지난 사건을 받으면
        // 정비가 지운 이벤트가 새 이벤트로 되살아나고, 답한 댓글이 다시 열린다.
        if occurred.is_some_and(|at| context.now.ms_since(at) > EVENT_RETENTION_MS) {
            return Ok(false);
        }
        if let Some(stored) = self.events.stored_type(&event.dedupe_key)? {
            // 형식을 몰라 저장한 블록만 다시 판정한다. 파서를 고친 뒤 다시 읽어도 나머지 블록은 그대로다.
            if stored != EventType::NotionUnknown.as_str() || event.event_type == EventType::NotionUnknown {
                return Ok(false);
            }
            self.events.delete(&event.dedupe_key)?;
        }

        let occurred = occurred.ok_or_else(|| KairosError::new("invalid_time"))?;
        let object_key = event.object_key.as_deref();
        let row = match object_key {
            Some(key) => self.objects.get(key)?,
            None => None,
        };
        let open_rule_ids = match object_key {
            Some(key) => self.events.open_rule_ids(key)?,
            None => Vec::new(),
        };
        let object = row.as_ref().map(crate::ObjectRow::to_rule_object);
        let rule_event = RuleEvent {
            event_type: event.event_type,
            actor_kind: event.actor_kind,
            actor_id: event.actor_id.clone(),
            mentions_me: event.mentions_me == Some(true),
            mention_count: event.mention_count.unwrap_or(0),
            text_head: event.text_head.clone(),
        };
        let judged = Classifier::classify(&ClassifyInput {
            event: &rule_event,
            object: object.as_ref(),
            now: context.now,
            deploy_linked: object_key.is_some_and(|key| context.deploy_linked.contains(key)),
            open_rule_ids: &open_rule_ids,
        });

        // 배포 위험은 baseline보다 우선한다(설계 4.3).
        let is_deploy_risk = matches!(event.event_type, EventType::DeployRiskD0 | EventType::DeployRiskD1);
        let is_calendar = event.event_type.as_str().starts_with("cal_");
        let baseline = !is_deploy_risk
            && (context.baseline_run
                || (context.before_ok && is_calendar)
                || context.t0.is_some_and(|t0| occurred < t0));
        let catchup = !baseline && !is_calendar && context.now.ms_since(occurred) >= CATCHUP_AGE_MS;

        self.events.insert(&NewEvent {
            dedupe_key: &event.dedupe_key,
            source: event.source,
            object_key,
            thread_key: event.thread_key.as_deref().or(object_key).unwrap_or(&event.dedupe_key),
            event_type: event.event_type,
            actor_id: event.actor_id.as_deref(),
            actor_kind: event.actor_kind,
            mentions_me: rule_event.mentions_me,
            mention_count: rule_event.mention_count,
            occurred_at: &occurred.to_iso(),
            first_seen_at: &context.now.to_iso(),
            run_id: context.run_id,
            catchup,
            baseline,
            channel_id: event.channel_id.as_deref(),
            permalink: event.permalink.as_deref(),
            text_head: event.text_head.as_deref(),
            trust_tier: event.trust_tier,
            disposition: judged.disposition,
            rule_id: judged.rule_id,
        })?;
        Ok(true)
    }
}
