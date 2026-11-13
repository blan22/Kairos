// orca plugin check가 상태마다 패널을 그린다. 워커는 점검 실행이라 실제 kairos를 부르지 않는다.
// 점검은 버튼을 누를 수 없다. 처리 중, 처리 실패, 펼친 행, 지금 말고 다른 탭, 검색은 panel.test.tsx가 맡는다.
// 스토리북에서는 탭을 눌러 볼 수 있어 몇 시나리오에 소스별 수집 상태를 실어 둔다.
import { defineCheck, hostError } from 'orca/testing'
import {
  accepted,
  bundle,
  card,
  carryover,
  counts,
  DANGEROUS_TITLE,
  failedCard,
  item,
  listJson,
  LONG_TITLE,
  manual,
  manyItems,
  merged,
  nextJson,
  offered,
  OFFER_WHY,
  parked,
  providerRows,
  readyCard,
  recordingGate,
  revivable,
  sessionNews,
  SLACK_TITLE,
  snapshotOf,
  sourceRows,
  startable,
  why,
  workdayRows,
} from './fixtures.ts'
import type { Snapshot } from './protocol.ts'

const answers = (snapshot: Snapshot) => ({
  requests: {
    'kairos.snapshot': () => snapshot,
    'kairos.action': () => ({ ok: true, code: null, message: '처리했습니다', run: null, snapshot }),
  },
})

const danger = item(2, { title: DANGEROUS_TITLE })
const until = '2026-11-04T05:00:00.000Z'
const held = (reason: string, rows: Record<string, unknown>[]) =>
  answers(snapshotOf(nextJson({ hold: { reason, message: '합성 보류', until }, counts: counts({ P1: rows.length }) }), listJson(rows)))
const shortList = [item(11), item(12, { tier: 'P2' })]
const everyTier = [
  item(21, { tier: 'P0', title: '합성 긴급 항목' }),
  accepted(22, 'waiting_user'),
  startable(23),
  item(24, { openedAt: '2026-11-03T22:00:00.000Z' }),
  item(25, { tier: 'P2' }),
  parked(26),
  item(27, { baseline: true }),
]

export default defineCheck({
  // 요청을 흉내 내지 않아 패널에서 워커까지 그대로 간다. 점검 실행이라는 실패가 그려져야 한다.
  workerInCheckMode: {},
  gateClosed: answers(snapshotOf(nextJson({ display: recordingGate, card: null, hold: { reason: 'recording', message: '기록 중입니다', until: null } }), listJson([], { hidden: 4 }))),
  gateClosedSources: answers(snapshotOf(nextJson({ display: recordingGate, card: null, sources: sourceRows(), hold: { reason: 'recording', message: '기록 중입니다', until: null } }), listJson([], { hidden: 4 }))),
  gatePaused: answers(snapshotOf(nextJson({ display: { ...recordingGate, reason: 'paused', startsAt: null } }), listJson([], { hidden: 2 }))),
  gateDisplayOff: answers(snapshotOf(nextJson({ display: { ...recordingGate, reason: 'display_off', startsAt: null } }), listJson([], { hidden: 2 }))),
  gateNoCollection: answers(snapshotOf(nextJson({ display: { ...recordingGate, reason: 'no_collection', startsAt: null } }), listJson())),
  empty: answers(snapshotOf(nextJson(), listJson())),
  card: answers(snapshotOf(nextJson({ card: card([startable(1)]), counts: counts({ P1: 2 }) }), listJson([startable(1), item(3)]))),
  offerCard: answers(snapshotOf(nextJson({ card: card([startable(1, { reason: 'review_owed', reasonLabel: '리뷰 요청', playbook: 'pr_review' })]), counts: counts({ P1: 2 }) }), listJson([startable(1), startable(2, { tier: 'P2' })]))),
  offerRows: answers(snapshotOf(nextJson({ counts: counts({ P1: 2, P2: 2 }) }), listJson([startable(1), startable(2, { tier: 'P2', playbook: 'reply_draft' }), item(3), item(4, { tier: 'P2' })]))),
  offerLocked: answers(snapshotOf(nextJson({ card: card([offered(1)]), counts: counts({ P1: 2 }) }), listJson([offered(1), offered(2)]))),
  offerUrgentRow: answers(snapshotOf(nextJson({ counts: counts({ P0: 1 }) }), listJson([startable(1, { tier: 'P0' }), item(2, { tier: 'P0' })]))),
  urgentCard: answers(snapshotOf(nextJson({ card: card([item(1, { tier: 'P0' })], { tier: 'P0' }), counts: counts({ P0: 1, P1: 1 }) }), listJson([item(1, { tier: 'P0' }), item(3)]))),
  everyGroup: answers(snapshotOf(nextJson({ card: card([startable(1)]), sources: sourceRows(true), counts: counts({ P0: 1, P1: 3, P2: 1 }, { accepted: 1, parked: 1, baseline: 1 }) }), listJson([startable(1), ...everyTier]))),
  running: answers(snapshotOf(nextJson({ card: card([accepted(1)]), counts: counts({}, { accepted: 1 }) }), listJson([accepted(1)]))),
  sessionReady: answers(snapshotOf(nextJson({ ready: [readyCard(1)], counts: counts({}, { accepted: 1 }) }), listJson([accepted(1, 'waiting_user')]))),
  sessionQuestion: answers(snapshotOf(nextJson({ ready: [sessionNews('question', 1, 'run-q')], counts: counts({}, { accepted: 1 }) }), listJson([accepted(1, 'waiting_user')]))),
  sessionAttention: answers(snapshotOf(nextJson({ ready: [sessionNews('attention', 1, 'run-a')], counts: counts({}, { accepted: 1 }) }), listJson([accepted(1, 'needs_attention')]))),
  sessionEndedNews: answers(snapshotOf(nextJson({ ready: [sessionNews('ended', 1, 'run-e')], counts: counts({}, { accepted: 1 }) }), listJson([revivable(1)]))),
  sessionManyNews: answers(
    snapshotOf(
      nextJson({ ready: [sessionNews('done', 1, 'run-1'), sessionNews('ended', 2, 'run-2'), sessionNews('question', 3, 'run-3'), sessionNews('attention', 4, 'run-4'), failedCard(5, 'run-5')] }),
      listJson([startable(5)]),
    ),
  ),
  sessionFailed: answers(snapshotOf(nextJson({ ready: [failedCard(1)], counts: counts({ P2: 1 }) }), listJson([startable(1)]))),
  sessionFailedGateClosed: answers(snapshotOf(nextJson({ display: recordingGate, card: null, ready: [failedCard(1)] }), listJson([], { hidden: 1 }))),
  sessionEnded: answers(snapshotOf(nextJson({ counts: counts({}, { accepted: 1 }) }), listJson([revivable(1)]))),
  bundle: answers(snapshotOf(nextJson({ card: bundle([item(4, { tier: 'P2' }), item(5, { tier: 'P2' })], 3), counts: counts({ P2: 5 }) }), listJson())),
  mergedCard: answers(snapshotOf(nextJson({ card: merged([item(4), item(5), item(6)]), counts: counts({ P1: 3 }) }), listJson([item(4), item(5), item(6)]))),
  carryoverCard: answers(
    snapshotOf(
      nextJson({ card: carryover([item(4, { carried: true, openedAt: '2026-11-03T02:00:00.000Z' }), item(5, { carried: true, openedAt: '2026-10-30T02:00:00.000Z' })]), counts: counts({ P1: 2 }) }),
      listJson([]),
    ),
  ),
  longAndDangerousTitle: answers(snapshotOf(nextJson({ card: card([danger]) }), listJson([danger, item(6, { title: LONG_TITLE })]))),
  slackTitle: answers(snapshotOf(nextJson({ card: card([item(1, { title: SLACK_TITLE })]), counts: counts({ P1: 2 }) }), listJson([item(1, { title: SLACK_TITLE }), item(2, { title: SLACK_TITLE })]))),
  health: answers(snapshotOf(nextJson({ health: 'Slack 수집이 30분 늦습니다' }), listJson())),
  healthWithRows: answers(snapshotOf(nextJson({ health: 'Slack 수집이 30분 늦습니다', counts: counts({ P1: 1, P2: 1 }) }), listJson(shortList))),
  healthWithSources: answers(snapshotOf(nextJson({ health: 'Slack 수집이 37분 늦습니다', sources: sourceRows(), counts: counts({ P1: 1, P2: 1 }) }), listJson(shortList))),
  nowEmptyOthersLeft: answers(snapshotOf(nextJson({ counts: counts({ P2: 1 }, { parked: 1, accepted: 1 }) }), listJson([item(12, { tier: 'P2' }), parked(13), accepted(14)]))),
  holdDayOff: held('day_off', shortList),
  holdOffHours: held('off_hours', shortList),
  holdFocus: held('focus', shortList),
  holdLunch: held('lunch', shortList),
  holdBudgetHour: held('budget_hour', shortList),
  holdBudgetDay: held('budget_day', shortList),
  holdNothing: held('nothing', shortList),
  holdFocusEmpty: held('focus', []),
  focusEnded: answers(
    snapshotOf(
      nextJson({ hold: { reason: 'budget_hour', message: '합성 보류', until }, focus: { active: false, startedAt: null, until: null, heldP1: 3, heldLine: '포커스 중 쌓인 P1 3건' }, counts: counts({ P1: 2 }) }),
      listJson(shortList),
    ),
  ),
  focusActive: answers(
    snapshotOf(
      nextJson({ hold: { reason: 'focus', message: '합성 보류', until }, focus: { active: true, startedAt: null, until, heldP1: 2, heldLine: null }, counts: counts({ P1: 2 }) }),
      listJson(shortList),
    ),
  ),
  workday: answers(
    snapshotOf(
      nextJson({ card: card([startable(2, { title: workdayRows()[1]?.title, carried: true })]), sources: sourceRows(true), counts: counts({ P0: 1, P1: 5, P2: 15 }, { carriedP1: 3, parked: 1, manual: 1 }) }),
      listJson(workdayRows()),
    ),
  ),
  // 프로바이더 칩과 출처 아이콘. 저장소의 기준이 없어 처음 연 패널처럼 새 일 점이 없다.
  providers: answers(snapshotOf(nextJson({ sources: sourceRows(true), counts: counts({ P0: 2, P1: 4, P2: 1 }, { parked: 1, manual: 1 }), newest: 38 }), listJson(providerRows()))),
  // 패널이 마지막으로 본 뒤 Slack, Calendar, Gmail에 새 일이 왔다. 칩, 탭, 줄에 빨간 점이 있다.
  providersFresh: {
    ...answers(snapshotOf(nextJson({ sources: sourceRows(true), counts: counts({ P0: 2, P1: 4, P2: 1 }, { parked: 1, manual: 1 }), newest: 38 }), listJson(providerRows()))),
    storage: { marks: { floor: 32, seen: [35], known: 32, unseen: 3 } },
  },
  // 먼저 볼 일 카드의 제안. 맡겨도 된다고 본 근거와 일할 곳을 쓴다.
  offerWhyCard: answers(
    snapshotOf(
      nextJson({ card: card([startable(1, { why: why({ rule: 'p2.assigned', events: 2, facts: ['issue', 'assigned_me'], offer: { ...OFFER_WHY, repo: 'contoso-synthetic/issues' } }) })]), counts: counts({ P1: 1 }) }),
      listJson([startable(1, { why: why({ rule: 'p2.assigned', events: 2, facts: ['issue', 'assigned_me'], offer: { ...OFFER_WHY, repo: 'contoso-synthetic/issues' } }) })]),
    ),
  ),
  items40: answers(snapshotOf(nextJson({ card: card([startable(900)]), counts: counts({ P0: 8, P1: 16, P2: 16 }) }), listJson(manyItems(40)))),
  items120: answers(snapshotOf(nextJson({ counts: counts({ P0: 24, P1: 48, P2: 48 }) }), listJson(manyItems(120)))),
  parkedRows: answers(snapshotOf(nextJson({ counts: counts({}, { parked: 3 }) }), listJson([parked(1), parked(2, { parkTrigger: 'deploy' }), parked(3, { parkTrigger: 'kept' })]))),
  baselineRows: answers(snapshotOf(nextJson(), listJson([item(1, { baseline: true }), item(2, { baseline: true })]))),
  manualRow: answers(snapshotOf(nextJson({ counts: counts({ P1: 1 }, { manual: 1 }) }), listJson([manual(1)]))),
  kairosFailed: answers({ next: null, list: null, error: { code: 'bad_output', message: 'kairos 응답을 읽지 못했습니다' } }),
  listFailed: answers({ ...snapshotOf(nextJson({ card: card([startable(1)]), counts: counts({ P1: 1 }) }), listJson()), list: null, error: { code: 'timeout', message: 'kairos 응답 시간을 넘겼습니다' } }),
  nextFailed: answers({ ...snapshotOf(nextJson(), listJson(shortList)), next: null, error: { code: 'timeout', message: 'kairos 응답 시간을 넘겼습니다' } }),
  workerDown: {
    requests: {
      'kairos.snapshot': () => {
        throw hostError('unavailable', 'worker down')
      },
    },
  },
})
