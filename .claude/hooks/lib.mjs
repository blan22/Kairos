// 훅 공용. 입력은 stdin의 JSON 한 덩어리다. 결정은 stdout JSON으로 내고, 막을 때는 hookSpecificOutput을 쓴다.
import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, relative, resolve } from 'node:path'

export function input() {
  try {
    return JSON.parse(readFileSync(0, 'utf8') || '{}')
  } catch {
    return {}
  }
}

export const projectDir = () => process.env.CLAUDE_PROJECT_DIR || process.cwd()

/** 운영 데이터 폴더와 launchd 폴더. 에이전트는 여기를 고치지 않는다. */
export const PROTECTED_DIRS = () => [
  join(homedir(), 'Library', 'Application Support', 'kairos'),
  join(homedir(), 'Library', 'LaunchAgents'),
]

export function repoPath(file) {
  const absolute = resolve(projectDir(), file)
  const rel = relative(projectDir(), absolute)
  return { absolute, rel, inside: !rel.startsWith('..') && !rel.startsWith('/') }
}

export function decide(event, decision, reason) {
  process.stdout.write(
    JSON.stringify({ hookSpecificOutput: { hookEventName: event, permissionDecision: decision, permissionDecisionReason: reason } }),
  )
  process.exit(0)
}

export function context(event, text) {
  process.stdout.write(JSON.stringify({ hookSpecificOutput: { hookEventName: event, additionalContext: text } }))
  process.exit(0)
}
