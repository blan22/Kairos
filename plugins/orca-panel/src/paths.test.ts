import { assert, test } from 'orca/testing'
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { findOrca } from './paths.ts'

test('orca는 PATH의 절대 경로 항목에서 남이 고칠 수 없는 파일만 고른다', () => {
  if (process.platform === 'win32') return
  const root = mkdtempSync(join(tmpdir(), 'kairos-orca-'))
  try {
    const uid = process.getuid?.() ?? null
    const place = (name: string, mode: number): string => {
      const dir = join(root, name)
      mkdirSync(dir)
      const file = join(dir, 'orca')
      writeFileSync(file, '#!/bin/sh\n')
      chmodSync(file, mode)
      return dir
    }
    const shared = place('shared', 0o777)
    const plain = place('plain', 0o644)
    const good = place('good', 0o755)
    // 상대 경로와 빈 항목은 보지 않는다. 남이 쓸 수 있거나 실행 권한이 없는 파일은 건너뛴다.
    assert.equal(findOrca('linux', ['', '.', 'good', shared, plain, good].join(':'), uid), join(good, 'orca'))
    assert.equal(findOrca('linux', [shared, plain].join(':'), uid), null)
    // 다른 사용자 소유는 받지 않는다.
    assert.equal(findOrca('linux', good, uid === null ? null : uid + 1), uid === 0 ? join(good, 'orca') : null)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('Windows는 orca.exe만 찾는다', () => {
  assert.equal(findOrca('win32', '', null), null)
  assert.equal(findOrca('win32', 'relative;.', null), null)
})
