import { assert, test } from 'orca/testing'
import { DANGEROUS_TITLE, SLACK_TITLE } from './fixtures.ts'
import {
  actionFailure,
  attendeesWord,
  carriedWord,
  checksOf,
  clip,
  dayLabel,
  displayTitle,
  eventsWord,
  factsOf,
  failureCopy,
  guideOf,
  holdLine,
  holdSentence,
  isSourceTrouble,
  joinLine,
  matchesSearch,
  openableUrl,
  playbookCopy,
  ruleSentence,
  SEARCH_LIMIT,
  searchWords,
  SESSION_LIMITS,
  sourceWord,
  whenLabel,
} from './text.ts'

const NOW = '2026-11-05T02:00:00.000Z' // 한국 시간 11월 5일 11:00
const shown = (value: unknown): string => displayTitle(value, 300)

test('Slack 표기를 사람이 읽는 꼴로 바꾼다', () => {
  const cases: readonly (readonly [string, string])[] = [
    ['<@U000AAA|합성이름> 안녕', '@합성이름 안녕'],
    ['<@U000AAA> 안녕', '@멤버 안녕'],
    ['<#C000AAA|합성채널> 공지', '#합성채널 공지'],
    ['<#C000AAA> 공지', '#채널 공지'],
    ['<!subteam^S000AAA|@합성팀> 확인', '@합성팀 확인'],
    ['<!subteam^S000AAA> 확인', '@팀 확인'],
    ['<!here> 확인', '@here 확인'],
    ['<!channel> 확인', '@channel 확인'],
    ['<!everyone> 확인', '@everyone 확인'],
    ['<!date^1392734382^{date}|2월 18일> 회의', '2월 18일 회의'],
    ['<[링크]|합성 문서> 참고', '합성 문서 참고'],
    ['<[주소]|합성 주소> 참고', '합성 주소 참고'],
    ['<[링크]> 참고', '[링크] 참고'],
    ['<[주소]> 참고', '[주소] 참고'],
    ['<https://example.test/a|합성 글자> 참고', '합성 글자 참고'],
    ['<http://example.test/a|합성 글자> 참고', '합성 글자 참고'],
    ['<mailto:a@example.test|합성 메일> 참고', '합성 메일 참고'],
    ['<https://example.test/a> 참고', '[링크] 참고'],
    ['<http://example.test/a> 참고', '[링크] 참고'],
    ['<mailto:a@example.test> 참고', '[링크] 참고'],
    ['<tel:+820000000000|합성 전화> 참고', '합성 전화 참고'],
    ['<tel:+820000000000> 참고', '[링크] 참고'],
    ['<HTTPS://example.test/a|합성 글자> 참고', '합성 글자 참고'],
    ['<!date^1392734382^{date}> 회의', '[날짜] 회의'],
    ['<!합성|합성 글자> 참고', '<!합성|합성 글자> 참고'],
    ['<!foo|합성 글자> 참고', '합성 글자 참고'],
    ['<https://a.test|<https://b.test|합성 글자>> 참고', '합성 글자 참고'],
  ]
  for (const [raw, expected] of cases) assert.equal(shown(raw), expected, raw)
})

test('제목이 잘려 닫히지 않은 끝 조각을 정리한다', () => {
  const cases: readonly (readonly [string, string])[] = [
    ['합성 <@U000AAA|이', '합성 @이'],
    ['합성 <@U000AAA|이…', '합성 @이…'],
    ['합성 <#C000AAA|채', '합성 #채'],
    ['합성 <@U000AAA', '합성'],
    ['합성 <@U000AAA…', '합성'],
    ['합성 <#C000AAA…', '합성'],
    ['합성 <!subteam^S0…', '합성'],
    ['합성 <[링크]|글…', '합성'],
    ['합성 <https://example.test/긴주소…', '합성'],
    ['합성 <tel:+8200…', '합성'],
    ['<@U000AAA', '(제목 없음)'],
  ]
  for (const [raw, expected] of cases) assert.equal(shown(raw), expected, raw)
})

test('엔티티는 표기를 바꾼 뒤 한 번만 풀어 새 표기를 만들지 않는다', () => {
  assert.equal(shown('a &lt; b &gt; c &amp; d'), 'a < b > c & d')
  assert.equal(shown('&amp;lt;'), '&lt;')
  assert.equal(shown('&lt;@U000AAA|합성&gt;'), '<@U000AAA|합성>')
})

test('위 꼴이 아닌 꺾쇠와 위험 문자는 바꾸지 않는다', () => {
  assert.equal(shown(DANGEROUS_TITLE), DANGEROUS_TITLE)
  for (const raw of ['<img src=x onerror=alert(1)>', '<script>alert(1)</script>', 'a <b> c', '값 < 3', '<!-- 주석', '<@!>', '<!doctype html>', '<!foo>', '<@소문자이름>']) {
    assert.equal(shown(raw), raw, raw)
  }
})

test('Slack 표기의 이름에 위험 문자가 있어도 글자로만 남고 제어 문자는 빠진다', () => {
  assert.equal(shown('<@U000AAA|$(id)>'), '@$(id)')
  assert.equal(shown('<@U000AAA|합‮성>'), '@합성')
})

test('빈 제목은 대체 문구다', () => {
  assert.equal(shown(''), '(제목 없음)')
  assert.equal(shown(42), '(제목 없음)')
  assert.equal(shown(SLACK_TITLE).includes('<'), false)
})

test('clip은 글자 수로 자르고 짧으면 그대로 둔다', () => {
  assert.equal(clip('가나다라', 4), '가나다라')
  assert.equal(clip('가나다라마', 4), '가나다…')
  assert.equal(clip('😀😀😀', 2), '😀…')
})

test('허용 호스트의 https만 연다', () => {
  for (const url of [
    'https://github.com/example/repo/issues/1',
    'https://calendar.google.com/calendar/event?eid=a',
    'https://mail.google.com/mail/u/0/#inbox/a',
    'https://notion.so/a',
    'https://www.notion.so/a',
    'https://example-team.slack.com/archives/C000AAA/p1',
    'https://GITHUB.com/example/repo',
  ]) {
    assert.notEqual(openableUrl(url), null, url)
  }
})

test('위조한 주소는 열지 않는다', () => {
  for (const url of [
    'https://github.com.evil.test',
    'https://evil.test/github.com',
    'https://evil.test#github.com',
    'https://evil.test?u=https://github.com',
    'http://github.com',
    'https://a@github.com',
    'https://a:b@github.com',
    'https://github.com@evil.test',
    'https://github.com:8443',
    'https://slack.com',
    'https://.slack.com',
    'https://evilslack.com',
    'https://evil.test.slack.com.evil.test',
    'https://notion.so.evil.test',
    'https://gist.github.com/a',
    'https://google.com',
    'https://github.com./a',
    'https://xn--gthub-n4a.com',
    'javascript:alert(1)',
    'data:text/html,a',
    'file:///etc/passwd',
    'github.com',
    '',
  ]) {
    assert.equal(openableUrl(url), null, url)
  }
  assert.equal(openableUrl(null), null)
})

test('연 주소는 정규화한 값이다', () => {
  assert.equal(openableUrl('https://GitHub.com/a b'), 'https://github.com/a%20b')
})

test('시각은 한국 달력으로 쓴다', () => {
  assert.equal(whenLabel('2026-11-05T05:20:00.000Z', NOW), '14:20')
  assert.equal(whenLabel('2026-11-05T14:59:00.000Z', NOW), '23:59')
  assert.equal(whenLabel('2026-11-05T15:00:00.000Z', NOW), '내일 00:00')
  assert.equal(whenLabel('2026-11-06T01:00:00.000Z', NOW), '내일 10:00')
  assert.equal(whenLabel('2026-11-09T01:00:00.000Z', NOW), '11월 9일 10:00')
  assert.equal(whenLabel('아님', NOW), null)
  assert.equal(whenLabel(null, NOW), null)
})

test('기준 시각을 못 읽으면 지금 시각을 쓴다', () => {
  assert.match(whenLabel('2020-01-02T01:00:00.000Z', null) ?? '', /^1월 2일 10:00$/)
  assert.match(whenLabel('2020-01-02T01:00:00.000Z', '아님') ?? '', /^1월 2일 10:00$/)
})

test('넘어온 일의 낱말은 달력 일수로 정한다', () => {
  assert.equal(carriedWord('2026-11-04T14:59:00.000Z', NOW), '어제부터')
  assert.equal(carriedWord('2026-11-03T14:59:00.000Z', NOW), '2일 전부터')
  assert.equal(carriedWord('2026-10-29T15:00:00.000Z', NOW), '6일 전부터')
  assert.equal(carriedWord('2026-10-28T15:00:00.000Z', NOW), '10월 29일부터')
  assert.equal(carriedWord('아님', NOW), null)
})

test('배포 날짜는 월과 일로 쓴다', () => {
  assert.equal(dayLabel('2026-11-07'), '11월 7일')
  assert.equal(dayLabel('합성'), '합성')
})

test('보류 사유를 두 문장으로 쓴다', () => {
  const until = '2026-11-05T05:20:00.000Z'
  const line = (reason: string) => holdSentence({ reason, message: '합성 메시지', until }, NOW)
  assert.deepEqual(line('day_off'), ['오늘은 쉬는 날입니다', '14:20에 다시 보여 드립니다.'])
  assert.deepEqual(line('off_hours'), ['근무 시간이 아닙니다', '14:20에 다시 보여 드립니다.'])
  assert.deepEqual(line('focus'), ['포커스 중입니다', '14:20까지 조용히 모아 둡니다.'])
  assert.deepEqual(line('lunch'), ['점심시간입니다', '14:20에 다시 보여 드립니다.'])
  assert.deepEqual(line('budget_hour'), ['이번 시간 제안은 여기까지입니다', '14:20에 다시 보여 드립니다.'])
  assert.deepEqual(line('budget_day'), ['오늘 제안은 여기까지입니다', '남은 일은 14:20에 모아 보여 드립니다.'])
  assert.deepEqual(line('nothing'), ['지금 먼저 볼 일은 없습니다', null])
  assert.deepEqual(line('다른 사유'), ['합성 메시지', null])
  assert.deepEqual(holdSentence(null, NOW), ['지금 먼저 볼 일은 없습니다', null])
})

test('풀리는 시각을 못 읽으면 뒷 문장이 없다', () => {
  assert.deepEqual(holdSentence({ reason: 'lunch', message: '', until: null }, NOW), ['점심시간입니다', null])
  assert.deepEqual(holdSentence({ reason: 'lunch', message: '', until: '아님' }, NOW), ['점심시간입니다', null])
})

test('이어 쓸 때만 제목에 마침표를 찍는다', () => {
  assert.equal(joinLine('합성 제목', null), '합성 제목.')
  assert.equal(joinLine('합성 제목', '합성 문장.'), '합성 제목. 합성 문장.')
  assert.equal(joinLine('이미 끝남.', null), '이미 끝남.')
  assert.equal(holdLine({ reason: 'lunch', message: '', until: '2026-11-05T05:20:00.000Z' }, NOW), '점심시간입니다. 14:20에 다시 보여 드립니다.')
  assert.equal(holdLine(null, NOW), '지금 먼저 볼 일은 없습니다.')
})

test('조작 실패 코드를 사용자 문구로 옮기고 모르는 코드는 원문을 쓰지 않는다', () => {
  assert.equal(actionFailure('worktree_failed'), '작업 공간을 만들지 못했습니다.')
  assert.equal(actionFailure('session_gone'), '세션의 터미널이 닫혀 있습니다.')
  assert.equal(actionFailure('move_failed'), null)
  assert.equal(actionFailure('합성 코드'), null)
  assert.equal(actionFailure('constructor'), null)
  assert.deepEqual(failureCopy('constructor', '합성 메시지'), { title: 'kairos를 읽지 못했습니다', body: '합성 메시지' })
  assert.deepEqual(holdSentence({ reason: 'constructor', message: '합성 메시지', until: null }, NOW), ['합성 메시지', null])
})

test('고장 코드를 사용자 문구로 옮긴다', () => {
  assert.equal(failureCopy('spawn_failed', '').title, 'kairos가 실행되지 않습니다')
  assert.equal(failureCopy('timeout', '').title, '응답이 늦습니다')
  assert.equal(failureCopy('bad_output', '').title, '설치된 kairos와 패널 버전이 다릅니다')
  assert.equal(failureCopy('schema', '').title, '설치된 kairos와 패널 버전이 다릅니다')
  assert.equal(failureCopy('unavailable', '').title, '패널이 응답을 받지 못했습니다')
  assert.deepEqual(failureCopy('other', '합성 메시지'), { title: 'kairos를 읽지 못했습니다', body: '합성 메시지' })
})

const guide = (overrides: Partial<Parameters<typeof guideOf>[0]> = {}) => guideOf({ reason: 'assigned', run: null, accepted: false, startable: false, canPr: false, ...overrides })

test('이유와 권장은 사유마다 고정 문구이고 모르는 사유는 그리지 않는다', () => {
  assert.deepEqual(guide(), { why: '담당자로 지정된 이슈입니다.', suggest: '내용을 확인하고 어떻게 진행할지 정하세요.' })
  assert.deepEqual(guide({ reason: 'review_owed' }), { why: '리뷰 요청을 받았습니다.', suggest: '변경을 읽고 리뷰를 남기세요.' })
  for (const reason of ['reply_owed', 'review_owed', 'assigned', 'my_pr_attention', 'meeting_new', 'deploy_risk', 'work_owed', 'manual']) {
    const copy = guide({ reason })
    assert.ok(copy !== null && copy.why.endsWith('.') && copy.suggest.endsWith('.'), reason)
  }
  for (const reason of ['', 'unknown', 'constructor', '__proto__', 'toString']) assert.equal(guide({ reason }), null, reason)
})

test('시작할 수 있는 제안이 붙으면 권장 끝에 맡길 수 있다고 덧붙인다', () => {
  assert.equal(guide({ startable: true })?.suggest, '내용을 확인하고 어떻게 진행할지 정하세요. 아래 제안대로 에이전트에게 맡길 수도 있습니다.')
  assert.equal(guide({ reason: 'unknown', startable: true }), null)
})

test('세션이 있으면 사유 대신 세션 상태가 권장을 정한다', () => {
  assert.deepEqual(guide({ run: 'waiting_user' }), { why: '세션이 답을 기다리고 있습니다.', suggest: '세션으로 이동해 답해 주세요.' })
  assert.equal(guide({ run: 'started', startable: true })?.suggest, '끝나거나 질문이 생기면 세션 소식으로 알려 드립니다.')
  assert.equal(guide({ run: 'ended' })?.suggest, '결과를 확인하고 완료로 닫으세요.')
  assert.equal(guide({ run: 'ended', canPr: true })?.suggest, '결과를 확인하고 PR을 올리거나 완료로 닫으세요.')
  assert.equal(guide({ run: 'started', canPr: true })?.suggest, '끝나거나 질문이 생기면 세션 소식으로 알려 드립니다.')
  // 모르는 세션 상태와 세션 상태 없이 수락만 된 일도 세션 쪽으로 안내한다. 사유를 몰라도 그린다.
  for (const input of [{ run: 'constructor' }, { run: null, accepted: true }, { run: 'future_state', reason: 'unknown' }]) {
    assert.deepEqual(guide(input), { why: '세션에 맡긴 일입니다.', suggest: '세션으로 이동해 상태를 확인해 주세요.' })
  }
})

test('소스 상태는 고정 낱말이고 지연만 분을 붙이며 수집 시간 밖은 문제가 아니다', () => {
  assert.equal(sourceWord('ok', null), '정상')
  assert.equal(sourceWord('delayed', 37), '37분 지연')
  assert.equal(sourceWord('delayed', null), '지연')
  assert.equal(sourceWord('ok', 5), '정상')
  assert.equal(sourceWord('never', null), '수집 기록 없음')
  assert.equal(sourceWord('not_connected', null), '미연결')
  assert.equal(sourceWord('quiet', null), '수집 시간 아님')
  for (const status of ['', 'constructor', '<b>ok</b>']) assert.equal(sourceWord(status, null), '확인 필요', status)
  assert.deepEqual(['ok', 'delayed', 'never', 'not_connected', 'quiet', 'constructor'].map(isSourceTrouble), [false, true, true, true, false, false])
})

test('검색어는 낱말로 나누고 제어 문자와 긴 글을 걸러 낸다', () => {
  assert.deepEqual(searchWords('  GitHub   리뷰 '), ['github', '리뷰'])
  assert.deepEqual(searchWords(''), [])
  assert.deepEqual(searchWords('   '), [])
  for (const value of [null, undefined, 1, {}, ['리뷰']]) assert.deepEqual(searchWords(value), [], String(value))
  assert.ok(searchWords('가'.repeat(500)).join(' ').length <= SEARCH_LIMIT)
  assert.deepEqual(searchWords('리뷰\u0000\u001b[31m 요청\n끝'), ['리뷰[31m', '요청', '끝'])
})

test('검색은 모든 낱말이 어느 칸에든 있으면 맞고 대소문자를 가리지 않는다', () => {
  const fields = ['합성 리뷰 요청', '리뷰 요청', 'GitHub', '오늘']
  assert.equal(matchesSearch([], fields), true)
  assert.equal(matchesSearch(searchWords('github'), fields), true)
  assert.equal(matchesSearch(searchWords('리뷰 github'), fields), true)
  assert.equal(matchesSearch(searchWords('리뷰 slack'), fields), false)
  assert.equal(matchesSearch(searchWords('.*'), fields), false)
  assert.equal(matchesSearch(searchWords('오늘'), fields), true)
})

test('규칙 ID는 고정 문장으로 옮기고 모르는 규칙은 문장이 없다', () => {
  assert.equal(ruleSentence('p1.ci_failed'), '내 PR의 CI 검사가 실패했습니다.')
  assert.equal(ruleSentence('p0.meeting_imminent'), '곧 시작하는 회의입니다.')
  for (const rule of [null, 'p9.unknown', 'constructor', '__proto__']) assert.equal(ruleSentence(rule), null, String(rule))
})

test('사실 코드는 고정 낱말과 색으로 옮기고 모르는 코드는 그리지 않는다', () => {
  assert.deepEqual(factsOf(['pull_request', 'checks_failed', 'changes_requested', 'approved', 'mystery', 'toString']), [
    { code: 'pull_request', text: 'PR', tone: null },
    { code: 'checks_failed', text: '검사 실패', tone: 'danger' },
    { code: 'changes_requested', text: '변경 요청됨', tone: 'warning' },
    { code: 'approved', text: '승인됨', tone: 'success' },
  ])
})

test('제안 근거와 플레이북은 고정 문구이고 모르는 값은 빠진다', () => {
  assert.deepEqual(checksOf(['assigned_me', 'mapped', 'nope', 'no_session']), ['내게 배정된 이슈', '작업할 저장소가 정해져 있음', '같은 일의 세션이 없음'])
  const copy = playbookCopy('qa_issue')
  assert.equal(copy?.title, '에이전트가 이슈를 고칩니다')
  assert.equal(copy?.steps.length, 3)
  assert.match(copy?.result ?? '', /직접 올립니다/)
  assert.equal(playbookCopy('pr_review')?.title, '에이전트가 리뷰 초안을 씁니다')
  for (const playbook of [null, 'deploy', 'hasOwnProperty']) assert.equal(playbookCopy(playbook), null, String(playbook))
  assert.match(SESSION_LIMITS, /push, PR, 코멘트/)
})

test('소식과 참석자 수는 둘 이상일 때만 쓴다', () => {
  assert.deepEqual([eventsWord(0), eventsWord(1), eventsWord(4)], [null, null, '소식 4건'])
  assert.deepEqual([attendeesWord(null), attendeesWord(1), attendeesWord(6)], [null, null, '참석자 6명'])
})
