# 구현계획서 — Task #1105: sample16-hwp5 p21 page break 정합

- 이슈: [edwardkim/rhwp#1105](https://github.com/edwardkim/rhwp/issues/1105)
- 수행계획서: `mydocs/plans/task_m100_1105.md`
- 구현 커밋: `d4587b27 fix: preserve hwp3 conversion page break`

## Stage 1 — 재현과 원인 정리

### 재현 명령

```bash
cargo run --quiet --bin rhwp -- dump-pages samples/hwp3-sample16-hwp5.hwp -p 20
cargo run --quiet --bin rhwp -- dump-pages samples/hwp3-sample16-hwp5.hwp -p 21
cargo run --quiet --bin rhwp -- dump samples/hwp3-sample16-hwp5.hwp --section 0 --para 440
cargo run --quiet --bin rhwp -- dump samples/hwp3-sample16-hwp5.hwp --section 0 --para 442
```

### 원인

`pi=440`은 HWP3 원본과 한컴 HWP5 변환본 모두 page-reset 성격의 `vpos=852` 신호를 갖지만, #1035에서 aux trigger를 제거한 뒤 rhwp는 cumulative height 기준으로 `pi=440`을 이전 페이지 끝에 넣었다.

과거 aux trigger를 단순 복원하면 `64 -> 65 pages` over-split 이 발생했다. 원인은 `pi=442` 이후 `PARA_LINE_SEG`가 없는 본문 문단들의 합성 줄높이가 `max_fs * line_spacing(160%)`로 계산되어 새 페이지가 과대 높이로 측정되는 것이다.

## Stage 2 — 구현

### 2.1 page-reset bridge 가드

기존 조건:

```text
prev_end > body * 0.95 && curr_first < low_threshold
```

추가 조건:

```text
prev_real_line_seg 와 현재 paragraph 사이에
LINE_SEG 없는 visible text 문단이 2개 이상 있고,
현재 paragraph 자체는 real LINE_SEG 를 가지며,
curr_first <= 1500,
prev_end > body * 0.75
```

적용 위치:

- `src/renderer/typeset.rs`
- `src/renderer/pagination/engine.rs`

이 조건은 p21의 `pi=437..439`처럼 변환 과정에서 line segment bridge가 누락된 구간을 좁게 복원한다.

### 2.2 HWP3-origin synthetic line height 보정

`PARA_LINE_SEG`가 없는 HWP3-origin HWP5 변환 문단의 합성 줄은 raw line height가 폰트보다 작다. 일반 문서에서는 ParaShape 줄간격으로 보정하지만, 변환본의 이 영역에서는 한컴이 compact하게 배치하므로 `max_fs`를 줄높이로 사용한다.

공통 helper:

```rust
corrected_line_height_for_variant_synthetic(...)
```

적용 위치:

- `src/renderer/mod.rs`
- `src/renderer/height_measurer.rs`
- `src/renderer/layout/paragraph_layout.rs`
- `src/renderer/layout/table_layout.rs`
- `src/renderer/typeset.rs`

### 2.3 variant flag 전달

높이 측정과 실제 레이아웃이 같은 판단을 하도록 기존 `document.is_hwp3_variant`를 전달한다.

적용 위치:

- `DocumentCore::paginate`
- `DocumentCore::build_page_tree`
- `LayoutEngine::set_hwp3_variant`
- `TypesetEngine` 내부 table split helper

## Stage 3 — 회귀 테스트

새 테스트:

- `tests/issue_1105.rs`

검증 내용:

```text
page_count == 64
page 21 contains pi=439
page 21 does not contain pi=440
page 22 contains pi=440, pi=441, pi=449
```

관련 회귀:

- `tests/issue_1086.rs`
- `tests/issue_1035_alignment.rs`
- `tests/issue_554.rs`
- `tests/issue_nested_table_border.rs`

## Stage 4 — PR 준비

PR #1106은 문서화 전에 생성되어 회수했다. 문서화와 검증 커밋을 추가한 뒤 새 PR 또는 재오픈은 작업지시자 승인 후 진행한다.

PR 본문 초안:

```markdown
## Summary
- restore the Hancom page break before sample16-hwp5 section 4 when HWP3-origin conversion lost explicit break metadata
- use compact synthetic line height for HWP3-origin paragraphs without PARA_LINE_SEG across pagination, measurement, and layout
- add issue #1105 regression coverage for page 21/22 paragraph boundaries

## Tests
- cargo fmt --all -- --check
- cargo test --test issue_1105 --test issue_1086 --test issue_1035_alignment --test issue_554
- cargo test --test issue_nested_table_border
- git diff --check

Refs #1105
Stacked on #1103 until #1103 lands.
```
