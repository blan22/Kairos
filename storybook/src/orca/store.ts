/**
 * Orca atom 대역. 패널은 렌더 중에 read()로 값을 읽고, set()/update()가 바뀌면 Orca가 다시 그린다.
 * 여기서는 바뀔 때마다 판 번호를 올리고 구독자(스토리 틀)가 전체를 다시 그린다.
 * 키가 있는 atom(persist)은 스토리마다 비우고, 시나리오의 storage가 있으면 그 값으로 채운다. 실제 Orca의 저장은 흉내 내지 않는다.
 */
export type Atom<T> = { readonly __orcaAtom: T; readonly __orcaWritable: true; key: string | null; initial: T }
export type ReadonlyAtom<T> = { readonly __orcaAtom: T; compute?: () => T }

const values = new Map<object, unknown>()
const keyed = new Map<string, unknown>()
const listeners = new Set<() => void>()
let version = 0

export function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export const snapshot = (): number => version

function changed(): void {
  version += 1
  for (const listener of [...listeners]) listener()
}

export function resetStore(seed: Record<string, unknown> = {}): void {
  values.clear()
  keyed.clear()
  for (const [key, value] of Object.entries(seed)) keyed.set(key, value)
  version += 1
}

export function atom<T>(keyOrInitial: string | T, maybeInitial?: T): Atom<T> {
  if (typeof keyOrInitial === 'string' && maybeInitial !== undefined) {
    return { key: keyOrInitial, initial: maybeInitial } as unknown as Atom<T>
  }
  return { key: null, initial: keyOrInitial } as unknown as Atom<T>
}

export function derived<T>(compute: () => T): ReadonlyAtom<T> {
  return { compute } as unknown as ReadonlyAtom<T>
}

export function read<T>(target: ReadonlyAtom<T> | Atom<T>): T {
  const derivedAtom = target as ReadonlyAtom<T>
  if (typeof derivedAtom.compute === 'function') return derivedAtom.compute()
  const writable = target as Atom<T>
  if (writable.key !== null) return (keyed.has(writable.key) ? keyed.get(writable.key) : writable.initial) as T
  return (values.has(writable) ? values.get(writable) : writable.initial) as T
}

export function set<T>(target: Atom<T>, value: T): void {
  if (target.key !== null) keyed.set(target.key, value)
  else values.set(target, value)
  changed()
}

export function update<T>(target: Atom<T>, updater: (previous: T) => T): void {
  set(target, updater(read(target)))
}
