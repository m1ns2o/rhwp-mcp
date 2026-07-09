# rhwp-mcp 대표 visual baseline 정책

이 정책은 `scripts/capture_mcp_government_report.mjs`가 읽는
[`rhwp-mcp-visual-baseline-policy.json`](rhwp-mcp-visual-baseline-policy.json)의
사람용 설명입니다. 목적은 수십 개 유사 fixture를 반복하지 않고, 성격이
다른 대표 문서 몇 개로 렌더링/저장/재생성 회귀를 잡는 것입니다.

## 공통 기준

| 항목 | 값 |
|---|---:|
| crop 영역 | `x=50, y=64, width=860, height=1208` |
| pixel changed 판정 threshold | channel delta `16` 초과 |
| shift 보정 탐색 | `±2px` |

## visual diff gate

| 대표 diff | 최대 changed % | 최대 mean abs diff | 의도 |
|---|---:|---:|---|
| `government-original-vs-edited-page18-diff` | `0.1` | `0.15` | 정부 양식 중간 제목만 바꾼 localized edit |
| `government-original-vs-regenerated-page18-diff` | `0.02` | `0.03` | 정부 양식 같은 page template 재생성 |
| `government-original-vs-regenerated-best-match-diff` | `0.02` | `0.03` | 정부 양식 best-match template 재생성 |
| `foreign-investment-original-vs-reopened-page1-diff` | `0.0001` | `0.0001` | 작성된 HWPX 저장/재오픈 page 1 동일성 |
| `business-overview-original-vs-reopened-page1-diff` | `0.0001` | `0.0001` | 짧은 업무 HWPX 저장/재오픈 page 1 동일성 |
| `k-water-rfp-original-vs-edited-page1-diff` | `0.35` | `0.65` | 긴 RFP HWPX 표지 날짜 localized edit |
| `k-water-rfp-edited-vs-reopened-page1-diff` | `0.0001` | `0.0001` | 날짜 수정된 긴 RFP HWPX 저장/재오픈 page 1 동일성 |
| `hwp-multi-original-vs-reopened-page1-diff` | `0.0001` | `0.0001` | 다중 표/그림 HWP 저장/재오픈 page 1 동일성 |
| `field-memo-original-vs-edited-page2-diff` | `0.8` | `1.2` | 작성물 field/memo HWP shape text-box 필드 국소 수정 |
| `field-memo-edited-vs-reopened-page2-diff` | `0.0001` | `0.0001` | 수정된 field/memo HWP 저장/재오픈 page 2 동일성 |
| `endnote-equation-original-vs-reopened-page1-diff` | `0.0001` | `0.0001` | 미주/수식 HWP 저장/재오픈 page 1 동일성 |
| `water-mark-original-vs-reopened-page1-diff` | `0.0001` | `0.0001` | water-mark HWP 저장/재오픈 page 1 동일성 |
| `hcar-original-vs-regenerated-page1-diff` | `1.2` | `0.6` | 실제 다중 section 양식 page 1 재생성 허용치 |
| `hcar-original-vs-regenerated-best-match-diff` | `1.2` | `0.6` | 실제 다중 section 양식 page 1 best-match 허용치 |
| `hcar-original-vs-regenerated-page2-diff` | `0.01` | `0.01` | 실제 다중 section 양식 page 2 사실상 동일성 |
| `hcar-original-vs-regenerated-page2-best-match-diff` | `0.01` | `0.01` | 실제 다중 section 양식 page 2 best-match 동일성 |
| `hcar-original-vs-regenerated-page3-diff` | `0.1` | `0.08` | 실제 다중 section 양식 page 3 재생성 허용치 |
| `hcar-original-vs-regenerated-page3-best-match-diff` | `0.1` | `0.08` | 실제 다중 section 양식 page 3 best-match 허용치 |

## render page-match gate

| 대표 page | 최소 score | 최소 text similarity |
|---|---:|---:|
| hcar page 1 | `0.999` | `0.999` |
| hcar page 2 | `0.9999` | `0.9999` |
| hcar page 3 | `0.995` | `0.99` |

## 운영 원칙

- 이 정책은 대표 회귀 gate입니다. 전체 corpus 정확도 인증이 아닙니다.
- 저장/재오픈 baseline은 가능한 한 `0.0000%` visual diff를 요구합니다.
- 양식 재생성 baseline은 현재 구현의 실제 잔여 차이를 반영하되, 수치가
  나빠지면 CI에서 실패하도록 둡니다.
- threshold를 완화할 때는 해당 문서의 원본/수정본/재생성본 캡처와
  diff 이미지를 함께 갱신하고, 보고서에 이유를 남깁니다.
