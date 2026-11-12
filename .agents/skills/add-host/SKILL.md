---
name: add-host
description: 새 호스트(편집기나 에이전트 도구)의 어댑터를 추가할 때 쓴다. 호스트 계약, 공용 클라이언트, 설정의 호스트 이름, 수락 호출자 판정을 맞춘다.
argument-hint: "<호스트 이름>"
---

1. 먼저 `docs/surface-protocol.md`를 읽는다. 호스트는 화면 그리기와 버튼만 맡고 판단은 kairos에 둔다.
2. 이름을 정하고 `kairos-config`의 `HOSTS`, `commands/surface.rs`의 `SURFACES`, `packages/kairos-client/src/contract.ts`의 `Surface`에 추가한다.
3. 어댑터는 `plugins/<호스트>/`에 둔다. 호스트가 폴더 밖의 모듈을 읽을 수 있으면 `packages/kairos-client`를 번들해서 쓴다. 읽을 수 없으면 같은 규칙(셸 없는 argv, ID 정규식, 계약 버전 검사, 실행 파일 소유와 권한 검사)을 자체 코드로 지킨다.
4. 버튼은 kairos가 준 `actions`의 이름만 참고하고 인자는 `argvFor`로 새로 만든다. 외부 문자열은 텍스트로만 그리고, URL은 허용 호스트의 https만 사용자가 눌렀을 때 연다.
5. 세션 시작을 허용할 호스트라면 `accept --from <호스트>`의 판정(`commands/sessions.rs`의 `inside_session`)을 그 호스트에 맞춰 좁게 추가하고, `hosts.enabled`로 켰을 때만 열리게 한다. git 세션 호스트의 `launch`는 `launchOf`로 검사한 뒤 사용자가 누른 순간에만 실행한다.
6. UI가 있으면 `storybook/`에 스토리를 추가한다.
7. 테스트: 어댑터의 순수 모델 테스트(node --test), CLI 프로세스 테스트(`crates/kairos-cli/tests/hosts_cli.rs` 방식), `scripts/verify.sh full`. 보안 경계를 건드렸다면 `security-reviewer`로 검토한다.
