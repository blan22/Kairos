//! 메일 본문 해석.
#![allow(clippy::unwrap_used)]

mod common;
mod notion_support;

use common::*;
use kairos_source_mcp::notion::{BlockKind, parse_mail};
use notion_support::*;
use unicode_normalization::UnicodeNormalization;

/// 시험 프로필(`Profile::fixture`)의 본인 이름.
const SELF_NAME: &str = "김준서";

/// 원본의 상한은 500ms다. 시험 빌드는 의존성까지 최적화하지 않아 정규화와 정규식이 수십 배 느리다.
/// 그 배율로도 줄 길이의 제곱으로 늘어나는 동작은 잡힌다. `--release`에서는 원본의 상한을 지킨다.
const TIME_LIMIT_MS: u128 = if cfg!(debug_assertions) { 10_000 } else { 500 };

fn titles_and_kinds(subject: &str, body: &str) -> Vec<(Option<String>, BlockKind)> {
    parse_mail(subject, body).into_iter().map(|block| (block.title, block.kind)).collect()
}

fn kinds(subject: &str, body: &str) -> Vec<BlockKind> {
    parse_mail(subject, body).into_iter().map(|block| block.kind).collect()
}

fn title(text: &str) -> Option<String> {
    Some(text.to_owned())
}

#[test]
fn 메일_하나를_페이지_블록으로_나눈다() {
    let parsed: Vec<_> = blocks().into_iter().map(|block| (block.title, block.kind)).collect();
    assert_eq!(
        parsed,
        [
            (title("합성 페이지 제목 A"), BlockKind::Comment),
            (title("합성 페이지 제목 B"), BlockKind::Edit),
            (title("합성 페이지 제목 C"), BlockKind::Mention),
            (title("합성 페이지 제목 D"), BlockKind::Comment),
            (title("합성 페이지 제목 E"), BlockKind::Comment),
            (title("합성 페이지 제목 F"), BlockKind::Edit),
            (title("합성 페이지 제목 G"), BlockKind::Mention),
            (None, BlockKind::Unknown),
        ]
    );
}

#[test]
fn 아이콘_칸이_비어_있거나_이모지여도_칸_수가_달라도_제목을_찾는다() {
    for head in ["[| 📝 | 합성 제목 |]", "[| | 합성 제목 |]", "[| || | 합성 제목 |]", "[| || 📝 | 합성 제목 |]"]
    {
        let body = format!("{head}(https://mg.mail.notion.com/c/x)\n| | 홍길동 이 페이지 페이지를 편집했습니다. |");
        assert_eq!(parse_mail(DIGEST_SUBJECT, &body)[0].title, title("합성 제목"), "{head}");
    }
}

#[test]
fn 미리보기와_추천_목록은_블록도_제목도_아니다() {
    let summary = titles_and_kinds(DIGEST_SUBJECT, &fixture("notion/summary.txt"));
    assert_eq!(summary, [(title("합성 페이지 제목 S"), BlockKind::Edit)]);
    assert!(!blocks().iter().any(|block| block.title.as_deref().is_some_and(|title| title.contains("미리보기"))));
}

#[test]
fn 동작이_하나_이상이면_강한_순서로_종류를_정한다() {
    assert_eq!(blocks()[6].kind, BlockKind::Mention);
    assert_eq!(blocks()[6].actors, ["홍길동", "김철수"]);
}

fn names_me(content: &str, actor: &str) -> bool {
    let body = format!(
        "[| | 합성 제목 |](https://mg.mail.notion.com/c/x)\n| | {actor} 이 페이지 페이지를 편집했습니다.[](https://mg.mail.notion.com/c/y) |\n| | {content}[](https://mg.mail.notion.com/c/z) |"
    );
    parse_mail(DIGEST_SUBJECT, &body)[0].mentions_me
}

#[test]
fn 이름은_내용_줄에서만_찾고_앞_글자가_한글이면_일치가_아니다() {
    let actor = "합성행위자";
    assert!(names_me("* 준서님 확인 예정", actor));
    assert!(names_me(&format!("* {SELF_NAME} 담당"), actor));
    assert!(names_me("* 담당: 준서", actor));
    assert!(!names_me("* 기준서 개정", actor));
    assert!(!names_me("* 표준서 초안", actor));
    assert!(!names_me("* 합성 내용만 있음", actor));
    assert!(
        !names_me(&format!("{SELF_NAME} 내가 쓴 글"), &format!("홍길동, {SELF_NAME}")),
        "글쓴이 머리는 이름 일치가 아니다"
    );
    assert!(names_me("홍길동준서님 확인 부탁", "홍길동"), "붙어 있는 글쓴이 이름이 판정을 막으면 안 된다");
}

#[test]
fn 행위자_줄과_제목의_내_이름은_세지_않는다() {
    let body = format!(
        "[| | 준서 합성 제목 |](https://mg.mail.notion.com/c/x)\n| | 홍길동, {SELF_NAME} 이 페이지 페이지를 편집했습니다. |"
    );
    let parsed = parse_mail(DIGEST_SUBJECT, &body);
    assert_eq!(parsed.len(), 1);
    assert_eq!((parsed[0].kind, parsed[0].mentions_me), (BlockKind::Edit, false));
}

#[test]
fn 멘션_동작은_항상_mentions_me이고_내용의_이름은_편집과_댓글에도_붙는다() {
    let flags: Vec<bool> = blocks().iter().map(|block| block.mentions_me).collect();
    assert_eq!(flags, [false, true, true, false, false, false, true, false]);
}

#[test]
fn 머리를_못_찾은_동작_줄도_버리지_않는다() {
    let parsed = titles_and_kinds(
        DIGEST_SUBJECT,
        "| | 홍길동 이 페이지페이지에서 나를 멘션했습니다.[](https://mg.mail.notion.com/c/x) |",
    );
    assert_eq!(parsed, [(None, BlockKind::Mention)]);
}

#[test]
fn 머리_형태인데_동작이_없는_블록은_unknown이고_다음_블록을_삼키지_않는다() {
    let body = [
        "[| | 합성 제목 1 |](https://mg.mail.notion.com/c/x)",
        "| | 합성 내용 줄 |",
        "[| | 합성 제목 2 |](https://mg.mail.notion.com/c/y)",
        "| | 홍길동 이 페이지 페이지를 편집했습니다. |",
    ]
    .join("\n");
    let parsed = titles_and_kinds(DIGEST_SUBJECT, &body);
    assert_eq!(parsed, [(None, BlockKind::Unknown), (title("합성 제목 2"), BlockKind::Edit)]);
}

#[test]
fn 본문에_바닥글_낱말이_있어도_뒤_블록을_잃지_않는다() {
    let body = [
        "[| | 합성 제목 1 |](https://mg.mail.notion.com/c/x)",
        "| | 홍길동 이 페이지 페이지를 편집했습니다. |",
        "Notion",
        "[| | 합성 제목 2 |](https://mg.mail.notion.com/c/y)",
        "| | 홍길동 이 페이지페이지에서 나를 멘션했습니다. |",
        "Notion",
        "[Notion.com](https://mg.mail.notion.com/c/z) - 합성",
    ]
    .join("\n");
    assert_eq!(kinds(DIGEST_SUBJECT, &body), [BlockKind::Edit, BlockKind::Mention]);
}

#[test]
fn 내용_칸이_멘션_문구를_흉내_내면_태그로_보되_그_줄의_이름을_행위자로_쓰지_않는다_놓침이_오탐보다_무겁다() {
    let body = [
        "[| | 합성 제목 |](https://mg.mail.notion.com/c/x)",
        "| | 홍길동 이 페이지 페이지를 편집했습니다. |",
        "| | 앞 칸 | 김철수 이 페이지페이지에서 나를 멘션했습니다. |",
    ]
    .join("\n");
    let parsed = parse_mail(DIGEST_SUBJECT, &body);
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].kind, BlockKind::Mention);
    assert_eq!(parsed[0].actors, ["홍길동"]);
}

#[test]
fn 이름이_nfd로_와도_찾는다() {
    let body = "[| | 합성 제목 |](https://mg.mail.notion.com/c/x)\n| | 홍길동 이 페이지 페이지를 편집했습니다. |\n| | 준서님 확인 |";
    let decomposed: String = body.nfd().collect();
    assert!(parse_mail(DIGEST_SUBJECT, &decomposed)[0].mentions_me);
}

#[test]
fn 새_문구의_머리는_앞_블록의_동작이나_미리보기_뒤에_와도_합쳐지지_않는다() {
    let added = "김합성 이 페이지 블록에서 나를 멘션했습니다.";
    let before_cases = [vec![preview()], vec![], vec!["| | 합성 내용 |".to_owned()]];
    for before in before_cases {
        let mut lines = vec![head("합성 A"), row(EDIT)];
        lines.extend(before.clone());
        lines.extend([head("합성 B"), row(added), row("합성 내용")]);
        assert_eq!(kinds(DIGEST_SUBJECT, &lines.join("\n")), [BlockKind::Edit, BlockKind::Mention], "{before:?}");
    }
}

#[test]
fn 부제_칸이_있는_머리_뒤의_새_문구도_블록이_사라지지_않고_부제는_제목이_아니다() {
    let listing = format!("[| || | 합성 제목 || | 합성 부제 저장금지 |]({LINK})");
    let body = [listing, row("김합성 이 페이지 블록에서 나를 멘션했습니다.")].join("\n");
    assert_eq!(titles_and_kinds(DIGEST_SUBJECT, &body), [(title("합성 제목"), BlockKind::Mention)]);
}

#[test]
fn 문구가_조금_달라도_태그는_멘션이다() {
    for phrase in [
        "김합성 이 페이지 블록에서 나를 멘션했습니다.",
        "김합성 이 페이지 댓글에서 나를 멘션했습니다.",
        "김합성 이 페이지블록에서 나를 멘션했습니다.",
        "김합성님이 페이지에서 나를 멘션했습니다.",
        "김합성 이 페이지에서 나를 언급했습니다.",
        "김합성 이 페이지에서 회원님을 멘션했습니다.",
    ] {
        let parsed = parse_mail(DIGEST_SUBJECT, &[head("합성 제목"), row(phrase)].join("\n"));
        assert_eq!(parsed[0].kind, BlockKind::Mention, "{phrase}");
        assert!(parsed[0].mentions_me, "{phrase}");
        assert_eq!(parsed[0].title, title("합성 제목"), "{phrase}");
        assert_eq!(parsed[0].actors, ["김합성"], "{phrase}");
    }
}

#[test]
fn 댓글_문구는_블록과_페이지_둘_다_받는다() {
    let body = [head("합성 제목"), row("김합성 이 페이지 페이지에서 댓글을 남겼습니다.")].join("\n");
    let parsed = parse_mail(DIGEST_SUBJECT, &body);
    assert_eq!(parsed[0].kind, BlockKind::Comment);
    assert_eq!(parsed[0].actors, ["김합성"]);
}

#[test]
fn 머리말의_페이지_수가_맞지_않으면_미리보기_뒤의_머리는_제목을_믿지_않는다() {
    let lines = |stated: &[&str]| {
        let mut lines: Vec<String> = stated.iter().map(|line| (*line).to_owned()).collect();
        lines.extend([
            head("합성 A"),
            row(EDIT),
            preview(),
            head("금지본문 미리보기 연속"),
            row("김합성 이 페이지페이지에서 나를 멘션했습니다."),
        ]);
        lines.join("\n")
    };
    let titles = |body: String| -> Vec<Option<String>> {
        parse_mail(DIGEST_SUBJECT, &body).into_iter().map(|block| block.title).collect()
    };
    assert_eq!(titles(lines(&[])), [title("합성 A"), None]);
    assert_eq!(titles(lines(&["내 워크스페이스에서 1페이지의 변경 사항을 검토합니다."])), [title("합성 A"), None]);
    assert_eq!(
        titles(lines(&["내 워크스페이스에서 2페이지의 변경 사항을 검토합니다."])),
        [title("합성 A"), title("금지본문 미리보기 연속")]
    );
}

#[test]
fn 동작_문구가_바뀌어도_뒤따르는_미리보기가_줄마다_블록이나_제목이_되지_않는다() {
    let body = [head("합성 제목"), row("김합성 이 페이지를 공유했습니다."), head("금지본문-D1"), head("금지본문-D2")]
        .join("\n");
    assert_eq!(titles_and_kinds(DIGEST_SUBJECT, &body), [(None, BlockKind::Unknown)]);
}

#[test]
fn 제목의_표시_이름이_홍보를_흉내_내도_본문의_멘션을_잃지_않는다() {
    let body = [head("합성 제목"), row("김합성 이 페이지페이지에서 나를 멘션했습니다.")].join("\n");
    for subject in
        ["합성의 인기 페이지: 공지이 Contoso에서 업데이트함", "Contoso의 인기 페이지: 합성", "추천 페이지 - 합성: 공지"]
    {
        assert_eq!(kinds(subject, &body), [BlockKind::Mention], "{subject}");
    }
    assert_eq!(
        kinds("다른워크의 인기 페이지: 합성", "합성 본문"),
        [BlockKind::Unknown],
        "홍보로 고정하지 않은 제목은 숨기지 않는다"
    );
}

#[test]
fn 추적_링크는_스킴이_없어도_제목에_남지_않는다() {
    let first = |subject: &str, body: &str| parse_mail(subject, body).remove(0).title;
    assert_eq!(
        first("추천 페이지 - Contoso: 합성 https://mg.mail.notion.com/c/synthetic-token", "합성 본문"),
        title("합성")
    );
    assert_eq!(first("Contoso의 인기 페이지: 합성 mg.mail.notion.com/c/synthetic-token", "합성 본문"), title("합성"));
    let body = [head("합성 mg.mail.notion.com/c/synthetic-token"), row(EDIT)].join("\n");
    assert_eq!(first(DIGEST_SUBJECT, &body), title("합성"));
}

#[test]
fn 공백이_많은_줄이_파서를_붙잡지_않는다() {
    for space in [" ", "\u{a0}", "\u{3000}"] {
        let spaces = space.repeat(200_000);
        let lines = [
            head("합성 제목"),
            row(EDIT),
            format!("| | {spaces}x"),
            format!("| | 합성이름{spaces}이 페이지 페이지를 편집했습니다. |"),
            format!("| {spaces}"),
        ];
        let body = lines.join("\n");
        let started = std::time::Instant::now();
        parse_mail(DIGEST_SUBJECT, &body);
        assert!(started.elapsed().as_millis() < TIME_LIMIT_MS, "{space:?}: {:?}", started.elapsed());
    }
}

#[test]
fn 행위자_묶음이_있어도_내용의_내_이름을_찾고_이름_앞부분이_같은_행위자가_내_이름을_먹지_않는다() {
    let names_me = |actor: &str, content: &str| {
        let lines = [
            head("합성 제목"),
            row(&format!("{actor} 이 페이지 페이지를 편집했습니다.")),
            format!("[{content}]({LINK})"),
        ];
        parse_mail(DIGEST_SUBJECT, &lines.join("\n"))[0].mentions_me
    };
    assert!(names_me("김합성 외 2명", "이합성준서님 확인 부탁"));
    assert!(!names_me("김합성 외 2명", "김합성 기준서 개정"));
    assert!(names_me("김합성 외 2명님", "이합성준서님 확인 부탁"));
    assert!(names_me("박준", &format!("{SELF_NAME}님 확인 부탁")));
    assert!(!names_me("홍길동", "홍길동 기준서 개정"));
}

#[test]
fn 미리보기는_다음_줄이_동작이어도_머리가_아니다_제목이_틀리는_것보다_본문_저장이_무겁다() {
    let body = [
        head("합성 A"),
        row("홍길동 이 페이지페이지에서 나를 멘션했습니다."),
        preview(),
        row("김철수 이 페이지 블록에서 댓글을 남겼습니다."),
    ]
    .join("\n");
    let parsed = parse_mail(DIGEST_SUBJECT, &body);
    assert_eq!(parsed.len(), 1);
    assert_eq!((parsed[0].title.clone(), parsed[0].kind), (title("합성 A"), BlockKind::Mention));
    assert_eq!(parsed[0].actors, ["홍길동", "김철수"]);
}

#[test]
fn 한_줄의_댓글_경계를_지켜_내가_쓴_뒤_댓글을_나를_부른_것으로_세지_않는다() {
    let body = [
        head("합성 제목"),
        row(&format!("이합성, {SELF_NAME}님 이 페이지 블록에서 댓글을 남겼습니다.")),
        format!("| | 이합성 합성 댓글[]({LINK}) {SELF_NAME} 내가 쓴 답글[]({LINK}) |"),
    ]
    .join("\n");
    assert!(!parse_mail(DIGEST_SUBJECT, &body)[0].mentions_me);
}

// 아래는 원본에 없는 시험이다.

#[test]
fn 아주_긴_내용_줄의_끝에_있는_이름도_찾는다() {
    // fancy-regex의 기본 되짚기 상한은 이 길이에서 오류를 낸다(측정).
    let content = format!("{}준서님", "ㄱ ".repeat(300_000));
    let body = [head("합성 제목"), row("김합성 이 페이지 페이지를 편집했습니다."), content].join("\n");
    assert!(parse_mail(DIGEST_SUBJECT, &body)[0].mentions_me, "되짚기 상한에 걸려 놓치면 안 된다");
}

#[test]
fn 행위자_칸_제한은_utf16_단위다() {
    let phrase = |name: &str| [head("합성 제목"), row(&format!("{name} 이 페이지 페이지를 편집했습니다."))].join("\n");
    // 쉼표로 나뉘어 이름마다는 짧지만 칸 전체는 240단위를 넘는다.
    let names = |unit: &str, count: usize| vec![unit.repeat(10); count].join(",");
    assert_eq!(kinds(DIGEST_SUBJECT, &phrase(&names("가", 20))), [BlockKind::Edit], "219단위");
    assert_eq!(kinds(DIGEST_SUBJECT, &phrase(&names("😀", 12))), [BlockKind::Unknown], "251단위, 131글자");
    assert_eq!(kinds(DIGEST_SUBJECT, &phrase(&names("😀", 11))), [BlockKind::Edit], "230단위");
}

#[test]
fn 링크_주소_제한은_utf16_단위다() {
    let body = |content: String| [head("합성 제목"), row(EDIT), content].join("\n");
    // 지워지지 않은 링크는 내용으로 남는다. 주소 안의 이름으로 가른다.
    let inside = |unit: &str, count: usize| format!("| | [합성](https://x.test/준서{}) |", unit.repeat(count));
    assert!(!parse_mail(DIGEST_SUBJECT, &body(inside("a", 2000)))[0].mentions_me, "2048단위 안의 링크는 지운다");
    assert!(parse_mail(DIGEST_SUBJECT, &body(inside("😀", 1100)))[0].mentions_me, "2200단위 링크는 남는다");
}
