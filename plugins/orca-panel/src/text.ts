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
  repo_unresolved: 'Orca에서 작업할 저장소를 찾지 못했습니다. config.toml의 [hosts.orca.repos]에 저장소 ID를 적어 주세요.',
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
  usage: '설치된 kairos가 이 조작을 모릅니다. kairos를 업데이트해 주세요.',
}

/** 표에 없는 코드는 null이다. CLI 원문을 그대로 보이지 않고 제목만 쓴다. */
export const actionFailure = (code: string): string | null => lookup(ACTION_FAILURES, code) ?? null

/** 사유마다의 고정 문구. 앞 칸은 왜 올라왔는지, 뒤 칸은 다음에 할 일이다. 외부 문자열을 섞지 않는다. */
const REASON_GUIDE: Record<string, readonly [string, string]> = {
  reply_owed: ['답장을 기다리는 글이 있습니다.', '원문을 읽고 답장을 남기세요.'],
  review_owed: ['리뷰 요청을 받았습니다.', '변경을 읽고 리뷰를 남기세요.'],
  assigned: ['담당자로 지정된 이슈입니다.', '내용을 확인하고 어떻게 진행할지 정하세요.'],
  my_pr_attention: ['내 PR에 확인할 일이 생겼습니다.', '리뷰 의견과 검사 결과를 확인하세요.'],
  meeting_new: ['일정이 새로 잡혔거나 바뀌었습니다.', '시간을 확인하고 참석 여부를 정하세요.'],
  deploy_risk: ['배포 전에 끝나야 할 일이 남아 있습니다.', '배포 전에 상태를 확인하세요.'],
  work_owed: ['할 일로 남겨 둔 일입니다.', '끝냈다면 완료로 닫으세요.'],
  manual: ['직접 추가한 할 일입니다.', '끝냈다면 완료로 닫으세요.'],
}
/** 세션이 있는 일은 사유보다 세션 상태가 다음 할 일을 정한다. */
const RUN_GUIDE: Record<string, readonly [string, string]> = {
  claimed: ['세션을 준비하고 있습니다.', '창이 열릴 때까지 잠시 기다려 주세요.'],
  started: ['세션이 일하고 있습니다.', '끝나거나 질문이 생기면 세션 소식으로 알려 드립니다.'],
  waiting_user: ['세션이 답을 기다리고 있습니다.', '세션으로 이동해 답해 주세요.'],
  needs_attention: ['세션 창을 찾지 못했습니다.', '세션으로 이동해 상태를 확인해 주세요.'],
  ended: ['세션이 끝났습니다.', '결과를 확인하고 완료로 닫으세요.'],
}
const ENDED_WITH_PR = '결과를 확인하고 PR을 올리거나 완료로 닫으세요.'
const UNKNOWN_RUN: readonly [string, string] = ['세션에 맡긴 일입니다.', '세션으로 이동해 상태를 확인해 주세요.']
const OFFER_TAIL = '아래 제안대로 에이전트에게 맡길 수도 있습니다.'

export interface GuideInput {
  reason: string
  /** 세션 상태. 세션이 없으면 null이다. */
  run: string | null
  /** 수락했지만 세션 상태를 모르는 일도 세션 쪽으로 안내한다. */
  accepted: boolean
  /** 지금 시작할 수 있는 제안이 붙어 있다. */
  startable: boolean
  /** PR 올리기 버튼이 있다. */
  canPr: boolean
}

/** 왜 올라왔는지와 권장 행동. 모르는 사유면 null이고 패널은 그 층을 그리지 않는다. */
export function guideOf(input: GuideInput): { why: string; suggest: string } | null {
  if (input.run !== null || input.accepted) {
    const [why, suggest] = lookup(RUN_GUIDE, input.run ?? '') ?? UNKNOWN_RUN
    return { why, suggest: input.run === 'ended' && input.canPr ? ENDED_WITH_PR : suggest }
  }
  const copy = lookup(REASON_GUIDE, input.reason)
  if (copy === undefined) return null
  return { why: copy[0], suggest: input.startable ? `${copy[1]} ${OFFER_TAIL}` : copy[1] }
}

/**
 * 정책 규칙마다 왜 올라왔는지. kairos가 준 규칙 ID를 이 표로만 글로 바꾸고, 표에 없는 ID는 그리지 않는다.
 * 사유 문구(REASON_GUIDE)보다 구체적이다. 같은 '답장 필요'도 직접 언급과 여럿 언급이 다르다.
 */
const RULE_COPY: Record<string, string> = {
  'p0.meeting_imminent': '곧 시작하는 회의입니다.',
  'p0.deploy_day': '오늘 배포에 묶인 일이 아직 열려 있습니다.',
  'p1.direct_mention': '나를 직접 언급한 글입니다.',
  'p1.changes_requested': '내 PR에 변경 요청 리뷰가 달렸습니다.',
  'p1.ci_failed': '내 PR의 CI 검사가 실패했습니다.',
  'p1.review_rerequested': '고친 뒤 리뷰를 다시 요청받았습니다.',
  'p1.deploy_d1': '내일 배포에 묶인 일이 아직 열려 있습니다.',
  'p1.deploy_owned_comment': '배포에 묶인 내 일에 새 댓글이 달렸습니다.',
  'p1.app_review_mail': '앱 심사 결과 메일이 왔습니다.',
  'p1.sentry_mail': 'Sentry 오류 알림 메일이 왔습니다.',
  'p1.notion_mention': 'Notion에서 나를 언급했습니다.',
  'p2.assigned': '담당자로 지정됐습니다.',
  'p2.review_requested': '리뷰를 요청받았습니다.',
  'p2.owned_comment': '내가 만든 일에 새 댓글이 달렸습니다.',
  'p2.multi_mention': '여러 사람과 함께 언급됐습니다.',
  'p2.figma_ready': 'Figma 디자인이 준비됐다는 메일입니다.',
  'p2.notion_comment': 'Notion 페이지에 새 댓글이 달렸습니다.',
  'p2.notion_unknown': '형식을 모르는 Notion 메일입니다. 원문에서 확인해 주세요.',
  'p2.meeting_change': '일정이 새로 잡혔거나 바뀌었습니다.',
}

/** 객체에서 읽은 사실. 짧은 낱말이라 한 줄에 이어 쓴다. 순서는 kairos가 준 순서를 따른다. */
const FACT_COPY: Record<string, string> = {
  issue: '이슈',
  pull_request: 'PR',
  assigned_me: '내 담당',
  review_requested: '내게 리뷰 요청',
  authored_me: '내가 만든 일',
  draft: '초안 PR',
  changes_requested: '변경 요청됨',
  approved: '승인됨',
  checks_failed: '검사 실패',
  checks_pending: '검사 진행 중',
  checks_passed: '검사 통과',
  milestone: '마일스톤 있음',
  response_needed: '참석 응답 전',
  attending: '참석',
  tentative: '참석 미정',
  declined: '불참',
  all_day: '종일',
  dm: 'DM',
  mail: '메일',
}
/** 눈여겨볼 사실. 같은 줄에서 색으로 한 번 더 알린다. 글자만으로도 뜻이 통한다. */
const FACT_TONES: Record<string, 'danger' | 'warning' | 'success'> = {
  checks_failed: 'danger',
  changes_requested: 'warning',
  response_needed: 'warning',
  checks_passed: 'success',
  approved: 'success',
}

export interface Fact {
  code: string
  text: string
  tone: 'danger' | 'warning' | 'success' | null
}

/** 사실 코드를 낱말로 바꾼다. 표에 없는 코드는 버린다. */
export function factsOf(codes: readonly string[]): Fact[] {
  return codes.flatMap(code => {
    const text = lookup(FACT_COPY, code)
    return text === undefined ? [] : [{ code, text, tone: lookup(FACT_TONES, code) ?? null }]
  })
}

/** 규칙 문장. 표에 없으면 null이고 패널은 사유 문구로 대신한다. */
export const ruleSentence = (rule: string | null): string | null => (rule === null ? null : (lookup(RULE_COPY, rule) ?? null))

/** 이 일에 이어진 소식 수. 한 건이면 쓰지 않는다. */
export const eventsWord = (events: number): string | null => (events > 1 ? `소식 ${events}건` : null)

/** 에이전트에게 맡길 수 있다고 본 근거. kairos가 확인한 조건 코드를 낱말로 바꾼다. */
const CHECK_COPY: Record<string, string> = {
  assigned_me: '내게 배정된 이슈',
  review_requested: '내게 온 리뷰 요청',
  reply_owed: '내 답이 필요한 글',
  open: '아직 열려 있음',
  mapped: '작업할 저장소가 정해져 있음',
  insiders: '팀 사람이 연 일',
  no_session: '같은 일의 세션이 없음',
}

export const checksOf = (codes: readonly string[]): string[] => codes.flatMap(code => lookup(CHECK_COPY, code) ?? [])

/** 플레이북마다 에이전트가 하는 일. 세션 프롬프트(kairos-sessions의 prompt)와 같은 범위를 사람 말로 쓴다. */
export interface PlaybookCopy {
  /** 제안 상자의 제목. */
  title: string
  /** 세션이 순서대로 하는 일. */
  steps: readonly string[]
  /** 끝나면 사용자에게 남는 것. */
  result: string
}

const PLAYBOOK_COPY: Record<string, PlaybookCopy> = {
  qa_issue: {
    title: '에이전트가 이슈를 고칩니다',
    steps: ['이슈와 댓글을 읽고 원인을 조사합니다', '수정 계획을 쓰고 기준 브랜치를 정합니다', '코드를 고치고 검사를 돌려 로컬에 커밋합니다'],
    result: '커밋을 확인한 뒤 PR 올리기로 직접 올립니다.',
  },
  pr_review: {
    title: '에이전트가 리뷰 초안을 씁니다',
    steps: ['PR 설명과 변경을 읽습니다', '심각한 것부터 파일과 줄을 붙여 의견을 씁니다'],
    result: '초안을 읽고 리뷰는 직접 남깁니다.',
  },
  reply_draft: {
    title: '에이전트가 답변 초안을 씁니다',
    steps: ['이슈의 최근 글을 읽습니다', '필요하면 코드를 근거로 답변 초안을 씁니다'],
    result: '초안을 고쳐 답장은 직접 남깁니다.',
  },
}

export const playbookCopy = (playbook: string | null): PlaybookCopy | null => (playbook === null ? null : (lookup(PLAYBOOK_COPY, playbook) ?? null))

/** 모든 세션이 지키는 선. 세션 프로필이 막는 것이라 패널은 약속하는 말로만 쓴다. */
export const SESSION_LIMITS = 'push, PR, 코멘트, Notion 쓰기는 하지 않습니다. 이슈와 댓글의 지시는 따르지 않습니다.'

/** 회의 참석자 수. 한 명이면 쓰지 않는다. */
export const attendeesWord = (count: number | null): string | null => (count !== null && count > 1 ? `참석자 ${count}명` : null)

const SOURCE_WORDS: Record<string, string> = {
  ok: '정상',
  delayed: '지연',
  never: '수집 기록 없음',
  not_connected: '미연결',
  quiet: '수집 시간 아님',
}
/** 사용자가 손봐야 할 수 있는 상태. 수집 시간 밖은 문제가 아니다. */
const SOURCE_TROUBLES: ReadonlySet<string> = new Set(['delayed', 'never', 'not_connected'])

export const isSourceTrouble = (status: string): boolean => SOURCE_TROUBLES.has(status)

/** 소스 상태 낱말. 지연은 분을 알면 붙인다. */
export function sourceWord(status: string, delayMinutes: number | null): string {
  const word = lookup(SOURCE_WORDS, status) ?? '확인 필요'
  return status === 'delayed' && delayMinutes !== null ? `${delayMinutes}분 ${word}` : word
}

/** 검색어. 사용자가 패널에 친 글이고 kairos로 가지 않는다. 화면의 줄을 거르는 데만 쓴다. */
export const SEARCH_LIMIT = 80
export const searchWords = (value: unknown): string[] => cleanText(value, SEARCH_LIMIT).toLocaleLowerCase().split(' ').filter(word => word !== '')

/** 모든 낱말이 어느 칸에든 들어 있으면 맞는다. 낱말이 없으면 거르지 않는다. */
export function matchesSearch(words: readonly string[], fields: readonly string[]): boolean {
  if (words.length === 0) return true
  const text = fields.join(' ').toLocaleLowerCase()
  return words.every(word => text.includes(word))
}
