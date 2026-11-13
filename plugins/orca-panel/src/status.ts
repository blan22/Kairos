/** 상태 표시줄에 그릴 값을 정한다. 건수는 예산과 무관하고, 게이트가 닫히면 건수 대신 사유를 쓴다. */
import type { Failure, NextView } from './protocol.ts'
import { cleanText, failureCopy, holdLine, lookup, whenLabel } from './text.ts'

export type StatusTone = 'neutral' | 'accent' | 'success' | 'warning' | 'danger'
/** gauge는 패널 아이콘과 같다. 알림 앱 아이콘(열린 고리와 점)에 가장 가까운 Orca 아이콘이다. */
export type StatusIcon = 'gauge' | 'pause' | 'warning' | 'error'
/** 새 일 점. 상태 표시줄은 글자 일부에 색을 줄 수 없어 모양으로 알리고 설명에 뜻을 쓴다. */
const FRESH_MARK = ' ●'
const FRESH_LINE = '패널에 아직 보지 않은 새 일이 있습니다'

export interface StatusView {
  text: string
  icon: StatusIcon
  tone: StatusTone
  tooltip: string
}

/** Orca가 받는 tooltip 길이는 256자까지다. */
const TOOLTIP_LIMIT = 256

const GATE_LABELS: Record<string, string> = {
  paused: '일시정지',
  display_off: '표시 꺼짐',
  no_collection: '수집 전',
  recording: '기록 중',
}

const tooltipOf = (lines: readonly (string | null)[]): string => cleanText(lines.filter(line => line).join(' / '), TOOLTIP_LIMIT)

export const badgeOf = (next: NextView): number => (next.display.open ? next.counts.P0 + next.counts.P1 + next.counts.P2 : 0)

/** 색은 긴급, 수집 경고, 세션 소식, 새 일에만 쓴다. 평소 건수는 무채색이다. 빨강은 긴급의 몫이라 새 일은 accent다. */
function toneOf(next: NextView, fresh: boolean): StatusTone {
  if (next.counts.P0 > 0) return 'danger'
  if (next.health !== null) return 'warning'
  return next.ready.length > 0 || fresh ? 'accent' : 'neutral'
}

function focusLine(next: NextView): string {
  const until = whenLabel(next.focus.until, next.at)
  return until === null ? '포커스 중' : `포커스 중 · ${until}까지`
}

/** fresh는 패널에 아직 보지 않은 새 일이 있다는 뜻이다. 워커가 패널이 남긴 기준으로 정한다. */
export function statusOfNext(next: NextView, fresh: boolean = false): StatusView {
  const { display, counts, health } = next
  // 세션 카드는 패널을 닫아 둔 사용자가 알 길이 여기뿐이라 글자에 넣는다. 포커스는 뺏지 않는다.
  const sessions = next.ready.length > 0 ? ` · 세션 ${next.ready.length}` : ''
  // 포커스는 패널을 닫아 둔 채 켜 두는 상태라 여기서 보여야 끈 줄 알고 놓치지 않는다.
  const focus = next.focus.active && display.open ? ' · 포커스' : ''
  const mark = `${focus}${sessions}${health === null ? '' : ' !'}`
  const sessionLine = next.ready.length > 0 ? `세션 소식 ${next.ready.length}건` : null
  if (!display.open) {
    const starts = whenLabel(display.startsAt, next.at)
    return {
      text: `kairos ${lookup(GATE_LABELS, display.reason ?? '') ?? '표시 대기'}${mark}`,
      icon: health === null ? 'pause' : 'warning',
      tone: health === null ? 'neutral' : 'warning',
      tooltip: tooltipOf([health, sessionLine, display.message, starts ? `${starts}부터 제안을 보여 드립니다` : null]),
    }
  }
  const tally = `긴급 ${counts.P0}, 오늘 ${counts.P1}, 여유 있을 때 ${counts.P2}`
  const extra = `할 일 ${counts.manual}, 미룬 일 ${counts.parked}, 세션 ${counts.accepted}`
  // 긴급은 색만으로 알리지 않는다. 건수를 글자로 쓴다.
  const urgent = counts.P0 > 0 ? ` · 긴급 ${counts.P0}` : ''
  return {
    text: `kairos ${badgeOf(next)}${fresh ? FRESH_MARK : ''}${urgent}${mark}`,
    icon: health === null ? 'gauge' : 'warning',
    tone: toneOf(next, fresh),
    tooltip: tooltipOf([
      health,
      fresh ? FRESH_LINE : null,
      sessionLine,
      next.focus.active ? focusLine(next) : null,
      tally,
      extra,
      next.card === null ? holdLine(next.hold, next.at) : null,
    ]),
  }
}

export function statusOfFailure(failure: Failure): StatusView {
  return { text: 'kairos 오류', icon: 'error', tone: 'danger', tooltip: tooltipOf([failureCopy(failure.code, failure.message).title, `코드 ${failure.code}`]) }
}
