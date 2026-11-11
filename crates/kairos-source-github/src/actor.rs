use kairos_core::model::{ActorKind, TrustTier};
use serde_json::Value;

use crate::json::{text, word};
use crate::self_login;

const OUTSIDE_ASSOCIATIONS: [&str; 5] = ["NONE", "FIRST_TIMER", "FIRST_TIME_CONTRIBUTOR", "CONTRIBUTOR", "MANNEQUIN"];

pub(crate) fn is_me(login: Option<&str>) -> bool {
    login.is_some_and(|login| login.to_lowercase() == self_login())
}

/// 이벤트의 행위자. 작성자 객체와 글에 달린 `authorAssociation`을 합친 모양이다.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Actor<'a> {
    bot_type: bool,
    pub(crate) login: Option<&'a str>,
    association: Option<&'a Value>,
}

impl<'a> Actor<'a> {
    /// CI 이벤트의 행위자.
    pub(crate) const CHECKS: Self = Self { bot_type: true, login: Some("checks"), association: None };

    pub(crate) fn of(value: Option<&'a Value>) -> Self {
        let field = |key| value.and_then(|actor| actor.get(key));
        Self {
            bot_type: field("__typename").and_then(Value::as_str) == Some("Bot"),
            login: text(field("login")),
            association: field("authorAssociation"),
        }
    }

    #[must_use]
    pub(crate) fn with_association(self, association: Option<&'a Value>) -> Self {
        Self { association, ..self }
    }

    /// 설계 3.3: Bot 타입이거나 login이 [bot]으로 끝나면 bot, 본인이면 self, 나머지는 human.
    pub(crate) fn kind(&self) -> ActorKind {
        let lower = self.login.map(str::to_lowercase);
        if self.bot_type || lower.as_deref().is_some_and(|login| login.ends_with("[bot]")) {
            ActorKind::Bot
        } else if lower.as_deref() == Some(self_login()) {
            ActorKind::SelfActor
        } else {
            ActorKind::Human
        }
    }

    pub(crate) fn trust(&self) -> TrustTier {
        match self.kind() {
            ActorKind::SelfActor => TrustTier::T0,
            ActorKind::Bot => TrustTier::T2,
            // 검색이 프로필의 조직(org:<조직>)으로 고정이라 연관 정보가 없으면 조직의 사람이다.
            ActorKind::Human if word(self.association).is_some_and(|word| OUTSIDE_ASSOCIATIONS.contains(&word)) => {
                TrustTier::T3
            }
            ActorKind::Human => TrustTier::T1,
        }
    }
}
