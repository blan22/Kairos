/**
 * kairos VS Code 확장. 판단은 kairos에 있고 확장은 그리기와 버튼만 맡는다.
 * kairos 호출은 공용 클라이언트(packages/kairos-client)로만 한다. 버튼은 고정 이름과 항목 ID만 넘긴다.
 * 세션 호스트가 git이면 수락이 돌려준 명령을 사용자가 누른 순간에만 VS Code 터미널로 띄운다(셸을 거치지 않는다).
 */
import * as vscode from 'vscode'
import { createClient, launchOf, type ItemAction, type KairosClient, type Reply } from '../../../packages/kairos-client/src/index.ts'
import { ACTION_LABELS, contextOf, countsOf, GROUP_LABELS, GROUP_ORDER, itemsOf, revealOf, startsCollapsed, type Group, type ItemNode } from './model.ts'

const POLL_MS = 60_000
/** 성공한 결과의 status. kairos의 reply.rs와 같다. */
const SUCCESS: ReadonlySet<string> = new Set(['changed', 'unchanged', 'already_closed'])

type Node = { kind: 'group'; group: Group; count: number } | { kind: 'item'; item: ItemNode }

class ItemsProvider implements vscode.TreeDataProvider<Node> {
  private readonly changed = new vscode.EventEmitter<Node | undefined>()
  readonly onDidChangeTreeData = this.changed.event
  private items: ItemNode[] = []
  private message: string | null = null

  update(items: ItemNode[], message: string | null): void {
    this.items = items
    this.message = message
    this.changed.fire(undefined)
  }

  get counts(): { pending: number; urgent: number } {
    return countsOf(this.items)
  }

  find(id: number): Node | undefined {
    const item = this.items.find(entry => entry.id === id)
    return item ? { kind: 'item', item } : undefined
  }

  getTreeItem(node: Node): vscode.TreeItem {
    if (node.kind === 'group') {
      const collapsed = startsCollapsed(node.group, this.items)
      const tree = new vscode.TreeItem(GROUP_LABELS[node.group], collapsed ? vscode.TreeItemCollapsibleState.Collapsed : vscode.TreeItemCollapsibleState.Expanded)
      tree.description = String(node.count)
      tree.contextValue = 'group'
      // id가 있어야 사용자가 접고 편 상태를 VS Code가 새로고침 뒤에도 기억한다.
      tree.id = `group-${node.group}`
      return tree
    }
    const tree = new vscode.TreeItem(node.item.label, vscode.TreeItemCollapsibleState.None)
    tree.description = node.item.description
    // 외부 제목은 MarkdownString이 아니라 글자로만 넘긴다.
    tree.tooltip = node.item.tooltip
    tree.iconPath = new vscode.ThemeIcon(node.item.icon)
    tree.contextValue = contextOf(node.item)
    tree.id = `item-${node.item.id}`
    return tree
  }

  getChildren(node?: Node): Node[] {
    if (node?.kind === 'item') return []
    if (node?.kind === 'group') {
      return this.items.filter(item => item.group === node.group).map(item => ({ kind: 'item', item }))
    }
    if (this.message !== null && this.items.length === 0) return []
    return GROUP_ORDER.map(group => ({ kind: 'group' as const, group, count: this.items.filter(item => item.group === group).length })).filter(
      group => group.count > 0,
    )
  }

  getParent(node: Node): Node | undefined {
    if (node.kind === 'group') return undefined
    return { kind: 'group', group: node.item.group, count: this.items.filter(item => item.group === node.item.group).length }
  }

  get emptyMessage(): string | undefined {
    return this.message ?? (this.items.length === 0 ? '지금은 챙길 일이 없습니다' : undefined)
  }
}

export function activate(context: vscode.ExtensionContext): void {
  const client: KairosClient = createClient({ surface: 'vscode' })
  const provider = new ItemsProvider()
  const view = vscode.window.createTreeView('kairos.items', { treeDataProvider: provider })
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 50)
  status.command = 'kairos.items.focus'
  status.show()
  let lastReveal = context.globalState.get<number>('kairos.revealAt', 0)
  let refreshing = false

  const refresh = async (): Promise<void> => {
    if (refreshing) return
    refreshing = true
    try {
      const listed = await client.list()
      if (!listed.ok) {
        provider.update([], listed.failure.message)
        view.message = provider.emptyMessage
        view.badge = undefined
        status.text = '$(bell-slash) kairos'
        status.tooltip = `${listed.failure.message} (코드 ${listed.failure.code})`
        status.backgroundColor = undefined
        return
      }
      const display = listed.value.display
      const hidden = typeof display === 'object' && display !== null && (display as { open?: unknown }).open === false
      const message = hidden ? String((display as { message?: unknown }).message ?? '제안이 꺼져 있습니다') : null
      provider.update(itemsOf(listed.value), message)
      view.message = provider.emptyMessage
      const { pending, urgent } = provider.counts
      status.text = `$(bell) kairos ${pending}`
      status.tooltip = urgent > 0 ? `남은 일 ${pending}건 · 긴급 ${urgent}건` : `남은 일 ${pending}건(여유 있을 때 제외)`
      status.backgroundColor = urgent > 0 ? new vscode.ThemeColor('statusBarItem.errorBackground') : undefined
      view.badge = pending > 0 ? { value: pending, tooltip: status.tooltip } : undefined
      const next = await client.next()
      const reveal = next.ok ? revealOf(next.value) : null
      if (reveal && reveal.at > lastReveal) {
        lastReveal = reveal.at
        await context.globalState.update('kairos.revealAt', reveal.at)
        const target = provider.find(reveal.items[0])
        if (target) await view.reveal(target, { focus: true, select: true, expand: true })
      }
    } finally {
      refreshing = false
    }
  }

  const report = (reply: Reply): void => {
    if (!reply.ok) {
      void vscode.window.showWarningMessage(`kairos: ${reply.failure.message}`)
      return
    }
    const results = Array.isArray(reply.value.results) ? reply.value.results : []
    const first = results[0] as { message?: unknown; status?: unknown } | undefined
    if (typeof first?.message !== 'string') return
    const done = typeof first.status === 'string' && SUCCESS.has(first.status)
    void vscode.window.setStatusBarMessage(`$(${done ? 'check' : 'warning'}) kairos: ${first.message.slice(0, 120)}`, 5000)
  }

  /** 세션은 맡길 범위를 본 뒤에만 연다. Orca 패널이 범위 곁에만 시작 버튼을 두는 것과 같다. */
  const confirmStart = async (item: ItemNode): Promise<boolean> => {
    if (item.scope === null) return false
    const picked = await vscode.window.showInformationMessage(
      `${ACTION_LABELS.start}: ${item.label}`,
      { modal: true, detail: `맡길 범위\n${item.scope}\n\n작업 공간을 만든 뒤 세션을 엽니다. 길면 2분쯤 걸립니다.` },
      ACTION_LABELS.start,
    )
    return picked === ACTION_LABELS.start
  }

  /** 처리하는 동안 보기 위쪽에 진행 막대를 띄운다. 오래 걸리는 시작과 PR 올리기도 눌렸다는 것이 보인다. */
  const busy = <T>(task: () => Promise<T>): Thenable<T> => vscode.window.withProgress({ location: { viewId: 'kairos.items' } }, task)

  const act = (action: ItemAction) => async (node?: Node): Promise<void> => {
    if (node?.kind !== 'item') return
    if (action === 'start' && !(await confirmStart(node.item))) return
    const reply = await busy(() => client.act(action, [node.item.id]))
    report(reply)
    if (action === 'start' && reply.ok) {
      const launch = launchOf(reply.value)
      if (launch) {
        const terminal = vscode.window.createTerminal({
          name: `kairos-${launch.argv[1].slice(0, 8)}`,
          cwd: launch.cwd,
          shellPath: launch.argv[0],
          shellArgs: [launch.argv[1]],
        })
        terminal.show()
      }
    }
    await refresh()
  }

  context.subscriptions.push(
    view,
    status,
    vscode.commands.registerCommand('kairos.refresh', () => busy(refresh)),
    vscode.commands.registerCommand('kairos.later', act('later')),
    vscode.commands.registerCommand('kairos.done', act('done')),
    vscode.commands.registerCommand('kairos.keep', act('keep')),
    vscode.commands.registerCommand('kairos.keepDeploy', act('keep_deploy')),
    vscode.commands.registerCommand('kairos.start', act('start')),
    vscode.commands.registerCommand('kairos.move', act('move')),
    vscode.commands.registerCommand('kairos.cancel', act('cancel')),
    vscode.commands.registerCommand('kairos.pr', act('pr')),
    vscode.commands.registerCommand('kairos.open', async (node?: Node) => {
      if (node?.kind === 'item' && node.item.url) await vscode.env.openExternal(vscode.Uri.parse(node.item.url, true))
    }),
    vscode.commands.registerCommand('kairos.add', async () => {
      const title = await vscode.window.showInputBox({
        title: '할 일 추가',
        prompt: '직접 추가한 일은 여유 있을 때 묶음에 들어갑니다',
        placeHolder: '할 일을 적고 Enter',
        validateInput: value => (value.trim() ? null : '내용을 적어 주세요'),
      })
      if (title === undefined) return
      report(await busy(() => client.add(title)))
      await refresh()
    }),
  )
  const stopWatch = client.watch(() => void refresh())
  const timer = setInterval(() => void refresh(), POLL_MS)
  context.subscriptions.push({ dispose: () => { stopWatch(); clearInterval(timer) } })
  void client.hello().then(hello => {
    if (!hello.ok) void vscode.window.showWarningMessage(`kairos: ${hello.failure.message}`)
  })
  void refresh()
}

export function deactivate(): void {}
