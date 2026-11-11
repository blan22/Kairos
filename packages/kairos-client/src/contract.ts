/**
 * kairos 표시면 계약의 순수 부분. Node나 브라우저 API에 기대지 않아 어느 호스트에서나(Storybook 포함) 쓴다.
 * 실행 파일을 부르는 부분은 index.ts에 있다. 계약은 docs/surface-protocol.md에 있다.
 */
export const CONTRACT_VERSION = 1
/** kairos의 항목 ID와 같은 모양이다. */
export const ITEM_ID = /^[1-9]\d{0,14}$/
export const IDS_LIMIT = 100
export const TITLE_LIMIT = 200

export type Surface = 'orca' | 'claude-code' | 'vscode'
export type ItemAction = 'start' | 'later' | 'done' | 'keep' | 'keep_deploy' | 'move' | 'cancel' | 'pr'

export interface Failure {
  code: string
  message: string
}

export type Reply = { ok: true; value: Record<string, unknown> } | { ok: false; failure: Failure }

export interface ExecOutcome {
  exitCode: number
  stdout: string
  fault: 'spawn' | 'timeout' | 'too_large' | null
}

export type Exec = (file: string, args: readonly string[], timeoutMs: number) => Promise<ExecOutcome>

export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value)

const FAULTS: Record<NonNullable<ExecOutcome['fault']>, Failure> = {
  spawn: { code: 'spawn_failed', message: 'kairos를 실행하지 못했습니다' },
  timeout: { code: 'timeout', message: 'kairos 응답 시간을 넘겼습니다' },
  too_large: { code: 'too_large', message: 'kairos 응답이 너무 큽니다' },
}
const BAD_OUTPUT: Failure = { code: 'bad_output', message: 'kairos 응답을 읽지 못했습니다' }
export const BAD_SCHEMA: Failure = { code: 'schema', message: 'kairos와 호스트의 계약 판이 다릅니다' }
const ERROR_CODE = /^[a-z][a-z0-9_]{0,39}$/

function failureOf(value: Record<string, unknown>): Failure {
  const error = isRecord(value.error) ? value.error : {}
  const code = typeof error.code === 'string' && ERROR_CODE.test(error.code) ? error.code : 'failed'
  const message = typeof error.message === 'string' ? error.message.slice(0, 300) : 'kairos가 실패했습니다'
  return { code, message }
}

/** stdout 전체가 JSON 한 덩어리여야 한다. 그 밖의 글은 버린다. */
export function readReply(outcome: ExecOutcome): Reply {
  if (outcome.fault !== null) return { ok: false, failure: FAULTS[outcome.fault] }
  let value: unknown
  try {
    value = JSON.parse(outcome.stdout)
  } catch {
    return { ok: false, failure: BAD_OUTPUT }
  }
  if (!isRecord(value)) return { ok: false, failure: BAD_OUTPUT }
  if (value.schema !== CONTRACT_VERSION) return { ok: false, failure: BAD_SCHEMA }
  if (value.ok === false) return { ok: false, failure: failureOf(value) }
  return outcome.exitCode === 0 ? { ok: true, value } : { ok: false, failure: BAD_OUTPUT }
}

function checkedIds(ids: readonly unknown[], min: number, max: number): string[] | null {
  if (ids.length < min || ids.length > max) return null
  const values = ids.map(id => (typeof id === 'number' || typeof id === 'string' ? String(id) : ''))
  return values.every(id => ITEM_ID.test(id)) ? values : null
}

/** 버튼 이름과 항목 ID를 kairos 인자로 바꾼다. 맞지 않으면 null이고 kairos를 부르지 않는다. */
export function argvFor(action: ItemAction, ids: readonly unknown[], surface: Surface): string[] | null {
  const one = (build: (id: string) => string[]): string[] | null => {
    const [id] = checkedIds(ids, 1, 1) ?? []
    return id === undefined ? null : build(id)
  }
  const many = (name: string): string[] | null => {
    const values = checkedIds(ids, 1, IDS_LIMIT)
    return values === null ? null : [name, ...values, '--json']
  }
  switch (action) {
    case 'start':
      return one(id => (surface === 'claude-code' ? ['accept', id, '--from', 'claude-code', '--json'] : ['accept', id, '--json']))
    case 'later':
      return many('later')
    case 'done':
      return many('done')
    case 'keep':
      return one(id => ['keep', id, '--json'])
    case 'keep_deploy':
      return one(id => ['keep', id, '--until-deploy', '--json'])
    case 'move':
      return one(id => ['move', id, '--json'])
    case 'cancel':
      return one(id => ['accept', id, '--cancel', '--json'])
    case 'pr':
      return one(id => ['pr', id, '--json'])
    default:
      return null
  }
}

/** 제어 문자와 줄 구분 문자. */
const CONTROL = new RegExp('[\\u0000-\\u001f\\u007f-\\u009f\\u2028\\u2029]', 'g')

/** 사용자가 직접 쓴 할 일 제목. 제어 문자를 지우고 자른 뒤 `--` 뒤에 둔다. */
export function addArgv(title: unknown): string[] | null {
  if (typeof title !== 'string') return null
  const cleaned = title.replace(CONTROL, ' ').trim().slice(0, TITLE_LIMIT)
  return cleaned ? ['add', '--json', '--', cleaned] : null
}

/** git 세션 호스트가 돌려준 띄울 명령. 모양이 맞을 때만 준다. */
export interface Launch {
  cwd: string
  argv: [string, string]
  command: string
}

const RUN_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

export function launchOf(value: Record<string, unknown>): Launch | null {
  const launch = value.launch
  if (!isRecord(launch)) return null
  const { cwd, argv, command } = launch
  if (typeof cwd !== 'string' || !cwd.startsWith('/') || typeof command !== 'string') return null
  if (!Array.isArray(argv) || argv.length !== 2) return null
  const [file, runId] = argv
  if (typeof file !== 'string' || !file.startsWith('/') || !file.endsWith('/kairos-run')) return null
  if (typeof runId !== 'string' || !RUN_ID.test(runId)) return null
  return { cwd, argv: [file, runId], command }
}
