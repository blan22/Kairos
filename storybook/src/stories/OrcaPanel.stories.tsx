// scripts/gen-stories.mjs가 만든 파일이다. 손으로 고치지 않는다.
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

export const WorkerInCheckMode: Story = { args: { scenario: 'workerInCheckMode' } }
export const GateClosed: Story = { args: { scenario: 'gateClosed' } }
export const GatePaused: Story = { args: { scenario: 'gatePaused' } }
export const GateDisplayOff: Story = { args: { scenario: 'gateDisplayOff' } }
export const GateNoCollection: Story = { args: { scenario: 'gateNoCollection' } }
export const Empty: Story = { args: { scenario: 'empty' } }
export const Card: Story = { args: { scenario: 'card' } }
export const OfferCard: Story = { args: { scenario: 'offerCard' } }
export const OfferRows: Story = { args: { scenario: 'offerRows' } }
export const OfferLocked: Story = { args: { scenario: 'offerLocked' } }
export const OfferUrgentRow: Story = { args: { scenario: 'offerUrgentRow' } }
export const UrgentCard: Story = { args: { scenario: 'urgentCard' } }
export const EveryGroup: Story = { args: { scenario: 'everyGroup' } }
export const Running: Story = { args: { scenario: 'running' } }
export const SessionReady: Story = { args: { scenario: 'sessionReady' } }
export const SessionQuestion: Story = { args: { scenario: 'sessionQuestion' } }
export const SessionAttention: Story = { args: { scenario: 'sessionAttention' } }
export const SessionEndedNews: Story = { args: { scenario: 'sessionEndedNews' } }
export const SessionManyNews: Story = { args: { scenario: 'sessionManyNews' } }
export const SessionFailed: Story = { args: { scenario: 'sessionFailed' } }
export const SessionFailedGateClosed: Story = { args: { scenario: 'sessionFailedGateClosed' } }
export const SessionEnded: Story = { args: { scenario: 'sessionEnded' } }
export const Bundle: Story = { args: { scenario: 'bundle' } }
export const MergedCard: Story = { args: { scenario: 'mergedCard' } }
export const CarryoverCard: Story = { args: { scenario: 'carryoverCard' } }
export const LongAndDangerousTitle: Story = { args: { scenario: 'longAndDangerousTitle' } }
export const SlackTitle: Story = { args: { scenario: 'slackTitle' } }
export const Health: Story = { args: { scenario: 'health' } }
export const HealthWithRows: Story = { args: { scenario: 'healthWithRows' } }
export const HoldDayOff: Story = { args: { scenario: 'holdDayOff' } }
export const HoldOffHours: Story = { args: { scenario: 'holdOffHours' } }
export const HoldFocus: Story = { args: { scenario: 'holdFocus' } }
export const HoldLunch: Story = { args: { scenario: 'holdLunch' } }
export const HoldBudgetHour: Story = { args: { scenario: 'holdBudgetHour' } }
export const HoldBudgetDay: Story = { args: { scenario: 'holdBudgetDay' } }
export const HoldNothing: Story = { args: { scenario: 'holdNothing' } }
export const HoldFocusEmpty: Story = { args: { scenario: 'holdFocusEmpty' } }
export const FocusEnded: Story = { args: { scenario: 'focusEnded' } }
export const FocusActive: Story = { args: { scenario: 'focusActive' } }
export const Workday: Story = { args: { scenario: 'workday' } }
export const Items40: Story = { args: { scenario: 'items40' } }
export const Items120: Story = { args: { scenario: 'items120' } }
export const ParkedRows: Story = { args: { scenario: 'parkedRows' } }
export const BaselineRows: Story = { args: { scenario: 'baselineRows' } }
export const ManualRow: Story = { args: { scenario: 'manualRow' } }
export const KairosFailed: Story = { args: { scenario: 'kairosFailed' } }
export const ListFailed: Story = { args: { scenario: 'listFailed' } }
export const NextFailed: Story = { args: { scenario: 'nextFailed' } }
export const WorkerDown: Story = { args: { scenario: 'workerDown' } }
