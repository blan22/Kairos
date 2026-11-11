/**
 * 실제 Orca 렌더 캡처. `npm run shots`가 `orca plugin shot --scenario`로 public/shots/<시나리오>-<테마>.png를 찍는다.
 * 캡처는 Orca 창을 숨겨야 해서 사람이 직접 돌린다. 파일이 없으면 안내만 보인다.
 * 브라우저 대역(Orca 패널/시나리오)과 나란히 놓고 모양 차이를 본다.
 */
import type { Meta, StoryObj } from '@storybook/react'
import { useState } from 'react'
import checks from '../../../plugins/orca-panel/src/panel.check.ts'
import { PanelFrame, type Scenario } from '../PanelFrame.tsx'

type Args = { scenario: keyof typeof checks; theme: 'dark' | 'light' }

function Compare({ scenario, theme }: Args) {
  const [missing, setMissing] = useState(false)
  const src = `shots/${scenario}-${theme}.png`
  return (
    <div style={{ display: 'flex', gap: 24, alignItems: 'flex-start' }}>
      <figure style={{ margin: 0 }}>
        <figcaption>실제 Orca</figcaption>
        {missing ? (
          <div className="sb-missing">캡처 없음. 저장소 루트에서 <code>npm --prefix storybook run shots</code>를 실행하세요(Orca 창이 잠시 숨겨집니다).</div>
        ) : (
          <img src={src} alt={`${scenario} 실제 렌더`} width={360} onError={() => setMissing(true)} />
        )}
      </figure>
      <figure style={{ margin: 0 }}>
        <figcaption>브라우저 대역</figcaption>
        <PanelFrame scenario={{ ...(checks[scenario] as Scenario), theme }} />
      </figure>
    </div>
  )
}

const names = Object.keys(checks) as (keyof typeof checks)[]
const meta: Meta<Args> = {
  title: 'Orca 패널/실제 렌더 비교',
  component: Compare,
  args: { scenario: 'workday', theme: 'dark' },
  argTypes: { scenario: { control: 'select', options: names }, theme: { control: 'inline-radio', options: ['dark', 'light'] } },
}
export default meta
export const Compare_: StoryObj<Args> = { name: '비교' }
