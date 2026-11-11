// 하네스 구성 시험. 에이전트와 스킬의 frontmatter, 문서가 가리키는 명령과 파일이 실제로 있는지 본다.
import assert from 'node:assert/strict'
import { existsSync, lstatSync, readdirSync, readFileSync, readlinkSync } from 'node:fs'
import { join } from 'node:path'
import { test } from 'node:test'

const root = new URL('../../', import.meta.url).pathname

function frontmatter(file) {
  const text = readFileSync(file, 'utf8')
  const match = /^---\n([\s\S]*?)\n---\n/.exec(text)
  assert.ok(match, `${file}: frontmatter 없음`)
  return Object.fromEntries(
    match[1].split('\n').filter(line => /^[a-zA-Z-]+:/.test(line)).map(line => [line.slice(0, line.indexOf(':')), line.slice(line.indexOf(':') + 1).trim()]),
  )
}

test('에이전트는 이름, 설명, 읽기 전용 도구를 갖는다', () => {
  const dir = join(root, '.claude/agents')
  for (const file of readdirSync(dir)) {
    const meta = frontmatter(join(dir, file))
    assert.equal(meta.name, file.replace(/\.md$/, ''), file)
    assert.ok(meta.description?.length > 20, file)
    assert.match(meta.disallowedTools ?? '', /Edit/, `${file}: 리뷰어는 고치지 않는다`)
  }
})

test('스킬은 .agents/skills가 원본이고 .claude/skills는 그 링크다', () => {
  const link = join(root, '.claude/skills')
  assert.ok(lstatSync(link).isSymbolicLink())
  assert.equal(readlinkSync(link), '../.agents/skills')
  for (const name of readdirSync(join(root, '.agents/skills'))) {
    const meta = frontmatter(join(root, '.agents/skills', name, 'SKILL.md'))
    assert.equal(meta.name, name)
    assert.ok(meta.description?.length > 20, name)
  }
})

test('AGENTS.md가 말하는 스킬과 스크립트가 있다', () => {
  const agents = readFileSync(join(root, 'AGENTS.md'), 'utf8')
  for (const [, name] of agents.matchAll(/`\/([a-z-]+)[^`]*`/g)) {
    assert.ok(existsSync(join(root, '.agents/skills', name, 'SKILL.md')), name)
  }
  for (const script of ['scripts/verify.sh', 'scripts/check-personal-data.sh']) assert.ok(existsSync(join(root, script)), script)
  assert.match(readFileSync(join(root, 'CLAUDE.md'), 'utf8'), /^@AGENTS\.md\n/)
})

test('CI는 하네스 시험도 돌린다', () => {
  const ci = readFileSync(join(root, '.github/workflows/ci.yml'), 'utf8')
  assert.match(ci, /scripts\/verify\.sh ts/)
  assert.match(ci, /scripts\/verify\.sh rust/)
})
