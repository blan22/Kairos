import { assert, test } from 'orca/testing'
import { accepted, item, listJson, manual, offered, parked, startable } from './fixtures.ts'
import { parseList } from './parse.ts'
import type { ItemRow } from './protocol.ts'
import { canStart, defaultOpen, DISPLAY_ORDER, groupRows, hasOffer, isCompact, ROW_LIMIT, sectionOf, sortReady, sortRows, trimRows, visibleCount, type Section } from './rows.ts'

const rows = (...raw: Record<string, unknown>[]): ItemRow[] => parseList(listJson(raw))?.items ?? []
const row = (raw: Record<string, unknown>): ItemRow => rows(raw)[0] as ItemRow
const idsOf = (list: readonly ItemRow[]): number[] => list.map(each => each.id)

test('묶음 판정은 세션, 미룸, 기준선, 등급 순서다', () => {
  assert.equal(sectionOf(row(accepted(1))), 'running')
  assert.equal(sectionOf(row({ ...item(1), state: 'accepted' })), 'running')
  assert.equal(sectionOf(row({ ...parked(1), tier: 'P0' })), 'parked')
  assert.equal(sectionOf(row(item(1, { baseline: true, tier: 'P0' }))), 'baseline')
  assert.equal(sectionOf(row(item(1, { tier: 'P0' }))), 'urgent')
  assert.equal(sectionOf(row(item(1, { tier: 'P1' }))), 'today')
  assert.equal(sectionOf(row(item(1, { tier: 'P2' }))), 'relaxed')
  assert.equal(sectionOf(row({ ...accepted(1), state: 'closed' })), 'running')
})

test('제안은 등급보다 앞서 묶지만 긴급, 세션, 미룸, 기준선은 자리를 지킨다', () => {
  assert.equal(sectionOf(row(startable(1))), 'offer')
  assert.equal(sectionOf(row(startable(1, { tier: 'P2' }))), 'offer')
  assert.equal(sectionOf(row(offered(1))), 'offer')
  assert.equal(sectionOf(row(startable(1, { tier: 'P0' }))), 'urgent')
  assert.equal(sectionOf(row(startable(1, { baseline: true }))), 'baseline')
  assert.equal(sectionOf(row({ ...startable(1), state: 'parked' })), 'parked')
  assert.equal(sectionOf(row({ ...startable(1), run: { id: 'r', state: 'started' } })), 'running')
  assert.equal(sectionOf(row({ ...offered(1), scope: null })), 'today')
})

test('제안 묶음은 접지 않고 등급 묶음보다 앞서 남는다', () => {
  const groups = groupRows(rows(startable(1, { tier: 'P2' }), item(2, { tier: 'P2' })))
  assert.deepEqual(idsOf(groups.offer), [1])
  assert.deepEqual(idsOf(groups.relaxed), [2])
  assert.equal(defaultOpen('offer', groups), undefined)
  assert.deepEqual(idsOf(sortRows(rows(item(1, { tier: 'P1' }), startable(2, { tier: 'P2' })))), [2, 1])
})

test('제안은 플레이북과 범위가 다 있어야 한다', () => {
  assert.equal(hasOffer(row(offered(1))), true)
  assert.equal(hasOffer(row(item(1))), false)
  assert.equal(hasOffer(row({ ...offered(1), scope: null })), false)
  assert.equal(hasOffer(row({ ...offered(1), playbook: null })), false)
})

test('묶음이 먼저이고 묶음 안에서는 최신이 위다', () => {
  const sorted = sortRows(
    rows(
      item(1, { tier: 'P2', openedAt: '2026-11-04T09:00:00.000Z' }),
      item(2, { tier: 'P1', openedAt: '2026-11-02T00:00:00.000Z' }),
      item(3, { tier: 'P1', openedAt: '2026-11-03T00:00:00.000Z' }),
      item(4, { tier: 'P0', openedAt: '2026-11-01T00:00:00.000Z' }),
    ),
  )
  assert.deepEqual(idsOf(sorted), [4, 3, 2, 1])
})

test('시각을 못 읽으면 같은 묶음의 맨 뒤로 가고 다른 묶음을 넘지 않는다', () => {
  const sorted = sortRows(
    rows(
      item(1, { openedAt: '아님' }),
      item(2, { openedAt: null }),
      item(3, { openedAt: '2026-11-01T00:00:00.000Z' }),
      item(4, { tier: 'P2', openedAt: '2026-11-04T00:00:00.000Z' }),
    ),
  )
  assert.deepEqual(idsOf(sorted), [3, 2, 1, 4])
})

test('시각이 같으면 ID가 큰 쪽이 위다', () => {
  assert.deepEqual(idsOf(sortRows(rows(item(1), item(3), item(2)))), [3, 2, 1])
})

test('정렬은 입력을 바꾸지 않는다', () => {
  const input = rows(item(1), item(2))
  const before = idsOf(input)
  sortRows(input)
  assert.deepEqual(idsOf(input), before)
})

test('화면 순서에서만 긴급이 진행 중 위로 간다', () => {
  assert.deepEqual(DISPLAY_ORDER, ['urgent', 'running', 'offer', 'today', 'relaxed', 'parked', 'baseline'])
  assert.deepEqual(idsOf(sortRows(rows(item(1, { tier: 'P0' }), accepted(2)))), [2, 1])
})

test('접기 기본값은 미룸과 기준선은 접힘, 여유는 6건 이상일 때만 접을 수 있고 오늘이 있으면 접힘이다', () => {
  const groupsOf = (today: number, relaxed: number): Record<Section, ItemRow[]> =>
    groupRows([
      ...rows(...Array.from({ length: today }, (_, index) => item(index + 1))),
      ...rows(...Array.from({ length: relaxed }, (_, index) => item(index + 100, { tier: 'P2' }))),
    ])
  assert.equal(defaultOpen('parked', groupsOf(0, 0)), false)
  assert.equal(defaultOpen('baseline', groupsOf(0, 0)), false)
  assert.equal(defaultOpen('relaxed', groupsOf(1, 5)), undefined)
  assert.equal(defaultOpen('relaxed', groupsOf(0, 6)), true)
  assert.equal(defaultOpen('relaxed', groupsOf(1, 6)), false)
  for (const section of ['urgent', 'running', 'today'] as const) assert.equal(defaultOpen(section, groupsOf(1, 6)), undefined)
})

test('자르기는 묶음마다 줄을 남기고 앞쪽 묶음부터 채운다', () => {
  const today = Array.from({ length: 100 }, (_, index) => item(index + 1, { openedAt: '2026-10-01T00:00:00.000Z' }))
  const later = Array.from({ length: 12 }, (_, index) => parked(index + 200))
  const sorted = sortRows(rows(...today, ...later, item(300, { baseline: true })))
  const kept = trimRows(sorted, 100)
  assert.equal(kept.length, 100)
  const groups = groupRows(kept)
  assert.equal(groups.parked.length, 10)
  assert.equal(groups.baseline.length, 1)
  assert.equal(groups.today.length, 89)
  assert.deepEqual(idsOf(kept), idsOf(sorted.filter(row => kept.includes(row))))
})

test('한도 안이면 그대로다', () => {
  assert.deepEqual(idsOf(trimRows(rows(item(1), item(2)), 100)), [2, 1])
})

test('한 묶음의 줄은 줄이되 세션 줄과 펼친 행은 숨기지 않는다', () => {
  assert.equal(visibleCount('today', ROW_LIMIT + 2, false, -1), ROW_LIMIT + 2)
  assert.equal(visibleCount('today', ROW_LIMIT + 3, false, -1), ROW_LIMIT)
  assert.equal(visibleCount('urgent', 40, false, -1), ROW_LIMIT)
  assert.equal(visibleCount('today', 40, true, -1), 40)
  assert.equal(visibleCount('running', 40, false, -1), 40)
  assert.equal(visibleCount('today', 40, false, 20), 21)
})

test('시작 가능은 시작이 있고 세션이 없고 범위가 있을 때뿐이다', () => {
  assert.equal(canStart(row(startable(1))), true)
  assert.equal(canStart(row(item(1))), false)
  assert.equal(canStart(row(manual(1))), false)
  assert.equal(canStart(row(offered(1))), false)
  assert.equal(canStart(row({ ...startable(1), scope: null })), false)
  assert.equal(canStart(row({ ...startable(1), run: { id: 'r', state: 'started' } })), false)
})

test('세션 소식은 실패, 질문, 확인, 준비됨, 끝남, 그 밖 순서이고 같은 종류는 나중에 온 것이 위다', () => {
  const order = sortReady([
    { kind: 'done', n: 1 },
    { kind: 'unknown', n: 2 },
    { kind: 'ended', n: 3 },
    { kind: 'done', n: 4 },
    { kind: 'question', n: 5 },
    { kind: 'attention', n: 6 },
    { kind: 'failed', n: 7 },
  ])
  assert.deepEqual(order.map(card => card.n), [7, 5, 6, 4, 1, 3, 2])
})

test('기다려도 되는 묶음만 한 줄로 그린다', () => {
  assert.deepEqual(['urgent', 'running', 'offer', 'today', 'relaxed', 'parked', 'baseline'].filter(section => isCompact(section as never)), ['relaxed', 'parked', 'baseline'])
})
