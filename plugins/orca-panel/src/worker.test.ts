import { assert, mockHost, mountWorker, test } from 'orca/testing'
import { REVEAL_DEBOUNCE_MS, STATUS_ITEM_ID } from './config.ts'
import { KAIROS_BIN, KAIROS_HOME, ORCA_BIN } from './paths.ts'
import { accepted, card, counts, DANGEROUS_TITLE, item, listJson, nextJson, recordingGate, startable } from './fixtures.ts'
import type { ExecOutcome, Exec } from './runner.ts'
import { startWorker, type WatchDir, type WorkerHandle } from './service.ts'
import * as main from './worker.ts'

type Json = Record<string, unknown>

const ok = (value: Json): ExecOutcome => ({ exitCode: 0, stdout: JSON.stringify(value), fault: null })
const done: ExecOutcome = ok({ schema: 1, ok: true, results: [{ id: 1, status: 'changed', state: 'parked', message: '대기로 옮김' }] })

interface Rig {
  calls: string[][]
  answer: (argv: readonly string[]) => ExecOutcome | Promise<ExecOutcome>
  exec: Exec
}

function rig(answer: Rig['answer'] = argv => (argv[0] === 'list' ? ok(listJson()) : ok(nextJson()))): Rig {
  const state: Rig = {
    calls: [],
    answer,
    exec: async (file, args) => {
      assert.equal(file, KAIROS_BIN)
      const argv = [...args]
      state.calls.push(argv)
      return state.answer(argv)
    },
  }
  return state
}

let handle: WorkerHandle

async function mountRig(state: Rig) {
  const host = mockHost({ capabilities: [] })
  const worker = await mountWorker(
    {
      default: (orca: Parameters<typeof startWorker>[0]) => {
        handle = startWorker(orca, { exec: state.exec, pollMs: 0 })
        return handle.ready
      },
    },
    { host, commands: ['kairos.open'] },
  )
  return worker
}

const updates = (worker: Awaited<ReturnType<typeof mountRig>>): Json[] =>
  worker.host.callsOf('statusBar.update') as Json[]

test('시작할 때 next --json을 불러 상태 표시줄을 그린다', async () => {
  const state = rig(() => ok(nextJson({ counts: counts({ P0: 1, P1: 1 }) })))
  const worker = await mountRig(state)
  assert.deepEqual(state.calls, [['next', '--json']])
  assert.deepEqual(updates(worker).at(-1), {
    id: STATUS_ITEM_ID,
    text: 'kairos 2',
    icon: 'bell',
    tone: 'danger',
    tooltip: '긴급 1, 오늘 1, 여유 있을 때 0 / 할 일 0, 미룬 일 0, 세션 0 / 지금 먼저 볼 일은 없습니다.',
  })
  assert.deepEqual(worker.refusals, [])
})

test('다시 시작하면 다시 그린다', async () => {
  const state = rig()
  await mountRig(state)
  const worker = await mountRig(state)
  assert.equal(state.calls.length, 2)
  assert.equal(updates(worker).length, 1)
})

test('게이트가 닫혀 있으면 숨기지 않고 사유를 그린다', async () => {
  const worker = await mountRig(rig(() => ok(nextJson({ display: recordingGate, counts: counts({ P0: 2 }) }))))
  assert.equal(updates(worker).at(-1)?.text, 'kairos 기록 중')
})

test('kairos 실패를 상태 표시줄에 그리고 로그에 코드만 남긴다', async () => {
  const cases: [ExecOutcome, string][] = [
    [{ exitCode: 1, stdout: '', fault: 'spawn' }, 'spawn_failed'],
    [{ exitCode: 1, stdout: '', fault: 'timeout' }, 'timeout'],
    [{ exitCode: 2, stdout: '알 수 없는 명령', fault: null }, 'bad_output'],
    [ok({ schema: 9, ok: true }), 'schema'],
    [ok({ schema: 1, ok: false, error: { code: 'failed', message: 'SQLITE_BUSY' } }), 'failed'],
    [ok({ schema: 1, ok: true, display: null }), 'schema'],
  ]
  for (const [outcome, code] of cases) {
    const worker = await mountRig(rig(() => outcome))
    const update = updates(worker).at(-1)
    assert.equal(update?.text, 'kairos 오류', code)
    assert.equal(update?.tone, 'danger', code)
    assert.match(String(update?.tooltip), new RegExp(`코드 ${code}`))
    assert.deepEqual(worker.logs, [`kairos next 실패: ${code}`])
  }
})

test('실패 뒤 다음 폴링이 성공하면 건수로 돌아온다', async () => {
  let down = true
  const state = rig(() => (down ? { exitCode: 1, stdout: '', fault: 'timeout' } : ok(nextJson({ counts: counts({ P1: 1 }) }))))
  const worker = await mountRig(state)
  down = false
  await handle.poll()
  assert.equal(updates(worker).at(-1)?.text, 'kairos 1')
})

test('폴링이 끝나면 패널에 알리고 제목은 싣지 않는다', async () => {
  const worker = await mountRig(rig(() => ok(nextJson({ card: card([item(1, { title: DANGEROUS_TITLE })]) }))))
  assert.equal(worker.posts.length, 1)
  assert.equal(worker.posts[0]?.event, 'kairos.tick')
  assert.ok(!JSON.stringify(worker.posts).includes('touch'))
})

test('활성 run이 없으면 kairos runs를 부르지 않는다. 수락 항목이 있어도 끝난 세션이면 같다', async () => {
  const state = rig(() => ok(nextJson({ counts: counts({}, { accepted: 1 }) })))
  await mountRig(state)
  await handle.poll()
  assert.ok(!state.calls.some(argv => argv[0] === 'runs'))
})

test('항목이 먼저 닫혀 수락 항목이 0건이어도 활성 run이 있으면 추적을 이어간다', async () => {
  const state = rig(argv => (argv[0] === 'runs' ? ok({ schema: 1, ok: true, orca: true, runs: [] }) : ok(nextJson({ counts: counts(), runs: { active: 1 } }))))
  await mountRig(state)
  await handle.poll()
  assert.ok(state.calls.some(argv => argv[0] === 'runs'))
})

test('활성 run이 있으면 폴링마다 kairos runs로 맞춘 뒤 next를 읽는다', async () => {
  const state = rig(argv => (argv[0] === 'runs' ? ok({ schema: 1, ok: true, orca: true, runs: [] }) : ok(nextJson({ runs: { active: 1 } }))))
  await mountRig(state)
  state.calls.length = 0
  await handle.poll()
  assert.deepEqual(state.calls, [['runs', '--json'], ['next', '--json']])
})

test('세션 상태를 못 읽으면 숨기지 않고 경고로 그린다', async () => {
  for (const reply of [{ exitCode: 1, stdout: '', fault: 'timeout' } as ExecOutcome, ok({ schema: 1, ok: true, orca: false, runs: [] })]) {
    const state = rig(argv => (argv[0] === 'runs' ? reply : ok(nextJson({ runs: { active: 1 } }))))
    const worker = await mountRig(state)
    await handle.poll()
    assert.equal(updates(worker).at(-1)?.text, 'kairos 0 !')
    assert.match(String(updates(worker).at(-1)?.tooltip), /세션 상태를 읽지 못했습니다/)
  }
})

test('앞선 폴링이 끝나기 전의 폴링은 건너뛴다', { timeout: 5_000 }, async () => {
  const state = rig()
  const worker = await mountRig(state)
  state.calls.length = 0
  let release: (value: ExecOutcome) => void = () => undefined
  state.answer = () => new Promise(resolve => (release = resolve))
  const first = handle.poll()
  const second = handle.poll()
  await second
  await new Promise(resolve => setImmediate(resolve))
  assert.equal(state.calls.length, 1)
  release(ok(nextJson()))
  await first
  assert.ok(worker)
})

test('주기 호출은 stop 뒤에 멈춘다', async () => {
  const state = rig()
  let reached: () => void = () => undefined
  const twice = new Promise<void>(resolve => (reached = resolve))
  state.answer = () => {
    if (state.calls.length >= 2) reached()
    return ok(nextJson())
  }
  await mountWorker(
    {
      default: (orca: Parameters<typeof startWorker>[0]) => {
        handle = startWorker(orca, { exec: state.exec, pollMs: 5 })
      },
    },
    { host: mockHost({ capabilities: ['notifications:show'] }), commands: ['kairos.open'] },
  )
  await twice
  handle.stop()
  await new Promise(resolve => setTimeout(resolve, 10))
  const seen = state.calls.length
  await new Promise(resolve => setTimeout(resolve, 30))
  assert.equal(state.calls.length, seen)
})

test('패널이 보일 때만 --seen을 주고 목록을 같이 읽는다', async () => {
  const state = rig()
  const worker = await mountRig(state)
  state.calls.length = 0
  await worker.request('kairos.snapshot', { seen: true })
  assert.deepEqual(state.calls, [['next', '--seen', '--surface', 'panel', '--json'], ['list', '--json']])
  state.calls.length = 0
  await worker.request('kairos.snapshot', { seen: false })
  assert.deepEqual(state.calls, [['next', '--json'], ['list', '--json']])
})

test('seen은 true일 때만 인정한다', async () => {
  const state = rig()
  const worker = await mountRig(state)
  state.calls.length = 0
  for (const input of [{ seen: 'true' }, { seen: 1 }, {}, null, 'seen']) await worker.request('kairos.snapshot', input as never)
  assert.ok(state.calls.every(argv => !argv.includes('--seen')))
})

test('목록이 실패해도 카드는 넘기고 실패를 함께 알린다', async () => {
  const state = rig(argv => (argv[0] === 'list' ? { exitCode: 1, stdout: '', fault: 'timeout' } : ok(nextJson({ card: card([item(1)]) }))))
  const worker = await mountRig(state)
  const snapshot = await worker.request('kairos.snapshot', { seen: true })
  assert.equal(snapshot.next?.card?.items[0]?.id, 1)
  assert.equal(snapshot.list, null)
  assert.equal(snapshot.error?.code, 'timeout')
})

test('버튼은 계약의 argv로 kairos를 부르고 조작 뒤 다시 읽는다', async () => {
  const state = rig(argv => (argv[0] === 'next' ? ok(nextJson()) : argv[0] === 'list' ? ok(listJson()) : done))
  const worker = await mountRig(state)
  const cases: [string, number[], string[]][] = [
    ['later', [3, 4], ['later', '3', '4', '--json']],
    ['done', [3], ['done', '3', '--json']],
    ['keep', [3], ['keep', '3', '--json']],
    ['keep_deploy', [3], ['keep', '3', '--until-deploy', '--json']],
    ['start', [3], ['accept', '3', '--json']],
    ['move', [3], ['move', '3', '--json']],
    ['pr', [3], ['pr', '3', '--json']],
  ]
  for (const [action, ids, argv] of cases) {
    state.calls.length = 0
    const outcome = await worker.request('kairos.action', { action, ids, seen: true } as never)
    assert.deepEqual(state.calls, [argv, ['next', '--seen', '--surface', 'panel', '--json'], ['list', '--json']], action)
    assert.equal(outcome.ok, true)
    assert.equal(outcome.message, '대기로 옮김')
    assert.ok(outcome.snapshot?.next)
  }
})

test('확인은 next --ack로 부른다', async () => {
  const state = rig()
  const worker = await mountRig(state)
  state.calls.length = 0
  await worker.request('kairos.action', { action: 'ack', ids: [], seen: false })
  assert.deepEqual(state.calls[0], ['next', '--ack', '--json'])
})

test('kairos 실패는 코드와 고정 문구로 패널에 넘긴다', async () => {
  const notImplemented = ok({ schema: 1, ok: false, error: { code: 'not_implemented', message: '수락은 아직 구현되지 않았습니다' } })
  const state = rig(argv => (argv[0] === 'accept' ? notImplemented : argv[0] === 'list' ? ok(listJson()) : ok(nextJson())))
  const worker = await mountRig(state)
  const outcome = await worker.request('kairos.action', { action: 'start', ids: [3], seen: true })
  assert.equal(outcome.ok, false)
  assert.equal(outcome.code, 'not_implemented')
  assert.equal(outcome.message, '수락은 아직 구현되지 않았습니다')
  assert.ok(outcome.snapshot?.next)
  assert.deepEqual(worker.logs, ['kairos accept 실패: not_implemented'])
})

test('수락 경고를 결과에 붙인다', async () => {
  const reply = ok({ schema: 1, ok: true, results: [{ id: 3, status: 'changed', state: 'accepted', message: '세션을 여는 중입니다' }], run: null, url: null, warning: '일하는 세션이 5개입니다' })
  const state = rig(argv => (argv[0] === 'accept' ? reply : argv[0] === 'list' ? ok(listJson()) : ok(nextJson())))
  const worker = await mountRig(state)
  const outcome = await worker.request('kairos.action', { action: 'start', ids: [3], seen: true })
  assert.equal(outcome.message, '세션을 여는 중입니다. 일하는 세션이 5개입니다')
})

test('수락 응답의 run을 넘긴다', async () => {
  const run = { id: '00000000-0000-4000-8000-000000000001', state: 'claimed' }
  const state = rig(argv =>
    argv[0] === 'accept' ? ok({ schema: 1, ok: true, results: [{ id: 3, status: 'changed', state: 'accepted', message: '수락함' }], run }) : argv[0] === 'list' ? ok(listJson([accepted(3)])) : ok(nextJson()),
  )
  const worker = await mountRig(state)
  const outcome = await worker.request('kairos.action', { action: 'start', ids: [3], seen: true })
  assert.deepEqual(outcome.run, run)
  assert.deepEqual(outcome.snapshot?.list?.items[0]?.actions.map(action => action.id), ['done', 'move'])
})

test('ID를 검사해 맞지 않으면 kairos를 부르지 않는다', async () => {
  const state = rig()
  const worker = await mountRig(state)
  state.calls.length = 0
  const inputs: unknown[] = [
    { action: 'later', ids: ['1; rm -rf /'] },
    { action: 'later', ids: [DANGEROUS_TITLE] },
    { action: 'later', ids: ['--drop'] },
    { action: 'collect', ids: [] },
    { action: 'install', ids: [1] },
    { action: 'later' },
    { action: 'later', ids: '1' },
    null,
    'later',
  ]
  for (const input of inputs) {
    const outcome = await worker.request('kairos.action', input as never)
    assert.equal(outcome.ok, false)
    assert.equal(outcome.code, 'bad_request')
    assert.equal(outcome.snapshot, null)
  }
  assert.deepEqual(state.calls, [])
})

test('처리 중인 조작이 있으면 두 번째는 거절한다', { timeout: 5_000 }, async () => {
  const state = rig(argv => (argv[0] === 'list' ? ok(listJson()) : ok(nextJson())))
  const worker = await mountRig(state)
  state.calls.length = 0
  let release: (value: ExecOutcome) => void = () => undefined
  state.answer = argv => (argv[0] === 'accept' ? new Promise(resolve => (release = resolve)) : argv[0] === 'list' ? ok(listJson()) : ok(nextJson()))
  const first = worker.request('kairos.action', { action: 'start', ids: [3], seen: true })
  await new Promise(resolve => setImmediate(resolve))
  const second = await worker.request('kairos.action', { action: 'start', ids: [3], seen: true })
  assert.equal(second.ok, false)
  assert.equal(second.code, 'busy')
  release(done)
  assert.equal((await first).ok, true)
  assert.equal(state.calls.filter(argv => argv[0] === 'accept').length, 1)
})

test('수락 뒤 조작은 다시 받는다', async () => {
  const state = rig()
  const worker = await mountRig(state)
  state.answer = argv => (argv[0] === 'later' ? { exitCode: 1, stdout: '', fault: 'timeout' } : argv[0] === 'list' ? ok(listJson()) : ok(nextJson()))
  assert.equal((await worker.request('kairos.action', { action: 'later', ids: [1], seen: true })).code, 'timeout')
  assert.equal((await worker.request('kairos.action', { action: 'ack', ids: [], seen: true })).code, null)
})

test('외부 문자열은 kairos 인자에 들어가지 않는다', async () => {
  const danger = startable(1)
  danger.title = DANGEROUS_TITLE
  danger.url = 'https://example.test/$(id)'
  const state = rig(argv => (argv[0] === 'list' ? ok(listJson([danger])) : argv[0] === 'next' ? ok(nextJson({ card: card([danger]) })) : done))
  const worker = await mountRig(state)
  const snapshot = await worker.request('kairos.snapshot', { seen: true })
  const row = snapshot.next?.card?.items[0]
  await worker.request('kairos.action', { action: 'start', ids: [row?.id ?? 0], seen: true })
  await worker.request('kairos.action', { action: 'later', ids: [row?.id ?? 0], seen: true })
  for (const argv of state.calls) for (const arg of argv) assert.match(arg, /^(?:[a-z]+|--[a-z-]+|\d+)$/)
})

test('알림과 작업 공간 이동을 부르지 않는다', async () => {
  const state = rig()
  const worker = await mountRig(state)
  await worker.request('kairos.action', { action: 'move', ids: [1], seen: true })
  assert.deepEqual([...new Set(worker.host.calls.map(call => call.method))], ['statusBar.update'])
})

test('명령은 고정 인자로 패널을 연다', async () => {
  const opened: { file: string; args: readonly string[] }[] = []
  const exec: Exec = async (file, args) => {
    if (file === ORCA_BIN) {
      opened.push({ file, args })
      return { exitCode: 0, stdout: '{}', fault: null }
    }
    return ok(nextJson())
  }
  const worker = await mountRig({ calls: [], answer: () => ok(nextJson()), exec })
  assert.equal(await worker.command('kairos.open'), 'kairos 패널을 열었습니다')
  assert.deepEqual(opened, [{ file: ORCA_BIN, args: ['plugin', 'open', 'local.kairos', '--panel', 'kairos', '--json'] }])
})

test('패널을 못 열면 사이드바 경로를 알려 준다', async () => {
  const exec: Exec = async file => (file === ORCA_BIN ? { exitCode: 1, stdout: '', fault: 'spawn' } : ok(nextJson()))
  const worker = await mountRig({ calls: [], answer: () => ok(nextJson()), exec })
  assert.match(String(await worker.command('kairos.open')), /오른쪽 사이드바/)
})

test('점검 실행의 진입점은 kairos를 부르지 않고 그 사실을 그린다', { timeout: 5_000 }, async () => {
  const before = process.env.ORCA_PLUGIN_CHECK_PANELS
  process.env.ORCA_PLUGIN_CHECK_PANELS = '{}'
  const worker = await mountWorker(main, { host: mockHost({ capabilities: ['notifications:show'] }), commands: ['kairos.open'] })
  try {
    const snapshot = await worker.request('kairos.snapshot', { seen: true })
    assert.equal(snapshot.error?.code, 'check_mode')
  } finally {
    await worker.deactivate()
    if (before === undefined) delete process.env.ORCA_PLUGIN_CHECK_PANELS
    else process.env.ORCA_PLUGIN_CHECK_PANELS = before
  }
})

test('패널에서 쓴 할 일만 add의 인자가 되고 다른 조작은 제목을 버린다', async () => {
  const state = rig()
  const worker = await mountRig(state)
  state.calls.length = 0
  const added = await worker.request('kairos.action', { action: 'add', ids: [], seen: false, title: '-x 합성' })
  assert.equal(added.ok, true)
  assert.deepEqual(state.calls[0], ['add', '--json', '--', '-x 합성'])
  state.calls.length = 0
  await worker.request('kairos.action', { action: 'later', ids: [1], seen: false, title: '합성' })
  assert.ok(!JSON.stringify(state.calls).includes('합성'))
})

test('제목 없는 add와 ID가 붙은 포커스는 kairos를 부르지 않는다', async () => {
  const state = rig()
  const worker = await mountRig(state)
  state.calls.length = 0
  assert.equal((await worker.request('kairos.action', { action: 'add', ids: [], seen: false, title: '   ' })).code, 'bad_request')
  assert.equal((await worker.request('kairos.action', { action: 'focus_50', ids: [1], seen: false })).code, 'bad_request')
  assert.deepEqual(state.calls, [])
})

test('results가 없는 포커스 응답도 성공으로 받고 다음 화면을 준다', async () => {
  const state = rig(argv => {
    if (argv[0] === 'focus') return ok({ schema: 1, ok: true, at: '2026-11-04T02:00:00.000Z', focus: { active: true, until: '2026-11-04T02:50:00.000Z', heldP1: 0, heldLine: null } })
    return argv[0] === 'list' ? ok(listJson()) : ok(nextJson())
  })
  const worker = await mountRig(state)
  const outcome = await worker.request('kairos.action', { action: 'focus_50', ids: [], seen: true })
  assert.equal(outcome.ok, true)
  assert.equal(outcome.status, null)
  assert.notEqual(outcome.snapshot?.next, null)
})

/** 가짜 폴더 감시. fire가 Orca 밖의 파일 변화를 흉내 낸다. */
interface FakeWatch {
  dirs: string[]
  closed: number
  /** 있으면 감시를 걸 때 이 값을 던진다. */
  refuse: unknown
  fire: (filename: string | null) => void
  fail: (error: unknown) => void
  watch: WatchDir
}

function fakeWatch(): FakeWatch {
  const state: FakeWatch = {
    dirs: [],
    closed: 0,
    refuse: null,
    fire: () => undefined,
    fail: () => undefined,
    watch: (dir, onChange, onError) => {
      if (state.refuse !== null) throw state.refuse
      state.dirs.push(dir)
      state.fire = onChange
      state.fail = onError
      return { close: () => (state.closed += 1) }
    },
  }
  return state
}

async function mountWatched(watch: FakeWatch) {
  const state = rig()
  return mountWorker(
    {
      default: (orca: Parameters<typeof startWorker>[0]) => {
        handle = startWorker(orca, { exec: state.exec, pollMs: 0, watch: watch.watch })
        return handle.ready
      },
    },
    { host: mockHost({ capabilities: [] }), commands: ['kairos.open'] },
  )
}

const settleWatch = (): Promise<void> => new Promise(resolve => setTimeout(resolve, REVEAL_DEBOUNCE_MS + 60))
const reveals = (worker: Awaited<ReturnType<typeof mountWatched>>) => worker.posts.filter(post => post.event === 'kairos.reveal')

test('reveal.json이 바뀌면 잠잠해진 뒤 시각만 담아 한 번 알린다', async () => {
  const watch = fakeWatch()
  const worker = await mountWatched(watch)
  assert.deepEqual(watch.dirs, [KAIROS_HOME])
  watch.fire('reveal.json')
  watch.fire('reveal.json')
  watch.fire('reveal.json')
  assert.equal(reveals(worker).length, 0)
  await settleWatch()
  const posted = reveals(worker)
  assert.equal(posted.length, 1)
  assert.deepEqual(Object.keys(posted[0]?.payload as Json), ['at'])
  assert.equal(typeof (posted[0]?.payload as Json).at, 'number')
  handle.stop()
})

test('다른 파일과 이름 없는 변화는 알리지 않는다', async () => {
  const watch = fakeWatch()
  const worker = await mountWatched(watch)
  for (const name of ['kairos.db', 'kairos.db-wal', '.reveal.json.tmp', 'reveal.json.bak', 'logs', null]) watch.fire(name)
  await settleWatch()
  assert.equal(reveals(worker).length, 0)
  handle.stop()
})

test('홈이 없으면 코드만 남기고 다음 폴링에서 다시 건다', async () => {
  const watch = fakeWatch()
  watch.refuse = Object.assign(new Error(`${KAIROS_HOME} 없음`), { code: 'ENOENT' })
  const worker = await mountWatched(watch)
  await handle.poll()
  assert.deepEqual(worker.logs, ['reveal 감시 실패: ENOENT'])
  assert.deepEqual(watch.dirs, [])
  watch.refuse = null
  await handle.poll()
  assert.deepEqual(watch.dirs, [KAIROS_HOME])
  await handle.poll()
  assert.deepEqual(watch.dirs, [KAIROS_HOME])
  watch.fire('reveal.json')
  await settleWatch()
  assert.equal(reveals(worker).length, 1)
  handle.stop()
})

test('감시가 끊기면 닫고 코드만 남긴 뒤 다시 건다', async () => {
  const watch = fakeWatch()
  const worker = await mountWatched(watch)
  watch.fail(Object.assign(new Error(KAIROS_HOME), { code: 'EPERM' }))
  assert.equal(watch.closed, 1)
  assert.deepEqual(worker.logs, ['reveal 감시 실패: EPERM'])
  assert.ok(!worker.logs.some(line => line.includes(KAIROS_HOME)))
  await handle.poll()
  assert.equal(watch.dirs.length, 2)
  handle.stop()
})

test('멈추면 감시를 닫고 기다리던 알림을 보내지 않는다', async () => {
  const watch = fakeWatch()
  const worker = await mountWatched(watch)
  watch.fire('reveal.json')
  handle.stop()
  assert.equal(watch.closed, 1)
  await settleWatch()
  assert.equal(reveals(worker).length, 0)
  watch.fire('reveal.json')
  await settleWatch()
  assert.equal(reveals(worker).length, 0)
})

test('감시 없이 띄우면 reveal.json을 보지 않는다', async () => {
  const worker = await mountRig(rig())
  await handle.poll()
  assert.equal(reveals(worker).length, 0)
  assert.deepEqual(worker.logs, [])
})
