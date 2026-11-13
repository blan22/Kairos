/** 시험과 점검 전용 합성 값. Orca는 이 파일을 싣지 않는다. 실제 이름이나 실제 데이터를 넣지 않는다. */
import { parseList, parseNext } from './parse.ts'
import type { Snapshot } from './protocol.ts'

type Json = Record<string, unknown>

export const DANGEROUS_TITLE = '$(touch /tmp/x); `id` && rm -rf ~ | <img src=x onerror=alert(1)> [링크](https://evil.test) **굵게**'
export const LONG_TITLE = `합성 긴 제목 ${'가나다라마바사 '.repeat(120)}`
/** Slack 원문 표기가 그대로 온 제목. 이름과 ID는 합성이다. */
export const SLACK_TITLE = '<@U000AAA|합성이름> 님이 <#C000AAA|합성채널>에서 <https://example.test/a|합성 링크> 확인을 부탁했습니다 &amp; <@U000BBB>'

export const openGate: Json = { open: true, mode: 'auto', reason: null, message: null, startsAt: null }
export const recordingGate: Json = {
  open: false,
  mode: 'auto',
  reason: 'recording',
  message: '기록 중입니다. 내일 시작합니다',
  startsAt: '2026-11-05T11:00:00.000Z',
}

const budget: Json = {
  p1HourUsed: 0,
  p1HourLimit: 2,
  p1DayUsed: 0,
  p1DayLimit: 6,
  bundlesToday: 0,
  shownToday: 0,
  dayTarget: 6,
  nextBundleAt: '2026-11-05T02:00:00.000Z',
}
const focus: Json = { active: false, startedAt: null, until: null, heldP1: 0, heldLine: null }

export function counts(open: { P0?: number; P1?: number; P2?: number } = {}, rest: Json = {}): Json {
  return {
    open: { P0: open.P0 ?? 0, P1: open.P1 ?? 0, P2: open.P2 ?? 0 },
    manual: 0,
    parked: 0,
    accepted: 0,
    baseline: 0,
    carriedP1: 0,
    overdueP1: 0,
    pushedOutP1: 0,
    p0Unnotified: 0,
    ...rest,
  }
}

const argvAction = (id: string, label: string, ...argv: string[]): Json => ({ id, label, argv })

export function item(id: number, overrides: Json = {}): Json {
  const key = String(id)
  return {
    id,
    tier: 'P1',
    reason: 'assigned',
    reasonLabel: '담당 이슈',
    state: 'open',
    source: 'github',
    sourceLabel: 'GitHub',
    title: `합성 이슈 ${key}`,
    url: `https://github.com/example/repo/issues/${key}`,
    openedAt: '2026-11-04T01:00:00.000Z',
    dueAt: null,
    parkedUntil: null,
    parkTrigger: null,
    baseline: false,
    carried: false,
    pushCount: 0,
    deploy: null,
    playbook: null,
    scope: null,
    run: null,
    actions: [
      argvAction('later', '나중에', 'later', key, '--json'),
      argvAction('done', '끝', 'done', key, '--json'),
      argvAction('keep', '할일로 남김', 'keep', key, '--json'),
    ],
    ...overrides,
  }
}

export const OFFER_SCOPE = '합성 제안 범위입니다. 코멘트는 하지 않습니다.'

/** 세션이 잠긴 제안. 범위는 있고 시작이 없다. */
export function offered(id: number, overrides: Json = {}): Json {
  return { ...item(id), playbook: 'qa_issue', scope: OFFER_SCOPE, ...overrides }
}

/** 제안 근거. 저장소 이름은 합성이다. */
export const OFFER_WHY: Json = {
  repo: 'contoso-synthetic/app',
  workspace: 'contoso-synthetic/app',
  base: 'origin/main',
  checks: ['assigned_me', 'open', 'mapped', 'insiders', 'no_session'],
}

/** 추천 이유. 규칙 ID와 사실 코드는 kairos 계약의 낱말이다. */
export function why(overrides: Json = {}): Json {
  return { rule: 'p1.ci_failed', events: 3, facts: ['pull_request', 'authored_me', 'checks_failed'], ...overrides }
}

/** 시작 버튼이 있는 항목. */
export function startable(id: number, overrides: Json = {}): Json {
  const key = String(id)
  const base = offered(id, overrides)
  return { ...base, actions: [argvAction('start', '시작', 'accept', key, '--json'), ...(base.actions as Json[])] }
}

/** 세션이 떠 있는 항목. 진행 중인 accepted에는 끝만 있다. */
export function accepted(id: number, state: string = 'started'): Json {
  const key = String(id)
  return {
    ...item(id),
    state: 'accepted',
    run: { id: '00000000-0000-4000-8000-000000000001', state },
    actions: [argvAction('done', '끝', 'done', key, '--json')],
  }
}

/** 세션이 끝나 되살릴 수 있는 항목. */
export function revivable(id: number): Json {
  const key = String(id)
  return {
    ...item(id),
    state: 'accepted',
    run: { id: '00000000-0000-4000-8000-000000000001', state: 'ended' },
    actions: [argvAction('cancel', '되돌리기', 'accept', key, '--cancel', '--json'), argvAction('done', '끝', 'done', key, '--json')],
  }
}

/** 미뤄 둔 항목. 완료만 할 수 있다. */
export function parked(id: number, overrides: Json = {}): Json {
  const key = String(id)
  return { ...item(id), state: 'parked', parkedUntil: '2026-11-05T01:00:00.000Z', actions: [argvAction('done', '끝', 'done', key, '--json')], ...overrides }
}

/** 사용자가 직접 넣은 할 일. 출처와 주소가 없다. */
export function manual(id: number): Json {
  const key = String(id)
  return {
    ...item(id),
    tier: 'P2',
    reason: 'manual',
    reasonLabel: '직접 추가',
    source: null,
    sourceLabel: 'kairos',
    title: `합성 할 일 ${key}`,
    url: null,
    actions: [argvAction('later', '나중에', 'later', key, '--json'), argvAction('done', '끝', 'done', key, '--json')],
  }
}

const SYNTHETIC_TIERS = ['P1', 'P2', 'P2', 'P1', 'P0'] as const

/** 건수가 많은 목록. 뒤로 갈수록 10분씩 오래됐다. */
export function manyItems(total: number): Json[] {
  const newest = Date.parse('2026-11-04T01:00:00.000Z')
  return Array.from({ length: total }, (_, index) =>
    item(index + 1, {
      tier: SYNTHETIC_TIERS[index % SYNTHETIC_TIERS.length],
      title: `합성 항목 ${String(index + 1)}`,
      openedAt: new Date(newest - index * 600_000).toISOString(),
    }),
  )
}

const LONG_ISSUE = '합성 결제 화면에서 쿠폰을 적용한 뒤 뒤로 가면 금액이 초기화되는 문제'
const REPLY = (id: number): string => `합성 문서의 배포 일정에 대한 질문 ${String(id)}번에 답장이 필요합니다`

/** 여러 프로바이더의 일과 추천 이유가 함께 있는 하루. 저장소 이름과 제목은 합성이다. */
export function providerRows(): Json[] {
  return [
    startable(31, {
      title: '합성 로그인 화면의 오류 문구가 잘리는 문제',
      why: why({ rule: 'p2.assigned', events: 1, facts: ['issue', 'assigned_me', 'milestone'], offer: OFFER_WHY }),
    }),
    item(32, { tier: 'P0', reason: 'ci_failed', reasonLabel: 'CI 실패', title: '합성 결제 모듈 리팩터링 PR', why: why() }),
    item(33, { source: 'slack', sourceLabel: 'Slack', reason: 'direct_mention', reasonLabel: '나를 언급', title: '합성 채널에서 배포 시각 확인 요청', why: why({ rule: 'p1.direct_mention', events: 2, facts: ['dm'] }) }),
    item(34, { source: 'calendar', sourceLabel: 'Calendar', reason: 'meeting_imminent', reasonLabel: '곧 회의', tier: 'P0', title: '합성 주간 계획 회의', dueAt: '2026-11-04T02:30:00.000Z', why: why({ rule: 'p0.meeting_imminent', events: 1, facts: ['response_needed'], attendees: 6 }) }),
    item(35, { source: 'notion', sourceLabel: 'Notion', reason: 'notion_mention', reasonLabel: 'Notion 언급', title: '합성 분기 회고 문서', why: why({ rule: 'p1.notion_mention', events: 1, facts: [] }) }),
    item(36, { source: 'gmail', sourceLabel: 'Gmail', reason: 'app_review_mail', reasonLabel: '앱 심사', tier: 'P2', title: '합성 앱 심사 결과 메일', why: why({ rule: 'p1.app_review_mail', events: 1, facts: ['mail'] }) }),
    parked(37, { source: 'slack', sourceLabel: 'Slack' }),
    manual(38),
  ]
}

/** 평소 하루의 모양. 건수, 사유 비율, 제목 길이를 실제 분포에 맞춘 합성 값이다. */
export function workdayRows(): Json[] {
  const hours = (ago: number): string => new Date(Date.parse('2026-11-04T02:00:00.000Z') - ago * 3_600_000).toISOString()
  return [
    item(1, { tier: 'P0', reason: 'deploy_risk', reasonLabel: '배포 위험', title: '합성 오늘 배포에 묶인 이슈가 아직 열려 있습니다', dueAt: '2026-11-04T05:00:00.000Z', openedAt: hours(2) }),
    startable(2, { title: LONG_ISSUE, openedAt: hours(26), carried: true }),
    startable(3, { title: '합성 알림 설정 저장이 간헐적으로 실패하는 문제', openedAt: hours(30), carried: true }),
    startable(4, { reason: 'review_owed', reasonLabel: '리뷰 요청', playbook: 'pr_review', title: '합성 주문 목록 페이지네이션 교체 PR', openedAt: hours(5) }),
    item(5, { reason: 'work_owed', reasonLabel: '할 일', sourceLabel: 'Slack', source: 'slack', title: '합성 팀 스레드에서 맡기로 한 지표 정리', openedAt: hours(50), carried: true }),
    item(6, { reason: 'meeting_new', reasonLabel: '일정 변경', sourceLabel: 'Calendar', source: 'calendar', title: '합성 주간 회의 시간 변경', dueAt: '2026-11-04T07:00:00.000Z', openedAt: hours(3) }),
    ...Array.from({ length: 15 }, (_, index) =>
      item(10 + index, { tier: 'P2', reason: 'reply_owed', reasonLabel: '답장 필요', title: REPLY(10 + index), openedAt: hours(1 + index * 3) }),
    ),
    parked(40, { title: '합성 다음 묶음으로 미룬 일' }),
    manual(41),
  ]
}

/** 뒤에서 실패한 세션의 카드. 이동이 없다. */
export function failedCard(itemId: number, runId: string = '00000000-0000-4000-8000-000000000003'): Json {
  return { runId, itemId, kind: 'failed', headline: '세션이 시작하자마자 끝났습니다', title: `합성 이슈 ${String(itemId)}`, url: null, actions: [] }
}

export function readyCard(itemId: number, runId: string = '00000000-0000-4000-8000-000000000002'): Json {
  return {
    runId,
    itemId,
    kind: 'done',
    headline: '준비됨',
    title: `합성 이슈 ${String(itemId)}`,
    url: `https://github.com/example/repo/issues/${String(itemId)}`,
    actions: [argvAction('move', '이동', 'move', String(itemId), '--json')],
  }
}

/** 세션 소식. 질문, 확인 필요, 끝남은 준비됨과 같은 모양에 종류만 다르다. */
export function sessionNews(kind: string, itemId: number, runId: string): Json {
  return { ...readyCard(itemId, runId), kind }
}

export function card(rows: Json[], overrides: Json = {}): Json {
  const [first] = rows
  return {
    id: `item-${String(first?.id)}`,
    kind: 'item',
    tier: 'P1',
    headline: 'P1 담당 이슈',
    items: rows,
    more: 0,
    itemIds: rows.map(row => row.id),
    actions: first?.actions ?? [],
    pending: false,
    shownAt: null,
    ...overrides,
  }
}

export function bundle(rows: Json[], more: number = 0): Json {
  return card(rows, {
    id: 'bundle-1',
    kind: 'bundle',
    tier: 'P2',
    headline: `묶음 ${rows.length + more}건`,
    more,
    actions: [argvAction('ack', '확인', 'next', '--ack', '--json')],
  })
}

/** 어제까지 남은 일을 모은 카드. 줄 행동은 펼쳐서 하고 카드에는 확인만 있다. */
export function carryover(rows: Json[]): Json {
  return card(rows, {
    id: 'carry-1',
    kind: 'carryover',
    headline: `어제에서 넘어온 P1 ${rows.length}건`,
    actions: [argvAction('ack', '확인', 'next', '--ack', '--json')],
  })
}

export function merged(rows: Json[]): Json {
  const ids = rows.map(row => String(row.id))
  return card(rows, {
    headline: `같은 채널 ${rows.length}건`,
    actions: [argvAction('later', '나중에', 'later', ...ids, '--json'), argvAction('done', '끝', 'done', ...ids, '--json')],
  })
}

/** 소스별 수집 상태. 이름과 시각은 합성이다. healthy이면 손볼 소스가 없는 날이다. */
export function sourceRows(healthy: boolean = false): Json[] {
  const ok = (source: string, label: string): Json => ({ source, label, status: 'ok', lastOkAt: '2026-11-04T01:57:00.000Z', delayMinutes: null })
  return [
    healthy ? ok('calendar', 'Calendar') : { source: 'calendar', label: 'Calendar', status: 'not_connected', lastOkAt: null, delayMinutes: null },
    ok('github', 'GitHub'),
    healthy ? ok('slack', 'Slack') : { source: 'slack', label: 'Slack', status: 'delayed', lastOkAt: '2026-11-04T01:20:00.000Z', delayMinutes: 37 },
    { source: 'gmail', label: 'Gmail', status: 'quiet', lastOkAt: '2026-11-03T10:00:00.000Z', delayMinutes: null },
    healthy ? ok('notion', 'Notion') : { source: 'notion', label: 'Notion', status: 'never', lastOkAt: null, delayMinutes: null },
  ]
}

export function nextJson(overrides: Json = {}): Json {
  return {
    schema: 1,
    ok: true,
    at: '2026-11-04T02:00:00.000Z',
    display: openGate,
    card: null,
    hold: { reason: 'nothing', message: '지금 낼 제안이 없습니다', until: null },
    ready: [],
    runs: { active: 0 },
    counts: counts(),
    budget,
    focus,
    health: null,
    ...overrides,
  }
}

export function listJson(rows: Json[] = [], overrides: Json = {}): Json {
  return { schema: 1, ok: true, at: '2026-11-04T02:00:00.000Z', display: openGate, items: rows, hidden: 0, counts: counts(), focus, health: null, ...overrides }
}

export function snapshotOf(next: Json, list: Json): Snapshot {
  return { next: parseNext(next), list: parseList(list), error: null }
}
