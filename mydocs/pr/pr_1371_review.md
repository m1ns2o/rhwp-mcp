# PR #1371 검토 — 미주 높이 모델 측정 SSOT A3 opt-in

- PR: https://github.com/edwardkim/rhwp/pull/1371
- 제목: Task #1363: 미주 높이 모델 측정 SSOT — scratch 전-단 순차 렌더 (A3 opt-in)
- 작성일: 2026-06-11
- 작성자: `planet6897`
- 관련 이슈: #1363, 후속 #1370
- base: `devel` (`2f985a90`)
- head: `task1363` (`f20199ae`)
- 검토 브랜치: `local/pr1371-upstream`
- 현재 기준 브랜치: `local/devel` (`45da5d91`)

## 1. 요약 판단

**현재 PR 그대로는 수용 불가, 최신 `devel` 기준 재기준화가 필요**하다.

기술 방향은 #1368에서 확인한 잔여 문제와 잘 맞는다. PR은 `RHWP_EN_SSOT=A3` opt-in으로
미주 단의 전 items를 scratch `LayoutEngine::build_single_column` 경로로 한 번 순차 렌더해
typeset 누적과 실제 render bottom을 맞추려 한다. 이는 `pi=1156/1157`처럼 현재 누적기가 단
전환을 늦게 판단하는 문제를 해결할 수 있는 올바른 계열의 접근이다.

다만 PR base가 #1368 수용 전 `devel`이라 현재 `devel`과 충돌한다. 또한 #1368에서 maintainer가
정리한 문서 archive 구조와 `EnSsotLevel` 기본값 주석이 되돌아가는 형태가 포함되어 있고,
`git diff --check`도 trailing whitespace로 실패한다. 따라서 이 PR은 최신 `devel`로 rebase 또는
maintainer 충돌 정리 후 다시 검증해야 한다.

수용하더라도 A3는 **기본 승격 대상이 아니다**. PR 본문과 보고서가 이미 밝히듯 A3에는 13건
hancom 배치 재보정 잔여와 export-svg CLI A3 페이지 폭발 이슈가 남아 있다. 이번 PR은 A3
측정 인프라의 opt-in 수용 범위로만 판단해야 하며 #1363 close는 보류한다.

## 2. PR 정보

| 항목 | 값 |
|---|---|
| 상태 | open |
| draft | false |
| mergeable | `CONFLICTING` |
| mergeStateStatus | `DIRTY` |
| 커밋 | 1 |
| 변경량 | 25 files, +9346 / -71 |
| 작성자 | `planet6897` |

커밋:

- `f20199ae` — Task #1363: 미주 높이 모델 측정 SSOT — scratch 전-단 순차 렌더 (A3 opt-in)

GitHub checks:

| 체크 | 결과 |
|---|---|
| Build & Test | pass |
| Canvas visual diff | pass |
| CodeQL | pass |
| Analyze rust | pass |
| Analyze javascript-typescript | pass |
| Analyze python | pass |
| WASM Build | skipped |

## 3. 변경 검토

### 3.1 코드 변경

`src/renderer/layout.rs`:

- `LayoutEngine::measure_endnote_column_bottom()` 추가
- 미주 단의 items를 scratch `build_single_column()`으로 순차 렌더해 단 bottom을 측정
- scratch `LayoutEngine`에서 `endnote_para_base=0`과 `endnote_between_notes_hu`를 설정해
  미주 vpos 정규화와 between-notes 보정을 렌더 경로와 맞춤
- `RHWP_EN_SSOT_DEBUG` 시 `EN_RENDER` 계측 출력 추가

`src/renderer/typeset.rs`:

- `EnSsotLevel::A3` 추가
- `RHWP_EN_SSOT=A3`에서 `simulate_endnote_column_bottom_y()`가 per-para 휴리스틱 대신 전 단
  scratch 렌더 측정 경로를 사용
- candidate para를 포함한 bottom 예측으로 split/advance fit 판단을 보강
- 측정 부작용 격리 테스트 `test_measure_endnote_advance_side_effect_free` 추가

### 3.2 긍정적 평가

- #1368 잔여인 body bottom 기준 단 전환 지연 문제와 같은 계열을 다룬다.
- A3가 opt-in이므로 기본 B 경로의 리스크를 낮춘다.
- per-para 고립 측정보다 전 단 순차 렌더 측정이 더 타당하다. 미주 흐름은 vpos forward-jump,
  trailing spacing, partial paragraph 상호작용이 있어 단일 para 측정보다 컬럼 컨텍스트가 중요하다.
- scratch 측정의 부작용 격리 테스트가 포함되어 있다.

## 4. 주요 지적

### 4.1 현재 `devel`에 merge 불가

GitHub 상태가 `mergeable=CONFLICTING`, `mergeStateStatus=DIRTY`이다.

로컬 확인:

```text
merge-base local/devel local/pr1371-upstream = 2f985a90
```

현재 `local/devel`은 PR #1368 완료 커밋 `45da5d91`까지 전진해 있다. `git merge-tree` 기준
`src/renderer/typeset.rs`에서 충돌이 발생한다. 충돌 위치는 `EnSsotLevel` 주석/enum 주변이며,
#1368에서 maintainer가 정정한 기본값 설명과 #1371의 A3 추가가 같은 구간을 수정한다.

### 4.2 #1368 maintainer fix가 되돌아가는 주석

PR head의 `EnSsotLevel` 주석은 다음 취지로 되어 있다.

```text
기본은 legacy ... 미설정 시 모든 동작이 종전과 동일
```

하지만 실제 `en_ssot_level()`의 기본값은 `B`다. #1368 수용 중 이 주석은 이미 maintainer fix로
정정했다. #1371 수용 시에는 이 주석을 반드시 `B` 기본값 기준으로 유지하고, A3가 opt-in임을
추가하는 방식으로 충돌을 풀어야 한다.

### 4.3 contributor 문서 위치 정리 필요

PR #1371은 #1368 contributor 문서를 다시 활성 폴더에 추가한다.

- `mydocs/plans/task_m100_1363*.md`
- `mydocs/working/task_m100_1363*.md`
- `mydocs/report/task_m100_1363*.md`
- `mydocs/report/task1363_ssot_diff_stage*.tsv`

현재 `devel`에서는 #1368 처리 과정에서 이 문서들이 archive로 정리되어 있다. 수용 시에는 기존
archive 구조를 보존하고, 새 v3 문서와 stage/report 산출물도 archive로 이동해야 한다. PR 검토
문서 `mydocs/pr/pr_1368_review.md`, `mydocs/pr/pr_1368_report.md`는 유지해야 한다.

### 4.4 diff-check 실패

로컬 확인:

```text
git diff --check local/devel...local/pr1371-upstream
```

결과:

```text
mydocs/working/task_m100_1363_v2_stage4.md:10: trailing whitespace.
```

수용 전 공백 정리가 필요하다.

### 4.5 A3는 아직 기본 승격 불가

PR 본문과 `task_m100_1363_v3_report.md`가 다음 잔여를 명시한다.

- `issue_1139/1189/1209/1284` 계열 13건 hancom 배치 재보정 필요
- `export-svg` CLI A3 페이지 폭발
- 전 단 scratch 렌더 측정의 O(n²) 성능 비용과 캐싱 필요

따라서 A3는 opt-in 인프라로만 수용 가능하다. 기본값 B는 유지해야 하며, #1363도 close하지 않는다.

## 5. 권장 처리

1. 컨트리뷰터에게 최신 `devel` 기준 rebase를 요청하거나, maintainer가 현재 `devel` 위에서
   수동 충돌 정리 브랜치를 만든다.
2. 충돌 정리 시:
   - `EnSsotLevel` 주석은 `B` 기본값 + `A3` opt-in 기준으로 유지
   - #1368 PR 검토/처리 문서는 유지
   - contributor task 문서와 TSV 산출물은 archive에 둔다
   - trailing whitespace 제거
3. 재기준화 후 검증:
   - `git diff --check`
   - `cargo fmt --check`
   - 기본 B 경로:
     - `CARGO_INCREMENTAL=0 cargo test --test issue_1082_endnote_multicolumn_drift -- --nocapture`
     - `CARGO_INCREMENTAL=0 cargo test --test issue_1139_inline_picture_duplicate -- --nocapture`
     - `CARGO_INCREMENTAL=0 cargo test --lib`
     - `CARGO_INCREMENTAL=0 cargo clippy --lib -- -D warnings`
   - A3 opt-in 경로:
     - `RHWP_EN_SSOT=A3 CARGO_INCREMENTAL=0 cargo test --test issue_1082_endnote_multicolumn_drift -- --nocapture`
     - 대상 샘플 `dump-pages`와 시각 판정 SVG
   - WASM:
     - `CARGO_INCREMENTAL=0 cargo check --lib --target wasm32-unknown-unknown -j 2`
     - 필요 시 `docker compose --env-file .env.docker run --rm wasm`

## 6. 승인 요청

위 검토 결과 기준으로 PR #1371에 대해 최신 `devel` 재기준화 또는 maintainer 충돌 정리 후
다시 검증하는 방향으로 진행해도 되는지 승인 요청한다.

## 7. 후속 조치

작업지시자 지시에 따라 컨트리뷰터에게 최신 `devel` 기준 rebase와 충돌 정리를 요청했다.

- 코멘트: https://github.com/edwardkim/rhwp/pull/1371#issuecomment-4677470193
