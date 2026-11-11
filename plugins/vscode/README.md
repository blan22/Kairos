# kairos VS Code 확장

활동 막대의 kairos 보기에 남은 일을 Orca 패널과 같은 묶음(긴급, 세션, 에이전트 제안, 오늘, 여유 있을 때, 미룬 일, 이전부터 있던 일)으로 보여 준다. 낱말도 패널과 같다.

- 줄 앞의 아이콘은 사유(답장, 리뷰, 담당, 일정, 배포, 세션)를 말한다. 마우스를 올리면 전체 제목, 맡길 범위, 항목 ID가 나온다. 제목 앞에 ID를 붙이지 않는다.
- 줄에 마우스를 올리면 주 행동(세션 열고 시작 또는 세션으로 이동), 원문 열기, 완료 버튼이 보인다. 나머지(나중에, 새 소식이 오면 다시, 배포 때 다시, PR 올리기, 되돌리기)는 오른쪽 클릭 메뉴에 있다.
- 세션 열고 시작은 맡길 범위가 있는 일에만 있고, 누르면 범위를 보여 주는 확인 창을 띄운다.
- 여유 있을 때는 6건 이상 쌓이고 오늘 할 일이 있으면 접고, 미룬 일과 이전부터 있던 일은 늘 접어서 연다. 사용자가 바꾼 펼침 상태는 VS Code가 기억한다.
- 상태 표시줄과 활동 막대 배지는 재촉하는 묶음(긴급, 세션, 제안, 오늘)의 건수다. 긴급이 있으면 상태 표시줄이 오류 색이 된다.
- 처리하는 동안 보기 위쪽에 진행 막대가 돈다.

- kairos 호출은 `packages/kairos-client`로만 한다. 실행 파일은 `~/Library/Application Support/kairos/current/bin/kairos`이고 본인 소유이며 다른 사용자가 고칠 수 없을 때만 부른다.
- `kairos watch --json`의 신호와 60초 폴링으로 다시 그린다. 알림을 누르면(`reveal.json`) 그 항목으로 간다.
- 세션 시작은 `kairos accept`다. `config.toml`의 `[sessions] host = "git"`이면 kairos가 만든 worktree에서 `kairos-run`을 VS Code 터미널로 띄운다. 셸을 거치지 않고 실행 파일과 run ID만 넘긴다.
- 버튼은 kairos가 준 `actions`의 이름만 보고 인자는 공용 클라이언트가 새로 만든다. 원문 열기는 허용한 호스트의 https만 연다.

## 빌드

```
npm install
npm run check   # 타입 검사
npm test        # 모델 시험
npm run package # dist/extension.cjs와 .vsix
```

설정에서 `[hosts] enabled`에 `vscode`를 넣으면 알림을 눌렀을 때 Orca를 부르지 않는다(넣지 않아도 확장은 돈다).
