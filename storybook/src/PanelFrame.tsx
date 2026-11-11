/**
 * Orca 패널을 스토리에서 띄우는 틀. 스토리마다 상태를 비우고, 시나리오의 요청 처리기를 넣고,
 * 패널의 setup(ctx)을 한 번 부른 뒤 기본 컴포넌트를 그린다. atom이 바뀌면 다시 그린다.
 */
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react'
import Panel, { setup } from '../../plugins/orca-panel/src/panel.tsx'
import { emit, installHandlers, type Handler } from './orca/host.ts'
import { atom, resetStore, snapshot, subscribe } from './orca/store.ts'

export type Scenario = { requests?: Record<string, Handler>; theme?: 'light' | 'dark' }

export function PanelFrame({ scenario, width = 360 }: { scenario: Scenario; width?: number }) {
  const [ready, setReady] = useState(false)
  useMemo(() => {
    resetStore()
    installHandlers(scenario.requests)
  }, [scenario])
  useEffect(() => {
    const controller = new AbortController()
    const subscriptions: { dispose(): void }[] = []
    setup({ subscriptions, signal: controller.signal, visible: atom(true), panel: { id: 'kairos' } } as never)
    setReady(true)
    return () => {
      controller.abort()
      for (const subscription of subscriptions) subscription.dispose()
    }
  }, [scenario])
  useSyncExternalStore(subscribe, snapshot)
  return (
    <div className="o-panel" data-theme={scenario.theme ?? 'dark'} style={{ width }}>
      {ready ? <Panel /> : null}
      <div className="sb-tools">
        <button type="button" onClick={() => emit('kairos.tick')}>틱 보내기</button>
        <button type="button" onClick={() => emit('kairos.reveal')}>알림 누름 보내기</button>
      </div>
    </div>
  )
}
