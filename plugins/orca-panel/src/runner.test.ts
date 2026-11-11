import { assert, test } from 'orca/testing'
import { fileURLToPath } from 'node:url'
import { KAIROS_BIN } from './paths.ts'
import { argvFor, createKairos, execProgram, readReply, type ExecOutcome } from './runner.ts'

const FAKE = fileURLToPath(new URL('./fake-kairos.mjs', import.meta.url))
const fake = (mode: string, args: string[] = [], timeoutMs: number = 5_000): Promise<ExecOutcome> =>
  execProgram(process.execPath, [FAKE, mode, ...args], timeoutMs)

test('셸을 거치지 않아 인자가 글자 그대로 간다', async () => {
  const hostile = '$(touch /tmp/kairos-hostile); `id` | cat & ~'
  const outcome = await fake('echo', [hostile, 'a b'])
  assert.equal(outcome.fault, null)
  assert.deepEqual(JSON.parse(outcome.stdout).argv, [hostile, 'a b'])
})

test('시간을 넘기면 끊고 timeout으로 답한다', async () => {
  const started = Date.now()
  const outcome = await fake('hang', [], 200)
  assert.equal(outcome.fault, 'timeout')
  assert.ok(Date.now() - started < 3_000)
})

test('손자가 표준 출력을 물고 있어도 풀린다', async () => {
  const outcome = await fake('orphan', [], 200)
  assert.equal(outcome.fault, 'timeout')
})

test('없는 실행 파일은 spawn 실패다', async () => {
  const outcome = await execProgram('/nonexistent/kairos-node', [], 1_000)
  assert.equal(outcome.fault, 'spawn')
})

test('출력이 한도를 넘으면 too_large다', async () => {
  assert.equal((await fake('big')).fault, 'too_large')
})

test('JSON이 아닌 출력은 내용을 내지 않고 bad_output이다', async () => {
  const reply = readReply(await fake('garbage'))
  assert.equal(reply.ok, false)
  if (!reply.ok) {
    assert.equal(reply.failure.code, 'bad_output')
    assert.ok(!reply.failure.message.includes('알 수 없는 명령'))
  }
})

test('kairos의 ok:false는 code와 message를 넘긴다', async () => {
  const reply = readReply(await fake('fail'))
  assert.deepEqual(reply, { ok: false, failure: { code: 'not_found', message: '없는 항목입니다' } })
})

test('schema가 다르면 거절한다', () => {
  const reply = readReply({ exitCode: 0, stdout: '{"schema":2,"ok":true}', fault: null })
  assert.equal(!reply.ok && reply.failure.code, 'schema')
})

test('ok:true인데 종료 코드가 0이 아니면 믿지 않는다', () => {
  const reply = readReply({ exitCode: 1, stdout: '{"schema":1,"ok":true}', fault: null })
  assert.equal(!reply.ok && reply.failure.code, 'bad_output')
})

test('고정 경로의 node로 설치본 bin을 부른다', async () => {
  const seen: { file: string; args: readonly string[] }[] = []
  const kairos = createKairos(async (file, args) => {
    seen.push({ file, args })
    return { exitCode: 0, stdout: '{"schema":1,"ok":true}', fault: null }
  })
  await kairos(['next', '--json'], 1_000)
  assert.deepEqual(seen, [{ file: KAIROS_BIN, args: ['next', '--json'] }])
  assert.ok(KAIROS_BIN.startsWith('/'))
})

test('호출은 겹치지 않고 앞이 실패해도 뒤가 돈다', async () => {
  let running = 0
  let most = 0
  const kairos = createKairos(async () => {
    running += 1
    most = Math.max(most, running)
    await new Promise(resolve => setImmediate(resolve))
    running -= 1
    return { exitCode: 1, stdout: '', fault: 'spawn' }
  })
  const replies = await Promise.all([kairos(['a'], 1), kairos(['b'], 1), kairos(['c'], 1)])
  assert.equal(most, 1)
  assert.equal(replies.length, 3)
})

test('버튼 이름과 ID를 계약의 argv로 바꾼다', () => {
  assert.deepEqual(argvFor('start', [7]), ['accept', '7', '--json'])
  assert.deepEqual(argvFor('later', [7, 8]), ['later', '7', '8', '--json'])
  assert.deepEqual(argvFor('done', [7, 8]), ['done', '7', '8', '--json'])
  assert.deepEqual(argvFor('keep', [7]), ['keep', '7', '--json'])
  assert.deepEqual(argvFor('keep_deploy', [7]), ['keep', '7', '--until-deploy', '--json'])
  assert.deepEqual(argvFor('move', [7]), ['move', '7', '--json'])
  assert.deepEqual(argvFor('pr', [7]), ['pr', '7', '--json'])
  assert.deepEqual(argvFor('cancel', [7]), ['accept', '7', '--cancel', '--json'])
  assert.deepEqual(argvFor('ack', []), ['next', '--ack', '--json'])
})

test('ID가 아니면 kairos 인자를 만들지 않는다', () => {
  const bad: unknown[] = ['1; rm -rf /', '$(id)', '`id`', '--drop', '-1', -1, 0, '0', 1.5, '1.5', '01', '1 2', '1\n2', 1e21, Number.NaN, null, undefined, {}, [], true, '9'.repeat(16)]
  for (const value of bad) assert.equal(argvFor('later', [value]), null, String(value))
})

test('한 건짜리 버튼은 여러 ID를 받지 않는다', () => {
  for (const action of ['start', 'keep', 'keep_deploy', 'move', 'cancel', 'pr']) {
    assert.equal(argvFor(action, [1, 2]), null, action)
    assert.equal(argvFor(action, []), null, action)
  }
})

test('확인은 ID를 받지 않고 나머지는 ID가 있어야 한다', () => {
  assert.equal(argvFor('ack', [1]), null)
  assert.equal(argvFor('later', []), null)
  assert.equal(argvFor('done', []), null)
})

test('ID 개수 한도를 넘기면 거절한다', () => {
  assert.equal(argvFor('later', Array.from({ length: 101 }, (_, index) => index + 1)), null)
})

test('없는 버튼 이름은 거절한다', () => {
  for (const action of ['collect', 'install', 'triage', '__proto__', 'constructor', '', null, undefined, 1, ['later']]) {
    assert.equal(argvFor(action, [1]), null, String(action))
  }
})

test('패널 조작은 고정 인자만 만들고 항목 ID를 받지 않는다', () => {
  assert.deepEqual(argvFor('focus_25', []), ['focus', '25', '--json'])
  assert.deepEqual(argvFor('focus_50', []), ['focus', '50', '--json'])
  assert.deepEqual(argvFor('focus_90', []), ['focus', '90', '--json'])
  assert.deepEqual(argvFor('focus_end', []), ['focus', '--end', '--json'])
  assert.deepEqual(argvFor('display_auto', []), ['display', 'auto', '--json'])
  assert.equal(argvFor('focus_50', [1]), null)
  assert.equal(argvFor('display_auto', [1]), null)
  assert.equal(argvFor('focus_45', []), null)
})

test('할 일 제목은 -- 뒤에 두어 옵션으로 읽히지 않는다', () => {
  assert.deepEqual(argvFor('add', [], '합성 할 일'), ['add', '--json', '--', '합성 할 일'])
  assert.deepEqual(argvFor('add', [], '--help'), ['add', '--json', '--', '--help'])
  assert.deepEqual(argvFor('add', [], '$(touch /tmp/x); rm -rf ~'), ['add', '--json', '--', '$(touch /tmp/x); rm -rf ~'])
})

test('할 일 제목은 정리하고 자르며 비었거나 글자가 아니면 kairos를 부르지 않는다', () => {
  assert.deepEqual(argvFor('add', [], '  합성\n둘째 줄\u0007  '), ['add', '--json', '--', '합성 둘째 줄'])
  assert.equal([...(argvFor('add', [], '가'.repeat(500))?.[3] ?? '')].length, 200)
  assert.equal(argvFor('add', [], '   '), null)
  assert.equal(argvFor('add', []), null)
  assert.equal(argvFor('add', [], 7), null)
  assert.equal(argvFor('add', [1], '합성'), null)
})

test('항목 조작은 제목을 받아도 인자에 넣지 않는다', () => {
  assert.deepEqual(argvFor('later', [1], '합성'), ['later', '1', '--json'])
})
