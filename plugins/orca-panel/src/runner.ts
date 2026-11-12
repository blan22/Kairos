/**
 * kairos 호출. 셸을 거치지 않고 execFile에 인자 배열만 넘긴다.
 * 인자는 argvFor가 고정 낱말과 정규식을 통과한 항목 ID로만 만든다. 외부에서 온 제목과 URL은 여기 오지 않는다.
 * 사용자가 패널에 직접 쓴 할 일 제목만 add의 인자가 된다. 셸을 거치지 않고, -- 뒤에 두어 옵션으로 읽히지 않는다.
 */
import { execFile } from 'node:child_process'
import { ADD_TITLE_LIMIT, IDS_LIMIT } from './config.ts'
import { KAIROS_BIN } from './paths.ts'
import { CONTRACT_VERSION, failureOf, ITEM_ID, isRecord } from './parse.ts'
import type { Failure } from './protocol.ts'
import { cleanText } from './text.ts'

export type ExecFault = 'spawn' | 'timeout' | 'too_large' | 'inert'

export interface ExecOutcome {
  exitCode: number
  stdout: string
  fault: ExecFault | null
}

export type Exec = (file: string, args: readonly string[], timeoutMs: number) => Promise<ExecOutcome>

export type Reply = { ok: true; value: Record<string, unknown> } | { ok: false; failure: Failure }

export type Kairos = (argv: readonly string[], timeoutMs: number) => Promise<Reply>

const MAX_OUTPUT = 4 * 1024 * 1024

function outcomeOf(error: unknown, stdout: string): ExecOutcome {
  if (error === null || error === undefined) return { exitCode: 0, stdout, fault: null }
  const { code, killed } = error as { code?: unknown; killed?: unknown }
  if (killed === true) return { exitCode: 1, stdout: '', fault: 'timeout' }
  if (code === 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER') return { exitCode: 1, stdout: '', fault: 'too_large' }
  if (typeof code === 'number') return { exitCode: code, stdout, fault: null }
  return { exitCode: 1, stdout: '', fault: 'spawn' }
}

export const execProgram: Exec = (file, args, timeoutMs) =>
  new Promise(resolve => {
    try {
      execFile(
        file,
        [...args],
        { timeout: timeoutMs, killSignal: 'SIGKILL', maxBuffer: MAX_OUTPUT, cwd: '/', shell: false, encoding: 'utf8' },
        (error, stdout) => resolve(outcomeOf(error, String(stdout))),
      )
    } catch {
      resolve({ exitCode: 1, stdout: '', fault: 'spawn' })
    }
  })

/** orca plugin check가 워커를 이 기기에서 돌릴 때 쓴다. 실제 kairos를 부르지 않는다. */
export const inertExec: Exec = async () => ({ exitCode: 1, stdout: '', fault: 'inert' })

const FAULTS: Record<ExecFault, Failure> = {
  spawn: { code: 'spawn_failed', message: 'kairos를 실행하지 못했습니다' },
  timeout: { code: 'timeout', message: 'kairos 응답 시간을 넘겼습니다' },
  too_large: { code: 'too_large', message: 'kairos 응답이 너무 큽니다' },
  inert: { code: 'check_mode', message: '점검 실행이라 kairos를 부르지 않습니다' },
}
const BAD_OUTPUT: Failure = { code: 'bad_output', message: 'kairos 응답을 읽지 못했습니다. 설치본이 오래됐을 수 있습니다' }
export const BAD_SCHEMA: Failure = { code: 'schema', message: 'kairos와 플러그인의 JSON 형식이 다릅니다' }

/** stdout 전체가 JSON 한 덩어리여야 한다. 그 밖의 글은 화면에 내지 않고 버린다. */
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
  if (value.ok !== true) return { ok: false, failure: failureOf(value) }
  return outcome.exitCode === 0 ? { ok: true, value } : { ok: false, failure: BAD_OUTPUT }
}

/** 앞선 호출이 끝나야 다음 호출이 시작한다. 시간 제한이 있어 뒤가 영원히 막히지 않는다. */
export function createKairos(exec: Exec): Kairos {
  let tail: Promise<unknown> = Promise.resolve()
  return (argv, timeoutMs) => {
    const job = async (): Promise<Reply> => readReply(await exec(KAIROS_BIN, argv, timeoutMs))
    const run = tail.then(job, job)
    tail = run
    return run
  }
}

function checkedIds(ids: readonly unknown[], min: number, max: number): string[] | null {
  if (ids.length < min || ids.length > max) return null
  const values = ids.map(id => (typeof id === 'number' || typeof id === 'string' ? String(id) : ''))
  return values.every(id => ITEM_ID.test(id)) ? values : null
}

/** 패널의 요청을 kairos 인자로 바꾼다. 맞지 않으면 null이고 kairos를 부르지 않는다. */
export function argvFor(action: unknown, ids: readonly unknown[], title?: unknown): string[] | null {
  const one = (build: (id: string) => string[]): string[] | null => {
    const [id] = checkedIds(ids, 1, 1) ?? []
    return id === undefined ? null : build(id)
  }
  const none = (argv: string[]): string[] | null => (ids.length === 0 ? argv : null)
  const many = (name: string, ...options: string[]): string[] | null => {
    const values = checkedIds(ids, 1, IDS_LIMIT)
    return values === null ? null : [name, ...values, ...options, '--json']
  }
  switch (action) {
    case 'start':
      return one(id => ['accept', id, '--json'])
    case 'later':
      return many('later')
    case 'later_hour':
      return many('later', '--when', 'hour')
    case 'later_tomorrow':
      return many('later', '--when', 'tomorrow')
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
    case 'ack':
      return none(['next', '--ack', '--json'])
    case 'focus_25':
      return none(['focus', '25', '--json'])
    case 'focus_50':
      return none(['focus', '50', '--json'])
    case 'focus_90':
      return none(['focus', '90', '--json'])
    case 'focus_end':
      return none(['focus', '--end', '--json'])
    case 'display_auto':
      return none(['display', 'auto', '--json'])
    case 'add': {
      const clean = cleanText(title, ADD_TITLE_LIMIT)
      return clean === '' ? null : none(['add', '--json', '--', clean])
    }
    default:
      return null
  }
}
