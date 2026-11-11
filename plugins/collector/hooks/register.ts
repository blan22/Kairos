import type { Register } from 'claude-code'
import { DROP_FAILED, isPermitted, runTick } from './tick.ts'

/**
 * 수집기 실행에서만 로드한다. 이 모드는 모델을 부르지 않고 prompt.submit 안에서 끝난다.
 *
 * tool.check는 기본 거부다. 전부 거부하면 자기 $.mcp.call까지 막히고(스파이크 A1), 훅이 없으면
 * dontAsk가 막는다(A3). 그래서 이 플러그인이 낸 허용목록 호출만 allow한다.
 *
 * .catch는 등록된 훅만 지킨다. 모듈 로드 실패는 ANTHROPIC_BASE_URL이 막는다. kairos가 이 값을
 * 멀티캐스트 주소(224.0.0.0)로 두므로 TCP 연결이 커널에서 거절되고, 어떤 로컬 프로세스도 요청을 받지 못한다.
 */
export const register: Register = on => {
  on('tool.check', ($, e, next) => ({ decision: isPermitted(next.origin?.plugin, e.tool) ? 'allow' : 'deny' })).catch(() => ({
    decision: 'deny',
  }))

  on('prompt.submit', async ($, e, next) => {
    try {
      return { drop: await runTick($, next.budget) }
    } catch {
      return { drop: DROP_FAILED }
    }
  }).catch(($, e, next) => (next.called ? next(e) : { drop: DROP_FAILED }))
}
