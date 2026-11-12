import { assert, test } from 'orca/testing'
import { counts, failedCard, nextJson, readyCard, recordingGate } from './fixtures.ts'
import { parseNext } from './parse.ts'
import type { NextView } from './protocol.ts'
import { badgeOf, statusOfFailure, statusOfNext } from './status.ts'

const next = (overrides: Record<string, unknown> = {}): NextView => parseNext(nextJson(overrides)) as NextView

test('건수는 모든 등급의 합이고 긴급은 색과 글자로 함께 알린다', () => {
  assert.deepEqual(
    [{ P2: 1 }, { P1: 2, P2: 1 }, { P0: 1, P1: 2, P2: 1 }].map(open => {
      const view = statusOfNext(next({ counts: counts(open) }))
      return [view.text, view.tone]
    }),
    [
      ['kairos 1', 'neutral'],
      ['kairos 3', 'neutral'],
      ['kairos 4 · 긴급 1', 'danger'],
    ],
  )
})

test('제안이 없으면 0건을 표시한다', () => {
  const view = statusOfNext(next())
  assert.equal(view.text, 'kairos 0')
  assert.equal(view.tone, 'neutral')
})

test('직접 추가와 대기 건수는 건수에 넣지 않고 설명에 쓴다', () => {
  const view = statusOfNext(next({ counts: counts({ P1: 1 }, { manual: 3, parked: 2, accepted: 1 }) }))
  assert.equal(view.text, 'kairos 1')
  assert.match(view.tooltip, /긴급 0, 오늘 1, 여유 있을 때 0 \/ 할 일 3, 미룬 일 2, 세션 1/)
})

test('건강 경고는 숨기지 않는다', () => {
  const view = statusOfNext(next({ health: 'Slack 수집이 30분 늦습니다', counts: counts({ P2: 1 }) }))
  assert.equal(view.text, 'kairos 1 !')
  assert.equal(view.icon, 'warning')
  assert.equal(view.tone, 'warning')
  assert.match(view.tooltip, /^Slack 수집이 30분 늦습니다/)
})

test('긴급이 있으면 건강 경고나 세션 소식이 있어도 danger다', () => {
  assert.equal(statusOfNext(next({ health: '경고', counts: counts({ P0: 1 }) })).tone, 'danger')
  assert.equal(statusOfNext(next({ ready: [readyCard(1)], counts: counts({ P0: 1 }) })).tone, 'danger')
})

test('설명에 내부 등급 이름이 없고 보류 문장은 사용자 문구로 쓴다', () => {
  const view = statusOfNext(next({ counts: counts({ P0: 1, P1: 2, P2: 3 }), hold: { reason: 'lunch', message: '점심', until: '2026-11-04T04:00:00.000Z' } }))
  assert.doesNotMatch(view.tooltip, /P0|P1|P2|대기|수락/)
  assert.match(view.tooltip, /점심시간입니다\. 13:00에 다시 보여 드립니다\.$/)
})

test('게이트가 닫히면 건수 대신 사유와 시작 시각을 쓴다', () => {
  const closed = next({ display: recordingGate, counts: counts({ P0: 1, P1: 5 }) })
  const view = statusOfNext(closed)
  assert.equal(badgeOf(closed), 0)
  assert.equal(view.text, 'kairos 기록 중')
  assert.equal(view.icon, 'pause')
  assert.equal(view.tone, 'neutral')
  assert.match(view.tooltip, /기록 중입니다.*내일 20:00부터 제안을 보여 드립니다/)
})

test('게이트 사유 4종을 모두 글로 보여 준다', () => {
  const texts = ['paused', 'display_off', 'no_collection', 'recording', 'unknown_reason', 'constructor', 'toString'].map(
    reason => statusOfNext(next({ display: { ...recordingGate, reason } })).text,
  )
  assert.deepEqual(texts, ['kairos 일시정지', 'kairos 표시 꺼짐', 'kairos 수집 전', 'kairos 기록 중', 'kairos 표시 대기', 'kairos 표시 대기', 'kairos 표시 대기'])
})

test('게이트가 닫혀도 건강 경고는 보인다', () => {
  const view = statusOfNext(next({ display: recordingGate, health: '수집 지연' }))
  assert.equal(view.text, 'kairos 기록 중 !')
  assert.equal(view.tone, 'warning')
})

test('실패는 danger로 코드와 함께 표시한다', () => {
  assert.deepEqual(statusOfFailure({ code: 'timeout', message: 'kairos 응답 시간을 넘겼습니다' }), {
    text: 'kairos 오류',
    icon: 'error',
    tone: 'danger',
    tooltip: '응답이 늦습니다 / 코드 timeout',
  })
})

test('세션 소식은 제안 건수에 넣지 않고 글자와 설명, 색으로 알린다', () => {
  const view = statusOfNext(next({ ready: [readyCard(1)] }))
  assert.equal(view.text, 'kairos 0 · 세션 1')
  assert.equal(view.tone, 'accent')
  assert.match(view.tooltip, /^세션 소식 1건/)
})

test('건강 경고로 색이 같아도 세션 소식은 글자로 구분된다', () => {
  const quiet = statusOfNext(next({ counts: counts({ P1: 1 }), health: '합성 경고' }))
  const ready = statusOfNext(next({ counts: counts({ P1: 1 }), health: '합성 경고', ready: [readyCard(1)] }))
  assert.equal(quiet.tone, ready.tone)
  assert.equal(quiet.text, 'kairos 1 !')
  assert.equal(ready.text, 'kairos 1 · 세션 1 !')
})

test('게이트가 닫혀도 실패 소식은 글자와 설명에 나온다', () => {
  const view = statusOfNext(next({ display: recordingGate, ready: [failedCard(1)] }))
  assert.equal(view.text, 'kairos 기록 중 · 세션 1')
  assert.match(view.tooltip, /세션 소식 1건/)
})

test('tooltip은 256자를 넘지 않는다', () => {
  assert.ok([...statusOfNext(next({ health: '가'.repeat(200), hold: { reason: 'nothing', message: '나'.repeat(200), until: null } })).tooltip].length <= 256)
})

test('포커스 중이면 글자와 설명에 쓰고 게이트가 닫혀 있으면 쓰지 않는다', () => {
  const focus = { active: true, startedAt: null, until: '2026-11-04T05:00:00.000Z', heldP1: 0, heldLine: null }
  const view = statusOfNext(next({ focus, counts: counts({ P1: 2 }) }))
  assert.equal(view.text, 'kairos 2 · 포커스')
  assert.ok(view.tooltip.includes('포커스 중 · 14:00까지'))
  assert.equal(view.tone, 'neutral')
  const closed = statusOfNext(next({ focus, display: recordingGate }))
  assert.ok(!closed.text.includes('포커스'))
})
