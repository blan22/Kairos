/**
 * 지금 구간 하나와 묶음 목록. 외부 문자열은 Text와 ListItem title로만 그린다. Markdown과 Link를 쓰지 않는다.
 * 주소는 글자로 그리지 않고, 사용자가 원문 열기를 눌렀을 때 허용 호스트의 https만 연다.
 * seen은 사용자가 패널을 연 순간과 버튼, 새로고침에서만 준다. 60초 틱은 열어 둔 패널을 다시 그릴 뿐 노출과 예산에 남기지 않는다.
 * 행 펼치기, 묶음 접기, 원문 열기도 kairos를 부르지 않는다. Orca가 앞에 있는지 패널이 알 수 없기 때문이다(설계 6.1, 16절).
 * 알림을 누르면 워커가 kairos.reveal을 보낸다. 사용자가 직접 본 것이라 seen으로 읽고, 누른 일을 맨 위 띠로 올린다.
 */
import {
  atom,
  Box,
  Button,
  Callout,
  Code,
  EmptyState,
  Heading,
  IconButton,
  Input,
  List,
  ListItem,
  plugin,
  read,
  RelativeTime,
  Row,
  Separator,
  set,
  shell,
  Skeleton,
  Spinner,
  Stack,
  Text,
  update,
  type PanelContext,
} from 'orca'
import { ADD_TITLE_LIMIT, PANEL_ACTION_TIMEOUT_MS, PANEL_SNAPSHOT_TIMEOUT_MS } from './config.ts'
import type { ActionId, ActionOutcome, CardView, Failure, GateView, ItemActionId, ItemRow, ListView, NextView, PanelAction, ReadyView, RevealView, Snapshot } from './protocol.ts'
import { canStart, defaultOpen, DISPLAY_ORDER, groupRows, hasOffer, isCompact, sectionOf, sortReady, visibleCount, type Section } from './rows.ts'
import { badgeOf } from './status.ts'
import { actionFailure, carriedWord, cleanText, clip, dayLabel, failureCopy, holdLine, holdSentence, joinLine, lookup, openableUrl, whenLabel } from './text.ts'

/** 지금까지 읽은 값. next나 list가 비어 오면 이전 값을 지우지 않고 그 부분만 stale로 표시한다. */
interface Reading {
  next: NextView | null
  list: ListView | null
  error: Failure | null
  nextStale: boolean
  listStale: boolean
  nextAt: number | null
  listAt: number | null
}

/** 누른 자리. where는 card-{id}, row-{id}, ready-{runId}이다. group은 성공 자취를 쓸 묶음이다. */
interface Spot {
  where: string
  group: string
  rowId: number | null
}

interface Pressing {
  where: string
  action: ActionId
  startedAt: number
}

/** failed는 색을 쓴다. late는 시작이 늦어질 뿐 실패로 단정할 수 없고 다음 읽기에서 풀린다. info는 알려 줄 일이다. */
interface Notice {
  where: string
  kind: 'failed' | 'late' | 'info'
  title: string
  message: string | null
  code: string | null
}

interface OpenNote {
  where: string
  text: string
}

interface Trace {
  group: string
  text: string
}

type IconName = 'edit' | 'search' | 'flag' | 'branch' | 'calendar' | 'rocket' | 'wrench' | 'file-text' | 'bell' | 'terminal'
type Tool = 'focus' | 'add'

interface Trouble {
  title: string
  body: string
  code: string
  readAt: number | null
}

interface RowPlace {
  group: string
  nested: boolean
  /** 합친 카드의 줄이다. 행동은 카드 끝에서 한 번에 하고 줄에는 원문 열기만 둔다. */
  merged: boolean
}

/** 항목 ID에서 화면에 떠 있는 세션 소식 문구로 가는 표. 같은 일을 두 곳에서 다른 말로 부르지 않게 한다. */
type NewsWords = ReadonlyMap<number, string>

const ACTION_WORDS: Record<ActionId, string> = {
  start: '시작',
  move: '세션으로 이동',
  pr: 'PR 올리기',
  later: '나중에',
  keep: '새 소식이 오면 다시',
  keep_deploy: '배포 때 다시',
  done: '완료',
  cancel: '되돌리기',
  ack: '다 봤습니다',
  focus_25: '25분',
  focus_50: '50분',
  focus_90: '90분',
  focus_end: '끝내기',
  add: '추가',
  display_auto: '다시 켜기',
}
const FAILURE_TITLES: Record<ActionId, string> = {
  start: '시작하지 못했습니다',
  move: '세션으로 이동하지 못했습니다',
  pr: 'PR을 올리지 못했습니다',
  later: '미루지 못했습니다',
  keep: '남기지 못했습니다',
  keep_deploy: '남기지 못했습니다',
  done: '완료하지 못했습니다',
  cancel: '되돌리지 못했습니다',
  ack: '확인하지 못했습니다',
  focus_25: '포커스를 시작하지 못했습니다',
  focus_50: '포커스를 시작하지 못했습니다',
  focus_90: '포커스를 시작하지 못했습니다',
  focus_end: '포커스를 끝내지 못했습니다',
  add: '추가하지 못했습니다',
  display_auto: '제안을 켜지 못했습니다',
}
const RUN_WORDS: Record<string, string> = {
  claimed: '세션을 준비하고 있습니다',
  started: '세션이 진행 중입니다',
  waiting_user: '확인을 기다립니다',
  needs_attention: '세션 확인이 필요합니다',
  ended: '세션이 끝났습니다',
}
const RUN_CALLS: ReadonlySet<string> = new Set(['waiting_user', 'needs_attention'])
const READY_WORDS: Record<string, string> = {
  question: '답을 기다립니다',
  attention: '세션 확인이 필요합니다',
  done: '결과가 준비됐습니다',
  ended: '세션이 끝났습니다',
}
const GROUP_WORDS: Record<Section, string> = {
  urgent: '긴급',
  running: '세션',
  offer: '에이전트 제안',
  today: '오늘',
  relaxed: '여유 있을 때',
  parked: '미룬 일',
  baseline: '이전부터 있던 일',
}
const HEAD_TONES: Partial<Record<Section, 'danger' | 'accent'>> = { urgent: 'danger', offer: 'accent' }
/** 무슨 일을 요구받았는지 형태로 읽게 한다. 색은 쓰지 않는다. */
const REASON_ICONS: Record<string, IconName> = {
  reply_owed: 'edit',
  review_owed: 'search',
  assigned: 'flag',
  my_pr_attention: 'branch',
  meeting_new: 'calendar',
  deploy_risk: 'rocket',
  work_owed: 'wrench',
  manual: 'file-text',
}
const FOCUS_CHOICES: readonly ActionId[] = ['focus_25', 'focus_50', 'focus_90']
const TIMED_REASONS: ReadonlySet<string> = new Set(['meeting_new', 'deploy_risk'])
const SESSION_WARNING = '세션 상태를 읽지 못했습니다'
const ALERT_LIMIT = 3
const TRACE_MS = 6_000
/** 펼친 행이 있으면 틱 결과를 맡아 둔다. 다섯 번째 틱에서 반영하므로 약 5분이다. */
const HOLD_TICKS = 4
const NOW_GROUP = 'now'
const HEAD_SPOT: Spot = { where: 'head', group: NOW_GROUP, rowId: null }
/** 직접 추가한 일은 여유 있을 때로 간다. 이 묶음은 자주 접혀 있어 어디로 갔는지 그 머리글에서 알린다. */
const ADD_SPOT: Spot = { ...HEAD_SPOT, group: 'relaxed' }
/** 두 줄에 들어가는 길이다. 340px 폭의 한글 기준이고, 전체 제목은 펼치면 본다. */
const ROW_TITLE_LIMIT = 46
const OPEN_TITLE_LIMIT = 120
const OPENING = '세션을 여는 중입니다'
const REVEAL_WORD = '알림에서 연 일'
/** 누른 알림의 일이 모두 사라졌을 때의 안내. 띠가 없으니 위로 띄운다. */
const REVEAL_SPOT: Spot = { where: 'reveal', group: NOW_GROUP, rowId: null }
const START_WORD = '세션 열고 시작'
/** 제안 상자의 제목. 묶음 이름(에이전트 제안)을 되풀이하지 않고 상자 안의 글이 무엇인지 말한다. */
const SCOPE_WORD = '에이전트에게 맡길 범위'

const reading = atom<Reading>({ next: null, list: null, error: null, nextStale: false, listStale: false, nextAt: null, listAt: null })
/** --seen 뒤로는 kairos가 다시 주지 않는다. 이동하거나 닫을 때까지 패널이 들고 있는다. */
const readyCards = atom<ReadyView[]>([])
/** 처리 중인 누름. 있으면 모든 kairos 버튼을 잠근다. */
const pressing = atom<Pressing | null>(null)
const notice = atom<Notice | null>(null)
const openNote = atom<OpenNote | null>(null)
const trace = atom<Trace | null>(null)
/** 워커까지 닿지 못한 실패. */
const bridgeError = atom<Failure | null>(null)
/** 한 번에 한 행만 펼친다. */
const openRow = atom<number | null>(null)
const heldSnapshot = atom<Snapshot | null>(null)
const alertsOpen = atom(false)
/** 사용자가 부른 읽기가 도는 중이다. 틱 읽기에는 표시하지 않는다. */
const refreshing = atom(false)
/** 머리말 아래에 열린 도구. 한 번에 하나만 연다. */
const tool = atom<Tool | null>(null)
/** 쓰다 만 할 일. 실패해도 지우지 않는다. */
const draft = atom('')
/** 사용자가 고른 값만 남긴다. 없으면 묶음의 기본값을 따른다. */
const folds = atom<Record<string, boolean>>('folds', {}, { persist: 'plugin', version: 1 })
/** 알림에서 연 일. 띠에 들어 올린 항목 ID와 누른 시각이다. */
const reveal = atom<RevealView | null>(null)
/** 마지막으로 적용한 알림의 시각. 패널을 다시 열어도 kairos는 10분 동안 같은 값을 주므로 남겨 둔다. */
const revealSeen = atom<number>('revealAt', 0, { persist: 'plugin', version: 1 })

let context: PanelContext | null = null
/** 같은 틱의 두 번째 누름을 그리기 전에 막는다. */
let locked = false
let wasVisible = false
/** 늦게 온 읽기가 새 결과를 덮지 않게 한다. */
let loadSeq = 0
/** 도는 seen 읽기의 수. 그동안 온 틱은 읽지 않는다. 틱이 seen 응답을 버리게 하면 누른 알림이 맡아 둔 틱 뒤로 밀린다. */
let seenLoads = 0
let heldTicks = 0
let traceTimer: ReturnType<typeof setTimeout> | null = null
/** 닫은 세션 소식. 틱은 --seen 없이 읽어 kairos가 같은 카드를 또 주므로 패널이 기억한다. 종류가 바뀌면 새 소식이다. */
const closedNews = new Set<string>()

const isVisible = (): boolean => (context ? read(context.visible) : false)
const isBusy = (): boolean => read(pressing) !== null
const hasAction = (actions: readonly PanelAction[], id: ActionId): boolean => actions.some(action => action.id === id)
const openedOf = (item: ItemRow): number => (item.openedAt === null ? Number.NaN : Date.parse(item.openedAt))
const isSingle = (card: CardView): boolean => card.kind === 'item' && card.items.length === 1
const newsKey = (card: ReadyView): string => `${card.runId}/${card.kind}`
const isOpening = (where: string): boolean => {
  const held = read(notice)
  return held?.kind === 'late' && held.where === where
}

/** 지금 카드로 간 항목을 뺀 목록 줄. 표시 게이트가 닫히면 그리지 않는다. */
function listedRows(state: Reading): ItemRow[] {
  const { next, list } = state
  if (next?.display.open === false) return []
  const onCard = new Set((next?.card?.items ?? []).map(item => item.id))
  return (list?.items ?? []).filter(item => !onCard.has(item.id))
}

/** 펼칠 수 있는 줄. 단건 카드는 줄이 아니므로 뺀다. */
function rowIdsOf(state: Reading): Set<number> {
  const card = state.next?.display.open === true ? state.next.card : null
  return new Set([...listedRows(state), ...(card === null || isSingle(card) ? [] : card.items)].map(item => item.id))
}

/** 띠에 올릴 수 있는 일. 지금 카드의 일은 카드가 보여 주므로 뺀다. */
function listedIds(state: Reading): Set<number> {
  return new Set(listedRows(state).map(item => item.id))
}

/** 화면에 있는 일. 목록 줄과 지금 카드의 줄이다. 알림에서 연 일이 아직 남았는지 볼 때 쓴다. */
function presentIds(state: Reading): Set<number> {
  const card = state.next?.display.open === true ? state.next.card : null
  return new Set([...listedRows(state), ...(card?.items ?? [])].map(item => item.id))
}

function errorCode(error: unknown): string {
  return typeof error === 'object' && error !== null && 'code' in error ? cleanText(error.code, 40) : ''
}

function stopTraceTimer(): void {
  if (traceTimer !== null) clearTimeout(traceTimer)
  traceTimer = null
}

function clearTrace(): void {
  stopTraceTimer()
  set(trace, null)
}

/** 자취는 위치가 고정된 칸에 쓴다. 나타나고 사라져도 줄이 움직이지 않으므로 펼친 행이 있어도 시간대로 지운다. */
function showTrace(value: Trace): void {
  stopTraceTimer()
  set(trace, value)
  traceTimer = setTimeout(() => {
    traceTimer = null
    set(trace, null)
  }, TRACE_MS)
}

/** kairos는 --seen으로 준 세션 카드를 다시 주지 않는다. 늦게 온 응답이어도 이 값만은 버리면 영영 잃는다. */
function holdReady(value: Snapshot): void {
  const incoming = (value.next?.ready ?? []).filter(card => !closedNews.has(newsKey(card)))
  if (incoming.length === 0) return
  update(readyCards, held => [
    ...held.map(old => incoming.find(card => card.runId === old.runId) ?? old),
    ...incoming.filter(card => !held.some(old => old.runId === card.runId)),
  ])
}

function absorb(value: Snapshot): void {
  const before = read(reading)
  const now = Date.now()
  const after: Reading = {
    next: value.next ?? before.next,
    list: value.list ?? before.list,
    error: value.error,
    nextStale: value.next === null,
    listStale: value.list === null,
    nextAt: value.next === null ? before.nextAt : now,
    listAt: value.list === null ? before.listAt : now,
  }
  set(reading, after)
  holdReady(value)
  set(heldSnapshot, null)
  heldTicks = 0
  clearTrace()
  // 새 목록이 왔으니 늦어진 시작의 결과가 화면에 있다. 아직 시작 가능하면 다시 누를 수 있어야 한다.
  if (read(notice)?.kind === 'late' && value.list !== null) set(notice, null)
  const opened = read(openRow)
  if (opened !== null && !rowIdsOf(after).has(opened)) set(openRow, null)
  const shown = read(reveal)
  if (shown !== null) {
    const listed = listedIds(after)
    if (!shown.items.some(id => listed.has(id))) set(reveal, null)
  }
  applyReveal(after)
}

function takeHeld(): void {
  const held = read(heldSnapshot)
  if (held !== null) absorb(held)
}

function dropReady(keep: (card: ReadyView) => boolean): void {
  for (const card of read(readyCards)) if (!keep(card)) closedNews.add(newsKey(card))
  update(readyCards, held => held.filter(keep))
}

function bridgeFailure(error: unknown): Failure {
  return { code: errorCode(error) || 'unknown', message: '패널이 응답을 받지 못했습니다' }
}

/** 펼쳐 둔 줄이 틱으로 움직이지 않게 미룬다. 늦어진 시작은 결과를 기다리는 중이라 미루지 않는다. */
function shouldHold(): boolean {
  const opened = read(openRow)
  return opened !== null && heldTicks < HOLD_TICKS && read(notice)?.kind !== 'late' && rowIdsOf(read(reading)).has(opened)
}

/** 답을 화면에 반영했고 고장이 없으면 참이다. */
async function load(seen: boolean): Promise<boolean> {
  if (locked || !isVisible() || (!seen && seenLoads > 0)) return false
  loadSeq += 1
  const seq = loadSeq
  if (seen) seenLoads += 1
  try {
    const answer = await plugin.request('kairos.snapshot', { seen }, { timeoutMs: PANEL_SNAPSHOT_TIMEOUT_MS })
    if (seq !== loadSeq) {
      holdReady(answer)
      return false
    }
    set(bridgeError, null)
    if (!seen && shouldHold()) {
      holdReady(answer)
      set(heldSnapshot, answer)
      heldTicks += 1
      return false
    }
    absorb(answer)
    return answer.error === null
  } catch (error) {
    if (seq === loadSeq) set(bridgeError, bridgeFailure(error))
    return false
  } finally {
    if (seen) seenLoads -= 1
  }
}

/** 사용자가 부른 읽기. 도는 동안 머리말이 그 사실을 보여 주고, 성공하면 자리를 잃은 알림도 지운다. */
async function refresh(): Promise<void> {
  if (read(refreshing)) return
  set(refreshing, true)
  // 이 읽기가 새로 띄운 안내(알림의 일이 이미 정리됨)는 남긴다.
  const before = read(notice)
  try {
    if ((await load(true)) && read(notice) === before) set(notice, null)
  } finally {
    set(refreshing, false)
  }
}

function startTrace(outcome: ActionOutcome): string {
  if (outcome.status === 'already_closed') return '이미 처리된 일입니다'
  if (outcome.status === 'unchanged' || outcome.run?.state === 'started') return '이미 세션이 있습니다'
  return outcome.run?.state === 'needs_attention' ? '세션 확인이 필요합니다' : OPENING
}

/** 나중에가 언제까지인지 알아야 안심하고 누른다. 시각을 못 읽으면 kairos가 정한 때를 지어내지 않는다. */
function bundleWord(): string | null {
  const { next } = read(reading)
  return whenLabel(next?.nextBundleAt ?? null, next?.at ?? null)
}

function laterTrace(count: number): string {
  const head = count > 1 ? `${count}건을 미뤘습니다` : '미뤘습니다'
  const when = bundleWord()
  return when === null ? head : `${head} · ${when}에 다시`
}

function traceText(action: ActionId, count: number, outcome: ActionOutcome): string | null {
  switch (action) {
    case 'start':
      return startTrace(outcome)
    case 'later':
      return laterTrace(count)
    case 'keep':
      return '새 소식이 오면 보여 드립니다'
    case 'keep_deploy':
      return '배포 때 다시 보여 드립니다'
    case 'done':
      return count > 1 ? `${count}건을 완료했습니다` : '완료했습니다'
    case 'cancel':
      return '되돌렸습니다'
    case 'pr':
      return outcome.message
    case 'ack':
      return '남은 일은 아래 목록에 있습니다'
    case 'focus_25':
    case 'focus_50':
    case 'focus_90':
      return '포커스를 시작했습니다'
    case 'focus_end':
      return '포커스를 끝냈습니다'
    case 'add':
      return '추가했습니다'
    case 'display_auto':
      return '제안을 다시 켰습니다'
    default:
      return null
  }
}

/** 알림을 누른 자리에 남긴다. 다른 행을 읽는 중이면 그 행을 접지 않고, 자리가 없으면 Panel이 위로 띄운다. */
function pin(value: Notice, spot: Spot): void {
  set(notice, value)
  const opened = read(openRow)
  const free = opened === null || opened === spot.rowId
  if (spot.rowId !== null && free && rowIdsOf(read(reading)).has(spot.rowId)) set(openRow, spot.rowId)
}

/**
 * 누른 알림을 한 번만 적용한다. 반영이 끝난 값으로 보므로 맡아 둔 틱이 있으면 반영된 뒤에 걸린다.
 * 띠에는 목록 줄만 올린다. 지금 카드의 일은 카드가 보여 준다. 한 건이면 펼친다.
 * 게이트가 닫혀 있으면 게이트 사유가 이미 말하므로 따로 알리지 않는다.
 */
function applyReveal(state: Reading): void {
  const target = state.next?.reveal ?? null
  if (target === null || target.at <= read(revealSeen)) return
  // 이전 값으로는 남은 일을 가리지 못한다. 적용했다고 적지 않으니 다음에 읽은 값이 같은 알림을 건다.
  if (state.nextStale || state.listStale || state.list === null) return
  set(revealSeen, target.at)
  if (state.next?.display.open !== true) return
  const present = presentIds(state)
  const found = target.items.filter(id => present.has(id))
  const listed = listedIds(state)
  const items = found.filter(id => listed.has(id))
  set(reveal, items.length === 0 ? null : { items, at: target.at })
  if (found.length === 0) {
    // 목록이 길어 넘기지 않은 일일 수 있다. 정리됐다고 단정하지 않는다.
    if (state.list.omitted > 0) return
    pin({ where: REVEAL_SPOT.where, kind: 'info', title: '이미 정리된 일입니다', message: null, code: null }, REVEAL_SPOT)
    return
  }
  const [only] = found
  if (found.length !== 1 || only === undefined || !rowIdsOf(state).has(only)) return
  const opened = read(openRow)
  if (opened !== null && opened !== only && read(notice)?.where === `row-${opened}`) set(notice, null)
  set(openNote, null)
  set(openRow, only)
}

/** 띠에서 한 줄을 뺀다. 남은 줄이 없으면 띠를 닫는다. */
function dropRevealed(id: number): void {
  update(reveal, shown => {
    if (shown === null || !shown.items.includes(id)) return shown
    const items = shown.items.filter(other => other !== id)
    return items.length === 0 ? null : { ...shown, items }
  })
}

/** 닫으면 줄은 제 묶음으로 돌아간다. 펼친 줄이 띠에 있었으면 접어서 맡아 둔 결과를 반영한다. */
function closeReveal(): void {
  const shown = read(reveal)
  set(reveal, null)
  const opened = read(openRow)
  if (shown !== null && opened !== null && shown.items.includes(opened)) toggleRow(opened)
}

/** 같은 이슈의 세션이 이미 있으면 kairos는 성공으로 답하고 시작 버튼이 그대로 남는다. */
function startBlocked(outcome: ActionOutcome, ids: readonly number[]): boolean {
  const { next, list } = read(reading)
  const rows = [...(list?.items ?? []), ...(next?.card?.items ?? [])]
  return outcome.status === 'unchanged' && rows.some(item => ids.includes(item.id) && canStart(item))
}

function succeed(action: ActionId, ids: readonly number[], spot: Spot, outcome: ActionOutcome): void {
  if (action === 'start' && startBlocked(outcome, ids)) {
    pin({ where: spot.where, kind: 'info', title: '같은 이슈의 세션이 이미 있습니다', message: '세션 묶음에서 그 세션으로 이동할 수 있습니다.', code: null }, spot)
    return
  }
  if (action === 'move' || action === 'cancel') dropReady(card => !ids.includes(card.itemId))
  if (action === 'start') dropReady(card => !(card.kind === 'failed' && ids.includes(card.itemId)))
  if (spot.where.startsWith('ready-')) dropReady(card => `ready-${card.runId}` !== spot.where)
  if (spot.rowId !== null && read(openRow) === spot.rowId) set(openRow, null)
  // 띠에서 처리한 일은 할 일을 마쳤으니 띠에서 뺀다. 남은 자리는 제 묶음이다.
  if (spot.rowId !== null) dropRevealed(spot.rowId)
  if (action === 'start' && typeof outcome.warning === 'string') {
    pin({ where: spot.where, kind: 'info', title: OPENING, message: outcome.warning, code: null }, spot)
    return
  }
  // 할 일은 이어서 적는 일이 많아 입력을 닫지 않고 비운다.
  if (action === 'add') set(draft, '')
  if (FOCUS_CHOICES.includes(action)) set(tool, null)
  const text = traceText(action, ids.length, outcome)
  if (text !== null) showTrace({ group: spot.group, text })
}

function fail(action: ActionId, spot: Spot, code: string): void {
  // 패널의 시간 제한이 끝나도 뒤에서는 세션이 열리고 있을 수 있다.
  const late = action === 'start' && code === 'timeout'
  const value: Notice = late
    ? { where: spot.where, kind: 'late', title: '시작이 늦어지고 있습니다', message: '결과는 잠시 뒤 목록에 나타납니다.', code: null }
    : { where: spot.where, kind: 'failed', title: FAILURE_TITLES[action], message: actionFailure(code), code }
  pin(value, spot)
}

async function press(action: ActionId, ids: readonly number[], spot: Spot, title?: string): Promise<void> {
  if (locked) return
  locked = true
  loadSeq += 1
  set(pressing, { where: spot.where, action, startedAt: Date.now() })
  set(notice, null)
  set(openNote, null)
  clearTrace()
  try {
    const input = title === undefined ? { action, ids: [...ids], seen: isVisible() } : { action, ids: [...ids], seen: isVisible(), title }
    const outcome = await plugin.request('kairos.action', input, { timeoutMs: PANEL_ACTION_TIMEOUT_MS })
    if (outcome.snapshot) absorb(outcome.snapshot)
    else takeHeld()
    set(bridgeError, null)
    if (outcome.ok) succeed(action, ids, spot, outcome)
    else fail(action, spot, outcome.code ?? 'failed')
  } catch (error) {
    takeHeld()
    fail(action, spot, bridgeFailure(error).code)
  } finally {
    locked = false
    set(pressing, null)
  }
}

/** 제스처는 누름 처리의 첫 동작에서만 인정되므로 앞에 다른 동작을 두지 않는다. kairos를 부르지 않고 seen을 주지 않는다. */
function openOriginal(url: string, where: string): void {
  shell.openExternal(url).then(
    result => {
      if (result.reason === 'consent-unavailable') set(openNote, { where, text: 'Orca 창이 가려져 있어 확인을 받지 못했습니다.' })
    },
    error => set(openNote, { where, text: errorCode(error) === 'not_permitted' ? '한 번 더 누르면 열립니다.' : '원문을 열지 못했습니다.' }),
  )
  set(openNote, null)
}

/** 접으면 맡아 둔 결과를 반영한다. 처리 중이면 누른 행이 사라지지 않게 응답이 올 때까지 기다린다. */
function settle(): void {
  if (!locked) takeHeld()
}

function toggleRow(id: number): void {
  const opened = read(openRow)
  set(openNote, null)
  if (opened !== null && read(notice)?.where === `row-${opened}`) set(notice, null)
  set(openRow, opened === id ? null : id)
  if (opened !== id) return
  // 띠에서 접으면 다 본 것이다. 띠를 닫는다.
  if (read(reveal)?.items.includes(id) === true) set(reveal, null)
  settle()
}

const moreKey = (key: Section): string => `${key}:all`

function toggleMore(key: Section): void {
  update(folds, chosen => ({ ...chosen, [moreKey(key)]: chosen[moreKey(key)] !== true }))
}

function toggleFold(key: Section, open: boolean, rows: readonly ItemRow[]): void {
  update(folds, chosen => ({ ...chosen, [key]: !open }))
  const opened = read(openRow)
  if (open && opened !== null && rows.some(row => row.id === opened)) toggleRow(opened)
}

function toggleTool(value: Tool): void {
  set(tool, read(tool) === value ? null : value)
}

/** 빈 제목은 보내지 않는다. 제목은 kairos에 저장만 되고 다른 명령이나 세션으로 가지 않는다. */
function submitDraft(): void {
  const title = cleanText(read(draft), ADD_TITLE_LIMIT)
  if (title !== '') void press('add', [], ADD_SPOT, title)
}

export function setup(ctx: PanelContext): void {
  context = ctx
  locked = false
  loadSeq = 0
  seenLoads = 0
  heldTicks = 0
  closedNews.clear()
  wasVisible = read(ctx.visible)
  ctx.signal.addEventListener('abort', stopTraceTimer)
  ctx.subscriptions.push(plugin.on('kairos.tick', () => void load(false)))
  ctx.subscriptions.push(plugin.on('kairos.reveal', () => void load(true)))
  void load(true)
}

function originalButton(url: string, where: string, key: string) {
  return (
    <Button key={`${key}-original`} size="sm" variant="outline" icon="open-external" onPress={() => openOriginal(url, where)}>
      원문 열기
    </Button>
  )
}

function progress(now: Pressing) {
  if (now.action !== 'start') {
    return (
      <Row gap="xs" align="center">
        <Spinner label="처리 중" labelHidden />
        <Text size="xs" muted>처리하는 중입니다</Text>
      </Row>
    )
  }
  return (
    <Stack gap="xs">
      <Row gap="xs" align="center">
        <Spinner label="처리 중" labelHidden />
        <Text size="xs" muted>{OPENING} · <RelativeTime at={now.startedAt} format="duration" muted /></Text>
      </Row>
      <Text size="xs" muted>작업 공간을 만든 뒤 세션 창을 엽니다. 길면 2분쯤 걸립니다.</Text>
    </Stack>
  )
}

/** 실패와 원문 열기 안내. 누른 자리의 맨 아래에 남는다. */
function placedNotes(where: string) {
  const failed = read(notice)
  const note = read(openNote)
  const shown = failed?.where === where ? failed : null
  const hint = note?.where === where ? note : null
  if (shown === null && hint === null) return null
  return (
    <Stack gap="xs">
      {shown === null ? null : <Text size="xs" tone={shown.kind === 'failed' ? 'danger' : undefined}>{joinLine(shown.title, shown.message)}</Text>}
      {shown?.code == null ? null : <Text size="xs" muted>{`코드 ${shown.code}`}</Text>}
      {hint === null ? null : <Text size="xs" muted>{hint.text}</Text>}
    </Stack>
  )
}

/** 에이전트에게 맡길 범위. 세션은 바로 아래 시작 버튼으로만 열린다. 잠겨 있으면 푸는 길을 알린다. 세션 잠금은 안전 장치라 패널에서 풀지 않는다. */
function offerBlock(item: ItemRow) {
  return (
    <Callout tone="accent" icon="bot" title={SCOPE_WORD}>
      <Stack gap="xs">
        <Text size="xs">{item.scope}</Text>
        {canStart(item) ? null : (
          <Stack gap="xs">
            <Text size="xs">세션이 잠겨 있습니다. 터미널에서 켠 뒤 새로고침하세요.</Text>
            <Code>kairos sessions on</Code>
          </Stack>
        )}
      </Stack>
    </Callout>
  )
}

/** 지금 카드와 펼친 행이 같이 쓴다. 제안, 주 행동(시작, 이동, 원문), 뒷줄, 제자리 알림의 네 층이고 빈 층은 그리지 않는다. */
function actionZone(item: ItemRow, spot: Spot, key: string) {
  const now = read(pressing)
  if (now?.where === spot.where) return progress(now)
  const busy = isBusy()
  const ids = [item.id]
  const has = (id: ActionId): boolean => hasAction(item.actions, id)
  const when = bundleWord()
  const back = (id: ItemActionId) =>
    has(id) ? (
      <Button key={`${key}-${id}`} size="xs" variant="ghost" tooltip={id === 'later' && when !== null ? `${when}에 다시 보여 드립니다` : undefined} disabled={busy} onPress={() => press(id, ids, spot)}>
        {ACTION_WORDS[id]}
      </Button>
    ) : null
  const url = item.state === 'accepted' || item.state === 'closed' ? null : openableUrl(item.url)
  const primary = [
    canStart(item) ? (
      <Button key={`${key}-start`} size="sm" variant="solid" tone="accent" icon="play" disabled={busy || isOpening(spot.where)} onPress={() => press('start', ids, spot)}>
        {START_WORD}
      </Button>
    ) : null,
    has('move') ? (
      <Button key={`${key}-move`} size="sm" variant="outline" disabled={busy} onPress={() => press('move', ids, spot)}>
        세션으로 이동
      </Button>
    ) : null,
    has('pr') ? (
      <Button key={`${key}-pr`} size="sm" variant="outline" tooltip="세션이 커밋한 브랜치를 push하고 draft PR을 만듭니다" disabled={busy} onPress={() => press('pr', ids, spot)}>
        {ACTION_WORDS.pr}
      </Button>
    ) : null,
    url === null ? null : originalButton(url, spot.where, key),
  ].filter(button => button !== null)
  const left = [back('later'), back('keep'), back('keep_deploy'), back('cancel')].filter(button => button !== null)
  const right = back('done')
  const lifted = primary.length > 0 && left.length === 0 ? right : null
  const backRow = left.length > 0 || (right !== null && lifted === null)
  return (
    <Stack gap="sm">
      {hasOffer(item) ? offerBlock(item) : null}
      {primary.length === 0 ? null : (
        <Stack gap="xs">
          <Row justify="between" align="center" gap="xs">
            <Row gap="xs" wrap>{primary}</Row>
            {lifted === null ? null : <Box shrink={false}>{lifted}</Box>}
          </Row>
          {has('cancel') ? <Text size="xs" muted>되돌려도 세션은 그대로 남습니다.</Text> : null}
        </Stack>
      )}
      {backRow ? (
        <Row justify={left.length === 0 ? 'end' : 'between'} align="center">
          {left.length === 0 ? null : <Row gap="xs" wrap>{left}</Row>}
          {right === null ? null : <Box shrink={false}>{right}</Box>}
        </Row>
      ) : null}
      {placedNotes(spot.where)}
    </Stack>
  )
}

/** 알릴 것도 열 주소도 없으면 null이다. 전체 제목만 보여 주고 빈 층을 그리지 않는다. */
function mergedZone(item: ItemRow, where: string, key: string) {
  const url = openableUrl(item.url)
  const notes = placedNotes(where)
  if (url === null && notes === null) return null
  return (
    <Stack gap="sm">
      {url === null ? null : <Row gap="xs">{originalButton(url, where, key)}</Row>}
      {notes}
    </Stack>
  )
}

function parkedWord(item: ItemRow, at: string | null): string {
  if (item.parkTrigger === 'deploy') return '배포 때 다시'
  if (item.parkTrigger === 'kept') return '새 소식이 오면 다시'
  const when = whenLabel(item.parkedUntil, at)
  return when === null ? '미룸' : `${when}에 다시`
}

function metaRight(item: ItemRow, pending: boolean, at: string | null) {
  if (pending) return <Spinner label="처리 중" labelHidden />
  const section = sectionOf(item)
  if (section === 'running') return null
  if (item.state === 'parked') return <Text size="xs" muted truncate>{parkedWord(item, at)}</Text>
  const due = TIMED_REASONS.has(item.reason) ? whenLabel(item.dueAt, at) : null
  if (due !== null) return <Text size="xs" muted truncate>{due}</Text>
  const carried = item.carried ? carriedWord(item.openedAt, at) : null
  if (carried !== null) return <Text size="xs" weight="medium" truncate>{carried}</Text>
  const opened = openedOf(item)
  return Number.isNaN(opened) ? null : <Text size="xs" muted truncate><RelativeTime at={opened} format="long" muted /></Text>
}

function metaLeft(item: ItemRow, pending: boolean, offerWord: boolean, news: string | null) {
  if (pending) return '처리하는 중입니다'
  const source = item.sourceLabel !== '' && item.sourceLabel !== 'kairos' ? item.sourceLabel : null
  const joined = (words: readonly (string | null)[]): string => words.filter(word => word !== null && word !== '').join(' · ')
  if (sectionOf(item) !== 'running') return joined([item.reasonLabel, source, offerWord ? GROUP_WORDS.offer : null])
  const word = news ?? lookup(RUN_WORDS, item.run?.state ?? '') ?? '세션 상태를 확인해 주세요'
  if (item.state === 'closed') return joined(['완료됨', word])
  if (news === null && !RUN_CALLS.has(item.run?.state ?? '')) return joined([word, source])
  return [<Text key="call" size="xs" weight="medium">{word}</Text>, source === null ? '' : ` · ${source}`]
}

function metaRow(item: ItemRow, pending: boolean, offerWord: boolean, news: string | null, at: string | null) {
  const right = metaRight(item, pending, at)
  return (
    <Row justify="between" align="center" gap="sm">
      <Text size="xs" muted truncate>{metaLeft(item, pending, offerWord, news)}</Text>
      {right === null ? null : <Box shrink={false}>{right}</Box>}
    </Row>
  )
}

/** 세션은 사유보다 세션이라는 사실이 먼저다. 모르는 사유는 일반 아이콘을 쓴다. */
function rowIcon(item: ItemRow): IconName {
  return sectionOf(item) === 'running' ? 'terminal' : (lookup(REASON_ICONS, item.reason) ?? 'bell')
}

/** 기다려도 되는 일의 한 줄. 사유는 아이콘에 맡기고 펼치면 글자로 나온다. */
function compactLine(item: ItemRow, pending: boolean, at: string | null) {
  const right = metaRight(item, pending, at)
  return (
    <Row justify="between" align="center" gap="sm">
      <Text truncate>{item.title}</Text>
      {right === null ? null : <Box shrink={false}>{right}</Box>}
    </Row>
  )
}

function detailLine(item: ItemRow) {
  const lines = [
    item.deploy === null ? null : `${dayLabel(item.deploy.date)} 배포에 연결되어 있습니다`,
    item.pushCount > 0 ? `${item.pushCount}번 미뤘습니다` : null,
  ].filter(line => line !== null)
  return lines.length === 0 ? null : <Text size="xs" muted>{lines.join(' · ')}</Text>
}

function groupLabel(label: string, tone: 'muted' | 'danger' | 'accent') {
  return (
    <Heading level={3}>
      <Text size="xs" weight="semibold" muted={tone === 'muted'} tone={tone === 'muted' ? undefined : tone}>{label}</Text>
    </Heading>
  )
}

/** 접힌 행과 펼친 행이 같은 틀과 같은 key를 쓴다. 눌린 ListItem이 다시 만들어지지 않아 포커스가 남는다. */
function rowView(item: ItemRow, place: RowPlace, news: NewsWords, at: string | null) {
  const where = `row-${item.id}`
  const spot: Spot = { where, group: place.group, rowId: item.id }
  const open = read(openRow) === item.id
  const pending = read(pressing)?.where === where
  const boxed = open && !place.nested
  const word = news.get(item.id) ?? null
  // 제안 묶음의 머리글과 펼친 행에는 이미 같은 말이 있다. 카드 안의 줄은 머리글 아래가 아니라서 낱말을 쓴다.
  const offerWord = !open && hasOffer(item) && (place.nested || sectionOf(item) !== 'offer')
  // 소식이 떠 있으면 이동은 소식에 있고, 끝난 세션의 다음 할 일은 펼쳐서 고른다.
  const quickMove = place.group === 'running' && !open && !pending && word === null && item.run?.state !== 'ended' && hasAction(item.actions, 'move')
  const zone = open ? (place.merged ? mergedZone(item, where, where) : actionZone(item, spot, where)) : null
  const detail = open ? detailLine(item) : null
  // 카드 안의 접힌 줄은 이미 묶인 일이라 아이콘 없이 한 줄 제목을 쓴다.
  const oneLine = place.nested && !open
  const compact = !open && !place.nested && isCompact(sectionOf(item))
  return (
    <Box key={where} direction="column" radius="md" background={boxed ? 'card' : 'none'} border={boxed}>
      <Box direction="row" align="center" gap="xs">
        <Box grow>
          <List>
            <ListItem
              key={`${where}-item`}
              title={oneLine ? item.title : undefined}
              icon={place.nested ? undefined : rowIcon(item)}
              current={open && place.nested}
              onPress={() => toggleRow(item.id)}
            >
              {compact ? compactLine(item, pending, at) : null}
              {compact || oneLine ? null : <Text>{clip(item.title, open ? OPEN_TITLE_LIMIT : ROW_TITLE_LIMIT)}</Text>}
              {compact ? null : metaRow(item, pending && !open, offerWord, word, at)}
            </ListItem>
          </List>
        </Box>
        {quickMove ? (
          <Box shrink={false}>
            <Button key={`${where}-quick`} size="xs" variant="ghost" tooltip="세션으로 이동" disabled={isBusy()} onPress={() => press('move', [item.id], spot)}>
              이동
            </Button>
          </Box>
        ) : null}
      </Box>
      {zone === null && detail === null ? null : (
        <Box direction="column" gap="sm" paddingX="sm" paddingY="sm">
          {detail}
          {zone}
        </Box>
      )}
      {open && place.nested ? <Separator /> : null}
    </Box>
  )
}

/** 카드는 kairos가 고른 한 건이고 목록의 최신순과 별개다. 맨 위 줄이 왜 오래됐는지 낱말로 알린다. */
function eyebrowOf(card: CardView, running: boolean, ended: boolean): string {
  if (card.kind === 'bundle') return `모아 둔 일 ${card.items.length + card.more}건`
  if (card.kind === 'carryover') return `어제까지 남은 일 ${card.items.length + card.more}건`
  if (!isSingle(card)) return `같은 채널 ${card.itemIds.length}건`
  if (running) return ended ? '끝난 세션' : '진행 중'
  return card.tier === 'P0' ? '긴급 · 먼저 볼 일' : '먼저 볼 일'
}

function mergedEnd(card: CardView, spot: Spot) {
  const count = card.itemIds.length
  const busy = isBusy()
  return (
    <Row justify={hasAction(card.actions, 'later') ? 'between' : 'end'} align="center" gap="sm">
      {hasAction(card.actions, 'later') ? (
        <Button key="card-later" size="xs" variant="ghost" disabled={busy} onPress={() => press('later', card.itemIds, spot)}>
          {`${count}건 모두 나중에`}
        </Button>
      ) : null}
      {hasAction(card.actions, 'done') ? (
        <Box shrink={false}>
          <Button key="card-done" size="xs" variant="ghost" disabled={busy} onPress={() => press('done', card.itemIds, spot)}>
            {`${count}건 모두 완료`}
          </Button>
        </Box>
      ) : null}
    </Row>
  )
}

function ackEnd(card: CardView, spot: Spot) {
  const remains = card.kind === 'bundle' && card.more > 0
  return (
    <Row justify={remains ? 'between' : 'end'} align="center" gap="sm">
      {remains ? <Text size="xs" muted>{`나머지 ${card.more}건은 아래 목록에 있습니다.`}</Text> : null}
      {hasAction(card.actions, 'ack') ? (
        <Box shrink={false}>
          <Button key="card-ack" size="xs" variant="outline" disabled={isBusy()} onPress={() => press('ack', [], spot)}>
            {ACTION_WORDS.ack}
          </Button>
        </Box>
      ) : null}
    </Row>
  )
}

function cardView(card: CardView, news: NewsWords, at: string | null) {
  const [first] = card.items
  if (first === undefined) return null
  const single = isSingle(card)
  const running = single && sectionOf(first) === 'running'
  const urgent = card.tier === 'P0' && !running
  const spot: Spot = { where: `card-${card.id}`, group: NOW_GROUP, rowId: null }
  const now = read(pressing)
  const pending = now?.where === spot.where ? now : null
  const place: RowPlace = { group: NOW_GROUP, nested: true, merged: card.kind === 'item' }
  return (
    <Box background="card" border={urgent ? 'danger' : true} radius="md" paddingY="md" direction="column" gap="sm">
      <Box paddingX="sm">{groupLabel(eyebrowOf(card, running, first.run?.state === 'ended'), urgent ? 'danger' : 'muted')}</Box>
      {single ? (
        <Box paddingX="sm" direction="column" gap="xs">
          <Text weight="medium">{clip(first.title, 120)}</Text>
          {metaRow(first, false, false, news.get(first.id) ?? null, at)}
          {detailLine(first)}
        </Box>
      ) : (
        <Stack gap="xs">{card.items.map(item => rowView(item, place, news, at))}</Stack>
      )}
      {single ? (
        <Box paddingX="sm">{actionZone(first, spot, 'card')}</Box>
      ) : (
        <Box paddingX="sm" direction="column" gap="xs">
          {pending !== null ? progress(pending) : card.kind === 'item' ? mergedEnd(card, spot) : ackEnd(card, spot)}
          {placedNotes(spot.where)}
        </Box>
      )}
    </Box>
  )
}

function restartBlock(card: ReadyView, row: ItemRow, spot: Spot) {
  return (
    <Stack gap="xs">
      <Stack gap="none">
        <Text size="xs" weight="medium">{SCOPE_WORD}</Text>
        <Text size="xs">{row.scope}</Text>
      </Stack>
      <Row>
        <Button key={`ready-${card.runId}-start`} size="xs" variant="solid" tone="accent" icon="play" disabled={isBusy() || isOpening(spot.where)} onPress={() => press('start', [card.itemId], spot)}>
          다시 시작
        </Button>
      </Row>
    </Stack>
  )
}

/** 실패 상자의 글자에는 muted를 쓰지 않는다. 라이트에서 이 바탕 위의 흐린 글자는 대비가 모자란다. */
function failedBox(card: ReadyView, row: ItemRow | null, gateOpen: boolean) {
  const spot: Spot = { where: `ready-${card.runId}`, group: NOW_GROUP, rowId: null }
  const now = read(pressing)
  return (
    <Box key={`ready-${card.runId}`} background="danger" radius="md" direction="column" gap="xs" paddingX="sm" paddingY="sm">
      <Row justify="between" align="start" gap="sm">
        <Text weight="medium">{card.headline}</Text>
        <Box shrink={false}>
          <IconButton icon="close" label="닫기" size="xs" variant="ghost" onPress={() => dropReady(other => other.runId !== card.runId)} />
        </Box>
      </Row>
      <Text size="xs" truncate>{card.title}</Text>
      <Text size="xs">{gateOpen ? '이 일은 목록으로 돌아왔습니다.' : '제안이 다시 보이면 목록에서 찾을 수 있습니다.'}</Text>
      {now?.where === spot.where ? progress(now) : row === null ? null : restartBlock(card, row, spot)}
      {placedNotes(spot.where)}
    </Box>
  )
}

function alertLine(card: ReadyView) {
  const spot: Spot = { where: `ready-${card.runId}`, group: NOW_GROUP, rowId: null }
  const pending = read(pressing)?.where === spot.where
  return (
    <Box key={spot.where} direction="column" gap="xs" paddingX="sm" paddingY="xs">
      <Text truncate>{card.title}</Text>
      <Row justify="between" align="center" gap="sm">
        <Text size="xs" weight="medium" truncate>{lookup(READY_WORDS, card.kind) ?? card.headline}</Text>
        {pending ? (
          <Spinner label="처리 중" labelHidden />
        ) : (
          <Box direction="row" gap="xs" shrink={false}>
            {hasAction(card.actions, 'move') ? (
              <Button key={`${spot.where}-move`} size="xs" variant="outline" disabled={isBusy()} onPress={() => press('move', [card.itemId], spot)}>
                세션으로 이동
              </Button>
            ) : null}
            {hasAction(card.actions, 'pr') ? (
              <Button key={`${spot.where}-pr`} size="xs" variant="outline" disabled={isBusy()} onPress={() => press('pr', [card.itemId], spot)}>
                {ACTION_WORDS.pr}
              </Button>
            ) : null}
            <IconButton icon="close" label="닫기" size="xs" variant="ghost" onPress={() => dropReady(other => other.runId !== card.runId)} />
          </Box>
        )}
      </Row>
      {placedNotes(spot.where)}
    </Box>
  )
}

function alertsZone(cards: readonly ReadyView[], total: number, list: ListView | null, gateOpen: boolean) {
  if (total === 0) return null
  const restartable = (card: ReadyView): ItemRow | null => (gateOpen ? (list?.items.find(item => item.id === card.itemId && canStart(item)) ?? null) : null)
  const failed = cards.filter(card => card.kind === 'failed')
  const others = cards.filter(card => card.kind !== 'failed')
  const opened = read(alertsOpen)
  return (
    <Stack gap="xs">
      {failed.map(card => failedBox(card, restartable(card), gateOpen))}
      {others.length === 0 ? null : (
        <Box background="card" border radius="md" direction="column">
          {others.flatMap((card, index) => (index === 0 ? [alertLine(card)] : [<Separator key={`line-${card.runId}`} />, alertLine(card)]))}
        </Box>
      )}
      {total > ALERT_LIMIT ? (
        <Button size="xs" variant="ghost" onPress={() => set(alertsOpen, !opened)}>
          {opened ? '세션 소식 접기' : `세션 소식 ${total - ALERT_LIMIT}건 더`}
        </Button>
      ) : null}
    </Stack>
  )
}

/** 알림을 눌러 연 일. 카드와 같은 틀에 제안과 같은 accent를 쓴다. 줄은 제 묶음에서 빼 와 두 번 그리지 않는다. */
function revealZone(target: RevealView, rows: readonly ItemRow[], news: NewsWords, at: string | null) {
  return (
    <Box key="reveal" background="card" border="accent" radius="md" paddingY="md" direction="column" gap="sm">
      <Box paddingX="sm" direction="row" justify="between" align="center" gap="sm">
        <Box shrink={false}>{groupLabel(REVEAL_WORD, 'accent')}</Box>
        <Row gap="xs" align="center" justify="end">
          <Text size="xs" muted><RelativeTime at={target.at} format="long" muted /></Text>
          <IconButton icon="close" label={`${REVEAL_WORD} 닫기`} tooltip="닫기" size="xs" variant="ghost" onPress={() => closeReveal()} />
        </Row>
      </Box>
      <Stack gap="xs">{rows.map(row => rowView(row, { group: sectionOf(row), nested: true, merged: false }, news, at))}</Stack>
    </Box>
  )
}

function gateView(gate: GateView, hidden: number, at: string | null) {
  switch (gate.reason) {
    case 'recording': {
      const starts = whenLabel(gate.startsAt, at)
      return (
        <EmptyState icon="layers" title="지금은 기록만 하고 있습니다" description={starts === null ? '기록이 끝나면 제안을 보여 드립니다' : `${starts}부터 제안을 보여 드립니다`}>
          {hidden > 0 ? <Text size="xs" muted>{`모아 둔 일 ${hidden}건`}</Text> : null}
        </EmptyState>
      )
    }
    case 'no_collection':
      return <EmptyState icon="layers" title="아직 수집 전입니다" description="첫 수집이 끝나면 기록을 시작합니다" />
    case 'paused':
      return (
        <EmptyState icon="pause" title="잠시 멈춰 두었습니다" description="다시 켜는 명령">
          <Code>kairos pause --resume</Code>
        </EmptyState>
      )
    case 'display_off':
      return (
        <EmptyState icon="pause" title="제안을 꺼 두었습니다" description="수집은 계속 돌고 있습니다">
          <Button size="xs" variant="outline" disabled={isBusy()} onPress={() => press('display_auto', [], HEAD_SPOT)}>
            {ACTION_WORDS.display_auto}
          </Button>
        </EmptyState>
      )
    default:
      return <EmptyState icon="layers" title="제안이 잠시 멈춰 있습니다" description={gate.message ?? undefined} />
  }
}

const isFocused = (next: NextView): boolean => next.focus.active || next.hold?.reason === 'focus'

function emptyView(next: NextView) {
  // 포커스는 띠가 이미 말하고 있다.
  if (next.hold !== null && next.hold.reason !== 'nothing' && !isFocused(next)) {
    const [title, description] = holdSentence(next.hold, next.at)
    return <EmptyState icon="pause" title={title} description={description ?? undefined} />
  }
  if (next.health !== null) return <EmptyState icon="info" title="확인된 일은 없습니다" />
  return <EmptyState icon="check" title="지금은 챙길 일이 없습니다" />
}

/** 카드가 없고 목록이 있을 때의 안내. 먼저 볼 일이 없다는 말은 알려 주는 것이 없어 그리지 않는다. 포커스는 띠에 있다. */
function holdNote(next: NextView) {
  const told = next.hold !== null && next.hold.reason !== 'nothing' && !isFocused(next)
  const held = next.focus.heldLine !== null && !isFocused(next)
  if (!told && !held) return null
  return (
    <Stack gap="xs">
      {told ? <Box paddingX="sm"><Text size="xs" muted>{holdLine(next.hold, next.at)}</Text></Box> : null}
      {held ? <Box paddingX="sm"><Text size="xs" muted>{`포커스 중 쌓인 일 ${next.focus.heldP1}건`}</Text></Box> : null}
    </Stack>
  )
}

/** 포커스는 패널이 켜는 상태라 켠 자리에서 끌 수 있어야 한다. 카드가 있어도 보인다. 긴급은 포커스를 넘어 오기 때문이다. 회색 바탕이라 muted를 쓰지 않는다. */
function focusBar(next: NextView) {
  const until = whenLabel(next.focus.until ?? next.hold?.until ?? null, next.at)
  const pending = read(pressing)?.where === HEAD_SPOT.where
  return (
    <Box background="muted" radius="md" direction="row" justify="between" align="center" gap="sm" paddingX="sm" paddingY="xs">
      <Stack gap="none">
        <Text size="xs" weight="medium">{until === null ? '포커스 중' : `포커스 중 · ${until}까지`}</Text>
        <Text size="xs">{next.focus.heldP1 > 0 ? `쌓인 일 ${next.focus.heldP1}건` : '끝나면 모아서 보여 드립니다'}</Text>
      </Stack>
      <Box shrink={false}>
        {pending ? (
          <Spinner label="처리 중" labelHidden />
        ) : (
          <Button size="xs" variant="ghost" disabled={isBusy()} onPress={() => press('focus_end', [], HEAD_SPOT)}>
            {ACTION_WORDS.focus_end}
          </Button>
        )}
      </Box>
    </Box>
  )
}

function nowZone(state: Reading, card: CardView | null, rows: number, unsure: boolean, news: NewsWords, at: string | null) {
  const { next, list } = state
  if (next === null) return null
  if (!next.display.open) return gateView(next.display, list?.hidden ?? 0, next.at)
  if (card !== null) return cardView(card, news, at)
  if (rows > 0) return holdNote(next)
  return unsure ? null : emptyView(next)
}

const isSectionOpen = (key: Section, groups: Record<Section, ItemRow[]>): boolean => {
  const fallback = defaultOpen(key, groups)
  return fallback === undefined ? true : (read(folds)[key] ?? fallback)
}

/** 고정 머리글과 접는 머리글이 같은 틀이라 높이가 같다. 오른쪽 칸은 접기 낱말이고, 성공 자취도 잠시 여기에 쓴다. */
function groupHead(key: Section, count: number, open: boolean, onFold: (() => void) | null, note: string | null) {
  const right = note ?? (onFold === null ? null : open ? '접기' : '펼치기')
  const body = (
    <Row justify="between" align="center" gap="sm">
      <Box shrink={false}>{groupLabel(`${GROUP_WORDS[key]} ${count}`, HEAD_TONES[key] ?? 'muted')}</Box>
      {right === null ? null : <Text size="xs" muted truncate>{right}</Text>}
    </Row>
  )
  return (
    <List key="head">
      {onFold === null ? <ListItem>{body}</ListItem> : <ListItem onPress={onFold}>{body}</ListItem>}
    </List>
  )
}

function moreLine(key: Section, hidden: number) {
  return (
    <List key="more">
      <ListItem onPress={() => toggleMore(key)}>
        <Text size="xs" muted>{hidden > 0 ? `${hidden}건 더 보기` : '최신 줄만 보기'}</Text>
      </ListItem>
    </List>
  )
}

function shownRowsOf(key: Section, rows: readonly ItemRow[]): readonly ItemRow[] {
  const opened = rows.findIndex(row => row.id === read(openRow))
  return rows.slice(0, visibleCount(key, rows.length, read(folds)[moreKey(key)] === true, opened))
}

function sectionView(key: Section, groups: Record<Section, ItemRow[]>, omittedBy: Record<string, number>, news: NewsWords, at: string | null) {
  const rows = groups[key]
  if (rows.length === 0) return null
  const open = isSectionOpen(key, groups)
  const shown = open ? shownRowsOf(key, rows) : []
  const hidden = rows.length - shown.length
  // 다 펼친 상태에서도 줄일 길이 있어야 한다. 줄일 것이 없는 묶음은 줄을 그리지 않는다.
  const reducible = visibleCount(key, rows.length, false, -1) < rows.length
  const mark = read(trace)
  const onFold = defaultOpen(key, groups) === undefined ? null : () => toggleFold(key, open, rows)
  // 줄 사이 간격은 묶음 안쪽에만 둔다. 머리글은 첫 줄에 붙어 있어야 어느 묶음인지 읽힌다.
  return (
    <Stack key={key} gap="none">
      {groupHead(key, rows.length + (omittedBy[key] ?? 0), open, onFold, mark?.group === key ? mark.text : null)}
      {open ? (
        <Stack gap={isCompact(key) ? 'none' : 'xs'}>
          {shown.map(row => rowView(row, { group: key, nested: false, merged: false }, news, at))}
          {reducible ? moreLine(key, hidden) : null}
        </Stack>
      ) : null}
    </Stack>
  )
}

function troubleOf(state: Reading, bridge: Failure | null): Trouble | null {
  const times = [state.nextAt, state.listAt].filter(time => time !== null)
  const latest = times.length === 0 ? null : Math.max(...times)
  if (bridge !== null) return { ...failureCopy(bridge.code === 'timeout' ? 'timeout' : 'unavailable', bridge.message), code: bridge.code, readAt: latest }
  const { error, nextStale, listStale } = state
  if (error === null) return null
  if (listStale && !nextStale) return { title: '목록을 읽지 못했습니다', body: '지금 볼 일은 최신입니다.', code: error.code, readAt: state.listAt }
  if (nextStale && !listStale) return { title: '지금 볼 일을 읽지 못했습니다', body: '목록은 최신입니다.', code: error.code, readAt: state.nextAt }
  return { ...failureCopy(error.code, error.message), code: error.code, readAt: latest }
}

function troubleView(trouble: Trouble) {
  return (
    <Callout key="trouble" tone="danger" icon="error" title={trouble.title}>
      <Stack gap="xs">
        <Text size="xs">{trouble.body}</Text>
        {trouble.readAt === null ? null : <Text size="xs" muted>마지막으로 읽은 시각 · <RelativeTime at={trouble.readAt} format="long" muted /></Text>}
        <Row justify="between" align="center">
          <Button size="xs" variant="outline" disabled={isBusy() || read(refreshing)} onPress={() => refresh()}>다시 시도</Button>
          <Text size="xs" muted>{`코드 ${trouble.code}`}</Text>
        </Row>
      </Stack>
    </Callout>
  )
}

/** 누른 자리를 잃은 알림. 다른 일로 가려지지 않게 위에 띄우고 사용자가 닫을 수 있다. */
function floatingNotice(value: Notice) {
  const failed = value.kind === 'failed'
  return (
    <Callout key="notice" tone={failed ? 'danger' : 'neutral'} icon={failed ? 'error' : 'info'} title={value.title}>
      <Stack gap="xs">
        {value.message === null ? null : <Text size="xs">{value.message}</Text>}
        <Row justify={value.code === null ? 'end' : 'between'} align="center">
          {value.code === null ? null : <Text size="xs" muted>{`코드 ${value.code}`}</Text>}
          <Button size="xs" variant="ghost" onPress={() => set(notice, null)}>닫기</Button>
        </Row>
      </Stack>
    </Callout>
  )
}

function healthLine(health: string) {
  const tail = health === SESSION_WARNING ? '세션 표시가 늦을 수 있습니다' : '이 출처의 새 일은 늦게 보일 수 있습니다'
  return (
    <Box key="health" paddingX="sm">
      <Text size="xs" muted><Text size="xs" tone="warning">{health}</Text>{` · ${tail}`}</Text>
    </Box>
  )
}

/** 남은 일의 모양을 한 줄로 쓴다. 0인 값은 쓰지 않는다. 제안은 시작할 수 있는 일만 센다. */
function overview(next: NextView, offers: number) {
  const badge = badgeOf(next)
  if (badge === 0) return null
  const rest = [
    next.counts.carried > 0 ? `밀린 일 ${next.counts.carried}` : null,
    offers > 0 ? `제안 ${offers}` : null,
  ].filter(word => word !== null)
  return (
    <Box paddingX="sm">
      <Text size="xs" muted>
        {`${badge}건`}
        {next.counts.P0 > 0 ? ' · ' : ''}
        {next.counts.P0 > 0 ? <Text size="xs" tone="danger" weight="medium">{`긴급 ${next.counts.P0}`}</Text> : null}
        {rest.map(word => ` · ${word}`).join('')}
      </Text>
    </Box>
  )
}

/** 머리글 도구. 게이트가 닫혀 있으면 포커스는 뜻이 없어 그리지 않고, 포커스 중이면 띠에서 끈다. */
function header(next: NextView | null, note: string | null, offers: number) {
  const counted = next?.display.open ? next : null
  const busy = isBusy() || read(refreshing)
  const opened = read(tool)
  return (
    <Stack gap="none">
      <Box direction="row" paddingX="sm" justify="between" align="center" gap="sm">
        <Box direction="row" align="center" gap="sm" shrink={false}>
          <Heading level={2}>kairos</Heading>
          {busy ? <Spinner label="처리 중" labelHidden /> : null}
        </Box>
        <Row gap="xs" align="center" justify="end">
          {note === null ? null : <Text size="xs" muted truncate>{note}</Text>}
          <Box direction="row" gap="xs" shrink={false}>
            {counted === null || isFocused(counted) ? null : (
              <Button size="xs" variant="ghost" tooltip={opened === 'focus' ? '포커스 고르기 닫기' : '알림을 모아 두고 집중합니다'} onPress={() => toggleTool('focus')}>
                포커스
              </Button>
            )}
            <IconButton icon="add" label={opened === 'add' ? '할 일 추가 닫기' : '할 일 추가'} size="xs" variant="ghost" onPress={() => toggleTool('add')} />
            <IconButton icon="refresh" label="새로고침" size="xs" variant="ghost" disabled={busy} onPress={() => refresh()} />
          </Box>
        </Row>
      </Box>
      {counted === null ? null : overview(counted, offers)}
    </Stack>
  )
}

function focusTool() {
  return (
    <Box key="focus" direction="row" align="center" justify="between" gap="sm" paddingX="sm">
      <Text size="xs" muted>얼마나 집중할까요</Text>
      <Row gap="xs">
        {FOCUS_CHOICES.map(action => (
          <Button key={action} size="xs" variant="outline" disabled={isBusy()} onPress={() => press(action, [], HEAD_SPOT)}>
            {ACTION_WORDS[action]}
          </Button>
        ))}
      </Row>
    </Box>
  )
}

function addTool() {
  const empty = cleanText(read(draft), ADD_TITLE_LIMIT) === ''
  return (
    <Box key="add" direction="row" align="center" gap="xs" paddingX="sm">
      <Box grow>
        <Input label="할 일 제목" labelHidden placeholder="할 일을 적고 Enter" maxLength={ADD_TITLE_LIMIT} value={read(draft)} onChange={(value: string) => set(draft, value)} onSubmit={() => submitDraft()} />
      </Box>
      <Box shrink={false}>
        <Button size="xs" variant="outline" disabled={isBusy() || empty} onPress={() => submitDraft()}>
          {ACTION_WORDS.add}
        </Button>
      </Box>
    </Box>
  )
}

/** 고장이 없을 때의 믿음 신호. 지연이 있으면 위쪽 경고가 말하므로 시각만 남긴다. */
function trustLine(state: Reading, health: string | null) {
  if (state.nextAt === null) return null
  return (
    <Box paddingX="sm">
      <Text size="xs" muted>
        {health === null ? '수집 지연 없음 · ' : '마지막 확인 · '}
        <RelativeTime at={state.nextAt} format="long" muted />
        {health === null ? ' 확인' : ''}
      </Text>
    </Box>
  )
}

/** 누른 자리가 화면에 남아 있는가. 없으면 실패를 믿음 구간으로 띄운다. */
function isPlaced(value: Notice, card: CardView | null, rows: ReadonlySet<number>, alerts: readonly ReadyView[]): boolean {
  if (value.where.startsWith('ready-')) return alerts.some(other => `ready-${other.runId}` === value.where)
  if (value.where.startsWith('row-')) {
    const opened = read(openRow)
    return opened !== null && value.where === `row-${opened}` && rows.has(opened)
  }
  return card !== null && value.where === `card-${card.id}`
}

export default function Panel() {
  const visible = context ? read(context.visible) : false
  // 다시 보일 때 읽는다. 그리는 도중에는 상태를 바꿀 수 없어 미룬다.
  if (visible && !wasVisible) queueMicrotask(() => void load(true))
  wasVisible = visible

  const state = read(reading)
  const { next, list } = state
  const gateOpen = next?.display.open === true
  const card = gateOpen ? next.card : null
  const trouble = troubleOf(state, read(bridgeError))
  const at = list?.at ?? next?.at ?? null

  const listed = listedRows(state)
  const shownReveal = read(reveal)
  const revealed = new Set(shownReveal?.items ?? [])
  const lifted = listed.filter(item => revealed.has(item.id))
  const groups = groupRows(lifted.length === 0 ? listed : listed.filter(item => !revealed.has(item.id)))

  const alerts = sortReady(read(readyCards).filter(other => gateOpen || other.kind === 'failed'))
  const shownAlerts = read(alertsOpen) ? alerts : alerts.slice(0, ALERT_LIMIT)
  const news: NewsWords = new Map(shownAlerts.filter(other => other.kind !== 'failed').map(other => [other.itemId, lookup(READY_WORDS, other.kind) ?? other.headline]))

  const shownRows = new Set([
    ...lifted.map(item => item.id),
    ...(card !== null && !isSingle(card) ? card.items : []).map(item => item.id),
    ...DISPLAY_ORDER.flatMap(key => (isSectionOpen(key, groups) ? shownRowsOf(key, groups[key]).map(item => item.id) : [])),
  ])
  const failed = read(notice)
  const floating = failed !== null && !isPlaced(failed, card, shownRows, shownAlerts) ? failed : null

  // 자취를 쓸 머리글이 없으면 머리말에 쓴다.
  const mark = read(trace)
  const headNote = mark !== null && (mark.group === NOW_GROUP || groups[mark.group as Section]?.length === 0) ? mark.text : null

  const health = gateOpen ? next.health : null
  const hasTrust = trouble !== null || floating !== null || health !== null
  const offers = [...listed, ...(card?.items ?? [])].filter(canStart).length
  const opened = read(tool)
  const unsure = trouble !== null || state.nextStale || state.listStale || list === null
  const omitted = next?.display.open === false ? 0 : (list?.omitted ?? 0)

  return (
    <Stack gap="md">
      {header(read(heldSnapshot)?.next ?? next, headNote, offers)}
      {opened === 'focus' && gateOpen && !isFocused(next) ? focusTool() : null}
      {opened === 'add' ? addTool() : null}
      {hasTrust ? (
        <Stack gap="xs">
          {trouble === null ? null : troubleView(trouble)}
          {floating === null ? null : floatingNotice(floating)}
          {health === null ? null : healthLine(health)}
        </Stack>
      ) : null}
      {gateOpen && isFocused(next) ? focusBar(next) : null}
      {alertsZone(shownAlerts, alerts.length, list, gateOpen)}
      {shownReveal === null || lifted.length === 0 ? null : revealZone(shownReveal, lifted, news, at)}
      {next === null && list === null && trouble === null ? <Skeleton shape="text" lines={4} label="불러오는 중" /> : null}
      {nowZone(state, card, listed.length, unsure, news, at)}
      {DISPLAY_ORDER.map(key => sectionView(key, groups, list?.omittedBy ?? {}, news, at))}
      {omitted > 0 ? <Box paddingX="sm"><Text size="xs" muted>{`목록이 길어 ${omitted}건은 여기에 나오지 않습니다.`}</Text></Box> : null}
      {gateOpen && trouble === null ? trustLine(state, health) : null}
    </Stack>
  )
}
