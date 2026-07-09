# rhwp-mcp HWPX package baseline 정책

이 문서는 [`rhwp-mcp-package-baseline-policy.json`](rhwp-mcp-package-baseline-policy.json)의
사람용 설명입니다. 목적은 Playwright visual baseline과 별도로, 외부 MCP와
같은 수정본을 만들었을 때 raw HWPX package 차이가 어느 범위까지 허용되는지
명확히 기록하는 것입니다.

## 기준

| 대표 비교 | 필수 semantic 조건 | 허용 package drift | 허용 구조 drift key | 의도 |
|---|---|---|---|---|
| 정부 양식 동일 수정본 | text/profile/style signature 동일, page count 동일, PNG diff 0 | `semantic_equal_lexical_package_drift` | 없음 | XML 직렬화 수준 차이만 허용 |
| 업무계획 동일 수정본 | text/profile/style signature 동일, page count 동일, PNG diff 0 | `semantic_equal_line_segment_package_drift` | `line_segment_count` | line segment 보존 방식 차이만 허용 |
| 해외직접투자 보도자료 동일 수정본 | `preserve_source_line_segments`, text/profile/style signature 동일, page count 동일, target page PNG/geometry 동일 | `structural_package_difference` | `run_count` | 긴 문서 치환 뒤 run-count 구조 차이만 허용 |

## 운영 원칙

- 이 정책은 `scripts/mcp_package_fidelity_gate.mjs`가 읽는 source of truth입니다.
- raw package가 byte-for-byte로 같아야 한다는 의미가 아닙니다. text/profile/style,
  page count, render geometry, PNG가 유지되는지를 먼저 보고, 남은 raw drift를
  허용 목록으로 제한합니다.
- 허용 drift key가 늘어나면 해당 원인과 시각/semantic 영향이 없는 근거를 보고서에
  함께 남겨야 합니다.
- 이 gate는 full corpus benchmark가 아니라 정부/업무/보도자료 대표 동일 수정본
  세 건의 package-fidelity 회귀 방지용입니다.
