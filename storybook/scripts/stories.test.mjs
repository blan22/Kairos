// 스토리 파일이 시나리오 목록과 같은지 본다. 다르면 npm run gen을 돌린다.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { render, STORIES_FILE } from './gen-stories.mjs'
import { scenarioNames } from './scenarios.mjs'

test('스토리는 panel.check.ts의 시나리오와 같다', () => {
  assert.equal(readFileSync(STORIES_FILE, 'utf8'), render(scenarioNames()))
})

test('시나리오가 비어 있지 않다', () => {
  assert.ok(scenarioNames().length > 10)
})
