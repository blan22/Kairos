/**
 * 워커 본체. 상태 표시줄을 주기적으로 그리고, 패널의 요청을 kairos 호출로 바꾼다.
 * 예산과 등급 판단은 kairos next에 있다. 여기서는 그리기와 호출만 한다.
 * workspace.open과 notifications.show는 쓰지 않는다. 세션 창은 사용자가 시작이나 이동을 눌렀을 때만 kairos가 앞으로 가져온다.
 * 상태 표시줄의 값은 kairos next --json으로 읽는다. state.json을 읽지 않는다. 60초마다 프로세스 하나를 띄우는 셈이다.
 * 알림을 누르면 kairos reveal이 홈에 reveal.json을 쓴다. 워커는 그 파일만 보고 패널에 kairos.reveal을 보낸다. 내용은 읽지 않는다.
 */
import type { OrcaWorkerApi } from 'orca/worker'
import { OPEN_COMMAND, PANEL_ID, PLUGIN_KEY, REVEAL_DEBOUNCE_MS, REVEAL_FILE, STATUS_ITEM_ID, TIMEOUT_MS } from './config.ts'
import { KAIROS_HOME, ORCA_BIN } from './paths.ts'
import { isRecord, parseList, parseNext, resultLine, runOf, statusOf, warningOf } from './parse.ts'
import type { ActionOutcome, Failure, ListView, NextView, Snapshot } from './protocol.ts'
import { argvFor, BAD_SCHEMA, createKairos, type Exec, type Kairos } from './runner.ts'
import { statusOfFailure, statusOfNext, type StatusView } from './status.ts'

export interface Watcher {
  close: () => void
}

/** 폴더 하나를 감시한다. 바뀐 파일 이름을 넘기고, 걸 수 없으면 던진다. */
export type WatchDir = (dir: string, onChange: (filename: string | null) => void, onError: (error: unknown) => void) => Watcher

export interface WorkerDeps {
  exec: Exec
  /** 0이면 주기 호출을 하지 않는다. */
  pollMs: number
  /** 없으면 reveal.json을 감시하지 않는다. 점검 실행과 대부분의 시험이 그렇다. */
  watch?: WatchDir | null
}

export interface WorkerHandle {
  /** 시작할 때 부른 첫 그리기가 끝나면 풀린다. */
  ready: Promise<void>
  poll: () => Promise<void>
  stop: () => void
}

const BAD_REQUEST: Failure = { code: 'bad_request', message: '잘못된 요청입니다' }
const BUSY: Failure = { code: 'busy', message: '앞선 처리가 끝나지 않았습니다' }

const NEXT_ARGV: readonly string[] = ['next', '--json']
const SEEN_ARGV: readonly string[] = ['next', '--seen', '--surface', 'panel', '--json']
const LIST_ARGV: readonly string[] = ['list', '--json']
const RUNS_ARGV: readonly string[] = ['runs', '--json']

const codeOf = (error: unknown): string => (isRecord(error) && typeof error.code === 'string' ? error.code : 'unknown')

const refused = (failure: Failure): ActionOutcome => ({ ok: false, code: failure.code, message: failure.message, run: null, snapshot: null })

function timeoutFor(action: unknown): number {
  // PR 올리기는 push와 gh 호출이 있어 수락과 같은 제한을 쓴다.
  return action === 'start' || action === 'pr' ? TIMEOUT_MS.accept : TIMEOUT_MS.write
}

async function readNext(kairos: Kairos, seen: boolean): Promise<NextView | Failure> {
  const reply = await kairos(seen ? SEEN_ARGV : NEXT_ARGV, TIMEOUT_MS.read)
  if (!reply.ok) return reply.failure
  return parseNext(reply.value) ?? BAD_SCHEMA
}

async function readList(kairos: Kairos): Promise<ListView | Failure> {
  const reply = await kairos(LIST_ARGV, TIMEOUT_MS.read)
  if (!reply.ok) return reply.failure
  return parseList(reply.value) ?? BAD_SCHEMA
}

const isFailure = (value: object): value is Failure => 'code' in value

export function startWorker(orca: OrcaWorkerApi, deps: WorkerDeps): WorkerHandle {
  const kairos = createKairos(deps.exec)
  let polling = false
  let acting = false
  let stopped = false
  /** 활성 run이 있을 때만 kairos runs로 세션 상태를 맞춘다. 항목이 먼저 닫혀도 세션은 살아 있다. */
  let tracking = false
  /** 세션 상태를 못 읽은 사실. 건강 경고와 같은 자리에 표시한다. */
  let sessionWarning: string | null = null
  let watcher: Watcher | null = null
  let revealTimer: ReturnType<typeof setTimeout> | null = null
  /** 마지막으로 남긴 감시 실패 코드. 폴링마다 다시 걸 때 같은 줄을 반복하지 않는다. */
  let watchFault: string | null = null

  const paint = async (view: StatusView): Promise<void> => {
    try {
      await orca.host.call('statusBar.update', { id: STATUS_ITEM_ID, ...view })
    } catch (error) {
      orca.log(`상태 표시줄 갱신 실패: ${codeOf(error)}`)
    }
  }

  /** next를 읽을 때마다 상태 표시줄을 같이 맞춘다. 실패도 그대로 그린다. */
  const fetchNext = async (seen: boolean): Promise<NextView | Failure> => {
    const read = await readNext(kairos, seen)
    if (isFailure(read)) {
      orca.log(`kairos next 실패: ${read.code}`)
      await paint(statusOfFailure(read))
      return read
    }
    tracking = read.activeRuns > 0
    const next = { ...read, health: read.health ?? sessionWarning }
    await paint(statusOfNext(next))
    return next
  }

  const syncRuns = async (): Promise<void> => {
    if (!tracking) {
      sessionWarning = null
      return
    }
    const reply = await kairos(RUNS_ARGV, TIMEOUT_MS.write)
    if (!reply.ok) orca.log(`kairos runs 실패: ${reply.failure.code}`)
    sessionWarning = reply.ok && reply.value.orca !== false ? null : '세션 상태를 읽지 못했습니다'
  }

  const snapshot = async (seen: boolean): Promise<Snapshot> => {
    const next = await fetchNext(seen)
    const list = await readList(kairos)
    if (isFailure(list)) orca.log(`kairos list 실패: ${list.code}`)
    if (isFailure(next)) return { next: null, list: isFailure(list) ? null : list, error: next }
    return isFailure(list) ? { next, list: null, error: list } : { next, list, error: null }
  }

  const unwatch = (): void => {
    watcher?.close()
    watcher = null
  }

  const watchFailed = (error: unknown): void => {
    const code = codeOf(error)
    if (code !== watchFault) orca.log(`reveal 감시 실패: ${code}`)
    watchFault = code
  }

  /** 원자 쓰기는 이벤트를 여러 번 낸다. 잠잠해진 뒤 한 번만 보내고, 패널이 다시 읽어 항목을 받는다. */
  const announce = (): void => {
    revealTimer = null
    if (!stopped) orca.panels.post('kairos.reveal', { at: Date.now() })
  }

  /** reveal.json만 본다. 다른 파일의 변화로 노출에 남는 읽기를 부르지 않는다. */
  const onChange = (filename: string | null): void => {
    if (stopped || filename !== REVEAL_FILE) return
    if (revealTimer !== null) clearTimeout(revealTimer)
    revealTimer = setTimeout(announce, REVEAL_DEBOUNCE_MS)
  }

  /** 홈이 아직 없거나 감시가 끊기면 다음 폴링에서 다시 건다. 원자 쓰기는 파일을 바꿔 끼우므로 파일이 아니라 폴더를 본다. */
  const startWatch = (): void => {
    if (deps.watch == null || watcher !== null || stopped) return
    try {
      watcher = deps.watch(KAIROS_HOME, onChange, error => {
        watchFailed(error)
        unwatch()
      })
      watchFault = null
    } catch (error) {
      watchFailed(error)
    }
  }

  /** 앞선 폴링이 안 끝났으면 건너뛴다. 끝나면 열린 패널에 알린다. */
  const poll = async (): Promise<void> => {
    startWatch()
    if (polling || stopped) return
    polling = true
    try {
      await syncRuns()
      await fetchNext(false)
      if (!stopped) orca.panels.post('kairos.tick', { at: Date.now() })
    } catch (error) {
      // 주기 호출이라 받아 줄 곳이 없다. 던지면 워커가 죽는다.
      orca.log(`폴링 실패: ${codeOf(error)}`)
    } finally {
      polling = false
    }
  }

  orca.panels.handle('kairos.snapshot', async input => snapshot(isRecord(input) && input.seen === true))

  orca.panels.handle('kairos.action', async input => {
    if (!isRecord(input) || !Array.isArray(input.ids)) return refused(BAD_REQUEST)
    const argv = argvFor(input.action, input.ids, input.title)
    if (argv === null) return refused(BAD_REQUEST)
    if (acting) return refused(BUSY)
    acting = true
    try {
      const reply = await kairos(argv, timeoutFor(input.action))
      const after = await snapshot(input.seen === true)
      if (!reply.ok) {
        orca.log(`kairos ${argv[0]} 실패: ${reply.failure.code}`)
        return { ok: false, code: reply.failure.code, message: reply.failure.message, run: null, snapshot: after }
      }
      return { ok: true, code: null, message: resultLine(reply.value), status: statusOf(reply.value), warning: warningOf(reply.value), run: runOf(reply.value), snapshot: after }
    } finally {
      acting = false
    }
  })

  // 상태 표시줄과 단축키가 워커에 닿지 않은 실측이 있다(S7). 원인은 아직 모른다. 첫 적재 때 재고 설계 16절에 적는다.
  // 패널은 사이드바 아이콘으로도 열리므로 이 명령에 의존하지 않는다.
  orca.commands.register(OPEN_COMMAND, async () => {
    orca.log('open command')
    const outcome = await deps.exec(ORCA_BIN, ['plugin', 'open', PLUGIN_KEY, '--panel', PANEL_ID, '--json'], TIMEOUT_MS.open)
    if (outcome.fault === null && outcome.exitCode === 0) return 'kairos 패널을 열었습니다'
    orca.log(`패널 열기 실패: ${outcome.fault ?? outcome.exitCode}`)
    return '패널을 열지 못했습니다. 오른쪽 사이드바의 kairos 아이콘을 누르세요'
  })

  const timer = deps.pollMs > 0 ? setInterval(() => void poll(), deps.pollMs) : null
  // activate가 끝나야 Orca가 워커를 준비됨으로 본다. 첫 그리기는 기다리지 않는다.
  const ready = poll()

  return {
    ready,
    poll,
    stop: () => {
      stopped = true
      if (timer !== null) clearInterval(timer)
      if (revealTimer !== null) clearTimeout(revealTimer)
      revealTimer = null
      unwatch()
    },
  }
}
