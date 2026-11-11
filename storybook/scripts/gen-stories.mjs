#!/usr/bin/env node
// panel.check.ts의 시나리오마다 스토리 하나를 만든다. 시나리오를 더하면 이 스크립트를 다시 돌린다(npm run gen).
import { writeFileSync } from 'node:fs'
import { scenarioNames } from './scenarios.mjs'

export const STORIES_FILE = new URL('../src/stories/OrcaPanel.stories.tsx', import.meta.url)

export function render(names) {
  const stories = names.map(name => `export const ${name[0].toUpperCase()}${name.slice(1)}: Story = { args: { scenario: '${name}' } }`).join('\n')
  return `// scripts/gen-stories.mjs가 만든 파일이다. 손으로 고치지 않는다.
// 시나리오는 plugins/orca-panel/src/panel.check.ts에 있고, 같은 시나리오로 실제 렌더 캡처(npm run shots)도 찍는다.
import type { Meta, StoryObj } from '@storybook/react'
import checks from '../../../plugins/orca-panel/src/panel.check.ts'
import { PanelFrame, type Scenario } from '../PanelFrame.tsx'

type Args = { scenario: keyof typeof checks; theme: 'dark' | 'light'; width: number }

const meta: Meta<Args> = {
  title: 'Orca 패널/시나리오',
  args: { theme: 'dark', width: 360 },
  argTypes: { theme: { control: 'inline-radio', options: ['dark', 'light'] }, width: { control: { type: 'range', min: 260, max: 640, step: 20 } } },
  render: ({ scenario, theme, width }) => <PanelFrame scenario={{ ...(checks[scenario] as Scenario), theme }} width={width} />,
}
export default meta
type Story = StoryObj<Args>

${stories}
`
}

if (import.meta.url === `file://${process.argv[1]}`) {
  writeFileSync(STORIES_FILE, render(scenarioNames()))
  console.log(`스토리 ${scenarioNames().length}개`)
}
