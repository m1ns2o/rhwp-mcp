//! Issue #1452: 텍스트 없이 TAC 그림만 있는 문단의 저장 커서 위치 복원.

use serde_json::Value;

fn parse_json(label: &str, json: &str) -> Value {
    serde_json::from_str(json).unwrap_or_else(|e| panic!("parse {label} json `{json}`: {e}"))
}

#[test]
fn transparency_sample_restores_saved_caret_after_second_inline_picture() {
    let repo_root = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(repo_root).join("samples/투명도0-50.hwp");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let parsed = rhwp::parser::parse_hwp(&bytes).expect("parse raw samples/투명도0-50.hwp");
    let props = &parsed.doc_properties;
    assert_eq!(props.caret_list_id, 0);
    assert_eq!(props.caret_para_id, 0);
    assert_eq!(props.caret_char_pos, 32);

    let mut doc =
        rhwp::wasm_api::HwpDocument::from_bytes(&bytes).expect("load samples/투명도0-50.hwp");

    let caret_json = doc.get_caret_position().expect("getCaretPosition");
    let caret = parse_json("caret", &caret_json);
    assert_eq!(caret["sectionIndex"], 0);
    assert_eq!(caret["paragraphIndex"], 0);
    assert_eq!(caret["charOffset"], 2);

    let first_line_rect = parse_json(
        "first-line cursor rect",
        &doc.get_cursor_rect_native(0, 0, 0)
            .expect("first line cursor rect"),
    );
    let saved_rect = parse_json(
        "saved cursor rect",
        &doc.get_cursor_rect_native(0, 0, 2)
            .expect("saved cursor rect"),
    );

    assert!(
        saved_rect["y"].as_f64().unwrap() > first_line_rect["y"].as_f64().unwrap(),
        "저장 커서는 첫 줄 시작이 아니라 두 번째 TAC 그림 뒤에 있어야 함: first={first_line_rect}, saved={saved_rect}"
    );
    assert!(
        saved_rect["x"].as_f64().unwrap() > first_line_rect["x"].as_f64().unwrap() + 400.0,
        "저장 커서는 두 번째 줄 첫머리가 아니라 두 번째 TAC 그림 오른쪽 끝에 있어야 함: first={first_line_rect}, saved={saved_rect}"
    );
    assert!(
        saved_rect["height"].as_f64().unwrap() < 40.0,
        "TAC 그림 뒤 캐럿 높이는 그림 높이가 아니라 글자 높이여야 함: saved={saved_rect}"
    );

    doc.set_show_paragraph_marks(true);
    let visible_mark_rect = parse_json(
        "visible paragraph-mark cursor rect",
        &doc.get_cursor_rect_native(0, 0, 2)
            .expect("visible paragraph-mark cursor rect"),
    );
    assert!(
        visible_mark_rect["x"].as_f64().unwrap() <= saved_rect["x"].as_f64().unwrap() + 1.0,
        "문단부호 표시 중 캐럿이 문단부호 오른쪽으로 과도하게 밀리면 안 됨: hidden={saved_rect}, visible={visible_mark_rect}"
    );
    assert!(
        visible_mark_rect["y"].as_f64().unwrap() > saved_rect["y"].as_f64().unwrap(),
        "문단부호 표시 중 캐럿은 한컴처럼 문단부호 기준으로 아래쪽에 맞춰야 함: hidden={saved_rect}, visible={visible_mark_rect}"
    );
    assert_eq!(visible_mark_rect["height"], saved_rect["height"]);
}
