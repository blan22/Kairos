/** 표시용 문자열 처리. 패널에서도 불러서 node 모듈을 쓰지 않는다. */
const LINE_BREAKS = /[\t\n\v\f\r\u0085\p{Zl}\p{Zp}]+/gu
/** 제어 문자, 양방향 재정렬, 폭 없는 문자를 뺀다. 한글 자모 결합에 쓰는 ZWNJ와 ZWJ만 둔다. */
const UNSAFE = /[\p{Cc}\p{Cf}\p{Cs}\p{Zl}\p{Zp}]/u
const JOINERS: ReadonlySet<string> = new Set(['\u200c', '\u200d'])

/** 코드로 표를 찾는다. constructor 같은 값이 Object의 속성으로 풀리지 않게 자기 키만 본다. */
export const lookup = <T>(table: Readonly<Record<string, T>>, key: string): T | undefined => (Object.hasOwn(table, key) ? table[key] : undefined)

/** 글자 수로 자른다. 잘렸으면 끝에 …을 둔다. */
export function clip(value: string, limit: number): string {
  const chars = [...value]
  return chars.length > limit ? `${chars.slice(0, limit - 1).join('')}…` : value
}

export function cleanText(value: unknown, limit: number): string {
  if (typeof value !== 'string') return ''
  const safe = [...value.replace(LINE_BREAKS, ' ')].filter(char => JOINERS.has(char) || !UNSAFE.test(char))
  return clip(safe.join('').trim(), limit)
}

const EMPTY_TITLE = '(제목 없음)'

/**
 * Slack 원문 표기를 사람이 읽는 꼴로 바꾸는 표. 순서대로 적용하고, 여기에 없는 <...>는 건드리지 않는다.
 * 라벨 없는 <!doctype html> 같은 꼴은 Slack이 아닌 출처의 제목일 수 있어 그대로 둔다.
 */
const SLACK_FORMS: readonly (readonly [RegExp, (...groups: string[]) => string])[] = [
  [/<@[A-Za-z0-9]+\|([^<>]+)>/g, (_all, name = '') => `@${name}`],
  [/<@[A-Z0-9]+\|?>/g, () => '@멤버'],
  [/<#[A-Za-z0-9]+\|([^<>]+)>/g, (_all, name = '') => `#${name}`],
  [/<#[A-Z0-9]+\|?>/g, () => '#채널'],
  [/<!subteam\^[A-Za-z0-9]+\|@?([^<>]+)>/g, (_all, name = '') => `@${name}`],
  [/<!subteam\^[A-Za-z0-9]+\|?>/g, () => '@팀'],
  [/<!(here|channel|everyone)(?:\|[^<>]*)?>/g, (_all, name = '') => `@${name}`],
  [/<!date\^[^<>|]*>/g, () => '[날짜]'],
  [/<![a-z][^<>|]*\|([^<>]+)>/g, (_all, shown = '') => shown],
  [/<\[(?:링크|주소)\]\|([^<>]+)>/g, (_all, shown = '') => shown],
  [/<(\[(?:링크|주소)\])>/g, (_all, mark = '') => mark],
  [/<[a-z][a-z0-9+.-]*:[^<>|]*\|([^<>]+)>/gi, (_all, shown = '') => shown],
  [/<(?:https?|tel|mailto|slack|ftp):[^<>|]*>/gi, () => '[링크]'],
  // 제목이 잘려 표기가 닫히지 않은 끝 조각이다. 이름이 있으면 살리고 없으면 지운다.
  [/<@[A-Za-z0-9]+\|([^<>]+)$/, (_all, name = '') => `@${name}`],
  [/<#[A-Za-z0-9]+\|([^<>]+)$/, (_all, name = '') => `#${name}`],
  [/\s*<(?:@[A-Za-z0-9]*\|?|#[A-Za-z0-9]*\|?|![a-z][^<>]*|\[(?:링크|주소)\][^<>]*|(?:https?|tel|mailto|slack|ftp)[^<>]*)…?$/i, () => ''],
]
/** 겹친 표기는 안쪽부터 풀린다. 겹침이 끝없을 수 없도록 횟수를 둔다. */
const SLACK_PASSES = 3

/** 표시용 제목. 외부 문자열이므로 글자로만 그린다. 엔티티는 표기를 바꾼 뒤 한 번만 풀어 새 표기가 생기지 않게 한다. */
export function displayTitle(value: unknown, limit: number): string {
  let title = cleanText(value, limit)
  for (let pass = 0; pass < SLACK_PASSES; pass += 1) {
    const before = title
    for (const [form, replace] of SLACK_FORMS) title = title.replace(form, replace)
    if (title === before) break
  }
  title = title.replaceAll('&lt;', '<').replaceAll('&gt;', '>').replaceAll('&amp;', '&').trim()
  return title || EMPTY_TITLE
}

const KST_MS = 9 * 3_600_000
const DAY_MS = 86_400_000

/** 한국은 일광 절약이 없어 고정 차이로 달력을 센다. */
const kstDay = (ms: number): number => Math.floor((ms + KST_MS) / DAY_MS)
const kstDate = (ms: number): Date => new Date(ms + KST_MS)
const pad = (value: number): string => String(value).padStart(2, '0')
const clockOf = (date: Date): string => `${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}`
const monthDay = (date: Date): string => `${date.getUTCMonth() + 1}월 ${date.getUTCDate()}일`

function nowMs(now: string | null): number {
  const ms = now === null ? Number.NaN : Date.parse(now)
  return Number.isNaN(ms) ? Date.now() : ms
}

/** 같은 날은 14:20, 다음 날은 내일 10:00, 그 밖은 11월 9일 10:00이다. now를 못 읽으면 지금 시각을 쓴다. */
export function whenLabel(value: string | null, now: string | null): string | null {
  const ms = value === null ? Number.NaN : Date.parse(value)
  if (Number.isNaN(ms)) return null
  const date = kstDate(ms)
  const days = kstDay(ms) - kstDay(nowMs(now))
  if (days === 0) return clockOf(date)
  return days === 1 ? `내일 ${clockOf(date)}` : `${monthDay(date)} ${clockOf(date)}`
}

/** 어제부터, 3일 전부터, 10월 29일부터. 시각을 못 읽으면 null이다. */
export function carriedWord(openedAt: string | null, now: string | null): string | null {
  const ms = openedAt === null ? Number.NaN : Date.parse(openedAt)
  if (Number.isNaN(ms)) return null
  const days = kstDay(nowMs(now)) - kstDay(ms)
  if (days <= 1) return '어제부터'
  return days <= 6 ? `${days}일 전부터` : `${monthDay(kstDate(ms))}부터`
}

/** 2026-11-07을 11월 7일로 쓴다. 다른 꼴이면 받은 값을 그대로 쓴다. */
export function dayLabel(date: string): string {
  const match = /^\d{4}-(\d{2})-(\d{2})/.exec(date)
  return match === null ? date : `${Number(match[1])}월 ${Number(match[2])}일`
}

const OPENABLE_HOSTS: ReadonlySet<string> = new Set(['github.com', 'calendar.google.com', 'mail.google.com', 'notion.so'])
const OPENABLE_SUFFIXES: readonly string[] = ['.slack.com', '.notion.so']

/** 사용자가 눌렀을 때 열어도 되는 주소만 정규화해서 돌려준다. 허용 호스트의 https이고 계정과 포트가 없어야 한다. */
export function openableUrl(value: string | null): string | null {
  if (value === null) return null
  let url: URL
  try {
    url = new URL(value)
  } catch {
    return null
  }
  if (url.protocol !== 'https:' || url.username !== '' || url.password !== '' || url.port !== '') return null
  const host = url.hostname.toLowerCase()
  return OPENABLE_HOSTS.has(host) || OPENABLE_SUFFIXES.some(end => host.endsWith(end) && host.length > end.length) ? url.href : null
}

const AGAIN = (time: string): string => `${time}에 다시 보여 드립니다.`
/** 앞 칸은 제목으로도 쓰므로 마침표가 없고, 뒤 칸은 문장이다. */
const HOLD_COPY: Record<string, readonly [string, (time: string) => string]> = {
  day_off: ['오늘은 쉬는 날입니다', AGAIN],
  off_hours: ['근무 시간이 아닙니다', AGAIN],
  focus: ['포커스 중입니다', time => `${time}까지 조용히 모아 둡니다.`],
  lunch: ['점심시간입니다', AGAIN],
  budget_hour: ['이번 시간 제안은 여기까지입니다', AGAIN],
  budget_day: ['오늘 제안은 여기까지입니다', time => `남은 일은 ${time}에 모아 보여 드립니다.`],
}
const NOTHING_NOW = '지금 먼저 볼 일은 없습니다'

type HoldInput = { reason: string; message: string; until: string | null } | null

/** 보류 사유를 제목과 뒷 문장으로 쓴다. 풀리는 시각을 못 읽으면 뒷 문장이 없다. */
export function holdSentence(hold: HoldInput, now: string | null): readonly [string, string | null] {
  if (hold === null || hold.reason === 'nothing') return [NOTHING_NOW, null]
  const copy = lookup(HOLD_COPY, hold.reason)
  if (copy === undefined) return [hold.message, null]
  const time = whenLabel(hold.until, now)
  return [copy[0], time === null ? null : copy[1](time)]
}

/** 제목 뒤에 문장을 이어 쓸 때만 제목에 마침표를 찍는다. */
export const joinLine = (title: string, sentence: string | null): string => {
  const head = title.endsWith('.') ? title : `${title}.`
  return sentence === null ? head : `${head} ${sentence}`
}

/** 보류 사유를 한 줄로 쓴다. 상태 표시줄 설명과 패널이 같이 쓴다. */
export function holdLine(hold: HoldInput, now: string | null): string {
  const [title, sentence] = holdSentence(hold, now)
  return joinLine(title, sentence)
}

const VERSION_GAP = { title: '설치된 kairos와 패널 버전이 다릅니다', body: 'kairos doctor로 확인할 수 있습니다.' }
const FAILURE_COPY: Record<string, { title: string; body: string }> = {
  spawn_failed: { title: 'kairos가 실행되지 않습니다', body: 'kairos doctor로 원인을 볼 수 있습니다.' },
  timeout: { title: '응답이 늦습니다', body: 'kairos가 제시간에 답하지 못했습니다.' },
  bad_output: VERSION_GAP,
  schema: VERSION_GAP,
  unavailable: { title: '패널이 응답을 받지 못했습니다', body: '패널을 닫았다가 다시 열면 풀릴 수 있습니다.' },
}

/** 고장 코드를 사용자 문구로 옮긴다. 모르는 코드는 받은 메시지를 그대로 쓴다. */
export function failureCopy(code: string, message: string): { title: string; body: string } {
  return lookup(FAILURE_COPY, code) ?? { title: 'kairos를 읽지 못했습니다', body: message }
}

/** kairos 조작의 실패 코드를 사용자 문구로 옮긴다. move_failed처럼 제목이 곧 뜻인 코드는 넣지 않는다. */
const ACTION_FAILURES: Record<string, string> = {
  not_found: '이 일은 이제 목록에 없습니다.',
  invalid_state: '이 일의 상태가 바뀌었습니다. 새로고침해 주세요.',
  not_eligible: '이 일은 세션을 열 수 없습니다. 원문에서 확인해 주세요.',
  gate_closed: '지금은 제안이 멈춰 있어 세션을 열지 않습니다.',
  aborted: '도중에 취소되어 세션을 열지 않았습니다.',
  no_session: '이동할 세션이 없습니다.',
  session_gone: '세션의 터미널이 닫혀 있습니다.',
  orca_unavailable: 'Orca가 응답하지 않습니다.',
  worktree_failed: '작업 공간을 만들지 못했습니다.',
  checkout_failed: '작업 공간을 확인하지 못했습니다.',
  terminal_failed: '터미널을 열지 못했습니다.',
  no_deploy: '연결된 배포가 없습니다.',
  deploy_imminent: '이미 배포 시점이라 기다릴 수 없습니다.',
  busy: '앞선 처리가 끝나지 않았습니다. 잠시 뒤 다시 눌러 주세요.',
  timeout: '응답이 늦습니다. 잠시 뒤 결과를 확인해 주세요.',
  unavailable: '패널이 응답을 받지 못했습니다.',
  spawn_failed: 'kairos가 실행되지 않습니다.',
  bad_output: '설치된 kairos와 패널 버전이 다릅니다.',
  schema: '설치된 kairos와 패널 버전이 다릅니다.',
  not_fix_session: '코드를 고치는 세션이 아니라 올릴 커밋이 없습니다.',
  adopted_session: 'kairos가 고치는 세션으로 띄운 기록이 아니라 올리지 않습니다. 직접 확인해 주세요.',
  unsafe_repo_config: '저장소 설정이 원격 주소나 git 명령을 바꿉니다. 직접 확인해 주세요.',
  no_worktree: '세션의 작업 공간을 찾지 못했습니다.',
  base_unavailable: '기준 브랜치를 원격에서 받지 못했습니다.',
  session_running: '세션이 아직 열려 있습니다. 세션을 끝낸 뒤 올려 주세요.',
  blocked_paths: 'CI나 리뷰 설정을 바꾼 커밋이라 올리지 않습니다. 직접 확인해 주세요.',
  no_commits: '세션이 아직 커밋하지 않았습니다.',
  push_failed: 'push하지 못했습니다. git 자격 증명과 원격 권한을 확인해 주세요.',
  create_failed: 'PR을 만들지 못했습니다. gh 로그인을 확인해 주세요.',
}

/** 표에 없는 코드는 null이다. CLI 원문을 그대로 보이지 않고 제목만 쓴다. */
export const actionFailure = (code: string): string | null => lookup(ACTION_FAILURES, code) ?? null
