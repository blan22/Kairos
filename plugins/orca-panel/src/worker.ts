/** 워커 진입점. Orca가 다시 시작하면 activate가 다시 돌아 상태 표시줄을 새로 그린다. */
import { watch } from 'node:fs'
import type { OrcaWorkerApi } from 'orca/worker'
import { POLL_MS } from './config.ts'
import { execProgram, inertExec } from './runner.ts'
import { startWorker, type WatchDir, type WorkerHandle } from './service.ts'

/** orca plugin check가 이 프로세스에 넣는 값이다. 점검은 실제 kairos와 DB를 건드리지 않는다. */
const isCheckRun = (): boolean => process.env.ORCA_PLUGIN_CHECK_PANELS !== undefined

/** 파일 이름만 넘긴다. 감시자의 error를 받지 않으면 워커가 죽으므로 항상 붙인다. */
const watchDir: WatchDir = (dir, onChange, onError) => {
  const watcher = watch(dir, { persistent: false }, (_event, filename) => onChange(typeof filename === 'string' ? filename : null))
  watcher.on('error', onError)
  return watcher
}

let handle: WorkerHandle | null = null

export default function activate(orca: OrcaWorkerApi): void {
  handle = startWorker(orca, isCheckRun() ? { exec: inertExec, pollMs: 0, watch: null } : { exec: execProgram, pollMs: POLL_MS, watch: watchDir })
}

export function deactivate(): void {
  handle?.stop()
  handle = null
}
