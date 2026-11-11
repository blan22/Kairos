import type { StorybookConfig } from '@storybook/react-vite'
import { fileURLToPath } from 'node:url'

const shim = (file: string) => fileURLToPath(new URL(`../src/orca/${file}`, import.meta.url))

const config: StorybookConfig = {
  stories: ['../src/stories/*.stories.tsx'],
  framework: { name: '@storybook/react-vite', options: {} },
  staticDirs: ['../public'],
  core: { disableTelemetry: true },
  // 플러그인 코드는 `orca`를 가져오고 JSX를 `orca/jsx-runtime`으로 그린다. 모두 브라우저 대역으로 돌린다.
  viteFinal: async config => {
    config.resolve ??= {}
    config.resolve.alias = [
      { find: /^orca\/testing$/, replacement: shim('testing.ts') },
      { find: /^orca\/jsx-runtime$/, replacement: shim('jsx-runtime.ts') },
      { find: /^orca\/jsx-dev-runtime$/, replacement: shim('jsx-dev-runtime.ts') },
      { find: /^orca$/, replacement: shim('index.tsx') },
    ]
    // 플러그인 폴더의 tsconfig는 Orca가 만드는 타입 폴더(.orca/types)를 상속한다. 그 폴더가 없어도 빌드되게 직접 준다.
    config.esbuild = {
      ...(config.esbuild || {}),
      jsx: 'automatic',
      jsxImportSource: 'orca',
      // 문자열이어야 Vite가 tsconfig 파일을 찾지 않는다.
      tsconfigRaw: JSON.stringify({ compilerOptions: { jsx: 'react-jsx', jsxImportSource: 'orca', verbatimModuleSyntax: true } }),
    }
    config.server = { ...(config.server ?? {}), fs: { allow: ['..'] } }
    return config
  },
}
export default config
