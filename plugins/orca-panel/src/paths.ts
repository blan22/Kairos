/**
 * 워커만 쓰는 경로. 패널(UI)은 이 파일을 읽지 않는다.
 * 경로는 설치본과 같아야 하고 외부 입력이나 환경 변수로 받지 않는다. 사용자 홈과 OS만 운영체제에서 읽는다.
 * kairos는 실행 파일이라 워커의 Node 버전과 무관하게 바로 부른다.
 * `current`는 `kairos install --apply`가 설치한 판을 가리키게 둔 링크다. 판이 바뀌어도 여기를 고치지 않는다.
 * OS별 자리는 `kairos_core::platform::Os::default_data_dir`, `packages/kairos-client`의 `homeFor`와 같다.
 */
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
/** macOS는 앱 번들 안의 고정 경로다. 다른 OS는 설치 위치가 하나가 아니라 PATH의 orca를 부른다. */
export const ORCA_BIN = process.platform === 'darwin' ? '/Applications/Orca.app/Contents/Resources/bin/orca' : 'orca'
