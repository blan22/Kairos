import { assert, afterEach, hostError, mockHost, mount, mountWorker, test, type TestPanel } from 'orca/testing'
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
  OFFER_SCOPE,
  offered,
  parked,
  readyCard,
  recordingGate,
  revivable,
  sessionNews,
  SLACK_TITLE,
  snapshotOf,
  sourceRows,
  startable,
} from './fixtures.ts'
import * as panelModule from './panel.tsx'
import type { ActionInput, ActionOutcome, Snapshot, SnapshotInput } from './protocol.ts'
import type { Exec } from './runner.ts'
import { startWorker } from './service.ts'

let view: TestPanel | undefined
afterEach(() => view?.unmount())

/** 매니페스트가 청하는 권한. 새 일 기준을 플러그인 저장소에 둔다. */
const CAPS: string[] = ['storage:own']
const SCOPE = OFFER_SCOPE
const START = '세션 열고 시작'
const scene = (next: Record<string, unknown> = {}, rows: Record<string, unknown>[] = [], list: Record<string, unknown> = {}): Snapshot =>
  snapshotOf(nextJson(next), listJson(rows, list))

interface Rig {
  snapshots: SnapshotInput[]
  actions: ActionInput[]
}

function serve(snapshot: Snapshot, onAction?: (input: ActionInput) => ActionOutcome | Promise<ActionOutcome>) {
  const rig: Rig = { snapshots: [], actions: [] }
  const host = mockHost({
    capabilities: CAPS,
    requests: {
      'kairos.snapshot': (input: SnapshotInput) => {
        rig.snapshots.push(input)
        return snapshot
      },
      'kairos.action': (input: ActionInput) => {
        rig.actions.push(input)
        return onAction ? onAction(input) : { ok: true, code: null, message: '대기로 옮김', run: null, snapshot }
      },
    },
  })
  return { rig, host }
}

const open = async (snapshot: Snapshot, onAction?: (input: ActionInput) => ActionOutcome | Promise<ActionOutcome>, visible: boolean = true) => {
  const { rig, host } = serve(snapshot, onAction)
  view = await mount(panelModule, { host, visible })
  return { rig, panel: view }
}

/** 틱마다 다른 값을 줄 수 있게 스냅숏을 바꿀 수 있는 패널. */
const openLive = async (first: Snapshot, onAction?: () => ActionOutcome) => {
  const state = { snapshot: first, reads: 0 }
  const host = mockHost({
    capabilities: CAPS,
    requests: {
      'kairos.snapshot': () => {
        state.reads += 1
        return state.snapshot
      },
      'kairos.action': () => onAction?.() ?? { ok: true, code: null, message: '처리했습니다', run: null, snapshot: state.snapshot },
    },
  })
  view = await mount(panelModule, { host })
  return { state, panel: view }
}

const button = (panel: TestPanel, text: string) => panel.find({ role: 'button', text })
const texts = (panel: TestPanel): string => JSON.stringify(panel.tree())
/** 줄의 ListItem만 모은다. 머리글과 더 보기도 ListItem이다. */
const rowsOf = (panel: TestPanel) => panel.findAll({ type: 'ListItem' }).filter(found => found.key?.endsWith('-item') === true)
/** 줄 제목. 카드 안의 줄은 title 속성이고 나머지는 첫 글자 요소다. */
const rowTitle = (panel: TestPanel, row: ReturnType<typeof rowsOf>[number]): string | undefined =>
  row.label ?? panel.findAll({ type: 'Text' }).find(found => found.path.startsWith(`${row.path}/`))?.text
const titled = (panel: TestPanel, title: string) => rowsOf(panel).find(row => rowTitle(panel, row) === title) ?? null
const expand = async (panel: TestPanel, title: string): Promise<void> => panel.press(titled(panel, title) as never)
/** 그린 순서대로 줄 제목을 모은다. */
const rowTitles = (panel: TestPanel): (string | undefined)[] => rowsOf(panel).map(row => rowTitle(panel, row))
const tick = async (panel: TestPanel): Promise<void> => panel.post('kairos.tick', { at: 1 })
/** 탭을 고른다. 고르지 않은 탭의 줄은 그려지지 않는다. */
const tabTo = async (panel: TestPanel, id: 'now' | 'waiting' | 'sessions'): Promise<void> => panel.select({ type: 'Tabs' }, id)
/** 탭 머리의 글자. 건수가 있으면 뒤에 붙는다. */
const tabLabels = (panel: TestPanel): unknown[] => panel.findAll({ type: 'Tab' }).map(found => found.props.label)
/** 머리말의 수집 상태 버튼. 누르면 목록 대신 상태 화면을 그리고, 다시 누르면 닫는다. */
const statusButton = (panel: TestPanel) => panel.find({ type: 'IconButton', label: /수집 상태/ })
const toggleStatus = async (panel: TestPanel): Promise<void> => panel.press(statusButton(panel) as never)
/** 프로바이더 칩. 이름 뒤에 지금 탭의 건수가 붙는다. */
const chip = (panel: TestPanel, name: string) => panel.findAll({ type: 'Button' }).find(found => found.key === `provider-${name}-button`) ?? null
const chipLabels = (panel: TestPanel): string[] => panel.findAll({ type: 'Button' }).filter(found => found.key?.startsWith('provider-') === true).map(found => found.text)
/** 자식이 없는 틀의 수. 빈 층을 그리지 않는지 볼 때 쓴다. */
function hollow(node: unknown, type: string): number {
  if (Array.isArray(node)) return node.reduce((sum: number, each) => sum + hollow(each, type), 0)
  if (typeof node !== 'object' || node === null) return 0
  const { type: own, children } = node as { type?: string; children?: unknown[] }
  return (own === type && (children ?? []).length === 0 ? 1 : 0) + hollow(children ?? [], type)
}

test('게이트가 닫히면 사유 하나만 그리고 건수와 카드와 버튼이 없다', async () => {
  const closed = scene({ display: recordingGate, counts: counts({ P1: 3 }), health: '합성 경고', hold: { reason: 'recording', message: '기록 중입니다', until: null } }, [], { hidden: 4 })
  const { panel } = await open(closed)
  assert.ok(panel.find({ type: 'EmptyState', text: /지금은 기록만 하고 있습니다/ }))
  assert.match(texts(panel), /내일 20:00부터 제안을 보여 드립니다/)
  assert.ok(texts(panel).includes('모아 둔 일 4건'))
  assert.equal(button(panel, '나중에'), null)
  assert.ok(!texts(panel).includes('3건'))
  assert.ok(!texts(panel).includes('합성 경고'))
  assert.equal(panel.findAll({ type: 'EmptyState' }).length, 1)
  assert.deepEqual(panel.errors, [])
})

test('게이트 사유마다 다음 할 일을 알려 준다', async () => {
  const gate = (reason: string) => ({ ...recordingGate, reason, startsAt: null })
  const cases: readonly (readonly [string, string, string])[] = [
    ['recording', '지금은 기록만 하고 있습니다', '기록이 끝나면 제안을 보여 드립니다'],
    ['no_collection', '아직 수집 전입니다', '첫 수집이 끝나면 기록을 시작합니다'],
    ['paused', '잠시 멈춰 두었습니다', 'kairos pause --resume'],
    ['display_off', '제안을 꺼 두었습니다', '다시 켜기'],
    ['other', '제안이 잠시 멈춰 있습니다', '기록 중입니다. 내일 시작합니다'],
  ]
  for (const [reason, title, line] of cases) {
    const { panel } = await open(scene({ display: gate(reason) }))
    assert.ok(texts(panel).includes(title), reason)
    assert.ok(texts(panel).includes(line), reason)
    view?.unmount()
  }
})

test('목록과 카드가 없으면 빈 표시 하나만 그린다', async () => {
  const { panel } = await open(scene())
  assert.ok(texts(panel).includes('지금은 챙길 일이 없습니다'))
  assert.equal(panel.findAll({ type: 'EmptyState' }).length, 1)
  assert.equal(panel.find({ text: '0건' }), null)
  assert.deepEqual(panel.errors, [])
  assert.deepEqual(panel.warnings, [])
})

test('수집이 늦으면 할 일이 없다고 단정하지 않는다', async () => {
  const { panel } = await open(scene({ health: 'Slack 수집이 30분 늦습니다' }))
  assert.ok(texts(panel).includes('확인된 일은 없습니다'))
  assert.ok(!texts(panel).includes('지금은 챙길 일이 없습니다'))
  assert.ok(!texts(panel).includes('수집이 늦어 새 일이 더 있을 수 있습니다'))
})

test('목록이 없고 보류 중이면 보류 사유를 빈 표시로 쓴다', async () => {
  const { panel } = await open(scene({ hold: { reason: 'focus', message: '포커스 중입니다', until: '2026-11-04T03:00:00.000Z' } }))
  assert.equal(panel.find({ type: 'EmptyState' })?.props.title, '지금은 챙길 일이 없습니다')
  assert.ok(panel.find({ type: 'Text', text: '포커스 중 · 12:00까지' }))
  assert.ok(button(panel, '끝내기'))
})

test('포커스가 아닌 보류는 사유를 빈 표시로 쓴다', async () => {
  const { panel } = await open(scene({ hold: { reason: 'lunch', message: '점심', until: '2026-11-04T03:00:00.000Z' } }))
  const empty = panel.find({ type: 'EmptyState' })
  assert.equal(empty?.props.title, '점심시간입니다')
  assert.equal(empty?.props.icon, 'pause')
  assert.equal(empty?.props.description, '12:00에 다시 보여 드립니다.')
})

test('목록이 있고 카드가 없으면 보류 사유를 한 줄로 쓰고 빈 표시를 그리지 않는다', async () => {
  const { panel } = await open(scene({ hold: { reason: 'lunch', message: '점심', until: '2026-11-04T04:00:00.000Z' } }, [item(1)]))
  assert.equal(panel.find({ text: '점심시간입니다. 13:00에 다시 보여 드립니다.' })?.props.size, 'xs')
  assert.equal(panel.find({ type: 'EmptyState' }), null)
})

test('목록이 있고 먼저 볼 일이 없다는 말은 그리지 않는다', async () => {
  const { panel } = await open(scene({}, [item(1)]))
  assert.ok(!texts(panel).includes('지금 먼저 볼 일은 없습니다'))
  assert.ok(titled(panel, '합성 이슈 1'))
})

test('첫 읽기 전에는 자리 표시만 그린다', async () => {
  const host = mockHost({ capabilities: CAPS })
  view = await mount(panelModule, { host, visible: false })
  assert.ok(view.find({ type: 'Skeleton' }))
  assert.equal(view.find({ type: 'EmptyState' }), null)
})

test('kairos 실패를 사용자 문구와 코드로 그리고 다시 시도할 수 있다', async () => {
  const { rig, panel } = await open({ next: null, list: null, error: { code: 'bad_output', message: 'kairos 응답을 읽지 못했습니다' } })
  assert.ok(panel.summary().some(line => line.includes('설치된 kairos와 패널 버전이 다릅니다')))
  assert.ok(texts(panel).includes('코드 bad_output'))
  assert.equal(panel.find({ type: 'EmptyState' }), null)
  await panel.press(button(panel, '다시 시도') as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: true })
})

test('목록만 실패하면 카드를 그리고 그 사실을 알린다', async () => {
  const good = scene({ card: card([item(1)]) })
  const { panel } = await open({ ...good, list: null, error: { code: 'timeout', message: 'kairos 응답 시간을 넘겼습니다' } })
  assert.ok(button(panel, '나중에'))
  assert.ok(panel.summary().some(line => line.includes('목록을 읽지 못했습니다')))
  assert.ok(texts(panel).includes('지금 볼 일은 최신입니다.'))
  assert.ok(texts(panel).includes('코드 timeout'))
})

test('지금 볼 일만 실패하면 목록을 그리고 그 사실을 알린다', async () => {
  const { panel } = await open({ next: null, list: scene({}, [item(1)]).list, error: { code: 'timeout', message: '합성' } })
  assert.ok(panel.summary().some(line => line.includes('지금 볼 일을 읽지 못했습니다')))
  assert.ok(titled(panel, '합성 이슈 1'))
  assert.equal(panel.find({ type: 'EmptyState' }), null)
})

test('읽기가 실패해도 이전 값을 지우지 않고 읽은 시각을 보여 준다', async () => {
  const { state, panel } = await openLive(scene({ card: card([item(1)]), counts: counts({ P1: 1 }) }, [item(1), item(2)]))
  state.snapshot = { next: null, list: null, error: { code: 'spawn_failed', message: '합성' } }
  await tick(panel)
  assert.ok(button(panel, '나중에'))
  assert.ok(titled(panel, '합성 이슈 2'))
  assert.ok(panel.find({ text: '1건' }))
  assert.ok(panel.summary().some(line => line.includes('kairos가 실행되지 않습니다')))
  assert.ok(texts(panel).includes('마지막으로 읽은 시각'))
})

test('워커가 죽어 있어도 조용히 빈 화면이 되지 않는다', async () => {
  const host = mockHost({
    capabilities: CAPS,
    requests: {
      'kairos.snapshot': () => {
        throw hostError('unavailable', 'worker down')
      },
    },
  })
  view = await mount(panelModule, { host })
  assert.ok(view.summary().some(line => line.includes('패널이 응답을 받지 못했습니다')))
  assert.ok(texts(view).includes('코드 unavailable'))
})

test('건강 경고는 한 줄이고 포커스가 끝나면 쌓인 건수를 보류 줄 아래에 쓴다', async () => {
  const focus = { active: false, startedAt: null, until: null, heldP1: 2, heldLine: '포커스 중 쌓인 P1 2건' }
  const { panel } = await open(scene({ health: 'Slack 수집이 30분 늦습니다', focus }, [item(1)]))
  assert.ok(texts(panel).includes('Slack 수집이 30분 늦습니다'))
  assert.ok(texts(panel).includes('이 출처의 새 일은 늦게 보일 수 있습니다'))
  assert.ok(texts(panel).includes('포커스 중 쌓인 일 2건'))
  assert.equal(panel.find({ type: 'Callout' }), null)
})

test('세션 상태를 못 읽은 경고는 뒷말이 다르다', async () => {
  const { panel } = await open(scene({ health: '세션 상태를 읽지 못했습니다' }))
  assert.ok(texts(panel).includes('세션 표시가 늦을 수 있습니다'))
})

test('카드 버튼은 항목 ID만 워커로 보낸다', async () => {
  const { rig, panel } = await open(scene({ card: card([item(7)]), counts: counts({ P1: 1 }) }))
  await panel.press(button(panel, '나중에') as never)
  assert.deepEqual(rig.actions, [{ action: 'later', ids: [7], seen: true }])
  assert.ok(panel.find({ text: '미뤘습니다 · 내일 11:00에 다시' }))
  assert.ok(panel.find({ text: '1건' }))
})

test('등급은 사용자 낱말로 쓰고 긴급 카드만 색이 있다', async () => {
  const { panel } = await open(scene({ card: card([item(1, { tier: 'P0' })], { tier: 'P0' }), counts: counts({ P0: 1 }) }))
  assert.ok(panel.find({ type: 'Text', text: '긴급 · 먼저 볼 일' }))
  assert.ok(texts(panel).includes('"border":"danger"'))
  assert.equal(panel.find({ type: 'Text', text: '긴급 1' })?.props.tone, 'danger')
  assert.equal(panel.find({ type: 'Text', text: '먼저 볼 일' }), null)
})

test('시작 버튼 곁에 범위 문구를 그리고 항목 ID만 보낸다', async () => {
  const { rig, panel } = await open(scene({ card: card([startable(7)]) }))
  assert.ok(panel.find({ type: 'Text', text: SCOPE }))
  await panel.press(button(panel, START) as never)
  assert.deepEqual(rig.actions, [{ action: 'start', ids: [7], seen: true }])
})

test('범위가 다른 값이면 받은 값을 그대로 쓰고 범위가 없으면 시작을 그리지 않는다', async () => {
  const first = await open(scene({ card: card([{ ...startable(7), scope: '합성 범위' }]) }))
  assert.ok(first.panel.find({ type: 'Text', text: '합성 범위' }))
  first.panel.unmount()
  const second = await open(scene({ card: card([{ ...startable(7), scope: null }]) }))
  assert.equal(button(second.panel, START), null)
})

test('시작 버튼이 있는 모든 곳에 범위 문구가 있다', async () => {
  const { panel } = await open(scene({ ready: [failedCard(9)] }, [startable(8), startable(9)]))
  assert.ok(panel.find({ role: 'button', text: '다시 시작' }))
  assert.equal(panel.findAll({ type: 'Text', text: SCOPE }).length, 1)
  await expand(panel, '합성 이슈 8')
  assert.ok(button(panel, START))
  assert.equal(panel.findAll({ type: 'Text', text: SCOPE }).length, 2)
})

test('제안은 에이전트 제안 묶음에 모이고 줄마다 같은 말을 되풀이하지 않는다', async () => {
  const { panel } = await open(scene({}, [startable(7), startable(8, { tier: 'P2' }), item(9)]))
  assert.ok(panel.find({ type: 'Text', text: '에이전트 제안 2' }))
  assert.equal(panel.find({ type: 'Text', text: '에이전트 제안 2' })?.props.tone, 'accent')
  assert.ok(panel.find({ type: 'Text', text: '오늘 1' }))
  assert.equal(panel.find({ type: 'Text', text: /· 에이전트 제안/ }), null)
})

test('접힌 줄에는 시작이 없고 펼쳐야 범위와 컨펌 버튼이 보인다', async () => {
  const { rig, panel } = await open(scene({}, [startable(7)]))
  assert.equal(button(panel, START), null)
  assert.equal(panel.find({ type: 'Callout' }), null)
  await expand(panel, '합성 이슈 7')
  const offer = panel.find({ type: 'Callout' })
  assert.equal(offer?.props.title, '에이전트가 이슈를 고칩니다')
  assert.equal(offer?.props.tone, 'accent')
  assert.ok(panel.find({ type: 'Text', text: SCOPE }))
  assert.deepEqual(rig.actions, [])
  await panel.press(button(panel, START) as never)
  assert.deepEqual(rig.actions, [{ action: 'start', ids: [7], seen: true }])
})

test('긴급 줄의 제안은 자리를 지키고 접힌 줄에 낱말로 알린다', async () => {
  const { panel } = await open(scene({}, [startable(7, { tier: 'P0' })]))
  assert.ok(panel.find({ type: 'Text', text: '긴급 1' }))
  assert.equal(panel.find({ type: 'Text', text: '에이전트 제안 1' }), null)
  assert.match(titled(panel, '합성 이슈 7')?.text ?? '', /GitHub · 담당 이슈 · 에이전트 제안/)
  await expand(panel, '합성 이슈 7')
  assert.equal(panel.find({ type: 'Text', text: /· 에이전트 제안/ }), null)
  assert.ok(button(panel, START))
})

test('묶음 카드의 접힌 줄은 제안 묶음 밖이므로 낱말로 알린다', async () => {
  const { panel } = await open(scene({ card: bundle([startable(7, { tier: 'P2' }), item(8, { tier: 'P2' })], 0) }))
  assert.match(titled(panel, '합성 이슈 7')?.text ?? '', /GitHub · 담당 이슈 · 에이전트 제안/)
  assert.doesNotMatch(titled(panel, '합성 이슈 8')?.text ?? '', /에이전트 제안/)
})

test('세션이 잠겨 있으면 제안과 푸는 길만 있고 시작 버튼은 없다', async () => {
  const { rig, panel } = await open(scene({ card: card([offered(7)]) }))
  assert.equal(panel.find({ type: 'Callout' })?.props.title, '에이전트가 이슈를 고칩니다')
  assert.ok(panel.find({ type: 'Text', text: SCOPE }))
  assert.equal(button(panel, START), null)
  assert.ok(panel.find({ type: 'Code', text: 'kairos sessions on' }))
  assert.deepEqual(rig.actions, [])
})

test('범위가 없는 제안은 제안으로 그리지 않는다', async () => {
  const { panel } = await open(scene({}, [offered(7, { scope: null })]))
  assert.ok(panel.find({ type: 'Text', text: '오늘 1' }))
  await expand(panel, '합성 이슈 7')
  assert.equal(panel.find({ type: 'Callout' }), null)
  assert.equal(panel.find({ type: 'Code' }), null)
})

test('줄 사이 간격은 묶음 안쪽에만 있고 머리글은 첫 줄에 붙는다', async () => {
  const { panel } = await open(scene({}, [item(7), item(8)]))
  /** 글자를 모두 담은 가장 안쪽 Stack의 간격. */
  const gapAround = (node: unknown, words: readonly string[], found: unknown = null): unknown => {
    if (typeof node !== 'object' || node === null) return found
    const here = node as { type?: unknown; props?: { gap?: unknown } }
    const holds = words.every(word => JSON.stringify(node).includes(word))
    if (!holds) return found
    const gap = here.type === 'Stack' ? here.props?.gap : found
    return Object.values(node).reduce((inner: unknown, child) => gapAround(child, words, inner), gap)
  }
  assert.equal(gapAround(panel.tree(), ['합성 이슈 7', '합성 이슈 8']), 'xs')
  assert.equal(gapAround(panel.tree(), ['오늘 2', '합성 이슈 8']), 'none')
})

test('시작이 없는 항목은 시작 버튼을 그리지 않는다', async () => {
  const { panel } = await open(scene({ card: card([item(7)]) }))
  assert.equal(button(panel, START), null)
})

test('시작 중에는 누른 자리가 경과와 안내로 바뀐다', async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const shown = scene({ card: card([startable(7)]) })
  const { panel } = await open(shown, () => new Promise(resolve => (release = resolve)))
  const pressed = panel.press(button(panel, START) as never)
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(button(panel, START), null)
  assert.ok(texts(panel).includes('작업 공간을 만든 뒤 세션 창을 엽니다. 길면 2분쯤 걸립니다.'))
  assert.ok(panel.findAll({ type: 'RelativeTime' }).some(found => found.props.format === 'duration'))
  release({ ok: true, code: null, message: '합성 원문', status: 'changed', run: { id: 'run-1', state: 'claimed' }, snapshot: shown })
  await pressed
  assert.ok(panel.find({ text: '세션을 여는 중입니다' }))
  assert.ok(!texts(panel).includes('합성 원문'))
})

test('시작의 경고는 사라지지 않고 닫을 때까지 남는다', async () => {
  const { panel } = await open(scene({ card: card([startable(7)]) }), () => ({ ok: true, code: null, message: '합성', status: 'changed', warning: '합성 경고', run: { id: 'run-1', state: 'claimed' }, snapshot: scene({}, [accepted(7)]) }))
  await panel.press(button(panel, START) as never)
  assert.equal(panel.find({ type: 'Callout' })?.props.title, '세션을 여는 중입니다')
  assert.equal(panel.find({ type: 'Callout' })?.props.tone, 'neutral')
  assert.ok(texts(panel).includes('합성 경고'))
  await panel.press(button(panel, '닫기') as never)
  assert.equal(panel.find({ type: 'Callout' }), null)
})

test('같은 이슈의 세션이 이미 있으면 사라지지 않는 안내로 알리고 시작을 성공이라고 말하지 않는다', async () => {
  const shown = scene({ card: card([startable(7)]) }, [accepted(8)])
  const { panel } = await open(shown, () => ({ ok: true, code: null, message: '합성 원문', status: 'unchanged', run: { id: 'run-8', state: 'started' }, snapshot: shown }))
  await panel.press(button(panel, START) as never)
  const line = panel.find({ text: '같은 이슈의 세션이 이미 있습니다. 세션 탭에서 그 세션으로 이동할 수 있습니다.' })
  assert.ok(line)
  assert.equal(line?.props.tone, undefined)
  assert.ok(!texts(panel).includes('합성 원문'))
  assert.equal(panel.find({ text: '세션을 여는 중입니다' }), null)
  await tick(panel)
  assert.ok(texts(panel).includes('같은 이슈의 세션이 이미 있습니다'))
})

test('시작이 이미 끝난 상태를 만나면 상태대로 말한다', async () => {
  const cases: readonly (readonly [Partial<ActionOutcome>, string])[] = [
    [{ status: 'already_closed' }, '이미 처리된 일입니다'],
    [{ status: 'unchanged', run: { id: 'r', state: 'started' } }, '이미 세션이 있습니다'],
    [{ status: 'changed', run: { id: 'r', state: 'started' } }, '이미 세션이 있습니다'],
    [{ status: 'changed', run: { id: 'r', state: 'needs_attention' } }, '세션 확인이 필요합니다'],
  ]
  for (const [extra, line] of cases) {
    const { panel } = await open(scene({ card: card([startable(7)]) }), () => ({ ok: true, code: null, message: '합성', run: null, snapshot: scene({}, [accepted(7)]), ...extra }))
    await panel.press(button(panel, START) as never)
    assert.ok(panel.find({ text: line }), line)
    panel.unmount()
  }
})

test('시작 실패를 누른 자리에 코드와 함께 보여 주고 다시 누를 수 있다', async () => {
  const shown = scene({ card: card([startable(7)]) })
  const { panel } = await open(shown, () => ({ ok: false, code: 'worktree_failed', message: '합성 원문', run: null, snapshot: shown }))
  await panel.press(button(panel, START) as never)
  assert.equal(panel.find({ text: '시작하지 못했습니다. 작업 공간을 만들지 못했습니다.' })?.props.tone, 'danger')
  assert.ok(panel.find({ text: '코드 worktree_failed' }))
  assert.ok(!texts(panel).includes('합성 원문'))
  assert.equal(panel.find({ type: 'Callout', label: '시작하지 못했습니다' }), null)
  assert.equal(button(panel, START)?.props.disabled ?? false, false)
})

test('실패는 틱이 와도 남고 다음 버튼을 누르면 지워진다', async () => {
  let fail = true
  const shown = scene({ card: card([item(7)]) })
  const { panel } = await open(shown, () => ({ ok: !fail, code: fail ? 'failed' : null, message: '합성 실패', run: null, snapshot: shown }))
  await panel.press(button(panel, '완료') as never)
  await tick(panel)
  assert.ok(panel.find({ text: '완료하지 못했습니다.' }))
  fail = false
  await panel.press(button(panel, '나중에') as never)
  assert.equal(panel.find({ text: '완료하지 못했습니다.' }), null)
})

test('시작이 패널 시간 제한으로 끝나면 실패로 단정하지 않는다', async () => {
  const { panel } = await open(scene({ card: card([startable(7)]) }), () => {
    throw hostError('timeout', 'late')
  })
  await panel.press(button(panel, START) as never)
  const line = panel.find({ text: '시작이 늦어지고 있습니다. 결과는 잠시 뒤 목록에 나타납니다.' })
  assert.ok(line)
  assert.equal(line?.props.tone, undefined)
  assert.equal(panel.find({ text: /코드 / }), null)
  assert.equal(button(panel, START)?.props.disabled, true)
})

test('펼친 행의 시작이 늦어지면 틱을 맡아 두지 않고 다음 읽기에서 결과를 그린다', async () => {
  let failing = true
  const state = { snapshot: scene({}, [startable(7)]) }
  const host = mockHost({
    capabilities: CAPS,
    requests: {
      'kairos.snapshot': () => state.snapshot,
      'kairos.action': () => {
        if (failing) throw hostError('timeout', 'late')
        return { ok: true, code: null, message: '합성', run: null, snapshot: state.snapshot }
      },
    },
  })
  view = await mount(panelModule, { host })
  await expand(view, '합성 이슈 7')
  await view.press(button(view, START) as never)
  assert.ok(view.find({ text: '시작이 늦어지고 있습니다. 결과는 잠시 뒤 목록에 나타납니다.' }))
  assert.equal(button(view, START)?.props.disabled, true)
  failing = false
  state.snapshot = scene({}, [accepted(7)])
  await tick(view)
  assert.equal(button(view, START), null)
  assert.equal(view.find({ text: /시작이 늦어지고/ }), null)
  await tabTo(view, 'sessions')
  assert.ok(texts(view).includes('세션이 진행 중입니다'))
})

test('늦어진 시작이 일어나지 않았으면 다음 읽기 뒤에 다시 누를 수 있다', async () => {
  const shown = scene({ card: card([startable(7)]) })
  const { panel } = await open(shown, () => {
    throw hostError('timeout', 'late')
  })
  await panel.press(button(panel, START) as never)
  await tick(panel)
  assert.equal(button(panel, START)?.props.disabled ?? false, false)
})

test('워커 응답이 끊겨도 잠금을 풀고 실패를 보여 준다', async () => {
  const { panel } = await open(scene({ card: card([item(7)]) }), () => {
    throw hostError('unavailable', 'gone')
  })
  await panel.press(button(panel, '완료') as never)
  assert.ok(panel.find({ text: '완료하지 못했습니다. 패널이 응답을 받지 못했습니다.' }))
  assert.ok(panel.find({ text: '코드 unavailable' }))
  assert.equal(button(panel, '완료')?.props.disabled ?? false, false)
})

test('누른 자리가 사라진 실패는 위로 띄운다', async () => {
  const { panel } = await open(scene({ card: card([item(7)]) }), () => ({ ok: false, code: 'not_found', message: '합성 실패', run: null, snapshot: scene() }))
  await panel.press(button(panel, '완료') as never)
  assert.equal(panel.find({ type: 'Callout' })?.props.title, '완료하지 못했습니다')
  assert.ok(texts(panel).includes('이 일은 이제 목록에 없습니다.'))
  assert.ok(texts(panel).includes('코드 not_found'))
})

test('위로 띄운 실패는 닫을 수 있고 틱으로는 지워지지 않는다', async () => {
  const gone = scene()
  const { state, panel } = await openLive(scene({ card: card([item(7)]) }), () => ({ ok: false, code: 'not_found', message: '합성', run: null, snapshot: gone }))
  await panel.press(button(panel, '완료') as never)
  state.snapshot = gone
  await tick(panel)
  assert.ok(panel.find({ type: 'Callout' }))
  await panel.press(button(panel, '닫기') as never)
  assert.equal(panel.find({ type: 'Callout' }), null)
})

test('새로고침이 성공하면 자리를 잃은 실패를 지운다', async () => {
  const { panel } = await open(scene({ card: card([item(7)]) }), () => ({ ok: false, code: 'not_found', message: '합성', run: null, snapshot: scene() }))
  await panel.press(button(panel, '완료') as never)
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.equal(panel.find({ type: 'Callout' }), null)
})

test('사라진 행의 실패는 펼칠 행을 남기지 않아 틱이 맡겨지지 않는다', async () => {
  const { state, panel } = await openLive(scene({}, [item(7), item(8)]), () => ({ ok: false, code: 'not_found', message: '합성', run: null, snapshot: scene({}, [item(8)]) }))
  await expand(panel, '합성 이슈 7')
  await panel.press(button(panel, '완료') as never)
  assert.equal(panel.find({ type: 'Callout' })?.props.title, '완료하지 못했습니다')
  state.snapshot = scene({}, [item(8), item(9, { title: '새 항목' })])
  await tick(panel)
  assert.ok(titled(panel, '새 항목'))
})

test('펼친 행이 단건 카드가 되면 펼침을 지워 틱이 맡겨지지 않는다', async () => {
  const { state, panel } = await openLive(scene({}, [item(7), item(8)]))
  await expand(panel, '합성 이슈 7')
  state.snapshot = scene({ card: card([item(7)]) }, [item(7), item(8)])
  await panel.press(panel.find({ label: '새로고침' }) as never)
  state.snapshot = scene({ card: card([item(7)]) }, [item(7), item(8), item(9, { title: '새 항목' })])
  await tick(panel)
  assert.ok(titled(panel, '새 항목'))
})

test('처리가 끝나도 그동안 펼쳐 읽던 다른 행은 접지 않는다', { timeout: 5_000 }, async () => {
  const shown = scene({}, [item(1), item(2)])
  const replies: ((value: ActionOutcome) => void)[] = []
  const { panel } = await open(shown, () => new Promise(resolve => replies.push(resolve)))
  await expand(panel, '합성 이슈 1')
  const first = panel.press(panel.find({ testId: 'row-1-later' }) as never)
  await new Promise(resolve => setImmediate(resolve))
  const opening = expand(panel, '합성 이슈 2')
  await new Promise(resolve => setImmediate(resolve))
  replies[0]?.({ ok: true, code: null, message: '합성', run: null, snapshot: shown })
  await Promise.all([first, opening])
  assert.ok(panel.find({ testId: 'row-2-later' }))
  const second = panel.press(panel.find({ testId: 'row-2-later' }) as never)
  await new Promise(resolve => setImmediate(resolve))
  const reopening = expand(panel, '합성 이슈 1')
  await new Promise(resolve => setImmediate(resolve))
  replies[1]?.({ ok: false, code: 'failed', message: '합성', run: null, snapshot: shown })
  await Promise.all([second, reopening])
  assert.ok(panel.find({ testId: 'row-1-later' }))
  assert.equal(panel.find({ testId: 'row-2-later' }), null)
  assert.equal(panel.find({ type: 'Callout' })?.props.title, '미루지 못했습니다')
})

test('처리 중에 행을 접어도 맡아 둔 결과가 그 행을 먼저 지우지 않는다', { timeout: 5_000 }, async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const state = { snapshot: scene({}, [item(1), item(2)]) }
  const host = mockHost({
    capabilities: CAPS,
    requests: {
      'kairos.snapshot': () => state.snapshot,
      'kairos.action': () => new Promise<ActionOutcome>(resolve => (release = resolve)),
    },
  })
  view = await mount(panelModule, { host })
  await expand(view, '합성 이슈 1')
  state.snapshot = scene({}, [item(2)])
  await tick(view)
  const pressed = view.press(view.find({ testId: 'row-1-later' }) as never)
  await new Promise(resolve => setImmediate(resolve))
  const folding = view.press(view.find({ type: 'ListItem', text: /합성 이슈 1/ }) as never)
  await new Promise(resolve => setImmediate(resolve))
  assert.ok(titled(view, '합성 이슈 1'))
  release({ ok: true, code: null, message: '합성', run: null, snapshot: state.snapshot })
  await Promise.all([pressed, folding])
  assert.equal(titled(view, '합성 이슈 1'), null)
})

test('행에서 실패하면 그 행이 펼쳐진 채 실패가 남는다', async () => {
  const shown = scene({}, [item(7)])
  const { panel } = await open(shown, () => ({ ok: false, code: 'failed', message: '합성 실패', run: null, snapshot: shown }))
  await expand(panel, '합성 이슈 7')
  await panel.press(button(panel, '완료') as never)
  assert.ok(panel.find({ text: '완료하지 못했습니다.' }))
  assert.equal(panel.find({ type: 'Callout' }), null)
  assert.ok(button(panel, '완료'))
})

test('두 번 눌러도 요청은 한 번이고 처리 중에는 kairos 버튼이 잠긴다', async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const shown = scene({ card: card([startable(7)]) }, [accepted(8)])
  const { rig, panel } = await open(shown, () => new Promise(resolve => (release = resolve)))
  const target = button(panel, START) as never
  const first = panel.press(target)
  const second = panel.press(target)
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(rig.actions.length, 1)
  assert.ok(texts(panel).includes('세션을 여는 중입니다'))
  assert.ok(panel.findAll({ type: 'Spinner' }).length >= 2)
  assert.equal(panel.find({ label: '새로고침' })?.props.disabled, true)
  release({ ok: true, code: null, message: '수락함', run: null, snapshot: shown })
  await Promise.all([first, second])
  assert.equal(rig.actions.length, 1)
  assert.equal(panel.find({ type: 'Spinner' }), null)
  assert.equal(button(panel, START)?.props.disabled ?? false, false)
})

test('처리 중에는 틱이 와도 다시 읽지 않아 결과를 덮지 않는다', async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const shown = scene({ card: card([item(7)]) })
  const { rig, panel } = await open(shown, () => new Promise(resolve => (release = resolve)))
  const before = rig.snapshots.length
  const pressed = panel.press(button(panel, '완료') as never)
  await new Promise(resolve => setImmediate(resolve))
  const posted = panel.post('kairos.tick', { at: 1 })
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(rig.snapshots.length, before)
  release({ ok: true, code: null, message: '끝냄', run: null, snapshot: scene() })
  await Promise.all([pressed, posted])
})

test('조작 응답의 새 화면을 바로 그리고 색 없는 자취를 남긴다', async () => {
  const { panel } = await open(scene({ card: card([item(7)]) }), () => ({ ok: true, code: null, message: '끝냄', run: null, snapshot: scene() }))
  await panel.press(button(panel, '완료') as never)
  assert.equal(button(panel, '완료'), null)
  const trace = panel.find({ text: '완료했습니다' })
  assert.equal(trace?.props.muted, true)
  assert.equal(trace?.props.tone, undefined)
})

test('성공 자취는 묶음 머리글 칸에 써서 줄을 밀지 않고 다음 읽기에서 지운다', async () => {
  const before = scene({}, [item(3), item(2), item(1)])
  const after = scene({}, [item(3), item(1)])
  const { panel } = await open(before, () => ({ ok: true, code: null, message: '끝냄', run: null, snapshot: after }))
  await expand(panel, '합성 이슈 2')
  await panel.press(button(panel, '나중에') as never)
  const flat = texts(panel)
  const mark = '미뤘습니다 · 내일 11:00에 다시'
  assert.ok(flat.indexOf('오늘 2') < flat.indexOf(mark))
  assert.ok(flat.indexOf(mark) < flat.indexOf('합성 이슈 3'))
  assert.equal(panel.find({ text: mark })?.props.truncate, true)
  await tick(panel)
  assert.equal(panel.find({ text: mark }), null)
})

test('묶음이 비어도 자취는 머리글 없이 남는다', async () => {
  const { panel } = await open(scene({}, [item(1)]), () => ({ ok: true, code: null, message: '끝냄', run: null, snapshot: scene({}, [item(2, { tier: 'P2' })]) }))
  await expand(panel, '합성 이슈 1')
  await panel.press(button(panel, '완료') as never)
  assert.ok(panel.find({ text: '완료했습니다' }))
  assert.equal(panel.find({ type: 'Heading', text: /오늘/ }), null)
})

test('자취는 펼친 행이 있어도 6초 뒤 지운다', async () => {
  const timers = new Map<number, () => void>()
  const original = globalThis.setTimeout
  globalThis.setTimeout = ((run: () => void, delay: number, ...rest: unknown[]) => {
    if (delay !== 6_000) return original(run, delay, ...(rest as []))
    timers.set(timers.size, run)
    return 0 as never
  }) as typeof setTimeout
  try {
    const shown = scene({}, [item(1), item(2)])
    const { panel } = await open(shown, () => ({ ok: true, code: null, message: '끝냄', run: null, snapshot: shown }))
    await expand(panel, '합성 이슈 1')
    await panel.press(button(panel, '완료') as never)
    assert.ok(panel.find({ text: '완료했습니다' }))
    await expand(panel, '합성 이슈 2')
    timers.get(0)?.()
    await panel.settled()
    assert.equal(panel.find({ text: '완료했습니다' }), null)
  } finally {
    globalThis.setTimeout = original
  }
})

test('거절 응답은 이전 화면을 유지한다', async () => {
  const { panel } = await open(scene({ card: card([item(7)]) }), () => ({ ok: false, code: 'busy', message: '앞선 처리가 끝나지 않았습니다', run: null, snapshot: null }))
  await panel.press(button(panel, '완료') as never)
  assert.ok(button(panel, '완료'))
  assert.ok(panel.find({ text: '코드 busy' }))
})

test('이동은 세션이 있을 때만 그리고 항목 ID만 보낸다', async () => {
  const { rig, panel } = await open(scene({ card: card([accepted(7)]), counts: counts({}, { accepted: 1 }) }, [item(8)]))
  assert.equal(panel.findAll({ role: 'button', text: '세션으로 이동' }).length, 1)
  assert.ok(panel.find({ type: 'Text', text: '진행 중' }))
  assert.ok(texts(panel).includes('세션이 진행 중입니다'))
  await panel.press(button(panel, '세션으로 이동') as never)
  assert.deepEqual(rig.actions, [{ action: 'move', ids: [7], seen: true }])
  assert.equal(panel.find({ text: /이동/ })?.type, 'Button')
})

test('진행 중 접힌 행에만 이동이 있고 펼치면 사라진다', async () => {
  const { rig, panel } = await open(scene({}, [accepted(7, 'waiting_user'), item(8)]))
  assert.equal(button(panel, '이동'), null)
  await tabTo(panel, 'sessions')
  assert.equal(panel.findAll({ role: 'button', text: '이동' }).length, 1)
  assert.equal(panel.find({ type: 'Text', text: '확인을 기다립니다' })?.props.weight, 'medium')
  await panel.press(button(panel, '이동') as never)
  assert.deepEqual(rig.actions, [{ action: 'move', ids: [7], seen: false, ready: true }])
  await expand(panel, '합성 이슈 7')
  assert.equal(button(panel, '이동'), null)
  assert.ok(button(panel, '세션으로 이동'))
})

test('소식이 떠 있으면 행은 소식과 같은 말을 쓰고 빠른 이동을 숨긴다', async () => {
  const { panel } = await open(scene({ ready: [readyCard(7)] }, [accepted(7, 'waiting_user')]))
  await tabTo(panel, 'sessions')
  assert.equal(panel.findAll({ role: 'button', text: '이동' }).length, 0)
  assert.equal(panel.findAll({ role: 'button', text: '세션으로 이동' }).length, 1)
  assert.equal(panel.findAll({ type: 'Text', text: '결과가 준비됐습니다' }).length, 2)
  assert.ok(!texts(panel).includes('확인을 기다립니다'))
  await panel.press(panel.find({ label: '닫기' }) as never)
  assert.ok(button(panel, '이동'))
  assert.ok(texts(panel).includes('확인을 기다립니다'))
})

test('접혀서 안 보이는 소식은 행의 이동을 숨기지 않는다', async () => {
  const ready = [sessionNews('ended', 1, 'run-1'), sessionNews('done', 2, 'run-2'), sessionNews('done', 3, 'run-3'), sessionNews('done', 4, 'run-4')]
  const { panel } = await open(scene({ ready }, [accepted(1)]))
  await tabTo(panel, 'sessions')
  assert.ok(button(panel, '이동'))
})

test('끝난 세션의 행에는 빠른 이동이 없고 머리글은 세션이다', async () => {
  const { panel } = await open(scene({}, [revivable(1)]))
  await tabTo(panel, 'sessions')
  assert.equal(button(panel, '이동'), null)
  assert.ok(panel.find({ type: 'Heading', text: '세션 1' }))
  assert.match(titled(panel, '합성 이슈 1')?.text ?? '', /세션이 끝났습니다/)
})

test('끝난 세션의 단건 카드는 진행 중이라고 말하지 않는다', async () => {
  const { panel } = await open(scene({ card: card([revivable(1)]) }))
  assert.ok(panel.find({ type: 'Text', text: '끝난 세션' }))
  assert.equal(panel.find({ type: 'Text', text: '진행 중' }), null)
})

test('닫은 소식은 틱으로 같은 카드가 와도 다시 나오지 않고 노출로 세지 않는다', async () => {
  const { state, panel } = await openLive(scene())
  state.snapshot = scene({ ready: [readyCard(7, 'run-1')] }, [accepted(7)])
  await tick(panel)
  assert.ok(texts(panel).includes('결과가 준비됐습니다'))
  const reads = state.reads
  await panel.press(panel.find({ label: '닫기' }) as never)
  assert.equal(state.reads, reads)
  await tick(panel)
  assert.ok(!texts(panel).includes('결과가 준비됐습니다'))
})

test('닫은 세션이라도 새 종류의 소식은 다시 나온다', async () => {
  const { state, panel } = await openLive(scene())
  state.snapshot = scene({ ready: [sessionNews('question', 7, 'run-1')] }, [accepted(7)])
  await tick(panel)
  await panel.press(panel.find({ label: '닫기' }) as never)
  state.snapshot = scene({ ready: [sessionNews('done', 7, 'run-1')] }, [accepted(7)])
  await tick(panel)
  assert.ok(texts(panel).includes('결과가 준비됐습니다'))
})

test('같은 세션의 소식이 바뀌면 새 문구로 하나만 남는다', async () => {
  const { state, panel } = await openLive(scene({ ready: [sessionNews('question', 7, 'run-1')] }))
  state.snapshot = scene({ ready: [sessionNews('done', 7, 'run-1')] })
  await tick(panel)
  assert.equal(panel.findAll({ text: '결과가 준비됐습니다' }).length, 1)
  assert.ok(!texts(panel).includes('답을 기다립니다'))
})

test('세션 소식은 다음 읽기에 빠져도 닫을 때까지 남는다', async () => {
  const { state, panel } = await openLive(scene({ ready: [readyCard(7)] }, [accepted(7)]))
  assert.ok(texts(panel).includes('결과가 준비됐습니다'))
  state.snapshot = scene({}, [accepted(7)])
  await tick(panel)
  assert.ok(texts(panel).includes('결과가 준비됐습니다'))
  await panel.press(panel.find({ label: '닫기' }) as never)
  assert.ok(!texts(panel).includes('결과가 준비됐습니다'))
})

test('같은 세션 소식이 다시 와도 하나만 그린다', async () => {
  const { panel } = await open(scene({ ready: [readyCard(7)] }))
  await tick(panel)
  assert.equal(panel.findAll({ text: '결과가 준비됐습니다' }).length, 1)
})

test('세션 소식의 이동은 항목 ID만 보내고 성공하면 내린다', async () => {
  const { rig, panel } = await open(scene({ ready: [readyCard(7)] }), () => ({ ok: true, code: null, message: '이동했습니다', run: null, snapshot: scene() }))
  await panel.press(button(panel, '세션으로 이동') as never)
  assert.deepEqual(rig.actions, [{ action: 'move', ids: [7], seen: true }])
  assert.ok(!texts(panel).includes('결과가 준비됐습니다'))
})

test('이동이 실패하면 세션 소식을 남기고 그 줄 아래에 실패를 보여 준다', async () => {
  const shown = scene({ ready: [readyCard(7)] })
  const { panel } = await open(shown, () => ({ ok: false, code: 'session_gone', message: '세션의 터미널이 없습니다', run: null, snapshot: shown }))
  await panel.press(button(panel, '세션으로 이동') as never)
  assert.ok(texts(panel).includes('결과가 준비됐습니다'))
  assert.ok(panel.find({ text: '세션으로 이동하지 못했습니다. 세션의 터미널이 닫혀 있습니다.' }))
  assert.equal(panel.find({ type: 'Callout' }), null)
})

test('세션 소식은 종류별 문구를 쓰고 실패가 맨 위이며 3건을 넘으면 접는다', async () => {
  const ready = [
    sessionNews('ended', 1, 'run-1'),
    sessionNews('done', 2, 'run-2'),
    sessionNews('attention', 3, 'run-3'),
    sessionNews('question', 4, 'run-4'),
    failedCard(5, 'run-5'),
  ]
  const { panel } = await open(scene({ ready }))
  const flat = texts(panel)
  assert.ok(flat.indexOf('세션이 시작하자마자 끝났습니다') < flat.indexOf('답을 기다립니다'))
  assert.ok(flat.indexOf('답을 기다립니다') < flat.indexOf('세션 확인이 필요합니다'))
  assert.ok(!flat.includes('결과가 준비됐습니다'))
  await panel.press(button(panel, '세션 소식 2건 더') as never)
  assert.ok(texts(panel).includes('결과가 준비됐습니다'))
  assert.ok(texts(panel).includes('세션이 끝났습니다'))
  await panel.press(button(panel, '세션 소식 접기') as never)
  assert.ok(!texts(panel).includes('세션이 끝났습니다'))
})

test('배포 연결 남김은 keep_deploy로 보낸다', async () => {
  const actions = [{ id: 'keep', label: '배포 때 다시', argv: ['keep', '7', '--until-deploy', '--json'] }]
  const { rig, panel } = await open(scene({ card: card([item(7, { actions })]) }))
  await panel.press(button(panel, '배포 때 다시') as never)
  assert.deepEqual(rig.actions, [{ action: 'keep_deploy', ids: [7], seen: true }])
  assert.ok(panel.find({ text: '배포 때 다시 보여 드립니다' }))
})

test('합친 카드는 건수를 쓰고 ID를 모두 보낸다', async () => {
  const { rig, panel } = await open(scene({ card: merged([item(7), item(8)]) }))
  assert.ok(panel.find({ type: 'Text', text: '같은 채널 2건' }))
  await panel.press(button(panel, '2건 모두 나중에') as never)
  assert.deepEqual(rig.actions, [{ action: 'later', ids: [7, 8], seen: true }])
  assert.ok(panel.find({ text: '2건을 미뤘습니다 · 내일 11:00에 다시' }))
})

test('합친 카드의 줄을 펼치면 전체 제목과 원문 열기만 있다', async () => {
  const { panel } = await open(scene({ card: merged([item(7), item(8)]) }))
  await expand(panel, '합성 이슈 8')
  assert.ok(button(panel, '원문 열기'))
  assert.equal(button(panel, '나중에'), null)
  assert.equal(button(panel, '완료'), null)
})

test('묶음의 줄은 펼쳐서 처리하고 확인은 ID 없이 보낸다', async () => {
  const { rig, panel } = await open(scene({ card: bundle([item(7, { tier: 'P2' }), item(8, { tier: 'P2' })], 3) }))
  assert.ok(panel.find({ type: 'Text', text: '모아 둔 일 5건' }))
  assert.ok(panel.find({ text: '나머지 3건은 목록에 있습니다.' }))
  assert.equal(button(panel, '완료'), null)
  await expand(panel, '합성 이슈 8')
  await panel.press(button(panel, '완료') as never)
  assert.deepEqual(rig.actions.at(-1), { action: 'done', ids: [8], seen: true })
  await panel.press(button(panel, '다 봤습니다') as never)
  assert.deepEqual(rig.actions.at(-1), { action: 'ack', ids: [], seen: true })
  assert.ok(panel.find({ text: '남은 일은 목록에 있습니다' }))
})

test('카드 안에서 펼친 줄은 경계가 있고 다른 곳의 펼친 행은 상자가 경계다', async () => {
  const { panel } = await open(scene({ card: bundle([item(7, { tier: 'P2' }), item(8, { tier: 'P2' })]) }, [item(9)]))
  await expand(panel, '합성 이슈 7')
  assert.equal(panel.find({ type: 'ListItem', text: /합성 이슈 7/ })?.props.current, true)
  assert.ok(panel.find({ type: 'Separator' }))
  await expand(panel, '합성 이슈 9')
  assert.equal(panel.find({ type: 'ListItem', text: /합성 이슈 9/ })?.props.current ?? false, false)
  assert.equal(panel.find({ type: 'Separator' }), null)
})

test('왼쪽이 빈 뒷줄은 그리지 않고 완료를 주 행동 줄로 올린다', async () => {
  const { panel } = await open(scene({ card: card([accepted(1)]) }))
  const zone = texts(panel)
  assert.ok(zone.indexOf('세션으로 이동') < zone.indexOf('완료'))
  assert.equal(hollow(panel.tree(), 'Row'), 0)
})

test('합친 줄을 펼쳐도 주소가 없으면 빈 층을 그리지 않는다', async () => {
  const { panel } = await open(scene({ card: merged([item(7, { url: null }), item(8)]) }))
  await expand(panel, '합성 이슈 7')
  assert.equal(button(panel, '원문 열기'), null)
  assert.equal(hollow(panel.tree(), 'Stack'), 0)
  assert.equal(hollow(panel.tree(), 'Box'), 0)
})

test('넘어온 카드는 건수와 시작일을 쓴다', async () => {
  const { panel } = await open(scene({ card: carryover([item(7, { carried: true, openedAt: '2026-11-03T02:00:00.000Z' })]) }))
  assert.ok(panel.find({ type: 'Text', text: '어제까지 남은 일 1건' }))
  assert.ok(panel.find({ type: 'Text', text: '어제부터' }))
  assert.ok(button(panel, '다 봤습니다'))
})

test('행의 버튼은 그 줄의 ID를 보내고 한 번에 한 행만 펼친다', async () => {
  const { rig, panel } = await open(scene({ card: card([item(7)]) }, [item(7), item(8), item(9)]))
  assert.equal(panel.find({ testId: 'row-8-keep' }), null)
  await expand(panel, '합성 이슈 8')
  await expand(panel, '합성 이슈 9')
  assert.equal(panel.find({ testId: 'row-8-keep' }), null)
  await panel.press(panel.find({ testId: 'row-9-keep' }) as never)
  assert.deepEqual(rig.actions, [{ action: 'keep', ids: [9], seen: true }])
  assert.ok(panel.find({ text: '새 소식이 오면 보여 드립니다' }))
  assert.equal(panel.find({ text: /할 일로/ }), null)
  assert.equal(panel.find({ testId: 'row-9-keep' }), null)
})

test('접힌 행에는 버튼이 없고 처리 중에도 펼칠 수 있다', async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const shown = scene({ card: card([item(1)]) }, [item(2)])
  const { panel } = await open(shown, () => new Promise(resolve => (release = resolve)))
  assert.equal(panel.findAll({ type: 'Button', text: '완료' }).length, 1)
  const pressed = panel.press(button(panel, '완료') as never)
  await new Promise(resolve => setImmediate(resolve))
  const expanding = expand(panel, '합성 이슈 2')
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(button(panel, '완료')?.props.disabled, true)
  release({ ok: true, code: null, message: '끝냄', run: null, snapshot: shown })
  await Promise.all([pressed, expanding])
})

test('처리 중인 행이 접혀도 메타 줄이 처리 중을 말한다', async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const shown = scene({}, [item(1), item(2)])
  const { panel } = await open(shown, () => new Promise(resolve => (release = resolve)))
  await expand(panel, '합성 이슈 1')
  const pressed = panel.press(button(panel, '완료') as never)
  await new Promise(resolve => setImmediate(resolve))
  const expanding = expand(panel, '합성 이슈 2')
  await new Promise(resolve => setImmediate(resolve))
  assert.match(titled(panel, '합성 이슈 1')?.text ?? '', /처리하는 중입니다/)
  release({ ok: true, code: null, message: '끝냄', run: null, snapshot: shown })
  await Promise.all([pressed, expanding])
})

test('행은 묶음으로 나뉘고 묶음 안에서는 최신이 위다', async () => {
  const { panel } = await open(
    scene({}, [
      item(1, { tier: 'P1', title: '오늘 오래됨', openedAt: '2026-11-03T00:00:00.000Z' }),
      item(2, { tier: 'P1', title: '오늘 새것', openedAt: '2026-11-04T00:00:00.000Z' }),
      item(3, { tier: 'P2', title: '여유', openedAt: '2026-11-04T01:00:00.000Z' }),
      item(4, { tier: 'P0', title: '긴급 건', openedAt: '2026-11-01T00:00:00.000Z' }),
      accepted(5),
    ]),
  )
  const heads = () => panel.findAll({ type: 'Heading' }).map(found => found.text).slice(1)
  assert.deepEqual(rowTitles(panel), ['긴급 건', '오늘 새것', '오늘 오래됨'])
  assert.deepEqual(heads(), ['긴급 1', '오늘 2'])
  assert.equal(panel.find({ type: 'Text', text: '긴급 1' })?.props.tone, 'danger')
  await tabTo(panel, 'waiting')
  assert.deepEqual(rowTitles(panel), ['여유'])
  assert.deepEqual(heads(), ['여유 있을 때 1'])
  await tabTo(panel, 'sessions')
  assert.deepEqual(rowTitles(panel), ['합성 이슈 5'])
  assert.deepEqual(heads(), ['세션 1'])
})

test('미룬 일과 이전부터 있던 일은 대기 탭에 있고 이전부터 있던 일은 접혀 건수만 보인다', async () => {
  const { panel } = await open(scene({}, [parked(1), item(2, { baseline: true }), item(3)]))
  assert.deepEqual(rowTitles(panel), ['합성 이슈 3'])
  assert.equal(panel.find({ type: 'Heading', text: '미룬 일 1' }), null)
  await tabTo(panel, 'waiting')
  assert.deepEqual(rowTitles(panel), ['합성 이슈 1'])
  assert.ok(panel.find({ type: 'Heading', text: '미룬 일 1' }))
  assert.ok(panel.find({ type: 'Heading', text: '이전부터 있던 일 1' }))
  assert.match(titled(panel, '합성 이슈 1')?.text ?? '', /10:00에 다시/)
  await panel.press(panel.find({ type: 'ListItem', text: /이전부터 있던 일 1/ }) as never)
  assert.deepEqual(rowTitles(panel), ['합성 이슈 1', '합성 이슈 2'])
})

test('미룬 일은 다시 볼 때를 쓴다', async () => {
  const { panel } = await open(scene({}, [parked(1, { parkTrigger: 'deploy' }), parked(2, { parkTrigger: 'kept' }), parked(3, { parkedUntil: null })]))
  await tabTo(panel, 'waiting')
  assert.ok(panel.find({ type: 'Heading', text: '미룬 일 3' }))
  assert.match(titled(panel, '합성 이슈 1')?.text ?? '', /배포 때 다시/)
  assert.match(titled(panel, '합성 이슈 2')?.text ?? '', /새 소식이 오면 다시/)
  assert.match(titled(panel, '합성 이슈 3')?.text ?? '', /미룸/)
})

test('직접 추가한 일은 출처를 쓰지 않는다', async () => {
  const { panel } = await open(scene({}, [manual(1)]))
  await tabTo(panel, 'waiting')
  await expand(panel, '합성 할 일 1')
  const text = titled(panel, '합성 할 일 1')?.text ?? ''
  assert.match(text, /직접 추가/)
  assert.doesNotMatch(text, /kairos/)
})

test('일정과 배포 위험은 시각을 쓴다', async () => {
  const { panel } = await open(scene({}, [item(1, { reason: 'meeting_new', dueAt: '2026-11-04T05:20:00.000Z' })]))
  assert.match(titled(panel, '합성 이슈 1')?.text ?? '', /14:20/)
})

test('펼친 행은 배포 연결과 미룬 횟수를 쓴다', async () => {
  const { panel } = await open(scene({}, [item(1, { deploy: { date: '2026-11-07', linkedCount: 2 }, pushCount: 2 })]))
  await expand(panel, '합성 이슈 1')
  assert.ok(panel.find({ text: '11월 7일 배포에 연결되어 있습니다 · 2번 미뤘습니다' }))
})

test('여유 있을 때가 6건이면 미룬 일은 접히고 고른 값은 남는다', async () => {
  const rows = [parked(1), ...Array.from({ length: 6 }, (_, index) => item(index + 10, { tier: 'P2' }))]
  const { rig, panel } = await open(scene({}, rows))
  await tabTo(panel, 'waiting')
  const before = rig.snapshots.length
  assert.equal(rowTitles(panel).length, 6)
  await panel.press(panel.find({ type: 'ListItem', text: /미룬 일 1/ }) as never)
  assert.equal(rowTitles(panel).length, 7)
  assert.deepEqual(panel.persisted.plugin.folds, { parked: true })
  await panel.press(panel.find({ type: 'ListItem', text: /미룬 일 1/ }) as never)
  assert.equal(rowTitles(panel).length, 6)
  assert.deepEqual(panel.persisted.plugin.folds, { parked: false })
  assert.equal(rig.snapshots.length, before)
})

test('묶음을 접으면 그 안의 펼친 행도 접고 맡아 둔 값을 반영한다', async () => {
  const { state, panel } = await openLive(scene({}, [parked(10), parked(11)]))
  await tabTo(panel, 'waiting')
  await expand(panel, '합성 이슈 10')
  state.snapshot = scene({ counts: counts({ P1: 9 }) }, [item(1)])
  await tick(panel)
  assert.deepEqual(tabLabels(panel), ['지금', '대기 2', '세션'])
  await panel.press(panel.find({ type: 'ListItem', text: /미룬 일 2/ }) as never)
  assert.ok(panel.find({ text: '9건' }))
  assert.deepEqual(tabLabels(panel), ['지금 1', '대기', '세션'])
})

test('여유 있을 때는 길어도 접는 머리글이 없다', async () => {
  const { panel } = await open(scene({}, Array.from({ length: 6 }, (_, index) => item(index + 10, { tier: 'P2' }))))
  await tabTo(panel, 'waiting')
  assert.equal(panel.find({ text: '접기' }), null)
  assert.equal(panel.find({ type: 'ListItem', text: /여유 있을 때 6/ })?.props.onPress, undefined)
})

test('고정 머리글과 접는 머리글은 같은 틀이다', async () => {
  const { panel } = await open(scene({}, [item(1, { tier: 'P2' }), parked(2)]))
  await tabTo(panel, 'waiting')
  const heads = panel.findAll({ type: 'ListItem' }).filter(found => found.key?.endsWith('-item') !== true)
  assert.equal(heads.length, 2)
  assert.equal(heads[0]?.props.onPress, undefined)
  assert.notEqual(heads[1]?.props.onPress, undefined)
})

test('한 묶음이 길면 최신 줄만 보이고 더 보기로 펼치며 고른 값이 남는다', async () => {
  const rows = Array.from({ length: 12 }, (_, index) => item(index + 1, { openedAt: new Date(Date.parse('2026-11-04T01:00:00.000Z') - index * 60_000).toISOString() }))
  const { rig, panel } = await open(scene({}, rows))
  const reads = rig.snapshots.length
  assert.equal(rowTitles(panel).length, 8)
  assert.equal(rowTitles(panel)[0], '합성 이슈 1')
  await panel.press(panel.find({ type: 'ListItem', text: '4건 더 보기' }) as never)
  assert.equal(rowTitles(panel).length, 12)
  assert.deepEqual(panel.persisted.plugin.folds, { 'today:all': true })
  await panel.press(panel.find({ type: 'ListItem', text: '최신 줄만 보기' }) as never)
  assert.equal(rowTitles(panel).length, 8)
  assert.equal(rig.snapshots.length, reads)
})

test('긴급도 길면 더 보기로 줄이고 세션 줄은 줄이지 않는다', async () => {
  const urgent = Array.from({ length: 11 }, (_, index) => item(index + 1, { tier: 'P0' }))
  const running = Array.from({ length: 11 }, (_, index) => accepted(index + 100))
  const { panel } = await open(scene({}, [...urgent, ...running]))
  assert.equal(rowTitles(panel).length, 8)
  assert.ok(panel.find({ type: 'ListItem', text: '3건 더 보기' }))
  await tabTo(panel, 'sessions')
  assert.equal(rowTitles(panel).length, 11)
  assert.equal(panel.find({ type: 'ListItem', text: /더 보기/ }), null)
})

test('펼친 행은 더 보기를 접어도 숨기지 않는다', async () => {
  const rows = Array.from({ length: 12 }, (_, index) => item(index + 1, { openedAt: new Date(Date.parse('2026-11-04T01:00:00.000Z') - index * 60_000).toISOString() }))
  const { panel } = await open(scene({}, rows))
  await panel.press(panel.find({ type: 'ListItem', text: '4건 더 보기' }) as never)
  await expand(panel, '합성 이슈 11')
  await panel.press(panel.find({ type: 'ListItem', text: '최신 줄만 보기' }) as never)
  assert.ok(button(panel, '완료'))
  assert.ok(panel.find({ type: 'ListItem', text: '1건 더 보기' }))
})

test('펼치고 접어도 kairos를 부르지 않는다', async () => {
  const { rig, panel } = await open(scene({}, [item(1), parked(2)]))
  const before = rig.snapshots.length
  for (let round = 0; round < 3; round += 1) {
    await expand(panel, '합성 이슈 1')
    await panel.press(panel.find({ type: 'ListItem', text: /합성 이슈 1/ }) as never)
    await tabTo(panel, 'waiting')
    await panel.press(panel.find({ type: 'ListItem', text: /미룬 일/ }) as never)
    await toggleStatus(panel)
    await toggleStatus(panel)
    // 카드가 없으면 지금 탭으로 돌아와도 읽지 않는다.
    await tabTo(panel, 'now')
  }
  assert.equal(rig.snapshots.length, before)
  assert.deepEqual(rig.actions, [])
})

test('펼친 행이 있으면 틱 결과를 맡아 두고 다섯 번째 틱에서 반영한다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1), item(2)]))
  await expand(panel, '합성 이슈 2')
  state.snapshot = scene({}, [item(1, { title: '새 제목' }), item(2)])
  for (let round = 0; round < 4; round += 1) {
    await tick(panel)
    assert.ok(titled(panel, '합성 이슈 1'))
  }
  await tick(panel)
  assert.ok(titled(panel, '새 제목'))
})

test('맡아 두는 동안에도 세션 소식은 받아 두고 머리말 건수는 최신이다', async () => {
  const { state, panel } = await openLive(scene({ counts: counts({ P1: 2 }) }, [item(1), item(2)]))
  await expand(panel, '합성 이슈 1')
  state.snapshot = scene({ ready: [readyCard(9)], counts: counts({ P1: 5 }) }, [item(1), item(2)])
  await tick(panel)
  assert.ok(texts(panel).includes('결과가 준비됐습니다'))
  assert.ok(panel.find({ text: '5건' }))
})

test('접으면 맡아 둔 결과를 반영한다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1), item(2)]))
  await expand(panel, '합성 이슈 2')
  state.snapshot = scene({}, [item(1, { title: '새 제목' }), item(2)])
  await tick(panel)
  assert.ok(titled(panel, '합성 이슈 1'))
  await panel.press(panel.find({ type: 'ListItem', text: /합성 이슈 2/ }) as never)
  assert.ok(titled(panel, '새 제목'))
})

test('새로고침과 버튼은 맡아 둔 결과를 덮고 바로 반영한다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1), item(2)]))
  await expand(panel, '합성 이슈 2')
  state.snapshot = scene({}, [item(1, { title: '새 제목' }), item(2)])
  await tick(panel)
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.ok(titled(panel, '새 제목'))
})

test('반영 뒤 펼친 행이 사라지면 비운다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1)]))
  await expand(panel, '합성 이슈 1')
  state.snapshot = scene({}, [item(2)])
  await panel.press(panel.find({ label: '새로고침' }) as never)
  state.snapshot = scene({}, [item(1), item(2)])
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.ok(titled(panel, '합성 이슈 1'))
})

test('긴 제목은 카드와 펼친 행에서 120자이고 손댈 줄은 두 줄 길이로 줄인다', async () => {
  const { panel } = await open(scene({ card: card([item(1, { title: LONG_TITLE })]) }, [item(2, { title: LONG_TITLE })]))
  const [row] = rowsOf(panel)
  assert.equal([...String(rowTitle(panel, row as never))].length, 46)
  assert.equal([...String(panel.find({ type: 'Text', text: /^합성 긴 제목/ })?.text)].length, 120)
  await panel.press(row as never)
  assert.equal(panel.findAll({ type: 'Text', text: /^합성 긴 제목/ }).every(found => [...String(found.text)].length === 120), true)
  assert.deepEqual(panel.errors, [])
})

test('위험 문자가 든 제목은 글자로만 그린다', async () => {
  const danger = item(1, { title: DANGEROUS_TITLE })
  const { panel } = await open(scene({ card: card([danger]) }, [item(2, { title: DANGEROUS_TITLE })]))
  assert.equal(panel.find({ type: 'Text', text: DANGEROUS_TITLE })?.type, 'Text')
  assert.ok(rowsOf(panel).some(row => rowTitle(panel, row)?.startsWith('$(touch /tmp/x)')))
  assert.equal(panel.find({ type: 'Markdown' }), null)
  assert.equal(panel.find({ type: 'Link' }), null)
  assert.equal(panel.find({ type: 'Image' }), null)
  assert.deepEqual(panel.errors, [])
})

test('사용자 화면에 내부 등급 이름과 Slack 원문 표기와 주소가 없다', async () => {
  const rows = [item(1, { title: SLACK_TITLE, tier: 'P0' }), startable(2), item(3, { tier: 'P2' }), accepted(4), parked(5)]
  const { panel } = await open(scene({ card: card([item(9, { title: SLACK_TITLE })]), ready: [readyCard(4), failedCard(2)], counts: counts({ P0: 1, P1: 2, P2: 1 }) }, rows))
  await expand(panel, '합성 이슈 2')
  const flat = texts(panel)
  for (const word of ['P0', 'P1', 'P2', '<@', '<#', 'https://', 'http://']) assert.ok(!flat.includes(word), word)
  assert.ok(flat.includes('@합성이름 님이 #합성채널에서 합성 링크'))
})

test('주소를 그리지 않고 원문 열기는 사용자가 누를 때만 연다', async () => {
  const { rig, panel } = await open(scene({ card: card([item(1)]) }))
  assert.ok(!texts(panel).includes('github.com'))
  assert.equal(panel.find({ type: 'Link' }), null)
  assert.deepEqual(panel.host.callsOf('shell.openExternal'), [])
  const reads = rig.snapshots.length
  await panel.press(button(panel, '원문 열기') as never)
  assert.deepEqual(panel.host.callsOf('shell.openExternal'), [{ url: 'https://github.com/example/repo/issues/1' }])
  assert.deepEqual(panel.refusals, [])
  assert.equal(rig.snapshots.length, reads)
  assert.deepEqual(rig.actions, [])
})

test('허용 밖 주소는 원문 열기를 그리지 않는다', async () => {
  for (const url of ['https://example.test/a', 'https://github.com.evil.test/a', 'https://slack.com/a']) {
    const { panel } = await open(scene({ card: card([item(1, { url })]) }))
    assert.equal(button(panel, '원문 열기'), null, url)
    panel.unmount()
  }
})

test('진행 중인 항목에는 원문 열기를 두지 않고 미룬 일에는 둔다', async () => {
  const first = await open(scene({ card: card([accepted(1)]) }))
  assert.equal(button(first.panel, '원문 열기'), null)
  first.panel.unmount()
  const second = await open(scene({ card: card([parked(2)]) }))
  assert.ok(button(second.panel, '원문 열기'))
})

test('원문 열기는 처리 중에도 눌린다', async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const shown = scene({ card: card([item(1)]) }, [item(2)])
  const { rig, panel } = await open(shown, () => new Promise(resolve => (release = resolve)))
  await expand(panel, '합성 이슈 2')
  const pressed = panel.press(panel.find({ testId: 'card-done' }) as never)
  await new Promise(resolve => setImmediate(resolve))
  const opener = button(panel, '원문 열기')
  assert.equal(opener?.props.disabled ?? false, false)
  const opening = panel.press(opener as never)
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(panel.host.callsOf('shell.openExternal').length, 1)
  assert.equal(rig.actions.length, 1)
  release({ ok: true, code: null, message: '끝냄', run: null, snapshot: shown })
  await Promise.all([pressed, opening])
})

test('원문 열기가 막히면 이유를 그 자리에 쓰고 취소는 조용히 넘긴다', async () => {
  const { panel } = await open(scene({ card: card([item(1)]) }))
  panel.host.respond('shell.openExternal', { opened: false, reason: 'consent-unavailable' })
  await panel.press(button(panel, '원문 열기') as never)
  assert.ok(panel.find({ text: 'Orca 창이 가려져 있어 확인을 받지 못했습니다.' }))
  panel.host.respond('shell.openExternal', { opened: false, reason: 'cancelled' })
  await panel.press(button(panel, '원문 열기') as never)
  assert.equal(panel.find({ text: 'Orca 창이 가려져 있어 확인을 받지 못했습니다.' }), null)
})

test('보일 때 읽으며 seen을 주고 안 보이면 부르지 않는다', async () => {
  const hidden = await open(scene({ card: card([item(1)]) }), undefined, false)
  assert.deepEqual(hidden.rig.snapshots, [])
  await hidden.panel.post('kairos.tick', { at: 1 })
  assert.deepEqual(hidden.rig.snapshots, [])
  await hidden.panel.setVisible(true)
  await hidden.panel.settled()
  assert.deepEqual(hidden.rig.snapshots, [{ seen: true }])
  assert.ok(button(hidden.panel, '나중에'))
})

test('틱은 다시 그리기만 하고 노출로 남기지 않는다. 연 순간과 새로고침만 seen이다', async () => {
  const { rig, panel } = await open(scene({ card: card([item(1)]) }))
  assert.deepEqual(rig.snapshots, [{ seen: true }])
  await panel.post('kairos.tick', { at: 1 })
  assert.deepEqual(rig.snapshots.at(-1), { seen: false })
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: true })
  await panel.setVisible(false)
  await panel.setVisible(true)
  await panel.settled()
  assert.deepEqual(rig.snapshots.at(-1), { seen: true })
})

test('늦게 온 응답의 세션 소식은 버리지 않는다. 화면 상태만 새 응답을 따른다', { timeout: 5_000 }, async () => {
  const replies: ((value: Snapshot) => void)[] = []
  const asked = async (count: number): Promise<void> => {
    while (replies.length < count) await new Promise(resolve => setImmediate(resolve))
  }
  const host = mockHost({ capabilities: CAPS, requests: { 'kairos.snapshot': () => new Promise<Snapshot>(resolve => replies.push(resolve)) } })
  const mounting = mount(panelModule, { host })
  await asked(1)
  replies[0]?.(scene())
  view = await mounting
  const posted = view.post('kairos.tick', { at: 1 })
  await asked(2)
  const refresh = view.press(view.find({ label: '새로고침' }) as never)
  await asked(3)
  replies[2]?.(scene({}, [item(1, { title: '새 응답' })]))
  await new Promise(resolve => setImmediate(resolve))
  replies[1]?.(scene({ ready: [readyCard(7)] }, [item(1, { title: '옛 응답' })]))
  await Promise.all([posted, refresh])
  await view.settled()
  assert.ok(texts(view).includes('결과가 준비됐습니다'))
  assert.ok(texts(view).includes('새 응답'))
  assert.ok(!texts(view).includes('옛 응답'))
})

test('끝난 세션의 항목에서 되돌리기를 누르면 항목 ID만 보내고 세션 소식을 내린다', async () => {
  const { rig, panel } = await open(scene({ ready: [readyCard(7)] }, [revivable(7)]), () => ({ ok: true, code: null, message: '되돌렸습니다', run: null, snapshot: scene() }))
  await tabTo(panel, 'sessions')
  assert.match(titled(panel, '합성 이슈 7')?.text ?? '', /결과가 준비됐습니다/)
  await expand(panel, '합성 이슈 7')
  assert.ok(panel.find({ text: '되돌려도 세션은 그대로 남습니다.' }))
  await panel.press(button(panel, '되돌리기') as never)
  assert.deepEqual(rig.actions, [{ action: 'cancel', ids: [7], seen: false, ready: true }])
  assert.ok(!texts(panel).includes('결과가 준비됐습니다'))
  assert.ok(panel.find({ text: '되돌렸습니다' }))
})

test('실패 소식은 이동 없이 사유를 보여 주고 게이트가 닫혀 있어도 나온다', async () => {
  const closed = scene({ display: recordingGate, ready: [failedCard(1)] })
  const { panel } = await open(closed)
  assert.ok(texts(panel).includes('세션이 시작하자마자 끝났습니다'))
  assert.ok(texts(panel).includes('제안이 다시 보이면 목록에서 찾을 수 있습니다.'))
  assert.equal(button(panel, '세션으로 이동'), null)
  assert.equal(button(panel, '다시 시작'), null)
  await panel.press(panel.find({ label: '닫기' }) as never)
  assert.ok(!texts(panel).includes('세션이 시작하자마자 끝났습니다'))
})

test('실패 소식의 다시 시작은 항목이 시작 가능할 때만 있고 성공하면 소식을 닫는다', async () => {
  const { rig, panel } = await open(scene({ ready: [failedCard(1)] }, [startable(1)]), () => ({ ok: true, code: null, message: '세션을 여는 중입니다', run: null, snapshot: scene({}, [accepted(1)]) }))
  assert.ok(texts(panel).includes('이 일은 목록으로 돌아왔습니다.'))
  await panel.press(button(panel, '다시 시작') as never)
  assert.deepEqual(rig.actions, [{ action: 'start', ids: [1], seen: true }])
  assert.ok(!texts(panel).includes('세션이 시작하자마자 끝났습니다'))
  assert.ok(panel.find({ text: '세션을 여는 중입니다' }))
  panel.unmount()
  const fromRow = await open(scene({ ready: [failedCard(1)] }, [startable(1)]), () => ({ ok: true, code: null, message: '합성', run: null, snapshot: scene({}, [accepted(1)]) }))
  await expand(fromRow.panel, '합성 이슈 1')
  await fromRow.panel.press(fromRow.panel.find({ testId: 'row-1-start' }) as never)
  assert.ok(!texts(fromRow.panel).includes('이 일은 목록으로 돌아왔습니다.'))
  fromRow.panel.unmount()
  const without = await open(scene({ ready: [failedCard(1)] }, [item(1)]))
  assert.equal(button(without.panel, '다시 시작'), null)
})

test('게이트가 닫히면 일반 세션 소식은 그리지 않는다', async () => {
  const { panel } = await open(scene({ display: recordingGate, ready: [readyCard(1)] }))
  assert.equal(button(panel, '세션으로 이동'), null)
})

test('닫힌 항목의 살아 있는 세션은 목록에 이동만 남는다', async () => {
  const closed = { ...accepted(7), state: 'closed', actions: [] }
  const { panel } = await open(scene({}, [closed]))
  await tabTo(panel, 'sessions')
  assert.match(titled(panel, '합성 이슈 7')?.text ?? '', /완료됨 · 세션이 진행 중입니다/)
  assert.ok(button(panel, '이동'))
  await expand(panel, '합성 이슈 7')
  assert.ok(button(panel, '세션으로 이동'))
  assert.equal(button(panel, '완료'), null)
})

test('건수가 많아도 그리기가 거절되지 않고 끝줄이 뺀 건수를 알린다', async () => {
  const { panel } = await open(scene({ counts: counts({ P1: 120 }) }, manyItems(120)))
  assert.deepEqual(panel.errors, [])
  assert.deepEqual(panel.warnings, [])
  assert.ok(texts(panel).includes('목록이 길어 20건은 여기에 나오지 않습니다.'))
  assert.equal(rowTitles(panel)[0], '합성 항목 5')
  await tabTo(panel, 'waiting')
  assert.deepEqual(panel.errors, [])
  assert.ok(panel.find({ type: 'Heading', text: '여유 있을 때 48' }))
})

test('실패 상자 안 글자는 흐리게 쓰지 않는다', async () => {
  const { panel } = await open(scene({ ready: [failedCard(1)] }, [startable(1)]))
  assert.equal(panel.find({ text: '이 일은 목록으로 돌아왔습니다.' })?.props.muted ?? false, false)
  assert.equal(panel.find({ type: 'Text', text: SCOPE })?.props.muted ?? false, false)
})

test('시각은 말로 쓰는 형식을 쓴다', async () => {
  const { panel } = await open(scene({}, [item(1)]))
  assert.equal(panel.find({ type: 'RelativeTime' })?.props.format, 'long')
})

test('틱이 오면 보이는 패널만 다시 읽는다', async () => {
  const { rig, panel } = await open(scene())
  assert.equal(rig.snapshots.length, 1)
  await panel.post('kairos.tick', { at: 1 })
  assert.equal(rig.snapshots.length, 2)
  await panel.setVisible(false)
  await panel.post('kairos.tick', { at: 2 })
  assert.equal(rig.snapshots.length, 2)
})

test('새로고침과 다시 시도는 읽는 동안 표시하고 잠그며 틱에는 표시하지 않는다', { timeout: 5_000 }, async () => {
  const replies: ((value: Snapshot) => void)[] = []
  const asked = async (count: number): Promise<void> => {
    while (replies.length < count) await new Promise(resolve => setImmediate(resolve))
  }
  const failed: Snapshot = { next: null, list: null, error: { code: 'spawn_failed', message: '합성' } }
  const host = mockHost({ capabilities: CAPS, requests: { 'kairos.snapshot': () => new Promise<Snapshot>(resolve => replies.push(resolve)) } })
  const mounting = mount(panelModule, { host })
  await asked(1)
  replies[0]?.(failed)
  view = await mounting
  const posted = view.post('kairos.tick', { at: 1 })
  await asked(2)
  assert.equal(view.find({ type: 'Spinner' }), null)
  replies[1]?.(failed)
  await posted
  const pressed = view.press(view.find({ label: '새로고침' }) as never)
  await asked(3)
  await new Promise(resolve => setImmediate(resolve))
  assert.ok(view.find({ type: 'Spinner' }))
  assert.equal(view.find({ label: '새로고침' })?.props.disabled, true)
  assert.equal(button(view, '다시 시도')?.props.disabled, true)
  replies[2]?.(scene())
  await pressed
  assert.equal(view.find({ type: 'Spinner' }), null)
  assert.equal(view.find({ label: '새로고침' })?.props.disabled ?? false, false)
})

test('새로고침을 누르면 다시 읽는다', async () => {
  const { rig, panel } = await open(scene())
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.equal(rig.snapshots.length, 2)
})

test('패널에서 워커를 지나 kairos까지 인자가 고정 낱말과 ID뿐이다', async () => {
  const danger = startable(7)
  danger.title = DANGEROUS_TITLE
  const calls: string[][] = []
  const exec: Exec = async (_file, args) => {
    const argv = [...args]
    calls.push([...argv])
    const value = argv[0] === 'list' ? listJson([danger]) : argv[0] === 'next' ? nextJson({ card: card([danger]) }) : { schema: 1, ok: true, results: [{ id: 7, status: 'changed', state: 'parked', message: '대기로 옮김' }] }
    return { exitCode: 0, stdout: JSON.stringify(value), fault: null }
  }
  const host = mockHost({ capabilities: CAPS })
  const worker = await mountWorker(
    {
      default: (orca: Parameters<typeof startWorker>[0]) => startWorker(orca, { exec, pollMs: 0 }).ready,
    },
    { host, commands: ['kairos.open'] },
  )
  view = await mount(panelModule, { host })
  await view.press(button(view, '나중에') as never)
  assert.deepEqual(calls.filter(argv => argv[0] === 'later'), [['later', '7', '--json']])
  assert.ok(calls.some(argv => argv.includes('--seen')))
  for (const argv of calls) for (const arg of argv) assert.match(arg, /^(?:[a-z]+|--[a-z-]+|\d+)$/)
  assert.deepEqual(worker.refusals, [])
  await worker.deactivate()
})

const focusOn = { active: true, startedAt: null, until: '2026-11-04T05:00:00.000Z', heldP1: 2, heldLine: null }

test('머리말 개요는 남은 일의 모양을 쓰고 0인 값은 빼며 게이트가 닫히면 그리지 않는다', async () => {
  const rows = [item(1, { tier: 'P0' }), startable(2), item(3)]
  const { panel } = await open(scene({ counts: counts({ P0: 1, P1: 2 }, { carriedP1: 1 }) }, rows))
  assert.ok(panel.find({ type: 'Text', text: '3건 · 긴급 1 · 밀린 일 1 · 제안 1' }))
  view?.unmount()
  const quiet = await open(scene({ counts: counts({ P1: 1 }) }, [item(1)]))
  assert.ok(quiet.panel.find({ type: 'Text', text: '1건' }))
  assert.ok(!texts(quiet.panel).includes('밀린 일'))
  view?.unmount()
  const closed = await open(scene({ display: recordingGate, counts: counts({ P1: 3 }) }))
  assert.ok(!texts(closed.panel).includes('3건'))
})

test('잠긴 제안은 개요의 제안으로 세지 않는다', async () => {
  const { panel } = await open(scene({ counts: counts({ P1: 1 }) }, [offered(1)]))
  assert.ok(!texts(panel).includes('제안 1 '))
  assert.ok(panel.find({ type: 'Text', text: '1건' }))
})

test('손댈 줄은 제목을 줄바꿈하고 사유를 쓰고 기다려도 되는 줄은 한 줄이다', async () => {
  const { panel } = await open(scene({}, [item(1), item(2, { tier: 'P2' })]))
  const [today] = rowsOf(panel)
  assert.equal(panel.find({ type: 'Text', text: '합성 이슈 1' })?.props.truncate, undefined)
  assert.ok(today?.text.includes('담당 이슈'))
  await tabTo(panel, 'waiting')
  const [relaxed] = rowsOf(panel)
  assert.equal(panel.find({ type: 'Text', text: '합성 이슈 2' })?.props.truncate, true)
  assert.ok(!relaxed?.text.includes('담당 이슈'))
  await panel.press(relaxed as never)
  assert.ok(rowsOf(panel)[0]?.text.includes('담당 이슈'))
})

test('줄 아이콘은 사유를 말하고 세션은 터미널이며 모르는 사유는 일반 아이콘이다', async () => {
  const { panel } = await open(scene({}, [item(1, { reason: 'deploy_risk', tier: 'P0' }), accepted(2), item(3, { reason: 'constructor' })]))
  assert.deepEqual(rowsOf(panel).map(row => row.props.icon), ['rocket', 'bell'])
  await tabTo(panel, 'sessions')
  assert.deepEqual(rowsOf(panel).map(row => row.props.icon), ['terminal'])
})

test('카드 안의 줄은 아이콘 없이 한 줄이다', async () => {
  const { panel } = await open(scene({ card: merged([item(7), item(8)]) }))
  assert.deepEqual(rowsOf(panel).map(row => row.props.icon), [undefined, undefined])
  assert.deepEqual(rowsOf(panel).map(row => row.label), ['합성 이슈 8', '합성 이슈 7'])
})

test('시작과 원문 열기는 같은 줄이고 범위는 바로 위에 있다', async () => {
  const { panel } = await open(scene({ card: card([startable(7)]) }))
  const flat = texts(panel)
  assert.ok(flat.indexOf(SCOPE) < flat.indexOf(START))
  assert.ok(flat.indexOf(START) < flat.indexOf('원문 열기'))
  assert.equal(button(panel, START)?.path.split('/').slice(0, -1).join('/'), button(panel, '원문 열기')?.path.split('/').slice(0, -1).join('/'))
})

test('나중에 곁에 다음 묶음 시각을 쓰고 시각을 모르면 지어내지 않는다', async () => {
  const { panel } = await open(scene({ card: card([item(7)]) }))
  assert.equal(button(panel, '나중에')?.props.tooltip, '내일 11:00에 다시 보여 드립니다')
  view?.unmount()
  const unknown = await open(scene({ card: card([item(7)]), budget: {} }))
  assert.equal(button(unknown.panel, '나중에')?.props.tooltip, undefined)
  await unknown.panel.press(button(unknown.panel, '나중에') as never)
  assert.ok(unknown.panel.find({ text: '미뤘습니다' }))
})

test('포커스 버튼은 시간을 고르게 하고 고른 값만 보낸 뒤 닫힌다', async () => {
  const { rig, panel } = await open(scene({ counts: counts({ P1: 1 }) }, [item(1)]))
  assert.equal(button(panel, '50분'), null)
  await panel.press(button(panel, '포커스') as never)
  assert.deepEqual(['25분', '50분', '90분'].map(word => button(panel, word)?.text), ['25분', '50분', '90분'])
  await panel.press(button(panel, '50분') as never)
  assert.deepEqual(rig.actions, [{ action: 'focus_50', ids: [], seen: true }])
  assert.equal(button(panel, '50분'), null)
  assert.ok(panel.find({ text: '포커스를 시작했습니다' }))
})

test('포커스 시작을 다시 누르면 고르기가 닫힌다', async () => {
  const { rig, panel } = await open(scene())
  await panel.press(button(panel, '포커스') as never)
  await panel.press(button(panel, '포커스') as never)
  assert.equal(button(panel, '25분'), null)
  assert.deepEqual(rig.actions, [])
})

test('포커스 중에는 띠가 시각과 쌓인 일을 말하고 끝내기만 있다', async () => {
  const { rig, panel } = await open(scene({ focus: focusOn, hold: { reason: 'focus', message: '포커스', until: focusOn.until } }, [item(1)]))
  assert.ok(panel.find({ type: 'Text', text: '포커스 중 · 14:00까지' }))
  assert.ok(panel.find({ type: 'Text', text: '쌓인 일 2건' }))
  assert.equal(button(panel, '포커스'), null)
  assert.ok(!texts(panel).includes('조용히 모아 둡니다'))
  await panel.press(button(panel, '끝내기') as never)
  assert.deepEqual(rig.actions, [{ action: 'focus_end', ids: [], seen: true }])
  assert.ok(panel.find({ text: '포커스를 끝냈습니다' }))
})

test('긴급 카드가 포커스를 넘어 와도 띠로 끌 수 있다', async () => {
  const { panel } = await open(scene({ focus: focusOn, card: card([item(1, { tier: 'P0' })], { tier: 'P0' }), counts: counts({ P0: 1 }) }))
  assert.ok(button(panel, '끝내기'))
  assert.ok(panel.find({ type: 'Text', text: '긴급 · 먼저 볼 일' }))
})

test('포커스 실패는 위로 띄우고 게이트가 닫히면 포커스 입구가 없다', async () => {
  const { panel } = await open(scene(), () => ({ ok: false, code: 'busy', message: '거절', run: null, snapshot: null }))
  await panel.press(button(panel, '포커스') as never)
  await panel.press(button(panel, '25분') as never)
  assert.equal(panel.find({ type: 'Callout' })?.props.title, '포커스를 시작하지 못했습니다')
  assert.ok(button(panel, '25분'))
  view?.unmount()
  const closed = await open(scene({ display: recordingGate }))
  assert.equal(button(closed.panel, '포커스'), null)
})

test('할 일을 쓰고 Enter를 누르면 제목을 보내고 입력은 열린 채 비운다', async () => {
  const { rig, panel } = await open(scene())
  assert.equal(panel.find({ type: 'Input' }), null)
  await panel.press(panel.find({ label: '할 일 추가' }) as never)
  await panel.type(panel.find({ type: 'Input' }) as never, '합성 할 일 적기', { submit: true })
  assert.deepEqual(rig.actions, [{ action: 'add', ids: [], seen: true, title: '합성 할 일 적기' }])
  assert.equal(panel.find({ type: 'Input' })?.props.value, '')
  assert.ok(panel.find({ text: '대기 탭에 추가했습니다' }))
})

test('빈 할 일은 보내지 않고 추가 버튼이 잠긴다', async () => {
  const { rig, panel } = await open(scene())
  await panel.press(panel.find({ label: '할 일 추가' }) as never)
  assert.equal(button(panel, '추가')?.disabled, true)
  await panel.type(panel.find({ type: 'Input' }) as never, '   ', { submit: true })
  assert.deepEqual(rig.actions, [])
  await panel.type(panel.find({ type: 'Input' }) as never, '합성', { replace: true })
  assert.equal(button(panel, '추가')?.disabled, false)
})

test('할 일 추가가 실패하면 쓴 글을 지우지 않는다', async () => {
  const { panel } = await open(scene(), () => ({ ok: false, code: 'busy', message: '거절', run: null, snapshot: null }))
  await panel.press(panel.find({ label: '할 일 추가' }) as never)
  await panel.type(panel.find({ type: 'Input' }) as never, '합성 남을 글', { submit: true })
  assert.equal(panel.find({ type: 'Input' })?.props.value, '합성 남을 글')
  assert.equal(panel.find({ type: 'Callout' })?.props.title, '추가하지 못했습니다')
})

test('제안을 꺼 두었으면 패널에서 다시 켠다. 일시정지는 명령을 알려 준다', async () => {
  const gate = (reason: string) => ({ ...recordingGate, reason, startsAt: null })
  const { rig, panel } = await open(scene({ display: gate('display_off') }))
  await panel.press(button(panel, '다시 켜기') as never)
  assert.deepEqual(rig.actions, [{ action: 'display_auto', ids: [], seen: true }])
  view?.unmount()
  const paused = await open(scene({ display: gate('paused') }))
  assert.equal(button(paused.panel, '다시 켜기'), null)
  assert.ok(paused.panel.find({ type: 'Code', text: 'kairos pause --resume' }))
})

test('세션 잠금은 패널에서 풀지 않는다', async () => {
  const { panel } = await open(scene({ card: card([offered(1)]) }))
  assert.ok(panel.find({ type: 'Code', text: 'kairos sessions on' }))
  assert.equal(button(panel, START), null)
})

test('바닥글은 지연이 없다고 말하고 경고가 있으면 시각만 남기며 고장이면 그리지 않는다', async () => {
  const { panel } = await open(scene({}, [item(1)]))
  assert.match(texts(panel), /수집 지연 없음/)
  view?.unmount()
  const late = await open(scene({ health: 'Slack 30분 지연' }, [item(1)]))
  assert.ok(!texts(late.panel).includes('수집 지연 없음'))
  assert.match(texts(late.panel), /마지막 확인/)
  view?.unmount()
  const broken = await open({ next: null, list: null, error: { code: 'bad_output', message: '합성' } })
  assert.ok(!texts(broken.panel).includes('수집 지연 없음'))
  view?.unmount()
  const closed = await open(scene({ display: recordingGate }))
  assert.ok(!texts(closed.panel).includes('수집 지연 없음'))
})

test('추가한 일은 대기 탭의 머리글이 알려 주고 다른 탭에서는 머리말이 어디로 갔는지 말한다', async () => {
  const many = Array.from({ length: 6 }, (_, index) => item(index + 1, { tier: 'P2' }))
  const before = scene({}, [item(20), ...many])
  const after = scene({}, [item(20), ...many, manual(30)])
  const add = async (panel: TestPanel): Promise<void> => {
    await panel.press(panel.find({ label: '할 일 추가' }) as never)
    await panel.type(panel.find({ type: 'Input' }) as never, '합성 할 일', { submit: true })
  }
  const outcome = () => ({ ok: true, code: null, message: '추가했습니다', run: null, snapshot: after })
  const waiting = await open(before, outcome)
  await tabTo(waiting.panel, 'waiting')
  await add(waiting.panel)
  assert.match(String(waiting.panel.find({ type: 'ListItem', text: /여유 있을 때/ })?.text), /추가했습니다/)
  assert.equal(waiting.panel.find({ text: '대기 탭에 추가했습니다' }), null)
  view?.unmount()
  const now = await open(before, outcome)
  await add(now.panel)
  assert.ok(now.panel.find({ text: '대기 탭에 추가했습니다' }))
  assert.deepEqual(tabLabels(now.panel), ['지금 1', '대기 7', '세션'])
})

const REVEAL_AT = 1_762_740_000_000
const REVEAL_HEAD = '알림에서 연 일'
const revealed = (items: number[], at: number = REVEAL_AT) => ({ reveal: { items, at } })
const hasStrip = (panel: TestPanel): boolean => panel.find({ type: 'Text', text: REVEAL_HEAD }) !== null
const countOf = (panel: TestPanel, title: string): number => rowTitles(panel).filter(shown => shown === title).length
const cleared = (panel: TestPanel) => panel.find({ type: 'Callout' })?.props.title === '이미 정리된 일입니다'

test('알림에서 연 한 건은 맨 위 띠로 올라가 펼쳐지고 제 묶음에는 없다', async () => {
  const { panel } = await open(scene(revealed([2]), [item(1), item(2), item(3)]))
  assert.ok(hasStrip(panel))
  assert.equal(rowTitles(panel)[0], '합성 이슈 2')
  assert.equal(countOf(panel, '합성 이슈 2'), 1)
  assert.equal(panel.findAll({ role: 'button', text: '완료' }).length, 1)
  assert.ok(panel.find({ type: 'Text', text: '오늘 2' }))
  assert.ok(panel.find({ type: 'RelativeTime' }))
  assert.equal(panel.persisted.plugin.revealAt, REVEAL_AT)
  assert.deepEqual(panel.errors, [])
  assert.deepEqual(panel.warnings, [])
})

test('띠는 세션 소식 아래, 먼저 볼 일 카드 위에 있다', async () => {
  const { panel } = await open(scene({ ...revealed([2]), card: card([item(9)]), ready: [readyCard(8)] }, [item(2), item(8), item(9)]))
  const drawn = texts(panel)
  assert.ok(drawn.indexOf('결과가 준비됐습니다') < drawn.indexOf(REVEAL_HEAD))
  assert.ok(drawn.indexOf(REVEAL_HEAD) < drawn.indexOf('먼저 볼 일'))
})

test('여러 건이면 펼치지 않고 띠에 모아 올린다', async () => {
  const { panel } = await open(scene(revealed([1, 3]), [item(1), item(2), item(3), item(4)]))
  assert.deepEqual(rowTitles(panel).slice(0, 2), ['합성 이슈 3', '합성 이슈 1'])
  for (const title of ['합성 이슈 1', '합성 이슈 3']) assert.equal(countOf(panel, title), 1)
  assert.equal(button(panel, '완료'), null)
  assert.ok(panel.find({ type: 'Text', text: '오늘 2' }))
})

test('일부만 남았으면 남은 일만 올리고 한 건이면 펼친다', async () => {
  const { panel } = await open(scene(revealed([2, 99]), [item(1), item(2)]))
  assert.equal(rowTitles(panel)[0], '합성 이슈 2')
  assert.ok(button(panel, '완료'))
  assert.ok(!cleared(panel))
})

test('이미 적용한 알림은 패널을 다시 열어도 다시 걸지 않고 새 알림만 건다', async () => {
  const { host } = serve(scene(revealed([2]), [item(1), item(2)]))
  view = await mount(panelModule, { host, persisted: { plugin: { revealAt: REVEAL_AT } } })
  assert.ok(!hasStrip(view))
  assert.equal(button(view, '완료'), null)
  view.unmount()
  const again = serve(scene(revealed([2], REVEAL_AT + 1), [item(1), item(2)]))
  view = await mount(panelModule, { host: again.host, persisted: { plugin: { revealAt: REVEAL_AT } } })
  assert.ok(hasStrip(view))
  assert.equal(view.persisted.plugin.revealAt, REVEAL_AT + 1)
})

test('알림의 일이 모두 없으면 이미 정리됐다고 알리고 띠를 그리지 않는다', async () => {
  const { panel } = await open(scene(revealed([98, 99]), [item(1)]))
  assert.ok(!hasStrip(panel))
  assert.ok(cleared(panel))
  await panel.press(button(panel, '닫기') as never)
  assert.equal(panel.find({ type: 'Callout' }), null)
})

test('알림의 일이 지금 카드에 있으면 카드가 보여 주고 안내를 띄우지 않는다', async () => {
  const { panel } = await open(scene({ ...revealed([9]), card: card([item(9)]) }, [item(1), item(9)]))
  assert.ok(!hasStrip(panel))
  assert.ok(!cleared(panel))
  assert.ok(texts(panel).includes('먼저 볼 일'))
})

test('게이트가 닫혀 있으면 알림의 일을 따로 알리지 않는다', async () => {
  const { panel } = await open(scene({ ...revealed([2]), display: recordingGate }, [item(2)]))
  assert.ok(!hasStrip(panel))
  assert.ok(!cleared(panel))
  assert.equal(panel.persisted.plugin.revealAt, REVEAL_AT)
})

test('닫으면 줄이 제 묶음으로 돌아가 접히고 틱이 와도 다시 띄우지 않는다', async () => {
  const { state, panel } = await openLive(scene(revealed([2]), [item(1), item(2)]))
  await panel.press(panel.find({ label: `${REVEAL_HEAD} 닫기` }) as never)
  assert.ok(!hasStrip(panel))
  assert.equal(countOf(panel, '합성 이슈 2'), 1)
  assert.equal(button(panel, '완료'), null)
  await tick(panel)
  assert.ok(!hasStrip(panel))
  assert.equal(state.reads, 2)
})

test('띠에서 펼친 줄을 접으면 띠를 닫는다', async () => {
  const { panel } = await open(scene(revealed([2]), [item(1), item(2)]))
  await panel.press(titled(panel, '합성 이슈 2') as never)
  assert.ok(!hasStrip(panel))
  assert.equal(countOf(panel, '합성 이슈 2'), 1)
  assert.equal(button(panel, '완료'), null)
})

test('띠에서 처리한 일은 띠에서 빠지고 같은 알림을 다시 걸지 않는다', async () => {
  const { rig, panel } = await open(scene(revealed([2]), [item(1), item(2)]))
  await panel.press(button(panel, '나중에') as never)
  assert.deepEqual(rig.actions.map(input => [input.action, input.ids]), [['later', [2]]])
  assert.ok(!hasStrip(panel))
  assert.equal(countOf(panel, '합성 이슈 2'), 1)
})

test('띠의 일이 사라지면 띠를 줄이거나 닫고 새 알림은 앞의 것을 바꾼다', async () => {
  const { state, panel } = await openLive(scene(revealed([1, 2]), [item(1), item(2), item(3)]))
  assert.deepEqual(rowTitles(panel).slice(0, 2), ['합성 이슈 2', '합성 이슈 1'])
  state.snapshot = scene(revealed([1, 2]), [item(2), item(3)])
  await tick(panel)
  assert.ok(hasStrip(panel))
  assert.equal(rowTitles(panel)[0], '합성 이슈 2')
  state.snapshot = scene(revealed([1, 2]), [item(3)])
  await tick(panel)
  assert.ok(!hasStrip(panel))
  assert.ok(!cleared(panel))
  state.snapshot = scene(revealed([3], REVEAL_AT + 1), [item(3), item(4)])
  await tick(panel)
  assert.ok(hasStrip(panel))
  assert.equal(rowTitles(panel)[0], '합성 이슈 3')
  assert.ok(button(panel, '완료'))
})

test('워커가 알림을 알리면 seen으로 다시 읽어 띠를 띄운다', async () => {
  const inputs: SnapshotInput[] = []
  let current = scene({}, [item(1), item(2)])
  const host = mockHost({
    capabilities: CAPS,
    requests: {
      'kairos.snapshot': (input: SnapshotInput) => {
        inputs.push(input)
        return current
      },
      'kairos.action': () => ({ ok: true, code: null, message: '처리했습니다', run: null, snapshot: current }),
    },
  })
  view = await mount(panelModule, { host })
  await tick(view)
  assert.deepEqual(inputs.map(input => input.seen), [true, false])
  current = scene(revealed([2]), [item(1), item(2)])
  await view.post('kairos.reveal', { at: 1 })
  assert.deepEqual(inputs.at(-1), { seen: true })
  assert.ok(hasStrip(view))
  assert.ok(button(view, '완료'))
})

test('펼친 행이 틱을 맡아 두는 동안 온 알림은 반영된 뒤에 걸린다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1), item(2)]))
  await expand(panel, '합성 이슈 1')
  state.snapshot = scene(revealed([2]), [item(1), item(2)])
  await tick(panel)
  assert.ok(!hasStrip(panel))
  await panel.post('kairos.reveal', { at: 1 })
  assert.ok(hasStrip(panel))
  assert.equal(rowTitles(panel)[0], '합성 이슈 2')
  assert.equal(panel.findAll({ role: 'button', text: '완료' }).length, 1)
})

test('새로고침이 띄운 정리 안내는 그 새로고침이 지우지 않는다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1)]))
  state.snapshot = scene(revealed([99]), [item(1)])
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.ok(cleared(panel))
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.equal(panel.find({ type: 'Callout' }), null)
})

test('카드에서 연 알림의 일을 카드에서 처리해도 띠가 뒤늦게 뜨지 않는다', async () => {
  const after = scene(revealed([9]), [item(1), item(9)])
  const { panel } = await open(scene({ ...revealed([9]), card: card([item(9)]) }, [item(1), item(9)]), () => ({ ok: true, code: null, message: '대기로 옮김', run: null, snapshot: after }))
  assert.ok(!hasStrip(panel))
  await panel.press(button(panel, '나중에') as never)
  assert.ok(!hasStrip(panel))
  assert.equal(countOf(panel, '합성 이슈 9'), 1)
  assert.equal(button(panel, '완료'), null)
})

test('목록을 읽지 못한 틱은 알림을 판정하지 않고 다음에 읽은 값이 건다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1)]))
  const fresh = scene(revealed([2]), [item(1), item(2)])
  state.snapshot = { next: fresh.next, list: null, error: { code: 'timeout', message: '합성' } }
  await tick(panel)
  assert.ok(!hasStrip(panel))
  assert.ok(!cleared(panel))
  assert.notEqual(panel.persisted.plugin?.revealAt, REVEAL_AT)
  state.snapshot = fresh
  await tick(panel)
  assert.ok(hasStrip(panel))
  assert.ok(button(panel, '완료'))
  assert.equal(panel.persisted.plugin.revealAt, REVEAL_AT)
})

test('알림의 seen 읽기가 도는 동안 온 틱이 그 응답을 버리지 않는다', { timeout: 5_000 }, async () => {
  let current = scene({}, [item(1), item(2)])
  let hold = false
  let release: (value: Snapshot) => void = () => undefined
  const host = mockHost({
    capabilities: CAPS,
    requests: {
      'kairos.snapshot': (input: SnapshotInput) => (hold && input.seen ? new Promise<Snapshot>(resolve => (release = resolve)) : current),
      'kairos.action': () => ({ ok: true, code: null, message: '처리했습니다', run: null, snapshot: current }),
    },
  })
  view = await mount(panelModule, { host })
  await expand(view, '합성 이슈 1')
  current = scene(revealed([2]), [item(1), item(2)])
  hold = true
  const revealing = view.post('kairos.reveal', { at: 1 })
  await new Promise(resolve => setImmediate(resolve))
  const ticking = tick(view)
  await new Promise(resolve => setImmediate(resolve))
  release(current)
  await Promise.all([revealing, ticking])
  assert.ok(hasStrip(view))
  assert.equal(rowTitles(view)[0], '합성 이슈 2')
  assert.equal(view.persisted.plugin.revealAt, REVEAL_AT)
})

test('목록이 길어 넘기지 않은 일이면 이미 정리됐다고 하지 않는다', async () => {
  const rows = manyItems(120)
  const kept = new Set(scene({}, rows).list?.items.map(row => row.id))
  const gone = rows.map(row => Number(row.id)).find(id => !kept.has(id))
  assert.ok(gone !== undefined)
  const { panel } = await open(scene(revealed([gone]), rows))
  assert.ok(!hasStrip(panel))
  assert.ok(!cleared(panel))
  assert.equal(panel.persisted.plugin.revealAt, REVEAL_AT)
})

const tabValue = (panel: TestPanel): unknown => panel.find({ type: 'Tabs' })?.props.value
const pairs = (panel: TestPanel, label: string): [string, unknown][] => {
  const items = (panel.findAll({ type: 'KeyValue' }).find(found => found.props.label === label)?.props.items ?? []) as { label: string; value: unknown }[]
  return items.map(each => [each.label, each.value])
}
const spread = [item(1, { tier: 'P0' }), startable(2), item(3), item(4, { tier: 'P2' }), parked(5), item(6, { baseline: true }), accepted(7)]

test('진행 상태 탭은 지금, 대기, 세션이고 탭마다 건수를 쓴다', async () => {
  const { rig, panel } = await open(scene({}, spread))
  const reads = rig.snapshots.length
  assert.equal(panel.find({ type: 'Tabs' })?.props.label, '진행 상태')
  assert.equal(tabValue(panel), 'now')
  assert.deepEqual(tabLabels(panel), ['지금 3', '대기 3', '세션 1'])
  assert.deepEqual(rowTitles(panel), ['합성 이슈 1', '합성 이슈 2', '합성 이슈 3'])
  await tabTo(panel, 'waiting')
  assert.equal(tabValue(panel), 'waiting')
  assert.deepEqual(rowTitles(panel), ['합성 이슈 4', '합성 이슈 5'])
  await tabTo(panel, 'sessions')
  assert.deepEqual(rowTitles(panel), ['합성 이슈 7'])
  // 수집 상태는 탭이 아니라 머리말 버튼이다. 누르면 목록 대신 상태를 그리고 다시 누르면 보던 탭으로 돌아온다.
  await toggleStatus(panel)
  assert.deepEqual(rowTitles(panel), [])
  assert.equal(panel.find({ type: 'Tabs' }), null)
  assert.equal(statusButton(panel)?.props.label, '수집 상태 닫기')
  await toggleStatus(panel)
  assert.deepEqual(rowTitles(panel), ['합성 이슈 7'])
  // 탭을 바꾸는 것은 보기만 바꾼다. kairos를 부르지 않는다.
  assert.equal(rig.snapshots.length, reads)
  assert.deepEqual(rig.actions, [])
  assert.deepEqual(panel.errors, [])
})

test('먼저 볼 일 카드는 지금 탭에만 있고 건수에 든다', async () => {
  const { panel } = await open(scene({ card: card([startable(7)]) }, [item(1), parked(2)]))
  assert.deepEqual(tabLabels(panel), ['지금 2', '대기 1', '세션'])
  assert.ok(button(panel, START))
  await tabTo(panel, 'waiting')
  assert.equal(button(panel, START), null)
  await tabTo(panel, 'now')
  assert.ok(button(panel, START))
})

test('늦는 소스가 있으면 수집 상태 버튼이 경고색과 이름으로 알린다', async () => {
  const { panel } = await open(scene({ health: 'Slack 37분 지연', sources: sourceRows() }, [item(1)]))
  assert.equal(statusButton(panel)?.props.tone, 'warning')
  assert.equal(statusButton(panel)?.props.label, '수집 상태 · 지연된 소스가 있습니다')
  view?.unmount()
  // 세션 상태를 못 읽은 경고처럼 상태 화면에 볼 것이 없는 경고에는 붙이지 않는다. 미연결과 수집 기록 없음도 지연이 아니다.
  const quiet = sourceRows().map(row => (row.status === 'delayed' ? { ...row, status: 'ok', delayMinutes: null } : row))
  const other = await open(scene({ health: '세션 상태를 읽지 못했습니다', sources: quiet }, [item(1)]))
  assert.equal(statusButton(other.panel)?.props.tone, undefined)
  assert.equal(statusButton(other.panel)?.props.label, '수집 상태')
  view?.unmount()
  const old = await open(scene({ health: '합성 경고' }, [item(1)]))
  assert.equal(statusButton(old.panel)?.props.tone, undefined)
})

test('탭을 바꾸면 펼친 행을 접어 보이지 않는 줄이 틱을 붙잡지 않는다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1), parked(2)]))
  await expand(panel, '합성 이슈 1')
  assert.ok(button(panel, '완료'))
  state.snapshot = scene({}, [item(1, { title: '새 제목' }), parked(2)])
  await tabTo(panel, 'waiting')
  await tick(panel)
  await tabTo(panel, 'now')
  assert.equal(button(panel, '완료'), null)
  assert.deepEqual(rowTitles(panel), ['새 제목'])
})

test('지금 탭이 비면 다른 탭에 남은 일로 가는 길을 준다', async () => {
  const { panel } = await open(scene({}, [parked(1), item(2, { tier: 'P2' }), accepted(3)]))
  assert.deepEqual(rowTitles(panel), [])
  await panel.press(button(panel, '대기 2건 보기') as never)
  assert.equal(tabValue(panel), 'waiting')
  assert.deepEqual(rowTitles(panel), ['합성 이슈 2', '합성 이슈 1'])
  await tabTo(panel, 'now')
  await panel.press(button(panel, '세션 1건 보기') as never)
  assert.equal(tabValue(panel), 'sessions')
  view?.unmount()
  const busy = await open(scene({}, [item(1), parked(2)]))
  assert.equal(busy.panel.find({ role: 'button', text: /건 보기$/ }), null)
  view?.unmount()
  const empty = await open(scene())
  assert.equal(empty.panel.find({ role: 'button', text: /건 보기$/ }), null)
})

test('빈 대기 탭과 세션 탭은 무엇이 모이는 곳인지 말한다', async () => {
  const { panel } = await open(scene({}, [item(1)]))
  await tabTo(panel, 'waiting')
  assert.equal(panel.find({ type: 'EmptyState' })?.props.title, '기다리는 일이 없습니다')
  await tabTo(panel, 'sessions')
  assert.equal(panel.find({ type: 'EmptyState' })?.props.title, '맡긴 세션이 없습니다')
  view?.unmount()
  const filled = await open(scene({}, [parked(1), accepted(2)]))
  await tabTo(filled.panel, 'waiting')
  assert.equal(filled.panel.find({ type: 'EmptyState' }), null)
  await tabTo(filled.panel, 'sessions')
  assert.equal(filled.panel.find({ type: 'EmptyState' }), null)
})

test('수집 상태 화면은 소스별 수집 상태와 건수와 예산을 조작 없이 보여 준다', async () => {
  const budget = { p1HourUsed: 1, p1HourLimit: 2, p1DayUsed: 4, p1DayLimit: 6, bundlesToday: 2, shownToday: 5, dayTarget: 6, nextBundleAt: '2026-11-04T05:00:00.000Z' }
  const next = { sources: sourceRows(), budget, counts: counts({ P0: 1, P1: 2, P2: 3 }, { parked: 4, accepted: 1, carriedP1: 2 }) }
  const { rig, panel } = await open(scene(next, [item(1)]))
  await toggleStatus(panel)
  for (const word of ['수집 상태', 'Calendar', '미연결', 'GitHub', '정상', 'Slack', '37분 지연', 'Gmail', '수집 시간 아님', 'Notion', '수집 기록 없음']) assert.ok(texts(panel).includes(word), word)
  assert.equal(panel.find({ type: 'Text', text: '37분 지연' })?.props.tone, 'warning')
  assert.equal(panel.find({ type: 'Text', text: '정상' })?.props.tone, undefined)
  assert.equal(panel.find({ type: 'Text', text: '수집 시간 아님' })?.props.tone, undefined)
  assert.deepEqual(pairs(panel, '열린 일'), [['긴급', 1], ['오늘', 2], ['여유 있을 때', 3], ['밀린 일', 2], ['미룬 일', 4], ['세션', 1]])
  assert.deepEqual(pairs(panel, '먼저 볼 일 카드'), [['이번 시간', '1 / 2'], ['오늘', '4 / 6'], ['오늘 낸 묶음', 2], ['다음 묶음', '14:00']])
  assert.ok(panel.find({ type: 'Code', text: 'kairos doctor' }))
  // 상태 화면에는 kairos를 바꾸는 버튼이 없다.
  assert.deepEqual(panel.findAll({ role: 'button' }).map(found => found.label ?? found.text).filter(label => !['포커스', '검색', '할 일 추가', '새로고침', '수집 상태 닫기'].includes(String(label))), [])
  assert.deepEqual(rig.actions, [])
  assert.deepEqual(panel.errors, [])
})

test('소스별 상태를 못 받은 옛 kairos에서도 수집 상태 화면은 건수를 보여 준다', async () => {
  const { panel } = await open(scene({ counts: counts({ P1: 2 }) }, [item(1)]))
  await toggleStatus(panel)
  assert.ok(texts(panel).includes('소스별 상태를 받지 못했습니다'))
  assert.deepEqual(pairs(panel, '열린 일')[1], ['오늘', 2])
  assert.deepEqual(panel.errors, [])
})

test('게이트가 닫히면 탭과 검색이 없고 수집 상태만 덧붙인다', async () => {
  const { panel } = await open(scene({ display: recordingGate, sources: sourceRows() }, [item(1)], { hidden: 1 }))
  assert.equal(panel.find({ type: 'Tabs' }), null)
  assert.equal(panel.find({ label: '검색' }), null)
  assert.ok(panel.find({ type: 'Text', text: '수집 상태' }))
  assert.ok(texts(panel).includes('37분 지연'))
  assert.deepEqual(rowTitles(panel), [])
  view?.unmount()
  const bare = await open(scene({ display: recordingGate }))
  assert.equal(bare.panel.find({ type: 'Text', text: '수집 상태' }), null)
})

test('펼친 행은 왜 올라왔는지와 권장 행동을 먼저 말한다', async () => {
  const { panel } = await open(scene({}, [item(1), item(2, { reason: 'constructor' }), parked(3)]))
  assert.ok(!texts(panel).includes('담당자로 지정된 이슈입니다.'))
  await expand(panel, '합성 이슈 1')
  assert.ok(panel.find({ type: 'Text', text: '이유 · ' }))
  assert.ok(panel.find({ type: 'Text', text: '권장 · ' }))
  assert.ok(texts(panel).includes('담당자로 지정된 이슈입니다.'))
  assert.ok(texts(panel).includes('내용을 확인하고 어떻게 진행할지 정하세요.'))
  assert.ok(!texts(panel).includes('에이전트에게 맡길 수도 있습니다.'))
  await expand(panel, '합성 이슈 2')
  assert.equal(panel.find({ type: 'Text', text: '이유 · ' }), null)
  await tabTo(panel, 'waiting')
  await expand(panel, '합성 이슈 3')
  assert.ok(texts(panel).includes('10:00에 다시 보여 드립니다.'))
})

test('제안이 붙은 일은 이유, 권장, 맡길 범위, 시작 순서로 읽힌다', async () => {
  const { panel } = await open(scene({}, [startable(1)]))
  await expand(panel, '합성 이슈 1')
  const all = texts(panel)
  const order = ['담당자로 지정된 이슈입니다.', '에이전트에게 맡길 수도 있습니다.', '에이전트가 이슈를 고칩니다', SCOPE, '시작을 누르면 작업 공간을 만들고 세션 창을 엽니다.', START].map(word => all.indexOf(word))
  assert.ok(order.every(at => at >= 0), JSON.stringify(order))
  assert.deepEqual([...order].sort((a, b) => a - b), order)
  view?.unmount()
  const locked = await open(scene({}, [offered(1)]))
  await expand(locked.panel, '합성 이슈 1')
  assert.ok(!texts(locked.panel).includes('시작을 누르면 작업 공간을 만들고'))
  assert.ok(!texts(locked.panel).includes('에이전트에게 맡길 수도 있습니다.'))
})

test('세션이 있는 일은 세션 상태가 권장을 정한다', async () => {
  const { panel } = await open(scene({}, [accepted(1, 'waiting_user'), revivable(2)]))
  await tabTo(panel, 'sessions')
  await expand(panel, '합성 이슈 1')
  assert.ok(texts(panel).includes('세션이 답을 기다리고 있습니다.'))
  assert.ok(texts(panel).includes('세션으로 이동해 답해 주세요.'))
  await expand(panel, '합성 이슈 2')
  assert.ok(texts(panel).includes('세션이 끝났습니다.'))
})

test('단건 카드는 제안과 밀린 일을 배지로 알린다', async () => {
  const badges = (panel: TestPanel) => panel.findAll({ type: 'Badge' }).map(found => [found.text, found.props.tone])
  const { panel } = await open(scene({ card: card([startable(7, { carried: true })]) }))
  assert.deepEqual(badges(panel), [['에이전트 제안', 'accent'], ['밀린 일', 'warning']])
  assert.ok(texts(panel).includes('담당자로 지정된 이슈입니다.'))
  view?.unmount()
  const plain = await open(scene({ card: card([item(7)]) }))
  assert.deepEqual(badges(plain.panel), [])
})

test('나중에 곁에서 다시 볼 때를 고르면 고정 이름과 항목 ID만 보낸다', async () => {
  const { rig, panel } = await open(scene({}, [item(1), item(2)]))
  await expand(panel, '합성 이슈 1')
  assert.equal(button(panel, '1시간 뒤'), null)
  await panel.press(panel.find({ label: '다시 볼 때 고르기' }) as never)
  assert.ok(panel.find({ type: 'Text', text: '언제 다시 볼까요' }))
  assert.ok(button(panel, '내일 아침'))
  await panel.press(button(panel, '1시간 뒤') as never)
  assert.deepEqual(rig.actions, [{ action: 'later_hour', ids: [1], seen: true }])
  assert.ok(texts(panel).includes('미뤘습니다 · 1시간 뒤에 다시'))
  assert.equal(button(panel, '1시간 뒤'), null)
})

test('다시 볼 때 고르기는 다시 누르거나 다른 행을 펼치면 닫힌다', async () => {
  const { rig, panel } = await open(scene({}, [item(1), item(2)]))
  await expand(panel, '합성 이슈 1')
  await panel.press(panel.find({ label: '다시 볼 때 고르기' }) as never)
  await panel.press(panel.find({ label: '다시 볼 때 고르기' }) as never)
  assert.equal(button(panel, '1시간 뒤'), null)
  await panel.press(panel.find({ label: '다시 볼 때 고르기' }) as never)
  await expand(panel, '합성 이슈 2')
  assert.equal(button(panel, '1시간 뒤'), null)
  await expand(panel, '합성 이슈 1')
  assert.equal(button(panel, '1시간 뒤'), null)
  assert.deepEqual(rig.actions, [])
})

test('단건 카드에서도 내일 아침으로 미룰 수 있고 나중에가 없는 일에는 고르기가 없다', async () => {
  const { rig, panel } = await open(scene({ card: card([item(7)]) }, [accepted(8)]))
  await panel.press(panel.find({ label: '다시 볼 때 고르기' }) as never)
  await panel.press(button(panel, '내일 아침') as never)
  assert.deepEqual(rig.actions, [{ action: 'later_tomorrow', ids: [7], seen: true }])
  await tabTo(panel, 'sessions')
  await expand(panel, '합성 이슈 8')
  assert.equal(panel.find({ label: '다시 볼 때 고르기' }), null)
})

test('옛 kairos가 다시 볼 때를 모르면 업데이트하라고 알린다', async () => {
  const { panel } = await open(scene({}, [item(1)]), () => ({ ok: false, code: 'usage', message: '사용법: kairos later', run: null, snapshot: null }))
  await expand(panel, '합성 이슈 1')
  await panel.press(panel.find({ label: '다시 볼 때 고르기' }) as never)
  await panel.press(button(panel, '1시간 뒤') as never)
  assert.equal(panel.find({ text: '미루지 못했습니다. 설치된 kairos가 이 조작을 모릅니다. kairos를 업데이트해 주세요.' })?.props.tone, 'danger')
  assert.ok(!texts(panel).includes('사용법: kairos later'))
})

const SEARCH = '일 찾기'
const searchFor = async (panel: TestPanel, value: string): Promise<void> => panel.type(panel.find({ type: 'Input', label: SEARCH }) as never, value, { replace: true })

test('검색은 탭을 가로질러 찾고 kairos를 부르지 않는다', async () => {
  const rows = [item(1, { title: '합성 배포 점검' }), item(2, { tier: 'P2', title: '합성 문서 정리', source: 'slack', sourceLabel: 'Slack' }), parked(3, { title: '합성 배포 회고' }), accepted(4)]
  const { rig, panel } = await open(scene({}, rows))
  const reads = rig.snapshots.length
  assert.equal(panel.find({ type: 'Input', label: SEARCH }), null)
  await panel.press(panel.find({ label: '검색' }) as never)
  assert.equal(panel.find({ type: 'Input', label: SEARCH })?.props.type, 'search')
  assert.ok(panel.find({ type: 'Tabs' }))
  await searchFor(panel, '배포')
  assert.equal(panel.find({ type: 'Tabs' }), null)
  assert.deepEqual(rowTitles(panel), ['합성 배포 점검', '합성 배포 회고'])
  assert.ok(panel.find({ type: 'Heading', text: '검색 결과 2' }) ?? panel.find({ text: '검색 결과 2' }))
  await searchFor(panel, 'SLACK 문서')
  assert.deepEqual(rowTitles(panel), ['합성 문서 정리'])
  await searchFor(panel, '세션')
  assert.deepEqual(rowTitles(panel), ['합성 이슈 4'])
  await searchFor(panel, '없는낱말')
  assert.deepEqual(rowTitles(panel), [])
  assert.equal(panel.find({ type: 'EmptyState' })?.props.title, '찾는 일이 없습니다')
  await panel.press(panel.find({ label: '검색 닫기' }) as never)
  assert.ok(panel.find({ type: 'Tabs' }))
  assert.equal(panel.find({ type: 'EmptyState' }), null)
  await panel.press(panel.find({ label: '검색' }) as never)
  assert.equal(panel.find({ type: 'Input', label: SEARCH })?.props.value, '')
  assert.equal(rig.snapshots.length, reads)
  assert.deepEqual(rig.actions, [])
  assert.deepEqual(panel.errors, [])
})

test('검색 결과의 줄도 펼쳐서 조작할 수 있고 검색어를 바꾸면 접힌다', async () => {
  const { rig, panel } = await open(scene({}, [item(1), parked(2, { title: '합성 대기 건' })]))
  await panel.press(panel.find({ label: '검색' }) as never)
  await searchFor(panel, '대기')
  await expand(panel, '합성 대기 건')
  assert.ok(button(panel, '완료'))
  await panel.press(button(panel, '완료') as never)
  assert.deepEqual(rig.actions, [{ action: 'done', ids: [2], seen: false, ready: true }])
  // 검색어가 바뀌어도 그 줄이 남아 있으면 펼친 채 둔다. 걸러져 사라지면 접는다.
  await expand(panel, '합성 대기 건')
  await panel.type(panel.find({ type: 'Input', label: SEARCH }) as never, ' 건')
  assert.equal(panel.find({ type: 'Input', label: SEARCH })?.props.value, '대기 건')
  assert.ok(button(panel, '완료'))
  await searchFor(panel, '이슈 1')
  assert.deepEqual(rowTitles(panel), ['합성 이슈 1'])
  assert.equal(button(panel, '완료'), null)
  await searchFor(panel, '합성')
  assert.equal(button(panel, '완료'), null)
})

test('검색어가 카드의 일과 맞으면 카드도 보여 주고 아니면 숨긴다', async () => {
  const { panel } = await open(scene({ card: card([startable(7, { title: '합성 카드 건' })]) }, [item(1)]))
  await panel.press(panel.find({ label: '검색' }) as never)
  await searchFor(panel, '카드')
  assert.ok(button(panel, START))
  assert.equal(panel.find({ type: 'EmptyState' }), null)
  await searchFor(panel, '이슈 1')
  assert.equal(button(panel, START), null)
  assert.deepEqual(rowTitles(panel), ['합성 이슈 1'])
})

test('검색 결과가 길면 앞쪽만 그리고 남은 건수를 알린다', async () => {
  const { panel } = await open(scene({}, Array.from({ length: 40 }, (_, index) => item(index + 1))))
  await panel.press(panel.find({ label: '검색' }) as never)
  await searchFor(panel, '합성')
  assert.equal(rowTitles(panel).length, 30)
  assert.ok(texts(panel).includes('10건이 더 있습니다. 낱말을 더해 좁혀 보세요.'))
  assert.deepEqual(panel.errors, [])
})

test('검색어는 글자로만 다루고 정규식이나 마크업으로 읽지 않는다', async () => {
  const { panel } = await open(scene({}, [item(1, { title: DANGEROUS_TITLE }), item(2)]))
  await panel.press(panel.find({ label: '검색' }) as never)
  for (const value of ['.*', '[', '(?=', '<img', '$(touch']) {
    await searchFor(panel, value)
    assert.ok(rowTitles(panel).length <= 1, value)
    assert.deepEqual(panel.errors, [], value)
  }
  assert.equal(panel.find({ type: 'Markdown' }), null)
  assert.equal(panel.find({ type: 'Link' }), null)
})

test('카드를 그리지 않는 탭에서는 세션 소식만 확인으로 적고 지금 탭으로 돌아오면 노출을 적는다', async () => {
  const { rig, panel } = await open(scene({ card: card([startable(7)]) }, [item(1), parked(2)]))
  assert.deepEqual(rig.snapshots, [{ seen: true }])
  await tabTo(panel, 'waiting')
  assert.equal(rig.snapshots.length, 1)
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: false, ready: true })
  await expand(panel, '합성 이슈 2')
  await panel.press(button(panel, '완료') as never)
  assert.deepEqual(rig.actions, [{ action: 'done', ids: [2], seen: false, ready: true }])
  await toggleStatus(panel)
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: false, ready: true })
  // 틱은 어느 화면에서든 아무것도 적지 않는다.
  await tick(panel)
  assert.deepEqual(rig.snapshots.at(-1), { seen: false })
  // 상태 화면을 닫으면 보던 대기 탭으로 돌아온다. 카드는 아직 화면에 없다.
  await toggleStatus(panel)
  const before = rig.snapshots.length
  // 지금 탭으로 돌아오는 것은 패널을 여는 것과 같다. 카드가 화면에 들어오므로 한 번 읽어 노출을 적는다.
  await tabTo(panel, 'now')
  assert.deepEqual(rig.snapshots.slice(before), [{ seen: true }])
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: true })
  await panel.press(button(panel, '완료') as never)
  assert.deepEqual(rig.actions.at(-1), { action: 'done', ids: [7], seen: true })
})

test('검색어로 거르는 동안에는 카드가 보여도 노출을 청하지 않고 검색을 닫을 때 적는다', async () => {
  const { rig, panel } = await open(scene({ card: card([startable(7, { title: '합성 카드 건' })]) }, [item(1)]))
  await panel.press(panel.find({ label: '검색' }) as never)
  // 검색어가 없으면 거르지 않으므로 지금 탭 그대로다.
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: true })
  await searchFor(panel, '이슈 1')
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: false, ready: true })
  // 검색어가 카드와 맞아 카드가 보여도 같다. 답으로 온 카드가 검색어와 맞지 않으면 그리지 않은 카드가 노출로 남기 때문이다.
  await searchFor(panel, '카드')
  assert.match(texts(panel), /합성 카드 건/)
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: false, ready: true })
  await panel.press(button(panel, '완료') as never)
  assert.deepEqual(rig.actions.at(-1), { action: 'done', ids: [7], seen: false, ready: true })
  const before = rig.snapshots.length
  // 검색을 닫아 카드가 걸러지지 않고 보이면 지금 탭으로 돌아온 것과 같이 읽는다.
  await panel.press(panel.find({ label: '검색 닫기' }) as never)
  assert.deepEqual(rig.snapshots.slice(before), [{ seen: true }])
  // 거른 적 없이 열고 닫으면 읽지 않는다.
  await panel.press(panel.find({ label: '검색' }) as never)
  await panel.press(panel.find({ label: '검색 닫기' }) as never)
  assert.equal(rig.snapshots.length, before + 1)
  // 검색어는 워커로 가지 않는다.
  assert.ok(rig.snapshots.every(input => Object.keys(input).every(key => key === 'seen' || key === 'ready')))
})

test('검색어를 지운 뒤에 닫아도 거르는 동안 못 적은 노출을 적는다', async () => {
  const { rig, panel } = await open(scene({ card: card([startable(7)]) }, [item(1)]))
  await panel.press(panel.find({ label: '검색' }) as never)
  await searchFor(panel, '이슈 1')
  await searchFor(panel, '')
  const before = rig.snapshots.length
  await panel.press(panel.find({ label: '검색 닫기' }) as never)
  assert.deepEqual(rig.snapshots.slice(before), [{ seen: true }])
})

test('지금 탭으로 돌아온 읽기가 도는 동안에는 탭을 오가도 다시 읽지 않는다', async () => {
  const shown = scene({ card: card([startable(7)]) }, [item(1), parked(2)])
  const reads: SnapshotInput[] = []
  const waiting: ((value: Snapshot) => void)[] = []
  const host = mockHost({
    capabilities: CAPS,
    requests: {
      'kairos.snapshot': (input: SnapshotInput) => {
        reads.push(input)
        return reads.length === 1 ? shown : new Promise<Snapshot>(resolve => waiting.push(resolve))
      },
    },
  })
  view = await mount(panelModule, { host })
  const panel = view
  const settle = (): Promise<void> => new Promise(resolve => setImmediate(resolve))
  // 읽기가 도는 동안의 조작은 끝을 기다리지 않고 보낸다. 기다리면 답을 풀기 전까지 돌아오지 않는다.
  const moves: Promise<void>[] = []
  const go = async (id: 'now' | 'waiting'): Promise<void> => {
    moves.push(tabTo(panel, id))
    await settle()
  }
  await tabTo(panel, 'waiting')
  await go('now')
  assert.deepEqual(reads, [{ seen: true }, { seen: true }])
  for (let round = 0; round < 3; round += 1) {
    await go('waiting')
    assert.equal(tabValue(panel), 'waiting')
    await go('now')
    assert.equal(tabValue(panel), 'now')
  }
  assert.equal(reads.length, 2)
  waiting.shift()?.(shown)
  await Promise.all(moves)
  // 읽기가 끝난 뒤에 돌아오면 다시 읽는다.
  await tabTo(panel, 'waiting')
  const back = tabTo(panel, 'now')
  await settle()
  assert.equal(reads.length, 3)
  waiting.shift()?.(shown)
  await back
})

test('게이트가 닫혀 있으면 탭이 없으므로 읽기는 예전처럼 노출을 청한다', async () => {
  const { rig, panel } = await open(scene({ display: recordingGate }))
  await panel.press(panel.find({ label: '새로고침' }) as never)
  assert.deepEqual(rig.snapshots, [{ seen: true }, { seen: true }])
})

const bundleRows = [item(4, { tier: 'P2' }), item(5, { tier: 'P2' })]

test('알림에서 연 일이 카드에 있으면 다른 탭이나 검색 중이어도 지금 탭으로 돌아와 카드를 보여 준다', async () => {
  const shown = scene({ card: card([startable(9)]) }, [item(1), parked(2)])
  const { state, panel } = await openLive(shown)
  await tabTo(panel, 'waiting')
  assert.equal(button(panel, START), null)
  state.snapshot = scene({ card: card([startable(9)]), ...revealed([9]) }, [item(1), parked(2)])
  await panel.post('kairos.reveal', { at: 1 })
  assert.equal(tabValue(panel), 'now')
  assert.ok(button(panel, START))
  assert.equal(hasStrip(panel), false)
  view?.unmount()
  const searching = await openLive(shown)
  await searching.panel.press(searching.panel.find({ label: '검색' }) as never)
  await searchFor(searching.panel, '이슈 1')
  assert.equal(button(searching.panel, START), null)
  searching.state.snapshot = scene({ card: card([startable(9)]), ...revealed([9], REVEAL_AT + 1) }, [item(1), parked(2)])
  await tick(searching.panel)
  assert.equal(searching.panel.find({ type: 'Input', label: SEARCH }), null)
  assert.ok(button(searching.panel, START))
})

test('알림을 누르면 카드를 그리는 화면으로 돌아와 읽으므로 노출을 적는다', async () => {
  const { rig, panel } = await open(scene({ card: card([startable(9)]) }, [item(1), parked(2)]))
  await tabTo(panel, 'sessions')
  await panel.post('kairos.reveal', { at: 1 })
  assert.deepEqual(rig.snapshots.at(-1), { seen: true })
  assert.equal(tabValue(panel), 'now')
})

test('패널을 다시 보이면 지금 탭에서 시작하고 노출을 적는다', async () => {
  const { rig, panel } = await open(scene({ card: card([startable(9)]) }, [item(1), parked(2)]))
  await tabTo(panel, 'waiting')
  await panel.setVisible(false)
  await panel.setVisible(true)
  assert.equal(tabValue(panel), 'now')
  assert.deepEqual(rig.snapshots.at(-1), { seen: true })
  assert.ok(button(panel, START))
})

test('묶음 카드가 접어 둔 일은 지금 탭이 아니라 제 탭에서 센다', async () => {
  const rest = Array.from({ length: 3 }, (_, index) => item(index + 10, { tier: 'P2' }))
  const pile = { ...bundle(bundleRows, 3), itemIds: [4, 5, 10, 11, 12] }
  const { panel } = await open(scene({ card: pile }, rest))
  assert.deepEqual(tabLabels(panel), ['지금 2', '대기 3', '세션'])
})

test('카드 안에서 펼친 줄은 카드가 화면에서 사라지면 접어 틱을 붙잡지 않는다', async () => {
  const before = scene({ card: bundle(bundleRows) }, [parked(20)])
  const { state, panel } = await openLive(before)
  await expand(panel, '합성 이슈 4')
  assert.ok(button(panel, '완료'))
  await tabTo(panel, 'waiting')
  state.snapshot = scene({ card: bundle(bundleRows) }, [parked(20, { title: '새 제목' })])
  await tick(panel)
  assert.deepEqual(rowTitles(panel), ['새 제목'])
  await tabTo(panel, 'now')
  assert.equal(button(panel, '완료'), null)
})

test('펼친 줄이 틱으로 다른 탭의 묶음으로 옮겨 가면 접고 다음 틱을 붙잡지 않는다', async () => {
  const { state, panel } = await openLive(scene({}, [item(1), item(2)]))
  await expand(panel, '합성 이슈 1')
  state.snapshot = scene({}, [accepted(1), item(2)])
  // 펼친 줄이 있어 네 번은 맡아 두고 다섯 번째에 반영한다.
  for (let round = 0; round < 5; round += 1) await tick(panel)
  assert.deepEqual(rowTitles(panel), ['합성 이슈 2'])
  state.snapshot = scene({}, [accepted(1), item(2, { title: '새 제목' })])
  await tick(panel)
  assert.deepEqual(rowTitles(panel), ['새 제목'])
  await tabTo(panel, 'sessions')
  assert.equal(button(panel, '세션으로 이동'), null)
  assert.ok(button(panel, '이동'))
})

test('다른 탭에 있는 줄의 실패는 그 줄을 펼치지 않고 위로 띄운다', async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const { panel } = await open(scene({}, [item(1), parked(2)]), () => new Promise(resolve => (release = resolve)))
  await expand(panel, '합성 이슈 1')
  const pressed = panel.press(button(panel, '완료') as never)
  await new Promise(resolve => setImmediate(resolve))
  release({ ok: false, code: 'busy', message: '거절', run: null, snapshot: scene({}, [parked(1), parked(2)]) })
  await pressed
  assert.deepEqual(rowTitles(panel), [])
  assert.equal(panel.find({ type: 'Callout' })?.props.title, '완료하지 못했습니다')
  await tabTo(panel, 'waiting')
  assert.equal(button(panel, '완료'), null)
})

test('띠로 올라간 줄이 탭의 유일한 일이어도 탭이 비었다고 말하지 않는다', async () => {
  const { panel } = await open(scene(revealed([1]), [item(1), parked(2)]))
  assert.equal(hasStrip(panel), true)
  assert.equal(panel.find({ type: 'EmptyState' }), null)
  assert.equal(panel.find({ role: 'button', text: /건 보기$/ }), null)
  assert.deepEqual(tabLabels(panel), ['지금 1', '대기 1', '세션'])
  view?.unmount()
  const waiting = await open(scene(revealed([2], REVEAL_AT + 5), [item(1), parked(2)]))
  await tabTo(waiting.panel, 'waiting')
  assert.equal(hasStrip(waiting.panel), true)
  assert.equal(waiting.panel.find({ type: 'EmptyState' }), null)
  assert.deepEqual(tabLabels(waiting.panel), ['지금 1', '대기 1', '세션'])
})

test('게이트가 닫혀 탭이 없으면 추가한 자리를 탭으로 말하지 않는다', async () => {
  const { panel } = await open(scene({ display: recordingGate }), () => ({ ok: true, code: null, message: '추가했습니다', run: null, snapshot: scene({ display: recordingGate }) }))
  await panel.press(panel.find({ label: '할 일 추가' }) as never)
  await panel.type(panel.find({ type: 'Input' }) as never, '합성 할 일', { submit: true })
  assert.ok(panel.find({ text: '추가했습니다' }))
  assert.equal(panel.find({ text: '대기 탭에 추가했습니다' }), null)
})

test('세션 탭에서 처리 중이면 다른 줄의 이동이 잠긴다', async () => {
  let release: (value: ActionOutcome) => void = () => undefined
  const shown = scene({}, [accepted(8), accepted(9)])
  const { rig, panel } = await open(shown, () => new Promise(resolve => (release = resolve)))
  await tabTo(panel, 'sessions')
  await expand(panel, '합성 이슈 8')
  const pressed = panel.press(button(panel, '세션으로 이동') as never)
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(rig.actions.length, 1)
  assert.equal(button(panel, '이동')?.props.disabled, true)
  release({ ok: true, code: null, message: '이동함', run: null, snapshot: shown })
  await pressed
  assert.equal(button(panel, '이동')?.props.disabled ?? false, false)
})

/** 프로바이더가 여럿인 하루. 소스는 모두 연결돼 있다. */
const mixedRows = (): Record<string, unknown>[] => [
  item(1, { title: '합성 깃허브 이슈' }),
  item(2, { source: 'slack', sourceLabel: 'Slack', reason: 'direct_mention', reasonLabel: '나를 언급', title: '합성 슬랙 질문' }),
  item(3, { source: 'notion', sourceLabel: 'Notion', reason: 'notion_mention', reasonLabel: 'Notion 언급', title: '합성 노션 문서' }),
]

/** 새 일 기준을 저장소에 미리 둔 패널. 틱마다 스냅숏을 바꿀 수 있다. */
const openMarked = async (first: Snapshot, stored: Record<string, unknown> | null) => {
  const state = { snapshot: first }
  const host = mockHost({
    capabilities: CAPS,
    storage: stored === null ? {} : { marks: stored as never },
    requests: {
      'kairos.snapshot': () => state.snapshot,
      'kairos.action': () => ({ ok: true, code: null, message: '처리했습니다', run: null, snapshot: state.snapshot }),
    },
  })
  view = await mount(panelModule, { host })
  return { state, panel: view }
}

/** 머문 시간을 흉내 낸다. 화면을 떠날 때 2초 넘게 본 줄만 본 것으로 적는다. */
const realNow = Date.now
afterEach(() => {
  Date.now = realNow
})
const later = (ms: number): void => {
  const base = Date.now()
  Date.now = () => base + ms
}
const freshWords = (panel: TestPanel): number => panel.findAll({ type: 'Text', text: '● 새 일' }).length

test('프로바이더 칩은 전체와 연결된 소스를 늘 같은 순서로 두고 지금 탭의 건수를 붙인다', async () => {
  const { panel } = await open(scene({ sources: sourceRows(true) }, mixedRows()))
  assert.deepEqual(chipLabels(panel), ['전체 3', 'GitHub 1', 'Slack 1', 'Calendar', 'Gmail', 'Notion 1'])
  assert.equal(chip(panel, 'all')?.props.variant, 'solid')
  assert.equal(chip(panel, 'slack')?.props.variant, 'ghost')
  // 여러 곳의 일이 섞이면 줄의 아이콘과 첫 낱말이 출처다. 같은 묶음 안에서는 최신이 위다.
  assert.deepEqual(rowsOf(panel).map(row => row.props.icon), ['book', 'zap', 'branch'])
  assert.ok(panel.find({ type: 'Text', text: 'Slack · 나를 언급' }))
  assert.equal(chip(panel, 'all')?.props.tooltip, '모든 곳의 일을 함께 봅니다')
  assert.deepEqual(panel.errors, [])
})

test('칩을 고르면 그 곳의 일만 그리고 줄에서 출처 낱말을 뺀다', async () => {
  const { panel } = await open(scene({ sources: sourceRows(true) }, mixedRows()))
  await panel.press(chip(panel, 'slack') as never)
  assert.equal(chip(panel, 'slack')?.props.variant, 'solid')
  assert.deepEqual(rowTitles(panel), ['합성 슬랙 질문'])
  assert.equal(panel.find({ type: 'Text', text: 'Slack · 나를 언급' }), null)
  assert.ok(panel.find({ type: 'Text', text: '나를 언급' }))
  assert.deepEqual(tabLabels(panel), ['지금 1', '대기', '세션'])
  // 일이 없는 소스를 고르면 빈 상태다. 다른 곳의 일은 보이지 않는다.
  await panel.press(chip(panel, 'calendar') as never)
  assert.deepEqual(rowTitles(panel), [])
  await panel.press(chip(panel, 'all') as never)
  assert.equal(rowTitles(panel).length, 3)
})

test('칩은 고를 곳이 둘 이상일 때만 그리고 한 곳뿐이면 줄에 출처를 쓰지 않는다', async () => {
  const { panel } = await open(scene({}, [item(1), item(2)]))
  assert.deepEqual(chipLabels(panel), [])
  assert.equal(rowsOf(panel).length, 2)
  assert.ok(!rowsOf(panel).some(row => row.props.icon === 'branch'))
  assert.ok(!texts(panel).includes('GitHub · 담당 이슈'))
})

test('먼저 볼 일 카드는 그 카드의 일이 고른 곳에서 왔을 때만 보인다', async () => {
  const first = item(1, { tier: 'P0', title: '합성 깃허브 긴급' })
  const { panel } = await open(scene({ sources: sourceRows(true), card: card([first]), counts: counts({ P0: 1 }) }, [first, ...mixedRows().slice(1)]))
  assert.ok(texts(panel).includes('합성 깃허브 긴급'))
  await panel.press(chip(panel, 'slack') as never)
  assert.ok(!texts(panel).includes('합성 깃허브 긴급'))
  await panel.press(chip(panel, 'github') as never)
  assert.ok(texts(panel).includes('합성 깃허브 긴급'))
})

test('처음 연 패널은 그때 있던 일을 새 일로 치지 않고 기준을 저장소에 적는다', async () => {
  const { panel } = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), null)
  assert.equal(freshWords(panel), 0)
  assert.deepEqual(tabLabels(panel), ['지금 3', '대기', '세션'])
  assert.deepEqual(panel.persisted.storage.marks, { floor: 3, seen: [], known: 3, unseen: 0 })
})

test('기준 뒤에 열린 일은 줄과 탭과 칩에 빨간 점을 찍는다', async () => {
  const { panel } = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), { floor: 1, seen: [], known: 1, unseen: 0 })
  assert.equal(freshWords(panel), 2)
  assert.deepEqual(tabLabels(panel), ['지금 3 ●', '대기', '세션'])
  const dots = panel.findAll({ type: 'Text', text: '●' })
  // 전체, Slack, Notion 칩 옆에 점이 있다. GitHub는 기준 안의 일뿐이다.
  assert.equal(dots.length, 3)
  assert.equal(chip(panel, 'slack')?.props.tooltip, 'Slack · 새 일 1건')
  assert.equal(chip(panel, 'github')?.props.tooltip, 'GitHub에서 온 일')
  assert.deepEqual(panel.errors, [])
})

test('2초 넘게 본 화면을 떠나면 그린 새 일을 본 것으로 적고 점을 지운다', async () => {
  const { panel } = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), { floor: 1, seen: [], known: 1, unseen: 0 })
  later(3_000)
  await panel.press(chip(panel, 'slack') as never)
  await panel.settled()
  assert.deepEqual(panel.persisted.storage.marks, { floor: 3, seen: [], known: 3, unseen: 0 })
  assert.equal(freshWords(panel), 0)
  assert.equal(chip(panel, 'notion')?.props.tooltip, 'Notion에서 온 일')
})

test('잠깐 지나친 화면은 본 것으로 적지 않는다', async () => {
  const { panel } = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), { floor: 1, seen: [], known: 1, unseen: 0 })
  later(500)
  await panel.press(chip(panel, 'github') as never)
  await panel.settled()
  assert.equal((panel.persisted.storage.marks as { unseen: number }).unseen, 2)
  assert.equal(chip(panel, 'slack')?.props.tooltip, 'Slack · 새 일 1건')
})

test('고른 곳만 본 것으로 적는다. 다른 곳의 새 일은 점이 남는다', async () => {
  const { panel } = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), { floor: 1, seen: [], known: 1, unseen: 0 })
  await panel.press(chip(panel, 'slack') as never)
  later(3_000)
  await panel.press(chip(panel, 'github') as never)
  await panel.settled()
  assert.deepEqual(panel.persisted.storage.marks, { floor: 2, seen: [], known: 3, unseen: 1 })
  assert.equal(chip(panel, 'notion')?.props.tooltip, 'Notion · 새 일 1건')
  assert.equal(chip(panel, 'slack')?.props.tooltip, 'Slack에서 온 일')
})

test('틱으로 새 일이 오면 그 곳의 칩에 점이 생긴다', async () => {
  const { state, panel } = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), null)
  state.snapshot = scene({ sources: sourceRows(true), newest: 4 }, [...mixedRows(), item(4, { source: 'gmail', sourceLabel: 'Gmail', reason: 'mail', reasonLabel: '메일', title: '합성 메일' })])
  await tick(panel)
  assert.equal(chip(panel, 'gmail')?.props.tooltip, 'Gmail · 새 일 1건')
  assert.equal(freshWords(panel), 1)
})

test('펼친 줄은 왜 올라왔는지와 판단 근거를 고정 문구로 쓴다', async () => {
  const row = item(1, { title: '합성 PR', why: { rule: 'p1.ci_failed', events: 3, facts: ['pull_request', 'authored_me', 'checks_failed'] } })
  const { panel } = await open(scene({}, [row]))
  await expand(panel, '합성 PR')
  assert.match(texts(panel), /내 PR의 CI 검사가 실패했습니다\./)
  const badges = panel.findAll({ type: 'Badge' }).map(found => [found.text, found.props.tone])
  for (const expected of [['PR', 'neutral'], ['내가 만든 일', 'neutral'], ['검사 실패', 'danger'], ['소식 3건', 'neutral']]) {
    assert.ok(badges.some(([text, tone]) => text === expected[0] && tone === expected[1]), JSON.stringify(expected))
  }
})

test('제안 상자는 맡기는 일, 근거, 일할 곳, 하는 일을 차례로 쓴다', async () => {
  const offer = { repo: 'contoso-synthetic/issues', workspace: 'contoso-synthetic/app', base: 'origin/main', checks: ['assigned_me', 'open', 'mapped', 'no_session'] }
  const row = startable(1, { title: '합성 제안', why: { rule: 'p2.assigned', events: 1, facts: ['issue', 'assigned_me'], offer } })
  const { panel } = await open(scene({}, [row]))
  await expand(panel, '합성 제안')
  const callout = panel.find({ type: 'Callout' })
  assert.equal(callout?.props.title, '에이전트가 이슈를 고칩니다')
  const body = callout?.text ?? ''
  const order = [SCOPE, '맡겨도 된다고 본 근거', '내게 배정된 이슈', '에이전트가 하는 일', '시작을 누르면']
  const at = order.map(word => body.indexOf(word))
  assert.ok(at.every(index => index >= 0), JSON.stringify(at))
  assert.deepEqual([...at].sort((a, b) => a - b), at)
  const place = panel.find({ type: 'KeyValue' })
  assert.equal(place?.props.label, '세션이 일할 곳')
  assert.deepEqual(place?.props.items, [
    { label: '작업 저장소', value: 'contoso-synthetic/app', code: true },
    { label: '기준 브랜치', value: 'origin/main', code: true },
    { label: '이슈 저장소', value: 'contoso-synthetic/issues', code: true },
  ])
  // 하는 일은 접혀 있다. 펼치면 순서와 하지 않는 일을 쓴다.
  const step = { type: 'Text', text: '1. 이슈와 댓글을 읽고 원인을 조사합니다' }
  assert.equal(panel.find(step), null)
  await panel.toggle({ type: 'Collapsible' })
  assert.ok(panel.find(step))
  assert.ok(panel.find({ type: 'Text', text: /push, PR, 코멘트, Notion 쓰기는 하지 않습니다/ }))
})

test('저장소의 기준은 칸마다 검사해 맞는 값만 쓰고 틀린 모양에도 그리기가 멈추지 않는다', async () => {
  const partial = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), { floor: 1 })
  assert.deepEqual(partial.panel.errors, [])
  assert.equal(freshWords(partial.panel), 2)
  await partial.panel.unmount()
  const broken = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), { floor: 'x', seen: 'y' })
  assert.deepEqual(broken.panel.errors, [])
  assert.equal(freshWords(broken.panel), 0)
  assert.deepEqual(broken.panel.persisted.storage.marks, { floor: 3, seen: [], known: 3, unseen: 0 })
})

test('칩을 고른 화면의 읽기는 카드가 보여도 노출을 적지 않는다', async () => {
  const first = item(1, { tier: 'P0', title: '합성 깃허브 긴급' })
  const { rig, panel } = await open(scene({ sources: sourceRows(true), card: card([first]), counts: counts({ P0: 1 }) }, [first, ...mixedRows().slice(1)]))
  await panel.press(chip(panel, 'github') as never)
  assert.ok(texts(panel).includes('합성 깃허브 긴급'))
  const before = rig.snapshots.length
  await panel.press({ type: 'IconButton', label: '새로고침' })
  assert.deepEqual(rig.snapshots.slice(before), [{ seen: false, ready: true }])
  // 전체로 돌아오면 카드가 다시 화면에 든 것이라 한 번 seen으로 읽는다.
  await panel.press(chip(panel, 'all') as never)
  assert.deepEqual(rig.snapshots.at(-1), { seen: true })
})

test('칩을 고른 화면에서도 카드의 줄을 펼칠 수 있다', async () => {
  const rowsInCard = [item(1, { title: '합성 묶음 하나' }), item(2, { title: '합성 묶음 둘' })]
  const { panel } = await open(scene({ sources: sourceRows(true), card: merged(rowsInCard), counts: counts({ P1: 2 }) }, [...rowsInCard, ...mixedRows().slice(1)]))
  await panel.press(chip(panel, 'github') as never)
  await expand(panel, '합성 묶음 하나')
  const opened = () => rowsOf(panel).find(row => row.key === 'row-1-item')?.props.current
  assert.equal(opened(), true)
  // 펼친 줄은 화면에 있으므로 다음 틱에 접히지 않는다.
  await tick(panel)
  assert.equal(opened(), true)
})

test('게이트가 닫힌 채 처음 읽으면 기준을 잡지 않고 열린 뒤 처음 읽은 목록이 기준이다', async () => {
  const closed = scene({ display: recordingGate, hold: { reason: 'recording', message: '기록 중입니다', until: null }, newest: 3 }, [], { hidden: 3 })
  const { state, panel } = await openMarked(closed, null)
  assert.equal(panel.persisted.storage.marks, undefined)
  state.snapshot = scene({ sources: sourceRows(true), newest: 3 }, mixedRows())
  await tick(panel)
  assert.equal(freshWords(panel), 0)
  assert.deepEqual(panel.persisted.storage.marks, { floor: 3, seen: [], known: 3, unseen: 0 })
})

test('보는 동안 2초가 지나면 본 것을 저장하고 화면의 점은 떠날 때 지운다', async () => {
  const { panel } = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), { floor: 1, seen: [], known: 1, unseen: 0 })
  later(3_000)
  // 저장은 실제 시간으로 2초 뒤에 돈다. 닫아도 남도록 보는 동안 저장한다.
  await new Promise(resolve => setTimeout(resolve, 2_100))
  await panel.settled()
  assert.deepEqual(panel.persisted.storage.marks, { floor: 3, seen: [], known: 3, unseen: 0 })
  assert.equal(freshWords(panel), 2)
  assert.deepEqual(tabLabels(panel), ['지금 3 ●', '대기', '세션'])
  await panel.press(chip(panel, 'slack') as never)
  await panel.settled()
  assert.equal(freshWords(panel), 0)
})

test('머문 시간은 줄마다 잰다. 떠나기 직전에 온 새 일은 본 것으로 적지 않는다', async () => {
  const { state, panel } = await openMarked(scene({ sources: sourceRows(true) }, mixedRows()), null)
  later(10_000)
  state.snapshot = scene({ sources: sourceRows(true), newest: 4 }, [...mixedRows(), item(4, { source: 'slack', sourceLabel: 'Slack', title: '합성 새 슬랙' })])
  await tick(panel)
  await panel.press(chip(panel, 'slack') as never)
  await panel.settled()
  assert.equal(freshWords(panel), 1)
  assert.equal((panel.persisted.storage.marks as { unseen: number }).unseen, 1)
})

test('칩의 새 일이 카드에만 있으면 지금 탭으로 옮긴다', async () => {
  const first = item(4, { tier: 'P0', title: '합성 새 긴급' })
  const { panel } = await openMarked(scene({ sources: sourceRows(true), card: card([first]), counts: counts({ P0: 1 }) }, [...mixedRows(), first, parked(5)]), { floor: 3, seen: [], known: 3, unseen: 0 })
  await tabTo(panel, 'waiting')
  await panel.press(chip(panel, 'github') as never)
  assert.equal(panel.valueOf({ type: 'Tabs' }), 'now')
})
