#!/usr/bin/env node
// 시험용 가짜 kairos. 실제 kairos나 DB를 건드리지 않는다.
import { spawn } from 'node:child_process'

const [mode, ...rest] = process.argv.slice(2)
if (mode === 'echo') process.stdout.write(JSON.stringify({ schema: 1, ok: true, argv: rest }))
if (mode === 'garbage') {
  process.stdout.write('알 수 없는 명령: next\n')
  process.exitCode = 2
}
if (mode === 'fail') {
  process.stdout.write(JSON.stringify({ schema: 1, ok: false, error: { code: 'not_found', message: '없는 항목입니다' } }))
  process.exitCode = 1
}
if (mode === 'big') process.stdout.write('x'.repeat(5 * 1024 * 1024))
if (mode === 'hang') setTimeout(() => undefined, 60_000)
if (mode === 'orphan') {
  // 자식이 표준 출력을 물고 있어 부모를 죽여도 파이프가 닫히지 않는다.
  spawn(process.execPath, ['-e', 'setTimeout(() => undefined, 2500)'], { stdio: 'inherit' })
  setTimeout(() => undefined, 60_000)
}
