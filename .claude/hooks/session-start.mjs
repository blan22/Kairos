// SessionStart. 지금 시각(KST)과 근무 시간 여부, 브랜치를 알려 준다. 근무 시간에는 운영 수집 틱이 돈다.
import { spawnSync } from 'node:child_process'
import { context, projectDir } from './lib.mjs'

const now = new Date()
const kst = new Intl.DateTimeFormat('ko-KR', { timeZone: 'Asia/Seoul', weekday: 'short', hour: '2-digit', minute: '2-digit', hour12: false }).formatToParts(now)
const part = type => kst.find(entry => entry.type === type)?.value ?? ''
const hour = Number(part('hour'))
const minute = Number(part('minute'))
const weekend = ['토', '일'].includes(part('weekday'))
const minutes = hour * 60 + minute
const working = !weekend && minutes >= 600 && minutes < 1170
const branch = spawnSync('git', ['branch', '--show-current'], { cwd: projectDir(), encoding: 'utf8' }).stdout?.trim() || '(분리된 HEAD)'
const lines = [
  `지금 ${part('weekday')} ${part('hour')}:${part('minute')} KST, 브랜치 ${branch}.`,
  working
    ? '근무 시간이다. 운영 수집 틱이 3분마다 돈다. cargo test --workspace 같은 무거운 빌드는 꼭 필요할 때만 돌린다.'
    : '근무 시간 밖이다. 무거운 빌드를 돌려도 된다.',
  'main에서 바로 커밋하지 않는다. 검증은 scripts/verify.sh fast|full.',
]
context('SessionStart', lines.join('\n'))
