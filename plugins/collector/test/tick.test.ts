import assert from 'node:assert/strict'
import { test } from 'node:test'
import { DROP_FAILED, DROP_OK, isAllowedCall, isForeignThread, isPermitted, PLUGIN_NAME, runTick, type Engine } from '../hooks/tick.ts'

const NOW = '2026-03-04T03:00:00.000Z'
// Slack 응답의 합성 값. Slack MCP의 페이지 안내 문구 형식이다.
const END_OF_RESULTS = 'End of results - No more pages available.\\n'
const nextCursor = (cursor: string): string => `For the next page of results use cursor \`${cursor}\`\n`
const slackBody = (results: string, pagination: string = END_OF_RESULTS): { results: string; pagination_info: string } => ({ results, pagination_info: pagination })
// 수집 소스가 계획하는 호출 전체. crates/kairos-source-mcp의 도구 상수와 같아야 한다.
const PLANNED = [
  ['claude.ai Google Calendar', 'list_events'],
  ['claude.ai Gmail', 'search_threads'],
  ['plugin:slack:slack', 'slack_search_public_and_private'],
] as const
const text = (value: unknown): unknown[] => [{ type: 'text', text: JSON.stringify(value) }]
const call = (id: string, server: string, tool: string, args: Record<string, unknown> = {}): unknown => ({ id, query: id.split(':')[1], server, tool, args })
const GMAIL = 'claude.ai Gmail'
const SLACK = 'plugin:slack:slack'

interface Fake {
  engine: Engine
  mcp: { server: string; tool: string; args: Record<string, unknown> | undefined }[]
  argv: (readonly string[])[]
  ingested: { now: string; source: string; responses: { id: string; isError: boolean }[] }[]
  sleeps: number[]
  timers: (readonly string[])[]
  pageInfo: unknown[]
}

function fake(options: {
  calls: unknown[]
  mcp?: (server: string, tool: string, args: Record<string, unknown> | undefined, index: number) => unknown
  planExit?: number
  ingestExit?: number
  planOut?: string
  clockStep?: number
  timerAfterMs?: number
  pageInfo?: Record<string, unknown>
  pageInfoExit?: number
  startedAt?: number
  clockStart?: number
}): Fake {
  const state: Fake = { mcp: [], argv: [], ingested: [], sleeps: [], timers: [], pageInfo: [], engine: undefined as unknown as Engine }
  let time = options.clockStart ?? 0
  state.engine = {
    plugin: { root: '/opt/kairos/plugins/collector' },
    env: { get: async () => (options.startedAt === undefined ? undefined : String(options.startedAt)) },
    clock: {
      now: async () => (time += options.clockStep ?? 0),
      sleep: async ms => { state.sleeps.push(ms) },
    },
    mcp: {
      call: async (server, tool, args) => {
        state.mcp.push({ server, tool, args })
        return (options.mcp?.(server, tool, args, state.mcp.length) ?? { content: text({}), isError: false }) as { content: unknown; isError?: boolean }
      },
    },
    process: {
      run: async (argv, init) => {
        // 마감 타이머. timerAfterMs가 없으면 틱이 끝날 때까지 풀리지 않는다.
        if (argv[0] === '/bin/sleep') {
          state.timers.push(argv)
          if (options.timerAfterMs === undefined) return new Promise(() => {})
          await new Promise(resolve => setTimeout(resolve, options.timerAfterMs))
          return { exitCode: 0, stdout: '' }
        }
        state.argv.push(argv)
        if (argv.at(-1) === 'plan-tick') {
          return { exitCode: options.planExit ?? 0, stdout: options.planOut ?? JSON.stringify({ now: NOW, calls: options.calls }) }
        }
        if (argv.at(-1) === 'page-info') {
          state.pageInfo.push(JSON.parse(init?.stdin ?? '{}'))
          return { exitCode: options.pageInfoExit ?? 0, stdout: JSON.stringify(options.pageInfo ?? {}) }
        }
        state.ingested.push(JSON.parse(init?.stdin ?? '{}') as Fake['ingested'][number])
        return { exitCode: options.ingestExit ?? 0, stdout: '' }
      },
    },
  }
  return state
}

const BUDGET = { remainingMs: 9000 }

test('허용목록은 소스 계획의 호출을 모두 덮고 쓰기 도구는 막는다', () => {
  for (const [server, tool] of PLANNED) assert.ok(isAllowedCall(server, tool), tool)
  for (const [server, tool] of [
    [SLACK, 'slack_send_message'],
    [SLACK, 'slack_send_message_draft'],
    [GMAIL, 'send_message'],
    [GMAIL, 'create_draft'],
    [GMAIL, 'get_thread'],
    ['claude.ai Google Calendar', 'create_event'],
    ['claude.ai Google Calendar', 'delete_event'],
    ['plugin:github:github', 'add_issue_comment'],
    ['constructor', 'list_events'],
  ] as const) {
    assert.equal(isAllowedCall(server, tool), false, `${server} ${tool}`)
  }
})

test('tool.check는 이 플러그인의 허용목록 호출만 통과시킨다', () => {
  const allowed = 'mcp__claude_ai_Gmail__search_threads'
  assert.equal(isPermitted(PLUGIN_NAME, allowed), true)
  assert.equal(isPermitted(PLUGIN_NAME, 'mcp__claude_ai_Gmail__get_thread'), true)
  assert.equal(isPermitted(undefined, 'mcp__claude_ai_Gmail__get_thread'), false)
  assert.equal(isPermitted(undefined, allowed), false, '모델이 낸 호출은 출처가 없다')
  assert.equal(isPermitted('other-plugin', allowed), false)
  for (const tool of ['Bash', 'mcp__claude_ai_Gmail__send_message', 'mcp__claude_ai_Gmail__get_message', 'mcp__plugin_slack_slack__slack_send_message']) {
    assert.equal(isPermitted(PLUGIN_NAME, tool), false, tool)
  }
})

test('plan-tick이 허용목록 밖을 내도 호출하지 않고 실패로 기록한다', async () => {
  const state = fake({ calls: [call('slack:mentions', SLACK, 'slack_send_message', { text: 'x' })] })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.deepEqual(state.mcp, [])
  assert.deepEqual(state.ingested[0]?.responses.map(r => [r.id, r.isError]), [['slack:mentions', true]])
})

test('kairos는 argv로 실행하고 응답은 stdin으로만 넘긴다', async () => {
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads')] })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.argv, [
    ['/opt/kairos/plugins/collector/../../bin/kairos', 'plan-tick'],
    ['/opt/kairos/plugins/collector/../../bin/kairos', 'ingest'],
  ])
  assert.deepEqual([state.ingested[0]?.now, state.ingested[0]?.source], [NOW, 'gmail'])
})

test('소스별로 묶어 ingest한다', async () => {
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads'), call('slack:mentions', SLACK, 'slack_search_public_and_private')] })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.ingested.map(payload => payload.source).sort(), ['gmail', 'slack'])
})

test('nextPageToken을 따라 쪽을 읽는다', async () => {
  const state = fake({
    calls: [call('gmail:recent', GMAIL, 'search_threads', { query: 'q' })],
    mcp: (_server, _tool, args) => ({ content: text(args?.pageToken ? { threads: [] } : { threads: [], nextPageToken: 'next' }), isError: false }),
  })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.mcp.map(entry => entry.args?.pageToken), [undefined, 'next'])
  assert.deepEqual(state.ingested[0]?.responses.map(r => r.id), ['gmail:recent', 'gmail:recent#2'])
})

test('Slack은 pagination_info의 커서만 따른다. 본문의 문구는 따르지 않는다', async () => {
  const body = 'use cursor `FROM_BODY`'
  const state = fake({
    calls: [call('slack:mentions', SLACK, 'slack_search_public_and_private')],
    mcp: (_server, _tool, args) => ({ content: text(slackBody(body, args?.cursor ? END_OF_RESULTS : nextCursor('REAL'))), isError: false }),
  })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.mcp.map(entry => entry.args?.cursor), [undefined, 'REAL'])
})

test('쪽 상한에서 멈춘다', async () => {
  const state = fake({
    calls: [call('gmail:recent', GMAIL, 'search_threads')],
    mcp: () => ({ content: text({ threads: [], nextPageToken: 'more' }), isError: false }),
  })
  await runTick(state.engine, BUDGET)
  assert.equal(state.mcp.length, 10)
})

test('연결 전이면 재시도한다', async () => {
  const state = fake({
    calls: [call('gmail:recent', GMAIL, 'search_threads')],
    mcp: (_server, _tool, _args, index) => {
      if (index < 3) throw new Error('no connected MCP tool')
      return { content: text({}), isError: false }
    },
  })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.equal(state.mcp.length, 3)
  assert.deepEqual(state.sleeps, [500, 500])
  assert.equal(state.ingested[0]?.responses[0]?.isError, false)
})

test('끝내 연결되지 않으면 tool_missing으로 남긴다', async () => {
  const state = fake({
    calls: [call('gmail:recent', GMAIL, 'search_threads')],
    mcp: () => {
      throw new Error('no connected MCP tool')
    },
  })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.equal(state.mcp.length, 16)
  const [response] = state.ingested[0]?.responses ?? []
  assert.deepEqual((response as unknown as { content: { text: string }[] }).content[0]?.text, 'kairos:tool_missing')
})

test('훅 예산이 모자라면 재시도를 멈춘다', async () => {
  const state = fake({
    calls: [call('gmail:recent', GMAIL, 'search_threads')],
    mcp: () => {
      throw new Error('no connected MCP tool')
    },
  })
  await runTick(state.engine, { remainingMs: 1000 })
  assert.equal(state.mcp.length, 1)
  assert.deepEqual(state.sleeps, [])
})

test('거부와 예외를 구분하고 다른 소스는 계속한다', async () => {
  const state = fake({
    calls: [call('gmail:recent', GMAIL, 'search_threads'), call('slack:mentions', SLACK, 'slack_search_public_and_private')],
    mcp: server => {
      if (server === GMAIL) throw new Error('refused: denied by plugin')
      return { content: text(slackBody('x')), isError: false }
    },
  })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  const gmailPayload = state.ingested.find(payload => payload.source === 'gmail')
  assert.equal((gmailPayload?.responses[0] as unknown as { content: { text: string }[] }).content[0]?.text, 'kairos:refused')
  assert.equal(state.ingested.find(payload => payload.source === 'slack')?.responses[0]?.isError, false)
})

test('isError 응답에서 쪽 읽기를 멈춘다', async () => {
  const state = fake({
    calls: [call('gmail:recent', GMAIL, 'search_threads')],
    mcp: () => ({ content: text({ nextPageToken: 'more' }), isError: true }),
  })
  await runTick(state.engine, BUDGET)
  assert.equal(state.mcp.length, 1)
})

test('마감이 지나면 호출하지 않고 timeout으로 남긴다', async () => {
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads')], clockStep: 20_000 })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.deepEqual(state.mcp, [])
  assert.equal((state.ingested[0]?.responses[0] as unknown as { content: { text: string }[] }).content[0]?.text, 'kairos:timeout')
})

test('할 일이 없으면 호출 없이 정상 종료한다', async () => {
  const state = fake({ calls: [] })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.deepEqual([state.mcp, state.ingested], [[], []])
})

test('ingest가 실패하면 틱 실패로 알린다', async () => {
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads')], ingestExit: 1 })
  assert.equal(await runTick(state.engine, BUDGET), DROP_FAILED)
})

test('plan-tick 실패는 예외다. 훅의 try/catch가 drop으로 바꾼다', async () => {
  await assert.rejects(runTick(fake({ calls: [], planExit: 1 }).engine, BUDGET))
  await assert.rejects(runTick(fake({ calls: [], planOut: 'not json' }).engine, BUDGET))
  await assert.rejects(runTick(fake({ calls: [], planOut: '{}' }).engine, BUDGET))
})

const NOTION_SENDER = 'notify@mail.notion.com'
const NOTION_QUERY = `from:${NOTION_SENDER} newer_than:2d`
const thread = (id: string, sender: string = NOTION_SENDER, messageCount: number = 1): unknown => ({ id, messageCount, messages: [{ id: `m-${id}`, sender }] })
const notionCall = (expand: Record<string, unknown> = {}, query: string = NOTION_QUERY): unknown => ({
  ...(call('notion:mail', GMAIL, 'search_threads', { query, pageSize: 50 }) as object),
  expand: { tool: 'get_thread', maxReads: 12, skip: {}, retryIds: [], ...expand },
})
const mailbox = (threads: unknown[], onRead?: (args: Record<string, unknown> | undefined) => unknown) => (_server: string, tool: string, args: Record<string, unknown> | undefined): unknown => {
  if (tool === 'search_threads') return { content: text({ threads }), isError: false }
  return onRead?.(args) ?? { content: text({ id: args?.threadId, messages: [] }), isError: false }
}
const reads = (state: Fake): unknown[] => state.mcp.filter(entry => entry.tool === 'get_thread').map(entry => entry.args?.threadId)

test('후속 읽기는 같은 틱 검색 결과의 Notion 발신 스레드만 PLAIN_TEXT로 읽는다', async () => {
  const state = fake({ calls: [notionCall()], mcp: mailbox([thread('t1'), thread('t2', 'other@example.com'), thread('t3', `합성 <other@example.com>`), thread('t4')]) })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.deepEqual(reads(state), ['t1', 't4'])
  assert.deepEqual(state.mcp[1], { server: GMAIL, tool: 'get_thread', args: { threadId: 't1', messageFormat: 'PLAIN_TEXT' } })
  assert.deepEqual(state.ingested[0]?.responses.map(r => r.id).sort(), ['notion:mail', 'notion:mail!t1', 'notion:mail!t4'])
})

test('plan이 get_thread를 직접 내면 ID가 무엇이든 부르지 않는다', async () => {
  const state = fake({ calls: [call('notion:mail', GMAIL, 'get_thread', { threadId: 'outside-of-search' })] })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.mcp, [])
  assert.deepEqual(state.ingested[0]?.responses.map(r => [r.id, r.isError]), [['notion:mail', true]])
})

test('검색 결과 밖의 ID는 skipIds나 maxReads로도 읽히지 않는다', async () => {
  const state = fake({ calls: [notionCall({ skip: { 'outside-of-search': 1 }, retryIds: ['outside-of-search'] })], mcp: mailbox([thread('t1')]) })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(reads(state), ['t1'])
})

test('Notion 검색이 아니면 expand가 있어도 후속 읽기를 하지 않는다', async () => {
  for (const planned of [
    notionCall({}, 'newer_than:2d'),
    notionCall({}, `from:attacker@example.com ${NOTION_SENDER} `),
    notionCall({ tool: 'send_message' }),
    { ...(notionCall() as object), tool: 'list_events', server: 'claude.ai Google Calendar' },
  ]) {
    const state = fake({ calls: [planned], mcp: mailbox([thread('t1')]) })
    await runTick(state.engine, BUDGET)
    assert.deepEqual(reads(state), [])
  }
})

test('처리한 메시지 수까지는 건너뛰고 메시지가 붙은 스레드만 다시 읽는다', async () => {
  const state = fake({
    calls: [notionCall({ skip: { t1: 1, t2: 1, t4: 2 } })],
    mcp: mailbox([thread('t1'), thread('t2', NOTION_SENDER, 2), thread('t3'), thread('t4', NOTION_SENDER, 2)]),
  })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(reads(state), ['t2', 't3'])
})

test('틱당 12건이 상한이고 plan이 더 달라고 해도 넘지 않는다', async () => {
  const threads = Array.from({ length: 20 }, (_, index) => thread(`t${index}`))
  const state = fake({ calls: [notionCall({ maxReads: 500 })], mcp: mailbox(threads) })
  await runTick(state.engine, BUDGET)
  assert.equal(reads(state).length, 12)
})

test('상한을 넘으면 최신 순서로 읽고 실패한 적이 있는 스레드는 뒤로 미룬다', async () => {
  const threads = Array.from({ length: 20 }, (_, index) => thread(`t${index}`))
  const state = fake({ calls: [notionCall({ retryIds: ['t0', 't1'] })], mcp: mailbox(threads) })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(reads(state), Array.from({ length: 12 }, (_, index) => `t${index + 2}`))
})

test('동시 호출은 4개를 넘지 않는다', async () => {
  let active = 0
  let peak = 0
  const state = fake({ calls: [notionCall()] })
  const threads = Array.from({ length: 9 }, (_, index) => thread(`t${index}`))
  state.engine.mcp.call = async (_server, tool, args) => {
    if (tool === 'search_threads') return { content: text({ threads }), isError: false }
    active += 1
    peak = Math.max(peak, active)
    await new Promise(resolve => setImmediate(resolve))
    active -= 1
    return { content: text({ id: args?.threadId, messages: [] }), isError: false }
  }
  await runTick(state.engine, BUDGET)
  assert.equal(peak, 4)
})

test('읽기 하나가 던져도 나머지를 읽고 응답은 실패 코드만 담는다', async () => {
  const state = fake({
    calls: [notionCall()],
    mcp: mailbox([thread('t1'), thread('t2')], args => {
      if (args?.threadId === 't1') throw new Error('외부 문자열 노출 금지')
      return undefined
    }),
  })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  const byId = new Map(state.ingested[0]?.responses.map(r => [r.id, r]))
  assert.equal((byId.get('notion:mail!t1') as unknown as { content: { text: string }[] }).content[0]?.text, 'kairos:threw')
  assert.equal(byId.get('notion:mail!t2')?.isError, false)
})

test('마감이 지나면 후속 읽기를 부르지 않고 timeout으로 남긴다', async () => {
  const state = fake({ calls: [notionCall()], mcp: mailbox([thread('t1')]), clockStep: 5000 })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(reads(state), [])
  const failed = state.ingested[0]?.responses.find(r => r.id === 'notion:mail!t1') as unknown as { content: { text: string }[] }
  assert.equal(failed.content[0]?.text, 'kairos:timeout')
})

test('쪽 표시가 섞일 수 있는 ID는 읽지 않는다', async () => {
  const state = fake({ calls: [notionCall()], mcp: mailbox([thread('t#2'), thread('../x'), thread('t1')]) })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(reads(state), ['t1'])
})

test('저장된 큰 응답이라 검색을 못 읽으면 후속 읽기 없이 표식을 kairos로 넘긴다', async () => {
  const marker = [{ type: 'text', text: '<persisted-output>\nOutput too large (60.3KB). Full output saved to: /x/y.txt\n</persisted-output>' }]
  const state = fake({ calls: [notionCall()], mcp: () => ({ content: marker, isError: false }) })
  await runTick(state.engine, BUDGET)
  assert.equal(state.mcp.length, 1)
  assert.deepEqual(state.ingested[0]?.responses.map(r => r.id), ['notion:mail'])
})

const failureText = (state: Fake, id: string): string | undefined =>
  (state.ingested[0]?.responses.find(r => r.id === id) as unknown as { content: { text: string }[] } | undefined)?.content[0]?.text

test('표시 이름이 붙은 Notion 주소와 답장이 붙은 스레드는 읽는다', async () => {
  const replied = { id: 't2', messageCount: 2, messages: [{ sender: NOTION_SENDER }, { sender: 'me@example.com' }] }
  const cut = { id: 't3', messageCount: 2, messages: [{ sender: 'me@example.com' }] }
  const state = fake({ calls: [notionCall()], mcp: mailbox([thread('t1', `Notion <${NOTION_SENDER}>`), replied, cut, thread('t4', 'Notion Notifications')]) })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(reads(state), ['t1', 't2', 't3', 't4'])
})

test('훅의 주소 판정은 수집 소스와 같다', () => {
  // 기대값은 Gmail 소스의 주소 판정이 낸 값이다. 어긋나면 crates/kairos-source-mcp/src/gmail.rs의 sender_address와 맞춘다.
  const cases: [string, boolean][] = [
    ['notify@mail.notion.com', false],
    ['Notion <notify@mail.notion.com>', false],
    ['other@example.com', true],
    ['합성 <other@example.com>', true],
    ['Notion Notifications', false],
    ['"notify@mail.notion.com" <other@example.com>', true],
  ]
  for (const [sender, foreign] of cases) assert.equal(isForeignThread([{ sender }], 1), foreign, sender)
})

test('후속 읽기 상한이 음수나 소수여도 상한을 넘지 않는다', async () => {
  const threads = Array.from({ length: 50 }, (_, index) => thread(`t${index}`))
  for (const [maxReads, expected] of [[-1, 0], [2.9, 2], ['x', 0], [12, 12]] as const) {
    const state = fake({ calls: [notionCall({ maxReads })], mcp: mailbox(threads) })
    await runTick(state.engine, BUDGET)
    assert.equal(reads(state).length, expected, String(maxReads))
  }
})

test('읽을 만큼 찾으면 쪽을 더 넘기지 않고 바로 읽는다', async () => {
  const state = fake({
    calls: [notionCall()],
    mcp: (_server, tool, args) => {
      if (tool !== 'search_threads') return { content: text({ id: args?.threadId, messages: [] }), isError: false }
      return { content: text({ threads: Array.from({ length: 50 }, (_, index) => thread(`t${index}`)), nextPageToken: 'more' }), isError: false }
    },
  })
  await runTick(state.engine, BUDGET)
  assert.equal(state.mcp.filter(entry => entry.tool === 'search_threads').length, 1)
  assert.equal(reads(state).length, 12)
})

test('시간이 모자라면 쪽 넘기기를 멈추고 찾은 스레드를 읽는다', async () => {
  const state = fake({
    calls: [notionCall()],
    clockStep: 1500,
    mcp: (_server, tool, args) => {
      if (tool !== 'search_threads') return { content: text({ id: args?.threadId, messages: [] }), isError: false }
      return { content: text({ threads: [thread(`p${String(args?.pageToken ?? 0)}`)], nextPageToken: `${Number(args?.pageToken ?? 0) + 1}` }), isError: false }
    },
  })
  await runTick(state.engine, BUDGET)
  const pages = state.mcp.filter(entry => entry.tool === 'search_threads').length
  assert.ok(pages < 10, String(pages))
  assert.ok(reads(state).length > 0)
})

test('앞 검색이 읽기 몫을 다 쓰면 밀린 메일 검색은 다음 틱이다', async () => {
  const threads = Array.from({ length: 20 }, (_, index) => thread(`t${index}`))
  const backlog = { ...(notionCall({}, `${NOTION_QUERY} older_than:1d`) as object), id: 'notion:mail~old' }
  const state = fake({ calls: [notionCall(), backlog], mcp: mailbox(threads) })
  await runTick(state.engine, BUDGET)
  assert.equal(state.mcp.filter(entry => entry.tool === 'search_threads').length, 1)
  assert.equal(reads(state).length, 12)
})

test('두 검색이 읽기 몫 12건을 나눠 쓰고 같은 스레드를 두 번 읽지 않는다', async () => {
  const fresh = [thread('n1'), thread('n2')]
  const old = [thread('n2'), ...Array.from({ length: 20 }, (_, index) => thread(`o${index}`))]
  const backlog = { ...(notionCall({}, `${NOTION_QUERY} older_than:1d`) as object), id: 'notion:mail~old' }
  const state = fake({
    calls: [notionCall(), backlog],
    mcp: (_server, tool, args) => {
      if (tool === 'search_threads') return { content: text({ threads: String(args?.query).includes('older_than') ? old : fresh }), isError: false }
      return { content: text({ id: args?.threadId, messages: [] }), isError: false }
    },
  })
  await runTick(state.engine, BUDGET)
  const ids = reads(state)
  assert.equal(ids.length, 12)
  assert.equal(new Set(ids).size, 12)
  assert.deepEqual(ids.slice(0, 2), ['n1', 'n2'])
  assert.ok(state.ingested[0]?.responses.some(r => r.id === 'notion:mail~old!o0'))
})

test('읽기 하나가 끝나지 않아도 마감에 그 스레드만 hung으로 남기고 나머지를 저장한다', async () => {
  const threads = Array.from({ length: 6 }, (_, index) => thread(`t${index}`))
  const state = fake({ calls: [notionCall()], timerAfterMs: 10 })
  state.engine.mcp.call = async (_server, tool, args) => {
    if (tool === 'search_threads') return { content: text({ threads }), isError: false }
    if (args?.threadId === 't2') return new Promise(() => {})
    return { content: text({ id: args?.threadId, messages: [] }), isError: false }
  }
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.equal(failureText(state, 'notion:mail!t2'), 'kairos:hung')
  assert.equal(state.ingested[0]?.responses.filter(r => r.id.includes('!') && !r.isError).length, 5)
})

test('검색이 끝나지 않아도 틱은 끝나고 hung으로 기록한다. 다른 소스는 저장한다', async () => {
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads'), call('slack:mentions', SLACK, 'slack_search_public_and_private')], timerAfterMs: 10 })
  state.engine.mcp.call = async server => (server === GMAIL ? new Promise(() => {}) : { content: text(slackBody('x')), isError: false })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  const gmailPayload = state.ingested.find(payload => payload.source === 'gmail')
  assert.equal((gmailPayload?.responses[0] as unknown as { content: { text: string }[] }).content[0]?.text, 'kairos:hung')
  assert.equal(state.ingested.find(payload => payload.source === 'slack')?.responses[0]?.isError, false)
})

test('마감 타이머는 sleep 프로세스 하나이고 예산을 쓰는 clock.sleep이 아니다', async () => {
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads')] })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.timers, [['/bin/sleep', '12.000']])
  assert.deepEqual(state.sleeps, [])
})

test('claude 시작 시각을 알면 ingest 몫을 남긴 마감을 쓴다', async () => {
  const clockStart = 1_000_000
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads')], clockStart, startedAt: clockStart - 7000 })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.timers, [['/bin/sleep', '7.000']])
})

test('타이머를 띄우지 못해도 틱은 돈다', async () => {
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads')] })
  const run = state.engine.process.run
  state.engine.process.run = async (argv, init) => {
    if (argv[0] === '/bin/sleep') throw new Error('synthetic')
    return run(argv, init)
  }
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.equal(state.ingested[0]?.responses[0]?.isError, false)
})

const MARKER = [{ type: 'text', text: '<persisted-output>\nOutput too large (60.3KB). Full output saved to: /x/y.txt\n</persisted-output>' }]

test('저장된 큰 응답의 다음 쪽은 kairos page-info로 받아 이어 읽는다', async () => {
  const state = fake({
    calls: [call('gmail:recent', GMAIL, 'search_threads', { query: 'q' })],
    pageInfo: { nextPageToken: 'from-file' },
    mcp: (_server, _tool, args) => (args?.pageToken ? { content: text({ threads: [] }), isError: false } : { content: MARKER, isError: false }),
  })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.mcp.map(entry => entry.args?.pageToken), [undefined, 'from-file'])
  assert.deepEqual(state.pageInfo, [{ content: MARKER }])
  assert.deepEqual(state.ingested[0]?.responses.map(r => r.id), ['gmail:recent', 'gmail:recent#2'])
})

test('저장된 Slack 응답도 pagination_info로 이어 읽는다', async () => {
  const state = fake({
    calls: [call('slack:mentions', SLACK, 'slack_search_public_and_private')],
    pageInfo: { pagination_info: nextCursor('FILE') },
    mcp: (_server, _tool, args) => (args?.cursor ? { content: text(slackBody('x')), isError: false } : { content: MARKER, isError: false }),
  })
  await runTick(state.engine, BUDGET)
  assert.deepEqual(state.mcp.map(entry => entry.args?.cursor), [undefined, 'FILE'])
})

test('page-info가 실패하면 쪽을 멈추고 표식을 그대로 넘긴다', async () => {
  const state = fake({ calls: [call('gmail:recent', GMAIL, 'search_threads')], pageInfoExit: 1, mcp: () => ({ content: MARKER, isError: false }) })
  assert.equal(await runTick(state.engine, BUDGET), DROP_OK)
  assert.equal(state.mcp.length, 1)
})
