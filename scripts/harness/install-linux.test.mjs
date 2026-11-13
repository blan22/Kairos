// scripts/install-linux.sh와 scripts/install-release-linux.sh 시험. 가짜 저장소, 가짜 도구, 임시 HOME으로 돌린다.
// 실제 셸 설정, 운영 데이터 폴더, cargo, gh, systemd, 네트워크를 쓰지 않는다. kairos는 받은 인자를 찍는 가짜다.
// 러너의 /usr/bin에는 gh, cc, systemctl이 있을 수 있다. PATH는 기본 도구만 건 폴더와 가짜 도구 폴더로만 만든다.
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
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

const scripts = new URL('..', import.meta.url).pathname
const arch = { x64: 'x86_64', arm64: 'aarch64' }[process.arch]
const asset = `kairos-${arch}-unknown-linux-gnu.tar.gz`
const skip = process.platform === 'linux' && arch ? false : 'Linux(x86_64, aarch64)에서만 돈다'
/** 설치 스크립트가 부르는 기본 도구. 이것만 PATH에 건다. */
const basics = [
  'sh', 'uname', 'sed', 'dirname', 'mkdir', 'mktemp', 'chmod', 'mv', 'ln', 'readlink', 'rm', 'cat', 'cp',
  'git', 'curl', 'sha256sum', 'head', 'cut', 'tar', 'gzip',
]

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

function which(name) {
  const found = spawnSync('/bin/sh', ['-c', `command -v ${name}`], { encoding: 'utf8' }).stdout.trim()
  assert.ok(found.startsWith('/'), `${name}가 없다`)
  return found
}

/** 임시 폴더에 기본 도구, 가짜 도구, 소스 저장소, 릴리스 묶음을 둔다. */
function world() {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'kairos-install-linux-')))
  const base = join(root, 'base')
  mkdirSync(base)
  for (const name of basics) symlinkSync(which(name), join(base, name))
  const stubs = join(root, 'stubs')
  for (const name of ['cargo', 'cc', 'gh', 'systemctl']) executable(join(stubs, name), 'exit 0')
  const runtime = join(root, 'runtime-tools')
  for (const name of ['gh', 'systemctl']) executable(join(runtime, name), 'exit 0')
  const repo = join(root, 'repo')
  mkdirSync(join(repo, 'scripts'), { recursive: true })
  copyFileSync(join(scripts, 'install-linux.sh'), join(repo, 'scripts/install-linux.sh'))
  writeFileSync(join(repo, 'Cargo.toml'), '')
  executable(join(repo, 'target/release/kairos'), fakeKairos)
  const bundle = join(root, 'bundle/kairos-9.9.9')
  mkdirSync(join(bundle, 'scripts'), { recursive: true })
  copyFileSync(join(scripts, 'install-linux.sh'), join(bundle, 'scripts/install-linux.sh'))
  executable(join(bundle, 'bin/kairos'), fakeKairos)
  writeFileSync(join(bundle, 'VERSION'), '9.9.9\n')
  mkdirSync(join(root, 'tmp'))
  return {
    root,
    repo,
    bundle,
    base,
    source: `${stubs}:${base}`,
    runtime: `${runtime}:${base}`,
    done: () => rmSync(root, { recursive: true, force: true }),
  }
}

function run(w, script, args, { home = 'home', path = w.source, env = {} } = {}) {
  const dir = join(w.root, home)
  mkdirSync(dir, { recursive: true })
  const result = spawnSync(join(w.base, 'sh'), [script, ...args], {
    encoding: 'utf8',
    input: '',
    env: { HOME: dir, KAIROS_HOME: join(dir, 'data'), PATH: path, TMPDIR: join(w.root, 'tmp'), ...env },
  })
  return { status: result.status, output: result.stdout + result.stderr, home: dir }
}

test('소스 설치는 링크와 설치 방식을 남기고 setup으로 넘긴다. 셸 설정은 고치지 않는다', { skip }, () => {
  const w = world()
  try {
    const script = join(w.repo, 'scripts/install-linux.sh')
    const first = run(w, script, ['--yes', '--google', 'me@contoso.example', '--sha256', ''])
    assert.equal(first.status, 0, first.output)
    assert.match(first.output, /ARGS \[setup\] \[--yes\] \[--google\] \[me@contoso\.example\] \[--sha256\] \[\]\n$/)
    const record = join(first.home, 'data/install-source')
    assert.equal(readFileSync(record, 'utf8'), `source:${w.repo}\n`)
    assert.equal(statSync(record).mode & 0o777, 0o600)
    assert.equal(statSync(join(first.home, 'data')).mode & 0o777, 0o700)
    assert.equal(readlinkSync(join(first.home, '.local/bin/kairos')), join(first.home, 'data/current/bin/kairos'))
    assert.match(first.output, /export PATH="\$HOME\/\.local\/bin:\$PATH"/)
    for (const rc of ['.bashrc', '.zshrc', '.profile']) assert.equal(existsSync(join(first.home, rc)), false, rc)
    // PATH에 있으면 알리지 않는다. --no-modify-path는 setup으로 넘어가지 않는다.
    const present = run(w, script, ['--no-modify-path'], { home: 'present', path: `${join(w.root, 'present/.local/bin')}:${w.source}` })
    assert.equal(present.status, 0, present.output)
    assert.doesNotMatch(present.output, /export PATH=/)
    assert.match(present.output, /ARGS \[setup\]\n$/)
  } finally {
    w.done()
  }
})

test('빠진 도구를 한 번에 모아 알리고 아무것도 쓰지 않는다', { skip }, () => {
  const w = world()
  try {
    const result = run(w, join(w.repo, 'scripts/install-linux.sh'), ['--yes'], { path: w.base })
    assert.equal(result.status, 1, result.output)
    assert.match(result.output, /멈춤: 아래를 갖춘 뒤 다시 실행하세요/)
    for (const name of ['Rust:', 'C 컴파일러:', 'gh:', 'systemd:']) assert.ok(result.output.includes(`  - ${name}`), result.output)
    assert.equal(existsSync(join(result.home, '.local')), false)
    assert.equal(existsSync(join(result.home, 'data')), false)
  } finally {
    w.done()
  }
})

test('릴리스 묶음은 빌드하지 않고, 설치하지 않았으면 알리고 링크를 남기지 않는다', { skip }, () => {
  const w = world()
  try {
    const script = join(w.bundle, 'scripts/install-linux.sh')
    const skipped = run(w, script, ['--yes'], { path: w.runtime })
    assert.equal(skipped.status, 0, skipped.output)
    assert.match(skipped.output, /릴리스 묶음이라 빌드하지 않습니다/)
    assert.match(skipped.output, /ARGS \[setup\] \[--yes\]\n/)
    assert.match(skipped.output, /설치하지 않아 kairos 명령이 없습니다/)
    assert.equal(readFileSync(join(skipped.home, 'data/install-source'), 'utf8'), 'release\n')
    assert.throws(() => lstatSync(join(skipped.home, '.local/bin/kairos')))
    const installed = run(w, script, ['--yes'], { home: 'installed', path: w.runtime, env: { FAKE_INSTALL: '1' } })
    assert.equal(installed.status, 0, installed.output)
    assert.doesNotMatch(installed.output, /설치하지 않아/)
    assert.ok(lstatSync(join(installed.home, '.local/bin/kairos')).isSymbolicLink())
    const update = run(w, script, ['--update'], { home: 'update', path: w.runtime })
    assert.equal(update.status, 1, update.output)
    assert.match(update.output, /릴리스 설치는 설치 명령을 다시 실행하면/)
  } finally {
    w.done()
  }
})

test('--update는 변경이 있으면 멈추고, 당긴 스크립트로 다시 시작한다', { skip }, () => {
  const w = world()
  try {
    const git = (dir, ...args) => {
      const options = ['-c', 'user.name=t', '-c', 'user.email=t@contoso.example', '-c', 'init.defaultBranch=main']
      const result = spawnSync(join(w.base, 'git'), [...options, '-C', dir, ...args], { encoding: 'utf8' })
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
    const script = join(w.repo, 'scripts/install-linux.sh')

    writeFileSync(join(w.repo, 'dirty.txt'), 'x')
    const dirty = run(w, script, ['--update'])
    assert.equal(dirty.status, 1, dirty.output)
    assert.match(dirty.output, /커밋하지 않은 변경이 있습니다/)
    assert.doesNotMatch(dirty.output, /ARGS/)
    rmSync(join(w.repo, 'dirty.txt'))

    const other = join(w.root, 'other')
    git(w.root, 'clone', '-q', origin, other)
    appendFileSync(join(other, 'scripts/install-linux.sh'), '# 새 판\n')
    git(other, 'commit', '-qam', 'second')
    git(other, 'push', '-q')

    const updated = run(w, script, ['--update', '--google', 'me@contoso.example'])
    assert.equal(updated.status, 0, updated.output)
    assert.match(updated.output, /ARGS \[setup\] \[--google\] \[me@contoso\.example\]\n/)
    assert.ok(readFileSync(script, 'utf8').endsWith('# 새 판\n'))
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
      const packed = spawnSync(join(w.base, 'tar'), ['-czf', join(dir, asset), '-C', join(w.bundle, '..'), 'kairos-9.9.9'], {
        env: { PATH: w.base },
      })
      assert.equal(packed.status, 0)
      const sum = spawnSync(join(w.base, 'sha256sum'), [join(dir, asset)], { encoding: 'utf8' }).stdout.split(' ')[0]
      writeFileSync(join(dir, `${asset}.sha256`), `${sum}\n`)
      return dir
    }
    const script = join(scripts, 'install-release-linux.sh')
    const install = (dir, home) =>
      run(w, script, ['--yes'], { home, path: w.runtime, env: { KAIROS_RELEASE_URL: `file://${dir}`, FAKE_INSTALL: '1' } })

    const good = install(mirror('good'), 'good-home')
    assert.equal(good.status, 0, good.output)
    assert.match(good.output, /SHA256 확인 [0-9a-f]{64}\n {2}kairos 9\.9\.9\n/)
    assert.match(good.output, /ARGS \[setup\] \[--yes\]\n$/)
    assert.equal(readFileSync(join(good.home, 'data/install-source'), 'utf8'), 'release\n')
    assert.deepEqual(readdirSync(join(w.root, 'tmp')), [])

    const changed = mirror('changed')
    appendFileSync(join(changed, asset), 'x')
    const refused = install(changed, 'changed-home')
    assert.equal(refused.status, 1, refused.output)
    assert.match(refused.output, /SHA256이 다릅니다\. 받은 파일을 쓰지 않습니다/)
    const odd = mirror('odd')
    writeFileSync(join(odd, `${asset}.sha256`), 'not-a-hash\n')
    assert.match(install(odd, 'odd-home').output, /SHA256 파일의 모양이 맞지 않습니다/)
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
