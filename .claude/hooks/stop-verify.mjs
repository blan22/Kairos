// Stop. 이번에 Rust나 TS를 고쳤으면 scripts/verify.sh fast를 돌린다. 실패하면 exit 2로 턴을 잇게 하고 출력 끝을 넘긴다.
// stop_hook_active면(이미 이 훅 때문에 이어지는 중) 다시 막지 않는다. KAIROS_HARNESS_STOP=0이면 건너뛴다.
import { spawnSync } from 'node:child_process'
import { input, projectDir } from './lib.mjs'

const payload = input()
if (payload.stop_hook_active === true || process.env.KAIROS_HARNESS_STOP === '0') process.exit(0)
const cwd = projectDir()
const status = spawnSync('git', ['status', '--porcelain', '--untracked-files=all'], { cwd, encoding: 'utf8' })
const changed = (status.stdout ?? '').split('\n').map(line => line.slice(3)).filter(Boolean)
if (!changed.some(file => /\.(rs|ts|tsx|mjs|toml)$/.test(file))) process.exit(0)
const run = spawnSync('scripts/verify.sh', ['fast'], { cwd, encoding: 'utf8', timeout: 600_000 })
if (run.status === 0) process.exit(0)
const output = `${run.stdout ?? ''}${run.stderr ?? ''}`.split('\n').slice(-40).join('\n')
process.stderr.write(`scripts/verify.sh fast가 실패했다. 고친 뒤 끝낸다(원래 있던 실패라고 가정하지 않는다).\n${output}\n`)
process.exit(2)
