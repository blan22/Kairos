/**
 * `orca` 모듈 대역. Storybook의 Vite가 플러그인 코드의 `import ... from 'orca'`를 여기로 돌린다.
 * 상태(atom), 요청(plugin.request), 이벤트(plugin.on), 컴포넌트를 브라우저에서 흉내 낸다.
 */
import { createElement, Fragment as ReactFragment } from 'react'

export * from './components.tsx'
export { atom, derived, read, set, update } from './store.ts'
export { api, env, host, plugin, shell, ui } from './host.ts'
export type { Atom, ReadonlyAtom } from './store.ts'

export const Fragment = ReactFragment
export const h = createElement
export function mount(): void {
  throw new Error('스토리에서는 mount 대신 스토리 틀이 그린다')
}

export type PanelContext = {
  subscriptions: { dispose(): void }[]
  signal: AbortSignal
  visible: { readonly __orcaAtom: boolean }
  panel: { id: string }
}
export type OrcaNode = unknown
