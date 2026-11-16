import assert from 'node:assert/strict'
import { chmodSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import {
  addArgv,
  argvFor,
  binaryFor,
  checkBinary,
  createClient,
  homeFor,
  insideLocalAppData,
  launchOf,
  readReply,
  type Exec,
} from './index.ts'

test('기본 데이터 폴더와 실행 파일은 OS마다 kairos와 같은 자리다', () => {
  assert.equal(homeFor('darwin', '/Users/synth'), '/Users/synth/Library/Application Support/kairos')
  assert.equal(homeFor('linux', '/home/synth'), '/home/synth/.local/share/kairos')
  assert.equal(homeFor('win32', 'C:\\Users\\synth'), 'C:\\Users\\synth\\AppData\\Local\\kairos')
  assert.equal(binaryFor('linux', '/home/synth/.local/share/kairos'), '/home/synth/.local/share/kairos/current/bin/kairos')
  assert.equal(binaryFor('win32', 'C:\\k'), 'C:\\k\\current\\bin\\kairos.exe')
})

test('Windows는 %LOCALAPPDATA% 아래의 실행 파일만 받는다', () => {
  const user = 'C:\\Users\\synth'
  assert.ok(insideLocalAppData('c:\\users\\SYNTH\\AppData\\Local\\kairos\\current\\bin\\kairos.exe', user))
  assert.ok(!insideLocalAppData('C:\\kairos\\bin\\kairos.exe', user))
  assert.ok(!insideLocalAppData('C:\\Users\\synth\\AppData\\LocalLow\\kairos.exe', user))
  assert.ok(!insideLocalAppData('C:\\Users\\synth\\AppData\\Local\\..\\Roaming\\kairos.exe', user))
})

test('버튼 이름과 검사한 ID로만 인자를 만든다', () => {
  assert.deepEqual(argvFor('later', [1, '2'], 'vscode'), ['later', '1', '2', '--json'])
  assert.deepEqual(argvFor('start', [3], 'claude-code'), ['accept', '3', '--from', 'claude-code', '--json'])
  assert.deepEqual(argvFor('start', [3], 'vscode'), ['accept', '3', '--json'])
  assert.equal(argvFor('done', ['1; rm -rf /'], 'vscode'), null)
  assert.equal(argvFor('keep', [1, 2], 'vscode'), null)
  assert.equal(argvFor('done', [], 'vscode'), null)
})

test('할 일 제목은 정리해서 -- 뒤에 둔다', () => {
  assert.deepEqual(addArgv('--force\n지우기'), ['add', '--json', '--', '--force 지우기'])
  assert.equal(addArgv('   '), null)
  assert.equal(addArgv(1), null)
})

test('응답은 계약 판과 JSON 한 덩어리여야 한다', () => {
  assert.equal(readReply({ exitCode: 0, stdout: '{"schema":1,"ok":true}', fault: null }).ok, true)
  assert.deepEqual(readReply({ exitCode: 0, stdout: '{"schema":2,"ok":true}', fault: null }), { ok: false, failure: { code: 'schema', message: 'kairos와 호스트의 계약 판이 다릅니다' } })
  const failed = readReply({ exitCode: 1, stdout: '{"schema":1,"ok":false,"error":{"code":"gate_closed","message":"닫힘"}}', fault: null })
  assert.deepEqual(failed, { ok: false, failure: { code: 'gate_closed', message: '닫힘' } })
  assert.equal(readReply({ exitCode: 0, stdout: 'hello', fault: null }).ok, false)
})

test('호출은 큐로 한 번에 하나만 돈다', async () => {
  const order: string[] = []
  const exec: Exec = async (_file, args) => {
    order.push(`start ${args[0]}`)
    await new Promise(resolve => setTimeout(resolve, 10))
    order.push(`end ${args[0]}`)
    return { exitCode: 0, stdout: '{"schema":1,"ok":true}', fault: null }
  }
  const client = createClient({ surface: 'vscode', binary: '/x/kairos', exec, checkBinary: false })
  await Promise.all([client.next(), client.list()])
  assert.deepEqual(order, ['start next', 'end next', 'start list', 'end list'])
})

// Windows는 권한 비트 대신 자리로 본다(위 시험).
test('남이 쓸 수 있는 실행 파일은 부르지 않는다', { skip: process.platform === 'win32' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'kairos-client-'))
  const file = join(dir, 'kairos')
  writeFileSync(file, '#!/bin/sh\n')
  chmodSync(file, 0o755)
  assert.equal(checkBinary(file), null)
  chmodSync(file, 0o777)
  assert.equal(checkBinary(file)?.code, 'binary_unsafe')
  assert.equal(checkBinary(join(dir, 'none'))?.code, 'binary_missing')
})

test('띄울 명령은 모양이 맞을 때만 받는다', () => {
  const runId = '11111111-2222-4333-8444-555555555555'
  const good = { launch: { cwd: '/w/t-1', argv: ['/k/bin/kairos-run', runId], command: 'x' } }
  assert.deepEqual(launchOf(good)?.argv, ['/k/bin/kairos-run', runId])
  assert.equal(launchOf({ launch: { ...good.launch, argv: ['/bin/sh', runId] } }), null)
  assert.equal(launchOf({ launch: { ...good.launch, argv: ['/k/bin/kairos-run', 'x; y'] } }), null)
  assert.equal(launchOf({}), null)
})
