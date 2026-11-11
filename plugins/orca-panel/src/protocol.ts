/** 패널과 워커의 약속. 패널은 argv를 보지 않고, 항목 ID와 고정 버튼 이름만 워커로 보낸다. */
export type Tier = 'P0' | 'P1' | 'P2'
export type ItemActionId = 'start' | 'later' | 'done' | 'keep' | 'keep_deploy' | 'ack' | 'move' | 'cancel' | 'pr'
/** 항목에 달리지 않은 패널 조작. kairos가 주는 버튼이 아니라 패널이 고정으로 그린다. */
export type PanelActionId = 'focus_25' | 'focus_50' | 'focus_90' | 'focus_end' | 'add' | 'display_auto'
export type ActionId = ItemActionId | PanelActionId

export interface PanelAction {
  id: ItemActionId
  label: string
}

export interface RunView {
  id: string
  state: string
}

export interface DeployView {
  date: string
}

export interface ItemRow {
  id: number
  tier: Tier
  reason: string
  reasonLabel: string
  source: string | null
  sourceLabel: string
  state: string
  /** 외부 문자열이다. 텍스트로만 그린다. */
  title: string
  /** 허용 호스트의 https만 사용자가 눌렀을 때 연다. 글자로 그리지 않는다. */
  url: string | null
  openedAt: string | null
  dueAt: string | null
  parkedUntil: string | null
  parkTrigger: string | null
  baseline: boolean
  carried: boolean
  pushCount: number
  deploy: DeployView | null
  /** kairos가 세션에 맡길 수 있다고 본 일. 잠겨 있어도 오고, 그때는 actions에 start가 없다. */
  playbook: string | null
  /** 제안 문구. 고정 값이고 그대로 그린다. */
  scope: string | null
  run: RunView | null
  actions: PanelAction[]
}

export interface CardView {
  id: string
  kind: 'item' | 'bundle' | 'carryover'
  tier: Tier
  headline: string
  items: ItemRow[]
  more: number
  itemIds: number[]
  actions: PanelAction[]
  pending: boolean
}

/** 세션 쪽 카드. kairos는 run마다 한 번만 내므로 패널이 닫을 때까지 들고 있는다. */
export interface ReadyView {
  runId: string
  itemId: number
  /** failed는 뒤에서 실패해 항목을 되돌린 카드다. 표시 게이트가 닫혀 있어도 그린다. */
  kind: string
  headline: string
  title: string
  /** 허용 호스트의 https만 사용자가 눌렀을 때 연다. 글자로 그리지 않는다. */
  url: string | null
  actions: PanelAction[]
}

export interface GateView {
  open: boolean
  mode: string
  reason: string | null
  message: string | null
  startsAt: string | null
}

export interface HoldView {
  reason: string
  message: string
  until: string | null
}

export interface CountsView {
  P0: number
  P1: number
  P2: number
  manual: number
  parked: number
  accepted: number
  /** 오늘 전에 열려 아직 남은 P1. */
  carried: number
}

export interface FocusLine {
  active: boolean
  until: string | null
  heldP1: number
  heldLine: string | null
}

/** 사용자가 누른 알림의 항목. at은 kairos reveal이 쓴 시각(ms)이고 패널은 같은 at을 한 번만 적용한다. */
export interface RevealView {
  items: number[]
  at: number
}

export interface NextView {
  /** kairos가 답한 시각. 표시 시각의 기준이다. */
  at: string | null
  display: GateView
  card: CardView | null
  hold: HoldView | null
  ready: ReadyView[]
  /** 활성 run 수. 항목이 먼저 닫혀도 세션 추적은 이 값으로 이어간다. */
  activeRuns: number
  counts: CountsView
  focus: FocusLine
  health: string | null
  /** 나중에를 누르면 다시 올 묶음 시각. */
  nextBundleAt: string | null
  /** 10분 안에 누른 알림. 없거나 형식이 틀리면 null이다. */
  reveal: RevealView | null
}

export interface ListView {
  at: string | null
  items: ItemRow[]
  /** 표시 게이트 때문에 내지 않은 건수. */
  hidden: number
  /** LIST_LIMIT을 넘어 넘기지 않은 건수. */
  omitted: number
  /** 그 건수의 묶음별 내역. 머리글 숫자가 실제 건수와 맞게 한다. */
  omittedBy: Record<string, number>
}

export interface Failure {
  code: string
  message: string
}

export interface Snapshot {
  next: NextView | null
  list: ListView | null
  error: Failure | null
}

export interface ActionOutcome {
  ok: boolean
  code: string | null
  message: string
  /** kairos 결과의 상태. changed, unchanged, already_closed처럼 ok여도 뜻이 갈린다. */
  status?: string | null
  warning?: string | null
  run: RunView | null
  /** 잘못된 요청이나 처리 중 거절이면 null이다. 패널은 이전 화면을 유지한다. */
  snapshot: Snapshot | null
}

export interface SnapshotInput {
  seen: boolean
}

export interface ActionInput {
  action: ActionId
  ids: number[]
  seen: boolean
  /** add에만 있다. 사용자가 패널에 직접 쓴 제목이다. */
  title?: string
}

declare module 'orca' {
  interface PluginRequests {
    'kairos.snapshot': { input: SnapshotInput; output: Snapshot }
    'kairos.action': { input: ActionInput; output: ActionOutcome }
  }
  interface PluginPanelEvents {
    /** 폴링 한 번이 끝났다. 제목 같은 값은 담지 않는다. */
    'kairos.tick': { at: number }
    /** reveal.json이 바뀌었다. 항목 ID와 제목은 담지 않고, 패널이 다시 읽어 받는다. */
    'kairos.reveal': { at: number }
  }
}
