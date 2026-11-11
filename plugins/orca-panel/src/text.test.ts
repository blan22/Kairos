import { assert, test } from 'orca/testing'
import { DANGEROUS_TITLE, SLACK_TITLE } from './fixtures.ts'
import { actionFailure, carriedWord, clip, dayLabel, displayTitle, failureCopy, holdLine, holdSentence, joinLine, openableUrl, whenLabel } from './text.ts'

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
