/**
 * 워커만 쓰는 경로. 패널(UI)은 이 파일을 읽지 않는다.
 * 경로는 설치본과 같아야 하고 외부 입력이나 환경 변수로 받지 않는다. 사용자 홈과 OS만 운영체제에서 읽는다.
 * kairos는 실행 파일이라 워커의 Node 버전과 무관하게 바로 부른다.
 * `current`는 `kairos install --apply`가 설치한 판을 가리키게 둔 링크다. 판이 바뀌어도 여기를 고치지 않는다.
 * OS별 자리는 `kairos_core::platform::Os::default_data_dir`, `packages/kairos-client`의 `homeFor`와 같다.
 */
import { statSync } from 'node:fs'
import { homedir } from 'node:os'
import { posix, win32 } from 'node:path'

const path = process.platform === 'win32' ? win32 : posix

function homeOf(platform: NodeJS.Platform, user: string): string {
  if (platform === 'darwin') return path.join(user, 'Library', 'Application Support', 'kairos')
  if (platform === 'win32') return path.join(user, 'AppData', 'Local', 'kairos')
  return path.join(user, '.local', 'share', 'kairos')
}

/** kairos 홈의 기본값이다. 알림을 누르면 kairos reveal이 여기에 REVEAL_FILE을 쓴다. */
export const KAIROS_HOME = homeOf(process.platform, homedir())
export const KAIROS_BIN = path.join(KAIROS_HOME, 'current', 'bin', process.platform === 'win32' ? 'kairos.exe' : 'kairos')
/**
 * PATH에서 orca를 한 번 찾아 절대 경로로 고정한다. 이름만 넘기면 Windows는 현재 폴더부터 찾는다.
 * 절대 경로 항목만 보고, Windows는 `orca.exe`만 받는다(`.cmd`는 셸 없이 띄울 수 없다).
 * unix는 본인이나 root 소유이고 다른 사용자가 고칠 수 없는 파일만 받는다. 못 찾으면 null이고 패널 열기 명령이 안내만 한다.
 */
export function findOrca(platform: NodeJS.Platform, pathList: string, uid: number | null): string | null {
  const paths = platform === 'win32' ? win32 : posix
  const name = platform === 'win32' ? 'orca.exe' : 'orca'
  for (const dir of pathList.split(platform === 'win32' ? ';' : ':')) {
    if (!paths.isAbsolute(dir)) continue
    const file = paths.join(dir, name)
    try {
      const info = statSync(file)
      if (!info.isFile()) continue
      if (platform !== 'win32' && ((info.mode & 0o022) !== 0 || (info.uid !== uid && info.uid !== 0) || (info.mode & 0o111) === 0)) continue
      return file
    } catch {
      continue
    }
  }
  return null
}

/** macOS는 앱 번들 안의 고정 경로다. 다른 OS는 설치 위치가 하나가 아니라 PATH에서 찾는다. */
export const ORCA_BIN: string | null =
  process.platform === 'darwin'
    ? '/Applications/Orca.app/Contents/Resources/bin/orca'
    : findOrca(process.platform, process.env.PATH ?? process.env.Path ?? '', process.getuid?.() ?? null)
