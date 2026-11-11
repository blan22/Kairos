import { assert, test } from 'orca/testing'
import { LIST_LIMIT } from './config.ts'
import { DANGEROUS_TITLE, LONG_TITLE, SLACK_TITLE, accepted, bundle, card, counts, failedCard, item, listJson, manyItems, nextJson, parked, readyCard, revivable, startable } from './fixtures.ts'
import { failureOf, parseList, parseNext, resultLine } from './parse.ts'
import { cleanText } from './text.ts'

test('위험 문자가 든 제목은 고치지 않고 글자 그대로 넘긴다', () => {
  const next = parseNext(nextJson({ card: card([item(1, { title: DANGEROUS_TITLE })]) }))
  assert.equal(next?.card?.items[0]?.title, DANGEROUS_TITLE)
})

test('제어 문자와 양방향 재정렬 문자는 뺀다', () => {
  assert.equal(cleanText('앞\u202e뒤\u0000\n다음\u200b줄', 50), '앞뒤 다음줄')
  assert.equal(cleanText(42, 50), '')
})

test('긴 제목은 300자로 자른다', () => {
  const [row] = parseList(listJson([item(1, { title: LONG_TITLE })]))?.items ?? []
  assert.equal([...(row?.title ?? '')].length, 300)
  assert.ok(row?.title.endsWith('…'))
})

test('빈 제목은 대체 문구를 쓴다', () => {
  const [row] = parseList(listJson([item(1, { title: '\u200b' })]))?.items ?? []
  assert.equal(row?.title, '(제목 없음)')
})

test('https가 아닌 주소는 버린다', () => {
  for (const url of ['javascript:alert(1)', 'http://example.test', 'https://a b', 'file:///etc/passwd']) {
    const [row] = parseList(listJson([item(1, { url })]))?.items ?? []
    assert.equal(row?.url, null, url)
  }
})

test('패널로 가는 값에 argv가 없다', () => {
  const next = parseNext(nextJson({ card: card([startable(1)]) }))
  assert.ok(!JSON.stringify(next).includes('argv'))
  assert.deepEqual(next?.card?.actions.map(action => action.id), ['start', 'later', 'done', 'keep'])
})

test('배포 연결 남김은 keep_deploy가 된다', () => {
  const actions = [{ id: 'keep', label: '배포 때 다시', argv: ['keep', '1', '--until-deploy', '--json'] }]
  const [row] = parseList(listJson([item(1, { actions })]))?.items ?? []
  assert.deepEqual(row?.actions, [{ id: 'keep_deploy', label: '배포 때 다시' }])
})

test('계약에 없는 버튼은 버린다', () => {
  const actions = [
    { id: 'triage_drop', label: '버리기', argv: ['triage', 'drop', '1', '--json'] },
    { id: 'rm', label: '지우기', argv: ['rm', '-rf', '/'] },
    { id: 'start', label: '', argv: [] },
  ]
  const [row] = parseList(listJson([item(1, { actions })]))?.items ?? []
  assert.deepEqual(row?.actions, [])
})

test('이동은 run이 있을 때만 붙는다', () => {
  const list = parseList(listJson([accepted(1), item(2)]))
  assert.deepEqual(list?.items[0]?.actions.map(action => action.id), ['done', 'move'])
  assert.ok(!list?.items[1]?.actions.some(action => action.id === 'move'))
  const next = parseNext(nextJson({ card: card([accepted(1)]) }))
  assert.deepEqual(next?.card?.actions.map(action => action.id), ['done', 'move'])
})

test('묶음 카드 버튼에는 이동이 붙지 않는다', () => {
  const next = parseNext(nextJson({ card: bundle([accepted(1)]) }))
  assert.deepEqual(next?.card?.actions.map(action => action.id), ['ack'])
})

test('kairos가 이동을 주면 하나만 둔다', () => {
  const actions = [{ id: 'move', label: '이동', argv: ['move', '1', '--json'] }]
  const [row] = parseList(listJson([{ ...accepted(1), actions }]))?.items ?? []
  assert.deepEqual(row?.actions, [{ id: 'move', label: '이동' }])
})

test('세션 카드는 정제하고 이동만 남긴다', () => {
  const raw = { ...readyCard(1), title: DANGEROUS_TITLE, url: 'http://example.test', actions: [{ id: 'done', label: '끝', argv: [] }, ...(readyCard(1).actions as unknown[])] }
  const [ready] = parseNext(nextJson({ ready: [raw, { ...readyCard(2), itemId: '2; id' }, { ...readyCard(3), runId: '' }] }))?.ready ?? []
  assert.equal(ready?.title, DANGEROUS_TITLE)
  assert.equal(ready?.url, null)
  assert.deepEqual(ready?.actions, [{ id: 'move', label: '이동' }])
  assert.equal(parseNext(nextJson({ ready: [raw, { ...readyCard(2), itemId: '2; id' }] }))?.ready.length, 1)
})

test('ready가 없어도 읽는다', () => {
  const { ready: _ignored, ...older } = nextJson()
  assert.deepEqual(parseNext(older)?.ready, [])
})

test('ID 모양이 아닌 항목은 뺀다', () => {
  const list = parseList(listJson([item(1), { ...item(2), id: '2; rm' }, { ...item(3), id: 0 }, { ...item(4), id: 1.5 }, { ...item(5), tier: 'P9' }]))
  assert.deepEqual(list?.items.map(row => row.id), [1])
})

test('목록은 한도까지만 넘기고 뺀 건수를 알린다', () => {
  const rows = Array.from({ length: LIST_LIMIT + 7 }, (_, index) => item(index + 1))
  const list = parseList(listJson(rows))
  assert.equal(list?.items.length, LIST_LIMIT)
  assert.equal(list?.omitted, 7)
})

test('목록은 묶음 순서로 정렬한 뒤 같은 묶음 안에서는 최신이 위다', () => {
  const list = parseList(
    listJson([
      item(1, { tier: 'P2', openedAt: '2026-11-04T05:00:00.000Z' }),
      item(2, { tier: 'P1', openedAt: '2026-11-03T05:00:00.000Z' }),
      item(3, { tier: 'P1', openedAt: '2026-11-04T05:00:00.000Z' }),
      item(4, { tier: 'P0', openedAt: '2026-11-01T05:00:00.000Z' }),
      accepted(5),
      parked(6),
    ]),
  )
  assert.deepEqual(list?.items.map(row => row.id), [5, 4, 3, 2, 1, 6])
})

test('잘려도 뒤쪽 묶음이 사라지지 않고 묶음별 뺀 건수를 알린다', () => {
  const today = Array.from({ length: 100 }, (_, index) => item(index + 1, { openedAt: '2026-10-01T00:00:00.000Z' }))
  const list = parseList(listJson([...today, parked(200), item(201, { baseline: true })]))
  const ids = list?.items.map(row => row.id) ?? []
  assert.ok(ids.includes(200))
  assert.ok(ids.includes(201))
  assert.equal(list?.omitted, 2)
  assert.deepEqual(list?.omittedBy, { today: 2 })
})

test('120건을 자르면 최신과 세션 행이 남고 오래된 것이 빠진다', () => {
  const rows = [...manyItems(119).map(row => ({ ...row, tier: 'P1' })), accepted(500)]
  const list = parseList(listJson(rows))
  const ids = list?.items.map(row => row.id) ?? []
  assert.equal(ids.length, LIST_LIMIT)
  assert.equal(list?.omitted, 20)
  assert.equal(ids[0], 500)
  assert.equal(ids[1], 1)
  assert.ok(!ids.includes(119))
})

test('카드의 줄도 최신이 위다', () => {
  const next = parseNext(nextJson({ card: bundle([item(1, { openedAt: '2026-11-01T00:00:00.000Z' }), item(2, { openedAt: '2026-11-03T00:00:00.000Z' })]) }))
  assert.deepEqual(next?.card?.items.map(row => row.id), [2, 1])
  assert.deepEqual(next?.card?.itemIds, [1, 2])
})

test('제목의 Slack 표기는 항목과 세션 소식 모두 사람이 읽는 꼴로 바꾼다', () => {
  const shown = '@합성이름 님이 #합성채널에서 합성 링크 확인을 부탁했습니다 & @멤버'
  assert.equal(parseList(listJson([item(1, { title: SLACK_TITLE })]))?.items[0]?.title, shown)
  assert.equal(parseNext(nextJson({ ready: [{ ...readyCard(1), title: SLACK_TITLE }] }))?.ready[0]?.title, shown)
})

test('행 표시에 쓰는 값을 계약에서 옮긴다', () => {
  const [row] =
    parseList(
      listJson([
        item(1, {
          reason: 'deploy_risk',
          source: 'github',
          baseline: true,
          parkTrigger: 'deploy',
          dueAt: '2026-11-05T01:00:00.000Z',
          pushCount: 2,
          deploy: { date: '2026-11-07', linkedCount: 3 },
        }),
      ]),
    )?.items ?? []
  assert.deepEqual(
    [row?.reason, row?.source, row?.baseline, row?.parkTrigger, row?.dueAt, row?.pushCount, row?.deploy],
    ['deploy_risk', 'github', true, 'deploy', '2026-11-05T01:00:00.000Z', 2, { date: '2026-11-07' }],
  )
  const focus = { active: false, until: null, heldP1: 3, heldLine: '합성' }
  const next = parseNext(nextJson({ focus }))
  assert.equal(next?.focus.heldP1, 3)
  assert.equal(next?.at, '2026-11-04T02:00:00.000Z')
  assert.equal(parseList(listJson())?.at, '2026-11-04T02:00:00.000Z')
})

test('필수 값이 없으면 형식 오류다', () => {
  assert.equal(parseNext(nextJson({ display: null })), null)
  assert.equal(parseNext(nextJson({ counts: {} })), null)
  assert.equal(parseList({ schema: 1 }), null)
})

test('오류 코드는 모양을 지키고 메시지는 정제한다', () => {
  assert.deepEqual(failureOf({ error: { code: 'not_found', message: '없는\u202e 항목' } }), { code: 'not_found', message: '없는 항목' })
  assert.deepEqual(failureOf({ error: { code: '$(id)', message: '' } }), { code: 'failed', message: '처리하지 못했습니다' })
  assert.deepEqual(failureOf({}), { code: 'failed', message: '처리하지 못했습니다' })
})

test('조작 결과는 첫 줄을 쓴다', () => {
  assert.equal(resultLine({ results: [{ id: 1, status: 'already_closed', message: '이미 처리됨' }] }), '이미 처리됨')
  assert.equal(resultLine({}), '처리했습니다')
})

test('수락 경고는 결과 뒤에 붙인다', () => {
  assert.equal(resultLine({ results: [{ message: '세션을 여는 중입니다' }], warning: '일하는 세션이 5개입니다' }), '세션을 여는 중입니다. 일하는 세션이 5개입니다')
  assert.equal(resultLine({ results: [{ message: '세션을 여는 중입니다' }], warning: null }), '세션을 여는 중입니다')
})

test('되돌리기 버튼과 활성 run 수, 세션 카드의 종류를 옮긴다', () => {
  const next = parseNext(nextJson({ runs: { active: 2 }, ready: [failedCard(1)] }))
  assert.equal(next?.activeRuns, 2)
  assert.deepEqual([next?.ready[0]?.kind, next?.ready[0]?.actions], ['failed', []])
  assert.equal(parseNext(nextJson({ runs: undefined }))?.activeRuns, 0)
  assert.deepEqual(parseList(listJson([revivable(1)]))?.items[0]?.actions.map(action => action.id), ['cancel', 'done', 'move'])
})

test('플레이북은 값이 있을 때만 옮기고 제어 문자를 지운다', () => {
  const [plain, shown] = parseList(listJson([item(1), item(2, { playbook: 'pr\u0007_review', scope: '합성 범위' })]))?.items.sort((a, b) => a.id - b.id) ?? []
  assert.equal(plain?.playbook, null)
  assert.equal(shown?.playbook, 'pr_review')
  assert.equal(shown?.scope, '합성 범위')
})

test('밀린 일과 다음 묶음 시각을 읽고 없으면 비운다', () => {
  const next = parseNext(nextJson({ counts: counts({ P1: 2 }, { carriedP1: 2 }) }))
  assert.equal(next?.counts.carried, 2)
  assert.equal(next?.nextBundleAt, '2026-11-05T02:00:00.000Z')
  const bare = parseNext(nextJson({ budget: null }))
  assert.equal(bare?.nextBundleAt, null)
  assert.equal(bare?.counts.carried, 0)
})

const REVEAL_AT = 1_762_740_000_000

test('누른 알림의 항목과 시각을 읽고 겹친 ID는 하나로 둔다', () => {
  const next = parseNext(nextJson({ reveal: { items: [3, 5, 3], at: REVEAL_AT } }))
  assert.deepEqual(next?.reveal, { items: [3, 5], at: REVEAL_AT })
  assert.equal(parseNext(nextJson())?.reveal, null)
})

test('누른 알림은 ID와 시각만 옮기고 다른 값은 버린다', () => {
  const next = parseNext(nextJson({ reveal: { items: [1], at: REVEAL_AT, title: DANGEROUS_TITLE, url: 'https://evil.test' } }))
  assert.deepEqual(next?.reveal, { items: [1], at: REVEAL_AT })
})

test('형식이 틀린 알림은 통째로 버리고 나머지 값은 그대로 읽는다', () => {
  const cases: unknown[] = [
    'reveal',
    [1],
    { items: [], at: REVEAL_AT },
    { items: 1, at: REVEAL_AT },
    { items: ['1'], at: REVEAL_AT },
    { items: [1, 0], at: REVEAL_AT },
    { items: [1.5], at: REVEAL_AT },
    { items: [1e16], at: REVEAL_AT },
    { items: Array.from({ length: 21 }, (_, index) => index + 1), at: REVEAL_AT },
    { items: [1] },
    { items: [1], at: String(REVEAL_AT) },
    { items: [1], at: 0 },
    { items: [1], at: -1 },
    { items: [1], at: Number.NaN },
    { items: [1], at: Number.POSITIVE_INFINITY },
  ]
  for (const reveal of cases) {
    const next = parseNext(nextJson({ reveal }))
    assert.notEqual(next, null, JSON.stringify(reveal))
    assert.equal(next?.reveal, null, JSON.stringify(reveal))
  }
  assert.equal(parseNext(nextJson({ reveal: { items: Array.from({ length: 20 }, (_, index) => index + 1), at: REVEAL_AT } }))?.reveal?.items.length, 20)
})

test('PR 올리기는 세션 카드와 목록에 남고 결과는 문구와 주소 한 줄이다', () => {
  const actions = [
    { id: 'move', label: '이동', argv: ['move', '1', '--json'] },
    { id: 'pr', label: 'PR 올리기', argv: ['pr', '1', '--json'] },
    { id: 'evil', label: '나쁜', argv: ['uninstall'] },
  ]
  const ready = parseNext(nextJson({ ready: [{ ...readyCard(1), actions }] }))?.ready[0]
  assert.deepEqual(ready?.actions.map(action => action.id), ['move', 'pr'])
  assert.equal(resultLine({ pr: { message: 'draft PR을 만들었습니다', url: 'https://github.com/acme/web/pull/3' } }), 'draft PR을 만들었습니다: https://github.com/acme/web/pull/3')
  assert.equal(resultLine({ pr: { message: '열린 PR에 push했습니다', url: 'javascript:alert(1)', uncommitted: true } }), '열린 PR에 push했습니다. 커밋하지 않은 변경은 들지 않았습니다')
})
