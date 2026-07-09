# rhwp-mcp shared fidelity 정책

이 문서는 [`rhwp-mcp-shared-fidelity-policy.json`](rhwp-mcp-shared-fidelity-policy.json)의
사람용 설명입니다. 목적은 외부 `hwp-mcp`와 비교할 때 “정확도”를 막연히
말하지 않고, 작은 공유 fixture set에서 어떤 신호가 유지되어야 하는지 고정하는
것입니다.

## 대표 set

| 대표 비교 | 기준 | 의도 |
|---|---|---|
| 정부 HWPX 동일 수정본 | text/profile/style signature, page count, render geometry, PNG diff, package drift | 정부 양식 중간 문구 수정 후 semantic/visual 보존 |
| 업무계획 HWPX 동일 수정본 | text/profile/style signature, page count, render geometry, PNG diff, line-segment drift | 작은 업무 문서의 HWPX 저장/재오픈 보존 |
| 해외직접투자 HWPX 동일 수정본 | `preserve_source_line_segments`, page 1/target page render geometry/PNG, style signature, lineSeg 진단 | 긴 보도자료에서 국소 수정 후 페이지/위치 안정성 확인 |
| 차트/OLE fixture | 원본 렌더 PNG diff 0, local chart semantic edit/save/reopen, external chart tool 0개 | 렌더가 같더라도 local만 제공하는 차트 의미 편집 surface 기록 |
| HWP binary fixture | 양쪽 read/render 가능, 공통 본문 문구, text 추출량, page 1 PNG diff 1.0% 이하 및 mean diff 1.0 이하 | `.hwp` 바이너리 읽기/렌더링의 공통 최소 기준 고정. font fallback/rasterization 차이는 bounded difference로 보되, layout/text 손실은 통과시키지 않음 |

## 운영 원칙

- 이 정책은 `scripts/mcp_shared_fidelity_gate.mjs`가 읽는 source of truth입니다.
- full corpus benchmark가 아닙니다. 사용자가 요청한 대로 수십 개 유사 파일을
  돌리는 대신 정부/업무/보도자료/차트/HWP 바이너리의 서로 다른 위험 영역을
  작게 섞습니다.
- raw package byte-for-byte 동일성은 이 정책의 성공 기준이 아닙니다. text,
  profile, style signature, render geometry, PNG diff를 먼저 보고 남은 package
  drift는 허용 목록으로 제한합니다.
- 기준을 완화해야 할 때는 visual/semantic 영향이 없는 근거와 함께 보고서에
  이유를 남겨야 합니다.
- 2026-07-03 재실행에서 HWP binary page 1은 local/external 모두 같은 위치와
  공통 문구를 렌더했지만, local SVG의 Linux fallback font chain과 external
  SVG의 fallback chain 차이 때문에 제목/날짜 글자 raster가 달라졌습니다. 이
  정책은 해당 case를 정확히 같은 픽셀 기준이 아니라 `completed_with_difference`
  대표 case로 유지합니다.
