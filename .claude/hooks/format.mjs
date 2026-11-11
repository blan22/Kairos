// PostToolUse(Edit|Write|MultiEdit). 고친 Rust 파일 하나만 rustfmt로 맞춘다. 실패해도 막지 않는다(Stop 훅의 fmt 검사가 잡는다).
import { spawnSync } from 'node:child_process'
import { input, projectDir, repoPath } from './lib.mjs'

const file = input().tool_input?.file_path
if (typeof file === 'string' && file.endsWith('.rs')) {
  const { absolute, inside } = repoPath(file)
  if (inside) spawnSync('rustfmt', ['--edition', '2024', '--config-path', projectDir(), absolute], { stdio: 'ignore', timeout: 30_000 })
}
process.exit(0)
