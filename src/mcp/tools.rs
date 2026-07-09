use crate::diagnostics::hwp5_inventory::{build_inventory_from_bytes, Hwp5InventoryItem};
use crate::diagnostics::render_geom_diff::{
    diff_render_geometry, DocGeomDiff, NodeDelta, PageGeomDiff, TypeDelta,
};
use crate::document_core::builders::document_template::{DocumentTemplate, TemplateBlock};
use crate::document_core::helpers::{
    build_tab_def_from_json, get_textbox_from_shape, json_has_border_keys, json_has_tab_keys,
    parse_char_shape_mods, parse_json_i16_array, parse_para_shape_mods,
};
use crate::document_core::TextReplaceLayoutPolicy;
use crate::mcp::fs_guard::FsGuard;
use crate::mcp::session::{format_name, info_value, SessionManager};
use crate::model::control::Control;
use crate::model::event::DocumentEvent;
use crate::model::paragraph::Paragraph;
use crate::model::shape::ShapeObject;
use crate::model::style::{
    Alignment, CharShape, HeadType, LineSpacingType, ParaShape, Style, UnderlineType,
};
use crate::model::table::Table;
use crate::parser::byte_reader::ByteReader;
use crate::parser::tags;
use crate::parser::FileFormat;
use crate::renderer::layout::LayoutOverflow;
use crate::renderer::style_resolver::resolve_styles;
use crate::DocumentCore;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use quick_xml::events::Event as XmlEvent;
use quick_xml::Reader as XmlReader;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read};

const TOOL_NAMES: &[&str] = &[
    "rhwp_open",
    "rhwp_new",
    "rhwp_new_exam_from_ingest",
    "rhwp_new_government_report",
    "rhwp_new_from_document_template",
    "rhwp_close",
    "rhwp_document_info",
    "rhwp_document_profile",
    "rhwp_style_signature",
    "rhwp_revision_info",
    "rhwp_hwpx_package_info",
    "rhwp_get_hwpx_package_entry",
    "rhwp_save",
    "rhwp_export_bytes",
    "rhwp_extract_text",
    "rhwp_extract_markdown",
    "rhwp_extract_document_template",
    "rhwp_render_svg",
    "rhwp_render_png",
    "rhwp_preview_page",
    "rhwp_search",
    "rhwp_list_controls",
    "rhwp_compare_document_profile",
    "rhwp_compare_style_signature",
    "rhwp_compare_fidelity_summary",
    "rhwp_compare_text",
    "rhwp_compare_render_geometry",
    "rhwp_compare_render_png",
    "rhwp_match_render_pages",
    "rhwp_compare_hwp_records",
    "rhwp_compare_hwpx_package",
    "rhwp_insert_text",
    "rhwp_delete_text",
    "rhwp_replace_text",
    "rhwp_split_paragraph",
    "rhwp_merge_paragraph",
    "rhwp_insert_paragraph",
    "rhwp_delete_paragraph",
    "rhwp_list_fields",
    "rhwp_get_field",
    "rhwp_set_field",
    "rhwp_insert_click_here_field",
    "rhwp_remove_field",
    "rhwp_create_table",
    "rhwp_get_table_dimensions",
    "rhwp_get_table_properties",
    "rhwp_set_table_properties",
    "rhwp_get_cell_text",
    "rhwp_set_cell_text",
    "rhwp_insert_table_row",
    "rhwp_insert_table_column",
    "rhwp_delete_table_row",
    "rhwp_delete_table_column",
    "rhwp_merge_table_cells",
    "rhwp_split_table_cell",
    "rhwp_evaluate_table_formula",
    "rhwp_apply_char_format",
    "rhwp_apply_para_format",
    "rhwp_get_style_list",
    "rhwp_apply_style",
    "rhwp_create_style",
    "rhwp_update_style",
    "rhwp_delete_style",
    "rhwp_insert_picture",
    "rhwp_get_picture_properties",
    "rhwp_set_picture_properties",
    "rhwp_delete_picture",
    "rhwp_get_shape_properties",
    "rhwp_set_shape_properties",
    "rhwp_get_chart_data",
    "rhwp_set_chart_data",
    "rhwp_insert_shape",
    "rhwp_delete_shape",
    "rhwp_change_shape_z_order",
    "rhwp_group_shapes",
    "rhwp_insert_shape_group_child",
    "rhwp_get_shape_group_children",
    "rhwp_ungroup_shape",
    "rhwp_insert_equation",
    "rhwp_set_equation_properties",
    "rhwp_delete_equation",
    "rhwp_insert_footnote",
    "rhwp_insert_endnote",
    "rhwp_insert_hidden_comment",
    "rhwp_get_hidden_comment",
    "rhwp_insert_hidden_comment_text",
    "rhwp_delete_hidden_comment_text",
    "rhwp_split_hidden_comment_paragraph",
    "rhwp_merge_hidden_comment_paragraph",
    "rhwp_apply_hidden_comment_char_format",
    "rhwp_apply_hidden_comment_para_format",
    "rhwp_list_header_footers",
    "rhwp_get_header_footer",
    "rhwp_create_header_footer",
    "rhwp_delete_header_footer",
    "rhwp_get_header_footer_para_info",
    "rhwp_insert_header_footer_text",
    "rhwp_delete_header_footer_text",
    "rhwp_split_header_footer_paragraph",
    "rhwp_merge_header_footer_paragraph",
    "rhwp_get_header_footer_para_format",
    "rhwp_apply_header_footer_para_format",
    "rhwp_insert_header_footer_field",
    "rhwp_apply_header_footer_template",
    "rhwp_get_note_info",
    "rhwp_insert_note_text",
    "rhwp_delete_note_text",
    "rhwp_split_note_paragraph",
    "rhwp_merge_note_paragraph",
    "rhwp_apply_note_char_format",
    "rhwp_apply_note_para_format",
    "rhwp_get_page_def",
    "rhwp_set_page_def",
    "rhwp_get_section_def",
    "rhwp_set_section_def",
    "rhwp_insert_page_break",
    "rhwp_insert_column_break",
    "rhwp_list_bookmarks",
    "rhwp_add_bookmark",
    "rhwp_rename_bookmark",
    "rhwp_delete_bookmark",
];

pub struct ToolRuntime {
    guard: FsGuard,
    sessions: SessionManager,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LinkedImagePolicy {
    PreserveLinks,
    EmbedAccessible,
    EmbedRequired,
}

impl LinkedImagePolicy {
    fn from_args(args: &Map<String, Value>) -> Result<Self, String> {
        let raw = opt_str(args, "linked_image_policy")
            .or_else(|| opt_str(args, "linkedImagePolicy"))
            .unwrap_or("preserve_links");
        match raw {
            "preserve_links" | "preserve" => Ok(Self::PreserveLinks),
            "embed_accessible" | "embed_if_accessible" => Ok(Self::EmbedAccessible),
            "embed_required" | "embed_all" => Ok(Self::EmbedRequired),
            other => Err(format!(
                "unsupported linked_image_policy: {other}; expected preserve_links, embed_accessible, or embed_required"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::PreserveLinks => "preserve_links",
            Self::EmbedAccessible => "embed_accessible",
            Self::EmbedRequired => "embed_required",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageFlowPolicy {
    Preserve,
    CompactArtifacts,
}

impl PageFlowPolicy {
    fn from_args(args: &Map<String, Value>) -> Result<Self, String> {
        let raw = opt_str(args, "page_flow_policy")
            .or_else(|| opt_str(args, "pageFlowPolicy"))
            .unwrap_or("preserve");
        match raw {
            "preserve" => Ok(Self::Preserve),
            "compact_artifacts" | "compact" => Ok(Self::CompactArtifacts),
            other => Err(format!(
                "unsupported page_flow_policy: {other}; expected preserve or compact_artifacts"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::CompactArtifacts => "compact_artifacts",
        }
    }
}

#[derive(Default)]
struct LinkedImagePolicyReport {
    embedded: usize,
    preserved: usize,
    missing: usize,
    errors: Vec<String>,
}

impl LinkedImagePolicyReport {
    fn json(&self, policy: LinkedImagePolicy) -> Value {
        json!({
            "policy": policy.as_str(),
            "embedded": self.embedded,
            "preserved": self.preserved,
            "missing": self.missing,
            "errors": self.errors,
        })
    }
}

impl ToolRuntime {
    pub fn new(guard: FsGuard) -> Self {
        Self {
            guard,
            sessions: SessionManager::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn core_for_test(&self, session_id: &str) -> Result<&DocumentCore, String> {
        self.sessions.get(session_id).map(|session| &session.core)
    }

    #[cfg(test)]
    pub(crate) fn core_for_test_mut(
        &mut self,
        session_id: &str,
    ) -> Result<&mut DocumentCore, String> {
        self.sessions
            .get_mut(session_id)
            .map(|session| &mut session.core)
    }

    pub fn tools_list(&self) -> Value {
        let tools = TOOL_NAMES
            .iter()
            .map(|name| {
                json!({
                    "name": name,
                    "description": description(name),
                    "inputSchema": tool_input_schema(name)
                })
            })
            .collect::<Vec<_>>();
        json!({ "tools": tools })
    }

    pub fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, String> {
        let args = arguments.as_object().cloned().unwrap_or_default();
        match name {
            "rhwp_open" => {
                let path = req_str(&args, "path")?;
                self.sessions.open(&self.guard, path)
            }
            "rhwp_new" => self.sessions.new_document(),
            "rhwp_new_exam_from_ingest" => self.new_exam_from_ingest(args),
            "rhwp_new_government_report" => self.new_government_report(args),
            "rhwp_new_from_document_template" => self.new_from_document_template(args),
            "rhwp_close" => {
                let id = req_str(&args, "session_id")?;
                self.sessions.close(id)
            }
            "rhwp_document_info" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                Ok(json!({
                    "document_info": info_value(&session.core),
                    "metadata": session.metadata_json(),
                }))
            }
            "rhwp_document_profile" => {
                let session_id = req_str(&args, "session_id")?;
                let session = self.sessions.get(session_id)?;
                Ok(document_profile_json(
                    &format!("session:{session_id}"),
                    format_name(session.source_format),
                    &document_structure_profile(&session.core),
                ))
            }
            "rhwp_style_signature" => {
                let session_id = req_str(&args, "session_id")?;
                let session = self.sessions.get(session_id)?;
                Ok(style_signature_json(
                    &format!("session:{session_id}"),
                    format_name(session.source_format),
                    &session.core,
                ))
            }
            "rhwp_revision_info" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                Ok(revision_info(
                    &session.core,
                    format_name(session.source_format),
                ))
            }
            "rhwp_hwpx_package_info" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                Ok(hwpx_package_info(
                    &session.core,
                    format_name(session.source_format),
                ))
            }
            "rhwp_get_hwpx_package_entry" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                get_hwpx_package_entry(&session.core, &args)
            }
            "rhwp_save" => self.save(args),
            "rhwp_export_bytes" => self.export_bytes(args),
            "rhwp_extract_text" => self.extract_text(args),
            "rhwp_extract_markdown" => self.extract_markdown(args),
            "rhwp_extract_document_template" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let preserve_empty_paragraphs = opt_bool(&args, "preserve_empty_paragraphs")
                    .or_else(|| opt_bool(&args, "preserveEmptyParagraphs"))
                    .unwrap_or(false);
                let template =
                    crate::document_core::builders::document_template::extract_document_template_with_options(
                        &session.core,
                        crate::document_core::builders::document_template::ExtractDocumentTemplateOptions {
                            preserve_empty_paragraphs,
                        },
                    );
                let stats =
                    crate::document_core::builders::document_template::template_stats(&template);
                Ok(json!({
                    "template": template,
                    "stats": stats,
                    "options": {
                        "preserve_empty_paragraphs": preserve_empty_paragraphs,
                    },
                }))
            }
            "rhwp_render_svg" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let page = opt_u32(&args, "page").unwrap_or(0);
                session
                    .core
                    .render_page_svg_native_with_overflows(page)
                    .map(|(svg, overflows)| {
                        let page_overflows = layout_overflow_json_for_page(&overflows, page);
                        let page_overflow_count = page_overflows.len();
                        json!({
                            "page": page,
                            "svg": svg,
                            "layout_overflow_count": page_overflow_count,
                            "layout_overflows": page_overflows,
                            "document_layout_overflow_count": overflows.len(),
                        })
                    })
                    .map_err(|e| e.to_string())
            }
            "rhwp_render_png" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let page = opt_u32(&args, "page").unwrap_or(0);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    return session
                        .core
                        .render_page_png_from_svg_native_with_overflows(page)
                        .map(|(png, width, height, overflows)| {
                            let byte_length = png.len();
                            let page_overflows = layout_overflow_json_for_page(&overflows, page);
                            let page_overflow_count = page_overflows.len();
                            json!({
                                "page": page,
                                "mime_type": "image/png",
                                "extension": "png",
                                "png_base64": BASE64.encode(&png),
                                "byte_length": byte_length,
                                "width_px": width,
                                "height_px": height,
                                "renderer": "svg-resvg",
                                "layout_overflow_count": page_overflow_count,
                                "layout_overflows": page_overflows,
                                "document_layout_overflow_count": overflows.len(),
                            })
                        })
                        .map_err(|e| e.to_string());
                }
                #[cfg(target_arch = "wasm32")]
                {
                    Err("rhwp_render_png is not available on wasm32".to_string())
                }
            }
            "rhwp_preview_page" => self.preview_page(args),
            "rhwp_search" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let query = req_str(&args, "query")?;
                let case_sensitive = opt_bool(&args, "case_sensitive").unwrap_or(true);
                let include_cells = opt_bool(&args, "include_cells").unwrap_or(true);
                let result = session
                    .core
                    .search_all_text_native(query, case_sensitive, include_cells)
                    .map_err(|e| e.to_string())?;
                Ok(serde_json::from_str(&result).unwrap_or_else(|_| json!({ "text": result })))
            }
            "rhwp_list_controls" => self.list_controls(args),
            "rhwp_compare_document_profile" => self.compare_document_profile(args),
            "rhwp_compare_style_signature" => self.compare_style_signature(args),
            "rhwp_compare_fidelity_summary" => self.compare_fidelity_summary(args),
            "rhwp_compare_text" => self.compare_text(args),
            "rhwp_compare_render_geometry" => self.compare_render_geometry(args),
            "rhwp_compare_render_png" => self.compare_render_png(args),
            "rhwp_match_render_pages" => self.match_render_pages(args),
            "rhwp_compare_hwp_records" => self.compare_hwp_records(args),
            "rhwp_compare_hwpx_package" => self.compare_hwpx_package(args),
            "rhwp_insert_text" => self.mutate(args, |core, args| {
                hwp(core.insert_text_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                    req_str(args, "text")?,
                ))
            }),
            "rhwp_delete_text" => self.mutate(args, |core, args| {
                hwp(core.delete_text_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                    req_usize(args, "count")?,
                ))
            }),
            "rhwp_replace_text" => self.replace_text(args),
            "rhwp_split_paragraph" => self.mutate(args, |core, args| {
                hwp(core.split_paragraph_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                ))
            }),
            "rhwp_merge_paragraph" => self.mutate(args, |core, args| {
                hwp(core.merge_paragraph_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                ))
            }),
            "rhwp_insert_paragraph" => self.mutate(args, |core, args| {
                hwp(core.insert_paragraph_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                ))
            }),
            "rhwp_delete_paragraph" => self.mutate(args, |core, args| {
                hwp(core.delete_paragraph_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                ))
            }),
            "rhwp_list_fields" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(Ok(session.core.get_field_list_json()))
            }
            "rhwp_get_field" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let target = require_exactly_one_key(&args, &["field_id", "name"], "field target")?;
                if target == "field_id" {
                    let id = req_u32(&args, "field_id")?;
                    core_json(session.core.get_field_value_by_id(id))
                } else {
                    core_json(
                        session
                            .core
                            .get_field_value_by_name(req_str(&args, "name")?),
                    )
                }
            }
            "rhwp_set_field" => self.mutate(args, |core, args| {
                let value = req_str(args, "value")?;
                let target = require_exactly_one_key(args, &["field_id", "name"], "field target")?;
                if target == "field_id" {
                    let id = req_u32(args, "field_id")?;
                    hwp(core.set_field_value_by_id(id, value))
                } else {
                    hwp(core.set_field_value_by_name(req_str(args, "name")?, value))
                }
            }),
            "rhwp_insert_click_here_field" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = opt_usize(args, "para").unwrap_or(0);
                let char_offset = req_usize(args, "char_offset")?;
                let guide = opt_str(args, "guide").unwrap_or("");
                let memo = opt_str(args, "memo").unwrap_or("");
                let name = opt_str(args, "name").unwrap_or("");
                let editable = opt_bool(args, "editable").unwrap_or(true);
                if is_plain_header_footer_paragraph_target(args) {
                    hwp(core.insert_click_here_field_in_header_footer_native(
                        section,
                        hf_is_header_arg(args)?,
                        hf_apply_to_arg(args),
                        hf_para_arg(args),
                        char_offset,
                        guide,
                        memo,
                        name,
                        editable,
                    ))
                } else if let Some(cell_path) = exact_cell_path_arg(args)? {
                    hwp(core.insert_click_here_field_at_by_path(
                        section,
                        para,
                        &cell_path,
                        char_offset,
                        guide,
                        memo,
                        name,
                        editable,
                    ))
                } else {
                    hwp(core.insert_click_here_field_at(
                        section,
                        para,
                        char_offset,
                        guide,
                        memo,
                        name,
                        editable,
                    ))
                }
            }),
            "rhwp_remove_field" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = opt_usize(args, "para").unwrap_or(0);
                let char_offset = req_usize(args, "char_offset")?;
                if is_plain_header_footer_paragraph_target(args) {
                    hwp(core.remove_field_in_header_footer_native(
                        section,
                        hf_is_header_arg(args)?,
                        hf_apply_to_arg(args),
                        hf_para_arg(args),
                        char_offset,
                    ))
                } else if let Some(cell_path) = exact_cell_path_arg(args)? {
                    hwp(core.remove_field_at_by_path(section, para, &cell_path, char_offset))
                } else {
                    hwp(core.remove_field_at(section, para, char_offset))
                }
            }),
            "rhwp_create_table" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = opt_usize(args, "para").unwrap_or(0);
                let char_offset = req_usize(args, "char_offset")?;
                let rows = req_u16(args, "rows")?;
                let cols = req_u16(args, "cols")?;
                if is_header_footer_nested_target(args) {
                    create_header_footer_table(core, args)
                } else if let Some(cell_path) = exact_cell_path_arg(args)? {
                    hwp(core.create_table_by_cell_path_native(
                        section,
                        para,
                        &cell_path,
                        char_offset,
                        rows,
                        cols,
                    ))
                } else {
                    hwp(core.create_table_native(section, para, char_offset, rows, cols))
                }
            }),
            "rhwp_get_table_dimensions" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let section = opt_usize(&args, "section").unwrap_or(0);
                let para = req_usize(&args, "para")?;
                if is_header_footer_nested_target(&args) {
                    get_header_footer_table_dimensions(&session.core, &args)
                } else if let Some(table_path) = table_path_arg(&args)? {
                    core_json(session.core.get_table_dimensions_by_cell_path_native(
                        section,
                        para,
                        &table_path,
                    ))
                } else {
                    core_json(session.core.get_table_dimensions_native(
                        section,
                        para,
                        req_usize(&args, "control")?,
                    ))
                }
            }
            "rhwp_get_table_properties" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let section = opt_usize(&args, "section").unwrap_or(0);
                let para = req_usize(&args, "para")?;
                if is_header_footer_nested_target(&args) {
                    get_header_footer_table_properties(&session.core, &args)
                } else if let Some(table_path) = table_path_arg(&args)? {
                    core_json(session.core.get_table_properties_by_cell_path_native(
                        section,
                        para,
                        &table_path,
                    ))
                } else {
                    core_json(session.core.get_table_properties_native(
                        section,
                        para,
                        req_usize(&args, "control")?,
                    ))
                }
            }
            "rhwp_set_table_properties" => self.mutate(args, |core, args| {
                let props = props_json(args);
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if is_header_footer_nested_target(args) {
                    set_header_footer_table_properties(core, args)
                } else if let Some(table_path) = table_path_arg(args)? {
                    hwp(core.set_table_properties_by_cell_path_native(
                        section,
                        para,
                        &table_path,
                        &props,
                    ))
                } else {
                    hwp(core.set_table_properties_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        &props,
                    ))
                }
            }),
            "rhwp_get_cell_text" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let section = opt_usize(&args, "section").unwrap_or(0);
                let para = req_usize(&args, "para")?;
                if is_header_footer_nested_target(&args) {
                    get_header_footer_table_cell_text(&session.core, &args)
                } else if args.contains_key("cell_path") {
                    let cell_path = parse_cell_path(args.get("cell_path"))?;
                    let text = session
                        .core
                        .get_text_in_cell_by_path(section, para, &cell_path, 0, usize::MAX)
                        .map_err(|e| e.to_string())?;
                    Ok(json!({ "cell_path": cell_path_json(&cell_path), "text": text }))
                } else {
                    let (cell, cell_para) = cell_target(&session.core, &args)?;
                    let len = session
                        .core
                        .get_cell_paragraph_length_native(
                            section,
                            para,
                            req_usize(&args, "control")?,
                            cell,
                            cell_para,
                        )
                        .map_err(|e| e.to_string())?;
                    let text = session
                        .core
                        .get_text_in_cell_native(
                            section,
                            para,
                            req_usize(&args, "control")?,
                            cell,
                            cell_para,
                            0,
                            len,
                        )
                        .map_err(|e| e.to_string())?;
                    Ok(json!({ "cell": cell, "cell_para": cell_para, "text": text }))
                }
            }
            "rhwp_set_cell_text" => self.set_cell_text(args),
            "rhwp_insert_table_row" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if is_header_footer_nested_target(args) {
                    insert_header_footer_table_row(core, args)
                } else if let Some(table_path) = table_path_arg(args)? {
                    hwp(core.insert_table_row_by_cell_path_native(
                        section,
                        para,
                        &table_path,
                        req_u16(args, "row")?,
                        opt_bool(args, "below").unwrap_or(true),
                    ))
                } else {
                    hwp(core.insert_table_row_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        req_u16(args, "row")?,
                        opt_bool(args, "below").unwrap_or(true),
                    ))
                }
            }),
            "rhwp_insert_table_column" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if is_header_footer_nested_target(args) {
                    insert_header_footer_table_column(core, args)
                } else if let Some(table_path) = table_path_arg(args)? {
                    hwp(core.insert_table_column_by_cell_path_native(
                        section,
                        para,
                        &table_path,
                        req_u16(args, "col")?,
                        opt_bool(args, "right").unwrap_or(true),
                    ))
                } else {
                    hwp(core.insert_table_column_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        req_u16(args, "col")?,
                        opt_bool(args, "right").unwrap_or(true),
                    ))
                }
            }),
            "rhwp_delete_table_row" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if is_header_footer_nested_target(args) {
                    delete_header_footer_table_row(core, args)
                } else if let Some(table_path) = table_path_arg(args)? {
                    hwp(core.delete_table_row_by_cell_path_native(
                        section,
                        para,
                        &table_path,
                        req_u16(args, "row")?,
                    ))
                } else {
                    hwp(core.delete_table_row_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        req_u16(args, "row")?,
                    ))
                }
            }),
            "rhwp_delete_table_column" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if is_header_footer_nested_target(args) {
                    delete_header_footer_table_column(core, args)
                } else if let Some(table_path) = table_path_arg(args)? {
                    hwp(core.delete_table_column_by_cell_path_native(
                        section,
                        para,
                        &table_path,
                        req_u16(args, "col")?,
                    ))
                } else {
                    hwp(core.delete_table_column_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        req_u16(args, "col")?,
                    ))
                }
            }),
            "rhwp_merge_table_cells" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if is_header_footer_nested_target(args) {
                    merge_header_footer_table_cells(core, args)
                } else if let Some(table_path) = table_path_arg(args)? {
                    merge_table_cells_by_path(core, section, para, &table_path, args)
                } else {
                    hwp(core.merge_table_cells_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        req_u16(args, "start_row")?,
                        req_u16(args, "start_col")?,
                        req_u16(args, "end_row")?,
                        req_u16(args, "end_col")?,
                    ))
                }
            }),
            "rhwp_split_table_cell" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if is_header_footer_nested_target(args) {
                    split_header_footer_table_cell(core, args)
                } else if let Some(table_path) = table_path_arg(args)? {
                    split_table_cell_by_path(core, section, para, &table_path, args)
                } else if opt_u16(args, "rows").is_some() || opt_u16(args, "cols").is_some() {
                    hwp(core.split_table_cell_into_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        req_u16(args, "row")?,
                        req_u16(args, "col")?,
                        opt_u16(args, "rows").unwrap_or(1),
                        opt_u16(args, "cols").unwrap_or(1),
                        opt_bool(args, "equal_row_height").unwrap_or(true),
                        opt_bool(args, "merge_first").unwrap_or(false),
                    ))
                } else {
                    hwp(core.split_table_cell_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        req_u16(args, "row")?,
                        req_u16(args, "col")?,
                    ))
                }
            }),
            "rhwp_evaluate_table_formula" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                let (target_row, target_col) = formula_target(args)?;
                let formula = req_str(args, "formula")?;
                let write_result = formula_write_result(args);
                if is_header_footer_nested_target(args) {
                    evaluate_header_footer_table_formula(core, args)
                } else if let Some(table_path) = table_path_arg(args)? {
                    hwp(core.evaluate_table_formula_by_cell_path_native(
                        section,
                        para,
                        &table_path,
                        target_row,
                        target_col,
                        formula,
                        write_result,
                    ))
                } else {
                    hwp(core.evaluate_table_formula(
                        section,
                        para,
                        req_usize(args, "control")?,
                        target_row,
                        target_col,
                        formula,
                        write_result,
                    ))
                }
            }),
            "rhwp_apply_char_format" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let props = props_json(args);
                validate_style_char_format(&props)?;
                if is_plain_header_footer_paragraph_target(args) {
                    hwp(core.apply_char_format_in_hf_native(
                        section,
                        hf_is_header_arg(args)?,
                        hf_apply_to_arg(args),
                        hf_para_arg(args),
                        req_usize(args, "start")?,
                        req_usize(args, "end")?,
                        &props,
                    ))
                } else {
                    let para = req_usize(args, "para")?;
                    if is_header_footer_nested_target(args) {
                        apply_header_footer_table_cell_char_format(core, args)
                    } else if let Some(cell_path) = format_cell_path_arg(core, section, para, args)?
                    {
                        hwp(core.apply_char_format_in_cell_by_path_native(
                            section,
                            para,
                            &cell_path,
                            req_usize(args, "start")?,
                            req_usize(args, "end")?,
                            &props,
                        ))
                    } else if is_table_cell_format_target(args) {
                        let (cell, cell_para) = cell_target(core, args)?;
                        hwp(core.apply_char_format_in_cell_native(
                            section,
                            para,
                            req_usize(args, "control")?,
                            cell,
                            cell_para,
                            req_usize(args, "start")?,
                            req_usize(args, "end")?,
                            &props,
                        ))
                    } else {
                        hwp(core.apply_char_format_native(
                            section,
                            para,
                            req_usize(args, "start")?,
                            req_usize(args, "end")?,
                            &props,
                        ))
                    }
                }
            }),
            "rhwp_apply_para_format" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let props = props_json(args);
                validate_style_para_format(&props)?;
                if is_plain_header_footer_paragraph_target(args) {
                    hwp(core.apply_para_format_in_hf_native(
                        section,
                        hf_is_header_arg(args)?,
                        hf_apply_to_arg(args),
                        hf_para_arg(args),
                        &props,
                    ))
                } else {
                    let para = req_usize(args, "para")?;
                    if is_header_footer_nested_target(args) {
                        apply_header_footer_table_cell_para_format(core, args)
                    } else if let Some(cell_path) = format_cell_path_arg(core, section, para, args)?
                    {
                        hwp(core.apply_para_format_in_cell_by_path_native(
                            section, para, &cell_path, &props,
                        ))
                    } else if is_table_cell_format_target(args) {
                        let (cell, cell_para) = cell_target(core, args)?;
                        hwp(core.apply_para_format_in_cell_native(
                            section,
                            para,
                            req_usize(args, "control")?,
                            cell,
                            cell_para,
                            &props,
                        ))
                    } else {
                        hwp(core.apply_para_format_native(section, para, &props))
                    }
                }
            }),
            "rhwp_get_style_list" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let include_formats = opt_bool(&args, "include_formats")
                    .or_else(|| opt_bool(&args, "includeFormats"))
                    .unwrap_or(false);
                let include_raw = opt_bool(&args, "include_raw")
                    .or_else(|| opt_bool(&args, "includeRaw"))
                    .unwrap_or(false);
                Ok(style_list_json(&session.core, include_formats, include_raw))
            }
            "rhwp_apply_style" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let style_id = req_usize(args, "style_id")?;
                if is_plain_header_footer_paragraph_target(args) {
                    hwp(core.apply_header_footer_style_native(
                        section,
                        hf_is_header_arg(args)?,
                        hf_apply_to_arg(args),
                        hf_para_arg(args),
                        style_id,
                    ))
                } else {
                    let para = req_usize(args, "para")?;
                    if is_header_footer_nested_target(args) {
                        apply_header_footer_table_cell_style(core, args)
                    } else if let Some(cell_path) = format_cell_path_arg(core, section, para, args)?
                    {
                        hwp(core.apply_style_in_cell_by_path_native(
                            section, para, &cell_path, style_id,
                        ))
                    } else if is_table_cell_format_target(args) {
                        let (cell, cell_para) = cell_target(core, args)?;
                        hwp(core.apply_cell_style_native(
                            section,
                            para,
                            req_usize(args, "control")?,
                            cell,
                            cell_para,
                            style_id,
                        ))
                    } else {
                        hwp(core.apply_style_native(section, para, style_id))
                    }
                }
            }),
            "rhwp_create_style" => self.create_style(args),
            "rhwp_update_style" => self.update_style(args),
            "rhwp_delete_style" => self.delete_style(args),
            "rhwp_insert_picture" => self.insert_picture(args),
            "rhwp_get_picture_properties" => self.get_picture_properties(args),
            "rhwp_set_picture_properties" => self.set_picture_properties(args),
            "rhwp_delete_picture" => self.delete_picture(args),
            "rhwp_get_shape_properties" => self.get_shape_properties(args),
            "rhwp_set_shape_properties" => self.set_shape_properties(args),
            "rhwp_get_chart_data" => self.get_chart_data(args),
            "rhwp_set_chart_data" => self.set_chart_data(args),
            "rhwp_insert_shape" => self.insert_shape(args),
            "rhwp_delete_shape" => self.delete_shape(args),
            "rhwp_change_shape_z_order" => self.change_shape_z_order(args),
            "rhwp_group_shapes" => self.group_shapes(args),
            "rhwp_insert_shape_group_child" => self.insert_shape_group_child(args),
            "rhwp_get_shape_group_children" => self.get_shape_group_children(args),
            "rhwp_ungroup_shape" => self.ungroup_shape(args),
            "rhwp_insert_equation" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = opt_usize(args, "para").unwrap_or(0);
                let char_offset = req_usize(args, "char_offset")?;
                let script = req_str(args, "script")?;
                let font_size = opt_u32(args, "font_size").unwrap_or(1000);
                let color = opt_u32(args, "color").unwrap_or(0);
                if is_header_footer_nested_target(args) {
                    hwp(core.insert_equation_in_header_footer_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        nested_para_arg(args),
                        char_offset,
                        script,
                        font_size,
                        color,
                    ))
                } else if let Some(cell_path) = format_cell_path_arg(core, section, para, args)? {
                    hwp(core.insert_equation_in_cell_by_path_native(
                        section,
                        para,
                        &cell_path,
                        char_offset,
                        script,
                        font_size,
                        color,
                    ))
                } else if is_table_cell_format_target(args) {
                    let (cell, cell_para) = cell_target(core, args)?;
                    hwp(core.insert_equation_in_cell_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        cell,
                        cell_para,
                        char_offset,
                        script,
                        font_size,
                        color,
                    ))
                } else {
                    hwp(core.insert_equation_native(
                        section,
                        para,
                        char_offset,
                        script,
                        font_size,
                        color,
                    ))
                }
            }),
            "rhwp_set_equation_properties" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                let props = props_json(args);
                if is_header_footer_nested_target(args) {
                    hwp(core.set_header_footer_equation_properties_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        nested_para_arg(args),
                        req_inner_control(args)?,
                        &props,
                    ))
                } else if let Some(cell_path) = exact_cell_path_arg(args)? {
                    hwp(core.set_equation_properties_in_cell_by_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(args)?,
                        &props,
                    ))
                } else if let Some(cell_path) = format_cell_path_arg(core, section, para, args)? {
                    hwp(core.set_equation_properties_in_cell_by_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(args)?,
                        &props,
                    ))
                } else if is_table_cell_format_target(args) {
                    let (cell, cell_para) = cell_target(core, args)?;
                    if let Ok(inner_control) = req_inner_control(args) {
                        hwp(core.set_equation_properties_in_cell_native(
                            section,
                            para,
                            req_usize(args, "control")?,
                            cell,
                            cell_para,
                            inner_control,
                            &props,
                        ))
                    } else {
                        hwp(core.set_equation_properties_native(
                            section,
                            para,
                            req_usize(args, "control")?,
                            Some(cell),
                            Some(cell_para),
                            &props,
                        ))
                    }
                } else {
                    hwp(core.set_equation_properties_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        None,
                        None,
                        &props,
                    ))
                }
            }),
            "rhwp_delete_equation" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if is_header_footer_nested_target(args) {
                    hwp(core.delete_header_footer_equation_control_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        nested_para_arg(args),
                        req_inner_control(args)?,
                    ))
                } else if let Some(cell_path) = exact_cell_path_arg(args)? {
                    hwp(core.delete_equation_control_in_cell_by_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(args)?,
                    ))
                } else if let Some(cell_path) = format_cell_path_arg(core, section, para, args)? {
                    hwp(core.delete_equation_control_in_cell_by_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(args)?,
                    ))
                } else if is_table_cell_format_target(args) {
                    let (cell, cell_para) = cell_target(core, args)?;
                    hwp(core.delete_equation_control_in_cell_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        cell,
                        cell_para,
                        req_inner_control(args)?,
                    ))
                } else {
                    hwp(core.delete_equation_control_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                    ))
                }
            }),
            "rhwp_insert_footnote" => self.mutate(args, |core, args| {
                hwp(core.insert_footnote_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                ))
            }),
            "rhwp_insert_endnote" => self.mutate(args, |core, args| {
                hwp(core.insert_endnote_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                ))
            }),
            "rhwp_insert_hidden_comment" => self.mutate(args, |core, args| {
                hwp(core.insert_hidden_comment_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                    req_str(args, "text")?,
                ))
            }),
            "rhwp_get_hidden_comment" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                let section = opt_usize(&args, "section").unwrap_or(0);
                let para = req_usize(&args, "para")?;
                if let Some(cell_path) = hidden_comment_cell_path_arg(&args)? {
                    let cell_path = cell_path_object_json(&cell_path);
                    core_json(session.core.get_hidden_comment_info_by_cell_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(&args)?,
                    ))
                } else {
                    core_json(session.core.get_hidden_comment_info_native(
                        section,
                        para,
                        req_usize(&args, "control")?,
                    ))
                }
            }
            "rhwp_insert_hidden_comment_text" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if let Some(cell_path) = hidden_comment_cell_path_arg(args)? {
                    let cell_path = cell_path_object_json(&cell_path);
                    hwp(core.insert_text_in_hidden_comment_by_cell_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(args)?,
                        hidden_para_arg(args),
                        req_usize(args, "char_offset")?,
                        req_str(args, "text")?,
                    ))
                } else {
                    hwp(core.insert_text_in_hidden_comment_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        hidden_para_arg(args),
                        req_usize(args, "char_offset")?,
                        req_str(args, "text")?,
                    ))
                }
            }),
            "rhwp_delete_hidden_comment_text" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if let Some(cell_path) = hidden_comment_cell_path_arg(args)? {
                    let cell_path = cell_path_object_json(&cell_path);
                    hwp(core.delete_text_in_hidden_comment_by_cell_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(args)?,
                        hidden_para_arg(args),
                        req_usize(args, "char_offset")?,
                        req_usize(args, "count")?,
                    ))
                } else {
                    hwp(core.delete_text_in_hidden_comment_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        hidden_para_arg(args),
                        req_usize(args, "char_offset")?,
                        req_usize(args, "count")?,
                    ))
                }
            }),
            "rhwp_split_hidden_comment_paragraph" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if let Some(cell_path) = hidden_comment_cell_path_arg(args)? {
                    let cell_path = cell_path_object_json(&cell_path);
                    hwp(core.split_paragraph_in_hidden_comment_by_cell_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(args)?,
                        hidden_para_arg(args),
                        req_usize(args, "char_offset")?,
                    ))
                } else {
                    hwp(core.split_paragraph_in_hidden_comment_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        hidden_para_arg(args),
                        req_usize(args, "char_offset")?,
                    ))
                }
            }),
            "rhwp_merge_hidden_comment_paragraph" => self.mutate(args, |core, args| {
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if let Some(cell_path) = hidden_comment_cell_path_arg(args)? {
                    let cell_path = cell_path_object_json(&cell_path);
                    hwp(core.merge_paragraph_in_hidden_comment_by_cell_path_native(
                        section,
                        para,
                        &cell_path,
                        req_inner_control(args)?,
                        hidden_para_arg(args),
                    ))
                } else {
                    hwp(core.merge_paragraph_in_hidden_comment_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        hidden_para_arg(args),
                    ))
                }
            }),
            "rhwp_apply_hidden_comment_char_format" => self.mutate(args, |core, args| {
                let props = props_json(args);
                validate_style_char_format(&props)?;
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if let Some(cell_path) = hidden_comment_cell_path_arg(args)? {
                    let cell_path = cell_path_object_json(&cell_path);
                    hwp(
                        core.apply_char_format_in_hidden_comment_by_cell_path_native(
                            section,
                            para,
                            &cell_path,
                            req_inner_control(args)?,
                            hidden_para_arg(args),
                            req_usize(args, "start")?,
                            req_usize(args, "end")?,
                            &props,
                        ),
                    )
                } else {
                    hwp(core.apply_char_format_in_hidden_comment_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        hidden_para_arg(args),
                        req_usize(args, "start")?,
                        req_usize(args, "end")?,
                        &props,
                    ))
                }
            }),
            "rhwp_apply_hidden_comment_para_format" => self.mutate(args, |core, args| {
                let props = props_json(args);
                validate_style_para_format(&props)?;
                let section = opt_usize(args, "section").unwrap_or(0);
                let para = req_usize(args, "para")?;
                if let Some(cell_path) = hidden_comment_cell_path_arg(args)? {
                    let cell_path = cell_path_object_json(&cell_path);
                    hwp(
                        core.apply_para_format_in_hidden_comment_by_cell_path_native(
                            section,
                            para,
                            &cell_path,
                            req_inner_control(args)?,
                            hidden_para_arg(args),
                            &props,
                        ),
                    )
                } else {
                    hwp(core.apply_para_format_in_hidden_comment_native(
                        section,
                        para,
                        req_usize(args, "control")?,
                        hidden_para_arg(args),
                        &props,
                    ))
                }
            }),
            "rhwp_list_header_footers" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(
                    session.core.get_header_footer_list_native(
                        opt_usize(&args, "section").unwrap_or(0),
                        opt_bool(&args, "is_header")
                            .or_else(|| opt_bool(&args, "isHeader"))
                            .unwrap_or(true),
                        hf_apply_to_arg(&args),
                    ),
                )
            }
            "rhwp_get_header_footer" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(session.core.get_header_footer_native(
                    opt_usize(&args, "section").unwrap_or(0),
                    hf_is_header_arg(&args)?,
                    hf_apply_to_arg(&args),
                ))
            }
            "rhwp_create_header_footer" => self.mutate(args, |core, args| {
                hwp(core.create_header_footer_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                ))
            }),
            "rhwp_delete_header_footer" => self.mutate(args, |core, args| {
                hwp(core.delete_header_footer_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                ))
            }),
            "rhwp_get_header_footer_para_info" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(session.core.get_header_footer_para_info_native(
                    opt_usize(&args, "section").unwrap_or(0),
                    hf_is_header_arg(&args)?,
                    hf_apply_to_arg(&args),
                    hf_para_arg(&args),
                ))
            }
            "rhwp_insert_header_footer_text" => self.mutate(args, |core, args| {
                hwp(core.insert_text_in_header_footer_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                    hf_para_arg(args),
                    req_usize(args, "char_offset")?,
                    req_str(args, "text")?,
                ))
            }),
            "rhwp_delete_header_footer_text" => self.mutate(args, |core, args| {
                hwp(core.delete_text_in_header_footer_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                    hf_para_arg(args),
                    req_usize(args, "char_offset")?,
                    req_usize(args, "count")?,
                ))
            }),
            "rhwp_split_header_footer_paragraph" => self.mutate(args, |core, args| {
                hwp(core.split_paragraph_in_header_footer_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                    hf_para_arg(args),
                    req_usize(args, "char_offset")?,
                ))
            }),
            "rhwp_merge_header_footer_paragraph" => self.mutate(args, |core, args| {
                hwp(core.merge_paragraph_in_header_footer_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                    hf_para_arg(args),
                ))
            }),
            "rhwp_get_header_footer_para_format" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(session.core.get_para_properties_in_hf_native(
                    opt_usize(&args, "section").unwrap_or(0),
                    hf_is_header_arg(&args)?,
                    hf_apply_to_arg(&args),
                    hf_para_arg(&args),
                ))
            }
            "rhwp_apply_header_footer_para_format" => self.mutate(args, |core, args| {
                let props = props_json(args);
                validate_style_para_format(&props)?;
                hwp(core.apply_para_format_in_hf_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                    hf_para_arg(args),
                    &props,
                ))
            }),
            "rhwp_insert_header_footer_field" => self.mutate(args, |core, args| {
                hwp(core.insert_field_in_hf_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                    hf_para_arg(args),
                    req_usize(args, "char_offset")?,
                    req_u8(args, "field_type")?,
                ))
            }),
            "rhwp_apply_header_footer_template" => self.mutate(args, |core, args| {
                hwp(core.apply_hf_template_native(
                    opt_usize(args, "section").unwrap_or(0),
                    hf_is_header_arg(args)?,
                    hf_apply_to_arg(args),
                    req_u8(args, "template_id")?,
                ))
            }),
            "rhwp_get_note_info" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(session.core.get_footnote_info_native(
                    opt_usize(&args, "section").unwrap_or(0),
                    req_usize(&args, "para")?,
                    req_usize(&args, "control")?,
                ))
            }
            "rhwp_insert_note_text" => self.mutate(args, |core, args| {
                hwp(core.insert_text_in_footnote_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                    req_usize(args, "control")?,
                    note_para_arg(args),
                    req_usize(args, "char_offset")?,
                    req_str(args, "text")?,
                ))
            }),
            "rhwp_delete_note_text" => self.mutate(args, |core, args| {
                hwp(core.delete_text_in_footnote_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                    req_usize(args, "control")?,
                    note_para_arg(args),
                    req_usize(args, "char_offset")?,
                    req_usize(args, "count")?,
                ))
            }),
            "rhwp_split_note_paragraph" => self.mutate(args, |core, args| {
                hwp(core.split_paragraph_in_footnote_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                    req_usize(args, "control")?,
                    note_para_arg(args),
                    req_usize(args, "char_offset")?,
                ))
            }),
            "rhwp_merge_note_paragraph" => self.mutate(args, |core, args| {
                hwp(core.merge_paragraph_in_footnote_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                    req_usize(args, "control")?,
                    note_para_arg(args),
                ))
            }),
            "rhwp_apply_note_char_format" => self.mutate(args, |core, args| {
                let props = props_json(args);
                validate_style_char_format(&props)?;
                hwp(core.apply_char_format_in_footnote_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                    req_usize(args, "control")?,
                    note_para_arg(args),
                    req_usize(args, "start")?,
                    req_usize(args, "end")?,
                    &props,
                ))
            }),
            "rhwp_apply_note_para_format" => self.mutate(args, |core, args| {
                let props = props_json(args);
                validate_style_para_format(&props)?;
                hwp(core.apply_para_format_in_footnote_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                    req_usize(args, "control")?,
                    note_para_arg(args),
                    &props,
                ))
            }),
            "rhwp_get_page_def" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(
                    session
                        .core
                        .get_page_def_native(opt_usize(&args, "section").unwrap_or(0)),
                )
            }
            "rhwp_set_page_def" => self.mutate(args, |core, args| {
                hwp(core.set_page_def_native(
                    opt_usize(args, "section").unwrap_or(0),
                    &props_json(args),
                ))
            }),
            "rhwp_get_section_def" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(
                    session
                        .core
                        .get_section_def_native(opt_usize(&args, "section").unwrap_or(0)),
                )
            }
            "rhwp_set_section_def" => self.mutate(args, |core, args| {
                hwp(core.set_section_def_native(
                    opt_usize(args, "section").unwrap_or(0),
                    &props_json(args),
                ))
            }),
            "rhwp_insert_page_break" => self.mutate(args, |core, args| {
                hwp(core.insert_page_break_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                ))
            }),
            "rhwp_insert_column_break" => self.mutate(args, |core, args| {
                hwp(core.insert_column_break_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                ))
            }),
            "rhwp_list_bookmarks" => {
                let session = self.sessions.get(req_str(&args, "session_id")?)?;
                core_json(session.core.get_bookmarks_native())
            }
            "rhwp_add_bookmark" => self.mutate(args, |core, args| {
                hwp(core.add_bookmark_native(
                    opt_usize(args, "section").unwrap_or(0),
                    opt_usize(args, "para").unwrap_or(0),
                    req_usize(args, "char_offset")?,
                    req_str(args, "name")?,
                ))
            }),
            "rhwp_rename_bookmark" => self.mutate(args, |core, args| {
                let new_name = opt_str(args, "new_name")
                    .or_else(|| opt_str(args, "newName"))
                    .ok_or_else(|| "new_name is required".to_string())?;
                hwp(core.rename_bookmark_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                    req_usize(args, "control")?,
                    new_name,
                ))
            }),
            "rhwp_delete_bookmark" => self.mutate(args, |core, args| {
                hwp(core.delete_bookmark_native(
                    opt_usize(args, "section").unwrap_or(0),
                    req_usize(args, "para")?,
                    req_usize(args, "control")?,
                ))
            }),
            _ => Err(format!("unknown tool: {name}")),
        }
    }

    fn mutate<F>(&mut self, args: Map<String, Value>, f: F) -> Result<Value, String>
    where
        F: FnOnce(&mut DocumentCore, &Map<String, Value>) -> Result<String, String>,
    {
        let id = req_str(&args, "session_id")?.to_string();
        let session = self.sessions.get_mut(&id)?;
        let result = f(&mut session.core, &args)?;
        session.dirty = true;
        core_json(Ok(result))
    }

    fn replace_text(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        validate_replace_text_mode(&args)?;
        let layout_policy = replace_layout_policy_arg(&args)?;
        let session = self.sessions.get_mut(&id)?;
        let result = if let Some(query) = opt_str(&args, "query") {
            if opt_bool(&args, "all").unwrap_or(false) {
                session
                    .core
                    .replace_all_with_layout_policy_native(
                        query,
                        req_str(&args, "new_text")?,
                        opt_bool(&args, "case_sensitive").unwrap_or(true),
                        layout_policy,
                    )
                    .map_err(|e| e.to_string())
            } else {
                session
                    .core
                    .replace_one_with_layout_policy_native(
                        query,
                        req_str(&args, "new_text")?,
                        opt_bool(&args, "case_sensitive").unwrap_or(true),
                        layout_policy,
                    )
                    .map_err(|e| e.to_string())
            }
        } else {
            session
                .core
                .replace_text_with_layout_policy_native(
                    opt_usize(&args, "section").unwrap_or(0),
                    opt_usize(&args, "para").unwrap_or(0),
                    req_usize(&args, "char_offset")?,
                    req_usize(&args, "length")?,
                    req_str(&args, "new_text")?,
                    layout_policy,
                )
                .map_err(|e| e.to_string())
        }?;
        session.dirty = true;
        core_json(Ok(result))
    }

    fn set_cell_text(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let session = self.sessions.get_mut(&id)?;
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        let text = req_str(&args, "text")?;
        if is_header_footer_nested_target(&args) {
            let result = set_header_footer_table_cell_text(&mut session.core, &args, text)?;
            session.dirty = true;
            Ok(result)
        } else if args.contains_key("cell_path") {
            let cell_path = parse_cell_path(args.get("cell_path"))?;
            let existing = session
                .core
                .get_text_in_cell_by_path(section, para, &cell_path, 0, usize::MAX)
                .map_err(|e| e.to_string())?;
            let len = existing.chars().count();
            if len > 0 {
                session
                    .core
                    .delete_text_in_cell_by_path(section, para, &cell_path, 0, len)
                    .map_err(|e| e.to_string())?;
            }
            if !text.is_empty() {
                session
                    .core
                    .insert_text_in_cell_by_path(section, para, &cell_path, 0, text)
                    .map_err(|e| e.to_string())?;
            }
            session.dirty = true;
            Ok(json!({ "ok": true, "cell_path": cell_path_json(&cell_path) }))
        } else {
            let (cell, cell_para) = cell_target(&session.core, &args)?;
            let control = req_usize(&args, "control")?;
            let len = session
                .core
                .get_cell_paragraph_length_native(section, para, control, cell, cell_para)
                .map_err(|e| e.to_string())?;
            if len > 0 {
                session
                    .core
                    .delete_text_in_cell_native(section, para, control, cell, cell_para, 0, len)
                    .map_err(|e| e.to_string())?;
            }
            if !text.is_empty() {
                session
                    .core
                    .insert_text_in_cell_native(section, para, control, cell, cell_para, 0, text)
                    .map_err(|e| e.to_string())?;
            }
            session.dirty = true;
            Ok(json!({ "ok": true, "cell": cell, "cell_para": cell_para }))
        }
    }

    fn save(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let current_path = self.sessions.get(&id)?.path.clone();
        let path = match opt_str(&args, "path") {
            Some(path) => self.guard.resolve_target_file(path)?,
            None => {
                current_path.ok_or_else(|| "path is required for unsaved sessions".to_string())?
            }
        };
        let format = opt_str(&args, "format")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "hwp".to_string());
        let overwrite = opt_bool(&args, "overwrite").unwrap_or(false);
        let session = self.sessions.get_mut(&id)?;
        let bytes = export_core_bytes(&mut session.core, &format)?;
        let write = self.guard.atomic_write(&path, &bytes, overwrite)?;
        session.path = Some(path);
        session.source_format = if format == "hwpx" {
            FileFormat::Hwpx
        } else {
            FileFormat::Hwp
        };
        session.dirty = false;
        Ok(json!({
            "ok": true,
            "format": format,
            "write": write,
        }))
    }

    fn export_bytes(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let session = self.sessions.get_mut(&id)?;
        let format = opt_str(&args, "format")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| "hwp".to_string());
        let bytes = export_core_bytes(&mut session.core, &format)?;
        let mime = if format == "hwpx" {
            "application/vnd.hancom.hwpx"
        } else {
            "application/x-hwp"
        };
        Ok(json!({
            "format": format,
            "mime": mime,
            "bytes": bytes.len(),
            "base64": BASE64.encode(bytes),
        }))
    }

    fn extract_text(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session = self.sessions.get(req_str(&args, "session_id")?)?;
        if let Some(page) = opt_u32(&args, "page") {
            return session
                .core
                .extract_page_text_native(page)
                .map(|text| json!({ "page": page, "text": text }))
                .map_err(|e| e.to_string());
        }
        let mut text = String::new();
        for page in 0..session.core.page_count() {
            if page > 0 {
                text.push('\n');
            }
            text.push_str(
                &session
                    .core
                    .extract_page_text_native(page)
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(json!({ "page_count": session.core.page_count(), "text": text }))
    }

    fn extract_markdown(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session = self.sessions.get(req_str(&args, "session_id")?)?;
        if let Some(page) = opt_u32(&args, "page") {
            return session
                .core
                .extract_page_markdown_native(page)
                .map(|markdown| json!({ "page": page, "markdown": markdown }))
                .map_err(|e| e.to_string());
        }
        let mut markdown = String::new();
        for page in 0..session.core.page_count() {
            if page > 0 {
                markdown.push_str("\n\n");
            }
            markdown.push_str(
                &session
                    .core
                    .extract_page_markdown_native(page)
                    .map_err(|e| e.to_string())?,
            );
        }
        Ok(json!({
            "page_count": session.core.page_count(),
            "markdown": markdown,
        }))
    }

    fn preview_page(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session = self.sessions.get(req_str(&args, "session_id")?)?;
        let page = opt_u32(&args, "page").unwrap_or(0);
        let page_count = session.core.page_count();
        if page >= page_count {
            return Err(format!(
                "page index {page} is out of range for {page_count} page(s)"
            ));
        }

        let include_svg = opt_bool(&args, "include_svg")
            .or_else(|| opt_bool(&args, "includeSvg"))
            .unwrap_or(true);
        let include_html = opt_bool(&args, "include_html")
            .or_else(|| opt_bool(&args, "includeHtml"))
            .unwrap_or(true);
        let include_text = opt_bool(&args, "include_text")
            .or_else(|| opt_bool(&args, "includeText"))
            .unwrap_or(true);
        let include_markdown = opt_bool(&args, "include_markdown")
            .or_else(|| opt_bool(&args, "includeMarkdown"))
            .unwrap_or(true);
        let include_png = opt_bool(&args, "include_png")
            .or_else(|| opt_bool(&args, "includePng"))
            .unwrap_or(false);

        let page_info_raw = session
            .core
            .get_page_info_native(page)
            .map_err(|e| e.to_string())?;
        let page_info = serde_json::from_str::<Value>(&page_info_raw)
            .map_err(|err| format!("invalid page info JSON: {err}"))?;

        let mut svg_payload = None;
        let mut layout_overflows = Vec::new();
        let mut document_layout_overflow_count = 0usize;
        if include_svg || include_html {
            let (svg, overflows) = session
                .core
                .render_page_svg_native_with_overflows(page)
                .map_err(|e| e.to_string())?;
            layout_overflows = layout_overflow_json_for_page(&overflows, page);
            document_layout_overflow_count = overflows.len();
            svg_payload = Some(svg);
        }

        let text = if include_text {
            Some(
                session
                    .core
                    .extract_page_text_native(page)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let markdown = if include_markdown {
            Some(
                session
                    .core
                    .extract_page_markdown_native(page)
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };

        let html = if include_html {
            svg_payload
                .as_ref()
                .map(|svg| preview_html(page, page_count, &page_info, svg, text.as_deref()))
        } else {
            None
        };

        let png = if include_png {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (bytes, width, height, overflows) = session
                    .core
                    .render_page_png_from_svg_native_with_overflows(page)
                    .map_err(|e| e.to_string())?;
                if layout_overflows.is_empty() {
                    layout_overflows = layout_overflow_json_for_page(&overflows, page);
                    document_layout_overflow_count = overflows.len();
                }
                Some(json!({
                    "mime_type": "image/png",
                    "extension": "png",
                    "png_base64": BASE64.encode(&bytes),
                    "byte_length": bytes.len(),
                    "width_px": width,
                    "height_px": height,
                    "renderer": "svg-resvg",
                }))
            }
            #[cfg(target_arch = "wasm32")]
            {
                return Err("rhwp_preview_page include_png is not available on wasm32".to_string());
            }
        } else {
            None
        };

        Ok(json!({
            "session_id": &session.id,
            "page": page,
            "page_count": page_count,
            "source_format": format_name(session.source_format),
            "dirty": session.dirty,
            "page_info": page_info,
            "svg": svg_payload,
            "html": html,
            "text": text,
            "markdown": markdown,
            "png": png,
            "layout_overflow_count": layout_overflows.len(),
            "layout_overflows": layout_overflows,
            "document_layout_overflow_count": document_layout_overflow_count,
        }))
    }

    fn compare_text(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?;
        let left = self.sessions.get(session_id)?;
        let left_text = extract_full_text(&left.core)?;
        let left_label = format!("session:{session_id}");
        let target = require_compare_target(&args)?;
        let right = if target == "other_session_id" || target == "otherSessionId" {
            let other_session_id = req_str(&args, target)?;
            let other = self.sessions.get(other_session_id)?;
            CompareSide {
                label: format!("session:{other_session_id}"),
                page_count: other.core.page_count(),
                text: extract_full_text(&other.core)?,
            }
        } else {
            let path = req_str(&args, target)?;
            let resolved = self.guard.resolve_existing_file(path)?;
            let bytes = fs::read(&resolved)
                .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
            let core = DocumentCore::from_bytes(&bytes).map_err(|e| e.to_string())?;
            CompareSide {
                label: resolved.display().to_string(),
                page_count: core.page_count(),
                text: extract_full_text(&core)?,
            }
        };

        let normalize_whitespace = opt_bool(&args, "normalize_whitespace")
            .or_else(|| opt_bool(&args, "normalizeWhitespace"))
            .unwrap_or(false);
        let max_diffs = opt_usize(&args, "max_diffs")
            .or_else(|| opt_usize(&args, "maxDiffs"))
            .unwrap_or(20);
        let left_cmp = comparable_text(&left_text, normalize_whitespace);
        let right_cmp = comparable_text(&right.text, normalize_whitespace);
        let diffs = line_diffs(&left_cmp, &right_cmp, max_diffs);

        Ok(json!({
            "equal": left_cmp == right_cmp,
            "normalize_whitespace": normalize_whitespace,
            "left": text_profile(&left_label, left.core.page_count(), &left_text),
            "right": text_profile(&right.label, right.page_count, &right.text),
            "difference_count": diffs.len(),
            "differences_truncated": line_diff_count(&left_cmp, &right_cmp) > diffs.len(),
            "differences": diffs,
        }))
    }

    fn compare_document_profile(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?;
        let left = self.sessions.get(session_id)?;
        let left_label = format!("session:{session_id}");
        let left_source_format = format_name(left.source_format);
        let left_profile = document_structure_profile(&left.core);
        let target = require_compare_target(&args)?;

        let (right_label, right_source_format, right_profile) =
            if target == "other_session_id" || target == "otherSessionId" {
                let other_session_id = req_str(&args, target)?;
                let other = self.sessions.get(other_session_id)?;
                (
                    format!("session:{other_session_id}"),
                    format_name(other.source_format).to_string(),
                    document_structure_profile(&other.core),
                )
            } else {
                let path = req_str(&args, target)?;
                let resolved = self.guard.resolve_existing_file(path)?;
                let bytes = fs::read(&resolved)
                    .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
                let format = crate::parser::detect_format(&bytes);
                let core = DocumentCore::from_bytes(&bytes).map_err(|e| e.to_string())?;
                (
                    resolved.display().to_string(),
                    format_name(format).to_string(),
                    document_structure_profile(&core),
                )
            };

        let max_diffs = opt_usize(&args, "max_diffs")
            .or_else(|| opt_usize(&args, "maxDiffs"))
            .unwrap_or(50);
        let ignore_page_count = opt_bool(&args, "ignore_page_count")
            .or_else(|| opt_bool(&args, "ignorePageCount"))
            .unwrap_or(false);
        Ok(document_profile_diff_json(
            &left_label,
            left_source_format,
            &left_profile,
            &right_label,
            &right_source_format,
            &right_profile,
            max_diffs,
            ignore_page_count,
        ))
    }

    fn compare_style_signature(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?;
        let left = self.sessions.get(session_id)?;
        let left_label = format!("session:{session_id}");
        let left_source_format = format_name(left.source_format);
        let left_signature = style_signature_json(&left_label, left_source_format, &left.core);
        let target = require_compare_target(&args)?;

        let right_signature = if target == "other_session_id" || target == "otherSessionId" {
            let other_session_id = req_str(&args, target)?;
            let other = self.sessions.get(other_session_id)?;
            style_signature_json(
                &format!("session:{other_session_id}"),
                format_name(other.source_format),
                &other.core,
            )
        } else {
            let path = req_str(&args, target)?;
            let resolved = self.guard.resolve_existing_file(path)?;
            let bytes = fs::read(&resolved)
                .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
            let format = crate::parser::detect_format(&bytes);
            let core = DocumentCore::from_bytes(&bytes).map_err(|e| e.to_string())?;
            style_signature_json(&resolved.display().to_string(), format_name(format), &core)
        };
        let max_diffs = opt_usize(&args, "max_diffs")
            .or_else(|| opt_usize(&args, "maxDiffs"))
            .unwrap_or(20);
        Ok(style_signature_diff_json(
            left_signature,
            right_signature,
            max_diffs,
        ))
    }

    fn compare_fidelity_summary(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?.to_string();
        let target = require_compare_target(&args)?;
        let include_package = opt_bool(&args, "include_package")
            .or_else(|| opt_bool(&args, "includePackage"))
            .unwrap_or(false);
        let strict_package = opt_bool(&args, "strict_package")
            .or_else(|| opt_bool(&args, "strictPackage"))
            .unwrap_or(false);

        let mut result = {
            let left = self.sessions.get(&session_id)?;
            let left_label = format!("session:{session_id}");
            let left_source_format = format_name(left.source_format);

            if target == "other_session_id" || target == "otherSessionId" {
                let other_session_id = req_str(&args, target)?;
                let other = self.sessions.get(other_session_id)?;
                fidelity_summary_json(
                    &left_label,
                    left_source_format,
                    &left.core,
                    &format!("session:{other_session_id}"),
                    format_name(other.source_format),
                    &other.core,
                    &args,
                )?
            } else {
                let path = req_str(&args, target)?;
                let resolved = self.guard.resolve_existing_file(path)?;
                let bytes = fs::read(&resolved)
                    .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
                let format = crate::parser::detect_format(&bytes);
                let core = DocumentCore::from_bytes(&bytes).map_err(|e| e.to_string())?;
                fidelity_summary_json(
                    &left_label,
                    left_source_format,
                    &left.core,
                    &resolved.display().to_string(),
                    format_name(format),
                    &core,
                    &args,
                )?
            }
        };

        if include_package {
            let package = self.compare_hwpx_package(args.clone())?;
            append_fidelity_package_result(&mut result, package, strict_package);
        }

        Ok(result)
    }

    fn list_controls(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?;
        let session = self.sessions.get(session_id)?;
        let section_filter = opt_usize(&args, "section");
        let para_filter = opt_usize(&args, "para");
        let kind_filter = opt_str(&args, "kind").map(|kind| kind.to_ascii_lowercase());
        let include_nested = opt_bool(&args, "include_nested")
            .or_else(|| opt_bool(&args, "includeNested"))
            .unwrap_or(true);
        let max_items = opt_usize(&args, "max_items")
            .or_else(|| opt_usize(&args, "maxItems"))
            .unwrap_or(500);

        let mut controls = Vec::new();
        let mut truncated = false;
        for (section_idx, section) in session.core.document.sections.iter().enumerate() {
            if section_filter
                .map(|filter| filter != section_idx)
                .unwrap_or(false)
            {
                continue;
            }
            for (para_idx, para) in section.paragraphs.iter().enumerate() {
                if para_filter
                    .map(|filter| filter != para_idx)
                    .unwrap_or(false)
                {
                    continue;
                }
                collect_controls_from_paragraph(
                    &mut controls,
                    &mut truncated,
                    para,
                    ControlWalk {
                        section: section_idx,
                        host_para: para_idx,
                        scope: "body",
                        path_prefix: format!("section[{section_idx}].para[{para_idx}]"),
                        container: Value::Null,
                        edit_target: None,
                    },
                    include_nested,
                    kind_filter.as_deref(),
                    max_items,
                );
            }
        }

        Ok(json!({
            "session_id": session_id,
            "section_filter": section_filter,
            "para_filter": para_filter,
            "kind_filter": kind_filter,
            "include_nested": include_nested,
            "control_count": controls.len(),
            "truncated": truncated,
            "controls": controls,
        }))
    }

    fn compare_render_geometry(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?;
        let left = self.sessions.get(session_id)?;
        let left_label = format!("session:{session_id}");
        let max_deltas = opt_usize(&args, "max_deltas")
            .or_else(|| opt_usize(&args, "maxDeltas"))
            .unwrap_or(10);
        let max_disp_threshold = opt_f64(&args, "max_disp_threshold")
            .or_else(|| opt_f64(&args, "maxDispThreshold"))
            .unwrap_or(1.0);
        let page_filter = opt_u32(&args, "page");
        let target = require_compare_target(&args)?;

        if target == "other_session_id" || target == "otherSessionId" {
            let other_session_id = req_str(&args, target)?;
            let other = self.sessions.get(other_session_id)?;
            let right_label = format!("session:{other_session_id}");
            let diff = diff_render_geometry(&left.core, &other.core).map_err(|e| e.to_string())?;
            return Ok(geom_diff_json(
                &left_label,
                &right_label,
                &diff,
                max_deltas,
                max_disp_threshold,
                page_filter,
            ));
        }

        let path = req_str(&args, target)?;
        let resolved = self.guard.resolve_existing_file(path)?;
        let bytes = fs::read(&resolved)
            .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
        let other = DocumentCore::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let right_label = resolved.display().to_string();
        let diff = diff_render_geometry(&left.core, &other).map_err(|e| e.to_string())?;
        Ok(geom_diff_json(
            &left_label,
            &right_label,
            &diff,
            max_deltas,
            max_disp_threshold,
            page_filter,
        ))
    }

    fn compare_render_png(&self, args: Map<String, Value>) -> Result<Value, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let session_id = req_str(&args, "session_id")?;
            let left = self.sessions.get(session_id)?;
            let left_label = format!("session:{session_id}");
            let left_page = opt_u32(&args, "page")
                .or_else(|| opt_u32(&args, "left_page"))
                .or_else(|| opt_u32(&args, "leftPage"))
                .unwrap_or(0);
            let right_page = opt_u32(&args, "right_page")
                .or_else(|| opt_u32(&args, "rightPage"))
                .unwrap_or(left_page);
            ensure_page_in_range("left", left_page, left.core.page_count())?;
            let target = require_compare_target(&args)?;

            if target == "other_session_id" || target == "otherSessionId" {
                let other_session_id = req_str(&args, target)?;
                let other = self.sessions.get(other_session_id)?;
                ensure_page_in_range("right", right_page, other.core.page_count())?;
                let right_label = format!("session:{other_session_id}");
                return compare_render_png_json(
                    &left_label,
                    &left.core,
                    left_page,
                    &right_label,
                    &other.core,
                    right_page,
                );
            }

            let path = req_str(&args, target)?;
            let resolved = self.guard.resolve_existing_file(path)?;
            let bytes = fs::read(&resolved)
                .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
            let other = DocumentCore::from_bytes(&bytes).map_err(|e| e.to_string())?;
            ensure_page_in_range("right", right_page, other.page_count())?;
            let right_label = resolved.display().to_string();
            compare_render_png_json(
                &left_label,
                &left.core,
                left_page,
                &right_label,
                &other,
                right_page,
            )
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = args;
            Err("rhwp_compare_render_png is not available on wasm32".to_string())
        }
    }

    fn match_render_pages(&self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?;
        let left = self.sessions.get(session_id)?;
        let left_label = format!("session:{session_id}");
        let source_page = opt_u32(&args, "source_page")
            .or_else(|| opt_u32(&args, "sourcePage"))
            .or_else(|| opt_u32(&args, "page"))
            .unwrap_or(0);
        ensure_page_in_range("source", source_page, left.core.page_count())?;
        let (source_svg, source_overflows) = left
            .core
            .render_page_svg_native_with_overflows(source_page)
            .map_err(|e| e.to_string())?;
        let source_signature = render_page_signature(&source_svg);
        let target = require_compare_target(&args)?;

        if target == "other_session_id" || target == "otherSessionId" {
            let other_session_id = req_str(&args, target)?;
            let other = self.sessions.get(other_session_id)?;
            let right_label = format!("session:{other_session_id}");
            return match_render_pages_json(
                &left_label,
                left.core.page_count(),
                source_page,
                &source_signature,
                source_overflows,
                &right_label,
                &other.core,
                &args,
            );
        }

        let path = req_str(&args, target)?;
        let resolved = self.guard.resolve_existing_file(path)?;
        let bytes = fs::read(&resolved)
            .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
        let other = DocumentCore::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let right_label = resolved.display().to_string();
        match_render_pages_json(
            &left_label,
            left.core.page_count(),
            source_page,
            &source_signature,
            source_overflows,
            &right_label,
            &other,
            &args,
        )
    }

    fn compare_hwp_records(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?.to_string();
        let section = opt_u32(&args, "section");
        let max_diffs = opt_usize(&args, "max_diffs")
            .or_else(|| opt_usize(&args, "maxDiffs"))
            .unwrap_or(50);
        let (left_label, left_bytes) = {
            let session = self.sessions.get_mut(&session_id)?;
            (
                format!("session:{session_id}"),
                export_core_bytes(&mut session.core, "hwp")?,
            )
        };

        let target = require_compare_target(&args)?;
        let (right_label, right_bytes) =
            if target == "other_session_id" || target == "otherSessionId" {
                let other_session_id = req_str(&args, target)?;
                let session = self.sessions.get_mut(other_session_id)?;
                (
                    format!("session:{other_session_id}"),
                    export_core_bytes(&mut session.core, "hwp")?,
                )
            } else {
                let path = req_str(&args, target)?;
                let resolved = self.guard.resolve_existing_file(path)?;
                let bytes = fs::read(&resolved)
                    .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
                (
                    resolved.display().to_string(),
                    hwp_record_source_bytes(&bytes)?,
                )
            };

        let left_inventory = build_inventory_from_bytes(&left_label, "left", &left_bytes, section)?;
        let right_inventory =
            build_inventory_from_bytes(&right_label, "right", &right_bytes, section)?;
        Ok(record_diff_json(
            &left_label,
            &right_label,
            &left_inventory.items,
            &right_inventory.items,
            section,
            max_diffs,
        ))
    }

    fn compare_hwpx_package(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let session_id = req_str(&args, "session_id")?.to_string();
        let max_diffs = opt_usize(&args, "max_diffs")
            .or_else(|| opt_usize(&args, "maxDiffs"))
            .unwrap_or(50);
        let (left_label, left_bytes) = {
            let session = self.sessions.get_mut(&session_id)?;
            (
                format!("session:{session_id}"),
                export_core_bytes(&mut session.core, "hwpx")?,
            )
        };

        let target = require_compare_target(&args)?;
        let (right_label, right_bytes) =
            if target == "other_session_id" || target == "otherSessionId" {
                let other_session_id = req_str(&args, target)?;
                let session = self.sessions.get_mut(other_session_id)?;
                (
                    format!("session:{other_session_id}"),
                    export_core_bytes(&mut session.core, "hwpx")?,
                )
            } else {
                let path = req_str(&args, target)?;
                let resolved = self.guard.resolve_existing_file(path)?;
                let bytes = fs::read(&resolved)
                    .map_err(|e| format!("failed to read {}: {e}", resolved.display()))?;
                (
                    resolved.display().to_string(),
                    hwpx_package_source_bytes(&bytes)?,
                )
            };

        let left_entries = hwpx_package_entries(&left_label, &left_bytes)?;
        let right_entries = hwpx_package_entries(&right_label, &right_bytes)?;
        Ok(hwpx_package_diff_json(
            &left_label,
            &right_label,
            &left_entries,
            &right_entries,
            max_diffs,
        ))
    }

    fn create_style(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let session = self.sessions.get_mut(&id)?;
        if let Some(raw_data) = style_raw_data_arg(&args)? {
            reject_raw_style_with_semantic_args(&args, false)?;
            let style = style_from_raw_hwp_payload(raw_data)?;
            validate_style_refs(
                &session.core,
                &style,
                session.core.document.doc_info.styles.len() + 1,
                "raw_hwp_style_base64",
            )?;
            let response =
                style_mutation_response(session.core.document.doc_info.styles.len(), &style, true);
            session.core.document.doc_info.styles.push(style);
            session.core.document.doc_info.raw_stream_dirty = true;
            session.core.styles = resolve_styles(&session.core.document.doc_info, session.core.dpi);
            session.dirty = true;
            return Ok(response);
        }
        let base_style_id = opt_usize(&args, "base_style_id")
            .or_else(|| opt_usize(&args, "baseStyleId"))
            .or_else(|| opt_usize(&args, "based_on_style_id"))
            .or_else(|| opt_usize(&args, "basedOnStyleId"));
        let (fallback_char_shape_id, fallback_para_shape_id, fallback_lang_id) = match base_style_id
        {
            Some(style_id) => {
                let style = session
                    .core
                    .document
                    .doc_info
                    .styles
                    .get(style_id)
                    .ok_or_else(|| format!("base_style_id out of range: {style_id}"))?;
                (style.char_shape_id, style.para_shape_id, style.lang_id)
            }
            None => session
                .core
                .document
                .doc_info
                .styles
                .first()
                .map(|style| (style.char_shape_id, style.para_shape_id, style.lang_id))
                .unwrap_or((0, 0, 1042)),
        };
        let explicit_char_shape_id = style_u16_ref_arg(
            &args,
            &[
                "char_shape_id",
                "charShapeId",
                "base_char_shape_id",
                "baseCharShapeId",
            ],
            "char_shape_id",
        )?;
        let explicit_para_shape_id = style_u16_ref_arg(
            &args,
            &[
                "para_shape_id",
                "paraShapeId",
                "base_para_shape_id",
                "baseParaShapeId",
            ],
            "para_shape_id",
        )?;
        let mut char_shape_id = match explicit_char_shape_id {
            Some(id) => {
                if id as usize >= session.core.document.doc_info.char_shapes.len() {
                    return Err(format!("char_shape_id out of range: {id}"));
                }
                id
            }
            None => fallback_char_shape_id,
        };
        let mut para_shape_id = match explicit_para_shape_id {
            Some(id) => {
                if id as usize >= session.core.document.doc_info.para_shapes.len() {
                    return Err(format!("para_shape_id out of range: {id}"));
                }
                id
            }
            None => fallback_para_shape_id,
        };
        let next_style_id =
            style_u8_arg(&args, &["next_style_id", "nextStyleId"], "next_style_id")?.unwrap_or(0);
        validate_next_style_id(
            next_style_id,
            session.core.document.doc_info.styles.len() + 1,
            "next_style_id",
        )?;
        let style_type = style_u8_arg(&args, &["type", "style_type"], "style_type")?.unwrap_or(0);
        validate_style_type_id(style_type, "style_type")?;
        let lang_id =
            style_i16_arg(&args, &["lang_id", "langId"], "lang_id")?.unwrap_or(fallback_lang_id);
        let char_format_json =
            style_format_json(&args, "char_format", "charFormat", "character_format")?;
        let para_format_json =
            style_format_json(&args, "para_format", "paraFormat", "paragraph_format")?;
        if let Some(format_json) = char_format_json.as_deref() {
            validate_style_char_format(format_json)?;
        }
        if let Some(format_json) = para_format_json.as_deref() {
            validate_style_para_format(format_json)?;
        }
        if let Some(format_json) = char_format_json.as_deref() {
            char_shape_id =
                style_char_shape_from_format(&mut session.core, char_shape_id, format_json)?;
        }
        if let Some(format_json) = para_format_json.as_deref() {
            para_shape_id =
                style_para_shape_from_format(&mut session.core, para_shape_id, format_json)?;
        }
        let style = Style {
            raw_data: None,
            local_name: opt_str(&args, "name").unwrap_or("").to_string(),
            english_name: opt_str(&args, "english_name")
                .or_else(|| opt_str(&args, "englishName"))
                .unwrap_or("")
                .to_string(),
            style_type,
            next_style_id,
            lang_id,
            para_shape_id,
            char_shape_id,
        };
        let style_type = style.style_type;
        let next_style_id = style.next_style_id;
        let lang_id = style.lang_id;
        session.core.document.doc_info.styles.push(style);
        session.core.document.doc_info.raw_stream_dirty = true;
        session.core.styles = resolve_styles(&session.core.document.doc_info, session.core.dpi);
        session.dirty = true;
        let mut response = json!({
            "ok": true,
            "style_id": session.core.document.doc_info.styles.len() - 1,
            "type": style_type,
            "nextStyleId": next_style_id,
            "langId": lang_id,
            "paraShapeId": para_shape_id,
            "charShapeId": char_shape_id,
        });
        if let Some(base_style_id) = base_style_id {
            if let Some(object) = response.as_object_mut() {
                object.insert("baseStyleId".to_string(), json!(base_style_id));
            }
        }
        Ok(response)
    }

    fn update_style(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let style_id = req_usize(&args, "style_id")?;
        let session = self.sessions.get_mut(&id)?;
        if let Some(raw_data) = style_raw_data_arg(&args)? {
            reject_raw_style_with_semantic_args(&args, true)?;
            if style_id >= session.core.document.doc_info.styles.len() {
                return Err(format!("style_id out of range: {style_id}"));
            }
            let style = style_from_raw_hwp_payload(raw_data)?;
            validate_style_refs(
                &session.core,
                &style,
                session.core.document.doc_info.styles.len(),
                "raw_hwp_style_base64",
            )?;
            let response = style_mutation_response(style_id, &style, true);
            session.core.document.doc_info.styles[style_id] = style;
            session.core.document.doc_info.raw_stream_dirty = true;
            session.core.styles = resolve_styles(&session.core.document.doc_info, session.core.dpi);
            session.core.mark_all_sections_dirty();
            session.core.paginate_if_needed();
            session.dirty = true;
            return Ok(response);
        }
        let style_type_update = style_u8_arg(&args, &["type", "style_type"], "style_type")?;
        if let Some(style_type) = style_type_update {
            validate_style_type_id(style_type, "style_type")?;
        }
        let next_style_id_update =
            style_u8_arg(&args, &["next_style_id", "nextStyleId"], "next_style_id")?;
        if let Some(next_style_id) = next_style_id_update {
            validate_next_style_id(
                next_style_id,
                session.core.document.doc_info.styles.len(),
                "next_style_id",
            )?;
        }
        let current_style = session
            .core
            .document
            .doc_info
            .styles
            .get(style_id)
            .ok_or_else(|| format!("style_id out of range: {style_id}"))?;
        let current_char_shape_id = current_style.char_shape_id;
        let current_para_shape_id = current_style.para_shape_id;
        let base_style_refs = match opt_usize(&args, "base_style_id")
            .or_else(|| opt_usize(&args, "baseStyleId"))
            .or_else(|| opt_usize(&args, "based_on_style_id"))
            .or_else(|| opt_usize(&args, "basedOnStyleId"))
        {
            Some(base_style_id) => {
                let base_style = session
                    .core
                    .document
                    .doc_info
                    .styles
                    .get(base_style_id)
                    .ok_or_else(|| format!("base_style_id out of range: {base_style_id}"))?;
                Some((
                    base_style_id,
                    base_style.char_shape_id,
                    base_style.para_shape_id,
                    base_style.lang_id,
                ))
            }
            None => None,
        };
        let explicit_char_shape_update =
            style_u16_ref_arg(&args, &["char_shape_id", "charShapeId"], "char_shape_id")?;
        let explicit_para_shape_update =
            style_u16_ref_arg(&args, &["para_shape_id", "paraShapeId"], "para_shape_id")?;
        let explicit_lang_id_update = style_i16_arg(&args, &["lang_id", "langId"], "lang_id")?;
        let mut char_shape_update = explicit_char_shape_update
            .or(base_style_refs.map(|(_, char_shape_id, _, _)| char_shape_id));
        let mut para_shape_update = explicit_para_shape_update
            .or(base_style_refs.map(|(_, _, para_shape_id, _)| para_shape_id));
        let lang_id_update =
            explicit_lang_id_update.or(base_style_refs.map(|(_, _, _, lang_id)| lang_id));
        if let Some(char_shape_id) = char_shape_update {
            if char_shape_id as usize >= session.core.document.doc_info.char_shapes.len() {
                return Err(format!("char_shape_id out of range: {char_shape_id}"));
            }
        }
        if let Some(para_shape_id) = para_shape_update {
            if para_shape_id as usize >= session.core.document.doc_info.para_shapes.len() {
                return Err(format!("para_shape_id out of range: {para_shape_id}"));
            }
        }
        let char_format_json =
            style_format_json(&args, "char_format", "charFormat", "character_format")?;
        let para_format_json =
            style_format_json(&args, "para_format", "paraFormat", "paragraph_format")?;
        if let Some(format_json) = char_format_json.as_deref() {
            validate_style_char_format(format_json)?;
        }
        if let Some(format_json) = para_format_json.as_deref() {
            validate_style_para_format(format_json)?;
        }
        if let Some(format_json) = char_format_json.as_deref() {
            let base_id = char_shape_update.unwrap_or(current_char_shape_id);
            char_shape_update = Some(style_char_shape_from_format(
                &mut session.core,
                base_id,
                format_json,
            )?);
        }
        if let Some(format_json) = para_format_json.as_deref() {
            let base_id = para_shape_update.unwrap_or(current_para_shape_id);
            para_shape_update = Some(style_para_shape_from_format(
                &mut session.core,
                base_id,
                format_json,
            )?);
        }
        let (
            updated_style_type,
            updated_next_style_id,
            updated_lang_id,
            updated_para_shape_id,
            updated_char_shape_id,
        ) = {
            let style = session
                .core
                .document
                .doc_info
                .styles
                .get_mut(style_id)
                .ok_or_else(|| format!("style_id out of range: {style_id}"))?;
            if let Some(name) = opt_str(&args, "name") {
                style.local_name = name.to_string();
            }
            if let Some(name) =
                opt_str(&args, "english_name").or_else(|| opt_str(&args, "englishName"))
            {
                style.english_name = name.to_string();
            }
            if let Some(next) = next_style_id_update {
                style.next_style_id = next;
            }
            if let Some(lang_id) = lang_id_update {
                style.lang_id = lang_id;
            }
            if let Some(style_type) = style_type_update {
                style.style_type = style_type;
            }
            if let Some(char_shape_id) = char_shape_update {
                style.char_shape_id = char_shape_id;
            }
            if let Some(para_shape_id) = para_shape_update {
                style.para_shape_id = para_shape_id;
            }
            style.raw_data = None;
            (
                style.style_type,
                style.next_style_id,
                style.lang_id,
                style.para_shape_id,
                style.char_shape_id,
            )
        };
        session.core.document.doc_info.raw_stream_dirty = true;
        session.core.styles = resolve_styles(&session.core.document.doc_info, session.core.dpi);
        session.core.mark_all_sections_dirty();
        session.core.paginate_if_needed();
        session.dirty = true;
        let mut response = json!({
            "ok": true,
            "style_id": style_id,
            "type": updated_style_type,
            "nextStyleId": updated_next_style_id,
            "langId": updated_lang_id,
            "paraShapeId": updated_para_shape_id,
            "charShapeId": updated_char_shape_id,
        });
        if let Some((base_style_id, _, _, _)) = base_style_refs {
            if let Some(object) = response.as_object_mut() {
                object.insert("baseStyleId".to_string(), json!(base_style_id));
            }
        }
        Ok(response)
    }

    fn delete_style(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let style_id = req_usize(&args, "style_id")?;
        if style_id == 0 {
            return Err("style_id 0 cannot be deleted".to_string());
        }
        let session = self.sessions.get_mut(&id)?;
        if style_id >= session.core.document.doc_info.styles.len() {
            return Err(format!("style_id out of range: {style_id}"));
        }
        if style_id > u8::MAX as usize {
            return Err(format!(
                "style_id out of range for HWP style refs: {style_id}"
            ));
        }
        let sid = style_id as u8;
        let mut remapped_style_ref_count = 0usize;
        for section in &mut session.core.document.sections {
            remapped_style_ref_count +=
                Self::remap_deleted_style_refs_in_paragraphs(&mut section.paragraphs, sid);
            for master_page in &mut section.section_def.master_pages {
                remapped_style_ref_count +=
                    Self::remap_deleted_style_refs_in_paragraphs(&mut master_page.paragraphs, sid);
            }
        }
        session.core.document.doc_info.styles.remove(style_id);
        for style in &mut session.core.document.doc_info.styles {
            if style.next_style_id == sid {
                style.next_style_id = 0;
            } else if style.next_style_id > sid {
                style.next_style_id -= 1;
            }
        }
        session.core.document.doc_info.raw_stream_dirty = true;
        session.core.styles = resolve_styles(&session.core.document.doc_info, session.core.dpi);
        session.core.mark_all_sections_dirty();
        session.core.paginate_if_needed();
        session.dirty = true;
        Ok(json!({
            "ok": true,
            "deleted_style_id": style_id,
            "remappedStyleRefCount": remapped_style_ref_count,
        }))
    }

    fn remap_deleted_style_refs_in_paragraphs(
        paragraphs: &mut [Paragraph],
        deleted_style_id: u8,
    ) -> usize {
        let mut count = 0usize;
        for paragraph in paragraphs {
            count += Self::remap_deleted_style_ref(&mut paragraph.style_id, deleted_style_id);
            for control in &mut paragraph.controls {
                count += Self::remap_deleted_style_refs_in_control(control, deleted_style_id);
            }
        }
        count
    }

    fn remap_deleted_style_ref(style_id: &mut u8, deleted_style_id: u8) -> usize {
        if *style_id == deleted_style_id {
            *style_id = 0;
            1
        } else if *style_id > deleted_style_id {
            *style_id -= 1;
            1
        } else {
            0
        }
    }

    fn remap_deleted_style_refs_in_control(control: &mut Control, deleted_style_id: u8) -> usize {
        match control {
            Control::Table(table) => {
                let mut count = 0usize;
                for cell in &mut table.cells {
                    count += Self::remap_deleted_style_refs_in_paragraphs(
                        &mut cell.paragraphs,
                        deleted_style_id,
                    );
                }
                if let Some(caption) = &mut table.caption {
                    count += Self::remap_deleted_style_refs_in_paragraphs(
                        &mut caption.paragraphs,
                        deleted_style_id,
                    );
                }
                count
            }
            Control::Shape(shape) => {
                Self::remap_deleted_style_refs_in_shape(shape, deleted_style_id)
            }
            Control::Picture(picture) => picture
                .caption
                .as_mut()
                .map(|caption| {
                    Self::remap_deleted_style_refs_in_paragraphs(
                        &mut caption.paragraphs,
                        deleted_style_id,
                    )
                })
                .unwrap_or(0),
            Control::Header(header) => Self::remap_deleted_style_refs_in_paragraphs(
                &mut header.paragraphs,
                deleted_style_id,
            ),
            Control::Footer(footer) => Self::remap_deleted_style_refs_in_paragraphs(
                &mut footer.paragraphs,
                deleted_style_id,
            ),
            Control::Footnote(note) => {
                Self::remap_deleted_style_refs_in_paragraphs(&mut note.paragraphs, deleted_style_id)
            }
            Control::Endnote(note) => {
                Self::remap_deleted_style_refs_in_paragraphs(&mut note.paragraphs, deleted_style_id)
            }
            Control::HiddenComment(comment) => Self::remap_deleted_style_refs_in_paragraphs(
                &mut comment.paragraphs,
                deleted_style_id,
            ),
            Control::Field(field) => Self::remap_deleted_style_refs_in_paragraphs(
                &mut field.memo_paragraphs,
                deleted_style_id,
            ),
            _ => 0,
        }
    }

    fn remap_deleted_style_refs_in_shape(shape: &mut ShapeObject, deleted_style_id: u8) -> usize {
        let mut count = 0usize;
        match shape {
            ShapeObject::Group(group) => {
                for child in &mut group.children {
                    count += Self::remap_deleted_style_refs_in_shape(child, deleted_style_id);
                }
                if let Some(caption) = &mut group.caption {
                    count += Self::remap_deleted_style_refs_in_paragraphs(
                        &mut caption.paragraphs,
                        deleted_style_id,
                    );
                }
            }
            ShapeObject::Picture(picture) => {
                if let Some(caption) = &mut picture.caption {
                    count += Self::remap_deleted_style_refs_in_paragraphs(
                        &mut caption.paragraphs,
                        deleted_style_id,
                    );
                }
            }
            ShapeObject::Chart(chart) => {
                if let Some(caption) = &mut chart.caption {
                    count += Self::remap_deleted_style_refs_in_paragraphs(
                        &mut caption.paragraphs,
                        deleted_style_id,
                    );
                }
            }
            ShapeObject::Ole(ole) => {
                if let Some(caption) = &mut ole.caption {
                    count += Self::remap_deleted_style_refs_in_paragraphs(
                        &mut caption.paragraphs,
                        deleted_style_id,
                    );
                }
            }
            _ => {}
        }
        if let Some(drawing) = shape.drawing_mut() {
            if let Some(text_box) = &mut drawing.text_box {
                count += Self::remap_deleted_style_refs_in_paragraphs(
                    &mut text_box.paragraphs,
                    deleted_style_id,
                );
            }
            if let Some(caption) = &mut drawing.caption {
                count += Self::remap_deleted_style_refs_in_paragraphs(
                    &mut caption.paragraphs,
                    deleted_style_id,
                );
            }
        }
        count
    }

    fn insert_picture(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let source = require_exactly_one_key(
            &args,
            &["image_base64", "image_path"],
            "picture image source",
        )?;
        let image_data = if source == "image_base64" {
            let encoded = req_str(&args, "image_base64")?;
            BASE64
                .decode(encoded)
                .map_err(|e| format!("invalid image_base64: {e}"))?
        } else {
            let path = req_str(&args, "image_path")?;
            let resolved = self.guard.resolve_existing_file(path)?;
            fs::read(&resolved)
                .map_err(|e| format!("failed to read image {}: {e}", resolved.display()))?
        };
        let session = self.sessions.get_mut(&id)?;
        let result = if is_header_footer_nested_target(&args) {
            session.core.insert_header_footer_picture_native(
                opt_usize(&args, "section").unwrap_or(0),
                req_usize(&args, "para")?,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                req_usize(&args, "char_offset")?,
                &image_data,
                req_u32(&args, "width")?,
                req_u32(&args, "height")?,
                opt_u32(&args, "natural_width_px")
                    .unwrap_or_else(|| req_u32(&args, "width").unwrap_or(0)),
                opt_u32(&args, "natural_height_px")
                    .unwrap_or_else(|| req_u32(&args, "height").unwrap_or(0)),
                opt_str(&args, "extension").unwrap_or("png"),
                opt_str(&args, "description").unwrap_or(""),
                opt_i32(&args, "paper_offset_x_hu"),
                opt_i32(&args, "paper_offset_y_hu"),
            )
        } else {
            let cell_path = parse_cell_path(args.get("cell_path"))?;
            session.core.insert_picture_native(
                opt_usize(&args, "section").unwrap_or(0),
                opt_usize(&args, "para").unwrap_or(0),
                req_usize(&args, "char_offset")?,
                &cell_path,
                &image_data,
                req_u32(&args, "width")?,
                req_u32(&args, "height")?,
                opt_u32(&args, "natural_width_px")
                    .unwrap_or_else(|| req_u32(&args, "width").unwrap_or(0)),
                opt_u32(&args, "natural_height_px")
                    .unwrap_or_else(|| req_u32(&args, "height").unwrap_or(0)),
                opt_str(&args, "extension").unwrap_or("png"),
                opt_str(&args, "description").unwrap_or(""),
                opt_i32(&args, "paper_offset_x_hu"),
                opt_i32(&args, "paper_offset_y_hu"),
            )
        }
        .map_err(|e| e.to_string())?;
        session.dirty = true;
        core_json(Ok(result))
    }

    fn get_picture_properties(&self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?;
        let session = self.sessions.get(id)?;
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        if let Some(group_child_path) = group_child_path_arg(&args)? {
            let cell_path = picture_cell_path_arg(&args)?;
            let cell_path_json = cell_path.as_ref().map(|path| cell_path_object_json(path));
            return core_json(
                session
                    .core
                    .get_shape_group_child_picture_properties_native(
                        section,
                        para,
                        req_usize(&args, "control")?,
                        cell_path_json.as_deref(),
                        is_header_footer_nested_target(&args).then(|| nested_para_arg(&args)),
                        (cell_path.is_some() || is_header_footer_nested_target(&args))
                            .then(|| req_inner_control(&args))
                            .transpose()?,
                        &group_child_path_json(&group_child_path),
                    ),
            );
        }
        if let Some(cell_path) = picture_cell_path_arg(&args)? {
            let inner_control = req_inner_control(&args)?;
            return core_json(session.core.get_cell_picture_properties_by_path_native(
                section,
                para,
                &cell_path_object_json(&cell_path),
                inner_control,
            ));
        }
        if is_header_footer_nested_target(&args) {
            return core_json(session.core.get_header_footer_picture_properties_native(
                section,
                para,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                req_inner_control(&args)?,
            ));
        }
        core_json(session.core.get_picture_properties_native(
            section,
            para,
            req_usize(&args, "control")?,
        ))
    }

    fn set_picture_properties(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        let props = props_json(&args);
        let session = self.sessions.get_mut(&id)?;
        let result = if let Some(group_child_path) = group_child_path_arg(&args)? {
            let cell_path = picture_cell_path_arg(&args)?;
            let cell_path_json = cell_path.as_ref().map(|path| cell_path_object_json(path));
            session
                .core
                .set_shape_group_child_picture_properties_native(
                    section,
                    para,
                    req_usize(&args, "control")?,
                    cell_path_json.as_deref(),
                    is_header_footer_nested_target(&args).then(|| nested_para_arg(&args)),
                    (cell_path.is_some() || is_header_footer_nested_target(&args))
                        .then(|| req_inner_control(&args))
                        .transpose()?,
                    &group_child_path_json(&group_child_path),
                    &props,
                )
        } else if let Some(cell_path) = picture_cell_path_arg(&args)? {
            session.core.set_cell_picture_properties_by_path_native(
                section,
                para,
                &cell_path_object_json(&cell_path),
                req_inner_control(&args)?,
                &props,
            )
        } else if is_header_footer_nested_target(&args) {
            session.core.set_header_footer_picture_properties_native(
                section,
                para,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                req_inner_control(&args)?,
                &props,
            )
        } else {
            session.core.set_picture_properties_native(
                section,
                para,
                req_usize(&args, "control")?,
                &props,
            )
        };
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn get_shape_properties(&self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?;
        let session = self.sessions.get(id)?;
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        if let Some(group_child_path) = group_child_path_arg(&args)? {
            let cell_path = shape_cell_path_arg(&args)?;
            let cell_path_json = cell_path.as_ref().map(|path| cell_path_object_json(path));
            return core_json(
                session.core.get_shape_group_child_properties_native(
                    section,
                    para,
                    req_usize(&args, "control")?,
                    cell_path_json.as_deref(),
                    is_header_footer_nested_target(&args).then(|| nested_para_arg(&args)),
                    (cell_path.is_some() || is_header_footer_nested_target(&args))
                        .then(|| req_inner_control(&args))
                        .transpose()?,
                    &group_child_path_json(&group_child_path),
                ),
            );
        }
        if let Some(cell_path) = shape_cell_path_arg(&args)? {
            return core_json(session.core.get_cell_shape_properties_by_path_native(
                section,
                para,
                &cell_path_object_json(&cell_path),
                req_inner_control(&args)?,
            ));
        }
        if is_header_footer_nested_target(&args) {
            return core_json(session.core.get_header_footer_shape_properties_native(
                section,
                para,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                req_inner_control(&args)?,
            ));
        }
        core_json(session.core.get_shape_properties_native(
            section,
            para,
            req_usize(&args, "control")?,
        ))
    }

    fn set_shape_properties(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        let props = props_json(&args);
        let session = self.sessions.get_mut(&id)?;
        let result = if let Some(group_child_path) = group_child_path_arg(&args)? {
            let cell_path = shape_cell_path_arg(&args)?;
            let cell_path_json = cell_path.as_ref().map(|path| cell_path_object_json(path));
            session.core.set_shape_group_child_properties_native(
                section,
                para,
                req_usize(&args, "control")?,
                cell_path_json.as_deref(),
                is_header_footer_nested_target(&args).then(|| nested_para_arg(&args)),
                (cell_path.is_some() || is_header_footer_nested_target(&args))
                    .then(|| req_inner_control(&args))
                    .transpose()?,
                &group_child_path_json(&group_child_path),
                &props,
            )
        } else if let Some(cell_path) = shape_cell_path_arg(&args)? {
            session.core.set_cell_shape_properties_by_path_native(
                section,
                para,
                &cell_path_object_json(&cell_path),
                req_inner_control(&args)?,
                &props,
            )
        } else if is_header_footer_nested_target(&args) {
            session.core.set_header_footer_shape_properties_native(
                section,
                para,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                req_inner_control(&args)?,
                &props,
            )
        } else {
            session.core.set_shape_properties_native(
                section,
                para,
                req_usize(&args, "control")?,
                &props,
            )
        };
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn get_chart_data(&self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?;
        let session = self.sessions.get(id)?;
        core_json(session.core.get_chart_data_native(
            opt_usize(&args, "section").unwrap_or(0),
            req_usize(&args, "para")?,
            req_usize(&args, "control")?,
        ))
    }

    fn set_chart_data(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let props = props_json(&args);
        let session = self.sessions.get_mut(&id)?;
        let result = session.core.set_chart_data_native(
            opt_usize(&args, "section").unwrap_or(0),
            req_usize(&args, "para")?,
            req_usize(&args, "control")?,
            &props,
        );
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn insert_shape(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = opt_usize(&args, "para").unwrap_or(0);
        let char_offset = req_usize(&args, "char_offset")?;
        let width = req_u32(&args, "width")?;
        let height = req_u32(&args, "height")?;
        let horizontal_offset = opt_u32(&args, "horizontal_offset")
            .or_else(|| opt_u32(&args, "horizontalOffset"))
            .or_else(|| opt_u32(&args, "horz_offset"))
            .or_else(|| opt_u32(&args, "horzOffset"))
            .unwrap_or(0);
        let vertical_offset = opt_u32(&args, "vertical_offset")
            .or_else(|| opt_u32(&args, "verticalOffset"))
            .or_else(|| opt_u32(&args, "vert_offset"))
            .or_else(|| opt_u32(&args, "vertOffset"))
            .unwrap_or(0);
        let treat_as_char = opt_bool(&args, "treat_as_char")
            .or_else(|| opt_bool(&args, "treatAsChar"))
            .unwrap_or(false);
        let shape_type = opt_str(&args, "shape_type")
            .or_else(|| opt_str(&args, "shapeType"))
            .unwrap_or("rectangle");
        let text_wrap = opt_str(&args, "text_wrap")
            .or_else(|| opt_str(&args, "textWrap"))
            .unwrap_or("InFrontOfText");
        let line_flip_x = opt_bool(&args, "line_flip_x")
            .or_else(|| opt_bool(&args, "lineFlipX"))
            .unwrap_or(false);
        let line_flip_y = opt_bool(&args, "line_flip_y")
            .or_else(|| opt_bool(&args, "lineFlipY"))
            .unwrap_or(false);
        let polygon_points = polygon_points_arg(&args)?;
        let cell_path = shape_cell_path_arg(&args)?;
        let cell_path_json = cell_path.as_ref().map(|path| cell_path_object_json(path));
        let session = self.sessions.get_mut(&id)?;
        let result = if let Some(cell_path_json) = cell_path_json.as_deref() {
            session.core.create_cell_shape_control_by_path_native(
                section,
                para,
                cell_path_json,
                char_offset,
                width,
                height,
                horizontal_offset,
                vertical_offset,
                treat_as_char,
                text_wrap,
                shape_type,
                line_flip_x,
                line_flip_y,
                &polygon_points,
            )
        } else if is_header_footer_nested_target(&args) {
            session.core.create_header_footer_shape_control_native(
                section,
                para,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                char_offset,
                width,
                height,
                horizontal_offset,
                vertical_offset,
                treat_as_char,
                text_wrap,
                shape_type,
                line_flip_x,
                line_flip_y,
                &polygon_points,
            )
        } else {
            session.core.create_shape_control_native(
                section,
                para,
                char_offset,
                width,
                height,
                horizontal_offset,
                vertical_offset,
                treat_as_char,
                text_wrap,
                shape_type,
                line_flip_x,
                line_flip_y,
                &polygon_points,
            )
        };
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn insert_shape_group_child(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        let control = req_usize(&args, "control")?;
        let shape_type = opt_str(&args, "shape_type")
            .or_else(|| opt_str(&args, "shapeType"))
            .unwrap_or("rectangle");
        let text_wrap = opt_str(&args, "text_wrap")
            .or_else(|| opt_str(&args, "textWrap"))
            .unwrap_or("InFrontOfText");
        let polygon_points = polygon_points_arg(&args)?;
        let group_child_path = group_child_path_arg(&args)?;
        let group_child_path_json = group_child_path
            .as_ref()
            .map(|path| group_child_path_json(path));
        let cell_path = shape_cell_path_arg(&args)?;
        let cell_path_json = cell_path.as_ref().map(|path| cell_path_object_json(path));
        let session = self.sessions.get_mut(&id)?;
        let result = session.core.insert_shape_group_child_native(
            section,
            para,
            control,
            cell_path_json.as_deref(),
            is_header_footer_nested_target(&args).then(|| nested_para_arg(&args)),
            (cell_path.is_some() || is_header_footer_nested_target(&args))
                .then(|| req_inner_control(&args))
                .transpose()?,
            group_child_path_json.as_deref(),
            opt_usize(&args, "child_index").or_else(|| opt_usize(&args, "childIndex")),
            req_u32(&args, "width")?,
            req_u32(&args, "height")?,
            opt_u32(&args, "horizontal_offset")
                .or_else(|| opt_u32(&args, "horizontalOffset"))
                .or_else(|| opt_u32(&args, "horz_offset"))
                .or_else(|| opt_u32(&args, "horzOffset"))
                .unwrap_or(0),
            opt_u32(&args, "vertical_offset")
                .or_else(|| opt_u32(&args, "verticalOffset"))
                .or_else(|| opt_u32(&args, "vert_offset"))
                .or_else(|| opt_u32(&args, "vertOffset"))
                .unwrap_or(0),
            opt_bool(&args, "treat_as_char")
                .or_else(|| opt_bool(&args, "treatAsChar"))
                .unwrap_or(true),
            text_wrap,
            shape_type,
            opt_bool(&args, "line_flip_x")
                .or_else(|| opt_bool(&args, "lineFlipX"))
                .unwrap_or(false),
            opt_bool(&args, "line_flip_y")
                .or_else(|| opt_bool(&args, "lineFlipY"))
                .unwrap_or(false),
            &polygon_points,
        );
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn delete_shape(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        let cell_path = shape_cell_path_arg(&args)?;
        let group_child_path = group_child_path_arg(&args)?;
        let session = self.sessions.get_mut(&id)?;
        let result = if let Some(group_child_path) = group_child_path {
            let cell_path_json = cell_path.as_ref().map(|path| cell_path_object_json(path));
            session.core.delete_shape_group_child_native(
                section,
                para,
                req_usize(&args, "control")?,
                cell_path_json.as_deref(),
                is_header_footer_nested_target(&args).then(|| nested_para_arg(&args)),
                (cell_path.is_some() || is_header_footer_nested_target(&args))
                    .then(|| req_inner_control(&args))
                    .transpose()?,
                &group_child_path_json(&group_child_path),
            )
        } else if let Some(cell_path) = cell_path {
            session.core.delete_cell_shape_control_by_path_native(
                section,
                para,
                &cell_path_object_json(&cell_path),
                req_inner_control(&args)?,
            )
        } else if is_header_footer_nested_target(&args) {
            session.core.delete_header_footer_shape_control_native(
                section,
                para,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                req_inner_control(&args)?,
            )
        } else {
            session
                .core
                .delete_shape_control_native(section, para, req_usize(&args, "control")?)
        };
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn change_shape_z_order(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        let operation = req_str(&args, "operation")?;
        let session = self.sessions.get_mut(&id)?;
        let result = if let Some(group_child_path) = group_child_path_arg(&args)? {
            let cell_path = shape_cell_path_arg(&args)?;
            let cell_path_json = cell_path.as_ref().map(|path| cell_path_object_json(path));
            session.core.change_shape_group_child_z_order_native(
                section,
                para,
                req_usize(&args, "control")?,
                cell_path_json.as_deref(),
                is_header_footer_nested_target(&args).then(|| nested_para_arg(&args)),
                (cell_path.is_some() || is_header_footer_nested_target(&args))
                    .then(|| req_inner_control(&args))
                    .transpose()?,
                &group_child_path_json(&group_child_path),
                operation,
            )
        } else {
            session.core.change_shape_z_order_native(
                section,
                para,
                req_usize(&args, "control")?,
                operation,
            )
        };
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn group_shapes(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let session = self.sessions.get_mut(&id)?;
        let result = if is_header_footer_nested_target(&args) {
            let para = req_usize(&args, "para")?;
            let control = req_usize(&args, "control")?;
            let targets = header_footer_shape_group_targets_arg(
                &args,
                section,
                para,
                control,
                nested_para_arg(&args),
            )?;
            session
                .core
                .group_header_footer_shapes_native(section, para, control, &targets)
        } else {
            let targets = shape_group_targets_arg(&args, section)?;
            session.core.group_shapes_native(section, &targets)
        };
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn get_shape_group_children(&self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?;
        let section_idx = opt_usize(&args, "section").unwrap_or(0);
        let para_idx = req_usize(&args, "para")?;
        let control_idx = req_usize(&args, "control")?;
        let cell_path = shape_cell_path_arg(&args)?;
        let session = self.sessions.get(id)?;
        let section = session
            .core
            .document
            .sections
            .get(section_idx)
            .ok_or_else(|| format!("section {section_idx} not found"))?;
        let para = section
            .paragraphs
            .get(para_idx)
            .ok_or_else(|| format!("para {para_idx} not found"))?;

        let shape: &ShapeObject;
        let path_prefix: String;
        let mut target_meta = Map::new();

        if let Some(cell_path) = cell_path {
            let (inner_para, target_scope) =
                paragraph_by_cell_path_with_scope(section, para_idx, &cell_path)?;
            let inner_control_idx = req_inner_control(&args)?;
            let inner_control = inner_para
                .controls
                .get(inner_control_idx)
                .ok_or_else(|| format!("inner_control {inner_control_idx} not found"))?;
            let Control::Shape(inner_shape) = inner_control else {
                return Err("target inner control is not a Shape".to_string());
            };
            shape = inner_shape.as_ref();
            path_prefix = format!(
                "section[{section_idx}].para[{para_idx}].cell_path{}.control[{inner_control_idx}]",
                cell_path
                    .iter()
                    .map(|(control, cell, para)| format!("[{control},{cell},{para}]"))
                    .collect::<Vec<_>>()
                    .join("")
            );
            target_meta.insert("scope".to_string(), json!(target_scope));
            target_meta.insert("cell_path".to_string(), cell_path_json(&cell_path));
            target_meta.insert("inner_control".to_string(), json!(inner_control_idx));
        } else if is_header_footer_nested_target(&args) {
            let outer_control = para
                .controls
                .get(control_idx)
                .ok_or_else(|| format!("control {control_idx} not found"))?;
            let (scope, inner_paragraphs) = match outer_control {
                Control::Header(header) => ("header", header.paragraphs.as_slice()),
                Control::Footer(footer) => ("footer", footer.paragraphs.as_slice()),
                _ => return Err("target control is not a Header/Footer".to_string()),
            };
            let inner_para_idx = nested_para_arg(&args);
            let inner_para = inner_paragraphs
                .get(inner_para_idx)
                .ok_or_else(|| format!("inner_para {inner_para_idx} not found"))?;
            let inner_control_idx = req_inner_control(&args)?;
            let inner_control = inner_para
                .controls
                .get(inner_control_idx)
                .ok_or_else(|| format!("inner_control {inner_control_idx} not found"))?;
            let Control::Shape(inner_shape) = inner_control else {
                return Err("target inner control is not a Shape".to_string());
            };
            shape = inner_shape.as_ref();
            path_prefix = format!(
                "section[{section_idx}].para[{para_idx}].control[{control_idx}].{scope}.para[{inner_para_idx}].control[{inner_control_idx}]"
            );
            target_meta.insert("scope".to_string(), json!(scope));
            target_meta.insert("container_scope".to_string(), json!(scope));
            target_meta.insert("inner_para".to_string(), json!(inner_para_idx));
            target_meta.insert("hf_para".to_string(), json!(inner_para_idx));
            target_meta.insert("inner_control".to_string(), json!(inner_control_idx));
        } else {
            let control = para
                .controls
                .get(control_idx)
                .ok_or_else(|| format!("control {control_idx} not found"))?;
            let Control::Shape(body_shape) = control else {
                return Err("target control is not a Shape".to_string());
            };
            shape = body_shape.as_ref();
            path_prefix = format!("section[{section_idx}].para[{para_idx}].control[{control_idx}]");
            target_meta.insert("scope".to_string(), json!("body"));
        }

        let requested_child_path = group_child_path_arg(&args)?;
        let mut target_path_prefix = path_prefix;
        let mut target_shape = shape;
        if let Some(child_path) = requested_child_path.as_deref() {
            target_shape = shape_group_child_ref_for_mcp(shape, child_path)?;
            for child_idx in child_path {
                target_path_prefix.push_str(&format!(".child[{child_idx}]"));
            }
            target_meta.insert("scope".to_string(), json!("shape_group_child"));
            target_meta.insert("group_child".to_string(), json!(child_path.last().copied()));
            target_meta.insert("group_child_path".to_string(), json!(child_path));
        }

        let ShapeObject::Group(group) = target_shape else {
            return Err("target shape is not a ShapeGroup".to_string());
        };
        let parent_child_path = requested_child_path.clone().unwrap_or_default();

        let mut response = json!({
            "session_id": id,
            "section": section_idx,
            "para": para_idx,
            "control": control_idx,
            "kind": "ShapeGroup",
            "child_count": group.children.len(),
            "has_caption": group.caption.is_some(),
            "summary": shape_object_summary_json(target_shape),
            "children": group
                .children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    let mut value = shape_object_summary_json(child);
                    if let Some(object) = value.as_object_mut() {
                        let mut child_path = parent_child_path.clone();
                        child_path.push(index);
                        object.insert("index".to_string(), json!(index));
                        object.insert("group_child".to_string(), json!(index));
                        object.insert("group_child_path".to_string(), json!(child_path));
                        object.insert(
                            "path".to_string(),
                            json!(format!(
                                "{target_path_prefix}.child[{index}]"
                            )),
                        );
                    }
                    value
                })
                .collect::<Vec<_>>(),
        });
        if let Some(object) = response.as_object_mut() {
            object.extend(target_meta);
        }
        Ok(response)
    }

    fn ungroup_shape(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        let session = self.sessions.get_mut(&id)?;
        let result = if is_header_footer_nested_target(&args) {
            session.core.ungroup_header_footer_shape_native(
                section,
                para,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                req_inner_control(&args)?,
            )
        } else {
            session
                .core
                .ungroup_shape_native(section, para, req_usize(&args, "control")?)
        };
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn delete_picture(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let id = req_str(&args, "session_id")?.to_string();
        let section = opt_usize(&args, "section").unwrap_or(0);
        let para = req_usize(&args, "para")?;
        let cell_path = picture_cell_path_arg(&args)?;
        let session = self.sessions.get_mut(&id)?;
        let result = if let Some(cell_path) = cell_path {
            session.core.delete_cell_picture_control_by_path_native(
                section,
                para,
                &cell_path_object_json(&cell_path),
                req_inner_control(&args)?,
            )
        } else if is_header_footer_nested_target(&args) {
            session.core.delete_header_footer_picture_control_native(
                section,
                para,
                req_usize(&args, "control")?,
                nested_para_arg(&args),
                req_inner_control(&args)?,
            )
        } else {
            session
                .core
                .delete_picture_control_native(section, para, req_usize(&args, "control")?)
        };
        if result.is_ok() {
            session.dirty = true;
        }
        core_json(result)
    }

    fn new_exam_from_ingest(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let source = require_exactly_one_key(
            &args,
            &["ingest", "ingest_json", "ingest_path"],
            "ingest source",
        )?;
        let ingest = if source == "ingest" {
            let value = args
                .get("ingest")
                .ok_or_else(|| "ingest is required".to_string())?;
            serde_json::from_value::<crate::parser::ingest::IngestDocument>(value.clone())
                .map_err(|e| format!("invalid ingest object: {e}"))?
        } else if source == "ingest_json" {
            let text = req_str(&args, "ingest_json")?;
            crate::parser::ingest::parse_ingest_str(text).map_err(|e| e.to_string())?
        } else {
            let path = req_str(&args, "ingest_path")?;
            let resolved = self.guard.resolve_existing_file(path)?;
            let bytes = fs::read(&resolved)
                .map_err(|e| format!("failed to read ingest {}: {e}", resolved.display()))?;
            crate::parser::ingest::parse_ingest_bytes(&bytes).map_err(|e| e.to_string())?
        };

        let question_count = ingest.questions.len();
        let document = crate::document_core::builders::exam_paper::build_exam_paper(&ingest);
        let core = DocumentCore::from_document(document);
        let mut result = self
            .sessions
            .new_core_session(core, FileFormat::Hwp, true)?;
        if let Some(object) = result.as_object_mut() {
            object.insert("template".to_string(), json!("exam_paper"));
            object.insert("question_count".to_string(), json!(question_count));
        }
        Ok(result)
    }

    fn new_government_report(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let source = require_exactly_one_key(
            &args,
            &["report", "report_json", "report_path"],
            "government report source",
        )?;
        let report = if source == "report" {
            let value = args
                .get("report")
                .ok_or_else(|| "report is required".to_string())?;
            crate::document_core::builders::government_report::parse_report_value(value.clone())?
        } else if source == "report_json" {
            let text = req_str(&args, "report_json")?;
            crate::document_core::builders::government_report::parse_report_str(text)?
        } else {
            let path = req_str(&args, "report_path")?;
            let resolved = self.guard.resolve_existing_file(path)?;
            let bytes = fs::read(&resolved)
                .map_err(|e| format!("failed to read report {}: {e}", resolved.display()))?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|e| format!("report file is not UTF-8: {e}"))?;
            crate::document_core::builders::government_report::parse_report_str(text)?
        };

        let row_count = report.rows.len();
        let section_count = report.sections.len();
        let table_count =
            crate::document_core::builders::government_report::report_table_count(&report);
        let has_header =
            crate::document_core::builders::government_report::has_repeating_header(&report);
        let has_page_footer =
            crate::document_core::builders::government_report::has_repeating_footer(&report);
        let title = report.title.clone();
        let core =
            crate::document_core::builders::government_report::build_government_report(&report)?;
        let mut result = self
            .sessions
            .new_core_session(core, FileFormat::Hwp, true)?;
        if let Some(object) = result.as_object_mut() {
            object.insert("template".to_string(), json!("government_report"));
            object.insert("title".to_string(), json!(title));
            object.insert("row_count".to_string(), json!(row_count));
            object.insert("section_count".to_string(), json!(section_count));
            object.insert("table_count".to_string(), json!(table_count));
            object.insert("has_header".to_string(), json!(has_header));
            object.insert("has_page_footer".to_string(), json!(has_page_footer));
        }
        Ok(result)
    }

    fn new_from_document_template(&mut self, args: Map<String, Value>) -> Result<Value, String> {
        let linked_image_policy = LinkedImagePolicy::from_args(&args)?;
        let page_flow_policy = PageFlowPolicy::from_args(&args)?;
        let source = require_exactly_one_key(
            &args,
            &["template", "template_json", "template_path"],
            "document template source",
        )?;
        let mut template = if source == "template" {
            let value = args
                .get("template")
                .ok_or_else(|| "template is required".to_string())?;
            crate::document_core::builders::document_template::parse_template_value(value.clone())?
        } else if source == "template_json" {
            let text = req_str(&args, "template_json")?;
            crate::document_core::builders::document_template::parse_template_str(text)?
        } else {
            let path = req_str(&args, "template_path")?;
            let resolved = self.guard.resolve_existing_file(path)?;
            let bytes = fs::read(&resolved)
                .map_err(|e| format!("failed to read template {}: {e}", resolved.display()))?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|e| format!("template file is not UTF-8: {e}"))?;
            crate::document_core::builders::document_template::parse_template_str(text)?
        };

        let linked_image_report =
            self.apply_linked_image_policy(&mut template, linked_image_policy)?;
        let source_page_count = template.source_page_count;
        let precompact_page_count = if matches!(page_flow_policy, PageFlowPolicy::CompactArtifacts)
            && source_page_count.is_some()
        {
            let preserved_core =
                crate::document_core::builders::document_template::build_document_template(
                    &template,
                )?;
            Some(preserved_core.page_count())
        } else {
            None
        };
        let page_flow_report = match page_flow_policy {
            PageFlowPolicy::Preserve => {
                crate::document_core::builders::document_template::TemplatePageFlowCompactReport::default()
            }
            PageFlowPolicy::CompactArtifacts => {
                let max_removals = match (precompact_page_count, source_page_count) {
                    (Some(precompact), Some(source)) if precompact > source => {
                        usize::try_from(precompact - source).unwrap_or(usize::MAX)
                    }
                    (Some(_), Some(_)) => 0,
                    _ => usize::MAX,
                };
                crate::document_core::builders::document_template::compact_page_flow_artifacts_limited(
                    &mut template,
                    max_removals,
                )
            }
        };
        let stats = crate::document_core::builders::document_template::template_stats(&template);
        let core =
            crate::document_core::builders::document_template::build_document_template(&template)?;
        let mut result = self
            .sessions
            .new_core_session(core, FileFormat::Hwp, true)?;
        if let Some(object) = result.as_object_mut() {
            object.insert("template".to_string(), json!("document_template_v1"));
            object.insert("template_stats".to_string(), json!(stats));
            object.insert("linked_image_policy".to_string(), linked_image_report);
            object.insert(
                "page_flow_policy".to_string(),
                json!({
                    "policy": page_flow_policy.as_str(),
                    "source_page_count": source_page_count,
                    "precompact_page_count": precompact_page_count,
                    "compact_report": page_flow_report,
                }),
            );
        }
        Ok(result)
    }

    fn apply_linked_image_policy(
        &self,
        template: &mut DocumentTemplate,
        policy: LinkedImagePolicy,
    ) -> Result<Value, String> {
        let mut report = LinkedImagePolicyReport::default();
        for section in &mut template.sections {
            self.apply_linked_image_policy_to_blocks(&mut section.blocks, policy, &mut report)?;
        }
        for header in &mut template.headers {
            self.apply_linked_image_policy_to_blocks(&mut header.blocks, policy, &mut report)?;
        }
        for footer in &mut template.footers {
            self.apply_linked_image_policy_to_blocks(&mut footer.blocks, policy, &mut report)?;
        }
        Ok(report.json(policy))
    }

    fn apply_linked_image_policy_to_blocks(
        &self,
        blocks: &mut [TemplateBlock],
        policy: LinkedImagePolicy,
        report: &mut LinkedImagePolicyReport,
    ) -> Result<(), String> {
        for block in blocks {
            match block {
                TemplateBlock::Picture {
                    image_base64,
                    external_path,
                    extension,
                    ..
                } => {
                    self.apply_linked_image_policy_to_fields(
                        image_base64,
                        external_path,
                        extension,
                        policy,
                        report,
                    )?;
                }
                TemplateBlock::Table { cell_blocks, .. } => {
                    for row in cell_blocks {
                        for cell in row {
                            self.apply_linked_image_policy_to_blocks(cell, policy, report)?;
                        }
                    }
                }
                TemplateBlock::ObjectPlaceholder {
                    drawing_style,
                    children,
                    ..
                } => {
                    if let Some(style) = drawing_style {
                        self.apply_linked_image_policy_to_drawing_style(style, policy, report)?;
                    }
                    self.apply_linked_image_policy_to_blocks(children, policy, report)?;
                }
                TemplateBlock::Paragraph { .. } | TemplateBlock::Equation { .. } => {}
            }
        }
        Ok(())
    }

    fn apply_linked_image_policy_to_drawing_style(
        &self,
        drawing_style: &mut Value,
        policy: LinkedImagePolicy,
        report: &mut LinkedImagePolicyReport,
    ) -> Result<(), String> {
        let Some(image) = drawing_style
            .get_mut("fill")
            .and_then(Value::as_object_mut)
            .and_then(|fill| fill.get_mut("image"))
        else {
            return Ok(());
        };
        let Some(image) = image.as_object_mut() else {
            return Ok(());
        };

        let mut image_base64 = image
            .get("image_base64")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut external_path = image
            .get("external_path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut extension = image
            .get("extension")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        self.apply_linked_image_policy_to_fields(
            &mut image_base64,
            &mut external_path,
            &mut extension,
            policy,
            report,
        )?;

        image.insert("image_base64".to_string(), json!(image_base64));
        image.insert("external_path".to_string(), json!(external_path));
        if !extension.trim().is_empty() {
            image.insert("extension".to_string(), json!(extension));
        }
        Ok(())
    }

    fn apply_linked_image_policy_to_fields(
        &self,
        image_base64: &mut String,
        external_path: &mut String,
        extension: &mut String,
        policy: LinkedImagePolicy,
        report: &mut LinkedImagePolicyReport,
    ) -> Result<(), String> {
        let path = external_path.trim().to_string();
        if !image_base64.trim().is_empty() || path.is_empty() {
            return Ok(());
        }
        if matches!(policy, LinkedImagePolicy::PreserveLinks) {
            report.preserved += 1;
            return Ok(());
        }

        let resolved = self.guard.resolve_existing_file(&path);
        match resolved {
            Ok(resolved) => {
                let bytes = fs::read(&resolved).map_err(|e| {
                    format!("failed to read linked image {}: {e}", resolved.display())
                })?;
                if bytes.is_empty() {
                    let err = format!("linked image is empty: {}", resolved.display());
                    if matches!(policy, LinkedImagePolicy::EmbedRequired) {
                        return Err(err);
                    }
                    report.errors.push(err);
                    report.missing += 1;
                    report.preserved += 1;
                    return Ok(());
                }
                *image_base64 = BASE64.encode(bytes);
                if extension.trim().is_empty() {
                    if let Some(path_extension) = extension_from_path(&path) {
                        *extension = path_extension;
                    }
                }
                external_path.clear();
                report.embedded += 1;
                Ok(())
            }
            Err(err) => {
                let err = format!("linked image not embedded from {path}: {err}");
                if matches!(policy, LinkedImagePolicy::EmbedRequired) {
                    return Err(err);
                }
                report.errors.push(err);
                report.missing += 1;
                report.preserved += 1;
                Ok(())
            }
        }
    }
}

fn revision_info(core: &DocumentCore, source_format: &str) -> Value {
    let header = &core.document.header;
    let doc_info = &core.document.doc_info;
    let tail = doc_info.hwpx_head_tail.as_deref();
    let hwpx_provenance = hwpx_package_provenance(core);
    let tail_info = tail.map(parse_hwpx_head_tail_info).unwrap_or_else(|| {
        json!({
            "present": false,
            "byte_len": 0,
            "compatible_document": { "present": false },
            "doc_option": { "present": false, "linkinfo": { "present": false } },
            "track_change_config": { "present": false }
        })
    });

    let hwp5_track_records: Vec<Value> = doc_info
        .extra_records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.tag_id == tags::HWPTAG_TRACKCHANGE)
        .map(|(extra_record_index, record)| {
            let flags = record
                .data
                .get(0..4)
                .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
            json!({
                "extra_record_index": extra_record_index,
                "tag_id": record.tag_id,
                "tag_name": tags::tag_name(record.tag_id),
                "flags": flags,
                "byte_len": record.data.len(),
                "head_hex": bytes_head_hex(&record.data, 32),
                "hash": format!("blake3:{}", blake3::hash(&record.data).to_hex()),
            })
        })
        .collect();
    let hwp5_track_record = doc_info
        .extra_records
        .iter()
        .find(|record| record.tag_id == tags::HWPTAG_TRACKCHANGE);
    let hwp5_track_flags = hwp5_track_record
        .and_then(|record| record.data.get(0..4))
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
    let hwp5_track_head_hex = hwp5_track_record.map(|record| bytes_head_hex(&record.data, 32));
    let hwp5_track_hash =
        hwp5_track_record.map(|record| format!("blake3:{}", blake3::hash(&record.data).to_hex()));
    let hwpx_track_change_entries: Vec<Value> = core
        .document
        .hwpx_aux_entries
        .iter()
        .filter(|(path, _)| hwpx_aux_entry_kind(path) == "track_change")
        .map(|(path, data)| {
            json!({
                "path": path,
                "byte_len": data.len(),
                "hash": format!("blake3:{}", blake3::hash(data).to_hex()),
                "xml": hwpx_aux_xml_info(data),
            })
        })
        .collect();
    let hwpx_history_entries: Vec<Value> = core
        .document
        .hwpx_aux_entries
        .iter()
        .filter(|(path, _)| hwpx_aux_entry_kind(path) == "history")
        .map(|(path, data)| {
            json!({
                "path": path,
                "byte_len": data.len(),
                "hash": format!("blake3:{}", blake3::hash(data).to_hex()),
                "xml": hwpx_aux_xml_info(data),
            })
        })
        .collect();
    let inline_track_changes =
        inline_hwpx_track_changes_info(doc_info.hwpx_ref_list_track_change_xml.as_deref());
    let body_track_changes = hwpx_body_track_changes_info(core);

    json!({
        "source_format": source_format,
        "hwp_version": format!(
            "{}.{}.{}.{}",
            header.version.major,
            header.version.minor,
            header.version.build,
            header.version.revision
        ),
        "file_revision": header.version.revision,
        "hwpml_version": doc_info.hwpml_version.clone(),
        "hwpx_provenance": hwpx_provenance,
        "hwpx_head_tail": tail_info,
        "track_change": {
            "config_present": tail
                .map(|text| text.contains("trackchageConfig") || text.contains("trackchangeConfig"))
                .unwrap_or(false),
            "config_flags": tail
                .and_then(parse_hwpx_track_change_flags),
            "hwp5_record_present": hwp5_track_record.is_some(),
            "hwp5_record_flags": hwp5_track_flags,
            "hwp5_record_len": hwp5_track_record.map(|record| record.data.len()),
            "hwp5_record_head_hex": hwp5_track_head_hex,
            "hwp5_record_hash": hwp5_track_hash,
            "hwp5_record_count": hwp5_track_records.len(),
            "hwp5_records": hwp5_track_records,
            "hwpx_track_change_entry_count": hwpx_track_change_entries.len(),
            "hwpx_history_entry_count": hwpx_history_entries.len(),
            "hwpx_inline_track_change_present": inline_track_changes["present"],
            "hwpx_inline_track_change_count": inline_track_changes["track_change_count"],
            "hwpx_inline_track_change_author_count": inline_track_changes["author_count"],
            "hwpx_inline_track_change_hash": inline_track_changes["hash"],
            "hwpx_body_track_change_marker_count": body_track_changes["marker_count"],
            "hwpx_body_track_change_tag_counts": body_track_changes["tag_counts"],
        },
        "hwpx_inline_track_changes": inline_track_changes,
        "hwpx_body_track_changes": body_track_changes,
        "hwpx_auxiliary": {
            "track_change_entry_count": hwpx_track_change_entries.len(),
            "history_entry_count": hwpx_history_entries.len(),
            "track_change_entries": hwpx_track_change_entries,
            "history_entries": hwpx_history_entries,
        },
    })
}

#[derive(Default)]
struct HwpxBodyTrackChangeSummary {
    marker_count: usize,
    tag_counts: BTreeMap<String, usize>,
    entries: Vec<Value>,
}

fn hwpx_body_track_changes_info(core: &DocumentCore) -> Value {
    let mut summary = HwpxBodyTrackChangeSummary::default();
    for (section_idx, section) in core.document.sections.iter().enumerate() {
        collect_hwpx_body_track_changes_from_paragraphs(
            &section.paragraphs,
            section_idx,
            "body",
            &format!("section[{section_idx}]"),
            &mut summary,
        );
    }
    json!({
        "present": summary.marker_count > 0,
        "marker_count": summary.marker_count,
        "tag_counts": summary.tag_counts,
        "entries": summary.entries,
        "entries_truncated": summary.marker_count > summary.entries.len(),
    })
}

fn collect_hwpx_body_track_changes_from_paragraphs(
    paragraphs: &[Paragraph],
    section_idx: usize,
    scope: &str,
    path_prefix: &str,
    summary: &mut HwpxBodyTrackChangeSummary,
) {
    for (para_idx, para) in paragraphs.iter().enumerate() {
        let para_path = format!("{path_prefix}.para[{para_idx}]");
        for marker in &para.hwpx_text_markers {
            push_hwpx_body_track_marker(summary, section_idx, scope, &para_path, marker);
        }
        for (control_idx, control) in para.controls.iter().enumerate() {
            let control_path = format!("{para_path}.control[{control_idx}]");
            collect_hwpx_body_track_changes_from_control(
                control,
                section_idx,
                &control_path,
                summary,
            );
        }
    }
}

fn collect_hwpx_body_track_changes_from_control(
    control: &Control,
    section_idx: usize,
    path_prefix: &str,
    summary: &mut HwpxBodyTrackChangeSummary,
) {
    match control {
        Control::Table(table) => {
            for (cell_idx, cell) in table.cells.iter().enumerate() {
                collect_hwpx_body_track_changes_from_paragraphs(
                    &cell.paragraphs,
                    section_idx,
                    "table_cell",
                    &format!("{path_prefix}.cell[{cell_idx}]"),
                    summary,
                );
            }
            if let Some(caption) = &table.caption {
                collect_hwpx_body_track_changes_from_paragraphs(
                    &caption.paragraphs,
                    section_idx,
                    "table_caption",
                    &format!("{path_prefix}.caption"),
                    summary,
                );
            }
        }
        Control::Picture(picture) => {
            if let Some(caption) = &picture.caption {
                collect_hwpx_body_track_changes_from_paragraphs(
                    &caption.paragraphs,
                    section_idx,
                    "picture_caption",
                    &format!("{path_prefix}.caption"),
                    summary,
                );
            }
        }
        Control::Shape(shape) => {
            collect_hwpx_body_track_changes_from_shape(shape, section_idx, path_prefix, summary);
        }
        Control::Header(header) => {
            collect_hwpx_body_track_changes_from_paragraphs(
                &header.paragraphs,
                section_idx,
                "header",
                path_prefix,
                summary,
            );
        }
        Control::Footer(footer) => {
            collect_hwpx_body_track_changes_from_paragraphs(
                &footer.paragraphs,
                section_idx,
                "footer",
                path_prefix,
                summary,
            );
        }
        Control::Footnote(note) => {
            collect_hwpx_body_track_changes_from_paragraphs(
                &note.paragraphs,
                section_idx,
                "footnote",
                path_prefix,
                summary,
            );
        }
        Control::Endnote(note) => {
            collect_hwpx_body_track_changes_from_paragraphs(
                &note.paragraphs,
                section_idx,
                "endnote",
                path_prefix,
                summary,
            );
        }
        Control::HiddenComment(comment) => {
            collect_hwpx_body_track_changes_from_paragraphs(
                &comment.paragraphs,
                section_idx,
                "hidden_comment",
                path_prefix,
                summary,
            );
        }
        _ => {}
    }
}

fn collect_hwpx_body_track_changes_from_shape(
    shape: &ShapeObject,
    section_idx: usize,
    path_prefix: &str,
    summary: &mut HwpxBodyTrackChangeSummary,
) {
    if let ShapeObject::Group(group) = shape {
        for (child_idx, child) in group.children.iter().enumerate() {
            collect_hwpx_body_track_changes_from_shape(
                child,
                section_idx,
                &format!("{path_prefix}.child[{child_idx}]"),
                summary,
            );
        }
    }
    if let Some(drawing) = shape.drawing() {
        if let Some(text_box) = &drawing.text_box {
            collect_hwpx_body_track_changes_from_paragraphs(
                &text_box.paragraphs,
                section_idx,
                "shape_text_box",
                &format!("{path_prefix}.text_box"),
                summary,
            );
        }
        if let Some(caption) = &drawing.caption {
            collect_hwpx_body_track_changes_from_paragraphs(
                &caption.paragraphs,
                section_idx,
                "shape_caption",
                &format!("{path_prefix}.caption"),
                summary,
            );
        }
    }
}

fn push_hwpx_body_track_marker(
    summary: &mut HwpxBodyTrackChangeSummary,
    section_idx: usize,
    scope: &str,
    paragraph_path: &str,
    marker: &crate::model::paragraph::HwpxTextMarker,
) {
    let tag = hwpx_body_track_marker_tag(&marker.raw_xml);
    summary.marker_count += 1;
    *summary.tag_counts.entry(tag.to_string()).or_default() += 1;
    if summary.entries.len() >= 128 {
        return;
    }
    summary.entries.push(json!({
        "section": section_idx,
        "scope": scope,
        "paragraph_path": paragraph_path,
        "char_idx": marker.char_idx,
        "tag": tag,
        "id": xml_attr_from_raw(&marker.raw_xml, b"Id"),
        "tc_id": xml_attr_from_raw(&marker.raw_xml, b"TcId"),
        "paraend": xml_attr_from_raw(&marker.raw_xml, b"paraend"),
        "raw_xml": marker.raw_xml,
    }));
}

fn hwpx_body_track_marker_tag(raw_xml: &str) -> &'static str {
    if raw_xml.contains("insertBegin") {
        "insertBegin"
    } else if raw_xml.contains("insertEnd") {
        "insertEnd"
    } else if raw_xml.contains("deleteBegin") {
        "deleteBegin"
    } else if raw_xml.contains("deleteEnd") {
        "deleteEnd"
    } else {
        "unknown"
    }
}

fn xml_attr_from_raw(raw_xml: &str, key: &[u8]) -> Option<String> {
    let mut reader = XmlReader::from_str(raw_xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) | Ok(XmlEvent::Empty(ref e)) => {
                return xml_attr(e, key);
            }
            Ok(XmlEvent::Eof) | Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

fn inline_hwpx_track_changes_info(raw: Option<&str>) -> Value {
    let Some(raw) = raw.filter(|text| !text.trim().is_empty()) else {
        return json!({
            "present": false,
            "byte_len": 0,
            "hash": null,
            "track_changes_item_count": null,
            "track_change_count": 0,
            "track_change_author_item_count": null,
            "author_count": 0,
            "entries": [],
            "authors": [],
        });
    };

    let mut reader = XmlReader::from_str(raw);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut item_count = None;
    let mut author_item_count = None;
    let mut entries = Vec::new();
    let mut authors = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) | Ok(XmlEvent::Empty(ref e)) => {
                match xml_local_name(e.name().as_ref()) {
                    b"trackChanges" => {
                        item_count =
                            xml_attr(e, b"itemCnt").and_then(|value| value.parse::<u32>().ok());
                    }
                    b"trackChange" => {
                        entries.push(json!({
                            "id": xml_attr(e, b"id"),
                            "type": xml_attr(e, b"type"),
                            "date": xml_attr(e, b"date"),
                            "author_id": xml_attr(e, b"authorID"),
                            "hide": xml_attr(e, b"hide"),
                            "char_shape_id": xml_attr(e, b"charshapeID"),
                        }));
                    }
                    b"trackChangeAuthors" => {
                        author_item_count =
                            xml_attr(e, b"itemCnt").and_then(|value| value.parse::<u32>().ok());
                    }
                    b"trackChangeAuthor" => {
                        authors.push(json!({
                            "id": xml_attr(e, b"id"),
                            "name": xml_attr(e, b"name"),
                            "mark": xml_attr(e, b"mark"),
                        }));
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    json!({
        "present": true,
        "byte_len": raw.len(),
        "hash": format!("blake3:{}", blake3::hash(raw.as_bytes()).to_hex()),
        "track_changes_item_count": item_count,
        "track_change_count": entries.len(),
        "track_change_author_item_count": author_item_count,
        "author_count": authors.len(),
        "entries": entries,
        "authors": authors,
    })
}

fn hwpx_aux_xml_info(data: &[u8]) -> Value {
    let Ok(text) = std::str::from_utf8(data) else {
        return json!({
            "present": false,
            "parse_error": "entry is not UTF-8 XML",
            "root": null,
            "element_count": 0,
            "element_counts": {},
            "track_change_count": 0,
            "revision_count": 0,
            "history_item_count": 0,
            "ids": [],
            "targets": [],
            "types": [],
            "authors": [],
            "sections": [],
            "paras": [],
            "entries_truncated": false,
        });
    };
    let mut reader = XmlReader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut root: Option<Value> = None;
    let mut element_count = 0usize;
    let mut element_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut track_change_count = 0usize;
    let mut revision_count = 0usize;
    let mut history_item_count = 0usize;
    let mut ids = Vec::new();
    let mut targets = Vec::new();
    let mut types = Vec::new();
    let mut authors = Vec::new();
    let mut sections = Vec::new();
    let mut paras = Vec::new();
    let mut parse_error: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) | Ok(XmlEvent::Empty(ref e)) => {
                let local = String::from_utf8_lossy(xml_local_name(e.name().as_ref())).to_string();
                if root.is_none() {
                    root = Some(json!({
                        "name": String::from_utf8_lossy(e.name().as_ref()).to_string(),
                        "local_name": local,
                    }));
                }
                element_count += 1;
                *element_counts.entry(local.clone()).or_default() += 1;
                match local.as_str() {
                    "change" | "trackChange" => track_change_count += 1,
                    "revision" => revision_count += 1,
                    "item" => history_item_count += 1,
                    _ => {}
                }
                if ids.len() < 64 {
                    if let Some(id) = xml_attr(e, b"id") {
                        ids.push(id);
                    }
                }
                if targets.len() < 64 {
                    if let Some(target) = xml_attr(e, b"target") {
                        targets.push(target);
                    }
                }
                if types.len() < 64 {
                    if let Some(value) = xml_attr(e, b"type") {
                        types.push(value);
                    }
                }
                if authors.len() < 64 {
                    if let Some(value) = xml_attr(e, b"author") {
                        authors.push(value);
                    } else if let Some(value) = xml_attr(e, b"authorID") {
                        authors.push(value);
                    }
                }
                if sections.len() < 64 {
                    if let Some(value) = xml_attr(e, b"section") {
                        sections.push(value);
                    }
                }
                if paras.len() < 64 {
                    if let Some(value) = xml_attr(e, b"para") {
                        paras.push(value);
                    }
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(error) => {
                parse_error = Some(error.to_string());
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    json!({
        "present": root.is_some(),
        "parse_error": parse_error,
        "root": root,
        "element_count": element_count,
        "element_counts": element_counts,
        "track_change_count": track_change_count,
        "revision_count": revision_count,
        "history_item_count": history_item_count,
        "ids": ids,
        "targets": targets,
        "types": types,
        "authors": authors,
        "sections": sections,
        "paras": paras,
        "entries_truncated": ids.len() >= 64
            || targets.len() >= 64
            || types.len() >= 64
            || authors.len() >= 64
            || sections.len() >= 64
            || paras.len() >= 64,
    })
}

fn bytes_head_hex(bytes: &[u8], max_len: usize) -> String {
    bytes
        .iter()
        .take(max_len)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn hwpx_package_info(core: &DocumentCore, source_format: &str) -> Value {
    let entries: Vec<Value> = core
        .document
        .hwpx_aux_entries
        .iter()
        .map(|(path, data)| {
            json!({
                "path": path,
                "byte_len": data.len(),
                "kind": hwpx_aux_entry_kind(path),
                "passthrough": is_passthrough_hwpx_aux_path(path),
            })
        })
        .collect();
    let passthrough_count = entries
        .iter()
        .filter(|entry| entry["passthrough"].as_bool().unwrap_or(false))
        .count();
    let history_count = entries
        .iter()
        .filter(|entry| entry["kind"] == "history")
        .count();
    let track_change_count = entries
        .iter()
        .filter(|entry| entry["kind"] == "track_change")
        .count();

    json!({
        "source_format": source_format,
        "hwpx_provenance": hwpx_package_provenance(core),
        "aux_entry_count": entries.len(),
        "passthrough_entry_count": passthrough_count,
        "history_entry_count": history_count,
        "track_change_entry_count": track_change_count,
        "entries": entries,
    })
}

fn hwpx_package_provenance(core: &DocumentCore) -> Value {
    let version = core.document.hwpx_aux_entry("version.xml");
    let content_hpf = core.document.hwpx_aux_entry("Contents/content.hpf");
    json!({
        "version": parse_hwpx_version_entry(version),
        "metadata": parse_hwpx_content_metadata_entry(content_hpf),
    })
}

fn parse_hwpx_version_entry(data: Option<&[u8]>) -> Value {
    let Some(data) = data else {
        return json!({
            "present": false,
            "parse_error": null,
            "byte_len": 0,
            "hash": null,
            "application": null,
            "appVersion": null,
            "xmlVersion": null,
            "major": null,
            "minor": null,
            "micro": null,
            "buildNumber": null,
            "os": null,
            "targetApplication": null,
        });
    };
    let Ok(text) = std::str::from_utf8(data) else {
        return json!({
            "present": true,
            "parse_error": "version.xml is not UTF-8 XML",
            "byte_len": data.len(),
            "hash": format!("blake3:{}", blake3::hash(data).to_hex()),
            "application": null,
            "appVersion": null,
            "xmlVersion": null,
            "major": null,
            "minor": null,
            "micro": null,
            "buildNumber": null,
            "os": null,
            "targetApplication": null,
        });
    };

    let mut reader = XmlReader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut parse_error: Option<String> = None;
    let mut attrs = BTreeMap::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref event)) | Ok(XmlEvent::Empty(ref event)) => {
                if xml_local_name(event.name().as_ref()) == b"HCFVersion" {
                    for key in [
                        b"application".as_slice(),
                        b"appVersion".as_slice(),
                        b"xmlVersion".as_slice(),
                        b"major".as_slice(),
                        b"minor".as_slice(),
                        b"micro".as_slice(),
                        b"buildNumber".as_slice(),
                        b"os".as_slice(),
                    ] {
                        if let Some(value) = xml_attr(event, key) {
                            attrs.insert(String::from_utf8_lossy(key).to_string(), value);
                        }
                    }
                    let target = xml_attr(event, b"tagetApplication")
                        .or_else(|| xml_attr(event, b"targetApplication"));
                    if let Some(value) = target {
                        attrs.insert("targetApplication".to_string(), value);
                    }
                    break;
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(error) => {
                parse_error = Some(error.to_string());
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    json!({
        "present": !attrs.is_empty(),
        "parse_error": parse_error,
        "byte_len": data.len(),
        "hash": format!("blake3:{}", blake3::hash(data).to_hex()),
        "application": attrs.get("application"),
        "appVersion": attrs.get("appVersion"),
        "xmlVersion": attrs.get("xmlVersion"),
        "major": attrs.get("major"),
        "minor": attrs.get("minor"),
        "micro": attrs.get("micro"),
        "buildNumber": attrs.get("buildNumber"),
        "os": attrs.get("os"),
        "targetApplication": attrs.get("targetApplication"),
    })
}

fn parse_hwpx_content_metadata_entry(data: Option<&[u8]>) -> Value {
    let Some(data) = data else {
        return json!({
            "present": false,
            "parse_error": null,
            "byte_len": 0,
            "hash": null,
            "title": null,
            "language": null,
            "creator": null,
            "lastsaveby": null,
            "createdDate": null,
            "modifiedDate": null,
            "date": null,
            "entries": {},
        });
    };
    let Ok(text) = std::str::from_utf8(data) else {
        return json!({
            "present": true,
            "parse_error": "Contents/content.hpf is not UTF-8 XML",
            "byte_len": data.len(),
            "hash": format!("blake3:{}", blake3::hash(data).to_hex()),
            "title": null,
            "language": null,
            "creator": null,
            "lastsaveby": null,
            "createdDate": null,
            "modifiedDate": null,
            "date": null,
            "entries": {},
        });
    };

    let mut reader = XmlReader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_metadata = false;
    let mut current_key: Option<String> = None;
    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    let mut parse_error: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref event)) => match xml_local_name(event.name().as_ref()) {
                b"metadata" => in_metadata = true,
                b"meta" if in_metadata => {
                    current_key = xml_attr(event, b"name");
                    if let Some(key) = &current_key {
                        entries.entry(key.clone()).or_default();
                    }
                }
                b"title" if in_metadata => {
                    current_key = Some("title".to_string());
                    entries.entry("title".to_string()).or_default();
                }
                b"language" if in_metadata => {
                    current_key = Some("language".to_string());
                    entries.entry("language".to_string()).or_default();
                }
                _ => {}
            },
            Ok(XmlEvent::Empty(ref event)) => {
                if in_metadata && xml_local_name(event.name().as_ref()) == b"meta" {
                    if let Some(key) = xml_attr(event, b"name") {
                        entries.entry(key).or_default();
                    }
                }
            }
            Ok(XmlEvent::Text(ref event)) => {
                if in_metadata {
                    if let Some(key) = &current_key {
                        let decoded = event.decode().unwrap_or_default();
                        entries
                            .entry(key.clone())
                            .and_modify(|value| value.push_str(decoded.as_ref()))
                            .or_insert_with(|| decoded.into_owned());
                    }
                }
            }
            Ok(XmlEvent::CData(ref event)) => {
                if in_metadata {
                    if let Some(key) = &current_key {
                        let decoded = String::from_utf8_lossy(event.as_ref());
                        entries
                            .entry(key.clone())
                            .and_modify(|value| value.push_str(decoded.as_ref()))
                            .or_insert_with(|| decoded.into_owned());
                    }
                }
            }
            Ok(XmlEvent::End(ref event)) => match xml_local_name(event.name().as_ref()) {
                b"metadata" => {
                    in_metadata = false;
                    current_key = None;
                }
                b"meta" | b"title" | b"language" => current_key = None,
                _ => {}
            },
            Ok(XmlEvent::Eof) => break,
            Err(error) => {
                parse_error = Some(error.to_string());
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    json!({
        "present": !entries.is_empty(),
        "parse_error": parse_error,
        "byte_len": data.len(),
        "hash": format!("blake3:{}", blake3::hash(data).to_hex()),
        "title": entries.get("title"),
        "language": entries.get("language"),
        "creator": entries.get("creator"),
        "lastsaveby": entries.get("lastsaveby"),
        "createdDate": entries.get("CreatedDate"),
        "modifiedDate": entries.get("ModifiedDate"),
        "date": entries.get("date"),
        "entries": entries,
    })
}

fn get_hwpx_package_entry(core: &DocumentCore, args: &Map<String, Value>) -> Result<Value, String> {
    let path = req_str(args, "path")?;
    let include_text = opt_bool(args, "include_text")
        .or_else(|| opt_bool(args, "includeText"))
        .unwrap_or(true);
    let max_text_bytes = opt_usize(args, "max_text_bytes")
        .or_else(|| opt_usize(args, "maxTextBytes"))
        .unwrap_or(64 * 1024);
    let (_, data) = core
        .document
        .hwpx_aux_entries
        .iter()
        .find(|(entry_path, _)| entry_path == path)
        .ok_or_else(|| format!("HWPX package auxiliary entry not found: {path}"))?;

    let mut result = json!({
        "path": path,
        "byte_len": data.len(),
        "kind": hwpx_aux_entry_kind(path),
        "passthrough": is_passthrough_hwpx_aux_path(path),
        "base64": BASE64.encode(data),
    });

    if include_text {
        if let Ok(text) = std::str::from_utf8(data) {
            let limit = max_text_bytes.min(text.len());
            let safe_limit = safe_utf8_prefix_len(text, limit);
            if let Some(object) = result.as_object_mut() {
                object.insert("text".to_string(), json!(&text[..safe_limit]));
                object.insert("text_truncated".to_string(), json!(safe_limit < text.len()));
                object.insert("text_byte_len".to_string(), json!(text.len()));
            }
        } else if let Some(object) = result.as_object_mut() {
            object.insert("text".to_string(), Value::Null);
            object.insert("text_truncated".to_string(), json!(false));
            object.insert("text_byte_len".to_string(), Value::Null);
        }
    }

    Ok(result)
}

fn safe_utf8_prefix_len(text: &str, limit: usize) -> usize {
    let mut end = limit.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn hwpx_aux_entry_kind(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower == "contents/content.hpf" {
        "content_manifest"
    } else if lower == "version.xml" {
        "version"
    } else if lower == "settings.xml" {
        "settings"
    } else if lower.starts_with("preview/") {
        "preview"
    } else if lower.starts_with("history/")
        || lower.starts_with("histories/")
        || lower.starts_with("contents/history/")
        || lower.starts_with("contents/histories/")
    {
        "history"
    } else if lower.starts_with("trackchange/")
        || lower.starts_with("trackchanges/")
        || lower.starts_with("contents/trackchange/")
        || lower.starts_with("contents/trackchanges/")
        || lower.starts_with("revision/")
        || lower.starts_with("revisions/")
        || lower.starts_with("contents/revision/")
        || lower.starts_with("contents/revisions/")
    {
        "track_change"
    } else {
        "other"
    }
}

fn is_passthrough_hwpx_aux_path(path: &str) -> bool {
    matches!(hwpx_aux_entry_kind(path), "history" | "track_change")
}

fn parse_hwpx_head_tail_info(tail: &str) -> Value {
    let mut reader = XmlReader::from_str(tail);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut compatible_present = false;
    let mut compatible_target: Option<String> = None;
    let mut doc_option_present = false;
    let mut linkinfo_present = false;
    let mut linkinfo_path: Option<String> = None;
    let mut page_inherit: Option<bool> = None;
    let mut footnote_inherit: Option<bool> = None;
    let mut track_change_present = false;
    let mut track_change_flags: Option<u32> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) | Ok(XmlEvent::Empty(ref e)) => {
                match xml_local_name(e.name().as_ref()) {
                    b"compatibleDocument" => {
                        compatible_present = true;
                        compatible_target = xml_attr(e, b"targetProgram");
                    }
                    b"docOption" => {
                        doc_option_present = true;
                    }
                    b"linkinfo" => {
                        linkinfo_present = true;
                        linkinfo_path = xml_attr(e, b"path");
                        page_inherit = xml_attr(e, b"pageInherit").and_then(|v| parse_xml_bool(&v));
                        footnote_inherit =
                            xml_attr(e, b"footnoteInherit").and_then(|v| parse_xml_bool(&v));
                    }
                    b"trackchageConfig" | b"trackchangeConfig" => {
                        track_change_present = true;
                        track_change_flags = xml_attr(e, b"flags").and_then(|v| v.parse().ok());
                    }
                    _ => {}
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    json!({
        "present": true,
        "byte_len": tail.len(),
        "compatible_document": {
            "present": compatible_present,
            "target_program": compatible_target,
        },
        "doc_option": {
            "present": doc_option_present,
            "linkinfo": {
                "present": linkinfo_present,
                "path": linkinfo_path,
                "page_inherit": page_inherit,
                "footnote_inherit": footnote_inherit,
            },
        },
        "track_change_config": {
            "present": track_change_present,
            "flags": track_change_flags,
        },
    })
}

fn parse_hwpx_track_change_flags(tail: &str) -> Option<u32> {
    let mut reader = XmlReader::from_str(tail);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref e)) | Ok(XmlEvent::Empty(ref e)) => {
                if matches!(
                    xml_local_name(e.name().as_ref()),
                    b"trackchageConfig" | b"trackchangeConfig"
                ) {
                    return xml_attr(e, b"flags").and_then(|v| v.parse().ok());
                }
            }
            Ok(XmlEvent::Eof) | Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

fn xml_local_name(name: &[u8]) -> &[u8] {
    name.iter()
        .rposition(|byte| *byte == b':')
        .map(|idx| &name[idx + 1..])
        .unwrap_or(name)
}

fn xml_attr(e: &quick_xml::events::BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        if xml_local_name(attr.key.as_ref()) == key {
            Some(String::from_utf8_lossy(attr.value.as_ref()).to_string())
        } else {
            None
        }
    })
}

fn parse_xml_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "TRUE" => Some(true),
        "0" | "false" | "FALSE" => Some(false),
        _ => None,
    }
}

fn description(name: &str) -> &'static str {
    match name {
        "rhwp_open" => "Open a HWP/HWPX file into an in-memory rhwp session.",
        "rhwp_new" => "Create a new blank HWP session.",
        "rhwp_new_exam_from_ingest" => "Create a new exam-paper session from ingest_schema_v1 JSON.",
        "rhwp_new_government_report" => {
            "Create a deterministic government/business report session from JSON."
        }
        "rhwp_new_from_document_template" => {
            "Create a new session from rhwp document-template v1 JSON."
        }
        "rhwp_document_profile" => {
            "Summarize document structure for complex-form comparison: pages, paragraphs, controls, tables, pictures, notes, and header/footer scopes."
        }
        "rhwp_style_signature" => {
            "Summarize style-list, template-style, and paragraph/table style-reference signatures for preservation checks."
        }
        "rhwp_revision_info" => {
            "Inspect HWP/HWPX revision, HWPML version, HWPX inline trackChanges header XML, HWPX body insert/delete track-change markers, HWPX TrackChange/Revisions/History auxiliary metadata, and HWP5 hwp5_record_count/hwp5_records diagnostics."
        }
        "rhwp_hwpx_package_info" => {
            "Inspect preserved HWPX package auxiliary entries such as history or track-change XML."
        }
        "rhwp_get_hwpx_package_entry" => {
            "Read a preserved HWPX package auxiliary entry as base64 and optional UTF-8 text."
        }
        "rhwp_save" => "Save a session to HWP or HWPX with atomic write.",
        "rhwp_export_bytes" => "Export a session as base64 HWP/HWPX bytes.",
        "rhwp_extract_text" => "Extract text from one page or the whole document.",
        "rhwp_extract_markdown" => "Extract markdown from one page or the whole document.",
        "rhwp_extract_document_template" => {
            "Extract rhwp document-template v1 JSON for structural regeneration, optionally preserving empty paragraphs for closer layout reproduction."
        }
        "rhwp_render_svg" => "Render a page to SVG with layout overflow diagnostics.",
        "rhwp_render_png" => {
            "Render a page to PNG base64 through the SVG/resvg compatibility raster path with layout overflow diagnostics."
        }
        "rhwp_preview_page" => {
            "Build an integrated preview payload for one page: page metadata, SVG/HTML preview layer, text/markdown, optional PNG, and layout overflow diagnostics."
        }
        "rhwp_search" => "Search document text.",
        "rhwp_list_controls" => {
            "List document controls with edit targets for tables, pictures, equations, notes, fields, bookmarks, headers/footers, OLE/chart objects, and hidden comments."
        }
        "rhwp_compare_text" => {
            "Compare extracted text for a session against another session or file."
        }
        "rhwp_compare_document_profile" => {
            "Compare structural document profiles for a session against another session or file."
        }
        "rhwp_compare_style_signature" => {
            "Compare style-list, template-style, and style-reference signatures for a session against another session or file."
        }
        "rhwp_compare_fidelity_summary" => {
            "Compare text, document profile, style signature, render geometry, optional PNG pixels, and optional HWPX package drift in one fidelity summary."
        }
        "rhwp_compare_render_geometry" => {
            "Compare render-tree geometry for a session against another session or file."
        }
        "rhwp_compare_render_png" => {
            "Render two pages to PNG through the SVG/resvg path and return pixel-diff metrics."
        }
        "rhwp_match_render_pages" => {
            "Find the closest rendered pages in another session or file for a source page."
        }
        "rhwp_compare_hwp_records" => {
            "Compare HWP5 record inventories for a session against another session or file."
        }
        "rhwp_compare_hwpx_package" => {
            "Compare HWPX ZIP package entries for a session against another session or file."
        }
        "rhwp_get_note_info" => "Get footnote or endnote paragraph text and metadata.",
        "rhwp_insert_note_text" => "Insert text into an existing footnote or endnote paragraph.",
        "rhwp_delete_note_text" => "Delete text from an existing footnote or endnote paragraph.",
        "rhwp_split_note_paragraph" => "Split a footnote or endnote paragraph.",
        "rhwp_merge_note_paragraph" => "Merge a footnote or endnote paragraph with the previous one.",
        "rhwp_apply_note_char_format" => "Apply character formatting to a footnote or endnote paragraph range.",
        "rhwp_apply_note_para_format" => "Apply paragraph formatting to a footnote or endnote paragraph.",
        "rhwp_insert_hidden_comment" => "Insert a hidden comment control with one comment paragraph.",
        "rhwp_get_hidden_comment" => "Get hidden comment paragraph text and metadata.",
        "rhwp_insert_hidden_comment_text" => "Insert text into an existing hidden comment paragraph.",
        "rhwp_delete_hidden_comment_text" => "Delete text from an existing hidden comment paragraph.",
        "rhwp_split_hidden_comment_paragraph" => "Split a hidden comment paragraph.",
        "rhwp_merge_hidden_comment_paragraph" => {
            "Merge a hidden comment paragraph with the previous one."
        }
        "rhwp_apply_hidden_comment_char_format" => {
            "Apply character formatting to a hidden comment paragraph range."
        }
        "rhwp_apply_hidden_comment_para_format" => {
            "Apply paragraph formatting to a hidden comment paragraph."
        }
        "rhwp_get_picture_properties" => {
            "Get picture properties for a body, table-cell, or header/footer picture, including captionText, HWPX shadow/glow/softEdge/reflection/threeD/blur/fillOverlay effects, and preserved raw effect XML fragments."
        }
        "rhwp_set_picture_properties" => {
            "Set picture properties for a body, table-cell, or header/footer picture, including captionText, HWPX shadow/glow/softEdge/reflection/threeD/blur/fillOverlay effects, and effectsRawXml raw effect fragments."
        },
        "rhwp_get_shape_properties" => {
            "Get shape/OLE/chart object properties, including captionText, shape shadow metadata, HWPX effects.threeD/shadow/glow/softEdge/reflection metadata, and rawHwpxChildXml fragments, for a body, table-cell, or header/footer shape target."
        }
        "rhwp_set_shape_properties" => {
            "Set shape/OLE/chart object properties, including captionText, shape shadow metadata, HWPX effects.threeD/shadow/glow/softEdge/reflection metadata, and rawHwpxChildXml fragments, for a body, table-cell, or header/footer shape target."
        }
        "rhwp_get_chart_data" => {
            "Get semantic OOXML chart data from a body OLE chart target, read-only legacy OLE Contents chart IR, or raw diagnostics for a native HWP CHART_DATA target."
        }
        "rhwp_set_chart_data" => {
            "Edit supported semantic fields and cached labels/values in a body OOXML chart target, or explicitly replace native HWP CHART_DATA raw payload bytes. Legacy OLE Contents charts are read-only."
        }
        "rhwp_insert_shape" => "Insert a body drawing shape using the native DocumentCore shape API.",
        "rhwp_delete_shape" => "Delete a body, table-cell, or header/footer Shape/OLE/chart control.",
        "rhwp_delete_equation" => "Delete a body, table-cell, or header/footer equation control.",
        "rhwp_change_shape_z_order" => {
            "Move a body Shape/OLE/chart control or ShapeGroup child in z-order."
        }
        "rhwp_group_shapes" => {
            "Group body or header/footer Shape/Picture controls in the same section."
        }
        "rhwp_insert_shape_group_child" => {
            "Insert a drawing shape child into a body, table-cell, or header/footer ShapeGroup."
        }
        "rhwp_get_shape_group_children" => {
            "Inspect child shapes inside a body, table-cell, or header/footer ShapeGroup control."
        }
        "rhwp_ungroup_shape" => "Ungroup one body or header/footer ShapeGroup control.",
        "rhwp_list_header_footers" => "List header and footer controls in a session.",
        "rhwp_get_header_footer" => "Get header/footer text and metadata.",
        "rhwp_create_header_footer" => "Create an empty header/footer control.",
        "rhwp_delete_header_footer" => "Delete a header/footer control.",
        "rhwp_get_header_footer_para_info" => "Get header/footer paragraph count and character count.",
        "rhwp_insert_header_footer_text" => "Insert text into a header/footer paragraph.",
        "rhwp_delete_header_footer_text" => "Delete text from a header/footer paragraph.",
        "rhwp_split_header_footer_paragraph" => "Split a header/footer paragraph.",
        "rhwp_merge_header_footer_paragraph" => "Merge a header/footer paragraph with the previous one.",
        "rhwp_get_header_footer_para_format" => "Get paragraph formatting for a header/footer paragraph.",
        "rhwp_apply_header_footer_para_format" => {
            "Apply paragraph formatting to a header/footer paragraph."
        },
        "rhwp_insert_header_footer_field" => {
            "Insert a header/footer field marker such as page number, total pages, or file name."
        },
        "rhwp_apply_header_footer_template" => {
            "Apply a built-in header/footer page-number template."
        },
        "rhwp_get_page_def" => "Get page definition settings for a section.",
        "rhwp_get_section_def" => "Get section definition settings for a section.",
        "rhwp_list_bookmarks" => "List bookmark controls in a session.",
        "rhwp_rename_bookmark" => "Rename an existing bookmark control.",
        _ => "rhwp document tool.",
    }
}

type SchemaProp = (&'static str, Value);

fn tool_input_schema(name: &str) -> Value {
    match name {
        "rhwp_open" => object_schema(
            vec![string_prop(
                "path",
                "HWP/HWPX file path under RHWP_MCP_ROOT or the server cwd.",
            )],
            vec!["path"],
            false,
        ),
        "rhwp_new" => object_schema(Vec::new(), Vec::new(), false),
        "rhwp_new_exam_from_ingest" => with_one_of_required(
            object_schema(
            vec![
                (
                    "ingest",
                    json!({
                        "type": "object",
                        "description": "ingest_schema_v1 JSON object."
                    }),
                ),
                string_prop("ingest_json", "ingest_schema_v1 JSON string."),
                string_prop(
                    "ingest_path",
                    "Path to ingest_schema_v1 JSON under RHWP_MCP_ROOT or cwd.",
                ),
            ],
            Vec::new(),
            false,
            ),
            vec![vec!["ingest"], vec!["ingest_json"], vec!["ingest_path"]],
        ),
        "rhwp_new_government_report" => with_one_of_required(
            object_schema(
                vec![
                    (
                        "report",
                        json!({
                            "type": "object",
                            "description": "Government/business report JSON object."
                        }),
                    ),
                    string_prop("report_json", "Government/business report JSON string."),
                    string_prop(
                        "report_path",
                        "Path to government/business report JSON under RHWP_MCP_ROOT or cwd.",
                    ),
                ],
                Vec::new(),
                false,
            ),
            vec![vec!["report"], vec!["report_json"], vec!["report_path"]],
        ),
        "rhwp_new_from_document_template" => with_one_of_required(
            object_schema(
                vec![
                    (
                        "template",
                        json!({
                            "type": "object",
                            "description": "rhwp document-template v1 JSON object."
                        }),
                    ),
                    string_prop("template_json", "rhwp document-template v1 JSON string."),
                    string_prop(
                        "template_path",
                        "Path to rhwp document-template v1 JSON under RHWP_MCP_ROOT or cwd.",
                    ),
                    (
                        "linked_image_policy",
                        json!({
                            "type": "string",
                            "enum": [
                                "preserve_links",
                                "embed_accessible",
                                "embed_required"
                            ],
                            "description": "How document-template linked pictures and drawing image fills are regenerated. preserve_links keeps Link BinData; embed_accessible embeds readable paths under RHWP_MCP_ROOT/cwd and preserves missing links; embed_required fails if any linked image cannot be embedded."
                        }),
                    ),
                    (
                        "linkedImagePolicy",
                        json!({
                            "type": "string",
                            "enum": [
                                "preserve_links",
                                "embed_accessible",
                                "embed_required"
                            ],
                            "description": "CamelCase alias for linked_image_policy."
                        }),
                    ),
                    (
                        "page_flow_policy",
                        json!({
                            "type": "string",
                            "enum": [
                                "preserve",
                                "compact_artifacts"
                            ],
                            "description": "How document-template page-flow hints are applied. preserve keeps all extracted page breaks; compact_artifacts removes empty page-break paragraphs and floating-object page breaks that often inflate regenerated page counts."
                        }),
                    ),
                    (
                        "pageFlowPolicy",
                        json!({
                            "type": "string",
                            "enum": [
                                "preserve",
                                "compact_artifacts"
                            ],
                            "description": "CamelCase alias for page_flow_policy."
                        }),
                    ),
                ],
                Vec::new(),
                false,
            ),
            vec![
                vec!["template"],
                vec!["template_json"],
                vec!["template_path"],
            ],
        ),
        "rhwp_close"
        | "rhwp_document_info"
        | "rhwp_document_profile"
        | "rhwp_style_signature"
        | "rhwp_revision_info"
        | "rhwp_hwpx_package_info"
        | "rhwp_list_fields"
        | "rhwp_list_bookmarks" => session_schema(Vec::new(), Vec::new(), false),
        "rhwp_get_style_list" => session_schema(
            vec![
                bool_prop(
                    "include_formats",
                    "Include resolved charFormat and paraFormat summaries for each style.",
                ),
                bool_prop("includeFormats", "CamelCase alias for include_formats."),
                bool_prop(
                    "include_raw",
                    "Include raw HWP STYLE record payloads as base64 when available.",
                ),
                bool_prop("includeRaw", "CamelCase alias for include_raw."),
            ],
            Vec::new(),
            false,
        ),
        "rhwp_get_hwpx_package_entry" => session_schema(
            vec![
                string_prop(
                    "path",
                    "Preserved HWPX auxiliary entry path returned by rhwp_hwpx_package_info.",
                ),
                bool_prop("include_text", "Include UTF-8 text preview when possible."),
                bool_prop("includeText", "CamelCase alias for include_text."),
                int_prop("max_text_bytes", "Maximum UTF-8 text bytes to return."),
                int_prop("maxTextBytes", "CamelCase alias for max_text_bytes."),
            ],
            vec!["path"],
            false,
        ),
        "rhwp_save" => session_schema(
            vec![
                string_prop(
                    "path",
                    "Target path. Required for unsaved sessions; guarded by RHWP_MCP_ROOT/cwd.",
                ),
                format_prop(),
                bool_prop("overwrite", "Allow replacing an existing target file."),
            ],
            Vec::new(),
            false,
        ),
        "rhwp_export_bytes" => session_schema(vec![format_prop()], Vec::new(), false),
        "rhwp_extract_text" | "rhwp_extract_markdown" | "rhwp_render_svg" | "rhwp_render_png" => {
            session_schema(vec![page_prop()], Vec::new(), false)
        }
        "rhwp_preview_page" => session_schema(
            vec![
                page_prop(),
                bool_prop("include_svg", "Include raw SVG in the preview payload. Defaults to true."),
                bool_prop("includeSvg", "CamelCase alias for include_svg."),
                bool_prop("include_html", "Include embeddable HTML around the SVG preview. Defaults to true."),
                bool_prop("includeHtml", "CamelCase alias for include_html."),
                bool_prop("include_text", "Include visual-order page text. Defaults to true."),
                bool_prop("includeText", "CamelCase alias for include_text."),
                bool_prop("include_markdown", "Include page markdown with table/image tokens. Defaults to true."),
                bool_prop("includeMarkdown", "CamelCase alias for include_markdown."),
                bool_prop("include_png", "Include PNG base64 rendered through the SVG/resvg path. Defaults to false."),
                bool_prop("includePng", "CamelCase alias for include_png."),
            ],
            Vec::new(),
            false,
        ),
        "rhwp_extract_document_template" => session_schema(
            vec![
                bool_prop(
                    "preserve_empty_paragraphs",
                    "Keep empty paragraphs as template paragraph blocks for closer visual reproduction. Defaults to false for compact templates.",
                ),
                bool_prop(
                    "preserveEmptyParagraphs",
                    "CamelCase alias for preserve_empty_paragraphs.",
                ),
            ],
            Vec::new(),
            false,
        ),
        "rhwp_search" => session_schema(
            vec![
                string_prop("query", "Search string."),
                bool_prop("case_sensitive", "Use case-sensitive matching. Defaults to true."),
                bool_prop("include_cells", "Search inside table cells. Defaults to true."),
            ],
            vec!["query"],
            false,
        ),
        "rhwp_list_controls" => session_schema(
            vec![
                section_prop(),
                para_prop(),
                string_prop("kind", "Optional control kind filter, such as Table, Picture, Equation, Footnote, Bookmark, Chart, Ole, Header, Footer, Field, or HiddenComment."),
                bool_prop("include_nested", "Include controls inside tables, notes, headers, captions, and text boxes."),
                bool_prop("includeNested", "CamelCase alias for include_nested."),
                int_prop("max_items", "Maximum controls to return."),
                int_prop("maxItems", "CamelCase alias for max_items."),
            ],
            Vec::new(),
            false,
        ),
        "rhwp_compare_text" => compare_schema(compare_props(true)),
        "rhwp_compare_document_profile" => {
            let mut props = compare_props(false);
            props.extend([
                int_prop("max_diffs", "Maximum structural profile differences to include."),
                int_prop("maxDiffs", "CamelCase alias for max_diffs."),
                bool_prop("ignore_page_count", "Ignore profile page_count differences."),
                bool_prop("ignorePageCount", "CamelCase alias for ignore_page_count."),
            ]);
            compare_schema(props)
        }
        "rhwp_compare_style_signature" => {
            let mut props = compare_props(false);
            props.extend([
                int_prop("max_diffs", "Maximum style signature differences to include."),
                int_prop("maxDiffs", "CamelCase alias for max_diffs."),
            ]);
            compare_schema(props)
        }
        "rhwp_compare_fidelity_summary" => {
            let mut props = compare_props(true);
            props.extend([
                int_prop("max_text_diffs", "Maximum text line differences to include."),
                int_prop("maxTextDiffs", "CamelCase alias for max_text_diffs."),
                int_prop("max_profile_diffs", "Maximum document-profile differences to include."),
                int_prop("maxProfileDiffs", "CamelCase alias for max_profile_diffs."),
                int_prop("max_style_diffs", "Maximum style-signature differences to include."),
                int_prop("maxStyleDiffs", "CamelCase alias for max_style_diffs."),
                int_prop("max_deltas", "Maximum geometry deltas per page."),
                int_prop("maxDeltas", "CamelCase alias for max_deltas."),
                number_prop("max_disp_threshold", "Maximum allowed displacement in render units."),
                number_prop("maxDispThreshold", "CamelCase alias for max_disp_threshold."),
                number_prop("max_png_changed_percent", "Maximum allowed PNG changed percentage."),
                number_prop(
                    "maxPngChangedPercent",
                    "CamelCase alias for max_png_changed_percent.",
                ),
                bool_prop("ignore_page_count", "Ignore profile page_count differences."),
                bool_prop("ignorePageCount", "CamelCase alias for ignore_page_count."),
                bool_prop(
                    "include_render_geometry",
                    "Include render-tree geometry comparison. Defaults to true.",
                ),
                bool_prop(
                    "includeRenderGeometry",
                    "CamelCase alias for include_render_geometry.",
                ),
                bool_prop(
                    "include_render_png",
                    "Include PNG pixel comparison for one page. Defaults to false.",
                ),
                bool_prop("includeRenderPng", "CamelCase alias for include_render_png."),
                bool_prop(
                    "include_package",
                    "Include HWPX package diff by exporting both sides to HWPX.",
                ),
                bool_prop("includePackage", "CamelCase alias for include_package."),
                bool_prop(
                    "strict_package",
                    "When include_package is true, require byte/package equality instead of structural equality.",
                ),
                bool_prop("strictPackage", "CamelCase alias for strict_package."),
                page_prop(),
                int_prop("left_page", "Left-hand zero-based page index for PNG comparison."),
                int_prop("leftPage", "CamelCase alias for left_page."),
                int_prop("right_page", "Right-hand zero-based page index for PNG comparison."),
                int_prop("rightPage", "CamelCase alias for right_page."),
            ]);
            compare_schema(props)
        }
        "rhwp_compare_render_geometry" => {
            let mut props = compare_props(false);
            props.extend([
                int_prop("max_deltas", "Maximum geometry deltas per page."),
                int_prop("maxDeltas", "CamelCase alias for max_deltas."),
                number_prop("max_disp_threshold", "Maximum allowed displacement in render units."),
                number_prop("maxDispThreshold", "CamelCase alias for max_disp_threshold."),
                page_prop(),
            ]);
            compare_schema(props)
        }
        "rhwp_compare_render_png" => {
            let mut props = compare_props(false);
            props.extend([
                page_prop(),
                int_prop("left_page", "Left-hand zero-based page index. Overrides page."),
                int_prop("leftPage", "CamelCase alias for left_page."),
                int_prop("right_page", "Right-hand zero-based page index. Defaults to the left page."),
                int_prop("rightPage", "CamelCase alias for right_page."),
            ]);
            compare_schema(props)
        }
        "rhwp_match_render_pages" => {
            let mut props = compare_props(false);
            props.extend([
                page_prop(),
                int_prop("source_page", "Source zero-based page index. Overrides page."),
                int_prop("sourcePage", "CamelCase alias for source_page."),
                int_prop("start_page", "First candidate page index in the right-hand document."),
                int_prop("startPage", "CamelCase alias for start_page."),
                int_prop("end_page", "Last candidate page index in the right-hand document."),
                int_prop("endPage", "CamelCase alias for end_page."),
                int_prop("max_matches", "Maximum page matches to return."),
                int_prop("maxMatches", "CamelCase alias for max_matches."),
            ]);
            compare_schema(props)
        }
        "rhwp_compare_hwp_records" => {
            let mut props = compare_props(false);
            props.extend([
                section_u32_prop(),
                int_prop("max_diffs", "Maximum HWP record differences to include."),
                int_prop("maxDiffs", "CamelCase alias for max_diffs."),
            ]);
            compare_schema(props)
        }
        "rhwp_compare_hwpx_package" => {
            let mut props = compare_props(false);
            props.extend([
                int_prop("max_diffs", "Maximum HWPX package differences to include."),
                int_prop("maxDiffs", "CamelCase alias for max_diffs."),
            ]);
            compare_schema(props)
        }
        "rhwp_insert_text" => session_schema(
            text_position_props(vec![string_prop("text", "Text to insert.")]),
            vec!["char_offset", "text"],
            false,
        ),
        "rhwp_delete_text" => session_schema(
            text_position_props(vec![int_prop("count", "Number of characters to delete.")]),
            vec!["char_offset", "count"],
            false,
        ),
        "rhwp_replace_text" => with_one_of_required(
            session_schema(
            text_position_props(vec![
                string_prop("query", "Search query for replace-one or replace-all mode."),
                int_prop("length", "Number of characters to replace in positional mode."),
                string_prop("new_text", "Replacement text."),
                bool_prop("all", "Replace every matching query occurrence."),
                bool_prop("case_sensitive", "Use case-sensitive query matching. Defaults to true."),
                string_prop(
                    "layout_policy",
                    "Replacement layout policy: reflow (default) or preserve_source_line_segments for HWPX replacements.",
                ),
                string_prop(
                    "layoutPolicy",
                    "CamelCase alias for layout_policy.",
                ),
            ]),
            vec!["new_text"],
            false,
            ),
            vec![vec!["query"], vec!["char_offset", "length"]],
        ),
        "rhwp_split_paragraph" | "rhwp_insert_page_break" | "rhwp_insert_column_break" => {
            session_schema(text_position_props(Vec::new()), vec!["char_offset"], false)
        }
        "rhwp_merge_paragraph" | "rhwp_insert_paragraph" | "rhwp_delete_paragraph" => {
            session_schema(section_para_props(), vec!["para"], false)
        }
        "rhwp_get_field" => with_one_of_required(
            session_schema(
            vec![
                int_prop("field_id", "Field id returned by rhwp_list_fields."),
                string_prop("name", "Field name when selecting by name."),
            ],
            Vec::new(),
            false,
            ),
            vec![vec!["field_id"], vec!["name"]],
        ),
        "rhwp_set_field" => with_one_of_required(
            session_schema(
            vec![
                int_prop("field_id", "Field id returned by rhwp_list_fields."),
                string_prop("name", "Field name when selecting by name."),
                string_prop("value", "Field value to write."),
            ],
            vec!["value"],
            false,
            ),
            vec![vec!["field_id"], vec!["name"]],
        ),
        "rhwp_insert_click_here_field" => session_schema(
            field_position_props(vec![
                string_prop("guide", "Visible guide text."),
                string_prop("memo", "Field memo text."),
                string_prop("name", "Field name."),
                bool_prop("editable", "Whether the field is editable."),
            ]),
            vec!["char_offset"],
            false,
        ),
        "rhwp_remove_field" => {
            session_schema(field_position_props(Vec::new()), vec!["char_offset"], false)
        }
        "rhwp_create_table" => session_schema(
            table_insert_target_props(vec![
                int_prop("rows", "Table row count."),
                int_prop("cols", "Table column count."),
            ]),
            vec!["char_offset", "rows", "cols"],
            false,
        ),
        "rhwp_get_table_dimensions" => session_schema(
            table_target_props(Vec::new()),
            vec!["para"],
            false,
        ),
        "rhwp_get_table_properties" => session_schema(
            table_target_props(Vec::new()),
            vec!["para", "control"],
            false,
        ),
        "rhwp_set_table_properties" => session_schema(
            table_target_props(vec![table_props_prop()]),
            vec!["para", "control", "props"],
            true,
        ),
        "rhwp_get_cell_text" => session_schema(
            cell_target_props(Vec::new()),
            vec!["para"],
            false,
        ),
        "rhwp_set_cell_text" => session_schema(
            cell_target_props(vec![string_prop("text", "Cell text to write.")]),
            vec!["para", "text"],
            false,
        ),
        "rhwp_insert_table_row" | "rhwp_delete_table_row" => session_schema(
            table_target_props(vec![
                int_prop("row", "Zero-based table row index."),
                bool_prop("below", "Insert below the row. Insert tools only; defaults to true."),
            ]),
            vec!["para", "row"],
            false,
        ),
        "rhwp_insert_table_column" | "rhwp_delete_table_column" => session_schema(
            table_target_props(vec![
                int_prop("col", "Zero-based table column index."),
                bool_prop("right", "Insert to the right of the column. Insert tools only; defaults to true."),
            ]),
            vec!["para", "col"],
            false,
        ),
        "rhwp_merge_table_cells" => session_schema(
            table_target_props(vec![
                int_prop("start_row", "Start row."),
                int_prop("start_col", "Start column."),
                int_prop("end_row", "End row."),
                int_prop("end_col", "End column."),
            ]),
            vec![
                "para",
                "start_row",
                "start_col",
                "end_row",
                "end_col",
            ],
            false,
        ),
        "rhwp_split_table_cell" => session_schema(
            table_target_props(vec![
                int_prop("row", "Cell row."),
                int_prop("col", "Cell column."),
                int_prop("rows", "Number of rows after split."),
                int_prop("cols", "Number of columns after split."),
                bool_prop("equal_row_height", "Use equal row heights when splitting."),
                bool_prop("merge_first", "Merge the target cell first before splitting."),
            ]),
            vec!["para", "row", "col"],
            false,
        ),
        "rhwp_evaluate_table_formula" => session_schema(
            table_target_props(vec![
                int_prop("target_row", "Formula output row."),
                int_prop("targetRow", "CamelCase alias for target_row."),
                int_prop("target_col", "Formula output column."),
                int_prop("targetCol", "CamelCase alias for target_col."),
                int_prop("row", "Alias for target_row."),
                int_prop("col", "Alias for target_col."),
                string_prop("formula", "Formula expression."),
                bool_prop("write_result", "Write the evaluated result into the target cell."),
                bool_prop("writeResult", "CamelCase alias for write_result."),
            ]),
            vec!["para", "formula"],
            false,
        ),
        "rhwp_apply_char_format" => session_schema(
            format_target_props(vec![
                int_prop("start", "Start character offset."),
                int_prop("end", "End character offset."),
                bool_prop(
                    "is_header",
                    "Target a plain header/footer paragraph instead of a body paragraph.",
                ),
                bool_prop("isHeader", "CamelCase alias for is_header."),
                int_prop(
                    "apply_to",
                    "Header/footer apply target: 0=both, 1=even, 2=odd.",
                ),
                int_prop("applyTo", "CamelCase alias for apply_to."),
                char_format_props_prop(),
            ]),
            vec!["start", "end"],
            true,
        ),
        "rhwp_apply_para_format" => session_schema(
            format_target_props(vec![
                bool_prop(
                    "is_header",
                    "Target a plain header/footer paragraph instead of a body paragraph.",
                ),
                bool_prop("isHeader", "CamelCase alias for is_header."),
                int_prop(
                    "apply_to",
                    "Header/footer apply target: 0=both, 1=even, 2=odd.",
                ),
                int_prop("applyTo", "CamelCase alias for apply_to."),
                para_format_props_prop(),
            ]),
            Vec::new(),
            true,
        ),
        "rhwp_apply_style" => session_schema(
            format_target_props(vec![
                int_prop("style_id", "Style id to apply."),
                bool_prop(
                    "is_header",
                    "Target a plain header/footer paragraph instead of a body paragraph.",
                ),
                bool_prop("isHeader", "CamelCase alias for is_header."),
                int_prop(
                    "apply_to",
                    "Header/footer apply target: 0=both, 1=even, 2=odd.",
                ),
                int_prop("applyTo", "CamelCase alias for apply_to."),
            ]),
            vec!["style_id"],
            true,
        ),
        "rhwp_create_style" => session_schema(
            vec![
                string_prop("name", "Local style name."),
                string_prop("english_name", "English style name."),
                string_prop("englishName", "CamelCase alias for english_name."),
                int_prop(
                    "type",
                    "Style type id. 0=paragraph, 1=character. Must fit in u8.",
                ),
                int_prop("style_type", "Alias for type."),
                int_prop(
                    "next_style_id",
                    "Next style id. Must reference an existing style or the style being created.",
                ),
                int_prop("nextStyleId", "CamelCase alias for next_style_id."),
                signed_int_prop("lang_id", "HWP STYLE language id, defaulting to base style language or Korean 1042. Must fit in i16."),
                signed_int_prop("langId", "CamelCase alias for lang_id."),
                string_prop(
                    "raw_hwp_style_base64",
                    "Exact serialized HWP STYLE record body. Cannot be combined with semantic style fields.",
                ),
                string_prop("rawHwpStyleBase64", "CamelCase alias for raw_hwp_style_base64."),
                int_prop(
                    "base_style_id",
                    "Style id whose char/para shape refs should be used as the base.",
                ),
                int_prop("baseStyleId", "CamelCase alias for base_style_id."),
                int_prop("based_on_style_id", "Alias for base_style_id."),
                int_prop("basedOnStyleId", "CamelCase alias for based_on_style_id."),
                int_prop(
                    "char_shape_id",
                    "Character shape id to reference. Must be in range.",
                ),
                int_prop("charShapeId", "CamelCase alias for char_shape_id."),
                int_prop(
                    "para_shape_id",
                    "Paragraph shape id to reference. Must be in range.",
                ),
                int_prop("paraShapeId", "CamelCase alias for para_shape_id."),
                style_char_format_prop(
                    "char_format",
                    "Character formatter property bag used to create or reuse a CharShape for this style. Known fields are type/range validated.",
                ),
                style_char_format_prop(
                    "charFormat",
                    "CamelCase alias for char_format.",
                ),
                style_para_format_prop(
                    "para_format",
                    "Paragraph formatter property bag used to create or reuse a ParaShape for this style. Known fields are type/range/enum validated.",
                ),
                style_para_format_prop(
                    "paraFormat",
                    "CamelCase alias for para_format.",
                ),
                int_prop("base_char_shape_id", "Base character shape id. Must be in range."),
                int_prop("baseCharShapeId", "CamelCase alias for base_char_shape_id."),
                int_prop("base_para_shape_id", "Base paragraph shape id. Must be in range."),
                int_prop("baseParaShapeId", "CamelCase alias for base_para_shape_id."),
            ],
            Vec::new(),
            false,
        ),
        "rhwp_update_style" => session_schema(
            vec![
                int_prop("style_id", "Style id to update."),
                string_prop("name", "New local style name."),
                string_prop("english_name", "New English style name."),
                string_prop("englishName", "CamelCase alias for english_name."),
                int_prop("type", "New style type id. 0=paragraph, 1=character. Must fit in u8."),
                int_prop("style_type", "Alias for type."),
                int_prop(
                    "next_style_id",
                    "New next style id. Must reference an existing style.",
                ),
                int_prop("nextStyleId", "CamelCase alias for next_style_id."),
                signed_int_prop("lang_id", "New HWP STYLE language id. Must fit in i16."),
                signed_int_prop("langId", "CamelCase alias for lang_id."),
                string_prop(
                    "raw_hwp_style_base64",
                    "Exact replacement serialized HWP STYLE record body. Cannot be combined with semantic style fields.",
                ),
                string_prop("rawHwpStyleBase64", "CamelCase alias for raw_hwp_style_base64."),
                int_prop(
                    "base_style_id",
                    "Style id whose char/para shape refs should be used as the update base.",
                ),
                int_prop("baseStyleId", "CamelCase alias for base_style_id."),
                int_prop("based_on_style_id", "Alias for base_style_id."),
                int_prop("basedOnStyleId", "CamelCase alias for based_on_style_id."),
                int_prop("char_shape_id", "New character shape id reference."),
                int_prop("charShapeId", "CamelCase alias for char_shape_id."),
                int_prop("para_shape_id", "New paragraph shape id reference."),
                int_prop("paraShapeId", "CamelCase alias for para_shape_id."),
                style_char_format_prop(
                    "char_format",
                    "Character formatter property bag used to create or reuse a CharShape for this style. Known fields are type/range validated.",
                ),
                style_char_format_prop(
                    "charFormat",
                    "CamelCase alias for char_format.",
                ),
                style_para_format_prop(
                    "para_format",
                    "Paragraph formatter property bag used to create or reuse a ParaShape for this style. Known fields are type/range/enum validated.",
                ),
                style_para_format_prop(
                    "paraFormat",
                    "CamelCase alias for para_format.",
                ),
            ],
            vec!["style_id"],
            false,
        ),
        "rhwp_delete_style" => session_schema(
            vec![int_prop("style_id", "Style id to delete.")],
            vec!["style_id"],
            false,
        ),
        "rhwp_insert_picture" => with_one_of_required(
            session_schema(
            text_position_props(vec![
                cell_path_prop(
                    "cell_path",
                    "Optional table-cell or shape text-box cell path target.",
                ),
                int_prop(
                    "control",
                    "Header/footer control index when inserting into a header/footer paragraph.",
                ),
                string_prop(
                    "container_scope",
                    "Header/footer insertion scope, either header or footer.",
                ),
                string_prop("containerScope", "CamelCase alias for container_scope."),
                int_prop("inner_para", "Nested header/footer paragraph index."),
                int_prop("innerPara", "CamelCase alias for inner_para."),
                int_prop("hf_para", "Header/footer paragraph index alias."),
                int_prop("hfPara", "CamelCase alias for hf_para."),
                int_prop("hf_para_idx", "Header/footer paragraph index alias."),
                int_prop("hfParaIndex", "CamelCase alias for hf_para_idx."),
                string_prop("image_base64", "Base64 image bytes."),
                string_prop("image_path", "Image file path under RHWP_MCP_ROOT or cwd."),
                int_prop("width", "Picture width in HWP units."),
                int_prop("height", "Picture height in HWP units."),
                int_prop("natural_width_px", "Natural image pixel width."),
                int_prop("natural_height_px", "Natural image pixel height."),
                string_prop("extension", "Image extension, such as png or jpg."),
                string_prop("description", "Picture description."),
                signed_int_prop("paper_offset_x_hu", "Absolute paper x offset in HWP units."),
                signed_int_prop("paper_offset_y_hu", "Absolute paper y offset in HWP units."),
            ]),
            vec!["char_offset", "width", "height"],
            false,
            ),
            vec![vec!["image_base64"], vec!["image_path"]],
        ),
        "rhwp_get_picture_properties" => session_schema(
            picture_target_props(Vec::new()),
            vec!["para", "control"],
            false,
        ),
        "rhwp_set_picture_properties" => session_schema(
            picture_target_props(vec![picture_props_prop()]),
            vec!["para", "control"],
            true,
        ),
        "rhwp_get_shape_properties" => session_schema(
            shape_target_props(Vec::new()),
            vec!["para", "control"],
            false,
        ),
        "rhwp_set_shape_properties" => session_schema(
            shape_target_props(vec![shape_props_prop()]),
            vec!["para", "control"],
            true,
        ),
        "rhwp_get_chart_data" => session_schema(
            vec![
                int_prop("section", "Section index. Defaults to 0."),
                int_prop("para", "Body paragraph index containing the chart OLE control."),
                int_prop("control", "Body control index for the chart OLE control."),
            ],
            vec!["para", "control"],
            false,
        ),
        "rhwp_set_chart_data" => session_schema(
            vec![
                int_prop("section", "Section index. Defaults to 0."),
                int_prop("para", "Body paragraph index containing the chart OLE control."),
                int_prop("control", "Body control index for the chart OLE control."),
                chart_props_prop(),
            ],
            vec!["para", "control", "props"],
            true,
        ),
        "rhwp_insert_shape" => session_schema(
            text_position_props(vec![
                cell_path_prop(
                    "cell_path",
                    "Optional table-cell or shape text-box cell path target.",
                ),
                int_prop(
                    "control",
                    "Header/footer control index when inserting into a header/footer paragraph.",
                ),
                string_prop(
                    "container_scope",
                    "Header/footer insertion scope, either header or footer.",
                ),
                string_prop("containerScope", "CamelCase alias for container_scope."),
                int_prop("inner_para", "Nested header/footer paragraph index."),
                int_prop("innerPara", "CamelCase alias for inner_para."),
                int_prop("hf_para", "Header/footer paragraph index alias."),
                int_prop("hfPara", "CamelCase alias for hf_para."),
                int_prop("hf_para_idx", "Header/footer paragraph index alias."),
                int_prop("hfParaIndex", "CamelCase alias for hf_para_idx."),
                shape_type_prop("shape_type", "Drawing shape type. Defaults to rectangle."),
                shape_type_prop("shapeType", "CamelCase alias for shape_type."),
                int_prop("width", "Shape width in HWP units."),
                int_prop("height", "Shape height in HWP units."),
                int_prop("horizontal_offset", "Shape horizontal offset in HWP units."),
                int_prop("horizontalOffset", "CamelCase alias for horizontal_offset."),
                int_prop("horz_offset", "Alias for horizontal_offset."),
                int_prop("horzOffset", "CamelCase alias for horz_offset."),
                int_prop("vertical_offset", "Shape vertical offset in HWP units."),
                int_prop("verticalOffset", "CamelCase alias for vertical_offset."),
                int_prop("vert_offset", "Alias for vertical_offset."),
                int_prop("vertOffset", "CamelCase alias for vert_offset."),
                bool_prop("treat_as_char", "Whether the shape is treated as a character."),
                bool_prop("treatAsChar", "CamelCase alias for treat_as_char."),
                text_wrap_prop("text_wrap", "Text-wrap mode. Defaults to InFrontOfText."),
                text_wrap_prop("textWrap", "CamelCase alias for text_wrap."),
                bool_prop("line_flip_x", "Reverse line/connector x direction."),
                bool_prop("lineFlipX", "CamelCase alias for line_flip_x."),
                bool_prop("line_flip_y", "Reverse line/connector y direction."),
                bool_prop("lineFlipY", "CamelCase alias for line_flip_y."),
                polygon_points_prop("polygon_points", "Optional polygon points for shape_type=polygon."),
                polygon_points_prop("polygonPoints", "CamelCase alias for polygon_points."),
            ]),
            vec!["char_offset", "width", "height"],
            false,
        ),
        "rhwp_delete_shape" => {
            session_schema(shape_target_props(Vec::new()), vec!["para", "control"], false)
        }
        "rhwp_change_shape_z_order" => session_schema(
            shape_target_props(vec![shape_z_order_operation_prop()]),
            vec!["para", "control", "operation"],
            false,
        ),
        "rhwp_group_shapes" => session_schema(
            shape_target_props(vec![shape_group_targets_prop()]),
            vec!["targets"],
            false,
        ),
        "rhwp_insert_shape_group_child" => session_schema(
            shape_target_props(vec![
                shape_type_prop("shape_type", "Drawing shape type. Defaults to rectangle."),
                shape_type_prop("shapeType", "CamelCase alias for shape_type."),
                int_prop("width", "Shape child width in HWP units."),
                int_prop("height", "Shape child height in HWP units."),
                int_prop(
                    "horizontal_offset",
                    "Shape child local horizontal offset inside the parent ShapeGroup.",
                ),
                int_prop("horizontalOffset", "CamelCase alias for horizontal_offset."),
                int_prop("horz_offset", "Alias for horizontal_offset."),
                int_prop("horzOffset", "CamelCase alias for horz_offset."),
                int_prop(
                    "vertical_offset",
                    "Shape child local vertical offset inside the parent ShapeGroup.",
                ),
                int_prop("verticalOffset", "CamelCase alias for vertical_offset."),
                int_prop("vert_offset", "Alias for vertical_offset."),
                int_prop("vertOffset", "CamelCase alias for vert_offset."),
                int_prop("child_index", "Optional insertion index inside the parent ShapeGroup."),
                int_prop("childIndex", "CamelCase alias for child_index."),
                bool_prop("treat_as_char", "Whether the child shape is treated as a character."),
                bool_prop("treatAsChar", "CamelCase alias for treat_as_char."),
                text_wrap_prop("text_wrap", "Text-wrap mode. Defaults to InFrontOfText."),
                text_wrap_prop("textWrap", "CamelCase alias for text_wrap."),
                bool_prop("line_flip_x", "Reverse line/connector x direction."),
                bool_prop("lineFlipX", "CamelCase alias for line_flip_x."),
                bool_prop("line_flip_y", "Reverse line/connector y direction."),
                bool_prop("lineFlipY", "CamelCase alias for line_flip_y."),
                polygon_points_prop("polygon_points", "Optional polygon points for shape_type=polygon."),
                polygon_points_prop("polygonPoints", "CamelCase alias for polygon_points."),
            ]),
            vec!["para", "control", "width", "height"],
            false,
        ),
        "rhwp_get_shape_group_children" => {
            session_schema(shape_target_props(Vec::new()), vec!["para", "control"], false)
        }
        "rhwp_ungroup_shape" => {
            session_schema(shape_target_props(Vec::new()), vec!["para", "control"], false)
        }
        "rhwp_set_equation_properties" => session_schema(
            format_target_props(vec![equation_props_prop()]),
            vec!["para", "control"],
            true,
        ),
        "rhwp_delete_equation" => {
            session_schema(format_target_props(Vec::new()), vec!["para", "control"], false)
        }
        "rhwp_delete_picture" => {
            session_schema(picture_target_props(Vec::new()), vec!["para", "control"], false)
        }
        "rhwp_insert_equation" => session_schema(
            format_target_props(vec![
                int_prop(
                    "char_offset",
                    "Character offset in the target body, cell, or header/footer paragraph.",
                ),
                string_prop("script", "Equation script."),
                int_prop("font_size", "Equation font size."),
                int_prop("color", "Equation color value."),
            ]),
            vec!["char_offset", "script"],
            false,
        ),
        "rhwp_insert_footnote" | "rhwp_insert_endnote" => {
            session_schema(text_position_props(Vec::new()), vec!["char_offset"], false)
        }
        "rhwp_insert_hidden_comment" => session_schema(
            text_position_props(vec![string_prop("text", "Hidden comment paragraph text.")]),
            vec!["char_offset", "text"],
            false,
        ),
        "rhwp_get_hidden_comment" => {
            hidden_comment_schema(hidden_comment_target_props(Vec::new()), vec!["para"], false)
        }
        "rhwp_insert_hidden_comment_text" => hidden_comment_schema(
            hidden_comment_target_props(vec![
                int_prop(
                    "char_offset",
                    "Character offset inside the hidden comment paragraph.",
                ),
                string_prop("text", "Hidden comment text to insert."),
            ]),
            vec!["para", "char_offset", "text"],
            false,
        ),
        "rhwp_delete_hidden_comment_text" => hidden_comment_schema(
            hidden_comment_target_props(vec![
                int_prop(
                    "char_offset",
                    "Character offset inside the hidden comment paragraph.",
                ),
                int_prop("count", "Number of characters to delete."),
            ]),
            vec!["para", "char_offset", "count"],
            false,
        ),
        "rhwp_split_hidden_comment_paragraph" => hidden_comment_schema(
            hidden_comment_target_props(vec![int_prop(
                "char_offset",
                "Character offset inside the hidden comment paragraph.",
            )]),
            vec!["para", "char_offset"],
            false,
        ),
        "rhwp_merge_hidden_comment_paragraph" => {
            hidden_comment_schema(hidden_comment_target_props(Vec::new()), vec!["para"], false)
        }
        "rhwp_apply_hidden_comment_char_format" => hidden_comment_schema(
            hidden_comment_target_props(vec![
                int_prop("start", "Start character offset inside the hidden comment paragraph."),
                int_prop("end", "End character offset inside the hidden comment paragraph."),
                char_format_props_prop(),
            ]),
            vec!["para", "start", "end"],
            true,
        ),
        "rhwp_apply_hidden_comment_para_format" => hidden_comment_schema(
            hidden_comment_target_props(vec![para_format_props_prop()]),
            vec!["para"],
            true,
        ),
        "rhwp_list_header_footers" => {
            session_schema(hf_optional_target_props(Vec::new()), Vec::new(), false)
        }
        "rhwp_get_header_footer" | "rhwp_create_header_footer"
        | "rhwp_delete_header_footer" => hf_schema(Vec::new(), Vec::new(), false),
        "rhwp_get_header_footer_para_info" | "rhwp_get_header_footer_para_format" => {
            hf_schema(hf_para_props(Vec::new()), Vec::new(), false)
        }
        "rhwp_insert_header_footer_text" => hf_schema(
            hf_para_props(vec![
                int_prop(
                    "char_offset",
                    "Character offset inside the header/footer paragraph.",
                ),
                string_prop("text", "Text to insert."),
            ]),
            vec!["char_offset", "text"],
            false,
        ),
        "rhwp_delete_header_footer_text" => hf_schema(
            hf_para_props(vec![
                int_prop(
                    "char_offset",
                    "Character offset inside the header/footer paragraph.",
                ),
                int_prop("count", "Number of characters to delete."),
            ]),
            vec!["char_offset", "count"],
            false,
        ),
        "rhwp_split_header_footer_paragraph" => hf_schema(
            hf_para_props(vec![int_prop(
                "char_offset",
                "Character offset inside the header/footer paragraph.",
            )]),
            vec!["char_offset"],
            false,
        ),
        "rhwp_merge_header_footer_paragraph" => {
            hf_schema(hf_para_props(Vec::new()), vec!["hf_para"], false)
        }
        "rhwp_apply_header_footer_para_format" => {
            hf_schema(hf_para_props(vec![para_format_props_prop()]), Vec::new(), true)
        }
        "rhwp_insert_header_footer_field" => hf_schema(
            hf_para_props(vec![
                int_prop(
                    "char_offset",
                    "Character offset inside the header/footer paragraph.",
                ),
                hf_field_type_prop(),
            ]),
            vec!["char_offset", "field_type"],
            false,
        ),
        "rhwp_apply_header_footer_template" => hf_schema(
            vec![hf_template_id_prop()],
            vec!["template_id"],
            false,
        ),
        "rhwp_get_note_info" => {
            session_schema(control_target_props(Vec::new()), vec!["para", "control"], false)
        }
        "rhwp_insert_note_text" => session_schema(
            note_target_props(vec![
                int_prop("char_offset", "Character offset inside the note paragraph."),
                string_prop("text", "Note text to insert."),
            ]),
            vec!["para", "control", "char_offset", "text"],
            false,
        ),
        "rhwp_delete_note_text" => session_schema(
            note_target_props(vec![
                int_prop("char_offset", "Character offset inside the note paragraph."),
                int_prop("count", "Number of characters to delete."),
            ]),
            vec!["para", "control", "char_offset", "count"],
            false,
        ),
        "rhwp_split_note_paragraph" => session_schema(
            note_target_props(vec![int_prop(
                "char_offset",
                "Character offset inside the note paragraph.",
            )]),
            vec!["para", "control", "char_offset"],
            false,
        ),
        "rhwp_merge_note_paragraph" => {
            session_schema(note_target_props(Vec::new()), vec!["para", "control"], false)
        }
        "rhwp_apply_note_char_format" => session_schema(
            note_target_props(vec![
                int_prop("start", "Start character offset inside the note paragraph."),
                int_prop("end", "End character offset inside the note paragraph."),
                char_format_props_prop(),
            ]),
            vec!["para", "control", "start", "end"],
            true,
        ),
        "rhwp_apply_note_para_format" => session_schema(
            note_target_props(vec![para_format_props_prop()]),
            vec!["para", "control"],
            true,
        ),
        "rhwp_get_page_def" | "rhwp_get_section_def" => {
            session_schema(vec![section_prop()], Vec::new(), false)
        }
        "rhwp_set_page_def" => {
            session_schema(vec![section_prop(), page_def_props_prop()], Vec::new(), true)
        }
        "rhwp_set_section_def" => {
            session_schema(vec![section_prop(), section_def_props_prop()], Vec::new(), true)
        }
        "rhwp_add_bookmark" => session_schema(
            text_position_props(vec![string_prop("name", "Bookmark name.")]),
            vec!["char_offset", "name"],
            false,
        ),
        "rhwp_rename_bookmark" => session_schema(
            control_target_props(vec![
                string_prop("new_name", "New bookmark name."),
                string_prop("newName", "CamelCase alias for new_name."),
            ]),
            vec!["para", "control"],
            false,
        ),
        "rhwp_delete_bookmark" => {
            session_schema(control_target_props(Vec::new()), vec!["para", "control"], false)
        }
        _ => object_schema(Vec::new(), Vec::new(), true),
    }
}

fn object_schema(props: Vec<SchemaProp>, required: Vec<&'static str>, additional: bool) -> Value {
    let properties = props
        .into_iter()
        .map(|(name, schema)| (name.to_string(), schema))
        .collect::<Map<_, _>>();
    let mut schema = Map::new();
    schema.insert("type".to_string(), json!("object"));
    schema.insert("properties".to_string(), Value::Object(properties));
    schema.insert("additionalProperties".to_string(), json!(additional));
    if !required.is_empty() {
        schema.insert("required".to_string(), json!(required));
    }
    Value::Object(schema)
}

fn session_schema(
    mut props: Vec<SchemaProp>,
    required: Vec<&'static str>,
    additional: bool,
) -> Value {
    props.insert(0, session_prop());
    let mut required_with_session = vec!["session_id"];
    required_with_session.extend(required);
    object_schema(props, required_with_session, additional)
}

fn hf_schema(extra: Vec<SchemaProp>, required: Vec<&'static str>, additional: bool) -> Value {
    with_one_of_required(
        session_schema(hf_target_props(extra), required, additional),
        vec![vec!["is_header"], vec!["isHeader"]],
    )
}

fn hidden_comment_schema(
    props: Vec<SchemaProp>,
    required: Vec<&'static str>,
    additional: bool,
) -> Value {
    with_one_of_required(
        session_schema(props, required, additional),
        vec![
            vec!["control"],
            vec!["cell_path", "inner_control"],
            vec!["cell_path", "innerControl"],
            vec!["table_path", "inner_control"],
            vec!["table_path", "innerControl"],
            vec!["tablePath", "inner_control"],
            vec!["tablePath", "innerControl"],
        ],
    )
}

fn compare_schema(props: Vec<SchemaProp>) -> Value {
    with_one_of_required(
        session_schema(props, Vec::new(), false),
        vec![
            vec!["other_session_id"],
            vec!["otherSessionId"],
            vec!["other_path"],
            vec!["path"],
        ],
    )
}

fn with_one_of_required(mut schema: Value, alternatives: Vec<Vec<&'static str>>) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.insert(
            "oneOf".to_string(),
            Value::Array(
                alternatives
                    .into_iter()
                    .map(|required| json!({ "required": required }))
                    .collect(),
            ),
        );
    }
    schema
}

fn session_prop() -> SchemaProp {
    string_prop(
        "session_id",
        "Session id returned by rhwp_open or rhwp_new.",
    )
}

fn page_prop() -> SchemaProp {
    int_prop("page", "Zero-based page index.")
}

fn section_prop() -> SchemaProp {
    int_prop("section", "Zero-based section index. Defaults to 0.")
}

fn section_u32_prop() -> SchemaProp {
    int_prop(
        "section",
        "Optional HWP section stream index for record comparison.",
    )
}

fn hf_optional_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = vec![
        section_prop(),
        bool_prop(
            "is_header",
            "Current header/footer selector. true for header, false for footer.",
        ),
        bool_prop("isHeader", "CamelCase alias for is_header."),
        hf_apply_to_prop("apply_to", "Header/footer apply target. Defaults to 0."),
        hf_apply_to_prop("applyTo", "CamelCase alias for apply_to."),
    ];
    props.extend(extra);
    props
}

fn hf_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = hf_optional_target_props(extra);
    if let Some((_, schema)) = props.iter_mut().find(|(name, _)| *name == "is_header") {
        if let Some(object) = schema.as_object_mut() {
            object.insert(
                "description".to_string(),
                json!("true targets a header, false targets a footer."),
            );
        }
    }
    props
}

fn hf_para_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = vec![
        int_prop("hf_para", "Header/footer paragraph index. Defaults to 0."),
        int_prop("hfPara", "CamelCase alias for hf_para."),
        int_prop("hf_para_idx", "Alias for hf_para."),
        int_prop("hfParaIndex", "CamelCase alias for hf_para_idx."),
    ];
    props.extend(extra);
    props
}

fn para_prop() -> SchemaProp {
    int_prop("para", "Zero-based paragraph index.")
}

fn hf_apply_to_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "integer",
            "enum": [0, 1, 2],
            "description": format!("{description} 0=both pages, 1=even pages, 2=odd pages.")
        }),
    )
}

fn hf_field_type_prop() -> SchemaProp {
    (
        "field_type",
        json!({
            "type": "integer",
            "enum": [1, 2, 3],
            "description": "Header/footer field marker. 1=current page, 2=total pages, 3=file name."
        }),
    )
}

fn hf_template_id_prop() -> SchemaProp {
    (
        "template_id",
        json!({
            "type": "integer",
            "minimum": 0,
            "maximum": 10,
            "description": "Built-in header/footer template id. 0=empty, 1/2/3=page number left/center/right, 4/5=page+file combinations, 6-10=styled variants."
        }),
    )
}

fn format_prop() -> SchemaProp {
    (
        "format",
        json!({
            "type": "string",
            "enum": ["hwp", "hwpx"],
            "description": "Export/save format. Defaults to hwp."
        }),
    )
}

fn shape_type_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "string",
            "enum": [
                "rectangle",
                "textbox",
                "ellipse",
                "line",
                "polygon",
                "arc",
                "connector-straight",
                "connector-stroke",
                "connector-arc",
                "connector-straight-arrow",
                "connector-stroke-arrow",
                "connector-arc-arrow"
            ],
            "description": description
        }),
    )
}

fn text_wrap_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "string",
            "enum": ["Square", "Tight", "Through", "TopAndBottom", "BehindText", "InFrontOfText"],
            "description": description
        }),
    )
}

fn shape_z_order_operation_prop() -> SchemaProp {
    (
        "operation",
        json!({
            "type": "string",
            "enum": ["front", "back", "forward", "backward"],
            "description": "Z-order operation for the target shape."
        }),
    )
}

fn shape_group_targets_prop() -> SchemaProp {
    (
        "targets",
        json!({
            "type": "array",
            "minItems": 2,
            "description": "Shape/Picture targets in the same section. Body items may be {para, control} or [para, control]. Header/footer items may use edit_target objects with inner_para and inner_control, or [inner_para, inner_control].",
            "items": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "section": { "type": "integer", "minimum": 0 },
                            "para": { "type": "integer", "minimum": 0 },
                            "control": { "type": "integer", "minimum": 0 },
                            "container_scope": { "type": "string", "enum": ["header", "footer"] },
                            "containerScope": { "type": "string", "enum": ["header", "footer"] },
                            "inner_para": { "type": "integer", "minimum": 0 },
                            "innerPara": { "type": "integer", "minimum": 0 },
                            "hf_para": { "type": "integer", "minimum": 0 },
                            "hfPara": { "type": "integer", "minimum": 0 },
                            "hf_para_idx": { "type": "integer", "minimum": 0 },
                            "hfParaIndex": { "type": "integer", "minimum": 0 },
                            "inner_control": { "type": "integer", "minimum": 0 },
                            "innerControl": { "type": "integer", "minimum": 0 }
                        },
                        "anyOf": [
                            { "required": ["para", "control"] },
                            { "required": ["inner_control"] },
                            { "required": ["innerControl"] }
                        ],
                        "additionalProperties": false
                    },
                    {
                        "type": "array",
                        "prefixItems": [
                            { "type": "integer", "minimum": 0 },
                            { "type": "integer", "minimum": 0 }
                        ],
                        "minItems": 2,
                        "maxItems": 2
                    }
                ]
            }
        }),
    )
}

fn props_prop() -> SchemaProp {
    (
        "props",
        json!({
            "type": ["object", "string"],
            "description": "Property bag passed to the underlying DocumentCore formatter."
        }),
    )
}

fn table_props_prop() -> SchemaProp {
    (
        "props",
        json!({
            "type": ["object", "string"],
            "description": "Body table property bag passed to DocumentCore. Known fields match rhwp_get_table_properties; snake_case aliases are accepted for MCP clients.",
            "properties": {
                "cellSpacing": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "cell_spacing": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "paddingLeft": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "padding_left": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "paddingRight": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "padding_right": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "paddingTop": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "padding_top": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "paddingBottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "padding_bottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "pageBreak": { "type": "integer", "enum": [0, 1, 2] },
                "page_break": { "type": "integer", "enum": [0, 1, 2] },
                "repeatHeader": { "type": "boolean" },
                "repeat_header": { "type": "boolean" },
                "treatAsChar": { "type": "boolean" },
                "treat_as_char": { "type": "boolean" },
                "textWrap": { "type": "string", "enum": ["Square", "TopAndBottom", "BehindText", "InFrontOfText"] },
                "text_wrap": { "type": "string", "enum": ["Square", "TopAndBottom", "BehindText", "InFrontOfText"] },
                "textFlow": { "type": "string", "enum": ["BothSides", "LeftOnly", "RightOnly", "LargestOnly"] },
                "text_flow": { "type": "string", "enum": ["BothSides", "LeftOnly", "RightOnly", "LargestOnly"] },
                "numberingType": { "type": "string", "enum": ["None", "Picture", "Table", "Equation"] },
                "numbering_type": { "type": "string", "enum": ["None", "Picture", "Table", "Equation"] },
                "lock": { "type": "boolean" },
                "locked": { "type": "boolean" },
                "dropcapStyle": { "type": "string" },
                "dropcap_style": { "type": "string" },
                "vertRelTo": { "type": "string", "enum": ["Paper", "Page", "Para"] },
                "vert_rel_to": { "type": "string", "enum": ["Paper", "Page", "Para"] },
                "vertAlign": { "type": "string", "enum": ["Top", "Center", "Bottom", "Inside", "Outside"] },
                "vert_align": { "type": "string", "enum": ["Top", "Center", "Bottom", "Inside", "Outside"] },
                "horzRelTo": { "type": "string", "enum": ["Paper", "Page", "Column", "Para"] },
                "horz_rel_to": { "type": "string", "enum": ["Paper", "Page", "Column", "Para"] },
                "horzAlign": { "type": "string", "enum": ["Left", "Center", "Right", "Inside", "Outside"] },
                "horz_align": { "type": "string", "enum": ["Left", "Center", "Right", "Inside", "Outside"] },
                "vertOffset": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "vert_offset": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "horzOffset": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "horz_offset": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "restrictInPage": { "type": "boolean" },
                "restrict_in_page": { "type": "boolean" },
                "allowOverlap": { "type": "boolean" },
                "allow_overlap": { "type": "boolean" },
                "keepWithAnchor": { "type": "boolean" },
                "keep_with_anchor": { "type": "boolean" },
                "outerLeft": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_left": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerRight": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_right": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerTop": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_top": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerBottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_bottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "hasCaption": { "type": "boolean" },
                "has_caption": { "type": "boolean" },
                "captionDirection": { "type": "integer", "enum": [0, 1, 2, 3] },
                "caption_direction": { "type": "integer", "enum": [0, 1, 2, 3] },
                "captionVertAlign": { "type": "integer", "enum": [0, 1, 2] },
                "caption_vert_align": { "type": "integer", "enum": [0, 1, 2] },
                "captionWidth": integer_schema(0, u32::MAX as i64),
                "caption_width": integer_schema(0, u32::MAX as i64),
                "captionSpacing": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "caption_spacing": integer_schema(i16::MIN as i64, i16::MAX as i64)
            },
            "additionalProperties": true
        }),
    )
}

fn equation_props_prop() -> SchemaProp {
    (
        "props",
        json!({
            "type": ["object", "string"],
            "description": "Equation property bag passed to DocumentCore. Known fields match rhwp_get_equation_properties and set_equation_properties; extra fields are ignored by the current core.",
            "properties": {
                "script": { "type": "string" },
                "fontSize": integer_schema(0, u32::MAX as i64),
                "font_size": integer_schema(0, u32::MAX as i64),
                "color": integer_schema(0, u32::MAX as i64),
                "baseline": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "fontName": { "type": "string" },
                "font_name": { "type": "string" },
                "lineMode": { "type": "string" },
                "line_mode": { "type": "string" },
                "width": integer_schema(0, u32::MAX as i64),
                "height": integer_schema(0, u32::MAX as i64),
                "treatAsChar": { "type": "boolean" },
                "treat_as_char": { "type": "boolean" },
                "vertRelTo": { "type": "string", "enum": ["Paper", "Page", "Para"] },
                "vert_rel_to": { "type": "string", "enum": ["Paper", "Page", "Para"] },
                "horzRelTo": { "type": "string", "enum": ["Paper", "Page", "Column", "Para"] },
                "horz_rel_to": { "type": "string", "enum": ["Paper", "Page", "Column", "Para"] },
                "vertAlign": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "vert_align": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "horzAlign": { "type": "string", "enum": ["Left", "Center", "Right"] },
                "horz_align": { "type": "string", "enum": ["Left", "Center", "Right"] },
                "textWrap": { "type": "string", "enum": ["Square", "Tight", "Through", "TopAndBottom", "BehindText", "InFrontOfText"] },
                "text_wrap": { "type": "string", "enum": ["Square", "Tight", "Through", "TopAndBottom", "BehindText", "InFrontOfText"] },
                "restrictInPage": { "type": "boolean" },
                "restrict_in_page": { "type": "boolean" },
                "allowOverlap": { "type": "boolean" },
                "allow_overlap": { "type": "boolean" },
                "sizeProtect": { "type": "boolean" },
                "size_protect": { "type": "boolean" },
                "vertOffset": integer_schema(0, u32::MAX as i64),
                "vert_offset": integer_schema(0, u32::MAX as i64),
                "horzOffset": integer_schema(0, u32::MAX as i64),
                "horz_offset": integer_schema(0, u32::MAX as i64),
                "description": { "type": "string" },
                "outerMarginLeft": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_left": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginTop": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_top": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginRight": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_right": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginBottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_bottom": integer_schema(i16::MIN as i64, i16::MAX as i64)
            },
            "additionalProperties": true
        }),
    )
}

fn page_def_props_prop() -> SchemaProp {
    (
        "props",
        json!({
            "type": ["object", "string"],
            "description": "Page definition property bag passed to DocumentCore. Known fields match rhwp_get_page_def and set_page_def_native; extra fields are ignored by the current core.",
            "properties": {
                "width": integer_schema(0, u32::MAX as i64),
                "height": integer_schema(0, u32::MAX as i64),
                "marginLeft": integer_schema(0, u32::MAX as i64),
                "margin_left": integer_schema(0, u32::MAX as i64),
                "marginRight": integer_schema(0, u32::MAX as i64),
                "margin_right": integer_schema(0, u32::MAX as i64),
                "marginTop": integer_schema(0, u32::MAX as i64),
                "margin_top": integer_schema(0, u32::MAX as i64),
                "marginBottom": integer_schema(0, u32::MAX as i64),
                "margin_bottom": integer_schema(0, u32::MAX as i64),
                "marginHeader": integer_schema(0, u32::MAX as i64),
                "margin_header": integer_schema(0, u32::MAX as i64),
                "marginFooter": integer_schema(0, u32::MAX as i64),
                "margin_footer": integer_schema(0, u32::MAX as i64),
                "marginGutter": integer_schema(0, u32::MAX as i64),
                "margin_gutter": integer_schema(0, u32::MAX as i64),
                "landscape": { "type": "boolean" },
                "binding": {
                    "type": "integer",
                    "enum": [0, 1, 2],
                    "description": "0=SingleSided, 1=DuplexSided, 2=TopFlip."
                }
            },
            "additionalProperties": true
        }),
    )
}

fn section_def_props_prop() -> SchemaProp {
    (
        "props",
        json!({
            "type": ["object", "string"],
            "description": "Section definition property bag passed to DocumentCore. Known fields match rhwp_get_section_def and set_section_def_native; extra fields are ignored by the current core.",
            "properties": {
                "sectionId": {
                    "type": "string",
                    "description": "HWPX hp:secPr@id value preserved for section-definition roundtrips."
                },
                "section_id": {
                    "type": "string",
                    "description": "Snake_case alias for sectionId."
                },
                "pageNum": integer_schema(0, u16::MAX as i64),
                "page_num": integer_schema(0, u16::MAX as i64),
                "pageNumType": {
                    "type": "integer",
                    "enum": [0, 1, 2, 3],
                    "description": "Section page-numbering type stored in SectionDef flags bits 20-21."
                },
                "page_num_type": {
                    "type": "integer",
                    "enum": [0, 1, 2, 3],
                    "description": "Snake_case alias for pageNumType."
                },
                "pictureNum": integer_schema(0, u16::MAX as i64),
                "picture_num": integer_schema(0, u16::MAX as i64),
                "tableNum": integer_schema(0, u16::MAX as i64),
                "table_num": integer_schema(0, u16::MAX as i64),
                "equationNum": integer_schema(0, u16::MAX as i64),
                "equation_num": integer_schema(0, u16::MAX as i64),
                "columnSpacing": integer_schema(0, i16::MAX as i64),
                "column_spacing": integer_schema(0, i16::MAX as i64),
                "lineGrid": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "line_grid": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "charGrid": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "char_grid": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "wonggojiFormat": integer_schema(0, u8::MAX as i64),
                "wonggoji_format": integer_schema(0, u8::MAX as i64),
                "defaultTabSpacing": integer_schema(0, u32::MAX as i64),
                "default_tab_spacing": integer_schema(0, u32::MAX as i64),
                "tabStopVal": integer_schema(0, u32::MAX as i64),
                "tab_stop_val": integer_schema(0, u32::MAX as i64),
                "tabStopUnit": { "type": "string" },
                "tab_stop_unit": { "type": "string" },
                "textDirection": {
                    "type": "integer",
                    "enum": [0, 1, 2],
                    "description": "Section text direction: 0=horizontal, 1=vertical, 2=vertical all."
                },
                "text_direction": {
                    "type": "integer",
                    "enum": [0, 1, 2],
                    "description": "Snake_case alias for textDirection."
                },
                "outlineShapeIDRef": integer_schema(0, u16::MAX as i64),
                "outline_shape_id_ref": integer_schema(0, u16::MAX as i64),
                "outlineNumberingId": integer_schema(0, u16::MAX as i64),
                "outline_numbering_id": integer_schema(0, u16::MAX as i64),
                "memoShapeIDRef": integer_schema(0, u16::MAX as i64),
                "memo_shape_id_ref": integer_schema(0, u16::MAX as i64),
                "textVerticalWidthHead": integer_schema(0, u32::MAX as i64),
                "text_vertical_width_head": integer_schema(0, u32::MAX as i64),
                "hideHeader": { "type": "boolean" },
                "hide_header": { "type": "boolean" },
                "hideFooter": { "type": "boolean" },
                "hide_footer": { "type": "boolean" },
                "hideMasterPage": { "type": "boolean" },
                "hide_master_page": { "type": "boolean" },
                "hideBorder": { "type": "boolean" },
                "hide_border": { "type": "boolean" },
                "visibilityBorder": {
                    "type": "string",
                    "enum": ["HIDE_FIRST", "SHOW_FIRST", "SHOW_ALL"],
                    "description": "HWPX hp:visibility@border value. Takes precedence over hideBorder when both are supplied."
                },
                "visibility_border": {
                    "type": "string",
                    "enum": ["HIDE_FIRST", "SHOW_FIRST", "SHOW_ALL"],
                    "description": "Snake_case alias for visibilityBorder."
                },
                "hideFill": { "type": "boolean" },
                "hide_fill": { "type": "boolean" },
                "visibilityFill": {
                    "type": "string",
                    "enum": ["HIDE_FIRST", "SHOW_FIRST", "SHOW_ALL"],
                    "description": "HWPX hp:visibility@fill value. Takes precedence over hideFill when both are supplied."
                },
                "visibility_fill": {
                    "type": "string",
                    "enum": ["HIDE_FIRST", "SHOW_FIRST", "SHOW_ALL"],
                    "description": "Snake_case alias for visibilityFill."
                },
                "hidePageNumber": { "type": "boolean" },
                "hide_page_number": { "type": "boolean" },
                "hideFirstPageNum": { "type": "boolean" },
                "hide_first_page_num": { "type": "boolean" },
                "hideEmptyLine": { "type": "boolean" },
                "hide_empty_line": { "type": "boolean" },
                "showLineNumber": { "type": "boolean" },
                "show_line_number": { "type": "boolean" },
                "lineNumberRestartType": integer_schema(0, u8::MAX as i64),
                "line_number_restart_type": integer_schema(0, u8::MAX as i64),
                "lineNumberCountBy": integer_schema(0, u16::MAX as i64),
                "line_number_count_by": integer_schema(0, u16::MAX as i64),
                "lineNumberDistance": integer_schema(0, u32::MAX as i64),
                "line_number_distance": integer_schema(0, u32::MAX as i64),
                "lineNumberStartNumber": integer_schema(0, u16::MAX as i64),
                "line_number_start_number": integer_schema(0, u16::MAX as i64)
            },
            "additionalProperties": true
        }),
    )
}

fn picture_props_prop() -> SchemaProp {
    let effect = shape_effect_schema();
    let three_d = shape_three_d_effect_schema();
    (
        "props",
        json!({
            "type": ["object", "string"],
            "description": "Picture property bag passed to DocumentCore. Supports display/layout fields, image adjustments, caption fields, and HWPX picture effects under effects.* or top-level aliases.",
            "properties": {
                "width": integer_schema(0, u32::MAX as i64),
                "height": integer_schema(0, u32::MAX as i64),
                "treatAsChar": { "type": "boolean" },
                "treat_as_char": { "type": "boolean" },
                "vertRelTo": { "type": "string", "enum": ["Paper", "Page", "Para"] },
                "vert_rel_to": { "type": "string", "enum": ["Paper", "Page", "Para"] },
                "horzRelTo": { "type": "string", "enum": ["Paper", "Page", "Column", "Para"] },
                "horz_rel_to": { "type": "string", "enum": ["Paper", "Page", "Column", "Para"] },
                "vertAlign": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "vert_align": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "horzAlign": { "type": "string", "enum": ["Left", "Center", "Right"] },
                "horz_align": { "type": "string", "enum": ["Left", "Center", "Right"] },
                "textWrap": { "type": "string", "enum": ["Square", "Tight", "Through", "TopAndBottom", "BehindText", "InFrontOfText"] },
                "text_wrap": { "type": "string", "enum": ["Square", "Tight", "Through", "TopAndBottom", "BehindText", "InFrontOfText"] },
                "textFlow": { "type": "string", "enum": ["BothSides", "LeftOnly", "RightOnly", "LargestOnly"] },
                "text_flow": { "type": "string", "enum": ["BothSides", "LeftOnly", "RightOnly", "LargestOnly"] },
                "numberingType": { "type": "string", "enum": ["None", "Picture", "Table", "Equation"] },
                "numbering_type": { "type": "string", "enum": ["None", "Picture", "Table", "Equation"] },
                "numberingTypeExplicit": { "type": "boolean" },
                "numbering_type_explicit": { "type": "boolean" },
                "lock": { "type": "boolean" },
                "locked": { "type": "boolean" },
                "restrictInPage": { "type": "boolean" },
                "restrict_in_page": { "type": "boolean" },
                "allowOverlap": { "type": "boolean" },
                "allow_overlap": { "type": "boolean" },
                "sizeProtect": { "type": "boolean" },
                "size_protect": { "type": "boolean" },
                "widthCriterion": { "type": "string", "enum": ["Paper", "Page", "Column", "Para", "Absolute"] },
                "width_criterion": { "type": "string", "enum": ["Paper", "Page", "Column", "Para", "Absolute"] },
                "heightCriterion": { "type": "string", "enum": ["Paper", "Page", "Absolute"] },
                "height_criterion": { "type": "string", "enum": ["Paper", "Page", "Absolute"] },
                "dropcapStyle": { "type": "string" },
                "dropcap_style": { "type": "string" },
                "zOrder": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "z_order": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "instanceId": integer_schema(0, u32::MAX as i64),
                "instance_id": integer_schema(0, u32::MAX as i64),
                "instId": integer_schema(0, u32::MAX as i64),
                "inst_id": integer_schema(0, u32::MAX as i64),
                "pictureInstanceId": integer_schema(0, u32::MAX as i64),
                "picture_instance_id": integer_schema(0, u32::MAX as i64),
                "groupLevel": integer_schema(0, u16::MAX as i64),
                "group_level": integer_schema(0, u16::MAX as i64),
                "href": { "type": "string" },
                "vertOffset": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "vert_offset": integer_schema(0, u32::MAX as i64),
                "horzOffset": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "horz_offset": integer_schema(0, u32::MAX as i64),
                "brightness": integer_schema(i8::MIN as i64, i8::MAX as i64),
                "contrast": integer_schema(i8::MIN as i64, i8::MAX as i64),
                "transparency": integer_schema(0, 100),
                "effect": { "type": "string", "enum": ["RealPic", "GrayScale", "BlackWhite", "Pattern8x8"] },
                "rotationAngle": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "rotation_angle": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "rotateImage": { "type": "boolean" },
                "rotate_image": { "type": "boolean" },
                "horzFlip": { "type": "boolean" },
                "horz_flip": { "type": "boolean" },
                "vertFlip": { "type": "boolean" },
                "vert_flip": { "type": "boolean" },
                "cropLeft": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "crop_left": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "cropTop": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "crop_top": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "cropRight": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "crop_right": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "cropBottom": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "crop_bottom": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "paddingLeft": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "padding_left": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "paddingTop": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "padding_top": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "paddingRight": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "padding_right": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "paddingBottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "padding_bottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginLeft": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_left": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginTop": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_top": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginRight": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_right": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginBottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_bottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "borderColor": integer_schema(0, u32::MAX as i64),
                "border_color": integer_schema(0, u32::MAX as i64),
                "borderColorHex": css_hex_color_schema(),
                "border_color_hex": css_hex_color_schema(),
                "borderWidth": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "border_width": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "description": { "type": "string" },
                "hasCaption": { "type": "boolean" },
                "has_caption": { "type": "boolean" },
                "captionDirection": { "type": "string", "enum": ["Left", "Right", "Top", "Bottom"] },
                "caption_direction": { "type": "string", "enum": ["Left", "Right", "Top", "Bottom"] },
                "captionVertAlign": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "caption_vert_align": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "captionWidth": integer_schema(0, u32::MAX as i64),
                "caption_width": integer_schema(0, u32::MAX as i64),
                "captionSpacing": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "caption_spacing": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "captionIncludeMargin": { "type": "boolean" },
                "caption_include_margin": { "type": "boolean" },
                "captionText": { "type": "string" },
                "caption_text": { "type": "string" },
                "effects": picture_effects_schema(),
                "shadow": effect.clone(),
                "pictureShadow": effect.clone(),
                "picture_shadow": effect.clone(),
                "glow": effect.clone(),
                "pictureGlow": effect.clone(),
                "picture_glow": effect.clone(),
                "softEdge": effect.clone(),
                "soft_edge": effect.clone(),
                "pictureSoftEdge": effect.clone(),
                "picture_soft_edge": effect.clone(),
                "reflection": effect.clone(),
                "pictureReflection": effect.clone(),
                "picture_reflection": effect.clone(),
                "threeD": three_d.clone(),
                "three_d": three_d.clone(),
                "pictureThreeD": three_d.clone(),
                "picture_three_d": three_d,
                "blur": effect.clone(),
                "pictureBlur": effect.clone(),
                "picture_blur": effect.clone(),
                "fillOverlay": effect.clone(),
                "fill_overlay": effect.clone(),
                "pictureFillOverlay": effect.clone(),
                "picture_fill_overlay": effect,
                "effectsRawXml": raw_xml_value_schema(),
                "rawEffectsXml": raw_xml_value_schema(),
                "effects_raw_xml": raw_xml_value_schema()
            },
            "additionalProperties": true
        }),
    )
}

fn picture_effects_schema() -> Value {
    let effect = shape_effect_schema();
    let three_d = shape_three_d_effect_schema();
    json!({
        "type": "object",
        "description": "HWPX effect fragments for pictures. Individual effects may be set to null to clear them.",
        "properties": {
            "shadow": effect.clone(),
            "pictureShadow": effect.clone(),
            "picture_shadow": effect.clone(),
            "glow": effect.clone(),
            "pictureGlow": effect.clone(),
            "picture_glow": effect.clone(),
            "softEdge": effect.clone(),
            "soft_edge": effect.clone(),
            "pictureSoftEdge": effect.clone(),
            "picture_soft_edge": effect.clone(),
            "reflection": effect.clone(),
            "pictureReflection": effect.clone(),
            "picture_reflection": effect.clone(),
            "threeD": three_d.clone(),
            "three_d": three_d.clone(),
            "pictureThreeD": three_d.clone(),
            "picture_three_d": three_d,
            "blur": effect.clone(),
            "pictureBlur": effect.clone(),
            "picture_blur": effect.clone(),
            "fillOverlay": effect.clone(),
            "fill_overlay": effect.clone(),
            "pictureFillOverlay": effect.clone(),
            "picture_fill_overlay": effect.clone(),
            "rawXml": raw_xml_value_schema(),
            "raw_xml": raw_xml_value_schema(),
            "effectsRawXml": raw_xml_value_schema()
        },
        "additionalProperties": true
    })
}

fn shape_props_prop() -> SchemaProp {
    let effect = shape_effect_schema();
    let three_d = shape_three_d_effect_schema();
    (
        "props",
        json!({
            "type": ["object", "string"],
            "description": "Shape property bag passed to DocumentCore. Top-level shadow edits classic DrawingObjAttr shadow metadata; HWPX effect fragments are exposed under effects.* or the top-level effect aliases.",
            "properties": {
                "width": integer_schema(0, u32::MAX as i64),
                "height": integer_schema(0, u32::MAX as i64),
                "treatAsChar": { "type": "boolean" },
                "treat_as_char": { "type": "boolean" },
                "vertRelTo": { "type": "string", "enum": ["Paper", "Page", "Para"] },
                "vert_rel_to": { "type": "string", "enum": ["Paper", "Page", "Para"] },
                "horzRelTo": { "type": "string", "enum": ["Paper", "Page", "Column", "Para"] },
                "horz_rel_to": { "type": "string", "enum": ["Paper", "Page", "Column", "Para"] },
                "vertAlign": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "vert_align": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "horzAlign": { "type": "string", "enum": ["Left", "Center", "Right"] },
                "horz_align": { "type": "string", "enum": ["Left", "Center", "Right"] },
                "textWrap": { "type": "string", "enum": ["Square", "Tight", "Through", "TopAndBottom", "BehindText", "InFrontOfText"] },
                "text_wrap": { "type": "string", "enum": ["Square", "Tight", "Through", "TopAndBottom", "BehindText", "InFrontOfText"] },
                "textFlow": { "type": "string", "enum": ["BothSides", "LeftOnly", "RightOnly", "LargestOnly"] },
                "text_flow": { "type": "string", "enum": ["BothSides", "LeftOnly", "RightOnly", "LargestOnly"] },
                "numberingType": { "type": "string", "enum": ["None", "Picture", "Table", "Equation"] },
                "numbering_type": { "type": "string", "enum": ["None", "Picture", "Table", "Equation"] },
                "numberingTypeExplicit": { "type": "boolean" },
                "numbering_type_explicit": { "type": "boolean" },
                "lock": { "type": "boolean" },
                "locked": { "type": "boolean" },
                "restrictInPage": { "type": "boolean" },
                "restrict_in_page": { "type": "boolean" },
                "allowOverlap": { "type": "boolean" },
                "allow_overlap": { "type": "boolean" },
                "sizeProtect": { "type": "boolean" },
                "size_protect": { "type": "boolean" },
                "widthCriterion": { "type": "string", "enum": ["Paper", "Page", "Column", "Para", "Absolute"] },
                "width_criterion": { "type": "string", "enum": ["Paper", "Page", "Column", "Para", "Absolute"] },
                "heightCriterion": { "type": "string", "enum": ["Paper", "Page", "Absolute"] },
                "height_criterion": { "type": "string", "enum": ["Paper", "Page", "Absolute"] },
                "dropcapStyle": { "type": "string" },
                "dropcap_style": { "type": "string" },
                "zOrder": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "z_order": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "instanceId": integer_schema(0, u32::MAX as i64),
                "instance_id": integer_schema(0, u32::MAX as i64),
                "instId": integer_schema(0, u32::MAX as i64),
                "inst_id": integer_schema(0, u32::MAX as i64),
                "groupLevel": integer_schema(0, u16::MAX as i64),
                "group_level": integer_schema(0, u16::MAX as i64),
                "binDataId": integer_schema(0, u32::MAX as i64),
                "bin_data_id": integer_schema(0, u32::MAX as i64),
                "binaryItemId": integer_schema(0, u32::MAX as i64),
                "binary_item_id": integer_schema(0, u32::MAX as i64),
                "extentX": integer_schema(0, i32::MAX as i64),
                "extent_x": integer_schema(0, i32::MAX as i64),
                "extentY": integer_schema(0, i32::MAX as i64),
                "extent_y": integer_schema(0, i32::MAX as i64),
                "objectType": { "type": "string" },
                "object_type": { "type": "string" },
                "drawAspect": { "type": "string" },
                "draw_aspect": { "type": "string" },
                "eqBaseLine": { "type": "string" },
                "eq_base_line": { "type": "string" },
                "hasMoniker": { "type": "string" },
                "has_moniker": { "type": "string" },
                "href": { "type": "string" },
                "vertOffset": integer_schema(0, u32::MAX as i64),
                "vert_offset": integer_schema(0, u32::MAX as i64),
                "horzOffset": integer_schema(0, u32::MAX as i64),
                "horz_offset": integer_schema(0, u32::MAX as i64),
                "outerMarginLeft": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_left": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginTop": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_top": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginRight": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_right": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outerMarginBottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "outer_margin_bottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "description": { "type": "string" },
                "hasCaption": { "type": "boolean" },
                "has_caption": { "type": "boolean" },
                "captionDirection": { "type": "string", "enum": ["Left", "Right", "Top", "Bottom"] },
                "caption_direction": { "type": "string", "enum": ["Left", "Right", "Top", "Bottom"] },
                "captionVertAlign": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "caption_vert_align": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "captionWidth": integer_schema(0, u32::MAX as i64),
                "caption_width": integer_schema(0, u32::MAX as i64),
                "captionSpacing": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "caption_spacing": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "captionMaxWidth": integer_schema(0, u32::MAX as i64),
                "caption_max_width": integer_schema(0, u32::MAX as i64),
                "captionIncludeMargin": { "type": "boolean" },
                "caption_include_margin": { "type": "boolean" },
                "captionText": { "type": "string" },
                "caption_text": { "type": "string" },
                "borderColor": integer_schema(0, u32::MAX as i64),
                "border_color": integer_schema(0, u32::MAX as i64),
                "borderColorHex": css_hex_color_schema(),
                "border_color_hex": css_hex_color_schema(),
                "borderWidth": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "border_width": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "borderAttr": integer_schema(0, u32::MAX as i64),
                "border_attr": integer_schema(0, u32::MAX as i64),
                "borderOutlineStyle": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "border_outline_style": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "lineType": integer_schema(0, 0x3F),
                "line_type": integer_schema(0, 0x3F),
                "lineEndShape": integer_schema(0, 0x0F),
                "line_end_shape": integer_schema(0, 0x0F),
                "arrowStart": integer_schema(0, 0x3F),
                "arrow_start": integer_schema(0, 0x3F),
                "arrowEnd": integer_schema(0, 0x3F),
                "arrow_end": integer_schema(0, 0x3F),
                "arrowStartSize": integer_schema(0, 0x0F),
                "arrow_start_size": integer_schema(0, 0x0F),
                "arrowEndSize": integer_schema(0, 0x0F),
                "arrow_end_size": integer_schema(0, 0x0F),
                "rotationAngle": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "rotation_angle": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "rotateImage": { "type": "boolean" },
                "rotate_image": { "type": "boolean" },
                "horzFlip": { "type": "boolean" },
                "horz_flip": { "type": "boolean" },
                "vertFlip": { "type": "boolean" },
                "vert_flip": { "type": "boolean" },
                "fillType": { "type": "string", "enum": ["none", "solid", "gradient", "image"] },
                "fill_type": { "type": "string", "enum": ["none", "solid", "gradient", "image"] },
                "fillBgColor": integer_schema(0, u32::MAX as i64),
                "fill_bg_color": integer_schema(0, u32::MAX as i64),
                "fillBgColorHex": css_hex_color_schema(),
                "fill_bg_color_hex": css_hex_color_schema(),
                "fillPatColor": integer_schema(0, u32::MAX as i64),
                "fill_pat_color": integer_schema(0, u32::MAX as i64),
                "fillPatColorHex": css_hex_color_schema(),
                "fill_pat_color_hex": css_hex_color_schema(),
                "fillPatType": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "fill_pat_type": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "fillAlpha": integer_schema(0, u8::MAX as i64),
                "fill_alpha": integer_schema(0, u8::MAX as i64),
                "gradientType": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradient_type": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradientAngle": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradient_angle": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradientCenterX": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradient_center_x": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradientCenterY": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradient_center_y": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradientBlur": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "gradient_blur": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "shadowType": integer_schema(0, u32::MAX as i64),
                "shadow_type": integer_schema(0, u32::MAX as i64),
                "shadowColor": integer_schema(0, u32::MAX as i64),
                "shadow_color": integer_schema(0, u32::MAX as i64),
                "shadowColorHex": css_hex_color_schema(),
                "shadow_color_hex": css_hex_color_schema(),
                "shadowOffsetX": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "shadow_offset_x": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "shadowOffsetY": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "shadow_offset_y": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "shadowAlpha": integer_schema(0, u8::MAX as i64),
                "shadow_alpha": integer_schema(0, u8::MAX as i64),
                "shadow": shape_shadow_schema(),
                "tbMarginLeft": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "tb_margin_left": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "tbMarginRight": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "tb_margin_right": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "tbMarginTop": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "tb_margin_top": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "tbMarginBottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "tb_margin_bottom": integer_schema(i16::MIN as i64, i16::MAX as i64),
                "tbVerticalAlign": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "tb_vertical_align": { "type": "string", "enum": ["Top", "Center", "Bottom"] },
                "roundRate": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "round_rate": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "connectorType": integer_schema(0, u32::MAX as i64),
                "connector_type": integer_schema(0, u32::MAX as i64),
                "connectorMidX": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "connector_mid_x": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "connectorMidY": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "connector_mid_y": integer_schema(i32::MIN as i64, i32::MAX as i64),
                "rawHwpxChildXml": raw_xml_value_schema(),
                "raw_hwpx_child_xml": raw_xml_value_schema(),
                "shapeRawXml": raw_xml_value_schema(),
                "shape_raw_xml": raw_xml_value_schema(),
                "effects": shape_effects_schema(),
                "threeD": three_d.clone(),
                "three_d": three_d.clone(),
                "shapeThreeD": three_d.clone(),
                "shape_three_d": three_d,
                "effectShadow": effect.clone(),
                "effect_shadow": effect.clone(),
                "shapeEffectShadow": effect.clone(),
                "shape_effect_shadow": effect.clone(),
                "glow": effect.clone(),
                "shapeGlow": effect.clone(),
                "shape_glow": effect.clone(),
                "softEdge": effect.clone(),
                "soft_edge": effect.clone(),
                "shapeSoftEdge": effect.clone(),
                "shape_soft_edge": effect.clone(),
                "reflection": effect.clone(),
                "shapeReflection": effect.clone(),
                "shape_reflection": effect.clone(),
                "blur": effect.clone(),
                "shapeBlur": effect.clone(),
                "shape_blur": effect.clone(),
                "fillOverlay": effect.clone(),
                "fill_overlay": effect.clone(),
                "shapeFillOverlay": effect.clone(),
                "shape_fill_overlay": effect
            },
            "additionalProperties": true
        }),
    )
}

fn shape_shadow_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "description": "Classic shape shadow metadata. Use effects.shadow or effectShadow for HWPX effect shadow XML.",
        "properties": {
            "type": integer_schema(0, u32::MAX as i64),
            "shadowType": integer_schema(0, u32::MAX as i64),
            "typeName": { "type": "string" },
            "type_name": { "type": "string" },
            "color": integer_schema(0, u32::MAX as i64),
            "shadowColor": integer_schema(0, u32::MAX as i64),
            "colorHex": css_hex_color_schema(),
            "color_hex": css_hex_color_schema(),
            "offsetX": integer_schema(i32::MIN as i64, i32::MAX as i64),
            "offset_x": integer_schema(i32::MIN as i64, i32::MAX as i64),
            "shadowOffsetX": integer_schema(i32::MIN as i64, i32::MAX as i64),
            "offsetY": integer_schema(i32::MIN as i64, i32::MAX as i64),
            "offset_y": integer_schema(i32::MIN as i64, i32::MAX as i64),
            "shadowOffsetY": integer_schema(i32::MIN as i64, i32::MAX as i64),
            "alpha": integer_schema(0, u8::MAX as i64),
            "shadowAlpha": integer_schema(0, u8::MAX as i64)
        },
        "additionalProperties": true
    })
}

fn shape_effects_schema() -> Value {
    let effect = shape_effect_schema();
    let three_d = shape_three_d_effect_schema();
    json!({
        "type": "object",
        "description": "HWPX effect fragments for ordinary drawing shapes. Individual effects may be set to null to clear them.",
        "properties": {
            "threeD": three_d.clone(),
            "three_d": three_d.clone(),
            "shapeThreeD": three_d.clone(),
            "shape_three_d": three_d,
            "shadow": effect.clone(),
            "effectShadow": effect.clone(),
            "effect_shadow": effect.clone(),
            "shapeEffectShadow": effect.clone(),
            "shape_effect_shadow": effect.clone(),
            "glow": effect.clone(),
            "shapeGlow": effect.clone(),
            "shape_glow": effect.clone(),
            "softEdge": effect.clone(),
            "soft_edge": effect.clone(),
            "shapeSoftEdge": effect.clone(),
            "shape_soft_edge": effect.clone(),
            "reflection": effect.clone(),
            "shapeReflection": effect.clone(),
            "shape_reflection": effect.clone(),
            "blur": effect.clone(),
            "shapeBlur": effect.clone(),
            "shape_blur": effect.clone(),
            "fillOverlay": effect.clone(),
            "fill_overlay": effect.clone(),
            "shapeFillOverlay": effect.clone(),
            "shape_fill_overlay": effect.clone(),
            "rawHwpxChildXml": raw_xml_value_schema(),
            "raw_hwpx_child_xml": raw_xml_value_schema(),
            "rawXml": raw_xml_value_schema(),
            "raw_xml": raw_xml_value_schema()
        },
        "additionalProperties": true
    })
}

fn shape_three_d_effect_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "properties": {
            "depth": string_or_integer_schema(),
            "bevel": {
                "type": ["object", "string"],
                "properties": {
                    "type": { "type": "string" }
                },
                "additionalProperties": true
            },
            "bevelType": { "type": "string" },
            "bevel_type": { "type": "string" },
            "rawChildXml": raw_xml_value_schema(),
            "raw_child_xml": raw_xml_value_schema(),
            "rawChildrenXml": raw_xml_value_schema(),
            "raw_children_xml": raw_xml_value_schema()
        },
        "additionalProperties": true
    })
}

fn shape_effect_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "properties": {
            "radius": string_or_integer_schema(),
            "direction": string_or_integer_schema(),
            "distance": string_or_integer_schema(),
            "style": { "type": "string" },
            "alignStyle": { "type": "string" },
            "rotationStyle": string_or_integer_schema(),
            "rotation_style": string_or_integer_schema(),
            "fadeDirection": string_or_integer_schema(),
            "fade_direction": string_or_integer_schema(),
            "alpha": {
                "type": ["object", "string", "integer"],
                "properties": {
                    "start": string_or_integer_schema(),
                    "end": string_or_integer_schema()
                },
                "additionalProperties": true
            },
            "blend": { "type": "string" },
            "color": shape_effect_color_schema(),
            "effectsColor": shape_effect_color_schema(),
            "colorHex": css_hex_color_schema(),
            "color_hex": css_hex_color_schema(),
            "rgbHex": css_hex_color_schema(),
            "rgb_hex": css_hex_color_schema(),
            "rgb": rgb_schema(),
            "colorType": { "type": "string" },
            "color_type": { "type": "string" },
            "schemeIdx": string_or_integer_schema(),
            "scheme_idx": string_or_integer_schema(),
            "systemIdx": string_or_integer_schema(),
            "system_idx": string_or_integer_schema(),
            "presetIdx": string_or_integer_schema(),
            "preset_idx": string_or_integer_schema(),
            "skew": shape_effect_attr_object_schema(),
            "skewX": string_or_integer_schema(),
            "skew_x": string_or_integer_schema(),
            "skewY": string_or_integer_schema(),
            "skew_y": string_or_integer_schema(),
            "scale": shape_effect_attr_object_schema(),
            "scaleX": string_or_integer_schema(),
            "scale_x": string_or_integer_schema(),
            "scaleY": string_or_integer_schema(),
            "scale_y": string_or_integer_schema(),
            "pos": shape_effect_attr_object_schema(),
            "alphaStart": string_or_integer_schema(),
            "alpha_start": string_or_integer_schema(),
            "alphaEnd": string_or_integer_schema(),
            "alpha_end": string_or_integer_schema(),
            "posStart": string_or_integer_schema(),
            "pos_start": string_or_integer_schema(),
            "posEnd": string_or_integer_schema(),
            "pos_end": string_or_integer_schema(),
            "solidFill": shape_solid_fill_schema(),
            "solid_fill": shape_solid_fill_schema(),
            "rawChildXml": raw_xml_value_schema(),
            "raw_child_xml": raw_xml_value_schema(),
            "rawChildrenXml": raw_xml_value_schema(),
            "raw_children_xml": raw_xml_value_schema()
        },
        "additionalProperties": true
    })
}

fn shape_solid_fill_schema() -> Value {
    json!({
        "type": ["object", "string"],
        "properties": {
            "color": shape_effect_color_schema(),
            "effectsColor": shape_effect_color_schema(),
            "colorHex": css_hex_color_schema(),
            "color_hex": css_hex_color_schema(),
            "rawChildXml": raw_xml_value_schema(),
            "raw_child_xml": raw_xml_value_schema(),
            "rawChildrenXml": raw_xml_value_schema(),
            "raw_children_xml": raw_xml_value_schema()
        },
        "additionalProperties": true
    })
}

fn shape_effect_color_schema() -> Value {
    json!({
        "type": ["object", "string"],
        "properties": {
            "type": { "type": "string" },
            "colorType": { "type": "string" },
            "color_type": { "type": "string" },
            "schemeIdx": string_or_integer_schema(),
            "scheme_idx": string_or_integer_schema(),
            "systemIdx": string_or_integer_schema(),
            "system_idx": string_or_integer_schema(),
            "presetIdx": string_or_integer_schema(),
            "preset_idx": string_or_integer_schema(),
            "rgb": rgb_schema(),
            "colorHex": css_hex_color_schema(),
            "color_hex": css_hex_color_schema(),
            "rgbHex": css_hex_color_schema(),
            "rgb_hex": css_hex_color_schema(),
            "rawChildXml": raw_xml_value_schema(),
            "raw_child_xml": raw_xml_value_schema(),
            "rawChildrenXml": raw_xml_value_schema(),
            "raw_children_xml": raw_xml_value_schema()
        },
        "additionalProperties": true
    })
}

fn shape_effect_attr_object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "x": string_or_integer_schema(),
            "y": string_or_integer_schema(),
            "start": string_or_integer_schema(),
            "end": string_or_integer_schema()
        },
        "additionalProperties": true
    })
}

fn rgb_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "r": string_or_integer_schema(),
            "g": string_or_integer_schema(),
            "b": string_or_integer_schema()
        },
        "additionalProperties": true
    })
}

fn raw_xml_value_schema() -> Value {
    json!({
        "anyOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "string" } },
            { "type": "null" }
        ]
    })
}

fn string_or_integer_schema() -> Value {
    json!({ "type": ["string", "integer"] })
}

fn char_format_props_prop() -> SchemaProp {
    style_char_format_prop(
        "props",
        "Character formatter property bag. Known fields are type/range/enum validated.",
    )
}

fn para_format_props_prop() -> SchemaProp {
    style_para_format_prop(
        "props",
        "Paragraph formatter property bag. Known fields are type/range/enum validated.",
    )
}

fn style_char_format_prop(name: &'static str, description: &'static str) -> SchemaProp {
    let mut properties = Map::new();
    for key in [
        "bold",
        "italic",
        "underline",
        "strikethrough",
        "subscript",
        "superscript",
        "emboss",
        "engrave",
        "kerning",
    ] {
        properties.insert(key.to_string(), json!({ "type": "boolean" }));
    }
    properties.insert(
        "fontSize".to_string(),
        integer_schema(i32::MIN as i64, i32::MAX as i64),
    );
    properties.insert("fontId".to_string(), integer_schema(0, u16::MAX as i64));
    for key in [
        "outlineType",
        "shadowType",
        "emphasisDot",
        "underlineShape",
        "strikeShape",
    ] {
        properties.insert(key.to_string(), integer_schema(0, u8::MAX as i64));
    }
    properties.insert(
        "underlineType".to_string(),
        json!({ "type": "string", "enum": ["Bottom", "Top", "None"] }),
    );
    properties.insert(
        "shadowOffsetX".to_string(),
        integer_schema(i8::MIN as i64, i8::MAX as i64),
    );
    properties.insert(
        "shadowOffsetY".to_string(),
        integer_schema(i8::MIN as i64, i8::MAX as i64),
    );
    for key in [
        "textColor",
        "shadeColor",
        "underlineColor",
        "shadowColor",
        "strikeColor",
    ] {
        properties.insert(key.to_string(), css_hex_color_schema());
    }
    add_border_fill_schema_props(&mut properties);
    properties.insert(
        "fontIds".to_string(),
        integer_array_schema(7, 0, u16::MAX as i64),
    );
    properties.insert(
        "ratios".to_string(),
        integer_array_schema(7, 0, u8::MAX as i64),
    );
    properties.insert(
        "spacings".to_string(),
        integer_array_schema(7, i8::MIN as i64, i8::MAX as i64),
    );
    properties.insert(
        "relativeSizes".to_string(),
        integer_array_schema(7, 0, u8::MAX as i64),
    );
    properties.insert(
        "charOffsets".to_string(),
        integer_array_schema(7, i8::MIN as i64, i8::MAX as i64),
    );

    (
        name,
        json!({
            "type": ["object", "string"],
            "description": description,
            "properties": properties,
            "additionalProperties": true
        }),
    )
}

fn style_para_format_prop(name: &'static str, description: &'static str) -> SchemaProp {
    let mut properties = Map::new();
    properties.insert(
        "alignment".to_string(),
        json!({
            "type": "string",
            "enum": ["left", "right", "center", "justify", "distribute"]
        }),
    );
    properties.insert(
        "lineSpacingType".to_string(),
        json!({
            "type": "string",
            "enum": ["Percent", "Fixed", "SpaceOnly", "Minimum"]
        }),
    );
    properties.insert(
        "headType".to_string(),
        json!({
            "type": "string",
            "enum": ["None", "Outline", "Number", "Bullet"]
        }),
    );
    for key in [
        "lineSpacing",
        "indent",
        "marginLeft",
        "marginRight",
        "spacingBefore",
        "spacingAfter",
    ] {
        properties.insert(
            key.to_string(),
            integer_schema(i32::MIN as i64, i32::MAX as i64),
        );
    }
    for key in [
        "paraLevel",
        "verticalAlign",
        "englishBreakUnit",
        "koreanBreakUnit",
    ] {
        properties.insert(key.to_string(), integer_schema(0, u8::MAX as i64));
    }
    properties.insert(
        "numberingId".to_string(),
        integer_schema(0, u16::MAX as i64),
    );
    for key in [
        "widowOrphan",
        "keepWithNext",
        "keepLines",
        "pageBreakBefore",
        "fontLineHeight",
        "singleLine",
        "autoSpaceKrEn",
        "autoSpaceKrNum",
        "borderConnect",
        "borderIgnoreMargin",
        "tabAutoLeft",
        "tabAutoRight",
    ] {
        properties.insert(key.to_string(), json!({ "type": "boolean" }));
    }
    properties.insert(
        "borderSpacing".to_string(),
        integer_array_schema(4, i16::MIN as i64, i16::MAX as i64),
    );
    add_border_fill_schema_props(&mut properties);
    properties.insert(
        "tabStops".to_string(),
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "position": integer_schema(0, u32::MAX as i64),
                    "type": integer_schema(0, u8::MAX as i64),
                    "fill": integer_schema(0, u8::MAX as i64)
                },
                "required": ["position"],
                "additionalProperties": true
            }
        }),
    );

    (
        name,
        json!({
            "type": ["object", "string"],
            "description": description,
            "properties": properties,
            "additionalProperties": true
        }),
    )
}

fn add_border_fill_schema_props(properties: &mut Map<String, Value>) {
    for key in ["borderLeft", "borderRight", "borderTop", "borderBottom"] {
        properties.insert(key.to_string(), border_line_schema());
    }
    properties.insert(
        "fillType".to_string(),
        json!({ "type": "string", "enum": ["none", "solid"] }),
    );
    properties.insert("fillColor".to_string(), css_hex_color_schema());
    properties.insert("patternColor".to_string(), css_hex_color_schema());
    properties.insert(
        "patternType".to_string(),
        integer_schema(i32::MIN as i64, i32::MAX as i64),
    );
}

fn border_line_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": integer_schema(0, u8::MAX as i64),
            "width": integer_schema(0, u8::MAX as i64),
            "color": css_hex_color_schema()
        },
        "additionalProperties": true
    })
}

fn integer_array_schema(len: usize, min: i64, max: i64) -> Value {
    json!({
        "type": "array",
        "minItems": len,
        "maxItems": len,
        "items": integer_schema(min, max)
    })
}

fn integer_schema(min: i64, max: i64) -> Value {
    json!({
        "type": "integer",
        "minimum": min,
        "maximum": max
    })
}

fn css_hex_color_schema() -> Value {
    json!({
        "type": "string",
        "pattern": "^#[0-9A-Fa-f]{6}$"
    })
}

fn chart_props_prop() -> SchemaProp {
    (
        "props",
        json!({
            "type": "object",
            "description": "Chart title, supported bar-chart type/layout, bar/line grouping, line marker/smooth style, pie first-slice/explosion/doughnut hole/ofPie secondary plot and connector-line style, scatter style/marker/smooth update-or-insert, stock up/down-bar and high-low-line style, data labels, chart data table flags, chart title/legend overlay, chart-space/title/plot flags, chart display options including plotVisibleOnly, category/value axis title/visibility/label/tick-mark/line/scale/number-format, legend position, series color, and cache edits. Category/value point counts may change when categories and every series values array use the same length. Native HWP CHART_DATA targets support explicit raw payload replacement through rawHwpChartDataBase64; rhwp-generated native Chart payloads also support title/chartType/legend/categories/series.colorHex/xAxis/yAxis semantic metadata roundtrip. Legacy OLE Contents charts are exposed by rhwp_get_chart_data as read-only renderer-neutral IR and intentionally reject semantic edits here.",
            "properties": {
                "rawHwpChartDataBase64": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Replacement raw HWPTAG_CHART_DATA payload for a native HWP Chart target. This is an explicit raw-level operation; Hancom-authored native CHART_DATA fields are still not decoded."
                },
                "raw_hwp_chart_data_base64": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Snake_case alias for rawHwpChartDataBase64."
                },
                "title": {
                    "type": "string",
                    "description": "Replacement chart title text."
                },
                "titleOverlay": {
                    "type": "boolean",
                    "description": "Whether the OOXML chart title overlays the plot area. Updates or inserts c:title/c:overlay."
                },
                "title_overlay": {
                    "type": "boolean",
                    "description": "Snake_case alias for titleOverlay."
                },
                "date1904": {
                    "type": "boolean",
                    "description": "Whether the OOXML chart uses the 1904 date system. Updates or inserts c:chartSpace/c:date1904."
                },
                "date_1904": {
                    "type": "boolean",
                    "description": "Snake_case alias for date1904."
                },
                "chartStyle": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 48,
                    "description": "OOXML chart style number. Updates existing c:chartSpace/c:style and c14 AlternateContent style values when present."
                },
                "chart_style": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 48,
                    "description": "Snake_case alias for chartStyle."
                },
                "chartAreaFillColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement OOXML chart-area fill RGB color. Updates or inserts c:chartSpace/c:spPr fill while preserving other direct children such as line style."
                },
                "chart_area_fill_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for chartAreaFillColor."
                },
                "plotAreaFillColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement OOXML plot-area fill RGB color. Updates or inserts c:plotArea/c:spPr fill while preserving other direct children such as line style."
                },
                "plot_area_fill_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for plotAreaFillColor."
                },
                "roundedCorners": {
                    "type": "boolean",
                    "description": "Whether the OOXML chart area uses rounded corners. Updates or inserts c:chartSpace/c:roundedCorners."
                },
                "rounded_corners": {
                    "type": "boolean",
                    "description": "Snake_case alias for roundedCorners."
                },
                "autoTitleDeleted": {
                    "type": "boolean",
                    "description": "Whether the automatically generated OOXML chart title is deleted. Updates or inserts c:chart/c:autoTitleDeleted."
                },
                "auto_title_deleted": {
                    "type": "boolean",
                    "description": "Snake_case alias for autoTitleDeleted."
                },
                "varyColors": {
                    "type": "boolean",
                    "description": "Whether the OOXML chart varies colors by data point or series. Updates or inserts c:*Chart/c:varyColors."
                },
                "vary_colors": {
                    "type": "boolean",
                    "description": "Snake_case alias for varyColors."
                },
                "view3DRotationX": {
                    "type": "integer",
                    "minimum": -90,
                    "maximum": 90,
                    "description": "OOXML 3D chart X rotation. Updates or inserts c:view3D/c:rotX."
                },
                "view_3d_rotation_x": {
                    "type": "integer",
                    "minimum": -90,
                    "maximum": 90,
                    "description": "Snake_case alias for view3DRotationX."
                },
                "view3DRotationY": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 360,
                    "description": "OOXML 3D chart Y rotation. Updates or inserts c:view3D/c:rotY."
                },
                "view_3d_rotation_y": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 360,
                    "description": "Snake_case alias for view3DRotationY."
                },
                "view3DPerspective": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 240,
                    "description": "OOXML 3D chart perspective. Updates or inserts c:view3D/c:perspective."
                },
                "view_3d_perspective": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 240,
                    "description": "Snake_case alias for view3DPerspective."
                },
                "view3DRightAngleAxes": {
                    "type": "boolean",
                    "description": "Whether the OOXML 3D chart uses right-angle axes. Updates or inserts c:view3D/c:rAngAx."
                },
                "view_3d_right_angle_axes": {
                    "type": "boolean",
                    "description": "Snake_case alias for view3DRightAngleAxes."
                },
                "view3DHeightPercent": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 5000,
                    "description": "OOXML 3D chart height percentage. Updates or inserts c:view3D/c:hPercent."
                },
                "view_3d_height_percent": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 5000,
                    "description": "Snake_case alias for view3DHeightPercent."
                },
                "view3DDepthPercent": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 5000,
                    "description": "OOXML 3D chart depth percentage. Updates or inserts c:view3D/c:depthPercent."
                },
                "view_3d_depth_percent": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 5000,
                    "description": "Snake_case alias for view3DDepthPercent."
                },
                "chartType": {
                    "type": "string",
                    "enum": ["Column", "Bar", "Line", "Pie", "Area", "Scatter", "Unknown"],
                    "description": "Replacement chart type. OOXML chart payloads currently support Column/Bar conversion through c:barDir; rhwp-generated native HWP CHART_DATA payloads also accept Line, Pie, Area, Scatter, and Unknown."
                },
                "chart_type": {
                    "type": "string",
                    "enum": ["Column", "Bar", "Line", "Pie", "Area", "Scatter", "Unknown"],
                    "description": "Snake_case alias for chartType."
                },
                "grouping": {
                    "type": "string",
                    "enum": ["Clustered", "Stacked", "PercentStacked"],
                    "description": "Supported bar/line chart grouping. Updates or inserts OOXML c:grouping for c:barChart, c:bar3DChart, and c:lineChart."
                },
                "barGapWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 500,
                    "description": "Replacement bar-chart gap width. Updates or inserts OOXML c:barChart/c:gapWidth or c:bar3DChart/c:gapWidth."
                },
                "barOverlap": {
                    "type": "integer",
                    "minimum": -100,
                    "maximum": 100,
                    "description": "Replacement bar-chart overlap. Updates or inserts OOXML c:barChart/c:overlap or c:bar3DChart/c:overlap."
                },
                "bar3DGapDepth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 500,
                    "description": "Replacement 3D bar-chart depth gap. Updates or inserts OOXML c:bar3DChart/c:gapDepth."
                },
                "bar_3d_gap_depth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 500,
                    "description": "Snake_case alias for bar3DGapDepth."
                },
                "bar3DShape": {
                    "type": "string",
                    "enum": ["box", "cone", "coneToMax", "cylinder", "pyramid", "pyramidToMax"],
                    "description": "Replacement 3D bar-chart shape. Updates or inserts OOXML c:bar3DChart/c:shape."
                },
                "bar_3d_shape": {
                    "type": "string",
                    "enum": ["box", "cone", "coneToMax", "cylinder", "pyramid", "pyramidToMax"],
                    "description": "Snake_case alias for bar3DShape."
                },
                "lineSmooth": {
                    "type": "boolean",
                    "description": "Whether line-chart series are rendered as smoothed curves. Updates or inserts OOXML c:lineChart/c:smooth and c:lineChart/c:ser/c:smooth elements."
                },
                "lineMarkerVisible": {
                    "type": "boolean",
                    "description": "Whether line-chart markers are shown. Updates or inserts OOXML c:lineChart/c:marker."
                },
                "line_marker_visible": {
                    "type": "boolean",
                    "description": "Snake_case alias for lineMarkerVisible."
                },
                "lineMarkerSize": {
                    "type": "integer",
                    "minimum": 2,
                    "maximum": 72,
                    "description": "Replacement line-chart marker size. Updates or inserts OOXML c:lineChart/c:ser/c:marker/c:size elements."
                },
                "line_marker_size": {
                    "type": "integer",
                    "minimum": 2,
                    "maximum": 72,
                    "description": "Snake_case alias for lineMarkerSize."
                },
                "lineMarkerSymbol": {
                    "type": "string",
                    "enum": ["Circle", "Dash", "Diamond", "Dot", "None", "Picture", "Plus", "Square", "Star", "Triangle", "X"],
                    "description": "Replacement line-chart marker symbol. Updates or inserts OOXML c:lineChart/c:ser/c:marker/c:symbol elements."
                },
                "line_marker_symbol": {
                    "type": "string",
                    "enum": ["Circle", "Dash", "Diamond", "Dot", "None", "Picture", "Plus", "Square", "Star", "Triangle", "X"],
                    "description": "Snake_case alias for lineMarkerSymbol."
                },
                "lineMarkerFillColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement line-chart marker fill RGB color."
                },
                "lineMarkerFillColorHex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Hex alias for lineMarkerFillColor."
                },
                "line_marker_fill_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for lineMarkerFillColor."
                },
                "line_marker_fill_color_hex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Snake_case hex alias for lineMarkerFillColor."
                },
                "lineMarkerLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement line-chart marker outline RGB color."
                },
                "lineMarkerLineColorHex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Hex alias for lineMarkerLineColor."
                },
                "line_marker_line_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for lineMarkerLineColor."
                },
                "line_marker_line_color_hex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Snake_case hex alias for lineMarkerLineColor."
                },
                "lineMarkerLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement line-chart marker outline width in OOXML EMUs."
                },
                "line_marker_line_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Snake_case alias for lineMarkerLineWidth."
                },
                "pieFirstSliceAngle": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 360,
                    "description": "Replacement pie-chart first slice angle. Updates or inserts OOXML c:pieChart/c:firstSliceAng."
                },
                "pieExplosion": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 400,
                    "description": "Replacement pie-chart explosion amount. Updates or inserts OOXML c:pieChart/c:ser/c:explosion elements."
                },
                "doughnutHoleSize": {
                    "type": "integer",
                    "minimum": 10,
                    "maximum": 90,
                    "description": "Replacement doughnut-chart hole size. Updates or inserts OOXML c:doughnutChart/c:holeSize."
                },
                "doughnut_hole_size": {
                    "type": "integer",
                    "minimum": 10,
                    "maximum": 90,
                    "description": "Snake_case alias for doughnutHoleSize."
                },
                "pieOfPieType": {
                    "type": "string",
                    "enum": ["Pie", "Bar"],
                    "description": "Replacement ofPie secondary plot type. Updates or inserts OOXML c:ofPieChart/c:ofPieType between pie-of-pie and bar-of-pie."
                },
                "pieOfPieGapWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 500,
                    "description": "Replacement ofPie gap width. Updates or inserts OOXML c:ofPieChart/c:gapWidth."
                },
                "pieOfPieSecondSize": {
                    "type": "integer",
                    "minimum": 5,
                    "maximum": 200,
                    "description": "Replacement ofPie secondary plot size. Updates or inserts OOXML c:ofPieChart/c:secondPieSize."
                },
                "pieOfPieSerLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement ofPie connector-line RGB color. Updates or inserts OOXML c:ofPieChart/c:serLines/c:spPr/a:ln."
                },
                "pie_of_pie_ser_line_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for pieOfPieSerLineColor."
                },
                "pieOfPieSerLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement ofPie connector-line width in OOXML EMUs."
                },
                "pie_of_pie_ser_line_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Snake_case alias for pieOfPieSerLineWidth."
                },
                "scatterStyle": {
                    "type": "string",
                    "enum": ["Line", "LineMarker", "Marker", "Smooth", "SmoothMarker"],
                    "description": "Replacement scatter-chart style. Updates or inserts OOXML c:scatterChart/c:scatterStyle."
                },
                "scatterSmooth": {
                    "type": "boolean",
                    "description": "Whether scatter-chart series are rendered as smoothed curves. Updates or inserts OOXML c:scatterChart/c:ser/c:smooth elements."
                },
                "scatterMarkerSize": {
                    "type": "integer",
                    "minimum": 2,
                    "maximum": 72,
                    "description": "Replacement scatter-chart marker size. Updates or inserts OOXML c:scatterChart/c:ser/c:marker/c:size elements."
                },
                "scatter_marker_size": {
                    "type": "integer",
                    "minimum": 2,
                    "maximum": 72,
                    "description": "Snake_case alias for scatterMarkerSize."
                },
                "scatterMarkerSymbol": {
                    "type": "string",
                    "enum": ["Circle", "Dash", "Diamond", "Dot", "None", "Picture", "Plus", "Square", "Star", "Triangle", "X"],
                    "description": "Replacement scatter-chart marker symbol. Updates or inserts OOXML c:scatterChart/c:ser/c:marker/c:symbol elements."
                },
                "scatter_marker_symbol": {
                    "type": "string",
                    "enum": ["Circle", "Dash", "Diamond", "Dot", "None", "Picture", "Plus", "Square", "Star", "Triangle", "X"],
                    "description": "Snake_case alias for scatterMarkerSymbol."
                },
                "scatterMarkerFillColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement scatter-chart marker fill RGB color."
                },
                "scatterMarkerFillColorHex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Hex alias for scatterMarkerFillColor."
                },
                "scatter_marker_fill_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for scatterMarkerFillColor."
                },
                "scatter_marker_fill_color_hex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Snake_case hex alias for scatterMarkerFillColor."
                },
                "scatterMarkerLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement scatter-chart marker outline RGB color."
                },
                "scatterMarkerLineColorHex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Hex alias for scatterMarkerLineColor."
                },
                "scatter_marker_line_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for scatterMarkerLineColor."
                },
                "scatter_marker_line_color_hex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Snake_case hex alias for scatterMarkerLineColor."
                },
                "scatterMarkerLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement scatter-chart marker outline width in OOXML EMUs."
                },
                "scatter_marker_line_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Snake_case alias for scatterMarkerLineWidth."
                },
                "trendlineType": {
                    "type": "string",
                    "enum": ["Linear", "Exponential", "Logarithmic", "MovingAverage", "Polynomial", "Power"],
                    "description": "Replacement line/scatter chart trendline type. Updates or inserts OOXML c:ser/c:trendline/c:trendlineType elements."
                },
                "trendline_type": {
                    "type": "string",
                    "enum": ["Linear", "Exponential", "Logarithmic", "MovingAverage", "Polynomial", "Power"],
                    "description": "Snake_case alias for trendlineType."
                },
                "trendlineOrder": {
                    "type": "integer",
                    "minimum": 2,
                    "maximum": 6,
                    "description": "Polynomial line/scatter chart trendline order. Updates or inserts OOXML c:ser/c:trendline/c:order."
                },
                "trendline_order": {
                    "type": "integer",
                    "minimum": 2,
                    "maximum": 6,
                    "description": "Snake_case alias for trendlineOrder."
                },
                "trendlinePeriod": {
                    "type": "integer",
                    "minimum": 2,
                    "maximum": 255,
                    "description": "Moving-average line/scatter chart trendline period. Updates or inserts OOXML c:ser/c:trendline/c:period."
                },
                "trendline_period": {
                    "type": "integer",
                    "minimum": 2,
                    "maximum": 255,
                    "description": "Snake_case alias for trendlinePeriod."
                },
                "trendlineDisplayEquation": {
                    "type": "boolean",
                    "description": "Whether line/scatter chart trendlines display their equation. Updates or inserts OOXML c:ser/c:trendline/c:dispEq."
                },
                "trendline_display_equation": {
                    "type": "boolean",
                    "description": "Snake_case alias for trendlineDisplayEquation."
                },
                "trendlineDisplayRSquared": {
                    "type": "boolean",
                    "description": "Whether line/scatter chart trendlines display R-squared. Updates or inserts OOXML c:ser/c:trendline/c:dispRSqr."
                },
                "trendline_display_r_squared": {
                    "type": "boolean",
                    "description": "Snake_case alias for trendlineDisplayRSquared."
                },
                "trendlineLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement line/scatter chart trendline RGB line color. Updates or inserts OOXML c:ser/c:trendline/c:spPr/a:ln."
                },
                "trendlineLineColorHex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Hex string alias for trendlineLineColor."
                },
                "trendline_line_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for trendlineLineColor."
                },
                "trendline_line_color_hex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Snake_case hex alias for trendlineLineColor."
                },
                "trendlineLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement line/scatter chart trendline line width in OOXML EMUs."
                },
                "trendline_line_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Snake_case alias for trendlineLineWidth."
                },
                "errorBarDirection": {
                    "type": "string",
                    "enum": ["X", "Y"],
                    "description": "Line/scatter chart error-bar direction. Updates or inserts OOXML c:ser/c:errBars/c:errDir."
                },
                "error_bar_direction": {
                    "type": "string",
                    "enum": ["X", "Y"],
                    "description": "Snake_case alias for errorBarDirection."
                },
                "errorBarType": {
                    "type": "string",
                    "enum": ["Both", "Plus", "Minus"],
                    "description": "Line/scatter chart error-bar display type. Updates or inserts OOXML c:ser/c:errBars/c:errBarType."
                },
                "error_bar_type": {
                    "type": "string",
                    "enum": ["Both", "Plus", "Minus"],
                    "description": "Snake_case alias for errorBarType."
                },
                "errorBarValueType": {
                    "type": "string",
                    "enum": ["FixedValue", "Percentage", "StandardDeviation", "StandardError"],
                    "description": "Line/scatter chart error-bar value type. Updates or inserts OOXML c:ser/c:errBars/c:errValType."
                },
                "error_bar_value_type": {
                    "type": "string",
                    "enum": ["FixedValue", "Percentage", "StandardDeviation", "StandardError"],
                    "description": "Snake_case alias for errorBarValueType."
                },
                "errorBarValue": {
                    "type": "number",
                    "minimum": 0,
                    "description": "Line/scatter chart error-bar numeric value. Updates or inserts OOXML c:ser/c:errBars/c:val."
                },
                "error_bar_value": {
                    "type": "number",
                    "minimum": 0,
                    "description": "Snake_case alias for errorBarValue."
                },
                "errorBarNoEndCap": {
                    "type": "boolean",
                    "description": "Whether line/scatter chart error bars hide end caps. Updates or inserts OOXML c:ser/c:errBars/c:noEndCap."
                },
                "error_bar_no_end_cap": {
                    "type": "boolean",
                    "description": "Snake_case alias for errorBarNoEndCap."
                },
                "errorBarLineColor": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 16777215,
                    "description": "Replacement line/scatter chart error-bar line color as RGB integer. Updates or inserts OOXML c:ser/c:errBars/c:spPr/a:ln."
                },
                "errorBarLineColorHex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Hex string alias for errorBarLineColor."
                },
                "error_bar_line_color": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 16777215,
                    "description": "Snake_case alias for errorBarLineColor."
                },
                "error_bar_line_color_hex": {
                    "type": "string",
                    "pattern": "^#?[0-9A-Fa-f]{6}$",
                    "description": "Snake_case hex alias for errorBarLineColor."
                },
                "errorBarLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement line/scatter chart error-bar line width in OOXML EMUs."
                },
                "error_bar_line_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Snake_case alias for errorBarLineWidth."
                },
                "stockUpDownBarGapWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 500,
                    "description": "Replacement stock-chart up/down bar gap width. Updates or inserts OOXML c:stockChart/c:upDownBars/c:gapWidth."
                },
                "stock_up_down_bar_gap_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 500,
                    "description": "Snake_case alias for stockUpDownBarGapWidth."
                },
                "stockUpBarFillColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement stock-chart up-bar fill RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "stock_up_bar_fill_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for stockUpBarFillColor."
                },
                "stockDownBarFillColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement stock-chart down-bar fill RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "stock_down_bar_fill_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for stockDownBarFillColor."
                },
                "stockUpBarLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement stock-chart up-bar line RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "stock_up_bar_line_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for stockUpBarLineColor."
                },
                "stockDownBarLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement stock-chart down-bar line RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "stock_down_bar_line_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for stockDownBarLineColor."
                },
                "stockUpBarLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement stock-chart up-bar line width in OOXML EMUs."
                },
                "stock_up_bar_line_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Snake_case alias for stockUpBarLineWidth."
                },
                "stockDownBarLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement stock-chart down-bar line width in OOXML EMUs."
                },
                "stock_down_bar_line_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Snake_case alias for stockDownBarLineWidth."
                },
                "stockHiLowLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement stock-chart high-low line RGB color. Updates or inserts OOXML c:stockChart/c:hiLowLines/c:spPr/a:ln."
                },
                "stock_hi_low_line_color": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Snake_case alias for stockHiLowLineColor."
                },
                "stockHiLowLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement stock-chart high-low line width in OOXML EMUs."
                },
                "stock_hi_low_line_width": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Snake_case alias for stockHiLowLineWidth."
                },
                "dataLabelPosition": {
                    "type": "string",
                    "enum": ["BestFit", "Bottom", "Center", "InsideBase", "InsideEnd", "Left", "OutsideEnd", "Right", "Top"],
                    "description": "Replacement OOXML chart data-label position. Updates or inserts c:dLbls/c:dLblPos."
                },
                "data_label_position": {
                    "type": "string",
                    "enum": ["BestFit", "Bottom", "Center", "InsideBase", "InsideEnd", "Left", "OutsideEnd", "Right", "Top"],
                    "description": "Snake_case alias for dataLabelPosition."
                },
                "dataLabelsShowValue": {
                    "type": "boolean",
                    "description": "Whether OOXML chart data labels show values. Updates or inserts c:dLbls/c:showVal."
                },
                "data_labels_show_value": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataLabelsShowValue."
                },
                "dataLabelsShowCategoryName": {
                    "type": "boolean",
                    "description": "Whether OOXML chart data labels show category names. Updates or inserts c:dLbls/c:showCatName."
                },
                "data_labels_show_category_name": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataLabelsShowCategoryName."
                },
                "dataLabelsShowSeriesName": {
                    "type": "boolean",
                    "description": "Whether OOXML chart data labels show series names. Updates or inserts c:dLbls/c:showSerName."
                },
                "data_labels_show_series_name": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataLabelsShowSeriesName."
                },
                "dataLabelsShowPercent": {
                    "type": "boolean",
                    "description": "Whether OOXML chart data labels show percentages. Updates or inserts c:dLbls/c:showPercent."
                },
                "data_labels_show_percent": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataLabelsShowPercent."
                },
                "dataLabelsShowLegendKey": {
                    "type": "boolean",
                    "description": "Whether OOXML chart data labels show legend keys. Updates or inserts c:dLbls/c:showLegendKey."
                },
                "data_labels_show_legend_key": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataLabelsShowLegendKey."
                },
                "displayBlanksAs": {
                    "type": "string",
                    "enum": ["Gap", "Span", "Zero"],
                    "description": "How an OOXML chart displays blank cells. Updates or inserts c:chart/c:dispBlanksAs."
                },
                "display_blanks_as": {
                    "type": "string",
                    "enum": ["Gap", "Span", "Zero"],
                    "description": "Snake_case alias for displayBlanksAs."
                },
                "showHiddenData": {
                    "type": "boolean",
                    "description": "Whether an OOXML chart plots hidden row/column data. Updates or inserts c:chart/c:showHiddenData."
                },
                "show_hidden_data": {
                    "type": "boolean",
                    "description": "Snake_case alias for showHiddenData."
                },
                "plotVisibleOnly": {
                    "type": "boolean",
                    "description": "Whether an OOXML chart plots visible cells only. Updates or inserts c:chart/c:plotVisOnly."
                },
                "plot_visible_only": {
                    "type": "boolean",
                    "description": "Snake_case alias for plotVisibleOnly."
                },
                "dataTableShowHorizontalBorder": {
                    "type": "boolean",
                    "description": "Whether an OOXML chart data table shows horizontal borders. Updates or inserts c:plotArea/c:dTable/c:showHorzBorder."
                },
                "data_table_show_horizontal_border": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataTableShowHorizontalBorder."
                },
                "dataTableShowVerticalBorder": {
                    "type": "boolean",
                    "description": "Whether an OOXML chart data table shows vertical borders. Updates or inserts c:plotArea/c:dTable/c:showVertBorder."
                },
                "data_table_show_vertical_border": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataTableShowVerticalBorder."
                },
                "dataTableShowOutline": {
                    "type": "boolean",
                    "description": "Whether an OOXML chart data table shows an outline. Updates or inserts c:plotArea/c:dTable/c:showOutline."
                },
                "data_table_show_outline": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataTableShowOutline."
                },
                "dataTableShowKeys": {
                    "type": "boolean",
                    "description": "Whether an OOXML chart data table shows legend keys. Updates or inserts c:plotArea/c:dTable/c:showKeys."
                },
                "data_table_show_keys": {
                    "type": "boolean",
                    "description": "Snake_case alias for dataTableShowKeys."
                },
                "legendPosition": {
                    "type": "string",
                    "enum": ["Right", "Left", "Top", "Bottom", "TopRight"],
                    "description": "Replacement chart legend position. Updates or inserts OOXML c:legend/c:legendPos."
                },
                "legendOverlay": {
                    "type": "boolean",
                    "description": "Whether the OOXML chart legend overlays the plot area. Updates or inserts c:legend/c:overlay."
                },
                "legend_overlay": {
                    "type": "boolean",
                    "description": "Snake_case alias for legendOverlay."
                },
                "categoryAxisTitle": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Replacement OOXML category-axis title. Updates or inserts c:catAx/c:title."
                },
                "category_axis_title": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Snake_case alias for categoryAxisTitle."
                },
                "valueAxisTitle": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Replacement OOXML value-axis title. Updates or inserts c:valAx/c:title."
                },
                "value_axis_title": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Snake_case alias for valueAxisTitle."
                },
                "legendVisible": {
                    "type": "boolean",
                    "description": "Native HWP semantic legend visibility for rhwp-generated CHART_DATA payloads."
                },
                "legend_visible": {
                    "type": "boolean",
                    "description": "Snake_case alias for legendVisible."
                },
                "categoryAxisVisible": {
                    "type": "boolean",
                    "description": "Whether the category axis is visible. Updates or inserts OOXML c:catAx/c:delete."
                },
                "valueAxisVisible": {
                    "type": "boolean",
                    "description": "Whether the value axis is visible. Updates or inserts OOXML c:valAx/c:delete."
                },
                "categoryAxisPosition": {
                    "type": "string",
                    "enum": ["Bottom", "Left", "Top", "Right"],
                    "description": "Replacement category-axis position. Updates or inserts OOXML c:catAx/c:axPos."
                },
                "category_axis_position": {
                    "type": "string",
                    "enum": ["Bottom", "Left", "Top", "Right"],
                    "description": "Snake_case alias for categoryAxisPosition."
                },
                "valueAxisPosition": {
                    "type": "string",
                    "enum": ["Bottom", "Left", "Top", "Right"],
                    "description": "Replacement value-axis position. Updates or inserts OOXML c:valAx/c:axPos."
                },
                "value_axis_position": {
                    "type": "string",
                    "enum": ["Bottom", "Left", "Top", "Right"],
                    "description": "Snake_case alias for valueAxisPosition."
                },
                "categoryAxisLabelPosition": {
                    "type": "string",
                    "enum": ["NextTo", "High", "Low", "None"],
                    "description": "Replacement category-axis tick label position. Updates or inserts OOXML c:catAx/c:tickLblPos."
                },
                "valueAxisLabelPosition": {
                    "type": "string",
                    "enum": ["NextTo", "High", "Low", "None"],
                    "description": "Replacement value-axis tick label position. Updates or inserts OOXML c:valAx/c:tickLblPos."
                },
                "categoryAxisAuto": {
                    "type": "boolean",
                    "description": "Replacement category-axis automatic label setting. Updates or inserts OOXML c:catAx/c:auto."
                },
                "category_axis_auto": {
                    "type": "boolean",
                    "description": "Snake_case alias for categoryAxisAuto."
                },
                "categoryAxisLabelAlignment": {
                    "type": "string",
                    "enum": ["Center", "Left", "Right"],
                    "description": "Replacement category-axis label alignment. Updates or inserts OOXML c:catAx/c:lblAlgn."
                },
                "category_axis_label_alignment": {
                    "type": "string",
                    "enum": ["Center", "Left", "Right"],
                    "description": "Snake_case alias for categoryAxisLabelAlignment."
                },
                "categoryAxisLabelOffset": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Replacement category-axis label offset. Updates or inserts OOXML c:catAx/c:lblOffset."
                },
                "category_axis_label_offset": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Snake_case alias for categoryAxisLabelOffset."
                },
                "categoryAxisTickMarkSkip": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Replacement category-axis tick mark skip. Updates or inserts OOXML c:catAx/c:tickMarkSkip."
                },
                "category_axis_tick_mark_skip": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Snake_case alias for categoryAxisTickMarkSkip."
                },
                "categoryAxisNoMultiLevelLabels": {
                    "type": "boolean",
                    "description": "Replacement category-axis no-multi-level-labels flag. Updates or inserts OOXML c:catAx/c:noMultiLvlLbl."
                },
                "category_axis_no_multi_level_labels": {
                    "type": "boolean",
                    "description": "Snake_case alias for categoryAxisNoMultiLevelLabels."
                },
                "categoryAxisOrientation": {
                    "type": "string",
                    "enum": ["MinMax", "MaxMin"],
                    "description": "Replacement category-axis scale orientation. Updates existing OOXML c:catAx/c:scaling/c:orientation or inserts c:scaling/c:orientation when missing."
                },
                "category_axis_orientation": {
                    "type": "string",
                    "enum": ["MinMax", "MaxMin"],
                    "description": "Snake_case alias for categoryAxisOrientation."
                },
                "valueAxisOrientation": {
                    "type": "string",
                    "enum": ["MinMax", "MaxMin"],
                    "description": "Replacement value-axis scale orientation. Updates existing OOXML c:valAx/c:scaling/c:orientation or inserts c:scaling/c:orientation when missing."
                },
                "value_axis_orientation": {
                    "type": "string",
                    "enum": ["MinMax", "MaxMin"],
                    "description": "Snake_case alias for valueAxisOrientation."
                },
                "categoryAxisCrosses": {
                    "type": "string",
                    "enum": ["AutoZero", "Min", "Max"],
                    "description": "Replacement category-axis crossing position. Updates or inserts OOXML c:catAx/c:crosses."
                },
                "category_axis_crosses": {
                    "type": "string",
                    "enum": ["AutoZero", "Min", "Max"],
                    "description": "Snake_case alias for categoryAxisCrosses."
                },
                "categoryAxisCrossesAt": {
                    "type": "number",
                    "description": "Replacement category-axis numeric crossing point. Updates or inserts OOXML c:catAx/c:crossesAt."
                },
                "category_axis_crosses_at": {
                    "type": "number",
                    "description": "Snake_case alias for categoryAxisCrossesAt."
                },
                "valueAxisCrosses": {
                    "type": "string",
                    "enum": ["AutoZero", "Min", "Max"],
                    "description": "Replacement value-axis crossing position. Updates or inserts OOXML c:valAx/c:crosses."
                },
                "value_axis_crosses": {
                    "type": "string",
                    "enum": ["AutoZero", "Min", "Max"],
                    "description": "Snake_case alias for valueAxisCrosses."
                },
                "valueAxisCrossesAt": {
                    "type": "number",
                    "description": "Replacement value-axis numeric crossing point. Updates or inserts OOXML c:valAx/c:crossesAt."
                },
                "value_axis_crosses_at": {
                    "type": "number",
                    "description": "Snake_case alias for valueAxisCrossesAt."
                },
                "valueAxisCrossBetween": {
                    "type": "string",
                    "enum": ["Between", "MidCategory"],
                    "description": "Replacement value-axis bar/category crossing position. Updates or inserts OOXML c:valAx/c:crossBetween."
                },
                "value_axis_cross_between": {
                    "type": "string",
                    "enum": ["Between", "MidCategory"],
                    "description": "Snake_case alias for valueAxisCrossBetween."
                },
                "categoryAxisMajorTickMark": {
                    "type": "string",
                    "enum": ["Cross", "In", "Out", "None"],
                    "description": "Replacement category-axis major tick mark style. Updates or inserts OOXML c:catAx/c:majorTickMark."
                },
                "categoryAxisMinorTickMark": {
                    "type": "string",
                    "enum": ["Cross", "In", "Out", "None"],
                    "description": "Replacement category-axis minor tick mark style. Updates or inserts OOXML c:catAx/c:minorTickMark."
                },
                "categoryAxisLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement category-axis line RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "categoryAxisLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement category-axis line width in OOXML EMUs."
                },
                "categoryAxisMajorGridLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement category-axis major gridline RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "categoryAxisMajorGridLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement category-axis major gridline width in OOXML EMUs."
                },
                "categoryAxisMinorGridLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement category-axis minor gridline RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "categoryAxisMinorGridLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement category-axis minor gridline width in OOXML EMUs."
                },
                "valueAxisMajorTickMark": {
                    "type": "string",
                    "enum": ["Cross", "In", "Out", "None"],
                    "description": "Replacement value-axis major tick mark style. Updates or inserts OOXML c:valAx/c:majorTickMark."
                },
                "valueAxisMinorTickMark": {
                    "type": "string",
                    "enum": ["Cross", "In", "Out", "None"],
                    "description": "Replacement value-axis minor tick mark style. Updates or inserts OOXML c:valAx/c:minorTickMark."
                },
                "valueAxisLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement value-axis line RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "valueAxisLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement value-axis line width in OOXML EMUs."
                },
                "valueAxisMajorGridLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement value-axis major gridline RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "valueAxisMajorGridLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement value-axis major gridline width in OOXML EMUs."
                },
                "valueAxisMinorGridLineColor": {
                    "oneOf": [
                        { "type": "integer", "minimum": 0, "maximum": 16777215 },
                        { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                    ],
                    "description": "Replacement value-axis minor gridline RGB color. Accepts an integer 0x000000..0xFFFFFF or #RRGGBB."
                },
                "valueAxisMinorGridLineWidth": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 2000000,
                    "description": "Replacement value-axis minor gridline width in OOXML EMUs."
                },
                "valueAxisMinimum": {
                    "type": "number",
                    "description": "Replacement value-axis minimum. Updates existing OOXML c:valAx/c:scaling/c:min or inserts c:scaling/c:min when missing."
                },
                "valueAxisMaximum": {
                    "type": "number",
                    "description": "Replacement value-axis maximum. Updates existing OOXML c:valAx/c:scaling/c:max or inserts c:scaling/c:max when missing."
                },
                "valueAxisMajorUnit": {
                    "type": "number",
                    "exclusiveMinimum": 0,
                    "description": "Replacement value-axis major tick unit. Updates existing OOXML c:valAx/c:majorUnit or inserts it into the value axis."
                },
                "valueAxisMinorUnit": {
                    "type": "number",
                    "exclusiveMinimum": 0,
                    "description": "Replacement value-axis minor tick unit. Updates existing OOXML c:valAx/c:minorUnit or inserts it into the value axis."
                },
                "valueAxisLogBase": {
                    "type": "number",
                    "minimum": 2,
                    "description": "Replacement value-axis logarithmic scale base. Updates existing OOXML c:valAx/c:scaling/c:logBase or inserts c:scaling/c:logBase when missing."
                },
                "value_axis_log_base": {
                    "type": "number",
                    "minimum": 2,
                    "description": "Snake_case alias for valueAxisLogBase."
                },
                "valueAxisDisplayUnit": {
                    "type": "string",
                    "enum": ["Hundreds", "Thousands", "TenThousands", "HundredThousands", "Millions", "TenMillions", "HundredMillions", "Billions", "Trillions"],
                    "description": "Replacement value-axis display unit. Updates or inserts OOXML c:valAx/c:dispUnits/c:builtInUnit."
                },
                "value_axis_display_unit": {
                    "type": "string",
                    "enum": ["Hundreds", "Thousands", "TenThousands", "HundredThousands", "Millions", "TenMillions", "HundredMillions", "Billions", "Trillions"],
                    "description": "Snake_case alias for valueAxisDisplayUnit."
                },
                "categoryAxisNumberFormat": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Replacement category-axis number/date format. Updates or inserts OOXML c:catAx/c:numFmt formatCode."
                },
                "categoryAxisNumberFormatSourceLinked": {
                    "type": "boolean",
                    "description": "Whether the category-axis number/date format remains source-linked. Updates or inserts OOXML c:catAx/c:numFmt sourceLinked."
                },
                "category_axis_number_format": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Snake_case alias for categoryAxisNumberFormat."
                },
                "category_axis_number_format_source_linked": {
                    "type": "boolean",
                    "description": "Snake_case alias for categoryAxisNumberFormatSourceLinked."
                },
                "valueAxisNumberFormat": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Replacement value-axis number format. Updates or inserts OOXML c:valAx/c:numFmt formatCode."
                },
                "valueAxisNumberFormatSourceLinked": {
                    "type": "boolean",
                    "description": "Whether the value-axis number format remains source-linked. Updates or inserts OOXML c:valAx/c:numFmt sourceLinked."
                },
                "value_axis_number_format": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Snake_case alias for valueAxisNumberFormat."
                },
                "value_axis_number_format_source_linked": {
                    "type": "boolean",
                    "description": "Snake_case alias for valueAxisNumberFormatSourceLinked."
                },
                "categories": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Replacement category labels. If the length changes, every existing series with values must also provide a values array of the same length."
                },
                "series": {
                    "type": "array",
                    "description": "Series updates. Omit fields that should stay unchanged.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "index": {
                                "type": "integer",
                                "minimum": 0,
                                "description": "0-based series index. Defaults to the array index."
                            },
                            "name": {
                                "type": "string",
                                "description": "Replacement series label."
                            },
                            "values": {
                                "type": "array",
                                "items": { "type": "number" },
                                "description": "Replacement numeric values. Length must match the effective category count."
                            },
                            "color": {
                                "oneOf": [
                                    { "type": "integer", "minimum": 0, "maximum": 16777215 },
                                    { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                                ],
                                "description": "Series RGB color. OOXML charts write this to c:ser/c:spPr/a:solidFill/a:srgbClr; native HWP semantic charts store it in the rhwp-generated CHART_DATA payload."
                            },
                            "colorHex": {
                                "type": "string",
                                "pattern": "^#?[0-9A-Fa-f]{6}$",
                                "description": "Series RGB color as #RRGGBB. Supported for OOXML chart series fill and native HWP semantic chart metadata."
                            },
                            "color_hex": {
                                "type": "string",
                                "pattern": "^#?[0-9A-Fa-f]{6}$",
                                "description": "Snake_case alias for colorHex."
                            },
                            "lineColor": {
                                "oneOf": [
                                    { "type": "integer", "minimum": 0, "maximum": 16777215 },
                                    { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                                ],
                                "description": "OOXML series line RGB color. Updates or inserts c:ser/c:spPr/a:ln/a:solidFill/a:srgbClr."
                            },
                            "lineColorHex": {
                                "type": "string",
                                "pattern": "^#?[0-9A-Fa-f]{6}$",
                                "description": "OOXML series line RGB color as #RRGGBB."
                            },
                            "line_color": {
                                "oneOf": [
                                    { "type": "integer", "minimum": 0, "maximum": 16777215 },
                                    { "type": "string", "pattern": "^#?[0-9A-Fa-f]{6}$" }
                                ],
                                "description": "Snake_case alias for lineColor."
                            },
                            "line_color_hex": {
                                "type": "string",
                                "pattern": "^#?[0-9A-Fa-f]{6}$",
                                "description": "Snake_case alias for lineColorHex."
                            },
                            "lineWidth": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 2000000,
                                "description": "OOXML series line width in EMUs. Updates or inserts c:ser/c:spPr/a:ln@w."
                            },
                            "line_width": {
                                "type": "integer",
                                "minimum": 0,
                                "maximum": 2000000,
                                "description": "Snake_case alias for lineWidth."
                            }
                        },
                        "additionalProperties": false
                    }
                },
                "xAxis": {
                    "type": "object",
                    "description": "Native HWP semantic x-axis metadata for rhwp-generated CHART_DATA payloads.",
                    "properties": {
                        "label": { "type": "string" },
                        "labels": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "min": { "type": "number" },
                        "max": { "type": "number" }
                    },
                    "additionalProperties": false
                },
                "x_axis": {
                    "type": "object",
                    "description": "Snake_case alias for xAxis.",
                    "properties": {
                        "label": { "type": "string" },
                        "labels": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "min": { "type": "number" },
                        "max": { "type": "number" }
                    },
                    "additionalProperties": false
                },
                "yAxis": {
                    "type": "object",
                    "description": "Native HWP semantic y-axis metadata for rhwp-generated CHART_DATA payloads.",
                    "properties": {
                        "label": { "type": "string" },
                        "labels": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "min": { "type": "number" },
                        "max": { "type": "number" }
                    },
                    "additionalProperties": false
                },
                "y_axis": {
                    "type": "object",
                    "description": "Snake_case alias for yAxis.",
                    "properties": {
                        "label": { "type": "string" },
                        "labels": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "min": { "type": "number" },
                        "max": { "type": "number" }
                    },
                    "additionalProperties": false
                }
            },
            "anyOf": [
                { "required": ["rawHwpChartDataBase64"] },
                { "required": ["raw_hwp_chart_data_base64"] },
                { "required": ["title"] },
                { "required": ["titleOverlay"] },
                { "required": ["title_overlay"] },
                { "required": ["date1904"] },
                { "required": ["date_1904"] },
                { "required": ["chartStyle"] },
                { "required": ["chart_style"] },
                { "required": ["chartAreaFillColor"] },
                { "required": ["chart_area_fill_color"] },
                { "required": ["plotAreaFillColor"] },
                { "required": ["plot_area_fill_color"] },
                { "required": ["roundedCorners"] },
                { "required": ["rounded_corners"] },
                { "required": ["autoTitleDeleted"] },
                { "required": ["auto_title_deleted"] },
                { "required": ["varyColors"] },
                { "required": ["vary_colors"] },
                { "required": ["view3DRotationX"] },
                { "required": ["view_3d_rotation_x"] },
                { "required": ["view3DRotationY"] },
                { "required": ["view_3d_rotation_y"] },
                { "required": ["view3DPerspective"] },
                { "required": ["view_3d_perspective"] },
                { "required": ["view3DRightAngleAxes"] },
                { "required": ["view_3d_right_angle_axes"] },
                { "required": ["view3DHeightPercent"] },
                { "required": ["view_3d_height_percent"] },
                { "required": ["view3DDepthPercent"] },
                { "required": ["view_3d_depth_percent"] },
                { "required": ["chartType"] },
                { "required": ["chart_type"] },
                { "required": ["grouping"] },
                { "required": ["barGapWidth"] },
                { "required": ["barOverlap"] },
                { "required": ["bar3DGapDepth"] },
                { "required": ["bar_3d_gap_depth"] },
                { "required": ["bar3DShape"] },
                { "required": ["bar_3d_shape"] },
                { "required": ["lineSmooth"] },
                { "required": ["lineMarkerVisible"] },
                { "required": ["line_marker_visible"] },
                { "required": ["lineMarkerSize"] },
                { "required": ["line_marker_size"] },
                { "required": ["lineMarkerSymbol"] },
                { "required": ["line_marker_symbol"] },
                { "required": ["lineMarkerFillColor"] },
                { "required": ["lineMarkerFillColorHex"] },
                { "required": ["line_marker_fill_color"] },
                { "required": ["line_marker_fill_color_hex"] },
                { "required": ["lineMarkerLineColor"] },
                { "required": ["lineMarkerLineColorHex"] },
                { "required": ["line_marker_line_color"] },
                { "required": ["line_marker_line_color_hex"] },
                { "required": ["lineMarkerLineWidth"] },
                { "required": ["line_marker_line_width"] },
                { "required": ["pieFirstSliceAngle"] },
                { "required": ["pieExplosion"] },
                { "required": ["doughnutHoleSize"] },
                { "required": ["doughnut_hole_size"] },
                { "required": ["pieOfPieType"] },
                { "required": ["pieOfPieGapWidth"] },
                { "required": ["pieOfPieSecondSize"] },
                { "required": ["pieOfPieSerLineColor"] },
                { "required": ["pie_of_pie_ser_line_color"] },
                { "required": ["pieOfPieSerLineWidth"] },
                { "required": ["pie_of_pie_ser_line_width"] },
                { "required": ["scatterStyle"] },
                { "required": ["scatterSmooth"] },
                { "required": ["scatterMarkerSize"] },
                { "required": ["scatter_marker_size"] },
                { "required": ["scatterMarkerSymbol"] },
                { "required": ["scatter_marker_symbol"] },
                { "required": ["scatterMarkerFillColor"] },
                { "required": ["scatterMarkerFillColorHex"] },
                { "required": ["scatter_marker_fill_color"] },
                { "required": ["scatter_marker_fill_color_hex"] },
                { "required": ["scatterMarkerLineColor"] },
                { "required": ["scatterMarkerLineColorHex"] },
                { "required": ["scatter_marker_line_color"] },
                { "required": ["scatter_marker_line_color_hex"] },
                { "required": ["scatterMarkerLineWidth"] },
                { "required": ["scatter_marker_line_width"] },
                { "required": ["trendlineType"] },
                { "required": ["trendline_type"] },
                { "required": ["trendlineOrder"] },
                { "required": ["trendline_order"] },
                { "required": ["trendlinePeriod"] },
                { "required": ["trendline_period"] },
                { "required": ["trendlineDisplayEquation"] },
                { "required": ["trendline_display_equation"] },
                { "required": ["trendlineDisplayRSquared"] },
                { "required": ["trendline_display_r_squared"] },
                { "required": ["trendlineLineColor"] },
                { "required": ["trendlineLineColorHex"] },
                { "required": ["trendline_line_color"] },
                { "required": ["trendline_line_color_hex"] },
                { "required": ["trendlineLineWidth"] },
                { "required": ["trendline_line_width"] },
                { "required": ["errorBarDirection"] },
                { "required": ["error_bar_direction"] },
                { "required": ["errorBarType"] },
                { "required": ["error_bar_type"] },
                { "required": ["errorBarValueType"] },
                { "required": ["error_bar_value_type"] },
                { "required": ["errorBarValue"] },
                { "required": ["error_bar_value"] },
                { "required": ["errorBarNoEndCap"] },
                { "required": ["error_bar_no_end_cap"] },
                { "required": ["errorBarLineColor"] },
                { "required": ["errorBarLineColorHex"] },
                { "required": ["error_bar_line_color"] },
                { "required": ["error_bar_line_color_hex"] },
                { "required": ["errorBarLineWidth"] },
                { "required": ["error_bar_line_width"] },
                { "required": ["stockUpDownBarGapWidth"] },
                { "required": ["stock_up_down_bar_gap_width"] },
                { "required": ["stockUpBarFillColor"] },
                { "required": ["stock_up_bar_fill_color"] },
                { "required": ["stockDownBarFillColor"] },
                { "required": ["stock_down_bar_fill_color"] },
                { "required": ["stockUpBarLineColor"] },
                { "required": ["stock_up_bar_line_color"] },
                { "required": ["stockDownBarLineColor"] },
                { "required": ["stock_down_bar_line_color"] },
                { "required": ["stockUpBarLineWidth"] },
                { "required": ["stock_up_bar_line_width"] },
                { "required": ["stockDownBarLineWidth"] },
                { "required": ["stock_down_bar_line_width"] },
                { "required": ["stockHiLowLineColor"] },
                { "required": ["stock_hi_low_line_color"] },
                { "required": ["stockHiLowLineWidth"] },
                { "required": ["stock_hi_low_line_width"] },
                { "required": ["dataLabelPosition"] },
                { "required": ["data_label_position"] },
                { "required": ["dataLabelsShowValue"] },
                { "required": ["data_labels_show_value"] },
                { "required": ["dataLabelsShowCategoryName"] },
                { "required": ["data_labels_show_category_name"] },
                { "required": ["dataLabelsShowSeriesName"] },
                { "required": ["data_labels_show_series_name"] },
                { "required": ["dataLabelsShowPercent"] },
                { "required": ["data_labels_show_percent"] },
                { "required": ["dataLabelsShowLegendKey"] },
                { "required": ["data_labels_show_legend_key"] },
                { "required": ["displayBlanksAs"] },
                { "required": ["display_blanks_as"] },
                { "required": ["showHiddenData"] },
                { "required": ["show_hidden_data"] },
                { "required": ["plotVisibleOnly"] },
                { "required": ["plot_visible_only"] },
                { "required": ["dataTableShowHorizontalBorder"] },
                { "required": ["data_table_show_horizontal_border"] },
                { "required": ["dataTableShowVerticalBorder"] },
                { "required": ["data_table_show_vertical_border"] },
                { "required": ["dataTableShowOutline"] },
                { "required": ["data_table_show_outline"] },
                { "required": ["dataTableShowKeys"] },
                { "required": ["data_table_show_keys"] },
                { "required": ["legendPosition"] },
                { "required": ["legendOverlay"] },
                { "required": ["legend_overlay"] },
                { "required": ["categoryAxisTitle"] },
                { "required": ["category_axis_title"] },
                { "required": ["valueAxisTitle"] },
                { "required": ["value_axis_title"] },
                { "required": ["legendVisible"] },
                { "required": ["legend_visible"] },
                { "required": ["categoryAxisVisible"] },
                { "required": ["valueAxisVisible"] },
                { "required": ["categoryAxisPosition"] },
                { "required": ["category_axis_position"] },
                { "required": ["valueAxisPosition"] },
                { "required": ["value_axis_position"] },
                { "required": ["categoryAxisLabelPosition"] },
                { "required": ["valueAxisLabelPosition"] },
                { "required": ["categoryAxisAuto"] },
                { "required": ["category_axis_auto"] },
                { "required": ["categoryAxisLabelAlignment"] },
                { "required": ["category_axis_label_alignment"] },
                { "required": ["categoryAxisLabelOffset"] },
                { "required": ["category_axis_label_offset"] },
                { "required": ["categoryAxisTickMarkSkip"] },
                { "required": ["category_axis_tick_mark_skip"] },
                { "required": ["categoryAxisNoMultiLevelLabels"] },
                { "required": ["category_axis_no_multi_level_labels"] },
                { "required": ["categoryAxisOrientation"] },
                { "required": ["category_axis_orientation"] },
                { "required": ["valueAxisOrientation"] },
                { "required": ["value_axis_orientation"] },
                { "required": ["categoryAxisCrosses"] },
                { "required": ["category_axis_crosses"] },
                { "required": ["categoryAxisCrossesAt"] },
                { "required": ["category_axis_crosses_at"] },
                { "required": ["valueAxisCrosses"] },
                { "required": ["value_axis_crosses"] },
                { "required": ["valueAxisCrossesAt"] },
                { "required": ["value_axis_crosses_at"] },
                { "required": ["valueAxisCrossBetween"] },
                { "required": ["value_axis_cross_between"] },
                { "required": ["categoryAxisMajorTickMark"] },
                { "required": ["categoryAxisMinorTickMark"] },
                { "required": ["categoryAxisLineColor"] },
                { "required": ["categoryAxisLineWidth"] },
                { "required": ["categoryAxisMajorGridLineColor"] },
                { "required": ["categoryAxisMajorGridLineWidth"] },
                { "required": ["categoryAxisMinorGridLineColor"] },
                { "required": ["categoryAxisMinorGridLineWidth"] },
                { "required": ["valueAxisMajorTickMark"] },
                { "required": ["valueAxisMinorTickMark"] },
                { "required": ["valueAxisLineColor"] },
                { "required": ["valueAxisLineWidth"] },
                { "required": ["valueAxisMajorGridLineColor"] },
                { "required": ["valueAxisMajorGridLineWidth"] },
                { "required": ["valueAxisMinorGridLineColor"] },
                { "required": ["valueAxisMinorGridLineWidth"] },
                { "required": ["valueAxisMinimum"] },
                { "required": ["valueAxisMaximum"] },
                { "required": ["valueAxisMajorUnit"] },
                { "required": ["valueAxisMinorUnit"] },
                { "required": ["valueAxisLogBase"] },
                { "required": ["value_axis_log_base"] },
                { "required": ["valueAxisDisplayUnit"] },
                { "required": ["value_axis_display_unit"] },
                { "required": ["categoryAxisNumberFormat"] },
                { "required": ["categoryAxisNumberFormatSourceLinked"] },
                { "required": ["category_axis_number_format"] },
                { "required": ["category_axis_number_format_source_linked"] },
                { "required": ["valueAxisNumberFormat"] },
                { "required": ["valueAxisNumberFormatSourceLinked"] },
                { "required": ["value_axis_number_format"] },
                { "required": ["value_axis_number_format_source_linked"] },
                { "required": ["categories"] },
                { "required": ["series"] },
                { "required": ["xAxis"] },
                { "required": ["x_axis"] },
                { "required": ["yAxis"] },
                { "required": ["y_axis"] }
            ],
            "additionalProperties": false
        }),
    )
}

fn string_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "string",
            "description": description
        }),
    )
}

fn int_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "integer",
            "minimum": 0,
            "description": description
        }),
    )
}

fn signed_int_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "integer",
            "description": description
        }),
    )
}

fn number_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "number",
            "description": description
        }),
    )
}

fn bool_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "boolean",
            "description": description
        }),
    )
}

fn usize_array_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "array",
            "items": {
                "type": "integer",
                "minimum": 0
            },
            "description": description
        }),
    )
}

fn polygon_points_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "array",
            "description": description,
            "items": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "x": { "type": "integer" },
                            "y": { "type": "integer" }
                        },
                        "required": ["x", "y"],
                        "additionalProperties": false
                    },
                    {
                        "type": "array",
                        "prefixItems": [
                            { "type": "integer" },
                            { "type": "integer" }
                        ],
                        "minItems": 2,
                        "maxItems": 2
                    }
                ]
            }
        }),
    )
}

fn extension_from_path(path: &str) -> Option<String> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (_, extension) = file_name.rsplit_once('.')?;
    let extension = extension.trim().trim_start_matches('.');
    if extension.is_empty() {
        None
    } else {
        Some(extension.to_string())
    }
}

fn cell_path_prop(name: &'static str, description: &'static str) -> SchemaProp {
    (
        name,
        json!({
            "type": "array",
            "description": description,
            "items": {
                "oneOf": [
                    {
                        "type": "array",
                        "prefixItems": [
                            { "type": "integer", "minimum": 0 },
                            { "type": "integer", "minimum": 0 },
                            { "type": "integer", "minimum": 0 }
                        ],
                        "minItems": 3,
                        "maxItems": 3
                    },
                    {
                        "type": "object",
                        "properties": {
                            "control": { "type": "integer", "minimum": 0 },
                            "cell": { "type": "integer", "minimum": 0 },
                            "para": { "type": "integer", "minimum": 0 }
                        },
                        "required": ["control", "cell"],
                        "additionalProperties": false
                    }
                ]
            }
        }),
    )
}

fn section_para_props() -> Vec<SchemaProp> {
    section_para_props_with(Vec::new())
}

fn section_para_props_with(mut extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = vec![section_prop(), para_prop()];
    props.append(&mut extra);
    props
}

fn text_position_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = section_para_props();
    props.push(int_prop("char_offset", "Zero-based character offset."));
    props.extend(extra);
    props
}

fn field_position_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = text_position_props(vec![cell_path_prop(
        "cell_path",
        "Nested table cell or text-box path returned by rhwp_list_controls.",
    )]);
    props.extend(hf_optional_target_props(hf_para_props(Vec::new())));
    props.extend(extra);
    props
}

fn control_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = section_para_props();
    props.push(int_prop(
        "control",
        "Zero-based control index in the host paragraph.",
    ));
    props.extend(extra);
    props
}

fn table_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = control_target_props(vec![
        cell_path_prop(
            "cell_path",
            "Nested table cell path returned by rhwp_list_controls.",
        ),
        cell_path_prop("table_path", "Explicit nested table path."),
        cell_path_prop("tablePath", "CamelCase alias for table_path."),
        int_prop(
            "inner_control",
            "Nested table control index appended to cell_path.",
        ),
        int_prop("innerControl", "CamelCase alias for inner_control."),
        string_prop(
            "container_scope",
            "Nested control scope returned by rhwp_list_controls, such as header or footer.",
        ),
        string_prop("containerScope", "CamelCase alias for container_scope."),
        int_prop(
            "inner_para",
            "Nested paragraph index for a header/footer table target.",
        ),
        int_prop("innerPara", "CamelCase alias for inner_para."),
        int_prop("hf_para", "Header/footer paragraph index alias."),
        int_prop("hfPara", "CamelCase alias for hf_para."),
        int_prop("hf_para_idx", "Header/footer paragraph index alias."),
        int_prop("hfParaIndex", "CamelCase alias for hf_para_idx."),
    ]);
    props.extend(extra);
    props
}

fn table_insert_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = text_position_props(vec![
        cell_path_prop(
            "cell_path",
            "Nested container cell/text-box path returned by rhwp_list_controls.",
        ),
        string_prop(
            "container_scope",
            "Nested control scope returned by rhwp_list_controls, such as header or footer.",
        ),
        string_prop("containerScope", "CamelCase alias for container_scope."),
        int_prop(
            "control",
            "Header/footer control index in the host paragraph when inserting inside header/footer content.",
        ),
        int_prop(
            "inner_para",
            "Nested paragraph index for a header/footer insertion target.",
        ),
        int_prop("innerPara", "CamelCase alias for inner_para."),
        int_prop("hf_para", "Header/footer paragraph index alias."),
        int_prop("hfPara", "CamelCase alias for hf_para."),
        int_prop("hf_para_idx", "Header/footer paragraph index alias."),
        int_prop("hfParaIndex", "CamelCase alias for hf_para_idx."),
    ]);
    props.extend(extra);
    props
}

fn cell_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = table_target_props(vec![
        int_prop("row", "Zero-based row index."),
        int_prop("col", "Zero-based column index."),
        int_prop("cell", "Zero-based cell index."),
        int_prop("cell_para", "Paragraph index inside the target cell."),
    ]);
    props.extend(extra);
    props
}

fn format_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = section_para_props_with(vec![
        int_prop(
            "control",
            "Optional table control index when formatting a top-level table cell.",
        ),
        cell_path_prop(
            "cell_path",
            "Optional exact table cell path returned by rhwp_list_controls.",
        ),
        cell_path_prop(
            "table_path",
            "Optional nested table path. Combine with row/col or cell.",
        ),
        cell_path_prop("tablePath", "CamelCase alias for table_path."),
        int_prop(
            "inner_control",
            "Nested table control index appended to cell_path before selecting row/col.",
        ),
        int_prop("innerControl", "CamelCase alias for inner_control."),
        int_prop("row", "Zero-based table cell row when formatting a cell."),
        int_prop(
            "col",
            "Zero-based table cell column when formatting a cell.",
        ),
        int_prop(
            "cell",
            "Zero-based table cell index when formatting a top-level cell.",
        ),
        int_prop("cell_para", "Paragraph index inside the target cell."),
        bool_prop(
            "caption",
            "Target a top-level table or picture caption paragraph.",
        ),
        bool_prop("is_caption", "Alias for caption."),
        bool_prop("isCaption", "CamelCase alias for is_caption."),
        int_prop(
            "caption_para",
            "Paragraph index inside the target caption. Defaults to 0.",
        ),
        int_prop("captionPara", "CamelCase alias for caption_para."),
        int_prop("caption_para_idx", "Alias for caption_para."),
        int_prop("captionParaIndex", "CamelCase alias for caption_para_idx."),
        int_prop(
            "target_cell",
            "Zero-based cell index inside a nested target table.",
        ),
        int_prop("targetCell", "CamelCase alias for target_cell."),
        int_prop(
            "target_cell_para",
            "Paragraph index inside a nested target table cell. Defaults to 0.",
        ),
        int_prop("targetCellPara", "CamelCase alias for target_cell_para."),
        string_prop(
            "container_scope",
            "Header/footer table scope returned by rhwp_list_controls.",
        ),
        string_prop("containerScope", "CamelCase alias for container_scope."),
        int_prop(
            "hf_para",
            "Header/footer paragraph index when formatting a header/footer paragraph or table cell.",
        ),
        int_prop("hfPara", "CamelCase alias for hf_para."),
        int_prop("hf_para_idx", "Header/footer paragraph index alias."),
        int_prop("hfParaIndex", "CamelCase alias for hf_para_idx."),
    ]);
    props.extend(extra);
    props
}

fn picture_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = control_target_props(vec![
        cell_path_prop(
            "cell_path",
            "Nested table-cell or shape text-box path returned by rhwp_list_controls for a picture target.",
        ),
        cell_path_prop("table_path", "Explicit nested table path."),
        cell_path_prop("tablePath", "CamelCase alias for table_path."),
        int_prop(
            "inner_control",
            "Nested picture control index returned by rhwp_list_controls.",
        ),
        int_prop("innerControl", "CamelCase alias for inner_control."),
        string_prop(
            "container_scope",
            "Nested control scope returned by rhwp_list_controls, such as header or footer.",
        ),
        string_prop("containerScope", "CamelCase alias for container_scope."),
        int_prop(
            "inner_para",
            "Nested paragraph index for a header/footer picture target.",
        ),
        int_prop("innerPara", "CamelCase alias for inner_para."),
        int_prop("hf_para", "Header/footer paragraph index alias."),
        int_prop("hfPara", "CamelCase alias for hf_para."),
        int_prop("hf_para_idx", "Header/footer paragraph index alias."),
        int_prop("hfParaIndex", "CamelCase alias for hf_para_idx."),
        usize_array_prop(
            "group_child_path",
            "Nested picture child path inside a ShapeGroup returned by rhwp_list_controls.",
        ),
        usize_array_prop("groupChildPath", "CamelCase alias for group_child_path."),
    ]);
    props.extend(extra);
    props
}

fn shape_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = control_target_props(vec![
        cell_path_prop(
            "cell_path",
            "Nested table-cell or shape text-box path returned by rhwp_list_controls for a shape/OLE/chart target.",
        ),
        cell_path_prop("table_path", "Explicit nested table path."),
        cell_path_prop("tablePath", "CamelCase alias for table_path."),
        int_prop(
            "inner_control",
            "Nested shape/OLE/chart control index returned by rhwp_list_controls.",
        ),
        int_prop("innerControl", "CamelCase alias for inner_control."),
        string_prop(
            "container_scope",
            "Nested control scope returned by rhwp_list_controls, such as header or footer.",
        ),
        string_prop("containerScope", "CamelCase alias for container_scope."),
        int_prop(
            "inner_para",
            "Nested paragraph index for a header/footer shape target.",
        ),
        int_prop("innerPara", "CamelCase alias for inner_para."),
        int_prop("hf_para", "Header/footer paragraph index alias."),
        int_prop("hfPara", "CamelCase alias for hf_para."),
        int_prop("hf_para_idx", "Header/footer paragraph index alias."),
        int_prop("hfParaIndex", "CamelCase alias for hf_para_idx."),
        usize_array_prop(
            "group_child_path",
            "Nested child path inside a ShapeGroup returned by rhwp_list_controls.",
        ),
        usize_array_prop("groupChildPath", "CamelCase alias for group_child_path."),
    ]);
    props.extend(extra);
    props
}

fn note_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = control_target_props(vec![
        int_prop("note_para", "Paragraph index inside the note."),
        int_prop("notePara", "CamelCase alias for note_para."),
        int_prop("fn_para", "Footnote paragraph alias."),
        int_prop("fnPara", "CamelCase alias for fn_para."),
        int_prop("fn_para_idx", "Footnote paragraph index alias."),
        int_prop("fnParaIndex", "CamelCase alias for fn_para_idx."),
    ]);
    props.extend(extra);
    props
}

fn hidden_comment_target_props(extra: Vec<SchemaProp>) -> Vec<SchemaProp> {
    let mut props = control_target_props(vec![
        cell_path_prop(
            "cell_path",
            "Nested table-cell or shape text-box path returned by rhwp_list_controls for a hidden comment target.",
        ),
        cell_path_prop("table_path", "Explicit nested table/text-box path."),
        cell_path_prop("tablePath", "CamelCase alias for table_path."),
        int_prop(
            "inner_control",
            "Nested hidden comment control index returned by rhwp_list_controls.",
        ),
        int_prop("innerControl", "CamelCase alias for inner_control."),
        int_prop("hidden_para", "Paragraph index inside the hidden comment."),
        int_prop("hiddenPara", "CamelCase alias for hidden_para."),
        int_prop("hc_para", "Hidden comment paragraph alias."),
        int_prop("hcPara", "CamelCase alias for hc_para."),
    ]);
    props.extend(extra);
    props
}

fn compare_props(text_compare: bool) -> Vec<SchemaProp> {
    let mut props = vec![
        string_prop(
            "other_session_id",
            "Right-hand session id to compare against.",
        ),
        string_prop("otherSessionId", "CamelCase alias for other_session_id."),
        string_prop("other_path", "Right-hand file path to compare against."),
        string_prop("path", "Alias for other_path."),
    ];
    if text_compare {
        props.extend([
            bool_prop(
                "normalize_whitespace",
                "Collapse whitespace before text comparison.",
            ),
            bool_prop(
                "normalizeWhitespace",
                "CamelCase alias for normalize_whitespace.",
            ),
            int_prop("max_diffs", "Maximum text differences to include."),
            int_prop("maxDiffs", "CamelCase alias for max_diffs."),
        ]);
    }
    props
}

fn export_core_bytes(core: &mut DocumentCore, format: &str) -> Result<Vec<u8>, String> {
    match format {
        "hwp" => core.export_hwp_with_adapter().map_err(|e| e.to_string()),
        "hwpx" => core.export_hwpx_native().map_err(|e| e.to_string()),
        other => Err(format!("unsupported format: {other}; expected hwp or hwpx")),
    }
}

fn core_json(result: Result<String, crate::HwpError>) -> Result<Value, String> {
    let text = result.map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&text).unwrap_or_else(|_| json!({ "text": text })))
}

fn hwp(result: Result<String, crate::HwpError>) -> Result<String, String> {
    result.map_err(|e| e.to_string())
}

fn layout_overflow_json(overflow: &LayoutOverflow) -> Value {
    json!({
        "page": overflow.page_index,
        "section": overflow.section_index,
        "column": overflow.column_index,
        "para": overflow.para_index,
        "item_type": overflow.item_type,
        "is_first_in_column": overflow.is_first_in_column,
        "element_y": overflow.element_y,
        "column_bottom": overflow.column_bottom,
        "overflow_px": overflow.overflow_px,
        "message": overflow.to_string(),
    })
}

fn layout_overflow_json_with_context(
    side: &'static str,
    rendered_page: u32,
    overflow: &LayoutOverflow,
) -> Value {
    let mut value = layout_overflow_json(overflow);
    if let Some(object) = value.as_object_mut() {
        object.insert("side".to_string(), json!(side));
        object.insert("rendered_page".to_string(), json!(rendered_page));
    }
    value
}

fn layout_overflow_json_for_page(overflows: &[LayoutOverflow], page: u32) -> Vec<Value> {
    overflows
        .iter()
        .filter(|overflow| overflow.page_index == page)
        .map(layout_overflow_json)
        .collect()
}

fn layout_overflow_json_with_context_for_page(
    side: &'static str,
    rendered_page: u32,
    overflows: &[LayoutOverflow],
) -> Vec<Value> {
    overflows
        .iter()
        .filter(|overflow| overflow.page_index == rendered_page)
        .map(|overflow| layout_overflow_json_with_context(side, rendered_page, overflow))
        .collect()
}

fn style_raw_data_arg(args: &Map<String, Value>) -> Result<Option<Vec<u8>>, String> {
    let Some(encoded) =
        opt_str(args, "raw_hwp_style_base64").or_else(|| opt_str(args, "rawHwpStyleBase64"))
    else {
        return Ok(None);
    };
    let trimmed = encoded.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    BASE64
        .decode(trimmed)
        .map(Some)
        .map_err(|e| format!("invalid raw_hwp_style_base64: {e}"))
}

fn has_any_key(args: &Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().any(|key| args.contains_key(*key))
}

fn reject_raw_style_with_semantic_args(
    args: &Map<String, Value>,
    update: bool,
) -> Result<(), String> {
    let create_only = [
        "base_style_id",
        "baseStyleId",
        "based_on_style_id",
        "basedOnStyleId",
        "base_char_shape_id",
        "baseCharShapeId",
        "base_para_shape_id",
        "baseParaShapeId",
    ];
    let semantic = [
        "name",
        "english_name",
        "englishName",
        "type",
        "style_type",
        "next_style_id",
        "nextStyleId",
        "lang_id",
        "langId",
        "char_shape_id",
        "charShapeId",
        "para_shape_id",
        "paraShapeId",
        "char_format",
        "charFormat",
        "character_format",
        "para_format",
        "paraFormat",
        "paragraph_format",
    ];
    if has_any_key(args, &semantic) || (!update && has_any_key(args, &create_only)) {
        return Err(
            "raw_hwp_style_base64 cannot be combined with semantic style fields".to_string(),
        );
    }
    Ok(())
}

fn style_from_raw_hwp_payload(raw_data: Vec<u8>) -> Result<Style, String> {
    let mut reader = ByteReader::new(&raw_data);
    let local_name = reader
        .read_hwp_string()
        .map_err(|e| format!("invalid raw_hwp_style_base64 local_name: {e}"))?;
    let english_name = reader
        .read_hwp_string()
        .map_err(|e| format!("invalid raw_hwp_style_base64 english_name: {e}"))?;
    let style_type = reader
        .read_u8()
        .map_err(|e| format!("invalid raw_hwp_style_base64 style_type: {e}"))?;
    let next_style_id = reader
        .read_u8()
        .map_err(|e| format!("invalid raw_hwp_style_base64 next_style_id: {e}"))?;
    let lang_id = reader
        .read_i16()
        .map_err(|e| format!("invalid raw_hwp_style_base64 lang_id: {e}"))?;
    let para_shape_id = reader
        .read_u16()
        .map_err(|e| format!("invalid raw_hwp_style_base64 para_shape_id: {e}"))?;
    let char_shape_id = reader
        .read_u16()
        .map_err(|e| format!("invalid raw_hwp_style_base64 char_shape_id: {e}"))?;
    let _trailing = reader.read_u16().unwrap_or(0);
    Ok(Style {
        raw_data: Some(raw_data),
        local_name,
        english_name,
        style_type,
        next_style_id,
        lang_id,
        para_shape_id,
        char_shape_id,
    })
}

fn validate_next_style_id(
    next_style_id: u8,
    style_count: usize,
    context: &str,
) -> Result<(), String> {
    if next_style_id as usize >= style_count {
        return Err(format!(
            "{context} next_style_id out of range: {next_style_id} (style count: {style_count})"
        ));
    }
    Ok(())
}

fn validate_style_type_id(style_type: u8, label: &str) -> Result<(), String> {
    if style_type > 1 {
        return Err(format!(
            "{label} must be 0 (paragraph) or 1 (character): {style_type}"
        ));
    }
    Ok(())
}

fn style_u16_ref_arg(
    args: &Map<String, Value>,
    keys: &[&str],
    label: &str,
) -> Result<Option<u16>, String> {
    let Some(key) = keys.iter().find(|key| args.contains_key(**key)) else {
        return Ok(None);
    };
    let value = args
        .get(*key)
        .and_then(value_to_usize)
        .ok_or_else(|| format!("{label} must be a non-negative integer"))?;
    u16::try_from(value)
        .map(Some)
        .map_err(|_| format!("{label} out of range: {value}"))
}

fn style_u8_arg(
    args: &Map<String, Value>,
    keys: &[&str],
    label: &str,
) -> Result<Option<u8>, String> {
    let Some(key) = keys.iter().find(|key| args.contains_key(**key)) else {
        return Ok(None);
    };
    let value = args
        .get(*key)
        .and_then(value_to_usize)
        .ok_or_else(|| format!("{label} must be a non-negative integer"))?;
    u8::try_from(value)
        .map(Some)
        .map_err(|_| format!("{label} out of range: {value}"))
}

fn style_i16_arg(
    args: &Map<String, Value>,
    keys: &[&str],
    label: &str,
) -> Result<Option<i16>, String> {
    let Some(key) = keys.iter().find(|key| args.contains_key(**key)) else {
        return Ok(None);
    };
    let value = args
        .get(*key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{label} must be an integer"))?;
    i16::try_from(value)
        .map(Some)
        .map_err(|_| format!("{label} out of range: {value}"))
}

fn validate_style_refs(
    core: &DocumentCore,
    style: &Style,
    style_count: usize,
    context: &str,
) -> Result<(), String> {
    validate_style_type_id(style.style_type, &format!("{context} style_type"))?;
    validate_next_style_id(style.next_style_id, style_count, context)?;
    if style.para_shape_id as usize >= core.document.doc_info.para_shapes.len() {
        return Err(format!(
            "{context} para_shape_id out of range: {}",
            style.para_shape_id
        ));
    }
    if style.char_shape_id as usize >= core.document.doc_info.char_shapes.len() {
        return Err(format!(
            "{context} char_shape_id out of range: {}",
            style.char_shape_id
        ));
    }
    Ok(())
}

fn style_mutation_response(style_id: usize, style: &Style, include_raw: bool) -> Value {
    let mut response = json!({
        "ok": true,
        "style_id": style_id,
        "name": &style.local_name,
        "englishName": &style.english_name,
        "type": style.style_type,
        "nextStyleId": style.next_style_id,
        "langId": style.lang_id,
        "paraShapeId": style.para_shape_id,
        "charShapeId": style.char_shape_id,
    });
    if include_raw {
        if let (Some(object), Some(raw)) = (response.as_object_mut(), style.raw_data.as_ref()) {
            let encoded = BASE64.encode(raw);
            object.insert("rawHwpStyleBase64".to_string(), json!(encoded));
        }
    }
    response
}

fn style_list_json(core: &DocumentCore, include_formats: bool, include_raw: bool) -> Value {
    let styles = core
        .document
        .doc_info
        .styles
        .iter()
        .enumerate()
        .map(|(id, style)| {
            let mut value = json!({
                "id": id,
                "name": &style.local_name,
                "englishName": &style.english_name,
                "type": style.style_type,
                "nextStyleId": style.next_style_id,
                "langId": style.lang_id,
                "paraShapeId": style.para_shape_id,
                "charShapeId": style.char_shape_id,
            });
            if include_raw {
                if let Some(object) = value.as_object_mut() {
                    let encoded = style
                        .raw_data
                        .as_ref()
                        .map(|raw| BASE64.encode(raw))
                        .unwrap_or_default();
                    object.insert("rawHwpStyleBase64".to_string(), json!(encoded));
                    object.insert("raw_hwp_style_base64".to_string(), json!(encoded));
                }
            }
            if include_formats {
                if let Some(object) = value.as_object_mut() {
                    if let Some(shape) = core
                        .document
                        .doc_info
                        .char_shapes
                        .get(style.char_shape_id as usize)
                    {
                        object.insert("charFormat".to_string(), style_char_format_json(shape));
                    }
                    if let Some(shape) = core
                        .document
                        .doc_info
                        .para_shapes
                        .get(style.para_shape_id as usize)
                    {
                        object.insert("paraFormat".to_string(), style_para_format_json(shape));
                    }
                }
            }
            value
        })
        .collect::<Vec<_>>();
    json!(styles)
}

fn style_char_format_json(shape: &CharShape) -> Value {
    json!({
        "fontSize": shape.base_size,
        "bold": shape.bold,
        "italic": shape.italic,
        "underlineType": style_underline_type_name(shape.underline_type),
        "strikethrough": shape.strikethrough,
        "textColor": style_color_ref_to_css(shape.text_color),
        "shadeColor": style_color_ref_to_css(shape.shade_color),
        "borderFillId": shape.border_fill_id,
        "fontIds": shape.font_ids,
        "ratios": shape.ratios,
        "spacings": shape.spacings,
        "relativeSizes": shape.relative_sizes,
        "charOffsets": shape.char_offsets,
        "rawPreserved": shape.raw_data.is_some(),
    })
}

fn style_para_format_json(shape: &ParaShape) -> Value {
    json!({
        "alignment": style_alignment_name(shape.alignment),
        "lineSpacing": shape.line_spacing,
        "lineSpacingType": style_line_spacing_type_name(shape.line_spacing_type),
        "indent": shape.indent,
        "marginLeft": shape.margin_left,
        "marginRight": shape.margin_right,
        "spacingBefore": shape.spacing_before,
        "spacingAfter": shape.spacing_after,
        "tabDefId": shape.tab_def_id,
        "numberingId": shape.numbering_id,
        "borderFillId": shape.border_fill_id,
        "borderSpacing": shape.border_spacing,
        "lineSpacingV2": shape.line_spacing_v2,
        "headType": style_head_type_name(shape.head_type),
        "paraLevel": shape.para_level,
        "rawPreserved": shape.raw_data.is_some(),
    })
}

fn style_alignment_name(value: Alignment) -> &'static str {
    match value {
        Alignment::Left => "left",
        Alignment::Right => "right",
        Alignment::Center => "center",
        Alignment::Distribute => "distribute",
        Alignment::Justify | Alignment::Split => "justify",
    }
}

fn style_line_spacing_type_name(value: LineSpacingType) -> &'static str {
    match value {
        LineSpacingType::Fixed => "Fixed",
        LineSpacingType::SpaceOnly => "SpaceOnly",
        LineSpacingType::Minimum => "Minimum",
        LineSpacingType::Percent => "Percent",
    }
}

fn style_underline_type_name(value: UnderlineType) -> &'static str {
    match value {
        UnderlineType::Bottom => "Bottom",
        UnderlineType::Top => "Top",
        UnderlineType::None => "None",
    }
}

fn style_head_type_name(value: HeadType) -> &'static str {
    match value {
        HeadType::Outline => "outline",
        HeadType::Number => "number",
        HeadType::Bullet => "bullet",
        HeadType::None => "none",
    }
}

fn style_color_ref_to_css(color: u32) -> String {
    let r = color & 0xff;
    let g = (color >> 8) & 0xff;
    let b = (color >> 16) & 0xff;
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DocumentStructureProfile {
    page_count: u32,
    section_count: usize,
    body_paragraph_count: usize,
    paragraph_count: usize,
    text_char_count: usize,
    control_count: usize,
    table_count: usize,
    table_cell_count: usize,
    picture_count: usize,
    header_count: usize,
    footer_count: usize,
    footnote_count: usize,
    endnote_count: usize,
    hidden_comment_count: usize,
    field_count: usize,
    bookmark_count: usize,
    equation_count: usize,
    ole_count: usize,
    control_kinds: BTreeMap<String, usize>,
    control_scopes: BTreeMap<String, usize>,
    table_shapes: BTreeMap<String, usize>,
}

fn document_structure_profile(core: &DocumentCore) -> DocumentStructureProfile {
    let mut profile = DocumentStructureProfile {
        page_count: core.page_count(),
        section_count: core.document.sections.len(),
        ..Default::default()
    };
    for section in &core.document.sections {
        for para in &section.paragraphs {
            profile.body_paragraph_count += 1;
            profile_paragraph(para, "body", &mut profile);
        }
    }
    profile
}

fn profile_paragraph(
    para: &Paragraph,
    scope: &'static str,
    profile: &mut DocumentStructureProfile,
) {
    profile.paragraph_count += 1;
    profile.text_char_count += para.text.chars().count();
    for control in &para.controls {
        profile_control(control, scope, profile);
    }
}

fn profile_control(control: &Control, scope: &'static str, profile: &mut DocumentStructureProfile) {
    let kind = mcp_control_kind(control);
    profile.control_count += 1;
    *profile.control_kinds.entry(kind.to_string()).or_default() += 1;
    *profile.control_scopes.entry(scope.to_string()).or_default() += 1;

    match control {
        Control::Table(table) => {
            profile.table_count += 1;
            profile.table_cell_count += table.cells.len();
            *profile
                .table_shapes
                .entry(format!("{}x{}", table.row_count, table.col_count))
                .or_default() += 1;
            for cell in &table.cells {
                for para in &cell.paragraphs {
                    profile_paragraph(para, "table_cell", profile);
                }
            }
        }
        Control::Picture(pic) => {
            profile.picture_count += 1;
            if let Some(caption) = &pic.caption {
                for para in &caption.paragraphs {
                    profile_paragraph(para, "picture_caption", profile);
                }
            }
        }
        Control::Shape(shape) => {
            if matches!(shape.as_ref(), ShapeObject::Picture(_)) {
                profile.picture_count += 1;
            } else if matches!(shape.as_ref(), ShapeObject::Ole(_)) {
                profile.ole_count += 1;
            }
            if let Some(drawing) = shape.drawing() {
                if let Some(text_box) = &drawing.text_box {
                    for para in &text_box.paragraphs {
                        profile_paragraph(para, "shape_text_box", profile);
                    }
                }
                if let Some(caption) = &drawing.caption {
                    for para in &caption.paragraphs {
                        profile_paragraph(para, "shape_caption", profile);
                    }
                }
            }
        }
        Control::Header(header) => {
            profile.header_count += 1;
            for para in &header.paragraphs {
                profile_paragraph(para, "header", profile);
            }
        }
        Control::Footer(footer) => {
            profile.footer_count += 1;
            for para in &footer.paragraphs {
                profile_paragraph(para, "footer", profile);
            }
        }
        Control::Footnote(note) => {
            profile.footnote_count += 1;
            for para in &note.paragraphs {
                profile_paragraph(para, "footnote", profile);
            }
        }
        Control::Endnote(note) => {
            profile.endnote_count += 1;
            for para in &note.paragraphs {
                profile_paragraph(para, "endnote", profile);
            }
        }
        Control::HiddenComment(comment) => {
            profile.hidden_comment_count += 1;
            for para in &comment.paragraphs {
                profile_paragraph(para, "hidden_comment", profile);
            }
        }
        Control::Field(_) => {
            profile.field_count += 1;
        }
        Control::Bookmark(_) => {
            profile.bookmark_count += 1;
        }
        Control::Equation(_) => {
            profile.equation_count += 1;
        }
        _ => {}
    }
}

fn document_profile_json(
    label: &str,
    source_format: &str,
    profile: &DocumentStructureProfile,
) -> Value {
    json!({
        "label": label,
        "source_format": source_format,
        "page_count": profile.page_count,
        "section_count": profile.section_count,
        "body_paragraph_count": profile.body_paragraph_count,
        "paragraph_count": profile.paragraph_count,
        "text_char_count": profile.text_char_count,
        "control_count": profile.control_count,
        "table_count": profile.table_count,
        "table_cell_count": profile.table_cell_count,
        "picture_count": profile.picture_count,
        "header_count": profile.header_count,
        "footer_count": profile.footer_count,
        "footnote_count": profile.footnote_count,
        "endnote_count": profile.endnote_count,
        "hidden_comment_count": profile.hidden_comment_count,
        "field_count": profile.field_count,
        "bookmark_count": profile.bookmark_count,
        "equation_count": profile.equation_count,
        "ole_count": profile.ole_count,
        "control_kinds": &profile.control_kinds,
        "control_scopes": &profile.control_scopes,
        "table_shapes": &profile.table_shapes,
    })
}

fn document_profile_diff_json(
    left_label: &str,
    left_source_format: &str,
    left: &DocumentStructureProfile,
    right_label: &str,
    right_source_format: &str,
    right: &DocumentStructureProfile,
    max_diffs: usize,
    ignore_page_count: bool,
) -> Value {
    let mut differences = Vec::new();
    let mut difference_count = 0usize;

    macro_rules! compare_field {
        ($field:ident) => {
            push_profile_diff(
                &mut differences,
                &mut difference_count,
                max_diffs,
                stringify!($field),
                json!(left.$field),
                json!(right.$field),
            );
        };
    }

    if !ignore_page_count {
        compare_field!(page_count);
    }
    compare_field!(section_count);
    compare_field!(body_paragraph_count);
    compare_field!(paragraph_count);
    compare_field!(text_char_count);
    compare_field!(control_count);
    compare_field!(table_count);
    compare_field!(table_cell_count);
    compare_field!(picture_count);
    compare_field!(header_count);
    compare_field!(footer_count);
    compare_field!(footnote_count);
    compare_field!(endnote_count);
    compare_field!(hidden_comment_count);
    compare_field!(field_count);
    compare_field!(bookmark_count);
    compare_field!(equation_count);
    compare_field!(ole_count);
    compare_profile_map(
        &mut differences,
        &mut difference_count,
        max_diffs,
        "control_kinds",
        &left.control_kinds,
        &right.control_kinds,
    );
    compare_profile_map(
        &mut differences,
        &mut difference_count,
        max_diffs,
        "control_scopes",
        &left.control_scopes,
        &right.control_scopes,
    );
    compare_profile_map(
        &mut differences,
        &mut difference_count,
        max_diffs,
        "table_shapes",
        &left.table_shapes,
        &right.table_shapes,
    );

    json!({
        "equal": difference_count == 0,
        "left": document_profile_json(left_label, left_source_format, left),
        "right": document_profile_json(right_label, right_source_format, right),
        "difference_count": difference_count,
        "differences_truncated": difference_count > differences.len(),
        "differences": differences,
    })
}

fn style_signature_json(label: &str, source_format: &str, core: &DocumentCore) -> Value {
    let style_list = normalized_style_list(core);
    let template =
        crate::document_core::builders::document_template::extract_document_template(core);
    let template_styles = normalized_template_styles(&template);
    let template_value = serde_json::to_value(&template).unwrap_or_else(|_| json!({}));
    let mut style_refs = Vec::new();
    collect_template_style_refs(&template_value, &mut style_refs);
    let mut style_ref_counts: BTreeMap<String, usize> = BTreeMap::new();
    for style in &style_refs {
        let key = format!("{}:{}:{}", style.id, style.name, style.english_name);
        *style_ref_counts.entry(key).or_default() += 1;
    }
    let style_ref_counts_value =
        serde_json::to_value(&style_ref_counts).unwrap_or_else(|_| json!({}));
    let mut char_shape_ref_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut para_shape_ref_counts: BTreeMap<String, usize> = BTreeMap::new();
    collect_format_ref_counts(core, &mut char_shape_ref_counts, &mut para_shape_ref_counts);
    let char_shape_ref_counts_value =
        serde_json::to_value(&char_shape_ref_counts).unwrap_or_else(|_| json!({}));
    let para_shape_ref_counts_value =
        serde_json::to_value(&para_shape_ref_counts).unwrap_or_else(|_| json!({}));
    let template_stats =
        crate::document_core::builders::document_template::template_stats(&template);

    json!({
        "label": label,
        "source_format": source_format,
        "signature_version": "rhwp-style-signature-v1",
        "hash_algorithm": "blake3-json",
        "style_list_count": style_list.len(),
        "style_list_hash": hash_json(&style_list),
        "style_list_preview": style_list.iter().take(12).cloned().collect::<Vec<_>>(),
        "template_style_count": template_styles.len(),
        "template_style_hash": hash_json(&template_styles),
        "template_style_preview": template_styles.iter().take(12).cloned().collect::<Vec<_>>(),
        "style_ref_count": style_refs.len(),
        "style_ref_counts": style_ref_counts,
        "style_ref_hash": hash_json(&style_ref_counts_value),
        "char_shape_ref_count": char_shape_ref_counts.values().sum::<usize>(),
        "char_shape_ref_counts": char_shape_ref_counts,
        "char_shape_ref_hash": hash_json(&char_shape_ref_counts_value),
        "para_shape_ref_count": para_shape_ref_counts.values().sum::<usize>(),
        "para_shape_ref_counts": para_shape_ref_counts,
        "para_shape_ref_hash": hash_json(&para_shape_ref_counts_value),
        "template_stats": template_stats,
    })
}

fn style_signature_diff_json(left: Value, right: Value, max_diffs: usize) -> Value {
    let mut differences = Vec::new();
    let mut difference_count = 0usize;
    for field in [
        "signature_version",
        "hash_algorithm",
        "style_list_count",
        "style_list_hash",
        "template_style_count",
        "template_style_hash",
        "style_ref_count",
        "style_ref_hash",
        "char_shape_ref_count",
        "char_shape_ref_hash",
        "para_shape_ref_count",
        "para_shape_ref_hash",
    ] {
        let left_value = left.get(field).cloned().unwrap_or(Value::Null);
        let right_value = right.get(field).cloned().unwrap_or(Value::Null);
        if left_value != right_value {
            difference_count += 1;
            if differences.len() < max_diffs {
                differences.push(json!({
                    "field": field,
                    "left": left_value,
                    "right": right_value,
                }));
            }
        }
    }

    json!({
        "equal": difference_count == 0,
        "difference_count": difference_count,
        "differences_truncated": difference_count > differences.len(),
        "differences": differences,
        "left": left,
        "right": right,
    })
}

fn collect_format_ref_counts(
    core: &DocumentCore,
    char_shape_ref_counts: &mut BTreeMap<String, usize>,
    para_shape_ref_counts: &mut BTreeMap<String, usize>,
) {
    for section in &core.document.sections {
        collect_format_ref_counts_in_paragraphs(
            &section.paragraphs,
            &core.document.doc_info.char_shapes,
            &core.document.doc_info.para_shapes,
            char_shape_ref_counts,
            para_shape_ref_counts,
        );
        for master_page in &section.section_def.master_pages {
            collect_format_ref_counts_in_paragraphs(
                &master_page.paragraphs,
                &core.document.doc_info.char_shapes,
                &core.document.doc_info.para_shapes,
                char_shape_ref_counts,
                para_shape_ref_counts,
            );
        }
    }
}

fn collect_format_ref_counts_in_paragraphs(
    paragraphs: &[Paragraph],
    char_shapes: &[CharShape],
    para_shapes: &[ParaShape],
    char_shape_ref_counts: &mut BTreeMap<String, usize>,
    para_shape_ref_counts: &mut BTreeMap<String, usize>,
) {
    for paragraph in paragraphs {
        *para_shape_ref_counts
            .entry(para_shape_ref_key(para_shapes, paragraph.para_shape_id))
            .or_default() += 1;
        for char_ref in &paragraph.char_shapes {
            *char_shape_ref_counts
                .entry(char_shape_ref_key(char_shapes, char_ref.char_shape_id))
                .or_default() += 1;
        }
        for control in &paragraph.controls {
            collect_format_ref_counts_in_control(
                control,
                char_shapes,
                para_shapes,
                char_shape_ref_counts,
                para_shape_ref_counts,
            );
        }
    }
}

fn collect_format_ref_counts_in_control(
    control: &Control,
    char_shapes: &[CharShape],
    para_shapes: &[ParaShape],
    char_shape_ref_counts: &mut BTreeMap<String, usize>,
    para_shape_ref_counts: &mut BTreeMap<String, usize>,
) {
    match control {
        Control::Table(table) => {
            for cell in &table.cells {
                collect_format_ref_counts_in_paragraphs(
                    &cell.paragraphs,
                    char_shapes,
                    para_shapes,
                    char_shape_ref_counts,
                    para_shape_ref_counts,
                );
            }
        }
        Control::Picture(picture) => {
            if let Some(caption) = &picture.caption {
                collect_format_ref_counts_in_paragraphs(
                    &caption.paragraphs,
                    char_shapes,
                    para_shapes,
                    char_shape_ref_counts,
                    para_shape_ref_counts,
                );
            }
        }
        Control::Shape(shape) => {
            collect_format_ref_counts_in_shape(
                shape,
                char_shapes,
                para_shapes,
                char_shape_ref_counts,
                para_shape_ref_counts,
            );
        }
        Control::Header(header) => {
            collect_format_ref_counts_in_paragraphs(
                &header.paragraphs,
                char_shapes,
                para_shapes,
                char_shape_ref_counts,
                para_shape_ref_counts,
            );
        }
        Control::Footer(footer) => {
            collect_format_ref_counts_in_paragraphs(
                &footer.paragraphs,
                char_shapes,
                para_shapes,
                char_shape_ref_counts,
                para_shape_ref_counts,
            );
        }
        Control::Footnote(note) => {
            collect_format_ref_counts_in_paragraphs(
                &note.paragraphs,
                char_shapes,
                para_shapes,
                char_shape_ref_counts,
                para_shape_ref_counts,
            );
        }
        Control::Endnote(note) => {
            collect_format_ref_counts_in_paragraphs(
                &note.paragraphs,
                char_shapes,
                para_shapes,
                char_shape_ref_counts,
                para_shape_ref_counts,
            );
        }
        Control::HiddenComment(comment) => {
            collect_format_ref_counts_in_paragraphs(
                &comment.paragraphs,
                char_shapes,
                para_shapes,
                char_shape_ref_counts,
                para_shape_ref_counts,
            );
        }
        _ => {}
    }
}

fn collect_format_ref_counts_in_shape(
    shape: &ShapeObject,
    char_shapes: &[CharShape],
    para_shapes: &[ParaShape],
    char_shape_ref_counts: &mut BTreeMap<String, usize>,
    para_shape_ref_counts: &mut BTreeMap<String, usize>,
) {
    match shape {
        ShapeObject::Group(group) => {
            if let Some(caption) = &group.caption {
                collect_format_ref_counts_in_paragraphs(
                    &caption.paragraphs,
                    char_shapes,
                    para_shapes,
                    char_shape_ref_counts,
                    para_shape_ref_counts,
                );
            }
            for child in &group.children {
                collect_format_ref_counts_in_shape(
                    child,
                    char_shapes,
                    para_shapes,
                    char_shape_ref_counts,
                    para_shape_ref_counts,
                );
            }
        }
        ShapeObject::Picture(picture) => {
            if let Some(caption) = &picture.caption {
                collect_format_ref_counts_in_paragraphs(
                    &caption.paragraphs,
                    char_shapes,
                    para_shapes,
                    char_shape_ref_counts,
                    para_shape_ref_counts,
                );
            }
        }
        _ => {
            if let Some(drawing) = shape.drawing() {
                if let Some(text_box) = &drawing.text_box {
                    collect_format_ref_counts_in_paragraphs(
                        &text_box.paragraphs,
                        char_shapes,
                        para_shapes,
                        char_shape_ref_counts,
                        para_shape_ref_counts,
                    );
                }
                if let Some(caption) = &drawing.caption {
                    collect_format_ref_counts_in_paragraphs(
                        &caption.paragraphs,
                        char_shapes,
                        para_shapes,
                        char_shape_ref_counts,
                        para_shape_ref_counts,
                    );
                }
            }
        }
    }
}

fn char_shape_ref_key(char_shapes: &[CharShape], id: u32) -> String {
    if let Some(shape) = char_shapes.get(id as usize) {
        format!(
            "{}:{}:{}:{}:{}:{}:{}",
            id,
            shape.base_size,
            shape.bold,
            shape.italic,
            style_underline_type_name(shape.underline_type),
            shape.strikethrough,
            style_color_ref_to_css(shape.text_color)
        )
    } else {
        format!("{id}:missing")
    }
}

fn para_shape_ref_key(para_shapes: &[ParaShape], id: u16) -> String {
    if let Some(shape) = para_shapes.get(id as usize) {
        format!(
            "{}:{}:{}:{}:{}:{}:{}",
            id,
            style_alignment_name(shape.alignment),
            shape.margin_left,
            shape.margin_right,
            shape.spacing_before,
            shape.spacing_after,
            shape.line_spacing
        )
    } else {
        format!("{id}:missing")
    }
}

#[allow(clippy::too_many_arguments)]
fn fidelity_summary_json(
    left_label: &str,
    left_source_format: &str,
    left_core: &DocumentCore,
    right_label: &str,
    right_source_format: &str,
    right_core: &DocumentCore,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let normalize_whitespace = opt_bool(args, "normalize_whitespace")
        .or_else(|| opt_bool(args, "normalizeWhitespace"))
        .unwrap_or(false);
    let max_text_diffs = opt_usize(args, "max_text_diffs")
        .or_else(|| opt_usize(args, "maxTextDiffs"))
        .or_else(|| opt_usize(args, "max_diffs"))
        .or_else(|| opt_usize(args, "maxDiffs"))
        .unwrap_or(10);
    let max_profile_diffs = opt_usize(args, "max_profile_diffs")
        .or_else(|| opt_usize(args, "maxProfileDiffs"))
        .or_else(|| opt_usize(args, "max_diffs"))
        .or_else(|| opt_usize(args, "maxDiffs"))
        .unwrap_or(20);
    let max_style_diffs = opt_usize(args, "max_style_diffs")
        .or_else(|| opt_usize(args, "maxStyleDiffs"))
        .or_else(|| opt_usize(args, "max_diffs"))
        .or_else(|| opt_usize(args, "maxDiffs"))
        .unwrap_or(20);
    let max_deltas = opt_usize(args, "max_deltas")
        .or_else(|| opt_usize(args, "maxDeltas"))
        .unwrap_or(10);
    let max_disp_threshold = opt_f64(args, "max_disp_threshold")
        .or_else(|| opt_f64(args, "maxDispThreshold"))
        .unwrap_or(1.0);
    let max_png_changed_percent = opt_f64(args, "max_png_changed_percent")
        .or_else(|| opt_f64(args, "maxPngChangedPercent"))
        .unwrap_or(0.0);
    let ignore_page_count = opt_bool(args, "ignore_page_count")
        .or_else(|| opt_bool(args, "ignorePageCount"))
        .unwrap_or(false);
    let include_render_geometry = opt_bool(args, "include_render_geometry")
        .or_else(|| opt_bool(args, "includeRenderGeometry"))
        .unwrap_or(true);
    let include_render_png = opt_bool(args, "include_render_png")
        .or_else(|| opt_bool(args, "includeRenderPng"))
        .unwrap_or(false);
    let page_filter = opt_u32(args, "page");

    let left_text = extract_full_text(left_core)?;
    let right_text = extract_full_text(right_core)?;
    let left_cmp = comparable_text(&left_text, normalize_whitespace);
    let right_cmp = comparable_text(&right_text, normalize_whitespace);
    let text_difference_count = line_diff_count(&left_cmp, &right_cmp);
    let text_differences = line_diffs(&left_cmp, &right_cmp, max_text_diffs);
    let text_equal = left_cmp == right_cmp;
    let text_result = json!({
        "equal": text_equal,
        "normalize_whitespace": normalize_whitespace,
        "left": text_profile(left_label, left_core.page_count(), &left_text),
        "right": text_profile(right_label, right_core.page_count(), &right_text),
        "difference_count": text_difference_count,
        "differences_truncated": text_difference_count > text_differences.len(),
        "differences": text_differences,
    });

    let left_profile = document_structure_profile(left_core);
    let right_profile = document_structure_profile(right_core);
    let profile_result = document_profile_diff_json(
        left_label,
        left_source_format,
        &left_profile,
        right_label,
        right_source_format,
        &right_profile,
        max_profile_diffs,
        ignore_page_count,
    );
    let profile_equal = profile_result["equal"].as_bool().unwrap_or(false);

    let style_result = style_signature_diff_json(
        style_signature_json(left_label, left_source_format, left_core),
        style_signature_json(right_label, right_source_format, right_core),
        max_style_diffs,
    );
    let style_equal = style_result["equal"].as_bool().unwrap_or(false);

    let mut checks = Vec::new();
    push_fidelity_check(
        &mut checks,
        "text",
        text_equal,
        json!(text_difference_count),
        json!(0),
        true,
    );
    push_fidelity_check(
        &mut checks,
        "document_profile",
        profile_equal,
        json!(profile_result["difference_count"].clone()),
        json!(0),
        true,
    );
    push_fidelity_check(
        &mut checks,
        "style_signature",
        style_equal,
        json!(style_result["difference_count"].clone()),
        json!(0),
        true,
    );

    let render_geometry_result = if include_render_geometry {
        let diff = diff_render_geometry(left_core, right_core).map_err(|e| e.to_string())?;
        let value = geom_diff_json(
            left_label,
            right_label,
            &diff,
            max_deltas,
            max_disp_threshold,
            page_filter,
        );
        let within_threshold = value["within_threshold"].as_bool().unwrap_or(false);
        push_fidelity_check(
            &mut checks,
            "render_geometry",
            within_threshold,
            json!({
                "max_disp": value["max_disp"].clone(),
                "page_count_mismatch": value["page_count_mismatch"].clone(),
                "structure_mismatch": value["structure_mismatch"].clone(),
            }),
            json!({
                "max_disp": format!("<= {max_disp_threshold}"),
                "page_count_mismatch": false,
                "structure_mismatch": false,
            }),
            true,
        );
        value
    } else {
        json!({ "skipped": true })
    };

    let render_png_result = if include_render_png {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let left_page = opt_u32(args, "left_page")
                .or_else(|| opt_u32(args, "leftPage"))
                .or(page_filter)
                .unwrap_or(0);
            let right_page = opt_u32(args, "right_page")
                .or_else(|| opt_u32(args, "rightPage"))
                .unwrap_or(left_page);
            ensure_page_in_range("left", left_page, left_core.page_count())?;
            ensure_page_in_range("right", right_page, right_core.page_count())?;
            let value = compare_render_png_json(
                left_label,
                left_core,
                left_page,
                right_label,
                right_core,
                right_page,
            )?;
            let changed_percent = value["changed_percent"].as_f64().unwrap_or(f64::INFINITY);
            let same_size = value["same_size"].as_bool().unwrap_or(false);
            let within_threshold = same_size && changed_percent <= max_png_changed_percent;
            push_fidelity_check(
                &mut checks,
                "render_png",
                within_threshold,
                json!({
                    "changed_percent": value["changed_percent"].clone(),
                    "same_size": value["same_size"].clone(),
                }),
                json!({
                    "changed_percent": format!("<= {max_png_changed_percent}"),
                    "same_size": true,
                }),
                true,
            );
            value
        }
        #[cfg(target_arch = "wasm32")]
        {
            return Err(
                "rhwp_compare_fidelity_summary include_render_png is not available on wasm32"
                    .to_string(),
            );
        }
    } else {
        json!({ "skipped": true })
    };

    let mut summary = Map::new();
    summary.insert("text_equal".to_string(), json!(text_equal));
    summary.insert("document_profile_equal".to_string(), json!(profile_equal));
    summary.insert("style_signature_equal".to_string(), json!(style_equal));
    summary.insert(
        "render_geometry_included".to_string(),
        json!(include_render_geometry),
    );
    summary.insert(
        "render_geometry_within_threshold".to_string(),
        if include_render_geometry {
            render_geometry_result["within_threshold"].clone()
        } else {
            Value::Null
        },
    );
    summary.insert("render_png_included".to_string(), json!(include_render_png));
    summary.insert(
        "render_png_within_threshold".to_string(),
        if include_render_png {
            checks
                .iter()
                .find(|check| check["name"].as_str() == Some("render_png"))
                .and_then(|check| check["passed"].as_bool())
                .map(Value::Bool)
                .unwrap_or(Value::Null)
        } else {
            Value::Null
        },
    );
    summary.insert("package_included".to_string(), json!(false));

    let comparisons = json!({
        "text": text_result,
        "document_profile": profile_result,
        "style_signature": style_result,
        "render_geometry": render_geometry_result,
        "render_png": render_png_result,
        "package": { "skipped": true },
    });

    Ok(json!({
        "equal": fidelity_required_checks_passed(&checks),
        "summary": summary,
        "left": {
            "label": left_label,
            "source_format": left_source_format,
            "page_count": left_core.page_count(),
            "text_char_count": left_text.chars().count(),
        },
        "right": {
            "label": right_label,
            "source_format": right_source_format,
            "page_count": right_core.page_count(),
            "text_char_count": right_text.chars().count(),
        },
        "checks": checks,
        "comparisons": comparisons,
    }))
}

fn push_fidelity_check(
    checks: &mut Vec<Value>,
    name: &str,
    passed: bool,
    actual: Value,
    expected: Value,
    required: bool,
) {
    checks.push(json!({
        "name": name,
        "passed": passed,
        "required": required,
        "actual": actual,
        "expected": expected,
    }));
}

fn fidelity_required_checks_passed(checks: &[Value]) -> bool {
    checks.iter().all(|check| {
        check["required"].as_bool() != Some(true) || check["passed"].as_bool() == Some(true)
    })
}

fn append_fidelity_package_result(result: &mut Value, package: Value, strict_package: bool) {
    let package_equal = package["equal"].as_bool().unwrap_or(false);
    let package_structural_equal = package["structural_equal"].as_bool().unwrap_or(false);
    let package_drift_status = package["package_drift_status"].clone();
    let passed = if strict_package {
        package_equal
    } else {
        package_structural_equal
    };

    if let Some(summary) = result.get_mut("summary").and_then(Value::as_object_mut) {
        summary.insert("package_included".to_string(), json!(true));
        summary.insert("package_equal".to_string(), json!(package_equal));
        summary.insert(
            "package_structural_equal".to_string(),
            json!(package_structural_equal),
        );
        summary.insert(
            "package_drift_status".to_string(),
            package_drift_status.clone(),
        );
        summary.insert("strict_package".to_string(), json!(strict_package));
    }

    if let Some(comparisons) = result.get_mut("comparisons").and_then(Value::as_object_mut) {
        comparisons.insert("package".to_string(), package);
    }

    if let Some(checks) = result.get_mut("checks").and_then(Value::as_array_mut) {
        push_fidelity_check(
            checks,
            "hwpx_package",
            passed,
            json!({
                "equal": package_equal,
                "structural_equal": package_structural_equal,
                "package_drift_status": package_drift_status,
            }),
            if strict_package {
                json!({ "equal": true })
            } else {
                json!({ "structural_equal": true })
            },
            strict_package,
        );
        result["equal"] = json!(fidelity_required_checks_passed(checks));
    }
}

fn normalized_style_list(core: &DocumentCore) -> Vec<Value> {
    core.document
        .doc_info
        .styles
        .iter()
        .enumerate()
        .map(|(id, style)| {
            json!({
                "id": id,
                "name": &style.local_name,
                "englishName": &style.english_name,
                "type": style.style_type,
                "nextStyleId": style.next_style_id,
                "langId": style.lang_id,
                "paraShapeId": style.para_shape_id,
                "charShapeId": style.char_shape_id,
            })
        })
        .collect()
}

fn normalized_template_styles(template: &DocumentTemplate) -> Vec<Value> {
    template
        .styles
        .iter()
        .map(|style| {
            json!({
                "id": style.id,
                "name": &style.name,
                "english_name": &style.english_name,
                "style_type": style.style_type,
                "next_style_id": style.next_style_id,
                "lang_id": style.lang_id,
                "para_shape_id": style.para_shape_id,
                "char_shape_id": style.char_shape_id,
                "raw_hwp_style_hash": if style.raw_hwp_style_base64.is_empty() {
                    String::new()
                } else {
                    hash_bytes(style.raw_hwp_style_base64.as_bytes())
                },
            })
        })
        .collect()
}

#[derive(Debug)]
struct TemplateStyleRefSummary {
    id: i64,
    name: String,
    english_name: String,
}

fn collect_template_style_refs(value: &Value, refs: &mut Vec<TemplateStyleRefSummary>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_template_style_refs(item, refs);
            }
        }
        Value::Object(map) => {
            if let Some(style) = map.get("style").and_then(Value::as_object) {
                if let Some(id) = style.get("id").and_then(Value::as_i64) {
                    let name = style
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let english_name = style
                        .get("english_name")
                        .or_else(|| style.get("englishName"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    refs.push(TemplateStyleRefSummary {
                        id,
                        name,
                        english_name,
                    });
                }
            }
            for (key, child) in map {
                if key != "style" {
                    collect_template_style_refs(child, refs);
                }
            }
        }
        _ => {}
    }
}

fn hash_json<T: serde::Serialize + ?Sized>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    hash_bytes(&bytes)
}

fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn push_profile_diff(
    differences: &mut Vec<Value>,
    difference_count: &mut usize,
    max_diffs: usize,
    field: &str,
    left: Value,
    right: Value,
) {
    if left == right {
        return;
    }
    *difference_count += 1;
    if differences.len() < max_diffs {
        differences.push(json!({
            "field": field,
            "left": left,
            "right": right,
        }));
    }
}

fn compare_profile_map(
    differences: &mut Vec<Value>,
    difference_count: &mut usize,
    max_diffs: usize,
    field: &str,
    left: &BTreeMap<String, usize>,
    right: &BTreeMap<String, usize>,
) {
    let mut keys = BTreeSet::new();
    keys.extend(left.keys().cloned());
    keys.extend(right.keys().cloned());
    for key in keys {
        let left_value = left.get(&key).copied().unwrap_or(0);
        let right_value = right.get(&key).copied().unwrap_or(0);
        if left_value != right_value {
            push_profile_diff(
                differences,
                difference_count,
                max_diffs,
                &format!("{field}.{key}"),
                json!(left_value),
                json!(right_value),
            );
        }
    }
}

struct ControlWalk {
    section: usize,
    host_para: usize,
    scope: &'static str,
    path_prefix: String,
    container: Value,
    edit_target: Option<Value>,
}

fn collect_controls_from_paragraph(
    controls: &mut Vec<Value>,
    truncated: &mut bool,
    para: &Paragraph,
    walk: ControlWalk,
    include_nested: bool,
    kind_filter: Option<&str>,
    max_items: usize,
) {
    let positions = para.control_text_positions();
    for (control_idx, control) in para.controls.iter().enumerate() {
        let kind = mcp_control_kind(control);
        let path = format!("{}.control[{control_idx}]", walk.path_prefix);
        let mut edit_target = walk.edit_target.clone().unwrap_or_else(|| {
            json!({
                "section": walk.section,
                "para": walk.host_para,
                "control": control_idx,
            })
        });
        if walk.edit_target.is_some() {
            if let Some(target) = edit_target.as_object_mut() {
                target.insert("inner_control".to_string(), json!(control_idx));
                target.insert("inner_kind".to_string(), json!(kind));
            }
        }
        let shape_group_inspection_target = edit_target.clone();

        maybe_push_control(
            controls,
            truncated,
            kind_filter,
            max_items,
            kind,
            control_entry_json(
                control,
                kind,
                walk.section,
                walk.host_para,
                control_idx,
                positions.get(control_idx).copied(),
                walk.scope,
                &path,
                &walk.container,
                edit_target,
            ),
        );
        if !include_nested || *truncated {
            continue;
        }

        match control {
            Control::Table(table) => {
                for (cell_idx, cell) in table.cells.iter().enumerate() {
                    for (cell_para_idx, cell_para) in cell.paragraphs.iter().enumerate() {
                        let mut cell_path = edit_target_cell_path(&walk.edit_target);
                        cell_path.push((control_idx, cell_idx, cell_para_idx));
                        let mut nested_target = walk.edit_target.clone().unwrap_or_else(|| {
                            json!({
                                "section": walk.section,
                                "para": walk.host_para,
                                "control": control_idx,
                            })
                        });
                        if let Some(target) = nested_target.as_object_mut() {
                            target.insert("section".to_string(), json!(walk.section));
                            target.insert("para".to_string(), json!(walk.host_para));
                            target.insert("control".to_string(), json!(control_idx));
                            target.insert("cell".to_string(), json!(cell_idx));
                            target.insert("cell_para".to_string(), json!(cell_para_idx));
                            target.insert("cell_path".to_string(), cell_path_json(&cell_path));
                        }
                        collect_controls_from_paragraph(
                            controls,
                            truncated,
                            cell_para,
                            ControlWalk {
                                section: walk.section,
                                host_para: walk.host_para,
                                scope: "table_cell",
                                path_prefix: format!(
                                    "{path}.cell[{cell_idx}].para[{cell_para_idx}]"
                                ),
                                container: json!({
                                    "kind": "TableCell",
                                    "table_control": control_idx,
                                    "cell": cell_idx,
                                    "row": cell.row,
                                    "col": cell.col,
                                    "cell_para": cell_para_idx,
                                }),
                                edit_target: Some(nested_target),
                            },
                            include_nested,
                            kind_filter,
                            max_items,
                        );
                    }
                }
            }
            Control::Header(header) => collect_nested_paragraphs(
                controls,
                truncated,
                &header.paragraphs,
                &walk,
                "header",
                control_idx,
                &path,
                include_nested,
                kind_filter,
                max_items,
            ),
            Control::Footer(footer) => collect_nested_paragraphs(
                controls,
                truncated,
                &footer.paragraphs,
                &walk,
                "footer",
                control_idx,
                &path,
                include_nested,
                kind_filter,
                max_items,
            ),
            Control::Footnote(note) => collect_nested_paragraphs(
                controls,
                truncated,
                &note.paragraphs,
                &walk,
                "footnote",
                control_idx,
                &path,
                include_nested,
                kind_filter,
                max_items,
            ),
            Control::Endnote(note) => collect_nested_paragraphs(
                controls,
                truncated,
                &note.paragraphs,
                &walk,
                "endnote",
                control_idx,
                &path,
                include_nested,
                kind_filter,
                max_items,
            ),
            Control::HiddenComment(comment) => collect_nested_paragraphs(
                controls,
                truncated,
                &comment.paragraphs,
                &walk,
                "hidden_comment",
                control_idx,
                &path,
                include_nested,
                kind_filter,
                max_items,
            ),
            Control::Picture(pic) => {
                if let Some(caption) = &pic.caption {
                    collect_nested_paragraphs(
                        controls,
                        truncated,
                        &caption.paragraphs,
                        &walk,
                        "picture_caption",
                        control_idx,
                        &path,
                        include_nested,
                        kind_filter,
                        max_items,
                    );
                }
            }
            Control::Shape(shape) => {
                if let ShapeObject::Group(group) = shape.as_ref() {
                    collect_shape_group_children(
                        controls,
                        truncated,
                        &group.children,
                        &walk,
                        control_idx,
                        &path,
                        Some(&shape_group_inspection_target),
                        Vec::new(),
                        kind_filter,
                        max_items,
                    );
                    if *truncated {
                        continue;
                    }
                }
                if let Some(drawing) = shape.drawing() {
                    if let Some(text_box) = &drawing.text_box {
                        collect_nested_paragraphs(
                            controls,
                            truncated,
                            &text_box.paragraphs,
                            &walk,
                            "shape_text_box",
                            control_idx,
                            &path,
                            include_nested,
                            kind_filter,
                            max_items,
                        );
                    }
                    if let Some(caption) = &drawing.caption {
                        collect_nested_paragraphs(
                            controls,
                            truncated,
                            &caption.paragraphs,
                            &walk,
                            "shape_caption",
                            control_idx,
                            &path,
                            include_nested,
                            kind_filter,
                            max_items,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_shape_group_children(
    controls: &mut Vec<Value>,
    truncated: &mut bool,
    children: &[ShapeObject],
    walk: &ControlWalk,
    parent_control: usize,
    parent_path: &str,
    inspection_target: Option<&Value>,
    parent_child_path: Vec<usize>,
    kind_filter: Option<&str>,
    max_items: usize,
) {
    for (child_idx, child) in children.iter().enumerate() {
        let mut child_path = parent_child_path.clone();
        child_path.push(child_idx);
        let kind = mcp_shape_object_control_kind(child);
        let path_suffix = child_path
            .iter()
            .map(|idx| format!("child[{idx}]"))
            .collect::<Vec<_>>()
            .join(".");
        let path = format!("{parent_path}.{path_suffix}");
        maybe_push_control(
            controls,
            truncated,
            kind_filter,
            max_items,
            kind,
            shape_group_child_entry_json(
                child,
                kind,
                walk.section,
                walk.host_para,
                parent_control,
                &child_path,
                &path,
                inspection_target,
            ),
        );
        if *truncated {
            return;
        }
        if let ShapeObject::Group(group) = child {
            collect_shape_group_children(
                controls,
                truncated,
                &group.children,
                walk,
                parent_control,
                parent_path,
                inspection_target,
                child_path,
                kind_filter,
                max_items,
            );
            if *truncated {
                return;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_nested_paragraphs(
    controls: &mut Vec<Value>,
    truncated: &mut bool,
    paragraphs: &[Paragraph],
    parent: &ControlWalk,
    scope: &'static str,
    parent_control: usize,
    parent_path: &str,
    include_nested: bool,
    kind_filter: Option<&str>,
    max_items: usize,
) {
    for (para_idx, para) in paragraphs.iter().enumerate() {
        let mut edit_target = json!({
            "section": parent.section,
            "para": parent.host_para,
            "control": parent_control,
            "container_scope": scope,
            "inner_para": para_idx,
        });
        if matches!(scope, "header" | "footer") {
            if let Some(target) = edit_target.as_object_mut() {
                target.insert("hf_para".to_string(), json!(para_idx));
            }
        } else if matches!(scope, "footnote" | "endnote") {
            if let Some(target) = edit_target.as_object_mut() {
                target.insert("note_para".to_string(), json!(para_idx));
            }
        } else if scope == "shape_text_box" {
            if let Some(target) = edit_target.as_object_mut() {
                let mut cell_path = edit_target_cell_path(&parent.edit_target);
                cell_path.push((parent_control, 0, para_idx));
                target.insert("cell_path".to_string(), cell_path_json(&cell_path));
                target.insert("text_box_para".to_string(), json!(para_idx));
            }
        }
        collect_controls_from_paragraph(
            controls,
            truncated,
            para,
            ControlWalk {
                section: parent.section,
                host_para: parent.host_para,
                scope,
                path_prefix: format!("{parent_path}.{scope}.para[{para_idx}]"),
                container: json!({
                    "kind": scope,
                    "parent_control": parent_control,
                    "nested_para": para_idx,
                }),
                edit_target: Some(edit_target),
            },
            include_nested,
            kind_filter,
            max_items,
        );
    }
}

fn maybe_push_control(
    controls: &mut Vec<Value>,
    truncated: &mut bool,
    kind_filter: Option<&str>,
    max_items: usize,
    kind: &str,
    value: Value,
) {
    if kind_filter
        .map(|filter| filter != kind.to_ascii_lowercase())
        .unwrap_or(false)
    {
        return;
    }
    if controls.len() >= max_items {
        *truncated = true;
        return;
    }
    controls.push(value);
}

fn control_entry_json(
    control: &Control,
    kind: &str,
    section: usize,
    para: usize,
    control_idx: usize,
    char_offset: Option<usize>,
    scope: &str,
    path: &str,
    container: &Value,
    edit_target: Value,
) -> Value {
    json!({
        "section": section,
        "para": para,
        "control": control_idx,
        "char_offset": char_offset,
        "kind": kind,
        "scope": scope,
        "path": path,
        "container": container,
        "edit_target": edit_target,
        "summary": control_summary_json(control),
    })
}

fn shape_group_child_entry_json(
    shape: &ShapeObject,
    kind: &str,
    section: usize,
    para: usize,
    parent_control: usize,
    child_path: &[usize],
    path: &str,
    inspection_target: Option<&Value>,
) -> Value {
    let mut edit_target = inspection_target.cloned().unwrap_or(Value::Null);
    if let Some(target) = edit_target.as_object_mut() {
        target.insert("group_child".to_string(), json!(child_path.last().copied()));
        target.insert("group_child_path".to_string(), json!(child_path));
        target.insert("inner_kind".to_string(), json!(kind));
    }
    let read_only = edit_target.is_null();
    json!({
        "section": section,
        "para": para,
        "control": parent_control,
        "kind": kind,
        "scope": "shape_group_child",
        "path": path,
        "container": {
            "kind": "ShapeGroup",
            "parent_control": parent_control,
            "group_child": child_path.last().copied(),
            "group_child_path": child_path,
        },
        "edit_target": edit_target,
        "inspection_target": inspection_target.cloned().unwrap_or(Value::Null),
        "read_only": read_only,
        "summary": shape_object_summary_json(shape),
    })
}

fn shape_group_child_ref_for_mcp<'a>(
    shape: &'a ShapeObject,
    child_path: &[usize],
) -> Result<&'a ShapeObject, String> {
    let mut current = shape;
    for (depth, child_idx) in child_path.iter().copied().enumerate() {
        let ShapeObject::Group(group) = current else {
            return Err(format!(
                "group_child_path[{depth}] parent is not a ShapeGroup"
            ));
        };
        current = group
            .children
            .get(child_idx)
            .ok_or_else(|| format!("group_child_path[{depth}]={child_idx} out of range"))?;
    }
    Ok(current)
}

fn mcp_control_kind(control: &Control) -> &'static str {
    match control {
        Control::SectionDef(_) => "SectionDef",
        Control::ColumnDef(_) => "ColumnDef",
        Control::Table(_) => "Table",
        Control::Shape(shape) => match shape.as_ref() {
            ShapeObject::Picture(_) => "Picture",
            ShapeObject::Chart(_) => "Chart",
            ShapeObject::Ole(_) => "Ole",
            ShapeObject::Group(_) => "ShapeGroup",
            _ => "Shape",
        },
        Control::Picture(_) => "Picture",
        Control::Header(_) => "Header",
        Control::Footer(_) => "Footer",
        Control::Footnote(_) => "Footnote",
        Control::Endnote(_) => "Endnote",
        Control::AutoNumber(_) => "AutoNumber",
        Control::NewNumber(_) => "NewNumber",
        Control::PageNumberPos(_) => "PageNumberPos",
        Control::Bookmark(_) => "Bookmark",
        Control::Hyperlink(_) => "Hyperlink",
        Control::Ruby(_) => "Ruby",
        Control::CharOverlap(_) => "CharOverlap",
        Control::PageHide(_) => "PageHide",
        Control::HiddenComment(_) => "HiddenComment",
        Control::Equation(_) => "Equation",
        Control::Field(_) => "Field",
        Control::Form(_) => "Form",
        Control::Unknown(_) => "Unknown",
    }
}

fn mcp_shape_object_control_kind(shape: &ShapeObject) -> &'static str {
    match shape {
        ShapeObject::Picture(_) => "Picture",
        ShapeObject::Chart(_) => "Chart",
        ShapeObject::Ole(_) => "Ole",
        ShapeObject::Group(_) => "ShapeGroup",
        _ => "Shape",
    }
}

fn control_summary_json(control: &Control) -> Value {
    match control {
        Control::Table(table) => json!({
            "row_count": table.row_count,
            "col_count": table.col_count,
            "cell_count": table.cells.len(),
            "treat_as_char": table.common.treat_as_char,
            "width": table.common.width,
            "height": table.common.height,
        }),
        Control::Picture(pic) => picture_summary_json(pic),
        Control::Shape(shape) => match shape.as_ref() {
            ShapeObject::Picture(pic) => picture_summary_json(pic),
            other => shape_object_summary_json(other),
        },
        Control::Equation(eq) => json!({
            "script": eq.script,
            "font_size": eq.font_size,
            "color": eq.color,
            "baseline": eq.baseline,
            "width": eq.common.width,
            "height": eq.common.height,
            "treat_as_char": eq.common.treat_as_char,
        }),
        Control::Footnote(note) => json!({
            "number": note.number,
            "paragraph_count": note.paragraphs.len(),
        }),
        Control::Endnote(note) => json!({
            "number": note.number,
            "paragraph_count": note.paragraphs.len(),
        }),
        Control::HiddenComment(comment) => json!({
            "paragraph_count": comment.paragraphs.len(),
            "text_preview": comment
                .paragraphs
                .iter()
                .map(|para| para.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                .chars()
                .take(120)
                .collect::<String>(),
        }),
        Control::Bookmark(bookmark) => json!({
            "name": bookmark.name,
        }),
        Control::Field(field) => json!({
            "field_type": format!("{:?}", field.field_type),
            "field_id": field.field_id,
            "command": field.command,
            "name": field.field_name(),
        }),
        Control::Header(header) => json!({
            "paragraph_count": header.paragraphs.len(),
        }),
        Control::Footer(footer) => json!({
            "paragraph_count": footer.paragraphs.len(),
        }),
        Control::Hyperlink(link) => json!({
            "url": link.url,
            "text": link.text,
        }),
        Control::Ruby(ruby) => json!({
            "main_text": ruby.main_text,
            "ruby_text": ruby.ruby_text,
            "alignment": ruby.alignment,
            "pos_type": ruby.pos_type,
            "align": ruby.align,
            "size_ratio": ruby.size_ratio,
            "option": ruby.option,
            "style_id_ref": ruby.style_id_ref,
        }),
        Control::CharOverlap(co) => json!({
            "text": co.chars.iter().collect::<String>(),
            "border_type": co.border_type,
            "inner_char_size": co.inner_char_size,
            "expansion": co.expansion,
            "char_shape_count": co.char_shape_ids.len(),
        }),
        Control::Unknown(unknown) => json!({
            "ctrl_id": unknown.ctrl_id,
        }),
        _ => json!({}),
    }
}

fn picture_summary_json(pic: &crate::model::image::Picture) -> Value {
    json!({
        "treat_as_char": pic.common.treat_as_char,
        "width": pic.common.width,
        "height": pic.common.height,
        "description": pic.common.description,
        "bin_data_id": pic.image_attr.bin_data_id,
        "transparency": pic.image_attr.clamped_transparency(),
        "has_caption": pic.caption.is_some(),
    })
}

fn shape_object_summary_json(shape: &ShapeObject) -> Value {
    let common = shape.common();
    let attr = shape.shape_attr();
    let caption = shape_object_caption(shape);
    let caption_text_preview = caption.map(|caption| {
        caption
            .paragraphs
            .iter()
            .map(|para| para.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .take(120)
            .collect::<String>()
    });
    json!({
        "shape_kind": shape_object_kind(shape),
        "treat_as_char": common.treat_as_char,
        "width": common.width,
        "height": common.height,
        "horizontal_offset": common.horizontal_offset,
        "vertical_offset": common.vertical_offset,
        "z_order": common.z_order,
        "description": common.description.as_str(),
        "group_level": attr.group_level,
        "group_offset_x": attr.offset_x,
        "group_offset_y": attr.offset_y,
        "rotation_angle": attr.rotation_angle,
        "horz_flip": attr.horz_flip,
        "vert_flip": attr.vert_flip,
        "has_caption": caption.is_some(),
        "caption_direction": caption.map(|caption| caption_direction_summary(caption.direction)),
        "caption_vert_align": caption.map(|caption| caption_vert_align_summary(caption.vert_align)),
        "caption_paragraph_count": caption.map_or(0, |caption| caption.paragraphs.len()),
        "caption_text_preview": caption_text_preview,
    })
}

fn shape_object_caption(shape: &ShapeObject) -> Option<&crate::model::shape::Caption> {
    match shape {
        ShapeObject::Line(s) => s.drawing.caption.as_ref(),
        ShapeObject::Rectangle(s) => s.drawing.caption.as_ref(),
        ShapeObject::Ellipse(s) => s.drawing.caption.as_ref(),
        ShapeObject::Arc(s) => s.drawing.caption.as_ref(),
        ShapeObject::Polygon(s) => s.drawing.caption.as_ref(),
        ShapeObject::Curve(s) => s.drawing.caption.as_ref(),
        ShapeObject::Group(s) => s.caption.as_ref(),
        ShapeObject::Picture(s) => s.caption.as_ref(),
        ShapeObject::Chart(s) => s.caption.as_ref(),
        ShapeObject::Ole(s) => s.caption.as_ref(),
    }
}

fn caption_direction_summary(direction: crate::model::shape::CaptionDirection) -> &'static str {
    match direction {
        crate::model::shape::CaptionDirection::Left => "Left",
        crate::model::shape::CaptionDirection::Right => "Right",
        crate::model::shape::CaptionDirection::Top => "Top",
        crate::model::shape::CaptionDirection::Bottom => "Bottom",
    }
}

fn caption_vert_align_summary(vert_align: crate::model::shape::CaptionVertAlign) -> &'static str {
    match vert_align {
        crate::model::shape::CaptionVertAlign::Top => "Top",
        crate::model::shape::CaptionVertAlign::Center => "Center",
        crate::model::shape::CaptionVertAlign::Bottom => "Bottom",
    }
}

fn shape_object_kind(shape: &ShapeObject) -> &'static str {
    match shape {
        ShapeObject::Line(_) => "Line",
        ShapeObject::Rectangle(_) => "Rectangle",
        ShapeObject::Ellipse(_) => "Ellipse",
        ShapeObject::Arc(_) => "Arc",
        ShapeObject::Polygon(_) => "Polygon",
        ShapeObject::Curve(_) => "Curve",
        ShapeObject::Group(_) => "Group",
        ShapeObject::Picture(_) => "Picture",
        ShapeObject::Chart(_) => "Chart",
        ShapeObject::Ole(_) => "Ole",
    }
}

struct CompareSide {
    label: String,
    page_count: u32,
    text: String,
}

struct RenderPageSignature {
    svg_bytes: usize,
    text: String,
    text_node_count: usize,
    text_tokens: BTreeMap<String, u32>,
    structure_tokens: BTreeMap<String, u32>,
}

fn ensure_page_in_range(label: &str, page: u32, page_count: u32) -> Result<(), String> {
    if page < page_count {
        return Ok(());
    }
    Err(format!(
        "{label} page {page} out of range 0..{}",
        page_count.saturating_sub(1)
    ))
}

fn match_render_pages_json(
    left_label: &str,
    left_page_count: u32,
    source_page: u32,
    source: &RenderPageSignature,
    source_overflows: Vec<LayoutOverflow>,
    right_label: &str,
    right: &DocumentCore,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let right_page_count = right.page_count();
    let start_page = opt_u32(args, "start_page")
        .or_else(|| opt_u32(args, "startPage"))
        .unwrap_or(0);
    let end_page_raw = opt_u32(args, "end_page")
        .or_else(|| opt_u32(args, "endPage"))
        .unwrap_or_else(|| right_page_count.saturating_sub(1));
    ensure_page_in_range("candidate start", start_page, right_page_count)?;
    let end_page = end_page_raw.min(right_page_count.saturating_sub(1));
    if end_page < start_page {
        return Err(format!(
            "candidate end_page {end_page} is before start_page {start_page}"
        ));
    }
    let max_matches = opt_usize(args, "max_matches")
        .or_else(|| opt_usize(args, "maxMatches"))
        .unwrap_or(5)
        .clamp(1, 100);

    let mut matches = Vec::new();
    let mut document_layout_overflow_count = source_overflows.len();
    let mut layout_overflows =
        layout_overflow_json_with_context_for_page("left", source_page, &source_overflows);
    for page in start_page..=end_page {
        let (svg, overflows) = right
            .render_page_svg_native_with_overflows(page)
            .map_err(|e| format!("failed to render candidate page {page}: {e}"))?;
        document_layout_overflow_count += overflows.len();
        layout_overflows.extend(layout_overflow_json_with_context_for_page(
            "right", page, &overflows,
        ));
        let candidate = render_page_signature(&svg);
        matches.push(render_page_match_json(page, source, &candidate));
    }
    matches.sort_by(|a, b| {
        let a_score = a["score"].as_f64().unwrap_or(0.0);
        let b_score = b["score"].as_f64().unwrap_or(0.0);
        b_score
            .partial_cmp(&a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a["page"]
                    .as_u64()
                    .unwrap_or(0)
                    .cmp(&b["page"].as_u64().unwrap_or(0))
            })
    });
    matches.truncate(max_matches);

    Ok(json!({
        "left": {
            "label": left_label,
            "page_count": left_page_count,
        },
        "right": {
            "label": right_label,
            "page_count": right_page_count,
        },
        "source": {
            "page": source_page,
            "svg_bytes": source.svg_bytes,
            "text_node_count": source.text_node_count,
            "text_char_count": source.text.chars().count(),
            "text_preview": preview_chars(&source.text, 160),
        },
        "candidate_range": {
            "start_page": start_page,
            "end_page": end_page,
        },
        "scoring": {
            "score": "0.55*text_similarity + 0.35*structure_similarity + 0.10*svg_length_ratio",
            "note": "Render page matching is a diagnostic heuristic, not a pixel-perfect visual diff."
        },
        "layout_overflow_count": layout_overflows.len(),
        "layout_overflows": layout_overflows,
        "document_layout_overflow_count": document_layout_overflow_count,
        "matches": matches,
    }))
}

#[cfg(not(target_arch = "wasm32"))]
fn compare_render_png_json(
    left_label: &str,
    left: &DocumentCore,
    left_page: u32,
    right_label: &str,
    right: &DocumentCore,
    right_page: u32,
) -> Result<Value, String> {
    let (left_png, left_width, left_height, left_overflows) = left
        .render_page_png_from_svg_native_with_overflows(left_page)
        .map_err(|e| format!("failed to render left page {left_page}: {e}"))?;
    let (right_png, right_width, right_height, right_overflows) = right
        .render_page_png_from_svg_native_with_overflows(right_page)
        .map_err(|e| format!("failed to render right page {right_page}: {e}"))?;
    let metrics = diff_png_rgba(&left_png, &right_png)?;
    let layout_overflows =
        layout_overflow_json_with_context_for_page("left", left_page, &left_overflows)
            .into_iter()
            .chain(layout_overflow_json_with_context_for_page(
                "right",
                right_page,
                &right_overflows,
            ))
            .collect::<Vec<_>>();
    Ok(json!({
        "left": {
            "label": left_label,
            "page": left_page,
            "page_count": left.page_count(),
            "width_px": left_width,
            "height_px": left_height,
            "byte_length": left_png.len(),
        },
        "right": {
            "label": right_label,
            "page": right_page,
            "page_count": right.page_count(),
            "width_px": right_width,
            "height_px": right_height,
            "byte_length": right_png.len(),
        },
        "renderer": "svg-resvg",
        "same_size": metrics.same_size,
        "width_px": metrics.width,
        "height_px": metrics.height,
        "overlap_width_px": metrics.overlap_width,
        "overlap_height_px": metrics.overlap_height,
        "pixel_count": metrics.pixel_count,
        "overlap_pixel_count": metrics.overlap_pixel_count,
        "changed_pixels": metrics.changed_pixels,
        "changed_percent": metrics.changed_percent,
        "mean_abs_diff": metrics.mean_abs_diff,
        "max_channel_abs_diff": metrics.max_channel_abs_diff,
        "size_mismatch_extra_pixels": metrics.size_mismatch_extra_pixels,
        "layout_overflow_count": layout_overflows.len(),
        "layout_overflows": layout_overflows,
        "document_layout_overflow_count": left_overflows.len() + right_overflows.len(),
    }))
}

#[cfg(not(target_arch = "wasm32"))]
struct PngDiffMetrics {
    same_size: bool,
    width: u32,
    height: u32,
    overlap_width: u32,
    overlap_height: u32,
    pixel_count: u64,
    overlap_pixel_count: u64,
    changed_pixels: u64,
    changed_percent: f64,
    mean_abs_diff: f64,
    max_channel_abs_diff: u8,
    size_mismatch_extra_pixels: u64,
}

#[cfg(not(target_arch = "wasm32"))]
fn diff_png_rgba(left_png: &[u8], right_png: &[u8]) -> Result<PngDiffMetrics, String> {
    let left = image::load_from_memory(left_png)
        .map_err(|e| format!("failed to decode left PNG: {e}"))?
        .to_rgba8();
    let right = image::load_from_memory(right_png)
        .map_err(|e| format!("failed to decode right PNG: {e}"))?
        .to_rgba8();
    let left_width = left.width();
    let left_height = left.height();
    let right_width = right.width();
    let right_height = right.height();
    let width = left_width.max(right_width);
    let height = left_height.max(right_height);
    let overlap_width = left_width.min(right_width);
    let overlap_height = left_height.min(right_height);
    let pixel_count = u64::from(width).saturating_mul(u64::from(height));
    let overlap_pixel_count = u64::from(overlap_width).saturating_mul(u64::from(overlap_height));
    if pixel_count == 0 {
        return Err("cannot compare empty PNG images".to_string());
    }

    let mut changed_pixels = 0u64;
    let mut total_abs_diff = 0u64;
    let mut max_channel_abs_diff = 0u8;
    for y in 0..overlap_height {
        for x in 0..overlap_width {
            let left_px = left.get_pixel(x, y).0;
            let right_px = right.get_pixel(x, y).0;
            let mut pixel_changed = false;
            for idx in 0..4 {
                let diff = left_px[idx].abs_diff(right_px[idx]);
                if diff > 0 {
                    pixel_changed = true;
                    max_channel_abs_diff = max_channel_abs_diff.max(diff);
                }
                total_abs_diff += u64::from(diff);
            }
            if pixel_changed {
                changed_pixels += 1;
            }
        }
    }

    let size_mismatch_extra_pixels = pixel_count.saturating_sub(overlap_pixel_count);
    changed_pixels += size_mismatch_extra_pixels;
    total_abs_diff += size_mismatch_extra_pixels
        .saturating_mul(4)
        .saturating_mul(255);
    if size_mismatch_extra_pixels > 0 {
        max_channel_abs_diff = 255;
    }

    Ok(PngDiffMetrics {
        same_size: left_width == right_width && left_height == right_height,
        width,
        height,
        overlap_width,
        overlap_height,
        pixel_count,
        overlap_pixel_count,
        changed_pixels,
        changed_percent: round4((changed_pixels as f64 / pixel_count as f64) * 100.0),
        mean_abs_diff: round4(total_abs_diff as f64 / (pixel_count as f64 * 4.0)),
        max_channel_abs_diff,
        size_mismatch_extra_pixels,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn render_page_match_json(
    page: u32,
    source: &RenderPageSignature,
    candidate: &RenderPageSignature,
) -> Value {
    let text_similarity = cosine_similarity(&source.text_tokens, &candidate.text_tokens);
    let structure_similarity =
        cosine_similarity(&source.structure_tokens, &candidate.structure_tokens);
    let svg_length_ratio = length_ratio(source.svg_bytes, candidate.svg_bytes);
    let score = 0.55 * text_similarity + 0.35 * structure_similarity + 0.10 * svg_length_ratio;
    json!({
        "page": page,
        "score": score,
        "text_similarity": text_similarity,
        "structure_similarity": structure_similarity,
        "svg_length_ratio": svg_length_ratio,
        "svg_bytes": candidate.svg_bytes,
        "text_node_count": candidate.text_node_count,
        "text_char_count": candidate.text.chars().count(),
        "text_preview": preview_chars(&candidate.text, 160),
    })
}

fn render_page_signature(svg: &str) -> RenderPageSignature {
    let mut reader = XmlReader::from_str(svg);
    let mut buf = Vec::new();
    let mut text = String::new();
    let mut text_node_count = 0usize;
    let mut structure_tokens = BTreeMap::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(ref event)) | Ok(XmlEvent::Empty(ref event)) => {
                collect_svg_element_tokens(event, &mut structure_tokens);
            }
            Ok(XmlEvent::Text(ref event)) => {
                let decoded = event.decode().unwrap_or_default();
                let visible = decoded.trim();
                if !visible.is_empty() {
                    text.push_str(visible);
                    text_node_count += 1;
                }
            }
            Ok(XmlEvent::CData(ref event)) => {
                let decoded = String::from_utf8_lossy(event.as_ref());
                let visible = decoded.trim();
                if !visible.is_empty() {
                    text.push_str(visible);
                    text_node_count += 1;
                }
            }
            Ok(XmlEvent::Eof) => break,
            Err(_) => {
                collect_plain_svg_tokens(svg, &mut structure_tokens);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    let text_tokens = text_similarity_tokens(&text);
    RenderPageSignature {
        svg_bytes: svg.len(),
        text,
        text_node_count,
        text_tokens,
        structure_tokens,
    }
}

fn collect_svg_element_tokens(
    event: &quick_xml::events::BytesStart<'_>,
    tokens: &mut BTreeMap<String, u32>,
) {
    let tag = String::from_utf8_lossy(event.name().as_ref()).to_ascii_lowercase();
    bump_token(tokens, format!("tag:{tag}"));
    for attr in event.attributes().flatten() {
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_ascii_lowercase();
        bump_token(tokens, format!("attr:{key}"));
        let value = String::from_utf8_lossy(attr.value.as_ref());
        collect_svg_attr_value_tokens(&key, &value, tokens);
    }
}

fn collect_svg_attr_value_tokens(key: &str, value: &str, tokens: &mut BTreeMap<String, u32>) {
    match key {
        "x" | "y" | "width" | "height" | "x1" | "y1" | "x2" | "y2" | "cx" | "cy" | "r" | "rx"
        | "ry" | "font-size" | "textlength" => {
            if let Some(number) = parse_leading_number(value) {
                let bin = (number / 4.0).round() * 4.0;
                bump_token(tokens, format!("num:{key}:{bin:.0}"));
            }
        }
        "font-weight" | "fill" | "stroke" | "preserveaspectratio" => {
            let normalized = value.trim().to_ascii_lowercase();
            if !normalized.is_empty() && normalized.len() <= 64 {
                bump_token(tokens, format!("value:{key}:{normalized}"));
            }
        }
        _ => {}
    }
}

fn collect_plain_svg_tokens(svg: &str, tokens: &mut BTreeMap<String, u32>) {
    let mut current = String::new();
    for ch in svg.chars() {
        if ch.is_ascii_alphabetic() || ch == '-' || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            if current.len() >= 2 {
                bump_token(tokens, format!("raw:{current}"));
            }
            current.clear();
        }
    }
    if current.len() >= 2 {
        bump_token(tokens, format!("raw:{current}"));
    }
}

fn text_similarity_tokens(text: &str) -> BTreeMap<String, u32> {
    let chars = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<Vec<_>>();
    let mut tokens = BTreeMap::new();
    for ch in &chars {
        bump_token(&mut tokens, format!("ch:{ch}"));
    }
    for window in chars.windows(2) {
        bump_token(&mut tokens, format!("bi:{}{}", window[0], window[1]));
    }
    tokens
}

fn bump_token(tokens: &mut BTreeMap<String, u32>, token: String) {
    *tokens.entry(token).or_insert(0) += 1;
}

fn parse_leading_number(value: &str) -> Option<f64> {
    let mut number = String::new();
    let mut seen_digit = false;
    for ch in value.trim().chars() {
        if ch.is_ascii_digit() || ch == '-' || ch == '+' || ch == '.' {
            if ch.is_ascii_digit() {
                seen_digit = true;
            }
            number.push(ch);
        } else {
            break;
        }
    }
    if seen_digit {
        number.parse::<f64>().ok()
    } else {
        None
    }
}

fn cosine_similarity(left: &BTreeMap<String, u32>, right: &BTreeMap<String, u32>) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    let dot = left
        .iter()
        .filter_map(|(token, left_count)| {
            right
                .get(token)
                .map(|right_count| (*left_count as f64) * (*right_count as f64))
        })
        .sum::<f64>();
    let left_norm = left
        .values()
        .map(|count| (*count as f64) * (*count as f64))
        .sum::<f64>()
        .sqrt();
    let right_norm = right
        .values()
        .map(|count| (*count as f64) * (*count as f64))
        .sum::<f64>()
        .sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm * right_norm)
    }
}

fn length_ratio(left: usize, right: usize) -> f64 {
    let max = left.max(right);
    if max == 0 {
        1.0
    } else {
        left.min(right) as f64 / max as f64
    }
}

fn preview_chars(text: &str, max_chars: usize) -> String {
    let mut preview = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

fn preview_html(
    page: u32,
    page_count: u32,
    page_info: &Value,
    svg: &str,
    text_preview: Option<&str>,
) -> String {
    let width = page_info
        .get("width")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let height = page_info
        .get("height")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let title = format!("rhwp preview page {} of {}", page + 1, page_count);
    let summary = text_preview
        .map(|text| preview_chars(text, 280))
        .unwrap_or_default();

    format!(
        r#"<!doctype html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light dark; }}
body {{ margin: 0; font: 13px system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #e5e7eb; color: #111827; }}
.rhwp-preview-shell {{ min-height: 100vh; display: grid; grid-template-rows: auto minmax(0, 1fr); }}
.rhwp-preview-meta {{ display: flex; gap: 12px; align-items: center; padding: 8px 12px; border-bottom: 1px solid #cbd5e1; background: #f8fafc; color: #334155; }}
.rhwp-preview-page {{ overflow: auto; padding: 16px; }}
.rhwp-preview-paper {{ width: max-content; margin: 0 auto; background: white; box-shadow: 0 2px 12px rgba(15, 23, 42, .22); }}
.rhwp-preview-paper svg {{ display: block; max-width: min(100vw - 48px, {width}px); height: auto; }}
.rhwp-preview-text {{ max-width: min(100vw - 48px, {width}px); margin: 10px auto 0; padding: 8px 10px; background: rgba(255,255,255,.82); border: 1px solid #d7dde6; white-space: pre-wrap; color: #475569; }}
@media (prefers-color-scheme: dark) {{
  body {{ background: #111827; color: #e5e7eb; }}
  .rhwp-preview-meta {{ background: #1f2937; border-color: #374151; color: #d1d5db; }}
  .rhwp-preview-text {{ background: rgba(31, 41, 55, .9); border-color: #4b5563; color: #d1d5db; }}
}}
</style>
</head>
<body>
<main class="rhwp-preview-shell" data-rhwp-preview-page="{page}" data-rhwp-page-count="{page_count}">
  <header class="rhwp-preview-meta">
    <strong>{title}</strong>
    <span>{width:.0} x {height:.0}px</span>
  </header>
  <section class="rhwp-preview-page">
    <div class="rhwp-preview-paper">{svg}</div>
    <pre class="rhwp-preview-text">{summary}</pre>
  </section>
</main>
</body>
</html>"#,
        title = html_escape_text(&title),
        page = page,
        page_count = page_count,
        width = width,
        height = height,
        svg = svg,
        summary = html_escape_text(&summary),
    )
}

fn html_escape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn extract_full_text(core: &DocumentCore) -> Result<String, String> {
    let mut text = String::new();
    for page in 0..core.page_count() {
        if page > 0 {
            text.push('\n');
        }
        text.push_str(
            &core
                .extract_page_text_native(page)
                .map_err(|e| e.to_string())?,
        );
    }
    Ok(text)
}

fn comparable_text(text: &str, normalize_whitespace: bool) -> String {
    if !normalize_whitespace {
        return text.to_string();
    }
    let mut out = String::new();
    let mut in_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !in_space && !out.is_empty() {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(ch);
            in_space = false;
        }
    }
    out.trim().to_string()
}

fn text_profile(label: &str, page_count: u32, text: &str) -> Value {
    json!({
        "label": label,
        "page_count": page_count,
        "char_count": text.chars().count(),
        "line_count": text.lines().count(),
    })
}

fn line_diff_count(left: &str, right: &str) -> usize {
    let left_lines = left.lines().collect::<Vec<_>>();
    let right_lines = right.lines().collect::<Vec<_>>();
    let max_len = left_lines.len().max(right_lines.len());
    (0..max_len)
        .filter(|idx| left_lines.get(*idx) != right_lines.get(*idx))
        .count()
}

fn line_diffs(left: &str, right: &str, max_diffs: usize) -> Vec<Value> {
    let left_lines = left.lines().collect::<Vec<_>>();
    let right_lines = right.lines().collect::<Vec<_>>();
    let max_len = left_lines.len().max(right_lines.len());
    let mut diffs = Vec::new();
    for idx in 0..max_len {
        let left_line = left_lines.get(idx);
        let right_line = right_lines.get(idx);
        if left_line == right_line {
            continue;
        }
        diffs.push(json!({
            "line": idx,
            "left": left_line,
            "right": right_line,
        }));
        if diffs.len() >= max_diffs {
            break;
        }
    }
    diffs
}

fn geom_diff_json(
    left_label: &str,
    right_label: &str,
    diff: &DocGeomDiff,
    max_deltas: usize,
    max_disp_threshold: f64,
    page_filter: Option<u32>,
) -> Value {
    let pages = diff
        .pages
        .iter()
        .filter(|page| page_filter.map(|p| p == page.page).unwrap_or(true))
        .map(|page| page_geom_json(page, max_deltas))
        .collect::<Vec<_>>();
    let max_page_disp = if let Some(page) = page_filter {
        diff.pages
            .iter()
            .find(|p| p.page == page)
            .map(|p| p.max_disp)
            .unwrap_or(0.0)
    } else {
        diff.max_disp
    };
    let page_structure_mismatch = if let Some(page) = page_filter {
        diff.pages
            .iter()
            .find(|p| p.page == page)
            .map(|p| p.structure_mismatch)
            .unwrap_or(false)
    } else {
        diff.any_structure_mismatch()
    };

    json!({
        "left": {
            "label": left_label,
            "page_count": diff.page_count_a,
        },
        "right": {
            "label": right_label,
            "page_count": diff.page_count_b,
        },
        "page_filter": page_filter,
        "max_disp_threshold": max_disp_threshold,
        "page_count_mismatch": diff.page_count_mismatch(),
        "structure_mismatch": page_structure_mismatch,
        "max_disp": max_page_disp,
        "within_threshold": !diff.page_count_mismatch()
            && !page_structure_mismatch
            && max_page_disp <= max_disp_threshold,
        "pages": pages,
    })
}

fn page_geom_json(page: &PageGeomDiff, max_deltas: usize) -> Value {
    json!({
        "page": page.page,
        "node_count_left": page.node_count_a,
        "node_count_right": page.node_count_b,
        "structure_mismatch": page.structure_mismatch,
        "max_disp": page.max_disp,
        "mean_disp": page.mean_disp,
        "type_deltas": page.type_deltas.iter().map(type_delta_json).collect::<Vec<_>>(),
        "top_deltas": page
            .top_deltas
            .iter()
            .take(max_deltas)
            .map(node_delta_json)
            .collect::<Vec<_>>(),
    })
}

fn type_delta_json(delta: &TypeDelta) -> Value {
    json!({
        "node_type": delta.node_type,
        "count_left": delta.count_a,
        "count_right": delta.count_b,
        "net": delta.net(),
    })
}

fn node_delta_json(delta: &NodeDelta) -> Value {
    json!({
        "path": &delta.path,
        "node_type": delta.node_type,
        "dx": delta.dx,
        "dy": delta.dy,
        "dw": delta.dw,
        "dh": delta.dh,
        "disp": delta.disp(),
    })
}

#[derive(Default)]
struct RecordDiffStats {
    matched: usize,
    changed: usize,
    missing: usize,
    extra: usize,
}

fn hwp_record_source_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if matches!(crate::parser::detect_format(bytes), FileFormat::Hwp) {
        return Ok(bytes.to_vec());
    }
    let mut core = DocumentCore::from_bytes(bytes).map_err(|e| e.to_string())?;
    core.export_hwp_with_adapter().map_err(|e| e.to_string())
}

#[derive(Clone)]
struct HwpxPackageEntry {
    path: String,
    byte_len: usize,
    hash: String,
    kind: &'static str,
    passthrough: bool,
    data: Vec<u8>,
}

#[derive(Default)]
struct HwpxPackageDiffStats {
    matched: usize,
    changed: usize,
    missing: usize,
    extra: usize,
}

fn hwpx_package_source_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if matches!(crate::parser::detect_format(bytes), FileFormat::Hwpx) {
        return Ok(bytes.to_vec());
    }
    let mut core = DocumentCore::from_bytes(bytes).map_err(|e| e.to_string())?;
    core.export_hwpx_native().map_err(|e| e.to_string())
}

fn hwpx_package_entries(label: &str, bytes: &[u8]) -> Result<Vec<HwpxPackageEntry>, String> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("{label}: invalid HWPX ZIP: {e}"))?;
    let mut entries = Vec::new();
    for idx in 0..archive.len() {
        let mut file = archive
            .by_index(idx)
            .map_err(|e| format!("{label}: failed to read ZIP entry #{idx}: {e}"))?;
        if file.is_dir() {
            continue;
        }
        let path = file.name().to_string();
        let mut data = Vec::new();
        let limit = (crate::parser::hwpx::reader::MAX_BINDATA_SIZE as u64).saturating_add(1);
        (&mut file)
            .take(limit)
            .read_to_end(&mut data)
            .map_err(|e| format!("{label}: failed to read {path}: {e}"))?;
        if data.len() > crate::parser::hwpx::reader::MAX_BINDATA_SIZE {
            return Err(format!(
                "{label}: HWPX entry {path} exceeds {} byte limit",
                crate::parser::hwpx::reader::MAX_BINDATA_SIZE
            ));
        }
        entries.push(HwpxPackageEntry {
            kind: hwpx_aux_entry_kind(&path),
            passthrough: is_passthrough_hwpx_aux_path(&path),
            path,
            byte_len: data.len(),
            hash: blake3::hash(&data).to_hex().to_string(),
            data,
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn hwpx_package_diff_json(
    left_label: &str,
    right_label: &str,
    left_entries: &[HwpxPackageEntry],
    right_entries: &[HwpxPackageEntry],
    max_diffs: usize,
) -> Value {
    let left_map = hwpx_package_entry_map(left_entries);
    let right_map = hwpx_package_entry_map(right_entries);
    let mut keys = BTreeSet::new();
    keys.extend(left_map.keys().map(|key| (*key).to_string()));
    keys.extend(right_map.keys().map(|key| (*key).to_string()));

    let mut stats = HwpxPackageDiffStats::default();
    let mut differences = Vec::new();
    let mut structural_diagnostics = Vec::new();
    let mut package_drift_diagnostics = Vec::new();
    let mut difference_count = 0usize;
    let mut structural_difference_count = 0usize;
    let mut passthrough_difference_count = 0usize;
    let mut content_manifest_changed = false;

    for path in keys {
        let left = left_map.get(path.as_str()).copied();
        let right = right_map.get(path.as_str()).copied();
        match (left, right) {
            (Some(left), Some(right)) if left.hash == right.hash => {
                stats.matched += 1;
            }
            (Some(left), Some(right)) => {
                stats.changed += 1;
                difference_count += 1;
                if let Some(diagnostic) = hwpx_package_structural_diagnostic(&path, left, right) {
                    if !diagnostic["structural_equal"].as_bool().unwrap_or(false) {
                        structural_difference_count += 1;
                    }
                    package_drift_diagnostics.push(diagnostic.clone());
                    if structural_diagnostics.len() < max_diffs {
                        structural_diagnostics.push(diagnostic);
                    }
                } else {
                    structural_difference_count += 1;
                }
                if left.passthrough || right.passthrough {
                    passthrough_difference_count += 1;
                }
                if path == "Contents/content.hpf" {
                    content_manifest_changed = true;
                }
                if differences.len() < max_diffs {
                    differences.push(json!({
                        "diff": "changed",
                        "path": path,
                        "left": hwpx_package_entry_json(left),
                        "right": hwpx_package_entry_json(right),
                    }));
                }
            }
            (Some(left), None) => {
                stats.missing += 1;
                difference_count += 1;
                structural_difference_count += 1;
                if left.passthrough {
                    passthrough_difference_count += 1;
                }
                if path == "Contents/content.hpf" {
                    content_manifest_changed = true;
                }
                if differences.len() < max_diffs {
                    differences.push(json!({
                        "diff": "missing",
                        "path": path,
                        "left": hwpx_package_entry_json(left),
                        "right": Value::Null,
                    }));
                }
            }
            (None, Some(right)) => {
                stats.extra += 1;
                difference_count += 1;
                structural_difference_count += 1;
                if right.passthrough {
                    passthrough_difference_count += 1;
                }
                if path == "Contents/content.hpf" {
                    content_manifest_changed = true;
                }
                if differences.len() < max_diffs {
                    differences.push(json!({
                        "diff": "extra",
                        "path": path,
                        "left": Value::Null,
                        "right": hwpx_package_entry_json(right),
                    }));
                }
            }
            (None, None) => {}
        }
    }
    let package_drift = hwpx_package_drift_summary(
        difference_count,
        structural_difference_count,
        passthrough_difference_count,
        &package_drift_diagnostics,
    );
    let package_drift_status = package_drift["status"].clone();
    let package_drift_kinds = package_drift["drift_kinds"].clone();
    let package_structural_drift_keys = package_drift["structural_drift_keys"].clone();
    let line_segment_only_package_drift = package_drift["line_segment_only_package_drift"].clone();

    json!({
        "equal": difference_count == 0,
        "left": hwpx_package_profile(left_label, left_entries),
        "right": hwpx_package_profile(right_label, right_entries),
        "stats": {
            "matched": stats.matched,
            "changed": stats.changed,
            "missing": stats.missing,
            "extra": stats.extra,
        },
        "difference_count": difference_count,
        "differences_truncated": difference_count > differences.len(),
        "structural_equal": structural_difference_count == 0,
        "structural_difference_count": structural_difference_count,
        "structural_diagnostics_truncated": structural_diagnostics.len() < difference_count,
        "structural_diagnostics": structural_diagnostics,
        "passthrough_difference_count": passthrough_difference_count,
        "content_manifest_changed": content_manifest_changed,
        "package_drift_status": package_drift_status,
        "package_drift_kinds": package_drift_kinds,
        "package_structural_drift_keys": package_structural_drift_keys,
        "line_segment_only_package_drift": line_segment_only_package_drift,
        "package_drift": package_drift,
        "differences": differences,
    })
}

fn hwpx_package_drift_summary(
    difference_count: usize,
    structural_difference_count: usize,
    passthrough_difference_count: usize,
    diagnostics: &[Value],
) -> Value {
    let package_equal = difference_count == 0;
    let package_structural_equal = structural_difference_count == 0;
    let mut drift_kinds = BTreeSet::new();
    let mut structural_drift_keys = BTreeSet::new();

    for diagnostic in diagnostics {
        if let Some(counts) = diagnostic["structural_counts"].as_object() {
            for (key, value) in counts {
                let left = value["left"].as_u64();
                let right = value["right"].as_u64();
                if left != right {
                    structural_drift_keys.insert(key.clone());
                    if key == "line_segment_count" {
                        drift_kinds.insert("line_segment_count".to_string());
                    } else {
                        drift_kinds.insert(format!("structural_{key}"));
                    }
                }
            }
        }

        let manifest = &diagnostic["manifest"];
        if manifest["item_set_equal"].as_bool() == Some(true)
            && manifest["item_order_equal"].as_bool() == Some(false)
        {
            drift_kinds.insert("manifest_order".to_string());
        }

        let empty = &diagnostic["empty_text_serialization"];
        if empty["explicit_empty_text"]["left"] != empty["explicit_empty_text"]["right"]
            || empty["self_closing_empty_text"]["left"] != empty["self_closing_empty_text"]["right"]
            || empty["self_closing_runs"]["left"] != empty["self_closing_runs"]["right"]
        {
            drift_kinds.insert("empty_text_serialization".to_string());
        }

        let paragraph_ids = &diagnostic["paragraph_id_serialization"];
        if paragraph_ids["id_0_count"]["left"] != paragraph_ids["id_0_count"]["right"]
            || paragraph_ids["id_2147483648_count"]["left"]
                != paragraph_ids["id_2147483648_count"]["right"]
        {
            drift_kinds.insert("paragraph_id_serialization".to_string());
        }

        if diagnostic["structural_equal"].as_bool() == Some(true)
            && diagnostic["notes"].as_array().is_some_and(|notes| {
                notes.iter().any(|note| {
                    note.as_str()
                        .is_some_and(|text| text.to_ascii_lowercase().contains("lexical"))
                })
            })
        {
            drift_kinds.insert("lexical_serialization".to_string());
        }
    }

    let lexical_only_package_drift =
        !package_equal && package_structural_equal && passthrough_difference_count == 0;
    let line_segment_only_package_drift = !package_equal
        && !package_structural_equal
        && passthrough_difference_count == 0
        && !structural_drift_keys.is_empty()
        && structural_drift_keys
            .iter()
            .all(|key| key == "line_segment_count");
    let status = if package_equal {
        "package_identical"
    } else if lexical_only_package_drift {
        "structural_equal_package_drift"
    } else if line_segment_only_package_drift {
        "line_segment_only_package_drift"
    } else {
        "structural_package_difference"
    };

    json!({
        "status": status,
        "package_equal": package_equal,
        "package_structural_equal": package_structural_equal,
        "lexical_only_package_drift": lexical_only_package_drift,
        "line_segment_only_package_drift": line_segment_only_package_drift,
        "package_difference_count": difference_count,
        "package_structural_difference_count": structural_difference_count,
        "package_passthrough_difference_count": passthrough_difference_count,
        "structural_drift_keys": structural_drift_keys.into_iter().collect::<Vec<_>>(),
        "drift_kinds": drift_kinds.into_iter().collect::<Vec<_>>(),
    })
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct HwpxManifestItem {
    id: String,
    href: String,
    media_type: String,
    is_embeded: String,
}

struct HwpxXmlStructuralProfile {
    counts: BTreeMap<&'static str, usize>,
    manifest_items: Vec<HwpxManifestItem>,
    explicit_empty_text_count: usize,
    self_closing_empty_text_count: usize,
    self_closing_run_count: usize,
    paragraph_id_0_count: usize,
    paragraph_id_2147483648_count: usize,
}

fn hwpx_package_structural_diagnostic(
    path: &str,
    left: &HwpxPackageEntry,
    right: &HwpxPackageEntry,
) -> Option<Value> {
    if !path.ends_with(".xml") && !path.ends_with(".hpf") {
        return None;
    }
    let left_profile = match hwpx_xml_structural_profile(&left.data) {
        Ok(profile) => profile,
        Err(err) => {
            return Some(json!({
                "path": path,
                "structural_equal": false,
                "parse_error": { "left": err, "right": Value::Null },
            }));
        }
    };
    let right_profile = match hwpx_xml_structural_profile(&right.data) {
        Ok(profile) => profile,
        Err(err) => {
            return Some(json!({
                "path": path,
                "structural_equal": false,
                "parse_error": { "left": Value::Null, "right": err },
            }));
        }
    };
    let counts_equal = left_profile.counts == right_profile.counts;
    let mut notes = Vec::new();
    if counts_equal {
        notes.push("Core structural tag counts are equal; this diff is tracked as lexical/serialization drift.");
    }
    let manifest = if path == "Contents/content.hpf" {
        let left_set: BTreeSet<_> = left_profile.manifest_items.iter().cloned().collect();
        let right_set: BTreeSet<_> = right_profile.manifest_items.iter().cloned().collect();
        let left_order: Vec<_> = left_profile
            .manifest_items
            .iter()
            .map(|item| item.href.as_str())
            .collect();
        let right_order: Vec<_> = right_profile
            .manifest_items
            .iter()
            .map(|item| item.href.as_str())
            .collect();
        let item_set_equal = left_set == right_set;
        let item_order_equal = left_order == right_order;
        if item_set_equal && !item_order_equal {
            notes.push("Manifest item set is equal but item order differs.");
        }
        Some(json!({
            "item_count": {
                "left": left_profile.manifest_items.len(),
                "right": right_profile.manifest_items.len(),
                "equal": left_profile.manifest_items.len() == right_profile.manifest_items.len(),
            },
            "item_set_equal": item_set_equal,
            "item_order_equal": item_order_equal,
            "first_order_difference": first_manifest_order_difference(
                &left_profile.manifest_items,
                &right_profile.manifest_items,
            ),
        }))
    } else {
        None
    };
    if left_profile.explicit_empty_text_count != right_profile.explicit_empty_text_count
        || left_profile.self_closing_empty_text_count != right_profile.self_closing_empty_text_count
        || left_profile.self_closing_run_count != right_profile.self_closing_run_count
    {
        notes.push("Empty text nodes are serialized differently.");
    }
    if left_profile.paragraph_id_0_count != right_profile.paragraph_id_0_count
        || left_profile.paragraph_id_2147483648_count != right_profile.paragraph_id_2147483648_count
    {
        notes.push("Paragraph id serialization differs while structural profile remains equal.");
    }
    let manifest_structural_equal = manifest
        .as_ref()
        .and_then(|value| value["item_set_equal"].as_bool())
        .unwrap_or(true);
    Some(json!({
        "path": path,
        "structural_equal": counts_equal && manifest_structural_equal,
        "byte_len": {
            "left": left.byte_len,
            "right": right.byte_len,
        },
        "structural_counts": structural_counts_json(&left_profile.counts, &right_profile.counts),
        "manifest": manifest,
        "empty_text_serialization": {
            "explicit_empty_text": {
                "left": left_profile.explicit_empty_text_count,
                "right": right_profile.explicit_empty_text_count,
            },
            "self_closing_empty_text": {
                "left": left_profile.self_closing_empty_text_count,
                "right": right_profile.self_closing_empty_text_count,
            },
            "self_closing_runs": {
                "left": left_profile.self_closing_run_count,
                "right": right_profile.self_closing_run_count,
            },
        },
        "paragraph_id_serialization": {
            "id_0_count": {
                "left": left_profile.paragraph_id_0_count,
                "right": right_profile.paragraph_id_0_count,
            },
            "id_2147483648_count": {
                "left": left_profile.paragraph_id_2147483648_count,
                "right": right_profile.paragraph_id_2147483648_count,
            },
        },
        "notes": notes,
    }))
}

fn hwpx_xml_structural_profile(data: &[u8]) -> Result<HwpxXmlStructuralProfile, String> {
    let mut reader = XmlReader::from_reader(Cursor::new(data));
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut counts = hwpx_empty_structural_counts();
    let mut manifest_items = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(XmlEvent::Start(e)) | Ok(XmlEvent::Empty(e)) => {
                let name = e.name();
                let local = xml_local_name(name.as_ref());
                if let Some(key) = hwpx_structural_count_key(local) {
                    if let Some(count) = counts.get_mut(key) {
                        *count += 1;
                    }
                }
                if local == b"item" {
                    manifest_items.push(HwpxManifestItem {
                        id: xml_attr(&e, b"id").unwrap_or_default(),
                        href: xml_attr(&e, b"href").unwrap_or_default(),
                        media_type: xml_attr(&e, b"media-type").unwrap_or_default(),
                        is_embeded: xml_attr(&e, b"isEmbeded").unwrap_or_default(),
                    });
                }
            }
            Ok(XmlEvent::Eof) => break,
            Ok(_) => {}
            Err(err) => return Err(err.to_string()),
        }
        buf.clear();
    }
    let text = String::from_utf8_lossy(data);
    Ok(HwpxXmlStructuralProfile {
        counts,
        manifest_items,
        explicit_empty_text_count: text.matches("<hp:t></hp:t>").count(),
        self_closing_empty_text_count: text.matches("<hp:t/>").count(),
        self_closing_run_count: count_self_closing_runs(&text),
        paragraph_id_0_count: text.matches(r#"<hp:p id="0""#).count(),
        paragraph_id_2147483648_count: text.matches(r#"<hp:p id="2147483648""#).count(),
    })
}

fn hwpx_empty_structural_counts() -> BTreeMap<&'static str, usize> {
    [
        "paragraph_count",
        "run_count",
        "table_count",
        "picture_count",
        "control_count",
        "line_segment_count",
        "char_shape_count",
        "para_shape_count",
        "style_count",
        "border_fill_count",
        "font_count",
    ]
    .into_iter()
    .map(|key| (key, 0))
    .collect()
}

fn hwpx_structural_count_key(local_name: &[u8]) -> Option<&'static str> {
    match local_name {
        b"p" => Some("paragraph_count"),
        b"run" => Some("run_count"),
        b"tbl" => Some("table_count"),
        b"pic" => Some("picture_count"),
        b"ctrl" => Some("control_count"),
        b"lineseg" => Some("line_segment_count"),
        b"charPr" => Some("char_shape_count"),
        b"paraPr" => Some("para_shape_count"),
        b"style" => Some("style_count"),
        b"borderFill" => Some("border_fill_count"),
        b"font" => Some("font_count"),
        _ => None,
    }
}

fn structural_counts_json(
    left: &BTreeMap<&'static str, usize>,
    right: &BTreeMap<&'static str, usize>,
) -> Value {
    let mut out = serde_json::Map::new();
    for key in left.keys().chain(right.keys()) {
        if out.contains_key(*key) {
            continue;
        }
        out.insert(
            (*key).to_string(),
            json!({
                "left": left.get(key).copied().unwrap_or(0),
                "right": right.get(key).copied().unwrap_or(0),
            }),
        );
    }
    Value::Object(out)
}

fn first_manifest_order_difference(left: &[HwpxManifestItem], right: &[HwpxManifestItem]) -> Value {
    let len = left.len().max(right.len());
    for index in 0..len {
        let left_href = left.get(index).map(|item| item.href.as_str());
        let right_href = right.get(index).map(|item| item.href.as_str());
        if left_href != right_href {
            return json!({
                "index": index,
                "left": left_href,
                "right": right_href,
            });
        }
    }
    Value::Null
}

fn count_self_closing_runs(text: &str) -> usize {
    text.match_indices("<hp:run")
        .filter(|(start, _)| {
            text[*start..]
                .find('>')
                .map(|offset| text[*start..*start + offset].trim_end().ends_with('/'))
                .unwrap_or(false)
        })
        .count()
}

fn hwpx_package_entry_map(entries: &[HwpxPackageEntry]) -> BTreeMap<&str, &HwpxPackageEntry> {
    entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect()
}

fn hwpx_package_profile(label: &str, entries: &[HwpxPackageEntry]) -> Value {
    let passthrough_entry_count = entries.iter().filter(|entry| entry.passthrough).count();
    let history_entry_count = entries
        .iter()
        .filter(|entry| entry.kind == "history")
        .count();
    let track_change_entry_count = entries
        .iter()
        .filter(|entry| entry.kind == "track_change")
        .count();
    json!({
        "label": label,
        "entry_count": entries.len(),
        "passthrough_entry_count": passthrough_entry_count,
        "history_entry_count": history_entry_count,
        "track_change_entry_count": track_change_entry_count,
        "content_manifest_present": entries.iter().any(|entry| entry.path == "Contents/content.hpf"),
    })
}

fn hwpx_package_entry_json(entry: &HwpxPackageEntry) -> Value {
    json!({
        "path": &entry.path,
        "byte_len": entry.byte_len,
        "hash": &entry.hash,
        "kind": entry.kind,
        "passthrough": entry.passthrough,
    })
}

fn record_diff_json(
    left_label: &str,
    right_label: &str,
    left_items: &[Hwp5InventoryItem],
    right_items: &[Hwp5InventoryItem],
    section: Option<u32>,
    max_diffs: usize,
) -> Value {
    let left_map = record_inventory_map(left_items);
    let right_map = record_inventory_map(right_items);
    let mut keys = BTreeSet::new();
    keys.extend(left_map.keys().cloned());
    keys.extend(right_map.keys().cloned());

    let mut stats = RecordDiffStats::default();
    let mut rows = Vec::new();
    let mut all_difference_count = 0usize;
    let mut kind_counts: BTreeMap<&'static str, usize> = BTreeMap::new();

    for key in keys {
        match (left_map.get(&key), right_map.get(&key)) {
            (Some(left), Some(right)) => {
                let fields = record_changed_fields(left, right);
                if fields.is_empty() {
                    stats.matched += 1;
                } else {
                    stats.changed += 1;
                    all_difference_count += 1;
                    *kind_counts.entry("changed").or_insert(0) += 1;
                    if rows.len() < max_diffs {
                        rows.push(json!({
                            "kind": "changed",
                            "key": key,
                            "changed_fields": fields,
                            "left": record_summary_json(left),
                            "right": record_summary_json(right),
                        }));
                    }
                }
            }
            (Some(left), None) => {
                stats.missing += 1;
                all_difference_count += 1;
                *kind_counts.entry("missing").or_insert(0) += 1;
                if rows.len() < max_diffs {
                    rows.push(json!({
                        "kind": "missing",
                        "key": key,
                        "left": record_summary_json(left),
                        "right": null,
                    }));
                }
            }
            (None, Some(right)) => {
                stats.extra += 1;
                all_difference_count += 1;
                *kind_counts.entry("extra").or_insert(0) += 1;
                if rows.len() < max_diffs {
                    rows.push(json!({
                        "kind": "extra",
                        "key": key,
                        "left": null,
                        "right": record_summary_json(right),
                    }));
                }
            }
            (None, None) => {}
        }
    }

    json!({
        "equal": all_difference_count == 0,
        "section": section,
        "left": {
            "label": left_label,
            "record_count": left_items.len(),
        },
        "right": {
            "label": right_label,
            "record_count": right_items.len(),
        },
        "stats": {
            "matched": stats.matched,
            "changed": stats.changed,
            "missing": stats.missing,
            "extra": stats.extra,
        },
        "kind_counts": kind_counts,
        "difference_count": all_difference_count,
        "differences_truncated": all_difference_count > rows.len(),
        "differences": rows,
    })
}

fn record_inventory_map(items: &[Hwp5InventoryItem]) -> BTreeMap<String, &Hwp5InventoryItem> {
    items
        .iter()
        .map(|item| (item.record_uid.clone(), item))
        .collect()
}

fn record_changed_fields(left: &Hwp5InventoryItem, right: &Hwp5InventoryItem) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if left.tag_id != right.tag_id {
        fields.push("tag");
    }
    if left.size != right.size {
        fields.push("size");
    }
    if left.payload_hash != right.payload_hash {
        fields.push("payload_hash");
    }
    if left.scope_path != right.scope_path {
        fields.push("scope_path");
    }
    if left.control_id != right.control_id || left.control_name != right.control_name {
        fields.push("control");
    }
    fields
}

fn record_summary_json(item: &Hwp5InventoryItem) -> Value {
    json!({
        "record_uid": &item.record_uid,
        "stream_path": &item.stream_path,
        "section": item.section,
        "record_index": item.record_index,
        "level": item.level,
        "tag_id": item.tag_id,
        "tag_name": &item.tag_name,
        "size": item.size,
        "tuple_role": &item.tuple_role,
        "tuple_index": item.tuple_index,
        "control_id": &item.control_id,
        "control_name": &item.control_name,
        "scope_path": &item.scope_path,
        "payload_hash": &item.payload_hash,
    })
}

fn cell_target(core: &DocumentCore, args: &Map<String, Value>) -> Result<(usize, usize), String> {
    let section = opt_usize(args, "section").unwrap_or(0);
    let para = req_usize(args, "para")?;
    let control = req_usize(args, "control")?;
    let ctrl = core
        .document
        .sections
        .get(section)
        .and_then(|s| s.paragraphs.get(para))
        .and_then(|p| p.controls.get(control))
        .ok_or_else(|| "control not found".to_string())?;
    match ctrl {
        Control::Table(table) => {
            if is_caption_target(args) {
                let caption_para = caption_para_arg(args);
                let caption = table
                    .caption
                    .as_ref()
                    .ok_or_else(|| "table caption not found".to_string())?;
                if caption_para >= caption.paragraphs.len() {
                    return Err(format!(
                        "caption paragraph index {caption_para} out of range (total {})",
                        caption.paragraphs.len()
                    ));
                }
                Ok((TABLE_CAPTION_CELL_INDEX, caption_para))
            } else {
                table_cell_target(table, args)
            }
        }
        Control::Picture(pic) if is_caption_target(args) => {
            let caption_para = caption_para_arg(args);
            let caption = pic
                .caption
                .as_ref()
                .ok_or_else(|| "picture caption not found".to_string())?;
            if caption_para >= caption.paragraphs.len() {
                return Err(format!(
                    "caption paragraph index {caption_para} out of range (total {})",
                    caption.paragraphs.len()
                ));
            }
            Ok((0, caption_para))
        }
        Control::Picture(_) => {
            Err("picture caption target requires caption=true or caption_para".to_string())
        }
        _ => Err("table control or picture caption target not found".to_string()),
    }
}

fn is_table_cell_format_target(args: &Map<String, Value>) -> bool {
    args.contains_key("control")
        && (args.contains_key("cell")
            || args.contains_key("row")
            || args.contains_key("col")
            || args.contains_key("cell_para")
            || is_caption_target(args))
}

const TABLE_CAPTION_CELL_INDEX: usize = 65534;

fn is_caption_target(args: &Map<String, Value>) -> bool {
    opt_bool(args, "caption").unwrap_or(false)
        || opt_bool(args, "is_caption").unwrap_or(false)
        || opt_bool(args, "isCaption").unwrap_or(false)
        || args.contains_key("caption_para")
        || args.contains_key("captionPara")
        || args.contains_key("caption_para_idx")
        || args.contains_key("captionParaIndex")
}

fn caption_para_arg(args: &Map<String, Value>) -> usize {
    opt_usize(args, "caption_para")
        .or_else(|| opt_usize(args, "captionPara"))
        .or_else(|| opt_usize(args, "caption_para_idx"))
        .or_else(|| opt_usize(args, "captionParaIndex"))
        .or_else(|| opt_usize(args, "cell_para"))
        .unwrap_or(0)
}

fn table_cell_target(table: &Table, args: &Map<String, Value>) -> Result<(usize, usize), String> {
    let cell_para = opt_usize(args, "cell_para").unwrap_or(0);
    if let Some(cell) = opt_usize(args, "cell") {
        if cell == TABLE_CAPTION_CELL_INDEX {
            let caption = table
                .caption
                .as_ref()
                .ok_or_else(|| "table caption not found".to_string())?;
            if cell_para >= caption.paragraphs.len() {
                return Err(format!(
                    "caption paragraph index {cell_para} out of range (total {})",
                    caption.paragraphs.len()
                ));
            }
            return Ok((TABLE_CAPTION_CELL_INDEX, cell_para));
        }
        if cell >= table.cells.len() {
            return Err(format!(
                "cell index {cell} out of range (total {})",
                table.cells.len()
            ));
        }
        return Ok((cell, cell_para));
    }
    let row = req_u16(args, "row")?;
    let col = req_u16(args, "col")?;
    let cell = table
        .cells
        .iter()
        .position(|cell| cell.row == row && cell.col == col)
        .ok_or_else(|| format!("cell not found at row={row}, col={col}"))?;
    Ok((cell, cell_para))
}

fn header_footer_table_ref<'a>(
    core: &'a DocumentCore,
    args: &Map<String, Value>,
) -> Result<(&'static str, &'a Table), String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let outer_para_idx = req_usize(args, "para")?;
    let outer_control_idx = req_usize(args, "control")?;
    let inner_para_idx = nested_para_arg(args);
    let inner_control_idx = req_inner_control(args)?;
    let section = core
        .document
        .sections
        .get(section_idx)
        .ok_or_else(|| format!("section index {section_idx} out of range"))?;
    let outer_para = section
        .paragraphs
        .get(outer_para_idx)
        .ok_or_else(|| format!("outer paragraph index {outer_para_idx} out of range"))?;
    let outer_ctrl = outer_para
        .controls
        .get(outer_control_idx)
        .ok_or_else(|| format!("outer control index {outer_control_idx} out of range"))?;
    let (scope, inner_paragraphs) = match outer_ctrl {
        Control::Header(header) => ("header", header.paragraphs.as_slice()),
        Control::Footer(footer) => ("footer", footer.paragraphs.as_slice()),
        _ => return Err("outer control is not a header/footer".to_string()),
    };
    validate_container_scope(args, scope)?;
    let inner_para = inner_paragraphs
        .get(inner_para_idx)
        .ok_or_else(|| format!("inner paragraph index {inner_para_idx} out of range"))?;
    let table = inner_para
        .controls
        .get(inner_control_idx)
        .and_then(|control| match control {
            Control::Table(table) => Some(table.as_ref()),
            _ => None,
        })
        .ok_or_else(|| {
            format!("inner control index {inner_control_idx} is not a header/footer table")
        })?;
    Ok((scope, table))
}

fn header_footer_table_mut<'a>(
    core: &'a mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<(&'static str, &'a mut Table), String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let outer_para_idx = req_usize(args, "para")?;
    let outer_control_idx = req_usize(args, "control")?;
    let inner_para_idx = nested_para_arg(args);
    let inner_control_idx = req_inner_control(args)?;
    let section = core
        .document
        .sections
        .get_mut(section_idx)
        .ok_or_else(|| format!("section index {section_idx} out of range"))?;
    let outer_para = section
        .paragraphs
        .get_mut(outer_para_idx)
        .ok_or_else(|| format!("outer paragraph index {outer_para_idx} out of range"))?;
    let outer_ctrl = outer_para
        .controls
        .get_mut(outer_control_idx)
        .ok_or_else(|| format!("outer control index {outer_control_idx} out of range"))?;
    let (scope, inner_paragraphs) = match outer_ctrl {
        Control::Header(header) => ("header", &mut header.paragraphs),
        Control::Footer(footer) => ("footer", &mut footer.paragraphs),
        _ => return Err("outer control is not a header/footer".to_string()),
    };
    validate_container_scope(args, scope)?;
    let inner_para = inner_paragraphs
        .get_mut(inner_para_idx)
        .ok_or_else(|| format!("inner paragraph index {inner_para_idx} out of range"))?;
    let table = inner_para
        .controls
        .get_mut(inner_control_idx)
        .and_then(|control| match control {
            Control::Table(table) => Some(table.as_mut()),
            _ => None,
        })
        .ok_or_else(|| {
            format!("inner control index {inner_control_idx} is not a header/footer table")
        })?;
    Ok((scope, table))
}

fn create_header_footer_table(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let outer_para_idx = req_usize(args, "para")?;
    let outer_control_idx = req_usize(args, "control")?;
    let inner_para_idx = nested_para_arg(args);
    let char_offset = req_usize(args, "char_offset")?;
    let rows = req_u16(args, "rows")?;
    let cols = req_u16(args, "cols")?;
    let (scope, default_char_shape_id, default_para_shape_id) = {
        let section = core
            .document
            .sections
            .get(section_idx)
            .ok_or_else(|| format!("section index {section_idx} out of range"))?;
        let outer_para = section
            .paragraphs
            .get(outer_para_idx)
            .ok_or_else(|| format!("outer paragraph index {outer_para_idx} out of range"))?;
        let outer_ctrl = outer_para
            .controls
            .get(outer_control_idx)
            .ok_or_else(|| format!("outer control index {outer_control_idx} out of range"))?;
        let (scope, inner_paragraphs) = match outer_ctrl {
            Control::Header(header) => ("header", header.paragraphs.as_slice()),
            Control::Footer(footer) => ("footer", footer.paragraphs.as_slice()),
            _ => return Err("outer control is not a header/footer".to_string()),
        };
        validate_container_scope(args, scope)?;
        let inner_para = inner_paragraphs
            .get(inner_para_idx)
            .ok_or_else(|| format!("inner paragraph index {inner_para_idx} out of range"))?;
        (
            scope,
            inner_para
                .char_shapes
                .first()
                .map(|shape| shape.char_shape_id)
                .unwrap_or(0),
            inner_para.para_shape_id,
        )
    };
    let (table_para, empty_para) = core
        .default_table_paragraph_pair(
            section_idx,
            rows,
            cols,
            default_char_shape_id,
            default_para_shape_id,
        )
        .map_err(|e| e.to_string())?;
    let (insert_para_idx, table_control_idx) = {
        let section = core
            .document
            .sections
            .get_mut(section_idx)
            .ok_or_else(|| format!("section index {section_idx} out of range"))?;
        let outer_para = section
            .paragraphs
            .get_mut(outer_para_idx)
            .ok_or_else(|| format!("outer paragraph index {outer_para_idx} out of range"))?;
        let outer_ctrl = outer_para
            .controls
            .get_mut(outer_control_idx)
            .ok_or_else(|| format!("outer control index {outer_control_idx} out of range"))?;
        let inner_paragraphs = match outer_ctrl {
            Control::Header(header) => &mut header.paragraphs,
            Control::Footer(footer) => &mut footer.paragraphs,
            _ => return Err("outer control is not a header/footer".to_string()),
        };
        DocumentCore::insert_table_paragraph_into_paragraphs(
            inner_paragraphs,
            inner_para_idx,
            char_offset,
            table_para,
            empty_para,
        )
        .map_err(|e| e.to_string())?
    };
    if let Some(section) = core.document.sections.get_mut(section_idx) {
        section.raw_stream = None;
    }
    core.rebuild_section(section_idx);
    core.paginate_if_needed();
    core.event_log.push(DocumentEvent::TableRowInserted {
        section: section_idx,
        para: outer_para_idx,
        ctrl: outer_control_idx,
    });
    Ok(json!({
        "ok": true,
        "scope": scope,
        "container_scope": scope,
        "containerScope": scope,
        "paraIdx": outer_para_idx,
        "controlIdx": outer_control_idx,
        "inner_para": insert_para_idx,
        "innerPara": insert_para_idx,
        "hf_para": insert_para_idx,
        "hfPara": insert_para_idx,
        "inner_control": table_control_idx,
        "innerControl": table_control_idx,
    })
    .to_string())
}

fn formula_target(args: &Map<String, Value>) -> Result<(usize, usize), String> {
    let row = opt_usize(args, "target_row")
        .or_else(|| opt_usize(args, "targetRow"))
        .or_else(|| opt_usize(args, "row"))
        .ok_or_else(|| "target_row is required".to_string())?;
    let col = opt_usize(args, "target_col")
        .or_else(|| opt_usize(args, "targetCol"))
        .or_else(|| opt_usize(args, "col"))
        .ok_or_else(|| "target_col is required".to_string())?;
    Ok((row, col))
}

fn formula_write_result(args: &Map<String, Value>) -> bool {
    opt_bool(args, "write_result")
        .or_else(|| opt_bool(args, "writeResult"))
        .unwrap_or(false)
}

fn evaluate_header_footer_table_formula(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let inner_control_idx = req_inner_control(args)?;
    let (target_row, target_col) = formula_target(args)?;
    let formula = req_str(args, "formula")?;
    let write_result = formula_write_result(args);
    let (scope, result) = {
        let (scope, table) = header_footer_table_mut(core, args)?;
        let result = DocumentCore::evaluate_table_formula_in_table(
            table,
            target_row,
            target_col,
            formula,
            write_result,
        )
        .map_err(|e| e.to_string())?;
        (scope, result)
    };
    if write_result {
        if let Some(section) = core.document.sections.get_mut(section_idx) {
            section.raw_stream = None;
        }
        core.recompose_section(section_idx);
        core.paginate_if_needed();
    }
    let mut value: Value = serde_json::from_str(&result).map_err(|e| e.to_string())?;
    if let Some(object) = value.as_object_mut() {
        object.insert("scope".to_string(), json!(scope));
        object.insert("inner_control".to_string(), json!(inner_control_idx));
    }
    Ok(value.to_string())
}

fn get_header_footer_table_dimensions(
    core: &DocumentCore,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let inner_control_idx = req_inner_control(args)?;
    let (scope, table) = header_footer_table_ref(core, args)?;
    Ok(json!({
        "scope": scope,
        "inner_control": inner_control_idx,
        "rowCount": table.row_count,
        "colCount": table.col_count,
        "cellCount": table.cells.len(),
    }))
}

fn get_header_footer_table_properties(
    core: &DocumentCore,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let inner_control_idx = req_inner_control(args)?;
    let (scope, table) = header_footer_table_ref(core, args)?;
    let props = core
        .table_properties_json(table)
        .map_err(|e| e.to_string())?;
    let mut value: Value = serde_json::from_str(&props).map_err(|e| e.to_string())?;
    if let Some(object) = value.as_object_mut() {
        object.insert("scope".to_string(), json!(scope));
        object.insert("inner_control".to_string(), json!(inner_control_idx));
    }
    Ok(value)
}

fn set_header_footer_table_properties(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let inner_control_idx = req_inner_control(args)?;
    let (scope, _) = header_footer_table_ref(core, args)?;
    let props = props_json(args);
    let result = core
        .set_table_properties_target(section_idx, req_usize(args, "para")?, &props, |core| {
            header_footer_table_mut(core, args)
                .map(|(_, table)| table)
                .map_err(crate::HwpError::RenderError)
        })
        .map_err(|e| e.to_string())?;
    let mut value: Value = serde_json::from_str(&result).map_err(|e| e.to_string())?;
    if let Some(object) = value.as_object_mut() {
        object.insert("scope".to_string(), json!(scope));
        object.insert("inner_control".to_string(), json!(inner_control_idx));
    }
    Ok(value.to_string())
}

enum HeaderFooterTableEvent {
    RowInserted,
    ColumnInserted,
    RowDeleted,
    ColumnDeleted,
    CellsMerged,
    CellSplit,
}

fn edit_header_footer_table_structure<F>(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
    event: HeaderFooterTableEvent,
    edit: F,
) -> Result<String, String>
where
    F: FnOnce(&mut Table) -> Result<(), String>,
{
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let outer_para_idx = req_usize(args, "para")?;
    let outer_control_idx = req_usize(args, "control")?;
    let inner_control_idx = req_inner_control(args)?;
    let (scope, row_count, col_count, cell_count) = {
        let (scope, table) = header_footer_table_mut(core, args)?;
        edit(table)?;
        table.dirty = true;
        (scope, table.row_count, table.col_count, table.cells.len())
    };
    if let Some(section) = core.document.sections.get_mut(section_idx) {
        section.raw_stream = None;
    }
    core.recompose_section(section_idx);
    core.paginate_if_needed();
    match event {
        HeaderFooterTableEvent::RowInserted => {
            core.event_log.push(DocumentEvent::TableRowInserted {
                section: section_idx,
                para: outer_para_idx,
                ctrl: outer_control_idx,
            });
        }
        HeaderFooterTableEvent::ColumnInserted => {
            core.event_log.push(DocumentEvent::TableColumnInserted {
                section: section_idx,
                para: outer_para_idx,
                ctrl: outer_control_idx,
            });
        }
        HeaderFooterTableEvent::RowDeleted => {
            core.event_log.push(DocumentEvent::TableRowDeleted {
                section: section_idx,
                para: outer_para_idx,
                ctrl: outer_control_idx,
            });
        }
        HeaderFooterTableEvent::ColumnDeleted => {
            core.event_log.push(DocumentEvent::TableColumnDeleted {
                section: section_idx,
                para: outer_para_idx,
                ctrl: outer_control_idx,
            });
        }
        HeaderFooterTableEvent::CellsMerged => {
            core.event_log.push(DocumentEvent::CellsMerged {
                section: section_idx,
                para: outer_para_idx,
                ctrl: outer_control_idx,
            });
        }
        HeaderFooterTableEvent::CellSplit => {
            core.event_log.push(DocumentEvent::CellSplit {
                section: section_idx,
                para: outer_para_idx,
                ctrl: outer_control_idx,
            });
        }
    }
    Ok(json!({
        "ok": true,
        "scope": scope,
        "inner_control": inner_control_idx,
        "rowCount": row_count,
        "colCount": col_count,
        "cellCount": cell_count,
    })
    .to_string())
}

fn insert_header_footer_table_row(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let row = req_u16(args, "row")?;
    let below = opt_bool(args, "below").unwrap_or(true);
    edit_header_footer_table_structure(core, args, HeaderFooterTableEvent::RowInserted, |table| {
        table.insert_row(row, below)
    })
}

fn insert_header_footer_table_column(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let col = req_u16(args, "col")?;
    let right = opt_bool(args, "right").unwrap_or(true);
    edit_header_footer_table_structure(
        core,
        args,
        HeaderFooterTableEvent::ColumnInserted,
        |table| table.insert_column(col, right),
    )
}

fn delete_header_footer_table_row(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let row = req_u16(args, "row")?;
    edit_header_footer_table_structure(core, args, HeaderFooterTableEvent::RowDeleted, |table| {
        table.delete_row(row)
    })
}

fn delete_header_footer_table_column(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let col = req_u16(args, "col")?;
    edit_header_footer_table_structure(core, args, HeaderFooterTableEvent::ColumnDeleted, |table| {
        table.delete_column(col)
    })
}

fn merge_table_cells_by_path(
    core: &mut DocumentCore,
    section_idx: usize,
    parent_para_idx: usize,
    path: &[(usize, usize, usize)],
    args: &Map<String, Value>,
) -> Result<String, String> {
    let start_row = req_u16(args, "start_row")?;
    let start_col = req_u16(args, "start_col")?;
    let end_row = req_u16(args, "end_row")?;
    let end_col = req_u16(args, "end_col")?;
    let (row_count, col_count, cell_count, event_ctrl) = {
        let table = core
            .get_table_mut_by_cell_path(section_idx, parent_para_idx, path)
            .map_err(|e| e.to_string())?;
        table
            .merge_cells(start_row, start_col, end_row, end_col)
            .map_err(|e| e.to_string())?;
        table.dirty = true;
        (
            table.row_count,
            table.col_count,
            table.cells.len(),
            path[0].0,
        )
    };
    if let Some(section) = core.document.sections.get_mut(section_idx) {
        section.raw_stream = None;
    }
    core.recompose_section(section_idx);
    core.paginate_if_needed();
    core.event_log.push(DocumentEvent::CellsMerged {
        section: section_idx,
        para: parent_para_idx,
        ctrl: event_ctrl,
    });
    Ok(json!({
        "ok": true,
        "cellCount": cell_count,
        "rowCount": row_count,
        "colCount": col_count,
    })
    .to_string())
}

fn split_table_cell_by_path(
    core: &mut DocumentCore,
    section_idx: usize,
    parent_para_idx: usize,
    path: &[(usize, usize, usize)],
    args: &Map<String, Value>,
) -> Result<String, String> {
    let row = req_u16(args, "row")?;
    let col = req_u16(args, "col")?;
    let rows = opt_u16(args, "rows");
    let cols = opt_u16(args, "cols");
    let (row_count, col_count, cell_count, event_ctrl) = {
        let table = core
            .get_table_mut_by_cell_path(section_idx, parent_para_idx, path)
            .map_err(|e| e.to_string())?;
        if rows.is_some() || cols.is_some() {
            table
                .split_cell_into(
                    row,
                    col,
                    rows.unwrap_or(1),
                    cols.unwrap_or(1),
                    opt_bool(args, "equal_row_height").unwrap_or(true),
                    opt_bool(args, "merge_first").unwrap_or(false),
                )
                .map_err(|e| e.to_string())?;
        } else {
            table.split_cell(row, col).map_err(|e| e.to_string())?;
        }
        table.dirty = true;
        (
            table.row_count,
            table.col_count,
            table.cells.len(),
            path[0].0,
        )
    };
    if let Some(section) = core.document.sections.get_mut(section_idx) {
        section.raw_stream = None;
    }
    core.recompose_section(section_idx);
    core.paginate_if_needed();
    core.event_log.push(DocumentEvent::CellSplit {
        section: section_idx,
        para: parent_para_idx,
        ctrl: event_ctrl,
    });
    Ok(json!({
        "ok": true,
        "cellCount": cell_count,
        "rowCount": row_count,
        "colCount": col_count,
    })
    .to_string())
}

fn merge_header_footer_table_cells(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let start_row = req_u16(args, "start_row")?;
    let start_col = req_u16(args, "start_col")?;
    let end_row = req_u16(args, "end_row")?;
    let end_col = req_u16(args, "end_col")?;
    edit_header_footer_table_structure(core, args, HeaderFooterTableEvent::CellsMerged, |table| {
        table.merge_cells(start_row, start_col, end_row, end_col)
    })
}

fn split_header_footer_table_cell(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let row = req_u16(args, "row")?;
    let col = req_u16(args, "col")?;
    let rows = opt_u16(args, "rows");
    let cols = opt_u16(args, "cols");
    edit_header_footer_table_structure(core, args, HeaderFooterTableEvent::CellSplit, |table| {
        if rows.is_some() || cols.is_some() {
            table.split_cell_into(
                row,
                col,
                rows.unwrap_or(1),
                cols.unwrap_or(1),
                opt_bool(args, "equal_row_height").unwrap_or(true),
                opt_bool(args, "merge_first").unwrap_or(false),
            )
        } else {
            table.split_cell(row, col)
        }
    })
}

fn get_header_footer_table_cell_text(
    core: &DocumentCore,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let inner_control_idx = req_inner_control(args)?;
    let (scope, table) = header_footer_table_ref(core, args)?;
    let (cell_idx, cell_para_idx) = table_cell_target(table, args)?;
    let cell = table
        .cells
        .get(cell_idx)
        .ok_or_else(|| format!("cell index {cell_idx} out of range"))?;
    let cell_para = cell.paragraphs.get(cell_para_idx).ok_or_else(|| {
        format!(
            "cell paragraph index {cell_para_idx} out of range (total {})",
            cell.paragraphs.len()
        )
    })?;
    Ok(json!({
        "scope": scope,
        "inner_control": inner_control_idx,
        "cell": cell_idx,
        "cell_para": cell_para_idx,
        "text": cell_para.text.clone(),
    }))
}

fn set_header_footer_table_cell_text(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
    text: &str,
) -> Result<Value, String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let outer_para_idx = req_usize(args, "para")?;
    let outer_control_idx = req_usize(args, "control")?;
    let inner_para_idx = nested_para_arg(args);
    let inner_control_idx = req_inner_control(args)?;
    let (scope, cell_idx, cell_para_idx) = {
        let section = core
            .document
            .sections
            .get_mut(section_idx)
            .ok_or_else(|| format!("section index {section_idx} out of range"))?;
        let outer_para = section
            .paragraphs
            .get_mut(outer_para_idx)
            .ok_or_else(|| format!("outer paragraph index {outer_para_idx} out of range"))?;
        let outer_ctrl = outer_para
            .controls
            .get_mut(outer_control_idx)
            .ok_or_else(|| format!("outer control index {outer_control_idx} out of range"))?;
        let (scope, inner_paragraphs) = match outer_ctrl {
            Control::Header(header) => ("header", &mut header.paragraphs),
            Control::Footer(footer) => ("footer", &mut footer.paragraphs),
            _ => return Err("outer control is not a header/footer".to_string()),
        };
        validate_container_scope(args, scope)?;
        let inner_para = inner_paragraphs
            .get_mut(inner_para_idx)
            .ok_or_else(|| format!("inner paragraph index {inner_para_idx} out of range"))?;
        let table = inner_para
            .controls
            .get_mut(inner_control_idx)
            .and_then(|control| match control {
                Control::Table(table) => Some(table.as_mut()),
                _ => None,
            })
            .ok_or_else(|| {
                format!("inner control index {inner_control_idx} is not a header/footer table")
            })?;
        let (cell_idx, cell_para_idx) = table_cell_target(table, args)?;
        table.dirty = true;
        {
            let cell = table
                .cells
                .get_mut(cell_idx)
                .ok_or_else(|| format!("cell index {cell_idx} out of range"))?;
            let paragraph_count = cell.paragraphs.len();
            let cell_para = cell.paragraphs.get_mut(cell_para_idx).ok_or_else(|| {
                format!(
                    "cell paragraph index {cell_para_idx} out of range (total {})",
                    paragraph_count
                )
            })?;
            let len = cell_para.text.chars().count();
            if len > 0 {
                cell_para.delete_text_at(0, len);
            }
            if !text.is_empty() {
                cell_para.insert_text_at(0, text);
            }
        }
        (scope, cell_idx, cell_para_idx)
    };
    if let Some(section) = core.document.sections.get_mut(section_idx) {
        section.raw_stream = None;
    }
    core.recompose_section(section_idx);
    core.paginate_if_needed();
    core.event_log.push(DocumentEvent::CellTextChanged {
        section: section_idx,
        para: outer_para_idx,
        ctrl: outer_control_idx,
        cell: cell_idx,
    });
    Ok(json!({
        "ok": true,
        "scope": scope,
        "inner_control": inner_control_idx,
        "cell": cell_idx,
        "cell_para": cell_para_idx,
    }))
}

fn apply_header_footer_table_cell_char_format(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let outer_para_idx = req_usize(args, "para")?;
    let inner_control_idx = req_inner_control(args)?;
    let start = req_usize(args, "start")?;
    let end = req_usize(args, "end")?;
    let props = props_json(args);
    let mut mods = parse_char_shape_mods(&props);
    if json_has_border_keys(&props) {
        let bf_id = core.create_border_fill_from_json(&props);
        mods.border_fill_id = Some(bf_id);
    }

    let (scope, cell_idx, cell_para_idx, base_id) = {
        let (scope, table) = header_footer_table_ref(core, args)?;
        let (cell_idx, cell_para_idx) = table_cell_target(table, args)?;
        let cell = table
            .cells
            .get(cell_idx)
            .ok_or_else(|| format!("cell index {cell_idx} out of range"))?;
        let cell_para = cell.paragraphs.get(cell_para_idx).ok_or_else(|| {
            format!(
                "cell paragraph index {cell_para_idx} out of range (total {})",
                cell.paragraphs.len()
            )
        })?;
        (
            scope,
            cell_idx,
            cell_para_idx,
            cell_para.char_shape_id_at(start).unwrap_or(0),
        )
    };
    let new_id = core.document.find_or_create_char_shape(base_id, &mods);

    {
        let (_, table) = header_footer_table_mut(core, args)?;
        let cell = table
            .cells
            .get_mut(cell_idx)
            .ok_or_else(|| format!("cell index {cell_idx} out of range"))?;
        let paragraph_count = cell.paragraphs.len();
        let cell_para = cell.paragraphs.get_mut(cell_para_idx).ok_or_else(|| {
            format!("cell paragraph index {cell_para_idx} out of range (total {paragraph_count})")
        })?;
        cell_para.apply_char_shape_range(start, end, new_id);
        cell_para.line_segs.clear();
        table.dirty = true;
    }

    if let Some(section) = core.document.sections.get_mut(section_idx) {
        section.raw_stream = None;
    }
    core.recompose_section(section_idx);
    core.paginate_if_needed();
    core.event_log.push(DocumentEvent::CharFormatChanged {
        section: section_idx,
        para: outer_para_idx,
        start,
        end,
    });
    Ok(json!({
        "ok": true,
        "scope": scope,
        "inner_control": inner_control_idx,
        "cell": cell_idx,
        "cell_para": cell_para_idx,
    })
    .to_string())
}

fn apply_header_footer_table_cell_para_format(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let outer_para_idx = req_usize(args, "para")?;
    let inner_control_idx = req_inner_control(args)?;
    let props = props_json(args);
    let mut mods = parse_para_shape_mods(&props);

    let (scope, cell_idx, cell_para_idx, base_id) = {
        let (scope, table) = header_footer_table_ref(core, args)?;
        let (cell_idx, cell_para_idx) = table_cell_target(table, args)?;
        let cell = table
            .cells
            .get(cell_idx)
            .ok_or_else(|| format!("cell index {cell_idx} out of range"))?;
        let cell_para = cell.paragraphs.get(cell_para_idx).ok_or_else(|| {
            format!(
                "cell paragraph index {cell_para_idx} out of range (total {})",
                cell.paragraphs.len()
            )
        })?;
        (scope, cell_idx, cell_para_idx, cell_para.para_shape_id)
    };

    if json_has_tab_keys(&props) {
        let base_tab_def_id = core
            .document
            .doc_info
            .para_shapes
            .get(base_id as usize)
            .map(|shape| shape.tab_def_id)
            .unwrap_or(0);
        let new_td =
            build_tab_def_from_json(&props, base_tab_def_id, &core.document.doc_info.tab_defs);
        let new_tab_id = core.document.find_or_create_tab_def(new_td);
        mods.tab_def_id = Some(new_tab_id);
    }
    if json_has_border_keys(&props) {
        let bf_id = core.create_border_fill_from_json(&props);
        mods.border_fill_id = Some(bf_id);
    }
    if let Some(arr) = parse_json_i16_array(&props, "borderSpacing", 4) {
        mods.border_spacing = Some([arr[0], arr[1], arr[2], arr[3]]);
    }
    let new_id = core.document.find_or_create_para_shape(base_id, &mods);

    {
        let (_, table) = header_footer_table_mut(core, args)?;
        let cell = table
            .cells
            .get_mut(cell_idx)
            .ok_or_else(|| format!("cell index {cell_idx} out of range"))?;
        let paragraph_count = cell.paragraphs.len();
        let cell_para = cell.paragraphs.get_mut(cell_para_idx).ok_or_else(|| {
            format!("cell paragraph index {cell_para_idx} out of range (total {paragraph_count})")
        })?;
        cell_para.para_shape_id = new_id;
        if mods.line_spacing.is_some() || mods.line_spacing_type.is_some() {
            cell_para.line_segs.clear();
        }
        table.dirty = true;
    }

    if let Some(section) = core.document.sections.get_mut(section_idx) {
        section.raw_stream = None;
    }
    core.recompose_section(section_idx);
    core.paginate_if_needed();
    core.event_log.push(DocumentEvent::ParaFormatChanged {
        section: section_idx,
        para: outer_para_idx,
    });
    Ok(json!({
        "ok": true,
        "scope": scope,
        "inner_control": inner_control_idx,
        "cell": cell_idx,
        "cell_para": cell_para_idx,
    })
    .to_string())
}

fn apply_header_footer_table_cell_style(
    core: &mut DocumentCore,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let section_idx = opt_usize(args, "section").unwrap_or(0);
    let outer_para_idx = req_usize(args, "para")?;
    let outer_control_idx = req_usize(args, "control")?;
    let inner_para_idx = nested_para_arg(args);
    let inner_control_idx = req_inner_control(args)?;
    let style_id = req_usize(args, "style_id")?;
    let (scope, cell_idx, cell_para_idx) = {
        let (scope, table) = header_footer_table_ref(core, args)?;
        let (cell_idx, cell_para_idx) = table_cell_target(table, args)?;
        (scope, cell_idx, cell_para_idx)
    };
    let result = core
        .apply_header_footer_cell_style_native(
            section_idx,
            outer_para_idx,
            outer_control_idx,
            inner_para_idx,
            inner_control_idx,
            cell_idx,
            cell_para_idx,
            style_id,
        )
        .map_err(|e| e.to_string())?;
    let mut value: Value = serde_json::from_str(&result).map_err(|e| e.to_string())?;
    if let Some(object) = value.as_object_mut() {
        object.insert("scope".to_string(), json!(scope));
        object.insert("inner_control".to_string(), json!(inner_control_idx));
        object.insert("cell".to_string(), json!(cell_idx));
        object.insert("cell_para".to_string(), json!(cell_para_idx));
    }
    Ok(value.to_string())
}

fn validate_container_scope(args: &Map<String, Value>, actual: &str) -> Result<(), String> {
    if let Some(scope) =
        opt_str(args, "container_scope").or_else(|| opt_str(args, "containerScope"))
    {
        if scope != actual {
            return Err(format!(
                "container_scope {scope:?} does not match target scope {actual:?}"
            ));
        }
    }
    Ok(())
}

fn parse_cell_path(value: Option<&Value>) -> Result<Vec<(usize, usize, usize)>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| "cell_path must be an array".to_string())?;
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        if let Some(parts) = item.as_array() {
            if parts.len() != 3 {
                return Err("cell_path tuple must have 3 numbers".to_string());
            }
            out.push((
                value_to_usize(&parts[0]).ok_or_else(|| "invalid cell_path control".to_string())?,
                value_to_usize(&parts[1]).ok_or_else(|| "invalid cell_path cell".to_string())?,
                value_to_usize(&parts[2]).ok_or_else(|| "invalid cell_path para".to_string())?,
            ));
        } else if let Some(map) = item.as_object() {
            out.push((
                req_usize(map, "control")?,
                req_usize(map, "cell")?,
                opt_usize(map, "para").unwrap_or(0),
            ));
        } else {
            return Err("cell_path entries must be arrays or objects".to_string());
        }
    }
    Ok(out)
}

fn table_path_arg(args: &Map<String, Value>) -> Result<Option<Vec<(usize, usize, usize)>>, String> {
    if let Some(value) = args.get("table_path").or_else(|| args.get("tablePath")) {
        let path = parse_cell_path(Some(value))?;
        if path.is_empty() {
            return Err("table_path must not be empty".to_string());
        }
        return Ok(Some(path));
    }

    if args.contains_key("cell_path") {
        let mut path = parse_cell_path(args.get("cell_path"))?;
        if let Some(inner_control) =
            opt_usize(args, "inner_control").or_else(|| opt_usize(args, "innerControl"))
        {
            path.push((inner_control, 0, 0));
        }
        if path.is_empty() {
            return Err("cell_path/table_path must not be empty".to_string());
        }
        return Ok(Some(path));
    }

    Ok(None)
}

fn format_cell_path_arg(
    core: &DocumentCore,
    section: usize,
    para: usize,
    args: &Map<String, Value>,
) -> Result<Option<Vec<(usize, usize, usize)>>, String> {
    let targets_table = args.get("table_path").is_some()
        || args.get("tablePath").is_some()
        || req_inner_control(args).is_ok();
    if targets_table {
        let Some(mut path) = table_path_arg(args)? else {
            return Ok(None);
        };
        let table = core
            .resolve_table_by_path(section, para, &path)
            .map_err(|e| e.to_string())?;
        let cell = if let Some(cell) =
            opt_usize(args, "target_cell").or_else(|| opt_usize(args, "targetCell"))
        {
            if cell >= table.cells.len() {
                return Err(format!(
                    "cell index {cell} out of range (total {})",
                    table.cells.len()
                ));
            }
            cell
        } else if args.contains_key("row") || args.contains_key("col") {
            let row = req_u16(args, "row")?;
            let col = req_u16(args, "col")?;
            table
                .cells
                .iter()
                .position(|cell| cell.row == row && cell.col == col)
                .ok_or_else(|| format!("cell not found at row={row}, col={col}"))?
        } else if let Some(cell) = opt_usize(args, "cell") {
            if cell >= table.cells.len() {
                return Err(format!(
                    "cell index {cell} out of range (total {})",
                    table.cells.len()
                ));
            }
            cell
        } else {
            let row = req_u16(args, "row")?;
            let col = req_u16(args, "col")?;
            table
                .cells
                .iter()
                .position(|cell| cell.row == row && cell.col == col)
                .ok_or_else(|| format!("cell not found at row={row}, col={col}"))?
        };
        let cell_para = opt_usize(args, "target_cell_para")
            .or_else(|| opt_usize(args, "targetCellPara"))
            .unwrap_or(0);
        let last = path
            .last_mut()
            .ok_or_else(|| "cell_path/table_path must not be empty".to_string())?;
        last.1 = cell;
        last.2 = cell_para;
        return Ok(Some(path));
    }

    if args.contains_key("cell_path") {
        let path = parse_cell_path(args.get("cell_path"))?;
        if path.is_empty() {
            return Err("cell_path must not be empty".to_string());
        }
        return Ok(Some(path));
    }

    Ok(None)
}

fn exact_cell_path_arg(
    args: &Map<String, Value>,
) -> Result<Option<Vec<(usize, usize, usize)>>, String> {
    if args.contains_key("cell_path")
        && args.get("table_path").is_none()
        && args.get("tablePath").is_none()
    {
        let path = parse_cell_path(args.get("cell_path"))?;
        if path.is_empty() {
            return Err("cell_path must not be empty".to_string());
        }
        return Ok(Some(path));
    }
    Ok(None)
}

fn picture_cell_path_arg(
    args: &Map<String, Value>,
) -> Result<Option<Vec<(usize, usize, usize)>>, String> {
    if let Some(value) = args.get("table_path").or_else(|| args.get("tablePath")) {
        let path = parse_cell_path(Some(value))?;
        if path.is_empty() {
            return Err("table_path must not be empty".to_string());
        }
        return Ok(Some(path));
    }

    if args.contains_key("cell_path") {
        let path = parse_cell_path(args.get("cell_path"))?;
        if path.is_empty() {
            return Err("cell_path must not be empty".to_string());
        }
        return Ok(Some(path));
    }
    Ok(None)
}

fn shape_cell_path_arg(
    args: &Map<String, Value>,
) -> Result<Option<Vec<(usize, usize, usize)>>, String> {
    picture_cell_path_arg(args)
}

fn hidden_comment_cell_path_arg(
    args: &Map<String, Value>,
) -> Result<Option<Vec<(usize, usize, usize)>>, String> {
    picture_cell_path_arg(args)
}

fn group_child_path_arg(args: &Map<String, Value>) -> Result<Option<Vec<usize>>, String> {
    let value = args
        .get("group_child_path")
        .or_else(|| args.get("groupChildPath"));
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(items) = value.as_array() else {
        return Err("group_child_path must be an array".to_string());
    };
    let mut path = Vec::with_capacity(items.len());
    for item in items {
        let index = item
            .as_u64()
            .or_else(|| item.get("index").and_then(Value::as_u64))
            .or_else(|| item.get("child").and_then(Value::as_u64))
            .or_else(|| item.get("group_child").and_then(Value::as_u64))
            .or_else(|| item.get("groupChild").and_then(Value::as_u64))
            .ok_or_else(|| {
                "group_child_path entries must be integers or objects with index".to_string()
            })?;
        path.push(index as usize);
    }
    if path.is_empty() {
        return Err("group_child_path must not be empty".to_string());
    }
    Ok(Some(path))
}

fn group_child_path_json(path: &[usize]) -> String {
    Value::Array(path.iter().map(|index| json!(index)).collect()).to_string()
}

fn is_header_footer_nested_target(args: &Map<String, Value>) -> bool {
    matches!(
        opt_str(args, "container_scope").or_else(|| opt_str(args, "containerScope")),
        Some("header" | "footer")
    ) || args.contains_key("hf_para")
        || args.contains_key("hfPara")
        || args.contains_key("hf_para_idx")
        || args.contains_key("hfParaIndex")
}

fn is_plain_header_footer_paragraph_target(args: &Map<String, Value>) -> bool {
    (args.contains_key("is_header") || args.contains_key("isHeader"))
        && !has_nested_table_cell_selector(args)
}

fn has_nested_table_cell_selector(args: &Map<String, Value>) -> bool {
    [
        "cell_path",
        "table_path",
        "tablePath",
        "inner_control",
        "innerControl",
        "row",
        "col",
        "cell",
        "cell_para",
        "cellPara",
        "target_cell",
        "targetCell",
        "target_cell_para",
        "targetCellPara",
    ]
    .iter()
    .any(|key| args.contains_key(*key))
}

fn nested_para_arg(args: &Map<String, Value>) -> usize {
    opt_usize(args, "inner_para")
        .or_else(|| opt_usize(args, "innerPara"))
        .or_else(|| opt_usize(args, "hf_para"))
        .or_else(|| opt_usize(args, "hfPara"))
        .or_else(|| opt_usize(args, "hf_para_idx"))
        .or_else(|| opt_usize(args, "hfParaIndex"))
        .unwrap_or(0)
}

fn req_inner_control(args: &Map<String, Value>) -> Result<usize, String> {
    opt_usize(args, "inner_control")
        .or_else(|| opt_usize(args, "innerControl"))
        .ok_or_else(|| "inner_control is required".to_string())
}

fn cell_path_json(path: &[(usize, usize, usize)]) -> Value {
    Value::Array(
        path.iter()
            .map(|(control, cell, para)| json!([control, cell, para]))
            .collect(),
    )
}

fn cell_path_object_json(path: &[(usize, usize, usize)]) -> String {
    Value::Array(
        path.iter()
            .map(|(control, cell, para)| {
                json!({
                    "controlIdx": control,
                    "cellIdx": cell,
                    "cellParaIdx": para,
                })
            })
            .collect(),
    )
    .to_string()
}

fn edit_target_cell_path(value: &Option<Value>) -> Vec<(usize, usize, usize)> {
    value
        .as_ref()
        .and_then(|target| target.get("cell_path"))
        .map(|path| parse_cell_path(Some(path)))
        .and_then(Result::ok)
        .unwrap_or_default()
}

fn paragraph_by_cell_path_with_scope<'a>(
    section: &'a crate::model::document::Section,
    parent_para_idx: usize,
    path: &[(usize, usize, usize)],
) -> Result<(&'a Paragraph, &'static str), String> {
    let mut para = section
        .paragraphs
        .get(parent_para_idx)
        .ok_or_else(|| format!("para {parent_para_idx} not found"))?;
    let mut scope = "body";
    for (path_idx, &(control_idx, cell_idx, cell_para_idx)) in path.iter().enumerate() {
        let control = para
            .controls
            .get(control_idx)
            .ok_or_else(|| format!("cell_path control {control_idx} not found"))?;
        para = match control {
            Control::Table(table) => {
                scope = "table_cell";
                let cell = table
                    .cells
                    .get(cell_idx)
                    .ok_or_else(|| format!("cell_path cell {cell_idx} not found"))?;
                cell.paragraphs
                    .get(cell_para_idx)
                    .ok_or_else(|| format!("cell_path paragraph {cell_para_idx} not found"))?
            }
            Control::Shape(shape) => {
                if cell_idx != 0 {
                    return Err(format!(
                        "cell_path[{path_idx}] shape text-box cell index must be 0, got {cell_idx}"
                    ));
                }
                scope = "shape_text_box";
                let text_box = get_textbox_from_shape(shape.as_ref()).ok_or_else(|| {
                    format!("cell_path control {control_idx} is not a shape text-box")
                })?;
                text_box.paragraphs.get(cell_para_idx).ok_or_else(|| {
                    format!("cell_path text-box paragraph {cell_para_idx} not found")
                })?
            }
            _ => {
                return Err(format!(
                    "cell_path control {control_idx} is not a Table or shape text-box"
                ))
            }
        };
    }
    Ok((para, scope))
}

fn props_json(args: &Map<String, Value>) -> String {
    match args.get("props") {
        Some(Value::String(s)) => s.clone(),
        Some(value) => value.to_string(),
        None => {
            let mut props = Map::new();
            for (key, value) in args {
                if !matches!(
                    key.as_str(),
                    "session_id"
                        | "section"
                        | "para"
                        | "control"
                        | "cell"
                        | "cell_para"
                        | "note_para"
                        | "notePara"
                        | "fn_para"
                        | "fnPara"
                        | "fn_para_idx"
                        | "fnParaIndex"
                        | "start"
                        | "end"
                        | "char_offset"
                        | "style_id"
                        | "caption"
                        | "is_caption"
                        | "isCaption"
                        | "caption_para"
                        | "captionPara"
                        | "caption_para_idx"
                        | "captionParaIndex"
                ) {
                    props.insert(key.clone(), value.clone());
                }
            }
            Value::Object(props).to_string()
        }
    }
}

fn style_format_json(
    args: &Map<String, Value>,
    snake_key: &str,
    camel_key: &str,
    long_key: &str,
) -> Result<Option<String>, String> {
    let Some(value) = args
        .get(snake_key)
        .or_else(|| args.get(camel_key))
        .or_else(|| args.get(long_key))
    else {
        return Ok(None);
    };
    match value {
        Value::String(text) => Ok(Some(text.clone())),
        Value::Object(_) => Ok(Some(value.to_string())),
        _ => Err(format!(
            "{snake_key} must be an object or JSON string containing formatter properties"
        )),
    }
}

fn style_char_shape_from_format(
    core: &mut DocumentCore,
    base_id: u16,
    format_json: &str,
) -> Result<u16, String> {
    validate_style_char_format(format_json)?;
    let mut mods = parse_char_shape_mods(format_json);
    if json_has_border_keys(format_json) {
        let border_fill_id = core.create_border_fill_from_json(format_json);
        mods.border_fill_id = Some(border_fill_id);
    }
    let shape_id = core
        .document
        .find_or_create_char_shape(base_id as u32, &mods);
    u16::try_from(shape_id).map_err(|_| format!("char_shape_id out of range: {shape_id}"))
}

fn style_para_shape_from_format(
    core: &mut DocumentCore,
    base_id: u16,
    format_json: &str,
) -> Result<u16, String> {
    validate_style_para_format(format_json)?;
    let mut mods = parse_para_shape_mods(format_json);
    if json_has_tab_keys(format_json) {
        let base_tab_def_id = core
            .document
            .doc_info
            .para_shapes
            .get(base_id as usize)
            .map(|shape| shape.tab_def_id)
            .unwrap_or(0);
        let tab_def = build_tab_def_from_json(
            format_json,
            base_tab_def_id,
            &core.document.doc_info.tab_defs,
        );
        let tab_def_id = core.document.find_or_create_tab_def(tab_def);
        mods.tab_def_id = Some(tab_def_id);
    }
    if json_has_border_keys(format_json) {
        let border_fill_id = core.create_border_fill_from_json(format_json);
        mods.border_fill_id = Some(border_fill_id);
    }
    if let Some(values) = parse_json_i16_array(format_json, "borderSpacing", 4) {
        mods.border_spacing = Some([values[0], values[1], values[2], values[3]]);
    }
    Ok(core.document.find_or_create_para_shape(base_id, &mods))
}

fn validate_style_char_format(format_json: &str) -> Result<(), String> {
    let object = style_format_object(format_json, "char_format")?;
    validate_bool_props(
        &object,
        &[
            "bold",
            "italic",
            "underline",
            "strikethrough",
            "subscript",
            "superscript",
            "emboss",
            "engrave",
            "kerning",
        ],
        "char_format",
    )?;
    validate_integer_props(
        &object,
        &["fontSize"],
        i32::MIN as i64,
        i32::MAX as i64,
        "char_format",
    )?;
    validate_integer_props(&object, &["fontId"], 0, u16::MAX as i64, "char_format")?;
    validate_integer_props(
        &object,
        &[
            "outlineType",
            "shadowType",
            "emphasisDot",
            "underlineShape",
            "strikeShape",
        ],
        0,
        u8::MAX as i64,
        "char_format",
    )?;
    validate_integer_props(
        &object,
        &["shadowOffsetX", "shadowOffsetY"],
        i8::MIN as i64,
        i8::MAX as i64,
        "char_format",
    )?;
    validate_string_enum_prop(
        &object,
        "underlineType",
        &["Bottom", "Top", "None"],
        "char_format",
    )?;
    validate_color_props(
        &object,
        &[
            "textColor",
            "shadeColor",
            "underlineColor",
            "shadowColor",
            "strikeColor",
        ],
        "char_format",
    )?;
    validate_border_fill_format(&object, "char_format")?;
    validate_integer_array_prop(&object, "fontIds", 7, 0, u16::MAX as i64, "char_format")?;
    validate_integer_array_prop(&object, "ratios", 7, 0, u8::MAX as i64, "char_format")?;
    validate_integer_array_prop(
        &object,
        "spacings",
        7,
        i8::MIN as i64,
        i8::MAX as i64,
        "char_format",
    )?;
    validate_integer_array_prop(
        &object,
        "relativeSizes",
        7,
        0,
        u8::MAX as i64,
        "char_format",
    )?;
    validate_integer_array_prop(
        &object,
        "charOffsets",
        7,
        i8::MIN as i64,
        i8::MAX as i64,
        "char_format",
    )?;
    Ok(())
}

fn validate_style_para_format(format_json: &str) -> Result<(), String> {
    let object = style_format_object(format_json, "para_format")?;
    validate_string_enum_prop(
        &object,
        "alignment",
        &["left", "right", "center", "justify", "distribute"],
        "para_format",
    )?;
    validate_string_enum_prop(
        &object,
        "lineSpacingType",
        &["Percent", "Fixed", "SpaceOnly", "Minimum"],
        "para_format",
    )?;
    validate_string_enum_prop(
        &object,
        "headType",
        &["None", "Outline", "Number", "Bullet"],
        "para_format",
    )?;
    validate_integer_props(
        &object,
        &[
            "lineSpacing",
            "indent",
            "marginLeft",
            "marginRight",
            "spacingBefore",
            "spacingAfter",
        ],
        i32::MIN as i64,
        i32::MAX as i64,
        "para_format",
    )?;
    validate_integer_props(
        &object,
        &[
            "paraLevel",
            "verticalAlign",
            "englishBreakUnit",
            "koreanBreakUnit",
        ],
        0,
        u8::MAX as i64,
        "para_format",
    )?;
    validate_integer_props(&object, &["numberingId"], 0, u16::MAX as i64, "para_format")?;
    validate_bool_props(
        &object,
        &[
            "widowOrphan",
            "keepWithNext",
            "keepLines",
            "pageBreakBefore",
            "fontLineHeight",
            "singleLine",
            "autoSpaceKrEn",
            "autoSpaceKrNum",
            "borderConnect",
            "borderIgnoreMargin",
        ],
        "para_format",
    )?;
    validate_integer_array_prop(
        &object,
        "borderSpacing",
        4,
        i16::MIN as i64,
        i16::MAX as i64,
        "para_format",
    )?;
    validate_border_fill_format(&object, "para_format")?;
    validate_tab_format(&object, "para_format")?;
    Ok(())
}

fn style_format_object(format_json: &str, context: &str) -> Result<Map<String, Value>, String> {
    let value: Value = serde_json::from_str(format_json)
        .map_err(|e| format!("{context} must be a JSON object: {e}"))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{context} must be a JSON object"))
}

fn validate_bool_props(
    object: &Map<String, Value>,
    keys: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in keys {
        if let Some(value) = object.get(*key) {
            if !value.is_boolean() {
                return Err(format!("{context}.{key} must be a boolean"));
            }
        }
    }
    Ok(())
}

fn validate_integer_props(
    object: &Map<String, Value>,
    keys: &[&str],
    min: i64,
    max: i64,
    context: &str,
) -> Result<(), String> {
    for key in keys {
        if let Some(value) = object.get(*key) {
            let Some(value) = value.as_i64() else {
                return Err(format!("{context}.{key} must be an integer"));
            };
            if value < min || value > max {
                return Err(format!(
                    "{context}.{key} out of range {min}..={max}: {value}"
                ));
            }
        }
    }
    Ok(())
}

fn validate_string_enum_prop(
    object: &Map<String, Value>,
    key: &str,
    allowed: &[&str],
    context: &str,
) -> Result<(), String> {
    let Some(value) = object.get(key) else {
        return Ok(());
    };
    let Some(text) = value.as_str() else {
        return Err(format!("{context}.{key} must be a string"));
    };
    if !allowed.contains(&text) {
        return Err(format!(
            "{context}.{key} must be one of {}; got {text}",
            allowed.join(", ")
        ));
    }
    Ok(())
}

fn validate_color_props(
    object: &Map<String, Value>,
    keys: &[&str],
    context: &str,
) -> Result<(), String> {
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        let Some(text) = value.as_str() else {
            return Err(format!("{context}.{key} must be a CSS #RRGGBB string"));
        };
        if !is_css_hex_color(text) {
            return Err(format!("{context}.{key} must be a CSS #RRGGBB string"));
        }
    }
    Ok(())
}

fn validate_border_fill_format(object: &Map<String, Value>, context: &str) -> Result<(), String> {
    for key in ["borderLeft", "borderRight", "borderTop", "borderBottom"] {
        let Some(value) = object.get(key) else {
            continue;
        };
        let Some(border) = value.as_object() else {
            return Err(format!("{context}.{key} must be an object"));
        };
        let field_context = format!("{context}.{key}");
        validate_integer_props(
            border,
            &["type", "width"],
            0,
            u8::MAX as i64,
            &field_context,
        )?;
        validate_color_props(border, &["color"], &field_context)?;
    }
    validate_string_enum_prop(object, "fillType", &["none", "solid"], context)?;
    validate_color_props(object, &["fillColor", "patternColor"], context)?;
    validate_integer_props(
        object,
        &["patternType"],
        i32::MIN as i64,
        i32::MAX as i64,
        context,
    )?;
    Ok(())
}

fn validate_tab_format(object: &Map<String, Value>, context: &str) -> Result<(), String> {
    validate_bool_props(object, &["tabAutoLeft", "tabAutoRight"], context)?;
    let Some(value) = object.get("tabStops") else {
        return Ok(());
    };
    let Some(items) = value.as_array() else {
        return Err(format!("{context}.tabStops must be an array"));
    };
    for (idx, item) in items.iter().enumerate() {
        let Some(tab) = item.as_object() else {
            return Err(format!("{context}.tabStops[{idx}] must be an object"));
        };
        let Some(position) = tab.get("position").and_then(Value::as_i64) else {
            return Err(format!(
                "{context}.tabStops[{idx}].position must be an integer"
            ));
        };
        if position < 0 || position > u32::MAX as i64 {
            return Err(format!(
                "{context}.tabStops[{idx}].position out of range 0..={}: {position}",
                u32::MAX
            ));
        }
        let field_context = format!("{context}.tabStops[{idx}]");
        validate_integer_props(tab, &["type", "fill"], 0, u8::MAX as i64, &field_context)?;
    }
    Ok(())
}

fn validate_integer_array_prop(
    object: &Map<String, Value>,
    key: &str,
    len: usize,
    min: i64,
    max: i64,
    context: &str,
) -> Result<(), String> {
    let Some(value) = object.get(key) else {
        return Ok(());
    };
    let Some(items) = value.as_array() else {
        return Err(format!("{context}.{key} must be an integer array"));
    };
    if items.len() != len {
        return Err(format!(
            "{context}.{key} must contain exactly {len} integers"
        ));
    }
    for (idx, item) in items.iter().enumerate() {
        let Some(value) = item.as_i64() else {
            return Err(format!("{context}.{key}[{idx}] must be an integer"));
        };
        if value < min || value > max {
            return Err(format!(
                "{context}.{key}[{idx}] out of range {min}..={max}: {value}"
            ));
        }
    }
    Ok(())
}

fn is_css_hex_color(text: &str) -> bool {
    text.len() == 7
        && text.starts_with('#')
        && text[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn polygon_points_arg(args: &Map<String, Value>) -> Result<Vec<crate::model::Point>, String> {
    let Some(value) = args
        .get("polygon_points")
        .or_else(|| args.get("polygonPoints"))
    else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .ok_or_else(|| "polygon_points must be an array".to_string())?;
    let mut points = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let (x, y) = if let Some(object) = item.as_object() {
            let x = object
                .get("x")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("polygon_points[{idx}].x must be a 32-bit integer"))?;
            let y = object
                .get("y")
                .and_then(Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("polygon_points[{idx}].y must be a 32-bit integer"))?;
            (x, y)
        } else if let Some(pair) = item.as_array() {
            if pair.len() != 2 {
                return Err(format!(
                    "polygon_points[{idx}] array must have two integers"
                ));
            }
            let x = pair[0]
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("polygon_points[{idx}][0] must be a 32-bit integer"))?;
            let y = pair[1]
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("polygon_points[{idx}][1] must be a 32-bit integer"))?;
            (x, y)
        } else {
            return Err(format!(
                "polygon_points[{idx}] must be an object {{x,y}} or [x,y]"
            ));
        };
        points.push(crate::model::Point { x, y });
    }
    Ok(points)
}

fn shape_group_targets_arg(
    args: &Map<String, Value>,
    section: usize,
) -> Result<Vec<(usize, usize)>, String> {
    let items = args
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| "targets must be an array".to_string())?;
    if items.len() < 2 {
        return Err("targets must contain at least two controls".to_string());
    }
    let mut targets = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let target = if let Some(object) = item.as_object() {
            if let Some(target_section) = object.get("section").and_then(value_to_usize) {
                if target_section != section {
                    return Err(format!(
                        "targets[{idx}].section must match section {section}"
                    ));
                }
            }
            let para = object
                .get("para")
                .and_then(value_to_usize)
                .ok_or_else(|| format!("targets[{idx}].para must be a non-negative integer"))?;
            let control = object
                .get("control")
                .and_then(value_to_usize)
                .ok_or_else(|| format!("targets[{idx}].control must be a non-negative integer"))?;
            (para, control)
        } else if let Some(pair) = item.as_array() {
            if pair.len() != 2 {
                return Err(format!("targets[{idx}] array must have [para, control]"));
            }
            let para = value_to_usize(&pair[0])
                .ok_or_else(|| format!("targets[{idx}][0] must be a non-negative integer"))?;
            let control = value_to_usize(&pair[1])
                .ok_or_else(|| format!("targets[{idx}][1] must be a non-negative integer"))?;
            (para, control)
        } else {
            return Err(format!(
                "targets[{idx}] must be an object {{para, control}} or [para, control]"
            ));
        };
        targets.push(target);
    }
    Ok(targets)
}

fn header_footer_shape_group_targets_arg(
    args: &Map<String, Value>,
    section: usize,
    outer_para: usize,
    outer_control: usize,
    default_inner_para: usize,
) -> Result<Vec<(usize, usize)>, String> {
    let items = args
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| "targets must be an array".to_string())?;
    if items.len() < 2 {
        return Err("targets must contain at least two controls".to_string());
    }

    let mut targets = Vec::with_capacity(items.len());
    for (idx, item) in items.iter().enumerate() {
        let target = if let Some(object) = item.as_object() {
            if let Some(target_section) = object.get("section").and_then(value_to_usize) {
                if target_section != section {
                    return Err(format!(
                        "targets[{idx}].section must match section {section}"
                    ));
                }
            }

            let explicit_inner_control = object
                .get("inner_control")
                .or_else(|| object.get("innerControl"))
                .is_some();
            if explicit_inner_control {
                if let Some(target_outer_para) = object.get("para").and_then(value_to_usize) {
                    if target_outer_para != outer_para {
                        return Err(format!(
                            "targets[{idx}].para must match outer header/footer para {outer_para}"
                        ));
                    }
                }
                if let Some(target_outer_control) = object.get("control").and_then(value_to_usize) {
                    if target_outer_control != outer_control {
                        return Err(format!(
                            "targets[{idx}].control must match outer header/footer control {outer_control}"
                        ));
                    }
                }
            }

            let inner_para = object
                .get("inner_para")
                .or_else(|| object.get("innerPara"))
                .or_else(|| object.get("hf_para"))
                .or_else(|| object.get("hfPara"))
                .or_else(|| object.get("hf_para_idx"))
                .or_else(|| object.get("hfParaIndex"))
                .and_then(value_to_usize)
                .or_else(|| {
                    if explicit_inner_control {
                        Some(default_inner_para)
                    } else {
                        object.get("para").and_then(value_to_usize)
                    }
                })
                .ok_or_else(|| {
                    format!("targets[{idx}] requires inner_para or [inner_para, inner_control]")
                })?;
            let inner_control = object
                .get("inner_control")
                .or_else(|| object.get("innerControl"))
                .and_then(value_to_usize)
                .or_else(|| {
                    if explicit_inner_control {
                        None
                    } else {
                        object.get("control").and_then(value_to_usize)
                    }
                })
                .ok_or_else(|| {
                    format!("targets[{idx}] requires inner_control or [inner_para, inner_control]")
                })?;
            (inner_para, inner_control)
        } else if let Some(pair) = item.as_array() {
            if pair.len() != 2 {
                return Err(format!(
                    "targets[{idx}] array must have [inner_para, inner_control]"
                ));
            }
            let inner_para = value_to_usize(&pair[0])
                .ok_or_else(|| format!("targets[{idx}][0] must be a non-negative integer"))?;
            let inner_control = value_to_usize(&pair[1])
                .ok_or_else(|| format!("targets[{idx}][1] must be a non-negative integer"))?;
            (inner_para, inner_control)
        } else {
            return Err(format!(
                "targets[{idx}] must be an object edit_target or [inner_para, inner_control]"
            ));
        };
        targets.push(target);
    }
    Ok(targets)
}

fn req_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    opt_str(args, key).ok_or_else(|| format!("{key} is required"))
}

fn opt_str<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn req_usize(args: &Map<String, Value>, key: &str) -> Result<usize, String> {
    opt_usize(args, key).ok_or_else(|| format!("{key} is required"))
}

fn opt_usize(args: &Map<String, Value>, key: &str) -> Option<usize> {
    args.get(key).and_then(value_to_usize)
}

fn note_para_arg(args: &Map<String, Value>) -> usize {
    opt_usize(args, "note_para")
        .or_else(|| opt_usize(args, "notePara"))
        .or_else(|| opt_usize(args, "fn_para"))
        .or_else(|| opt_usize(args, "fnPara"))
        .or_else(|| opt_usize(args, "fn_para_idx"))
        .or_else(|| opt_usize(args, "fnParaIndex"))
        .unwrap_or(0)
}

fn hidden_para_arg(args: &Map<String, Value>) -> usize {
    opt_usize(args, "hidden_para")
        .or_else(|| opt_usize(args, "hiddenPara"))
        .or_else(|| opt_usize(args, "hc_para"))
        .or_else(|| opt_usize(args, "hcPara"))
        .unwrap_or(0)
}

fn hf_is_header_arg(args: &Map<String, Value>) -> Result<bool, String> {
    opt_bool(args, "is_header")
        .or_else(|| opt_bool(args, "isHeader"))
        .ok_or_else(|| "is_header is required".to_string())
}

fn hf_apply_to_arg(args: &Map<String, Value>) -> u8 {
    opt_u8(args, "apply_to")
        .or_else(|| opt_u8(args, "applyTo"))
        .unwrap_or(0)
}

fn hf_para_arg(args: &Map<String, Value>) -> usize {
    opt_usize(args, "hf_para")
        .or_else(|| opt_usize(args, "hfPara"))
        .or_else(|| opt_usize(args, "hf_para_idx"))
        .or_else(|| opt_usize(args, "hfParaIndex"))
        .unwrap_or(0)
}

fn require_compare_target(args: &Map<String, Value>) -> Result<&'static str, String> {
    require_exactly_one_key(
        args,
        &["other_session_id", "otherSessionId", "other_path", "path"],
        "compare target",
    )
}

fn require_exactly_one_key<'a>(
    args: &Map<String, Value>,
    keys: &[&'a str],
    label: &str,
) -> Result<&'a str, String> {
    let present = keys
        .iter()
        .copied()
        .filter(|key| args.contains_key(*key))
        .collect::<Vec<_>>();
    match present.len() {
        1 => Ok(present[0]),
        0 => Err(format!(
            "{label} requires exactly one of {}",
            keys.join(", ")
        )),
        _ => Err(format!(
            "{label} accepts exactly one of {}; got {}",
            keys.join(", "),
            present.join(", ")
        )),
    }
}

fn validate_replace_text_mode(args: &Map<String, Value>) -> Result<(), String> {
    let query_mode = args.contains_key("query");
    let positional_mode = args.contains_key("char_offset") || args.contains_key("length");
    if query_mode && positional_mode {
        return Err(
            "replace target accepts either query or char_offset+length, not both".to_string(),
        );
    }
    if query_mode {
        return Ok(());
    }
    if args.contains_key("char_offset") && args.contains_key("length") {
        return Ok(());
    }
    Err("replace target requires query or char_offset+length".to_string())
}

fn replace_layout_policy_arg(args: &Map<String, Value>) -> Result<TextReplaceLayoutPolicy, String> {
    let value = opt_str(args, "layout_policy")
        .or_else(|| opt_str(args, "layoutPolicy"))
        .unwrap_or("reflow");
    match value {
        "reflow" => Ok(TextReplaceLayoutPolicy::Reflow),
        "preserve_source_line_segments" | "preserveSourceLineSegments" => {
            Ok(TextReplaceLayoutPolicy::PreserveSourceLineSegments)
        }
        other => Err(format!(
            "unsupported layout_policy {other:?}; expected reflow or preserve_source_line_segments"
        )),
    }
}

fn req_u16(args: &Map<String, Value>, key: &str) -> Result<u16, String> {
    opt_u16(args, key).ok_or_else(|| format!("{key} is required"))
}

fn opt_u16(args: &Map<String, Value>, key: &str) -> Option<u16> {
    opt_usize(args, key).and_then(|v| u16::try_from(v).ok())
}

fn req_u8(args: &Map<String, Value>, key: &str) -> Result<u8, String> {
    opt_u8(args, key).ok_or_else(|| format!("{key} is required"))
}

fn opt_u8(args: &Map<String, Value>, key: &str) -> Option<u8> {
    opt_usize(args, key).and_then(|v| u8::try_from(v).ok())
}

fn opt_i16(args: &Map<String, Value>, key: &str) -> Option<i16> {
    args.get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i16::try_from(v).ok())
}

fn req_u32(args: &Map<String, Value>, key: &str) -> Result<u32, String> {
    opt_u32(args, key).ok_or_else(|| format!("{key} is required"))
}

fn opt_u32(args: &Map<String, Value>, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
}

fn opt_f64(args: &Map<String, Value>, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64)
}

fn opt_i32(args: &Map<String, Value>, key: &str) -> Option<i32> {
    args.get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
}

fn opt_bool(args: &Map<String, Value>, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

fn value_to_usize(value: &Value) -> Option<usize> {
    value.as_u64().and_then(|v| usize::try_from(v).ok())
}
