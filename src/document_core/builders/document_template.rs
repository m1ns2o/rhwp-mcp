//! Lightweight document-template extraction and rebuild support for MCP tests.
//!
//! This template is intentionally structural, not a lossless document clone. It
//! captures text paragraphs, page/column breaks, table cell text, nested table
//! blocks, table geometry/layout/style, embedded/linked pictures, object placeholders,
//! basic text formatting, and simple header/footer text/tables so MCP workflows can
//! regenerate and compare complex forms.

use crate::document_core::{parse_char_shape_mods, parse_para_shape_mods};
use crate::model::bin_data::{
    BinData, BinDataCompression, BinDataContent, BinDataStatus, BinDataType,
};
use crate::model::control::{Control, Equation};
use crate::model::document::Section;
use crate::model::footnote::{FootnoteNumbering, FootnotePlacement, FootnoteShape, NumberFormat};
use crate::model::header_footer::{HeaderFooterApply, MasterPage};
use crate::model::image::{
    CropInfo, EffectColor, EffectPoint, EffectRange, EffectRgb, ImageAttr, ImageEffect, Picture,
    PictureBlur, PictureEffectChild, PictureEffects, PictureFillOverlay, PictureGlow,
    PictureReflection, PictureShadow, PictureSoftEdge, PictureSolidFill, PictureThreeD,
};
use crate::model::page::{BindingMethod, PageBorderBasis, PageBorderUiBasis, PageDef};
use crate::model::paragraph::{CharShapeRef, ColumnBreakType, LineSeg, Paragraph};
use crate::model::shape::{
    apply_rhwp_chart_data_semantic, common_obj_offsets, ArcShape, Caption, CaptionDirection,
    CaptionVertAlign, ChartShape, CommonObjAttr, ConnectorControlPoint, ConnectorData, CurveShape,
    DrawingObjAttr, EllipseShape, GroupShape, HorzAlign, HorzRelTo, LineShape, LinkLineType,
    ObjectNumberingType, OleShape, PolygonShape, RectangleShape, ShapeComponentAttr, ShapeObject,
    SizeCriterion, TextBox, TextFlow, TextWrap, VertAlign, VertRelTo,
};
use crate::model::style::{
    Alignment, BorderFill, BorderLine, BorderLineType, CharShape, DiagonalLine, Fill, FillType,
    Font, GradientFill, HeadType, ImageFill, ImageFillMode, LineSpacingType, ParaShape,
    ShapeBorderLine, SolidFill, Style, SubstFont, UnderlineType,
};
use crate::model::table::{Cell, Table, TablePageBreak, TableZone, VerticalAlign};
use crate::model::{Padding, Point};
use crate::parser::tags::{CTRL_EQUATION, CTRL_GEN_SHAPE, SHAPE_OLE_ID, SHAPE_RECT_ID};
use crate::parser::tags::{
    SHAPE_ARC_ID, SHAPE_CONNECTOR_ID, SHAPE_CURVE_ID, SHAPE_ELLIPSE_ID, SHAPE_LINE_ID,
    SHAPE_POLYGON_ID,
};
use crate::renderer::style_resolver::resolve_styles;
use crate::DocumentCore;
use base64::Engine;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const TEMPLATE_VERSION: &str = "rhwp-document-template-v1";
const OBJECT_PLACEHOLDER_PREFIX: &str = "rhwp-template-placeholder:";
const MAX_PICTURE_HOST_LINE_SEGMENT_HEIGHT: i32 = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentTemplate {
    #[serde(default = "default_template_version")]
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_page_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub font_faces: Vec<Vec<TemplateFont>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub char_shapes: Vec<TemplateCharShape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub para_shapes: Vec<TemplateParaShape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<TemplateStyle>,
    #[serde(default)]
    pub sections: Vec<TemplateSection>,
    #[serde(default)]
    pub headers: Vec<TemplateHeaderFooter>,
    #[serde(default)]
    pub footers: Vec<TemplateHeaderFooter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub master_pages: Vec<TemplateMasterPage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_def: Option<TemplatePageDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_def: Option<TemplateSectionDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_border_fill: Option<TemplatePageBorderFill>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footnote_shape: Option<TemplateFootnoteShape>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endnote_shape: Option<TemplateFootnoteShape>,
    #[serde(default)]
    pub blocks: Vec<TemplateBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplatePageDef {
    #[serde(default = "default_page_width")]
    pub width: u32,
    #[serde(default = "default_page_height")]
    pub height: u32,
    #[serde(default = "default_page_margin_left")]
    pub margin_left: u32,
    #[serde(default = "default_page_margin_right")]
    pub margin_right: u32,
    #[serde(default = "default_page_margin_top")]
    pub margin_top: u32,
    #[serde(default = "default_page_margin_bottom")]
    pub margin_bottom: u32,
    #[serde(default = "default_page_margin_header")]
    pub margin_header: u32,
    #[serde(default = "default_page_margin_footer")]
    pub margin_footer: u32,
    #[serde(default)]
    pub margin_gutter: u32,
    #[serde(default)]
    pub pagination_bottom_tolerance: u32,
    #[serde(default)]
    pub attr: u32,
    #[serde(default)]
    pub landscape: bool,
    #[serde(default)]
    pub binding: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateSectionDef {
    #[serde(default)]
    pub section_id: String,
    #[serde(default)]
    pub flags: u32,
    #[serde(default)]
    pub column_spacing: i16,
    #[serde(default)]
    pub line_grid: i16,
    #[serde(default)]
    pub char_grid: i16,
    #[serde(default)]
    pub wonggoji_format: u8,
    #[serde(default)]
    pub default_tab_spacing: u32,
    #[serde(default)]
    pub tab_stop_val: u32,
    #[serde(default)]
    pub tab_stop_unit: String,
    #[serde(default)]
    pub page_num: u16,
    #[serde(default)]
    pub page_num_type: u8,
    #[serde(default)]
    pub picture_num: u16,
    #[serde(default)]
    pub table_num: u16,
    #[serde(default)]
    pub equation_num: u16,
    #[serde(default)]
    pub hide_header: bool,
    #[serde(default)]
    pub hide_footer: bool,
    #[serde(default)]
    pub hide_master_page: bool,
    #[serde(default)]
    pub hide_border: bool,
    #[serde(default)]
    pub visibility_border: String,
    #[serde(default)]
    pub hide_fill: bool,
    #[serde(default)]
    pub visibility_fill: String,
    #[serde(default)]
    pub hide_page_number: bool,
    #[serde(default)]
    pub hide_empty_line: bool,
    #[serde(default)]
    pub show_line_number: bool,
    #[serde(default)]
    pub text_direction: u8,
    #[serde(default)]
    pub outline_numbering_id: u16,
    #[serde(default)]
    pub memo_shape_id_ref: u16,
    #[serde(default)]
    pub text_vertical_width_head: u32,
    #[serde(default)]
    pub line_number_restart_type: u8,
    #[serde(default)]
    pub line_number_count_by: u16,
    #[serde(default)]
    pub line_number_distance: u32,
    #[serde(default)]
    pub line_number_start_number: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplatePageBorderFill {
    #[serde(default)]
    pub attr: u32,
    #[serde(default = "default_page_border_basis")]
    pub basis: String,
    #[serde(default)]
    pub spacing_left: i16,
    #[serde(default)]
    pub spacing_right: i16,
    #[serde(default)]
    pub spacing_top: i16,
    #[serde(default)]
    pub spacing_bottom: i16,
    #[serde(default)]
    pub header_inside: bool,
    #[serde(default)]
    pub footer_inside: bool,
    #[serde(default = "default_page_border_fill_area")]
    pub fill_area: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateFootnoteShape {
    #[serde(default)]
    pub attr: u32,
    #[serde(default = "default_footnote_number_format")]
    pub number_format: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_char: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix_char: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix_char: String,
    #[serde(default = "default_footnote_start_number")]
    pub start_number: u16,
    #[serde(default)]
    pub separator_length: i16,
    #[serde(default)]
    pub separator_margin_top: i16,
    #[serde(default)]
    pub separator_margin_bottom: i16,
    #[serde(default)]
    pub note_spacing: i16,
    #[serde(default)]
    pub separator_line_type: u8,
    #[serde(default)]
    pub separator_line_width: u8,
    #[serde(default)]
    pub separator_color: u32,
    #[serde(default = "default_footnote_numbering")]
    pub numbering: String,
    #[serde(default = "default_footnote_placement")]
    pub placement: String,
    #[serde(default)]
    pub number_code_superscript: bool,
    #[serde(default)]
    pub print_inline_after_text: bool,
    #[serde(default)]
    pub raw_unknown: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TemplateBlock {
    Paragraph {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        break_before: Option<TemplateBreak>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<TemplateStyleRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        char_format: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        char_shape_runs: Vec<TemplateCharShapeRun>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_format: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        line_segments: Vec<TemplateLineSeg>,
    },
    Table {
        rows: Vec<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        break_before: Option<TemplateBreak>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rhwp_saved_gap_before: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_group: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<TemplateStyleRef>,
        #[serde(default, skip_serializing_if = "is_zero_u16")]
        host_para_shape_id: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        para_format: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        line_segments: Vec<TemplateLineSeg>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<TemplateCaption>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        column_widths: Vec<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        row_heights: Vec<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        table_layout: Option<TemplateTableLayout>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_layout: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        border_fill: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        table_zones: Vec<TemplateTableZone>,
        #[serde(default, skip_serializing_if = "is_empty_cell_layout_matrix")]
        cell_layouts: Vec<Vec<TemplateCellLayout>>,
        #[serde(default, skip_serializing_if = "is_empty_format_matrix")]
        cell_formats: Vec<Vec<TemplateTextFormat>>,
        #[serde(default, skip_serializing_if = "is_empty_block_matrix")]
        cell_blocks: Vec<Vec<Vec<TemplateBlock>>>,
    },
    Equation {
        script: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        break_before: Option<TemplateBreak>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_group: Option<u32>,
        #[serde(default = "default_equation_font_size")]
        font_size: u32,
        #[serde(default)]
        color: u32,
        #[serde(default)]
        baseline: i16,
        #[serde(default = "default_equation_font_name")]
        font_name: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        line_mode: String,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        width: u32,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        height: u32,
        #[serde(default = "default_true")]
        treat_as_char: bool,
    },
    Picture {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        break_before: Option<TemplateBreak>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_group: Option<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        line_segments: Vec<TemplateLineSeg>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        image_base64: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        external_path: String,
        #[serde(default = "default_picture_extension")]
        extension: String,
        width: u32,
        height: u32,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        natural_width_px: u32,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        natural_height_px: u32,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        #[serde(default, skip_serializing_if = "is_zero_u8")]
        transparency: u8,
        #[serde(default, skip_serializing_if = "is_zero_i8")]
        brightness: i8,
        #[serde(default, skip_serializing_if = "is_zero_i8")]
        contrast: i8,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        effect: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effects: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layout: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_layout: Option<Value>,
        #[serde(default = "default_true")]
        treat_as_char: bool,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        horz_offset: u32,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        vert_offset: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<TemplateCaption>,
    },
    ObjectPlaceholder {
        object_kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        break_before: Option<TemplateBreak>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        host_group: Option<u32>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        shape_kind: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        description: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        placeholder_text: String,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        width: u32,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        height: u32,
        #[serde(default = "default_true")]
        treat_as_char: bool,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        horz_offset: u32,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        vert_offset: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<TemplateCaption>,
        #[serde(default, skip_serializing_if = "is_zero_u32")]
        shape_component_id: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        geometry: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        drawing_style: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layout: Option<Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        children: Vec<TemplateBlock>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        raw_hwp_chart_data_base64: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        raw_hwp_ole_tag_base64: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        ole_bin_data_base64: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        ole_extension: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        ole_object_type: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        ole_draw_aspect: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        ole_eq_base_line: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        ole_has_moniker: String,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateTextFormat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<TemplateStyleRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_format: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub char_shape_runs: Vec<TemplateCharShapeRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub para_format: Option<Value>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateCharShapeRun {
    #[serde(default)]
    pub start_pos: u32,
    #[serde(default)]
    pub char_shape_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateCellLayout {
    #[serde(default = "default_one_u16")]
    pub col_span: u16,
    #[serde(default = "default_one_u16")]
    pub row_span: u16,
    #[serde(default = "default_cell_padding_left")]
    pub padding_left: i16,
    #[serde(default = "default_cell_padding_right")]
    pub padding_right: i16,
    #[serde(default = "default_cell_padding_top")]
    pub padding_top: i16,
    #[serde(default = "default_cell_padding_bottom")]
    pub padding_bottom: i16,
    #[serde(default)]
    pub apply_inner_margin: bool,
    #[serde(default = "default_cell_vertical_align")]
    pub vertical_align: String,
    #[serde(default)]
    pub text_direction: u8,
    #[serde(default)]
    pub is_header: bool,
    #[serde(default)]
    pub cell_protect: bool,
    #[serde(default)]
    pub editable_in_form: bool,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default = "default_cell_line_wrap")]
    pub line_wrap: String,
    #[serde(default)]
    pub link_list_id_ref: u32,
    #[serde(default)]
    pub link_list_next_id_ref: u32,
    #[serde(default)]
    pub text_width: u32,
    #[serde(default)]
    pub text_height: u32,
    #[serde(default)]
    pub has_text_ref: u8,
    #[serde(default)]
    pub has_num_ref: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub field_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateTableZone {
    #[serde(default)]
    pub start_row: u16,
    #[serde(default)]
    pub start_col: u16,
    #[serde(default)]
    pub end_row: u16,
    #[serde(default)]
    pub end_col: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TemplateTableLayout {
    #[serde(default)]
    pub cell_spacing: i16,
    #[serde(default = "default_cell_padding_left")]
    pub padding_left: i16,
    #[serde(default = "default_cell_padding_right")]
    pub padding_right: i16,
    #[serde(default = "default_cell_padding_top")]
    pub padding_top: i16,
    #[serde(default = "default_cell_padding_bottom")]
    pub padding_bottom: i16,
    #[serde(default = "default_table_page_break")]
    pub page_break: String,
    #[serde(default)]
    pub repeat_header: bool,
    #[serde(default = "default_table_outer_margin")]
    pub outer_margin_left: i16,
    #[serde(default = "default_table_outer_margin")]
    pub outer_margin_right: i16,
    #[serde(default = "default_table_outer_margin")]
    pub outer_margin_top: i16,
    #[serde(default = "default_table_outer_margin")]
    pub outer_margin_bottom: i16,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateCaption {
    #[serde(default = "default_caption_direction")]
    pub direction: String,
    #[serde(default = "default_caption_vert_align")]
    pub vert_align: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub spacing: i16,
    #[serde(default)]
    pub max_width: u32,
    #[serde(default)]
    pub include_margin: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<TemplateBlock>,
}

impl Default for TemplateTableLayout {
    fn default() -> Self {
        Self {
            cell_spacing: 0,
            padding_left: default_cell_padding_left(),
            padding_right: default_cell_padding_right(),
            padding_top: default_cell_padding_top(),
            padding_bottom: default_cell_padding_bottom(),
            page_break: default_table_page_break(),
            repeat_header: false,
            outer_margin_left: default_table_outer_margin(),
            outer_margin_right: default_table_outer_margin(),
            outer_margin_top: default_table_outer_margin(),
            outer_margin_bottom: default_table_outer_margin(),
        }
    }
}

impl Default for TemplateCellLayout {
    fn default() -> Self {
        Self {
            col_span: 1,
            row_span: 1,
            padding_left: 510,
            padding_right: 510,
            padding_top: 141,
            padding_bottom: 141,
            apply_inner_margin: false,
            vertical_align: default_cell_vertical_align(),
            text_direction: 0,
            is_header: false,
            cell_protect: false,
            editable_in_form: false,
            dirty: false,
            line_wrap: default_cell_line_wrap(),
            link_list_id_ref: 0,
            link_list_next_id_ref: 0,
            text_width: 0,
            text_height: 0,
            has_text_ref: 0,
            has_num_ref: 0,
            field_name: String::new(),
            border_fill: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateFont {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default)]
    pub alt_type: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_name: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_hwp_font_base64: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub type_info_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subst_font: Option<TemplateSubstFont>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TemplateSubstFont {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub face: String,
    #[serde(default)]
    pub font_type: u8,
    #[serde(default)]
    pub is_embedded: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bin_item_id_ref: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateCharShape {
    pub id: u32,
    #[serde(default)]
    pub font_ids: [u16; 7],
    #[serde(default = "default_ratio_array")]
    pub ratios: [u8; 7],
    #[serde(default)]
    pub spacings: [i8; 7],
    #[serde(default = "default_ratio_array")]
    pub relative_sizes: [u8; 7],
    #[serde(default)]
    pub char_offsets: [i8; 7],
    #[serde(default)]
    pub base_size: i32,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub underline_type: String,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub underline_shape: u8,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub underline_color: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strikethrough: bool,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub strike_shape: u8,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub strike_color: u32,
    #[serde(default)]
    pub text_color: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_hwp_char_shape_base64: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateParaShape {
    pub id: u16,
    #[serde(default)]
    pub alignment: String,
    #[serde(default)]
    pub line_spacing_type: String,
    #[serde(default)]
    pub line_spacing: i32,
    #[serde(default)]
    pub indent: i32,
    #[serde(default)]
    pub margin_left: i32,
    #[serde(default)]
    pub margin_right: i32,
    #[serde(default)]
    pub spacing_before: i32,
    #[serde(default)]
    pub spacing_after: i32,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub tab_def_id: u16,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub numbering_id: u16,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub border_fill_id: u16,
    #[serde(default, skip_serializing_if = "is_default_border_spacing")]
    pub border_spacing: [i16; 4],
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub attr1: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub attr2: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub attr3: u32,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub line_spacing_v2: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub suppress_line_numbers: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub checked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_spacing_easian_eng: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_spacing_easian_num: Option<bool>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head_type: String,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub para_level: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_hwp_para_shape_base64: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateStyle {
    pub id: u8,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub english_name: String,
    #[serde(default)]
    pub style_type: u8,
    #[serde(default)]
    pub next_style_id: u8,
    #[serde(default)]
    pub lang_id: i16,
    #[serde(default)]
    pub para_shape_id: u16,
    #[serde(default)]
    pub char_shape_id: u16,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw_hwp_style_base64: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateStyleRef {
    pub id: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub english_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateBreak {
    Page,
    Column,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TemplateLineSeg {
    #[serde(default)]
    pub text_start: u32,
    #[serde(default)]
    pub vertical_pos: i32,
    #[serde(default)]
    pub line_height: i32,
    #[serde(default)]
    pub text_height: i32,
    #[serde(default)]
    pub baseline_distance: i32,
    #[serde(default)]
    pub line_spacing: i32,
    #[serde(default)]
    pub column_start: i32,
    #[serde(default)]
    pub segment_width: i32,
    #[serde(default)]
    pub tag: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateHeaderFooter {
    #[serde(default)]
    pub section: usize,
    #[serde(default)]
    pub apply_to: u8,
    #[serde(default)]
    pub blocks: Vec<TemplateBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateMasterPage {
    #[serde(default)]
    pub section: usize,
    #[serde(default)]
    pub apply_to: u8,
    #[serde(default)]
    pub is_extension: bool,
    #[serde(default)]
    pub overlap: bool,
    #[serde(default)]
    pub replace_base: bool,
    #[serde(default)]
    pub ext_flags: u16,
    #[serde(default)]
    pub text_width: u32,
    #[serde(default)]
    pub text_height: u32,
    #[serde(default)]
    pub text_ref: u8,
    #[serde(default)]
    pub num_ref: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hwpx_page_number: Option<u16>,
    #[serde(default)]
    pub blocks: Vec<TemplateBlock>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TemplateStats {
    pub section_count: usize,
    pub block_count: usize,
    pub paragraph_count: usize,
    pub table_count: usize,
    pub table_cell_count: usize,
    pub equation_count: usize,
    pub picture_count: usize,
    pub object_placeholder_count: usize,
    pub char_shape_count: usize,
    pub para_shape_count: usize,
    pub style_count: usize,
    pub header_count: usize,
    pub footer_count: usize,
    pub master_page_count: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TemplatePageFlowCompactReport {
    pub empty_paragraph_page_breaks_removed: usize,
    pub floating_object_page_breaks_removed: usize,
    pub inline_picture_page_breaks_removed: usize,
    pub sibling_paragraph_page_breaks_removed: usize,
    pub total_removed: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ExtractDocumentTemplateOptions {
    pub preserve_empty_paragraphs: bool,
}

pub fn parse_template_value(value: Value) -> Result<DocumentTemplate, String> {
    serde_json::from_value(value).map_err(|e| format!("invalid document template object: {e}"))
}

pub fn parse_template_str(text: &str) -> Result<DocumentTemplate, String> {
    serde_json::from_str(text).map_err(|e| format!("invalid document template JSON: {e}"))
}

pub fn compact_page_flow_artifacts(
    template: &mut DocumentTemplate,
) -> TemplatePageFlowCompactReport {
    compact_page_flow_artifacts_limited(template, usize::MAX)
}

pub fn compact_page_flow_artifacts_limited(
    template: &mut DocumentTemplate,
    max_removals: usize,
) -> TemplatePageFlowCompactReport {
    let mut report = TemplatePageFlowCompactReport::default();
    let mut remaining = max_removals;
    for category in [
        CompactArtifactCategory::EmptyParagraph,
        CompactArtifactCategory::DelayedSibling,
        CompactArtifactCategory::FloatingObject,
        CompactArtifactCategory::InlinePicture,
    ] {
        if remaining == 0 {
            break;
        }
        for section in &mut template.sections {
            compact_page_flow_artifacts_in_blocks(
                &mut section.blocks,
                &mut report,
                &mut remaining,
                category,
            );
            if remaining == 0 {
                break;
            }
        }
    }
    report.total_removed = report.empty_paragraph_page_breaks_removed
        + report.floating_object_page_breaks_removed
        + report.inline_picture_page_breaks_removed
        + report.sibling_paragraph_page_breaks_removed;
    report
}

fn compact_page_flow_artifacts_in_blocks(
    blocks: &mut [TemplateBlock],
    report: &mut TemplatePageFlowCompactReport,
    remaining: &mut usize,
    category: CompactArtifactCategory,
) {
    let mut flow_state = CompactFlowState::default();
    for block in blocks {
        if *remaining == 0 {
            break;
        }
        match block {
            TemplateBlock::Paragraph {
                text, break_before, ..
            } if category == CompactArtifactCategory::EmptyParagraph
                && text.is_empty()
                && *break_before == Some(TemplateBreak::Page) =>
            {
                *break_before = None;
                *remaining = remaining.saturating_sub(1);
                report.empty_paragraph_page_breaks_removed += 1;
            }
            TemplateBlock::Paragraph {
                text, break_before, ..
            } if category == CompactArtifactCategory::DelayedSibling
                && *break_before == Some(TemplateBreak::Page)
                && delayed_sibling_page_break_artifact(&flow_state, text) =>
            {
                *break_before = None;
                *remaining = remaining.saturating_sub(1);
                report.sibling_paragraph_page_breaks_removed += 1;
            }
            TemplateBlock::Picture {
                treat_as_char,
                break_before,
                ..
            } if category == CompactArtifactCategory::InlinePicture
                && *treat_as_char
                && *break_before == Some(TemplateBreak::Page)
                && flow_state
                    .recent_paragraph_vpos
                    .is_some_and(is_bottom_anchor_vpos) =>
            {
                *break_before = None;
                *remaining = remaining.saturating_sub(1);
                report.inline_picture_page_breaks_removed += 1;
            }
            TemplateBlock::Picture {
                treat_as_char,
                break_before,
                ..
            }
            | TemplateBlock::ObjectPlaceholder {
                treat_as_char,
                break_before,
                ..
            } if category == CompactArtifactCategory::FloatingObject
                && !*treat_as_char
                && *break_before == Some(TemplateBreak::Page) =>
            {
                *break_before = None;
                *remaining = remaining.saturating_sub(1);
                report.floating_object_page_breaks_removed += 1;
            }
            TemplateBlock::Table { cell_blocks, .. } => {
                for cell in cell_blocks
                    .iter_mut()
                    .flat_map(|row| row.iter_mut())
                    .flat_map(|cell| cell.iter_mut())
                {
                    compact_page_flow_artifacts_in_blocks(
                        std::slice::from_mut(cell),
                        report,
                        remaining,
                        category,
                    );
                    if *remaining == 0 {
                        break;
                    }
                }
            }
            TemplateBlock::ObjectPlaceholder { children, .. } => {
                compact_page_flow_artifacts_in_blocks(children, report, remaining, category);
            }
            _ => {}
        }
        flow_state.note_block(block);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompactArtifactCategory {
    EmptyParagraph,
    DelayedSibling,
    FloatingObject,
    InlinePicture,
}

#[derive(Default)]
struct CompactFlowState {
    recent_paragraph_text: Option<String>,
    recent_paragraph_vpos: Option<i32>,
    intervening_tables: usize,
    intervening_inline_pictures: usize,
}

impl CompactFlowState {
    fn note_block(&mut self, block: &TemplateBlock) {
        match block {
            TemplateBlock::Paragraph {
                text,
                line_segments,
                ..
            } if !text.is_empty() => {
                self.recent_paragraph_text = Some(text.clone());
                self.recent_paragraph_vpos = line_segments.last().map(|seg| seg.vertical_pos);
                self.intervening_tables = 0;
                self.intervening_inline_pictures = 0;
            }
            TemplateBlock::Table { .. } if self.recent_paragraph_text.is_some() => {
                self.intervening_tables += 1;
            }
            TemplateBlock::Picture { treat_as_char, .. }
            | TemplateBlock::ObjectPlaceholder { treat_as_char, .. }
                if *treat_as_char && self.recent_paragraph_text.is_some() =>
            {
                self.intervening_inline_pictures += 1;
            }
            _ => self.clear(),
        }
    }

    fn clear(&mut self) {
        self.recent_paragraph_text = None;
        self.recent_paragraph_vpos = None;
        self.intervening_tables = 0;
        self.intervening_inline_pictures = 0;
    }
}

fn is_bottom_anchor_vpos(vpos: i32) -> bool {
    vpos >= 50_000
}

fn delayed_sibling_page_break_artifact(state: &CompactFlowState, current_text: &str) -> bool {
    state
        .recent_paragraph_text
        .as_deref()
        .is_some_and(|text| text.trim_start().starts_with("(1)"))
        && current_text.trim_start().starts_with("(2)")
        && state
            .recent_paragraph_vpos
            .is_some_and(|vpos| (0..=20_000).contains(&vpos))
        && state.intervening_tables >= 1
        && state.intervening_inline_pictures >= 1
}

fn template_font_faces(core: &DocumentCore) -> Vec<Vec<TemplateFont>> {
    core.document
        .doc_info
        .font_faces
        .iter()
        .map(|fonts| {
            fonts
                .iter()
                .map(|font| TemplateFont {
                    name: font.name.clone(),
                    alt_type: font.alt_type,
                    alt_name: font.alt_name.clone(),
                    default_name: font.default_name.clone(),
                    raw_hwp_font_base64: font
                        .raw_data
                        .as_ref()
                        .map(|raw| base64::engine::general_purpose::STANDARD.encode(raw))
                        .unwrap_or_default(),
                    type_info_base64: font
                        .type_info
                        .as_ref()
                        .map(|raw| base64::engine::general_purpose::STANDARD.encode(raw))
                        .unwrap_or_default(),
                    subst_font: font.subst_font.as_ref().map(|subst| TemplateSubstFont {
                        face: subst.face.clone(),
                        font_type: subst.font_type,
                        is_embedded: subst.is_embedded,
                        bin_item_id_ref: subst.bin_item_id_ref.clone(),
                    }),
                })
                .collect()
        })
        .collect()
}

fn template_char_shapes(core: &DocumentCore) -> Vec<TemplateCharShape> {
    core.document
        .doc_info
        .char_shapes
        .iter()
        .enumerate()
        .filter_map(|(id, shape)| {
            let id = u32::try_from(id).ok()?;
            Some(TemplateCharShape {
                id,
                font_ids: shape.font_ids,
                ratios: shape.ratios,
                spacings: shape.spacings,
                relative_sizes: shape.relative_sizes,
                char_offsets: shape.char_offsets,
                base_size: shape.base_size,
                bold: shape.bold,
                italic: shape.italic,
                underline_type: non_default_underline_type(shape.underline_type),
                underline_shape: shape.underline_shape,
                underline_color: shape.underline_color,
                strikethrough: shape.strikethrough,
                strike_shape: shape.strike_shape,
                strike_color: shape.strike_color,
                text_color: shape.text_color,
                raw_hwp_char_shape_base64: shape
                    .raw_data
                    .as_ref()
                    .map(|raw| base64::engine::general_purpose::STANDARD.encode(raw))
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn template_para_shapes(core: &DocumentCore) -> Vec<TemplateParaShape> {
    core.document
        .doc_info
        .para_shapes
        .iter()
        .enumerate()
        .filter_map(|(id, shape)| {
            let id = u16::try_from(id).ok()?;
            Some(TemplateParaShape {
                id,
                alignment: alignment_str(shape.alignment).to_string(),
                line_spacing_type: line_spacing_type_str(shape.line_spacing_type).to_string(),
                line_spacing: shape.line_spacing,
                indent: shape.indent,
                margin_left: shape.margin_left,
                margin_right: shape.margin_right,
                spacing_before: shape.spacing_before,
                spacing_after: shape.spacing_after,
                tab_def_id: shape.tab_def_id,
                numbering_id: shape.numbering_id,
                border_fill_id: shape.border_fill_id,
                border_spacing: shape.border_spacing,
                attr1: shape.attr1,
                attr2: shape.attr2,
                attr3: shape.attr3,
                line_spacing_v2: shape.line_spacing_v2,
                suppress_line_numbers: shape.suppress_line_numbers,
                checked: shape.checked,
                auto_spacing_easian_eng: shape.auto_spacing_easian_eng,
                auto_spacing_easian_num: shape.auto_spacing_easian_num,
                head_type: head_type_name(shape.head_type).to_string(),
                para_level: shape.para_level,
                raw_hwp_para_shape_base64: shape
                    .raw_data
                    .as_ref()
                    .map(|raw| base64::engine::general_purpose::STANDARD.encode(raw))
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn apply_template_font_faces(
    core: &mut DocumentCore,
    font_faces: &[Vec<TemplateFont>],
) -> Result<(), String> {
    if font_faces.is_empty() {
        return Ok(());
    }

    let restored = font_faces
        .iter()
        .enumerate()
        .map(|(lang_idx, fonts)| {
            fonts
                .iter()
                .enumerate()
                .map(|(font_idx, font)| {
                    let raw_data = if font.raw_hwp_font_base64.is_empty() {
                        None
                    } else {
                        Some(
                            base64::engine::general_purpose::STANDARD
                                .decode(&font.raw_hwp_font_base64)
                                .map_err(|e| {
                                    format!(
                                        "invalid raw_hwp_font_base64 for font face {lang_idx}/{font_idx}: {e}"
                                    )
                                })?,
                        )
                    };
                    let type_info = if font.type_info_base64.is_empty() {
                        None
                    } else {
                        let bytes = base64::engine::general_purpose::STANDARD
                            .decode(&font.type_info_base64)
                            .map_err(|e| {
                                format!(
                                    "invalid type_info_base64 for font face {lang_idx}/{font_idx}: {e}"
                                )
                            })?;
                        let array: [u8; 10] = bytes.try_into().map_err(|bytes: Vec<u8>| {
                            format!(
                                "type_info_base64 for font face {lang_idx}/{font_idx} must decode to 10 bytes, got {}",
                                bytes.len()
                            )
                        })?;
                        Some(array)
                    };
                    Ok(Font {
                        raw_data,
                        raw_hwpx_children: None,
                        name: font.name.clone(),
                        alt_type: font.alt_type,
                        alt_name: font.alt_name.clone(),
                        type_info,
                        default_name: font.default_name.clone(),
                        subst_font: font.subst_font.as_ref().map(|subst| SubstFont {
                            face: subst.face.clone(),
                            font_type: subst.font_type,
                            is_embedded: subst.is_embedded,
                            bin_item_id_ref: subst.bin_item_id_ref.clone(),
                        }),
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .collect::<Result<Vec<_>, String>>()?;

    core.document.doc_info.font_faces = restored;
    core.document.doc_info.raw_stream = None;
    core.document.doc_info.raw_stream_dirty = true;
    core.styles = resolve_styles(&core.document.doc_info, core.dpi);
    Ok(())
}

fn apply_template_shape_families(
    core: &mut DocumentCore,
    char_shapes: &[TemplateCharShape],
    para_shapes: &[TemplateParaShape],
) -> Result<(), String> {
    let mut changed = false;
    if !char_shapes.is_empty() {
        let fallback = core
            .document
            .doc_info
            .char_shapes
            .first()
            .cloned()
            .unwrap_or_default();
        let max_id = char_shapes
            .iter()
            .map(|shape| shape.id as usize)
            .max()
            .unwrap_or(0);
        let mut restored = vec![fallback; max_id + 1];
        for shape in char_shapes {
            let raw_data = if shape.raw_hwp_char_shape_base64.is_empty() {
                None
            } else {
                Some(
                    base64::engine::general_purpose::STANDARD
                        .decode(&shape.raw_hwp_char_shape_base64)
                        .map_err(|e| {
                            format!(
                                "invalid raw_hwp_char_shape_base64 for char shape {}: {e}",
                                shape.id
                            )
                        })?,
                )
            };
            restored[shape.id as usize] = CharShape {
                raw_data,
                font_ids: shape.font_ids,
                ratios: shape.ratios,
                spacings: shape.spacings,
                relative_sizes: shape.relative_sizes,
                char_offsets: shape.char_offsets,
                base_size: shape.base_size,
                bold: shape.bold,
                italic: shape.italic,
                underline_type: underline_type_from_template(&shape.underline_type),
                underline_shape: shape.underline_shape,
                underline_color: shape.underline_color,
                strikethrough: shape.strikethrough,
                strike_shape: shape.strike_shape,
                strike_color: shape.strike_color,
                text_color: shape.text_color,
                ..CharShape::default()
            };
        }
        core.document.doc_info.char_shapes = restored;
        changed = true;
    }

    if !para_shapes.is_empty() {
        let fallback = core
            .document
            .doc_info
            .para_shapes
            .first()
            .cloned()
            .unwrap_or_default();
        let max_id = para_shapes
            .iter()
            .map(|shape| shape.id as usize)
            .max()
            .unwrap_or(0);
        let mut restored = vec![fallback; max_id + 1];
        for shape in para_shapes {
            let raw_data = if shape.raw_hwp_para_shape_base64.is_empty() {
                None
            } else {
                Some(
                    base64::engine::general_purpose::STANDARD
                        .decode(&shape.raw_hwp_para_shape_base64)
                        .map_err(|e| {
                            format!(
                                "invalid raw_hwp_para_shape_base64 for para shape {}: {e}",
                                shape.id
                            )
                        })?,
                )
            };
            restored[shape.id as usize] = ParaShape {
                raw_data,
                attr1: shape.attr1,
                alignment: template_alignment(&shape.alignment),
                line_spacing_type: line_spacing_type_from_name(&shape.line_spacing_type),
                line_spacing: shape.line_spacing,
                indent: shape.indent,
                margin_left: shape.margin_left,
                margin_right: shape.margin_right,
                spacing_before: shape.spacing_before,
                spacing_after: shape.spacing_after,
                tab_def_id: shape.tab_def_id,
                numbering_id: shape.numbering_id,
                border_fill_id: shape.border_fill_id,
                border_spacing: shape.border_spacing,
                attr2: shape.attr2,
                attr3: shape.attr3,
                line_spacing_v2: shape.line_spacing_v2,
                suppress_line_numbers: shape.suppress_line_numbers,
                checked: shape.checked,
                auto_spacing_easian_eng: shape.auto_spacing_easian_eng,
                auto_spacing_easian_num: shape.auto_spacing_easian_num,
                head_type: head_type_from_name(&shape.head_type),
                para_level: shape.para_level,
            };
        }
        core.document.doc_info.para_shapes = restored;
        changed = true;
    }

    if changed {
        core.document.doc_info.raw_stream = None;
        core.document.doc_info.raw_stream_dirty = true;
        core.styles = resolve_styles(&core.document.doc_info, core.dpi);
    }
    Ok(())
}

fn template_styles(core: &DocumentCore) -> Vec<TemplateStyle> {
    core.document
        .doc_info
        .styles
        .iter()
        .enumerate()
        .filter_map(|(id, style)| {
            let id = u8::try_from(id).ok()?;
            Some(TemplateStyle {
                id,
                name: style.local_name.clone(),
                english_name: style.english_name.clone(),
                style_type: style.style_type,
                next_style_id: style.next_style_id,
                lang_id: style.lang_id,
                para_shape_id: style.para_shape_id,
                char_shape_id: style.char_shape_id,
                raw_hwp_style_base64: style
                    .raw_data
                    .as_ref()
                    .map(|raw| base64::engine::general_purpose::STANDARD.encode(raw))
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn apply_template_styles(core: &mut DocumentCore, styles: &[TemplateStyle]) -> Result<(), String> {
    if styles.is_empty() {
        return Ok(());
    }
    let char_shape_count = core.document.doc_info.char_shapes.len();
    let para_shape_count = core.document.doc_info.para_shapes.len();
    let max_style_id = styles.iter().map(|style| style.id).max().unwrap_or(0) as usize;
    let mut restored = vec![Style::default(); max_style_id + 1];
    for style in styles {
        let raw_data = if style.raw_hwp_style_base64.is_empty() {
            None
        } else {
            Some(
                base64::engine::general_purpose::STANDARD
                    .decode(&style.raw_hwp_style_base64)
                    .map_err(|e| {
                        format!("invalid raw_hwp_style_base64 for style {}: {e}", style.id)
                    })?,
            )
        };
        restored[style.id as usize] = Style {
            raw_data,
            local_name: style.name.clone(),
            english_name: style.english_name.clone(),
            style_type: style.style_type,
            next_style_id: if (style.next_style_id as usize) <= max_style_id {
                style.next_style_id
            } else {
                0
            },
            lang_id: style.lang_id,
            para_shape_id: if (style.para_shape_id as usize) < para_shape_count {
                style.para_shape_id
            } else {
                0
            },
            char_shape_id: if (style.char_shape_id as usize) < char_shape_count {
                style.char_shape_id
            } else {
                0
            },
        };
    }
    core.document.doc_info.styles = restored;
    core.document.doc_info.raw_stream = None;
    core.document.doc_info.raw_stream_dirty = true;
    core.styles = resolve_styles(&core.document.doc_info, core.dpi);
    Ok(())
}

pub fn extract_document_template(core: &DocumentCore) -> DocumentTemplate {
    extract_document_template_with_options(core, ExtractDocumentTemplateOptions::default())
}

pub fn extract_document_template_with_options(
    core: &DocumentCore,
    options: ExtractDocumentTemplateOptions,
) -> DocumentTemplate {
    let mut template = DocumentTemplate {
        version: TEMPLATE_VERSION.to_string(),
        source_page_count: Some(core.page_count()),
        font_faces: template_font_faces(core),
        char_shapes: template_char_shapes(core),
        para_shapes: template_para_shapes(core),
        styles: template_styles(core),
        sections: Vec::new(),
        headers: Vec::new(),
        footers: Vec::new(),
        master_pages: Vec::new(),
    };

    for (section_idx, section) in core.document.sections.iter().enumerate() {
        let mut blocks = Vec::new();
        let page_hints = template_page_hints(core, section_idx);
        extract_blocks_from_paragraphs(
            core,
            &section.paragraphs,
            &mut blocks,
            Some((section_idx, &mut template.headers, &mut template.footers)),
            true,
            Some(&page_hints),
            options.preserve_empty_paragraphs,
        );
        normalize_zero_vpos_heading_table_page_breaks(&mut blocks);
        template.sections.push(TemplateSection {
            page_def: Some(template_page_def(&section.section_def.page_def)),
            section_def: Some(template_section_def(&section.section_def)),
            page_border_fill: Some(template_page_border_fill(core, &section.section_def)),
            footnote_shape: Some(template_footnote_shape(&section.section_def.footnote_shape)),
            endnote_shape: Some(template_footnote_shape(&section.section_def.endnote_shape)),
            blocks,
        });
        for master_page in &section.section_def.master_pages {
            let mut blocks = Vec::new();
            extract_blocks_from_paragraphs(
                core,
                &master_page.paragraphs,
                &mut blocks,
                None,
                true,
                None,
                options.preserve_empty_paragraphs,
            );
            template.master_pages.push(TemplateMasterPage {
                section: section_idx,
                apply_to: apply_to_u8(master_page.apply_to),
                is_extension: master_page.is_extension,
                overlap: master_page.overlap,
                replace_base: master_page.replace_base,
                ext_flags: master_page.ext_flags,
                text_width: master_page.text_width,
                text_height: master_page.text_height,
                text_ref: master_page.text_ref,
                num_ref: master_page.num_ref,
                hwpx_page_number: master_page.hwpx_page_number,
                blocks,
            });
        }
    }

    if template.sections.is_empty() {
        template.sections.push(TemplateSection::default());
    }

    template
}

pub fn template_stats(template: &DocumentTemplate) -> TemplateStats {
    let mut stats = TemplateStats {
        section_count: template.sections.len(),
        char_shape_count: template.char_shapes.len(),
        para_shape_count: template.para_shapes.len(),
        style_count: template.styles.len(),
        header_count: template.headers.len(),
        footer_count: template.footers.len(),
        master_page_count: template.master_pages.len(),
        ..Default::default()
    };
    for section in &template.sections {
        accumulate_block_stats(&section.blocks, &mut stats);
    }
    for hf in template.headers.iter().chain(template.footers.iter()) {
        accumulate_block_stats(&hf.blocks, &mut stats);
    }
    for master_page in &template.master_pages {
        accumulate_block_stats(&master_page.blocks, &mut stats);
    }
    stats
}

pub fn build_document_template(template: &DocumentTemplate) -> Result<DocumentCore, String> {
    let mut core = DocumentCore::new_empty();
    core.create_blank_document_native()
        .map_err(|e| e.to_string())?;
    apply_template_font_faces(&mut core, &template.font_faces)?;
    apply_template_shape_families(&mut core, &template.char_shapes, &template.para_shapes)?;
    apply_template_styles(&mut core, &template.styles)?;

    ensure_template_section_count(&mut core, template.sections.len().max(1));
    for (section_idx, section) in template.sections.iter().enumerate() {
        apply_template_section_settings(&mut core, section_idx, section)?;
        let mut body_state = BodyBuildState::default();
        let mut previous_block: Option<&TemplateBlock> = None;
        for block in &section.blocks {
            let saved_tac_gap_before =
                previous_block.and_then(|previous| template_saved_tac_gap_before(previous, block));
            let saved_para_gap_before =
                previous_block.and_then(|previous| template_saved_para_gap_before(previous, block));
            append_body_block(
                &mut core,
                section_idx,
                block,
                None,
                saved_tac_gap_before,
                saved_para_gap_before,
                &mut body_state,
            )?;
            previous_block = Some(block);
        }
    }

    for header in &template.headers {
        create_header_footer_from_template(&mut core, true, header)?;
    }
    for footer in &template.footers {
        create_header_footer_from_template(&mut core, false, footer)?;
    }
    for master_page in &template.master_pages {
        create_master_page_from_template(&mut core, master_page)?;
    }

    core.rebuild_section(0);
    Ok(core)
}

#[derive(Default)]
struct BodyBuildState {
    used_first_body_para: bool,
    last_host_group: Option<u32>,
    last_para_idx: Option<usize>,
}

impl BodyBuildState {
    fn note_para(&mut self, host_group: Option<u32>, para_idx: usize) {
        self.last_host_group = host_group;
        self.last_para_idx = Some(para_idx);
    }

    fn reusable_para(&self, host_group: Option<u32>) -> Option<usize> {
        match (host_group, self.last_host_group, self.last_para_idx) {
            (Some(current), Some(previous), Some(para_idx)) if current == previous => {
                Some(para_idx)
            }
            _ => None,
        }
    }
}

fn ensure_template_section_count(core: &mut DocumentCore, section_count: usize) {
    if section_count == 0 {
        return;
    }
    if core.document.sections.is_empty() {
        core.document.sections.push(Section {
            section_def: Default::default(),
            paragraphs: vec![Paragraph::new_empty()],
            raw_stream: None,
        });
    }

    let base_section_def = core
        .document
        .sections
        .first()
        .map(|section| section.section_def.clone())
        .unwrap_or_default();
    while core.document.sections.len() < section_count {
        core.document.sections.push(Section {
            section_def: base_section_def.clone(),
            paragraphs: vec![Paragraph::new_empty()],
            raw_stream: None,
        });
    }
    core.document.sections.truncate(section_count);
    core.composed = core
        .document
        .sections
        .iter()
        .map(crate::renderer::composer::compose_section)
        .collect();
    core.dirty_sections = vec![true; core.document.sections.len()];
    core.page_tree_cache.borrow_mut().clear();
}

fn template_page_def(page_def: &PageDef) -> TemplatePageDef {
    TemplatePageDef {
        width: page_def.width,
        height: page_def.height,
        margin_left: page_def.margin_left,
        margin_right: page_def.margin_right,
        margin_top: page_def.margin_top,
        margin_bottom: page_def.margin_bottom,
        margin_header: page_def.margin_header,
        margin_footer: page_def.margin_footer,
        margin_gutter: page_def.margin_gutter,
        pagination_bottom_tolerance: page_def.pagination_bottom_tolerance,
        attr: page_def.attr,
        landscape: page_def.landscape,
        binding: binding_method_to_u8(page_def.binding),
    }
}

fn template_section_def(section_def: &crate::model::document::SectionDef) -> TemplateSectionDef {
    TemplateSectionDef {
        section_id: section_def.section_id.clone(),
        flags: section_def.flags,
        column_spacing: section_def.column_spacing,
        line_grid: section_def.line_grid,
        char_grid: section_def.char_grid,
        wonggoji_format: section_def.wonggoji_format,
        default_tab_spacing: section_def.default_tab_spacing,
        tab_stop_val: section_def.tab_stop_val,
        tab_stop_unit: section_def.tab_stop_unit.clone(),
        page_num: section_def.page_num,
        page_num_type: section_def.page_num_type,
        picture_num: section_def.picture_num,
        table_num: section_def.table_num,
        equation_num: section_def.equation_num,
        hide_header: section_def.hide_header,
        hide_footer: section_def.hide_footer,
        hide_master_page: section_def.hide_master_page,
        hide_border: section_def.hide_border,
        visibility_border: section_def.visibility_border.clone(),
        hide_fill: section_def.hide_fill,
        visibility_fill: section_def.visibility_fill.clone(),
        hide_page_number: section_def.hide_page_number,
        hide_empty_line: section_def.hide_empty_line,
        show_line_number: section_def.show_line_number,
        text_direction: section_def.text_direction,
        outline_numbering_id: section_def.outline_numbering_id,
        memo_shape_id_ref: section_def.memo_shape_id_ref,
        text_vertical_width_head: section_def.text_vertical_width_head,
        line_number_restart_type: section_def.line_number_restart_type,
        line_number_count_by: section_def.line_number_count_by,
        line_number_distance: section_def.line_number_distance,
        line_number_start_number: section_def.line_number_start_number,
    }
}

fn template_page_border_fill(
    core: &DocumentCore,
    section_def: &crate::model::document::SectionDef,
) -> TemplatePageBorderFill {
    let page_border_fill = &section_def.page_border_fill;
    let basis = match page_border_fill.ui_basis {
        PageBorderUiBasis::Page => "page",
        PageBorderUiBasis::Paper => "paper",
    };
    let fill_area = match (page_border_fill.attr >> 3) & 0x03 {
        1 => "page",
        2 => "border",
        _ => "paper",
    };
    TemplatePageBorderFill {
        attr: page_border_fill.attr,
        basis: basis.to_string(),
        spacing_left: page_border_fill.spacing_left,
        spacing_right: page_border_fill.spacing_right,
        spacing_top: page_border_fill.spacing_top,
        spacing_bottom: page_border_fill.spacing_bottom,
        header_inside: (page_border_fill.attr & 0x02) != 0,
        footer_inside: (page_border_fill.attr & 0x04) != 0,
        fill_area: fill_area.to_string(),
        border_fill: border_fill_template_json(core, page_border_fill.border_fill_id),
    }
}

fn template_footnote_shape(shape: &FootnoteShape) -> TemplateFootnoteShape {
    TemplateFootnoteShape {
        attr: shape.attr,
        number_format: footnote_number_format_name(shape.number_format).to_string(),
        user_char: footnote_char_to_string(shape.user_char),
        prefix_char: footnote_char_to_string(shape.prefix_char),
        suffix_char: footnote_char_to_string(shape.suffix_char),
        start_number: shape.start_number,
        separator_length: shape.separator_length,
        separator_margin_top: shape.separator_margin_top,
        separator_margin_bottom: shape.separator_margin_bottom,
        note_spacing: shape.note_spacing,
        separator_line_type: shape.separator_line_type,
        separator_line_width: shape.separator_line_width,
        separator_color: shape.separator_color,
        numbering: footnote_numbering_name(shape.numbering).to_string(),
        placement: footnote_placement_name(shape.placement).to_string(),
        number_code_superscript: shape.number_code_superscript,
        print_inline_after_text: shape.print_inline_after_text,
        raw_unknown: shape.raw_unknown,
    }
}

fn apply_template_section_settings(
    core: &mut DocumentCore,
    section_idx: usize,
    template: &TemplateSection,
) -> Result<(), String> {
    if let Some(page_border_fill) = &template.page_border_fill {
        apply_page_border_fill_template(core, section_idx, page_border_fill)?;
    }
    let section = core
        .document
        .sections
        .get_mut(section_idx)
        .ok_or_else(|| format!("template section target not found: {section_idx}"))?;

    if let Some(page_def) = &template.page_def {
        section.section_def.page_def = page_def_to_model(page_def);
    }
    if let Some(section_def) = &template.section_def {
        apply_section_def_template(&mut section.section_def, section_def);
    }
    if let Some(footnote_shape) = &template.footnote_shape {
        section.section_def.footnote_shape = footnote_shape_to_model(footnote_shape);
    }
    if let Some(endnote_shape) = &template.endnote_shape {
        section.section_def.endnote_shape = footnote_shape_to_model(endnote_shape);
    }
    sync_section_def_control(section);
    section.raw_stream = None;
    Ok(())
}

fn footnote_shape_to_model(template: &TemplateFootnoteShape) -> FootnoteShape {
    let mut shape = FootnoteShape {
        attr: template.attr,
        number_format: FootnoteShape::number_format_from_name(
            &template.number_format,
            NumberFormat::Digit,
        ),
        user_char: first_template_char(&template.user_char),
        prefix_char: first_template_char(&template.prefix_char),
        suffix_char: first_template_char(&template.suffix_char),
        start_number: template.start_number.max(1),
        separator_length: template.separator_length,
        separator_margin_top: template.separator_margin_top,
        separator_margin_bottom: template.separator_margin_bottom,
        note_spacing: template.note_spacing,
        separator_line_type: template.separator_line_type,
        separator_line_width: template.separator_line_width,
        separator_color: template.separator_color,
        numbering: footnote_numbering_from_str(&template.numbering, FootnoteNumbering::Continue),
        placement: footnote_placement_from_str(&template.placement, FootnotePlacement::EachColumn),
        number_code_superscript: template.number_code_superscript,
        print_inline_after_text: template.print_inline_after_text,
        raw_unknown: template.raw_unknown,
        raw_hwpx_children: None,
    };
    shape.attr = shape.encode_attr();
    shape
}

fn page_def_to_model(template: &TemplatePageDef) -> PageDef {
    let mut page_def = PageDef {
        width: template.width,
        height: template.height,
        margin_left: template.margin_left,
        margin_right: template.margin_right,
        margin_top: template.margin_top,
        margin_bottom: template.margin_bottom,
        margin_header: template.margin_header,
        margin_footer: template.margin_footer,
        margin_gutter: template.margin_gutter,
        pagination_bottom_tolerance: template.pagination_bottom_tolerance,
        attr: template.attr,
        landscape: template.landscape,
        binding: binding_method_from_u8(template.binding),
    };
    normalize_page_def_attr(&mut page_def);
    page_def
}

fn apply_section_def_template(
    section_def: &mut crate::model::document::SectionDef,
    template: &TemplateSectionDef,
) {
    section_def.flags = template.flags;
    section_def.section_id = template.section_id.clone();
    section_def.column_spacing = template.column_spacing;
    section_def.line_grid = template.line_grid;
    section_def.char_grid = template.char_grid;
    section_def.wonggoji_format = template.wonggoji_format;
    section_def.default_tab_spacing = template.default_tab_spacing;
    section_def.tab_stop_val = template.tab_stop_val;
    section_def.tab_stop_unit = template.tab_stop_unit.clone();
    section_def.page_num = template.page_num;
    section_def.page_num_type = template.page_num_type;
    section_def.picture_num = template.picture_num;
    section_def.table_num = template.table_num;
    section_def.equation_num = template.equation_num;
    section_def.hide_header = template.hide_header;
    section_def.hide_footer = template.hide_footer;
    section_def.hide_master_page = template.hide_master_page;
    section_def.hide_border = template.hide_border;
    section_def.visibility_border = template.visibility_border.clone();
    section_def.hide_fill = template.hide_fill;
    section_def.visibility_fill = template.visibility_fill.clone();
    section_def.hide_page_number = template.hide_page_number;
    section_def.hide_empty_line = template.hide_empty_line;
    section_def.show_line_number = template.show_line_number;
    section_def.text_direction = template.text_direction;
    section_def.outline_numbering_id = template.outline_numbering_id;
    section_def.memo_shape_id_ref = template.memo_shape_id_ref;
    section_def.text_vertical_width_head = template.text_vertical_width_head;
    section_def.line_number_restart_type = template.line_number_restart_type;
    section_def.line_number_count_by = template.line_number_count_by;
    section_def.line_number_distance = template.line_number_distance;
    section_def.line_number_start_number = template.line_number_start_number;
    normalize_section_def_flags(section_def);
}

fn apply_page_border_fill_template(
    core: &mut DocumentCore,
    section_idx: usize,
    template: &TemplatePageBorderFill,
) -> Result<(), String> {
    let border_fill_id = template
        .border_fill
        .as_ref()
        .map(|border_fill| ensure_border_fill_for_template(core, border_fill))
        .transpose()?
        .unwrap_or(0);
    let section = core
        .document
        .sections
        .get_mut(section_idx)
        .ok_or_else(|| format!("template section target not found: {section_idx}"))?;
    let page_border_fill = &mut section.section_def.page_border_fill;
    page_border_fill.attr = page_border_attr_from_template(template);
    page_border_fill.spacing_left = template.spacing_left;
    page_border_fill.spacing_right = template.spacing_right;
    page_border_fill.spacing_top = template.spacing_top;
    page_border_fill.spacing_bottom = template.spacing_bottom;
    page_border_fill.border_fill_id = border_fill_id;
    page_border_fill.raw_hwpx_children = None;
    if template.basis == "page" {
        page_border_fill.ui_basis = PageBorderUiBasis::Page;
        page_border_fill.basis = PageBorderBasis::BodyBased;
    } else {
        page_border_fill.ui_basis = PageBorderUiBasis::Paper;
        page_border_fill.basis = PageBorderBasis::PaperBased;
    }
    section.raw_stream = None;
    Ok(())
}

fn page_border_attr_from_template(template: &TemplatePageBorderFill) -> u32 {
    let mut attr = template.attr;
    if template.basis == "page" {
        attr |= 0x01;
    } else {
        attr &= !0x01;
    }
    if template.header_inside {
        attr |= 0x02;
    } else {
        attr &= !0x02;
    }
    if template.footer_inside {
        attr |= 0x04;
    } else {
        attr &= !0x04;
    }
    attr &= !(0x03 << 3);
    attr |= match template.fill_area.as_str() {
        "page" => 0x01 << 3,
        "border" => 0x02 << 3,
        _ => 0,
    };
    attr
}

fn first_template_char(value: &str) -> char {
    value.chars().next().unwrap_or('\0')
}

fn footnote_char_to_string(value: char) -> String {
    if value == '\0' {
        String::new()
    } else {
        value.to_string()
    }
}

fn footnote_number_format_name(format: NumberFormat) -> &'static str {
    match format {
        NumberFormat::Digit => "digit",
        NumberFormat::CircledDigit => "circledDigit",
        NumberFormat::UpperRoman => "upperRoman",
        NumberFormat::LowerRoman => "lowerRoman",
        NumberFormat::UpperAlpha => "upperAlpha",
        NumberFormat::LowerAlpha => "lowerAlpha",
        NumberFormat::CircledUpperAlpha => "circledUpperAlpha",
        NumberFormat::CircledLowerAlpha => "circledLowerAlpha",
        NumberFormat::HangulSyllable => "hangulSyllable",
        NumberFormat::CircledHangulSyllable => "circledHangulSyllable",
        NumberFormat::HangulJamo => "hangulJamo",
        NumberFormat::CircledHangulJamo => "circledHangulJamo",
        NumberFormat::HangulDigit => "hangulDigit",
        NumberFormat::HanjaDigit => "hanjaDigit",
        NumberFormat::CircledHanjaDigit => "circledHanjaDigit",
        NumberFormat::HanjaGapEul => "hanjaGapEul",
        NumberFormat::HanjaGapEulHanja => "hanjaGapEulHanja",
        NumberFormat::FourSymbol => "fourSymbol",
        NumberFormat::UserChar => "userChar",
    }
}

fn footnote_numbering_name(numbering: FootnoteNumbering) -> &'static str {
    match numbering {
        FootnoteNumbering::Continue => "continue",
        FootnoteNumbering::RestartSection => "restartSection",
        FootnoteNumbering::RestartPage => "restartPage",
    }
}

fn footnote_numbering_from_str(value: &str, fallback: FootnoteNumbering) -> FootnoteNumbering {
    match value {
        "continue" | "CONTINUOUS" | "continuous" => FootnoteNumbering::Continue,
        "restartSection" | "ON_SECTION" | "RESTART_SECTION" | "onSection" => {
            FootnoteNumbering::RestartSection
        }
        "restartPage" | "ON_PAGE" | "RESTART_PAGE" | "onPage" => FootnoteNumbering::RestartPage,
        _ => fallback,
    }
}

fn footnote_placement_name(placement: FootnotePlacement) -> &'static str {
    match placement {
        FootnotePlacement::EachColumn => "documentEnd",
        FootnotePlacement::BelowText => "sectionEnd",
        FootnotePlacement::RightColumn => "rightColumn",
    }
}

fn footnote_placement_from_str(value: &str, fallback: FootnotePlacement) -> FootnotePlacement {
    match value {
        "documentEnd" | "eachColumn" => FootnotePlacement::EachColumn,
        "sectionEnd" | "belowText" => FootnotePlacement::BelowText,
        "rightColumn" => FootnotePlacement::RightColumn,
        _ => fallback,
    }
}

fn sync_section_def_control(section: &mut crate::model::document::Section) {
    let updated = section.section_def.clone();
    let Some(first_para) = section.paragraphs.first_mut() else {
        return;
    };
    if let Some(Control::SectionDef(section_def)) = first_para
        .controls
        .iter_mut()
        .find(|control| matches!(control, Control::SectionDef(_)))
    {
        **section_def = updated;
    } else {
        first_para
            .controls
            .insert(0, Control::SectionDef(Box::new(updated)));
        first_para.control_mask |= 1u32 << 0x0002;
    }
}

fn binding_method_to_u8(binding: BindingMethod) -> u8 {
    match binding {
        BindingMethod::SingleSided => 0,
        BindingMethod::DuplexSided => 1,
        BindingMethod::TopFlip => 2,
    }
}

fn binding_method_from_u8(binding: u8) -> BindingMethod {
    match binding {
        1 => BindingMethod::DuplexSided,
        2 => BindingMethod::TopFlip,
        _ => BindingMethod::SingleSided,
    }
}

fn normalize_page_def_attr(page_def: &mut PageDef) {
    page_def.attr = (page_def.attr & !0x07)
        | (if page_def.landscape { 1 } else { 0 })
        | (match page_def.binding {
            BindingMethod::SingleSided => 0u32,
            BindingMethod::DuplexSided => 1u32 << 1,
            BindingMethod::TopFlip => 2u32 << 1,
        });
}

fn normalize_section_def_flags(section_def: &mut crate::model::document::SectionDef) {
    fn set_bit(flags: &mut u32, mask: u32, val: bool) {
        if val {
            *flags |= mask;
        } else {
            *flags &= !mask;
        }
    }

    set_bit(&mut section_def.flags, 0x0001, section_def.hide_header);
    set_bit(&mut section_def.flags, 0x0002, section_def.hide_footer);
    set_bit(&mut section_def.flags, 0x0004, section_def.hide_master_page);
    set_bit(&mut section_def.flags, 0x0008, section_def.hide_border);
    set_bit(&mut section_def.flags, 0x0010, section_def.hide_fill);
    set_bit(&mut section_def.flags, 0x0020, section_def.hide_page_number);
    set_bit(
        &mut section_def.flags,
        0x0008_0000,
        section_def.hide_empty_line,
    );
    section_def.flags &= !0x0030_0000;
    section_def.flags |= ((section_def.page_num_type as u32) & 0x03) << 20;
}

fn default_template_version() -> String {
    TEMPLATE_VERSION.to_string()
}

fn default_ratio_array() -> [u8; 7] {
    [100; 7]
}

fn default_true() -> bool {
    true
}

fn default_one_u16() -> u16 {
    1
}

fn default_equation_font_size() -> u32 {
    1000
}

fn default_equation_font_name() -> String {
    "HYhwpEQ".to_string()
}

fn string_option(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn default_page_border_basis() -> String {
    "paper".to_string()
}

fn default_page_border_fill_area() -> String {
    "paper".to_string()
}

fn default_footnote_number_format() -> String {
    "digit".to_string()
}

fn default_footnote_start_number() -> u16 {
    1
}

fn default_footnote_numbering() -> String {
    "continue".to_string()
}

fn default_footnote_placement() -> String {
    "documentEnd".to_string()
}

fn default_page_width() -> u32 {
    59528
}

fn default_page_height() -> u32 {
    84188
}

fn default_page_margin_left() -> u32 {
    8504
}

fn default_page_margin_right() -> u32 {
    8504
}

fn default_page_margin_top() -> u32 {
    5669
}

fn default_page_margin_bottom() -> u32 {
    4252
}

fn default_page_margin_header() -> u32 {
    4252
}

fn default_page_margin_footer() -> u32 {
    4252
}

fn default_picture_extension() -> String {
    "png".to_string()
}

fn default_cell_padding_left() -> i16 {
    510
}

fn default_cell_padding_right() -> i16 {
    510
}

fn default_cell_padding_top() -> i16 {
    141
}

fn default_cell_padding_bottom() -> i16 {
    141
}

fn default_cell_vertical_align() -> String {
    "center".to_string()
}

fn default_cell_line_wrap() -> String {
    "BREAK".to_string()
}

fn default_table_page_break() -> String {
    "row".to_string()
}

fn default_table_outer_margin() -> i16 {
    283
}

fn default_caption_direction() -> String {
    "bottom".to_string()
}

fn default_caption_vert_align() -> String {
    "top".to_string()
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

fn is_zero_u16(value: &u16) -> bool {
    *value == 0
}

fn is_zero_u8(value: &u8) -> bool {
    *value == 0
}

fn is_zero_i8(value: &i8) -> bool {
    *value == 0
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_default_border_spacing(value: &[i16; 4]) -> bool {
    *value == [0; 4]
}

type HeaderFooterSinks<'a> = (
    usize,
    &'a mut Vec<TemplateHeaderFooter>,
    &'a mut Vec<TemplateHeaderFooter>,
);

#[derive(Debug, Clone, Default)]
struct TemplatePageHints {
    paragraph_pages: Vec<Option<u32>>,
    control_pages: Vec<Vec<Option<u32>>>,
    control_offsets: Vec<Vec<Option<TemplateControlOffsetHint>>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct TemplateControlOffsetHint {
    horizontal_offset: Option<u32>,
    vertical_offset: Option<u32>,
}

fn extract_blocks_from_paragraphs(
    core: &DocumentCore,
    paragraphs: &[Paragraph],
    blocks: &mut Vec<TemplateBlock>,
    mut header_footer_sinks: Option<HeaderFooterSinks<'_>>,
    include_formats: bool,
    page_hints: Option<&TemplatePageHints>,
    preserve_empty_paragraphs: bool,
) {
    let mut last_block_page: Option<u32> = None;
    let mut suppress_next_flow_page_break = false;
    let mut pending_empty_para_gap_before: Option<i32> = None;
    for (para_idx, para) in paragraphs.iter().enumerate() {
        let block_count_before_para = blocks.len();
        let paragraph_page = paragraph_page_hint(page_hints, para_idx);
        let mut paragraph_break = break_from_column_type(para.column_type);
        let has_page_split_line_segments = paragraph_has_page_split_line_segments(para);
        let template_control_count = para
            .controls
            .iter()
            .filter(|control| template_control_may_emit_block(control))
            .count();
        let template_table_count = para
            .controls
            .iter()
            .filter(|control| matches!(control, Control::Table(_)))
            .count();
        let paragraph_has_template_control = template_control_count > 0;
        let paragraph_host_group = (template_table_count > 1
            || can_auto_group_mixed_template_controls(
                para,
                page_hints,
                para_idx,
                paragraph_page,
                template_control_count,
                template_table_count,
                has_page_split_line_segments,
            ))
        .then(|| u32::try_from(para_idx.saturating_add(1)).ok())
        .flatten();
        if !para.text.is_empty()
            || preserve_empty_paragraphs
            || (paragraph_break.is_some() && !paragraph_has_template_control)
        {
            let break_before = take_break_for_source_page(
                blocks,
                paragraph_page,
                &mut last_block_page,
                &mut suppress_next_flow_page_break,
                paragraph_break.take(),
            );
            blocks.push(TemplateBlock::Paragraph {
                text: para.text.clone(),
                break_before,
                style: style_ref_json(core, para),
                char_format: include_formats
                    .then(|| char_format_json(core, para))
                    .flatten(),
                char_shape_runs: if include_formats {
                    template_char_shape_runs(para)
                } else {
                    Default::default()
                },
                para_format: include_formats
                    .then(|| para_format_json(core, para))
                    .flatten(),
                line_segments: template_line_segments(para),
            });
            pending_empty_para_gap_before = None;
            if has_page_split_line_segments {
                suppress_next_flow_page_break = true;
            }
        }

        for (control_idx, control) in para.controls.iter().enumerate() {
            let control_page =
                control_page_hint(page_hints, para_idx, control_idx).or(paragraph_page);
            match control {
                Control::Table(table) => {
                    let break_before = take_break_for_source_page(
                        blocks,
                        control_page,
                        &mut last_block_page,
                        &mut suppress_next_flow_page_break,
                        paragraph_break.take(),
                    );
                    let rhwp_saved_gap_before = saved_gap_before_table_after_skipped_empty_para(
                        blocks.last(),
                        table,
                        pending_empty_para_gap_before.take(),
                    );
                    blocks.push(TemplateBlock::Table {
                        rows: table_text_matrix(table),
                        break_before,
                        rhwp_saved_gap_before,
                        host_group: paragraph_host_group,
                        style: style_ref_json(core, para),
                        host_para_shape_id: para.para_shape_id,
                        para_format: include_formats
                            .then(|| para_format_json(core, para))
                            .flatten(),
                        line_segments: template_table_host_line_segments(para),
                        caption: caption_template(core, table.caption.as_ref()),
                        column_widths: table.get_column_widths(),
                        row_heights: table.get_row_heights(),
                        table_layout: table_layout_template(table),
                        object_layout: table_object_layout_template(table),
                        border_fill: border_fill_template_json(core, table.border_fill_id),
                        table_zones: table_zone_templates(core, table),
                        cell_layouts: table_cell_layout_matrix(core, table),
                        cell_formats: if include_formats {
                            table_format_matrix(core, table)
                        } else {
                            Vec::new()
                        },
                        cell_blocks: table_cell_block_matrix(core, table),
                    });
                }
                Control::Equation(equation) => {
                    let mut block = equation_block(equation);
                    let break_before = take_break_for_source_page(
                        blocks,
                        control_page,
                        &mut last_block_page,
                        &mut suppress_next_flow_page_break,
                        paragraph_break.take(),
                    );
                    set_block_break_before(&mut block, break_before);
                    set_block_host_group(&mut block, paragraph_host_group);
                    pending_empty_para_gap_before = None;
                    blocks.push(block);
                }
                Control::Picture(picture) => {
                    if let Some(mut block) = picture_block(core, picture) {
                        let break_before = take_break_for_source_page(
                            blocks,
                            control_page,
                            &mut last_block_page,
                            &mut suppress_next_flow_page_break,
                            paragraph_break.take(),
                        );
                        set_block_break_before(&mut block, break_before);
                        set_block_host_group(&mut block, paragraph_host_group);
                        if break_before == Some(TemplateBreak::Page) {
                            if let Some(offset_hint) =
                                control_offset_hint(page_hints, para_idx, control_idx)
                            {
                                set_picture_block_offset_hint(&mut block, offset_hint);
                            } else {
                                set_picture_block_page_top_offset(&mut block, para, picture);
                            }
                        } else {
                            set_picture_block_line_segments(
                                &mut block,
                                template_picture_host_line_segments(para),
                            );
                        }
                        pending_empty_para_gap_before = None;
                        blocks.push(block);
                    }
                }
                Control::Shape(shape) => {
                    if let Some(mut block) = object_placeholder_block(core, shape) {
                        let break_before = take_break_for_source_page(
                            blocks,
                            control_page,
                            &mut last_block_page,
                            &mut suppress_next_flow_page_break,
                            paragraph_break.take(),
                        );
                        set_block_break_before(&mut block, break_before);
                        set_block_host_group(&mut block, paragraph_host_group);
                        pending_empty_para_gap_before = None;
                        blocks.push(block);
                    }
                }
                Control::Header(header) => {
                    if let Some(sinks) = header_footer_sinks.as_mut() {
                        let mut blocks = Vec::new();
                        extract_blocks_from_paragraphs(
                            core,
                            &header.paragraphs,
                            &mut blocks,
                            None,
                            include_formats,
                            None,
                            preserve_empty_paragraphs,
                        );
                        sinks.1.push(TemplateHeaderFooter {
                            section: sinks.0,
                            apply_to: apply_to_u8(header.apply_to),
                            blocks,
                        });
                    }
                }
                Control::Footer(footer) => {
                    if let Some(sinks) = header_footer_sinks.as_mut() {
                        let mut blocks = Vec::new();
                        extract_blocks_from_paragraphs(
                            core,
                            &footer.paragraphs,
                            &mut blocks,
                            None,
                            include_formats,
                            None,
                            preserve_empty_paragraphs,
                        );
                        sinks.2.push(TemplateHeaderFooter {
                            section: sinks.0,
                            apply_to: apply_to_u8(footer.apply_to),
                            blocks,
                        });
                    }
                }
                _ => {}
            }
        }

        if paragraph_break.is_some()
            && para.text.is_empty()
            && !preserve_empty_paragraphs
            && blocks.len() == block_count_before_para
        {
            let break_before = take_break_for_source_page(
                blocks,
                paragraph_page,
                &mut last_block_page,
                &mut suppress_next_flow_page_break,
                paragraph_break.take(),
            );
            blocks.push(TemplateBlock::Paragraph {
                text: String::new(),
                break_before,
                style: None,
                char_format: None,
                char_shape_runs: Vec::new(),
                para_format: None,
                line_segments: Vec::new(),
            });
            pending_empty_para_gap_before = None;
        } else if para.text.is_empty()
            && !preserve_empty_paragraphs
            && !paragraph_has_template_control
            && blocks.len() == block_count_before_para
            && paragraph_break.is_none()
        {
            pending_empty_para_gap_before = skipped_empty_paragraph_gap_hu(para);
        }
    }
}

fn skipped_empty_paragraph_gap_hu(para: &Paragraph) -> Option<i32> {
    let segment = para.line_segs.first()?;
    let gap = segment.line_height + segment.line_spacing;
    (800..=4_000).contains(&gap).then_some(gap)
}

fn saved_gap_before_table_after_skipped_empty_para(
    previous_block: Option<&TemplateBlock>,
    table: &Table,
    pending_gap_hu: Option<i32>,
) -> Option<i32> {
    let gap = pending_gap_hu?;
    if !matches!(
        previous_block,
        Some(TemplateBlock::Picture {
            object_layout: Some(layout),
            ..
        }) if layout
            .get("rhwp_page_top_offset_hint")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    ) {
        return None;
    }
    if !table.common.treat_as_char
        || table_text_matrix(table)
            .iter()
            .flat_map(|row| row.iter())
            .any(|text| !text.trim().is_empty())
    {
        return None;
    }
    Some(gap)
}

fn template_control_may_emit_block(control: &Control) -> bool {
    matches!(
        control,
        Control::Table(_) | Control::Equation(_) | Control::Picture(_) | Control::Shape(_)
    )
}

fn can_auto_group_mixed_template_controls(
    para: &Paragraph,
    page_hints: Option<&TemplatePageHints>,
    para_idx: usize,
    paragraph_page: Option<u32>,
    template_control_count: usize,
    template_table_count: usize,
    has_page_split_line_segments: bool,
) -> bool {
    if template_table_count == 0
        || template_control_count <= 1
        || !para.text.trim().is_empty()
        || has_page_split_line_segments
    {
        return false;
    }
    let mut shared_page: Option<u32> = None;
    let mut seen = 0usize;
    for (control_idx, control) in para.controls.iter().enumerate() {
        if !template_control_may_emit_block(control) {
            continue;
        }
        match control {
            Control::Table(_) => {}
            Control::Picture(picture)
                if picture.common.treat_as_char
                    && picture.common.height.max(picture.shape_attr.current_height)
                        <= MAX_PICTURE_HOST_LINE_SEGMENT_HEIGHT as u32 => {}
            _ => return false,
        }
        let Some(page) = control_page_hint(page_hints, para_idx, control_idx).or(paragraph_page)
        else {
            return false;
        };
        if shared_page.is_some_and(|shared_page| shared_page != page) {
            return false;
        }
        shared_page = Some(page);
        seen += 1;
    }
    seen == template_control_count && shared_page.is_some()
}

fn template_page_hints(core: &DocumentCore, section_idx: usize) -> TemplatePageHints {
    let para_count = core
        .document
        .sections
        .get(section_idx)
        .map(|section| section.paragraphs.len())
        .unwrap_or(0);
    let control_pages = core
        .document
        .sections
        .get(section_idx)
        .map(|section| {
            section
                .paragraphs
                .iter()
                .map(|paragraph| vec![None; paragraph.controls.len()])
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let control_offsets = control_pages
        .iter()
        .map(|controls| vec![None; controls.len()])
        .collect::<Vec<_>>();
    let mut hints = TemplatePageHints {
        paragraph_pages: vec![None; para_count],
        control_pages,
        control_offsets,
    };
    let Some(pagination) = core.pagination.get(section_idx) else {
        return hints;
    };
    for (page_idx, page) in pagination.pages.iter().enumerate() {
        for column in &page.column_contents {
            for item in &column.items {
                note_page_hint(&mut hints, item, page_idx as u32);
            }
        }
    }
    note_page_break_picture_offset_hints(core, section_idx, &mut hints);
    hints
}

fn note_page_break_picture_offset_hints(
    core: &DocumentCore,
    section_idx: usize,
    hints: &mut TemplatePageHints,
) {
    let Some(section) = core.document.sections.get(section_idx) else {
        return;
    };
    for (para_idx, para) in section.paragraphs.iter().enumerate() {
        for (control_idx, control) in para.controls.iter().enumerate() {
            let Control::Picture(picture) = control else {
                continue;
            };
            if !picture.common.treat_as_char {
                continue;
            }
            if !page_break_picture_offset_hint_candidate(para, control_idx, picture) {
                continue;
            }
            let Some(control_page) = hints
                .control_pages
                .get(para_idx)
                .and_then(|controls| controls.get(control_idx))
                .copied()
                .flatten()
            else {
                continue;
            };
            let Some(paragraph_page) = hints.paragraph_pages.get(para_idx).copied().flatten()
            else {
                continue;
            };
            if control_page <= paragraph_page {
                continue;
            }
            let Some(page_content) = core
                .pagination
                .get(section_idx)
                .and_then(|pagination| pagination.pages.get(control_page as usize))
            else {
                continue;
            };
            let Ok(tree) = core.build_page_tree_cached(control_page) else {
                continue;
            };
            let Some(bbox) = find_picture_bbox(&tree.root, section_idx, para_idx, control_idx)
            else {
                continue;
            };
            let body = &page_content.layout.body_area;
            let horizontal_offset =
                offset_px_to_hwpunit((bbox.x - body.x).max(0.0), core.dpi, 10_000);
            let vertical_offset =
                offset_px_to_hwpunit((bbox.y - body.y).max(0.0), core.dpi, 10_000);
            if horizontal_offset.is_none() && vertical_offset.is_none() {
                continue;
            }
            if let Some(slot) = hints
                .control_offsets
                .get_mut(para_idx)
                .and_then(|controls| controls.get_mut(control_idx))
            {
                *slot = Some(TemplateControlOffsetHint {
                    horizontal_offset,
                    vertical_offset,
                });
            }
        }
    }
}

fn find_picture_bbox(
    node: &crate::renderer::render_tree::RenderNode,
    section_idx: usize,
    para_idx: usize,
    control_idx: usize,
) -> Option<crate::renderer::render_tree::BoundingBox> {
    if let crate::renderer::render_tree::RenderNodeType::Image(image) = &node.node_type {
        if image.section_index == Some(section_idx)
            && image.para_index == Some(para_idx)
            && image.control_index == Some(control_idx)
        {
            return Some(node.bbox.clone());
        }
    }
    node.children
        .iter()
        .find_map(|child| find_picture_bbox(child, section_idx, para_idx, control_idx))
}

fn page_break_picture_offset_hint_candidate(
    para: &Paragraph,
    control_idx: usize,
    picture: &Picture,
) -> bool {
    if control_idx == 0
        || picture.common.horizontal_offset != 0
        || picture.common.vertical_offset != 0
    {
        return false;
    }
    let picture_height = picture.common.height.max(picture.shape_attr.current_height) as i32;
    if picture_height <= MAX_PICTURE_HOST_LINE_SEGMENT_HEIGHT {
        return false;
    }
    let picture_width = picture.common.width.max(picture.shape_attr.current_width);
    if picture_width > 45_000 {
        return false;
    }
    let Some(seg) = para.line_segs.first() else {
        return false;
    };
    seg.vertical_pos >= 30_000
        && seg.line_height > 0
        && seg.line_height <= MAX_PICTURE_HOST_LINE_SEGMENT_HEIGHT
}

fn offset_px_to_hwpunit(px: f64, dpi: f64, max_hu: u32) -> Option<u32> {
    if px <= 0.5 || dpi <= 0.0 {
        return None;
    }
    let hwpunit = (px / dpi * 7200.0).round();
    (hwpunit > 0.0 && hwpunit <= max_hu as f64).then_some(hwpunit as u32)
}

fn note_page_hint(
    hints: &mut TemplatePageHints,
    item: &crate::renderer::pagination::PageItem,
    page_idx: u32,
) {
    use crate::renderer::pagination::PageItem;
    match item {
        PageItem::FullParagraph { para_index } | PageItem::PartialParagraph { para_index, .. } => {
            note_paragraph_page(hints, *para_index, page_idx);
        }
        PageItem::Table {
            para_index,
            control_index,
        }
        | PageItem::PartialTable {
            para_index,
            control_index,
            ..
        }
        | PageItem::Shape {
            para_index,
            control_index,
        } => {
            note_paragraph_page(hints, *para_index, page_idx);
            note_control_page(hints, *para_index, *control_index, page_idx);
        }
        PageItem::EndnoteSeparator { .. } => {}
    }
}

fn note_paragraph_page(hints: &mut TemplatePageHints, para_idx: usize, page_idx: u32) {
    if let Some(slot) = hints.paragraph_pages.get_mut(para_idx) {
        if slot.is_none() {
            *slot = Some(page_idx);
        }
    }
}

fn note_control_page(
    hints: &mut TemplatePageHints,
    para_idx: usize,
    control_idx: usize,
    page_idx: u32,
) {
    if let Some(slot) = hints
        .control_pages
        .get_mut(para_idx)
        .and_then(|controls| controls.get_mut(control_idx))
    {
        if slot.is_none() {
            *slot = Some(page_idx);
        }
    }
}

fn paragraph_page_hint(page_hints: Option<&TemplatePageHints>, para_idx: usize) -> Option<u32> {
    page_hints
        .and_then(|hints| hints.paragraph_pages.get(para_idx))
        .copied()
        .flatten()
}

fn control_page_hint(
    page_hints: Option<&TemplatePageHints>,
    para_idx: usize,
    control_idx: usize,
) -> Option<u32> {
    page_hints
        .and_then(|hints| hints.control_pages.get(para_idx))
        .and_then(|controls| controls.get(control_idx))
        .copied()
        .flatten()
}

fn control_offset_hint(
    page_hints: Option<&TemplatePageHints>,
    para_idx: usize,
    control_idx: usize,
) -> Option<TemplateControlOffsetHint> {
    page_hints
        .and_then(|hints| hints.control_offsets.get(para_idx))
        .and_then(|controls| controls.get(control_idx))
        .copied()
        .flatten()
}

fn take_break_for_source_page(
    blocks: &mut Vec<TemplateBlock>,
    source_page: Option<u32>,
    last_block_page: &mut Option<u32>,
    suppress_next_flow_page_break: &mut bool,
    fallback_break: Option<TemplateBreak>,
) -> Option<TemplateBreak> {
    let mut flow_page_breaks =
        flow_page_break_count(blocks.is_empty(), source_page, *last_block_page);
    if *suppress_next_flow_page_break && flow_page_breaks > 0 {
        // A paragraph that already contains a page-reset line segment can make
        // the following source page hint look like a new hard break.
        flow_page_breaks = flow_page_breaks.saturating_sub(1);
        *suppress_next_flow_page_break = false;
    } else if *suppress_next_flow_page_break && fallback_break.is_some() {
        *suppress_next_flow_page_break = false;
    }
    let mut pending_blank_page_breaks = flow_page_breaks.saturating_sub(1);
    insert_blank_flow_page_breaks(blocks, &mut pending_blank_page_breaks);
    let break_before = if flow_page_breaks > 0 {
        Some(TemplateBreak::Page)
    } else if blocks.is_empty() {
        None
    } else {
        fallback_break
    };
    *last_block_page = source_page.or(*last_block_page);
    break_before
}

fn flow_page_break_count(
    is_first_block: bool,
    source_page: Option<u32>,
    last_block_page: Option<u32>,
) -> u32 {
    if is_first_block {
        return 0;
    }
    match (last_block_page, source_page) {
        (Some(previous), Some(current)) if current > previous => current - previous,
        _ => 0,
    }
}

fn insert_blank_flow_page_breaks(blocks: &mut Vec<TemplateBlock>, count: &mut u32) {
    while *count > 0 {
        blocks.push(TemplateBlock::Paragraph {
            text: String::new(),
            break_before: Some(TemplateBreak::Page),
            style: None,
            char_format: None,
            char_shape_runs: Vec::new(),
            para_format: None,
            line_segments: Vec::new(),
        });
        *count -= 1;
    }
}

fn set_block_break_before(block: &mut TemplateBlock, break_before: Option<TemplateBreak>) {
    if break_before.is_none() {
        return;
    }
    match block {
        TemplateBlock::Paragraph {
            break_before: slot, ..
        }
        | TemplateBlock::Table {
            break_before: slot, ..
        }
        | TemplateBlock::Equation {
            break_before: slot, ..
        }
        | TemplateBlock::Picture {
            break_before: slot, ..
        }
        | TemplateBlock::ObjectPlaceholder {
            break_before: slot, ..
        } => {
            *slot = break_before;
        }
    }
}

fn set_block_host_group(block: &mut TemplateBlock, host_group: Option<u32>) {
    if host_group.is_none() {
        return;
    }
    match block {
        TemplateBlock::Table {
            host_group: slot, ..
        }
        | TemplateBlock::Equation {
            host_group: slot, ..
        }
        | TemplateBlock::Picture {
            host_group: slot, ..
        }
        | TemplateBlock::ObjectPlaceholder {
            host_group: slot, ..
        } => {
            *slot = host_group;
        }
        TemplateBlock::Paragraph { .. } => {}
    }
}

fn set_picture_block_line_segments(block: &mut TemplateBlock, line_segments: Vec<TemplateLineSeg>) {
    if line_segments.is_empty() {
        return;
    }
    if let TemplateBlock::Picture {
        line_segments: slot,
        ..
    } = block
    {
        *slot = line_segments;
    }
}

fn set_picture_block_offset_hint(block: &mut TemplateBlock, hint: TemplateControlOffsetHint) {
    if hint.horizontal_offset.is_none() && hint.vertical_offset.is_none() {
        return;
    }
    if let TemplateBlock::Picture { object_layout, .. } = block {
        let mut layout = object_layout
            .as_ref()
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(offset) = hint.horizontal_offset {
            layout.insert("horizontal_offset".to_string(), json!(offset));
        }
        if let Some(offset) = hint.vertical_offset {
            layout.insert("vertical_offset".to_string(), json!(offset));
        }
        layout.insert("rhwp_page_top_offset_hint".to_string(), json!(true));
        *object_layout = Some(Value::Object(layout));
    }
}

fn set_picture_block_page_top_offset(
    block: &mut TemplateBlock,
    para: &Paragraph,
    picture: &Picture,
) {
    let Some(offset) = template_picture_page_top_offset(para, picture) else {
        return;
    };
    if let TemplateBlock::Picture { object_layout, .. } = block {
        let mut layout = object_layout
            .as_ref()
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        layout.insert("vertical_offset".to_string(), json!(offset));
        layout.insert("rhwp_page_top_offset_hint".to_string(), json!(true));
        *object_layout = Some(Value::Object(layout));
    }
}

fn template_picture_page_top_offset(para: &Paragraph, picture: &Picture) -> Option<u32> {
    if !picture.common.treat_as_char || picture.common.vertical_offset != 0 {
        return None;
    }
    let seg = para.line_segs.first()?;
    if seg.vertical_pos <= 0 || seg.line_height <= MAX_PICTURE_HOST_LINE_SEGMENT_HEIGHT {
        return None;
    }
    let picture_height = picture.common.height.max(picture.shape_attr.current_height) as i32;
    if picture_height <= MAX_PICTURE_HOST_LINE_SEGMENT_HEIGHT {
        return None;
    }
    let max_page_top_hint = 20_000;
    (seg.vertical_pos <= max_page_top_hint).then_some(seg.vertical_pos as u32)
}

fn accumulate_block_stats(blocks: &[TemplateBlock], stats: &mut TemplateStats) {
    for block in blocks {
        stats.block_count += 1;
        match block {
            TemplateBlock::Paragraph { .. } => stats.paragraph_count += 1,
            TemplateBlock::Table {
                rows, cell_blocks, ..
            } => {
                stats.table_count += 1;
                stats.table_cell_count += rows.iter().map(Vec::len).sum::<usize>();
                for cell_blocks in cell_blocks.iter().flat_map(|row| row.iter()) {
                    accumulate_block_stats(cell_blocks, stats);
                }
            }
            TemplateBlock::Equation { .. } => stats.equation_count += 1,
            TemplateBlock::Picture { .. } => stats.picture_count += 1,
            TemplateBlock::ObjectPlaceholder { children, .. } => {
                stats.object_placeholder_count += 1;
                accumulate_block_stats(children, stats);
            }
        }
    }
}

fn break_from_column_type(column_type: ColumnBreakType) -> Option<TemplateBreak> {
    match column_type {
        ColumnBreakType::Page | ColumnBreakType::Section => Some(TemplateBreak::Page),
        ColumnBreakType::Column | ColumnBreakType::MultiColumn => Some(TemplateBreak::Column),
        ColumnBreakType::None => None,
    }
}

fn table_text_matrix(table: &Table) -> Vec<Vec<String>> {
    let row_count = table.row_count.max(1) as usize;
    let col_count = table.col_count.max(1) as usize;
    let mut rows = vec![vec![String::new(); col_count]; row_count];
    for cell in &table.cells {
        let row = cell.row as usize;
        let col = cell.col as usize;
        if row < row_count && col < col_count {
            rows[row][col] = paragraph_text(&cell.paragraphs);
        }
    }
    rows
}

fn caption_template(core: &DocumentCore, caption: Option<&Caption>) -> Option<TemplateCaption> {
    let caption = caption?;
    let mut blocks = Vec::new();
    extract_blocks_from_paragraphs(
        core,
        &caption.paragraphs,
        &mut blocks,
        None,
        true,
        None,
        false,
    );
    Some(TemplateCaption {
        direction: caption_direction_name(caption.direction).to_string(),
        vert_align: caption_vert_align_name(caption.vert_align).to_string(),
        width: caption.width,
        spacing: caption.spacing,
        max_width: caption.max_width,
        include_margin: caption.include_margin,
        blocks,
    })
}

fn template_caption(
    core: &mut DocumentCore,
    section_idx: usize,
    caption: Option<&TemplateCaption>,
) -> Result<Option<Caption>, String> {
    let Some(caption) = caption else {
        return Ok(None);
    };
    let paragraphs = header_footer_paragraphs_from_blocks(core, section_idx, &caption.blocks)?;
    Ok(Some(Caption {
        direction: caption_direction_from_name(&caption.direction),
        vert_align: caption_vert_align_from_name(&caption.vert_align),
        width: caption.width,
        spacing: caption.spacing,
        max_width: caption.max_width,
        include_margin: caption.include_margin,
        paragraphs,
    }))
}

fn shape_caption(shape: &ShapeObject) -> &Option<Caption> {
    match shape {
        ShapeObject::Line(shape) => &shape.drawing.caption,
        ShapeObject::Rectangle(shape) => &shape.drawing.caption,
        ShapeObject::Ellipse(shape) => &shape.drawing.caption,
        ShapeObject::Arc(shape) => &shape.drawing.caption,
        ShapeObject::Polygon(shape) => &shape.drawing.caption,
        ShapeObject::Curve(shape) => &shape.drawing.caption,
        ShapeObject::Group(shape) => &shape.caption,
        ShapeObject::Picture(shape) => &shape.caption,
        ShapeObject::Chart(shape) => &shape.caption,
        ShapeObject::Ole(shape) => &shape.caption,
    }
}

fn apply_shape_caption(shape: &mut ShapeObject, caption: Option<Caption>) {
    match shape {
        ShapeObject::Line(shape) => shape.drawing.caption = caption,
        ShapeObject::Rectangle(shape) => shape.drawing.caption = caption,
        ShapeObject::Ellipse(shape) => shape.drawing.caption = caption,
        ShapeObject::Arc(shape) => shape.drawing.caption = caption,
        ShapeObject::Polygon(shape) => shape.drawing.caption = caption,
        ShapeObject::Curve(shape) => shape.drawing.caption = caption,
        ShapeObject::Group(shape) => shape.caption = caption,
        ShapeObject::Picture(shape) => shape.caption = caption,
        ShapeObject::Chart(shape) => shape.caption = caption,
        ShapeObject::Ole(shape) => shape.caption = caption,
    }
}

fn template_line_segments(para: &Paragraph) -> Vec<TemplateLineSeg> {
    if paragraph_has_page_split_line_segments(para) {
        return Vec::new();
    }
    template_line_segments_raw(para)
}

fn template_table_host_line_segments(para: &Paragraph) -> Vec<TemplateLineSeg> {
    let table_control_count = para
        .controls
        .iter()
        .filter(|control| matches!(control, Control::Table(_)))
        .count();
    if table_control_count > 1 {
        template_line_segments_raw(para)
    } else {
        template_line_segments(para)
    }
}

fn template_picture_host_line_segments(para: &Paragraph) -> Vec<TemplateLineSeg> {
    let line_segments = template_line_segments(para);
    let is_large_picture_line = line_segments
        .iter()
        .any(|seg| seg.line_height > MAX_PICTURE_HOST_LINE_SEGMENT_HEIGHT);
    if is_large_picture_line {
        Vec::new()
    } else {
        line_segments
    }
}

fn template_line_segments_raw(para: &Paragraph) -> Vec<TemplateLineSeg> {
    para.line_segs
        .iter()
        .map(|seg| TemplateLineSeg {
            text_start: seg.text_start,
            vertical_pos: seg.vertical_pos,
            line_height: seg.line_height,
            text_height: seg.text_height,
            baseline_distance: seg.baseline_distance,
            line_spacing: seg.line_spacing,
            column_start: seg.column_start,
            segment_width: seg.segment_width,
            tag: seg.tag,
        })
        .collect()
}

fn paragraph_has_page_split_line_segments(para: &Paragraph) -> bool {
    line_segments_reset_vertical_position(&para.line_segs)
}

fn line_segments_reset_vertical_position(line_segments: &[LineSeg]) -> bool {
    line_segments
        .windows(2)
        .any(|pair| pair[1].vertical_pos < pair[0].vertical_pos)
}

fn line_segments_from_template(line_segments: &[TemplateLineSeg]) -> Vec<LineSeg> {
    line_segments
        .iter()
        .map(|seg| LineSeg {
            text_start: seg.text_start,
            vertical_pos: seg.vertical_pos,
            line_height: seg.line_height,
            text_height: seg.text_height,
            baseline_distance: seg.baseline_distance,
            line_spacing: seg.line_spacing,
            column_start: seg.column_start,
            segment_width: seg.segment_width,
            tag: seg.tag,
        })
        .collect()
}

fn apply_template_line_segments(para: &mut Paragraph, line_segments: &[TemplateLineSeg]) {
    if !line_segments.is_empty() {
        para.line_segs = line_segments_from_template(line_segments);
    }
}

fn template_char_shape_runs(para: &Paragraph) -> Vec<TemplateCharShapeRun> {
    if para.char_shapes.is_empty() {
        return Vec::new();
    }
    let text_utf16_end = paragraph_text_utf16_len(para);
    para.char_shapes
        .iter()
        .filter(|run| run.start_pos <= text_utf16_end)
        .map(|run| TemplateCharShapeRun {
            start_pos: run.start_pos,
            char_shape_id: run.char_shape_id,
        })
        .collect()
}

fn paragraph_text_utf16_len(para: &Paragraph) -> u32 {
    para.text.chars().map(|ch| ch.len_utf16() as u32).sum()
}

fn apply_template_char_shape_runs(
    core: &DocumentCore,
    para: &mut Paragraph,
    runs: &[TemplateCharShapeRun],
) {
    apply_template_char_shape_runs_with_count(core.document.doc_info.char_shapes.len(), para, runs);
}

fn apply_template_char_shape_runs_with_count(
    char_shape_count: usize,
    para: &mut Paragraph,
    runs: &[TemplateCharShapeRun],
) {
    if runs.is_empty() || char_shape_count == 0 {
        return;
    }

    let text_utf16_end = paragraph_text_utf16_len(para);
    let mut sorted_runs: Vec<TemplateCharShapeRun> = runs
        .iter()
        .copied()
        .filter(|run| {
            run.start_pos <= text_utf16_end && (run.char_shape_id as usize) < char_shape_count
        })
        .collect();
    sorted_runs.sort_by_key(|run| run.start_pos);

    let fallback = para.char_shapes.first().cloned().unwrap_or(CharShapeRef {
        start_pos: 0,
        char_shape_id: 0,
    });
    let mut restored = Vec::with_capacity(sorted_runs.len().saturating_add(1));
    if sorted_runs.first().map_or(true, |run| run.start_pos != 0) {
        restored.push(CharShapeRef {
            start_pos: 0,
            char_shape_id: fallback.char_shape_id,
        });
    }

    for run in sorted_runs {
        if let Some(last) = restored
            .last_mut()
            .filter(|last| last.start_pos == run.start_pos)
        {
            last.char_shape_id = run.char_shape_id;
        } else {
            restored.push(CharShapeRef {
                start_pos: run.start_pos,
                char_shape_id: run.char_shape_id,
            });
        }
    }

    if !restored.is_empty() {
        para.char_shapes = restored;
    }
}

fn table_format_matrix(core: &DocumentCore, table: &Table) -> Vec<Vec<TemplateTextFormat>> {
    let row_count = table.row_count.max(1) as usize;
    let col_count = table.col_count.max(1) as usize;
    let mut formats = vec![vec![TemplateTextFormat::default(); col_count]; row_count];
    for cell in &table.cells {
        let row = cell.row as usize;
        let col = cell.col as usize;
        if row < row_count && col < col_count {
            if let Some(para) = cell.paragraphs.first() {
                formats[row][col] = TemplateTextFormat {
                    style: style_ref_json(core, para),
                    char_format: char_format_json(core, para),
                    char_shape_runs: template_char_shape_runs(para),
                    para_format: para_format_json(core, para),
                };
            }
        }
    }
    if is_empty_format_matrix(&formats) {
        Vec::new()
    } else {
        formats
    }
}

fn table_layout_template(table: &Table) -> Option<TemplateTableLayout> {
    let layout = TemplateTableLayout {
        cell_spacing: table.cell_spacing,
        padding_left: table.padding.left,
        padding_right: table.padding.right,
        padding_top: table.padding.top,
        padding_bottom: table.padding.bottom,
        page_break: table_page_break_name(table.page_break).to_string(),
        repeat_header: table.repeat_header,
        outer_margin_left: table.outer_margin_left,
        outer_margin_right: table.outer_margin_right,
        outer_margin_top: table.outer_margin_top,
        outer_margin_bottom: table.outer_margin_bottom,
    };
    (layout != TemplateTableLayout::default()).then_some(layout)
}

fn table_object_layout_template(table: &Table) -> Option<Value> {
    common_object_layout_template(&table.common)
}

fn normalize_zero_vpos_heading_table_page_breaks(blocks: &mut [TemplateBlock]) {
    if blocks.len() < 2 {
        return;
    }
    for idx in 0..blocks.len() - 1 {
        let (left, right) = blocks.split_at_mut(idx + 1);
        let current = &mut left[idx];
        let next = &mut right[0];
        if !is_zero_vpos_tac_heading_table(current) || !is_following_large_tac_page_break(next) {
            continue;
        }
        if let Some(current_break) = block_break_before_mut(current) {
            *current_break = Some(TemplateBreak::Page);
        }
        if let Some(next_break) = block_break_before_mut(next) {
            *next_break = None;
        }
    }
}

fn is_zero_vpos_tac_heading_table(block: &TemplateBlock) -> bool {
    let TemplateBlock::Table {
        rows,
        break_before,
        line_segments,
        row_heights,
        object_layout,
        ..
    } = block
    else {
        return false;
    };
    break_before.is_none()
        && template_object_treats_as_char(object_layout)
        && line_segments
            .first()
            .is_some_and(|segment| segment.vertical_pos == 0)
        && row_heights.len() == 1
        && row_heights[0] <= 4_000
        && rows
            .iter()
            .flat_map(|row| row.iter())
            .map(|text| text.chars().count())
            .sum::<usize>()
            <= 120
}

fn is_following_large_tac_page_break(block: &TemplateBlock) -> bool {
    let TemplateBlock::Table {
        break_before,
        row_heights,
        object_layout,
        ..
    } = block
    else {
        return false;
    };
    *break_before == Some(TemplateBreak::Page)
        && template_object_treats_as_char(object_layout)
        && row_heights.iter().any(|height| *height >= 20_000)
}

fn block_break_before_mut(block: &mut TemplateBlock) -> Option<&mut Option<TemplateBreak>> {
    match block {
        TemplateBlock::Paragraph { break_before, .. }
        | TemplateBlock::Table { break_before, .. }
        | TemplateBlock::Equation { break_before, .. }
        | TemplateBlock::Picture { break_before, .. }
        | TemplateBlock::ObjectPlaceholder { break_before, .. } => Some(break_before),
    }
}

fn template_object_treats_as_char(object_layout: &Option<Value>) -> bool {
    object_layout
        .as_ref()
        .and_then(|layout| layout.get("treat_as_char"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn common_object_layout_template(common: &CommonObjAttr) -> Option<Value> {
    Some(json!({
        "attr": common.attr,
        "width": common.width,
        "height": common.height,
        "z_order": common.z_order,
        "instance_id": common.instance_id,
        "inst_id": common.inst_id,
        "treat_as_char": common.treat_as_char,
        "flow_with_text": common.flow_with_text,
        "allow_overlap": common.allow_overlap,
        "size_protect": common.size_protect,
        "lock": common.lock,
        "prevent_page_break": common.prevent_page_break,
        "vert_rel_to": vert_rel_to_name(common.vert_rel_to),
        "vert_align": obj_vert_align_name(common.vert_align),
        "horz_rel_to": horz_rel_to_name(common.horz_rel_to),
        "horz_align": obj_horz_align_name(common.horz_align),
        "text_wrap": text_wrap_name(common.text_wrap),
        "text_flow": text_flow_name(common.text_flow),
        "width_criterion": size_criterion_name(common.width_criterion),
        "height_criterion": size_criterion_name(common.height_criterion),
        "horizontal_offset": common.horizontal_offset,
        "vertical_offset": common.vertical_offset,
        "description": common.description,
        "dropcap_style": common.dropcap_style,
        "href": common.href,
        "numbering_type": object_numbering_type_name(common.numbering_type),
        "numbering_type_explicit": common.numbering_type_explicit,
    }))
}

fn table_cell_layout_matrix(core: &DocumentCore, table: &Table) -> Vec<Vec<TemplateCellLayout>> {
    let row_count = table.row_count.max(1) as usize;
    let col_count = table.col_count.max(1) as usize;
    let mut layouts = vec![vec![TemplateCellLayout::default(); col_count]; row_count];
    for cell in &table.cells {
        let row = cell.row as usize;
        let col = cell.col as usize;
        if row < row_count && col < col_count {
            layouts[row][col] = TemplateCellLayout {
                col_span: cell.col_span.max(1),
                row_span: cell.row_span.max(1),
                padding_left: cell.padding.left,
                padding_right: cell.padding.right,
                padding_top: cell.padding.top,
                padding_bottom: cell.padding.bottom,
                apply_inner_margin: cell.apply_inner_margin,
                vertical_align: vertical_align_name(cell.vertical_align).to_string(),
                text_direction: cell.text_direction,
                is_header: cell.is_header,
                cell_protect: cell.cell_protect(),
                editable_in_form: cell.editable_in_form(),
                dirty: cell.dirty,
                line_wrap: if cell.sub_list_line_wrap.is_empty() {
                    default_cell_line_wrap()
                } else {
                    cell.sub_list_line_wrap.clone()
                },
                link_list_id_ref: cell.sub_list_link_list_id_ref,
                link_list_next_id_ref: cell.sub_list_link_list_next_id_ref,
                text_width: cell.sub_list_text_width,
                text_height: cell.sub_list_text_height,
                has_text_ref: cell.sub_list_text_ref,
                has_num_ref: cell.sub_list_num_ref,
                field_name: cell.field_name.clone().unwrap_or_default(),
                border_fill: (cell.border_fill_id != table.border_fill_id)
                    .then(|| border_fill_template_json(core, cell.border_fill_id))
                    .flatten(),
            };
        }
    }
    if is_empty_cell_layout_matrix(&layouts) {
        Vec::new()
    } else {
        layouts
    }
}

fn table_zone_templates(core: &DocumentCore, table: &Table) -> Vec<TemplateTableZone> {
    table
        .zones
        .iter()
        .map(|zone| TemplateTableZone {
            start_row: zone.start_row,
            start_col: zone.start_col,
            end_row: zone.end_row,
            end_col: zone.end_col,
            border_fill: border_fill_template_json(core, zone.border_fill_id),
        })
        .collect()
}

fn border_fill_template_json(core: &DocumentCore, border_fill_id: u16) -> Option<Value> {
    let border_fill = border_fill_by_id(core, border_fill_id)?;
    let raw_hwp_border_fill_base64 = border_fill
        .raw_data
        .as_ref()
        .filter(|raw| !raw.is_empty())
        .map(|raw| base64::engine::general_purpose::STANDARD.encode(raw))
        .unwrap_or_default();
    let mut value = json!({
        "attr": border_fill.attr,
        "three_d": border_fill.three_d || (border_fill.attr & 0x0001) != 0,
        "shadow": border_fill.shadow || (border_fill.attr & 0x0002) != 0,
        "center_line": border_fill.center_line.as_deref().unwrap_or("NONE"),
        "break_cell_separate_line": border_fill.break_cell_separate_line,
        "borders": {
            "left": border_line_json(border_fill.borders[0]),
            "right": border_line_json(border_fill.borders[1]),
            "top": border_line_json(border_fill.borders[2]),
            "bottom": border_line_json(border_fill.borders[3]),
        },
        "diagonal": {
            "diagonal_type": border_fill.diagonal.diagonal_type,
            "width": border_fill.diagonal.width,
            "color": border_fill.diagonal.color,
            "color_hex": color_ref_to_hex(border_fill.diagonal.color),
        },
        "fill": fill_json(core, &border_fill.fill),
    });
    if !raw_hwp_border_fill_base64.is_empty() {
        value["raw_hwp_border_fill_base64"] = json!(raw_hwp_border_fill_base64);
    }
    Some(value)
}

fn border_fill_by_id(core: &DocumentCore, border_fill_id: u16) -> Option<&BorderFill> {
    if border_fill_id == 0 {
        return None;
    }
    core.document
        .doc_info
        .border_fills
        .get(border_fill_id as usize - 1)
}

fn border_line_json(line: BorderLine) -> Value {
    json!({
        "type": border_line_type_name(line.line_type),
        "width": line.width,
        "color": line.color,
        "color_hex": color_ref_to_hex(line.color),
    })
}

fn table_cell_block_matrix(core: &DocumentCore, table: &Table) -> Vec<Vec<Vec<TemplateBlock>>> {
    let row_count = table.row_count.max(1) as usize;
    let col_count = table.col_count.max(1) as usize;
    let mut matrix = vec![vec![Vec::new(); col_count]; row_count];
    for cell in &table.cells {
        let row = cell.row as usize;
        let col = cell.col as usize;
        if row < row_count && col < col_count {
            let mut blocks = Vec::new();
            extract_blocks_from_paragraphs(
                core,
                &cell.paragraphs,
                &mut blocks,
                None,
                true,
                None,
                true,
            );
            if should_preserve_cell_blocks(&blocks) {
                matrix[row][col] = blocks;
            }
        }
    }
    if is_empty_block_matrix(&matrix) {
        Vec::new()
    } else {
        matrix
    }
}

fn should_preserve_cell_blocks(blocks: &[TemplateBlock]) -> bool {
    if blocks.is_empty() {
        return false;
    }
    blocks.len() > 1
        || blocks.iter().any(block_contains_table)
        || blocks.iter().any(block_has_line_segments)
}

fn block_contains_table(block: &TemplateBlock) -> bool {
    matches!(
        block,
        TemplateBlock::Table { .. }
            | TemplateBlock::Equation { .. }
            | TemplateBlock::Picture { .. }
            | TemplateBlock::ObjectPlaceholder { .. }
    )
}

fn block_has_line_segments(block: &TemplateBlock) -> bool {
    match block {
        TemplateBlock::Paragraph { line_segments, .. }
        | TemplateBlock::Picture { line_segments, .. } => !line_segments.is_empty(),
        _ => false,
    }
}

fn template_saved_tac_gap_before(previous: &TemplateBlock, current: &TemplateBlock) -> Option<i32> {
    let TemplateBlock::Table {
        rows,
        line_segments: previous_segments,
        object_layout,
        ..
    } = previous
    else {
        return None;
    };
    if !template_object_treats_as_char(object_layout)
        || rows
            .iter()
            .flat_map(|row| row.iter())
            .any(|text| !text.trim().is_empty())
    {
        return None;
    }
    let TemplateBlock::Paragraph {
        text,
        line_segments: current_segments,
        ..
    } = current
    else {
        return None;
    };
    if text.trim().is_empty() {
        return None;
    }
    let previous_segment = previous_segments
        .iter()
        .rev()
        .find(|seg| seg.segment_width > 0)
        .or_else(|| previous_segments.last())?;
    let current_first_vpos = current_segments.first()?.vertical_pos;
    let previous_end_vpos = previous_segment.vertical_pos
        + (previous_segment.line_height + previous_segment.line_spacing).max(0);
    let saved_gap_hu = current_first_vpos - previous_end_vpos;
    (1_500..=12_000)
        .contains(&saved_gap_hu)
        .then_some(saved_gap_hu)
}

fn template_saved_para_gap_before(
    previous: &TemplateBlock,
    current: &TemplateBlock,
) -> Option<i32> {
    let TemplateBlock::Paragraph {
        text: previous_text,
        break_before: previous_break,
        line_segments: previous_segments,
        ..
    } = previous
    else {
        return None;
    };
    let TemplateBlock::Paragraph {
        text: current_text,
        break_before: current_break,
        line_segments: current_segments,
        ..
    } = current
    else {
        return None;
    };
    if previous_break.is_some()
        || current_break.is_some()
        || previous_text.trim().is_empty()
        || current_text.trim().is_empty()
        || previous_segments.len() != 1
        || current_segments.len() != 1
    {
        return None;
    }
    let previous_segment = previous_segments.first()?;
    let current_segment = current_segments.first()?;
    if previous_segment.segment_width <= 0
        || current_segment.segment_width <= 0
        || previous_segment.segment_width != current_segment.segment_width
    {
        return None;
    }
    let previous_end_vpos = previous_segment.vertical_pos
        + (previous_segment.line_height + previous_segment.line_spacing).max(0);
    let saved_gap_hu = current_segment.vertical_pos - previous_end_vpos;
    (1_200..=6_000)
        .contains(&saved_gap_hu)
        .then_some(saved_gap_hu)
}

fn paragraph_text(paragraphs: &[Paragraph]) -> String {
    paragraphs
        .iter()
        .map(|paragraph| paragraph.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn append_body_block(
    core: &mut DocumentCore,
    section_idx: usize,
    block: &TemplateBlock,
    force_break: Option<TemplateBreak>,
    saved_tac_gap_before: Option<i32>,
    saved_para_gap_before: Option<i32>,
    state: &mut BodyBuildState,
) -> Result<(), String> {
    match block {
        TemplateBlock::Paragraph {
            text,
            break_before,
            style,
            char_format,
            char_shape_runs,
            para_format,
            line_segments,
        } => {
            let para = prepare_body_paragraph(
                core,
                section_idx,
                &mut state.used_first_body_para,
                force_break.or(*break_before),
            )?;
            set_paragraph_text(core, section_idx, para, text)?;
            apply_body_paragraph_style(core, section_idx, para, style.as_ref());
            apply_body_formats(
                core,
                section_idx,
                para,
                text,
                char_format.as_ref(),
                para_format.as_ref(),
            )?;
            apply_body_char_shape_runs(core, section_idx, para, char_shape_runs);
            apply_body_line_segments(core, section_idx, para, line_segments);
            apply_body_saved_tac_gap_before(core, section_idx, para, saved_tac_gap_before);
            apply_body_saved_para_gap_before(core, section_idx, para, saved_para_gap_before);
            state.note_para(None, para);
        }
        TemplateBlock::Table {
            rows,
            break_before,
            rhwp_saved_gap_before,
            host_group,
            style,
            host_para_shape_id,
            para_format,
            line_segments,
            caption,
            column_widths,
            row_heights,
            table_layout,
            object_layout,
            border_fill,
            table_zones,
            cell_layouts,
            cell_formats,
            cell_blocks,
        } => {
            let reusable_para = state.reusable_para(*host_group);
            let para = if let Some(para) = reusable_para {
                para
            } else {
                prepare_body_paragraph(
                    core,
                    section_idx,
                    &mut state.used_first_body_para,
                    force_break.or(*break_before),
                )?
            };
            let table_para = create_table_from_rows(
                core,
                section_idx,
                para,
                rows,
                caption.as_ref(),
                column_widths,
                row_heights,
                table_layout.as_ref(),
                object_layout.as_ref(),
                border_fill.as_ref(),
                table_zones,
                cell_layouts,
                cell_formats,
                cell_blocks,
                style.as_ref(),
                *host_para_shape_id,
                para_format.as_ref(),
                line_segments,
                reusable_para.is_some(),
            )?;
            apply_body_saved_para_gap_before(core, section_idx, table_para, *rhwp_saved_gap_before);
            state.note_para(*host_group, table_para);
        }
        TemplateBlock::Equation {
            script,
            break_before,
            host_group,
            font_size,
            color,
            baseline,
            font_name,
            line_mode,
            width,
            height,
            treat_as_char,
        } => {
            let equation = template_equation(
                script,
                *font_size,
                *color,
                *baseline,
                font_name,
                line_mode,
                *width,
                *height,
                *treat_as_char,
            );
            let para = if let Some(para) = state.reusable_para(*host_group) {
                attach_equation_to_body_paragraph(core, section_idx, para, equation)?;
                para
            } else {
                let para = prepare_body_paragraph(
                    core,
                    section_idx,
                    &mut state.used_first_body_para,
                    force_break.or(*break_before),
                )?;
                set_equation_paragraph(core, section_idx, para, equation)?;
                para
            };
            state.note_para(*host_group, para);
        }
        TemplateBlock::Picture {
            break_before,
            host_group,
            line_segments,
            image_base64,
            external_path,
            extension,
            width,
            height,
            natural_width_px,
            natural_height_px,
            description,
            transparency,
            brightness,
            contrast,
            effect,
            effects,
            layout,
            object_layout,
            treat_as_char,
            horz_offset,
            vert_offset,
            caption,
        } => {
            let caption = template_caption(core, section_idx, caption.as_ref())?;
            let picture = template_picture(
                core,
                image_base64,
                external_path,
                extension,
                *width,
                *height,
                *natural_width_px,
                *natural_height_px,
                description,
                *transparency,
                *brightness,
                *contrast,
                effect,
                effects.as_ref(),
                layout.as_ref(),
                object_layout.as_ref(),
                *treat_as_char,
                *horz_offset,
                *vert_offset,
                caption,
            )?;
            let para = if let Some(para) = state.reusable_para(*host_group) {
                attach_picture_to_body_paragraph(core, section_idx, para, picture)?;
                apply_body_line_segments(core, section_idx, para, line_segments);
                para
            } else {
                let para = prepare_body_paragraph(
                    core,
                    section_idx,
                    &mut state.used_first_body_para,
                    force_break.or(*break_before),
                )?;
                set_picture_paragraph(core, section_idx, para, picture)?;
                apply_body_line_segments(core, section_idx, para, line_segments);
                para
            };
            state.note_para(*host_group, para);
        }
        TemplateBlock::ObjectPlaceholder {
            object_kind,
            break_before,
            host_group,
            shape_kind,
            description,
            placeholder_text,
            width,
            height,
            treat_as_char,
            horz_offset,
            vert_offset,
            caption,
            shape_component_id,
            geometry,
            drawing_style,
            layout,
            children,
            raw_hwp_chart_data_base64,
            raw_hwp_ole_tag_base64,
            ole_bin_data_base64,
            ole_extension,
            ole_object_type,
            ole_draw_aspect,
            ole_eq_base_line,
            ole_has_moniker,
        } => {
            let shape = template_object_placeholder(
                core,
                section_idx,
                object_kind,
                shape_kind,
                description,
                placeholder_text,
                *width,
                *height,
                *treat_as_char,
                *horz_offset,
                *vert_offset,
                caption.as_ref(),
                *shape_component_id,
                geometry.as_ref(),
                drawing_style.as_ref(),
                layout.as_ref(),
                children,
                raw_hwp_chart_data_base64,
                raw_hwp_ole_tag_base64,
                ole_bin_data_base64,
                ole_extension,
                ole_object_type,
                ole_draw_aspect,
                ole_eq_base_line,
                ole_has_moniker,
            )?;
            let para = if let Some(para) = state.reusable_para(*host_group) {
                attach_shape_to_body_paragraph(core, section_idx, para, shape)?;
                para
            } else {
                let para = prepare_body_paragraph(
                    core,
                    section_idx,
                    &mut state.used_first_body_para,
                    force_break.or(*break_before),
                )?;
                set_shape_paragraph(core, section_idx, para, shape)?;
                para
            };
            state.note_para(*host_group, para);
        }
    }
    Ok(())
}

fn prepare_body_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    used_first_body_para: &mut bool,
    break_before: Option<TemplateBreak>,
) -> Result<usize, String> {
    if !*used_first_body_para {
        *used_first_body_para = true;
        return Ok(0);
    }

    let para = append_empty_paragraph(core, section_idx)?;
    match break_before {
        Some(TemplateBreak::Page) => {
            mark_template_break_paragraph(core, section_idx, para, ColumnBreakType::Page, 0x04)
        }
        Some(TemplateBreak::Column) => {
            mark_template_break_paragraph(core, section_idx, para, ColumnBreakType::Column, 0x08)
        }
        None => Ok(para),
    }
}

fn mark_template_break_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    column_type: ColumnBreakType,
    raw_break_type: u8,
) -> Result<usize, String> {
    let Some(paragraphs) = core
        .document
        .sections
        .get_mut(section_idx)
        .map(|section| &mut section.paragraphs)
    else {
        return Err(format!("section not found: {section_idx}"));
    };
    let Some(paragraph) = paragraphs.get_mut(para_idx) else {
        return Err(format!(
            "paragraph not found: section={section_idx}, para={para_idx}"
        ));
    };

    paragraph.column_type = column_type;
    paragraph.raw_break_type = raw_break_type;
    crate::renderer::composer::recalculate_section_vpos(paragraphs, para_idx);
    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(para_idx)
}

fn append_empty_paragraph(core: &mut DocumentCore, section_idx: usize) -> Result<usize, String> {
    let para_idx = core
        .document
        .sections
        .get(section_idx)
        .map(|section| section.paragraphs.len())
        .unwrap_or(0);
    core.insert_paragraph_native(section_idx, para_idx)
        .map_err(|e| e.to_string())?;
    Ok(para_idx)
}

fn set_paragraph_text(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    text: &str,
) -> Result<(), String> {
    if !text.is_empty() {
        core.insert_text_native(section_idx, para_idx, 0, text)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn set_equation_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    equation: Equation,
) -> Result<(), String> {
    replace_body_paragraph_preserving_break(
        core,
        section_idx,
        para_idx,
        equation_paragraph(equation),
    )
}

fn set_picture_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    picture: Picture,
) -> Result<(), String> {
    replace_body_paragraph_preserving_break(core, section_idx, para_idx, picture_paragraph(picture))
}

fn set_shape_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    shape: ShapeObject,
) -> Result<(), String> {
    replace_body_paragraph_preserving_break(core, section_idx, para_idx, shape_paragraph(shape))
}

fn replace_body_paragraph_preserving_break(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    mut replacement: Paragraph,
) -> Result<(), String> {
    let paragraph = core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(para_idx))
        .ok_or_else(|| format!("paragraph not found: section={section_idx}, para={para_idx}"))?;
    replacement.column_type = paragraph.column_type;
    replacement.raw_break_type = paragraph.raw_break_type;
    *paragraph = replacement;
    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn attach_equation_to_body_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    equation: Equation,
) -> Result<(), String> {
    attach_control_to_body_paragraph(
        core,
        section_idx,
        para_idx,
        Control::Equation(Box::new(equation)),
    )
}

fn attach_picture_to_body_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    picture: Picture,
) -> Result<(), String> {
    attach_control_to_body_paragraph(
        core,
        section_idx,
        para_idx,
        Control::Picture(Box::new(picture)),
    )
}

fn attach_shape_to_body_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    shape: ShapeObject,
) -> Result<(), String> {
    attach_control_to_body_paragraph(core, section_idx, para_idx, Control::Shape(Box::new(shape)))
}

fn attach_control_to_body_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    control: Control,
) -> Result<(), String> {
    let paragraph = core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(para_idx))
        .ok_or_else(|| format!("paragraph not found: section={section_idx}, para={para_idx}"))?;
    attach_draw_control_to_paragraph(paragraph, control);
    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn create_table_from_rows(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    rows: &[Vec<String>],
    caption: Option<&TemplateCaption>,
    column_widths: &[u32],
    row_heights: &[u32],
    table_layout: Option<&TemplateTableLayout>,
    object_layout: Option<&Value>,
    border_fill: Option<&Value>,
    table_zones: &[TemplateTableZone],
    cell_layouts: &[Vec<TemplateCellLayout>],
    cell_formats: &[Vec<TemplateTextFormat>],
    cell_blocks: &[Vec<Vec<TemplateBlock>>],
    style: Option<&TemplateStyleRef>,
    host_para_shape_id: u16,
    para_format: Option<&Value>,
    line_segments: &[TemplateLineSeg],
    force_current_host: bool,
) -> Result<usize, String> {
    let table = template_table(
        core,
        section_idx,
        rows,
        caption,
        column_widths,
        row_heights,
        table_layout,
        object_layout,
        border_fill,
        table_zones,
        cell_layouts,
        cell_formats,
        cell_blocks,
    )?;

    let merge_into_previous = (!force_current_host)
        .then(|| {
            core.document.sections.get(section_idx).and_then(|section| {
                let current = section.paragraphs.get(para_idx)?;
                let previous = para_idx
                    .checked_sub(1)
                    .and_then(|idx| section.paragraphs.get(idx))?;
                let current_is_fresh_host = current.text.is_empty()
                    && current.controls.is_empty()
                    && matches!(
                        current.column_type,
                        ColumnBreakType::None | ColumnBreakType::Page
                    );
                let previous_has_table = previous
                    .controls
                    .iter()
                    .any(|control| matches!(control, Control::Table(_)));
                let previous_has_multirow_table = previous
                    .controls
                    .iter()
                    .any(|control| matches!(control, Control::Table(table) if table.row_count > 1));
                let previous_accepts_table = if current.column_type == ColumnBreakType::Page {
                    previous_has_multirow_table
                } else {
                    !previous_has_table
                };
                let previous_is_whitespace_host = previous.text.trim().is_empty()
                    && previous.column_type == ColumnBreakType::None
                    && previous.controls.iter().all(can_share_template_table_host)
                    && previous_accepts_table;
                (current_is_fresh_host && previous_is_whitespace_host).then_some(para_idx - 1)
            })
        })
        .flatten();

    let target_para_idx = merge_into_previous.unwrap_or(para_idx);
    let style_id = style
        .and_then(|style| valid_template_style_ref(core, style))
        .map(|style| style.id);
    let para_shape_id = resolve_template_host_para_shape_id(
        core,
        section_idx,
        host_para_shape_id,
        para_format,
        should_preserve_table_host_para_format(&table),
    );
    let section = core
        .document
        .sections
        .get_mut(section_idx)
        .ok_or_else(|| format!("section not found: {section_idx}"))?;
    let paragraph = section.paragraphs.get_mut(target_para_idx).ok_or_else(|| {
        format!("paragraph not found: section={section_idx}, para={target_para_idx}")
    })?;
    if let Some(style_id) = style_id {
        paragraph.style_id = style_id;
    }
    paragraph.para_shape_id = para_shape_id;
    apply_template_line_segments(paragraph, line_segments);
    attach_table_to_paragraph(paragraph, table);
    if merge_into_previous.is_some() {
        section.paragraphs.remove(para_idx);
    }
    section.raw_stream = None;
    core.rebuild_section(section_idx);

    Ok(target_para_idx)
}

fn can_share_template_table_host(control: &Control) -> bool {
    matches!(
        control,
        Control::Table(_)
            | Control::SectionDef(_)
            | Control::ColumnDef(_)
            | Control::PageHide(_)
            | Control::PageNumberPos(_)
    )
}

fn create_header_footer_from_template(
    core: &mut DocumentCore,
    is_header: bool,
    template: &TemplateHeaderFooter,
) -> Result<(), String> {
    if template.section >= core.document.sections.len() {
        return Ok(());
    }
    let paragraphs =
        header_footer_paragraphs_from_blocks(core, template.section, &template.blocks)?;
    core.create_header_footer_native(template.section, is_header, template.apply_to)
        .map_err(|e| e.to_string())?;

    {
        let target =
            header_footer_paragraphs_mut(core, template.section, is_header, template.apply_to)
                .ok_or_else(|| "created header/footer control was not found".to_string())?;
        *target = paragraphs;
    }

    core.document.sections[template.section].raw_stream = None;
    core.rebuild_section(template.section);
    Ok(())
}

fn create_master_page_from_template(
    core: &mut DocumentCore,
    template: &TemplateMasterPage,
) -> Result<(), String> {
    if template.section >= core.document.sections.len() {
        return Ok(());
    }
    let paragraphs =
        header_footer_paragraphs_from_blocks(core, template.section, &template.blocks)?;
    let (default_width, default_height) = master_page_default_text_size(core, template.section);
    let ext_flags = if template.ext_flags != 0 {
        template.ext_flags
    } else {
        u16::from(template.overlap) | if template.is_extension { 0x02 } else { 0 }
    };
    let is_extension = template.is_extension || (ext_flags & 0x02 != 0);
    let master_page = MasterPage {
        apply_to: apply_to_from_u8(template.apply_to),
        is_extension,
        overlap: template.overlap || (ext_flags & 0x01 != 0),
        replace_base: template.replace_base,
        ext_flags,
        paragraphs,
        text_width: if template.text_width != 0 {
            template.text_width
        } else {
            default_width
        },
        text_height: if template.text_height != 0 {
            template.text_height
        } else {
            default_height
        },
        text_ref: template.text_ref,
        num_ref: template.num_ref,
        hwpx_page_number: template.hwpx_page_number,
        raw_list_header: Vec::new(),
    };

    let section = &mut core.document.sections[template.section];
    section.section_def.master_pages.push(master_page);
    section.raw_stream = None;
    sync_section_def_control(section);
    core.rebuild_section(template.section);
    Ok(())
}

fn master_page_default_text_size(core: &DocumentCore, section_idx: usize) -> (u32, u32) {
    let Some(section) = core.document.sections.get(section_idx) else {
        return (0, 0);
    };
    let page_def = &section.section_def.page_def;
    let width = page_def
        .width
        .saturating_sub(page_def.margin_left)
        .saturating_sub(page_def.margin_right)
        .saturating_sub(page_def.margin_gutter);
    let height = page_def
        .height
        .saturating_sub(page_def.margin_top)
        .saturating_sub(page_def.margin_bottom)
        .saturating_sub(page_def.margin_header)
        .saturating_sub(page_def.margin_footer);
    (width, height)
}

fn header_footer_paragraphs_from_blocks(
    core: &mut DocumentCore,
    section_idx: usize,
    blocks: &[TemplateBlock],
) -> Result<Vec<Paragraph>, String> {
    if blocks.is_empty() {
        return Ok(vec![Paragraph::new_empty()]);
    }

    let mut paragraphs = Vec::new();
    for block in blocks {
        match block {
            TemplateBlock::Paragraph {
                text,
                style,
                char_format,
                char_shape_runs,
                para_format,
                line_segments,
                ..
            } => {
                let mut para = formatted_text_paragraph(
                    core,
                    section_idx,
                    text,
                    style.as_ref(),
                    char_format.as_ref(),
                    char_shape_runs,
                    para_format.as_ref(),
                );
                apply_template_line_segments(&mut para, line_segments);
                paragraphs.push(para);
            }
            TemplateBlock::Table {
                rows,
                break_before: _,
                rhwp_saved_gap_before: _,
                host_group: _,
                style,
                host_para_shape_id,
                para_format,
                line_segments,
                caption,
                column_widths,
                row_heights,
                table_layout,
                object_layout,
                border_fill,
                table_zones,
                cell_layouts,
                cell_formats,
                cell_blocks,
            } => {
                paragraphs.push(table_host_paragraph(
                    core,
                    section_idx,
                    rows,
                    caption.as_ref(),
                    column_widths,
                    row_heights,
                    table_layout.as_ref(),
                    object_layout.as_ref(),
                    border_fill.as_ref(),
                    table_zones,
                    cell_layouts,
                    cell_formats,
                    cell_blocks,
                    style.as_ref(),
                    *host_para_shape_id,
                    para_format.as_ref(),
                    line_segments,
                )?);
            }
            TemplateBlock::Equation {
                script,
                break_before: _,
                host_group: _,
                font_size,
                color,
                baseline,
                font_name,
                line_mode,
                width,
                height,
                treat_as_char,
            } => paragraphs.push(equation_paragraph(template_equation(
                script,
                *font_size,
                *color,
                *baseline,
                font_name,
                line_mode,
                *width,
                *height,
                *treat_as_char,
            ))),
            TemplateBlock::Picture {
                break_before: _,
                host_group: _,
                line_segments,
                image_base64,
                external_path,
                extension,
                width,
                height,
                natural_width_px,
                natural_height_px,
                description,
                transparency,
                brightness,
                contrast,
                effect,
                effects,
                layout,
                object_layout,
                treat_as_char,
                horz_offset,
                vert_offset,
                caption,
            } => {
                let caption = template_caption(core, section_idx, caption.as_ref())?;
                paragraphs.push(picture_paragraph_with_line_segments(
                    template_picture(
                        core,
                        image_base64,
                        external_path,
                        extension,
                        *width,
                        *height,
                        *natural_width_px,
                        *natural_height_px,
                        description,
                        *transparency,
                        *brightness,
                        *contrast,
                        effect,
                        effects.as_ref(),
                        layout.as_ref(),
                        object_layout.as_ref(),
                        *treat_as_char,
                        *horz_offset,
                        *vert_offset,
                        caption,
                    )?,
                    line_segments,
                ));
            }
            TemplateBlock::ObjectPlaceholder {
                object_kind,
                break_before: _,
                host_group: _,
                shape_kind,
                description,
                placeholder_text,
                width,
                height,
                treat_as_char,
                horz_offset,
                vert_offset,
                caption,
                shape_component_id,
                geometry,
                drawing_style,
                layout,
                children,
                raw_hwp_chart_data_base64,
                raw_hwp_ole_tag_base64,
                ole_bin_data_base64,
                ole_extension,
                ole_object_type,
                ole_draw_aspect,
                ole_eq_base_line,
                ole_has_moniker,
            } => paragraphs.push(shape_paragraph(template_object_placeholder(
                core,
                section_idx,
                object_kind,
                shape_kind,
                description,
                placeholder_text,
                *width,
                *height,
                *treat_as_char,
                *horz_offset,
                *vert_offset,
                caption.as_ref(),
                *shape_component_id,
                geometry.as_ref(),
                drawing_style.as_ref(),
                layout.as_ref(),
                children,
                raw_hwp_chart_data_base64,
                raw_hwp_ole_tag_base64,
                ole_bin_data_base64,
                ole_extension,
                ole_object_type,
                ole_draw_aspect,
                ole_eq_base_line,
                ole_has_moniker,
            )?)),
        }
    }
    Ok(paragraphs)
}

fn text_paragraph(text: &str) -> Paragraph {
    let mut para = Paragraph::new_empty();
    if !text.is_empty() {
        para.insert_text_at(0, text);
        para.has_para_text = true;
    }
    para
}

fn formatted_text_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    text: &str,
    style: Option<&TemplateStyleRef>,
    char_format: Option<&Value>,
    char_shape_runs: &[TemplateCharShapeRun],
    para_format: Option<&Value>,
) -> Paragraph {
    let (default_char_shape_id, default_para_shape_id) = default_shape_ids(core, section_idx);
    let mut para = text_paragraph(text);
    if let Some(style) = style.and_then(|style| valid_template_style_ref(core, style)) {
        para.style_id = style.id;
    }
    para.char_shapes = vec![CharShapeRef {
        start_pos: 0,
        char_shape_id: ensure_char_shape_for_template(core, default_char_shape_id, char_format),
    }];
    para.para_shape_id = ensure_para_shape_for_template(core, default_para_shape_id, para_format);
    apply_template_char_shape_runs(core, &mut para, char_shape_runs);
    para
}

fn table_host_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    rows: &[Vec<String>],
    caption: Option<&TemplateCaption>,
    column_widths: &[u32],
    row_heights: &[u32],
    table_layout: Option<&TemplateTableLayout>,
    object_layout: Option<&Value>,
    border_fill: Option<&Value>,
    table_zones: &[TemplateTableZone],
    cell_layouts: &[Vec<TemplateCellLayout>],
    cell_formats: &[Vec<TemplateTextFormat>],
    cell_blocks: &[Vec<Vec<TemplateBlock>>],
    style: Option<&TemplateStyleRef>,
    host_para_shape_id: u16,
    para_format: Option<&Value>,
    line_segments: &[TemplateLineSeg],
) -> Result<Paragraph, String> {
    let table = template_table(
        core,
        section_idx,
        rows,
        caption,
        column_widths,
        row_heights,
        table_layout,
        object_layout,
        border_fill,
        table_zones,
        cell_layouts,
        cell_formats,
        cell_blocks,
    )?;
    let preserve_host_para_format = should_preserve_table_host_para_format(&table);
    Ok(table_paragraph(
        core,
        section_idx,
        table,
        style,
        host_para_shape_id,
        para_format,
        preserve_host_para_format,
        line_segments,
    ))
}

fn table_paragraph(
    core: &mut DocumentCore,
    section_idx: usize,
    table: Table,
    style: Option<&TemplateStyleRef>,
    host_para_shape_id: u16,
    para_format: Option<&Value>,
    preserve_host_para_format: bool,
    line_segments: &[TemplateLineSeg],
) -> Paragraph {
    let mut para = Paragraph::new_empty();
    if let Some(style) = style.and_then(|style| valid_template_style_ref(core, style)) {
        para.style_id = style.id;
    }
    para.para_shape_id = resolve_template_host_para_shape_id(
        core,
        section_idx,
        host_para_shape_id,
        para_format,
        preserve_host_para_format,
    );
    apply_template_line_segments(&mut para, line_segments);
    attach_table_to_paragraph(&mut para, table);
    para.raw_header_extra = paragraph_header_extra();
    para
}

fn resolve_template_host_para_shape_id(
    core: &mut DocumentCore,
    section_idx: usize,
    _host_para_shape_id: u16,
    para_format: Option<&Value>,
    preserve: bool,
) -> u16 {
    let (_, default_para_shape_id) = default_shape_ids(core, section_idx);
    if !preserve {
        return default_para_shape_id;
    }
    let filtered = table_host_para_format_for_layout(para_format);
    ensure_para_shape_for_template(core, default_para_shape_id, filtered.as_ref())
}

fn should_preserve_table_host_para_format(table: &Table) -> bool {
    table.common.treat_as_char && !(table.row_count == 1 && table.common.height <= 4_000)
}

fn table_host_para_format_for_layout(para_format: Option<&Value>) -> Option<Value> {
    let object = para_format.and_then(Value::as_object)?;
    let mut filtered = Map::new();
    for key in ["alignment", "marginLeft", "marginRight", "indent"] {
        if let Some(value) = object.get(key) {
            filtered.insert(key.to_string(), value.clone());
        }
    }
    (!filtered.is_empty()).then_some(Value::Object(filtered))
}

fn attach_table_to_paragraph(para: &mut Paragraph, table: Table) {
    attach_draw_control_to_paragraph(para, Control::Table(Box::new(table)));
}

fn attach_draw_control_to_paragraph(para: &mut Paragraph, control: Control) {
    para.char_count = para.char_count.saturating_add(8);
    para.control_mask |= 0x0000_0800;
    para.controls.push(control);
    para.ctrl_data_records.push(None);
    para.has_para_text = true;
}

fn equation_block(equation: &Equation) -> TemplateBlock {
    TemplateBlock::Equation {
        script: equation.script.clone(),
        break_before: None,
        host_group: None,
        font_size: equation.font_size,
        color: equation.color,
        baseline: equation.baseline,
        font_name: equation.font_name.clone(),
        line_mode: equation.line_mode.clone(),
        width: equation.common.width,
        height: equation.common.height,
        treat_as_char: equation.common.treat_as_char,
    }
}

fn template_equation(
    script: &str,
    font_size: u32,
    color: u32,
    baseline: i16,
    font_name: &str,
    line_mode: &str,
    width: u32,
    height: u32,
    treat_as_char: bool,
) -> Equation {
    Equation {
        common: CommonObjAttr {
            ctrl_id: CTRL_EQUATION,
            treat_as_char,
            width,
            height,
            ..Default::default()
        },
        script: script.to_string(),
        font_size,
        color,
        baseline,
        font_name: if font_name.is_empty() {
            default_equation_font_name()
        } else {
            font_name.to_string()
        },
        line_mode: line_mode.to_string(),
        ..Default::default()
    }
}

fn equation_paragraph(equation: Equation) -> Paragraph {
    let mut para = Paragraph::new_empty();
    para.char_count = 9;
    para.control_mask = 0x0000_0800;
    para.controls.push(Control::Equation(Box::new(equation)));
    para.ctrl_data_records.push(None);
    para.has_para_text = true;
    para.raw_header_extra = paragraph_header_extra();
    para
}

fn picture_block(core: &DocumentCore, picture: &Picture) -> Option<TemplateBlock> {
    let content = picture_bin_data_content(core, picture.image_attr.bin_data_id);
    let external_path = picture.image_attr.external_path.clone().unwrap_or_default();
    if content.is_none() && external_path.is_empty() {
        return None;
    }
    let width = picture.common.width.max(picture.shape_attr.current_width);
    let height = picture.common.height.max(picture.shape_attr.current_height);
    if width == 0 || height == 0 {
        return None;
    }
    let natural_width_px = if picture.img_dim.0 > 0 {
        picture.img_dim.0
    } else {
        picture.shape_attr.original_width.saturating_div(75).max(1)
    };
    let natural_height_px = if picture.img_dim.1 > 0 {
        picture.img_dim.1
    } else {
        picture.shape_attr.original_height.saturating_div(75).max(1)
    };
    let image_base64 = content
        .filter(|content| !content.data.is_empty())
        .map(|content| base64::engine::general_purpose::STANDARD.encode(&content.data))
        .unwrap_or_default();
    if image_base64.is_empty() && external_path.is_empty() {
        return None;
    }
    let extension = content
        .map(|content| content.extension.clone())
        .filter(|extension| !extension.trim().is_empty())
        .or_else(|| picture_extension_from_path(&external_path))
        .unwrap_or_else(default_picture_extension);

    Some(TemplateBlock::Picture {
        break_before: None,
        host_group: None,
        line_segments: Vec::new(),
        image_base64,
        external_path,
        extension,
        width,
        height,
        natural_width_px,
        natural_height_px,
        description: picture.common.description.clone(),
        transparency: picture.image_attr.clamped_transparency(),
        brightness: picture.image_attr.brightness,
        contrast: picture.image_attr.contrast,
        effect: picture_effect_template_value(picture.image_attr.effect),
        effects: picture_effects_json(&picture.effects),
        layout: picture_layout_json(picture, width, height, natural_width_px, natural_height_px),
        object_layout: common_object_layout_template(&picture.common),
        treat_as_char: picture.common.treat_as_char,
        horz_offset: picture.common.horizontal_offset,
        vert_offset: picture.common.vertical_offset,
        caption: caption_template(core, picture.caption.as_ref()),
    })
}

fn picture_bin_data_content(
    core: &DocumentCore,
    bin_data_id: u16,
) -> Option<&crate::model::bin_data::BinDataContent> {
    if bin_data_id == 0 {
        return None;
    }
    core.document
        .bin_data_content
        .iter()
        .find(|content| content.id == bin_data_id)
        .or_else(|| {
            core.document
                .bin_data_content
                .get((bin_data_id - 1) as usize)
        })
}

fn bin_data_link_path(core: &DocumentCore, bin_data_id: u16) -> Option<String> {
    if bin_data_id == 0 {
        return None;
    }
    core.document
        .doc_info
        .bin_data_list
        .get((bin_data_id - 1) as usize)
        .filter(|bin_data| matches!(bin_data.data_type, BinDataType::Link))
        .and_then(|bin_data| {
            bin_data
                .abs_path
                .clone()
                .filter(|path| !path.is_empty())
                .or_else(|| bin_data.rel_path.clone().filter(|path| !path.is_empty()))
        })
}

fn next_template_bin_data_id(core: &DocumentCore) -> u16 {
    core.document.doc_info.bin_data_list.len() as u16 + 1
}

fn normalized_picture_extension(extension: &str, external_path: &str) -> String {
    let extension = extension.trim().trim_start_matches('.');
    if !extension.is_empty() {
        extension.to_string()
    } else {
        picture_extension_from_path(external_path).unwrap_or_else(default_picture_extension)
    }
}

fn picture_extension_from_path(path: &str) -> Option<String> {
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let (_, extension) = file_name.rsplit_once('.')?;
    let extension = extension.trim();
    if extension.is_empty() {
        None
    } else {
        Some(extension.to_string())
    }
}

fn picture_layout_json(
    picture: &Picture,
    width: u32,
    height: u32,
    natural_width_px: u32,
    natural_height_px: u32,
) -> Option<Value> {
    let mut map = Map::new();
    if let Some(crop) = picture_crop_json(picture, natural_width_px, natural_height_px) {
        map.insert("crop".to_string(), crop);
    }
    if let Some(padding) = picture_padding_json(&picture.padding) {
        map.insert("padding".to_string(), padding);
    }
    if let Some(image_rect) = picture_image_rect_json(picture, width, height) {
        map.insert("image_rect".to_string(), image_rect);
    }
    if let Some(shape_attr) = picture_shape_attr_json(&picture.shape_attr, width, height) {
        map.insert("shape_attr".to_string(), shape_attr);
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn picture_crop_json(
    picture: &Picture,
    natural_width_px: u32,
    natural_height_px: u32,
) -> Option<Value> {
    let default_right = natural_width_px.saturating_mul(75) as i32;
    let default_bottom = natural_height_px.saturating_mul(75) as i32;
    if picture.crop.left == 0
        && picture.crop.top == 0
        && picture.crop.right == default_right
        && picture.crop.bottom == default_bottom
    {
        return None;
    }
    Some(json!({
        "left": picture.crop.left,
        "top": picture.crop.top,
        "right": picture.crop.right,
        "bottom": picture.crop.bottom,
    }))
}

fn picture_padding_json(padding: &Padding) -> Option<Value> {
    if padding.left == 0 && padding.right == 0 && padding.top == 0 && padding.bottom == 0 {
        return None;
    }
    Some(json!({
        "left": padding.left,
        "right": padding.right,
        "top": padding.top,
        "bottom": padding.bottom,
    }))
}

fn picture_image_rect_json(picture: &Picture, width: u32, height: u32) -> Option<Value> {
    let points = picture_image_rect_points(picture);
    let default = [
        (0, 0),
        (width as i32, 0),
        (width as i32, height as i32),
        (0, height as i32),
    ];
    if points == default {
        return None;
    }
    Some(Value::Array(
        points
            .iter()
            .map(|(x, y)| json!({ "x": x, "y": y }))
            .collect(),
    ))
}

fn picture_image_rect_points(picture: &Picture) -> [(i32, i32); 4] {
    [
        (picture.border_x[0], picture.border_x[1]),
        (picture.border_x[2], picture.border_x[3]),
        (picture.border_y[0], picture.border_y[1]),
        (picture.border_y[2], picture.border_y[3]),
    ]
}

fn picture_shape_attr_json(
    shape_attr: &ShapeComponentAttr,
    width: u32,
    height: u32,
) -> Option<Value> {
    let mut map = Map::new();
    if shape_attr.offset_x != 0 {
        map.insert("offset_x".to_string(), json!(shape_attr.offset_x));
    }
    if shape_attr.offset_y != 0 {
        map.insert("offset_y".to_string(), json!(shape_attr.offset_y));
    }
    if shape_attr.group_level != 0 {
        map.insert("group_level".to_string(), json!(shape_attr.group_level));
    }
    if shape_attr.local_file_version > 1 {
        map.insert(
            "local_file_version".to_string(),
            json!(shape_attr.local_file_version),
        );
    }
    if shape_attr.original_width != 0 && shape_attr.original_width != width {
        map.insert(
            "original_width".to_string(),
            json!(shape_attr.original_width),
        );
    }
    if shape_attr.original_height != 0 && shape_attr.original_height != height {
        map.insert(
            "original_height".to_string(),
            json!(shape_attr.original_height),
        );
    }
    if shape_attr.current_width != 0 && shape_attr.current_width != width {
        map.insert("current_width".to_string(), json!(shape_attr.current_width));
    }
    if shape_attr.current_height != 0 && shape_attr.current_height != height {
        map.insert(
            "current_height".to_string(),
            json!(shape_attr.current_height),
        );
    }
    if shape_attr.flip != 0 {
        map.insert("flip".to_string(), json!(shape_attr.flip));
    }
    if shape_attr.horz_flip {
        map.insert("horz_flip".to_string(), json!(true));
    }
    if shape_attr.vert_flip {
        map.insert("vert_flip".to_string(), json!(true));
    }
    if shape_attr.rotation_angle != 0 {
        map.insert(
            "rotation_angle".to_string(),
            json!(shape_attr.rotation_angle),
        );
    }
    if shape_attr.rotate_image {
        map.insert("rotate_image".to_string(), json!(true));
    }
    if shape_attr.rotation_center.x != 0 {
        map.insert(
            "rotation_center_x".to_string(),
            json!(shape_attr.rotation_center.x),
        );
    }
    if shape_attr.rotation_center.y != 0 {
        map.insert(
            "rotation_center_y".to_string(),
            json!(shape_attr.rotation_center.y),
        );
    }
    if has_non_identity_rendering(shape_attr) {
        map.insert("render_sx".to_string(), json!(shape_attr.render_sx));
        map.insert("render_b".to_string(), json!(shape_attr.render_b));
        map.insert("render_tx".to_string(), json!(shape_attr.render_tx));
        map.insert("render_c".to_string(), json!(shape_attr.render_c));
        map.insert("render_sy".to_string(), json!(shape_attr.render_sy));
        map.insert("render_ty".to_string(), json!(shape_attr.render_ty));
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn has_non_identity_rendering(shape_attr: &ShapeComponentAttr) -> bool {
    const EPSILON: f64 = 0.000_001;
    (shape_attr.render_sx - 1.0).abs() > EPSILON
        || shape_attr.render_b.abs() > EPSILON
        || shape_attr.render_tx.abs() > EPSILON
        || shape_attr.render_c.abs() > EPSILON
        || (shape_attr.render_sy - 1.0).abs() > EPSILON
        || shape_attr.render_ty.abs() > EPSILON
}

fn object_layout_json(shape_attr: &ShapeComponentAttr, width: u32, height: u32) -> Option<Value> {
    let mut map = Map::new();
    if let Some(shape_attr) = picture_shape_attr_json(shape_attr, width, height) {
        map.insert("shape_attr".to_string(), shape_attr);
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn picture_effects_json(effects: &PictureEffects) -> Option<Value> {
    let mut map = Map::new();
    if let Some(shadow) = &effects.shadow {
        map.insert("shadow".to_string(), picture_shadow_json(shadow));
    }
    if let Some(glow) = &effects.glow {
        map.insert("glow".to_string(), picture_glow_json(glow));
    }
    if let Some(soft_edge) = &effects.soft_edge {
        map.insert("soft_edge".to_string(), picture_soft_edge_json(soft_edge));
    }
    if let Some(reflection) = &effects.reflection {
        map.insert(
            "reflection".to_string(),
            picture_reflection_json(reflection),
        );
    }
    if let Some(three_d) = &effects.three_d {
        map.insert("threeD".to_string(), picture_three_d_json(three_d));
    }
    if let Some(blur) = &effects.blur {
        map.insert("blur".to_string(), picture_blur_json(blur));
    }
    if let Some(fill_overlay) = &effects.fill_overlay {
        map.insert(
            "fillOverlay".to_string(),
            picture_fill_overlay_json(fill_overlay),
        );
    }
    for (json_key, effect_name) in [
        ("threeD", b"threeD".as_slice()),
        ("fillOverlay", b"fillOverlay".as_slice()),
    ] {
        if map.contains_key(json_key) {
            continue;
        }
        if let Some(value) = effects
            .raw_xml
            .iter()
            .find_map(|raw| drawing_style_effect_json_from_raw_fragment(raw, effect_name))
        {
            map.insert(json_key.to_string(), value);
        }
    }
    let unknown_raw_xml = effects
        .raw_xml
        .iter()
        .filter(|raw| {
            ![b"threeD".as_slice(), b"fillOverlay".as_slice()]
                .iter()
                .any(|effect_name| drawing_style_raw_fragment_contains_effect(raw, effect_name))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !unknown_raw_xml.is_empty() {
        map.insert(
            "raw_xml".to_string(),
            Value::Array(unknown_raw_xml.into_iter().map(Value::String).collect()),
        );
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn picture_shadow_json(shadow: &PictureShadow) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "style", &shadow.style);
    insert_opt_string(&mut map, "alpha", &shadow.alpha);
    insert_opt_string(&mut map, "radius", &shadow.radius);
    insert_opt_string(&mut map, "direction", &shadow.direction);
    insert_opt_string(&mut map, "distance", &shadow.distance);
    insert_opt_string(&mut map, "align_style", &shadow.align_style);
    insert_opt_string(&mut map, "rotation_style", &shadow.rotation_style);
    if let Some(skew) = &shadow.skew {
        map.insert("skew".to_string(), effect_point_json(skew));
    }
    if let Some(scale) = &shadow.scale {
        map.insert("scale".to_string(), effect_point_json(scale));
    }
    if let Some(color) = &shadow.color {
        map.insert("color".to_string(), effect_color_json(color));
    }
    if !shadow.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                shadow
                    .raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn picture_glow_json(glow: &PictureGlow) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "alpha", &glow.alpha);
    insert_opt_string(&mut map, "radius", &glow.radius);
    if let Some(color) = &glow.color {
        map.insert("color".to_string(), effect_color_json(color));
    }
    if !glow.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                glow.raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn picture_soft_edge_json(soft_edge: &PictureSoftEdge) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "radius", &soft_edge.radius);
    if !soft_edge.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                soft_edge
                    .raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn picture_reflection_json(reflection: &PictureReflection) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "align_style", &reflection.align_style);
    insert_opt_string(&mut map, "radius", &reflection.radius);
    insert_opt_string(&mut map, "direction", &reflection.direction);
    insert_opt_string(&mut map, "distance", &reflection.distance);
    insert_opt_string(&mut map, "rotation_style", &reflection.rotation_style);
    insert_opt_string(&mut map, "fade_direction", &reflection.fade_direction);
    if let Some(skew) = &reflection.skew {
        map.insert("skew".to_string(), effect_point_json(skew));
    }
    if let Some(scale) = &reflection.scale {
        map.insert("scale".to_string(), effect_point_json(scale));
    }
    if let Some(color) = &reflection.color {
        map.insert("color".to_string(), effect_color_json(color));
    }
    if let Some(alpha) = &reflection.alpha {
        map.insert("alpha".to_string(), effect_range_json(alpha));
    }
    if let Some(pos) = &reflection.pos {
        map.insert("pos".to_string(), effect_range_json(pos));
    }
    if !reflection.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                reflection
                    .raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn picture_blur_json(blur: &PictureBlur) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "radius", &blur.radius);
    if !blur.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                blur.raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn picture_effect_child_json(child: &PictureEffectChild) -> Value {
    Value::Object(
        child
            .attrs
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect(),
    )
}

fn picture_three_d_json(three_d: &PictureThreeD) -> Value {
    let mut map: Map<String, Value> = three_d
        .attrs
        .iter()
        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
        .collect();
    if let Some(bevel) = &three_d.bevel {
        map.insert("bevel".to_string(), picture_effect_child_json(bevel));
    }
    if !three_d.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                three_d
                    .raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn picture_solid_fill_json(solid_fill: &PictureSolidFill) -> Value {
    let mut map = Map::new();
    if let Some(color) = &solid_fill.color {
        map.insert("color".to_string(), Value::String(color.clone()));
        if let Some(color_hex) = drawing_style_normalize_xml_color_hex(color) {
            map.insert("color_hex".to_string(), Value::String(color_hex));
        }
    }
    if let Some(color) = &solid_fill.effect_color {
        map.insert("color".to_string(), effect_color_json(color));
    }
    if !solid_fill.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                solid_fill
                    .raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn picture_fill_overlay_json(fill_overlay: &PictureFillOverlay) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "blend", &fill_overlay.blend);
    if let Some(solid_fill) = &fill_overlay.solid_fill {
        map.insert(
            "solid_fill".to_string(),
            picture_solid_fill_json(solid_fill),
        );
    }
    if !fill_overlay.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                fill_overlay
                    .raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn effect_point_json(point: &EffectPoint) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "x", &point.x);
    insert_opt_string(&mut map, "y", &point.y);
    Value::Object(map)
}

fn effect_range_json(range: &EffectRange) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "start", &range.start);
    insert_opt_string(&mut map, "end", &range.end);
    Value::Object(map)
}

fn effect_color_json(color: &EffectColor) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "type", &color.color_type);
    insert_opt_string(&mut map, "scheme_idx", &color.scheme_idx);
    insert_opt_string(&mut map, "system_idx", &color.system_idx);
    insert_opt_string(&mut map, "preset_idx", &color.preset_idx);
    if let Some(rgb) = &color.rgb {
        map.insert("rgb".to_string(), effect_rgb_json(rgb));
        if let Some(color_hex) = effect_rgb_color_hex(rgb) {
            map.insert("color_hex".to_string(), Value::String(color_hex));
        }
    }
    if !color.raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(
                color
                    .raw_child_xml
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    Value::Object(map)
}

fn effect_rgb_color_hex(rgb: &EffectRgb) -> Option<String> {
    let r = rgb.r.as_deref()?.trim().parse::<u8>().ok()?;
    let g = rgb.g.as_deref()?.trim().parse::<u8>().ok()?;
    let b = rgb.b.as_deref()?.trim().parse::<u8>().ok()?;
    Some(format!("#{:02X}{:02X}{:02X}", r, g, b))
}

fn effect_rgb_json(rgb: &EffectRgb) -> Value {
    let mut map = Map::new();
    insert_opt_string(&mut map, "r", &rgb.r);
    insert_opt_string(&mut map, "g", &rgb.g);
    insert_opt_string(&mut map, "b", &rgb.b);
    Value::Object(map)
}

fn picture_effect_template_value(effect: ImageEffect) -> String {
    match effect {
        ImageEffect::RealPic => String::new(),
        ImageEffect::GrayScale => "gray_scale".to_string(),
        ImageEffect::BlackWhite => "black_white".to_string(),
        ImageEffect::Pattern8x8 => "pattern8x8".to_string(),
    }
}

fn picture_effect_from_template(value: &str) -> ImageEffect {
    let key = value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    match key.as_str() {
        "" | "0" | "realpic" => ImageEffect::RealPic,
        "1" | "gray" | "grey" | "grayscale" | "greyscale" => ImageEffect::GrayScale,
        "2" | "blackwhite" | "bw" => ImageEffect::BlackWhite,
        "3" | "pattern8x8" | "pattern88" => ImageEffect::Pattern8x8,
        _ => ImageEffect::RealPic,
    }
}

fn insert_opt_string(map: &mut Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        map.insert(key.to_string(), Value::String(value.clone()));
    }
}

fn picture_effects_from_json(effects: Option<&Value>) -> Result<PictureEffects, String> {
    let Some(effects) = effects.filter(|value| !value.is_null()) else {
        return Ok(PictureEffects::default());
    };
    let Some(effects) = effects.as_object() else {
        return Err("picture effects must be an object".to_string());
    };
    let shadow = effects
        .get("shadow")
        .or_else(|| effects.get("picture_shadow"))
        .filter(|value| !value.is_null())
        .map(picture_shadow_from_json)
        .transpose()?;
    let glow = effects
        .get("glow")
        .filter(|value| !value.is_null())
        .map(picture_glow_from_json)
        .transpose()?;
    let soft_edge = effects
        .get("soft_edge")
        .or_else(|| effects.get("softEdge"))
        .filter(|value| !value.is_null())
        .map(picture_soft_edge_from_json)
        .transpose()?;
    let reflection = effects
        .get("reflection")
        .filter(|value| !value.is_null())
        .map(picture_reflection_from_json)
        .transpose()?;
    let three_d = effects
        .get("threeD")
        .or_else(|| effects.get("three_d"))
        .or_else(|| effects.get("pictureThreeD"))
        .or_else(|| effects.get("picture_three_d"))
        .filter(|value| !value.is_null())
        .map(picture_three_d_from_json)
        .transpose()?;
    let blur = effects
        .get("blur")
        .or_else(|| effects.get("pictureBlur"))
        .or_else(|| effects.get("picture_blur"))
        .filter(|value| !value.is_null())
        .map(picture_blur_from_json)
        .transpose()?;
    let fill_overlay = effects
        .get("fillOverlay")
        .or_else(|| effects.get("fill_overlay"))
        .or_else(|| effects.get("pictureFillOverlay"))
        .or_else(|| effects.get("picture_fill_overlay"))
        .filter(|value| !value.is_null())
        .map(picture_fill_overlay_from_json)
        .transpose()?;
    let mut raw_xml = json_string_array_alias(effects, &["raw_xml", "rawXml", "effectsRawXml"])?;
    apply_picture_semantic_raw_effects(effects, &mut raw_xml)?;
    Ok(PictureEffects {
        shadow,
        glow,
        soft_edge,
        reflection,
        three_d,
        blur,
        fill_overlay,
        raw_xml,
    })
}

fn apply_picture_semantic_raw_effects(
    effects: &Map<String, Value>,
    raw_xml: &mut Vec<String>,
) -> Result<(), String> {
    let specs: [(&str, &[u8], &[&str]); 0] = [];
    for (xml_name, effect_name, keys) in specs {
        let Some(value) = keys.iter().find_map(|key| effects.get(*key)) else {
            continue;
        };
        raw_xml.retain(|raw| !drawing_style_raw_fragment_contains_effect(raw, effect_name));
        if value.is_null() {
            continue;
        }
        if let Some(raw) = picture_effect_raw_xml(xml_name, value)? {
            raw_xml.push(raw);
        }
    }
    Ok(())
}

fn picture_effect_raw_xml(effect_name: &str, value: &Value) -> Result<Option<String>, String> {
    if !value.is_object() {
        return Err(format!(
            "picture effects.{effect_name} must be an object or null"
        ));
    }
    Ok(match effect_name {
        "threeD" => drawing_style_three_d_effect_xml(value),
        "fillOverlay" => drawing_style_simple_effect_xml(value, effect_name),
        _ => None,
    })
}

fn json_string_array_alias(
    object: &Map<String, Value>,
    keys: &[&str],
) -> Result<Vec<String>, String> {
    let Some(value) = keys.iter().find_map(|key| object.get(*key)) else {
        return Ok(Vec::new());
    };
    json_string_array_value(value, "picture effects raw_xml")
}

fn json_string_array_value(value: &Value, context: &str) -> Result<Vec<String>, String> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Some(raw) = value.as_str() {
        return Ok(vec![raw.to_string()]);
    }
    let Some(values) = value.as_array() else {
        return Err(format!("{context} must be a string array"));
    };
    let mut out = Vec::new();
    for value in values {
        let Some(raw) = value.as_str() else {
            return Err(format!("{context} entries must be strings"));
        };
        out.push(raw.to_string());
    }
    Ok(out)
}

fn picture_shadow_from_json(value: &Value) -> Result<PictureShadow, String> {
    let Some(object) = value.as_object() else {
        return Err("picture effects.shadow must be an object".to_string());
    };
    Ok(PictureShadow {
        style: json_string_alias(value, &["style"]),
        alpha: json_string_alias(value, &["alpha"]),
        radius: json_string_alias(value, &["radius"]),
        direction: json_string_alias(value, &["direction"]),
        distance: json_string_alias(value, &["distance"]),
        align_style: json_string_alias(value, &["align_style", "alignStyle"]),
        rotation_style: json_string_alias(value, &["rotation_style", "rotationStyle"]),
        skew: value
            .get("skew")
            .filter(|value| !value.is_null())
            .map(effect_point_from_json)
            .transpose()?
            .or_else(|| effect_point_from_flat_aliases(value, "skew")),
        scale: value
            .get("scale")
            .filter(|value| !value.is_null())
            .map(effect_point_from_json)
            .transpose()?
            .or_else(|| effect_point_from_flat_aliases(value, "scale")),
        color: value
            .get("color")
            .or_else(|| value.get("effectsColor"))
            .filter(|value| !value.is_null())
            .map(effect_color_from_json)
            .transpose()?,
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    })
}

fn picture_glow_from_json(value: &Value) -> Result<PictureGlow, String> {
    let Some(object) = value.as_object() else {
        return Err("picture effects.glow must be an object".to_string());
    };
    Ok(PictureGlow {
        alpha: json_string_alias(value, &["alpha"]),
        radius: json_string_alias(value, &["radius"]),
        color: value
            .get("color")
            .or_else(|| value.get("effectsColor"))
            .filter(|value| !value.is_null())
            .map(effect_color_from_json)
            .transpose()?,
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    })
}

fn picture_reflection_from_json(value: &Value) -> Result<PictureReflection, String> {
    let Some(object) = value.as_object() else {
        return Err("picture effects.reflection must be an object".to_string());
    };
    Ok(PictureReflection {
        align_style: json_string_alias(value, &["align_style", "alignStyle"]),
        radius: json_string_alias(value, &["radius"]),
        direction: json_string_alias(value, &["direction"]),
        distance: json_string_alias(value, &["distance"]),
        rotation_style: json_string_alias(value, &["rotation_style", "rotationStyle"]),
        fade_direction: json_string_alias(value, &["fade_direction", "fadeDirection"]),
        skew: value
            .get("skew")
            .filter(|value| !value.is_null())
            .map(effect_point_from_json)
            .transpose()?
            .or_else(|| effect_point_from_flat_aliases(value, "skew")),
        scale: value
            .get("scale")
            .filter(|value| !value.is_null())
            .map(effect_point_from_json)
            .transpose()?
            .or_else(|| effect_point_from_flat_aliases(value, "scale")),
        color: value
            .get("color")
            .or_else(|| value.get("effectsColor"))
            .filter(|value| !value.is_null())
            .map(effect_color_from_json)
            .transpose()?,
        alpha: value
            .get("alpha")
            .filter(|value| !value.is_null())
            .map(effect_range_from_json)
            .transpose()?
            .or_else(|| effect_range_from_flat_aliases(value, "alpha")),
        pos: value
            .get("pos")
            .filter(|value| !value.is_null())
            .map(effect_range_from_json)
            .transpose()?
            .or_else(|| effect_range_from_flat_aliases(value, "pos")),
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    })
}

fn picture_soft_edge_from_json(value: &Value) -> Result<PictureSoftEdge, String> {
    let Some(object) = value.as_object() else {
        return Err("picture effects.soft_edge must be an object".to_string());
    };
    Ok(PictureSoftEdge {
        radius: json_string_alias(value, &["radius"]),
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    })
}

fn picture_blur_from_json(value: &Value) -> Result<PictureBlur, String> {
    let Some(object) = value.as_object() else {
        return Err("picture effects.blur must be an object".to_string());
    };
    Ok(PictureBlur {
        radius: json_string_alias(value, &["radius"]),
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    })
}

fn picture_effect_child_from_json(value: &Value) -> Result<PictureEffectChild, String> {
    if let Some(child_type) = json_scalar_string(value) {
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("type".to_string(), child_type);
        return Ok(PictureEffectChild { attrs });
    }
    if !value.is_object() {
        return Err("picture effect child must be an object or string".to_string());
    }
    Ok(PictureEffectChild {
        attrs: drawing_style_xml_attr_map(value, &[]),
    })
}

fn picture_three_d_from_json(value: &Value) -> Result<PictureThreeD, String> {
    let Some(object) = value.as_object() else {
        return Err("picture effects.threeD must be an object".to_string());
    };
    let mut three_d = PictureThreeD {
        attrs: drawing_style_xml_attr_map(
            value,
            &[
                "bevel",
                "bevelType",
                "bevel_type",
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        ),
        bevel: None,
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    };
    if let Some(bevel) = value.get("bevel").filter(|value| !value.is_null()) {
        three_d.bevel = Some(picture_effect_child_from_json(bevel)?);
    } else if let Some(bevel_type) = json_string_alias(value, &["bevelType", "bevel_type"]) {
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("type".to_string(), bevel_type);
        three_d.bevel = Some(PictureEffectChild { attrs });
    }
    Ok(three_d)
}

fn picture_solid_fill_from_json(value: &Value) -> Result<PictureSolidFill, String> {
    if let Some(color) = json_scalar_string(value) {
        return Ok(PictureSolidFill {
            color: Some(color),
            ..PictureSolidFill::default()
        });
    }
    let Some(object) = value.as_object() else {
        return Err("picture effects.fillOverlay.solid_fill must be an object".to_string());
    };
    let color_value = value.get("color");
    let color = json_string_alias(value, &["color_hex", "colorHex"]).or_else(|| {
        color_value
            .filter(|value| !value.is_object())
            .and_then(json_scalar_string)
    });
    let effect_color = color_value
        .filter(|value| value.is_object())
        .or_else(|| value.get("effectsColor").filter(|value| !value.is_null()))
        .map(effect_color_from_json)
        .transpose()?;
    Ok(PictureSolidFill {
        color,
        effect_color,
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    })
}

fn picture_fill_overlay_from_json(value: &Value) -> Result<PictureFillOverlay, String> {
    let Some(object) = value.as_object() else {
        return Err("picture effects.fillOverlay must be an object".to_string());
    };
    Ok(PictureFillOverlay {
        blend: json_string_alias(value, &["blend"]),
        solid_fill: value
            .get("solid_fill")
            .or_else(|| value.get("solidFill"))
            .filter(|value| !value.is_null())
            .map(picture_solid_fill_from_json)
            .transpose()?,
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    })
}

fn effect_range_from_json(value: &Value) -> Result<EffectRange, String> {
    let Some(_) = value.as_object() else {
        return Err("picture effect range must be an object".to_string());
    };
    Ok(EffectRange {
        start: json_string_alias(value, &["start"]),
        end: json_string_alias(value, &["end"]),
    })
}

fn effect_point_from_json(value: &Value) -> Result<EffectPoint, String> {
    let Some(_) = value.as_object() else {
        return Err("picture effect point must be an object".to_string());
    };
    Ok(EffectPoint {
        x: json_string_alias(value, &["x"]),
        y: json_string_alias(value, &["y"]),
    })
}

fn effect_point_from_flat_aliases(value: &Value, prefix: &str) -> Option<EffectPoint> {
    let (x_keys, y_keys): (&[&str], &[&str]) = match prefix {
        "skew" => (&["skew_x", "skewX"], &["skew_y", "skewY"]),
        "scale" => (&["scale_x", "scaleX"], &["scale_y", "scaleY"]),
        _ => return None,
    };
    let x = json_string_alias(value, x_keys);
    let y = json_string_alias(value, y_keys);
    if x.is_none() && y.is_none() {
        None
    } else {
        Some(EffectPoint { x, y })
    }
}

fn effect_range_from_flat_aliases(value: &Value, prefix: &str) -> Option<EffectRange> {
    let (start_keys, end_keys): (&[&str], &[&str]) = match prefix {
        "alpha" => (&["alpha_start", "alphaStart"], &["alpha_end", "alphaEnd"]),
        "pos" => (&["pos_start", "posStart"], &["pos_end", "posEnd"]),
        _ => return None,
    };
    let start = json_string_alias(value, start_keys);
    let end = json_string_alias(value, end_keys);
    if start.is_none() && end.is_none() {
        None
    } else {
        Some(EffectRange { start, end })
    }
}

fn effect_color_from_json(value: &Value) -> Result<EffectColor, String> {
    if let Some(_) = json_scalar_string(value) {
        if let Some(rgb) = effect_rgb_from_color_hex_value(value) {
            return Ok(EffectColor {
                color_type: Some("RGB".to_string()),
                scheme_idx: Some("-1".to_string()),
                system_idx: Some("-1".to_string()),
                preset_idx: Some("-1".to_string()),
                rgb: Some(rgb),
                raw_child_xml: Vec::new(),
            });
        }
        return Err("picture effect color string must be a CSS #RRGGBB color".to_string());
    }
    let Some(object) = value.as_object() else {
        return Err("picture effect color must be an object or CSS #RRGGBB string".to_string());
    };
    let mut color = EffectColor {
        color_type: json_string_alias(value, &["type", "color_type"]),
        scheme_idx: json_string_alias(value, &["scheme_idx", "schemeIdx"]),
        system_idx: json_string_alias(value, &["system_idx", "systemIdx"]),
        preset_idx: json_string_alias(value, &["preset_idx", "presetIdx"]),
        rgb: value
            .get("rgb")
            .filter(|value| !value.is_null())
            .map(effect_rgb_from_json)
            .transpose()?,
        raw_child_xml: json_string_array_alias(
            object,
            &[
                "raw_child_xml",
                "rawChildXml",
                "raw_children_xml",
                "rawChildrenXml",
            ],
        )?,
    };
    if let Some(rgb) = value
        .get("color_hex")
        .or_else(|| value.get("colorHex"))
        .or_else(|| value.get("rgb_hex"))
        .or_else(|| value.get("rgbHex"))
        .and_then(effect_rgb_from_color_hex_value)
    {
        color.color_type.get_or_insert_with(|| "RGB".to_string());
        color.scheme_idx.get_or_insert_with(|| "-1".to_string());
        color.system_idx.get_or_insert_with(|| "-1".to_string());
        color.preset_idx.get_or_insert_with(|| "-1".to_string());
        color.rgb = Some(rgb);
    }
    Ok(color)
}

fn effect_rgb_from_color_hex_value(value: &Value) -> Option<EffectRgb> {
    let color_ref = json_css_color_ref_value(value)?;
    if color_ref == 0xFFFF_FFFF {
        return None;
    }
    let rgb = color_ref_to_rgb_u32(color_ref);
    Some(EffectRgb {
        r: Some(((rgb >> 16) & 0xFF).to_string()),
        g: Some(((rgb >> 8) & 0xFF).to_string()),
        b: Some((rgb & 0xFF).to_string()),
    })
}

fn color_ref_to_rgb_u32(color: u32) -> u32 {
    let r = color & 0xFF;
    let g = (color >> 8) & 0xFF;
    let b = (color >> 16) & 0xFF;
    (r << 16) | (g << 8) | b
}

fn effect_rgb_from_json(value: &Value) -> Result<EffectRgb, String> {
    let Some(_) = value.as_object() else {
        return Err("picture effect rgb must be an object".to_string());
    };
    Ok(EffectRgb {
        r: json_string_alias(value, &["r"]),
        g: json_string_alias(value, &["g"]),
        b: json_string_alias(value, &["b"]),
    })
}

fn json_string_alias(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(json_scalar_string)
}

fn json_scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some((if *value { "1" } else { "0" }).to_string()),
        _ => None,
    }
}

fn apply_picture_layout(
    layout: Option<&Value>,
    shape_attr: &mut ShapeComponentAttr,
    crop: &mut CropInfo,
    padding: &mut Padding,
    border_x: &mut [i32; 4],
    border_y: &mut [i32; 4],
) -> Result<(), String> {
    let Some(layout) = layout.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(_) = layout.as_object() else {
        return Err("picture layout must be an object".to_string());
    };

    if let Some(crop_value) = layout.get("crop").filter(|value| !value.is_null()) {
        apply_picture_crop(crop_value, crop)?;
    }
    if let Some(padding_value) = layout.get("padding").filter(|value| !value.is_null()) {
        apply_picture_padding(padding_value, padding)?;
    }
    if let Some(rect_value) = layout
        .get("image_rect")
        .or_else(|| layout.get("img_rect"))
        .filter(|value| !value.is_null())
    {
        apply_picture_image_rect(rect_value, border_x, border_y)?;
    }
    if let Some(shape_value) = layout
        .get("shape_attr")
        .or_else(|| layout.get("transform"))
        .filter(|value| !value.is_null())
    {
        apply_picture_shape_attr(shape_value, shape_attr)?;
    }
    Ok(())
}

fn apply_picture_crop(value: &Value, crop: &mut CropInfo) -> Result<(), String> {
    let Some(_) = value.as_object() else {
        return Err("picture layout.crop must be an object".to_string());
    };
    crop.left = json_i32(value, "left").unwrap_or(crop.left);
    crop.top = json_i32(value, "top").unwrap_or(crop.top);
    crop.right = json_i32(value, "right").unwrap_or(crop.right);
    crop.bottom = json_i32(value, "bottom").unwrap_or(crop.bottom);
    Ok(())
}

fn apply_picture_padding(value: &Value, padding: &mut Padding) -> Result<(), String> {
    let Some(_) = value.as_object() else {
        return Err("picture layout.padding must be an object".to_string());
    };
    padding.left = json_i16(value, "left").unwrap_or(padding.left);
    padding.right = json_i16(value, "right").unwrap_or(padding.right);
    padding.top = json_i16(value, "top").unwrap_or(padding.top);
    padding.bottom = json_i16(value, "bottom").unwrap_or(padding.bottom);
    Ok(())
}

fn apply_picture_image_rect(
    value: &Value,
    border_x: &mut [i32; 4],
    border_y: &mut [i32; 4],
) -> Result<(), String> {
    let points = picture_image_rect_from_json(value)?;
    *border_x = [points[0].0, points[0].1, points[1].0, points[1].1];
    *border_y = [points[2].0, points[2].1, points[3].0, points[3].1];
    Ok(())
}

fn picture_image_rect_from_json(value: &Value) -> Result<[(i32, i32); 4], String> {
    if let Some(array) = value.as_array() {
        if array.len() != 4 {
            return Err("picture layout.image_rect must contain four points".to_string());
        }
        return Ok([
            picture_point_from_json(&array[0], "image_rect[0]")?,
            picture_point_from_json(&array[1], "image_rect[1]")?,
            picture_point_from_json(&array[2], "image_rect[2]")?,
            picture_point_from_json(&array[3], "image_rect[3]")?,
        ]);
    }
    let Some(object) = value.as_object() else {
        return Err("picture layout.image_rect must be an array or object".to_string());
    };
    Ok([
        picture_point_from_json(
            object
                .get("pt0")
                .ok_or_else(|| "picture layout.image_rect.pt0 is required".to_string())?,
            "image_rect.pt0",
        )?,
        picture_point_from_json(
            object
                .get("pt1")
                .ok_or_else(|| "picture layout.image_rect.pt1 is required".to_string())?,
            "image_rect.pt1",
        )?,
        picture_point_from_json(
            object
                .get("pt2")
                .ok_or_else(|| "picture layout.image_rect.pt2 is required".to_string())?,
            "image_rect.pt2",
        )?,
        picture_point_from_json(
            object
                .get("pt3")
                .ok_or_else(|| "picture layout.image_rect.pt3 is required".to_string())?,
            "image_rect.pt3",
        )?,
    ])
}

fn picture_point_from_json(value: &Value, label: &str) -> Result<(i32, i32), String> {
    let Some(_) = value.as_object() else {
        return Err(format!("picture layout.{label} must be an object"));
    };
    let x = json_i32(value, "x").ok_or_else(|| format!("picture layout.{label}.x is required"))?;
    let y = json_i32(value, "y").ok_or_else(|| format!("picture layout.{label}.y is required"))?;
    Ok((x, y))
}

fn apply_picture_shape_attr(
    value: &Value,
    shape_attr: &mut ShapeComponentAttr,
) -> Result<(), String> {
    apply_shape_attr_json(value, shape_attr, "picture layout.shape_attr")
}

fn apply_shape_attr_json(
    value: &Value,
    shape_attr: &mut ShapeComponentAttr,
    context: &str,
) -> Result<(), String> {
    let Some(_) = value.as_object() else {
        return Err(format!("{context} must be an object"));
    };
    shape_attr.offset_x = json_i32(value, "offset_x").unwrap_or(shape_attr.offset_x);
    shape_attr.offset_y = json_i32(value, "offset_y").unwrap_or(shape_attr.offset_y);
    shape_attr.group_level = json_u16(value, "group_level").unwrap_or(shape_attr.group_level);
    shape_attr.local_file_version =
        json_u16(value, "local_file_version").unwrap_or(shape_attr.local_file_version);
    shape_attr.original_width =
        json_u32(value, "original_width").unwrap_or(shape_attr.original_width);
    shape_attr.original_height =
        json_u32(value, "original_height").unwrap_or(shape_attr.original_height);
    shape_attr.current_width = json_u32(value, "current_width").unwrap_or(shape_attr.current_width);
    shape_attr.current_height =
        json_u32(value, "current_height").unwrap_or(shape_attr.current_height);

    if let Some(flip) = json_u32(value, "flip") {
        shape_attr.flip = flip;
        shape_attr.horz_flip = flip & 0x01 != 0;
        shape_attr.vert_flip = flip & 0x02 != 0;
    }
    if let Some(horz_flip) = json_bool(value, "horz_flip") {
        shape_attr.horz_flip = horz_flip;
        if shape_attr.flip != 0 {
            if horz_flip {
                shape_attr.flip |= 0x01;
            } else {
                shape_attr.flip &= !0x01;
            }
        }
    }
    if let Some(vert_flip) = json_bool(value, "vert_flip") {
        shape_attr.vert_flip = vert_flip;
        if shape_attr.flip != 0 {
            if vert_flip {
                shape_attr.flip |= 0x02;
            } else {
                shape_attr.flip &= !0x02;
            }
        }
    }
    shape_attr.rotation_angle =
        json_i16(value, "rotation_angle").unwrap_or(shape_attr.rotation_angle);
    shape_attr.rotate_image = json_bool(value, "rotate_image").unwrap_or(shape_attr.rotate_image);
    shape_attr.rotation_center.x =
        json_i32(value, "rotation_center_x").unwrap_or(shape_attr.rotation_center.x);
    shape_attr.rotation_center.y =
        json_i32(value, "rotation_center_y").unwrap_or(shape_attr.rotation_center.y);

    shape_attr.render_sx = json_f64(value, "render_sx").unwrap_or(shape_attr.render_sx);
    shape_attr.render_b = json_f64(value, "render_b").unwrap_or(shape_attr.render_b);
    shape_attr.render_tx = json_f64(value, "render_tx").unwrap_or(shape_attr.render_tx);
    shape_attr.render_c = json_f64(value, "render_c").unwrap_or(shape_attr.render_c);
    shape_attr.render_sy = json_f64(value, "render_sy").unwrap_or(shape_attr.render_sy);
    shape_attr.render_ty = json_f64(value, "render_ty").unwrap_or(shape_attr.render_ty);
    Ok(())
}

fn apply_object_layout(
    layout: Option<&Value>,
    shape_attr: &mut ShapeComponentAttr,
) -> Result<(), String> {
    let Some(layout) = layout.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(layout) = layout.as_object() else {
        return Err("object layout must be an object".to_string());
    };
    if let Some(shape_value) = layout.get("shape_attr").filter(|value| !value.is_null()) {
        apply_shape_attr_json(shape_value, shape_attr, "object layout.shape_attr")?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn template_picture(
    core: &mut DocumentCore,
    image_base64: &str,
    external_path: &str,
    extension: &str,
    width: u32,
    height: u32,
    natural_width_px: u32,
    natural_height_px: u32,
    description: &str,
    transparency: u8,
    brightness: i8,
    contrast: i8,
    effect: &str,
    effects: Option<&Value>,
    layout: Option<&Value>,
    object_layout: Option<&Value>,
    treat_as_char: bool,
    horz_offset: u32,
    vert_offset: u32,
    caption: Option<Caption>,
) -> Result<Picture, String> {
    let has_embedded_data = !image_base64.trim().is_empty();
    let external_path = external_path.trim();
    if !has_embedded_data && external_path.is_empty() {
        return Err("template picture requires image_base64 or external_path".to_string());
    }
    let extension = normalized_picture_extension(extension, external_path);
    let width = width.max(1);
    let height = height.max(1);
    let natural_width_px = natural_width_px.max(width.saturating_div(75)).max(1);
    let natural_height_px = natural_height_px.max(height.saturating_div(75)).max(1);

    let bin_data_id = next_template_bin_data_id(core);
    let image_external_path = if has_embedded_data {
        let image_data = base64::engine::general_purpose::STANDARD
            .decode(image_base64)
            .map_err(|e| format!("invalid template picture image_base64: {e}"))?;
        if image_data.is_empty() {
            return Err("template picture image data is empty".to_string());
        }
        core.document.bin_data_content.push(BinDataContent {
            id: bin_data_id,
            data: image_data,
            extension: extension.clone(),
        });
        core.document.doc_info.bin_data_list.push(BinData {
            raw_data: None,
            attr: 0x0101,
            data_type: BinDataType::Embedding,
            compression: BinDataCompression::Default,
            status: BinDataStatus::Success,
            abs_path: None,
            rel_path: None,
            storage_id: bin_data_id,
            extension: Some(extension.clone()),
        });
        None
    } else {
        core.document.doc_info.bin_data_list.push(BinData {
            raw_data: None,
            attr: 0x0000,
            data_type: BinDataType::Link,
            compression: BinDataCompression::Default,
            status: BinDataStatus::NotAccessed,
            abs_path: Some(external_path.to_string()),
            rel_path: None,
            storage_id: bin_data_id,
            extension: Some(extension.clone()),
        });
        Some(external_path.to_string())
    };
    core.document.doc_info.raw_stream = None;
    core.document.doc_info.raw_stream_dirty = true;

    let common_attr = if treat_as_char {
        1 | (4 << 15) | (2 << 18)
    } else {
        (4 << 15) | (2 << 18)
    };
    let mut common = CommonObjAttr {
        ctrl_id: 0x67736F20,
        attr: common_attr,
        treat_as_char,
        vert_rel_to: VertRelTo::Paper,
        horz_rel_to: HorzRelTo::Paper,
        text_wrap: TextWrap::Square,
        horizontal_offset: horz_offset,
        vertical_offset: vert_offset,
        width,
        height,
        z_order: 1,
        description: description.to_string(),
        ..Default::default()
    };
    apply_common_object_layout(&mut common, object_layout)?;
    common.ctrl_id = 0x67736F20;
    let mut shape_attr = ShapeComponentAttr {
        original_width: width,
        original_height: height,
        current_width: width,
        current_height: height,
        local_file_version: 1,
        render_sx: 1.0,
        render_sy: 1.0,
        ..Default::default()
    };
    let mut crop = CropInfo {
        left: 0,
        top: 0,
        right: (natural_width_px * 75) as i32,
        bottom: (natural_height_px * 75) as i32,
    };
    let mut padding = Padding::default();
    let mut border_x = [0, 0, width as i32, 0];
    let mut border_y = [width as i32, height as i32, 0, height as i32];
    apply_picture_layout(
        layout,
        &mut shape_attr,
        &mut crop,
        &mut padding,
        &mut border_x,
        &mut border_y,
    )?;
    let image_attr = ImageAttr {
        bin_data_id,
        brightness,
        contrast,
        effect: picture_effect_from_template(effect),
        transparency: transparency.min(100),
        external_path: image_external_path,
    };
    let effects = picture_effects_from_json(effects)?;

    Ok(Picture {
        common,
        shape_attr,
        border_x,
        border_y,
        crop,
        padding,
        image_attr,
        effects,
        img_dim: (natural_width_px, natural_height_px),
        caption,
        ..Default::default()
    })
}

fn picture_paragraph(picture: Picture) -> Paragraph {
    let mut para = Paragraph::new_empty();
    para.char_count = 9;
    para.control_mask = 0x0000_0800;
    para.controls.push(Control::Picture(Box::new(picture)));
    para.ctrl_data_records.push(None);
    para.has_para_text = true;
    para.raw_header_extra = paragraph_header_extra();
    para
}

fn picture_paragraph_with_line_segments(
    picture: Picture,
    line_segments: &[TemplateLineSeg],
) -> Paragraph {
    let mut para = picture_paragraph(picture);
    apply_template_line_segments(&mut para, line_segments);
    para
}

fn object_placeholder_block(core: &DocumentCore, shape: &ShapeObject) -> Option<TemplateBlock> {
    object_placeholder_block_with_depth(core, shape, 0)
}

fn object_placeholder_block_with_depth(
    core: &DocumentCore,
    shape: &ShapeObject,
    depth: usize,
) -> Option<TemplateBlock> {
    if let ShapeObject::Picture(picture) = shape {
        return picture_block(core, picture);
    }

    let common = shape.common();
    let shape_attr = shape.shape_attr();
    let width = common
        .width
        .max(shape_attr.current_width)
        .max(shape_attr.original_width);
    let height = common
        .height
        .max(shape_attr.current_height)
        .max(shape_attr.original_height);
    if width == 0 || height == 0 {
        return None;
    }

    let (object_kind, shape_kind, description) =
        parse_object_placeholder_description(&common.description).unwrap_or_else(|| {
            (
                object_placeholder_kind(shape).to_string(),
                object_shape_kind(shape).to_string(),
                common.description.clone(),
            )
        });
    let placeholder_text = shape_text_box_text(shape)
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| default_object_placeholder_text(&object_kind, &shape_kind));
    let (ole_bin_data_base64, ole_extension) = object_ole_bin_data(core, shape);
    let children = match shape {
        ShapeObject::Group(group) if depth < 16 => group
            .children
            .iter()
            .filter_map(|child| object_placeholder_block_with_depth(core, child, depth + 1))
            .collect(),
        _ => Vec::new(),
    };

    Some(TemplateBlock::ObjectPlaceholder {
        object_kind,
        break_before: None,
        host_group: None,
        shape_kind,
        description,
        placeholder_text,
        width,
        height,
        treat_as_char: common.treat_as_char,
        horz_offset: common.horizontal_offset,
        vert_offset: common.vertical_offset,
        caption: caption_template(core, shape_caption(shape).as_ref()),
        shape_component_id: shape_attr.ctrl_id,
        geometry: object_geometry_json(shape),
        drawing_style: object_drawing_style_json(core, shape),
        layout: object_layout_json(shape_attr, width, height),
        children,
        raw_hwp_chart_data_base64: object_chart_data_base64(shape),
        raw_hwp_ole_tag_base64: object_ole_tag_base64(shape),
        ole_bin_data_base64,
        ole_extension,
        ole_object_type: match shape {
            ShapeObject::Ole(ole) => ole.hwpx_object_type.clone().unwrap_or_default(),
            _ => String::new(),
        },
        ole_draw_aspect: match shape {
            ShapeObject::Ole(ole) => ole.hwpx_draw_aspect.clone().unwrap_or_default(),
            _ => String::new(),
        },
        ole_eq_base_line: match shape {
            ShapeObject::Ole(ole) => ole.hwpx_eq_base_line.clone().unwrap_or_default(),
            _ => String::new(),
        },
        ole_has_moniker: match shape {
            ShapeObject::Ole(ole) => ole.hwpx_has_moniker.clone().unwrap_or_default(),
            _ => String::new(),
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn template_object_placeholder(
    core: &mut DocumentCore,
    section_idx: usize,
    object_kind: &str,
    shape_kind: &str,
    description: &str,
    placeholder_text: &str,
    width: u32,
    height: u32,
    treat_as_char: bool,
    horz_offset: u32,
    vert_offset: u32,
    caption: Option<&TemplateCaption>,
    shape_component_id: u32,
    geometry: Option<&Value>,
    drawing_style: Option<&Value>,
    layout: Option<&Value>,
    children: &[TemplateBlock],
    raw_hwp_chart_data_base64: &str,
    raw_hwp_ole_tag_base64: &str,
    ole_bin_data_base64: &str,
    ole_extension: &str,
    ole_object_type: &str,
    ole_draw_aspect: &str,
    ole_eq_base_line: &str,
    ole_has_moniker: &str,
) -> Result<ShapeObject, String> {
    let object_kind = if object_kind.trim().is_empty() {
        "shape"
    } else {
        object_kind.trim()
    };
    let shape_kind = shape_kind.trim();
    let placeholder_text = if placeholder_text.trim().is_empty() {
        default_object_placeholder_text(object_kind, shape_kind)
    } else {
        placeholder_text.to_string()
    };
    let width = width.max(3600);
    let height = height.max(1600);
    let caption = template_caption(core, section_idx, caption)?;
    if !raw_hwp_chart_data_base64.is_empty() {
        let raw_chart_data = base64::engine::general_purpose::STANDARD
            .decode(raw_hwp_chart_data_base64)
            .map_err(|e| format!("invalid raw_hwp_chart_data_base64: {e}"))?;
        let mut drawing = template_object_drawing_attr(width, height, shape_component_id);
        apply_drawing_style(core, &mut drawing, drawing_style)?;
        apply_object_layout(layout, &mut drawing.shape_attr)?;
        let mut chart = ChartShape {
            common: template_object_common_attr(
                object_kind,
                shape_kind,
                description,
                width,
                height,
                treat_as_char,
                horz_offset,
                vert_offset,
            ),
            drawing,
            raw_chart_data,
            caption,
            ..Default::default()
        };
        apply_rhwp_chart_data_semantic(&mut chart);
        return Ok(ShapeObject::Chart(Box::new(chart)));
    }

    if !raw_hwp_ole_tag_base64.is_empty() {
        let mut raw_tag_data = base64::engine::general_purpose::STANDARD
            .decode(raw_hwp_ole_tag_base64)
            .map_err(|e| format!("invalid raw_hwp_ole_tag_base64: {e}"))?;
        let mut bin_data_id = ole_tag_bin_data_id(&raw_tag_data);
        if !ole_bin_data_base64.is_empty() {
            let extension = normalized_ole_extension(ole_extension);
            let data = base64::engine::general_purpose::STANDARD
                .decode(ole_bin_data_base64)
                .map_err(|e| format!("invalid ole_bin_data_base64: {e}"))?;
            if !data.is_empty() {
                bin_data_id = push_template_ole_bin_data(core, data, &extension) as u32;
                patch_ole_tag_bin_data_id(&mut raw_tag_data, bin_data_id);
            }
        }
        if raw_tag_data.is_empty() {
            raw_tag_data = template_ole_tag_data(width as i32, height as i32, bin_data_id);
        }
        let (extent_x, extent_y) = ole_tag_extents(&raw_tag_data, width, height);
        return Ok(ShapeObject::Ole(Box::new(OleShape {
            common: template_object_common_attr(
                object_kind,
                shape_kind,
                description,
                width,
                height,
                treat_as_char,
                horz_offset,
                vert_offset,
            ),
            drawing: {
                let mut drawing = template_object_drawing_attr(width, height, SHAPE_OLE_ID);
                apply_drawing_style(core, &mut drawing, drawing_style)?;
                apply_object_layout(layout, &mut drawing.shape_attr)?;
                drawing
            },
            extent_x,
            extent_y,
            bin_data_id,
            hwpx_object_type: string_option(ole_object_type),
            hwpx_draw_aspect: string_option(ole_draw_aspect),
            hwpx_eq_base_line: string_option(ole_eq_base_line),
            hwpx_has_moniker: string_option(ole_has_moniker),
            raw_tag_data,
            caption,
            ..Default::default()
        })));
    }

    if (shape_kind == "Group" || object_kind == "shape_group") && !children.is_empty() {
        let mut shape_attr = template_group_shape_attr(width, height, shape_component_id);
        apply_object_layout(layout, &mut shape_attr)?;
        let raw_component_extra = geometry
            .and_then(|geometry| {
                geometry
                    .get("raw_component_extra_base64")
                    .and_then(Value::as_str)
            })
            .filter(|raw| !raw.is_empty())
            .map(|raw| {
                base64::engine::general_purpose::STANDARD
                    .decode(raw)
                    .map_err(|e| format!("invalid group raw_component_extra_base64: {e}"))
            })
            .transpose()?
            .unwrap_or_default();
        return Ok(ShapeObject::Group(GroupShape {
            common: template_object_common_attr(
                object_kind,
                shape_kind,
                description,
                width,
                height,
                treat_as_char,
                horz_offset,
                vert_offset,
            ),
            shape_attr,
            children: template_group_children(core, section_idx, children)?,
            raw_component_extra,
            caption,
        }));
    }

    if let Some(mut shape) = template_shape_from_geometry(
        core,
        section_idx,
        object_kind,
        shape_kind,
        description,
        &placeholder_text,
        width,
        height,
        treat_as_char,
        horz_offset,
        vert_offset,
        shape_component_id,
        geometry,
        drawing_style,
        layout,
    )? {
        apply_shape_caption(&mut shape, caption);
        return Ok(shape);
    }

    let mut rect = RectangleShape {
        common: template_object_common_attr(
            object_kind,
            shape_kind,
            description,
            width,
            height,
            treat_as_char,
            horz_offset,
            vert_offset,
        ),
        ..Default::default()
    };
    rect.drawing = template_object_drawing_attr(width, height, SHAPE_RECT_ID);
    apply_drawing_style(core, &mut rect.drawing, drawing_style)?;
    apply_object_layout(layout, &mut rect.drawing.shape_attr)?;
    rect.drawing.text_box = Some(TextBox {
        max_width: width,
        paragraphs: vec![formatted_text_paragraph(
            core,
            section_idx,
            &placeholder_text,
            None,
            None,
            &[],
            None,
        )],
        ..Default::default()
    });
    rect.x_coords = [0, width as i32, width as i32, 0];
    rect.y_coords = [0, 0, height as i32, height as i32];
    let mut shape = ShapeObject::Rectangle(rect);
    apply_shape_caption(&mut shape, caption);
    Ok(shape)
}

fn template_group_shape_attr(
    width: u32,
    height: u32,
    shape_component_id: u32,
) -> ShapeComponentAttr {
    ShapeComponentAttr {
        ctrl_id: shape_component_id,
        is_two_ctrl_id: true,
        original_width: width,
        original_height: height,
        current_width: width,
        current_height: height,
        local_file_version: 1,
        render_sx: 1.0,
        render_sy: 1.0,
        ..Default::default()
    }
}

fn template_group_children(
    core: &mut DocumentCore,
    section_idx: usize,
    children: &[TemplateBlock],
) -> Result<Vec<ShapeObject>, String> {
    let mut shapes = Vec::new();
    for child in children {
        match child {
            TemplateBlock::ObjectPlaceholder {
                object_kind,
                break_before: _,
                host_group: _,
                shape_kind,
                description,
                placeholder_text,
                width,
                height,
                treat_as_char,
                horz_offset,
                vert_offset,
                caption,
                shape_component_id,
                geometry,
                drawing_style,
                layout,
                children,
                raw_hwp_chart_data_base64,
                raw_hwp_ole_tag_base64,
                ole_bin_data_base64,
                ole_extension,
                ole_object_type,
                ole_draw_aspect,
                ole_eq_base_line,
                ole_has_moniker,
            } => shapes.push(template_object_placeholder(
                core,
                section_idx,
                object_kind,
                shape_kind,
                description,
                placeholder_text,
                *width,
                *height,
                *treat_as_char,
                *horz_offset,
                *vert_offset,
                caption.as_ref(),
                *shape_component_id,
                geometry.as_ref(),
                drawing_style.as_ref(),
                layout.as_ref(),
                children,
                raw_hwp_chart_data_base64,
                raw_hwp_ole_tag_base64,
                ole_bin_data_base64,
                ole_extension,
                ole_object_type,
                ole_draw_aspect,
                ole_eq_base_line,
                ole_has_moniker,
            )?),
            TemplateBlock::Picture {
                break_before: _,
                host_group: _,
                line_segments: _,
                image_base64,
                external_path,
                extension,
                width,
                height,
                natural_width_px,
                natural_height_px,
                description,
                transparency,
                brightness,
                contrast,
                effect,
                effects,
                layout,
                object_layout,
                treat_as_char,
                horz_offset,
                vert_offset,
                caption,
            } => {
                let caption = template_caption(core, section_idx, caption.as_ref())?;
                shapes.push(ShapeObject::Picture(Box::new(template_picture(
                    core,
                    image_base64,
                    external_path,
                    extension,
                    *width,
                    *height,
                    *natural_width_px,
                    *natural_height_px,
                    description,
                    *transparency,
                    *brightness,
                    *contrast,
                    effect,
                    effects.as_ref(),
                    layout.as_ref(),
                    object_layout.as_ref(),
                    *treat_as_char,
                    *horz_offset,
                    *vert_offset,
                    caption,
                )?)));
            }
            _ => {
                return Err(
                    "group object_placeholder children must be object_placeholder or picture blocks"
                        .to_string(),
                );
            }
        }
    }
    Ok(shapes)
}

#[allow(clippy::too_many_arguments)]
fn template_shape_from_geometry(
    core: &mut DocumentCore,
    section_idx: usize,
    object_kind: &str,
    shape_kind: &str,
    description: &str,
    placeholder_text: &str,
    width: u32,
    height: u32,
    treat_as_char: bool,
    horz_offset: u32,
    vert_offset: u32,
    shape_component_id: u32,
    geometry: Option<&Value>,
    drawing_style: Option<&Value>,
    layout: Option<&Value>,
) -> Result<Option<ShapeObject>, String> {
    let Some(geometry) = geometry else {
        return Ok(None);
    };
    let common = template_object_common_attr(
        object_kind,
        shape_kind,
        description,
        width,
        height,
        treat_as_char,
        horz_offset,
        vert_offset,
    );
    let text_box = template_object_text_box(core, section_idx, width, placeholder_text);
    let ctrl_id = |fallback| {
        if shape_component_id == 0 {
            fallback
        } else {
            shape_component_id
        }
    };

    let mut shape = match shape_kind {
        "Line" => ShapeObject::Line(LineShape {
            common,
            drawing: template_object_drawing_attr_with_text_box(
                width,
                height,
                ctrl_id(SHAPE_LINE_ID),
                text_box,
            ),
            start: geometry_point(geometry, "start").unwrap_or(Point {
                x: 0,
                y: height as i32,
            }),
            end: geometry_point(geometry, "end").unwrap_or(Point {
                x: width as i32,
                y: 0,
            }),
            started_right_or_bottom: geometry
                .get("started_right_or_bottom")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            connector: geometry
                .get("connector")
                .filter(|value| !value.is_null())
                .map(connector_from_json)
                .transpose()?,
            raw_trailing: geometry
                .get("raw_trailing_base64")
                .and_then(Value::as_str)
                .filter(|raw| !raw.is_empty())
                .map(|raw| {
                    base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .map_err(|e| format!("invalid line raw_trailing_base64: {e}"))
                })
                .transpose()?
                .unwrap_or_default(),
        }),
        "Rectangle" => ShapeObject::Rectangle(RectangleShape {
            common,
            drawing: template_object_drawing_attr_with_text_box(
                width,
                height,
                ctrl_id(SHAPE_RECT_ID),
                text_box,
            ),
            round_rate: geometry
                .get("round_rate")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u8::MAX as u64) as u8,
            x_coords: geometry_i32_array4(geometry, "x_coords").unwrap_or([
                0,
                width as i32,
                width as i32,
                0,
            ]),
            y_coords: geometry_i32_array4(geometry, "y_coords").unwrap_or([
                0,
                0,
                height as i32,
                height as i32,
            ]),
            raw_trailing: geometry
                .get("raw_trailing_base64")
                .and_then(Value::as_str)
                .filter(|raw| !raw.is_empty())
                .map(|raw| {
                    base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .map_err(|e| format!("invalid rectangle raw_trailing_base64: {e}"))
                })
                .transpose()?
                .unwrap_or_default(),
        }),
        "Ellipse" => ShapeObject::Ellipse(EllipseShape {
            common,
            drawing: template_object_drawing_attr_with_text_box(
                width,
                height,
                ctrl_id(SHAPE_ELLIPSE_ID),
                text_box,
            ),
            attr: geometry
                .get("attr")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u32::MAX as u64) as u32,
            center: geometry_point(geometry, "center").unwrap_or(Point {
                x: width as i32 / 2,
                y: height as i32 / 2,
            }),
            axis1: geometry_point(geometry, "axis1").unwrap_or(Point {
                x: width as i32,
                y: height as i32 / 2,
            }),
            axis2: geometry_point(geometry, "axis2").unwrap_or(Point {
                x: width as i32 / 2,
                y: 0,
            }),
            start1: geometry_point(geometry, "start1").unwrap_or_default(),
            end1: geometry_point(geometry, "end1").unwrap_or_default(),
            start2: geometry_point(geometry, "start2").unwrap_or_default(),
            end2: geometry_point(geometry, "end2").unwrap_or_default(),
            raw_trailing: geometry
                .get("raw_trailing_base64")
                .and_then(Value::as_str)
                .filter(|raw| !raw.is_empty())
                .map(|raw| {
                    base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .map_err(|e| format!("invalid ellipse raw_trailing_base64: {e}"))
                })
                .transpose()?
                .unwrap_or_default(),
        }),
        "Arc" => ShapeObject::Arc(ArcShape {
            common,
            drawing: template_object_drawing_attr_with_text_box(
                width,
                height,
                ctrl_id(SHAPE_ARC_ID),
                text_box,
            ),
            arc_type: geometry
                .get("arc_type")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u8::MAX as u64) as u8,
            center: geometry_point(geometry, "center").unwrap_or(Point {
                x: width as i32 / 2,
                y: height as i32 / 2,
            }),
            axis1: geometry_point(geometry, "axis1").unwrap_or(Point {
                x: width as i32,
                y: height as i32 / 2,
            }),
            axis2: geometry_point(geometry, "axis2").unwrap_or(Point {
                x: width as i32 / 2,
                y: 0,
            }),
            raw_trailing: geometry
                .get("raw_trailing_base64")
                .and_then(Value::as_str)
                .filter(|raw| !raw.is_empty())
                .map(|raw| {
                    base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .map_err(|e| format!("invalid arc raw_trailing_base64: {e}"))
                })
                .transpose()?
                .unwrap_or_default(),
        }),
        "Polygon" => ShapeObject::Polygon(PolygonShape {
            common,
            drawing: template_object_drawing_attr_with_text_box(
                width,
                height,
                ctrl_id(SHAPE_POLYGON_ID),
                text_box,
            ),
            points: geometry_points(geometry, "points").unwrap_or_else(|| {
                vec![
                    Point { x: 0, y: 0 },
                    Point {
                        x: width as i32,
                        y: 0,
                    },
                    Point {
                        x: width as i32,
                        y: height as i32,
                    },
                ]
            }),
            raw_trailing: geometry
                .get("raw_trailing_base64")
                .and_then(Value::as_str)
                .filter(|raw| !raw.is_empty())
                .map(|raw| {
                    base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .map_err(|e| format!("invalid polygon raw_trailing_base64: {e}"))
                })
                .transpose()?
                .unwrap_or_default(),
        }),
        "Curve" => ShapeObject::Curve(CurveShape {
            common,
            drawing: template_object_drawing_attr_with_text_box(
                width,
                height,
                ctrl_id(SHAPE_CURVE_ID),
                text_box,
            ),
            points: geometry_points(geometry, "points").unwrap_or_else(|| {
                vec![
                    Point { x: 0, y: 0 },
                    Point {
                        x: width as i32,
                        y: height as i32,
                    },
                ]
            }),
            segment_types: geometry_u8_vec(geometry, "segment_types").unwrap_or_default(),
            raw_trailing: geometry
                .get("raw_trailing_base64")
                .and_then(Value::as_str)
                .filter(|raw| !raw.is_empty())
                .map(|raw| {
                    base64::engine::general_purpose::STANDARD
                        .decode(raw)
                        .map_err(|e| format!("invalid curve raw_trailing_base64: {e}"))
                })
                .transpose()?
                .unwrap_or_default(),
        }),
        _ => return Ok(None),
    };
    if let Some(drawing) = shape.drawing_mut() {
        apply_drawing_style(core, drawing, drawing_style)?;
        apply_object_layout(layout, &mut drawing.shape_attr)?;
    }
    if let ShapeObject::Line(line) = &mut shape {
        if line.connector.is_some() {
            line.drawing.shape_attr.ctrl_id = SHAPE_CONNECTOR_ID;
        }
    }
    Ok(Some(shape))
}

fn template_object_common_attr(
    object_kind: &str,
    shape_kind: &str,
    description: &str,
    width: u32,
    height: u32,
    treat_as_char: bool,
    horz_offset: u32,
    vert_offset: u32,
) -> CommonObjAttr {
    let common_attr = if treat_as_char {
        1 | (4 << 15) | (2 << 18)
    } else {
        (4 << 15) | (2 << 18)
    };
    CommonObjAttr {
        ctrl_id: CTRL_GEN_SHAPE,
        attr: common_attr,
        treat_as_char,
        vert_rel_to: VertRelTo::Paper,
        horz_rel_to: HorzRelTo::Paper,
        text_wrap: TextWrap::Square,
        horizontal_offset: horz_offset,
        vertical_offset: vert_offset,
        width,
        height,
        z_order: 1,
        description: object_placeholder_description(object_kind, shape_kind, description),
        ..Default::default()
    }
}

fn template_object_drawing_attr(width: u32, height: u32, ctrl_id: u32) -> DrawingObjAttr {
    template_object_drawing_attr_with_text_box(width, height, ctrl_id, None)
}

fn template_object_drawing_attr_with_text_box(
    width: u32,
    height: u32,
    ctrl_id: u32,
    text_box: Option<TextBox>,
) -> DrawingObjAttr {
    DrawingObjAttr {
        shape_attr: ShapeComponentAttr {
            ctrl_id,
            is_two_ctrl_id: true,
            original_width: width,
            original_height: height,
            current_width: width,
            current_height: height,
            local_file_version: 1,
            render_sx: 1.0,
            render_sy: 1.0,
            ..Default::default()
        },
        text_box,
        ..Default::default()
    }
}

fn template_object_text_box(
    core: &mut DocumentCore,
    section_idx: usize,
    width: u32,
    text: &str,
) -> Option<TextBox> {
    (!text.trim().is_empty()).then(|| TextBox {
        max_width: width,
        paragraphs: vec![formatted_text_paragraph(
            core,
            section_idx,
            text,
            None,
            None,
            &[],
            None,
        )],
        ..Default::default()
    })
}

fn object_drawing_style_json(core: &DocumentCore, shape: &ShapeObject) -> Option<Value> {
    shape
        .drawing()
        .and_then(|drawing| drawing_style_json(core, drawing))
}

fn drawing_style_json(core: &DocumentCore, drawing: &DrawingObjAttr) -> Option<Value> {
    if drawing_style_is_default(drawing) {
        return None;
    }
    let mut value = json!({
        "border_line": {
            "color": drawing.border_line.color,
            "color_hex": color_ref_to_hex(drawing.border_line.color),
            "width": drawing.border_line.width,
            "attr": drawing.border_line.attr,
            "outline_style": drawing.border_line.outline_style,
        },
        "fill": fill_json(core, &drawing.fill),
        "shadow": {
            "type": drawing.shadow_type,
            "color": drawing.shadow_color,
            "color_hex": color_ref_to_hex(drawing.shadow_color),
            "offset_x": drawing.shadow_offset_x,
            "offset_y": drawing.shadow_offset_y,
            "alpha": drawing.shadow_alpha,
        }
    });
    if !drawing.raw_hwpx_child_xml.is_empty() {
        value["raw_hwpx_child_xml"] = json!(drawing.raw_hwpx_child_xml);
    }
    if let Some(effects) = drawing_style_effects_json(&drawing.raw_hwpx_child_xml) {
        value["effects"] = effects;
    }
    Some(value)
}

fn drawing_style_is_default(drawing: &DrawingObjAttr) -> bool {
    drawing.border_line.color == 0
        && drawing.border_line.width == 0
        && drawing.border_line.attr == 0
        && drawing.border_line.outline_style == 0
        && fill_is_default(&drawing.fill)
        && drawing.shadow_type == 0
        && drawing.shadow_color == 0
        && drawing.shadow_offset_x == 0
        && drawing.shadow_offset_y == 0
        && drawing.shadow_alpha == 0
        && drawing.raw_hwpx_child_xml.is_empty()
}

fn fill_is_default(fill: &Fill) -> bool {
    matches!(fill.fill_type, FillType::None)
        && fill.solid.is_none()
        && fill.gradient.is_none()
        && fill.image.is_none()
        && fill.alpha == 0
}

fn fill_json(core: &DocumentCore, fill: &Fill) -> Value {
    json!({
        "type": fill_type_name(fill.fill_type),
        "alpha": fill.alpha,
        "solid": fill.solid.map(|solid| json!({
            "background_color": solid.background_color,
            "background_color_hex": color_ref_to_hex(solid.background_color),
            "pattern_color": solid.pattern_color,
            "pattern_color_hex": color_ref_to_hex(solid.pattern_color),
            "pattern_type": solid.pattern_type,
        })),
        "gradient": fill.gradient.as_ref().map(|gradient| json!({
            "gradient_type": gradient.gradient_type,
            "angle": gradient.angle,
            "center_x": gradient.center_x,
            "center_y": gradient.center_y,
            "blur": gradient.blur,
            "step_center": gradient.step_center,
            "colors": gradient.colors,
            "colors_hex": gradient.colors.iter().map(|color| color_ref_to_hex(*color)).collect::<Vec<_>>(),
            "positions": gradient.positions,
            "positions_percent": gradient.positions,
        })),
        "image": fill.image.as_ref().map(|image| image_fill_json(core, image)),
    })
}

fn image_fill_json(core: &DocumentCore, image: &ImageFill) -> Value {
    let content = picture_bin_data_content(core, image.bin_data_id);
    let external_path = bin_data_link_path(core, image.bin_data_id).unwrap_or_default();
    let image_base64 = content
        .filter(|content| !content.data.is_empty())
        .map(|content| base64::engine::general_purpose::STANDARD.encode(&content.data))
        .unwrap_or_default();
    let extension = content
        .map(|content| content.extension.clone())
        .filter(|extension| !extension.trim().is_empty())
        .or_else(|| picture_extension_from_path(&external_path))
        .unwrap_or_default();

    json!({
        "fill_mode": image_fill_mode_name(image.fill_mode),
        "brightness": image.brightness,
        "contrast": image.contrast,
        "effect": image.effect,
        "bin_data_id": image.bin_data_id,
        "image_base64": image_base64,
        "external_path": external_path,
        "extension": extension,
    })
}

fn border_fill_from_json(core: &mut DocumentCore, value: &Value) -> Result<BorderFill, String> {
    let mut attr = json_u16(value, "attr").unwrap_or(0);
    let three_d = json_bool(value, "three_d")
        .or_else(|| json_bool(value, "threeD"))
        .unwrap_or((attr & 0x0001) != 0);
    let shadow = json_bool(value, "shadow").unwrap_or((attr & 0x0002) != 0);
    let center_line = json_string_alias(value, &["center_line", "centerLine"])
        .filter(|value| !value.trim().is_empty());
    let break_cell_separate_line = json_bool(value, "break_cell_separate_line")
        .or_else(|| json_bool(value, "breakCellSeparateLine"))
        .unwrap_or(false);
    if three_d {
        attr |= 0x0001;
    }
    if shadow {
        attr |= 0x0002;
    }
    if center_line
        .as_deref()
        .map(|value| !value.eq_ignore_ascii_case("NONE"))
        .unwrap_or(false)
    {
        attr |= 0x2000;
    }
    let mut border_fill = BorderFill {
        raw_data: None,
        raw_hwpx_children: None,
        attr,
        three_d,
        shadow,
        center_line,
        break_cell_separate_line,
        borders: [
            border_line_from_json(border_line_value(value, "left", 0)),
            border_line_from_json(border_line_value(value, "right", 1)),
            border_line_from_json(border_line_value(value, "top", 2)),
            border_line_from_json(border_line_value(value, "bottom", 3)),
        ],
        diagonal: DiagonalLine {
            diagonal_type: value
                .get("diagonal")
                .and_then(|diagonal| json_u8(diagonal, "diagonal_type"))
                .unwrap_or(0),
            width: value
                .get("diagonal")
                .and_then(|diagonal| json_u8(diagonal, "width"))
                .unwrap_or(0),
            color: value
                .get("diagonal")
                .and_then(|diagonal| {
                    json_color_ref_alias(diagonal, "color", &["color_hex", "colorHex"])
                })
                .unwrap_or(0),
        },
        fill: value
            .get("fill")
            .map(|fill| fill_from_json(core, fill))
            .transpose()?
            .unwrap_or_default(),
    };

    if border_fill.fill.image.is_none() {
        if let Some(raw) = value
            .get("raw_hwp_border_fill_base64")
            .and_then(Value::as_str)
            .filter(|raw| !raw.trim().is_empty())
        {
            border_fill.raw_data = Some(
                base64::engine::general_purpose::STANDARD
                    .decode(raw)
                    .map_err(|e| format!("invalid raw_hwp_border_fill_base64: {e}"))?,
            );
        }
    }

    Ok(border_fill)
}

fn border_line_value<'a>(value: &'a Value, key: &str, index: usize) -> Option<&'a Value> {
    value
        .get("borders")
        .and_then(|borders| borders.get(key).or_else(|| borders.get(index)))
}

fn border_line_from_json(value: Option<&Value>) -> BorderLine {
    let Some(value) = value else {
        return BorderLine::default();
    };
    let line_type = value
        .get("type")
        .and_then(Value::as_str)
        .map(border_line_type_from_name)
        .or_else(|| json_u8(value, "type").map(border_line_type_from_code))
        .unwrap_or_default();
    BorderLine {
        line_type,
        width: json_u8(value, "width").unwrap_or(0),
        color: json_color_ref_alias(value, "color", &["color_hex", "colorHex"]).unwrap_or(0),
    }
}

fn fill_type_name(fill_type: FillType) -> &'static str {
    match fill_type {
        FillType::None => "none",
        FillType::Solid => "solid",
        FillType::Image => "image",
        FillType::Gradient => "gradient",
    }
}

fn border_line_type_name(line_type: BorderLineType) -> &'static str {
    match line_type {
        BorderLineType::None => "none",
        BorderLineType::Solid => "solid",
        BorderLineType::Dash => "dash",
        BorderLineType::Dot => "dot",
        BorderLineType::DashDot => "dash_dot",
        BorderLineType::DashDotDot => "dash_dot_dot",
        BorderLineType::LongDash => "long_dash",
        BorderLineType::Circle => "circle",
        BorderLineType::Double => "double",
        BorderLineType::ThinThickDouble => "thin_thick_double",
        BorderLineType::ThickThinDouble => "thick_thin_double",
        BorderLineType::ThinThickThinTriple => "thin_thick_thin_triple",
        BorderLineType::Wave => "wave",
        BorderLineType::DoubleWave => "double_wave",
        BorderLineType::Thick3D => "thick_3d",
        BorderLineType::Thick3DReverse => "thick_3d_reverse",
        BorderLineType::Thin3D => "thin_3d",
        BorderLineType::Thin3DReverse => "thin_3d_reverse",
    }
}

fn image_fill_mode_name(mode: ImageFillMode) -> &'static str {
    match mode {
        ImageFillMode::TileAll => "tile_all",
        ImageFillMode::TileHorzTop => "tile_horz_top",
        ImageFillMode::TileHorzBottom => "tile_horz_bottom",
        ImageFillMode::TileVertLeft => "tile_vert_left",
        ImageFillMode::TileVertRight => "tile_vert_right",
        ImageFillMode::FitToSize => "fit_to_size",
        ImageFillMode::Center => "center",
        ImageFillMode::CenterTop => "center_top",
        ImageFillMode::CenterBottom => "center_bottom",
        ImageFillMode::LeftCenter => "left_center",
        ImageFillMode::LeftTop => "left_top",
        ImageFillMode::LeftBottom => "left_bottom",
        ImageFillMode::RightCenter => "right_center",
        ImageFillMode::RightTop => "right_top",
        ImageFillMode::RightBottom => "right_bottom",
        ImageFillMode::None => "none",
    }
}

fn apply_drawing_style(
    core: &mut DocumentCore,
    drawing: &mut DrawingObjAttr,
    style: Option<&Value>,
) -> Result<(), String> {
    let Some(style) = style else {
        return Ok(());
    };
    if let Some(border_line) = style
        .get("border_line")
        .or_else(|| style.get("borderLine"))
        .or_else(|| drawing_style_has_flat_border_aliases(style).then_some(style))
    {
        drawing.border_line = ShapeBorderLine {
            color: json_u32_alias(border_line, "color", &["borderColor"])
                .or_else(|| {
                    json_color_ref_alias(
                        border_line,
                        "color",
                        &["color_hex", "colorHex", "borderColorHex"],
                    )
                })
                .unwrap_or(drawing.border_line.color),
            width: json_i32_alias(border_line, "width", &["borderWidth"])
                .unwrap_or(drawing.border_line.width),
            attr: json_u32_alias(border_line, "attr", &["borderAttr"])
                .unwrap_or(drawing.border_line.attr),
            outline_style: json_u8_alias(
                border_line,
                "outline_style",
                &["outlineStyle", "borderOutlineStyle"],
            )
            .unwrap_or(drawing.border_line.outline_style),
        };
    }
    if let Some(fill) = style
        .get("fill")
        .or_else(|| drawing_style_has_flat_fill_aliases(style).then_some(style))
    {
        drawing.fill = fill_from_json(core, fill)?;
    }
    if let Some(shadow) = style
        .get("shadow")
        .or_else(|| drawing_style_has_flat_shadow_aliases(style).then_some(style))
    {
        if let Some(shadow_type) = json_u32_alias(shadow, "type", &["shadowType"]) {
            drawing.shadow_type = shadow_type;
        } else if let Some(type_name) =
            json_string_alias(shadow, &["type", "typeName", "type_name"])
        {
            if let Some(shadow_type) = shape_shadow_type_from_name(&type_name) {
                drawing.shadow_type = shadow_type;
            }
        }
        drawing.shadow_color = json_u32_alias(shadow, "color", &["shadowColor"])
            .or_else(|| {
                json_color_ref_alias(
                    shadow,
                    "color",
                    &["color_hex", "colorHex", "shadowColorHex"],
                )
            })
            .unwrap_or(drawing.shadow_color);
        drawing.shadow_offset_x = json_i32_alias(shadow, "offset_x", &["offsetX", "shadowOffsetX"])
            .unwrap_or(drawing.shadow_offset_x);
        drawing.shadow_offset_y = json_i32_alias(shadow, "offset_y", &["offsetY", "shadowOffsetY"])
            .unwrap_or(drawing.shadow_offset_y);
        drawing.shadow_alpha =
            json_u8_alias(shadow, "alpha", &["shadowAlpha"]).unwrap_or(drawing.shadow_alpha);
    }
    if let Some(raw_xml) = style
        .get("raw_hwpx_child_xml")
        .or_else(|| style.get("rawHwpxChildXml"))
        .or_else(|| style.get("shapeRawXml"))
    {
        drawing.raw_hwpx_child_xml =
            json_string_array_value(raw_xml, "drawing_style.raw_hwpx_child_xml")?;
    }
    if let Some(effects) = style.get("effects").filter(|value| !value.is_null()) {
        apply_drawing_style_effects(drawing, effects)?;
    }
    Ok(())
}

fn apply_drawing_style_effects(
    drawing: &mut DrawingObjAttr,
    effects: &Value,
) -> Result<(), String> {
    let Some(_) = effects.as_object() else {
        return Err("drawing_style.effects must be an object".to_string());
    };
    let specs = [
        (
            "threeD",
            b"threeD".as_slice(),
            &["threeD", "three_d"] as &[&str],
        ),
        (
            "shadow",
            b"shadow".as_slice(),
            &["shadow", "effect_shadow", "effectShadow"] as &[&str],
        ),
        ("glow", b"glow".as_slice(), &["glow"] as &[&str]),
        (
            "softEdge",
            b"softEdge".as_slice(),
            &["soft_edge", "softEdge"] as &[&str],
        ),
        (
            "reflection",
            b"reflection".as_slice(),
            &["reflection"] as &[&str],
        ),
        ("blur", b"blur".as_slice(), &["blur"] as &[&str]),
        (
            "fillOverlay",
            b"fillOverlay".as_slice(),
            &["fill_overlay", "fillOverlay"] as &[&str],
        ),
    ];
    for (xml_name, effect_name, keys) in specs {
        let Some(value) = keys.iter().find_map(|key| effects.get(*key)) else {
            continue;
        };
        drawing
            .raw_hwpx_child_xml
            .retain(|raw| !drawing_style_raw_fragment_contains_effect(raw, effect_name));
        if value.is_null() {
            continue;
        }
        if let Some(raw_xml) = drawing_style_effect_raw_xml(xml_name, value)? {
            drawing.raw_hwpx_child_xml.push(raw_xml);
        }
    }
    Ok(())
}

fn drawing_style_effects_json(raw_fragments: &[String]) -> Option<Value> {
    let mut map = Map::new();
    for (json_key, effect_name) in [
        ("threeD", b"threeD".as_slice()),
        ("shadow", b"shadow".as_slice()),
        ("glow", b"glow".as_slice()),
        ("softEdge", b"softEdge".as_slice()),
        ("reflection", b"reflection".as_slice()),
        ("blur", b"blur".as_slice()),
        ("fillOverlay", b"fillOverlay".as_slice()),
    ] {
        if let Some(value) = raw_fragments
            .iter()
            .find_map(|raw| drawing_style_effect_json_from_raw_fragment(raw, effect_name))
        {
            map.insert(json_key.to_string(), value);
        }
    }
    (!map.is_empty()).then_some(Value::Object(map))
}

fn drawing_style_effect_json_from_raw_fragment(raw: &str, effect_name: &[u8]) -> Option<Value> {
    let mut reader = quick_xml::Reader::from_str(raw);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e))
                if drawing_style_xml_local_name(e.name().as_ref()) == effect_name =>
            {
                let start = e.to_owned();
                return Some(drawing_style_effect_json_from_start(
                    &start,
                    Some(&mut reader),
                    effect_name,
                ));
            }
            Ok(quick_xml::events::Event::Empty(ref e))
                if drawing_style_xml_local_name(e.name().as_ref()) == effect_name =>
            {
                return Some(drawing_style_effect_json_from_start(e, None, effect_name));
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

fn drawing_style_effect_json_from_start(
    start: &quick_xml::events::BytesStart<'_>,
    reader: Option<&mut quick_xml::Reader<&[u8]>>,
    effect_name: &[u8],
) -> Value {
    let mut map = drawing_style_xml_attrs_value(start);
    let Some(reader) = reader else {
        return Value::Object(map);
    };

    let mut raw_child_xml: Vec<String> = Vec::new();
    let mut depth = 1usize;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                if depth == 1 {
                    match drawing_style_xml_local_name(e.name().as_ref()) {
                        b"bevel" if effect_name == b"threeD" => {
                            map.insert(
                                "bevel".to_string(),
                                Value::Object(drawing_style_xml_attrs_value(e)),
                            );
                            depth += 1;
                        }
                        b"effectsColor" if effect_name != b"threeD" => {
                            let start = e.to_owned();
                            map.insert(
                                "color".to_string(),
                                drawing_style_effect_color_json_from_start(&start, Some(reader)),
                            );
                        }
                        b"solidFill" if effect_name != b"threeD" => {
                            let start = e.to_owned();
                            map.insert(
                                "solid_fill".to_string(),
                                drawing_style_solid_fill_json_from_start(&start, Some(reader)),
                            );
                        }
                        b"skew" | b"scale" | b"alpha" | b"pos" if effect_name != b"threeD" => {
                            let key = String::from_utf8_lossy(drawing_style_xml_local_name(
                                e.name().as_ref(),
                            ))
                            .to_string();
                            map.insert(key, Value::Object(drawing_style_xml_attrs_value(e)));
                            depth += 1;
                        }
                        _ => {
                            let start = e.to_owned();
                            if let Some(raw) = drawing_style_capture_raw_xml_element(&start, reader)
                            {
                                raw_child_xml.push(raw);
                            }
                        }
                    }
                } else {
                    depth += 1;
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                if depth == 1 {
                    match drawing_style_xml_local_name(e.name().as_ref()) {
                        b"bevel" if effect_name == b"threeD" => {
                            map.insert(
                                "bevel".to_string(),
                                Value::Object(drawing_style_xml_attrs_value(e)),
                            );
                        }
                        b"effectsColor" if effect_name != b"threeD" => {
                            map.insert(
                                "color".to_string(),
                                drawing_style_effect_color_json_from_start(e, None),
                            );
                        }
                        b"solidFill" if effect_name != b"threeD" => {
                            map.insert(
                                "solid_fill".to_string(),
                                drawing_style_solid_fill_json_from_start(e, None),
                            );
                        }
                        b"skew" | b"scale" | b"alpha" | b"pos" if effect_name != b"threeD" => {
                            let key = String::from_utf8_lossy(drawing_style_xml_local_name(
                                e.name().as_ref(),
                            ))
                            .to_string();
                            map.insert(key, Value::Object(drawing_style_xml_attrs_value(e)));
                        }
                        _ => {
                            if let Some(raw) = drawing_style_raw_empty_xml_element(e) {
                                raw_child_xml.push(raw);
                            }
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::End(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    if !raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(raw_child_xml.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(map)
}

fn drawing_style_write_raw_xml_event<'a>(
    writer: &mut quick_xml::Writer<&mut Vec<u8>>,
    event: quick_xml::events::Event<'a>,
) -> Option<()> {
    writer.write_event(event).ok()
}

fn drawing_style_raw_xml_string(raw: Vec<u8>) -> Option<String> {
    String::from_utf8(raw).ok()
}

fn drawing_style_raw_empty_xml_element(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    let mut raw = Vec::new();
    let mut writer = quick_xml::Writer::new(&mut raw);
    drawing_style_write_raw_xml_event(&mut writer, quick_xml::events::Event::Empty(e.to_owned()))?;
    drawing_style_raw_xml_string(raw)
}

fn drawing_style_capture_raw_xml_element(
    start: &quick_xml::events::BytesStart<'_>,
    reader: &mut quick_xml::Reader<&[u8]>,
) -> Option<String> {
    let mut raw = Vec::new();
    let mut writer = quick_xml::Writer::new(&mut raw);
    drawing_style_write_raw_xml_event(
        &mut writer,
        quick_xml::events::Event::Start(start.to_owned()),
    )?;

    let mut depth = 1usize;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                depth += 1;
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::Start(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::End(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::End(e.to_owned()),
                )?;
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::Empty(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::Text(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::Text(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::CData(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::CData(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::Comment(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::Comment(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::PI(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::PI(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::DocType(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::DocType(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::GeneralRef(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::GeneralRef(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::Decl(ref e)) => {
                drawing_style_write_raw_xml_event(
                    &mut writer,
                    quick_xml::events::Event::Decl(e.to_owned()),
                )?;
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
        }
        buf.clear();
    }

    drawing_style_raw_xml_string(raw)
}

fn drawing_style_effect_color_json_from_start(
    start: &quick_xml::events::BytesStart<'_>,
    reader: Option<&mut quick_xml::Reader<&[u8]>>,
) -> Value {
    let mut map = drawing_style_xml_attrs_value(start);
    let Some(reader) = reader else {
        return Value::Object(map);
    };
    let mut raw_child_xml: Vec<String> = Vec::new();
    let mut depth = 1usize;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                if depth == 1 {
                    let start = e.to_owned();
                    if drawing_style_xml_local_name(e.name().as_ref()) == b"rgb" {
                        let rgb = drawing_style_xml_attrs_value(e);
                        if let Some(color_hex) = drawing_style_rgb_color_hex(&rgb) {
                            map.insert("color_hex".to_string(), Value::String(color_hex));
                        }
                        map.insert("rgb".to_string(), Value::Object(rgb));
                        let _ = drawing_style_capture_raw_xml_element(&start, reader);
                    } else if let Some(raw) = drawing_style_capture_raw_xml_element(&start, reader)
                    {
                        raw_child_xml.push(raw);
                    }
                } else {
                    depth += 1;
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                if depth == 1 {
                    if drawing_style_xml_local_name(e.name().as_ref()) == b"rgb" {
                        let rgb = drawing_style_xml_attrs_value(e);
                        if let Some(color_hex) = drawing_style_rgb_color_hex(&rgb) {
                            map.insert("color_hex".to_string(), Value::String(color_hex));
                        }
                        map.insert("rgb".to_string(), Value::Object(rgb));
                    } else if let Some(raw) = drawing_style_raw_empty_xml_element(e) {
                        raw_child_xml.push(raw);
                    }
                }
            }
            Ok(quick_xml::events::Event::End(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    if !raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(raw_child_xml.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(map)
}

fn drawing_style_solid_fill_json_from_start(
    start: &quick_xml::events::BytesStart<'_>,
    reader: Option<&mut quick_xml::Reader<&[u8]>>,
) -> Value {
    let mut map = drawing_style_xml_attrs_value(start);
    if let Some(Value::String(color)) = map.get("color") {
        if let Some(color_hex) = drawing_style_normalize_xml_color_hex(color) {
            map.insert("color_hex".to_string(), Value::String(color_hex));
        }
    }
    let Some(reader) = reader else {
        return Value::Object(map);
    };

    let mut raw_child_xml: Vec<String> = Vec::new();
    let mut depth = 1usize;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(ref e)) => {
                if depth == 1 {
                    match drawing_style_xml_local_name(e.name().as_ref()) {
                        b"effectsColor" => {
                            let start = e.to_owned();
                            map.insert(
                                "color".to_string(),
                                drawing_style_effect_color_json_from_start(&start, Some(reader)),
                            );
                        }
                        _ => {
                            let start = e.to_owned();
                            if let Some(raw) = drawing_style_capture_raw_xml_element(&start, reader)
                            {
                                raw_child_xml.push(raw);
                            }
                        }
                    }
                } else {
                    depth += 1;
                }
            }
            Ok(quick_xml::events::Event::Empty(ref e)) => {
                if depth == 1 {
                    match drawing_style_xml_local_name(e.name().as_ref()) {
                        b"effectsColor" => {
                            map.insert(
                                "color".to_string(),
                                drawing_style_effect_color_json_from_start(e, None),
                            );
                        }
                        _ => {
                            if let Some(raw) = drawing_style_raw_empty_xml_element(e) {
                                raw_child_xml.push(raw);
                            }
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::End(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    if !raw_child_xml.is_empty() {
        map.insert(
            "raw_child_xml".to_string(),
            Value::Array(raw_child_xml.into_iter().map(Value::String).collect()),
        );
    }
    Value::Object(map)
}

fn drawing_style_normalize_xml_color_hex(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Some(format!("#{}", hex.to_ascii_uppercase()))
    } else {
        None
    }
}

fn drawing_style_effect_raw_xml(
    effect_name: &str,
    value: &Value,
) -> Result<Option<String>, String> {
    if !value.is_object() {
        return Err(format!(
            "drawing_style.effects.{effect_name} must be an object or null"
        ));
    }
    let child = match effect_name {
        "threeD" => drawing_style_three_d_effect_xml(value),
        "shadow" | "glow" | "softEdge" | "reflection" | "blur" | "fillOverlay" => {
            drawing_style_simple_effect_xml(value, effect_name)
        }
        _ => None,
    };
    Ok(child.map(|child| format!("<hp:effects>{child}</hp:effects>")))
}

fn drawing_style_three_d_effect_xml(value: &Value) -> Option<String> {
    let attrs = drawing_style_xml_attrs(
        value,
        &[
            "bevel",
            "bevelType",
            "bevel_type",
            "raw_child_xml",
            "rawChildXml",
            "raw_children_xml",
            "rawChildrenXml",
        ],
    );
    let bevel_xml = value
        .get("bevel")
        .and_then(|bevel| {
            if bevel.is_null() {
                None
            } else if let Some(bevel_type) = bevel.as_str() {
                Some(format!(
                    "<hp:bevel type=\"{}\"/>",
                    drawing_style_xml_attr_escape(bevel_type)
                ))
            } else if bevel.is_object() {
                Some(format!(
                    "<hp:bevel{}/>",
                    drawing_style_xml_attrs(bevel, &[])
                ))
            } else {
                None
            }
        })
        .or_else(|| {
            json_string_alias(value, &["bevelType", "bevel_type"]).map(|bevel_type| {
                format!(
                    "<hp:bevel type=\"{}\"/>",
                    drawing_style_xml_attr_escape(&bevel_type)
                )
            })
        });
    let mut children = Vec::new();
    if let Some(bevel_xml) = bevel_xml {
        children.push(bevel_xml);
    }
    children.extend(drawing_style_raw_child_xml_values(value));

    Some(if children.is_empty() {
        format!("<hp:threeD{attrs}/>")
    } else {
        format!("<hp:threeD{attrs}>{}</hp:threeD>", children.join(""))
    })
}

fn drawing_style_simple_effect_xml(value: &Value, effect_name: &str) -> Option<String> {
    let mut skip = match effect_name {
        "shadow" => vec![
            "color",
            "effectsColor",
            "skew",
            "skew_x",
            "skewX",
            "skew_y",
            "skewY",
            "scale",
            "scale_x",
            "scaleX",
            "scale_y",
            "scaleY",
        ],
        "reflection" => vec![
            "color",
            "effectsColor",
            "skew",
            "skew_x",
            "skewX",
            "skew_y",
            "skewY",
            "scale",
            "scale_x",
            "scaleX",
            "scale_y",
            "scaleY",
            "alpha",
            "alpha_start",
            "alphaStart",
            "alpha_end",
            "alphaEnd",
            "pos",
            "pos_start",
            "posStart",
            "pos_end",
            "posEnd",
        ],
        _ => vec!["color", "effectsColor"],
    };
    skip.extend([
        "raw_child_xml",
        "rawChildXml",
        "raw_children_xml",
        "rawChildrenXml",
        "solid_fill",
        "solidFill",
    ]);
    let attrs = drawing_style_xml_attrs(value, &skip);
    let mut children = Vec::new();
    if let Some(color_xml) = value
        .get("color")
        .or_else(|| value.get("effectsColor"))
        .and_then(drawing_style_effect_color_xml)
    {
        children.push(color_xml);
    }
    for child_name in ["skew", "scale", "alpha", "pos"] {
        if let Some(child) = value.get(child_name).filter(|child| child.is_object()) {
            children.push(format!(
                "<hp:{child_name}{}/>",
                drawing_style_xml_attrs(child, &[])
            ));
        } else if let Some(attrs) = drawing_style_flat_effect_child_attrs(value, child_name) {
            children.push(format!("<hp:{child_name}{attrs}/>"));
        }
    }
    if let Some(solid_fill_xml) = value
        .get("solid_fill")
        .or_else(|| value.get("solidFill"))
        .and_then(drawing_style_solid_fill_xml)
    {
        children.push(solid_fill_xml);
    }
    children.extend(drawing_style_raw_child_xml_values(value));
    Some(if children.is_empty() {
        format!("<hp:{effect_name}{attrs}/>")
    } else {
        format!(
            "<hp:{effect_name}{attrs}>{}</hp:{effect_name}>",
            children.join("")
        )
    })
}

fn drawing_style_flat_effect_child_attrs(value: &Value, child_name: &str) -> Option<String> {
    let pairs: &[(&str, &[&str])] = match child_name {
        "skew" => &[("x", &["skew_x", "skewX"]), ("y", &["skew_y", "skewY"])],
        "scale" => &[("x", &["scale_x", "scaleX"]), ("y", &["scale_y", "scaleY"])],
        "alpha" => &[
            ("start", &["alpha_start", "alphaStart"]),
            ("end", &["alpha_end", "alphaEnd"]),
        ],
        "pos" => &[
            ("start", &["pos_start", "posStart"]),
            ("end", &["pos_end", "posEnd"]),
        ],
        _ => return None,
    };
    let mut attrs = std::collections::BTreeMap::new();
    for (attr, keys) in pairs {
        if let Some(value) = json_string_alias(value, keys) {
            attrs.insert((*attr).to_string(), value);
        }
    }
    if attrs.is_empty() {
        None
    } else {
        Some(drawing_style_xml_attrs_from_map(attrs))
    }
}

fn drawing_style_solid_fill_xml(value: &Value) -> Option<String> {
    if let Some(color) = value.as_str() {
        return Some(format!(
            "<hp:solidFill color=\"{}\"/>",
            drawing_style_xml_attr_escape(color)
        ));
    }
    let mut attr_map = drawing_style_xml_attr_map(
        value,
        &[
            "color_hex",
            "colorHex",
            "color",
            "effectsColor",
            "raw_child_xml",
            "rawChildXml",
            "raw_children_xml",
            "rawChildrenXml",
        ],
    );
    if !attr_map.contains_key("color") {
        if let Some(color) = json_string_alias(value, &["color_hex", "colorHex"]) {
            attr_map.insert("color".to_string(), color);
        }
    }
    let attrs = drawing_style_xml_attrs_from_map(attr_map);
    let mut children = Vec::new();
    if let Some(color_xml) = value
        .get("color")
        .or_else(|| value.get("effectsColor"))
        .and_then(drawing_style_effect_color_xml)
    {
        children.push(color_xml);
    }
    children.extend(drawing_style_raw_child_xml_values(value));
    Some(if children.is_empty() {
        format!("<hp:solidFill{attrs}/>")
    } else {
        format!("<hp:solidFill{attrs}>{}</hp:solidFill>", children.join(""))
    })
}

fn drawing_style_raw_child_xml_values(value: &Value) -> Vec<String> {
    let Some(raw_value) = value
        .get("raw_child_xml")
        .or_else(|| value.get("rawChildXml"))
        .or_else(|| value.get("raw_children_xml"))
        .or_else(|| value.get("rawChildrenXml"))
    else {
        return Vec::new();
    };
    if raw_value.is_null() {
        return Vec::new();
    }
    if let Some(raw) = raw_value.as_str() {
        return vec![raw.to_string()];
    }
    raw_value
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn drawing_style_effect_color_xml(value: &Value) -> Option<String> {
    if !value.is_object() {
        let rgb_attrs = drawing_style_rgb_xml_attrs_from_hex(value)?;
        return Some(format!(
            r#"<hp:effectsColor presetIdx="-1" schemeIdx="-1" systemIdx="-1" type="RGB"><hp:rgb{rgb_attrs}/></hp:effectsColor>"#
        ));
    }
    let mut attr_map = drawing_style_xml_attr_map(
        value,
        &[
            "rgb",
            "color_hex",
            "colorHex",
            "rgb_hex",
            "rgbHex",
            "raw_child_xml",
            "rawChildXml",
            "raw_children_xml",
            "rawChildrenXml",
        ],
    );
    let rgb_attrs = value
        .get("rgb")
        .filter(|rgb| rgb.is_object())
        .map(|rgb| drawing_style_xml_attrs(rgb, &["color_hex", "colorHex", "rgb_hex", "rgbHex"]))
        .or_else(|| {
            value
                .get("color_hex")
                .or_else(|| value.get("colorHex"))
                .or_else(|| value.get("rgb_hex"))
                .or_else(|| value.get("rgbHex"))
                .and_then(drawing_style_rgb_xml_attrs_from_hex)
        });
    let mut children = Vec::new();
    if let Some(rgb_attrs) = rgb_attrs {
        attr_map
            .entry("presetIdx".to_string())
            .or_insert_with(|| "-1".to_string());
        attr_map
            .entry("schemeIdx".to_string())
            .or_insert_with(|| "-1".to_string());
        attr_map
            .entry("systemIdx".to_string())
            .or_insert_with(|| "-1".to_string());
        attr_map
            .entry("type".to_string())
            .or_insert_with(|| "RGB".to_string());
        children.push(format!("<hp:rgb{rgb_attrs}/>"));
    }
    children.extend(drawing_style_raw_child_xml_values(value));
    let attrs = drawing_style_xml_attrs_from_map(attr_map);
    if children.is_empty() {
        Some(format!("<hp:effectsColor{attrs}/>"))
    } else {
        Some(format!(
            "<hp:effectsColor{attrs}>{}</hp:effectsColor>",
            children.join("")
        ))
    }
}

fn drawing_style_raw_fragment_contains_effect(raw: &str, effect_name: &[u8]) -> bool {
    drawing_style_effect_json_from_raw_fragment(raw, effect_name).is_some()
}

fn drawing_style_xml_local_name(name: &[u8]) -> &[u8] {
    name.iter()
        .position(|&byte| byte == b':')
        .map(|idx| &name[idx + 1..])
        .unwrap_or(name)
}

fn drawing_style_xml_attrs_value(e: &quick_xml::events::BytesStart<'_>) -> Map<String, Value> {
    drawing_style_xml_attr_pairs(e)
        .into_iter()
        .map(|(key, value)| (key, Value::String(value)))
        .collect()
}

fn drawing_style_xml_attr_pairs(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    e.attributes()
        .flatten()
        .filter_map(|attr| {
            let key = String::from_utf8_lossy(drawing_style_xml_local_name(attr.key.as_ref()))
                .to_string();
            if key.is_empty() || key.starts_with("xmlns") {
                return None;
            }
            let value = String::from_utf8_lossy(attr.value.as_ref()).to_string();
            Some((key, value))
        })
        .collect()
}

fn drawing_style_rgb_color_hex(rgb: &Map<String, Value>) -> Option<String> {
    let r = rgb.get("r")?.as_str()?.trim().parse::<u8>().ok()?;
    let g = rgb.get("g")?.as_str()?.trim().parse::<u8>().ok()?;
    let b = rgb.get("b")?.as_str()?.trim().parse::<u8>().ok()?;
    Some(format!("#{:02X}{:02X}{:02X}", r, g, b))
}

fn drawing_style_rgb_xml_attrs_from_hex(value: &Value) -> Option<String> {
    let color_ref = json_css_color_ref_value(value)?;
    if color_ref == 0xFFFF_FFFF {
        return None;
    }
    let rgb = color_ref_to_rgb_u32(color_ref);
    Some(format!(
        " b=\"{}\" g=\"{}\" r=\"{}\"",
        rgb & 0xFF,
        (rgb >> 8) & 0xFF,
        (rgb >> 16) & 0xFF
    ))
}

fn drawing_style_xml_attrs(value: &Value, skip_keys: &[&str]) -> String {
    drawing_style_xml_attrs_from_map(drawing_style_xml_attr_map(value, skip_keys))
}

fn drawing_style_xml_attr_map(
    value: &Value,
    skip_keys: &[&str],
) -> std::collections::BTreeMap<String, String> {
    let mut attrs = std::collections::BTreeMap::new();
    if let Some(attr_obj) = value.get("attrs").and_then(Value::as_object) {
        for (key, raw_value) in attr_obj {
            if drawing_style_safe_xml_name(key) {
                if let Some(value) = json_scalar_string(raw_value) {
                    attrs.insert(key.to_string(), value);
                }
            }
        }
    }
    if let Some(obj) = value.as_object() {
        for (key, raw_value) in obj {
            if key == "attrs" || skip_keys.iter().any(|skip| skip == key) {
                continue;
            }
            if drawing_style_safe_xml_name(key) {
                if let Some(value) = json_scalar_string(raw_value) {
                    attrs.insert(key.to_string(), value);
                }
            }
        }
    }
    attrs
}

fn drawing_style_xml_attrs_from_map(attrs: std::collections::BTreeMap<String, String>) -> String {
    attrs
        .into_iter()
        .map(|(key, value)| format!(" {}=\"{}\"", key, drawing_style_xml_attr_escape(&value)))
        .collect::<Vec<_>>()
        .join("")
}

fn drawing_style_safe_xml_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == ':')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.'))
}

fn drawing_style_xml_attr_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn drawing_style_has_flat_border_aliases(style: &Value) -> bool {
    [
        "borderColor",
        "borderColorHex",
        "borderWidth",
        "borderAttr",
        "borderOutlineStyle",
    ]
    .iter()
    .any(|key| style.get(*key).is_some())
}

fn drawing_style_has_flat_fill_aliases(style: &Value) -> bool {
    [
        "fillBgColor",
        "fillBgColorHex",
        "fillPatColor",
        "fillPatColorHex",
        "fillPatType",
    ]
    .iter()
    .any(|key| style.get(*key).is_some())
}

fn drawing_style_has_flat_shadow_aliases(style: &Value) -> bool {
    [
        "shadowType",
        "shadowColor",
        "shadowColorHex",
        "shadowOffsetX",
        "shadowOffsetY",
        "shadowAlpha",
    ]
    .iter()
    .any(|key| style.get(*key).is_some())
}

fn fill_from_json(core: &mut DocumentCore, value: &Value) -> Result<Fill, String> {
    let has_flat_solid_aliases = [
        "fillBgColor",
        "fillBgColorHex",
        "fillPatColor",
        "fillPatColorHex",
        "fillPatType",
    ]
    .iter()
    .any(|key| value.get(*key).is_some());
    let mut fill = Fill {
        fill_type: value
            .get("type")
            .and_then(Value::as_str)
            .map(fill_type_from_name)
            .unwrap_or(if has_flat_solid_aliases {
                FillType::Solid
            } else {
                FillType::None
            }),
        alpha: json_u8(value, "alpha").unwrap_or(0),
        ..Default::default()
    };
    fill.solid = value
        .get("solid")
        .or_else(|| has_flat_solid_aliases.then_some(value))
        .filter(|solid| !solid.is_null())
        .and_then(|solid| {
            let background_color = json_u32_alias(solid, "background_color", &["fillBgColor"])
                .or_else(|| {
                    json_color_ref_alias(
                        solid,
                        "background_color",
                        &[
                            "background_color_hex",
                            "backgroundColorHex",
                            "fillBgColorHex",
                        ],
                    )
                })?;
            let pattern_color = json_u32_alias(solid, "pattern_color", &["fillPatColor"])
                .or_else(|| {
                    json_color_ref_alias(
                        solid,
                        "pattern_color",
                        &["pattern_color_hex", "patternColorHex", "fillPatColorHex"],
                    )
                })
                .unwrap_or(0);
            Some(SolidFill {
                background_color,
                pattern_color,
                pattern_type: json_i32_alias(
                    solid,
                    "pattern_type",
                    &["patternType", "fillPatType"],
                )
                .unwrap_or(0),
            })
        });
    fill.gradient = value
        .get("gradient")
        .filter(|gradient| !gradient.is_null())
        .map(|gradient| GradientFill {
            gradient_type: json_i16_alias(gradient, "gradient_type", &["gradientType"])
                .unwrap_or(0),
            angle: json_i16(gradient, "angle").unwrap_or(0),
            center_x: json_i16_alias(gradient, "center_x", &["centerX"]).unwrap_or(0),
            center_y: json_i16_alias(gradient, "center_y", &["centerY"]).unwrap_or(0),
            blur: json_i16(gradient, "blur").unwrap_or(0),
            step_center: json_u8_alias(gradient, "step_center", &["stepCenter"]).unwrap_or(0),
            colors: json_color_ref_vec_alias(gradient, "colors", &["colors_hex", "colorsHex"])
                .unwrap_or_default(),
            positions: json_i32_vec_alias(
                gradient,
                "positions",
                &[
                    "positions_percent",
                    "positionsPercent",
                    "stop_positions",
                    "stopPositions",
                ],
            )
            .unwrap_or_default(),
        });
    fill.image = value
        .get("image")
        .filter(|image| !image.is_null())
        .map(|image| image_fill_from_json(core, image))
        .transpose()?;
    Ok(fill)
}

fn image_fill_from_json(core: &mut DocumentCore, image: &Value) -> Result<ImageFill, String> {
    let mut image_fill = ImageFill {
        fill_mode: image
            .get("fill_mode")
            .and_then(Value::as_str)
            .map(image_fill_mode_from_name)
            .unwrap_or_default(),
        brightness: json_i8(image, "brightness").unwrap_or(0),
        contrast: json_i8(image, "contrast").unwrap_or(0),
        effect: json_u8(image, "effect").unwrap_or(0),
        bin_data_id: json_u16(image, "bin_data_id").unwrap_or(0),
    };

    let image_base64 = image
        .get("image_base64")
        .and_then(Value::as_str)
        .unwrap_or("");
    let external_path = image
        .get("external_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !image_base64.trim().is_empty() || !external_path.trim().is_empty() {
        let extension = image.get("extension").and_then(Value::as_str).unwrap_or("");
        image_fill.bin_data_id =
            push_template_image_fill_bin_data(core, image_base64, external_path, extension)?;
    }

    Ok(image_fill)
}

fn push_template_image_fill_bin_data(
    core: &mut DocumentCore,
    image_base64: &str,
    external_path: &str,
    extension: &str,
) -> Result<u16, String> {
    let has_embedded_data = !image_base64.trim().is_empty();
    let external_path = external_path.trim();
    if !has_embedded_data && external_path.is_empty() {
        return Err("template image fill requires image_base64 or external_path".to_string());
    }
    let extension = normalized_picture_extension(extension, external_path);
    let bin_data_id = next_template_bin_data_id(core);

    if has_embedded_data {
        let image_data = base64::engine::general_purpose::STANDARD
            .decode(image_base64)
            .map_err(|e| format!("invalid template image fill image_base64: {e}"))?;
        if image_data.is_empty() {
            return Err("template image fill data is empty".to_string());
        }
        core.document.bin_data_content.push(BinDataContent {
            id: bin_data_id,
            data: image_data,
            extension: extension.clone(),
        });
        core.document.doc_info.bin_data_list.push(BinData {
            raw_data: None,
            attr: 0x0101,
            data_type: BinDataType::Embedding,
            compression: BinDataCompression::Default,
            status: BinDataStatus::Success,
            abs_path: None,
            rel_path: None,
            storage_id: bin_data_id,
            extension: Some(extension),
        });
    } else {
        core.document.doc_info.bin_data_list.push(BinData {
            raw_data: None,
            attr: 0x0000,
            data_type: BinDataType::Link,
            compression: BinDataCompression::Default,
            status: BinDataStatus::NotAccessed,
            abs_path: Some(external_path.to_string()),
            rel_path: None,
            storage_id: bin_data_id,
            extension: Some(extension),
        });
    }
    core.document.doc_info.raw_stream = None;
    core.document.doc_info.raw_stream_dirty = true;

    Ok(bin_data_id)
}

fn fill_type_from_name(name: &str) -> FillType {
    match name {
        "solid" | "SOLID" => FillType::Solid,
        "image" | "IMAGE" => FillType::Image,
        "gradient" | "GRADIENT" => FillType::Gradient,
        _ => FillType::None,
    }
}

fn shape_shadow_type_from_name(value: &str) -> Option<u32> {
    let key = value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();

    match key.as_str() {
        "" | "0" | "none" => Some(0),
        "1" | "lefttop" | "topleft" => Some(1),
        "2" | "righttop" | "topright" => Some(2),
        "3" | "leftbottom" | "bottomleft" => Some(3),
        "4" | "rightbottom" | "bottomright" => Some(4),
        "5" | "center" | "inside" | "outside" => Some(5),
        _ => None,
    }
}

fn border_line_type_from_name(name: &str) -> BorderLineType {
    match name {
        "none" | "NONE" => BorderLineType::None,
        "solid" | "SOLID" => BorderLineType::Solid,
        "dash" | "DASH" => BorderLineType::Dash,
        "dot" | "DOT" => BorderLineType::Dot,
        "dash_dot" | "DASH_DOT" => BorderLineType::DashDot,
        "dash_dot_dot" | "DASH_DOT_DOT" => BorderLineType::DashDotDot,
        "long_dash" | "LONG_DASH" => BorderLineType::LongDash,
        "circle" | "CIRCLE" => BorderLineType::Circle,
        "double" | "DOUBLE" | "DOUBLE_SLIM" => BorderLineType::Double,
        "thin_thick_double" | "SLIM_THICK" => BorderLineType::ThinThickDouble,
        "thick_thin_double" | "THICK_SLIM" => BorderLineType::ThickThinDouble,
        "thin_thick_thin_triple" | "SLIM_THICK_SLIM" => BorderLineType::ThinThickThinTriple,
        "wave" | "WAVE" => BorderLineType::Wave,
        "double_wave" | "DOUBLE_WAVE" => BorderLineType::DoubleWave,
        "thick_3d" | "THICK_3D" => BorderLineType::Thick3D,
        "thick_3d_reverse" | "THICK_3D_REVERSE" => BorderLineType::Thick3DReverse,
        "thin_3d" | "THIN_3D" => BorderLineType::Thin3D,
        "thin_3d_reverse" | "THIN_3D_REVERSE" => BorderLineType::Thin3DReverse,
        _ => BorderLineType::Solid,
    }
}

fn border_line_type_from_code(code: u8) -> BorderLineType {
    match code {
        0 => BorderLineType::None,
        1 => BorderLineType::Solid,
        2 => BorderLineType::Dash,
        3 => BorderLineType::Dot,
        4 => BorderLineType::DashDot,
        5 => BorderLineType::DashDotDot,
        6 => BorderLineType::LongDash,
        7 => BorderLineType::Circle,
        8 => BorderLineType::Double,
        9 => BorderLineType::ThinThickDouble,
        10 => BorderLineType::ThickThinDouble,
        11 => BorderLineType::ThinThickThinTriple,
        12 => BorderLineType::Wave,
        13 => BorderLineType::DoubleWave,
        14 => BorderLineType::Thick3D,
        15 => BorderLineType::Thick3DReverse,
        16 => BorderLineType::Thin3D,
        17 => BorderLineType::Thin3DReverse,
        _ => BorderLineType::Solid,
    }
}

fn image_fill_mode_from_name(name: &str) -> ImageFillMode {
    match name {
        "tile_all" | "TILE" | "TILE_ALL" => ImageFillMode::TileAll,
        "tile_horz_top" | "TILE_HORZ_TOP" => ImageFillMode::TileHorzTop,
        "tile_horz_bottom" | "TILE_HORZ_BOTTOM" => ImageFillMode::TileHorzBottom,
        "tile_vert_left" | "TILE_VERT_LEFT" => ImageFillMode::TileVertLeft,
        "tile_vert_right" | "TILE_VERT_RIGHT" => ImageFillMode::TileVertRight,
        "fit_to_size" | "FIT" | "FIT_TO_SIZE" => ImageFillMode::FitToSize,
        "center" | "CENTER" => ImageFillMode::Center,
        "center_top" | "CENTER_TOP" => ImageFillMode::CenterTop,
        "center_bottom" | "CENTER_BOTTOM" => ImageFillMode::CenterBottom,
        "left_center" | "LEFT_CENTER" => ImageFillMode::LeftCenter,
        "left_top" | "LEFT_TOP" | "TOP_LEFT_ALIGN" => ImageFillMode::LeftTop,
        "left_bottom" | "LEFT_BOTTOM" => ImageFillMode::LeftBottom,
        "right_center" | "RIGHT_CENTER" => ImageFillMode::RightCenter,
        "right_top" | "RIGHT_TOP" => ImageFillMode::RightTop,
        "right_bottom" | "RIGHT_BOTTOM" => ImageFillMode::RightBottom,
        "none" | "NONE" => ImageFillMode::None,
        _ => ImageFillMode::TileAll,
    }
}

fn json_u32(value: &Value, key: &str) -> Option<u32> {
    value.get(key)?.as_u64()?.try_into().ok()
}

fn color_ref_to_hex(color: u32) -> String {
    if color == 0xFFFF_FFFF {
        return "none".to_string();
    }
    let a = (color >> 24) & 0xFF;
    let r = color & 0xFF;
    let g = (color >> 8) & 0xFF;
    let b = (color >> 16) & 0xFF;
    if a == 0 {
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", a, r, g, b)
    }
}

fn rgb_u32_to_color_ref(raw: u32) -> u32 {
    let a = raw & 0xFF00_0000;
    let r = (raw >> 16) & 0xFF;
    let g = (raw >> 8) & 0xFF;
    let b = raw & 0xFF;
    a | (b << 16) | (g << 8) | r
}

fn json_css_color_ref_value(value: &Value) -> Option<u32> {
    if let Some(raw) = value.as_u64() {
        return u32::try_from(raw).ok().map(rgb_u32_to_color_ref);
    }
    if let Some(raw) = value.as_i64() {
        return u32::try_from(raw).ok().map(rgb_u32_to_color_ref);
    }

    let raw = value.as_str()?.trim();
    if raw.eq_ignore_ascii_case("none") {
        return Some(0xFFFF_FFFF);
    }
    let hex = raw
        .strip_prefix('#')
        .unwrap_or(raw)
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or_else(|| raw.strip_prefix('#').unwrap_or(raw));
    match hex.len() {
        6 | 8 => u32::from_str_radix(hex, 16).ok().map(rgb_u32_to_color_ref),
        _ => raw.parse::<u32>().ok().map(rgb_u32_to_color_ref),
    }
}

fn json_color_ref_alias(value: &Value, raw_key: &str, hex_keys: &[&str]) -> Option<u32> {
    json_u32(value, raw_key).or_else(|| {
        hex_keys
            .iter()
            .find_map(|key| value.get(*key).and_then(json_css_color_ref_value))
    })
}

fn json_color_ref_vec_alias(value: &Value, raw_key: &str, hex_keys: &[&str]) -> Option<Vec<u32>> {
    json_u32_vec(value, raw_key).or_else(|| {
        hex_keys.iter().find_map(|key| {
            Some(
                value
                    .get(*key)?
                    .as_array()?
                    .iter()
                    .filter_map(json_css_color_ref_value)
                    .collect(),
            )
        })
    })
}

fn json_i16_alias(value: &Value, raw_key: &str, aliases: &[&str]) -> Option<i16> {
    json_i16(value, raw_key).or_else(|| aliases.iter().find_map(|key| json_i16(value, key)))
}

fn json_i32_alias(value: &Value, raw_key: &str, aliases: &[&str]) -> Option<i32> {
    json_i32(value, raw_key).or_else(|| aliases.iter().find_map(|key| json_i32(value, key)))
}

fn json_u32_alias(value: &Value, raw_key: &str, aliases: &[&str]) -> Option<u32> {
    json_u32(value, raw_key).or_else(|| aliases.iter().find_map(|key| json_u32(value, key)))
}

fn json_u8_alias(value: &Value, raw_key: &str, aliases: &[&str]) -> Option<u8> {
    json_u8(value, raw_key).or_else(|| aliases.iter().find_map(|key| json_u8(value, key)))
}

fn json_i32_vec_alias(value: &Value, raw_key: &str, aliases: &[&str]) -> Option<Vec<i32>> {
    json_i32_vec(value, raw_key).or_else(|| aliases.iter().find_map(|key| json_i32_vec(value, key)))
}

fn json_u16(value: &Value, key: &str) -> Option<u16> {
    value.get(key)?.as_u64()?.try_into().ok()
}

fn json_u8(value: &Value, key: &str) -> Option<u8> {
    value.get(key)?.as_u64()?.try_into().ok()
}

fn json_i32(value: &Value, key: &str) -> Option<i32> {
    value.get(key)?.as_i64()?.try_into().ok()
}

fn json_i16(value: &Value, key: &str) -> Option<i16> {
    value.get(key)?.as_i64()?.try_into().ok()
}

fn json_i8(value: &Value, key: &str) -> Option<i8> {
    value.get(key)?.as_i64()?.try_into().ok()
}

fn json_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key)?.as_f64()
}

fn json_bool(value: &Value, key: &str) -> Option<bool> {
    match value.get(key)? {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => value.as_i64().map(|value| value != 0),
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" => Some(true),
            "0" | "false" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn json_bool_alias(value: &Value, raw_key: &str, aliases: &[&str]) -> Option<bool> {
    json_bool(value, raw_key).or_else(|| aliases.iter().find_map(|key| json_bool(value, key)))
}

fn json_u32_vec(value: &Value, key: &str) -> Option<Vec<u32>> {
    Some(
        value
            .get(key)?
            .as_array()?
            .iter()
            .filter_map(|value| value.as_u64().and_then(|value| u32::try_from(value).ok()))
            .collect(),
    )
}

fn json_i32_vec(value: &Value, key: &str) -> Option<Vec<i32>> {
    Some(
        value
            .get(key)?
            .as_array()?
            .iter()
            .filter_map(|value| value.as_i64().and_then(|value| i32::try_from(value).ok()))
            .collect(),
    )
}

fn object_geometry_json(shape: &ShapeObject) -> Option<Value> {
    match shape {
        ShapeObject::Line(line) => Some(line_geometry_json(line)),
        ShapeObject::Rectangle(rect) => Some(json!({
            "round_rate": rect.round_rate,
            "x_coords": rect.x_coords,
            "y_coords": rect.y_coords,
            "raw_trailing_base64": if rect.raw_trailing.is_empty() {
                String::new()
            } else {
                base64::engine::general_purpose::STANDARD.encode(&rect.raw_trailing)
            },
        })),
        ShapeObject::Ellipse(ellipse) => Some(json!({
            "attr": ellipse.attr,
            "center": point_json(ellipse.center),
            "axis1": point_json(ellipse.axis1),
            "axis2": point_json(ellipse.axis2),
            "start1": point_json(ellipse.start1),
            "end1": point_json(ellipse.end1),
            "start2": point_json(ellipse.start2),
            "end2": point_json(ellipse.end2),
            "raw_trailing_base64": if ellipse.raw_trailing.is_empty() {
                String::new()
            } else {
                base64::engine::general_purpose::STANDARD.encode(&ellipse.raw_trailing)
            },
        })),
        ShapeObject::Arc(arc) => Some(json!({
            "arc_type": arc.arc_type,
            "center": point_json(arc.center),
            "axis1": point_json(arc.axis1),
            "axis2": point_json(arc.axis2),
            "raw_trailing_base64": if arc.raw_trailing.is_empty() {
                String::new()
            } else {
                base64::engine::general_purpose::STANDARD.encode(&arc.raw_trailing)
            },
        })),
        ShapeObject::Polygon(poly) => Some(json!({
            "points": poly.points.iter().copied().map(point_json).collect::<Vec<_>>(),
            "raw_trailing_base64": if poly.raw_trailing.is_empty() {
                String::new()
            } else {
                base64::engine::general_purpose::STANDARD.encode(&poly.raw_trailing)
            },
        })),
        ShapeObject::Curve(curve) => Some(json!({
            "points": curve.points.iter().copied().map(point_json).collect::<Vec<_>>(),
            "segment_types": curve.segment_types,
            "raw_trailing_base64": if curve.raw_trailing.is_empty() {
                String::new()
            } else {
                base64::engine::general_purpose::STANDARD.encode(&curve.raw_trailing)
            },
        })),
        ShapeObject::Group(group) if !group.raw_component_extra.is_empty() => Some(json!({
            "raw_component_extra_base64": base64::engine::general_purpose::STANDARD
                .encode(&group.raw_component_extra),
        })),
        _ => None,
    }
}

fn line_geometry_json(line: &LineShape) -> Value {
    let mut map = Map::new();
    map.insert("start".to_string(), point_json(line.start));
    map.insert("end".to_string(), point_json(line.end));
    map.insert(
        "started_right_or_bottom".to_string(),
        json!(line.started_right_or_bottom),
    );
    if let Some(connector) = &line.connector {
        map.insert("connector".to_string(), connector_json(connector));
    } else if !line.raw_trailing.is_empty() {
        map.insert(
            "raw_trailing_base64".to_string(),
            json!(base64::engine::general_purpose::STANDARD.encode(&line.raw_trailing)),
        );
    }
    Value::Object(map)
}

fn connector_json(connector: &ConnectorData) -> Value {
    let mut map = Map::new();
    map.insert("link_type".to_string(), json!(connector.link_type as u32));
    map.insert(
        "start_subject_id".to_string(),
        json!(connector.start_subject_id),
    );
    map.insert(
        "start_subject_index".to_string(),
        json!(connector.start_subject_index),
    );
    map.insert(
        "end_subject_id".to_string(),
        json!(connector.end_subject_id),
    );
    map.insert(
        "end_subject_index".to_string(),
        json!(connector.end_subject_index),
    );
    map.insert(
        "control_points".to_string(),
        Value::Array(
            connector
                .control_points
                .iter()
                .map(connector_control_point_json)
                .collect(),
        ),
    );
    if !connector.raw_trailing.is_empty() {
        map.insert(
            "raw_trailing_base64".to_string(),
            json!(base64::engine::general_purpose::STANDARD.encode(&connector.raw_trailing)),
        );
    }
    Value::Object(map)
}

fn connector_control_point_json(point: &ConnectorControlPoint) -> Value {
    json!({
        "x": point.x,
        "y": point.y,
        "point_type": point.point_type,
    })
}

fn point_json(point: Point) -> Value {
    json!({ "x": point.x, "y": point.y })
}

fn geometry_point(geometry: &Value, key: &str) -> Option<Point> {
    let point = geometry.get(key)?;
    Some(Point {
        x: point.get("x")?.as_i64()?.try_into().ok()?,
        y: point.get("y")?.as_i64()?.try_into().ok()?,
    })
}

fn connector_from_json(value: &Value) -> Result<ConnectorData, String> {
    let Some(_) = value.as_object() else {
        return Err("line geometry.connector must be an object".to_string());
    };
    let control_points = value
        .get("control_points")
        .or_else(|| value.get("controlPoints"))
        .filter(|value| !value.is_null())
        .map(connector_control_points_from_json)
        .transpose()?
        .unwrap_or_default();
    let raw_trailing = value
        .get("raw_trailing_base64")
        .or_else(|| value.get("rawTrailingBase64"))
        .and_then(Value::as_str)
        .filter(|raw| !raw.is_empty())
        .map(|raw| {
            base64::engine::general_purpose::STANDARD
                .decode(raw)
                .map_err(|e| format!("invalid line connector raw_trailing_base64: {e}"))
        })
        .transpose()?
        .unwrap_or_default();
    Ok(ConnectorData {
        link_type: LinkLineType::from_u32(json_u32(value, "link_type").unwrap_or(0)),
        start_subject_id: json_u32(value, "start_subject_id").unwrap_or(0),
        start_subject_index: json_u32(value, "start_subject_index").unwrap_or(0),
        end_subject_id: json_u32(value, "end_subject_id").unwrap_or(0),
        end_subject_index: json_u32(value, "end_subject_index").unwrap_or(0),
        control_points,
        raw_trailing,
    })
}

fn connector_control_points_from_json(value: &Value) -> Result<Vec<ConnectorControlPoint>, String> {
    let Some(values) = value.as_array() else {
        return Err("line geometry.connector.control_points must be an array".to_string());
    };
    values
        .iter()
        .map(|value| {
            let Some(_) = value.as_object() else {
                return Err(
                    "line geometry.connector.control_points entries must be objects".to_string(),
                );
            };
            Ok(ConnectorControlPoint {
                x: json_i32(value, "x").unwrap_or(0),
                y: json_i32(value, "y").unwrap_or(0),
                point_type: json_u16(value, "point_type").unwrap_or(0),
            })
        })
        .collect()
}

fn geometry_i32_array4(geometry: &Value, key: &str) -> Option<[i32; 4]> {
    let values = geometry.get(key)?.as_array()?;
    if values.len() != 4 {
        return None;
    }
    Some([
        values[0].as_i64()?.try_into().ok()?,
        values[1].as_i64()?.try_into().ok()?,
        values[2].as_i64()?.try_into().ok()?,
        values[3].as_i64()?.try_into().ok()?,
    ])
}

fn geometry_points(geometry: &Value, key: &str) -> Option<Vec<Point>> {
    let values = geometry.get(key)?.as_array()?;
    Some(
        values
            .iter()
            .filter_map(|value| {
                Some(Point {
                    x: value.get("x")?.as_i64()?.try_into().ok()?,
                    y: value.get("y")?.as_i64()?.try_into().ok()?,
                })
            })
            .collect(),
    )
}

fn geometry_u8_vec(geometry: &Value, key: &str) -> Option<Vec<u8>> {
    Some(
        geometry
            .get(key)?
            .as_array()?
            .iter()
            .filter_map(|value| value.as_u64().and_then(|value| u8::try_from(value).ok()))
            .collect(),
    )
}

fn object_chart_data_base64(shape: &ShapeObject) -> String {
    match shape {
        ShapeObject::Chart(chart) if !chart.raw_chart_data.is_empty() => {
            base64::engine::general_purpose::STANDARD.encode(&chart.raw_chart_data)
        }
        _ => String::new(),
    }
}

fn object_ole_tag_base64(shape: &ShapeObject) -> String {
    match shape {
        ShapeObject::Ole(ole) => {
            let raw_tag_data = if ole.raw_tag_data.is_empty() {
                template_ole_tag_data(ole.extent_x, ole.extent_y, ole.bin_data_id)
            } else {
                ole.raw_tag_data.clone()
            };
            base64::engine::general_purpose::STANDARD.encode(raw_tag_data)
        }
        _ => String::new(),
    }
}

fn object_ole_bin_data(core: &DocumentCore, shape: &ShapeObject) -> (String, String) {
    let ShapeObject::Ole(ole) = shape else {
        return (String::new(), String::new());
    };
    let Ok(bin_data_id) = u16::try_from(ole.bin_data_id) else {
        return (String::new(), String::new());
    };
    let Some(content) = picture_bin_data_content(core, bin_data_id) else {
        return (String::new(), String::new());
    };
    (
        base64::engine::general_purpose::STANDARD.encode(&content.data),
        content.extension.clone(),
    )
}

fn push_template_ole_bin_data(core: &mut DocumentCore, data: Vec<u8>, extension: &str) -> u16 {
    let bin_data_id = next_template_bin_data_id(core);
    core.document.bin_data_content.push(BinDataContent {
        id: bin_data_id,
        data,
        extension: extension.to_string(),
    });
    core.document.doc_info.bin_data_list.push(BinData {
        raw_data: None,
        attr: 0x0002,
        data_type: BinDataType::Storage,
        compression: BinDataCompression::Default,
        status: BinDataStatus::NotAccessed,
        abs_path: None,
        rel_path: None,
        storage_id: bin_data_id,
        extension: Some(extension.to_string()),
    });
    core.document.doc_info.raw_stream = None;
    core.document.doc_info.raw_stream_dirty = true;
    bin_data_id
}

fn normalized_ole_extension(extension: &str) -> String {
    let extension = extension.trim().trim_start_matches('.');
    if extension.is_empty() {
        "OLE".to_string()
    } else {
        extension.to_string()
    }
}

fn template_ole_tag_data(extent_x: i32, extent_y: i32, bin_data_id: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(26);
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&extent_x.to_le_bytes());
    data.extend_from_slice(&extent_y.to_le_bytes());
    data.extend_from_slice(&bin_data_id.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data
}

fn ole_tag_bin_data_id(raw_tag_data: &[u8]) -> u32 {
    read_u32_le(raw_tag_data, 12).unwrap_or(0)
}

fn patch_ole_tag_bin_data_id(raw_tag_data: &mut [u8], bin_data_id: u32) {
    if raw_tag_data.len() >= 16 {
        raw_tag_data[12..16].copy_from_slice(&bin_data_id.to_le_bytes());
    }
}

fn ole_tag_extents(raw_tag_data: &[u8], fallback_width: u32, fallback_height: u32) -> (i32, i32) {
    let extent_x = read_i32_le(raw_tag_data, 4).unwrap_or(fallback_width as i32);
    let extent_y = read_i32_le(raw_tag_data, 8).unwrap_or(fallback_height as i32);
    (extent_x.max(1), extent_y.max(1))
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn read_i32_le(data: &[u8], offset: usize) -> Option<i32> {
    let bytes = data.get(offset..offset + 4)?;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

fn shape_paragraph(shape: ShapeObject) -> Paragraph {
    let mut para = Paragraph::new_empty();
    para.char_count = 9;
    para.control_mask = 0x0000_0800;
    para.controls.push(Control::Shape(Box::new(shape)));
    para.ctrl_data_records.push(None);
    para.has_para_text = true;
    para.raw_header_extra = paragraph_header_extra();
    para
}

fn object_placeholder_kind(shape: &ShapeObject) -> &'static str {
    match shape {
        ShapeObject::Chart(_) => "chart",
        ShapeObject::Ole(_) => "ole",
        ShapeObject::Group(_) => "shape_group",
        ShapeObject::Picture(_) => "picture",
        _ => "shape",
    }
}

fn object_shape_kind(shape: &ShapeObject) -> &'static str {
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

fn shape_text_box_text(shape: &ShapeObject) -> Option<String> {
    shape.drawing().and_then(|drawing| {
        drawing.text_box.as_ref().map(|text_box| {
            text_box
                .paragraphs
                .iter()
                .map(|paragraph| paragraph.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    })
}

fn object_placeholder_description(
    object_kind: &str,
    shape_kind: &str,
    description: &str,
) -> String {
    format!(
        "{}{}:{}:{}",
        OBJECT_PLACEHOLDER_PREFIX, object_kind, shape_kind, description
    )
}

fn parse_object_placeholder_description(description: &str) -> Option<(String, String, String)> {
    let rest = description.strip_prefix(OBJECT_PLACEHOLDER_PREFIX)?;
    let mut parts = rest.splitn(3, ':');
    let object_kind = parts.next().unwrap_or("shape").to_string();
    let shape_kind = parts.next().unwrap_or("").to_string();
    let description = parts.next().unwrap_or("").to_string();
    Some((object_kind, shape_kind, description))
}

fn default_object_placeholder_text(object_kind: &str, shape_kind: &str) -> String {
    if shape_kind.trim().is_empty() {
        format!("[{object_kind} placeholder]")
    } else {
        format!("[{object_kind}:{shape_kind} placeholder]")
    }
}

fn template_table(
    core: &mut DocumentCore,
    section_idx: usize,
    rows: &[Vec<String>],
    caption: Option<&TemplateCaption>,
    column_widths: &[u32],
    row_heights: &[u32],
    table_layout: Option<&TemplateTableLayout>,
    object_layout: Option<&Value>,
    border_fill: Option<&Value>,
    table_zones: &[TemplateTableZone],
    cell_layouts: &[Vec<TemplateCellLayout>],
    cell_formats: &[Vec<TemplateTextFormat>],
    cell_blocks: &[Vec<Vec<TemplateBlock>>],
) -> Result<Table, String> {
    let row_count = rows.len().max(1);
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    if row_count > u16::MAX as usize || col_count > u16::MAX as usize {
        return Err(format!(
            "template table is too large: rows={row_count}, cols={col_count}"
        ));
    }

    let row_count_u16 = row_count as u16;
    let col_count_u16 = col_count as u16;
    let table_layout = table_layout.cloned().unwrap_or_default();
    let caption = template_caption(core, section_idx, caption)?;
    let cell_pad = table_layout_padding(&table_layout);
    let col_widths = template_column_widths(core, section_idx, col_count, column_widths);
    let row_heights = template_row_heights(row_count, row_heights, cell_pad);
    let total_width = col_widths.iter().sum();
    let total_height = row_heights.iter().sum();
    let border_fill_id = border_fill
        .map(|border_fill| ensure_border_fill_for_template(core, border_fill))
        .transpose()?
        .unwrap_or_else(|| solid_border_fill_id(core));
    let (default_char_shape_id, default_para_shape_id) = default_shape_ids(core, section_idx);
    let cell_border_fill_ids = resolve_cell_layout_border_fills(core, cell_layouts)?;
    let resolved_table_zones = resolve_table_zones(core, table_zones)?;
    let mut paragraph_overrides =
        build_cell_paragraph_overrides(core, section_idx, row_count, col_count, cell_blocks)?;

    let mut cells = Vec::with_capacity(row_count * col_count);
    for r in 0..row_count_u16 {
        let row_height = row_heights[r as usize];
        for c in 0..col_count_u16 {
            let col_width = col_widths[c as usize];
            let mut cell = Cell::new_empty(c, r, col_width, row_height, border_fill_id);
            cell.padding = cell_pad;
            cell.vertical_align = VerticalAlign::Center;
            cell.raw_list_extra = Vec::new();
            if let Some(cell_para) = cell.paragraphs.first_mut() {
                let format = cell_formats
                    .get(r as usize)
                    .and_then(|row| row.get(c as usize));
                let char_shape_id = format
                    .and_then(|format| {
                        format
                            .char_shape_runs
                            .first()
                            .filter(|run| {
                                (run.char_shape_id as usize)
                                    < core.document.doc_info.char_shapes.len()
                            })
                            .map(|run| run.char_shape_id)
                            .or_else(|| {
                                format.char_format.as_ref().map(|props| {
                                    ensure_char_shape_for_template(
                                        core,
                                        default_char_shape_id,
                                        Some(props),
                                    )
                                })
                            })
                    })
                    .unwrap_or(default_char_shape_id);
                let para_shape_id = format
                    .and_then(|format| {
                        format.para_format.as_ref().map(|props| {
                            ensure_para_shape_for_template(core, default_para_shape_id, Some(props))
                        })
                    })
                    .unwrap_or(default_para_shape_id);
                prepare_cell_paragraph(
                    cell_para,
                    char_shape_id,
                    para_shape_id,
                    col_width,
                    row_height,
                    cell_pad,
                );
                if let Some(text) = rows.get(r as usize).and_then(|row| row.get(c as usize)) {
                    if !text.is_empty() {
                        cell_para.insert_text_at(0, text);
                        cell_para.has_para_text = true;
                    }
                }
                if let Some(format) = format {
                    apply_template_char_shape_runs(core, cell_para, &format.char_shape_runs);
                }
            }
            if let Some(paragraphs) = paragraph_overrides[r as usize][c as usize].take() {
                cell.paragraphs = paragraphs;
            }
            cells.push(cell);
        }
    }

    let row_sizes = (0..row_count_u16)
        .map(|_| col_count_u16 as i16)
        .collect::<Vec<_>>();
    let raw_ctrl_data = table_ctrl_data(row_count_u16, col_count_u16, total_width, total_height);

    let mut table = Table {
        attr: 0x04000006,
        row_count: row_count_u16,
        col_count: col_count_u16,
        cell_spacing: table_layout.cell_spacing,
        padding: cell_pad,
        row_sizes,
        border_fill_id,
        zones: Vec::new(),
        cells,
        cell_grid: Vec::new(),
        page_break: table_page_break_from_name(&table_layout.page_break),
        repeat_header: table_layout.repeat_header,
        caption,
        common: CommonObjAttr {
            treat_as_char: true,
            text_wrap: TextWrap::TopAndBottom,
            vert_rel_to: VertRelTo::Page,
            horz_rel_to: HorzRelTo::Para,
            vert_align: VertAlign::Top,
            horz_align: HorzAlign::Left,
            width: total_width,
            height: total_height,
            ..Default::default()
        },
        outer_margin_left: table_layout.outer_margin_left,
        outer_margin_right: table_layout.outer_margin_right,
        outer_margin_top: table_layout.outer_margin_top,
        outer_margin_bottom: table_layout.outer_margin_bottom,
        raw_ctrl_data,
        raw_table_record_attr: 0,
        raw_table_record_extra: vec![0u8; 2],
        dirty: true,
        local_resize_rows: Vec::new(),
        local_resize_cols: Vec::new(),
        local_resize_cell_widths: Vec::new(),
        local_resize_cell_heights: Vec::new(),
    };
    apply_table_layout_to_table(&mut table, &table_layout);
    table.zones = materialize_table_zones(
        &resolved_table_zones,
        table.border_fill_id,
        table.row_count,
        table.col_count,
    )?;
    apply_template_cell_layouts(&mut table, cell_layouts, &cell_border_fill_ids)?;
    apply_table_object_layout_to_table(&mut table, object_layout)?;
    table.rebuild_grid();
    Ok(table)
}

fn template_column_widths(
    core: &DocumentCore,
    section_idx: usize,
    col_count: usize,
    column_widths: &[u32],
) -> Vec<u32> {
    if column_widths.len() == col_count && column_widths.iter().all(|width| *width > 0) {
        return column_widths.to_vec();
    }
    let col_width = table_content_width(core, section_idx) / col_count.max(1) as u32;
    vec![col_width; col_count]
}

fn template_row_heights(row_count: usize, row_heights: &[u32], cell_pad: Padding) -> Vec<u32> {
    if row_heights.len() == row_count && row_heights.iter().all(|height| *height > 0) {
        return row_heights.to_vec();
    }
    let row_height = cell_pad.top as u32 + 1000 + cell_pad.bottom as u32;
    vec![row_height; row_count]
}

fn apply_created_table_geometry(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    col_count: usize,
    row_count: usize,
    column_widths: &[u32],
    row_heights: &[u32],
) -> Result<(), String> {
    let valid_column_widths = (column_widths.len() == col_count
        && column_widths.iter().all(|width| *width > 0))
    .then(|| column_widths.to_vec());
    let valid_row_heights = (row_heights.len() == row_count
        && row_heights.iter().all(|height| *height > 0))
    .then(|| row_heights.to_vec());

    if valid_column_widths.is_none() && valid_row_heights.is_none() {
        return Ok(());
    }

    {
        let table = match core
            .document
            .sections
            .get_mut(section_idx)
            .and_then(|section| section.paragraphs.get_mut(table_para))
            .and_then(|para| para.controls.get_mut(table_control))
        {
            Some(Control::Table(table)) => table,
            _ => return Err("created table control was not found".to_string()),
        };

        if let Some(widths) = valid_column_widths.as_deref() {
            table.set_column_widths(widths)?;
        }
        if let Some(heights) = valid_row_heights.as_deref() {
            apply_row_heights_to_table(table, heights);
        }
        table.update_ctrl_dimensions();
        table.dirty = true;
    }

    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn apply_created_table_layout(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    layout: &TemplateTableLayout,
) -> Result<(), String> {
    {
        let table = match core
            .document
            .sections
            .get_mut(section_idx)
            .and_then(|section| section.paragraphs.get_mut(table_para))
            .and_then(|para| para.controls.get_mut(table_control))
        {
            Some(Control::Table(table)) => table,
            _ => return Err("created table control was not found".to_string()),
        };
        apply_table_layout_to_table(table, layout);
    }

    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn apply_created_table_caption(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    caption: Option<Caption>,
) -> Result<(), String> {
    if caption.is_none() {
        return Ok(());
    }
    {
        let table = match core
            .document
            .sections
            .get_mut(section_idx)
            .and_then(|section| section.paragraphs.get_mut(table_para))
            .and_then(|para| para.controls.get_mut(table_control))
        {
            Some(Control::Table(table)) => table,
            _ => return Err("created table control was not found".to_string()),
        };
        table.caption = caption;
        table.dirty = true;
    }

    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn apply_created_table_cell_layouts(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    table_border_fill_id: Option<u16>,
    cell_layouts: &[Vec<TemplateCellLayout>],
    cell_border_fill_ids: &[Vec<Option<u16>>],
) -> Result<(), String> {
    if table_border_fill_id.is_none() && cell_layouts.is_empty() {
        return Ok(());
    }
    {
        let table = match core
            .document
            .sections
            .get_mut(section_idx)
            .and_then(|section| section.paragraphs.get_mut(table_para))
            .and_then(|para| para.controls.get_mut(table_control))
        {
            Some(Control::Table(table)) => table,
            _ => return Err("created table control was not found".to_string()),
        };
        if let Some(border_fill_id) = table_border_fill_id {
            table.border_fill_id = border_fill_id;
            for cell in &mut table.cells {
                cell.border_fill_id = border_fill_id;
            }
        }
        apply_template_cell_layouts(table, cell_layouts, cell_border_fill_ids)?;
    }

    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn apply_created_table_zones(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    table_zones: &[ResolvedTemplateTableZone],
) -> Result<(), String> {
    if table_zones.is_empty() {
        return Ok(());
    }
    {
        let table = match core
            .document
            .sections
            .get_mut(section_idx)
            .and_then(|section| section.paragraphs.get_mut(table_para))
            .and_then(|para| para.controls.get_mut(table_control))
        {
            Some(Control::Table(table)) => table,
            _ => return Err("created table control was not found".to_string()),
        };
        table.zones = materialize_table_zones(
            table_zones,
            table.border_fill_id,
            table.row_count,
            table.col_count,
        )?;
        table.dirty = true;
    }

    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn apply_created_table_object_layout(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    object_layout: Option<&Value>,
) -> Result<(), String> {
    if object_layout.is_none() {
        return Ok(());
    }
    {
        let table = match core
            .document
            .sections
            .get_mut(section_idx)
            .and_then(|section| section.paragraphs.get_mut(table_para))
            .and_then(|para| para.controls.get_mut(table_control))
        {
            Some(Control::Table(table)) => table,
            _ => return Err("created table control was not found".to_string()),
        };
        apply_table_object_layout_to_table(table, object_layout)?;
    }

    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn apply_table_object_layout_to_table(
    table: &mut Table,
    object_layout: Option<&Value>,
) -> Result<(), String> {
    apply_common_object_layout(&mut table.common, object_layout)?;
    table.raw_ctrl_data =
        crate::document_core::converters::common_obj_attr_writer::serialize_common_obj_attr(
            &table.common,
        );
    table.attr = if table.common.treat_as_char && table.common.flow_with_text {
        1
    } else {
        0
    };
    table.dirty = true;
    Ok(())
}

fn apply_common_object_layout(
    common: &mut CommonObjAttr,
    object_layout: Option<&Value>,
) -> Result<(), String> {
    let Some(value) = object_layout.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(_) = value.as_object() else {
        return Err("template object_layout must be an object".to_string());
    };

    let explicit_attr = value.get("attr").and_then(Value::as_u64).is_some();
    common.attr = json_u32(value, "attr").unwrap_or(common.attr);
    common.width = json_u32(value, "width").unwrap_or(common.width);
    common.height = json_u32(value, "height").unwrap_or(common.height);
    common.z_order = json_i32_alias(value, "z_order", &["zOrder"]).unwrap_or(common.z_order);
    common.instance_id =
        json_u32_alias(value, "instance_id", &["instanceId"]).unwrap_or(common.instance_id);
    common.treat_as_char =
        json_bool_alias(value, "treat_as_char", &["treatAsChar"]).unwrap_or(common.treat_as_char);
    common.flow_with_text = json_bool_alias(value, "flow_with_text", &["flowWithText"])
        .unwrap_or(common.flow_with_text);
    common.allow_overlap =
        json_bool_alias(value, "allow_overlap", &["allowOverlap"]).unwrap_or(common.allow_overlap);
    common.size_protect =
        json_bool_alias(value, "size_protect", &["sizeProtect"]).unwrap_or(common.size_protect);
    common.lock = json_bool(value, "lock").unwrap_or(common.lock);
    common.prevent_page_break = json_i32_alias(value, "prevent_page_break", &["preventPageBreak"])
        .unwrap_or(common.prevent_page_break);
    common.vert_rel_to = json_string_alias(value, &["vert_rel_to", "vertRelTo"])
        .as_deref()
        .map(|value| vert_rel_to_from_name(value, common.vert_rel_to))
        .unwrap_or(common.vert_rel_to);
    common.vert_align = json_string_alias(value, &["vert_align", "vertAlign"])
        .as_deref()
        .map(|value| obj_vert_align_from_name(value, common.vert_align))
        .unwrap_or(common.vert_align);
    common.horz_rel_to = json_string_alias(value, &["horz_rel_to", "horzRelTo"])
        .as_deref()
        .map(|value| horz_rel_to_from_name(value, common.horz_rel_to))
        .unwrap_or(common.horz_rel_to);
    common.horz_align = json_string_alias(value, &["horz_align", "horzAlign"])
        .as_deref()
        .map(|value| obj_horz_align_from_name(value, common.horz_align))
        .unwrap_or(common.horz_align);
    common.text_wrap = json_string_alias(value, &["text_wrap", "textWrap"])
        .as_deref()
        .map(|value| text_wrap_from_name(value, common.text_wrap))
        .unwrap_or(common.text_wrap);
    common.text_flow = json_string_alias(value, &["text_flow", "textFlow"])
        .as_deref()
        .map(|value| text_flow_from_name(value, common.text_flow))
        .unwrap_or(common.text_flow);
    common.width_criterion = json_string_alias(value, &["width_criterion", "widthCriterion"])
        .as_deref()
        .map(|value| size_criterion_from_name(value, common.width_criterion))
        .unwrap_or(common.width_criterion);
    common.height_criterion = json_string_alias(value, &["height_criterion", "heightCriterion"])
        .as_deref()
        .map(|value| size_criterion_from_name(value, common.height_criterion))
        .unwrap_or(common.height_criterion);
    common.horizontal_offset = json_u32_alias(
        value,
        "horizontal_offset",
        &["horizontalOffset", "horzOffset"],
    )
    .unwrap_or(common.horizontal_offset);
    common.vertical_offset =
        json_u32_alias(value, "vertical_offset", &["verticalOffset", "vertOffset"])
            .unwrap_or(common.vertical_offset);
    if let Some(description) = value.get("description").and_then(Value::as_str) {
        common.description = description.to_string();
    }
    if let Some(dropcap_style) = json_string_alias(value, &["dropcap_style", "dropcapStyle"]) {
        common.dropcap_style = object_dropcap_style_from_name(&dropcap_style);
    }
    if let Some(inst_id) = json_u32_alias(value, "inst_id", &["instId", "instid"]) {
        common.inst_id = inst_id;
    }
    if let Some(href) = value.get("href").and_then(Value::as_str) {
        common.href = if href.is_empty() {
            None
        } else {
            Some(href.to_string())
        };
    }
    let numbering_type = json_string_alias(value, &["numbering_type", "numberingType"]);
    common.numbering_type = numbering_type
        .as_deref()
        .map(|value| object_numbering_type_from_name(value, common.numbering_type))
        .unwrap_or(common.numbering_type);
    if numbering_type.is_some() {
        common.numbering_type_explicit = true;
    }
    common.numbering_type_explicit =
        json_bool_alias(value, "numbering_type_explicit", &["numberingTypeExplicit"])
            .unwrap_or(common.numbering_type_explicit);
    if !explicit_attr {
        common.attr =
            crate::document_core::converters::common_obj_attr_writer::pack_common_attr_bits(common);
    }
    Ok(())
}

#[cfg(test)]
mod object_layout_tests {
    use super::*;

    #[test]
    fn common_object_layout_preserves_href_and_inst_id() {
        let mut common = CommonObjAttr::default();
        common.instance_id = 10;
        common.inst_id = 20;
        common.dropcap_style = Some("TripleLine".to_string());
        common.href = Some("?LinkedShape;0;0;0;".to_string());

        let layout = common_object_layout_template(&common).expect("object layout");
        assert_eq!(layout.get("inst_id").and_then(Value::as_u64), Some(20));
        assert_eq!(
            layout.get("dropcap_style").and_then(Value::as_str),
            Some("TripleLine")
        );
        assert_eq!(
            layout.get("href").and_then(Value::as_str),
            Some("?LinkedShape;0;0;0;")
        );

        let mut regenerated = CommonObjAttr::default();
        apply_common_object_layout(&mut regenerated, Some(&layout)).expect("apply layout");
        assert_eq!(regenerated.inst_id, 20);
        assert_eq!(regenerated.dropcap_style.as_deref(), Some("TripleLine"));
        assert_eq!(regenerated.href.as_deref(), Some("?LinkedShape;0;0;0;"));
    }

    #[test]
    fn common_object_layout_accepts_hwpx_aliases_and_enum_variants() {
        let layout = json!({
            "zOrder": 7,
            "instanceId": 101,
            "treatAsChar": true,
            "flowWithText": false,
            "allowOverlap": true,
            "sizeProtect": true,
            "lock": true,
            "preventPageBreak": 1,
            "vertRelTo": "page",
            "vertAlign": "inside",
            "horzRelTo": "COLUMN",
            "horzAlign": "right",
            "textWrap": "in-front-of-text",
            "textFlow": "RIGHT ONLY",
            "widthCriterion": "paragraph",
            "heightCriterion": "ABSOLUTE",
            "horizontalOffset": 321,
            "verticalOffset": 654,
            "dropcapStyle": "triple-line",
            "instId": 202,
            "numberingType": "PICTURE",
            "numberingTypeExplicit": true
        });

        let mut common = CommonObjAttr::default();
        apply_common_object_layout(&mut common, Some(&layout)).expect("apply object layout");

        assert_eq!(common.z_order, 7);
        assert_eq!(common.instance_id, 101);
        assert!(common.treat_as_char);
        assert!(!common.flow_with_text);
        assert!(common.allow_overlap);
        assert!(common.size_protect);
        assert!(common.lock);
        assert_eq!(common.prevent_page_break, 1);
        assert_eq!(common.vert_rel_to, VertRelTo::Page);
        assert_eq!(common.vert_align, VertAlign::Inside);
        assert_eq!(common.horz_rel_to, HorzRelTo::Column);
        assert_eq!(common.horz_align, HorzAlign::Right);
        assert_eq!(common.text_wrap, TextWrap::InFrontOfText);
        assert_eq!(common.text_flow, TextFlow::RightOnly);
        assert_eq!(common.width_criterion, SizeCriterion::Para);
        assert_eq!(common.height_criterion, SizeCriterion::Absolute);
        assert_eq!(common.horizontal_offset, 321);
        assert_eq!(common.vertical_offset, 654);
        assert_eq!(common.dropcap_style.as_deref(), Some("TripleLine"));
        assert_eq!(common.inst_id, 202);
        assert_eq!(common.numbering_type, ObjectNumberingType::Picture);
        assert!(common.numbering_type_explicit);
    }
}

fn apply_template_cell_layouts(
    table: &mut Table,
    cell_layouts: &[Vec<TemplateCellLayout>],
    cell_border_fill_ids: &[Vec<Option<u16>>],
) -> Result<(), String> {
    if cell_layouts.is_empty() {
        return Ok(());
    }

    let row_count = table.row_count.max(1) as usize;
    let col_count = table.col_count.max(1) as usize;
    let mut spans = Vec::new();

    for row_idx in 0..row_count {
        for col_idx in 0..col_count {
            let Some(layout) = cell_layouts.get(row_idx).and_then(|row| row.get(col_idx)) else {
                continue;
            };
            if let Some(cell) = table
                .cells
                .iter_mut()
                .find(|cell| cell.row as usize == row_idx && cell.col as usize == col_idx)
            {
                let border_fill_id = cell_border_fill_ids
                    .get(row_idx)
                    .and_then(|row| row.get(col_idx))
                    .copied()
                    .flatten();
                apply_template_cell_layout(cell, layout, border_fill_id);
                let col_span = layout.col_span.max(1) as usize;
                let row_span = layout.row_span.max(1) as usize;
                if col_span > 1 || row_span > 1 {
                    if row_idx + row_span > row_count || col_idx + col_span > col_count {
                        return Err(format!(
                            "template cell_layout span out of range: row={row_idx}, col={col_idx}, row_span={row_span}, col_span={col_span}"
                        ));
                    }
                    spans.push((
                        row_idx as u16,
                        col_idx as u16,
                        row_span as u16,
                        col_span as u16,
                    ));
                }
            }
        }
    }

    spans.sort_unstable();
    for (row, col, row_span, col_span) in spans {
        table.merge_cells(row, col, row + row_span - 1, col + col_span - 1)?;
    }
    table.update_ctrl_dimensions();
    table.dirty = true;
    table.rebuild_grid();
    Ok(())
}

fn apply_template_cell_layout(
    cell: &mut Cell,
    layout: &TemplateCellLayout,
    border_fill_id: Option<u16>,
) {
    cell.padding = Padding {
        left: layout.padding_left,
        right: layout.padding_right,
        top: layout.padding_top,
        bottom: layout.padding_bottom,
    };
    cell.set_apply_inner_margin(layout.apply_inner_margin);
    cell.vertical_align = vertical_align_from_name(&layout.vertical_align);
    cell.text_direction = layout.text_direction;
    cell.set_header(layout.is_header);
    cell.set_cell_protect(layout.cell_protect);
    cell.set_editable_in_form(layout.editable_in_form);
    cell.dirty = layout.dirty;
    cell.sub_list_line_wrap = layout.line_wrap.clone();
    cell.sub_list_link_list_id_ref = layout.link_list_id_ref;
    cell.sub_list_link_list_next_id_ref = layout.link_list_next_id_ref;
    cell.sub_list_text_width = layout.text_width;
    cell.sub_list_text_height = layout.text_height;
    cell.sub_list_text_ref = layout.has_text_ref;
    cell.sub_list_num_ref = layout.has_num_ref;
    cell.field_name = if layout.field_name.is_empty() {
        None
    } else {
        Some(layout.field_name.clone())
    };
    if let Some(border_fill_id) = border_fill_id {
        cell.border_fill_id = border_fill_id;
    }
}

fn apply_row_heights_to_table(table: &mut Table, row_heights: &[u32]) {
    for cell in &mut table.cells {
        let start = cell.row as usize;
        if start >= row_heights.len() {
            continue;
        }
        let span = cell.row_span.max(1) as usize;
        let end = (start + span).min(row_heights.len());
        cell.height = row_heights[start..end].iter().sum();
    }
    table.rebuild_grid();
}

fn table_layout_padding(layout: &TemplateTableLayout) -> Padding {
    Padding {
        left: layout.padding_left,
        right: layout.padding_right,
        top: layout.padding_top,
        bottom: layout.padding_bottom,
    }
}

fn apply_table_layout_to_table(table: &mut Table, layout: &TemplateTableLayout) {
    let padding = table_layout_padding(layout);
    table.cell_spacing = layout.cell_spacing;
    table.padding = padding;
    for cell in &mut table.cells {
        if !cell.apply_inner_margin {
            cell.padding = padding;
        }
    }
    table.page_break = table_page_break_from_name(&layout.page_break);
    table.repeat_header = layout.repeat_header;
    table.outer_margin_left = layout.outer_margin_left;
    table.outer_margin_right = layout.outer_margin_right;
    table.outer_margin_top = layout.outer_margin_top;
    table.outer_margin_bottom = layout.outer_margin_bottom;
    table.common.margin.left = layout.outer_margin_left;
    table.common.margin.right = layout.outer_margin_right;
    table.common.margin.top = layout.outer_margin_top;
    table.common.margin.bottom = layout.outer_margin_bottom;

    while table.raw_ctrl_data.len() < common_obj_offsets::MARGIN_BOTTOM.end {
        table.raw_ctrl_data.push(0);
    }
    table.raw_ctrl_data[common_obj_offsets::MARGIN_LEFT]
        .copy_from_slice(&layout.outer_margin_left.to_le_bytes());
    table.raw_ctrl_data[common_obj_offsets::MARGIN_RIGHT]
        .copy_from_slice(&layout.outer_margin_right.to_le_bytes());
    table.raw_ctrl_data[common_obj_offsets::MARGIN_TOP]
        .copy_from_slice(&layout.outer_margin_top.to_le_bytes());
    table.raw_ctrl_data[common_obj_offsets::MARGIN_BOTTOM]
        .copy_from_slice(&layout.outer_margin_bottom.to_le_bytes());

    table.raw_table_record_attr = table_record_attr(
        table.raw_table_record_attr,
        table.page_break,
        table.repeat_header,
    );
    table.dirty = true;
}

fn table_record_attr(base: u32, page_break: TablePageBreak, repeat_header: bool) -> u32 {
    let mut attr = base & !0x07;
    match page_break {
        TablePageBreak::CellBreak => attr |= 0x01,
        TablePageBreak::RowBreak => attr |= 0x02,
        TablePageBreak::None => {}
    }
    if repeat_header {
        attr |= 0x04;
    }
    attr
}

fn reflow_table_cell_paragraphs(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
) {
    let counts = core
        .document
        .sections
        .get(section_idx)
        .and_then(|section| section.paragraphs.get(table_para))
        .and_then(|para| para.controls.get(table_control))
        .and_then(|control| match control {
            Control::Table(table) => Some(
                table
                    .cells
                    .iter()
                    .enumerate()
                    .map(|(cell_idx, cell)| (cell_idx, cell.paragraphs.len()))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    for (cell_idx, para_count) in counts {
        for cell_para_idx in 0..para_count {
            core.reflow_cell_paragraph(
                section_idx,
                table_para,
                table_control,
                cell_idx,
                cell_para_idx,
            );
        }
    }
}

fn restore_table_cell_line_segments(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    col_count: usize,
    cell_blocks: &[Vec<Vec<TemplateBlock>>],
) {
    if cell_blocks.is_empty() {
        return;
    }
    let table = match core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(table_para))
        .and_then(|para| para.controls.get_mut(table_control))
    {
        Some(Control::Table(table)) => table,
        _ => return,
    };
    let mut changed = false;
    for (row_idx, row) in cell_blocks.iter().enumerate() {
        for (col_idx, blocks) in row.iter().enumerate() {
            let cell_idx = row_idx * col_count + col_idx;
            let Some(cell) = table.cells.get_mut(cell_idx) else {
                continue;
            };
            for (block_idx, block) in blocks.iter().enumerate() {
                let TemplateBlock::Paragraph { line_segments, .. } = block else {
                    continue;
                };
                if line_segments.is_empty() {
                    continue;
                }
                if let Some(para) = cell.paragraphs.get_mut(block_idx) {
                    apply_template_line_segments(para, line_segments);
                    changed = true;
                }
            }
        }
    }
    if changed {
        table.dirty = true;
        core.document.sections[section_idx].raw_stream = None;
        core.rebuild_section(section_idx);
    }
}

fn build_cell_paragraph_overrides(
    core: &mut DocumentCore,
    section_idx: usize,
    row_count: usize,
    col_count: usize,
    cell_blocks: &[Vec<Vec<TemplateBlock>>],
) -> Result<Vec<Vec<Option<Vec<Paragraph>>>>, String> {
    let mut overrides = vec![vec![None; col_count]; row_count];
    for row_idx in 0..row_count {
        for col_idx in 0..col_count {
            let Some(blocks) = cell_blocks.get(row_idx).and_then(|row| row.get(col_idx)) else {
                continue;
            };
            if blocks.is_empty() {
                continue;
            }
            overrides[row_idx][col_idx] = Some(paragraphs_from_blocks(core, section_idx, blocks)?);
        }
    }
    Ok(overrides)
}

fn paragraphs_from_blocks(
    core: &mut DocumentCore,
    section_idx: usize,
    blocks: &[TemplateBlock],
) -> Result<Vec<Paragraph>, String> {
    if blocks.is_empty() {
        return Ok(vec![Paragraph::new_empty()]);
    }

    let mut paragraphs = Vec::new();
    for block in blocks {
        match block {
            TemplateBlock::Paragraph {
                text,
                style,
                char_format,
                char_shape_runs,
                para_format,
                line_segments,
                ..
            } => {
                let mut para = formatted_text_paragraph(
                    core,
                    section_idx,
                    text,
                    style.as_ref(),
                    char_format.as_ref(),
                    char_shape_runs,
                    para_format.as_ref(),
                );
                para.para_shape_id = ensure_cell_block_para_shape_for_template(
                    core,
                    section_idx,
                    para_format.as_ref(),
                );
                apply_template_line_segments(&mut para, line_segments);
                paragraphs.push(para);
            }
            TemplateBlock::Table {
                rows,
                break_before: _,
                rhwp_saved_gap_before: _,
                host_group: _,
                style,
                host_para_shape_id: _,
                para_format,
                line_segments,
                caption,
                column_widths,
                row_heights,
                table_layout,
                object_layout,
                border_fill,
                table_zones,
                cell_layouts,
                cell_formats,
                cell_blocks,
            } => {
                let table = template_table(
                    core,
                    section_idx,
                    rows,
                    caption.as_ref(),
                    column_widths,
                    row_heights,
                    table_layout.as_ref(),
                    object_layout.as_ref(),
                    border_fill.as_ref(),
                    table_zones,
                    cell_layouts,
                    cell_formats,
                    cell_blocks,
                )?;
                let style_id = style
                    .as_ref()
                    .and_then(|style| valid_template_style_ref(core, style))
                    .map(|style| style.id);
                let para_shape_id = ensure_cell_block_para_shape_for_template(
                    core,
                    section_idx,
                    para_format.as_ref(),
                );
                if let Some(prev) = paragraphs
                    .last_mut()
                    .filter(|para| para.controls.is_empty() && para.text.trim().is_empty())
                {
                    if let Some(style_id) = style_id {
                        prev.style_id = style_id;
                    }
                    prev.para_shape_id = para_shape_id;
                    apply_template_line_segments(prev, line_segments);
                    attach_table_to_paragraph(prev, table);
                } else {
                    let mut para = table_paragraph(
                        core,
                        section_idx,
                        table,
                        style.as_ref(),
                        0,
                        para_format.as_ref(),
                        false,
                        line_segments,
                    );
                    para.para_shape_id = para_shape_id;
                    paragraphs.push(para);
                }
            }
            TemplateBlock::Equation {
                script,
                break_before: _,
                host_group: _,
                font_size,
                color,
                baseline,
                font_name,
                line_mode,
                width,
                height,
                treat_as_char,
            } => paragraphs.push(equation_paragraph(template_equation(
                script,
                *font_size,
                *color,
                *baseline,
                font_name,
                line_mode,
                *width,
                *height,
                *treat_as_char,
            ))),
            TemplateBlock::Picture {
                break_before: _,
                host_group: _,
                line_segments,
                image_base64,
                external_path,
                extension,
                width,
                height,
                natural_width_px,
                natural_height_px,
                description,
                transparency,
                brightness,
                contrast,
                effect,
                effects,
                layout,
                object_layout,
                treat_as_char,
                horz_offset,
                vert_offset,
                caption,
            } => {
                let caption = template_caption(core, section_idx, caption.as_ref())?;
                paragraphs.push(picture_paragraph_with_line_segments(
                    template_picture(
                        core,
                        image_base64,
                        external_path,
                        extension,
                        *width,
                        *height,
                        *natural_width_px,
                        *natural_height_px,
                        description,
                        *transparency,
                        *brightness,
                        *contrast,
                        effect,
                        effects.as_ref(),
                        layout.as_ref(),
                        object_layout.as_ref(),
                        *treat_as_char,
                        *horz_offset,
                        *vert_offset,
                        caption,
                    )?,
                    line_segments,
                ));
            }
            TemplateBlock::ObjectPlaceholder {
                object_kind,
                break_before: _,
                host_group: _,
                shape_kind,
                description,
                placeholder_text,
                width,
                height,
                treat_as_char,
                horz_offset,
                vert_offset,
                caption,
                shape_component_id,
                geometry,
                drawing_style,
                layout,
                children,
                raw_hwp_chart_data_base64,
                raw_hwp_ole_tag_base64,
                ole_bin_data_base64,
                ole_extension,
                ole_object_type,
                ole_draw_aspect,
                ole_eq_base_line,
                ole_has_moniker,
            } => paragraphs.push(shape_paragraph(template_object_placeholder(
                core,
                section_idx,
                object_kind,
                shape_kind,
                description,
                placeholder_text,
                *width,
                *height,
                *treat_as_char,
                *horz_offset,
                *vert_offset,
                caption.as_ref(),
                *shape_component_id,
                geometry.as_ref(),
                drawing_style.as_ref(),
                layout.as_ref(),
                children,
                raw_hwp_chart_data_base64,
                raw_hwp_ole_tag_base64,
                ole_bin_data_base64,
                ole_extension,
                ole_object_type,
                ole_draw_aspect,
                ole_eq_base_line,
                ole_has_moniker,
            )?)),
        }
    }
    Ok(paragraphs)
}

fn ensure_char_shape_for_template(
    core: &mut DocumentCore,
    base_id: u32,
    char_format: Option<&Value>,
) -> u32 {
    let Some(format) = char_format else {
        return base_id;
    };
    if format.as_object().map(Map::is_empty).unwrap_or(true) {
        return base_id;
    }
    let props = match serde_json::to_string(format) {
        Ok(props) => props,
        Err(_) => return base_id,
    };
    let mods = parse_char_shape_mods(&props);
    core.document.find_or_create_char_shape(base_id, &mods)
}

fn ensure_para_shape_for_template(
    core: &mut DocumentCore,
    base_id: u16,
    para_format: Option<&Value>,
) -> u16 {
    let Some(format) = para_format else {
        return base_id;
    };
    if format.as_object().map(Map::is_empty).unwrap_or(true) {
        return base_id;
    }
    let props = match serde_json::to_string(format) {
        Ok(props) => props,
        Err(_) => return base_id,
    };
    let mods = parse_para_shape_mods(&props);
    core.document.find_or_create_para_shape(base_id, &mods)
}

fn ensure_cell_block_para_shape_for_template(
    core: &mut DocumentCore,
    section_idx: usize,
    para_format: Option<&Value>,
) -> u16 {
    let base_id = if core.document.doc_info.para_shapes.is_empty() {
        default_shape_ids(core, section_idx).1
    } else {
        0
    };
    ensure_para_shape_for_template(core, base_id, para_format)
}

fn apply_table_cell_blocks(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    col_count: usize,
    cell_blocks: &[Vec<Vec<TemplateBlock>>],
) -> Result<(), String> {
    let row_count = cell_blocks.len();
    if row_count == 0 {
        return Ok(());
    }

    let mut overrides =
        build_cell_paragraph_overrides(core, section_idx, row_count, col_count, cell_blocks)?;
    let table = match core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(table_para))
        .and_then(|para| para.controls.get_mut(table_control))
    {
        Some(Control::Table(table)) => table,
        _ => return Err("created table control was not found".to_string()),
    };

    for row_idx in 0..row_count {
        let Some(row) = overrides.get_mut(row_idx) else {
            continue;
        };
        for col_idx in 0..col_count {
            let Some(Some(paragraphs)) = row.get_mut(col_idx) else {
                continue;
            };
            let cell_idx = row_idx * col_count + col_idx;
            if let Some(cell) = table.cells.get_mut(cell_idx) {
                cell.paragraphs = std::mem::take(paragraphs);
            }
        }
    }
    table.dirty = true;
    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn table_content_width(core: &DocumentCore, section_idx: usize) -> u32 {
    let pd = &core.document.sections[section_idx].section_def.page_def;
    let outer_margin_lr: i32 = 283 * 2;
    (pd.width as i32 - pd.margin_left as i32 - pd.margin_right as i32 - outer_margin_lr).max(7200)
        as u32
}

fn default_shape_ids(core: &DocumentCore, section_idx: usize) -> (u32, u16) {
    core.document.sections[section_idx]
        .paragraphs
        .first()
        .map(|para| {
            (
                para.char_shapes
                    .first()
                    .map(|shape| shape.char_shape_id)
                    .unwrap_or(0),
                para.para_shape_id,
            )
        })
        .unwrap_or((0, 0))
}

fn solid_border_fill_id(core: &mut DocumentCore) -> u16 {
    if let Some(idx) = core.document.doc_info.border_fills.iter().position(|bf| {
        bf.borders
            .iter()
            .all(|border| border.line_type == BorderLineType::Solid && border.width >= 1)
    }) {
        return (idx + 1) as u16;
    }

    let solid_border = BorderLine {
        line_type: BorderLineType::Solid,
        width: 1,
        color: 0,
    };
    core.document.doc_info.border_fills.push(BorderFill {
        raw_data: None,
        raw_hwpx_children: None,
        attr: 0,
        three_d: false,
        shadow: false,
        center_line: None,
        break_cell_separate_line: false,
        borders: [solid_border, solid_border, solid_border, solid_border],
        diagonal: DiagonalLine {
            diagonal_type: 1,
            width: 0,
            color: 0,
        },
        fill: Fill::default(),
    });
    core.document.doc_info.raw_stream = None;
    core.document.doc_info.border_fills.len() as u16
}

fn resolve_cell_layout_border_fills(
    core: &mut DocumentCore,
    cell_layouts: &[Vec<TemplateCellLayout>],
) -> Result<Vec<Vec<Option<u16>>>, String> {
    if cell_layouts.is_empty() {
        return Ok(Vec::new());
    }
    cell_layouts
        .iter()
        .map(|row| {
            row.iter()
                .map(|layout| {
                    layout
                        .border_fill
                        .as_ref()
                        .map(|border_fill| ensure_border_fill_for_template(core, border_fill))
                        .transpose()
                })
                .collect()
        })
        .collect()
}

#[derive(Debug, Clone)]
struct ResolvedTemplateTableZone {
    start_row: u16,
    start_col: u16,
    end_row: u16,
    end_col: u16,
    border_fill_id: Option<u16>,
}

fn resolve_table_zones(
    core: &mut DocumentCore,
    table_zones: &[TemplateTableZone],
) -> Result<Vec<ResolvedTemplateTableZone>, String> {
    table_zones
        .iter()
        .map(|zone| {
            Ok(ResolvedTemplateTableZone {
                start_row: zone.start_row,
                start_col: zone.start_col,
                end_row: zone.end_row,
                end_col: zone.end_col,
                border_fill_id: zone
                    .border_fill
                    .as_ref()
                    .map(|border_fill| ensure_border_fill_for_template(core, border_fill))
                    .transpose()?,
            })
        })
        .collect()
}

fn materialize_table_zones(
    resolved: &[ResolvedTemplateTableZone],
    default_border_fill_id: u16,
    row_count: u16,
    col_count: u16,
) -> Result<Vec<TableZone>, String> {
    resolved
        .iter()
        .map(|zone| {
            if zone.end_row < zone.start_row || zone.end_col < zone.start_col {
                return Err(format!(
                    "template table_zone range is invalid: start=({}, {}), end=({}, {})",
                    zone.start_row, zone.start_col, zone.end_row, zone.end_col
                ));
            }
            if zone.end_row >= row_count || zone.end_col >= col_count {
                return Err(format!(
                    "template table_zone range out of table bounds: end=({}, {}), rows={}, cols={}",
                    zone.end_row, zone.end_col, row_count, col_count
                ));
            }
            Ok(TableZone {
                start_row: zone.start_row,
                start_col: zone.start_col,
                end_row: zone.end_row,
                end_col: zone.end_col,
                border_fill_id: zone.border_fill_id.unwrap_or(default_border_fill_id),
            })
        })
        .collect()
}

fn ensure_border_fill_for_template(
    core: &mut DocumentCore,
    border_fill: &Value,
) -> Result<u16, String> {
    let border_fill = border_fill_from_json(core, border_fill)?;
    if border_fill.fill.image.is_none() {
        if let Some(idx) = core
            .document
            .doc_info
            .border_fills
            .iter()
            .position(|existing| border_fill_equivalent(existing, &border_fill))
        {
            return Ok((idx + 1) as u16);
        }
    }

    core.document.doc_info.border_fills.push(border_fill);
    core.document.doc_info.raw_stream = None;
    core.document.doc_info.raw_stream_dirty = true;
    core.styles = resolve_styles(&core.document.doc_info, core.dpi);
    Ok(core.document.doc_info.border_fills.len() as u16)
}

fn border_fill_equivalent(left: &BorderFill, right: &BorderFill) -> bool {
    let left_center = left
        .center_line
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("NONE");
    let right_center = right
        .center_line
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("NONE");
    if (left.three_d || (left.attr & 0x0001) != 0) != (right.three_d || (right.attr & 0x0001) != 0)
        || (left.shadow || (left.attr & 0x0002) != 0)
            != (right.shadow || (right.attr & 0x0002) != 0)
        || !left_center.eq_ignore_ascii_case(right_center)
        || left.break_cell_separate_line != right.break_cell_separate_line
    {
        return false;
    }
    crate::serializer::doc_info::serialize_border_fill(left)
        == crate::serializer::doc_info::serialize_border_fill(right)
}

fn prepare_cell_paragraph(
    para: &mut Paragraph,
    char_shape_id: u32,
    para_shape_id: u16,
    col_width: u32,
    row_height: u32,
    cell_pad: Padding,
) {
    para.char_count_msb = true;
    para.para_shape_id = para_shape_id;
    para.char_shapes = vec![CharShapeRef {
        start_pos: 0,
        char_shape_id,
    }];
    para.raw_header_extra = paragraph_header_extra();
    let segment_width = (col_width as i32) - cell_pad.left as i32 - cell_pad.right as i32;
    let text_height = row_height.saturating_sub((cell_pad.top + cell_pad.bottom) as u32);
    para.line_segs = vec![LineSeg {
        text_start: 0,
        line_height: text_height as i32,
        text_height: text_height as i32,
        baseline_distance: (text_height as f64 * 0.85) as i32,
        line_spacing: 600,
        segment_width,
        tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
        ..Default::default()
    }];
}

fn paragraph_header_extra() -> Vec<u8> {
    let mut raw = vec![0u8; 10];
    raw[0..2].copy_from_slice(&1u16.to_le_bytes());
    raw[4..6].copy_from_slice(&1u16.to_le_bytes());
    raw
}

fn table_ctrl_data(row_count: u16, col_count: u16, total_width: u32, total_height: u32) -> Vec<u8> {
    #[allow(clippy::identity_op)]
    let flags: u32 = (1 << 0) | (0 << 3) | (3 << 8) | (4 << 15) | (2 << 18) | (1 << 21);
    let outer_margin: i16 = 283;
    let mut raw_ctrl_data = vec![0u8; 38];
    raw_ctrl_data[common_obj_offsets::FLAGS].copy_from_slice(&flags.to_le_bytes());
    raw_ctrl_data[common_obj_offsets::WIDTH].copy_from_slice(&total_width.to_le_bytes());
    raw_ctrl_data[common_obj_offsets::HEIGHT].copy_from_slice(&total_height.to_le_bytes());
    raw_ctrl_data[common_obj_offsets::MARGIN_LEFT].copy_from_slice(&outer_margin.to_le_bytes());
    raw_ctrl_data[common_obj_offsets::MARGIN_RIGHT].copy_from_slice(&outer_margin.to_le_bytes());
    raw_ctrl_data[common_obj_offsets::MARGIN_TOP].copy_from_slice(&outer_margin.to_le_bytes());
    raw_ctrl_data[common_obj_offsets::MARGIN_BOTTOM].copy_from_slice(&outer_margin.to_le_bytes());
    let instance_id = 0x7c160000u32
        .wrapping_add(row_count as u32 * 0x1000)
        .wrapping_add(col_count as u32 * 0x100)
        .wrapping_add(total_width)
        .wrapping_add(total_height);
    raw_ctrl_data[common_obj_offsets::INSTANCE_ID].copy_from_slice(&instance_id.to_le_bytes());
    raw_ctrl_data
}

fn header_footer_paragraphs_mut(
    core: &mut DocumentCore,
    section_idx: usize,
    is_header: bool,
    apply_to: u8,
) -> Option<&mut Vec<Paragraph>> {
    let apply_to = apply_to_from_u8(apply_to);
    let section = core.document.sections.get_mut(section_idx)?;
    for para in &mut section.paragraphs {
        for control in &mut para.controls {
            match control {
                Control::Header(header) if is_header && header.apply_to == apply_to => {
                    return Some(&mut header.paragraphs);
                }
                Control::Footer(footer) if !is_header && footer.apply_to == apply_to => {
                    return Some(&mut footer.paragraphs);
                }
                _ => {}
            }
        }
    }
    None
}

fn apply_to_from_u8(apply_to: u8) -> HeaderFooterApply {
    match apply_to {
        1 => HeaderFooterApply::Even,
        2 => HeaderFooterApply::Odd,
        _ => HeaderFooterApply::Both,
    }
}

fn apply_to_u8(apply_to: HeaderFooterApply) -> u8 {
    match apply_to {
        HeaderFooterApply::Both => 0,
        HeaderFooterApply::Even => 1,
        HeaderFooterApply::Odd => 2,
    }
}

fn apply_body_formats(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    text: &str,
    char_format: Option<&Value>,
    para_format: Option<&Value>,
) -> Result<(), String> {
    if let Some(props) = para_format {
        let props = serde_json::to_string(props).map_err(|e| e.to_string())?;
        core.apply_para_format_native(section_idx, para_idx, &props)
            .map_err(|e| e.to_string())?;
    }
    if let Some(props) = char_format {
        let len = text.chars().count();
        if len > 0 {
            let props = serde_json::to_string(props).map_err(|e| e.to_string())?;
            core.apply_char_format_native(section_idx, para_idx, 0, len, &props)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn apply_body_char_shape_runs(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    runs: &[TemplateCharShapeRun],
) {
    let char_shape_count = core.document.doc_info.char_shapes.len();
    let Some(para) = core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(para_idx))
    else {
        return;
    };
    apply_template_char_shape_runs_with_count(char_shape_count, para, runs);
}

fn apply_cell_formats(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    cell_idx: usize,
    text: &str,
    format: &TemplateTextFormat,
) -> Result<(), String> {
    if let Some(props) = &format.para_format {
        let props = serde_json::to_string(props).map_err(|e| e.to_string())?;
        core.apply_para_format_in_cell_native(
            section_idx,
            table_para,
            table_control,
            cell_idx,
            0,
            &props,
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(props) = &format.char_format {
        let len = text.chars().count();
        if len > 0 {
            let props = serde_json::to_string(props).map_err(|e| e.to_string())?;
            core.apply_char_format_in_cell_native(
                section_idx,
                table_para,
                table_control,
                cell_idx,
                0,
                0,
                len,
                &props,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    if let Some(style) = &format.style {
        apply_cell_style_ref(
            core,
            section_idx,
            table_para,
            table_control,
            cell_idx,
            style,
        )?;
    }
    Ok(())
}

fn apply_body_paragraph_style(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    style: Option<&TemplateStyleRef>,
) {
    let Some(style) = style.and_then(|style| valid_template_style_ref(core, style)) else {
        return;
    };
    if let Some(para) = core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(para_idx))
    {
        para.style_id = style.id;
        core.document.sections[section_idx].raw_stream = None;
        core.rebuild_section(section_idx);
    }
}

fn apply_body_line_segments(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    line_segments: &[TemplateLineSeg],
) {
    if line_segments.is_empty() {
        return;
    }
    if let Some(para) = core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(para_idx))
    {
        apply_template_line_segments(para, line_segments);
        core.document.sections[section_idx].raw_stream = None;
        core.rebuild_section(section_idx);
    }
}

fn apply_body_saved_tac_gap_before(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    saved_gap_hu: Option<i32>,
) {
    let Some(saved_gap_hu) = saved_gap_hu.filter(|gap| *gap > 0) else {
        return;
    };
    if let Some(para) = core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(para_idx))
    {
        para.rhwp_saved_tac_gap_before = saved_gap_hu;
    }
}

fn apply_body_saved_para_gap_before(
    core: &mut DocumentCore,
    section_idx: usize,
    para_idx: usize,
    saved_gap_hu: Option<i32>,
) {
    let Some(saved_gap_hu) = saved_gap_hu.filter(|gap| *gap > 0) else {
        return;
    };
    if let Some(para) = core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(para_idx))
    {
        para.rhwp_saved_para_gap_before = saved_gap_hu;
    }
}

fn apply_cell_style_ref(
    core: &mut DocumentCore,
    section_idx: usize,
    table_para: usize,
    table_control: usize,
    cell_idx: usize,
    style: &TemplateStyleRef,
) -> Result<(), String> {
    let Some(style) = valid_template_style_ref(core, style) else {
        return Ok(());
    };
    let table = core
        .document
        .sections
        .get_mut(section_idx)
        .and_then(|section| section.paragraphs.get_mut(table_para))
        .and_then(|para| para.controls.get_mut(table_control))
        .and_then(|control| match control {
            Control::Table(table) => Some(table),
            _ => None,
        })
        .ok_or_else(|| "created table control was not found".to_string())?;
    if let Some(cell_para) = table
        .cells
        .get_mut(cell_idx)
        .and_then(|cell| cell.paragraphs.first_mut())
    {
        cell_para.style_id = style.id;
    }
    table.dirty = true;
    core.document.sections[section_idx].raw_stream = None;
    core.rebuild_section(section_idx);
    Ok(())
}

fn valid_template_style_ref<'a>(
    core: &DocumentCore,
    style: &'a TemplateStyleRef,
) -> Option<&'a TemplateStyleRef> {
    ((style.id as usize) < core.document.doc_info.styles.len()).then_some(style)
}

fn style_ref_json(core: &DocumentCore, para: &Paragraph) -> Option<TemplateStyleRef> {
    if para.style_id == 0 {
        return None;
    }
    let style = core.document.doc_info.styles.get(para.style_id as usize)?;
    Some(TemplateStyleRef {
        id: para.style_id,
        name: style.local_name.clone(),
        english_name: style.english_name.clone(),
    })
}

fn char_format_json(core: &DocumentCore, para: &Paragraph) -> Option<Value> {
    let char_shape_id = para.char_shape_id_at(0).unwrap_or(0);
    let shape = core
        .document
        .doc_info
        .char_shapes
        .get(char_shape_id as usize)?;
    let default = core.document.doc_info.char_shapes.first();
    char_shape_format_json(shape, default)
}

fn char_shape_format_json(shape: &CharShape, default: Option<&CharShape>) -> Option<Value> {
    let mut props = Map::new();
    let default_base_size = default.map(|s| s.base_size).unwrap_or(1000);
    if shape.base_size != default_base_size {
        props.insert("fontSize".to_string(), json!(shape.base_size));
    }
    if default.map(|s| s.bold) != Some(shape.bold) {
        props.insert("bold".to_string(), json!(shape.bold));
    }
    if default.map(|s| s.italic) != Some(shape.italic) {
        props.insert("italic".to_string(), json!(shape.italic));
    }
    if default.map(|s| s.strikethrough) != Some(shape.strikethrough) {
        props.insert("strikethrough".to_string(), json!(shape.strikethrough));
    }
    if default.map(|s| s.underline_type) != Some(shape.underline_type) {
        props.insert(
            "underlineType".to_string(),
            json!(underline_type_str(shape.underline_type)),
        );
    }
    if default.map(|s| s.text_color) != Some(shape.text_color) {
        props.insert("textColor".to_string(), json!(bgr_to_css(shape.text_color)));
    }
    if props.is_empty() {
        None
    } else {
        Some(Value::Object(props))
    }
}

fn para_format_json(core: &DocumentCore, para: &Paragraph) -> Option<Value> {
    let shape = core
        .document
        .doc_info
        .para_shapes
        .get(para.para_shape_id as usize)?;
    let default = core.document.doc_info.para_shapes.first();
    para_shape_format_json(shape, default)
}

fn para_shape_format_json(shape: &ParaShape, default: Option<&ParaShape>) -> Option<Value> {
    let mut props = Map::new();
    if default.map(|s| s.alignment) != Some(shape.alignment) {
        props.insert(
            "alignment".to_string(),
            json!(alignment_str(shape.alignment)),
        );
    }
    if default.map(|s| s.line_spacing) != Some(shape.line_spacing) {
        props.insert("lineSpacing".to_string(), json!(shape.line_spacing));
    }
    if default.map(|s| s.line_spacing_type) != Some(shape.line_spacing_type) {
        props.insert(
            "lineSpacingType".to_string(),
            json!(line_spacing_type_str(shape.line_spacing_type)),
        );
    }
    if default.map(|s| s.indent) != Some(shape.indent) {
        props.insert("indent".to_string(), json!(shape.indent));
    }
    if default.map(|s| s.margin_left) != Some(shape.margin_left) {
        props.insert("marginLeft".to_string(), json!(shape.margin_left));
    }
    if default.map(|s| s.margin_right) != Some(shape.margin_right) {
        props.insert("marginRight".to_string(), json!(shape.margin_right));
    }
    if default.map(|s| s.spacing_before) != Some(shape.spacing_before) {
        props.insert("spacingBefore".to_string(), json!(shape.spacing_before));
    }
    if default.map(|s| s.spacing_after) != Some(shape.spacing_after) {
        props.insert("spacingAfter".to_string(), json!(shape.spacing_after));
    }
    if props.is_empty() {
        None
    } else {
        Some(Value::Object(props))
    }
}

fn underline_type_str(value: UnderlineType) -> &'static str {
    match value {
        UnderlineType::Bottom => "Bottom",
        UnderlineType::Top => "Top",
        UnderlineType::None => "None",
    }
}

fn non_default_underline_type(value: UnderlineType) -> String {
    match value {
        UnderlineType::None => String::new(),
        _ => underline_type_str(value).to_string(),
    }
}

fn underline_type_from_template(value: &str) -> UnderlineType {
    match value {
        "Bottom" | "bottom" | "BOTTOM" | "1" => UnderlineType::Bottom,
        "Top" | "top" | "TOP" | "3" => UnderlineType::Top,
        _ => UnderlineType::None,
    }
}

fn alignment_str(value: Alignment) -> &'static str {
    match value {
        Alignment::Left => "left",
        Alignment::Right => "right",
        Alignment::Center => "center",
        Alignment::Distribute => "distribute",
        Alignment::Justify | Alignment::Split => "justify",
    }
}

fn vertical_align_name(value: VerticalAlign) -> &'static str {
    match value {
        VerticalAlign::Top => "top",
        VerticalAlign::Center => "center",
        VerticalAlign::Bottom => "bottom",
    }
}

fn vertical_align_from_name(value: &str) -> VerticalAlign {
    match value {
        "bottom" | "BOTTOM" | "2" => VerticalAlign::Bottom,
        "center" | "middle" | "CENTER" | "MIDDLE" | "1" => VerticalAlign::Center,
        _ => VerticalAlign::Top,
    }
}

fn object_layout_enum_key(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| !matches!(ch, '_' | '-' | ' '))
        .collect::<String>()
        .to_ascii_uppercase()
}

fn vert_rel_to_name(value: VertRelTo) -> &'static str {
    match value {
        VertRelTo::Paper => "paper",
        VertRelTo::Page => "page",
        VertRelTo::Para => "para",
    }
}

fn vert_rel_to_from_name(value: &str, fallback: VertRelTo) -> VertRelTo {
    match object_layout_enum_key(value).as_str() {
        "PAPER" | "0" => VertRelTo::Paper,
        "PAGE" | "1" => VertRelTo::Page,
        "PARA" | "PARAGRAPH" | "2" => VertRelTo::Para,
        _ => fallback,
    }
}

fn obj_vert_align_name(value: VertAlign) -> &'static str {
    match value {
        VertAlign::Top => "top",
        VertAlign::Center => "center",
        VertAlign::Bottom => "bottom",
        VertAlign::Inside => "inside",
        VertAlign::Outside => "outside",
    }
}

fn obj_vert_align_from_name(value: &str, fallback: VertAlign) -> VertAlign {
    match object_layout_enum_key(value).as_str() {
        "TOP" | "0" => VertAlign::Top,
        "CENTER" | "MIDDLE" | "1" => VertAlign::Center,
        "BOTTOM" | "2" => VertAlign::Bottom,
        "INSIDE" | "3" => VertAlign::Inside,
        "OUTSIDE" | "4" => VertAlign::Outside,
        _ => fallback,
    }
}

fn horz_rel_to_name(value: HorzRelTo) -> &'static str {
    match value {
        HorzRelTo::Paper => "paper",
        HorzRelTo::Page => "page",
        HorzRelTo::Column => "column",
        HorzRelTo::Para => "para",
    }
}

fn horz_rel_to_from_name(value: &str, fallback: HorzRelTo) -> HorzRelTo {
    match object_layout_enum_key(value).as_str() {
        "PAPER" | "0" => HorzRelTo::Paper,
        "PAGE" | "1" => HorzRelTo::Page,
        "COLUMN" | "2" => HorzRelTo::Column,
        "PARA" | "PARAGRAPH" | "3" => HorzRelTo::Para,
        _ => fallback,
    }
}

fn obj_horz_align_name(value: HorzAlign) -> &'static str {
    match value {
        HorzAlign::Left => "left",
        HorzAlign::Center => "center",
        HorzAlign::Right => "right",
        HorzAlign::Inside => "inside",
        HorzAlign::Outside => "outside",
    }
}

fn obj_horz_align_from_name(value: &str, fallback: HorzAlign) -> HorzAlign {
    match object_layout_enum_key(value).as_str() {
        "LEFT" | "0" => HorzAlign::Left,
        "CENTER" | "MIDDLE" | "1" => HorzAlign::Center,
        "RIGHT" | "2" => HorzAlign::Right,
        "INSIDE" | "3" => HorzAlign::Inside,
        "OUTSIDE" | "4" => HorzAlign::Outside,
        _ => fallback,
    }
}

fn text_wrap_name(value: TextWrap) -> &'static str {
    match value {
        TextWrap::Square => "square",
        TextWrap::Tight => "tight",
        TextWrap::Through => "through",
        TextWrap::TopAndBottom => "top_and_bottom",
        TextWrap::BehindText => "behind_text",
        TextWrap::InFrontOfText => "in_front_of_text",
    }
}

fn text_wrap_from_name(value: &str, fallback: TextWrap) -> TextWrap {
    match object_layout_enum_key(value).as_str() {
        "SQUARE" | "0" => TextWrap::Square,
        "TIGHT" => TextWrap::Tight,
        "THROUGH" => TextWrap::Through,
        "TOPANDBOTTOM" | "1" => TextWrap::TopAndBottom,
        "BEHINDTEXT" | "2" => TextWrap::BehindText,
        "INFRONTOFTEXT" | "3" => TextWrap::InFrontOfText,
        _ => fallback,
    }
}

fn text_flow_name(value: TextFlow) -> &'static str {
    match value {
        TextFlow::BothSides => "both_sides",
        TextFlow::LeftOnly => "left_only",
        TextFlow::RightOnly => "right_only",
        TextFlow::LargestOnly => "largest_only",
    }
}

fn text_flow_from_name(value: &str, fallback: TextFlow) -> TextFlow {
    match object_layout_enum_key(value).as_str() {
        "BOTHSIDES" | "0" => TextFlow::BothSides,
        "LEFTONLY" | "1" => TextFlow::LeftOnly,
        "RIGHTONLY" | "2" => TextFlow::RightOnly,
        "LARGESTONLY" | "3" => TextFlow::LargestOnly,
        _ => fallback,
    }
}

fn size_criterion_name(value: SizeCriterion) -> &'static str {
    match value {
        SizeCriterion::Paper => "paper",
        SizeCriterion::Page => "page",
        SizeCriterion::Column => "column",
        SizeCriterion::Para => "para",
        SizeCriterion::Absolute => "absolute",
    }
}

fn size_criterion_from_name(value: &str, fallback: SizeCriterion) -> SizeCriterion {
    match object_layout_enum_key(value).as_str() {
        "PAPER" | "0" => SizeCriterion::Paper,
        "PAGE" | "1" => SizeCriterion::Page,
        "COLUMN" | "2" => SizeCriterion::Column,
        "PARA" | "PARAGRAPH" | "3" => SizeCriterion::Para,
        "ABSOLUTE" | "4" => SizeCriterion::Absolute,
        _ => fallback,
    }
}

fn object_numbering_type_name(value: ObjectNumberingType) -> &'static str {
    match value {
        ObjectNumberingType::None => "none",
        ObjectNumberingType::Picture => "picture",
        ObjectNumberingType::Table => "table",
        ObjectNumberingType::Equation => "equation",
    }
}

fn object_numbering_type_from_name(
    value: &str,
    fallback: ObjectNumberingType,
) -> ObjectNumberingType {
    match object_layout_enum_key(value).as_str() {
        "NONE" | "" => ObjectNumberingType::None,
        "PICTURE" => ObjectNumberingType::Picture,
        "TABLE" => ObjectNumberingType::Table,
        "EQUATION" => ObjectNumberingType::Equation,
        _ => fallback,
    }
}

fn object_dropcap_style_from_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    match object_layout_enum_key(trimmed).as_str() {
        "NONE" => None,
        "DOUBLELINE" => Some("DoubleLine".to_string()),
        "TRIPLELINE" => Some("TripleLine".to_string()),
        _ => Some(trimmed.to_string()),
    }
}

fn caption_direction_name(value: CaptionDirection) -> &'static str {
    match value {
        CaptionDirection::Left => "left",
        CaptionDirection::Right => "right",
        CaptionDirection::Top => "top",
        CaptionDirection::Bottom => "bottom",
    }
}

fn caption_direction_from_name(value: &str) -> CaptionDirection {
    match value {
        "left" | "LEFT" | "0" => CaptionDirection::Left,
        "right" | "RIGHT" | "1" => CaptionDirection::Right,
        "top" | "TOP" | "2" => CaptionDirection::Top,
        _ => CaptionDirection::Bottom,
    }
}

fn caption_vert_align_name(value: CaptionVertAlign) -> &'static str {
    match value {
        CaptionVertAlign::Top => "top",
        CaptionVertAlign::Center => "center",
        CaptionVertAlign::Bottom => "bottom",
    }
}

fn caption_vert_align_from_name(value: &str) -> CaptionVertAlign {
    match value {
        "bottom" | "BOTTOM" | "2" => CaptionVertAlign::Bottom,
        "center" | "middle" | "CENTER" | "MIDDLE" | "1" => CaptionVertAlign::Center,
        _ => CaptionVertAlign::Top,
    }
}

fn table_page_break_name(value: TablePageBreak) -> &'static str {
    match value {
        TablePageBreak::None => "none",
        TablePageBreak::CellBreak => "cell",
        TablePageBreak::RowBreak => "row",
    }
}

fn table_page_break_from_name(value: &str) -> TablePageBreak {
    match value {
        "cell" | "table" | "CELL" | "TABLE" | "1" => TablePageBreak::CellBreak,
        "row" | "break" | "ROW" | "BREAK" | "2" => TablePageBreak::RowBreak,
        _ => TablePageBreak::None,
    }
}

fn template_alignment(value: &str) -> Alignment {
    match value {
        "left" => Alignment::Left,
        "right" => Alignment::Right,
        "center" => Alignment::Center,
        "distribute" => Alignment::Distribute,
        "split" => Alignment::Split,
        _ => Alignment::Justify,
    }
}

fn line_spacing_type_str(value: LineSpacingType) -> &'static str {
    match value {
        LineSpacingType::Fixed => "Fixed",
        LineSpacingType::SpaceOnly => "SpaceOnly",
        LineSpacingType::Minimum => "Minimum",
        LineSpacingType::Percent => "Percent",
    }
}

fn line_spacing_type_from_name(value: &str) -> LineSpacingType {
    match value {
        "Fixed" | "fixed" | "FIXED" | "1" => LineSpacingType::Fixed,
        "SpaceOnly" | "space_only" | "SPACE_ONLY" | "2" => LineSpacingType::SpaceOnly,
        "Minimum" | "minimum" | "MINIMUM" | "3" => LineSpacingType::Minimum,
        _ => LineSpacingType::Percent,
    }
}

fn head_type_name(value: HeadType) -> &'static str {
    match value {
        HeadType::Outline => "outline",
        HeadType::Number => "number",
        HeadType::Bullet => "bullet",
        HeadType::None => "none",
    }
}

fn head_type_from_name(value: &str) -> HeadType {
    match value {
        "outline" | "OUTLINE" | "1" => HeadType::Outline,
        "number" | "NUMBER" | "2" => HeadType::Number,
        "bullet" | "BULLET" | "3" => HeadType::Bullet,
        _ => HeadType::None,
    }
}

fn bgr_to_css(color: u32) -> String {
    let r = color & 0xff;
    let g = (color >> 8) & 0xff;
    let b = (color >> 16) & 0xff;
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn is_empty_format_matrix(matrix: &[Vec<TemplateTextFormat>]) -> bool {
    matrix.iter().all(|row| {
        row.iter().all(|format| {
            format.style.is_none()
                && format.char_format.is_none()
                && format.char_shape_runs.is_empty()
                && format.para_format.is_none()
        })
    })
}

fn is_empty_cell_layout_matrix(matrix: &[Vec<TemplateCellLayout>]) -> bool {
    matrix.iter().all(|row| {
        row.iter()
            .all(|layout| layout == &TemplateCellLayout::default())
    })
}

fn is_empty_block_matrix(matrix: &[Vec<Vec<TemplateBlock>>]) -> bool {
    matrix
        .iter()
        .all(|row| row.iter().all(|blocks| blocks.is_empty()))
}

fn parse_json_field<T: DeserializeOwned>(json_text: &str, key: &str) -> Result<T, String> {
    let value: Value = serde_json::from_str(json_text)
        .map_err(|e| format!("invalid command JSON result: {e}: {json_text}"))?;
    serde_json::from_value(
        value
            .get(key)
            .ok_or_else(|| format!("missing {key}: {json_text}"))?
            .clone(),
    )
    .map_err(|e| format!("invalid {key}: {e}: {json_text}"))
}
