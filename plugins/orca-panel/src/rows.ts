/** 목록 줄의 묶음과 순서를 정하는 순수 함수. 패널과 워커가 같이 쓰므로 node 모듈을 쓰지 않는다. */
import type { ItemRow } from './protocol.ts'

export type Section = 'running' | 'urgent' | 'offer' | 'today' | 'relaxed' | 'parked' | 'baseline'

/** 자르기 우선순위이기도 하다. 세션이 있는 줄은 잘리면 패널에서 그 세션으로 갈 길이 없어 맨 앞에 둔다. */
const SORT_ORDER: readonly Section[] = ['running', 'urgent', 'offer', 'today', 'relaxed', 'parked', 'baseline']

/** 여유 있을 때가 이보다 길면 미룬 일을 접어 둔다. */
const FOLD_FROM = 6
/** 한 묶음에서 먼저 보이는 줄 수. 최신이 위라서 여기까지가 최신 줄이다. */
export const ROW_LIMIT = 8
/** 숨길 줄이 이보다 적으면 더 보기 줄이 오히려 자리를 쓰므로 다 보여 준다. */
const MORE_FROM = 3
/** 목록을 자를 때 묶음마다 남기는 줄 수. 뒤쪽 묶음이 통째로 사라지지 않게 한다. */
const TRIM_FLOOR = 10

/** kairos가 세션을 제안한 줄. 범위 없이는 제안을 그리지 않는다. */
export const hasOffer = (item: ItemRow): boolean => item.playbook !== null && item.scope !== null

/** 제안은 등급보다 앞에서 묶는다. 여유 있을 때 묶음은 기본으로 접혀 있어 그 안에 두면 제안이 보이지 않는다. 긴급만 자리를 지킨다. */
export function sectionOf(item: ItemRow): Section {
  if (item.run !== null || item.state === 'accepted') return 'running'
  if (item.state === 'parked') return 'parked'
  if (item.baseline) return 'baseline'
  if (item.tier === 'P0') return 'urgent'
  if (hasOffer(item)) return 'offer'
  return item.tier === 'P1' ? 'today' : 'relaxed'
}

/** 시각을 못 읽으면 NaN 대신 가장 오래된 값으로 친다. 같은 묶음의 맨 뒤로 간다. */
function openedMs(item: ItemRow): number {
  const ms = item.openedAt === null ? Number.NaN : Date.parse(item.openedAt)
  return Number.isNaN(ms) ? Number.NEGATIVE_INFINITY : ms
}

function newerFirst(a: ItemRow, b: ItemRow): number {
  const [left, right] = [openedMs(a), openedMs(b)]
  return left === right ? b.id - a.id : right > left ? 1 : -1
}

/** 묶음 순서가 먼저이고 같은 묶음 안에서는 최신순이다. 같은 시각이면 ID가 큰 쪽이 위다. */
export function sortRows(rows: readonly ItemRow[]): ItemRow[] {
  const rank = (item: ItemRow): number => SORT_ORDER.indexOf(sectionOf(item))
  return [...rows].sort((a, b) => rank(a) - rank(b) || newerFirst(a, b))
}

/** 묶음마다 최소 줄을 남기고 나머지는 정렬 순서대로 채운다. rows는 sortRows를 지난 값이다. */
export function trimRows(rows: readonly ItemRow[], limit: number): ItemRow[] {
  if (rows.length <= limit) return [...rows]
  const kept = new Set<ItemRow>()
  const taken = new Map<Section, number>()
  for (const row of rows) {
    const section = sectionOf(row)
    const count = taken.get(section) ?? 0
    if (count >= TRIM_FLOOR) continue
    taken.set(section, count + 1)
    kept.add(row)
  }
  for (const row of rows) {
    if (kept.size >= limit) break
    kept.add(row)
  }
  return rows.filter(row => kept.has(row))
}

export function groupRows(rows: readonly ItemRow[]): Record<Section, ItemRow[]> {
  const groups: Record<Section, ItemRow[]> = { running: [], urgent: [], offer: [], today: [], relaxed: [], parked: [], baseline: [] }
  for (const row of rows) groups[sectionOf(row)].push(row)
  return groups
}

/** 사용자가 고른 값이 없을 때의 펼침 상태. undefined이면 접는 머리글이 없는 묶음이다. */
export function defaultOpen(section: Section, groups: Record<Section, readonly ItemRow[]>): boolean | undefined {
  if (section === 'baseline') return false
  // 대기 탭의 주인은 여유 있을 때다. 그 묶음이 길면 미룬 일은 접어 둔다.
  if (section === 'parked') return groups.relaxed.length < FOLD_FROM
  return undefined
}

/** 진행 상태 탭. 지금은 손댈 일, 대기는 기다려도 되는 일, 세션은 맡긴 일이다. 프로바이더를 고른 뒤 그 안에서 고른다. */
export type TabId = 'now' | 'waiting' | 'sessions'

export const TAB_ORDER: readonly TabId[] = ['now', 'waiting', 'sessions']

/** 탭마다 그리는 묶음. 순서가 화면 순서다. 긴급과 제안은 놓치면 안 되므로 지금에 둔다. */
export const TAB_SECTIONS: Record<TabId, readonly Section[]> = {
  now: ['urgent', 'offer', 'today'],
  waiting: ['relaxed', 'parked', 'baseline'],
  sessions: ['running'],
}

/** 일이 온 곳. 수집 소스와 같은 이름이고, 직접 추가한 일과 모르는 소스는 따로 둔다. all은 모두다. */
export type ProviderId = 'all' | 'github' | 'slack' | 'calendar' | 'gmail' | 'notion' | 'manual' | 'other'

/** 화면 순서. 자리가 바뀌지 않아야 손이 기억한다. */
export const PROVIDER_ORDER: readonly ProviderId[] = ['all', 'github', 'slack', 'calendar', 'gmail', 'notion', 'manual', 'other']
const SOURCE_PROVIDERS: ReadonlySet<string> = new Set(['github', 'slack', 'calendar', 'gmail', 'notion'])

export const isProviderId = (value: unknown): value is ProviderId => typeof value === 'string' && (PROVIDER_ORDER as readonly string[]).includes(value)

export function providerOf(item: ItemRow): ProviderId {
  if (item.source !== null && SOURCE_PROVIDERS.has(item.source)) return item.source as ProviderId
  return item.reason === 'manual' && item.source === null ? 'manual' : 'other'
}

export const inProvider = (item: ItemRow, provider: ProviderId): boolean => provider === 'all' || providerOf(item) === provider

/**
 * 고를 수 있게 늘어놓을 프로바이더. 연결된 소스는 일이 없어도 둔다. 일이 오갈 때마다 자리가 바뀌면 손이 헛짚는다.
 * 직접 추가와 기타는 일이 있을 때만 둔다. 연결 상태는 kairos가 준 소스 목록에서 읽는다.
 */
export function providersOf(rows: readonly ItemRow[], sources: readonly { source: string; status: string }[]): ProviderId[] {
  const present = new Set<ProviderId>(rows.map(providerOf))
  for (const row of sources) if (row.status !== 'not_connected' && SOURCE_PROVIDERS.has(row.source)) present.add(row.source as ProviderId)
  return PROVIDER_ORDER.filter(id => id === 'all' || present.has(id))
}

/** 새 항목 표시의 기준. 패널이 쓰고 워커가 상태 표시줄에 읽는다. 항목 ID와 수뿐이다. */
export interface Marks {
  /** 이 ID까지는 모두 본 일이다. null이면 아직 기준이 없다. */
  floor: number | null
  /** floor보다 크고 이미 본 ID. */
  seen: number[]
  /** 패널이 마지막으로 셈한 목록의 가장 큰 ID. 이보다 큰 ID가 열리면 새 항목이다. */
  known: number
  /** 그때 아직 보지 않은 새 항목 수. */
  unseen: number
}

export const EMPTY_MARKS: Marks = { floor: null, seen: [], known: 0, unseen: 0 }
/** 남겨 두는 본 ID의 수. 넘치면 floor를 버린 ID까지 당긴다. 버린 일이 다시 새 일로 돌아오지 않는다. 그 아래의 보지 않은 일은 본 것으로 친다. */
const SEEN_LIMIT = 200

/** 저장소에서 읽은 값. 모양이 틀리면 기준이 없는 것으로 친다. 워커도 같은 함수로 읽는다. */
export function marksOf(value: unknown): Marks {
  if (typeof value !== 'object' || value === null) return EMPTY_MARKS
  const { floor, seen, known, unseen } = value as Record<string, unknown>
  const whole = (each: unknown): each is number => typeof each === 'number' && Number.isSafeInteger(each) && each >= 0
  return {
    floor: whole(floor) ? floor : null,
    seen: Array.isArray(seen) ? seen.filter(whole).slice(-SEEN_LIMIT) : [],
    known: whole(known) ? known : 0,
    unseen: whole(unseen) ? unseen : 0,
  }
}

/**
 * 사용자가 아직 보지 않은 새 일인가. 기준선, 직접 추가, 미룬 일, 세션에 맡긴 일은 사용자가 이미 아는 일이라 새 일이 아니다.
 * 기준이 없으면 아무것도 새 일이 아니다. 처음 읽은 목록이 기준이 된다.
 */
export function isFresh(item: ItemRow, marks: Marks): boolean {
  if (marks.floor === null || item.id <= marks.floor) return false
  if (item.baseline || item.reason === 'manual' || item.state !== 'open' || item.run !== null) return false
  return !marks.seen.includes(item.id)
}

const topId = (rows: readonly ItemRow[], floor: number): number => rows.reduce((top, row) => Math.max(top, row.id), floor)

/**
 * 기준이 없으면 지금 목록을 기준으로 삼는다. 그때 있던 일은 새 일이 아니다.
 * newest는 kairos가 센 가장 큰 열린 ID다. 목록이 길어 넘기지 않은 일도 그때 있던 일이다.
 * newest가 floor보다 작으면 kairos의 기록을 지우고 다시 시작한 것이다. ID가 1부터 다시 붙으므로 기준을 새로 잡는다.
 */
export function startMarks(marks: Marks, rows: readonly ItemRow[], newest: number | null = null): Marks {
  const reset = marks.floor !== null && newest !== null && newest < marks.floor
  if (marks.floor !== null && !reset) return marks
  const top = Math.max(topId(rows, 0), newest ?? 0)
  return { floor: top, seen: [], known: top, unseen: 0 }
}

/**
 * 화면에 그렸던 새 일을 본 것으로 적는다. 아직 보지 않은 새 일보다 앞은 모두 본 것이라 floor를 당긴다.
 * rows는 지금 목록 전체다. 목록이 길어 넘기지 않은 일은 셀 수 없어 본 것으로 친다.
 */
export function commitMarks(marks: Marks, shown: ReadonlySet<number>, rows: readonly ItemRow[], newest: number | null = null): Marks {
  const started = startMarks(marks, rows, newest)
  const floor = started.floor ?? 0
  const seen = [...new Set([...started.seen, ...[...shown].filter(id => id > floor)])]
  const fresh = rows.filter(row => isFresh(row, { ...started, seen })).map(row => row.id)
  const top = Math.max(topId(rows, floor), newest ?? 0)
  const pulled = fresh.length === 0 ? top : Math.max(floor, Math.min(...fresh) - 1)
  const kept = seen.filter(id => id > pulled).sort((a, b) => a - b)
  // 본 ID가 넘치면 버릴 ID까지 floor를 당긴다.
  const nextFloor = kept.length > SEEN_LIMIT ? (kept[kept.length - SEEN_LIMIT - 1] ?? pulled) : pulled
  const after: Marks = { floor: nextFloor, seen: kept.filter(id => id > nextFloor), known: top, unseen: 0 }
  return { ...after, unseen: rows.filter(row => isFresh(row, after)).length }
}

export const sameMarks = (a: Marks, b: Marks): boolean =>
  a.floor === b.floor && a.known === b.known && a.unseen === b.unseen && a.seen.length === b.seen.length && a.seen.every((id, index) => b.seen[index] === id)

/** 상태 표시줄의 새 일 점. 패널이 센 뒤에 열린 일이 있거나, 센 때 아직 보지 않은 일이 남았다. */
export const hasFresh = (marks: Marks, newest: number | null): boolean => marks.floor !== null && (marks.unseen > 0 || (newest !== null && newest > marks.known))

export const isTabId = (value: unknown): value is TabId => typeof value === 'string' && (TAB_ORDER as readonly string[]).includes(value)

/** 묶음이 속한 탭. */
export function tabOf(section: Section): TabId {
  return TAB_ORDER.find(tab => TAB_SECTIONS[tab].includes(section)) ?? 'now'
}

/** 줄이 속한 탭. */
export const tabOfRow = (item: ItemRow): TabId => tabOf(sectionOf(item))

/** 탭의 건수. 목록이 길어 넘기지 않은 줄도 센다. */
export function tabCount(tab: TabId, groups: Record<Section, readonly ItemRow[]>, omittedBy: Readonly<Record<string, number>>): number {
  return TAB_SECTIONS[tab].reduce((sum, section) => sum + groups[section].length + (omittedBy[section] ?? 0), 0)
}

/** 줄 높이가 곧 중요도다. 손댈 일은 제목을 다 읽게 두 줄로, 기다려도 되는 일은 한 줄로 그린다. */
export const isCompact = (section: Section): boolean => section === 'relaxed' || section === 'parked' || section === 'baseline'

/** 묶음에서 그릴 줄 수. 세션 줄은 줄이지 않고, 펼친 행은 숨기지 않는다. openIndex는 펼친 행의 자리이고 없으면 -1이다. */
export function visibleCount(section: Section, total: number, showAll: boolean, openIndex: number): number {
  if (showAll || section === 'running' || total < ROW_LIMIT + MORE_FROM) return total
  return Math.max(ROW_LIMIT, openIndex + 1)
}

const READY_ORDER: readonly string[] = ['failed', 'question', 'attention', 'done', 'ended']

/** 세션 소식 순서. 실패가 맨 위이고 같은 종류는 나중에 온 것이 위다. 모르는 종류는 맨 뒤다. */
export function sortReady<T extends { kind: string }>(cards: readonly T[]): T[] {
  const rank = (card: T): number => {
    const found = READY_ORDER.indexOf(card.kind)
    return found < 0 ? READY_ORDER.length : found
  }
  return [...cards].reverse().sort((a, b) => rank(a) - rank(b))
}

const has = (item: ItemRow, id: string): boolean => item.actions.some(action => action.id === id)

/** 시작 곁에는 수락 범위가 늘 보여야 하므로 범위가 없으면 시작을 그리지 않는다. 시작 없는 제안은 세션이 잠긴 것이다. */
export const canStart = (item: ItemRow): boolean => has(item, 'start') && !has(item, 'move') && item.scope !== null
