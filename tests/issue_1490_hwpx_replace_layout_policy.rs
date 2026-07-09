use std::fs;
use std::path::Path;

use rhwp::document_core::{DocumentCore, TextReplaceLayoutPolicy};

const SAMPLE: &str = "samples/hwpx/2025년 2분기 해외직접투자 (최종).hwpx";
const OLD_TEXT: &str = "2025년 2분기 해외직접투자 감소";
const NEW_TEXT: &str = "2025년 2분기 해외직접투자 감소(MCP 공통 수정)";
const REPLACE_ALL_QUERY: &str = "해외직접투자";
const REPLACE_ALL_TEXT: &str = "해외직접투자(MCP일괄)";

fn load_sample() -> DocumentCore {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLE);
    let bytes = fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    DocumentCore::from_bytes(&bytes).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn body_paragraph_line_seg_count(core: &DocumentCore, needle: &str) -> usize {
    core.document()
        .sections
        .iter()
        .flat_map(|section| section.paragraphs.iter())
        .find(|para| para.text.contains(needle))
        .unwrap_or_else(|| panic!("paragraph containing {needle:?} not found"))
        .line_segs
        .len()
}

fn body_paragraph_char_shape_id_at_needle(core: &DocumentCore, needle: &str) -> u32 {
    let para = core
        .document()
        .sections
        .iter()
        .flat_map(|section| section.paragraphs.iter())
        .find(|para| para.text.contains(needle))
        .unwrap_or_else(|| panic!("paragraph containing {needle:?} not found"));
    let byte_offset = para
        .text
        .find(needle)
        .unwrap_or_else(|| panic!("needle {needle:?} not found in paragraph"));
    let char_offset = para.text[..byte_offset].chars().count();
    para.char_shape_id_at(char_offset)
        .unwrap_or_else(|| panic!("char shape at {needle:?} not found"))
}

fn body_line_seg_counts_by_section(core: &DocumentCore) -> Vec<Vec<usize>> {
    core.document()
        .sections
        .iter()
        .map(|section| {
            section
                .paragraphs
                .iter()
                .map(|para| para.line_segs.len())
                .collect()
        })
        .collect()
}

fn body_char_shape_ids_at_each_needle(core: &DocumentCore, needle: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    for para in core
        .document()
        .sections
        .iter()
        .flat_map(|section| section.paragraphs.iter())
    {
        let mut search_from = 0;
        while let Some(relative_byte_offset) = para.text[search_from..].find(needle) {
            let byte_offset = search_from + relative_byte_offset;
            let char_offset = para.text[..byte_offset].chars().count();
            let char_shape_id = para
                .char_shape_id_at(char_offset)
                .unwrap_or_else(|| panic!("char shape at {needle:?} not found"));
            ids.push(char_shape_id);
            search_from = byte_offset + needle.len();
        }
    }
    ids
}

#[test]
fn hwpx_single_replace_can_preserve_source_line_segments_and_page_count() {
    let mut core = load_sample();
    let original_page_count = core.page_count();
    let original_line_seg_count = body_paragraph_line_seg_count(&core, OLD_TEXT);
    let original_replaced_text_char_shape = body_paragraph_char_shape_id_at_needle(&core, OLD_TEXT);
    assert_eq!(
        original_page_count, 9,
        "representative fixture page count changed"
    );
    assert_eq!(
        original_line_seg_count, 3,
        "representative fixture paragraph line segment count changed"
    );

    let result = core
        .replace_one_with_layout_policy_native(
            OLD_TEXT,
            NEW_TEXT,
            true,
            TextReplaceLayoutPolicy::PreserveSourceLineSegments,
        )
        .expect("replace with layout preservation");
    assert!(
        result.contains(r#""ok":true"#),
        "replace should report success: {result}"
    );
    assert_eq!(core.page_count(), original_page_count);
    assert_eq!(
        body_paragraph_line_seg_count(&core, NEW_TEXT),
        original_line_seg_count
    );
    assert_eq!(
        body_paragraph_char_shape_id_at_needle(&core, NEW_TEXT),
        original_replaced_text_char_shape,
        "replacement should keep the deleted range's char shape"
    );

    let exported = core.export_hwpx_native().expect("export HWPX");
    let reparsed = DocumentCore::from_bytes(&exported).expect("reparse exported HWPX");
    assert_eq!(reparsed.page_count(), original_page_count);
    assert_eq!(
        body_paragraph_line_seg_count(&reparsed, NEW_TEXT),
        original_line_seg_count
    );
    assert_eq!(
        body_paragraph_char_shape_id_at_needle(&reparsed, NEW_TEXT),
        original_replaced_text_char_shape
    );
}

#[test]
fn hwpx_replace_all_can_preserve_source_line_segments_and_char_shapes() {
    let mut core = load_sample();
    let original_page_count = core.page_count();
    let original_line_seg_counts = body_line_seg_counts_by_section(&core);
    let original_char_shape_ids = body_char_shape_ids_at_each_needle(&core, REPLACE_ALL_QUERY);
    assert_eq!(
        original_char_shape_ids.len(),
        6,
        "representative fixture body occurrence count changed"
    );

    let result = core
        .replace_all_with_layout_policy_native(
            REPLACE_ALL_QUERY,
            REPLACE_ALL_TEXT,
            true,
            TextReplaceLayoutPolicy::PreserveSourceLineSegments,
        )
        .expect("replace all with layout preservation");
    assert!(
        result.contains(r#""ok":true"#) && result.contains(r#""count":9"#),
        "replace_all should report 9 body/cell/textbox replacements: {result}"
    );
    assert_eq!(core.page_count(), original_page_count);
    assert_eq!(
        body_line_seg_counts_by_section(&core),
        original_line_seg_counts,
        "replace_all preserve policy should keep body paragraph line segment counts"
    );
    assert_eq!(
        body_char_shape_ids_at_each_needle(&core, REPLACE_ALL_TEXT),
        original_char_shape_ids,
        "each replacement should keep its deleted range char shape"
    );

    let exported = core.export_hwpx_native().expect("export HWPX");
    let reparsed = DocumentCore::from_bytes(&exported).expect("reparse exported HWPX");
    assert_eq!(reparsed.page_count(), original_page_count);
    assert_eq!(
        body_line_seg_counts_by_section(&reparsed),
        original_line_seg_counts
    );
    assert_eq!(
        body_char_shape_ids_at_each_needle(&reparsed, REPLACE_ALL_TEXT),
        original_char_shape_ids
    );
}
