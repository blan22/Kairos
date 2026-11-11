/** 고정값. 패널과 워커가 함께 읽는다. 경로는 워커만 쓰므로 `paths.ts`에 있다. */
export const REVEAL_FILE = 'reveal.json'
/** 원자 쓰기는 이벤트를 여러 번 낸다. 잠잠해진 뒤 한 번만 알린다. */
export const REVEAL_DEBOUNCE_MS = 150

export const PLUGIN_KEY = 'local.kairos'
export const PANEL_ID = 'kairos'
export const STATUS_ITEM_ID = 'kairos'
export const OPEN_COMMAND = 'kairos.open'

export const POLL_MS = 60_000

/**
 * 호출별 시간 제한. 큐로 한 번에 하나만 돈다.
 * accept는 안쪽 orca 호출 제한의 합(약 113초)보다 길게 둔다. 중간에 죽이면 응답을 잃은 worktree가 남기 때문이다.
 */
export const TIMEOUT_MS = {
  read: 15_000,
  write: 20_000,
  accept: 115_000,
  open: 5_000,
} as const

/** plugin.request의 최대값은 120초다. 수락이 그보다 오래 걸리면 패널은 실패로 표시하고 다음 틱이 결과를 그린다. */
export const PANEL_SNAPSHOT_TIMEOUT_MS = 60_000
export const PANEL_ACTION_TIMEOUT_MS = 120_000

/** 플러그인 응답은 1 MiB까지다. 목록은 이 건수까지만 넘긴다. */
export const LIST_LIMIT = 100
export const IDS_LIMIT = 100
/** 패널에서 쓰는 할 일 제목의 길이. */
export const ADD_TITLE_LIMIT = 200
