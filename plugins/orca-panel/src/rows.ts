/** 목록 줄의 묶음과 순서를 정하는 순수 함수. 패널과 워커가 같이 쓰므로 node 모듈을 쓰지 않는다. */
import type { ItemRow } from './protocol.ts'

export type Section = 'running' | 'urgent' | 'offer' | 'today' | 'relaxed' | 'parked' | 'baseline'

/** 자르기 우선순위이기도 하다. 세션이 있는 줄은 잘리면 패널에서 그 세션으로 갈 길이 없어 맨 앞에 둔다. */
const SORT_ORDER: readonly Section[] = ['running', 'urgent', 'offer', 'today', 'relaxed', 'parked', 'baseline']
/** 화면 순서. 긴급을 진행 중 위에 둔다. 진행 중은 재촉하지 않는 구간이다. */
export const DISPLAY_ORDER: readonly Section[] = ['urgent', 'running', 'offer', 'today', 'relaxed', 'parked', 'baseline']

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
  if (section === 'parked' || section === 'baseline') return false
  if (section === 'relaxed' && groups.relaxed.length >= FOLD_FROM) return groups.today.length === 0
  return undefined
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
