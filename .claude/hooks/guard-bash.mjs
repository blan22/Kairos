// PreToolUse(Bash). 운영 kairos, launchd, Orca 플러그인 상태를 바꾸는 명령과 노출 예산을 쓰는 명령을 막는다.
import { decide, input } from './lib.mjs'

// 셸이 걷어 내는 따옴표와 역슬래시를 먼저 지운다. "--out""put"이나 --o\utput 같은 표기로 규칙을 피하지 못하게 한다.
const raw = String(input().tool_input?.command ?? '')
const command = raw.replace(/["'\\]/g, '')
// 설치본을 경로로 부르면 이름 뒤에 따옴표가 붙는다("…/kairos" sessions on).
const K = String.raw`\bkairos["']?\s+`
const rules = [
  [/\blaunchctl\s+(bootstrap|bootout|load|unload|kickstart|enable|disable)\b/, 'deny', 'launchd 잡은 사용자가 kairos install --register로 바꾼다'],
  [new RegExp(`${K}install\\s+--(apply|register)\\b`), 'ask', '운영 설치본을 바꾼다. 사용자 확인이 필요하다'],
  [new RegExp(`${K}uninstall\\b(?!.*--dry-run)`), 'deny', '운영 잡을 내린다. 사용자가 직접 한다'],
  [new RegExp(`${K}next\\b.*--seen\\b`), 'deny', '--seen은 실제 노출과 예산에 남는다. 시험은 --seen 없이 임시 KAIROS_HOME에서 본다'],
  [new RegExp(`${K}sessions\\s+on\\b`), 'deny', '세션 잠금은 차단 시험(S3, S4)을 본 사람이 켠다'],
  [/Application Support\/kairos\/(current|[0-9.]+)\/bin\/kairos(-run)?\b/, 'ask', '설치된 운영 kairos를 부른다. 상태를 바꾸는 명령이면 사용자 확인이 필요하다'],
  [/\borca\s+plugin\s+shot\s+local\.kairos\b/, 'deny', '설치된 패널을 열면 next --seen이 불려 노출 예산을 쓴다. 폴더와 --scenario로 찍는다'],
  [/\borca\s+plugin\s+shot\b/, 'ask', 'Orca 창을 숨겨야 해서 이 세션도 가려진다. 매번 사용자 확인을 받는다'],
  [/\borca\s+plugin\s+(enable|install|system|dev\s+(load|unload|reload))\b/, 'deny', 'Orca 플러그인 승인과 적재는 사용자의 일이다'],
  [/\bgit\s+push\b.*(--force|-f\b|--force-with-lease)/, 'deny', '강제 푸시는 하지 않는다'],
  [/\b(rm|mv|cp|tee)\b[^|;&]*Application Support\/kairos/, 'deny', '운영 데이터 폴더는 에이전트가 고치지 않는다'],
  [/>\s*["']?[^|;&]*Application Support\/kairos/, 'deny', '운영 데이터 폴더는 에이전트가 고치지 않는다'],
  // git은 겹치지 않는 줄임(--ou, --outp)도 --output으로 받는다. --ou로 시작하는 인자를 모두 막는다(--oneline은 지나간다).
  // format-patch는 -o, --output-directory가 없어도 현재 폴더에 패치 파일을 쓴다. --stdout일 때만 지나간다.
  [/\bgit\s+format-patch\b(?![^|;&]*\s--stdout\b)/, 'deny', 'git format-patch는 파일을 쓴다. --stdout으로 본다'],
  [/\bgit\s+format-patch\b[^|;&]*\s(-[a-zA-Z]*o|--output-directory)/, 'deny', 'git format-patch -o는 파일을 쓴다'],
  [/\bgit\b[^|;&]*\s--ou[a-z-]*(=|\s|$)/, 'deny', 'git의 --output은 파일을 쓴다. 출력은 표준 출력으로 본다'],
  [/\bnpm\b.*\brun\s+shots\b/, 'ask', 'Orca 창을 숨겨야 해서 이 세션도 가려진다. 매번 사용자 확인을 받는다'],
  [/\bsecurity\s+(find|add|delete)-generic-password\b/, 'deny', '키체인의 세션 토큰은 kairos connect session-token만 다룬다'],
]
// 첫 일치에서 멈추지 않고 모든 규칙을 본 뒤 가장 엄한 결정을 낸다. 앞의 ask가 뒤의 deny를 가리지 않는다.
const hits = rules.filter(([pattern]) => pattern.test(command))
const strictest = hits.find(([, decision]) => decision === 'deny') ?? hits.find(([, decision]) => decision === 'ask')
if (strictest) decide('PreToolUse', strictest[1], strictest[2])
process.exit(0)
