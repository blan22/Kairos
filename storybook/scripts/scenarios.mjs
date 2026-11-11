// panel.check.ts의 시나리오 이름. Storybook 인덱서는 내보내기를 정적으로 읽어야 해서 스토리 파일을 이 목록으로 만든다.
import { readFileSync } from 'node:fs'

export const CHECK_FILE = new URL('../../plugins/orca-panel/src/panel.check.ts', import.meta.url)

export function scenarioNames() {
  const text = readFileSync(CHECK_FILE, 'utf8')
  const start = text.indexOf('export default defineCheck({')
  if (start < 0) throw new Error('defineCheck를 찾지 못함')
  return [...text.slice(start).matchAll(/^ {2}([A-Za-z][A-Za-z0-9]*): /gm)].map(match => match[1])
}
