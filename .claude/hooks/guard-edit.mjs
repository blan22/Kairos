// PreToolUse(Edit|Write|MultiEdit|NotebookEdit). 생성 파일, golden 고정값, 운영 폴더를 지킨다.
import { decide, context, input, PROTECTED_DIRS, repoPath } from './lib.mjs'

const GENERATED = new Map([
  ['storybook/src/stories/OrcaPanel.stories.tsx', 'scripts/gen-stories.mjs가 만든다. panel.check.ts를 고친 뒤 npm --prefix storybook run gen을 돌린다'],
  ['plugins/collector/hooks/allowed.ts', '설치기의 collector_allowlist()와 같아야 한다(kairos-ops 시험). 커넥터 목록(kairos-core/src/connector.rs)을 고치고 생성 결과를 옮긴다'],
  ['Cargo.lock', 'cargo가 쓴다. 의존성은 Cargo.toml로 바꾼다'],
  ['storybook/package-lock.json', 'npm이 쓴다'],
])
const SECURITY = [
  'crates/kairos-sessions/src/profile.rs',
  'crates/kairos-sessions/src/workspace.rs',
  'crates/kairos-sessions/src/keychain.rs',
  'crates/kairos-config/src/values.rs',
  'crates/kairos-core/src/connector.rs',
  'plugins/collector/hooks/tick.ts',
  'plugins/collector/hooks/register.ts',
  'crates/kairos-cli/src/commands/mcp.rs',
]

const payload = input()
const file = payload.tool_input?.file_path ?? payload.tool_input?.notebook_path
if (typeof file !== 'string' || file === '') process.exit(0)
const { absolute, rel, inside } = repoPath(file)

if (PROTECTED_DIRS().some(dir => absolute === dir || absolute.startsWith(`${dir}/`))) {
  decide('PreToolUse', 'deny', '운영 데이터 폴더와 LaunchAgents는 에이전트가 고치지 않는다. 시험은 임시 KAIROS_HOME을 쓴다')
}
if (!inside) process.exit(0)
const generated = GENERATED.get(rel)
if (generated) decide('PreToolUse', 'deny', `생성 파일이다: ${generated}`)
if (/^crates\/[^/]+\/tests\/fixtures\/golden\//.test(rel)) {
  decide('PreToolUse', 'ask', 'golden은 지운 TS 구현의 마지막 출력이라 다시 만들 수 없다. 출력이 바뀌는 것이 의도인지 사용자 확인을 받는다')
}
if (SECURITY.includes(rel)) {
  context('PreToolUse', `보안 경계 파일(${rel})이다. SECURITY.md의 바닥값을 넓히지 않는지 보고, 끝나면 security-reviewer 에이전트로 검토한다`)
}
process.exit(0)
