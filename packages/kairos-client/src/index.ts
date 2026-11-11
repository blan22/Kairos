/**
 * kairos 표시면 계약 클라이언트. 호스트 API에 기대지 않는 순수 Node 모듈이다.
 *
 * - 셸을 거치지 않고 execFile에 인자 배열만 넘긴다. 인자는 고정 낱말과 정규식을 통과한 항목 ID로만 만든다.
 * - 호출은 큐로 한 번에 하나만 돈다. 호출마다 시간 제한이 있고 넘기면 SIGKILL로 끊는다.
 * - stdout 전체가 JSON 한 덩어리여야 하고 `schema`가 계약 판과 같아야 한다.
 * - 실행 파일은 본인 소유이고 그룹이나 남이 쓸 수 없어야 부른다.
 *
 * 계약은 docs/surface-protocol.md에 있다.
 */
import { execFile, spawn } from 'node:child_process'
import { statSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { addArgv, argvFor, isRecord, readReply, type Exec, type ExecOutcome, type Failure, type ItemAction, type Reply, type Surface } from './contract.ts'

export * from './contract.ts'

const MAX_OUTPUT = 4 * 1024 * 1024

/** 기본 데이터 폴더. 환경 변수로 받지 않는다. */
export const defaultHome = (): string => join(homedir(), 'Library', 'Application Support', 'kairos')

/** 설치본의 kairos. `kairos install --apply`가 `current`를 이번 판에 건다. */
export const defaultBinary = (home: string = defaultHome()): string => join(home, 'current', 'bin', 'kairos')

/** 본인 소유이고 그룹이나 남이 쓸 수 없는 일반 파일이어야 부른다. 아니면 이유 코드다. */
export function checkBinary(path: string): Failure | null {
  try {
    const stat = statSync(path)
    if (!stat.isFile()) return { code: 'binary_missing', message: 'kairos 실행 파일이 없습니다' }
    if (typeof process.getuid === 'function' && stat.uid !== process.getuid()) {
      return { code: 'binary_unsafe', message: 'kairos 실행 파일의 소유자가 다릅니다' }
    }
    if ((stat.mode & 0o022) !== 0) return { code: 'binary_unsafe', message: 'kairos 실행 파일을 다른 사용자가 고칠 수 있습니다' }
    return null
  } catch {
    return { code: 'binary_missing', message: 'kairos 실행 파일이 없습니다. kairos install --apply를 실행하세요' }
  }
}

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

export interface ClientOptions {
  surface: Surface
  binary?: string
  exec?: Exec
  /** 시험이 끈다. */
  checkBinary?: boolean
}

export interface KairosClient {
  readonly binary: string
  hello(): Promise<Reply>
  next(options?: { seen?: boolean }): Promise<Reply>
  list(): Promise<Reply>
  runs(): Promise<Reply>
  act(action: ItemAction, ids: readonly unknown[]): Promise<Reply>
  add(title: unknown): Promise<Reply>
  /** 상태가 바뀔 때마다 부른다. 돌려준 함수로 멈춘다. 끊기면 다시 붙는다. */
  watch(onChange: () => void): () => void
}

const TIMEOUT = { read: 15_000, write: 20_000, accept: 115_000 } as const
const INVALID: Failure = { code: 'invalid_request', message: '보낼 수 없는 요청입니다' }

export function createClient(options: ClientOptions): KairosClient {
  const binary = options.binary ?? defaultBinary()
  const exec = options.exec ?? execProgram
  let tail: Promise<unknown> = Promise.resolve()
  const call = (argv: readonly string[] | null, timeoutMs: number): Promise<Reply> => {
    if (argv === null) return Promise.resolve({ ok: false, failure: INVALID })
    const job = async (): Promise<Reply> => {
      if (options.checkBinary !== false) {
        const unsafe = checkBinary(binary)
        if (unsafe) return { ok: false, failure: unsafe }
      }
      return readReply(await exec(binary, argv, timeoutMs))
    }
    const run = tail.then(job, job)
    tail = run
    return run
  }
  return {
    binary,
    hello: () => call(['hello', '--json'], TIMEOUT.read),
    next: ({ seen = false } = {}) =>
      call(['next', ...(seen ? ['--seen'] : []), '--surface', options.surface, '--json'], TIMEOUT.read),
    list: () => call(['list', '--json'], TIMEOUT.read),
    runs: () => call(['runs', '--json'], TIMEOUT.write),
    act: (action, ids) => call(argvFor(action, ids, options.surface), action === 'start' || action === 'pr' ? TIMEOUT.accept : TIMEOUT.write),
    add: title => call(addArgv(title), TIMEOUT.write),
    watch: onChange => {
      let stopped = false
      let child: ReturnType<typeof spawn> | null = null
      const start = (): void => {
        if (stopped) return
        if (options.checkBinary !== false && checkBinary(binary)) {
          setTimeout(start, 30_000)
          return
        }
        child = spawn(binary, ['watch', '--json'], { cwd: '/', stdio: ['ignore', 'pipe', 'ignore'], shell: false })
        let buffer = ''
        child.stdout?.setEncoding('utf8')
        child.stdout?.on('data', (chunk: string) => {
          buffer += chunk
          for (let index = buffer.indexOf('\n'); index >= 0; index = buffer.indexOf('\n')) {
            const line = buffer.slice(0, index)
            buffer = buffer.slice(index + 1)
            try {
              const event: unknown = JSON.parse(line)
              if (isRecord(event) && event.kind === 'changed') onChange()
            } catch {
              // 계약 밖의 줄은 버린다.
            }
          }
          if (buffer.length > 64 * 1024) buffer = ''
        })
        child.on('exit', () => {
          child = null
          if (!stopped) setTimeout(start, 5_000)
        })
        child.on('error', () => undefined)
      }
      start()
      return () => {
        stopped = true
        child?.kill()
      }
    },
  }
}

