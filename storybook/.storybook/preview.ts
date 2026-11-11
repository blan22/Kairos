import type { Preview } from '@storybook/react'
import '../src/orca/orca.css'
import '../src/stories/vscode.css'

const preview: Preview = {
  parameters: { layout: 'padded', controls: { expanded: true } },
}
export default preview
