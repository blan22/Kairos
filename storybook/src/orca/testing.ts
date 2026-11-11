/** `orca/testing` 대역. panel.check.ts의 시나리오를 그대로 읽으려고 defineCheck와 hostError만 둔다. */
export { hostError } from './host.ts'

export type CheckScenario = {
  requests?: Record<string, (input: unknown) => unknown>
  theme?: 'light' | 'dark'
}

export const defineCheck = <T extends Record<string, CheckScenario>>(scenarios: T): T => scenarios
