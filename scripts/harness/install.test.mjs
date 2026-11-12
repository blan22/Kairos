// scripts/install.sh와 scripts/install-release.sh 시험. 가짜 저장소, 가짜 도구, 임시 HOME으로 돌린다.
// 실제 ~/.zshrc, 운영 데이터 폴더, cargo, gh, claude, 네트워크를 쓰지 않는다. kairos는 받은 인자를 찍는 가짜다.
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  appendFileSync,
  chmodSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

const scripts = new URL('..', import.meta.url).pathname
const asset = 'kairos-aarch64-apple-darwin.tar.gz'
// 설치 스크립트는 Apple Silicon Mac에서만 돈다. 다른 러너에서는 건너뛴다.
const skip = process.platform === 'darwin' && process.arch === 'arm64' ? false : 'Apple Silicon Mac에서만 돈다'

function executable(path, body) {
  mkdirSync(join(path, '..'), { recursive: true })
  writeFileSync(path, `#!/bin/sh\n${body}\n`)
  chmodSync(path, 0o755)
}

/** 받은 인자를 `ARGS [a] [b]`로 찍는다. FAKE_INSTALL이 있으면 설치본 자리에 자신을 놓는다. */
const fakeKairos = [
  'printf ARGS; for arg in "$@"; do printf " [%s]" "$arg"; done; echo',
  'if [ -n "${FAKE_INSTALL:-}" ]; then mkdir -p "$KAIROS_HOME/current/bin"; cp "$0" "$KAIROS_HOME/current/bin/kairos"; fi',
].join('\n')

/** 임시 폴더에 가짜 도구, 소스 저장소, 릴리스 묶음을 둔다. */
function world() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'kairos-install-')))
  const stubs = join(root, 'stubs')
  for (const name of ['cargo', 'gh', 'claude', 'xcode-select']) executable(join(stubs, name), 'exit 0')
  const runtime = join(root, 'runtime-tools')
  for (const name of ['gh', 'claude']) executable(join(runtime, name), 'exit 0')
  const repo = join(root, 'repo')
  mkdirSync(join(repo, 'scripts'), { recursive: true })
  copyFileSync(join(scripts, 'install.sh'), join(repo, 'scripts/install.sh'))
  writeFileSync(join(repo, 'Cargo.toml'), '')
  executable(join(repo, 'ops/notifier/build.sh'), 'exit 0')
  executable(join(repo, 'target/release/kairos'), fakeKairos)
  const bundle = join(root, 'bundle/kairos-9.9.9')
  mkdirSync(join(bundle, 'scripts'), { recursive: true })
  copyFileSync(join(scripts, 'install.sh'), join(bundle, 'scripts/install.sh'))
  executable(join(bundle, 'bin/kairos'), fakeKairos)
  writeFileSync(join(bundle, 'VERSION'), '9.9.9\n')
  mkdirSync(join(root, 'tmp'))
  return {
    root,
    repo,
    bundle,
    source: `${stubs}:/usr/bin:/bin`,
    runtime: `${runtime}:/usr/bin:/bin`,
    done: () => rmSync(root, { recursive: true, force: true }),
  }
}

function run(w, script, args, { home = 'home', path = w.source, env = {} } = {}) {
  const dir = join(w.root, home)
  mkdirSync(dir, { recursive: true })
  const result = spawnSync('/bin/zsh', ['-f', script, ...args], {
    encoding: 'utf8',
    input: '',
    env: { HOME: dir, KAIROS_HOME: join(dir, 'data'), SHELL: '/bin/zsh', PATH: path, TMPDIR: join(w.root, 'tmp'), ...env },
  })
  return { status: result.status, output: result.stdout + result.stderr, home: dir }
}

const pathLines = (home) =>
  readFileSync(join(home, '.zshrc'), 'utf8')
    .split('\n')
    .filter((line) => line.startsWith('export PATH=')).length

test('--yes면 PATH 줄을 한 번만 더하고 setup으로 넘긴다', { skip }, () => {
  const w = world()
  try {
    const script = join(w.repo, 'scripts/install.sh')
    const first = run(w, script, ['--yes', '--google', 'me@contoso.example'])
    assert.equal(first.status, 0, first.output)
    assert.match(first.output, /ARGS \[setup\] \[--yes\] \[--google\] \[me@contoso\.example\]\n$/)
    assert.equal(pathLines(first.home), 1)
    const record = join(first.home, 'data/install-source')
    assert.equal(readFileSync(record, 'utf8'), `source:${w.repo}\n`)
    assert.equal(statSync(record).mode & 0o777, 0o600)
    assert.equal(statSync(join(first.home, 'data')).mode & 0o777, 0o700)
    assert.equal(readlinkSync(join(first.home, '.local/bin/kairos')), join(first.home, 'data/current/bin/kairos'))
    // 다시 실행해도 줄이 늘지 않는다.
    const again = run(w, script, ['--yes'])
    assert.equal(again.status, 0, again.output)
    assert.match(again.output, /이미 ~\/\.local\/bin이 있습니다/)
    assert.equal(pathLines(again.home), 1)
    assert.deepEqual(readdirSync(join(first.home, 'data')), ['install-source'])
  } finally {
    w.done()
  }
})

test('묻지 못하거나 --no-modify-path면 셸 설정을 건드리지 않고, 빈 인자를 지킨다', { skip }, () => {
  const w = world()
  try {
    const script = join(w.repo, 'scripts/install.sh')
    // --yes가 없고 터미널이 아니다. 더할 줄만 알려 준다.
    const quiet = run(w, script, ['--org', '/synthetic/org.toml', '--sha256', ''], { home: 'quiet' })
    assert.equal(quiet.status, 0, quiet.output)
    assert.match(quiet.output, /ARGS \[setup\] \[--org\] \[\/synthetic\/org\.toml\] \[--sha256\] \[\]\n$/)
    assert.match(quiet.output, /셸 설정에 다음 줄을 더하세요/)
    assert.equal(existsSync(join(quiet.home, '.zshrc')), false)
    // --no-modify-path는 setup으로 넘어가지 않는다.
    const kept = run(w, script, ['--yes', '--no-modify-path'], { home: 'kept' })
    assert.match(kept.output, /ARGS \[setup\] \[--yes\]\n$/)
    assert.equal(existsSync(join(kept.home, '.zshrc')), false)
    // PATH에 이미 있으면 아무 말도 하지 않는다. SHELL이 없어도 멈추지 않는다.
    const present = run(w, script, ['--yes'], {
      home: 'present',
      path: `${join(w.root, 'present/.local/bin')}:${w.source}`,
      env: { SHELL: undefined },
    })
    assert.equal(present.status, 0, present.output)
    assert.doesNotMatch(present.output, /PATH/)
    const bare = run(w, script, ['--yes'], { home: 'bare', env: { SHELL: undefined } })
    assert.equal(bare.status, 0, bare.output)
    assert.equal(existsSync(join(bare.home, '.zshrc')), false)
  } finally {
    w.done()
  }
})

test('빠진 도구를 한 번에 모아 알리고 아무것도 쓰지 않는다', { skip }, () => {
  const w = world()
  try {
    const empty = join(w.root, 'empty')
    mkdirSync(empty)
    // uname, sed 같은 기본 도구는 절대 경로나 셸 내장으로 부른다. PATH가 비어도 도구 확인까지 간다.
    const result = run(w, join(w.repo, 'scripts/install.sh'), ['--yes'], { path: `${empty}:/usr/bin:/bin` })
    assert.equal(result.status, 1, result.output)
    assert.match(result.output, /멈춤: 아래 \d가지를 갖춘 뒤 다시 실행하세요/)
    for (const name of ['Rust:', 'gh:', 'claude:']) assert.ok(result.output.includes(`  - ${name}`), result.output)
    assert.equal(existsSync(join(result.home, '.local')), false)
    assert.equal(existsSync(join(result.home, 'data')), false)
  } finally {
    w.done()
  }
})

test('릴리스 묶음은 빌드하지 않고, 설치하지 않았으면 알리고 링크를 남기지 않는다', { skip }, () => {
  const w = world()
  try {
    const script = join(w.bundle, 'scripts/install.sh')
    // cargo와 xcode-select가 없어도 된다.
    const skipped = run(w, script, ['--yes', '--no-modify-path'], { path: w.runtime })
    assert.equal(skipped.status, 0, skipped.output)
    assert.match(skipped.output, /릴리스 묶음이라 빌드하지 않습니다/)
    assert.match(skipped.output, /ARGS \[setup\] \[--yes\]\n/)
    assert.match(skipped.output, /설치하지 않아 kairos 명령이 없습니다/)
    assert.equal(readFileSync(join(skipped.home, 'data/install-source'), 'utf8'), 'release\n')
    assert.throws(() => lstatSync(join(skipped.home, '.local/bin/kairos')))
    // 설치했으면 링크가 남고 안내는 없다.
    const installed = run(w, script, ['--yes', '--no-modify-path'], {
      home: 'installed',
      path: w.runtime,
      env: { FAKE_INSTALL: '1' },
    })
    assert.equal(installed.status, 0, installed.output)
    assert.doesNotMatch(installed.output, /설치하지 않아/)
    assert.ok(lstatSync(join(installed.home, '.local/bin/kairos')).isSymbolicLink())
    // 묶음은 당길 저장소가 없다.
    const update = run(w, script, ['--update'], { home: 'update', path: w.runtime })
    assert.equal(update.status, 1, update.output)
    assert.match(update.output, /릴리스 설치는 설치 명령을 다시 실행하면/)
  } finally {
    w.done()
  }
})

test('--update는 변경이 있으면 멈추고, 당긴 뒤 --yes를 스스로 붙이지 않는다', { skip }, () => {
  const w = world()
  try {
    const git = (dir, ...args) => {
      const options = ['-c', 'user.name=t', '-c', 'user.email=t@contoso.example', '-c', 'init.defaultBranch=main']
      const result = spawnSync('/usr/bin/git', [...options, '-C', dir, ...args], { encoding: 'utf8' })
      assert.equal(result.status, 0, result.stderr)
    }
    const origin = join(w.root, 'origin.git')
    mkdirSync(origin)
    git(origin, 'init', '-q', '--bare')
    git(w.repo, 'init', '-q')
    writeFileSync(join(w.repo, '.gitignore'), 'target/\n')
    git(w.repo, 'add', '-A')
    git(w.repo, 'commit', '-qm', 'first')
    git(w.repo, 'remote', 'add', 'origin', origin)
    git(w.repo, 'push', '-q', '-u', 'origin', 'main')
    const script = join(w.repo, 'scripts/install.sh')

    writeFileSync(join(w.repo, 'dirty.txt'), 'x')
    const dirty = run(w, script, ['--update'])
    assert.equal(dirty.status, 1, dirty.output)
    assert.match(dirty.output, /커밋하지 않은 변경이 있습니다/)
    assert.doesNotMatch(dirty.output, /ARGS/)
    rmSync(join(w.repo, 'dirty.txt'))

    // 다른 복제본이 새 판을 올린다.
    const other = join(w.root, 'other')
    git(w.root, 'clone', '-q', origin, other)
    appendFileSync(join(other, 'scripts/install.sh'), '# 새 판\n')
    git(other, 'commit', '-qam', 'second')
    git(other, 'push', '-q')

    const updated = run(w, script, ['--update', '--google', 'me@contoso.example'])
    assert.equal(updated.status, 0, updated.output)
    // 설치와 패널 승인은 사람이 답한다. 당긴 스크립트로 다시 시작하고 ~/.zshrc는 건드리지 않는다.
    // git의 표준 오류가 뒤에 붙으므로 줄 끝까지만 본다.
    assert.match(updated.output, /ARGS \[setup\] \[--google\] \[me@contoso\.example\]\n/)
    assert.ok(readFileSync(script, 'utf8').endsWith('# 새 판\n'))
    assert.equal(existsSync(join(updated.home, '.zshrc')), false)
    const unattended = run(w, script, ['--update', '--yes'])
    assert.match(unattended.output, /ARGS \[setup\] \[--yes\]\n/)
  } finally {
    w.done()
  }
})

test('릴리스 설치는 SHA256이 맞을 때만 풀고 임시 폴더를 남기지 않는다', { skip }, () => {
  const w = world()
  try {
    const mirror = (name) => {
      const dir = join(w.root, name)
      mkdirSync(dir)
      const packed = spawnSync('/usr/bin/tar', ['-czf', join(dir, asset), '-C', join(w.bundle, '..'), 'kairos-9.9.9'])
      assert.equal(packed.status, 0)
      const sum = spawnSync('/usr/bin/shasum', ['-a', '256', join(dir, asset)], { encoding: 'utf8' }).stdout.split(' ')[0]
      writeFileSync(join(dir, `${asset}.sha256`), `${sum}\n`)
      return dir
    }
    const script = join(scripts, 'install-release.sh')
    const install = (dir, home) =>
      run(w, script, ['--yes', '--no-modify-path'], {
        home,
        path: w.runtime,
        env: { KAIROS_RELEASE_URL: `file://${dir}`, FAKE_INSTALL: '1' },
      })

    const good = install(mirror('good'), 'good-home')
    assert.equal(good.status, 0, good.output)
    assert.match(good.output, /SHA256 확인 [0-9a-f]{64}\n {2}kairos 9\.9\.9\n/)
    assert.match(good.output, /ARGS \[setup\] \[--yes\]\n$/)
    assert.equal(readFileSync(join(good.home, 'data/install-source'), 'utf8'), 'release\n')
    assert.deepEqual(readdirSync(join(w.root, 'tmp')), [])

    // 묶음이 바뀌었다.
    const changed = mirror('changed')
    appendFileSync(join(changed, asset), 'x')
    const refused = install(changed, 'changed-home')
    assert.equal(refused.status, 1, refused.output)
    assert.match(refused.output, /SHA256이 다릅니다\. 받은 파일을 쓰지 않습니다/)
    // SHA256 파일의 모양이 틀렸다.
    const odd = mirror('odd')
    writeFileSync(join(odd, `${asset}.sha256`), 'not-a-hash\n')
    assert.match(install(odd, 'odd-home').output, /SHA256 파일의 모양이 맞지 않습니다/)
    // 묶음이 없다.
    const missing = install(join(w.root, 'none'), 'none-home')
    assert.equal(missing.status, 1, missing.output)
    assert.match(missing.output, /묶음을 받지 못했습니다/)
    for (const home of ['changed-home', 'odd-home', 'none-home']) {
      assert.equal(existsSync(join(w.root, home, 'data')), false, home)
      assert.equal(existsSync(join(w.root, home, '.local')), false, home)
    }
    assert.deepEqual(readdirSync(join(w.root, 'tmp')), [])
  } finally {
    w.done()
  }
})
