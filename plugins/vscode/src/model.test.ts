import assert from 'node:assert/strict'
import { test } from 'node:test'
import { contextOf, countsOf, itemsOf, revealOf, startsCollapsed, type ItemNode } from './model.ts'

const item = (fields: Record<string, unknown>) => ({
  id: 1,
  tier: 'P1',
  title: '합성 제목',
  sourceLabel: 'GitHub',
  reasonLabel: '리뷰 요청',
  url: 'https://github.com/acme/web/pull/1',
  actions: [{ id: 'later', argv: ['rm', '-rf'] }, { id: 'done' }, { id: 'triage_keep' }],
  ...fields,
})

test('항목을 묶음과 버튼으로 줄인다', () => {
  const [node] = itemsOf({ items: [item({})] })
  assert.equal(node.group, 'today')
  assert.equal(node.label, '합성 제목')
  assert.equal(node.description, '리뷰 요청 · GitHub')
  assert.equal(node.icon, 'bell')
  assert.equal(node.tooltip, '합성 제목\n리뷰 요청 · GitHub\n#1')
  assert.deepEqual(node.actions, ['later', 'done'])
  assert.equal(contextOf(node), 'item later done url')
})

test('세션이 있으면 세션 묶음이고 이동을 둔다', () => {
  const [node] = itemsOf({ items: [item({ run: { id: 'r', state: 'started' }, actions: [] })] })
  assert.equal(node.group, 'session')
  assert.deepEqual(node.actions, ['move'])
  assert.equal(node.icon, 'terminal')
  assert.equal(node.description, '세션이 진행 중입니다 · GitHub')
})

test('모양이 틀린 항목과 허용 밖 URL은 버린다', () => {
  const nodes = itemsOf({ items: [item({ id: '1' }), item({ id: 2, url: 'https://evil.example/x', title: 'a b' })] })
  assert.equal(nodes.length, 1)
  assert.equal(nodes[0].url, null)
  assert.equal(nodes[0].label, 'a b')
})

test('reveal은 ID와 시각만 받는다', () => {
  assert.deepEqual(revealOf({ reveal: { at: 5, items: [1, 'x', 0, 3] } }), { at: 5, items: [1, 3] })
  assert.equal(revealOf({}), null)
})

test('시작은 맡길 범위가 있을 때만 두고 제안 묶음에 모은다', () => {
  const start = [{ id: 'start' }, { id: 'later' }]
  const [offer, bare] = itemsOf({
    items: [item({ id: 1, actions: start, playbook: 'pr_review', scope: '합성 범위', reason: 'review_owed' }), item({ id: 2, actions: start })],
  })
  assert.equal(offer.group, 'offer')
  assert.deepEqual(offer.actions, ['start', 'later'])
  assert.equal(offer.icon, 'eye')
  assert.match(offer.tooltip, /맡길 범위: 합성 범위/)
  assert.equal(bare.group, 'today')
  assert.deepEqual(bare.actions, ['later'])
})

test('긴급은 제안보다 앞서고 미룬 일과 이전부터 있던 일은 따로 묶는다', () => {
  const nodes = itemsOf({
    items: [
      item({ id: 1, tier: 'P0', playbook: 'x', scope: 'y' }),
      item({ id: 2, state: 'parked' }),
      item({ id: 3, baseline: true }),
      item({ id: 4, tier: 'P2', reason: 'constructor' }),
    ],
  })
  assert.deepEqual(nodes.map(node => node.group), ['urgent', 'parked', 'baseline', 'later'])
  assert.equal(nodes[3].icon, 'bell')
})

test('조용한 묶음은 건수에서 빼고 기본으로 접는다', () => {
  const later = (id: number): ItemNode => ({ ...itemsOf({ items: [item({ id, tier: 'P2' })] })[0] })
  const nodes = [...itemsOf({ items: [item({ id: 1, tier: 'P0' }), item({ id: 2 }), item({ id: 3, state: 'parked' })] }), ...[4, 5, 6, 7, 8, 9].map(later)]
  assert.deepEqual(countsOf(nodes), { pending: 2, urgent: 1 })
  assert.equal(startsCollapsed('parked', nodes), true)
  assert.equal(startsCollapsed('later', nodes), true)
  assert.equal(startsCollapsed('later', nodes.filter(node => node.group !== 'today')), false)
  assert.equal(startsCollapsed('today', nodes), false)
})

test('양방향 재정렬과 폭 없는 문자는 지우고 한글 결합 문자는 둔다', () => {
  const [node] = itemsOf({ items: [item({ title: '합성‮제목⁦', scope: '범위​끝‍' , playbook: 'x' })] })
  assert.equal(node.label, '합성 제목')
  assert.equal(node.scope, '범위 끝‍')
})

test('배포 때 다시는 argv의 표시로만 가르고 수락만 된 항목에는 이동을 두지 않는다', () => {
  const keep = (deploy: boolean) => ({ id: 'keep', argv: ['keep', '5', ...(deploy ? ['--until-deploy'] : []), '--json'] })
  const [deploy, plain, accepted] = itemsOf({
    items: [item({ id: 5, actions: [keep(true)] }), item({ id: 6, actions: [keep(false)] }), item({ id: 7, state: 'accepted', run: null, actions: [{ id: 'cancel' }] })],
  })
  assert.deepEqual(deploy.actions, ['keep_deploy'])
  assert.deepEqual(plain.actions, ['keep'])
  assert.equal(accepted.group, 'session')
  assert.deepEqual(accepted.actions, ['cancel'])
})
