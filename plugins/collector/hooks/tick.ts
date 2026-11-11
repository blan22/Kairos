/**
 * 수집 틱 본체. 모드에는 Node가 없어 `$`만 쓴다. 판단은 전부 kairos 쪽에 있고 여기서는 호출을 나른다.
 * 외부 문자열은 stdin으로만 넘긴다. 내부 실행은 argv 방식이다.
 *
 * 시간: 훅 예산은 10초이고 $.mcp.call과 $.process.run은 세지 않지만 $.clock.sleep은 센다.
 * 그래서 연결 재시도는 남은 예산을 보고, 소스는 병렬로 돌려 재시도 대기가 겹치게 한다.
 * 마감 타이머는 sleep 프로세스다. 11초가 흘러도 예산은 그대로였다(측정). $.mcp.call에는 시간 제한이 없으므로
 * 호출은 이 타이머와 경쟁시켜, 하나가 멈춰도 그때까지 읽은 것을 저장한다.
 */

import { CALENDAR, GMAIL, SLACK, TOOLS } from './allowed.ts'

export const PLUGIN_NAME = 'kairos-collector'
export const DROP_OK = 'collected-without-model'
export const DROP_FAILED = 'collector-failed'

const NOTION_SENDER = 'notify@mail.notion.com'
const NOTION_SEARCH_PREFIX = `from:${NOTION_SENDER} `

/**
 * 허용목록. 설치기가 바인딩으로 만든 allowed.ts에서 온다. plan-tick 출력이 잘못되어도 이 밖으로 나가지 못한다. 모두 읽기 도구다.
 * 스레드 읽기는 여기 없다. plan이 직접 부를 수 없고 readThreads가 같은 틱의 검색 결과로만 부른다.
 */
const ALLOWED_CALLS: ReadonlyMap<string, ReadonlySet<string>> = (() => {
  const calls = new Map<string, Set<string>>()
  for (const [server, tool] of [
    [CALENDAR.server, CALENDAR.list],
    [GMAIL.server, GMAIL.search],
    [SLACK.server, SLACK.search],
  ]) {
    calls.set(server, (calls.get(server) ?? new Set()).add(tool))
  }
  return calls
})()

/** tool.check가 보는 이름. */
const ALLOWED_TOOLS: ReadonlySet<string> = new Set(TOOLS)

const MAX_PAGES = 10
const RETRY_DELAY_MS = 500
const MAX_ATTEMPTS = 16
/** 재시도를 멈추는 남은 예산. 뒤의 처리와 .catch 전환에 쓸 시간을 남긴다. */
const RETRY_RESERVE_MS = 1500
/** 훅 시작부터 호출에 쓰는 시간. claude 시작 시각을 알면 아래 몫이 먼저 닿는다. */
const CALL_DEADLINE_MS = 12_000
/** collect가 claude를 20초에 끊는다. 이 시간을 ingest 몫으로 남긴다. 측정은 극단 합성에서 2.6초다. */
const TICK_LIMIT_MS = 20_000
const INGEST_RESERVE_MS = 6000
const PLAN_TIMEOUT_MS = 8000
/** 검색 쪽을 그만 넘기고 읽기로 넘어가는 남은 시간. 읽기 12건이 약 5초다(측정). */
const READ_RESERVE_MS = 5000
const SLEEP = '/bin/sleep'
/** 틱당 후속 읽기 상한과 동시 호출 수. 읽기 1건이 0.7~1.6초라(측정) 12건이 마감 안에 든다. 소스의 검색을 합쳐 센다. */
const MAX_EXPAND_READS = 12
const EXPAND_CONCURRENCY = 4
/** 응답 id에 들어가므로 쪽 표시(#)가 섞일 수 없는 형식만 받는다. */
const THREAD_ID = /^[A-Za-z0-9_-]{1,64}$/
const INGEST_TIMEOUT_MS = 5000
const PAGE_INFO_TIMEOUT_MS = 3000
const PERSISTED_PREFIX = '<persisted-output>'

export const isPermitted = (plugin: string | undefined, tool: string): boolean =>
  plugin === PLUGIN_NAME && ALLOWED_TOOLS.has(tool)

export const isAllowedCall = (server: string, tool: string): boolean => ALLOWED_CALLS.get(server)?.has(tool) === true

export interface Budget {
  readonly remainingMs: number
}

interface McpResult {
  content: unknown
  isError?: boolean
}

interface ProcessResult {
  exitCode: number
  stdout: string
}

/** 이 모드가 쓰는 `$`의 범위. */
export interface Engine {
  plugin: { root: string }
  env: { get: (name: 'KAIROS_TICK_STARTED_AT') => Promise<string | undefined> }
  clock: { now: () => Promise<number>; sleep: (ms: number) => Promise<void> }
  mcp: { call: (server: string, tool: string, args?: Record<string, unknown>) => Promise<McpResult> }
  process: {
    run: (argv: readonly string[], init?: { stdin?: string; timeoutMs?: number }) => Promise<ProcessResult>
  }
}

interface Expand {
  tool: string
  maxReads: number
  skip: Readonly<Record<string, number>>
  retryIds: readonly string[]
}

interface Call {
  id: string
  query: string
  server: string
  tool: string
  args: Record<string, unknown>
  expand?: Expand
}

interface Plan {
  now: string
  calls: Call[]
}

interface Response {
  id: string
  isError: boolean
  content: unknown
}

/** 호출에 쓰는 마감. 두 시각은 같은 기준이고, expired는 마감에 풀린다. */
interface Clock {
  deadline: number
  expired: Promise<typeof EXPIRED>
}

const EXPIRED = Symbol('expired')

const modeFailure = (id: string, code: string): Response => ({
  id,
  isError: true,
  content: [{ type: 'text', text: `kairos:${code}` }],
})

const isNotConnected = (error: unknown): boolean => String(error).includes('no connected MCP tool')

/** 훅이 MCP 연결 완료보다 먼저 돌 수 있다(스파이크: 10번째 시도, 5.4초). */
async function callWithRetry($: Engine, budget: Budget, call: Call, args: Record<string, unknown>): Promise<McpResult> {
  for (let attempt = 1; ; attempt += 1) {
    try {
      return await $.mcp.call(call.server, call.tool, args)
    } catch (error) {
      if (!isNotConnected(error) || attempt >= MAX_ATTEMPTS || budget.remainingMs < RETRY_RESERVE_MS) throw error
      await $.clock.sleep(RETRY_DELAY_MS)
    }
  }
}

function failureCode(error: unknown): string {
  const message = String(error)
  if (isNotConnected(error)) return 'tool_missing'
  return /refused|denied/i.test(message) ? 'refused' : 'threw'
}

function bodyOf(content: unknown): Record<string, unknown> | null {
  if (!Array.isArray(content)) return null
  const text = content.map(block => (block?.type === 'text' ? String(block.text) : '')).join('')
  try {
    const body = JSON.parse(text)
    return body !== null && typeof body === 'object' ? body : null
  } catch {
    return null
  }
}

const isPersisted = (content: unknown): boolean =>
  Array.isArray(content) && content.length === 1 && String(content[0]?.text).startsWith(PERSISTED_PREFIX)

/** 큰 응답은 표식으로 온다. 경로 검사가 있는 kairos에 넘겨 다음 쪽 값만 돌려받는다. 파일은 ingest가 쓰고 지운다. */
async function pageInfo($: Engine, content: unknown): Promise<Record<string, unknown> | null> {
  try {
    const { exitCode, stdout } = await $.process.run(kairosArgv($, 'page-info'), {
      stdin: JSON.stringify({ content }),
      timeoutMs: PAGE_INFO_TIMEOUT_MS,
    })
    return exitCode === 0 ? (JSON.parse(stdout) as Record<string, unknown>) : null
  } catch {
    return null
  }
}

/** 다음 쪽의 인자. 읽지 못하면 null이고, 그 쪽의 해석 판정은 kairos가 한다. */
async function nextArgs($: Engine, call: Call, args: Record<string, unknown>, content: unknown): Promise<Record<string, unknown> | null> {
  const body = isPersisted(content) ? await pageInfo($, content) : bodyOf(content)
  if (call.server === SLACK.server && call.tool === SLACK.search) {
    const info = body?.pagination_info
    const cursor = typeof info === 'string' ? /use cursor `([^`]+)`/.exec(info)?.[1] : undefined
    return cursor ? { ...args, cursor } : null
  }
  const token = body?.nextPageToken
  return typeof token === 'string' && token ? { ...args, pageToken: token } : null
}

/**
 * 쪽을 끝까지 읽는다. 실패하면 그 자리에서 실패 응답으로 끝낸다. 쪽 상한은 kairos가 남은 커서로 알아챈다.
 * enough가 참이면 남은 쪽을 두고 멈춘다. 남은 쪽은 kairos가 다음 쪽 값으로 알아챈다.
 */
async function readCall(
  $: Engine,
  budget: Budget,
  call: Call,
  clock: Clock,
  enough: (pages: readonly Response[]) => Promise<boolean> = async () => false,
): Promise<Response[]> {
  if (!isAllowedCall(call.server, call.tool)) return [modeFailure(call.id, 'not_allowed')]
  const pages: Response[] = []
  let args: Record<string, unknown> | null = call.args
  for (let page = 1; page <= MAX_PAGES && args; page += 1) {
    const id = page === 1 ? call.id : `${call.id}#${page}`
    if ((await $.clock.now()) >= clock.deadline) return [...pages, modeFailure(id, 'timeout')]
    try {
      const result = await Promise.race([callWithRetry($, budget, call, args), clock.expired])
      if (result === EXPIRED) return [...pages, modeFailure(id, 'hung')]
      pages.push({ id, isError: result.isError === true, content: result.content })
      if (result.isError === true || (await enough(pages))) return pages
      args = await nextArgs($, call, args, result.content)
    } catch (error) {
      return [...pages, modeFailure(id, failureCode(error))]
    }
  }
  return pages
}

const isExpandable = (call: Call): boolean =>
  call.expand?.tool === GMAIL.thread &&
  call.server === GMAIL.server &&
  call.tool === GMAIL.search &&
  typeof call.args.query === 'string' &&
  call.args.query.startsWith(NOTION_SEARCH_PREFIX)

/** 꺾쇠 안 주소. src/sources/gmail.ts의 senderAddress와 같은 판정이다. 모드는 src를 불러오지 못한다. */
function addressOf(sender: unknown): string | null {
  if (typeof sender !== 'string') return null
  const text = sender.trim()
  const address = /<([^<>\s]+@[^<>\s]+)>\s*$/.exec(text)?.[1] ?? (/^[^<>\s"]+@[^<>\s"]+$/.test(text) ? text : null)
  return address?.toLowerCase() ?? null
}

/** 모두 읽을 수 있는 다른 주소이고 목록이 잘리지 않았을 때만 Notion 스레드가 아니다. 판정은 kairos가 다시 한다. */
export function isForeignThread(messages: unknown, messageCount: unknown): boolean {
  if (!Array.isArray(messages) || messages.length === 0) return false
  const addresses = messages.map(message => addressOf(message?.sender))
  const complete = typeof messageCount !== 'number' || messageCount <= messages.length
  return complete && addresses.every(address => address !== null && address !== NOTION_SENDER)
}

/** 검색 결과에서 읽을 스레드를 뽑는다. 검색 순서(최신 먼저)를 지키고 실패한 적이 있는 스레드는 뒤로 미룬다. */
function threadsToRead(pages: readonly Response[], expand: Expand | undefined, done: ReadonlySet<string>): string[] {
  const skip = expand?.skip !== null && typeof expand?.skip === 'object' ? expand.skip : {}
  const retry = new Set(Array.isArray(expand?.retryIds) ? expand.retryIds : [])
  const first = new Set<string>()
  const later = new Set<string>()
  for (const page of pages) {
    const threads = page.isError ? null : bodyOf(page.content)?.threads
    if (!Array.isArray(threads)) continue
    for (const thread of threads) {
      const { id, messageCount, messages } = thread ?? {}
      if (typeof id !== 'string' || !THREAD_ID.test(id) || done.has(id) || isForeignThread(messages, messageCount)) continue
      const known = Object.hasOwn(skip, id) ? skip[id] : undefined
      if (typeof known === 'number' && !(typeof messageCount === 'number' && messageCount > known)) continue
      ;(retry.has(id) ? later : first).add(id)
    }
  }
  return [...first, ...later]
}

const readLimit = (call: Call): number => Math.max(0, Math.min(Math.trunc(Number(call.expand?.maxReads)) || 0, MAX_EXPAND_READS))

async function readThread($: Engine, budget: Budget, call: Call, threadId: string, clock: Clock): Promise<Response> {
  const id = `${call.id}!${threadId}`
  if ((await $.clock.now()) >= clock.deadline) return modeFailure(id, 'timeout')
  try {
    const result = await Promise.race([
      callWithRetry($, budget, { ...call, tool: GMAIL.thread }, { threadId, messageFormat: 'PLAIN_TEXT' }),
      clock.expired,
    ])
    if (result === EXPIRED) return modeFailure(id, 'hung')
    return { id, isError: result.isError === true, content: result.content }
  } catch (error) {
    return modeFailure(id, failureCode(error))
  }
}

/** 후속 읽기. 호출 대상은 이 틱의 검색 응답에서만 나온다. 못 읽은 스레드는 kairos가 검색 결과와 대조해 알아챈다. */
async function readThreads($: Engine, budget: Budget, call: Call, ids: readonly string[], clock: Clock): Promise<Response[]> {
  const queue = [...ids]
  const read: Response[] = []
  const worker = async (): Promise<void> => {
    for (let threadId = queue.shift(); threadId !== undefined; threadId = queue.shift()) {
      read.push(await readThread($, budget, call, threadId, clock))
    }
  }
  await Promise.all(Array.from({ length: EXPAND_CONCURRENCY }, worker))
  return read
}

/**
 * 설치본의 kairos는 실행 파일이다. 런타임을 찾을 필요 없이 플러그인과 같은 설치본의 것을 바로 부른다.
 * 화살표 함수로 바꾸면 안 된다. 엔진은 `$`를 넘기는 함수 표현식을 거부하고 모듈 전체가 적재되지 않는다(`claude plugin validate`로 확인).
 */
function kairosArgv($: Engine, ...args: string[]): string[] {
  return [`${$.plugin.root}/../../bin/kairos`, ...args]
}

async function readPlan($: Engine): Promise<Plan> {
  const { exitCode, stdout } = await $.process.run(kairosArgv($, 'plan-tick'), { timeoutMs: PLAN_TIMEOUT_MS })
  if (exitCode !== 0) throw new Error('plan-tick failed')
  const plan = JSON.parse(stdout) as Plan
  if (typeof plan.now !== 'string' || !Array.isArray(plan.calls)) throw new Error('bad plan')
  return plan
}

/** 마감에 풀리는 타이머. 띄우지 못하면 풀리지 않고 호출 시작 전의 마감 검사만 남는다. */
function expiresAt($: Engine, deadline: number, now: number): Clock['expired'] {
  const seconds = Math.max(deadline - now, 0) / 1000
  return $.process
    .run([SLEEP, seconds.toFixed(3)], { timeoutMs: deadline - now + PAGE_INFO_TIMEOUT_MS })
    .then(() => EXPIRED as typeof EXPIRED, () => new Promise<typeof EXPIRED>(() => {}))
}

/** 소스 하나를 읽고 바로 저장한다. 다른 소스가 멈춰도 이 소스의 결과는 남는다. */
async function collectSource($: Engine, budget: Budget, source: string, calls: Call[], now: string, clock: Clock): Promise<boolean> {
  const responses: Response[] = []
  const done = new Set<string>()
  for (const call of calls) {
    if (!isExpandable(call)) {
      responses.push(...(await readCall($, budget, call, clock)))
      continue
    }
    // 앞 호출이 읽기 몫을 다 썼으면 뒤 호출은 다음 틱이다. kairos가 응답이 없는 것으로 알아챈다.
    const limit = readLimit(call) - done.size
    if (limit <= 0 && done.size > 0) continue
    // 찾은 만큼 바로 읽는다. 쪽을 끝까지 넘기다 마감을 다 쓰면 한 건도 못 읽는다.
    const enough = async (pages: readonly Response[]): Promise<boolean> =>
      threadsToRead(pages, call.expand, done).length >= limit || (await $.clock.now()) >= clock.deadline - READ_RESERVE_MS
    const pages = await readCall($, budget, call, clock, enough)
    responses.push(...pages)
    const ids = threadsToRead(pages, call.expand, done).slice(0, limit)
    for (const id of ids) done.add(id)
    responses.push(...(await readThreads($, budget, call, ids, clock)))
  }
  const { exitCode } = await $.process.run(kairosArgv($, 'ingest'), {
    stdin: JSON.stringify({ now, source, responses }),
    timeoutMs: INGEST_TIMEOUT_MS,
  })
  return exitCode === 0
}

const sourceOf = (call: Call): string => call.id.slice(0, call.id.indexOf(':'))

/** 어떤 경우에도 drop 사유를 돌려준다. 소스별 실패는 source_run에 남고 틱 자체의 실패가 아니다. */
export async function runTick($: Engine, budget: Budget): Promise<string> {
  const started = await $.clock.now()
  const plan = await readPlan($)
  const launched = Number(await $.env.get('KAIROS_TICK_STARTED_AT'))
  const tickEnd = Number.isFinite(launched) && launched > 0 ? launched + TICK_LIMIT_MS - INGEST_RESERVE_MS : Number.POSITIVE_INFINITY
  const deadline = Math.min(started + CALL_DEADLINE_MS, tickEnd)
  const clock: Clock = { deadline, expired: expiresAt($, deadline, await $.clock.now()) }
  const bySource = new Map<string, Call[]>()
  for (const call of plan.calls) bySource.set(sourceOf(call), [...(bySource.get(sourceOf(call)) ?? []), call])

  const saved = await Promise.all(
    [...bySource].map(([source, calls]) =>
      collectSource($, budget, source, calls, plan.now, clock).catch(() => false),
    ),
  )
  return saved.every(Boolean) ? DROP_OK : DROP_FAILED
}
