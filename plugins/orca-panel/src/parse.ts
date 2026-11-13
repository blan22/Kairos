/**
 * kairos JSON 계약 v1을 패널이 그릴 값으로 줄인다. kairos가 정제해서 내지만 여기가 마지막 경계라 다시 거른다.
 * argv는 넘기지 않는다. 버튼은 고정 이름으로만 전달되고, 인자는 워커가 argvFor로 새로 만든다.
 */
import { LIST_LIMIT } from './config.ts'
import type {
  CardView,
  CountsView,
  DeployView,
  Failure,
  FocusLine,
  GateView,
  HoldView,
  ItemActionId,
  ItemRow,
  ListView,
  BudgetView,
  NextView,
  OfferWhy,
  PanelAction,
  ReadyView,
  RevealView,
  RunView,
  SourceView,
  Tier,
  WhyView,
} from './protocol.ts'
import { sectionOf, sortRows, trimRows } from './rows.ts'
import { cleanText, displayTitle } from './text.ts'

export const CONTRACT_VERSION = 1
/** kairos의 ITEM_ID와 같은 모양이다. */
export const ITEM_ID = /^[1-9]\d{0,14}$/

const TITLE_LIMIT = 300
const LINE_LIMIT = 200
const LABEL_LIMIT = 40
const URL_LIMIT = 512
const ERROR_CODE = /^[a-z][a-z0-9_]{0,39}$/
const HTTPS_URL = /^https:\/\/\S+$/
const TIERS: readonly string[] = ['P0', 'P1', 'P2']
const CARD_KINDS: readonly string[] = ['item', 'bundle', 'carryover']
const MOVE: PanelAction = { id: 'move', label: '이동' }
/** kairos reveal이 받는 ID 수의 상한과 같다. */
const REVEAL_LIMIT = 20
const SOURCE_NAME = /^[a-z][a-z0-9_]{0,31}$/
const SOURCE_STATES: readonly string[] = ['ok', 'delayed', 'never', 'not_connected', 'quiet']
/** kairos의 소스는 다섯이다. 넉넉히 받고 나머지는 버린다. */
const SOURCE_LIMIT = 12

type Json = Record<string, unknown>

export const isRecord = (value: unknown): value is Json => typeof value === 'object' && value !== null && !Array.isArray(value)
const count = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : 0)
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : [])
const optional = (value: unknown, limit: number): string | null => cleanText(value, limit) || null

function itemId(value: unknown): number | null {
  return typeof value === 'number' && ITEM_ID.test(String(value)) ? value : null
}

function tierOf(value: unknown): Tier | null {
  return typeof value === 'string' && TIERS.includes(value) ? (value as Tier) : null
}

function parseRun(value: unknown): RunView | null {
  if (!isRecord(value)) return null
  const id = cleanText(value.id, 64)
  const state = cleanText(value.state, 32)
  return id && state ? { id, state } : null
}

/** 계약에 있는 버튼만 고정 이름으로 옮긴다. 나머지는 버린다. */
function actionName(raw: Json): ItemActionId | null {
  const argv = list(raw.argv)
  switch (raw.id) {
    case 'start':
    case 'later':
    case 'done':
    case 'ack':
    case 'move':
    case 'cancel':
    case 'pr':
      return raw.id
    case 'keep':
      return argv.includes('--until-deploy') ? 'keep_deploy' : 'keep'
    default:
      return null
  }
}

function parseActions(value: unknown): PanelAction[] {
  return list(value).flatMap(raw => {
    if (!isRecord(raw)) return []
    const id = actionName(raw)
    const label = cleanText(raw.label, LABEL_LIMIT)
    return id !== null && label !== '' ? [{ id, label }] : []
  })
}

/** 이동은 세션이 있을 때만 그린다. kairos가 이미 줬으면 하나만 둔다. */
function withMove(actions: PanelAction[], hasRun: boolean): PanelAction[] {
  return hasRun && !actions.some(action => action.id === 'move') ? [...actions, MOVE] : actions
}

function parseDeploy(value: unknown): DeployView | null {
  const date = isRecord(value) ? cleanText(value.date, 16) : ''
  return date === '' ? null : { date }
}

/** 규칙 ID와 사실 코드는 고정 낱말 모양만 받는다. 패널은 표에 있는 코드만 그린다. */
const RULE_ID = /^[a-z0-9_]{1,16}\.[a-z0-9_]{1,40}$/
const FACT_CODE = /^[a-z][a-z_]{0,31}$/
const FACT_LIMIT = 16
/** GitHub 저장소 이름 모양(`owner/name`). kairos가 플레이북 정규식으로 검사했지만 여기서 다시 거른다. */
const REPO_NAME = /^[A-Za-z0-9-]{1,39}\/[A-Za-z0-9_.-]{1,100}$/
/** 기준 브랜치. kairos-config의 검사와 같은 모양(`origin/` + 안전한 글자)이다. */
const BASE_BRANCH = /^origin\/[A-Za-z0-9._\/-]{1,200}$/

const codes = (value: unknown, pattern: RegExp, limit: number): string[] =>
  [...new Set(list(value).filter((code): code is string => typeof code === 'string' && pattern.test(code)))].slice(0, limit)

/** 저장소와 브랜치 중 하나라도 모양이 틀리면 제안 근거를 통째로 버린다. 버튼과 범위는 그대로 남는다. */
function parseOfferWhy(value: unknown): OfferWhy | null {
  if (!isRecord(value)) return null
  const { repo, workspace, base } = value
  if (typeof repo !== 'string' || !REPO_NAME.test(repo)) return null
  if (typeof workspace !== 'string' || !REPO_NAME.test(workspace)) return null
  if (typeof base !== 'string' || !BASE_BRANCH.test(base)) return null
  return { repo, workspace, base, checks: codes(value.checks, FACT_CODE, FACT_LIMIT) }
}

function parseWhy(value: unknown): WhyView | null {
  if (!isRecord(value)) return null
  const startsAt = optional(value.startsAt, 40)
  return {
    rule: typeof value.rule === 'string' && RULE_ID.test(value.rule) ? value.rule : null,
    events: Math.min(count(value.events), 9_999),
    facts: codes(value.facts, FACT_CODE, FACT_LIMIT),
    attendees: count(value.attendees) || null,
    startsAt: startsAt !== null && !Number.isNaN(Date.parse(startsAt)) ? startsAt : null,
    offer: parseOfferWhy(value.offer),
  }
}

function parseItem(value: unknown): ItemRow | null {
  if (!isRecord(value)) return null
  const id = itemId(value.id)
  const tier = tierOf(value.tier)
  if (id === null || tier === null) return null
  const url = cleanText(value.url, URL_LIMIT)
  const run = parseRun(value.run)
  return {
    id,
    tier,
    reason: cleanText(value.reason, 32),
    reasonLabel: cleanText(value.reasonLabel, LABEL_LIMIT),
    source: optional(value.source, 32),
    sourceLabel: cleanText(value.sourceLabel, LABEL_LIMIT),
    state: cleanText(value.state, 16),
    title: displayTitle(value.title, TITLE_LIMIT),
    url: HTTPS_URL.test(url) ? url : null,
    openedAt: optional(value.openedAt, 40),
    dueAt: optional(value.dueAt, 40),
    parkedUntil: optional(value.parkedUntil, 40),
    parkTrigger: optional(value.parkTrigger, 32),
    baseline: value.baseline === true,
    carried: value.carried === true,
    pushCount: count(value.pushCount),
    deploy: parseDeploy(value.deploy),
    playbook: optional(value.playbook, LABEL_LIMIT),
    scope: optional(value.scope, LINE_LIMIT),
    run,
    actions: withMove(parseActions(value.actions), run !== null),
    why: parseWhy(value.why),
  }
}

const parseItems = (value: unknown): ItemRow[] => list(value).flatMap(raw => parseItem(raw) ?? [])

function parseCard(value: unknown): CardView | null {
  if (!isRecord(value)) return null
  const tier = tierOf(value.tier)
  const kind = typeof value.kind === 'string' && CARD_KINDS.includes(value.kind) ? (value.kind as CardView['kind']) : null
  const items = parseItems(value.items)
  if (tier === null || kind === null || items.length === 0) return null
  const [only] = items
  return {
    id: cleanText(value.id, 80),
    kind,
    tier,
    headline: cleanText(value.headline, LINE_LIMIT),
    items: sortRows(items),
    more: count(value.more),
    itemIds: list(value.itemIds).flatMap(id => itemId(id) ?? []),
    actions: withMove(parseActions(value.actions), kind === 'item' && items.length === 1 && only?.run !== null),
    pending: value.pending === true,
  }
}

function parseReady(value: unknown): ReadyView | null {
  if (!isRecord(value)) return null
  const runId = cleanText(value.runId, 64)
  const id = itemId(value.itemId)
  if (runId === '' || id === null) return null
  const url = cleanText(value.url, URL_LIMIT)
  return {
    runId,
    itemId: id,
    kind: cleanText(value.kind, 16),
    headline: cleanText(value.headline, LINE_LIMIT),
    title: displayTitle(value.title, TITLE_LIMIT),
    url: HTTPS_URL.test(url) ? url : null,
    actions: parseActions(value.actions).filter(action => action.id === 'move' || action.id === 'pr'),
  }
}

function parseGate(value: unknown): GateView | null {
  if (!isRecord(value) || typeof value.open !== 'boolean') return null
  return {
    open: value.open,
    mode: cleanText(value.mode, 16),
    reason: optional(value.reason, 32),
    message: optional(value.message, LINE_LIMIT),
    startsAt: optional(value.startsAt, 40),
  }
}

function parseHold(value: unknown): HoldView | null {
  if (!isRecord(value)) return null
  return { reason: cleanText(value.reason, 32), message: cleanText(value.message, LINE_LIMIT), until: optional(value.until, 40) }
}

function parseCounts(value: unknown): CountsView | null {
  if (!isRecord(value) || !isRecord(value.open)) return null
  const { P0, P1, P2 } = value.open
  return { P0: count(P0), P1: count(P1), P2: count(P2), manual: count(value.manual), parked: count(value.parked), accepted: count(value.accepted), carried: count(value.carriedP1) }
}

function parseFocus(value: unknown): FocusLine {
  if (!isRecord(value)) return { active: false, until: null, heldP1: 0, heldLine: null }
  return { active: value.active === true, until: optional(value.until, 40), heldP1: count(value.heldP1), heldLine: optional(value.heldLine, LINE_LIMIT) }
}

/** ID가 하나라도 틀리거나 시각이 양수가 아니면 통째로 버린다. 알림을 누른 일을 잘못 짚는 것보다 띄우지 않는 편이 낫다. */
function parseReveal(value: unknown): RevealView | null {
  if (!isRecord(value)) return null
  const { at } = value
  if (typeof at !== 'number' || !Number.isFinite(at) || at <= 0) return null
  const raw = list(value.items)
  if (raw.length === 0 || raw.length > REVEAL_LIMIT) return null
  const items = raw.flatMap(id => itemId(id) ?? [])
  return items.length === raw.length ? { items: [...new Set(items)], at } : null
}

/** 이름과 상태가 고정 낱말이 아니면 그 줄을 버린다. 이름이 같은 줄은 처음 것만 쓴다. */
function parseSources(value: unknown): SourceView[] {
  const rows: SourceView[] = []
  for (const raw of list(value)) {
    if (rows.length >= SOURCE_LIMIT) break
    if (!isRecord(raw)) continue
    const { source, status, delayMinutes } = raw
    if (typeof source !== 'string' || !SOURCE_NAME.test(source) || typeof status !== 'string' || !SOURCE_STATES.includes(status)) continue
    if (rows.some(row => row.source === source)) continue
    const delay = typeof delayMinutes === 'number' && Number.isFinite(delayMinutes) && delayMinutes >= 0 ? Math.floor(delayMinutes) : null
    rows.push({ source, label: cleanText(raw.label, LABEL_LIMIT) || source, status, lastOkAt: optional(raw.lastOkAt, 40), delayMinutes: delay })
  }
  return rows
}

function parseBudget(value: unknown): BudgetView | null {
  if (!isRecord(value)) return null
  return {
    hourUsed: count(value.p1HourUsed),
    hourLimit: count(value.p1HourLimit),
    dayUsed: count(value.p1DayUsed),
    dayLimit: count(value.p1DayLimit),
    bundlesToday: count(value.bundlesToday),
    shownToday: count(value.shownToday),
  }
}

/** 필수 값이 없으면 null이다. 호출한 쪽이 형식 오류로 표시한다. */
export function parseNext(value: Json): NextView | null {
  const display = parseGate(value.display)
  const counts = parseCounts(value.counts)
  if (display === null || counts === null) return null
  return {
    at: optional(value.at, 40),
    display,
    card: parseCard(value.card),
    hold: parseHold(value.hold),
    ready: list(value.ready).flatMap(raw => parseReady(raw) ?? []),
    activeRuns: isRecord(value.runs) ? count(value.runs.active) : 0,
    counts,
    focus: parseFocus(value.focus),
    health: optional(value.health, LINE_LIMIT),
    nextBundleAt: isRecord(value.budget) ? optional(value.budget.nextBundleAt, 40) : null,
    reveal: parseReveal(value.reveal),
    sources: parseSources(value.sources),
    budget: parseBudget(value.budget),
    newest: itemId(value.newest),
  }
}

/** 한도를 넘으면 묶음마다 일부를 남기고 앞쪽 묶음부터 채운다. 빠지는 줄은 뒤쪽 묶음의 것이라 오래된 항목이라고 단정할 수 없다. */
export function parseList(value: Json): ListView | null {
  if (!Array.isArray(value.items)) return null
  const sorted = sortRows(parseItems(value.items))
  const items = trimRows(sorted, LIST_LIMIT)
  const kept = new Set(items)
  const omittedBy: Record<string, number> = {}
  for (const row of sorted) {
    if (kept.has(row)) continue
    const section = sectionOf(row)
    omittedBy[section] = (omittedBy[section] ?? 0) + 1
  }
  return { at: optional(value.at, 40), items, hidden: count(value.hidden), omitted: sorted.length - items.length, omittedBy }
}

/** kairos가 ok:false로 답한 오류. code와 message는 고정 값이지만 다시 거른다. */
export function failureOf(value: Json): Failure {
  const error = isRecord(value.error) ? value.error : {}
  const code = typeof error.code === 'string' && ERROR_CODE.test(error.code) ? error.code : 'failed'
  return { code, message: cleanText(error.message, LINE_LIMIT) || '처리하지 못했습니다' }
}

/** 항목 조작 응답을 한 줄로 줄인다. 수락의 경고가 있으면 뒤에 붙인다. PR 올리기는 결과 문구와 PR 주소다. */
export function resultLine(value: Json): string {
  if (isRecord(value.pr)) {
    const message = cleanText(value.pr.message, LINE_LIMIT) || 'PR을 올렸습니다'
    const url = cleanText(value.pr.url, URL_LIMIT)
    const line = HTTPS_URL.test(url) ? `${message}: ${url}` : message
    return value.pr.uncommitted === true ? `${line}. 커밋하지 않은 변경은 들지 않았습니다` : line
  }
  const [first] = list(value.results)
  const line = (isRecord(first) ? cleanText(first.message, LINE_LIMIT) : '') || '처리했습니다'
  const warning = cleanText(value.warning, LINE_LIMIT)
  return warning ? `${line}. ${warning}` : line
}

export const runOf = (value: Json): RunView | null => parseRun(value.run)

/** 첫 결과의 상태. 패널이 시작이 실제로 일어났는지 구분할 때 쓴다. */
export function statusOf(value: Json): string | null {
  const [first] = list(value.results)
  return isRecord(first) ? optional(first.status, 32) : null
}

export const warningOf = (value: Json): string | null => optional(value.warning, LINE_LIMIT)
