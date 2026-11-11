/**
 * list --json을 트리 노드로 줄인다. vscode API에 기대지 않아 node --test로 시험한다.
 * 외부 문자열(title)은 글자로만 그린다. 버튼은 kairos가 준 actions의 id만 보고, argv는 쓰지 않는다.
 * 묶음, 순서, 낱말은 Orca 패널(plugins/orca-panel/src/rows.ts, panel.tsx)과 같게 둔다. 같은 일을 표시면마다 다른 말로 부르지 않는다.
 */
import { isRecord, ITEM_ID } from '../../../packages/kairos-client/src/contract.ts'

export type Group = 'urgent' | 'session' | 'offer' | 'today' | 'later' | 'parked' | 'baseline'

export interface ItemNode {
  id: number
  group: Group
  label: string
  description: string
  /** 마우스를 올렸을 때의 여러 줄. 전체 제목, 사유와 출처, 맡길 범위, 항목 ID다. */
  tooltip: string
  /** codicon 이름. 사유를 형태로 알린다. */
  icon: string
  /** 에이전트에게 맡길 범위. 시작 확인 창에 그대로 보여 준다. */
  scope: string | null
  url: string | null
  actions: string[]
}

export const GROUP_LABELS: Record<Group, string> = {
  urgent: '긴급',
  session: '세션',
  offer: '에이전트 제안',
  today: '오늘',
  later: '여유 있을 때',
  parked: '미룬 일',
  baseline: '이전부터 있던 일',
}
/** 긴급을 세션 위에 둔다. 세션은 재촉하지 않는 구간이다. */
export const GROUP_ORDER: readonly Group[] = ['urgent', 'session', 'offer', 'today', 'later', 'parked', 'baseline']

/** 메뉴와 버튼의 낱말. Orca 패널과 같다. */
export const ACTION_LABELS: Record<string, string> = {
  start: '세션 열고 시작',
  move: '세션으로 이동',
  pr: 'PR 올리기',
  later: '나중에',
  keep: '새 소식이 오면 다시',
  keep_deploy: '배포 때 다시',
  done: '완료',
  cancel: '되돌리기',
  open: '원문 열기',
}

const ACTIONS = new Set(['start', 'later', 'done', 'keep', 'keep_deploy', 'move', 'cancel', 'pr'])
/** 사용자가 눌렀을 때만 연다. */
const OPENABLE = /^https:\/\/(github\.com|[a-z0-9-]+\.slack\.com|calendar\.google\.com|mail\.google\.com|www\.notion\.so)\//
/** 제어 문자, 양방향 재정렬, 폭 없는 문자. 한글 결합에 쓰는 ZWNJ와 ZWJ만 둔다. Orca 패널의 cleanText와 같다. 제목과 범위가 시작 확인 창에 나오기 때문이다. */
const UNSAFE = /(?![\u200c\u200d])[\p{Cc}\p{Cf}\p{Cs}\p{Zl}\p{Zp}]/gu
/** 여유 있을 때가 이만큼 쌓이면 접어 둔다. 오늘 할 일이 없으면 펼친다. */
const FOLD_FROM = 6

const REASON_ICONS: Record<string, string> = {
  reply_owed: 'comment',
  review_owed: 'eye',
  assigned: 'issues',
  my_pr_attention: 'git-pull-request',
  meeting_new: 'calendar',
  deploy_risk: 'rocket',
  work_owed: 'tools',
  manual: 'note',
}
const RUN_WORDS: Record<string, string> = {
  claimed: '세션을 준비하고 있습니다',
  started: '세션이 진행 중입니다',
  waiting_user: '확인을 기다립니다',
  needs_attention: '세션 확인이 필요합니다',
  ended: '세션이 끝났습니다',
}

/**
 * kairos는 배포 때 다시를 keep에 --until-deploy를 붙여 준다. argv는 이 구분에만 보고 인자로 쓰지 않는다.
 * Orca 패널의 parse.ts와 같다.
 */
function actionName(raw: Record<string, unknown>): string {
  const id = typeof raw.id === 'string' ? raw.id : ''
  return id === 'keep' && Array.isArray(raw.argv) && raw.argv.includes('--until-deploy') ? 'keep_deploy' : id
}

const clean = (value: unknown, limit: number): string =>
  typeof value === 'string' ? value.replace(UNSAFE, ' ').trim().slice(0, limit) : ''
const own = (table: Record<string, string>, key: string): string | null => (Object.hasOwn(table, key) ? table[key] : null)

function groupOf(item: Record<string, unknown>, scope: string | null): Group {
  if (isRecord(item.run) || item.state === 'accepted') return 'session'
  if (item.state === 'parked') return 'parked'
  if (item.baseline === true) return 'baseline'
  if (item.tier === 'P0') return 'urgent'
  if (scope !== null && clean(item.playbook, 40) !== '') return 'offer'
  return item.tier === 'P1' ? 'today' : 'later'
}

export function itemsOf(value: Record<string, unknown>): ItemNode[] {
  const items = Array.isArray(value.items) ? value.items : []
  const nodes: ItemNode[] = []
  for (const raw of items.slice(0, 200)) {
    if (!isRecord(raw) || typeof raw.id !== 'number' || !ITEM_ID.test(String(raw.id))) continue
    const actions = (Array.isArray(raw.actions) ? raw.actions : [])
      .map(action => (isRecord(action) ? actionName(action) : ''))
      .filter(id => ACTIONS.has(id))
    const session = isRecord(raw.run) || raw.state === 'accepted'
    // 이동은 세션 기록이 있을 때만 둔다. 수락만 된 항목은 되돌리기를 고른다.
    if (isRecord(raw.run) && !actions.includes('move')) actions.push('move')
    const scope = clean(raw.scope, 200) || null
    // 시작 곁에는 맡길 범위가 늘 보여야 한다. 범위가 없거나 이미 세션이 있으면 시작을 두지 않는다.
    const shown = scope === null || actions.includes('move') ? actions.filter(id => id !== 'start') : actions
    const url = typeof raw.url === 'string' && OPENABLE.test(raw.url) ? raw.url : null
    const reason = clean(raw.reasonLabel, 40)
    const source = clean(raw.sourceLabel, 20)
    const title = clean(raw.title, 120) || reason || '제목 없음'
    const group = groupOf(raw, scope)
    const runState = isRecord(raw.run) ? clean(raw.run.state, 32) : ''
    const runWord = session ? (own(RUN_WORDS, runState) ?? '세션 상태를 확인해 주세요') : null
    const meta = [runWord ?? reason, source === 'kairos' ? '' : source].filter(Boolean).join(' · ')
    nodes.push({
      id: raw.id,
      group,
      label: title,
      description: meta,
      tooltip: [title, meta, scope === null ? '' : `맡길 범위: ${scope}`, `#${raw.id}`].filter(Boolean).join('\n'),
      icon: session ? 'terminal' : (own(REASON_ICONS, clean(raw.reason, 32)) ?? 'bell'),
      scope,
      url,
      actions: shown,
    })
  }
  return nodes
}

/** 트리 항목의 contextValue. package.json의 메뉴 조건이 이 낱말을 본다. */
export const contextOf = (node: ItemNode): string => ['item', ...node.actions, ...(node.url ? ['url'] : [])].join(' ')

/** 묶음이 처음 그려질 때 접혀 있는가. 사용자가 바꾼 상태는 VS Code가 기억한다. */
export function startsCollapsed(group: Group, nodes: readonly ItemNode[]): boolean {
  if (group === 'parked' || group === 'baseline') return true
  if (group !== 'later') return false
  const count = (wanted: Group): number => nodes.filter(node => node.group === wanted).length
  return count('later') >= FOLD_FROM && count('today') > 0
}

/** 상태 표시줄과 활동 막대에 쓰는 건수. 여유 있을 때, 미룬 일, 이전부터 있던 일은 재촉하지 않으므로 세지 않는다. */
export function countsOf(nodes: readonly ItemNode[]): { pending: number; urgent: number } {
  const quiet: ReadonlySet<Group> = new Set(['later', 'parked', 'baseline'])
  return {
    pending: nodes.filter(node => !quiet.has(node.group)).length,
    urgent: nodes.filter(node => node.group === 'urgent').length,
  }
}

/** next --json의 reveal. 알림을 누른 항목 ID와 시각이다. */
export function revealOf(value: Record<string, unknown>): { at: number; items: number[] } | null {
  const reveal = value.reveal
  if (!isRecord(reveal) || typeof reveal.at !== 'number' || !Array.isArray(reveal.items)) return null
  const items = reveal.items.filter((id): id is number => typeof id === 'number' && ITEM_ID.test(String(id))).slice(0, 20)
  return items.length > 0 ? { at: reveal.at, items } : null
}
