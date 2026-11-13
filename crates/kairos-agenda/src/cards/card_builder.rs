use std::collections::{BTreeMap, BTreeSet};

use kairos_core::text::sanitize;
use kairos_core::{Instant, KairosResult};
use kairos_ledger::{EventHead, EventRepo, ObjectRepo, ObjectRow, upcoming_deploys};
use kairos_policy::{DeployLinkStatus, ItemReason, fold_lines};
use kairos_store::Connection;

use super::card_chooser::{Choice, tier_of_rows};
use super::playbook::{Playbook, label_of, playbook_for, playbook_of, reason_label, safe_url};
use super::views::{Card, CardAction, DeployView, ItemView, ReadyCard};
use crate::item::{Item, ItemState};
use crate::repo::{CardKind, ItemRepo, ReadOnlyRunRepo, ReadyKind, ReadyRow, RunView, SuggestionRepo};

const NO_TITLE: &str = "(제목 없음)";

fn shown_text(value: Option<&str>) -> String {
    let text = sanitize(value.unwrap_or(""));
    if text.is_empty() { NO_TITLE.to_owned() } else { text }
}

fn fail_headline(code: &str) -> &'static str {
    match code {
        "session_failed" => "세션이 시작하자마자 끝났습니다",
        "session_lost" => "세션이 뜨지 않았습니다",
        "claim_stale" => "세션을 여는 중에 멈췄습니다",
        "no_claude" => "세션을 열지 못했습니다. claude가 없습니다",
        "outsider_writing" => "외부 사람의 글이 있어 세션을 열지 않았습니다",
        "live_unverified" => "이슈의 글을 모두 확인하지 못해 세션을 열지 않았습니다",
        _ => "세션을 열지 못했습니다",
    }
}

const fn ready_headline(kind: ReadyKind) -> &'static str {
    match kind {
        ReadyKind::Question => "질문 대기",
        ReadyKind::Done => "준비됨",
        ReadyKind::Ended => "세션 끝남",
        ReadyKind::Attention => "세션 확인 필요",
        ReadyKind::Failed => "세션을 열지 못했습니다",
    }
}

/// 연결 건수는 호출 시점에 계산한다(설계 5절).
struct DeployIndex {
    linked: BTreeSet<String>,
    by_object: BTreeMap<String, DeployView>,
}

impl DeployIndex {
    fn load(connection: &Connection, now: Instant) -> KairosResult<Self> {
        let mut index = Self { linked: BTreeSet::new(), by_object: BTreeMap::new() };
        for deploy in upcoming_deploys(connection, now)? {
            if deploy.link.status == DeployLinkStatus::Linked {
                index.linked.extend(deploy.link.object_keys.iter().cloned());
            }
            let view = DeployView { date: deploy.date, linked_count: deploy.link.object_keys.len() };
            index.by_object.insert(deploy.object.object_key, view);
        }
        Ok(index)
    }
}

/// 활성 세션. 같은 이슈의 다른 항목이 연 세션도 낸다.
pub fn session_of(
    connection: &Connection,
    item_id: i64,
    state: ItemState,
    object_key: &str,
) -> KairosResult<Option<RunView>> {
    let runs = ReadOnlyRunRepo::new(connection);
    if let Some(found) = runs.of_item(item_id, state)? {
        return Ok(Some(found));
    }
    if state != ItemState::Open {
        return Ok(None);
    }
    Ok(runs.active_on_object(object_key)?.map(|run| RunView { id: run.id, state: run.state }))
}

/// 'PR 올리기'. 고치는 세션이 로컬에 커밋한 브랜치를 사용자가 누를 때만 올린다.
fn pr_action(id: &str) -> CardAction {
    CardAction::new("pr", "PR 올리기", &["pr", id, "--json"])
}

fn item_actions(
    row: &Item,
    startable: bool,
    deploy_linked: bool,
    run: Option<&RunView>,
    fix_run: bool,
) -> Vec<CardAction> {
    let id = row.id.to_string();
    let done = CardAction::new("done", "끝", &["done", &id, "--json"]);
    if row.state == ItemState::Closed {
        return Vec::new();
    }
    if row.state != ItemState::Open {
        // 끝난 세션의 항목을 끝 말고 되살릴 길이다. 진행 중인 세션에는 내지 않는다.
        let stuck = row.state == ItemState::Accepted
            && run.is_none_or(|run| run.state == "ended" || run.state == "needs_attention");
        let mut actions = Vec::new();
        if stuck {
            actions.push(CardAction::new("cancel", "되돌리기", &["accept", &id, "--cancel", "--json"]));
        }
        if fix_run && row.state == ItemState::Accepted {
            actions.push(pr_action(&id));
        }
        actions.push(done);
        return actions;
    }
    let mut actions = Vec::new();
    if startable {
        actions.push(CardAction::new("start", "시작", &["accept", &id, "--json"]));
    }
    actions.push(CardAction::new("later", "나중에", &["later", &id, "--json"]));
    actions.push(done);
    if matches!(row.reason, ItemReason::WorkOwed | ItemReason::Manual) {
        return actions;
    }
    actions.push(if deploy_linked {
        CardAction::new("keep", "배포 때 다시", &["keep", &id, "--until-deploy", "--json"])
    } else {
        CardAction::new("keep", "할일로 남김", &["keep", &id, "--json"])
    });
    actions
}

fn headline(choice: &Choice) -> String {
    let count = choice.rows.len();
    match (choice.kind, choice.rows.first()) {
        (CardKind::Bundle, _) => format!("묶음 {count}건"),
        (CardKind::Carryover, _) => format!("어제에서 넘어온 P1 {count}건"),
        _ if count > 1 => format!("같은 채널 {count}건"),
        (_, Some(row)) => format!("{} {}", row.tier, reason_label(row.reason)),
        (_, None) => String::new(),
    }
}

fn card_actions(choice: &Choice, views: &[ItemView]) -> Vec<CardAction> {
    if choice.kind != CardKind::Item {
        return vec![CardAction::new("ack", "확인", &["next", "--ack", "--json"])];
    }
    if choice.rows.len() == 1 {
        return views.first().map_or_else(Vec::new, |view| view.actions.clone());
    }
    let ids: Vec<String> = choice.rows.iter().map(|row| row.id.to_string()).collect();
    let argv = |command: &str| {
        let mut parts = vec![command];
        parts.extend(ids.iter().map(String::as_str));
        parts.push("--json");
        parts.into_iter().map(str::to_owned).collect()
    };
    vec![
        CardAction { id: "later".to_owned(), label: "나중에".to_owned(), argv: argv("later") },
        CardAction { id: "done".to_owned(), label: "끝".to_owned(), argv: argv("done") },
    ]
}

/// 항목 행을 패널의 뷰로 만든다. sessions는 차단 시험 잠금이 풀렸는가다.
pub struct CardBuilder<'a> {
    connection: &'a Connection,
    now: Instant,
    deploys: DeployIndex,
    sessions: bool,
}

impl<'a> CardBuilder<'a> {
    pub fn new(connection: &'a Connection, now: Instant, sessions: bool) -> KairosResult<Self> {
        Ok(Self { connection, now, deploys: DeployIndex::load(connection, now)?, sessions })
    }

    /// 제목과 링크는 항목에 복사하지 않고 원본 행에서 읽는다. 지우기가 한곳에서 끝난다.
    fn head_of(&self, row: &Item) -> KairosResult<Option<EventHead>> {
        let events = EventRepo::new(self.connection);
        let linked = events.heads_of_item(row.id)?;
        let first = linked
            .iter()
            .find(|event| event.rule_id.starts_with("p0."))
            .or_else(|| linked.iter().find(|event| event.rule_id.starts_with("p1.")))
            .or_else(|| linked.first());
        if let Some(first) = first {
            return Ok(Some(first.clone()));
        }
        match row.opened_by_event.as_deref() {
            Some(key) => events.head_by_key(key),
            None => Ok(None),
        }
    }

    /// 세션을 제안할 항목인가. 조건을 통과했고, 이 이슈에서 제안을 받을 항목이고, 세션이 아직 없을 때다.
    /// 잠금은 보지 않는다. 잠겨 있어도 제안은 보여야 사용자가 잠금을 풀 이유를 안다. 시작 버튼만 잠금을 따른다.
    fn offer_of(&self, row: &Item, object: Option<&ObjectRow>) -> KairosResult<Option<Playbook>> {
        let Some(playbook) = playbook_of(self.connection, row.reason, row.state, &row.object_key, object)? else {
            return Ok(None);
        };
        if ReadOnlyRunRepo::new(self.connection).active_on_object(&row.object_key)?.is_some() {
            return Ok(None);
        }
        // 한 이슈 한 버튼. 플레이북이 없는 항목이 먼저 열려 있어도 제안이 사라지지 않게 플레이북이 있는 첫 항목에 붙인다.
        let open = ItemRepo::new(self.connection).open_on_object(&row.object_key)?;
        let first = open.iter().find(|item| playbook_for(item.reason, item.state, object).is_some());
        Ok(first.is_some_and(|item| item.id == row.id).then_some(playbook))
    }

    pub fn view_item(&self, row: &Item) -> KairosResult<ItemView> {
        let object = ObjectRepo::new(self.connection).get(&row.object_key)?;
        let head = self.head_of(row)?;
        let playbook = self.offer_of(row, object.as_ref())?;
        let run = session_of(self.connection, row.id, row.state, &row.object_key)?;
        // 끝난 고치는 세션만. 일하는 세션은 확인과 push 사이에 브랜치를 바꿀 수 있다.
        let fix_run = match &run {
            Some(run) if run.state == "ended" => ReadOnlyRunRepo::new(self.connection).is_fix_run(&run.id)?,
            _ => false,
        };
        let head_text = head.as_ref().and_then(|head| head.text_head.as_deref());
        let object_text = object.as_ref().and_then(|object| object.text_head.as_deref());
        let head_link = head.as_ref().and_then(|head| head.permalink.as_deref());
        let object_link = object.as_ref().and_then(|object| object.permalink.as_deref());
        let linked = self.deploys.linked.contains(&row.object_key);
        Ok(ItemView {
            id: row.id,
            tier: row.tier,
            reason: row.reason,
            reason_label: reason_label(row.reason).to_owned(),
            state: row.state,
            source: row.source.clone(),
            source_label: label_of(row.source.as_deref()).to_owned(),
            title: shown_text(row.title.as_deref().or(head_text).or(object_text)),
            url: safe_url(head_link.or(object_link)),
            opened_at: row.opened_at.clone(),
            due_at: row.due_at.clone(),
            parked_until: row.parked_until.clone(),
            park_trigger: row.park_trigger,
            baseline: row.baseline,
            carried: row.is_carried(self.now),
            push_count: row.push_count,
            deploy: self.deploys.by_object.get(&row.object_key).cloned(),
            playbook,
            scope: playbook.map(|playbook| playbook.scope().to_owned()),
            actions: item_actions(row, playbook.is_some() && self.sessions, linked, run.as_ref(), fix_run),
            run,
            why: None,
        })
    }

    pub(super) fn build_card(&self, choice: &Choice, shown_at: Option<String>) -> KairosResult<Card> {
        let (shown, more) = if choice.kind == CardKind::Bundle {
            let folded = fold_lines(&choice.rows, choice.keep);
            (folded.shown, folded.more)
        } else {
            (choice.rows.clone(), 0)
        };
        let views = shown.iter().map(|row| self.view_item(row)).collect::<KairosResult<Vec<_>>>()?;
        Ok(Card {
            id: choice.base_id.clone(),
            kind: choice.kind,
            tier: tier_of_rows(&choice.rows),
            headline: headline(choice),
            more,
            item_ids: choice.rows.iter().map(|row| row.id).collect(),
            actions: card_actions(choice, &views),
            items: views,
            pending: shown_at.is_some(),
            shown_at,
        })
    }

    fn ready_card(&self, row: &ReadyRow) -> KairosResult<ReadyCard> {
        let view = self.view_item(&row.item)?;
        let shared = |kind, headline: Option<&str>, actions| ReadyCard {
            run_id: row.run_id.clone(),
            item_id: row.item.id,
            title: view.title.clone(),
            url: view.url.clone(),
            kind,
            headline: headline.map(str::to_owned),
            actions,
        };
        if let Some(code) = row.fail_code.as_deref() {
            return Ok(shared(Some(ReadyKind::Failed), Some(fail_headline(code)), Vec::new()));
        }
        let id = row.item.id.to_string();
        let mut actions = vec![CardAction::new("move", "이동", &["move", &id, "--json"])];
        if row.fixes && row.ready_kind == Some(ReadyKind::Ended) {
            actions.push(pr_action(&id));
        }
        Ok(shared(row.ready_kind, row.ready_kind.map(ready_headline), actions))
    }

    /// 세션 카드(설계 7.4). 패널이 그릴 때 --seen을 주면 다시 나오지 않는다.
    /// 항목이 먼저 닫혀도 세션이 살아 있으면 낸다. 게이트가 닫히면 실패 카드만 낸다. 사용자가 시작을 눌렀는데 아무 표시가 없으면 안 된다.
    pub(super) fn ready_cards(&self, seen: bool, gate_open: bool) -> KairosResult<Vec<ReadyCard>> {
        let rows = ReadOnlyRunRepo::new(self.connection).ready_rows(gate_open)?;
        let cards = rows.iter().map(|row| self.ready_card(row)).collect::<KairosResult<Vec<_>>>()?;
        if seen {
            let suggestions = SuggestionRepo::new(self.connection);
            let at = self.now.to_iso();
            for row in &rows {
                suggestions.mark_ready_seen(&row.run_id, &at)?;
            }
        }
        Ok(cards)
    }

    /// triage 줄. 같은 뷰에 유지와 버리기 버튼만 다르다.
    pub fn triage_view(&self, row: &Item) -> KairosResult<ItemView> {
        let id = row.id.to_string();
        Ok(ItemView {
            actions: vec![
                CardAction::new("triage_keep", "유지", &["triage", "keep", &id, "--json"]),
                CardAction::new("triage_drop", "버리기", &["triage", "drop", &id, "--json"]),
            ],
            ..self.view_item(row)?
        })
    }
}
