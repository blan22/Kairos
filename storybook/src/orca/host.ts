/** plugin, shell, host 대역. 스토리가 요청 처리기를 넣고, 이벤트를 쏠 수 있다. */
export type Handler = (input: unknown) => unknown
export type Disposable = { dispose(): void }

let handlers: Record<string, Handler> = {}
const events = new Map<string, Set<(payload: unknown) => void>>()
export const opened: string[] = []

export function hostError(code: string, message: string): Error & { code: string; retryable: boolean } {
  return Object.assign(new Error(message), { code, retryable: false })
}

export function installHandlers(next: Record<string, Handler> | undefined): void {
  handlers = next ?? {}
  events.clear()
}

export function emit(event: string, payload?: unknown): void {
  for (const listener of events.get(event) ?? []) listener(payload)
}

export const plugin = {
  async request(name: string, input?: unknown): Promise<unknown> {
    const handler = handlers[name]
    // 실제 Orca에서는 점검 실행 워커가 답한다. 처리기가 없으면 그 실패를 흉내 낸다.
    if (!handler) throw hostError('unavailable', `${name}: 처리기 없음(스토리)`)
    await new Promise(resolve => setTimeout(resolve, 120))
    return handler(input)
  },
  on(event: string, listener: (payload: unknown) => void): Disposable {
    const set = events.get(event) ?? new Set()
    set.add(listener)
    events.set(event, set)
    return { dispose: () => set.delete(listener) }
  },
}

export const shell = {
  async openExternal(url: string): Promise<{ opened: boolean }> {
    opened.push(url)
    console.info('[orca 대역] openExternal', url)
    return { opened: true }
  },
}

export const host = {
  async call(method: string): Promise<never> {
    throw hostError('unsupported', `${method}: 스토리에서 지원하지 않음`)
  },
  has: () => false,
}

export const api = { major: 1, minor: 0 }
export const env = { locale: 'ko', theme: { colorScheme: 'dark' as 'light' | 'dark' }, client: 'desktop' as const }
export const ui = { catalog: 2, has: () => true }
