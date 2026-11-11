// scripts/check-personal-data.sh 시험. 임시 git 저장소에 스크립트를 복사하고 합성 값만으로 돌린다.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

const script = new URL('../check-personal-data.sh', import.meta.url).pathname
// 걸려야 하는 값은 이어 붙여 만든다. 이 파일 자체가 저장소 검사에 걸리지 않게 한다.
const corp = ['real', 'corp'].join('')
const at = '@'

/** files: 경로 → 내용. patterns: 개인 규칙 줄(없으면 파일을 만들지 않는다). */
function check(files, patterns, links = {}) {
  const dir = mkdtempSync(join(tmpdir(), 'kairos-personal-'))
  try {
    const git = (...args) => spawnSync('git', ['-C', dir, ...args], { encoding: 'utf8' })
    git('init', '-q')
    mkdirSync(join(dir, 'scripts'))
    copyFileSync(script, join(dir, 'scripts/check-personal-data.sh'))
    for (const [path, content] of Object.entries(files)) {
      mkdirSync(join(dir, path, '..'), { recursive: true })
      writeFileSync(join(dir, path), content)
    }
    mkdirSync(join(dir, 'linked'))
    symlinkSync('linked', join(dir, 'link'))
    for (const [path, target] of Object.entries(links)) symlinkSync(target, join(dir, path))
    git('add', '-A')
    const env = { PATH: process.env.PATH, HOME: dir }
    if (patterns) {
      writeFileSync(join(dir, 'patterns'), patterns.join('\n') + '\n')
      env.KAIROS_PERSONAL_PATTERNS = join(dir, 'patterns')
    }
    const result = spawnSync('sh', [join(dir, 'scripts/check-personal-data.sh')], { encoding: 'utf8', env })
    return { status: result.status, output: result.stdout + result.stderr }
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

test('합성 값만 있으면 통과하고 폴더 링크는 건너뛴다', () => {
  const result = check({ 'a.txt': 'U0SYNTHAAA1 a@contoso.example /Users/synth x@sub.example.com\n' }, ['realcorp'])
  assert.equal(result.status, 0, result.output)
  assert.match(result.output, /개인 규칙 1개/)
})

test('개인 규칙은 대소문자와 구분자를 바꾼 꼴도 잡는다', () => {
  for (const content of ['fn real_corp_test() {}\n', 'RealCorp\n', 'real.corp\n']) {
    const result = check({ 'a.rs': content }, ['real-corp'])
    assert.equal(result.status, 1, `${content}: ${result.output}`)
  }
})

test('잘못된 개인 규칙은 통과로 치지 않는다', () => {
  const result = check({ 'a.txt': 'nothing\n' }, ['(unbalanced'])
  assert.equal(result.status, 2, result.output)
})

test('가리킨 개인 규칙 파일이 없으면 통과로 치지 않는다', () => {
  const dir = mkdtempSync(join(tmpdir(), 'kairos-personal-'))
  try {
    spawnSync('git', ['-C', dir, 'init', '-q'])
    mkdirSync(join(dir, 'scripts'))
    copyFileSync(script, join(dir, 'scripts/check-personal-data.sh'))
    const env = { PATH: process.env.PATH, HOME: dir, KAIROS_PERSONAL_PATTERNS: join(dir, 'missing') }
    const result = spawnSync('sh', [join(dir, 'scripts/check-personal-data.sh')], { encoding: 'utf8', env })
    assert.equal(result.status, 2, result.stdout + result.stderr)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('합성 표지가 같은 줄에 있어도 실제 모양의 값은 잡는다', () => {
  assert.equal(check({ 'a.txt': `U0SYNTHAAA1 ${'U0' + 'REALAAA12'}\n` }).status, 1)
  assert.equal(check({ 'a.txt': `a@contoso.example b${at}${corp}.co.kr\n` }).status, 1)
})

test('예약 도메인 허용은 주소 끝까지 맞아야 한다', () => {
  for (const domain of [`example.com.${corp}.kr`, `test.${corp}.com`, `google.com.${corp}.kr`]) {
    const address = `x${at}${domain}`
    const result = check({ 'a.txt': `${address}\n` })
    assert.equal(result.status, 1, `${address}: ${result.output}`)
  }
  assert.equal(check({ 'a.txt': 'A@Example.COM Mixed@Example.Invalid\n' }).status, 0)
})

test('한 줄에 이어 붙인 개인 키 본문도 잡고 16진수 해시는 둔다', () => {
  const body = Array.from({ length: 64 }, (_, i) => 'Ab9+/Zq'[i % 7]).join('')
  const header = ['-----BEGIN', 'PRIVATE', 'KEY-----'].join(' ')
  assert.equal(check({ 'a.ts': `const key = "${header}\\n${body}\\n"\n` }).status, 1)
  assert.equal(check({ 'a.ts': `  '${body}',\n` }).status, 1)
  assert.equal(check({ 'a.sql': `'${'0123456789abcdef'.repeat(4)}'\n` }).status, 0)
})

test('서비스 도메인 메일은 알림 주소 모양만 둔다', () => {
  for (const address of [`1234+login${at}users.noreply.github.com`, `real.person${at}google.com`]) {
    assert.equal(check({ 'a.txt': `${address}\n` }).status, 1, address)
  }
  assert.equal(check({ 'a.txt': 'git@github.com notify@mail.notion.com noreply@sentry.io\n' }).status, 0)
})

test('심볼릭 링크의 대상 문자열도 본다', () => {
  const result = check({}, undefined, { 'escape': ['', 'Users', 'realperson', 'x'].join('/') })
  assert.equal(result.status, 1, result.output)
})

test('파일 목록을 얻지 못하면 통과로 치지 않는다', () => {
  const dir = mkdtempSync(join(tmpdir(), 'kairos-personal-'))
  try {
    spawnSync('git', ['-C', dir, 'init', '-q'])
    mkdirSync(join(dir, 'scripts'))
    copyFileSync(script, join(dir, 'scripts/check-personal-data.sh'))
    const env = { PATH: process.env.PATH, HOME: dir, GIT_DIR: join(dir, 'missing') }
    const result = spawnSync('sh', [join(dir, 'scripts/check-personal-data.sh')], { encoding: 'utf8', env })
    assert.equal(result.status, 2, result.stdout + result.stderr)
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})
