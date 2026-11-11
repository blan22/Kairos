// 하네스 훅 시험. 훅을 실제처럼 stdin JSON으로 띄우고 결정을 본다.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

const root = new URL('../../', import.meta.url).pathname
const hook = name => join(root, '.claude', 'hooks', name)

function run(name, payload, env = {}) {
  const result = spawnSync('node', [hook(name)], {
    input: JSON.stringify(payload),
    encoding: 'utf8',
    env: { ...process.env, CLAUDE_PROJECT_DIR: root, ...env },
  })
  const decision = result.stdout ? JSON.parse(result.stdout).hookSpecificOutput : null
  return { status: result.status, decision, stderr: result.stderr }
}

const bash = command => run('guard-bash.mjs', { tool_name: 'Bash', tool_input: { command } })
const edit = file_path => run('guard-edit.mjs', { tool_name: 'Edit', tool_input: { file_path } })

test('운영을 바꾸는 명령은 막고 평범한 명령은 지나간다', () => {
  for (const command of [
    'launchctl bootout gui/501/dev.kairos.collect',
    'kairos uninstall',
    'kairos next --seen --json',
    '"$HOME/Library/Application Support/kairos/current/bin/kairos" sessions on',
    'orca plugin shot local.kairos',
    'orca plugin dev load plugins/orca-panel',
    'git push --force origin main',
    'rm -rf "$HOME/Library/Application Support/kairos/logs"',
    'security find-generic-password -s kairos -w',
    'git diff --output=/tmp/x HEAD',
    'git log -p --output /tmp/x',
    'git diff --outp=/tmp/x',
    'git show --ou /tmp/x HEAD',
    'git format-patch -o /tmp/x HEAD~1',
    'git format-patch -o/tmp/x HEAD~1',
    'git format-patch HEAD~1',
    'git format-patch --stdout -o /tmp/x HEAD~1',
    'git diff "--out""put=/tmp/x"',
    'git log --o\\utput /tmp/x',
    'npm --prefix storybook run shots && security find-generic-password -s kairos -w',
    'kairos install --apply; launchctl bootout gui/501/dev.kairos.collect',
  ]) {
    assert.equal(bash(command).decision?.permissionDecision, 'deny', command)
  }
  for (const command of ['kairos install --apply', 'orca plugin shot plugins/orca-panel --scenario empty', 'npm --prefix storybook run shots']) {
    assert.equal(bash(command).decision?.permissionDecision, 'ask', command)
  }
  for (const command of ['cargo test -p kairos-config', 'kairos uninstall --dry-run', 'git status', 'kairos next --json', 'git log --oneline -5', 'git diff --stat', 'git format-patch --stdout HEAD~1']) {
    const result = bash(command)
    assert.equal(result.status, 0)
    assert.equal(result.decision, null, command)
  }
})

test('생성 파일과 운영 폴더 편집은 막고 golden은 묻는다', () => {
  assert.equal(edit('storybook/src/stories/OrcaPanel.stories.tsx').decision?.permissionDecision, 'deny')
  assert.equal(edit(join(root, 'plugins/collector/hooks/allowed.ts')).decision?.permissionDecision, 'deny')
  assert.equal(edit(join(homedir(), 'Library/Application Support/kairos/config.toml')).decision?.permissionDecision, 'deny')
  assert.equal(edit('crates/kairos-tick/tests/fixtures/golden/expected.json').decision?.permissionDecision, 'ask')
  const security = edit('crates/kairos-sessions/src/profile.rs').decision
  assert.equal(security?.permissionDecision, undefined)
  assert.match(security?.additionalContext ?? '', /security-reviewer/)
  assert.equal(edit('crates/kairos-cli/src/lib.rs').decision, null)
})

test('Stop 훅은 이미 이어지는 중이면 다시 막지 않는다', () => {
  assert.equal(run('stop-verify.mjs', { stop_hook_active: true }).status, 0)
  assert.equal(run('stop-verify.mjs', {}, { KAIROS_HARNESS_STOP: '0' }).status, 0)
})

test('세션 시작 안내는 시각과 검증 명령을 준다', () => {
  const { decision } = run('session-start.mjs', {})
  assert.equal(decision?.hookEventName, 'SessionStart')
  assert.match(decision?.additionalContext ?? '', /KST/)
  assert.match(decision?.additionalContext ?? '', /scripts\/verify\.sh/)
})

test('설정의 훅은 있는 파일만 가리킨다', () => {
  const settings = JSON.parse(readFileSync(join(root, '.claude/settings.json'), 'utf8'))
  const commands = Object.values(settings.hooks).flat().flatMap(group => group.hooks.map(entry => entry.command))
  for (const command of commands) {
    const file = /hooks\/([a-z-]+\.mjs)/.exec(command)?.[1]
    assert.ok(file, command)
    assert.doesNotThrow(() => readFileSync(hook(file)), file)
  }
})
