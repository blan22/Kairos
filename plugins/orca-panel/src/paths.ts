/**
 * 워커만 쓰는 경로. 패널(UI)은 이 파일을 읽지 않는다.
 * 경로는 설치본과 같아야 하고 외부 입력이나 환경 변수로 받지 않는다. 사용자 홈만 운영체제에서 읽는다.
 * kairos는 실행 파일이라 워커의 Node 버전과 무관하게 바로 부른다.
 * `current`는 `kairos install --apply`가 설치한 판을 가리키게 둔 링크다. 판이 바뀌어도 여기를 고치지 않는다.
 */
import { homedir } from 'node:os'
import { join } from 'node:path'

/** kairos 홈의 기본값이다. 알림을 누르면 kairos reveal이 여기에 REVEAL_FILE을 쓴다. */
export const KAIROS_HOME = join(homedir(), 'Library', 'Application Support', 'kairos')
export const KAIROS_BIN = join(KAIROS_HOME, 'current', 'bin', 'kairos')
export const ORCA_BIN = '/Applications/Orca.app/Contents/Resources/bin/orca'
