/// 닫힌 문자열 코드를 enum으로 만든다. DB와 JSON에는 TS 시절과 같은 문자열로 나간다.
#[macro_export]
macro_rules! string_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($(#[$variant_meta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, ::serde::Serialize, ::serde::Deserialize)]
        $vis enum $name {
            $($(#[$variant_meta])* #[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::KairosError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                match text {
                    $($text => Ok(Self::$variant),)+
                    _ => Err($crate::KairosError::new(concat!("invalid_", stringify!($name)))),
                }
            }
        }
    };
}

string_enum! {
    /// 소스 이름. 순서가 수집과 표시의 순서다.
    pub enum SourceName {
        Calendar => "calendar",
        Github => "github",
        Slack => "slack",
        Gmail => "gmail",
        Notion => "notion",
    }
}

impl SourceName {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Calendar => "Calendar",
            Self::Github => "GitHub",
            Self::Slack => "Slack",
            Self::Gmail => "Gmail",
            Self::Notion => "Notion",
        }
    }

    /// 신원을 단언하는 소스인가. Gmail과 Notion은 응답에 신원이 없다.
    pub const fn asserts_identity(self) -> bool {
        matches!(self, Self::Github | Self::Calendar | Self::Slack)
    }

    /// 설계 9.2의 신원 단언 기대값. 프로필의 본인 식별자다. 단언하지 않는 소스와 설정하지 않은 값은 None이다.
    pub fn self_identity(self) -> Option<&'static str> {
        let identity = &crate::profile::current().identity;
        match self {
            Self::Github => identity.github_login.as_deref(),
            Self::Calendar => identity.google_email.as_deref(),
            Self::Slack => identity.slack_user.as_deref(),
            Self::Gmail | Self::Notion => None,
        }
    }
}

string_enum! {
    /// 질의 1회의 결과.
    pub enum Outcome {
        /// 모든 페이지를 읽고 검증까지 통과했다. 0건도 정상이면 ok다.
        Ok => "ok",
        /// 일부만 읽었다. 읽은 만큼 저장하지만 watermark와 종료 판정은 하지 않는다.
        Partial => "partial",
        /// 도구나 실행 파일이 없다.
        ToolMissing => "tool_missing",
        /// 인증 실패, 로그인 만료.
        Auth => "auth",
        /// 한도 초과.
        RateLimited => "rate_limited",
        /// 응답을 해석하지 못했다. 건수 불일치 포함.
        ParseError => "parse_error",
        /// 시간 상한 초과.
        Timeout => "timeout",
        /// 응답 형식, 신원, 허용 필드 계약이 깨졌다.
        ContractBreak => "contract_break",
        /// 연결이 아직 없다. 실패로 숨기지 않고 그대로 드러낸다.
        NotConnected => "not_connected",
    }
}

impl Outcome {
    /// ok나 partial이 아닌 결과는 objects와 events를 비워서 낸다. Ledger도 저장하지 않는다.
    pub const fn stores_records(self) -> bool {
        matches!(self, Self::Ok | Self::Partial)
    }
}

string_enum! {
    /// T0 본인, T1 조직의 사람, T2 봇과 연동, T3 외부 (설계 8절).
    pub enum TrustTier {
        T0 => "T0",
        T1 => "T1",
        T2 => "T2",
        T3 => "T3",
    }
}

string_enum! {
    /// 설계 3.3: user.type이 Bot이거나 login이 [bot]으로 끝나면 bot, 본인이면 self, 나머지는 human.
    pub enum ActorKind {
        Human => "human",
        Bot => "bot",
        SelfActor => "self",
    }
}

string_enum! {
    pub enum ObjectKind {
        /// 개인 캘린더 일정
        Meeting => "meeting",
        /// 500. 배포 캘린더의 [배포] 일정
        Deploy => "deploy",
        /// 공휴일과 연차 일정. 종일 일정이고 근무일 판정에 쓴다.
        DayOff => "day_off",
        Issue => "issue",
        PullRequest => "pull_request",
        /// Slack to:me 기록. 대화 ID, 마지막 발화자, 시각만 둔다. text_head는 비운다.
        DmConversation => "dm_conversation",
        /// Gmail 스레드
        MailThread => "mail_thread",
        /// Notion 알림 메일 스레드. 스레드 ID, 시각, 링크만 둔다. text_head는 비운다.
        NotionMail => "notion_mail",
    }
}

string_enum! {
    /// 이벤트 종류. 앞은 소스가 내는 것, 뒤는 Ledger가 만드는 것이다.
    pub enum EventType {
        /// 사람, 봇, 본인의 활동을 모두 낸다. 판별은 actor_kind로 한다.
        GhComment => "gh_comment",
        GhReview => "gh_review",
        /// 변경 요청 리뷰. gh_review와 별개로 낸다.
        GhChangesRequested => "gh_changes_requested",
        /// 승인 리뷰. gh_review와 별개로 낸다. 같은 리뷰어의 변경 요청을 닫는 증거다.
        GhReviewApproved => "gh_review_approved",
        /// 나에게 처음 온 리뷰 요청
        GhReviewRequested => "gh_review_requested",
        /// 내가 리뷰한 뒤 다시 온 요청
        GhReviewRerequested => "gh_review_rerequested",
        GhReviewRequestRemoved => "gh_review_request_removed",
        GhAssigned => "gh_assigned",
        GhUnassigned => "gh_unassigned",
        /// 체크 상태 전이. 내 PR 여부는 객체의 authored_by_me로 판단한다.
        GhCiFailed => "gh_ci_failed",
        GhCiRecovered => "gh_ci_recovered",
        GhClosed => "gh_closed",
        GhMerged => "gh_merged",
        /// <@me>가 본문에 확인된 메시지
        SlackMention => "slack_mention",
        /// 본인 메시지(from:me). 종료 증거로 쓰인다.
        SlackMessage => "slack_message",
        /// to:me 기록 전용
        SlackDmSeen => "slack_dm_seen",
        /// actor_id는 발신자 주소, text_head는 정제한 제목
        MailThread => "mail_thread",
        /// Notion 알림 메일의 페이지 블록. actor_id는 행위자, text_head는 페이지 제목
        NotionMention => "notion_mention",
        NotionComment => "notion_comment",
        NotionEdit => "notion_edit",
        /// 인기 페이지와 추천 페이지 메일. text_head는 제목의 페이지 이름만 쓴다
        NotionPromo => "notion_promo",
        /// 동작 문구를 찾지 못한 메일이나 블록. 형식 변경의 신호다. 제목이 본문일 수 있어 text_head는 비운다
        NotionUnknown => "notion_unknown",
        CalCreated => "cal_created",
        CalMoved => "cal_moved",
        CalUpdated => "cal_updated",
        CalCancelled => "cal_cancelled",
        ObjectWithdrawn => "object_withdrawn",
        DeployRiskD1 => "deploy_risk_d1",
        DeployRiskD0 => "deploy_risk_d0",
        DeployCleared => "deploy_cleared",
        GhChangesCleared => "gh_changes_cleared",
        ItemChanged => "item_changed",
    }
}

impl EventType {
    /// Ledger가 만드는 종류인가. 소스는 이 종류를 낼 수 없다.
    pub const fn is_derived(self) -> bool {
        matches!(
            self,
            Self::CalCreated
                | Self::CalMoved
                | Self::CalUpdated
                | Self::CalCancelled
                | Self::ObjectWithdrawn
                | Self::DeployRiskD1
                | Self::DeployRiskD0
                | Self::DeployCleared
                | Self::GhChangesCleared
                | Self::ItemChanged
        )
    }
}

/// Notion 알림 메일의 발신자. 이 메일은 notion 소스가 블록 단위로 처리한다.
pub const NOTION_SENDER: &str = "notify@mail.notion.com";
