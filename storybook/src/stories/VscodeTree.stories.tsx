/**
 * VS Code 확장의 트리 보기 미리보기. 확장은 VS Code 기본 트리를 쓰므로 그림은 VS Code와 다르다.
 * 여기서 보는 것은 확장의 모델(plugins/vscode/src/model.ts)이 같은 합성 목록을 어떤 묶음, 줄, 버튼으로 바꾸는지다.
 */
import type { Meta, StoryObj } from '@storybook/react'
import { accepted, item, listJson, manyItems, parked, startable, workdayRows, DANGEROUS_TITLE } from '../../../plugins/orca-panel/src/fixtures.ts'
import { ACTION_LABELS, contextOf, GROUP_LABELS, GROUP_ORDER, itemsOf, startsCollapsed } from '../../../plugins/vscode/src/model.ts'

const MENU: Record<string, string> = { ...ACTION_LABELS, url: ACTION_LABELS.open }

function Tree({ list }: { list: Record<string, unknown> }) {
  const nodes = itemsOf(list)
  return (
    <div className="vs-tree">
      <div className="vs-title">KAIROS: 남은 일</div>
      {nodes.length === 0 ? <div className="vs-empty">지금은 챙길 일이 없습니다</div> : null}
      {GROUP_ORDER.map(group => {
        const rows = nodes.filter(node => node.group === group)
        if (rows.length === 0) return null
        return (
          <details key={group} open={!startsCollapsed(group, nodes)}>
            <summary className="vs-group">{GROUP_LABELS[group]} <span className="vs-desc">{rows.length}</span></summary>
            {rows.map(node => (
              <details key={node.id} className="vs-row">
                <summary title={node.tooltip}>
                  <span className="vs-desc">{`$(${node.icon})`}</span> <span className="vs-label">{node.label}</span> <span className="vs-desc">{node.description}</span>
                </summary>
                <div className="vs-menu">
                  {contextOf(node).split(' ').filter(word => MENU[word]).map(word => <span key={word}>{MENU[word]}</span>)}
                </div>
              </details>
            ))}
          </details>
        )
      })}
    </div>
  )
}

const meta: Meta<typeof Tree> = { title: 'VS Code/트리 보기', component: Tree }
export default meta
type Story = StoryObj<typeof Tree>

export const Workday: Story = { args: { list: listJson(workdayRows()) } }
export const Sessions: Story = { args: { list: listJson([accepted(1), accepted(2, 'waiting_user'), startable(3)]) } }
export const DangerousTitle: Story = { args: { list: listJson([item(1, { title: DANGEROUS_TITLE, url: 'https://evil.test/x' })]) } }
export const Parked: Story = { args: { list: listJson([parked(1), parked(2, { parkTrigger: 'deploy' })]) } }
export const Many: Story = { args: { list: listJson(manyItems(40)) } }
export const Empty: Story = { args: { list: listJson([]) } }
