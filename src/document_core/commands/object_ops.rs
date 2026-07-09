//! 그림 속성/삽입/삭제 + 표 생성 + 셀 bbox 관련 native 메서드

use super::super::helpers::{
    build_tab_def_from_json, get_textbox_from_shape, get_textbox_from_shape_mut,
    json_has_border_keys, json_has_tab_keys, parse_char_shape_mods, parse_json_i16_array,
    parse_para_shape_mods,
};
use crate::document_core::DocumentCore;
use crate::error::HwpError;
use crate::model::control::Control;
use crate::model::event::DocumentEvent;
use crate::model::paragraph::Paragraph;
use crate::model::shape::{
    apply_rhwp_chart_data_semantic, chart_has_semantic, chart_type_from_name, chart_type_name,
    clear_chart_semantic, common_obj_offsets, encode_rhwp_chart_data_semantic,
    legend_position_from_name, legend_position_name, Axis, ChartShape, DataSeries, Legend,
    ShapeObject, RHWP_CHART_DATA_JSON_MAGIC,
};
use base64::Engine as _;
use std::io::{Cursor, Read};

/// 도형 최소 크기 (HWPUNIT).
/// 0으로 내려가면 Rectangle은 x_coords=[0,0,0,0]이 되고,
/// Group은 current/original 스케일이 0이 되어 자식이 전부 사라진다.
/// table_ops의 MIN_CELL_SIZE와 동일한 기준을 사용한다.
const MIN_SHAPE_SIZE: u32 = 200;

#[derive(Debug, Clone, Copy)]
enum ChartOoxmlLocation {
    DirectXml {
        content_idx: usize,
        bin_data_id: u16,
    },
    OleContainer {
        content_idx: usize,
        bin_data_id: u16,
    },
}

#[derive(Debug, Clone)]
struct LegacyOleChartContents {
    bin_data_id: u16,
    raw_contents: Vec<u8>,
}

#[derive(Debug, Default)]
struct HwpChartDataSemanticUpdate {
    title: Option<String>,
    chart_type: Option<crate::model::shape::ChartType>,
    legend_position: Option<crate::model::shape::LegendPosition>,
    legend_visible: Option<bool>,
    categories: Option<Vec<String>>,
    series: Option<Vec<DataSeries>>,
    x_axis: Option<Axis>,
    y_axis: Option<Axis>,
}

impl ChartOoxmlLocation {
    fn content_idx(self) -> usize {
        match self {
            Self::DirectXml { content_idx, .. } | Self::OleContainer { content_idx, .. } => {
                content_idx
            }
        }
    }

    fn bin_data_id(self) -> u16 {
        match self {
            Self::DirectXml { bin_data_id, .. } | Self::OleContainer { bin_data_id, .. } => {
                bin_data_id
            }
        }
    }
}

fn replace_ooxml_chart_contents_stream(
    ole_cfb: &[u8],
    edited_xml: &[u8],
) -> Result<Vec<u8>, HwpError> {
    let cursor = Cursor::new(ole_cfb);
    let mut compound = cfb::CompoundFile::open(cursor)
        .map_err(|e| HwpError::RenderError(format!("OLE chart CFB 열기 실패: {e}")))?;
    let stream_paths: Vec<String> = compound
        .walk()
        .filter(|entry| entry.is_stream())
        .map(|entry| entry.path().to_string_lossy().to_string())
        .collect();
    let mut replaced = false;
    let mut streams: Vec<(String, Vec<u8>)> = Vec::with_capacity(stream_paths.len());
    for path in stream_paths {
        let mut bytes = Vec::new();
        compound
            .open_stream(&path)
            .and_then(|mut stream| stream.read_to_end(&mut bytes))
            .map_err(|e| {
                HwpError::RenderError(format!("OLE chart stream 읽기 실패({path}): {e}"))
            })?;
        if path.trim_start_matches('/') == "OOXMLChartContents" {
            bytes.clear();
            bytes.extend_from_slice(edited_xml);
            replaced = true;
        }
        streams.push((path, bytes));
    }

    if !replaced {
        return Err(HwpError::RenderError(
            "OLE chart container에 OOXMLChartContents stream이 없습니다".to_string(),
        ));
    }

    let named_streams: Vec<(&str, &[u8])> = streams
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
        .collect();
    crate::serializer::mini_cfb::build_cfb(&named_streams)
        .map_err(|e| HwpError::RenderError(format!("OLE chart CFB 재작성 실패: {e}")))
}

impl DocumentCore {
    const COMMON_OBJ_ATTR_KNOWN_MASK: u32 = 0x01
        | (0x03 << 3)
        | (0x07 << 5)
        | (0x03 << 8)
        | (0x07 << 10)
        | (1 << 13)
        | (1 << 14)
        | (0x07 << 15)
        | (0x03 << 18)
        | (1 << 20)
        | (0x07 << 21)
        | (0x03 << 24)
        | (1 << 26)
        | (1 << 28);

    fn sync_common_obj_attr_known_bits(c: &mut crate::model::shape::CommonObjAttr) {
        let packed =
            crate::document_core::converters::common_obj_attr_writer::pack_common_attr_bits(c);
        c.attr = (c.attr & !Self::COMMON_OBJ_ATTR_KNOWN_MASK)
            | (packed & Self::COMMON_OBJ_ATTR_KNOWN_MASK);
    }

    fn is_structure_only_empty_paragraph(para: &Paragraph) -> bool {
        para.text.is_empty()
            && !para.controls.is_empty()
            && para
                .controls
                .iter()
                .all(|ctrl| matches!(ctrl, Control::SectionDef(_) | Control::ColumnDef(_)))
    }

    fn resolve_shape_control_ref(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<&ShapeObject, HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;

        let body_len = section.paragraphs.len();
        let para = if parent_para_idx < body_len {
            section.paragraphs.get(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?
        } else {
            let mut virtual_idx = parent_para_idx - body_len;
            let mut found = None;
            'outer: for body_para in &section.paragraphs {
                for ctrl in &body_para.controls {
                    if let Control::Endnote(en) = ctrl {
                        if virtual_idx < en.paragraphs.len() {
                            found = en.paragraphs.get(virtual_idx);
                            break 'outer;
                        }
                        virtual_idx -= en.paragraphs.len();
                    }
                }
            }
            found.ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?
        };

        let ctrl = para.controls.get(control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
        })?;
        match ctrl {
            Control::Shape(s) => Ok(s.as_ref()),
            _ => Err(HwpError::RenderError(
                "지정된 컨트롤이 Shape이 아닙니다".to_string(),
            )),
        }
    }

    fn resolve_shape_control_mut(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<&mut ShapeObject, HwpError> {
        let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;

        let body_len = section.paragraphs.len();
        let para = if parent_para_idx < body_len {
            section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?
        } else {
            let mut virtual_idx = parent_para_idx - body_len;
            let mut found = None;
            'outer: for body_para in &mut section.paragraphs {
                for ctrl in &mut body_para.controls {
                    if let Control::Endnote(en) = ctrl {
                        if virtual_idx < en.paragraphs.len() {
                            found = en.paragraphs.get_mut(virtual_idx);
                            break 'outer;
                        }
                        virtual_idx -= en.paragraphs.len();
                    }
                }
            }
            found.ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?
        };

        let ctrl = para.controls.get_mut(control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
        })?;
        match ctrl {
            Control::Shape(s) => Ok(s.as_mut()),
            _ => Err(HwpError::RenderError(
                "지정된 컨트롤이 Shape이 아닙니다".to_string(),
            )),
        }
    }

    fn resolve_picture_control_ref(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<&crate::model::image::Picture, HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;

        let body_len = section.paragraphs.len();
        let para = if parent_para_idx < body_len {
            section.paragraphs.get(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?
        } else {
            let mut virtual_idx = parent_para_idx - body_len;
            let mut found = None;
            'outer: for body_para in &section.paragraphs {
                for ctrl in &body_para.controls {
                    if let Control::Endnote(en) = ctrl {
                        if virtual_idx < en.paragraphs.len() {
                            found = en.paragraphs.get(virtual_idx);
                            break 'outer;
                        }
                        virtual_idx -= en.paragraphs.len();
                    }
                }
            }
            found.ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?
        };

        let ctrl = para.controls.get(control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
        })?;
        match ctrl {
            Control::Picture(p) => Ok(p),
            Control::Shape(shape) => match shape.as_ref() {
                ShapeObject::Picture(p) => Ok(p),
                _ => Err(HwpError::RenderError(
                    "지정된 Shape 컨트롤이 그림이 아닙니다".to_string(),
                )),
            },
            _ => Err(HwpError::RenderError(
                "지정된 컨트롤이 그림이 아닙니다".to_string(),
            )),
        }
    }

    fn resolve_picture_control_mut(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<&mut crate::model::image::Picture, HwpError> {
        let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;

        let body_len = section.paragraphs.len();
        let para = if parent_para_idx < body_len {
            section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?
        } else {
            let mut virtual_idx = parent_para_idx - body_len;
            let mut found = None;
            'outer: for body_para in &mut section.paragraphs {
                for ctrl in &mut body_para.controls {
                    if let Control::Endnote(en) = ctrl {
                        if virtual_idx < en.paragraphs.len() {
                            found = en.paragraphs.get_mut(virtual_idx);
                            break 'outer;
                        }
                        virtual_idx -= en.paragraphs.len();
                    }
                }
            }
            found.ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?
        };

        let ctrl = para.controls.get_mut(control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
        })?;
        match ctrl {
            Control::Picture(p) => Ok(p),
            Control::Shape(shape) => match shape.as_mut() {
                ShapeObject::Picture(p) => Ok(p),
                _ => Err(HwpError::RenderError(
                    "지정된 Shape 컨트롤이 그림이 아닙니다".to_string(),
                )),
            },
            _ => Err(HwpError::RenderError(
                "지정된 컨트롤이 그림이 아닙니다".to_string(),
            )),
        }
    }

    pub fn get_picture_properties_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<String, HwpError> {
        let pic = self.resolve_picture_control_ref(section_idx, parent_para_idx, control_idx)?;
        Self::format_picture_properties_json(pic)
    }

    fn picture_crop_extent_hu(pic: &crate::model::image::Picture) -> (i32, i32) {
        let width = if pic.shape_attr.original_width > 0 {
            pic.shape_attr.original_width
        } else {
            pic.shape_attr.current_width
        };
        let height = if pic.shape_attr.original_height > 0 {
            pic.shape_attr.original_height
        } else {
            pic.shape_attr.current_height
        };
        (
            i32::try_from(width).unwrap_or(i32::MAX),
            i32::try_from(height).unwrap_or(i32::MAX),
        )
    }

    fn picture_crop_ui_amounts(pic: &crate::model::image::Picture) -> (i32, i32, i32, i32) {
        let (extent_w, extent_h) = Self::picture_crop_extent_hu(pic);
        let left = pic.crop.left.max(0);
        let top = pic.crop.top.max(0);
        let right = if extent_w > 0 && pic.crop.right > left {
            (extent_w - pic.crop.right).max(0)
        } else {
            0
        };
        let bottom = if extent_h > 0 && pic.crop.bottom > top {
            (extent_h - pic.crop.bottom).max(0)
        } else {
            0
        };
        (left, top, right, bottom)
    }

    fn set_picture_crop_from_ui_amounts(
        pic: &mut crate::model::image::Picture,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    ) {
        let (extent_w, extent_h) = Self::picture_crop_extent_hu(pic);
        pic.crop.left = left.max(0);
        pic.crop.top = top.max(0);
        if extent_w > 0 {
            pic.crop.right = (extent_w - right.max(0)).max(pic.crop.left);
        } else {
            pic.crop.right = right.max(0);
        }
        if extent_h > 0 {
            pic.crop.bottom = (extent_h - bottom.max(0)).max(pic.crop.top);
        } else {
            pic.crop.bottom = bottom.max(0);
        }
    }

    fn picture_props_touch_shape_transform(props_json: &str) -> bool {
        const TRANSFORM_KEYS: [&str; 7] = [
            "\"width\"",
            "\"height\"",
            "\"vertOffset\"",
            "\"horzOffset\"",
            "\"rotationAngle\"",
            "\"horzFlip\"",
            "\"vertFlip\"",
        ];
        TRANSFORM_KEYS.iter().any(|key| props_json.contains(key))
    }

    fn effect_point_json(point: &crate::model::image::EffectPoint) -> String {
        let mut fields = Vec::new();
        if let Some(x) = &point.x {
            fields.push(format!(
                "\"x\":\"{}\"",
                super::super::helpers::json_escape(x)
            ));
        }
        if let Some(y) = &point.y {
            fields.push(format!(
                "\"y\":\"{}\"",
                super::super::helpers::json_escape(y)
            ));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn effect_range_json(range: &crate::model::image::EffectRange) -> String {
        let mut fields = Vec::new();
        if let Some(start) = &range.start {
            fields.push(format!(
                "\"start\":\"{}\"",
                super::super::helpers::json_escape(start)
            ));
        }
        if let Some(end) = &range.end {
            fields.push(format!(
                "\"end\":\"{}\"",
                super::super::helpers::json_escape(end)
            ));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn effect_rgb_color_hex(rgb: &crate::model::image::EffectRgb) -> Option<String> {
        let r = rgb.r.as_deref()?.trim().parse::<u8>().ok()?;
        let g = rgb.g.as_deref()?.trim().parse::<u8>().ok()?;
        let b = rgb.b.as_deref()?.trim().parse::<u8>().ok()?;
        Some(format!("#{:02X}{:02X}{:02X}", r, g, b))
    }

    fn effect_color_json(color: &crate::model::image::EffectColor) -> String {
        let mut fields = Vec::new();
        if let Some(value) = &color.color_type {
            fields.push(format!(
                "\"type\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &color.scheme_idx {
            fields.push(format!(
                "\"schemeIdx\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &color.system_idx {
            fields.push(format!(
                "\"systemIdx\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &color.preset_idx {
            fields.push(format!(
                "\"presetIdx\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(rgb) = &color.rgb {
            let mut rgb_fields = Vec::new();
            if let Some(value) = &rgb.r {
                rgb_fields.push(format!(
                    "\"r\":\"{}\"",
                    super::super::helpers::json_escape(value)
                ));
            }
            if let Some(value) = &rgb.g {
                rgb_fields.push(format!(
                    "\"g\":\"{}\"",
                    super::super::helpers::json_escape(value)
                ));
            }
            if let Some(value) = &rgb.b {
                rgb_fields.push(format!(
                    "\"b\":\"{}\"",
                    super::super::helpers::json_escape(value)
                ));
            }
            fields.push(format!("\"rgb\":{{{}}}", rgb_fields.join(",")));
            if let Some(color_hex) = Self::effect_rgb_color_hex(rgb) {
                fields.push(format!("\"colorHex\":\"{}\"", color_hex));
            }
        }
        if !color.raw_child_xml.is_empty() {
            let raw =
                serde_json::to_string(&color.raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn picture_shadow_json(shadow: &crate::model::image::PictureShadow) -> String {
        let mut fields = Vec::new();
        if let Some(value) = &shadow.style {
            fields.push(format!(
                "\"style\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &shadow.alpha {
            fields.push(format!(
                "\"alpha\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &shadow.radius {
            fields.push(format!(
                "\"radius\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &shadow.direction {
            fields.push(format!(
                "\"direction\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &shadow.distance {
            fields.push(format!(
                "\"distance\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &shadow.align_style {
            fields.push(format!(
                "\"alignStyle\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &shadow.rotation_style {
            fields.push(format!(
                "\"rotationStyle\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(skew) = &shadow.skew {
            fields.push(format!("\"skew\":{}", Self::effect_point_json(skew)));
        }
        if let Some(scale) = &shadow.scale {
            fields.push(format!("\"scale\":{}", Self::effect_point_json(scale)));
        }
        if let Some(color) = &shadow.color {
            fields.push(format!("\"color\":{}", Self::effect_color_json(color)));
        }
        if !shadow.raw_child_xml.is_empty() {
            let raw =
                serde_json::to_string(&shadow.raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn picture_effects_shadow_field(pic: &crate::model::image::Picture) -> String {
        match &pic.effects.shadow {
            Some(shadow) => format!(",\"shadow\":{}", Self::picture_shadow_json(shadow)),
            None => ",\"shadow\":null".to_string(),
        }
    }

    fn picture_glow_json(glow: &crate::model::image::PictureGlow) -> String {
        let mut fields = Vec::new();
        if let Some(value) = &glow.alpha {
            fields.push(format!(
                "\"alpha\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &glow.radius {
            fields.push(format!(
                "\"radius\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(color) = &glow.color {
            fields.push(format!("\"color\":{}", Self::effect_color_json(color)));
        }
        if !glow.raw_child_xml.is_empty() {
            let raw =
                serde_json::to_string(&glow.raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn picture_effects_glow_field(pic: &crate::model::image::Picture) -> String {
        match &pic.effects.glow {
            Some(glow) => format!(",\"glow\":{}", Self::picture_glow_json(glow)),
            None => ",\"glow\":null".to_string(),
        }
    }

    fn picture_soft_edge_json(soft_edge: &crate::model::image::PictureSoftEdge) -> String {
        let mut fields = Vec::new();
        if let Some(value) = &soft_edge.radius {
            fields.push(format!(
                "\"radius\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if !soft_edge.raw_child_xml.is_empty() {
            let raw = serde_json::to_string(&soft_edge.raw_child_xml)
                .unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn picture_effects_soft_edge_field(pic: &crate::model::image::Picture) -> String {
        match &pic.effects.soft_edge {
            Some(soft_edge) => {
                format!(",\"softEdge\":{}", Self::picture_soft_edge_json(soft_edge))
            }
            None => ",\"softEdge\":null".to_string(),
        }
    }

    fn picture_reflection_json(reflection: &crate::model::image::PictureReflection) -> String {
        let mut fields = Vec::new();
        if let Some(value) = &reflection.align_style {
            fields.push(format!(
                "\"alignStyle\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &reflection.radius {
            fields.push(format!(
                "\"radius\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &reflection.direction {
            fields.push(format!(
                "\"direction\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &reflection.distance {
            fields.push(format!(
                "\"distance\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &reflection.rotation_style {
            fields.push(format!(
                "\"rotationStyle\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(value) = &reflection.fade_direction {
            fields.push(format!(
                "\"fadeDirection\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(skew) = &reflection.skew {
            fields.push(format!("\"skew\":{}", Self::effect_point_json(skew)));
        }
        if let Some(scale) = &reflection.scale {
            fields.push(format!("\"scale\":{}", Self::effect_point_json(scale)));
        }
        if let Some(color) = &reflection.color {
            fields.push(format!("\"color\":{}", Self::effect_color_json(color)));
        }
        if let Some(alpha) = &reflection.alpha {
            fields.push(format!("\"alpha\":{}", Self::effect_range_json(alpha)));
        }
        if let Some(pos) = &reflection.pos {
            fields.push(format!("\"pos\":{}", Self::effect_range_json(pos)));
        }
        if !reflection.raw_child_xml.is_empty() {
            let raw = serde_json::to_string(&reflection.raw_child_xml)
                .unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn picture_effects_reflection_field(pic: &crate::model::image::Picture) -> String {
        match &pic.effects.reflection {
            Some(reflection) => {
                format!(
                    ",\"reflection\":{}",
                    Self::picture_reflection_json(reflection)
                )
            }
            None => ",\"reflection\":null".to_string(),
        }
    }

    fn picture_blur_json(blur: &crate::model::image::PictureBlur) -> String {
        let mut fields = Vec::new();
        if let Some(value) = &blur.radius {
            fields.push(format!(
                "\"radius\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if !blur.raw_child_xml.is_empty() {
            let raw =
                serde_json::to_string(&blur.raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn effect_attrs_json(attrs: &std::collections::BTreeMap<String, String>) -> String {
        let fields = attrs
            .iter()
            .map(|(key, value)| {
                format!(
                    "\"{}\":\"{}\"",
                    super::super::helpers::json_escape(key),
                    super::super::helpers::json_escape(value)
                )
            })
            .collect::<Vec<_>>();
        format!("{{{}}}", fields.join(","))
    }

    fn picture_effect_child_json(child: &crate::model::image::PictureEffectChild) -> String {
        Self::effect_attrs_json(&child.attrs)
    }

    fn picture_three_d_json(three_d: &crate::model::image::PictureThreeD) -> String {
        let mut fields = three_d
            .attrs
            .iter()
            .map(|(key, value)| {
                format!(
                    "\"{}\":\"{}\"",
                    super::super::helpers::json_escape(key),
                    super::super::helpers::json_escape(value)
                )
            })
            .collect::<Vec<_>>();
        if let Some(bevel) = &three_d.bevel {
            fields.push(format!(
                "\"bevel\":{}",
                Self::picture_effect_child_json(bevel)
            ));
        }
        if !three_d.raw_child_xml.is_empty() {
            let raw =
                serde_json::to_string(&three_d.raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn picture_solid_fill_json(solid_fill: &crate::model::image::PictureSolidFill) -> String {
        let mut fields = Vec::new();
        if let Some(value) = &solid_fill.color {
            if let Some(color_hex) = Self::normalize_xml_color_hex(value) {
                fields.push(format!("\"colorHex\":\"{}\"", color_hex));
            }
            if solid_fill.effect_color.is_none() {
                fields.push(format!(
                    "\"color\":\"{}\"",
                    super::super::helpers::json_escape(value)
                ));
            }
        }
        if let Some(color) = &solid_fill.effect_color {
            let color_json = Self::effect_color_json(color);
            fields.push(format!("\"color\":{}", color_json));
            fields.push(format!("\"effectsColor\":{}", color_json));
        }
        if !solid_fill.raw_child_xml.is_empty() {
            let raw = serde_json::to_string(&solid_fill.raw_child_xml)
                .unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn picture_fill_overlay_json(fill_overlay: &crate::model::image::PictureFillOverlay) -> String {
        let mut fields = Vec::new();
        if let Some(value) = &fill_overlay.blend {
            fields.push(format!(
                "\"blend\":\"{}\"",
                super::super::helpers::json_escape(value)
            ));
        }
        if let Some(solid_fill) = &fill_overlay.solid_fill {
            fields.push(format!(
                "\"solidFill\":{}",
                Self::picture_solid_fill_json(solid_fill)
            ));
        }
        if !fill_overlay.raw_child_xml.is_empty() {
            let raw = serde_json::to_string(&fill_overlay.raw_child_xml)
                .unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn picture_raw_effect_json_field(
        pic: &crate::model::image::Picture,
        json_key: &str,
        effect_name: &[u8],
    ) -> String {
        let value = pic
            .effects
            .raw_xml
            .iter()
            .find_map(|raw| Self::shape_effect_json_from_raw_fragment(raw, effect_name));
        match value {
            Some(value) => format!(",\"{}\":{}", json_key, value),
            None => format!(",\"{}\":null", json_key),
        }
    }

    fn picture_effects_blur_field(pic: &crate::model::image::Picture) -> String {
        match &pic.effects.blur {
            Some(blur) => format!(",\"blur\":{}", Self::picture_blur_json(blur)),
            None => Self::picture_raw_effect_json_field(pic, "blur", b"blur"),
        }
    }

    fn picture_effects_three_d_field(pic: &crate::model::image::Picture) -> String {
        match &pic.effects.three_d {
            Some(three_d) => format!(",\"threeD\":{}", Self::picture_three_d_json(three_d)),
            None => Self::picture_raw_effect_json_field(pic, "threeD", b"threeD"),
        }
    }

    fn picture_effects_fill_overlay_field(pic: &crate::model::image::Picture) -> String {
        match &pic.effects.fill_overlay {
            Some(fill_overlay) => format!(
                ",\"fillOverlay\":{}",
                Self::picture_fill_overlay_json(fill_overlay)
            ),
            None => Self::picture_raw_effect_json_field(pic, "fillOverlay", b"fillOverlay"),
        }
    }

    fn picture_effects_raw_xml_field(pic: &crate::model::image::Picture) -> String {
        let raw = serde_json::to_string(&pic.effects.raw_xml).unwrap_or_else(|_| "[]".to_string());
        format!(",\"effectsRawXml\":{}", raw)
    }

    fn json_attr_string(value: &serde_json::Value) -> Option<String> {
        if let Some(s) = value.as_str() {
            Some(s.to_string())
        } else if value.is_number() || value.is_boolean() {
            Some(value.to_string())
        } else {
            None
        }
    }

    fn object_string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|key| value.get(*key).and_then(Self::json_attr_string))
    }

    fn effect_point_from_json(
        value: &serde_json::Value,
        existing: Option<crate::model::image::EffectPoint>,
    ) -> Option<crate::model::image::EffectPoint> {
        if value.is_null() {
            return None;
        }
        if !value.is_object() {
            return existing;
        }
        let mut point = existing.unwrap_or_default();
        if let Some(v) = Self::object_string_field(value, &["x"]) {
            point.x = Some(v);
        }
        if let Some(v) = Self::object_string_field(value, &["y"]) {
            point.y = Some(v);
        }
        Some(point)
    }

    fn effect_point_from_flat_aliases(
        value: &serde_json::Value,
        prefix: &str,
        existing: Option<crate::model::image::EffectPoint>,
    ) -> Option<crate::model::image::EffectPoint> {
        let (x_keys, y_keys): (&[&str], &[&str]) = match prefix {
            "skew" => (&["skew_x", "skewX"], &["skew_y", "skewY"]),
            "scale" => (&["scale_x", "scaleX"], &["scale_y", "scaleY"]),
            _ => return existing,
        };
        let x = Self::object_string_field(value, x_keys);
        let y = Self::object_string_field(value, y_keys);
        if x.is_none() && y.is_none() {
            existing
        } else {
            let mut point = existing.unwrap_or_default();
            if let Some(x) = x {
                point.x = Some(x);
            }
            if let Some(y) = y {
                point.y = Some(y);
            }
            Some(point)
        }
    }

    fn effect_range_from_json(
        value: &serde_json::Value,
        existing: Option<crate::model::image::EffectRange>,
    ) -> Option<crate::model::image::EffectRange> {
        if value.is_null() {
            return None;
        }
        if !value.is_object() {
            return existing;
        }
        let mut range = existing.unwrap_or_default();
        if let Some(v) = Self::object_string_field(value, &["start"]) {
            range.start = Some(v);
        }
        if let Some(v) = Self::object_string_field(value, &["end"]) {
            range.end = Some(v);
        }
        Some(range)
    }

    fn effect_range_from_flat_aliases(
        value: &serde_json::Value,
        prefix: &str,
        existing: Option<crate::model::image::EffectRange>,
    ) -> Option<crate::model::image::EffectRange> {
        let (start_keys, end_keys): (&[&str], &[&str]) = match prefix {
            "alpha" => (&["alpha_start", "alphaStart"], &["alpha_end", "alphaEnd"]),
            "pos" => (&["pos_start", "posStart"], &["pos_end", "posEnd"]),
            _ => return existing,
        };
        let start = Self::object_string_field(value, start_keys);
        let end = Self::object_string_field(value, end_keys);
        if start.is_none() && end.is_none() {
            existing
        } else {
            let mut range = existing.unwrap_or_default();
            if let Some(start) = start {
                range.start = Some(start);
            }
            if let Some(end) = end {
                range.end = Some(end);
            }
            Some(range)
        }
    }

    fn effect_color_from_json(
        value: &serde_json::Value,
        existing: Option<crate::model::image::EffectColor>,
    ) -> Option<crate::model::image::EffectColor> {
        if value.is_null() {
            return None;
        }
        if !value.is_object() {
            let rgb = Self::effect_rgb_from_color_hex_value(value)?;
            let mut color = existing.unwrap_or_default();
            color.color_type.get_or_insert_with(|| "RGB".to_string());
            color.scheme_idx.get_or_insert_with(|| "-1".to_string());
            color.system_idx.get_or_insert_with(|| "-1".to_string());
            color.preset_idx.get_or_insert_with(|| "-1".to_string());
            color.rgb = Some(rgb);
            return Some(color);
        }
        let mut color = existing.unwrap_or_default();
        if let Some(v) = Self::object_string_field(value, &["type", "colorType", "color_type"]) {
            color.color_type = Some(v);
        }
        if let Some(v) = Self::object_string_field(value, &["schemeIdx", "scheme_idx"]) {
            color.scheme_idx = Some(v);
        }
        if let Some(v) = Self::object_string_field(value, &["systemIdx", "system_idx"]) {
            color.system_idx = Some(v);
        }
        if let Some(v) = Self::object_string_field(value, &["presetIdx", "preset_idx"]) {
            color.preset_idx = Some(v);
        }
        if let Some(rgb_value) = value.get("rgb") {
            if rgb_value.is_null() {
                color.rgb = None;
            } else if rgb_value.is_object() {
                let mut rgb = color.rgb.unwrap_or_default();
                if let Some(v) = Self::object_string_field(rgb_value, &["r"]) {
                    rgb.r = Some(v);
                }
                if let Some(v) = Self::object_string_field(rgb_value, &["g"]) {
                    rgb.g = Some(v);
                }
                if let Some(v) = Self::object_string_field(rgb_value, &["b"]) {
                    rgb.b = Some(v);
                }
                color.rgb = Some(rgb);
            }
        }
        if let Some(rgb) = value
            .get("colorHex")
            .or_else(|| value.get("color_hex"))
            .or_else(|| value.get("rgbHex"))
            .or_else(|| value.get("rgb_hex"))
            .and_then(Self::effect_rgb_from_color_hex_value)
        {
            color.color_type.get_or_insert_with(|| "RGB".to_string());
            color.scheme_idx.get_or_insert_with(|| "-1".to_string());
            color.system_idx.get_or_insert_with(|| "-1".to_string());
            color.preset_idx.get_or_insert_with(|| "-1".to_string());
            color.rgb = Some(rgb);
        }
        if Self::shape_effect_raw_child_xml_value(value).is_some() {
            color.raw_child_xml = Self::shape_effect_raw_child_xml_values(value);
        }
        Some(color)
    }

    fn direct_effect_color_json(value: &serde_json::Value) -> Option<serde_json::Value> {
        let obj = value.as_object()?;
        let mut color = serde_json::Map::new();
        for key in [
            "type",
            "colorType",
            "color_type",
            "schemeIdx",
            "scheme_idx",
            "systemIdx",
            "system_idx",
            "presetIdx",
            "preset_idx",
            "rgb",
            "colorHex",
            "color_hex",
            "rgbHex",
            "rgb_hex",
        ] {
            if let Some(raw_value) = obj.get(key) {
                color.insert(key.to_string(), raw_value.clone());
            }
        }
        if color.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(color))
        }
    }

    fn effect_rgb_from_color_hex_value(
        value: &serde_json::Value,
    ) -> Option<crate::model::image::EffectRgb> {
        let color_ref = Self::json_css_color_ref_value(value)?;
        if color_ref == 0xFFFF_FFFF {
            return None;
        }
        let rgb = Self::color_ref_to_rgb_u32(color_ref);
        Some(crate::model::image::EffectRgb {
            r: Some(((rgb >> 16) & 0xFF).to_string()),
            g: Some(((rgb >> 8) & 0xFF).to_string()),
            b: Some((rgb & 0xFF).to_string()),
        })
    }

    fn picture_shadow_value<'a>(value: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        value
            .get("shadow")
            .or_else(|| value.get("pictureShadow"))
            .or_else(|| value.get("picture_shadow"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("shadow")
                        .or_else(|| effects.get("pictureShadow"))
                        .or_else(|| effects.get("picture_shadow"))
                })
            })
    }

    fn apply_picture_shadow_props(pic: &mut crate::model::image::Picture, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(shadow_value) = Self::picture_shadow_value(&value) else {
            return;
        };
        if shadow_value.is_null() {
            pic.effects.shadow = None;
            return;
        }
        if !shadow_value.is_object() {
            return;
        }

        let mut shadow = pic.effects.shadow.take().unwrap_or_default();
        if let Some(v) = Self::object_string_field(shadow_value, &["style"]) {
            shadow.style = Some(v);
        }
        if let Some(v) = Self::object_string_field(shadow_value, &["alpha"]) {
            shadow.alpha = Some(v);
        }
        if let Some(v) = Self::object_string_field(shadow_value, &["radius"]) {
            shadow.radius = Some(v);
        }
        if let Some(v) = Self::object_string_field(shadow_value, &["direction"]) {
            shadow.direction = Some(v);
        }
        if let Some(v) = Self::object_string_field(shadow_value, &["distance"]) {
            shadow.distance = Some(v);
        }
        if let Some(v) = Self::object_string_field(shadow_value, &["alignStyle", "align_style"]) {
            shadow.align_style = Some(v);
        }
        if let Some(v) =
            Self::object_string_field(shadow_value, &["rotationStyle", "rotation_style"])
        {
            shadow.rotation_style = Some(v);
        }
        if let Some(skew_value) = shadow_value.get("skew") {
            shadow.skew = Self::effect_point_from_json(skew_value, shadow.skew);
        } else {
            shadow.skew = Self::effect_point_from_flat_aliases(shadow_value, "skew", shadow.skew);
        }
        if let Some(scale_value) = shadow_value.get("scale") {
            shadow.scale = Self::effect_point_from_json(scale_value, shadow.scale);
        } else {
            shadow.scale =
                Self::effect_point_from_flat_aliases(shadow_value, "scale", shadow.scale);
        }
        if let Some(color_value) = shadow_value
            .get("color")
            .or_else(|| shadow_value.get("effectsColor"))
        {
            shadow.color = Self::effect_color_from_json(color_value, shadow.color);
        } else if let Some(color_value) = Self::direct_effect_color_json(shadow_value) {
            shadow.color = Self::effect_color_from_json(&color_value, shadow.color);
        }
        if Self::shape_effect_raw_child_xml_value(shadow_value).is_some() {
            shadow.raw_child_xml = Self::shape_effect_raw_child_xml_values(shadow_value);
        }
        pic.effects.shadow = Some(shadow);
    }

    fn picture_glow_value<'a>(value: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        value
            .get("glow")
            .or_else(|| value.get("pictureGlow"))
            .or_else(|| value.get("picture_glow"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("glow")
                        .or_else(|| effects.get("pictureGlow"))
                        .or_else(|| effects.get("picture_glow"))
                })
            })
    }

    fn apply_picture_glow_props(pic: &mut crate::model::image::Picture, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(glow_value) = Self::picture_glow_value(&value) else {
            return;
        };
        if glow_value.is_null() {
            pic.effects.glow = None;
            return;
        }
        if !glow_value.is_object() {
            return;
        }

        let mut glow = pic.effects.glow.take().unwrap_or_default();
        if let Some(v) = Self::object_string_field(glow_value, &["alpha"]) {
            glow.alpha = Some(v);
        }
        if let Some(v) = Self::object_string_field(glow_value, &["radius"]) {
            glow.radius = Some(v);
        }
        if let Some(color_value) = glow_value
            .get("color")
            .or_else(|| glow_value.get("effectsColor"))
        {
            glow.color = Self::effect_color_from_json(color_value, glow.color);
        } else if let Some(color_value) = Self::direct_effect_color_json(glow_value) {
            glow.color = Self::effect_color_from_json(&color_value, glow.color);
        }
        if Self::shape_effect_raw_child_xml_value(glow_value).is_some() {
            glow.raw_child_xml = Self::shape_effect_raw_child_xml_values(glow_value);
        }
        pic.effects.glow = Some(glow);
    }

    fn picture_soft_edge_value<'a>(value: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        value
            .get("softEdge")
            .or_else(|| value.get("soft_edge"))
            .or_else(|| value.get("pictureSoftEdge"))
            .or_else(|| value.get("picture_soft_edge"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("softEdge")
                        .or_else(|| effects.get("soft_edge"))
                        .or_else(|| effects.get("pictureSoftEdge"))
                        .or_else(|| effects.get("picture_soft_edge"))
                })
            })
    }

    fn apply_picture_soft_edge_props(pic: &mut crate::model::image::Picture, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(soft_edge_value) = Self::picture_soft_edge_value(&value) else {
            return;
        };
        if soft_edge_value.is_null() {
            pic.effects.soft_edge = None;
            return;
        }
        if !soft_edge_value.is_object() {
            return;
        }

        let mut soft_edge = pic.effects.soft_edge.take().unwrap_or_default();
        if let Some(v) = Self::object_string_field(soft_edge_value, &["radius"]) {
            soft_edge.radius = Some(v);
        }
        if Self::shape_effect_raw_child_xml_value(soft_edge_value).is_some() {
            soft_edge.raw_child_xml = Self::shape_effect_raw_child_xml_values(soft_edge_value);
        }
        pic.effects.soft_edge = Some(soft_edge);
    }

    fn picture_reflection_value<'a>(value: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        value
            .get("reflection")
            .or_else(|| value.get("pictureReflection"))
            .or_else(|| value.get("picture_reflection"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("reflection")
                        .or_else(|| effects.get("pictureReflection"))
                        .or_else(|| effects.get("picture_reflection"))
                })
            })
    }

    fn apply_picture_reflection_props(pic: &mut crate::model::image::Picture, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(reflection_value) = Self::picture_reflection_value(&value) else {
            return;
        };
        if reflection_value.is_null() {
            pic.effects.reflection = None;
            return;
        }
        if !reflection_value.is_object() {
            return;
        }

        let mut reflection = pic.effects.reflection.take().unwrap_or_default();
        if let Some(v) = Self::object_string_field(reflection_value, &["alignStyle", "align_style"])
        {
            reflection.align_style = Some(v);
        }
        if let Some(v) = Self::object_string_field(reflection_value, &["radius"]) {
            reflection.radius = Some(v);
        }
        if let Some(v) = Self::object_string_field(reflection_value, &["direction"]) {
            reflection.direction = Some(v);
        }
        if let Some(v) = Self::object_string_field(reflection_value, &["distance"]) {
            reflection.distance = Some(v);
        }
        if let Some(v) =
            Self::object_string_field(reflection_value, &["rotationStyle", "rotation_style"])
        {
            reflection.rotation_style = Some(v);
        }
        if let Some(v) =
            Self::object_string_field(reflection_value, &["fadeDirection", "fade_direction"])
        {
            reflection.fade_direction = Some(v);
        }
        if let Some(skew_value) = reflection_value.get("skew") {
            reflection.skew = Self::effect_point_from_json(skew_value, reflection.skew);
        } else {
            reflection.skew =
                Self::effect_point_from_flat_aliases(reflection_value, "skew", reflection.skew);
        }
        if let Some(scale_value) = reflection_value.get("scale") {
            reflection.scale = Self::effect_point_from_json(scale_value, reflection.scale);
        } else {
            reflection.scale =
                Self::effect_point_from_flat_aliases(reflection_value, "scale", reflection.scale);
        }
        if let Some(color_value) = reflection_value
            .get("color")
            .or_else(|| reflection_value.get("effectsColor"))
        {
            reflection.color = Self::effect_color_from_json(color_value, reflection.color);
        } else if let Some(color_value) = Self::direct_effect_color_json(reflection_value) {
            reflection.color = Self::effect_color_from_json(&color_value, reflection.color);
        }
        if let Some(alpha_value) = reflection_value.get("alpha") {
            reflection.alpha = Self::effect_range_from_json(alpha_value, reflection.alpha);
        } else {
            reflection.alpha =
                Self::effect_range_from_flat_aliases(reflection_value, "alpha", reflection.alpha);
        }
        if let Some(pos_value) = reflection_value.get("pos") {
            reflection.pos = Self::effect_range_from_json(pos_value, reflection.pos);
        } else {
            reflection.pos =
                Self::effect_range_from_flat_aliases(reflection_value, "pos", reflection.pos);
        }
        if Self::shape_effect_raw_child_xml_value(reflection_value).is_some() {
            reflection.raw_child_xml = Self::shape_effect_raw_child_xml_values(reflection_value);
        }
        pic.effects.reflection = Some(reflection);
    }

    fn blur_value<'a>(value: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        value
            .get("blur")
            .or_else(|| value.get("pictureBlur"))
            .or_else(|| value.get("picture_blur"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("blur")
                        .or_else(|| effects.get("pictureBlur"))
                        .or_else(|| effects.get("picture_blur"))
                })
            })
    }

    fn apply_picture_blur_props(pic: &mut crate::model::image::Picture, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(blur_value) = Self::blur_value(&value) else {
            return;
        };
        pic.effects
            .raw_xml
            .retain(|raw| !Self::shape_raw_fragment_contains_effect(raw, b"blur"));
        if blur_value.is_null() {
            pic.effects.blur = None;
            return;
        }
        if !blur_value.is_object() {
            return;
        }
        let mut blur = pic.effects.blur.take().unwrap_or_default();
        if let Some(v) = Self::object_string_field(blur_value, &["radius"]) {
            blur.radius = Some(v);
        }
        if Self::shape_effect_raw_child_xml_value(blur_value).is_some() {
            blur.raw_child_xml = Self::shape_effect_raw_child_xml_values(blur_value);
        }
        pic.effects.blur = Some(blur);
    }

    fn picture_three_d_value<'a>(value: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        value
            .get("threeD")
            .or_else(|| value.get("three_d"))
            .or_else(|| value.get("pictureThreeD"))
            .or_else(|| value.get("picture_three_d"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("threeD")
                        .or_else(|| effects.get("three_d"))
                        .or_else(|| effects.get("pictureThreeD"))
                        .or_else(|| effects.get("picture_three_d"))
                })
            })
    }

    fn picture_effect_child_from_json(
        value: &serde_json::Value,
    ) -> Option<crate::model::image::PictureEffectChild> {
        if value.is_null() {
            return None;
        }
        if let Some(bevel_type) = Self::json_attr_string(value) {
            let mut attrs = std::collections::BTreeMap::new();
            attrs.insert("type".to_string(), bevel_type);
            return Some(crate::model::image::PictureEffectChild { attrs });
        }
        if !value.is_object() {
            return None;
        }
        Some(crate::model::image::PictureEffectChild {
            attrs: Self::shape_effect_xml_attr_map(value, &[]),
        })
    }

    fn apply_picture_three_d_props(pic: &mut crate::model::image::Picture, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(three_d_value) = Self::picture_three_d_value(&value) else {
            return;
        };
        pic.effects
            .raw_xml
            .retain(|raw| !Self::shape_raw_fragment_contains_effect(raw, b"threeD"));
        if three_d_value.is_null() {
            pic.effects.three_d = None;
            return;
        }
        if !three_d_value.is_object() {
            return;
        }

        let mut three_d = pic.effects.three_d.take().unwrap_or_default();
        for (key, value) in Self::shape_effect_xml_attr_map(
            three_d_value,
            &[
                "bevel",
                "bevelType",
                "bevel_type",
                "rawChildXml",
                "raw_child_xml",
                "rawChildrenXml",
                "raw_children_xml",
            ],
        ) {
            three_d.attrs.insert(key, value);
        }
        if let Some(bevel_value) = three_d_value.get("bevel") {
            three_d.bevel = Self::picture_effect_child_from_json(bevel_value);
        } else if let Some(bevel_type) =
            Self::object_string_field(three_d_value, &["bevelType", "bevel_type"])
        {
            let mut attrs = std::collections::BTreeMap::new();
            attrs.insert("type".to_string(), bevel_type);
            three_d.bevel = Some(crate::model::image::PictureEffectChild { attrs });
        }
        if Self::shape_effect_raw_child_xml_value(three_d_value).is_some() {
            three_d.raw_child_xml = Self::shape_effect_raw_child_xml_values(three_d_value);
        }
        pic.effects.three_d = Some(three_d);
    }

    fn picture_fill_overlay_value<'a>(
        value: &'a serde_json::Value,
    ) -> Option<&'a serde_json::Value> {
        value
            .get("fillOverlay")
            .or_else(|| value.get("fill_overlay"))
            .or_else(|| value.get("pictureFillOverlay"))
            .or_else(|| value.get("picture_fill_overlay"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("fillOverlay")
                        .or_else(|| effects.get("fill_overlay"))
                        .or_else(|| effects.get("pictureFillOverlay"))
                        .or_else(|| effects.get("picture_fill_overlay"))
                })
            })
    }

    fn solid_fill_from_json(
        value: &serde_json::Value,
        existing: Option<crate::model::image::PictureSolidFill>,
    ) -> Option<crate::model::image::PictureSolidFill> {
        if value.is_null() {
            return None;
        }
        if let Some(color) = Self::json_attr_string(value) {
            return Some(crate::model::image::PictureSolidFill {
                color: Some(color),
                ..existing.unwrap_or_default()
            });
        }
        if !value.is_object() {
            return existing;
        }

        let mut solid_fill = existing.unwrap_or_default();
        if let Some(color) = Self::object_string_field(value, &["colorHex", "color_hex"]) {
            solid_fill.color = Some(color);
        }
        if let Some(color_value) = value.get("color") {
            if color_value.is_null() {
                solid_fill.effect_color = None;
            } else if let Some(color) = Self::json_attr_string(color_value) {
                if solid_fill.color.is_none() {
                    solid_fill.color = Some(color);
                }
            } else {
                solid_fill.effect_color =
                    Self::effect_color_from_json(color_value, solid_fill.effect_color);
            }
        }
        if let Some(color_value) = value.get("effectsColor") {
            solid_fill.effect_color =
                Self::effect_color_from_json(color_value, solid_fill.effect_color);
        }
        if Self::shape_effect_raw_child_xml_value(value).is_some() {
            solid_fill.raw_child_xml = Self::shape_effect_raw_child_xml_values(value);
        }
        Some(solid_fill)
    }

    fn apply_picture_fill_overlay_props(pic: &mut crate::model::image::Picture, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(fill_overlay_value) = Self::picture_fill_overlay_value(&value) else {
            return;
        };
        pic.effects
            .raw_xml
            .retain(|raw| !Self::shape_raw_fragment_contains_effect(raw, b"fillOverlay"));
        if fill_overlay_value.is_null() {
            pic.effects.fill_overlay = None;
            return;
        }
        if !fill_overlay_value.is_object() {
            return;
        }

        let mut fill_overlay = pic.effects.fill_overlay.take().unwrap_or_default();
        if let Some(v) = Self::object_string_field(fill_overlay_value, &["blend"]) {
            fill_overlay.blend = Some(v);
        }
        if let Some(solid_fill_value) = fill_overlay_value
            .get("solidFill")
            .or_else(|| fill_overlay_value.get("solid_fill"))
        {
            fill_overlay.solid_fill =
                Self::solid_fill_from_json(solid_fill_value, fill_overlay.solid_fill);
        }
        if Self::shape_effect_raw_child_xml_value(fill_overlay_value).is_some() {
            fill_overlay.raw_child_xml =
                Self::shape_effect_raw_child_xml_values(fill_overlay_value);
        }
        pic.effects.fill_overlay = Some(fill_overlay);
    }

    fn raw_effects_value<'a>(value: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        value
            .get("effectsRawXml")
            .or_else(|| value.get("rawEffectsXml"))
            .or_else(|| value.get("effects_raw_xml"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("raw_xml")
                        .or_else(|| effects.get("rawXml"))
                        .or_else(|| effects.get("effectsRawXml"))
                })
            })
    }

    fn apply_picture_raw_effects_props(pic: &mut crate::model::image::Picture, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(raw_value) = Self::raw_effects_value(&value) else {
            return;
        };
        if raw_value.is_null() {
            pic.effects.raw_xml.clear();
            return;
        }
        if let Some(raw) = raw_value.as_str() {
            pic.effects.raw_xml = vec![raw.to_string()];
            return;
        }
        if let Some(values) = raw_value.as_array() {
            pic.effects.raw_xml = values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
        }
    }

    fn apply_picture_structured_raw_effect_props(
        pic: &mut crate::model::image::Picture,
        props_json: &str,
    ) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let effect_specs = [(
            "threeD",
            b"threeD".as_slice(),
            &["threeD", "three_d", "pictureThreeD", "picture_three_d"] as &[&str],
            &["threeD", "three_d", "pictureThreeD", "picture_three_d"] as &[&str],
        )];

        for (xml_name, effect_name, nested_keys, top_level_keys) in effect_specs {
            let Some(effect_value) =
                Self::shape_effect_value_with_top_aliases(&value, nested_keys, top_level_keys)
            else {
                continue;
            };
            pic.effects
                .raw_xml
                .retain(|raw| !Self::shape_raw_fragment_contains_effect(raw, effect_name));
            if effect_value.is_null() {
                continue;
            }
            let raw_xml = match xml_name {
                "threeD" => Self::shape_three_d_child_xml(effect_value),
                _ => Self::shape_simple_effect_child_xml(effect_value, xml_name),
            };
            if let Some(raw_xml) = raw_xml {
                pic.effects.raw_xml.push(raw_xml);
            }
        }
    }

    fn shape_shadow_type_name(shadow_type: u32) -> &'static str {
        match shadow_type {
            1 => "LEFT_TOP",
            2 => "RIGHT_TOP",
            3 => "LEFT_BOTTOM",
            4 => "RIGHT_BOTTOM",
            5 => "CENTER",
            _ => "NONE",
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

    fn json_u32_value(value: &serde_json::Value) -> Option<u32> {
        if let Some(raw) = value.as_u64() {
            u32::try_from(raw).ok()
        } else if let Some(raw) = value.as_i64() {
            u32::try_from(raw).ok()
        } else {
            let raw = value.as_str()?.trim();
            if let Some(hex) = raw.strip_prefix('#') {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                raw.parse::<u32>().ok()
            }
        }
    }

    fn rgb_u32_to_color_ref(raw: u32) -> u32 {
        let a = raw & 0xFF00_0000;
        let r = (raw >> 16) & 0xFF;
        let g = (raw >> 8) & 0xFF;
        let b = raw & 0xFF;
        a | (b << 16) | (g << 8) | r
    }

    fn color_ref_to_rgb_u32(color: u32) -> u32 {
        let r = color & 0xFF;
        let g = (color >> 8) & 0xFF;
        let b = (color >> 16) & 0xFF;
        (r << 16) | (g << 8) | b
    }

    fn color_ref_to_hex(color: u32) -> String {
        if color == 0xFFFF_FFFF {
            return "none".to_string();
        }
        let a = (color >> 24) & 0xFF;
        let rgb = Self::color_ref_to_rgb_u32(color);
        if a == 0 {
            format!("#{:06X}", rgb)
        } else {
            format!("#{:02X}{:06X}", a, rgb)
        }
    }

    fn json_css_color_ref_value(value: &serde_json::Value) -> Option<u32> {
        if let Some(raw) = value.as_u64() {
            return u32::try_from(raw).ok().map(Self::rgb_u32_to_color_ref);
        }
        if let Some(raw) = value.as_i64() {
            return u32::try_from(raw).ok().map(Self::rgb_u32_to_color_ref);
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
            6 | 8 => u32::from_str_radix(hex, 16)
                .ok()
                .map(Self::rgb_u32_to_color_ref),
            _ => raw.parse::<u32>().ok().map(Self::rgb_u32_to_color_ref),
        }
    }

    fn json_i32_value(value: &serde_json::Value) -> Option<i32> {
        if let Some(raw) = value.as_i64() {
            i32::try_from(raw).ok()
        } else if let Some(raw) = value.as_u64() {
            i32::try_from(raw).ok()
        } else {
            value.as_str()?.trim().parse::<i32>().ok()
        }
    }

    fn object_u32_field(value: &serde_json::Value, keys: &[&str]) -> Option<u32> {
        keys.iter()
            .find_map(|key| value.get(*key).and_then(Self::json_u32_value))
    }

    fn object_css_color_ref_field(value: &serde_json::Value, keys: &[&str]) -> Option<u32> {
        keys.iter()
            .find_map(|key| value.get(*key).and_then(Self::json_css_color_ref_value))
    }

    fn json_css_color_ref_field(json: &str, key: &str) -> Option<u32> {
        super::super::helpers::json_str(json, key)
            .and_then(|raw| Self::json_css_color_ref_value(&serde_json::Value::String(raw)))
    }

    fn json_css_color_ref_field_any(json: &str, keys: &[&str]) -> Option<u32> {
        keys.iter()
            .find_map(|key| Self::json_css_color_ref_field(json, key))
    }

    fn json_bool_field_any(json: &str, keys: &[&str]) -> Option<bool> {
        keys.iter()
            .find_map(|key| super::super::helpers::json_bool(json, key))
    }

    fn json_i16_field_any(json: &str, keys: &[&str]) -> Option<i16> {
        keys.iter()
            .find_map(|key| super::super::helpers::json_i16(json, key))
    }

    fn json_i32_field_any(json: &str, keys: &[&str]) -> Option<i32> {
        keys.iter()
            .find_map(|key| super::super::helpers::json_i32(json, key))
    }

    fn json_u32_field_any(json: &str, keys: &[&str]) -> Option<u32> {
        keys.iter()
            .find_map(|key| super::super::helpers::json_u32(json, key))
    }

    fn json_str_field_any(json: &str, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|key| super::super::helpers::json_str(json, key))
    }

    fn object_i32_field(value: &serde_json::Value, keys: &[&str]) -> Option<i32> {
        keys.iter()
            .find_map(|key| value.get(*key).and_then(Self::json_i32_value))
    }

    fn object_u8_field(value: &serde_json::Value, keys: &[&str]) -> Option<u8> {
        keys.iter().find_map(|key| {
            value
                .get(*key)
                .and_then(Self::json_i32_value)
                .map(|raw| raw.clamp(0, 255) as u8)
        })
    }

    fn shape_shadow_field(d: &crate::model::shape::DrawingObjAttr) -> String {
        if d.shadow_type == 0
            && d.shadow_color == 0
            && d.shadow_offset_x == 0
            && d.shadow_offset_y == 0
            && d.shadow_alpha == 0
        {
            return ",\"shadow\":null".to_string();
        }

        let type_name = Self::shape_shadow_type_name(d.shadow_type);
        let color_hex = Self::color_ref_to_hex(d.shadow_color);
        format!(
            ",\"shadow\":{{\"type\":{},\"typeName\":\"{}\",\"color\":{},\"colorHex\":\"{}\",\"offsetX\":{},\"offsetY\":{},\"alpha\":{}}}",
            d.shadow_type,
            type_name,
            d.shadow_color,
            color_hex,
            d.shadow_offset_x,
            d.shadow_offset_y,
            d.shadow_alpha
        )
    }

    fn shape_raw_hwpx_child_xml_field(d: &crate::model::shape::DrawingObjAttr) -> String {
        let raw = serde_json::to_string(&d.raw_hwpx_child_xml).unwrap_or_else(|_| "[]".to_string());
        format!(",\"rawHwpxChildXml\":{}", raw)
    }

    fn shape_xml_local_name(name: &[u8]) -> &[u8] {
        name.iter()
            .position(|&byte| byte == b':')
            .map(|idx| &name[idx + 1..])
            .unwrap_or(name)
    }

    fn shape_xml_attr_pairs(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
        e.attributes()
            .flatten()
            .filter_map(|attr| {
                let key = String::from_utf8_lossy(Self::shape_xml_local_name(attr.key.as_ref()))
                    .to_string();
                if key.is_empty() || key.starts_with("xmlns") {
                    return None;
                }
                let value = String::from_utf8_lossy(attr.value.as_ref()).to_string();
                Some((key, value))
            })
            .collect()
    }

    fn shape_xml_attrs_json(e: &quick_xml::events::BytesStart<'_>) -> String {
        let fields = Self::shape_xml_attr_pairs(e)
            .into_iter()
            .map(|(key, value)| {
                format!(
                    "\"{}\":\"{}\"",
                    super::super::helpers::json_escape(&key),
                    super::super::helpers::json_escape(&value)
                )
            })
            .collect::<Vec<_>>();
        format!("{{{}}}", fields.join(","))
    }

    fn shape_effect_rgb_hex_from_start(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
        let mut r = None;
        let mut g = None;
        let mut b = None;
        for (key, value) in Self::shape_xml_attr_pairs(e) {
            let parsed = value.trim().parse::<u8>().ok()?;
            match key.as_str() {
                "r" => r = Some(parsed),
                "g" => g = Some(parsed),
                "b" => b = Some(parsed),
                _ => {}
            }
        }
        Some(format!("#{:02X}{:02X}{:02X}", r?, g?, b?))
    }

    fn shape_effect_color_json_from_start(
        start: &quick_xml::events::BytesStart<'_>,
        reader: Option<&mut quick_xml::Reader<&[u8]>>,
    ) -> String {
        let mut fields = Self::shape_xml_attr_pairs(start)
            .into_iter()
            .map(|(key, value)| {
                format!(
                    "\"{}\":\"{}\"",
                    super::super::helpers::json_escape(&key),
                    super::super::helpers::json_escape(&value)
                )
            })
            .collect::<Vec<_>>();
        let Some(reader) = reader else {
            return format!("{{{}}}", fields.join(","));
        };

        let mut rgb: Option<String> = None;
        let mut color_hex: Option<String> = None;
        let mut raw_child_xml: Vec<String> = Vec::new();
        let mut depth = 1usize;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(ref e)) => {
                    if depth == 1 {
                        let start = e.to_owned();
                        if Self::shape_xml_local_name(e.name().as_ref()) == b"rgb" && rgb.is_none()
                        {
                            rgb = Some(Self::shape_xml_attrs_json(e));
                            color_hex = Self::shape_effect_rgb_hex_from_start(e);
                            let _ = Self::shape_capture_raw_xml_element(&start, reader);
                        } else if let Some(raw) =
                            Self::shape_capture_raw_xml_element(&start, reader)
                        {
                            raw_child_xml.push(raw);
                        }
                    } else {
                        depth += 1;
                    }
                }
                Ok(quick_xml::events::Event::Empty(ref e)) => {
                    if depth == 1 {
                        if Self::shape_xml_local_name(e.name().as_ref()) == b"rgb" && rgb.is_none()
                        {
                            rgb = Some(Self::shape_xml_attrs_json(e));
                            color_hex = Self::shape_effect_rgb_hex_from_start(e);
                        } else if let Some(raw) = Self::shape_raw_empty_xml_element(e) {
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

        if let Some(rgb) = rgb {
            fields.push(format!("\"rgb\":{}", rgb));
        }
        if let Some(color_hex) = color_hex {
            fields.push(format!("\"colorHex\":\"{}\"", color_hex));
        }
        if !raw_child_xml.is_empty() {
            let raw = serde_json::to_string(&raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn shape_three_d_json_from_start(
        start: &quick_xml::events::BytesStart<'_>,
        reader: Option<&mut quick_xml::Reader<&[u8]>>,
    ) -> String {
        let mut fields = Self::shape_xml_attr_pairs(start)
            .into_iter()
            .map(|(key, value)| {
                format!(
                    "\"{}\":\"{}\"",
                    super::super::helpers::json_escape(&key),
                    super::super::helpers::json_escape(&value)
                )
            })
            .collect::<Vec<_>>();
        let Some(reader) = reader else {
            return format!("{{{}}}", fields.join(","));
        };

        let mut bevel: Option<String> = None;
        let mut raw_child_xml: Vec<String> = Vec::new();
        let mut depth = 1usize;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(ref e)) => {
                    if depth == 1 {
                        if Self::shape_xml_local_name(e.name().as_ref()) == b"bevel"
                            && bevel.is_none()
                        {
                            bevel = Some(Self::shape_xml_attrs_json(e));
                            depth += 1;
                        } else {
                            let start = e.to_owned();
                            if let Some(raw) = Self::shape_capture_raw_xml_element(&start, reader) {
                                raw_child_xml.push(raw);
                            }
                        }
                    } else {
                        depth += 1;
                    }
                }
                Ok(quick_xml::events::Event::Empty(ref e)) => {
                    if depth == 1 {
                        if Self::shape_xml_local_name(e.name().as_ref()) == b"bevel"
                            && bevel.is_none()
                        {
                            bevel = Some(Self::shape_xml_attrs_json(e));
                        } else if let Some(raw) = Self::shape_raw_empty_xml_element(e) {
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

        if let Some(bevel) = bevel {
            fields.push(format!("\"bevel\":{}", bevel));
        }
        if !raw_child_xml.is_empty() {
            let raw = serde_json::to_string(&raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn shape_three_d_json_from_raw_fragment(raw: &str) -> Option<String> {
        if !raw.contains("threeD") && !raw.contains("three-d") {
            return None;
        }

        let mut reader = quick_xml::Reader::from_str(raw);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(ref e))
                    if Self::shape_xml_local_name(e.name().as_ref()) == b"threeD" =>
                {
                    let start = e.to_owned();
                    return Some(Self::shape_three_d_json_from_start(
                        &start,
                        Some(&mut reader),
                    ));
                }
                Ok(quick_xml::events::Event::Empty(ref e))
                    if Self::shape_xml_local_name(e.name().as_ref()) == b"threeD" =>
                {
                    return Some(Self::shape_three_d_json_from_start(e, None));
                }
                Ok(quick_xml::events::Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        None
    }

    fn shape_simple_effect_json_from_start(
        start: &quick_xml::events::BytesStart<'_>,
        reader: Option<&mut quick_xml::Reader<&[u8]>>,
    ) -> String {
        let mut fields = Self::shape_xml_attr_pairs(start)
            .into_iter()
            .map(|(key, value)| {
                format!(
                    "\"{}\":\"{}\"",
                    super::super::helpers::json_escape(&key),
                    super::super::helpers::json_escape(&value)
                )
            })
            .collect::<Vec<_>>();
        let Some(reader) = reader else {
            return format!("{{{}}}", fields.join(","));
        };

        let mut color: Option<String> = None;
        let mut skew: Option<String> = None;
        let mut scale: Option<String> = None;
        let mut alpha: Option<String> = None;
        let mut pos: Option<String> = None;
        let mut solid_fill: Option<String> = None;
        let mut raw_child_xml: Vec<String> = Vec::new();
        let mut depth = 1usize;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(ref e)) => {
                    if depth == 1 {
                        match Self::shape_xml_local_name(e.name().as_ref()) {
                            b"effectsColor" if color.is_none() => {
                                let start = e.to_owned();
                                color = Some(Self::shape_effect_color_json_from_start(
                                    &start,
                                    Some(reader),
                                ));
                            }
                            b"skew" if skew.is_none() => {
                                skew = Some(Self::shape_xml_attrs_json(e));
                                depth += 1;
                            }
                            b"scale" if scale.is_none() => {
                                scale = Some(Self::shape_xml_attrs_json(e));
                                depth += 1;
                            }
                            b"alpha" if alpha.is_none() => {
                                alpha = Some(Self::shape_xml_attrs_json(e));
                                depth += 1;
                            }
                            b"pos" if pos.is_none() => {
                                pos = Some(Self::shape_xml_attrs_json(e));
                                depth += 1;
                            }
                            b"solidFill" if solid_fill.is_none() => {
                                let start = e.to_owned();
                                solid_fill = Some(Self::shape_solid_fill_json_from_start(
                                    &start,
                                    Some(reader),
                                ));
                            }
                            _ => {
                                let start = e.to_owned();
                                if let Some(raw) =
                                    Self::shape_capture_raw_xml_element(&start, reader)
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
                        match Self::shape_xml_local_name(e.name().as_ref()) {
                            b"effectsColor" if color.is_none() => {
                                color = Some(Self::shape_effect_color_json_from_start(e, None));
                            }
                            b"skew" if skew.is_none() => {
                                skew = Some(Self::shape_xml_attrs_json(e));
                            }
                            b"scale" if scale.is_none() => {
                                scale = Some(Self::shape_xml_attrs_json(e));
                            }
                            b"alpha" if alpha.is_none() => {
                                alpha = Some(Self::shape_xml_attrs_json(e));
                            }
                            b"pos" if pos.is_none() => {
                                pos = Some(Self::shape_xml_attrs_json(e));
                            }
                            b"solidFill" if solid_fill.is_none() => {
                                solid_fill = Some(Self::shape_solid_fill_json_from_start(e, None));
                            }
                            _ => {
                                if let Some(raw) = Self::shape_raw_empty_xml_element(e) {
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

        if let Some(color) = color {
            fields.push(format!("\"color\":{}", color));
        }
        if let Some(skew) = skew {
            fields.push(format!("\"skew\":{}", skew));
        }
        if let Some(scale) = scale {
            fields.push(format!("\"scale\":{}", scale));
        }
        if let Some(alpha) = alpha {
            fields.push(format!("\"alpha\":{}", alpha));
        }
        if let Some(pos) = pos {
            fields.push(format!("\"pos\":{}", pos));
        }
        if let Some(solid_fill) = solid_fill {
            fields.push(format!("\"solidFill\":{}", solid_fill));
        }
        if !raw_child_xml.is_empty() {
            let raw = serde_json::to_string(&raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn shape_solid_fill_json_from_start(
        start: &quick_xml::events::BytesStart<'_>,
        reader: Option<&mut quick_xml::Reader<&[u8]>>,
    ) -> String {
        let mut fields = Self::shape_xml_attr_pairs(start)
            .into_iter()
            .map(|(key, value)| {
                format!(
                    "\"{}\":\"{}\"",
                    super::super::helpers::json_escape(&key),
                    super::super::helpers::json_escape(&value)
                )
            })
            .collect::<Vec<_>>();
        if let Some((_, color)) = Self::shape_xml_attr_pairs(start)
            .into_iter()
            .find(|(key, _)| key == "color")
        {
            if let Some(color_hex) = Self::normalize_xml_color_hex(&color) {
                fields.push(format!("\"colorHex\":\"{}\"", color_hex));
            }
        }
        let Some(reader) = reader else {
            return format!("{{{}}}", fields.join(","));
        };

        let mut color: Option<String> = None;
        let mut raw_child_xml: Vec<String> = Vec::new();
        let mut depth = 1usize;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(ref e)) => {
                    if depth == 1 {
                        match Self::shape_xml_local_name(e.name().as_ref()) {
                            b"effectsColor" if color.is_none() => {
                                let start = e.to_owned();
                                color = Some(Self::shape_effect_color_json_from_start(
                                    &start,
                                    Some(reader),
                                ));
                            }
                            _ => {
                                let start = e.to_owned();
                                if let Some(raw) =
                                    Self::shape_capture_raw_xml_element(&start, reader)
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
                        match Self::shape_xml_local_name(e.name().as_ref()) {
                            b"effectsColor" if color.is_none() => {
                                color = Some(Self::shape_effect_color_json_from_start(e, None));
                            }
                            _ => {
                                if let Some(raw) = Self::shape_raw_empty_xml_element(e) {
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

        if let Some(color) = color {
            fields.push(format!("\"color\":{}", color));
        }
        if !raw_child_xml.is_empty() {
            let raw = serde_json::to_string(&raw_child_xml).unwrap_or_else(|_| "[]".to_string());
            fields.push(format!("\"rawChildXml\":{}", raw));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn normalize_xml_color_hex(value: &str) -> Option<String> {
        let trimmed = value.trim();
        let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
        if hex.len() == 6 && hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            Some(format!("#{}", hex.to_ascii_uppercase()))
        } else {
            None
        }
    }

    fn shape_write_raw_xml_event<'a>(
        writer: &mut quick_xml::Writer<&mut Vec<u8>>,
        event: quick_xml::events::Event<'a>,
    ) -> Option<()> {
        writer.write_event(event).ok()
    }

    fn shape_raw_xml_string(raw: Vec<u8>) -> Option<String> {
        String::from_utf8(raw).ok()
    }

    fn shape_raw_empty_xml_element(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
        let mut raw = Vec::new();
        let mut writer = quick_xml::Writer::new(&mut raw);
        Self::shape_write_raw_xml_event(
            &mut writer,
            quick_xml::events::Event::Empty(e.to_owned()),
        )?;
        Self::shape_raw_xml_string(raw)
    }

    fn shape_capture_raw_xml_element(
        start: &quick_xml::events::BytesStart<'_>,
        reader: &mut quick_xml::Reader<&[u8]>,
    ) -> Option<String> {
        let mut raw = Vec::new();
        let mut writer = quick_xml::Writer::new(&mut raw);
        Self::shape_write_raw_xml_event(
            &mut writer,
            quick_xml::events::Event::Start(start.to_owned()),
        )?;

        let mut depth = 1usize;
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(ref e)) => {
                    depth += 1;
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::Start(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::End(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::End(e.to_owned()),
                    )?;
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                Ok(quick_xml::events::Event::Empty(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::Empty(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::Text(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::Text(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::CData(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::CData(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::Comment(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::Comment(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::PI(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::PI(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::DocType(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::DocType(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::GeneralRef(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::GeneralRef(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::Decl(ref e)) => {
                    Self::shape_write_raw_xml_event(
                        &mut writer,
                        quick_xml::events::Event::Decl(e.to_owned()),
                    )?;
                }
                Ok(quick_xml::events::Event::Eof) | Err(_) => break,
            }
            buf.clear();
        }

        Self::shape_raw_xml_string(raw)
    }

    fn shape_simple_effect_json_from_raw_fragment(raw: &str, effect_name: &[u8]) -> Option<String> {
        if !raw
            .as_bytes()
            .windows(effect_name.len())
            .any(|window| window.eq_ignore_ascii_case(effect_name))
        {
            return None;
        }

        let mut reader = quick_xml::Reader::from_str(raw);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(ref e))
                    if Self::shape_xml_local_name(e.name().as_ref()) == effect_name =>
                {
                    let start = e.to_owned();
                    return Some(Self::shape_simple_effect_json_from_start(
                        &start,
                        Some(&mut reader),
                    ));
                }
                Ok(quick_xml::events::Event::Empty(ref e))
                    if Self::shape_xml_local_name(e.name().as_ref()) == effect_name =>
                {
                    return Some(Self::shape_simple_effect_json_from_start(e, None));
                }
                Ok(quick_xml::events::Event::Eof) | Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        None
    }

    fn shape_effect_json_from_raw_fragment(raw: &str, effect_name: &[u8]) -> Option<String> {
        match effect_name {
            b"threeD" => Self::shape_three_d_json_from_raw_fragment(raw),
            b"shadow" | b"glow" | b"softEdge" | b"reflection" | b"blur" | b"fillOverlay" => {
                Self::shape_simple_effect_json_from_raw_fragment(raw, effect_name)
            }
            _ => None,
        }
    }

    fn shape_effects_field(d: &crate::model::shape::DrawingObjAttr) -> String {
        let effect_fields = [
            ("threeD", b"threeD".as_slice()),
            ("shadow", b"shadow".as_slice()),
            ("glow", b"glow".as_slice()),
            ("softEdge", b"softEdge".as_slice()),
            ("reflection", b"reflection".as_slice()),
            ("blur", b"blur".as_slice()),
            ("fillOverlay", b"fillOverlay".as_slice()),
        ]
        .into_iter()
        .map(|(json_key, effect_name)| {
            let value = d
                .raw_hwpx_child_xml
                .iter()
                .find_map(|raw| Self::shape_effect_json_from_raw_fragment(raw, effect_name));
            match value {
                Some(value) => format!("\"{}\":{}", json_key, value),
                None => format!("\"{}\":null", json_key),
            }
        })
        .collect::<Vec<_>>();
        format!(",\"effects\":{{{}}}", effect_fields.join(","))
    }

    fn shape_effect_value_with_top_aliases<'a>(
        value: &'a serde_json::Value,
        nested_keys: &[&str],
        top_level_keys: &[&str],
    ) -> Option<&'a serde_json::Value> {
        top_level_keys
            .iter()
            .find_map(|key| value.get(*key))
            .or_else(|| {
                value
                    .get("effects")
                    .and_then(|effects| nested_keys.iter().find_map(|key| effects.get(*key)))
            })
    }

    fn is_safe_xml_name(value: &str) -> bool {
        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        if !(first.is_ascii_alphabetic() || first == '_' || first == ':') {
            return false;
        }
        chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.'))
    }

    fn xml_attr_escape(value: &str) -> String {
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

    fn shape_effect_xml_attr_map(
        value: &serde_json::Value,
        skip_keys: &[&str],
    ) -> std::collections::BTreeMap<String, String> {
        let mut attrs = std::collections::BTreeMap::new();
        if let Some(attr_obj) = value.get("attrs").and_then(|attrs| attrs.as_object()) {
            for (key, raw_value) in attr_obj {
                if Self::is_safe_xml_name(key) {
                    if let Some(value) = Self::json_attr_string(raw_value) {
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
                if Self::is_safe_xml_name(key) {
                    if let Some(value) = Self::json_attr_string(raw_value) {
                        attrs.insert(key.to_string(), value);
                    }
                }
            }
        }
        attrs
    }

    fn shape_effect_xml_attrs(value: &serde_json::Value, skip_keys: &[&str]) -> String {
        Self::shape_effect_xml_attr_map(value, skip_keys)
            .into_iter()
            .map(|(key, value)| format!(" {}=\"{}\"", key, Self::xml_attr_escape(&value)))
            .collect::<Vec<_>>()
            .join("")
    }

    fn shape_effect_xml_attrs_from_map(
        attrs: std::collections::BTreeMap<String, String>,
    ) -> String {
        attrs
            .into_iter()
            .map(|(key, value)| format!(" {}=\"{}\"", key, Self::xml_attr_escape(&value)))
            .collect::<Vec<_>>()
            .join("")
    }

    fn shape_effect_rgb_xml_attrs_from_hex(value: &serde_json::Value) -> Option<String> {
        let color_ref = Self::json_css_color_ref_value(value)?;
        if color_ref == 0xFFFF_FFFF {
            return None;
        }
        let rgb = Self::color_ref_to_rgb_u32(color_ref);
        let r = (rgb >> 16) & 0xFF;
        let g = (rgb >> 8) & 0xFF;
        let b = rgb & 0xFF;
        Some(format!(" b=\"{b}\" g=\"{g}\" r=\"{r}\""))
    }

    fn shape_effect_color_xml(value: &serde_json::Value) -> Option<String> {
        if !value.is_object() {
            let rgb_attrs = Self::shape_effect_rgb_xml_attrs_from_hex(value)?;
            return Some(format!(
                r#"<hp:effectsColor presetIdx="-1" schemeIdx="-1" systemIdx="-1" type="RGB"><hp:rgb{rgb_attrs}/></hp:effectsColor>"#
            ));
        }
        let mut attr_map = Self::shape_effect_xml_attr_map(
            value,
            &[
                "colorType",
                "color_type",
                "rgb",
                "colorHex",
                "color_hex",
                "rgbHex",
                "rgb_hex",
                "scheme_idx",
                "system_idx",
                "preset_idx",
                "rawChildXml",
                "raw_child_xml",
                "rawChildrenXml",
                "raw_children_xml",
            ],
        );
        for (canonical, aliases) in [
            ("type", &["colorType", "color_type"][..]),
            ("schemeIdx", &["scheme_idx"][..]),
            ("systemIdx", &["system_idx"][..]),
            ("presetIdx", &["preset_idx"][..]),
        ] {
            if attr_map.contains_key(canonical) {
                continue;
            }
            if let Some(value) = Self::object_string_field(value, aliases) {
                attr_map.insert(canonical.to_string(), value);
            }
        }
        let mut children = Vec::new();
        if let Some(rgb_value) = value.get("rgb").filter(|value| value.is_object()) {
            let rgb_attrs = Self::shape_effect_xml_attrs(
                rgb_value,
                &["colorHex", "color_hex", "rgbHex", "rgb_hex"],
            );
            children.push(format!("<hp:rgb{rgb_attrs}/>"));
        } else if let Some(rgb_attrs) = value
            .get("colorHex")
            .or_else(|| value.get("color_hex"))
            .or_else(|| value.get("rgbHex"))
            .or_else(|| value.get("rgb_hex"))
            .and_then(Self::shape_effect_rgb_xml_attrs_from_hex)
        {
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

        children.extend(Self::shape_effect_raw_child_xml_values(value));
        let attrs = Self::shape_effect_xml_attrs_from_map(attr_map);
        if children.is_empty() {
            Some(format!("<hp:effectsColor{attrs}/>"))
        } else {
            Some(format!(
                "<hp:effectsColor{attrs}>{}</hp:effectsColor>",
                children.join("")
            ))
        }
    }

    fn shape_effect_child_xml(value: &serde_json::Value, xml_name: &str) -> Option<String> {
        if !value.is_object() {
            return None;
        }
        let attrs = Self::shape_effect_xml_attrs(value, &[]);
        Some(format!("<hp:{xml_name}{attrs}/>"))
    }

    fn shape_three_d_child_xml(value: &serde_json::Value) -> Option<String> {
        if !value.is_object() {
            return None;
        }

        let three_d_attrs = Self::shape_effect_xml_attrs(
            value,
            &[
                "bevel",
                "bevelType",
                "bevel_type",
                "rawChildXml",
                "raw_child_xml",
                "rawChildrenXml",
                "raw_children_xml",
            ],
        );
        let bevel_xml = if let Some(bevel_value) = value.get("bevel") {
            if bevel_value.is_null() {
                None
            } else if let Some(bevel_type) = bevel_value.as_str() {
                Some(format!(
                    "<hp:bevel type=\"{}\"/>",
                    Self::xml_attr_escape(bevel_type)
                ))
            } else if bevel_value.is_object() {
                let attrs = Self::shape_effect_xml_attrs(bevel_value, &[]);
                Some(format!("<hp:bevel{attrs}/>"))
            } else {
                None
            }
        } else {
            Self::object_string_field(value, &["bevelType", "bevel_type"]).map(|bevel_type| {
                format!(
                    "<hp:bevel type=\"{}\"/>",
                    Self::xml_attr_escape(&bevel_type)
                )
            })
        };

        let mut children = Vec::new();
        if let Some(bevel_xml) = bevel_xml {
            children.push(bevel_xml);
        }
        children.extend(Self::shape_effect_raw_child_xml_values(value));

        let three_d = if children.is_empty() {
            format!("<hp:threeD{three_d_attrs}/>")
        } else {
            format!(
                "<hp:threeD{three_d_attrs}>{}</hp:threeD>",
                children.join("")
            )
        };
        Some(three_d)
    }

    fn shape_simple_effect_child_xml(value: &serde_json::Value, xml_name: &str) -> Option<String> {
        if !value.is_object() {
            return None;
        }
        let mut skip_keys = match xml_name {
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
            _ => vec!["color", "effectsColor"],
        };
        let direct_color_value =
            if value.get("color").is_none() && value.get("effectsColor").is_none() {
                Self::direct_effect_color_json(value)
            } else {
                None
            };
        if direct_color_value.is_some() {
            skip_keys.extend([
                "type",
                "colorType",
                "color_type",
                "schemeIdx",
                "scheme_idx",
                "systemIdx",
                "system_idx",
                "presetIdx",
                "preset_idx",
                "rgb",
                "colorHex",
                "color_hex",
                "rgbHex",
                "rgb_hex",
            ]);
        }
        skip_keys.extend([
            "rawChildXml",
            "raw_child_xml",
            "rawChildrenXml",
            "raw_children_xml",
            "solidFill",
            "solid_fill",
        ]);
        let attrs = Self::shape_effect_xml_attrs(value, &skip_keys);
        let color_value = value.get("color").or_else(|| value.get("effectsColor"));
        let mut children = Vec::new();
        if let Some(color_xml) = color_value.and_then(Self::shape_effect_color_xml) {
            children.push(color_xml);
        } else if let Some(color_xml) = direct_color_value
            .as_ref()
            .and_then(Self::shape_effect_color_xml)
        {
            children.push(color_xml);
        }
        for child_name in ["skew", "scale", "alpha", "pos"] {
            if let Some(child_xml) = value
                .get(child_name)
                .and_then(|child| Self::shape_effect_child_xml(child, child_name))
            {
                children.push(child_xml);
            } else if let Some(attrs) = Self::shape_flat_effect_child_attrs(value, child_name) {
                children.push(format!("<hp:{child_name}{attrs}/>"));
            }
        }
        if let Some(solid_fill_xml) = value
            .get("solidFill")
            .or_else(|| value.get("solid_fill"))
            .and_then(Self::shape_solid_fill_child_xml)
        {
            children.push(solid_fill_xml);
        }
        children.extend(Self::shape_effect_raw_child_xml_values(value));
        if children.is_empty() {
            Some(format!("<hp:{xml_name}{attrs}/>"))
        } else {
            Some(format!(
                "<hp:{xml_name}{attrs}>{}</hp:{xml_name}>",
                children.join("")
            ))
        }
    }

    fn shape_flat_effect_child_attrs(
        value: &serde_json::Value,
        child_name: &str,
    ) -> Option<String> {
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
            if let Some(value) = Self::object_string_field(value, keys) {
                attrs.insert((*attr).to_string(), value);
            }
        }
        if attrs.is_empty() {
            None
        } else {
            Some(Self::shape_effect_xml_attrs_from_map(attrs))
        }
    }

    fn shape_solid_fill_child_xml(value: &serde_json::Value) -> Option<String> {
        if let Some(color) = value.as_str() {
            return Some(format!(
                "<hp:solidFill color=\"{}\"/>",
                Self::xml_attr_escape(color)
            ));
        }
        if !value.is_object() {
            return None;
        }
        let mut attr_map = Self::shape_effect_xml_attr_map(
            value,
            &[
                "colorHex",
                "color_hex",
                "color",
                "effectsColor",
                "rawChildXml",
                "raw_child_xml",
                "rawChildrenXml",
                "raw_children_xml",
            ],
        );
        if !attr_map.contains_key("color") {
            if let Some(color) = Self::object_string_field(value, &["colorHex", "color_hex"]) {
                attr_map.insert("color".to_string(), color);
            }
        }
        let attrs = Self::shape_effect_xml_attrs_from_map(attr_map);
        let mut children = Vec::new();
        if let Some(color_xml) = value
            .get("color")
            .or_else(|| value.get("effectsColor"))
            .and_then(Self::shape_effect_color_xml)
        {
            children.push(color_xml);
        }
        children.extend(Self::shape_effect_raw_child_xml_values(value));
        Some(if children.is_empty() {
            format!("<hp:solidFill{attrs}/>")
        } else {
            format!("<hp:solidFill{attrs}>{}</hp:solidFill>", children.join(""))
        })
    }

    fn shape_effect_raw_child_xml_values(value: &serde_json::Value) -> Vec<String> {
        let Some(raw_value) = Self::shape_effect_raw_child_xml_value(value) else {
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

    fn shape_effect_raw_child_xml_value(value: &serde_json::Value) -> Option<&serde_json::Value> {
        value
            .get("rawChildXml")
            .or_else(|| value.get("raw_child_xml"))
            .or_else(|| value.get("rawChildrenXml"))
            .or_else(|| value.get("raw_children_xml"))
    }

    fn shape_effect_raw_xml(effect_name: &str, value: &serde_json::Value) -> Option<String> {
        let child = match effect_name {
            "threeD" => Self::shape_three_d_child_xml(value),
            "shadow" | "glow" | "softEdge" | "reflection" | "blur" | "fillOverlay" => {
                Self::shape_simple_effect_child_xml(value, effect_name)
            }
            _ => None,
        }?;
        Some(format!("<hp:effects>{child}</hp:effects>"))
    }

    fn shape_raw_fragment_contains_effect(raw: &str, effect_name: &[u8]) -> bool {
        Self::shape_effect_json_from_raw_fragment(raw, effect_name).is_some()
    }

    fn apply_shape_effects_props(d: &mut crate::model::shape::DrawingObjAttr, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };

        let effect_specs = [
            (
                "threeD",
                b"threeD".as_slice(),
                &["threeD", "three_d", "shapeThreeD", "shape_three_d"] as &[&str],
                &["threeD", "three_d", "shapeThreeD", "shape_three_d"] as &[&str],
            ),
            (
                "glow",
                b"glow".as_slice(),
                &["glow", "shapeGlow", "shape_glow"] as &[&str],
                &["glow", "shapeGlow", "shape_glow"] as &[&str],
            ),
            (
                "shadow",
                b"shadow".as_slice(),
                &[
                    "shadow",
                    "effectShadow",
                    "effect_shadow",
                    "shapeEffectShadow",
                    "shape_effect_shadow",
                ] as &[&str],
                &[
                    "effectShadow",
                    "effect_shadow",
                    "shapeEffectShadow",
                    "shape_effect_shadow",
                ] as &[&str],
            ),
            (
                "softEdge",
                b"softEdge".as_slice(),
                &["softEdge", "soft_edge", "shapeSoftEdge", "shape_soft_edge"] as &[&str],
                &["softEdge", "soft_edge", "shapeSoftEdge", "shape_soft_edge"] as &[&str],
            ),
            (
                "reflection",
                b"reflection".as_slice(),
                &["reflection", "shapeReflection", "shape_reflection"] as &[&str],
                &["reflection", "shapeReflection", "shape_reflection"] as &[&str],
            ),
            (
                "blur",
                b"blur".as_slice(),
                &["blur", "shapeBlur", "shape_blur"] as &[&str],
                &["blur", "shapeBlur", "shape_blur"] as &[&str],
            ),
            (
                "fillOverlay",
                b"fillOverlay".as_slice(),
                &[
                    "fillOverlay",
                    "fill_overlay",
                    "shapeFillOverlay",
                    "shape_fill_overlay",
                ] as &[&str],
                &[
                    "fillOverlay",
                    "fill_overlay",
                    "shapeFillOverlay",
                    "shape_fill_overlay",
                ] as &[&str],
            ),
        ];

        for (xml_name, effect_name, nested_keys, top_level_keys) in effect_specs {
            let Some(effect_value) =
                Self::shape_effect_value_with_top_aliases(&value, nested_keys, top_level_keys)
            else {
                continue;
            };
            d.raw_hwpx_child_xml
                .retain(|raw| !Self::shape_raw_fragment_contains_effect(raw, effect_name));
            if effect_value.is_null() {
                continue;
            }
            if let Some(raw_xml) = Self::shape_effect_raw_xml(xml_name, effect_value) {
                d.raw_hwpx_child_xml.push(raw_xml);
            }
        }
    }

    fn apply_shape_shadow_props(d: &mut crate::model::shape::DrawingObjAttr, props_json: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(shadow_value) = value.get("shadow") else {
            return;
        };
        if shadow_value.is_null() {
            d.shadow_type = 0;
            d.shadow_color = 0;
            d.shadow_offset_x = 0;
            d.shadow_offset_y = 0;
            d.shadow_alpha = 0;
            return;
        }
        if !shadow_value.is_object() {
            return;
        }

        if let Some(v) = Self::object_u32_field(shadow_value, &["type", "shadowType"]) {
            d.shadow_type = v;
        } else if let Some(v) =
            Self::object_string_field(shadow_value, &["type", "typeName", "type_name"])
        {
            if let Some(shadow_type) = Self::shape_shadow_type_from_name(&v) {
                d.shadow_type = shadow_type;
            }
        }
        if let Some(v) = Self::object_css_color_ref_field(shadow_value, &["colorHex", "color_hex"])
        {
            d.shadow_color = v;
        } else if let Some(v) = Self::object_u32_field(shadow_value, &["color", "shadowColor"]) {
            d.shadow_color = v;
        }
        if let Some(v) =
            Self::object_i32_field(shadow_value, &["offsetX", "offset_x", "shadowOffsetX"])
        {
            d.shadow_offset_x = v;
        }
        if let Some(v) =
            Self::object_i32_field(shadow_value, &["offsetY", "offset_y", "shadowOffsetY"])
        {
            d.shadow_offset_y = v;
        }
        if let Some(v) = Self::object_u8_field(shadow_value, &["alpha", "shadowAlpha"]) {
            d.shadow_alpha = v;
        }
    }

    fn shape_raw_hwpx_child_xml_value<'a>(
        value: &'a serde_json::Value,
    ) -> Option<&'a serde_json::Value> {
        value
            .get("rawHwpxChildXml")
            .or_else(|| value.get("raw_hwpx_child_xml"))
            .or_else(|| value.get("shapeRawXml"))
            .or_else(|| value.get("shape_raw_xml"))
            .or_else(|| {
                value.get("effects").and_then(|effects| {
                    effects
                        .get("rawHwpxChildXml")
                        .or_else(|| effects.get("raw_hwpx_child_xml"))
                        .or_else(|| effects.get("rawXml"))
                        .or_else(|| effects.get("raw_xml"))
                })
            })
    }

    fn apply_shape_raw_hwpx_child_xml_props(
        d: &mut crate::model::shape::DrawingObjAttr,
        props_json: &str,
    ) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) else {
            return;
        };
        let Some(raw_value) = Self::shape_raw_hwpx_child_xml_value(&value) else {
            return;
        };
        if raw_value.is_null() {
            d.raw_hwpx_child_xml.clear();
            return;
        }
        if let Some(raw) = raw_value.as_str() {
            d.raw_hwpx_child_xml = vec![raw.to_string()];
            return;
        }
        if let Some(values) = raw_value.as_array() {
            d.raw_hwpx_child_xml = values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
        }
    }

    fn picture_rotated_bounds(width: u32, height: u32, angle: i16) -> (u32, u32) {
        if width == 0 || height == 0 || angle.rem_euclid(360) == 0 {
            return (width, height);
        }

        let theta = (angle as f64).to_radians();
        let cos = theta.cos().abs();
        let sin = theta.sin().abs();
        let rotated_width = width as f64 * cos + height as f64 * sin;
        let rotated_height = width as f64 * sin + height as f64 * cos;
        (
            rotated_width.round().max(1.0) as u32,
            rotated_height.round().max(1.0) as u32,
        )
    }

    fn refresh_picture_rotation_layout_for_save(pic: &mut crate::model::image::Picture) {
        let cur_w = if pic.shape_attr.current_width > 0 {
            pic.shape_attr.current_width
        } else {
            pic.common.width
        };
        let cur_h = if pic.shape_attr.current_height > 0 {
            pic.shape_attr.current_height
        } else {
            pic.common.height
        };

        if cur_w == 0 || cur_h == 0 {
            return;
        }

        pic.shape_attr.current_width = cur_w;
        pic.shape_attr.current_height = cur_h;

        let old_center_x =
            pic.common.horizontal_offset as i32 as i64 + (pic.common.width as i64 / 2);
        let old_center_y =
            pic.common.vertical_offset as i32 as i64 + (pic.common.height as i64 / 2);
        let (bbox_w, bbox_h) =
            Self::picture_rotated_bounds(cur_w, cur_h, pic.shape_attr.rotation_angle);

        if pic.shape_attr.rotation_angle.rem_euclid(360) != 0 {
            pic.common.width = bbox_w;
            pic.common.height = bbox_h;
            pic.common.horizontal_offset = (old_center_x - (bbox_w as i64 / 2)) as i32 as u32;
            pic.common.vertical_offset = (old_center_y - (bbox_h as i64 / 2)) as i32 as u32;
        } else {
            pic.common.width = cur_w;
            pic.common.height = cur_h;
            pic.common.horizontal_offset = (old_center_x - (cur_w as i64 / 2)) as i32 as u32;
            pic.common.vertical_offset = (old_center_y - (cur_h as i64 / 2)) as i32 as u32;
        }

        pic.shape_attr.rotation_center.x = (pic.common.width / 2) as i32;
        pic.shape_attr.rotation_center.y = (pic.common.height / 2) as i32;
        pic.shape_attr.rotate_image = true;
        pic.shape_attr.flip |= 0x0008_0000;
    }

    fn apply_picture_display_width(pic: &mut crate::model::image::Picture, width: u32) {
        let old_common_width = pic.common.width;
        let old_current_width = pic.shape_attr.current_width;
        pic.common.width = width;
        if pic.shape_attr.rotation_angle.rem_euclid(360) != 0
            && old_common_width > 0
            && old_current_width > 0
        {
            pic.shape_attr.current_width =
                ((old_current_width as f64 * width as f64 / old_common_width as f64).round())
                    .max(1.0) as u32;
        } else {
            pic.shape_attr.current_width = width;
        }
    }

    fn apply_picture_display_height(pic: &mut crate::model::image::Picture, height: u32) {
        let old_common_height = pic.common.height;
        let old_current_height = pic.shape_attr.current_height;
        pic.common.height = height;
        if pic.shape_attr.rotation_angle.rem_euclid(360) != 0
            && old_common_height > 0
            && old_current_height > 0
        {
            pic.shape_attr.current_height =
                ((old_current_height as f64 * height as f64 / old_common_height as f64).round())
                    .max(1.0) as u32;
        } else {
            pic.shape_attr.current_height = height;
        }
    }

    /// [Task #825] 머리말/꼬리말 안 그림의 속성 조회.
    /// path: section[si].paragraphs[outer_para].controls[outer_ctrl] = Header/Footer
    ///       → .paragraphs[inner_para].controls[inner_ctrl] = Picture
    pub fn get_header_footer_picture_properties_native(
        &self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let outer_para = section.paragraphs.get(outer_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
        })?;
        let outer_ctrl = outer_para.controls.get(outer_control_idx).ok_or_else(|| {
            HwpError::RenderError(format!(
                "외부 컨트롤 인덱스 {} 범위 초과",
                outer_control_idx
            ))
        })?;

        let inner_paras: &[crate::model::paragraph::Paragraph] = match outer_ctrl {
            crate::model::control::Control::Header(h) => &h.paragraphs,
            crate::model::control::Control::Footer(f) => &f.paragraphs,
            _ => {
                return Err(HwpError::RenderError(
                    "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                ))
            }
        };

        let inner_para = inner_paras.get(inner_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
        })?;
        let inner_ctrl = inner_para.controls.get(inner_control_idx).ok_or_else(|| {
            HwpError::RenderError(format!(
                "내부 컨트롤 인덱스 {} 범위 초과",
                inner_control_idx
            ))
        })?;

        let pic = match inner_ctrl {
            crate::model::control::Control::Picture(p) => p,
            _ => {
                return Err(HwpError::RenderError(
                    "지정된 내부 컨트롤이 그림이 아닙니다".to_string(),
                ))
            }
        };
        Self::format_picture_properties_json(pic)
    }

    /// 머리말/꼬리말 안 Shape/OLE/Chart 속성 조회.
    pub fn get_header_footer_shape_properties_native(
        &self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let outer_para = section.paragraphs.get(outer_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
        })?;
        let outer_ctrl = outer_para.controls.get(outer_control_idx).ok_or_else(|| {
            HwpError::RenderError(format!(
                "외부 컨트롤 인덱스 {} 범위 초과",
                outer_control_idx
            ))
        })?;

        let inner_paras: &[crate::model::paragraph::Paragraph] = match outer_ctrl {
            crate::model::control::Control::Header(h) => &h.paragraphs,
            crate::model::control::Control::Footer(f) => &f.paragraphs,
            _ => {
                return Err(HwpError::RenderError(
                    "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                ))
            }
        };
        let inner_para = inner_paras.get(inner_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
        })?;
        let inner_ctrl = inner_para.controls.get(inner_control_idx).ok_or_else(|| {
            HwpError::RenderError(format!(
                "내부 컨트롤 인덱스 {} 범위 초과",
                inner_control_idx
            ))
        })?;
        let shape = match inner_ctrl {
            crate::model::control::Control::Shape(shape) => shape.as_ref(),
            _ => {
                return Err(HwpError::RenderError(
                    "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                ))
            }
        };
        Self::format_shape_props_inner(shape)
    }

    fn format_picture_properties_json(
        pic: &crate::model::image::Picture,
    ) -> Result<String, HwpError> {
        let c = &pic.common;
        let vert_rel = match c.vert_rel_to {
            crate::model::shape::VertRelTo::Paper => "Paper",
            crate::model::shape::VertRelTo::Page => "Page",
            crate::model::shape::VertRelTo::Para => "Para",
        };
        let vert_align = match c.vert_align {
            crate::model::shape::VertAlign::Top => "Top",
            crate::model::shape::VertAlign::Center => "Center",
            crate::model::shape::VertAlign::Bottom => "Bottom",
            crate::model::shape::VertAlign::Inside => "Inside",
            crate::model::shape::VertAlign::Outside => "Outside",
        };
        let horz_rel = match c.horz_rel_to {
            crate::model::shape::HorzRelTo::Paper => "Paper",
            crate::model::shape::HorzRelTo::Page => "Page",
            crate::model::shape::HorzRelTo::Column => "Column",
            crate::model::shape::HorzRelTo::Para => "Para",
        };
        let horz_align = match c.horz_align {
            crate::model::shape::HorzAlign::Left => "Left",
            crate::model::shape::HorzAlign::Center => "Center",
            crate::model::shape::HorzAlign::Right => "Right",
            crate::model::shape::HorzAlign::Inside => "Inside",
            crate::model::shape::HorzAlign::Outside => "Outside",
        };
        let text_wrap = match c.text_wrap {
            crate::model::shape::TextWrap::Square => "Square",
            crate::model::shape::TextWrap::Tight => "Tight",
            crate::model::shape::TextWrap::Through => "Through",
            crate::model::shape::TextWrap::TopAndBottom => "TopAndBottom",
            crate::model::shape::TextWrap::BehindText => "BehindText",
            crate::model::shape::TextWrap::InFrontOfText => "InFrontOfText",
        };
        let effect = match pic.image_attr.effect {
            crate::model::image::ImageEffect::RealPic => "RealPic",
            crate::model::image::ImageEffect::GrayScale => "GrayScale",
            crate::model::image::ImageEffect::BlackWhite => "BlackWhite",
            crate::model::image::ImageEffect::Pattern8x8 => "Pattern8x8",
        };
        // description 내 JSON 제어 문자 이스케이프
        let desc_escaped = super::super::helpers::json_escape(&c.description);
        let dropcap_style =
            super::super::helpers::json_escape(c.dropcap_style.as_deref().unwrap_or("None"));
        // [Task #741 후속] 외부 file path (HWP3 외부 그림) 영역 영역 dialog 표시 영역
        let external_path_field = match &pic.image_attr.external_path {
            Some(p) => format!(
                ",\"externalPath\":\"{}\"",
                super::super::helpers::json_escape(p)
            ),
            None => String::new(),
        };

        let sa = &pic.shape_attr;
        let (crop_left, crop_top, crop_right, crop_bottom) = Self::picture_crop_ui_amounts(pic);
        let text_flow = Self::text_flow_json_name(c.text_flow);
        let numbering_type = Self::object_numbering_type_json_name(c.numbering_type);
        let width_criterion = Self::size_criterion_json_name(c.width_criterion);
        let height_criterion = Self::size_criterion_json_name(c.height_criterion);
        let href = super::super::helpers::json_escape(pic.href.as_deref().unwrap_or(""));
        let shadow_field = Self::picture_effects_shadow_field(pic);
        let glow_field = Self::picture_effects_glow_field(pic);
        let soft_edge_field = Self::picture_effects_soft_edge_field(pic);
        let reflection_field = Self::picture_effects_reflection_field(pic);
        let three_d_field = Self::picture_effects_three_d_field(pic);
        let blur_field = Self::picture_effects_blur_field(pic);
        let fill_overlay_field = Self::picture_effects_fill_overlay_field(pic);
        let raw_effects_field = Self::picture_effects_raw_xml_field(pic);

        let caption_text = pic
            .caption
            .as_ref()
            .and_then(|caption| caption.paragraphs.first())
            .map(|para| super::super::helpers::json_escape(&para.text))
            .unwrap_or_default();

        Ok(format!(
            concat!(
                "{{\"width\":{},\"height\":{},\"treatAsChar\":{},",
                "\"vertRelTo\":\"{}\",\"vertAlign\":\"{}\",",
                "\"horzRelTo\":\"{}\",\"horzAlign\":\"{}\",",
                "\"vertOffset\":{},\"horzOffset\":{},",
                "\"textWrap\":\"{}\",\"restrictInPage\":{},\"allowOverlap\":{},\"sizeProtect\":{},",
                "\"textFlow\":\"{}\",\"numberingType\":\"{}\",\"numberingTypeExplicit\":{},",
                "\"lock\":{},\"widthCriterion\":\"{}\",\"heightCriterion\":\"{}\",",
                "\"zOrder\":{},\"instanceId\":{},\"instId\":{},\"pictureInstanceId\":{},\"groupLevel\":{},\"href\":\"{}\",",
                "\"brightness\":{},\"contrast\":{},\"effect\":\"{}\",\"transparency\":{},",
                "\"description\":\"{}\",\"dropcapStyle\":\"{}\",",
                // 회전/대칭
                "\"rotationAngle\":{},\"rotateImage\":{},\"horzFlip\":{},\"vertFlip\":{},",
                // 원본 크기
                "\"originalWidth\":{},\"originalHeight\":{},",
                // 자르기
                "\"cropLeft\":{},\"cropTop\":{},\"cropRight\":{},\"cropBottom\":{},",
                // 안쪽 여백 (그림 여백)
                "\"paddingLeft\":{},\"paddingTop\":{},\"paddingRight\":{},\"paddingBottom\":{},",
                // 바깥 여백
                "\"outerMarginLeft\":{},\"outerMarginTop\":{},\"outerMarginRight\":{},\"outerMarginBottom\":{},",
                // 테두리
                "\"borderColor\":{},\"borderColorHex\":\"{}\",\"borderWidth\":{},",
                // 캡션
                "\"hasCaption\":{},\"captionDirection\":\"{}\",\"captionVertAlign\":\"{}\",",
                "\"captionWidth\":{},\"captionSpacing\":{},\"captionMaxWidth\":{},\"captionIncludeMargin\":{},",
                "\"captionText\":\"{}\"{}{}{}{}{}{}{}{}{}}}"
            ),
            c.width, c.height, c.treat_as_char,
            vert_rel, vert_align,
            horz_rel, horz_align,
            c.vertical_offset as i32, c.horizontal_offset as i32,
            text_wrap, c.flow_with_text, c.allow_overlap, c.size_protect,
            text_flow,
            numbering_type,
            c.numbering_type_explicit,
            c.lock,
            width_criterion,
            height_criterion,
            c.z_order,
            c.instance_id,
            pic.instance_id,
            pic.instance_id,
            sa.group_level,
            href,
            pic.image_attr.brightness,
            pic.image_attr.contrast,
            effect,
            pic.image_attr.clamped_transparency(),
            desc_escaped,
            dropcap_style,
            // 회전/대칭
            sa.rotation_angle, sa.rotate_image, sa.horz_flip, sa.vert_flip,
            // 원본 크기
            sa.original_width, sa.original_height,
            // 자르기
            crop_left, crop_top, crop_right, crop_bottom,
            // 안쪽 여백
            pic.padding.left, pic.padding.top, pic.padding.right, pic.padding.bottom,
            // 바깥 여백
            c.margin.left, c.margin.top, c.margin.right, c.margin.bottom,
            // 테두리
            pic.border_color,
            Self::color_ref_to_hex(pic.border_color),
            pic.border_width,
            // 캡션
            pic.caption.is_some(),
            pic.caption.as_ref().map_or("Bottom", |cap| match cap.direction {
                crate::model::shape::CaptionDirection::Left => "Left",
                crate::model::shape::CaptionDirection::Right => "Right",
                crate::model::shape::CaptionDirection::Top => "Top",
                crate::model::shape::CaptionDirection::Bottom => "Bottom",
            }),
            pic.caption.as_ref().map_or("Top", |cap| match cap.vert_align {
                crate::model::shape::CaptionVertAlign::Top => "Top",
                crate::model::shape::CaptionVertAlign::Center => "Center",
                crate::model::shape::CaptionVertAlign::Bottom => "Bottom",
            }),
            pic.caption.as_ref().map_or(0u32, |cap| cap.width),
            pic.caption.as_ref().map_or(0i16, |cap| cap.spacing),
            pic.caption.as_ref().map_or(0u32, |cap| cap.max_width),
            pic.caption.as_ref().map_or(false, |cap| cap.include_margin),
            caption_text,
            shadow_field,
            glow_field,
            soft_edge_field,
            reflection_field,
            three_d_field,
            blur_field,
            fill_overlay_field,
            raw_effects_field,
            external_path_field,
        ))
    }

    fn chart_ooxml_location_for_shape(
        &self,
        shape: &ShapeObject,
    ) -> Result<ChartOoxmlLocation, HwpError> {
        let bin_data_id = match shape {
            ShapeObject::Ole(ole) if ole.bin_data_id != 0 => ole.bin_data_id as u16,
            ShapeObject::Ole(_) => {
                return Err(HwpError::RenderError(
                    "차트/OLE 컨트롤에 chart BinData 참조가 없습니다".to_string(),
                ))
            }
            ShapeObject::Chart(_) => {
                return Err(HwpError::RenderError(
                    "HWP CHART_DATA 레코드의 semantic 편집은 아직 지원하지 않습니다".to_string(),
                ))
            }
            _ => {
                return Err(HwpError::RenderError(
                    "지정된 컨트롤이 OOXML chart OLE이 아닙니다".to_string(),
                ))
            }
        };

        let direct_xml = self
            .document
            .bin_data_content
            .iter()
            .enumerate()
            .find(|(_, content)| {
                content.id == bin_data_id && content.extension.eq_ignore_ascii_case("ooxml_chart")
            });
        if let Some((content_idx, _)) = direct_xml {
            return Ok(ChartOoxmlLocation::DirectXml {
                content_idx,
                bin_data_id,
            });
        }

        let ole_container =
            self.document
                .bin_data_content
                .iter()
                .enumerate()
                .find(|(_, content)| {
                    content.id == bin_data_id
                        && crate::parser::ole_container::parse_ole_container(&content.data)
                            .is_some_and(|container| container.has_ooxml_chart())
                });
        if let Some((content_idx, _)) = ole_container {
            return Ok(ChartOoxmlLocation::OleContainer {
                content_idx,
                bin_data_id,
            });
        }

        Err(HwpError::RenderError(format!(
            "OOXML chart payload를 찾을 수 없습니다: bin_data_id={}",
            bin_data_id
        )))
    }

    fn chart_ooxml_xml_for_location(
        &self,
        location: ChartOoxmlLocation,
    ) -> Result<Vec<u8>, HwpError> {
        let content = self
            .document
            .bin_data_content
            .get(location.content_idx())
            .ok_or_else(|| {
                HwpError::RenderError(format!(
                    "OOXML chart payload index 범위 초과: {}",
                    location.content_idx()
                ))
            })?;
        match location {
            ChartOoxmlLocation::DirectXml { .. } => Ok(content.data.clone()),
            ChartOoxmlLocation::OleContainer { .. } => {
                let container = crate::parser::ole_container::parse_ole_container(&content.data)
                    .ok_or_else(|| {
                        HwpError::RenderError(
                            "OLE chart container를 파싱할 수 없습니다".to_string(),
                        )
                    })?;
                container.ooxml_chart.ok_or_else(|| {
                    HwpError::RenderError(
                        "OLE chart container에 OOXMLChartContents가 없습니다".to_string(),
                    )
                })
            }
        }
    }

    fn legacy_ole_chart_contents_for_shape(
        &self,
        shape: &ShapeObject,
    ) -> Result<Option<LegacyOleChartContents>, HwpError> {
        let bin_data_id = match shape {
            ShapeObject::Ole(ole) if ole.bin_data_id != 0 => ole.bin_data_id as u16,
            _ => return Ok(None),
        };

        let Some(content) = self.document.bin_data_content.iter().find(|content| {
            content.id == bin_data_id && content.extension.eq_ignore_ascii_case("OLE")
        }) else {
            return Ok(None);
        };

        let Some(container) = crate::parser::ole_container::parse_ole_container(&content.data)
        else {
            return Ok(None);
        };
        let Some(raw_contents) = container.raw_contents else {
            return Ok(None);
        };

        Ok(Some(LegacyOleChartContents {
            bin_data_id,
            raw_contents,
        }))
    }

    fn ooxml_chart_json(
        bin_data_id: u16,
        editable: bool,
        chart: &crate::ooxml_chart::OoxmlChart,
    ) -> Result<String, HwpError> {
        let series: Vec<serde_json::Value> = chart
            .series
            .iter()
            .enumerate()
            .map(|(index, series)| {
                serde_json::json!({
                    "index": index,
                    "name": series.name,
                    "values": series.values,
                    "seriesType": Self::ooxml_chart_type_name(series.series_type),
                    "axisGroup": series.axis_group,
                    "formatCode": series.format_code,
                    "color": series.color,
                    "colorHex": series.color.map(|rgb| format!("#{rgb:06X}")),
                    "lineColor": series.line_color,
                    "lineColorHex": series.line_color.map(|rgb| format!("#{rgb:06X}")),
                    "lineWidth": series.line_width,
                })
            })
            .collect();
        serde_json::to_string(&serde_json::json!({
            "sourceKind": "OoxmlChart",
            "binDataId": bin_data_id,
            "editable": editable,
            "chartType": Self::ooxml_chart_type_name(chart.chart_type),
            "chartTypeLabel": chart.chart_type.label(),
            "supportedChartTypeValues": ["Column", "Bar"],
            "chartTypeSupportNote": "OOXML chartType editing currently supports Column/Bar conversion through c:barDir. Other OOXML chart families keep their parsed chartType and expose family-specific semantic fields.",
            "title": &chart.title,
            "categories": chart.categories,
            "series": series,
            "seriesCount": chart.series.len(),
            "categoryCount": chart.categories.len(),
            "hasSecondaryAxis": chart.has_secondary_axis,
            "grouping": Self::ooxml_chart_grouping_name(chart.grouping),
            "barGapWidth": chart.bar_gap_width,
            "barOverlap": chart.bar_overlap,
            "bar3DGapDepth": chart.bar_3d_gap_depth,
            "bar3DShape": chart.bar_3d_shape,
            "lineSmooth": chart.line_smooth,
            "lineMarkerVisible": chart.line_marker_visible,
            "lineMarkerSize": chart.line_marker_size,
            "lineMarkerSymbol": chart.line_marker_symbol.map(Self::ooxml_chart_marker_symbol_name),
            "lineMarkerFillColor": chart.line_marker_fill_color,
            "lineMarkerFillColorHex": chart.line_marker_fill_color.map(|rgb| format!("#{rgb:06X}")),
            "lineMarkerLineColor": chart.line_marker_line_color,
            "lineMarkerLineColorHex": chart.line_marker_line_color.map(|rgb| format!("#{rgb:06X}")),
            "lineMarkerLineWidth": chart.line_marker_line_width,
            "pieFirstSliceAngle": chart.pie_first_slice_angle,
            "pieExplosion": chart.pie_explosion,
            "hasDoughnutChart": chart.has_doughnut_chart,
            "doughnutHoleSize": chart.doughnut_hole_size,
            "pieOfPieType": chart.pie_of_pie_type.map(Self::ooxml_chart_of_pie_type_name),
            "pieOfPieGapWidth": chart.pie_of_pie_gap_width,
            "pieOfPieSecondSize": chart.pie_of_pie_second_size,
            "pieOfPieSerLineColor": chart.pie_of_pie_ser_line_color,
            "pieOfPieSerLineWidth": chart.pie_of_pie_ser_line_width,
            "scatterStyle": chart.scatter_style.map(Self::ooxml_chart_scatter_style_name),
            "scatterSmooth": chart.scatter_smooth,
            "scatterMarkerSize": chart.scatter_marker_size,
            "scatterMarkerSymbol": chart.scatter_marker_symbol.map(Self::ooxml_chart_marker_symbol_name),
            "scatterMarkerFillColor": chart.scatter_marker_fill_color,
            "scatterMarkerFillColorHex": chart.scatter_marker_fill_color.map(|rgb| format!("#{rgb:06X}")),
            "scatterMarkerLineColor": chart.scatter_marker_line_color,
            "scatterMarkerLineColorHex": chart.scatter_marker_line_color.map(|rgb| format!("#{rgb:06X}")),
            "scatterMarkerLineWidth": chart.scatter_marker_line_width,
            "trendlineType": chart.trendline_type.map(Self::ooxml_chart_trendline_type_name),
            "trendlineOrder": chart.trendline_order,
            "trendlinePeriod": chart.trendline_period,
            "trendlineDisplayEquation": chart.trendline_display_equation,
            "trendlineDisplayRSquared": chart.trendline_display_r_squared,
            "trendlineLineColor": chart.trendline_line_color,
            "trendlineLineColorHex": chart.trendline_line_color.map(|rgb| format!("#{rgb:06X}")),
            "trendlineLineWidth": chart.trendline_line_width,
            "errorBarDirection": chart.error_bar_direction.map(Self::ooxml_chart_error_bar_direction_name),
            "errorBarType": chart.error_bar_type.map(Self::ooxml_chart_error_bar_type_name),
            "errorBarValueType": chart.error_bar_value_type.map(Self::ooxml_chart_error_bar_value_type_name),
            "errorBarValue": chart.error_bar_value,
            "errorBarNoEndCap": chart.error_bar_no_end_cap,
            "errorBarLineColor": chart.error_bar_line_color,
            "errorBarLineColorHex": chart.error_bar_line_color.map(|rgb| format!("#{rgb:06X}")),
            "errorBarLineWidth": chart.error_bar_line_width,
            "stockUpDownBarGapWidth": chart.stock_up_down_bar_gap_width,
            "stockUpBarFillColor": chart.stock_up_bar_fill_color,
            "stockDownBarFillColor": chart.stock_down_bar_fill_color,
            "stockUpBarLineColor": chart.stock_up_bar_line_color,
            "stockDownBarLineColor": chart.stock_down_bar_line_color,
            "stockUpBarLineWidth": chart.stock_up_bar_line_width,
            "stockDownBarLineWidth": chart.stock_down_bar_line_width,
            "stockHiLowLineColor": chart.stock_hi_low_line_color,
            "stockHiLowLineWidth": chart.stock_hi_low_line_width,
            "dataLabelPosition": chart.data_label_position.map(Self::ooxml_chart_data_label_position_name),
            "dataLabelsShowValue": chart.data_labels_show_value,
            "dataLabelsShowCategoryName": chart.data_labels_show_category_name,
            "dataLabelsShowSeriesName": chart.data_labels_show_series_name,
            "dataLabelsShowPercent": chart.data_labels_show_percent,
            "dataLabelsShowLegendKey": chart.data_labels_show_legend_key,
            "titleOverlay": chart.title_overlay,
            "date1904": chart.date_1904,
            "chartStyle": chart.chart_style,
            "chartAreaFillColor": chart.chart_area_fill_color,
            "chartAreaFillColorHex": chart.chart_area_fill_color.map(|rgb| format!("#{rgb:06X}")),
            "plotAreaFillColor": chart.plot_area_fill_color,
            "plotAreaFillColorHex": chart.plot_area_fill_color.map(|rgb| format!("#{rgb:06X}")),
            "roundedCorners": chart.rounded_corners,
            "autoTitleDeleted": chart.auto_title_deleted,
            "varyColors": chart.vary_colors,
            "view3DRotationX": chart.view_3d_rotation_x,
            "view3DRotationY": chart.view_3d_rotation_y,
            "view3DPerspective": chart.view_3d_perspective,
            "view3DRightAngleAxes": chart.view_3d_right_angle_axes,
            "view3DHeightPercent": chart.view_3d_height_percent,
            "view3DDepthPercent": chart.view_3d_depth_percent,
            "displayBlanksAs": chart.display_blanks_as.map(Self::ooxml_chart_display_blanks_as_name),
            "showHiddenData": chart.show_hidden_data,
            "plotVisibleOnly": chart.plot_visible_only,
            "dataTableShowHorizontalBorder": chart.data_table_show_horizontal_border,
            "dataTableShowVerticalBorder": chart.data_table_show_vertical_border,
            "dataTableShowOutline": chart.data_table_show_outline,
            "dataTableShowKeys": chart.data_table_show_keys,
            "legendPosition": chart.legend_position.map(Self::ooxml_chart_legend_position_name),
            "legendOverlay": chart.legend_overlay,
            "categoryAxisVisible": chart.category_axis_visible,
            "valueAxisVisible": chart.value_axis_visible,
            "categoryAxisTitle": &chart.category_axis_title,
            "valueAxisTitle": &chart.value_axis_title,
            "categoryAxisPosition": chart.category_axis_position.map(Self::ooxml_chart_axis_position_name),
            "valueAxisPosition": chart.value_axis_position.map(Self::ooxml_chart_axis_position_name),
            "categoryAxisLabelPosition": chart.category_axis_label_position.map(Self::ooxml_chart_axis_label_position_name),
            "valueAxisLabelPosition": chart.value_axis_label_position.map(Self::ooxml_chart_axis_label_position_name),
            "categoryAxisAuto": chart.category_axis_auto,
            "categoryAxisLabelAlignment": chart.category_axis_label_alignment.map(Self::ooxml_chart_axis_label_alignment_name),
            "categoryAxisLabelOffset": chart.category_axis_label_offset,
            "categoryAxisTickMarkSkip": chart.category_axis_tick_mark_skip,
            "categoryAxisNoMultiLevelLabels": chart.category_axis_no_multi_level_labels,
            "categoryAxisOrientation": chart.category_axis_orientation.map(Self::ooxml_chart_axis_orientation_name),
            "valueAxisOrientation": chart.value_axis_orientation.map(Self::ooxml_chart_axis_orientation_name),
            "categoryAxisCrosses": chart.category_axis_crosses.map(Self::ooxml_chart_axis_crosses_name),
            "categoryAxisCrossesAt": chart.category_axis_crosses_at,
            "valueAxisCrosses": chart.value_axis_crosses.map(Self::ooxml_chart_axis_crosses_name),
            "valueAxisCrossesAt": chart.value_axis_crosses_at,
            "valueAxisCrossBetween": chart.value_axis_cross_between.map(Self::ooxml_chart_axis_cross_between_name),
            "categoryAxisMajorTickMark": chart.category_axis_major_tick_mark.map(Self::ooxml_chart_axis_tick_mark_name),
            "categoryAxisMinorTickMark": chart.category_axis_minor_tick_mark.map(Self::ooxml_chart_axis_tick_mark_name),
            "categoryAxisLineColor": chart.category_axis_line_color,
            "categoryAxisLineWidth": chart.category_axis_line_width,
            "categoryAxisMajorGridLineColor": chart.category_axis_major_grid_line_color,
            "categoryAxisMajorGridLineWidth": chart.category_axis_major_grid_line_width,
            "categoryAxisMinorGridLineColor": chart.category_axis_minor_grid_line_color,
            "categoryAxisMinorGridLineWidth": chart.category_axis_minor_grid_line_width,
            "valueAxisMajorTickMark": chart.value_axis_major_tick_mark.map(Self::ooxml_chart_axis_tick_mark_name),
            "valueAxisMinorTickMark": chart.value_axis_minor_tick_mark.map(Self::ooxml_chart_axis_tick_mark_name),
            "valueAxisLineColor": chart.value_axis_line_color,
            "valueAxisLineWidth": chart.value_axis_line_width,
            "valueAxisMajorGridLineColor": chart.value_axis_major_grid_line_color,
            "valueAxisMajorGridLineWidth": chart.value_axis_major_grid_line_width,
            "valueAxisMinorGridLineColor": chart.value_axis_minor_grid_line_color,
            "valueAxisMinorGridLineWidth": chart.value_axis_minor_grid_line_width,
            "valueAxisLogBase": chart.value_axis_log_base,
            "valueAxisDisplayUnit": chart.value_axis_display_unit.map(Self::ooxml_chart_axis_display_unit_name),
            "valueAxisMinimum": chart.value_axis_minimum,
            "valueAxisMaximum": chart.value_axis_maximum,
            "valueAxisMajorUnit": chart.value_axis_major_unit,
            "valueAxisMinorUnit": chart.value_axis_minor_unit,
            "categoryAxisNumberFormat": chart.category_axis_number_format,
            "categoryAxisNumberFormatSourceLinked": chart.category_axis_number_format_source_linked,
            "valueAxisNumberFormat": chart.value_axis_number_format,
            "valueAxisNumberFormatSourceLinked": chart.value_axis_number_format_source_linked,
        }))
        .map_err(|e| HwpError::RenderError(format!("chart JSON 직렬화 실패: {e}")))
    }

    fn legacy_ole_chart_json(
        contents: &LegacyOleChartContents,
        chart: &crate::ole_chart::OleChart,
    ) -> Result<String, HwpError> {
        let series: Vec<serde_json::Value> = chart
            .series
            .iter()
            .enumerate()
            .map(|(index, series)| {
                serde_json::json!({
                    "index": index,
                    "name": series.name,
                    "values": series.values,
                })
            })
            .collect();
        let ir_base64 = crate::ole_chart::ole_chart_ir_base64(chart)
            .map_err(|e| HwpError::RenderError(format!("legacy OLE chart IR 직렬화 실패: {e}")))?;
        serde_json::to_string(&serde_json::json!({
            "sourceKind": "LegacyOleChartContents",
            "binDataId": contents.bin_data_id,
            "editable": false,
            "semanticSupported": true,
            "semanticSource": "legacy_ole_contents",
            "rawEditable": false,
            "chartType": Self::legacy_ole_chart_type_name(chart.chart_type),
            "supportedChartTypeValues": [],
            "chartTypeSupportNote": "Legacy HWP OLE Contents charts are decoded to a read-only renderer-neutral IR. Field-level editing is not supported yet.",
            "title": chart.title,
            "categories": chart.categories,
            "categoryCount": chart.categories.len(),
            "series": series,
            "seriesCount": chart.series.len(),
            "rawOleContentsLength": contents.raw_contents.len(),
            "rawOleContentsHash": format!("blake3:{}", blake3::hash(&contents.raw_contents).to_hex()),
            "oleChartIrSchema": crate::ole_chart::OLE_CHART_IR_SCHEMA,
            "oleChartIrVersion": crate::ole_chart::OLE_CHART_IR_VERSION,
            "oleChartIrBase64": ir_base64,
            "note": "This payload came from an OLE Contents stream without OOXMLChartContents or OlePres preview. rhwp can parse and render the chart IR, but rhwp_set_chart_data intentionally rejects edits for this legacy format until field-level write support exists."
        }))
        .map_err(|e| HwpError::RenderError(format!("legacy OLE chart JSON 직렬화 실패: {e}")))
    }

    fn legacy_ole_chart_type_name(chart_type: crate::ole_chart::OleChartType) -> &'static str {
        match chart_type {
            crate::ole_chart::OleChartType::Column => "Column",
            crate::ole_chart::OleChartType::Bar => "Bar",
            crate::ole_chart::OleChartType::Line => "Line",
            crate::ole_chart::OleChartType::Pie => "Pie",
            crate::ole_chart::OleChartType::Unknown => "Unknown",
        }
    }

    fn hwp_chart_data_json(chart: &ChartShape, raw_replaced: bool) -> Result<String, HwpError> {
        let raw = &chart.raw_chart_data;
        let semantic_supported = chart_has_semantic(chart);
        let categories = Self::hwp_chart_categories(chart);
        serde_json::to_string(&serde_json::json!({
            "sourceKind": "HwpChartData",
            "editable": true,
            "semanticSupported": semantic_supported,
            "semanticSource": if semantic_supported { "rhwp_chart_data_json" } else { "none" },
            "rawEditable": true,
            "rawReplaced": raw_replaced,
            "chartType": chart_type_name(chart.chart_type),
            "supportedChartTypeValues": ["Bar", "Column", "Line", "Pie", "Area", "Scatter", "Unknown"],
            "chartTypeSupportNote": if semantic_supported {
                "Rhwp-generated native HWP CHART_DATA JSON payloads support these semantic chartType values. Hancom-authored native CHART_DATA field decoding remains limited."
            } else {
                "Native HWP CHART_DATA raw payloads can be preserved or replaced, but Hancom-authored semantic chartType fields are not decoded yet."
            },
            "title": chart.title,
            "legendPosition": chart.legend.as_ref().map(|legend| legend_position_name(legend.position)),
            "legendVisible": chart.legend.as_ref().map(|legend| legend.visible),
            "categoryCount": categories.len(),
            "categories": categories,
            "seriesCount": chart.series.len(),
            "series": chart.series.iter().map(|series| serde_json::json!({
                "name": &series.name,
                "values": &series.values,
                "categories": &series.categories,
                "color": series.color,
                "colorHex": series.color.map(|rgb| format!("#{rgb:06X}")),
            })).collect::<Vec<_>>(),
            "xAxis": chart.x_axis.as_ref().map(Self::hwp_chart_axis_json),
            "yAxis": chart.y_axis.as_ref().map(Self::hwp_chart_axis_json),
            "rawHwpChartDataLength": raw.len(),
            "rawHwpChartDataHash": format!("blake3:{}", blake3::hash(raw).to_hex()),
            "rawHwpChartDataBase64": base64::engine::general_purpose::STANDARD.encode(raw),
            "note": if semantic_supported {
                "Native HWP HWPTAG_CHART_DATA semantic fields were restored from an rhwp-generated CHART_DATA JSON payload. Hancom-authored native CHART_DATA field decoding remains limited."
            } else {
                "Native HWP HWPTAG_CHART_DATA semantic fields are not decoded yet for this payload. rawHwpChartDataBase64 can be used for explicit raw payload preservation or replacement."
            }
        }))
        .map_err(|e| HwpError::RenderError(format!("chart JSON 직렬화 실패: {e}")))
    }

    fn hwp_chart_categories(chart: &ChartShape) -> Vec<String> {
        chart
            .series
            .iter()
            .find_map(|series| {
                if series.categories.is_empty() {
                    None
                } else {
                    Some(series.categories.clone())
                }
            })
            .unwrap_or_else(|| {
                chart
                    .x_axis
                    .as_ref()
                    .map(|axis| axis.labels.clone())
                    .unwrap_or_default()
            })
    }

    fn hwp_chart_axis_json(axis: &Axis) -> serde_json::Value {
        serde_json::json!({
            "label": &axis.label,
            "labels": &axis.labels,
            "min": axis.min,
            "max": axis.max,
        })
    }

    fn ooxml_chart_type_name(chart_type: crate::ooxml_chart::OoxmlChartType) -> &'static str {
        match chart_type {
            crate::ooxml_chart::OoxmlChartType::Column => "Column",
            crate::ooxml_chart::OoxmlChartType::Bar => "Bar",
            crate::ooxml_chart::OoxmlChartType::Line => "Line",
            crate::ooxml_chart::OoxmlChartType::Pie => "Pie",
            crate::ooxml_chart::OoxmlChartType::Scatter => "Scatter",
            crate::ooxml_chart::OoxmlChartType::Stock => "Stock",
            crate::ooxml_chart::OoxmlChartType::Unknown => "Unknown",
        }
    }

    fn ooxml_chart_grouping_name(grouping: crate::ooxml_chart::BarGrouping) -> &'static str {
        match grouping {
            crate::ooxml_chart::BarGrouping::Clustered => "Clustered",
            crate::ooxml_chart::BarGrouping::Stacked => "Stacked",
            crate::ooxml_chart::BarGrouping::PercentStacked => "PercentStacked",
        }
    }

    fn ooxml_chart_legend_position_name(
        position: crate::ooxml_chart::ChartLegendPosition,
    ) -> &'static str {
        match position {
            crate::ooxml_chart::ChartLegendPosition::Right => "Right",
            crate::ooxml_chart::ChartLegendPosition::Left => "Left",
            crate::ooxml_chart::ChartLegendPosition::Top => "Top",
            crate::ooxml_chart::ChartLegendPosition::Bottom => "Bottom",
            crate::ooxml_chart::ChartLegendPosition::TopRight => "TopRight",
        }
    }

    fn ooxml_chart_axis_label_position_name(
        position: crate::ooxml_chart::AxisLabelPosition,
    ) -> &'static str {
        match position {
            crate::ooxml_chart::AxisLabelPosition::NextTo => "NextTo",
            crate::ooxml_chart::AxisLabelPosition::High => "High",
            crate::ooxml_chart::AxisLabelPosition::Low => "Low",
            crate::ooxml_chart::AxisLabelPosition::None => "None",
        }
    }

    fn ooxml_chart_axis_position_name(position: crate::ooxml_chart::AxisPosition) -> &'static str {
        match position {
            crate::ooxml_chart::AxisPosition::Bottom => "Bottom",
            crate::ooxml_chart::AxisPosition::Left => "Left",
            crate::ooxml_chart::AxisPosition::Top => "Top",
            crate::ooxml_chart::AxisPosition::Right => "Right",
        }
    }

    fn ooxml_chart_axis_label_alignment_name(
        value: crate::ooxml_chart::AxisLabelAlignment,
    ) -> &'static str {
        match value {
            crate::ooxml_chart::AxisLabelAlignment::Center => "Center",
            crate::ooxml_chart::AxisLabelAlignment::Left => "Left",
            crate::ooxml_chart::AxisLabelAlignment::Right => "Right",
        }
    }

    fn ooxml_chart_axis_orientation_name(
        value: crate::ooxml_chart::AxisOrientation,
    ) -> &'static str {
        match value {
            crate::ooxml_chart::AxisOrientation::MinMax => "MinMax",
            crate::ooxml_chart::AxisOrientation::MaxMin => "MaxMin",
        }
    }

    fn ooxml_chart_axis_cross_between_name(
        value: crate::ooxml_chart::AxisCrossBetween,
    ) -> &'static str {
        match value {
            crate::ooxml_chart::AxisCrossBetween::Between => "Between",
            crate::ooxml_chart::AxisCrossBetween::MidCategory => "MidCategory",
        }
    }

    fn ooxml_chart_axis_crosses_name(value: crate::ooxml_chart::AxisCrosses) -> &'static str {
        match value {
            crate::ooxml_chart::AxisCrosses::AutoZero => "AutoZero",
            crate::ooxml_chart::AxisCrosses::Min => "Min",
            crate::ooxml_chart::AxisCrosses::Max => "Max",
        }
    }

    fn ooxml_chart_axis_tick_mark_name(mark: crate::ooxml_chart::AxisTickMark) -> &'static str {
        match mark {
            crate::ooxml_chart::AxisTickMark::Cross => "Cross",
            crate::ooxml_chart::AxisTickMark::In => "In",
            crate::ooxml_chart::AxisTickMark::Out => "Out",
            crate::ooxml_chart::AxisTickMark::None => "None",
        }
    }

    fn ooxml_chart_axis_display_unit_name(
        unit: crate::ooxml_chart::AxisDisplayUnit,
    ) -> &'static str {
        match unit {
            crate::ooxml_chart::AxisDisplayUnit::Hundreds => "Hundreds",
            crate::ooxml_chart::AxisDisplayUnit::Thousands => "Thousands",
            crate::ooxml_chart::AxisDisplayUnit::TenThousands => "TenThousands",
            crate::ooxml_chart::AxisDisplayUnit::HundredThousands => "HundredThousands",
            crate::ooxml_chart::AxisDisplayUnit::Millions => "Millions",
            crate::ooxml_chart::AxisDisplayUnit::TenMillions => "TenMillions",
            crate::ooxml_chart::AxisDisplayUnit::HundredMillions => "HundredMillions",
            crate::ooxml_chart::AxisDisplayUnit::Billions => "Billions",
            crate::ooxml_chart::AxisDisplayUnit::Trillions => "Trillions",
        }
    }

    fn ooxml_chart_data_label_position_name(
        position: crate::ooxml_chart::ChartDataLabelPosition,
    ) -> &'static str {
        match position {
            crate::ooxml_chart::ChartDataLabelPosition::BestFit => "BestFit",
            crate::ooxml_chart::ChartDataLabelPosition::Bottom => "Bottom",
            crate::ooxml_chart::ChartDataLabelPosition::Center => "Center",
            crate::ooxml_chart::ChartDataLabelPosition::InsideBase => "InsideBase",
            crate::ooxml_chart::ChartDataLabelPosition::InsideEnd => "InsideEnd",
            crate::ooxml_chart::ChartDataLabelPosition::Left => "Left",
            crate::ooxml_chart::ChartDataLabelPosition::OutsideEnd => "OutsideEnd",
            crate::ooxml_chart::ChartDataLabelPosition::Right => "Right",
            crate::ooxml_chart::ChartDataLabelPosition::Top => "Top",
        }
    }

    fn ooxml_chart_display_blanks_as_name(
        value: crate::ooxml_chart::ChartDisplayBlanksAs,
    ) -> &'static str {
        match value {
            crate::ooxml_chart::ChartDisplayBlanksAs::Gap => "Gap",
            crate::ooxml_chart::ChartDisplayBlanksAs::Span => "Span",
            crate::ooxml_chart::ChartDisplayBlanksAs::Zero => "Zero",
        }
    }

    fn ooxml_chart_scatter_style_name(style: crate::ooxml_chart::ScatterStyle) -> &'static str {
        match style {
            crate::ooxml_chart::ScatterStyle::Line => "Line",
            crate::ooxml_chart::ScatterStyle::LineMarker => "LineMarker",
            crate::ooxml_chart::ScatterStyle::Marker => "Marker",
            crate::ooxml_chart::ScatterStyle::Smooth => "Smooth",
            crate::ooxml_chart::ScatterStyle::SmoothMarker => "SmoothMarker",
        }
    }

    fn ooxml_chart_marker_symbol_name(
        symbol: crate::ooxml_chart::ChartMarkerSymbol,
    ) -> &'static str {
        match symbol {
            crate::ooxml_chart::ChartMarkerSymbol::Circle => "Circle",
            crate::ooxml_chart::ChartMarkerSymbol::Dash => "Dash",
            crate::ooxml_chart::ChartMarkerSymbol::Diamond => "Diamond",
            crate::ooxml_chart::ChartMarkerSymbol::Dot => "Dot",
            crate::ooxml_chart::ChartMarkerSymbol::None => "None",
            crate::ooxml_chart::ChartMarkerSymbol::Picture => "Picture",
            crate::ooxml_chart::ChartMarkerSymbol::Plus => "Plus",
            crate::ooxml_chart::ChartMarkerSymbol::Square => "Square",
            crate::ooxml_chart::ChartMarkerSymbol::Star => "Star",
            crate::ooxml_chart::ChartMarkerSymbol::Triangle => "Triangle",
            crate::ooxml_chart::ChartMarkerSymbol::X => "X",
        }
    }

    fn ooxml_chart_trendline_type_name(
        kind: crate::ooxml_chart::ChartTrendlineType,
    ) -> &'static str {
        match kind {
            crate::ooxml_chart::ChartTrendlineType::Linear => "Linear",
            crate::ooxml_chart::ChartTrendlineType::Exponential => "Exponential",
            crate::ooxml_chart::ChartTrendlineType::Logarithmic => "Logarithmic",
            crate::ooxml_chart::ChartTrendlineType::MovingAverage => "MovingAverage",
            crate::ooxml_chart::ChartTrendlineType::Polynomial => "Polynomial",
            crate::ooxml_chart::ChartTrendlineType::Power => "Power",
        }
    }

    fn ooxml_chart_error_bar_direction_name(
        direction: crate::ooxml_chart::ChartErrorBarDirection,
    ) -> &'static str {
        match direction {
            crate::ooxml_chart::ChartErrorBarDirection::X => "X",
            crate::ooxml_chart::ChartErrorBarDirection::Y => "Y",
        }
    }

    fn ooxml_chart_error_bar_type_name(
        kind: crate::ooxml_chart::ChartErrorBarType,
    ) -> &'static str {
        match kind {
            crate::ooxml_chart::ChartErrorBarType::Both => "Both",
            crate::ooxml_chart::ChartErrorBarType::Plus => "Plus",
            crate::ooxml_chart::ChartErrorBarType::Minus => "Minus",
        }
    }

    fn ooxml_chart_error_bar_value_type_name(
        kind: crate::ooxml_chart::ChartErrorBarValueType,
    ) -> &'static str {
        match kind {
            crate::ooxml_chart::ChartErrorBarValueType::FixedValue => "FixedValue",
            crate::ooxml_chart::ChartErrorBarValueType::Percentage => "Percentage",
            crate::ooxml_chart::ChartErrorBarValueType::StandardDeviation => "StandardDeviation",
            crate::ooxml_chart::ChartErrorBarValueType::StandardError => "StandardError",
        }
    }

    fn ooxml_chart_of_pie_type_name(kind: crate::ooxml_chart::OfPieType) -> &'static str {
        match kind {
            crate::ooxml_chart::OfPieType::Pie => "Pie",
            crate::ooxml_chart::OfPieType::Bar => "Bar",
        }
    }

    fn parse_chart_xml_update(
        props_json: &str,
    ) -> Result<crate::ooxml_chart::edit::ChartXmlUpdate, HwpError> {
        let value: serde_json::Value = serde_json::from_str(props_json)
            .map_err(|e| HwpError::RenderError(format!("chart props JSON 파싱 실패: {e}")))?;
        let title = value
            .get("title")
            .map(|raw| {
                raw.as_str()
                    .map(|s| s.to_string())
                    .ok_or_else(|| HwpError::RenderError("title은 문자열이어야 합니다".to_string()))
            })
            .transpose()?;
        let chart_type = value
            .get("chartType")
            .or_else(|| value.get("chart_type"))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("chartType은 문자열이어야 합니다".to_string())
                })?;
                match raw {
                    "Column" | "column" | "col" => Ok(crate::ooxml_chart::OoxmlChartType::Column),
                    "Bar" | "bar" => Ok(crate::ooxml_chart::OoxmlChartType::Bar),
                    _ => Err(HwpError::RenderError(
                        "chartType은 현재 Column 또는 Bar만 지원합니다".to_string(),
                    )),
                }
            })
            .transpose()?;
        let grouping = value
            .get("grouping")
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("grouping은 문자열이어야 합니다".to_string())
                })?;
                match raw {
                    "Clustered" | "clustered" => Ok(crate::ooxml_chart::BarGrouping::Clustered),
                    "Stacked" | "stacked" => Ok(crate::ooxml_chart::BarGrouping::Stacked),
                    "PercentStacked" | "percentStacked" | "percent_stacked" => {
                        Ok(crate::ooxml_chart::BarGrouping::PercentStacked)
                    }
                    _ => Err(HwpError::RenderError(
                        "grouping은 Clustered, Stacked, PercentStacked 중 하나여야 합니다"
                            .to_string(),
                    )),
                }
            })
            .transpose()?;
        let bar_gap_width = value
            .get("barGapWidth")
            .or_else(|| value.get("bar_gap_width"))
            .map(|raw| {
                let value = raw.as_u64().ok_or_else(|| {
                    HwpError::RenderError("barGapWidth는 0 이상 정수여야 합니다".to_string())
                })?;
                u32::try_from(value).map_err(|_| {
                    HwpError::RenderError("barGapWidth는 u32 범위 정수여야 합니다".to_string())
                })
            })
            .transpose()?;
        let bar_overlap = value
            .get("barOverlap")
            .or_else(|| value.get("bar_overlap"))
            .map(|raw| {
                let value = raw.as_i64().ok_or_else(|| {
                    HwpError::RenderError("barOverlap은 정수여야 합니다".to_string())
                })?;
                i32::try_from(value).map_err(|_| {
                    HwpError::RenderError("barOverlap은 i32 범위 정수여야 합니다".to_string())
                })
            })
            .transpose()?;
        let bar_3d_gap_depth =
            Self::parse_optional_chart_u32(&value, "bar3DGapDepth", "bar_3d_gap_depth")?;
        let bar_3d_shape = Self::parse_optional_chart_bar_3d_shape(&value)?;
        let line_smooth = value
            .get("lineSmooth")
            .or_else(|| value.get("line_smooth"))
            .map(|raw| {
                raw.as_bool().ok_or_else(|| {
                    HwpError::RenderError("lineSmooth는 boolean이어야 합니다".to_string())
                })
            })
            .transpose()?;
        let line_marker_visible =
            Self::parse_optional_chart_bool(&value, "lineMarkerVisible", "line_marker_visible")?;
        let line_marker_size = value
            .get("lineMarkerSize")
            .or_else(|| value.get("line_marker_size"))
            .map(|raw| {
                let value = raw.as_u64().ok_or_else(|| {
                    HwpError::RenderError("lineMarkerSize는 0 이상 정수여야 합니다".to_string())
                })?;
                u32::try_from(value).map_err(|_| {
                    HwpError::RenderError("lineMarkerSize는 u32 범위 정수여야 합니다".to_string())
                })
            })
            .transpose()?;
        let line_marker_symbol = Self::parse_optional_chart_marker_symbol(
            &value,
            "lineMarkerSymbol",
            "line_marker_symbol",
        )?;
        let line_marker_fill_color = Self::parse_optional_chart_rgb_with_hex_alias(
            &value,
            "lineMarkerFillColor",
            "line_marker_fill_color",
            "lineMarkerFillColorHex",
            "line_marker_fill_color_hex",
        )?;
        let line_marker_line_color = Self::parse_optional_chart_rgb_with_hex_alias(
            &value,
            "lineMarkerLineColor",
            "line_marker_line_color",
            "lineMarkerLineColorHex",
            "line_marker_line_color_hex",
        )?;
        let line_marker_line_width = Self::parse_optional_chart_u32(
            &value,
            "lineMarkerLineWidth",
            "line_marker_line_width",
        )?;
        let pie_first_slice_angle = value
            .get("pieFirstSliceAngle")
            .or_else(|| value.get("pie_first_slice_angle"))
            .map(|raw| {
                let value = raw.as_u64().ok_or_else(|| {
                    HwpError::RenderError("pieFirstSliceAngle은 0 이상 정수여야 합니다".to_string())
                })?;
                u16::try_from(value).map_err(|_| {
                    HwpError::RenderError(
                        "pieFirstSliceAngle은 u16 범위 정수여야 합니다".to_string(),
                    )
                })
            })
            .transpose()?;
        let pie_explosion = value
            .get("pieExplosion")
            .or_else(|| value.get("pie_explosion"))
            .map(|raw| {
                let value = raw.as_u64().ok_or_else(|| {
                    HwpError::RenderError("pieExplosion은 0 이상 정수여야 합니다".to_string())
                })?;
                u32::try_from(value).map_err(|_| {
                    HwpError::RenderError("pieExplosion은 u32 범위 정수여야 합니다".to_string())
                })
            })
            .transpose()?;
        let doughnut_hole_size =
            Self::parse_optional_chart_u32(&value, "doughnutHoleSize", "doughnut_hole_size")?;
        let pie_of_pie_type = value
            .get("pieOfPieType")
            .or_else(|| value.get("pie_of_pie_type"))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("pieOfPieType은 문자열이어야 합니다".to_string())
                })?;
                match raw {
                    "Pie" | "pie" => Ok(crate::ooxml_chart::OfPieType::Pie),
                    "Bar" | "bar" => Ok(crate::ooxml_chart::OfPieType::Bar),
                    _ => Err(HwpError::RenderError(
                        "pieOfPieType은 Pie 또는 Bar 중 하나여야 합니다".to_string(),
                    )),
                }
            })
            .transpose()?;
        let pie_of_pie_gap_width =
            Self::parse_optional_chart_u32(&value, "pieOfPieGapWidth", "pie_of_pie_gap_width")?;
        let pie_of_pie_second_size =
            Self::parse_optional_chart_u32(&value, "pieOfPieSecondSize", "pie_of_pie_second_size")?;
        let pie_of_pie_ser_line_color = Self::parse_optional_chart_rgb(
            &value,
            "pieOfPieSerLineColor",
            "pie_of_pie_ser_line_color",
        )?;
        let pie_of_pie_ser_line_width = Self::parse_optional_chart_u32(
            &value,
            "pieOfPieSerLineWidth",
            "pie_of_pie_ser_line_width",
        )?;
        let scatter_style = value
            .get("scatterStyle")
            .or_else(|| value.get("scatter_style"))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("scatterStyle은 문자열이어야 합니다".to_string())
                })?;
                match raw {
                    "Line" | "line" => Ok(crate::ooxml_chart::ScatterStyle::Line),
                    "LineMarker" | "lineMarker" | "line_marker" => {
                        Ok(crate::ooxml_chart::ScatterStyle::LineMarker)
                    }
                    "Marker" | "marker" => Ok(crate::ooxml_chart::ScatterStyle::Marker),
                    "Smooth" | "smooth" => Ok(crate::ooxml_chart::ScatterStyle::Smooth),
                    "SmoothMarker" | "smoothMarker" | "smooth_marker" => {
                        Ok(crate::ooxml_chart::ScatterStyle::SmoothMarker)
                    }
                    _ => Err(HwpError::RenderError(
                        "scatterStyle은 Line, LineMarker, Marker, Smooth, SmoothMarker 중 하나여야 합니다"
                            .to_string(),
                    )),
                }
            })
            .transpose()?;
        let scatter_smooth = value
            .get("scatterSmooth")
            .or_else(|| value.get("scatter_smooth"))
            .map(|raw| {
                raw.as_bool().ok_or_else(|| {
                    HwpError::RenderError("scatterSmooth는 boolean이어야 합니다".to_string())
                })
            })
            .transpose()?;
        let scatter_marker_size = value
            .get("scatterMarkerSize")
            .or_else(|| value.get("scatter_marker_size"))
            .map(|raw| {
                let value = raw.as_u64().ok_or_else(|| {
                    HwpError::RenderError("scatterMarkerSize는 0 이상 정수여야 합니다".to_string())
                })?;
                u32::try_from(value).map_err(|_| {
                    HwpError::RenderError(
                        "scatterMarkerSize는 u32 범위 정수여야 합니다".to_string(),
                    )
                })
            })
            .transpose()?;
        let scatter_marker_symbol = Self::parse_optional_chart_marker_symbol(
            &value,
            "scatterMarkerSymbol",
            "scatter_marker_symbol",
        )?;
        let scatter_marker_fill_color = Self::parse_optional_chart_rgb_with_hex_alias(
            &value,
            "scatterMarkerFillColor",
            "scatter_marker_fill_color",
            "scatterMarkerFillColorHex",
            "scatter_marker_fill_color_hex",
        )?;
        let scatter_marker_line_color = Self::parse_optional_chart_rgb_with_hex_alias(
            &value,
            "scatterMarkerLineColor",
            "scatter_marker_line_color",
            "scatterMarkerLineColorHex",
            "scatter_marker_line_color_hex",
        )?;
        let scatter_marker_line_width = Self::parse_optional_chart_u32(
            &value,
            "scatterMarkerLineWidth",
            "scatter_marker_line_width",
        )?;
        let trendline_type = value
            .get("trendlineType")
            .or_else(|| value.get("trendline_type"))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("trendlineType은 문자열이어야 합니다".to_string())
                })?;
                match raw {
                    "Linear" | "linear" => Ok(crate::ooxml_chart::ChartTrendlineType::Linear),
                    "Exponential" | "exponential" | "exp" => {
                        Ok(crate::ooxml_chart::ChartTrendlineType::Exponential)
                    }
                    "Logarithmic" | "logarithmic" | "log" => {
                        Ok(crate::ooxml_chart::ChartTrendlineType::Logarithmic)
                    }
                    "MovingAverage" | "movingAverage" | "moving_average" | "movingAvg" => {
                        Ok(crate::ooxml_chart::ChartTrendlineType::MovingAverage)
                    }
                    "Polynomial" | "polynomial" | "poly" => {
                        Ok(crate::ooxml_chart::ChartTrendlineType::Polynomial)
                    }
                    "Power" | "power" => Ok(crate::ooxml_chart::ChartTrendlineType::Power),
                    _ => Err(HwpError::RenderError(
                        "trendlineType은 Linear, Exponential, Logarithmic, MovingAverage, Polynomial, Power 중 하나여야 합니다"
                            .to_string(),
                    )),
                }
            })
            .transpose()?;
        let trendline_display_equation = Self::parse_optional_chart_bool(
            &value,
            "trendlineDisplayEquation",
            "trendline_display_equation",
        )?;
        let trendline_order =
            Self::parse_optional_chart_u32(&value, "trendlineOrder", "trendline_order")?;
        if let Some(order) = trendline_order {
            if !(2..=6).contains(&order) {
                return Err(HwpError::RenderError(
                    "trendlineOrder는 2 이상 6 이하 정수여야 합니다".to_string(),
                ));
            }
        }
        let trendline_period =
            Self::parse_optional_chart_u32(&value, "trendlinePeriod", "trendline_period")?;
        if let Some(period) = trendline_period {
            if !(2..=255).contains(&period) {
                return Err(HwpError::RenderError(
                    "trendlinePeriod는 2 이상 255 이하 정수여야 합니다".to_string(),
                ));
            }
        }
        let trendline_display_r_squared = Self::parse_optional_chart_bool(
            &value,
            "trendlineDisplayRSquared",
            "trendline_display_r_squared",
        )?;
        let trendline_line_color = value
            .get("trendlineLineColor")
            .or_else(|| value.get("trendlineLineColorHex"))
            .or_else(|| value.get("trendline_line_color"))
            .or_else(|| value.get("trendline_line_color_hex"))
            .map(|raw| Self::parse_chart_rgb_value(raw, "trendlineLineColor"))
            .transpose()?;
        let trendline_line_width =
            Self::parse_optional_chart_u32(&value, "trendlineLineWidth", "trendline_line_width")?;
        if let Some(width) = trendline_line_width {
            if width > 2_000_000 {
                return Err(HwpError::RenderError(
                    "trendlineLineWidth는 0 이상 2000000 이하 정수여야 합니다".to_string(),
                ));
            }
        }
        let error_bar_direction = value
            .get("errorBarDirection")
            .or_else(|| value.get("error_bar_direction"))
            .map(Self::parse_error_bar_direction)
            .transpose()?;
        let error_bar_type = value
            .get("errorBarType")
            .or_else(|| value.get("error_bar_type"))
            .map(Self::parse_error_bar_type)
            .transpose()?;
        let error_bar_value_type = value
            .get("errorBarValueType")
            .or_else(|| value.get("error_bar_value_type"))
            .map(Self::parse_error_bar_value_type)
            .transpose()?;
        let error_bar_value =
            Self::parse_optional_chart_number(&value, "errorBarValue", "error_bar_value")?;
        if let Some(value) = error_bar_value {
            if !value.is_finite() || value < 0.0 {
                return Err(HwpError::RenderError(
                    "errorBarValue는 0 이상의 유한한 숫자여야 합니다".to_string(),
                ));
            }
        }
        let error_bar_no_end_cap =
            Self::parse_optional_chart_bool(&value, "errorBarNoEndCap", "error_bar_no_end_cap")?;
        let error_bar_line_color = value
            .get("errorBarLineColor")
            .or_else(|| value.get("errorBarLineColorHex"))
            .or_else(|| value.get("error_bar_line_color"))
            .or_else(|| value.get("error_bar_line_color_hex"))
            .map(|raw| Self::parse_chart_rgb_value(raw, "errorBarLineColor"))
            .transpose()?;
        let error_bar_line_width =
            Self::parse_optional_chart_u32(&value, "errorBarLineWidth", "error_bar_line_width")?;
        if let Some(width) = error_bar_line_width {
            if width > 2_000_000 {
                return Err(HwpError::RenderError(
                    "errorBarLineWidth는 0 이상 2000000 이하 정수여야 합니다".to_string(),
                ));
            }
        }
        let stock_up_down_bar_gap_width = Self::parse_optional_chart_u32(
            &value,
            "stockUpDownBarGapWidth",
            "stock_up_down_bar_gap_width",
        )?;
        let stock_up_bar_fill_color = Self::parse_optional_chart_rgb(
            &value,
            "stockUpBarFillColor",
            "stock_up_bar_fill_color",
        )?;
        let stock_down_bar_fill_color = Self::parse_optional_chart_rgb(
            &value,
            "stockDownBarFillColor",
            "stock_down_bar_fill_color",
        )?;
        let stock_up_bar_line_color = Self::parse_optional_chart_rgb(
            &value,
            "stockUpBarLineColor",
            "stock_up_bar_line_color",
        )?;
        let stock_down_bar_line_color = Self::parse_optional_chart_rgb(
            &value,
            "stockDownBarLineColor",
            "stock_down_bar_line_color",
        )?;
        let stock_up_bar_line_width = Self::parse_optional_chart_u32(
            &value,
            "stockUpBarLineWidth",
            "stock_up_bar_line_width",
        )?;
        let stock_down_bar_line_width = Self::parse_optional_chart_u32(
            &value,
            "stockDownBarLineWidth",
            "stock_down_bar_line_width",
        )?;
        let stock_hi_low_line_color = Self::parse_optional_chart_rgb(
            &value,
            "stockHiLowLineColor",
            "stock_hi_low_line_color",
        )?;
        let stock_hi_low_line_width = Self::parse_optional_chart_u32(
            &value,
            "stockHiLowLineWidth",
            "stock_hi_low_line_width",
        )?;
        let data_label_position = value
            .get("dataLabelPosition")
            .or_else(|| value.get("data_label_position"))
            .map(Self::parse_data_label_position)
            .transpose()?;
        let data_labels_show_value = Self::parse_optional_chart_bool(
            &value,
            "dataLabelsShowValue",
            "data_labels_show_value",
        )?;
        let data_labels_show_category_name = Self::parse_optional_chart_bool(
            &value,
            "dataLabelsShowCategoryName",
            "data_labels_show_category_name",
        )?;
        let data_labels_show_series_name = Self::parse_optional_chart_bool(
            &value,
            "dataLabelsShowSeriesName",
            "data_labels_show_series_name",
        )?;
        let data_labels_show_percent = Self::parse_optional_chart_bool(
            &value,
            "dataLabelsShowPercent",
            "data_labels_show_percent",
        )?;
        let data_labels_show_legend_key = Self::parse_optional_chart_bool(
            &value,
            "dataLabelsShowLegendKey",
            "data_labels_show_legend_key",
        )?;
        let title_overlay =
            Self::parse_optional_chart_bool(&value, "titleOverlay", "title_overlay")?;
        let date_1904 = Self::parse_optional_chart_bool(&value, "date1904", "date_1904")?;
        let chart_style = Self::parse_optional_chart_u32(&value, "chartStyle", "chart_style")?;
        let chart_area_fill_color =
            Self::parse_optional_chart_rgb(&value, "chartAreaFillColor", "chart_area_fill_color")?;
        let plot_area_fill_color =
            Self::parse_optional_chart_rgb(&value, "plotAreaFillColor", "plot_area_fill_color")?;
        let rounded_corners =
            Self::parse_optional_chart_bool(&value, "roundedCorners", "rounded_corners")?;
        let auto_title_deleted =
            Self::parse_optional_chart_bool(&value, "autoTitleDeleted", "auto_title_deleted")?;
        let vary_colors = Self::parse_optional_chart_bool(&value, "varyColors", "vary_colors")?;
        let view_3d_rotation_x =
            Self::parse_optional_chart_i32(&value, "view3DRotationX", "view_3d_rotation_x")?;
        let view_3d_rotation_y =
            Self::parse_optional_chart_i32(&value, "view3DRotationY", "view_3d_rotation_y")?;
        let view_3d_perspective =
            Self::parse_optional_chart_u32(&value, "view3DPerspective", "view_3d_perspective")?;
        let view_3d_right_angle_axes = Self::parse_optional_chart_bool(
            &value,
            "view3DRightAngleAxes",
            "view_3d_right_angle_axes",
        )?;
        let view_3d_height_percent = Self::parse_optional_chart_u32(
            &value,
            "view3DHeightPercent",
            "view_3d_height_percent",
        )?;
        let view_3d_depth_percent =
            Self::parse_optional_chart_u32(&value, "view3DDepthPercent", "view_3d_depth_percent")?;
        let display_blanks_as = value
            .get("displayBlanksAs")
            .or_else(|| value.get("display_blanks_as"))
            .map(Self::parse_display_blanks_as)
            .transpose()?;
        let show_hidden_data =
            Self::parse_optional_chart_bool(&value, "showHiddenData", "show_hidden_data")?;
        let plot_visible_only =
            Self::parse_optional_chart_bool(&value, "plotVisibleOnly", "plot_visible_only")?;
        let data_table_show_horizontal_border = Self::parse_optional_chart_bool(
            &value,
            "dataTableShowHorizontalBorder",
            "data_table_show_horizontal_border",
        )?;
        let data_table_show_vertical_border = Self::parse_optional_chart_bool(
            &value,
            "dataTableShowVerticalBorder",
            "data_table_show_vertical_border",
        )?;
        let data_table_show_outline = Self::parse_optional_chart_bool(
            &value,
            "dataTableShowOutline",
            "data_table_show_outline",
        )?;
        let data_table_show_keys =
            Self::parse_optional_chart_bool(&value, "dataTableShowKeys", "data_table_show_keys")?;
        let legend_position = value
            .get("legendPosition")
            .or_else(|| value.get("legend_position"))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("legendPosition은 문자열이어야 합니다".to_string())
                })?;
                match raw {
                    "Right" | "right" | "r" => Ok(crate::ooxml_chart::ChartLegendPosition::Right),
                    "Left" | "left" | "l" => Ok(crate::ooxml_chart::ChartLegendPosition::Left),
                    "Top" | "top" | "t" => Ok(crate::ooxml_chart::ChartLegendPosition::Top),
                    "Bottom" | "bottom" | "b" => {
                        Ok(crate::ooxml_chart::ChartLegendPosition::Bottom)
                    }
                    "TopRight" | "topRight" | "top_right" | "tr" => {
                        Ok(crate::ooxml_chart::ChartLegendPosition::TopRight)
                    }
                    _ => Err(HwpError::RenderError(
                        "legendPosition은 Right, Left, Top, Bottom, TopRight 중 하나여야 합니다"
                            .to_string(),
                    )),
                }
            })
            .transpose()?;
        let legend_overlay =
            Self::parse_optional_chart_bool(&value, "legendOverlay", "legend_overlay")?;
        let category_axis_title =
            Self::parse_optional_chart_string(&value, "categoryAxisTitle", "category_axis_title")?;
        let value_axis_title =
            Self::parse_optional_chart_string(&value, "valueAxisTitle", "value_axis_title")?;
        let category_axis_visible = value
            .get("categoryAxisVisible")
            .or_else(|| value.get("category_axis_visible"))
            .map(|raw| {
                raw.as_bool().ok_or_else(|| {
                    HwpError::RenderError("categoryAxisVisible은 boolean이어야 합니다".to_string())
                })
            })
            .transpose()?;
        let value_axis_visible = value
            .get("valueAxisVisible")
            .or_else(|| value.get("value_axis_visible"))
            .map(|raw| {
                raw.as_bool().ok_or_else(|| {
                    HwpError::RenderError("valueAxisVisible은 boolean이어야 합니다".to_string())
                })
            })
            .transpose()?;
        let category_axis_label_position = value
            .get("categoryAxisLabelPosition")
            .or_else(|| value.get("category_axis_label_position"))
            .map(Self::parse_axis_label_position)
            .transpose()?;
        let value_axis_label_position = value
            .get("valueAxisLabelPosition")
            .or_else(|| value.get("value_axis_label_position"))
            .map(Self::parse_axis_label_position)
            .transpose()?;
        let category_axis_position = value
            .get("categoryAxisPosition")
            .or_else(|| value.get("category_axis_position"))
            .map(Self::parse_axis_position)
            .transpose()?;
        let value_axis_position = value
            .get("valueAxisPosition")
            .or_else(|| value.get("value_axis_position"))
            .map(Self::parse_axis_position)
            .transpose()?;
        let category_axis_auto =
            Self::parse_optional_chart_bool(&value, "categoryAxisAuto", "category_axis_auto")?;
        let category_axis_label_alignment = value
            .get("categoryAxisLabelAlignment")
            .or_else(|| value.get("category_axis_label_alignment"))
            .map(Self::parse_axis_label_alignment)
            .transpose()?;
        let category_axis_label_offset = Self::parse_optional_chart_u32(
            &value,
            "categoryAxisLabelOffset",
            "category_axis_label_offset",
        )?;
        let category_axis_tick_mark_skip = Self::parse_optional_chart_u32(
            &value,
            "categoryAxisTickMarkSkip",
            "category_axis_tick_mark_skip",
        )?;
        let category_axis_no_multi_level_labels = Self::parse_optional_chart_bool(
            &value,
            "categoryAxisNoMultiLevelLabels",
            "category_axis_no_multi_level_labels",
        )?;
        let category_axis_orientation = value
            .get("categoryAxisOrientation")
            .or_else(|| value.get("category_axis_orientation"))
            .map(Self::parse_axis_orientation)
            .transpose()?;
        let value_axis_orientation = value
            .get("valueAxisOrientation")
            .or_else(|| value.get("value_axis_orientation"))
            .map(Self::parse_axis_orientation)
            .transpose()?;
        let category_axis_crosses = value
            .get("categoryAxisCrosses")
            .or_else(|| value.get("category_axis_crosses"))
            .map(Self::parse_axis_crosses)
            .transpose()?;
        let category_axis_crosses_at = Self::parse_optional_chart_number(
            &value,
            "categoryAxisCrossesAt",
            "category_axis_crosses_at",
        )?;
        let value_axis_crosses = value
            .get("valueAxisCrosses")
            .or_else(|| value.get("value_axis_crosses"))
            .map(Self::parse_axis_crosses)
            .transpose()?;
        let value_axis_crosses_at = Self::parse_optional_chart_number(
            &value,
            "valueAxisCrossesAt",
            "value_axis_crosses_at",
        )?;
        let value_axis_cross_between = value
            .get("valueAxisCrossBetween")
            .or_else(|| value.get("value_axis_cross_between"))
            .map(Self::parse_axis_cross_between)
            .transpose()?;
        let category_axis_major_tick_mark = value
            .get("categoryAxisMajorTickMark")
            .or_else(|| value.get("category_axis_major_tick_mark"))
            .map(Self::parse_axis_tick_mark)
            .transpose()?;
        let category_axis_minor_tick_mark = value
            .get("categoryAxisMinorTickMark")
            .or_else(|| value.get("category_axis_minor_tick_mark"))
            .map(Self::parse_axis_tick_mark)
            .transpose()?;
        let category_axis_line_color = Self::parse_optional_chart_rgb(
            &value,
            "categoryAxisLineColor",
            "category_axis_line_color",
        )?;
        let category_axis_line_width = Self::parse_optional_chart_u32(
            &value,
            "categoryAxisLineWidth",
            "category_axis_line_width",
        )?;
        let category_axis_major_grid_line_color = Self::parse_optional_chart_rgb(
            &value,
            "categoryAxisMajorGridLineColor",
            "category_axis_major_grid_line_color",
        )?;
        let category_axis_major_grid_line_width = Self::parse_optional_chart_u32(
            &value,
            "categoryAxisMajorGridLineWidth",
            "category_axis_major_grid_line_width",
        )?;
        let category_axis_minor_grid_line_color = Self::parse_optional_chart_rgb(
            &value,
            "categoryAxisMinorGridLineColor",
            "category_axis_minor_grid_line_color",
        )?;
        let category_axis_minor_grid_line_width = Self::parse_optional_chart_u32(
            &value,
            "categoryAxisMinorGridLineWidth",
            "category_axis_minor_grid_line_width",
        )?;
        let value_axis_major_tick_mark = value
            .get("valueAxisMajorTickMark")
            .or_else(|| value.get("value_axis_major_tick_mark"))
            .map(Self::parse_axis_tick_mark)
            .transpose()?;
        let value_axis_minor_tick_mark = value
            .get("valueAxisMinorTickMark")
            .or_else(|| value.get("value_axis_minor_tick_mark"))
            .map(Self::parse_axis_tick_mark)
            .transpose()?;
        let value_axis_line_color =
            Self::parse_optional_chart_rgb(&value, "valueAxisLineColor", "value_axis_line_color")?;
        let value_axis_line_width =
            Self::parse_optional_chart_u32(&value, "valueAxisLineWidth", "value_axis_line_width")?;
        let value_axis_major_grid_line_color = Self::parse_optional_chart_rgb(
            &value,
            "valueAxisMajorGridLineColor",
            "value_axis_major_grid_line_color",
        )?;
        let value_axis_major_grid_line_width = Self::parse_optional_chart_u32(
            &value,
            "valueAxisMajorGridLineWidth",
            "value_axis_major_grid_line_width",
        )?;
        let value_axis_minor_grid_line_color = Self::parse_optional_chart_rgb(
            &value,
            "valueAxisMinorGridLineColor",
            "value_axis_minor_grid_line_color",
        )?;
        let value_axis_minor_grid_line_width = Self::parse_optional_chart_u32(
            &value,
            "valueAxisMinorGridLineWidth",
            "value_axis_minor_grid_line_width",
        )?;
        let value_axis_log_base =
            Self::parse_optional_chart_number(&value, "valueAxisLogBase", "value_axis_log_base")?;
        let value_axis_display_unit = value
            .get("valueAxisDisplayUnit")
            .or_else(|| value.get("value_axis_display_unit"))
            .map(Self::parse_axis_display_unit)
            .transpose()?;
        let value_axis_minimum =
            Self::parse_optional_chart_number(&value, "valueAxisMinimum", "value_axis_minimum")?;
        let value_axis_maximum =
            Self::parse_optional_chart_number(&value, "valueAxisMaximum", "value_axis_maximum")?;
        let value_axis_major_unit = Self::parse_optional_chart_number(
            &value,
            "valueAxisMajorUnit",
            "value_axis_major_unit",
        )?;
        let value_axis_minor_unit = Self::parse_optional_chart_number(
            &value,
            "valueAxisMinorUnit",
            "value_axis_minor_unit",
        )?;
        let category_axis_number_format = Self::parse_optional_chart_string(
            &value,
            "categoryAxisNumberFormat",
            "category_axis_number_format",
        )?;
        let category_axis_number_format_source_linked = Self::parse_optional_chart_bool(
            &value,
            "categoryAxisNumberFormatSourceLinked",
            "category_axis_number_format_source_linked",
        )?;
        let value_axis_number_format = Self::parse_optional_chart_string(
            &value,
            "valueAxisNumberFormat",
            "value_axis_number_format",
        )?;
        let value_axis_number_format_source_linked = Self::parse_optional_chart_bool(
            &value,
            "valueAxisNumberFormatSourceLinked",
            "value_axis_number_format_source_linked",
        )?;
        let categories = value
            .get("categories")
            .map(|raw| {
                raw.as_array()
                    .ok_or_else(|| {
                        HwpError::RenderError("categories는 문자열 배열이어야 합니다".to_string())
                    })?
                    .iter()
                    .map(|item| {
                        item.as_str().map(|s| s.to_string()).ok_or_else(|| {
                            HwpError::RenderError(
                                "categories는 문자열 배열이어야 합니다".to_string(),
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let mut series_updates = Vec::new();
        if let Some(raw_series) = value.get("series") {
            let array = raw_series.as_array().ok_or_else(|| {
                HwpError::RenderError("series는 객체 배열이어야 합니다".to_string())
            })?;
            for (fallback_idx, item) in array.iter().enumerate() {
                let object = item.as_object().ok_or_else(|| {
                    HwpError::RenderError("series 항목은 객체여야 합니다".to_string())
                })?;
                let index = object
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize)
                    .unwrap_or(fallback_idx);
                let name = object
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let values = object
                    .get("values")
                    .map(|raw| {
                        raw.as_array()
                            .ok_or_else(|| {
                                HwpError::RenderError(
                                    "series.values는 숫자 배열이어야 합니다".to_string(),
                                )
                            })?
                            .iter()
                            .map(|item| {
                                item.as_f64().ok_or_else(|| {
                                    HwpError::RenderError(
                                        "series.values는 숫자 배열이어야 합니다".to_string(),
                                    )
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .transpose()?;
                let color = object
                    .get("color")
                    .or_else(|| object.get("colorHex"))
                    .or_else(|| object.get("color_hex"))
                    .map(|raw| Self::parse_chart_rgb_value(raw, "series.color"))
                    .transpose()?;
                let line_color = object
                    .get("lineColor")
                    .or_else(|| object.get("lineColorHex"))
                    .or_else(|| object.get("line_color"))
                    .or_else(|| object.get("line_color_hex"))
                    .map(|raw| Self::parse_chart_rgb_value(raw, "series.lineColor"))
                    .transpose()?;
                let line_width = object
                    .get("lineWidth")
                    .or_else(|| object.get("line_width"))
                    .map(|raw| {
                        raw.as_u64()
                            .and_then(|value| u32::try_from(value).ok())
                            .ok_or_else(|| {
                                HwpError::RenderError(
                                    "series.lineWidth은 0 이상의 정수여야 합니다".to_string(),
                                )
                            })
                    })
                    .transpose()?;
                series_updates.push(crate::ooxml_chart::edit::SeriesXmlUpdate {
                    index,
                    name,
                    values,
                    color,
                    line_color,
                    line_width,
                });
            }
        }
        if title.is_none()
            && chart_type.is_none()
            && grouping.is_none()
            && bar_gap_width.is_none()
            && bar_overlap.is_none()
            && bar_3d_gap_depth.is_none()
            && bar_3d_shape.is_none()
            && line_smooth.is_none()
            && line_marker_visible.is_none()
            && line_marker_size.is_none()
            && line_marker_symbol.is_none()
            && line_marker_fill_color.is_none()
            && line_marker_line_color.is_none()
            && line_marker_line_width.is_none()
            && pie_first_slice_angle.is_none()
            && pie_explosion.is_none()
            && doughnut_hole_size.is_none()
            && pie_of_pie_type.is_none()
            && pie_of_pie_gap_width.is_none()
            && pie_of_pie_second_size.is_none()
            && pie_of_pie_ser_line_color.is_none()
            && pie_of_pie_ser_line_width.is_none()
            && scatter_style.is_none()
            && scatter_smooth.is_none()
            && scatter_marker_size.is_none()
            && scatter_marker_symbol.is_none()
            && scatter_marker_fill_color.is_none()
            && scatter_marker_line_color.is_none()
            && scatter_marker_line_width.is_none()
            && trendline_type.is_none()
            && trendline_order.is_none()
            && trendline_period.is_none()
            && trendline_display_equation.is_none()
            && trendline_display_r_squared.is_none()
            && trendline_line_color.is_none()
            && trendline_line_width.is_none()
            && error_bar_direction.is_none()
            && error_bar_type.is_none()
            && error_bar_value_type.is_none()
            && error_bar_value.is_none()
            && error_bar_no_end_cap.is_none()
            && error_bar_line_color.is_none()
            && error_bar_line_width.is_none()
            && stock_up_down_bar_gap_width.is_none()
            && stock_up_bar_fill_color.is_none()
            && stock_down_bar_fill_color.is_none()
            && stock_up_bar_line_color.is_none()
            && stock_down_bar_line_color.is_none()
            && stock_up_bar_line_width.is_none()
            && stock_down_bar_line_width.is_none()
            && stock_hi_low_line_color.is_none()
            && stock_hi_low_line_width.is_none()
            && data_label_position.is_none()
            && data_labels_show_value.is_none()
            && data_labels_show_category_name.is_none()
            && data_labels_show_series_name.is_none()
            && data_labels_show_percent.is_none()
            && data_labels_show_legend_key.is_none()
            && title_overlay.is_none()
            && date_1904.is_none()
            && chart_style.is_none()
            && chart_area_fill_color.is_none()
            && plot_area_fill_color.is_none()
            && rounded_corners.is_none()
            && auto_title_deleted.is_none()
            && vary_colors.is_none()
            && view_3d_rotation_x.is_none()
            && view_3d_rotation_y.is_none()
            && view_3d_perspective.is_none()
            && view_3d_right_angle_axes.is_none()
            && view_3d_height_percent.is_none()
            && view_3d_depth_percent.is_none()
            && display_blanks_as.is_none()
            && show_hidden_data.is_none()
            && plot_visible_only.is_none()
            && data_table_show_horizontal_border.is_none()
            && data_table_show_vertical_border.is_none()
            && data_table_show_outline.is_none()
            && data_table_show_keys.is_none()
            && legend_position.is_none()
            && legend_overlay.is_none()
            && category_axis_title.is_none()
            && value_axis_title.is_none()
            && category_axis_visible.is_none()
            && value_axis_visible.is_none()
            && category_axis_position.is_none()
            && value_axis_position.is_none()
            && category_axis_label_position.is_none()
            && value_axis_label_position.is_none()
            && category_axis_auto.is_none()
            && category_axis_label_alignment.is_none()
            && category_axis_label_offset.is_none()
            && category_axis_tick_mark_skip.is_none()
            && category_axis_no_multi_level_labels.is_none()
            && category_axis_orientation.is_none()
            && value_axis_orientation.is_none()
            && category_axis_crosses.is_none()
            && category_axis_crosses_at.is_none()
            && value_axis_crosses.is_none()
            && value_axis_crosses_at.is_none()
            && value_axis_cross_between.is_none()
            && category_axis_major_tick_mark.is_none()
            && category_axis_minor_tick_mark.is_none()
            && category_axis_line_color.is_none()
            && category_axis_line_width.is_none()
            && category_axis_major_grid_line_color.is_none()
            && category_axis_major_grid_line_width.is_none()
            && category_axis_minor_grid_line_color.is_none()
            && category_axis_minor_grid_line_width.is_none()
            && value_axis_major_tick_mark.is_none()
            && value_axis_minor_tick_mark.is_none()
            && value_axis_line_color.is_none()
            && value_axis_line_width.is_none()
            && value_axis_major_grid_line_color.is_none()
            && value_axis_major_grid_line_width.is_none()
            && value_axis_minor_grid_line_color.is_none()
            && value_axis_minor_grid_line_width.is_none()
            && value_axis_log_base.is_none()
            && value_axis_display_unit.is_none()
            && value_axis_minimum.is_none()
            && value_axis_maximum.is_none()
            && value_axis_major_unit.is_none()
            && value_axis_minor_unit.is_none()
            && category_axis_number_format.is_none()
            && category_axis_number_format_source_linked.is_none()
            && value_axis_number_format.is_none()
            && value_axis_number_format_source_linked.is_none()
            && categories.is_none()
            && series_updates.is_empty()
        {
            return Err(HwpError::RenderError(
                "title, chartType, grouping, barGapWidth, barOverlap, bar3DGapDepth, bar3DShape, lineSmooth, lineMarkerVisible, lineMarkerSize, lineMarkerSymbol, lineMarkerFillColor, lineMarkerLineColor, lineMarkerLineWidth, pieFirstSliceAngle, pieExplosion, doughnutHoleSize, pieOfPieType, pieOfPieGapWidth, pieOfPieSecondSize, pieOfPieSerLineColor, pieOfPieSerLineWidth, scatterStyle, scatterSmooth, scatterMarkerSize, scatterMarkerSymbol, scatterMarkerFillColor, scatterMarkerLineColor, scatterMarkerLineWidth, trendlineType, trendlineOrder, trendlinePeriod, trendlineDisplayEquation, trendlineDisplayRSquared, trendlineLineColor, trendlineLineWidth, errorBarDirection, errorBarType, errorBarValueType, errorBarValue, errorBarNoEndCap, errorBarLineColor, errorBarLineWidth, stockUpDownBarGapWidth, stockUpBarFillColor, stockDownBarFillColor, stockUpBarLineColor, stockDownBarLineColor, stockUpBarLineWidth, stockDownBarLineWidth, stockHiLowLineColor, stockHiLowLineWidth, dataLabelPosition, dataLabelsShowValue, dataLabelsShowCategoryName, dataLabelsShowSeriesName, dataLabelsShowPercent, dataLabelsShowLegendKey, titleOverlay, date1904, chartStyle, chartAreaFillColor, plotAreaFillColor, roundedCorners, autoTitleDeleted, varyColors, view3DRotationX, view3DRotationY, view3DPerspective, view3DRightAngleAxes, view3DHeightPercent, view3DDepthPercent, displayBlanksAs, showHiddenData, plotVisibleOnly, dataTableShowHorizontalBorder, dataTableShowVerticalBorder, dataTableShowOutline, dataTableShowKeys, legendPosition, legendOverlay, categoryAxisTitle, valueAxisTitle, categoryAxisVisible, valueAxisVisible, categoryAxisPosition, valueAxisPosition, categoryAxisLabelPosition, valueAxisLabelPosition, categoryAxisAuto, categoryAxisLabelAlignment, categoryAxisLabelOffset, categoryAxisTickMarkSkip, categoryAxisNoMultiLevelLabels, categoryAxisOrientation, valueAxisOrientation, categoryAxisCrosses, categoryAxisCrossesAt, valueAxisCrosses, valueAxisCrossesAt, valueAxisCrossBetween, categoryAxisMajorTickMark, categoryAxisMinorTickMark, categoryAxisLineColor, categoryAxisLineWidth, categoryAxisMajorGridLineColor, categoryAxisMajorGridLineWidth, categoryAxisMinorGridLineColor, categoryAxisMinorGridLineWidth, valueAxisMajorTickMark, valueAxisMinorTickMark, valueAxisLineColor, valueAxisLineWidth, valueAxisMajorGridLineColor, valueAxisMajorGridLineWidth, valueAxisMinorGridLineColor, valueAxisMinorGridLineWidth, valueAxisLogBase, valueAxisDisplayUnit, valueAxisMinimum, valueAxisMaximum, valueAxisMajorUnit, valueAxisMinorUnit, categoryAxisNumberFormat, categoryAxisNumberFormatSourceLinked, valueAxisNumberFormat, valueAxisNumberFormatSourceLinked, categories, series.name/values/colorHex/lineColorHex/lineWidth 중 하나는 필요합니다"
                    .to_string(),
            ));
        }
        Ok(crate::ooxml_chart::edit::ChartXmlUpdate {
            title,
            chart_type,
            grouping,
            bar_gap_width,
            bar_overlap,
            bar_3d_gap_depth,
            bar_3d_shape,
            line_smooth,
            line_marker_visible,
            line_marker_size,
            line_marker_symbol,
            line_marker_fill_color,
            line_marker_line_color,
            line_marker_line_width,
            pie_first_slice_angle,
            pie_explosion,
            doughnut_hole_size,
            pie_of_pie_type,
            pie_of_pie_gap_width,
            pie_of_pie_second_size,
            pie_of_pie_ser_line_color,
            pie_of_pie_ser_line_width,
            scatter_style,
            scatter_smooth,
            scatter_marker_size,
            scatter_marker_symbol,
            scatter_marker_fill_color,
            scatter_marker_line_color,
            scatter_marker_line_width,
            trendline_type,
            trendline_order,
            trendline_period,
            trendline_display_equation,
            trendline_display_r_squared,
            trendline_line_color,
            trendline_line_width,
            error_bar_direction,
            error_bar_type,
            error_bar_value_type,
            error_bar_value,
            error_bar_no_end_cap,
            error_bar_line_color,
            error_bar_line_width,
            stock_up_down_bar_gap_width,
            stock_up_bar_fill_color,
            stock_down_bar_fill_color,
            stock_up_bar_line_color,
            stock_down_bar_line_color,
            stock_up_bar_line_width,
            stock_down_bar_line_width,
            stock_hi_low_line_color,
            stock_hi_low_line_width,
            data_label_position,
            data_labels_show_value,
            data_labels_show_category_name,
            data_labels_show_series_name,
            data_labels_show_percent,
            data_labels_show_legend_key,
            title_overlay,
            date_1904,
            chart_style,
            chart_area_fill_color,
            plot_area_fill_color,
            rounded_corners,
            auto_title_deleted,
            vary_colors,
            view_3d_rotation_x,
            view_3d_rotation_y,
            view_3d_perspective,
            view_3d_right_angle_axes,
            view_3d_height_percent,
            view_3d_depth_percent,
            display_blanks_as,
            show_hidden_data,
            plot_visible_only,
            data_table_show_horizontal_border,
            data_table_show_vertical_border,
            data_table_show_outline,
            data_table_show_keys,
            legend_position,
            legend_overlay,
            category_axis_title,
            value_axis_title,
            category_axis_visible,
            value_axis_visible,
            category_axis_position,
            value_axis_position,
            category_axis_label_position,
            value_axis_label_position,
            category_axis_auto,
            category_axis_label_alignment,
            category_axis_label_offset,
            category_axis_tick_mark_skip,
            category_axis_no_multi_level_labels,
            category_axis_orientation,
            value_axis_orientation,
            category_axis_crosses,
            category_axis_crosses_at,
            value_axis_crosses,
            value_axis_crosses_at,
            value_axis_cross_between,
            category_axis_major_tick_mark,
            category_axis_minor_tick_mark,
            category_axis_line_color,
            category_axis_line_width,
            category_axis_major_grid_line_color,
            category_axis_major_grid_line_width,
            category_axis_minor_grid_line_color,
            category_axis_minor_grid_line_width,
            value_axis_major_tick_mark,
            value_axis_minor_tick_mark,
            value_axis_line_color,
            value_axis_line_width,
            value_axis_major_grid_line_color,
            value_axis_major_grid_line_width,
            value_axis_minor_grid_line_color,
            value_axis_minor_grid_line_width,
            value_axis_log_base,
            value_axis_display_unit,
            value_axis_minimum,
            value_axis_maximum,
            value_axis_major_unit,
            value_axis_minor_unit,
            category_axis_number_format,
            category_axis_number_format_source_linked,
            value_axis_number_format,
            value_axis_number_format_source_linked,
            categories,
            series: series_updates,
        })
    }

    fn parse_raw_hwp_chart_data_update(props_json: &str) -> Result<Option<Vec<u8>>, HwpError> {
        let value: serde_json::Value = serde_json::from_str(props_json)
            .map_err(|e| HwpError::RenderError(format!("chart props JSON 파싱 실패: {e}")))?;
        let Some(raw) = value
            .get("rawHwpChartDataBase64")
            .or_else(|| value.get("raw_hwp_chart_data_base64"))
        else {
            return Ok(None);
        };
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("rawHwpChartDataBase64는 문자열이어야 합니다".to_string())
        })?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw)
            .map_err(|e| {
                HwpError::RenderError(format!("rawHwpChartDataBase64 디코드 실패: {e}"))
            })?;
        if bytes.is_empty() {
            return Err(HwpError::RenderError(
                "rawHwpChartDataBase64는 빈 payload일 수 없습니다".to_string(),
            ));
        }
        Ok(Some(bytes))
    }

    fn parse_hwp_chart_semantic_update(
        props_json: &str,
    ) -> Result<HwpChartDataSemanticUpdate, HwpError> {
        let value: serde_json::Value = serde_json::from_str(props_json)
            .map_err(|e| HwpError::RenderError(format!("chart props JSON 파싱 실패: {e}")))?;
        let title = value
            .get("title")
            .map(|raw| {
                raw.as_str()
                    .map(|text| text.to_string())
                    .ok_or_else(|| HwpError::RenderError("title은 문자열이어야 합니다".to_string()))
            })
            .transpose()?;
        let chart_type = value
            .get("chartType")
            .or_else(|| value.get("chart_type"))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("chartType은 문자열이어야 합니다".to_string())
                })?;
                chart_type_from_name(raw).ok_or_else(|| {
                    HwpError::RenderError(
                        "native HWP chartType은 Bar, Column, Line, Pie, Area, Scatter, Unknown 중 하나여야 합니다"
                            .to_string(),
                    )
                })
            })
            .transpose()?;
        let legend_position = value
            .get("legendPosition")
            .or_else(|| value.get("legend_position"))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("legendPosition은 문자열이어야 합니다".to_string())
                })?;
                legend_position_from_name(raw).ok_or_else(|| {
                    HwpError::RenderError(
                        "legendPosition은 Right, Left, Top, Bottom, TopLeft, TopRight, BottomLeft, BottomRight, Hidden 중 하나여야 합니다"
                            .to_string(),
                    )
                })
            })
            .transpose()?;
        let legend_visible = value
            .get("legendVisible")
            .or_else(|| value.get("legend_visible"))
            .map(|raw| {
                raw.as_bool().ok_or_else(|| {
                    HwpError::RenderError("legendVisible은 boolean이어야 합니다".to_string())
                })
            })
            .transpose()?;
        let categories = Self::parse_hwp_chart_categories(&value)?;
        let series = Self::parse_hwp_chart_series(&value, categories.as_deref())?;
        let x_axis = value
            .get("xAxis")
            .or_else(|| value.get("x_axis"))
            .map(Self::parse_hwp_chart_axis)
            .transpose()?;
        let y_axis = value
            .get("yAxis")
            .or_else(|| value.get("y_axis"))
            .map(Self::parse_hwp_chart_axis)
            .transpose()?;

        if title.is_none()
            && chart_type.is_none()
            && legend_position.is_none()
            && legend_visible.is_none()
            && categories.is_none()
            && series.is_none()
            && x_axis.is_none()
            && y_axis.is_none()
        {
            return Err(HwpError::RenderError(
                "native HWP Chart semantic 편집에는 title, chartType, legendPosition, legendVisible, categories, series, xAxis, yAxis 중 하나가 필요합니다"
                    .to_string(),
            ));
        }

        Ok(HwpChartDataSemanticUpdate {
            title,
            chart_type,
            legend_position,
            legend_visible,
            categories,
            series,
            x_axis,
            y_axis,
        })
    }

    fn parse_hwp_chart_categories(
        value: &serde_json::Value,
    ) -> Result<Option<Vec<String>>, HwpError> {
        value
            .get("categories")
            .map(|raw| {
                raw.as_array()
                    .ok_or_else(|| {
                        HwpError::RenderError("categories는 문자열 배열이어야 합니다".to_string())
                    })?
                    .iter()
                    .map(|item| {
                        item.as_str().map(|text| text.to_string()).ok_or_else(|| {
                            HwpError::RenderError(
                                "categories는 문자열 배열이어야 합니다".to_string(),
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()
    }

    fn parse_hwp_chart_series(
        value: &serde_json::Value,
        default_categories: Option<&[String]>,
    ) -> Result<Option<Vec<DataSeries>>, HwpError> {
        let Some(raw_series) = value.get("series") else {
            return Ok(None);
        };
        let array = raw_series
            .as_array()
            .ok_or_else(|| HwpError::RenderError("series는 객체 배열이어야 합니다".to_string()))?;
        let mut series = Vec::with_capacity(array.len());
        for item in array {
            let object = item.as_object().ok_or_else(|| {
                HwpError::RenderError("series 항목은 객체여야 합니다".to_string())
            })?;
            let values = object
                .get("values")
                .map(|raw| {
                    raw.as_array()
                        .ok_or_else(|| {
                            HwpError::RenderError(
                                "series.values는 숫자 배열이어야 합니다".to_string(),
                            )
                        })?
                        .iter()
                        .map(|item| {
                            item.as_f64().ok_or_else(|| {
                                HwpError::RenderError(
                                    "series.values는 숫자 배열이어야 합니다".to_string(),
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            let categories = object
                .get("categories")
                .map(|raw| {
                    raw.as_array()
                        .ok_or_else(|| {
                            HwpError::RenderError(
                                "series.categories는 문자열 배열이어야 합니다".to_string(),
                            )
                        })?
                        .iter()
                        .map(|item| {
                            item.as_str().map(|text| text.to_string()).ok_or_else(|| {
                                HwpError::RenderError(
                                    "series.categories는 문자열 배열이어야 합니다".to_string(),
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .or_else(|| default_categories.map(|categories| categories.to_vec()))
                .unwrap_or_default();
            let color = object
                .get("color")
                .or_else(|| object.get("colorHex"))
                .or_else(|| object.get("color_hex"))
                .map(|raw| Self::parse_chart_rgb_value(raw, "series.color"))
                .transpose()?;
            series.push(DataSeries {
                name: object
                    .get("name")
                    .and_then(|raw| raw.as_str())
                    .unwrap_or_default()
                    .to_string(),
                values,
                categories,
                color,
            });
        }
        Ok(Some(series))
    }

    fn parse_chart_rgb_value(raw: &serde_json::Value, name: &str) -> Result<u32, HwpError> {
        if let Some(number) = raw.as_u64() {
            let rgb = u32::try_from(number).map_err(|_| {
                HwpError::RenderError(format!("{name}는 0x000000..0xFFFFFF 범위여야 합니다"))
            })?;
            if rgb <= 0x00FF_FFFF {
                return Ok(rgb);
            }
            return Err(HwpError::RenderError(format!(
                "{name}는 0x000000..0xFFFFFF 범위여야 합니다"
            )));
        }
        let text = raw.as_str().ok_or_else(|| {
            HwpError::RenderError(format!(
                "{name}는 RGB 정수 또는 #RRGGBB 문자열이어야 합니다"
            ))
        })?;
        let hex = text.trim().trim_start_matches('#');
        if hex.len() != 6 {
            return Err(HwpError::RenderError(format!(
                "{name}는 RGB 정수 또는 #RRGGBB 문자열이어야 합니다"
            )));
        }
        u32::from_str_radix(hex, 16).map_err(|_| {
            HwpError::RenderError(format!(
                "{name}는 RGB 정수 또는 #RRGGBB 문자열이어야 합니다"
            ))
        })
    }

    fn parse_hwp_chart_axis(raw: &serde_json::Value) -> Result<Axis, HwpError> {
        let object = raw
            .as_object()
            .ok_or_else(|| HwpError::RenderError("axis는 객체여야 합니다".to_string()))?;
        let labels = object
            .get("labels")
            .map(|raw| {
                raw.as_array()
                    .ok_or_else(|| {
                        HwpError::RenderError("axis.labels는 문자열 배열이어야 합니다".to_string())
                    })?
                    .iter()
                    .map(|item| {
                        item.as_str().map(|text| text.to_string()).ok_or_else(|| {
                            HwpError::RenderError(
                                "axis.labels는 문자열 배열이어야 합니다".to_string(),
                            )
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok(Axis {
            label: object
                .get("label")
                .and_then(|raw| raw.as_str())
                .map(|text| text.to_string()),
            labels,
            min: object.get("min").and_then(|raw| raw.as_f64()),
            max: object.get("max").and_then(|raw| raw.as_f64()),
        })
    }

    fn validate_hwp_chart_semantic_update(
        chart: &ChartShape,
        update: &HwpChartDataSemanticUpdate,
    ) -> Result<(), HwpError> {
        if let Some(categories) = &update.categories {
            if categories.is_empty()
                && (!chart.series.is_empty()
                    || update
                        .series
                        .as_ref()
                        .is_some_and(|series| !series.is_empty()))
            {
                return Err(HwpError::RenderError(
                    "categories는 비어 있을 수 없습니다".to_string(),
                ));
            }
        }

        if let Some(series) = &update.series {
            if series.is_empty() {
                return Err(HwpError::RenderError(
                    "series는 비어 있을 수 없습니다".to_string(),
                ));
            }
            let default_category_count = update.categories.as_ref().map(Vec::len);
            for (idx, item) in series.iter().enumerate() {
                if item.values.is_empty() {
                    return Err(HwpError::RenderError(format!(
                        "series {idx} values는 비어 있을 수 없습니다"
                    )));
                }
                if item.values.iter().any(|value| !value.is_finite()) {
                    return Err(HwpError::RenderError(
                        "series values에는 유한한 숫자만 사용할 수 있습니다".to_string(),
                    ));
                }
                let expected = if item.categories.is_empty() {
                    default_category_count
                } else {
                    Some(item.categories.len())
                };
                if let Some(expected) = expected {
                    if item.values.len() != expected {
                        return Err(HwpError::RenderError(format!(
                            "series {idx} values 길이 불일치: 입력 {}, 기대 {}",
                            item.values.len(),
                            expected
                        )));
                    }
                }
            }
        }

        if let Some(categories) = &update.categories {
            if update.series.is_none() {
                for (idx, existing) in chart.series.iter().enumerate() {
                    if !existing.values.is_empty() && existing.values.len() != categories.len() {
                        return Err(HwpError::RenderError(format!(
                            "category/value point 수를 바꾸려면 series {idx} values도 같은 길이로 제공해야 합니다"
                        )));
                    }
                }
            }
        }

        for (label, axis) in [("xAxis", &update.x_axis), ("yAxis", &update.y_axis)] {
            if let Some(axis) = axis {
                if let (Some(min), Some(max)) = (axis.min, axis.max) {
                    if max <= min {
                        return Err(HwpError::RenderError(format!(
                            "{label}.max는 {label}.min보다 커야 합니다"
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    fn set_hwp_chart_data_semantic(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        props_json: &str,
    ) -> Result<ChartShape, HwpError> {
        let update = Self::parse_hwp_chart_semantic_update(props_json)?;
        let updated = {
            let shape =
                self.resolve_shape_control_mut(section_idx, parent_para_idx, control_idx)?;
            let ShapeObject::Chart(chart) = shape else {
                return Err(HwpError::RenderError(
                    "native HWP Chart semantic update target is not a Chart control".to_string(),
                ));
            };
            if !chart.raw_chart_data.starts_with(RHWP_CHART_DATA_JSON_MAGIC) {
                return Err(HwpError::RenderError(
                    "native HWP CHART_DATA semantic edit is only supported for rhwp-generated semantic payloads; use rawHwpChartDataBase64 for explicit opaque payload replacement"
                        .to_string(),
                ));
            }
            if !chart_has_semantic(chart) && !apply_rhwp_chart_data_semantic(chart) {
                return Err(HwpError::RenderError(
                    "rhwp-generated native HWP CHART_DATA semantic payload could not be decoded"
                        .to_string(),
                ));
            }
            Self::validate_hwp_chart_semantic_update(chart, &update)?;
            if let Some(title) = update.title {
                chart.title = Some(title);
            }
            if let Some(chart_type) = update.chart_type {
                chart.chart_type = chart_type;
            }
            if update.legend_position.is_some() || update.legend_visible.is_some() {
                let mut legend = chart.legend.clone().unwrap_or(Legend {
                    position: update.legend_position.unwrap_or_default(),
                    visible: true,
                });
                if let Some(position) = update.legend_position {
                    legend.position = position;
                }
                if let Some(visible) = update.legend_visible {
                    legend.visible = visible;
                }
                chart.legend = Some(legend);
            }
            if let Some(categories) = update.categories {
                if chart.series.is_empty() {
                    chart.x_axis.get_or_insert_with(Axis::default).labels = categories;
                } else {
                    for series in &mut chart.series {
                        series.categories = categories.clone();
                    }
                }
            }
            if let Some(series) = update.series {
                chart.series = series;
            }
            if let Some(x_axis) = update.x_axis {
                chart.x_axis = Some(x_axis);
            }
            if let Some(y_axis) = update.y_axis {
                chart.y_axis = Some(y_axis);
            }
            chart.raw_chart_data = encode_rhwp_chart_data_semantic(chart);
            chart.as_ref().clone()
        };
        for section in &mut self.document.sections {
            section.raw_stream = None;
        }
        self.invalidate_page_tree_cache();
        Ok(updated)
    }

    fn parse_axis_label_position(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::AxisLabelPosition, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("axis label position은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "NextTo" | "nextTo" | "next_to" => Ok(crate::ooxml_chart::AxisLabelPosition::NextTo),
            "High" | "high" => Ok(crate::ooxml_chart::AxisLabelPosition::High),
            "Low" | "low" => Ok(crate::ooxml_chart::AxisLabelPosition::Low),
            "None" | "none" => Ok(crate::ooxml_chart::AxisLabelPosition::None),
            _ => Err(HwpError::RenderError(
                "axis label position은 NextTo, High, Low, None 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_axis_position(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::AxisPosition, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("axis position은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "Bottom" | "bottom" | "b" => Ok(crate::ooxml_chart::AxisPosition::Bottom),
            "Left" | "left" | "l" => Ok(crate::ooxml_chart::AxisPosition::Left),
            "Top" | "top" | "t" => Ok(crate::ooxml_chart::AxisPosition::Top),
            "Right" | "right" | "r" => Ok(crate::ooxml_chart::AxisPosition::Right),
            _ => Err(HwpError::RenderError(
                "axis position은 Bottom, Left, Top, Right 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_axis_label_alignment(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::AxisLabelAlignment, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("axis label alignment는 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "Center" | "center" | "ctr" => Ok(crate::ooxml_chart::AxisLabelAlignment::Center),
            "Left" | "left" | "l" => Ok(crate::ooxml_chart::AxisLabelAlignment::Left),
            "Right" | "right" | "r" => Ok(crate::ooxml_chart::AxisLabelAlignment::Right),
            _ => Err(HwpError::RenderError(
                "axis label alignment는 Center, Left, Right 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_axis_orientation(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::AxisOrientation, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("axis orientation은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "MinMax" | "minMax" | "min_max" => Ok(crate::ooxml_chart::AxisOrientation::MinMax),
            "MaxMin" | "maxMin" | "max_min" => Ok(crate::ooxml_chart::AxisOrientation::MaxMin),
            _ => Err(HwpError::RenderError(
                "axis orientation은 MinMax, MaxMin 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_axis_crosses(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::AxisCrosses, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("axis crosses는 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "AutoZero" | "autoZero" | "auto_zero" => Ok(crate::ooxml_chart::AxisCrosses::AutoZero),
            "Min" | "min" => Ok(crate::ooxml_chart::AxisCrosses::Min),
            "Max" | "max" => Ok(crate::ooxml_chart::AxisCrosses::Max),
            _ => Err(HwpError::RenderError(
                "axis crosses는 AutoZero, Min, Max 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_axis_cross_between(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::AxisCrossBetween, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("valueAxisCrossBetween은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "Between" | "between" => Ok(crate::ooxml_chart::AxisCrossBetween::Between),
            "MidCategory" | "midCategory" | "mid_category" | "midCat" => {
                Ok(crate::ooxml_chart::AxisCrossBetween::MidCategory)
            }
            _ => Err(HwpError::RenderError(
                "valueAxisCrossBetween은 Between, MidCategory 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_axis_tick_mark(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::AxisTickMark, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("axis tick mark는 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "Cross" | "cross" => Ok(crate::ooxml_chart::AxisTickMark::Cross),
            "In" | "in" => Ok(crate::ooxml_chart::AxisTickMark::In),
            "Out" | "out" => Ok(crate::ooxml_chart::AxisTickMark::Out),
            "None" | "none" => Ok(crate::ooxml_chart::AxisTickMark::None),
            _ => Err(HwpError::RenderError(
                "axis tick mark는 Cross, In, Out, None 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_axis_display_unit(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::AxisDisplayUnit, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("valueAxisDisplayUnit은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "Hundreds" | "hundreds" => Ok(crate::ooxml_chart::AxisDisplayUnit::Hundreds),
            "Thousands" | "thousands" => Ok(crate::ooxml_chart::AxisDisplayUnit::Thousands),
            "TenThousands" | "tenThousands" | "ten_thousands" => {
                Ok(crate::ooxml_chart::AxisDisplayUnit::TenThousands)
            }
            "HundredThousands" | "hundredThousands" | "hundred_thousands" => {
                Ok(crate::ooxml_chart::AxisDisplayUnit::HundredThousands)
            }
            "Millions" | "millions" => Ok(crate::ooxml_chart::AxisDisplayUnit::Millions),
            "TenMillions" | "tenMillions" | "ten_millions" => {
                Ok(crate::ooxml_chart::AxisDisplayUnit::TenMillions)
            }
            "HundredMillions" | "hundredMillions" | "hundred_millions" => {
                Ok(crate::ooxml_chart::AxisDisplayUnit::HundredMillions)
            }
            "Billions" | "billions" => Ok(crate::ooxml_chart::AxisDisplayUnit::Billions),
            "Trillions" | "trillions" => Ok(crate::ooxml_chart::AxisDisplayUnit::Trillions),
            _ => Err(HwpError::RenderError(
                "valueAxisDisplayUnit은 Hundreds, Thousands, TenThousands, HundredThousands, Millions, TenMillions, HundredMillions, Billions, Trillions 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_data_label_position(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::ChartDataLabelPosition, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("dataLabelPosition은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "BestFit" | "bestFit" | "best_fit" => {
                Ok(crate::ooxml_chart::ChartDataLabelPosition::BestFit)
            }
            "Bottom" | "bottom" | "b" => {
                Ok(crate::ooxml_chart::ChartDataLabelPosition::Bottom)
            }
            "Center" | "center" | "ctr" => {
                Ok(crate::ooxml_chart::ChartDataLabelPosition::Center)
            }
            "InsideBase" | "insideBase" | "inside_base" | "inBase" => {
                Ok(crate::ooxml_chart::ChartDataLabelPosition::InsideBase)
            }
            "InsideEnd" | "insideEnd" | "inside_end" | "inEnd" => {
                Ok(crate::ooxml_chart::ChartDataLabelPosition::InsideEnd)
            }
            "Left" | "left" | "l" => Ok(crate::ooxml_chart::ChartDataLabelPosition::Left),
            "OutsideEnd" | "outsideEnd" | "outside_end" | "outEnd" => {
                Ok(crate::ooxml_chart::ChartDataLabelPosition::OutsideEnd)
            }
            "Right" | "right" | "r" => Ok(crate::ooxml_chart::ChartDataLabelPosition::Right),
            "Top" | "top" | "t" => Ok(crate::ooxml_chart::ChartDataLabelPosition::Top),
            _ => Err(HwpError::RenderError(
                "dataLabelPosition은 BestFit, Bottom, Center, InsideBase, InsideEnd, Left, OutsideEnd, Right, Top 중 하나여야 합니다"
                    .to_string(),
            )),
        }
    }

    fn parse_display_blanks_as(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::ChartDisplayBlanksAs, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("displayBlanksAs는 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "Gap" | "gap" => Ok(crate::ooxml_chart::ChartDisplayBlanksAs::Gap),
            "Span" | "span" => Ok(crate::ooxml_chart::ChartDisplayBlanksAs::Span),
            "Zero" | "zero" => Ok(crate::ooxml_chart::ChartDisplayBlanksAs::Zero),
            _ => Err(HwpError::RenderError(
                "displayBlanksAs는 Gap, Span, Zero 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_error_bar_direction(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::ChartErrorBarDirection, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("errorBarDirection은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "X" | "x" => Ok(crate::ooxml_chart::ChartErrorBarDirection::X),
            "Y" | "y" => Ok(crate::ooxml_chart::ChartErrorBarDirection::Y),
            _ => Err(HwpError::RenderError(
                "errorBarDirection은 X 또는 Y 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_error_bar_type(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::ChartErrorBarType, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("errorBarType은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "Both" | "both" => Ok(crate::ooxml_chart::ChartErrorBarType::Both),
            "Plus" | "plus" => Ok(crate::ooxml_chart::ChartErrorBarType::Plus),
            "Minus" | "minus" => Ok(crate::ooxml_chart::ChartErrorBarType::Minus),
            _ => Err(HwpError::RenderError(
                "errorBarType은 Both, Plus, Minus 중 하나여야 합니다".to_string(),
            )),
        }
    }

    fn parse_error_bar_value_type(
        raw: &serde_json::Value,
    ) -> Result<crate::ooxml_chart::ChartErrorBarValueType, HwpError> {
        let raw = raw.as_str().ok_or_else(|| {
            HwpError::RenderError("errorBarValueType은 문자열이어야 합니다".to_string())
        })?;
        match raw {
            "FixedValue" | "fixedValue" | "fixed_value" | "fixedVal" => {
                Ok(crate::ooxml_chart::ChartErrorBarValueType::FixedValue)
            }
            "Percentage" | "percentage" | "percent" => {
                Ok(crate::ooxml_chart::ChartErrorBarValueType::Percentage)
            }
            "StandardDeviation" | "standardDeviation" | "standard_deviation" | "stdDev" => {
                Ok(crate::ooxml_chart::ChartErrorBarValueType::StandardDeviation)
            }
            "StandardError" | "standardError" | "standard_error" | "stdErr" => {
                Ok(crate::ooxml_chart::ChartErrorBarValueType::StandardError)
            }
            _ => Err(HwpError::RenderError(
                "errorBarValueType은 FixedValue, Percentage, StandardDeviation, StandardError 중 하나여야 합니다"
                    .to_string(),
            )),
        }
    }

    fn parse_optional_chart_bar_3d_shape(
        value: &serde_json::Value,
    ) -> Result<Option<String>, HwpError> {
        value
            .get("bar3DShape")
            .or_else(|| value.get("bar_3d_shape"))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError("bar3DShape은 문자열이어야 합니다".to_string())
                })?;
                match raw {
                    "box" | "cone" | "coneToMax" | "cylinder" | "pyramid" | "pyramidToMax" => {
                        Ok(raw.to_string())
                    }
                    _ => Err(HwpError::RenderError(
                        "bar3DShape은 box, cone, coneToMax, cylinder, pyramid, pyramidToMax 중 하나여야 합니다"
                            .to_string(),
                    )),
                }
            })
            .transpose()
    }

    fn parse_optional_chart_bool(
        value: &serde_json::Value,
        camel_name: &str,
        snake_name: &str,
    ) -> Result<Option<bool>, HwpError> {
        value
            .get(camel_name)
            .or_else(|| value.get(snake_name))
            .map(|raw| {
                raw.as_bool().ok_or_else(|| {
                    HwpError::RenderError(format!("{camel_name}은 boolean이어야 합니다"))
                })
            })
            .transpose()
    }

    fn parse_optional_chart_string(
        value: &serde_json::Value,
        camel_name: &str,
        snake_name: &str,
    ) -> Result<Option<String>, HwpError> {
        value
            .get(camel_name)
            .or_else(|| value.get(snake_name))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError(format!("{camel_name}은 문자열이어야 합니다"))
                })?;
                if raw.trim().is_empty() {
                    return Err(HwpError::RenderError(format!(
                        "{camel_name}은 빈 문자열일 수 없습니다"
                    )));
                }
                Ok(raw.to_string())
            })
            .transpose()
    }

    fn parse_optional_chart_number(
        value: &serde_json::Value,
        camel_name: &str,
        snake_name: &str,
    ) -> Result<Option<f64>, HwpError> {
        value
            .get(camel_name)
            .or_else(|| value.get(snake_name))
            .map(|raw| {
                raw.as_f64()
                    .ok_or_else(|| HwpError::RenderError(format!("{camel_name}은 숫자여야 합니다")))
            })
            .transpose()
    }

    fn parse_optional_chart_u32(
        value: &serde_json::Value,
        camel_name: &str,
        snake_name: &str,
    ) -> Result<Option<u32>, HwpError> {
        value
            .get(camel_name)
            .or_else(|| value.get(snake_name))
            .map(|raw| {
                raw.as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .ok_or_else(|| {
                        HwpError::RenderError(format!("{camel_name}은 0 이상의 정수여야 합니다"))
                    })
            })
            .transpose()
    }

    fn parse_optional_chart_i32(
        value: &serde_json::Value,
        camel_name: &str,
        snake_name: &str,
    ) -> Result<Option<i32>, HwpError> {
        value
            .get(camel_name)
            .or_else(|| value.get(snake_name))
            .map(|raw| {
                raw.as_i64()
                    .and_then(|value| i32::try_from(value).ok())
                    .ok_or_else(|| HwpError::RenderError(format!("{camel_name}은 정수여야 합니다")))
            })
            .transpose()
    }

    fn parse_optional_chart_marker_symbol(
        value: &serde_json::Value,
        camel_name: &str,
        snake_name: &str,
    ) -> Result<Option<crate::ooxml_chart::ChartMarkerSymbol>, HwpError> {
        value
            .get(camel_name)
            .or_else(|| value.get(snake_name))
            .map(|raw| {
                let raw = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError(format!("{camel_name}은 문자열이어야 합니다"))
                })?;
                match raw {
                    "Circle" | "circle" => Ok(crate::ooxml_chart::ChartMarkerSymbol::Circle),
                    "Dash" | "dash" => Ok(crate::ooxml_chart::ChartMarkerSymbol::Dash),
                    "Diamond" | "diamond" => Ok(crate::ooxml_chart::ChartMarkerSymbol::Diamond),
                    "Dot" | "dot" => Ok(crate::ooxml_chart::ChartMarkerSymbol::Dot),
                    "None" | "none" => Ok(crate::ooxml_chart::ChartMarkerSymbol::None),
                    "Picture" | "picture" => Ok(crate::ooxml_chart::ChartMarkerSymbol::Picture),
                    "Plus" | "plus" => Ok(crate::ooxml_chart::ChartMarkerSymbol::Plus),
                    "Square" | "square" => Ok(crate::ooxml_chart::ChartMarkerSymbol::Square),
                    "Star" | "star" => Ok(crate::ooxml_chart::ChartMarkerSymbol::Star),
                    "Triangle" | "triangle" => {
                        Ok(crate::ooxml_chart::ChartMarkerSymbol::Triangle)
                    }
                    "X" | "x" => Ok(crate::ooxml_chart::ChartMarkerSymbol::X),
                    _ => Err(HwpError::RenderError(format!(
                        "{camel_name}은 Circle, Dash, Diamond, Dot, None, Picture, Plus, Square, Star, Triangle, X 중 하나여야 합니다"
                    ))),
                }
            })
            .transpose()
    }

    fn parse_optional_chart_rgb_with_hex_alias(
        value: &serde_json::Value,
        camel_name: &str,
        snake_name: &str,
        camel_hex_name: &str,
        snake_hex_name: &str,
    ) -> Result<Option<u32>, HwpError> {
        value
            .get(camel_name)
            .or_else(|| value.get(camel_hex_name))
            .or_else(|| value.get(snake_name))
            .or_else(|| value.get(snake_hex_name))
            .map(|raw| Self::parse_chart_rgb_value(raw, camel_name))
            .transpose()
    }

    fn parse_optional_chart_rgb(
        value: &serde_json::Value,
        camel_name: &str,
        snake_name: &str,
    ) -> Result<Option<u32>, HwpError> {
        value
            .get(camel_name)
            .or_else(|| value.get(snake_name))
            .map(|raw| {
                if let Some(number) = raw.as_u64() {
                    let rgb = u32::try_from(number).map_err(|_| {
                        HwpError::RenderError(format!(
                            "{camel_name}은 0x000000..0xFFFFFF 범위여야 합니다"
                        ))
                    })?;
                    if rgb <= 0x00FF_FFFF {
                        return Ok(rgb);
                    }
                    return Err(HwpError::RenderError(format!(
                        "{camel_name}은 0x000000..0xFFFFFF 범위여야 합니다"
                    )));
                }
                let text = raw.as_str().ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "{camel_name}은 RGB 정수 또는 #RRGGBB 문자열이어야 합니다"
                    ))
                })?;
                let hex = text.trim().trim_start_matches('#');
                if hex.len() != 6 {
                    return Err(HwpError::RenderError(format!(
                        "{camel_name}은 RGB 정수 또는 #RRGGBB 문자열이어야 합니다"
                    )));
                }
                u32::from_str_radix(hex, 16).map_err(|_| {
                    HwpError::RenderError(format!(
                        "{camel_name}은 RGB 정수 또는 #RRGGBB 문자열이어야 합니다"
                    ))
                })
            })
            .transpose()
    }

    /// Body OOXML chart semantic data 조회.
    pub fn get_chart_data_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<String, HwpError> {
        let shape = self.resolve_shape_control_ref(section_idx, parent_para_idx, control_idx)?;
        if let ShapeObject::Chart(chart) = shape {
            return Self::hwp_chart_data_json(chart, false);
        }
        let location = match self.chart_ooxml_location_for_shape(shape) {
            Ok(location) => location,
            Err(ooxml_err) => {
                if let Some(contents) = self.legacy_ole_chart_contents_for_shape(shape)? {
                    let chart = crate::ole_chart::parse_ole_chart_contents(&contents.raw_contents)
                        .map_err(|err| {
                            HwpError::RenderError(format!(
                                "legacy OLE Contents chart를 파싱할 수 없습니다: {}",
                                err.stable_message()
                            ))
                        })?;
                    return Self::legacy_ole_chart_json(&contents, &chart);
                }
                return Err(ooxml_err);
            }
        };
        let xml = self.chart_ooxml_xml_for_location(location)?;
        let chart = crate::ooxml_chart::OoxmlChart::parse(&xml).ok_or_else(|| {
            HwpError::RenderError("OOXML chart XML을 파싱할 수 없습니다".to_string())
        })?;
        Self::ooxml_chart_json(location.bin_data_id(), true, &chart)
    }

    /// Body OOXML chart semantic cache 값 변경.
    pub fn set_chart_data_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let raw_hwp_chart_update = Self::parse_raw_hwp_chart_data_update(props_json)?;
        if let Some(raw_hwp_chart_data) = raw_hwp_chart_update {
            let updated = {
                let shape =
                    self.resolve_shape_control_mut(section_idx, parent_para_idx, control_idx)?;
                match shape {
                    ShapeObject::Chart(chart) => {
                        chart.raw_chart_data = raw_hwp_chart_data;
                        if !apply_rhwp_chart_data_semantic(chart) {
                            clear_chart_semantic(chart);
                        }
                        chart.clone()
                    }
                    _ => {
                        return Err(HwpError::RenderError(
                            "rawHwpChartDataBase64는 HWP CHART_DATA Chart 컨트롤에서만 사용할 수 있습니다"
                                .to_string(),
                        ))
                    }
                }
            };
            for section in &mut self.document.sections {
                section.raw_stream = None;
            }
            self.invalidate_page_tree_cache();
            return Self::hwp_chart_data_json(&updated, true);
        }

        let location = {
            let shape =
                self.resolve_shape_control_ref(section_idx, parent_para_idx, control_idx)?;
            if matches!(shape, ShapeObject::Chart(_)) {
                let updated = self.set_hwp_chart_data_semantic(
                    section_idx,
                    parent_para_idx,
                    control_idx,
                    props_json,
                )?;
                return Self::hwp_chart_data_json(&updated, false);
            }
            if let Some(contents) = self.legacy_ole_chart_contents_for_shape(shape)? {
                if crate::ole_chart::parse_ole_chart_contents(&contents.raw_contents).is_ok() {
                    return Err(HwpError::RenderError(
                        "legacy OLE Contents chart는 현재 읽기/렌더 진단만 지원하며 rhwp_set_chart_data semantic 편집은 지원하지 않습니다"
                            .to_string(),
                    ));
                }
            }
            self.chart_ooxml_location_for_shape(shape)?
        };
        let update = Self::parse_chart_xml_update(props_json)?;
        let source_xml = self.chart_ooxml_xml_for_location(location)?;
        let edited = crate::ooxml_chart::edit::update_chart_xml(&source_xml, &update)
            .map_err(HwpError::RenderError)?;
        match location {
            ChartOoxmlLocation::DirectXml { content_idx, .. } => {
                self.document.bin_data_content[content_idx].data = edited.clone();
            }
            ChartOoxmlLocation::OleContainer { content_idx, .. } => {
                let rewritten = replace_ooxml_chart_contents_stream(
                    &self.document.bin_data_content[content_idx].data,
                    &edited,
                )?;
                self.document.bin_data_content[content_idx].data = rewritten;
            }
        }
        for section in &mut self.document.sections {
            section.raw_stream = None;
        }
        self.invalidate_page_tree_cache();
        let chart = crate::ooxml_chart::OoxmlChart::parse(&edited).ok_or_else(|| {
            HwpError::RenderError("수정된 OOXML chart XML을 파싱할 수 없습니다".to_string())
        })?;
        Self::ooxml_chart_json(location.bin_data_id(), true, &chart)
    }

    /// 그림 컨트롤의 속성을 변경한다 (네이티브).
    pub fn set_picture_properties_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        // JSON 파싱 (serde_json 사용 대신 수동 파싱 — 기존 패턴)
        // [Task #825] 픽쳐 속성 mutation 은 helper 로 분리 (머리말/꼬리말 path 와 공유).
        let (caption_created, should_migrate_to_inline, should_migrate_to_floating) = {
            let pic =
                self.resolve_picture_control_mut(section_idx, parent_para_idx, control_idx)?;
            // [Task #1151 v2] tac false→true migration 검출용 snapshot.
            let was_tac = pic.common.treat_as_char;
            let caption_created = Self::apply_picture_props_inner(pic, props_json);
            let now_tac = pic.common.treat_as_char;
            (caption_created, !was_tac && now_tac, was_tac && !now_tac)
        };

        // [Task #1151 v2] floating → inline migration (H1 정합, samples/tac-verify/).
        // 한컴 산출물 Scenario A~D 분석: tac false→true 시 picture 의 control 위치는
        // 불변이고, 4 필드만 갱신 (treat_as_char / h/v_rel_to=Para / h/v_offset=0 /
        // parent line_segs[0]). text/char_offsets/paragraph 수 변화 없음.
        if should_migrate_to_inline || should_migrate_to_floating {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let body_len = section.paragraphs.len();
            let para = if parent_para_idx < body_len {
                section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
                })?
            } else {
                let mut virtual_idx = parent_para_idx - body_len;
                let mut found = None;
                'outer: for body_para in &mut section.paragraphs {
                    for ctrl in &mut body_para.controls {
                        if let Control::Endnote(en) = ctrl {
                            if virtual_idx < en.paragraphs.len() {
                                found = en.paragraphs.get_mut(virtual_idx);
                                break 'outer;
                            }
                            virtual_idx -= en.paragraphs.len();
                        }
                    }
                }
                found.ok_or_else(|| {
                    HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
                })?
            };
            if should_migrate_to_inline {
                let crate::model::paragraph::Paragraph {
                    line_segs,
                    controls,
                    ..
                } = &mut *para;
                match controls.get_mut(control_idx) {
                    Some(Control::Picture(pic_box)) => {
                        Self::migrate_picture_floating_to_inline(line_segs, pic_box.as_mut());
                    }
                    Some(Control::Shape(shape)) => {
                        if let ShapeObject::Picture(pic) = shape.as_mut() {
                            Self::migrate_picture_floating_to_inline(line_segs, pic);
                        }
                    }
                    _ => {}
                }
            } else {
                Self::migrate_empty_picture_para_inline_to_floating(para);
            }
        }
        // 캡션 생성 시 AutoNumber 재할당 + 기본 텍스트 생성.
        if caption_created {
            crate::parser::assign_auto_numbers(&mut self.document);
            let pic_mut =
                self.resolve_picture_control_mut(section_idx, parent_para_idx, control_idx)?;
            Self::finish_new_picture_caption(pic_mut);
        }
        // 리플로우
        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        // [Task #1151 v5] page tree cache invalidate — 다른 picture/shape setter (셀 shape
        // by_path / 셀 picture by_path / header-footer picture / shape 등) 모두 호출하나
        // 본 본문 picture setter 만 누락되어 있어 studio 가 stale page tree 반환 → tac toggle
        // 후 시각 변화 없음 증상의 root cause.
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: parent_para_idx,
            ctrl: control_idx,
        });
        if caption_created {
            let char_offset = self
                .resolve_picture_control_ref(section_idx, parent_para_idx, control_idx)?
                .caption
                .as_ref()
                .map_or(0, |c| {
                    c.paragraphs.first().map_or(0, |p| p.text.chars().count())
                });
            Ok(format!(
                "{{\"ok\":true,\"captionCharOffset\":{}}}",
                char_offset
            ))
        } else {
            Ok("{\"ok\":true}".to_string())
        }
    }

    /// [Task #825] 머리말/꼬리말 안 그림 속성 변경.
    /// path: section[si].paragraphs[outer_para].controls[outer_ctrl] = Header/Footer
    ///       → .paragraphs[inner_para].controls[inner_ctrl] = Picture
    pub fn set_header_footer_picture_properties_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let caption_created;
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let inner_paras: &mut Vec<crate::model::paragraph::Paragraph> = match outer_ctrl {
                crate::model::control::Control::Header(h) => &mut h.paragraphs,
                crate::model::control::Control::Footer(f) => &mut f.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let inner_ctrl = inner_para
                .controls
                .get_mut(inner_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "내부 컨트롤 인덱스 {} 범위 초과",
                        inner_control_idx
                    ))
                })?;
            let pic = match inner_ctrl {
                crate::model::control::Control::Picture(p) => p,
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 내부 컨트롤이 그림이 아닙니다".to_string(),
                    ))
                }
            };
            caption_created = Self::apply_picture_props_inner(pic, props_json);
        }
        if caption_created {
            crate::parser::assign_auto_numbers(&mut self.document);
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let inner_paras: &mut Vec<crate::model::paragraph::Paragraph> = match outer_ctrl {
                crate::model::control::Control::Header(h) => &mut h.paragraphs,
                crate::model::control::Control::Footer(f) => &mut f.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let inner_ctrl = inner_para
                .controls
                .get_mut(inner_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "내부 컨트롤 인덱스 {} 범위 초과",
                        inner_control_idx
                    ))
                })?;
            let pic = match inner_ctrl {
                crate::model::control::Control::Picture(p) => p,
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 내부 컨트롤이 그림이 아닙니다".to_string(),
                    ))
                }
            };
            Self::finish_new_picture_caption(pic);
        }
        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: outer_para_idx,
            ctrl: outer_control_idx,
        });
        if caption_created {
            let section = self.document.sections.get(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let char_offset = section
                .paragraphs
                .get(outer_para_idx)
                .and_then(|outer_para| outer_para.controls.get(outer_control_idx))
                .and_then(|outer_ctrl| match outer_ctrl {
                    crate::model::control::Control::Header(h) => h.paragraphs.get(inner_para_idx),
                    crate::model::control::Control::Footer(f) => f.paragraphs.get(inner_para_idx),
                    _ => None,
                })
                .and_then(|inner_para| inner_para.controls.get(inner_control_idx))
                .and_then(|inner_ctrl| match inner_ctrl {
                    crate::model::control::Control::Picture(p) => p.caption.as_ref(),
                    _ => None,
                })
                .and_then(|caption| caption.paragraphs.first())
                .map_or(0, |para| para.text.chars().count());
            Ok(format!(
                "{{\"ok\":true,\"captionCharOffset\":{}}}",
                char_offset
            ))
        } else {
            Ok("{\"ok\":true}".to_string())
        }
    }

    /// 머리말/꼬리말 안 Shape/OLE/Chart 속성 변경.
    pub fn set_header_footer_shape_properties_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let requested_ole_bin_data_id = Self::requested_ole_bin_data_id(props_json);
        let requested_ole_bin_data_id_exists = requested_ole_bin_data_id
            .map(|bin_data_id| self.ole_bin_data_id_exists(bin_data_id))
            .unwrap_or(false);
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let inner_paras: &mut Vec<crate::model::paragraph::Paragraph> = match outer_ctrl {
                crate::model::control::Control::Header(h) => &mut h.paragraphs,
                crate::model::control::Control::Footer(f) => &mut f.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let inner_ctrl = inner_para
                .controls
                .get_mut(inner_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "내부 컨트롤 인덱스 {} 범위 초과",
                        inner_control_idx
                    ))
                })?;
            let shape = match inner_ctrl {
                crate::model::control::Control::Shape(shape) => shape.as_mut(),
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                    ))
                }
            };
            Self::validate_requested_ole_bin_data_id_for_shape(
                shape,
                requested_ole_bin_data_id,
                requested_ole_bin_data_id_exists,
            )?;
            Self::apply_shape_props_inner(shape, props_json);
        }
        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: outer_para_idx,
            ctrl: outer_control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// [Task #1151 v2] Floating picture → inline 마이그레이션 (H1 정합).
    ///
    /// 한컴 2022 산출물 (`samples/tac-verify/scenario-{a,b,c,d}-after.hwp`) 분석
    /// 결과: floating picture 의 `treat_as_char` 가 false→true 로 토글될 때
    /// 한컴은 다음만 갱신한다 (자세한 분석: `mydocs/tech/hancom_picture_tac_toggle.md`).
    ///
    /// Picture 자체: `horz_rel_to = Para`, `vert_rel_to = Para`,
    /// `horizontal_offset = 0`, `vertical_offset = 0`. (`treat_as_char = true` 와 attr
    /// 비트는 `apply_picture_props_inner` 가 이미 처리.)
    ///
    /// Parent paragraph 의 `line_segs[0]`: `line_height = picture.common.height`,
    /// `text_height = picture.common.height`, `baseline_distance = round(line_height × 0.85)`.
    /// 비율 0.85 는 한컴 산출물 4 시나리오 (5331/16038/4847/19019) 모두 정확 관찰.
    /// `line_segs` 가 비어있으면 신설 (line_spacing=600 기본).
    ///
    /// 변경 없음: paragraph.text / char_offsets / char_shapes / paragraph 수, picture
    /// control 의 paragraph 위치 (sentinel char 추가하지 않음, 셀 안 이동 / 새 paragraph
    /// 분리 모두 없음 — H1 정합).
    pub(crate) fn migrate_picture_floating_to_inline(
        line_segs: &mut Vec<crate::model::paragraph::LineSeg>,
        pic: &mut crate::model::image::Picture,
    ) {
        use crate::model::shape::{HorzRelTo, VertRelTo};
        pic.common.horz_rel_to = HorzRelTo::Para;
        pic.common.vert_rel_to = VertRelTo::Para;
        pic.common.horizontal_offset = 0;
        pic.common.vertical_offset = 0;

        let picture_height_hu = pic.common.height as i32;
        let baseline = (picture_height_hu as f64 * 0.85).round() as i32;
        if let Some(seg) = line_segs.first_mut() {
            seg.line_height = picture_height_hu;
            seg.text_height = picture_height_hu;
            seg.baseline_distance = baseline;
        } else {
            line_segs.push(crate::model::paragraph::LineSeg {
                line_height: picture_height_hu,
                text_height: picture_height_hu,
                baseline_distance: baseline,
                line_spacing: 600,
                ..Default::default()
            });
        }
    }

    /// TAC 그림을 자리차지 개체로 되돌릴 때, 텍스트 없는 그림 전용 문단의
    /// LINE_SEG를 남은 TAC 개체 수에 맞춰 재구성한다.
    ///
    /// 기존 false→true 마이그레이션은 첫 LINE_SEG를 그림 높이로 키운다. 반대로
    /// true→false가 되면 그 그림은 더 이상 inline 글자 슬롯이 아니므로, 같은
    /// 문단의 남은 TAC 그림만 빈 줄에 1개씩 매핑되어야 한다. 한컴 저장본
    /// `투명도0-50-2nd그림글차처럼off.hwp`처럼 TopAndBottom 예약 높이는 첫 TAC
    /// 줄의 `vertical_pos`에 반영한다.
    pub(crate) fn migrate_empty_picture_para_inline_to_floating(
        para: &mut crate::model::paragraph::Paragraph,
    ) {
        if !para.text.is_empty() || !para.char_offsets.is_empty() {
            return;
        }

        let old_seg = para.line_segs.first().cloned().unwrap_or_default();
        let line_spacing = if old_seg.line_spacing > 0 {
            old_seg.line_spacing
        } else {
            600
        };
        let reserved_hu = Self::topbottom_reserved_height_for_empty_picture_para(&para.controls);
        let tac_heights = para
            .controls
            .iter()
            .filter_map(Self::tac_control_height_for_empty_picture_para)
            .collect::<Vec<_>>();

        if tac_heights.is_empty() {
            para.line_segs = vec![crate::model::paragraph::LineSeg {
                text_start: 0,
                vertical_pos: reserved_hu,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing,
                segment_width: old_seg.segment_width,
                column_start: old_seg.column_start,
                tag: old_seg.tag,
            }];
            return;
        }

        let mut vpos = reserved_hu;
        let mut rebuilt = Vec::with_capacity(tac_heights.len());
        for (idx, height) in tac_heights.into_iter().enumerate() {
            let line_height = height.max(1);
            rebuilt.push(crate::model::paragraph::LineSeg {
                text_start: (idx as u32) * 8,
                vertical_pos: vpos,
                line_height,
                text_height: line_height,
                baseline_distance: (line_height as f64 * 0.85).round() as i32,
                line_spacing,
                segment_width: old_seg.segment_width,
                column_start: old_seg.column_start,
                tag: old_seg.tag,
            });
            vpos += line_height + line_spacing;
        }
        para.line_segs = rebuilt;
    }

    fn tac_control_height_for_empty_picture_para(ctrl: &Control) -> Option<i32> {
        match ctrl {
            Control::Picture(pic) if pic.common.treat_as_char => Some(pic.common.height as i32),
            Control::Shape(shape) if shape.common().treat_as_char => {
                let common_h = shape.common().height as i32;
                let current_h = shape.shape_attr().current_height as i32;
                Some(common_h.max(current_h))
            }
            Control::Table(table) if table.common.treat_as_char => Some(table.common.height as i32),
            Control::Equation(eq) if eq.common.treat_as_char => Some(eq.common.height as i32),
            _ => None,
        }
    }

    fn topbottom_reserved_height_for_empty_picture_para(controls: &[Control]) -> i32 {
        controls
            .iter()
            .map(|ctrl| match ctrl {
                Control::Picture(pic)
                    if !pic.common.treat_as_char
                        && matches!(
                            pic.common.text_wrap,
                            crate::model::shape::TextWrap::TopAndBottom
                        ) =>
                {
                    pic.common.height as i32
                        + pic.common.margin.top as i32
                        + pic.common.margin.bottom as i32
                }
                Control::Shape(shape)
                    if !shape.common().treat_as_char
                        && matches!(
                            shape.common().text_wrap,
                            crate::model::shape::TextWrap::TopAndBottom
                        ) =>
                {
                    let common = shape.common();
                    common.height as i32 + common.margin.top as i32 + common.margin.bottom as i32
                }
                Control::Table(table)
                    if !table.common.treat_as_char
                        && matches!(
                            table.common.text_wrap,
                            crate::model::shape::TextWrap::TopAndBottom
                        ) =>
                {
                    table.common.height as i32
                        + table.outer_margin_top as i32
                        + table.outer_margin_bottom as i32
                }
                _ => 0,
            })
            .sum()
    }

    /// [Task #1151 v7] cell_path JSON → Vec<(controlIdx, cellIdx, cellParaIdx)>.
    /// 4 개 by_path setter/getter (cell picture/shape × set/get) 의 공통 파싱.
    /// 빈 path 면 Err 반환.
    fn parse_cell_path_json(json: &str) -> Result<Vec<(usize, usize, usize)>, HwpError> {
        let path: Vec<(usize, usize, usize)> = serde_json::from_str::<Vec<serde_json::Value>>(json)
            .map_err(|e| HwpError::RenderError(format!("cell_path JSON 파싱 실패: {}", e)))?
            .iter()
            .map(|v| {
                let c = v
                    .get("controlIdx")
                    .or_else(|| v.get("controlIndex"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0) as usize;
                let ci = v
                    .get("cellIdx")
                    .or_else(|| v.get("cellIndex"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0) as usize;
                let cpi = v
                    .get("cellParaIdx")
                    .or_else(|| v.get("cellParaIndex"))
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0) as usize;
                (c, ci, cpi)
            })
            .collect();
        if path.is_empty() {
            return Err(HwpError::RenderError(
                "cell_path 가 비어있습니다".to_string(),
            ));
        }
        Ok(path)
    }

    fn parse_group_child_path_json(json: &str) -> Result<Vec<usize>, HwpError> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
            HwpError::RenderError(format!("group_child_path JSON 파싱 실패: {}", e))
        })?;
        let Some(items) = value.as_array() else {
            return Err(HwpError::RenderError(
                "group_child_path 는 배열이어야 합니다".to_string(),
            ));
        };
        let mut path = Vec::with_capacity(items.len());
        for item in items {
            let index = item
                .as_u64()
                .or_else(|| item.get("index").and_then(|value| value.as_u64()))
                .or_else(|| item.get("child").and_then(|value| value.as_u64()))
                .or_else(|| item.get("group_child").and_then(|value| value.as_u64()))
                .or_else(|| item.get("groupChild").and_then(|value| value.as_u64()))
                .ok_or_else(|| {
                    HwpError::RenderError(
                        "group_child_path 항목은 숫자 또는 index 필드가 필요합니다".to_string(),
                    )
                })?;
            path.push(index as usize);
        }
        if path.is_empty() {
            return Err(HwpError::RenderError(
                "group_child_path 가 비어있습니다".to_string(),
            ));
        }
        Ok(path)
    }

    fn shape_group_child_ref<'a>(
        shape: &'a ShapeObject,
        path: &[usize],
    ) -> Result<&'a ShapeObject, HwpError> {
        let mut current = shape;
        for (depth, child_idx) in path.iter().copied().enumerate() {
            let ShapeObject::Group(group) = current else {
                return Err(HwpError::RenderError(format!(
                    "group_child_path[{}]의 부모가 ShapeGroup이 아닙니다",
                    depth
                )));
            };
            current = group.children.get(child_idx).ok_or_else(|| {
                HwpError::RenderError(format!(
                    "group_child_path[{}]={} 범위 초과",
                    depth, child_idx
                ))
            })?;
        }
        Ok(current)
    }

    fn shape_group_child_mut<'a>(
        shape: &'a mut ShapeObject,
        path: &[usize],
    ) -> Result<&'a mut ShapeObject, HwpError> {
        let mut current = shape;
        for (depth, child_idx) in path.iter().copied().enumerate() {
            let ShapeObject::Group(group) = current else {
                return Err(HwpError::RenderError(format!(
                    "group_child_path[{}]의 부모가 ShapeGroup이 아닙니다",
                    depth
                )));
            };
            current = group.children.get_mut(child_idx).ok_or_else(|| {
                HwpError::RenderError(format!(
                    "group_child_path[{}]={} 범위 초과",
                    depth, child_idx
                ))
            })?;
        }
        Ok(current)
    }

    fn shape_group_child_picture_ref<'a>(
        shape: &'a ShapeObject,
        path: &[usize],
    ) -> Result<&'a crate::model::image::Picture, HwpError> {
        match Self::shape_group_child_ref(shape, path)? {
            ShapeObject::Picture(picture) => Ok(picture),
            _ => Err(HwpError::RenderError(
                "지정된 ShapeGroup child가 그림이 아닙니다".to_string(),
            )),
        }
    }

    fn shape_group_child_picture_mut<'a>(
        shape: &'a mut ShapeObject,
        path: &[usize],
    ) -> Result<&'a mut crate::model::image::Picture, HwpError> {
        match Self::shape_group_child_mut(shape, path)? {
            ShapeObject::Picture(picture) => Ok(picture),
            _ => Err(HwpError::RenderError(
                "지정된 ShapeGroup child가 그림이 아닙니다".to_string(),
            )),
        }
    }

    fn shape_group_parent_shape_ref(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
    ) -> Result<&ShapeObject, HwpError> {
        if let Some(cell_path_json) = cell_path_json {
            let path = Self::parse_cell_path_json(cell_path_json)?;
            let cell_para = self.resolve_paragraph_by_path(section_idx, parent_para_idx, &path)?;
            let inner_control_idx = inner_control_idx
                .ok_or_else(|| HwpError::RenderError("inner_control 이 필요합니다".to_string()))?;
            let ctrl = cell_para.controls.get(inner_control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
            })?;
            return match ctrl {
                Control::Shape(shape) => Ok(shape.as_ref()),
                _ => Err(HwpError::RenderError(
                    "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                )),
            };
        }

        if let Some(inner_para_idx) = inner_para_idx {
            let section = self.document.sections.get(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", parent_para_idx))
            })?;
            let outer_ctrl = outer_para.controls.get(control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 컨트롤 인덱스 {} 범위 초과", control_idx))
            })?;
            let inner_paras: &[Paragraph] = match outer_ctrl {
                Control::Header(header) => &header.paragraphs,
                Control::Footer(footer) => &footer.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let inner_control_idx = inner_control_idx
                .ok_or_else(|| HwpError::RenderError("inner_control 이 필요합니다".to_string()))?;
            let inner_ctrl = inner_para.controls.get(inner_control_idx).ok_or_else(|| {
                HwpError::RenderError(format!(
                    "내부 컨트롤 인덱스 {} 범위 초과",
                    inner_control_idx
                ))
            })?;
            return match inner_ctrl {
                Control::Shape(shape) => Ok(shape.as_ref()),
                _ => Err(HwpError::RenderError(
                    "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                )),
            };
        }

        self.resolve_shape_control_ref(section_idx, parent_para_idx, control_idx)
    }

    fn shape_group_parent_shape_mut(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
    ) -> Result<&mut ShapeObject, HwpError> {
        if let Some(cell_path_json) = cell_path_json {
            let path = Self::parse_cell_path_json(cell_path_json)?;
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let current_para = Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
            let inner_control_idx = inner_control_idx
                .ok_or_else(|| HwpError::RenderError("inner_control 이 필요합니다".to_string()))?;
            let ctrl = current_para
                .controls
                .get_mut(inner_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
                })?;
            return match ctrl {
                Control::Shape(shape) => Ok(shape.as_mut()),
                _ => Err(HwpError::RenderError(
                    "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                )),
            };
        }

        if let Some(inner_para_idx) = inner_para_idx {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", parent_para_idx))
            })?;
            let outer_ctrl = outer_para.controls.get_mut(control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 컨트롤 인덱스 {} 범위 초과", control_idx))
            })?;
            let inner_paras: &mut Vec<Paragraph> = match outer_ctrl {
                Control::Header(header) => &mut header.paragraphs,
                Control::Footer(footer) => &mut footer.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let inner_control_idx = inner_control_idx
                .ok_or_else(|| HwpError::RenderError("inner_control 이 필요합니다".to_string()))?;
            let inner_ctrl = inner_para
                .controls
                .get_mut(inner_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "내부 컨트롤 인덱스 {} 범위 초과",
                        inner_control_idx
                    ))
                })?;
            return match inner_ctrl {
                Control::Shape(shape) => Ok(shape.as_mut()),
                _ => Err(HwpError::RenderError(
                    "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                )),
            };
        }

        self.resolve_shape_control_mut(section_idx, parent_para_idx, control_idx)
    }

    fn remove_shape_group_child(
        shape: &mut ShapeObject,
        path: &[usize],
    ) -> Result<ShapeObject, HwpError> {
        let Some((&child_idx, parent_path)) = path.split_last() else {
            return Err(HwpError::RenderError(
                "group_child_path 가 비어있습니다".to_string(),
            ));
        };
        let parent = if parent_path.is_empty() {
            shape
        } else {
            Self::shape_group_child_mut(shape, parent_path)?
        };
        let ShapeObject::Group(group) = parent else {
            return Err(HwpError::RenderError(
                "삭제 대상의 부모가 ShapeGroup이 아닙니다".to_string(),
            ));
        };
        if child_idx >= group.children.len() {
            return Err(HwpError::RenderError(format!(
                "group_child_path 마지막 인덱스 {} 범위 초과",
                child_idx
            )));
        }
        Ok(group.children.remove(child_idx))
    }

    fn reorder_shape_group_child(
        shape: &mut ShapeObject,
        path: &[usize],
        operation: &str,
    ) -> Result<(usize, usize, usize), HwpError> {
        let Some((&child_idx, parent_path)) = path.split_last() else {
            return Err(HwpError::RenderError(
                "group_child_path 가 비어있습니다".to_string(),
            ));
        };
        let parent = if parent_path.is_empty() {
            shape
        } else {
            Self::shape_group_child_mut(shape, parent_path)?
        };
        let ShapeObject::Group(group) = parent else {
            return Err(HwpError::RenderError(
                "순서 변경 대상의 부모가 ShapeGroup이 아닙니다".to_string(),
            ));
        };
        let child_count = group.children.len();
        if child_idx >= child_count {
            return Err(HwpError::RenderError(format!(
                "group_child_path 마지막 인덱스 {} 범위 초과",
                child_idx
            )));
        }
        let new_idx = match operation {
            "front" => child_count.saturating_sub(1),
            "back" => 0,
            "forward" => (child_idx + 1).min(child_count.saturating_sub(1)),
            "backward" => child_idx.saturating_sub(1),
            _ => {
                return Err(HwpError::RenderError(format!(
                    "알 수 없는 operation: {}",
                    operation
                )))
            }
        };
        if new_idx != child_idx {
            let child = group.children.remove(child_idx);
            group.children.insert(new_idx, child);
        }
        for (index, child) in group.children.iter_mut().enumerate() {
            child.common_mut().z_order = index as i32;
        }
        Ok((child_idx, new_idx, child_count))
    }

    fn insert_shape_group_child(
        shape: &mut ShapeObject,
        parent_path: Option<&[usize]>,
        mut child: ShapeObject,
        child_index: Option<usize>,
    ) -> Result<(usize, usize), HwpError> {
        let parent = if let Some(path) = parent_path {
            Self::shape_group_child_mut(shape, path)?
        } else {
            shape
        };
        let ShapeObject::Group(group) = parent else {
            return Err(HwpError::RenderError(
                "삽입 대상이 ShapeGroup이 아닙니다".to_string(),
            ));
        };
        let insert_idx = child_index.unwrap_or(group.children.len());
        if insert_idx > group.children.len() {
            return Err(HwpError::RenderError(format!(
                "child_index {} 범위 초과",
                insert_idx
            )));
        }
        let child_group_level = group.shape_attr.group_level.saturating_add(1);
        {
            let common = child.common();
            let new_horz = common.horizontal_offset;
            let new_vert = common.vertical_offset;
            let sa = Self::shape_component_attr_mut(&mut child);
            sa.offset_x = new_horz as i32;
            sa.offset_y = new_vert as i32;
            sa.group_level = child_group_level;
            sa.is_two_ctrl_id = false;
            sa.raw_rendering = Vec::new();
            sa.render_tx = new_horz as f64;
            sa.render_ty = new_vert as f64;
            sa.render_sx = 1.0;
            sa.render_sy = 1.0;
            sa.render_b = 0.0;
            sa.render_c = 0.0;
        }
        group.children.insert(insert_idx, child);
        for (index, child) in group.children.iter_mut().enumerate() {
            child.common_mut().z_order = index as i32;
        }
        Ok((insert_idx, group.children.len()))
    }

    /// [Task #1151 v7] section + parent_para_idx + path → target paragraph (mut).
    /// 2 개 set_cell_*_by_path_native (Picture / Shape) 의 공통 traversal.
    /// immutable 버전은 cursor_nav.rs 의 `resolve_paragraph_by_path` 가 담당하며,
    /// [Task #1171] 이후 표 셀과 글상자(Shape text_box, cell_index=0 sentinel) 를 모두
    /// 처리하도록 immutable 짝과 동일하게 맞춘다.
    fn resolve_cell_paragraph_mut<'a>(
        section: &'a mut crate::model::document::Section,
        parent_para_idx: usize,
        path: &[(usize, usize, usize)],
    ) -> Result<&'a mut crate::model::paragraph::Paragraph, HwpError> {
        let mut current_para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
        })?;
        for (i, &(ctrl_idx, cell_idx, cell_para_idx)) in path.iter().enumerate() {
            let ctrl = current_para.controls.get_mut(ctrl_idx).ok_or_else(|| {
                HwpError::RenderError(format!("경로[{}]: controls[{}] 범위 초과", i, ctrl_idx))
            })?;
            current_para = match ctrl {
                crate::model::control::Control::Table(t) => {
                    let cell = t.cells.get_mut(cell_idx).ok_or_else(|| {
                        HwpError::RenderError(format!("경로[{}]: cells[{}] 범위 초과", i, cell_idx))
                    })?;
                    cell.paragraphs.get_mut(cell_para_idx).ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "경로[{}]: paragraphs[{}] 범위 초과",
                            i, cell_para_idx
                        ))
                    })?
                }
                // [Task #1171] 글상자(Shape text_box) — cell_index=0 sentinel.
                crate::model::control::Control::Shape(shape) => {
                    if cell_idx != 0 {
                        return Err(HwpError::RenderError(format!(
                            "경로[{}]: 글상자의 cell_index는 0이어야 합니다 ({})",
                            i, cell_idx
                        )));
                    }
                    let text_box = get_textbox_from_shape_mut(shape).ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "경로[{}]: controls[{}]가 텍스트 글상자가 아닙니다",
                            i, ctrl_idx
                        ))
                    })?;
                    text_box.paragraphs.get_mut(cell_para_idx).ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "경로[{}]: 글상자문단 {} 범위 초과",
                            i, cell_para_idx
                        ))
                    })?
                }
                _ => {
                    return Err(HwpError::RenderError(format!(
                        "경로[{}]: controls[{}] 가 표/글상자가 아닙니다",
                        i, ctrl_idx
                    )))
                }
            };
        }
        Ok(current_para)
    }

    fn required_cell_height_for_picture(
        cell: &crate::model::table::Cell,
        pic: &crate::model::image::Picture,
    ) -> u32 {
        Self::required_cell_height_for_picture_padding(cell.padding.top, cell.padding.bottom, pic)
    }

    fn required_cell_height_for_picture_padding(
        padding_top: i16,
        padding_bottom: i16,
        pic: &crate::model::image::Picture,
    ) -> u32 {
        let vert_offset = (pic.common.vertical_offset as i32).max(0) as u32;
        let visual_height = if pic.shape_attr.rotation_angle.rem_euclid(360) != 0
            && pic.shape_attr.current_width > 0
            && pic.shape_attr.current_height > 0
        {
            pic.common.height
        } else {
            let (_, height) = Self::picture_rotated_bounds(
                pic.common.width,
                pic.common.height,
                pic.shape_attr.rotation_angle,
            );
            height
        };
        vert_offset
            .saturating_add(visual_height)
            .saturating_add(padding_top.max(0) as u32)
            .saturating_add(padding_bottom.max(0) as u32)
    }

    fn take_place_picture_flow_offset(pic: &crate::model::image::Picture) -> Option<i32> {
        if pic.common.treat_as_char
            || !matches!(
                pic.common.text_wrap,
                crate::model::shape::TextWrap::TopAndBottom
            )
            || !matches!(pic.common.vert_rel_to, crate::model::shape::VertRelTo::Para)
        {
            return None;
        }

        let visual_height = if pic.shape_attr.rotation_angle.rem_euclid(360) != 0
            && pic.shape_attr.current_width > 0
            && pic.shape_attr.current_height > 0
        {
            pic.common.height
        } else {
            let (_, height) = Self::picture_rotated_bounds(
                pic.common.width,
                pic.common.height,
                pic.shape_attr.rotation_angle,
            );
            height
        };
        Some(
            (pic.common.vertical_offset as i32)
                .saturating_add(visual_height.min(i32::MAX as u32) as i32)
                .max(0),
        )
    }

    fn sync_direct_owner_cell_for_picture(
        section: &mut crate::model::document::Section,
        parent_para_idx: usize,
        path: &[(usize, usize, usize)],
        inner_control_idx: usize,
    ) -> Result<(), HwpError> {
        if path.len() != 1 {
            return Ok(());
        }

        let (table_ctrl_idx, cell_idx, cell_para_idx) = path[0];
        let para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
        })?;
        let existing_line_height = para
            .line_segs
            .first()
            .map(|seg| seg.line_height)
            .unwrap_or(0);
        let table = match para.controls.get_mut(table_ctrl_idx) {
            Some(Control::Table(table)) => table,
            _ => return Ok(()),
        };
        let line_height_extra = (existing_line_height - table.common.height as i32).max(0);
        let mut line_seg_update: Option<(i32, i32)> = None;

        let required_height = {
            let cell = table.cells.get(cell_idx).ok_or_else(|| {
                HwpError::RenderError(format!("경로[0]: cells[{}] 범위 초과", cell_idx))
            })?;
            let cell_para = cell.paragraphs.get(cell_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("경로[0]: paragraphs[{}] 범위 초과", cell_para_idx))
            })?;
            let pic = match cell_para.controls.get(inner_control_idx) {
                Some(Control::Picture(pic)) => pic,
                _ => return Ok(()),
            };
            let take_place_flow_offset = Self::take_place_picture_flow_offset(pic);
            if table.common.treat_as_char {
                if let Some(flow_offset) = take_place_flow_offset {
                    let vertical_pos = if pic.common.flow_with_text {
                        0
                    } else {
                        flow_offset
                    };
                    line_seg_update = Some((vertical_pos, line_height_extra));
                }
            }
            if pic.common.flow_with_text {
                Some(Self::required_cell_height_for_picture(cell, pic))
            } else {
                None
            }
        };

        if let (Some(required_height), Some(cell)) =
            (required_height, table.cells.get_mut(cell_idx))
        {
            let synced_height = required_height.max(MIN_SHAPE_SIZE);
            if cell.height != synced_height {
                cell.height = synced_height;
            }
        }
        table.update_ctrl_dimensions();
        table.dirty = true;
        let new_table_height = table.common.height as i32;
        if let Some((vertical_pos, line_height_extra)) = line_seg_update {
            if let Some(seg) = para.line_segs.first_mut() {
                let line_height = new_table_height
                    .saturating_add(line_height_extra)
                    .max(MIN_SHAPE_SIZE as i32);
                seg.vertical_pos = vertical_pos;
                seg.line_height = line_height;
                seg.text_height = line_height;
                seg.baseline_distance =
                    ((line_height as i64 * 17 + 10) / 20).min(i32::MAX as i64) as i32;
            }
        }
        Ok(())
    }

    fn clamp_direct_owner_cell_picture_offsets(
        section: &mut crate::model::document::Section,
        parent_para_idx: usize,
        path: &[(usize, usize, usize)],
        inner_control_idx: usize,
        clamp_horz: bool,
        clamp_vert: bool,
    ) -> Result<(), HwpError> {
        if path.len() != 1 || (!clamp_horz && !clamp_vert) {
            return Ok(());
        }

        let (table_ctrl_idx, cell_idx, cell_para_idx) = path[0];
        let para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
        })?;
        let table = match para.controls.get_mut(table_ctrl_idx) {
            Some(Control::Table(table)) => table,
            _ => return Ok(()),
        };
        let cell = table.cells.get_mut(cell_idx).ok_or_else(|| {
            HwpError::RenderError(format!("경로[0]: cells[{}] 범위 초과", cell_idx))
        })?;

        let inner_width = cell
            .width
            .saturating_sub(cell.padding.left.max(0) as u32)
            .saturating_sub(cell.padding.right.max(0) as u32) as i64;
        let cell_para = cell.paragraphs.get_mut(cell_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("경로[0]: paragraphs[{}] 범위 초과", cell_para_idx))
        })?;
        let pic = match cell_para.controls.get_mut(inner_control_idx) {
            Some(Control::Picture(pic)) => pic,
            _ => return Ok(()),
        };

        if !pic.common.flow_with_text {
            return Ok(());
        }

        if clamp_horz {
            let max_horz = (inner_width - pic.common.width as i64)
                .max(0)
                .min(i32::MAX as i64);
            let horz = (pic.common.horizontal_offset as i32).clamp(0, max_horz as i32);
            pic.common.horizontal_offset = horz as u32;
        }
        if clamp_vert {
            let vert = (pic.common.vertical_offset as i32).max(0);
            pic.common.vertical_offset = vert as u32;
        }
        Ok(())
    }

    /// path 의 마지막 엔트리가 글상자(Shape text_box)를 가리키는지 판정한다.
    ///
    /// 표 셀 picture 삽입은 한컴 정합상 parent paragraph 의 sibling floating
    /// picture 로 처리하지만, 글상자 내부 picture 는 text_box paragraph 안에
    /// 실제 Picture control 로 들어가야 한다. `resolve_cell_by_path` 는 마지막
    /// 엔트리가 표일 때만 성공하므로, insert path 에서는 표/글상자를 먼저 구분한다.
    fn cell_path_terminates_at_textbox(
        section: &crate::model::document::Section,
        parent_para_idx: usize,
        path: &[(usize, usize, usize)],
    ) -> Result<bool, HwpError> {
        let mut current_para = section.paragraphs.get(parent_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
        })?;

        for (i, &(ctrl_idx, cell_idx, cell_para_idx)) in path.iter().enumerate() {
            let ctrl = current_para.controls.get(ctrl_idx).ok_or_else(|| {
                HwpError::RenderError(format!("경로[{}]: controls[{}] 범위 초과", i, ctrl_idx))
            })?;
            match ctrl {
                crate::model::control::Control::Table(table) => {
                    let cell = table.cells.get(cell_idx).ok_or_else(|| {
                        HwpError::RenderError(format!("경로[{}]: cells[{}] 범위 초과", i, cell_idx))
                    })?;
                    if i == path.len() - 1 {
                        return Ok(false);
                    }
                    current_para = cell.paragraphs.get(cell_para_idx).ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "경로[{}]: paragraphs[{}] 범위 초과",
                            i, cell_para_idx
                        ))
                    })?;
                }
                crate::model::control::Control::Shape(shape) => {
                    if cell_idx != 0 {
                        return Err(HwpError::RenderError(format!(
                            "경로[{}]: 글상자의 cell_index는 0이어야 합니다 ({})",
                            i, cell_idx
                        )));
                    }
                    let text_box = get_textbox_from_shape(shape.as_ref()).ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "경로[{}]: controls[{}]가 텍스트 글상자가 아닙니다",
                            i, ctrl_idx
                        ))
                    })?;
                    if i == path.len() - 1 {
                        return Ok(true);
                    }
                    current_para = text_box.paragraphs.get(cell_para_idx).ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "경로[{}]: 글상자문단 {} 범위 초과",
                            i, cell_para_idx
                        ))
                    })?;
                }
                _ => {
                    return Err(HwpError::RenderError(format!(
                        "경로[{}]: controls[{}] 가 표/글상자가 아닙니다",
                        i, ctrl_idx
                    )))
                }
            }
        }

        Err(HwpError::RenderError("경로가 비어있습니다".to_string()))
    }

    /// [Task #825] Picture 속성 JSON 적용 (mutation only). 후처리 (AutoNumber /
    /// recompose / paginate / event log) 는 호출자 책임.
    /// 반환: caption_created (true 면 호출자가 AutoNumber 후처리 필요).
    fn apply_picture_props_inner(pic: &mut crate::model::image::Picture, props_json: &str) -> bool {
        use super::super::helpers::{json_bool, json_i16, json_i32, json_str, json_u32};

        let transform_changed = Self::picture_props_touch_shape_transform(props_json);
        let mut rotation_changed = false;

        // 크기 변경
        if let Some(w) = json_u32(props_json, "width") {
            Self::apply_picture_display_width(pic, w);
        }
        if let Some(h) = json_u32(props_json, "height") {
            Self::apply_picture_display_height(pic, h);
        }

        // 위치 속성
        if let Some(tac) = json_bool(props_json, "treatAsChar") {
            pic.common.treat_as_char = tac;
            // attr 비트 갱신
            if tac {
                pic.common.attr |= 0x01;
            } else {
                pic.common.attr &= !0x01;
            }
        }
        if let Some(v) = json_str(props_json, "vertRelTo") {
            pic.common.vert_rel_to = match v.as_str() {
                "Paper" => crate::model::shape::VertRelTo::Paper,
                "Page" => crate::model::shape::VertRelTo::Page,
                "Para" => crate::model::shape::VertRelTo::Para,
                _ => pic.common.vert_rel_to,
            };
        }
        if let Some(v) = json_str(props_json, "horzRelTo") {
            pic.common.horz_rel_to = match v.as_str() {
                "Paper" => crate::model::shape::HorzRelTo::Paper,
                "Page" => crate::model::shape::HorzRelTo::Page,
                "Column" => crate::model::shape::HorzRelTo::Column,
                "Para" => crate::model::shape::HorzRelTo::Para,
                _ => pic.common.horz_rel_to,
            };
        }
        if let Some(v) = json_str(props_json, "vertAlign") {
            pic.common.vert_align = match v.as_str() {
                "Top" => crate::model::shape::VertAlign::Top,
                "Center" => crate::model::shape::VertAlign::Center,
                "Bottom" => crate::model::shape::VertAlign::Bottom,
                _ => pic.common.vert_align,
            };
        }
        if let Some(v) = json_str(props_json, "horzAlign") {
            pic.common.horz_align = match v.as_str() {
                "Left" => crate::model::shape::HorzAlign::Left,
                "Center" => crate::model::shape::HorzAlign::Center,
                "Right" => crate::model::shape::HorzAlign::Right,
                _ => pic.common.horz_align,
            };
        }
        if let Some(v) = json_str(props_json, "textWrap") {
            pic.common.text_wrap = match v.as_str() {
                "Square" => crate::model::shape::TextWrap::Square,
                "Tight" => crate::model::shape::TextWrap::Tight,
                "Through" => crate::model::shape::TextWrap::Through,
                "TopAndBottom" => crate::model::shape::TextWrap::TopAndBottom,
                "BehindText" => crate::model::shape::TextWrap::BehindText,
                "InFrontOfText" => crate::model::shape::TextWrap::InFrontOfText,
                _ => pic.common.text_wrap,
            };
        }
        if let Some(v) = Self::json_str_field_any(props_json, &["textFlow", "text_flow"]) {
            pic.common.text_flow = Self::text_flow_from_json_name(&v, pic.common.text_flow);
        }
        if let Some(v) = Self::json_str_field_any(props_json, &["numberingType", "numbering_type"])
        {
            pic.common.numbering_type =
                Self::object_numbering_type_from_json_name(&v, pic.common.numbering_type);
            pic.common.numbering_type_explicit = true;
        }
        if let Some(v) = Self::json_bool_field_any(
            props_json,
            &["numberingTypeExplicit", "numbering_type_explicit"],
        ) {
            pic.common.numbering_type_explicit = v;
        }
        if let Some(v) = Self::json_bool_field_any(props_json, &["lock", "locked"]) {
            pic.common.lock = v;
        }
        if let Some(v) =
            Self::json_str_field_any(props_json, &["widthCriterion", "width_criterion"])
        {
            pic.common.width_criterion =
                Self::size_criterion_from_json_name(&v, pic.common.width_criterion);
        }
        if let Some(v) =
            Self::json_str_field_any(props_json, &["heightCriterion", "height_criterion"])
        {
            pic.common.height_criterion =
                Self::size_criterion_from_json_name(&v, pic.common.height_criterion);
        }
        if let Some(v) = Self::json_i32_field_any(props_json, &["zOrder", "z_order"]) {
            pic.common.z_order = v;
        }
        if let Some(v) = Self::json_u32_field_any(props_json, &["instanceId", "instance_id"]) {
            pic.common.instance_id = v;
        }
        if let Some(v) = Self::json_u32_field_any(
            props_json,
            &[
                "instId",
                "inst_id",
                "pictureInstanceId",
                "picture_instance_id",
            ],
        ) {
            pic.instance_id = v;
        }
        if let Some(v) = Self::json_u32_field_any(props_json, &["groupLevel", "group_level"]) {
            pic.shape_attr.group_level = v.min(u16::MAX as u32) as u16;
        }
        if let Some(v) = Self::json_str_field_any(props_json, &["href"]) {
            pic.href = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(v) = json_bool(props_json, "restrictInPage") {
            pic.common.flow_with_text = v;
            if v {
                pic.common.attr |= 1 << 13;
                pic.common.allow_overlap = false;
                pic.common.attr &= !(1 << 14);
            } else {
                pic.common.attr &= !(1 << 13);
            }
        }
        if let Some(v) = json_bool(props_json, "allowOverlap") {
            pic.common.allow_overlap = v;
            if v {
                pic.common.attr |= 1 << 14;
            } else {
                pic.common.attr &= !(1 << 14);
            }
        }
        if let Some(v) = json_bool(props_json, "sizeProtect") {
            pic.common.size_protect = v;
            if v {
                pic.common.attr |= 1 << 20;
            } else {
                pic.common.attr &= !(1 << 20);
            }
        }
        if pic.common.flow_with_text {
            pic.common.allow_overlap = false;
            pic.common.attr &= !(1 << 14);
        }
        if let Some(v) = json_i32(props_json, "vertOffset") {
            pic.common.vertical_offset = v as u32;
        }
        if let Some(v) = json_i32(props_json, "horzOffset") {
            pic.common.horizontal_offset = v as u32;
        }
        if let Some(v) =
            json_str(props_json, "dropcapStyle").or_else(|| json_str(props_json, "dropcap_style"))
        {
            pic.common.dropcap_style = if v.is_empty() || v == "None" {
                None
            } else {
                Some(v)
            };
        }
        Self::sync_common_obj_attr_known_bits(&mut pic.common);
        if transform_changed {
            pic.shape_attr.raw_rendering.clear();
            pic.shape_attr.render_tx = pic.shape_attr.offset_x as f64;
            pic.shape_attr.render_ty = pic.shape_attr.offset_y as f64;
            pic.shape_attr.render_sx = 1.0;
            pic.shape_attr.render_sy = 1.0;
            pic.shape_attr.render_b = 0.0;
            pic.shape_attr.render_c = 0.0;
        }

        // 이미지 속성
        if let Some(v) = json_i32(props_json, "brightness") {
            pic.image_attr.brightness = v as i8;
        }
        if let Some(v) = json_i32(props_json, "contrast") {
            pic.image_attr.contrast = v as i8;
        }
        if let Some(v) = json_i32(props_json, "transparency") {
            pic.image_attr.transparency = v.clamp(0, 100) as u8;
        }
        if let Some(v) = json_str(props_json, "effect") {
            pic.image_attr.effect = match v.as_str() {
                "GrayScale" => crate::model::image::ImageEffect::GrayScale,
                "BlackWhite" => crate::model::image::ImageEffect::BlackWhite,
                "Pattern8x8" => crate::model::image::ImageEffect::Pattern8x8,
                _ => crate::model::image::ImageEffect::RealPic,
            };
        }
        Self::apply_picture_shadow_props(pic, props_json);
        Self::apply_picture_glow_props(pic, props_json);
        Self::apply_picture_soft_edge_props(pic, props_json);
        Self::apply_picture_reflection_props(pic, props_json);
        Self::apply_picture_three_d_props(pic, props_json);
        Self::apply_picture_blur_props(pic, props_json);
        Self::apply_picture_fill_overlay_props(pic, props_json);
        Self::apply_picture_raw_effects_props(pic, props_json);

        // 회전/대칭
        if let Some(v) = Self::json_i16_field_any(props_json, &["rotationAngle", "rotation_angle"])
        {
            pic.shape_attr.rotation_angle = v;
            rotation_changed = true;
        }
        if let Some(v) = Self::json_bool_field_any(props_json, &["rotateImage", "rotate_image"]) {
            pic.shape_attr.rotate_image = v;
            if v {
                pic.shape_attr.flip |= 0x0008_0000;
            } else {
                pic.shape_attr.flip &= !0x0008_0000;
            }
        }
        if let Some(v) = Self::json_bool_field_any(props_json, &["horzFlip", "horz_flip"]) {
            pic.shape_attr.horz_flip = v;
            if v {
                pic.shape_attr.flip |= 0x01;
            } else {
                pic.shape_attr.flip &= !0x01;
            }
        }
        if let Some(v) = Self::json_bool_field_any(props_json, &["vertFlip", "vert_flip"]) {
            pic.shape_attr.vert_flip = v;
            if v {
                pic.shape_attr.flip |= 0x02;
            } else {
                pic.shape_attr.flip &= !0x02;
            }
        }
        if rotation_changed {
            Self::refresh_picture_rotation_layout_for_save(pic);
        }

        // 자르기: HWP 내부 crop은 원본 이미지의 source rect 좌표이고,
        // 속성 창 UI는 네 방향에서 잘라낸 양을 표시한다.
        let crop_left = Self::json_i32_field_any(props_json, &["cropLeft", "crop_left"]);
        let crop_top = Self::json_i32_field_any(props_json, &["cropTop", "crop_top"]);
        let crop_right = Self::json_i32_field_any(props_json, &["cropRight", "crop_right"]);
        let crop_bottom = Self::json_i32_field_any(props_json, &["cropBottom", "crop_bottom"]);
        if crop_left.is_some()
            || crop_top.is_some()
            || crop_right.is_some()
            || crop_bottom.is_some()
        {
            let (mut left, mut top, mut right, mut bottom) = Self::picture_crop_ui_amounts(pic);
            if let Some(v) = crop_left {
                left = v;
            }
            if let Some(v) = crop_top {
                top = v;
            }
            if let Some(v) = crop_right {
                right = v;
            }
            if let Some(v) = crop_bottom {
                bottom = v;
            }
            Self::set_picture_crop_from_ui_amounts(pic, left, top, right, bottom);
        }

        // 안쪽 여백 (그림 여백)
        if let Some(v) = Self::json_i16_field_any(props_json, &["paddingLeft", "padding_left"]) {
            pic.padding.left = v;
        }
        if let Some(v) = Self::json_i16_field_any(props_json, &["paddingTop", "padding_top"]) {
            pic.padding.top = v;
        }
        if let Some(v) = Self::json_i16_field_any(props_json, &["paddingRight", "padding_right"]) {
            pic.padding.right = v;
        }
        if let Some(v) = Self::json_i16_field_any(props_json, &["paddingBottom", "padding_bottom"])
        {
            pic.padding.bottom = v;
        }

        // 바깥 여백
        if let Some(v) =
            Self::json_i16_field_any(props_json, &["outerMarginLeft", "outer_margin_left"])
        {
            pic.common.margin.left = v;
        }
        if let Some(v) =
            Self::json_i16_field_any(props_json, &["outerMarginTop", "outer_margin_top"])
        {
            pic.common.margin.top = v;
        }
        if let Some(v) =
            Self::json_i16_field_any(props_json, &["outerMarginRight", "outer_margin_right"])
        {
            pic.common.margin.right = v;
        }
        if let Some(v) =
            Self::json_i16_field_any(props_json, &["outerMarginBottom", "outer_margin_bottom"])
        {
            pic.common.margin.bottom = v;
        }

        // 테두리. serde 기반으로도 읽어 props JSON string 입력의 공백/순서 차이를 허용한다.
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(props_json) {
            if let Some(v) =
                Self::object_css_color_ref_field(&value, &["borderColorHex", "border_color_hex"])
            {
                pic.border_color = v;
            } else if let Some(v) = Self::object_u32_field(&value, &["borderColor", "border_color"])
            {
                pic.border_color = v;
            }
            if let Some(v) = Self::object_i32_field(&value, &["borderWidth", "border_width"]) {
                pic.border_width = v;
            }
        } else {
            if let Some(v) = Self::json_css_color_ref_field_any(
                props_json,
                &["borderColorHex", "border_color_hex"],
            ) {
                pic.border_color = v;
            } else if let Some(v) =
                Self::json_u32_field_any(props_json, &["borderColor", "border_color"])
            {
                pic.border_color = v;
            }
            if let Some(v) = Self::json_i32_field_any(props_json, &["borderWidth", "border_width"])
            {
                pic.border_width = v;
            }
        }
        pic.border_attr.color = pic.border_color;
        pic.border_attr.width = pic.border_width;

        // description
        if let Some(v) = json_str(props_json, "description") {
            pic.common.description = v;
        }

        let mut caption_created = false;
        let caption_text =
            json_str(props_json, "captionText").or_else(|| json_str(props_json, "caption_text"));

        // 캡션
        if Self::json_bool_field_any(props_json, &["hasCaption", "has_caption"]) == Some(true)
            || caption_text.is_some()
        {
            let wants_auto_number_caption =
                Self::json_bool_field_any(props_json, &["hasCaption", "has_caption"]) == Some(true)
                    && caption_text.is_none();
            if pic.caption.is_none() {
                let mut cap = crate::model::shape::Caption::default();
                cap.paragraphs
                    .push(crate::model::paragraph::Paragraph::default());
                cap.max_width = pic.common.width;
                if wants_auto_number_caption {
                    let an = crate::model::control::AutoNumber {
                        number_type: crate::model::control::AutoNumberType::Picture,
                        ..Default::default()
                    };
                    cap.paragraphs[0]
                        .controls
                        .push(crate::model::control::Control::AutoNumber(an));
                }
                pic.caption = Some(cap);
                caption_created = true;
                pic.common.attr |= 1 << 29;
            }
            let cap = pic.caption.as_mut().unwrap();
            if let Some(v) =
                Self::json_str_field_any(props_json, &["captionDirection", "caption_direction"])
            {
                cap.direction = match v.as_str() {
                    "Left" => crate::model::shape::CaptionDirection::Left,
                    "Right" => crate::model::shape::CaptionDirection::Right,
                    "Top" => crate::model::shape::CaptionDirection::Top,
                    _ => crate::model::shape::CaptionDirection::Bottom,
                };
            }
            if let Some(v) =
                Self::json_str_field_any(props_json, &["captionVertAlign", "caption_vert_align"])
            {
                cap.vert_align = match v.as_str() {
                    "Center" => crate::model::shape::CaptionVertAlign::Center,
                    "Bottom" => crate::model::shape::CaptionVertAlign::Bottom,
                    _ => crate::model::shape::CaptionVertAlign::Top,
                };
            }
            if let Some(v) =
                Self::json_u32_field_any(props_json, &["captionWidth", "caption_width"])
            {
                cap.width = v;
            }
            if let Some(v) =
                Self::json_i16_field_any(props_json, &["captionSpacing", "caption_spacing"])
            {
                cap.spacing = v;
            }
            if let Some(v) =
                Self::json_u32_field_any(props_json, &["captionMaxWidth", "caption_max_width"])
            {
                cap.max_width = v;
            }
            if let Some(v) = Self::json_bool_field_any(
                props_json,
                &["captionIncludeMargin", "caption_include_margin"],
            ) {
                cap.include_margin = v;
            }
            if let Some(text) = caption_text {
                Self::set_picture_caption_literal_text(pic, text);
            }
        } else if let Some(has_cap) =
            Self::json_bool_field_any(props_json, &["hasCaption", "has_caption"])
        {
            if has_cap {
                // 캡션이 없으면 새로 생성 (기본 문단 포함)
                if pic.caption.is_none() {
                    let mut cap = crate::model::shape::Caption::default();
                    // AutoNumber 컨트롤 생성 (번호 할당은 아래에서)
                    let an = crate::model::control::AutoNumber {
                        number_type: crate::model::control::AutoNumberType::Picture,
                        ..Default::default()
                    };
                    cap.paragraphs
                        .push(crate::model::paragraph::Paragraph::default());
                    // 캡션 텍스트 최대 폭 = 개체 폭
                    cap.max_width = pic.common.width;
                    pic.caption = Some(cap);
                    caption_created = true;
                    // 번호 할당을 위해 컨트롤을 임시로 캡션에 추가
                    pic.caption.as_mut().unwrap().paragraphs[0]
                        .controls
                        .push(crate::model::control::Control::AutoNumber(an));
                    // attr bit 29: 캡션 존재 플래그 (한컴 호환성)
                    pic.common.attr |= 1 << 29;
                }
                let cap = pic.caption.as_mut().unwrap();
                if let Some(v) =
                    Self::json_str_field_any(props_json, &["captionDirection", "caption_direction"])
                {
                    cap.direction = match v.as_str() {
                        "Left" => crate::model::shape::CaptionDirection::Left,
                        "Right" => crate::model::shape::CaptionDirection::Right,
                        "Top" => crate::model::shape::CaptionDirection::Top,
                        _ => crate::model::shape::CaptionDirection::Bottom,
                    };
                }
                if let Some(v) = Self::json_str_field_any(
                    props_json,
                    &["captionVertAlign", "caption_vert_align"],
                ) {
                    cap.vert_align = match v.as_str() {
                        "Center" => crate::model::shape::CaptionVertAlign::Center,
                        "Bottom" => crate::model::shape::CaptionVertAlign::Bottom,
                        _ => crate::model::shape::CaptionVertAlign::Top,
                    };
                }
                if let Some(v) =
                    Self::json_u32_field_any(props_json, &["captionWidth", "caption_width"])
                {
                    cap.width = v;
                }
                if let Some(v) =
                    Self::json_i16_field_any(props_json, &["captionSpacing", "caption_spacing"])
                {
                    cap.spacing = v;
                }
                if let Some(v) =
                    Self::json_u32_field_any(props_json, &["captionMaxWidth", "caption_max_width"])
                {
                    cap.max_width = v;
                }
                if let Some(v) = Self::json_bool_field_any(
                    props_json,
                    &["captionIncludeMargin", "caption_include_margin"],
                ) {
                    cap.include_margin = v;
                }
            } else {
                // 캡션 제거 — 현재는 None 처리하지 않음 (캡션에 텍스트가 있을 수 있으므로)
            }
        }

        caption_created
    }

    fn finish_new_picture_caption(pic: &mut crate::model::image::Picture) {
        if let Some(caption) = pic.caption.as_mut() {
            if let Some(para) = caption.paragraphs.first_mut() {
                if !para.text.is_empty() {
                    return;
                }
                para.text = "그림  ".to_string();
                para.char_offsets = vec![0, 1, 2, 11];
                para.char_count = 13;
            }
        }
    }

    fn set_picture_caption_literal_text(pic: &mut crate::model::image::Picture, text: String) {
        let caption = pic.caption.get_or_insert_with(|| {
            let mut caption = crate::model::shape::Caption::default();
            caption.max_width = pic.common.width;
            caption
                .paragraphs
                .push(crate::model::paragraph::Paragraph::default());
            caption
        });
        if caption.paragraphs.is_empty() {
            caption
                .paragraphs
                .push(crate::model::paragraph::Paragraph::default());
        }
        let para = &mut caption.paragraphs[0];
        para.text = text;
        let char_len = para.text.chars().count();
        para.char_offsets = (0..char_len).map(|i| i as u32).collect();
        para.char_count = char_len as u32 + 1;
        para.controls.clear();
        para.ctrl_data_records.clear();
        para.field_ranges.clear();
        para.has_para_text = true;
        pic.common.attr |= 1 << 29;
    }

    fn shape_caption_ref(
        shape: &crate::model::shape::ShapeObject,
    ) -> Option<&crate::model::shape::Caption> {
        match shape {
            crate::model::shape::ShapeObject::Line(s) => s.drawing.caption.as_ref(),
            crate::model::shape::ShapeObject::Rectangle(s) => s.drawing.caption.as_ref(),
            crate::model::shape::ShapeObject::Ellipse(s) => s.drawing.caption.as_ref(),
            crate::model::shape::ShapeObject::Arc(s) => s.drawing.caption.as_ref(),
            crate::model::shape::ShapeObject::Polygon(s) => s.drawing.caption.as_ref(),
            crate::model::shape::ShapeObject::Curve(s) => s.drawing.caption.as_ref(),
            crate::model::shape::ShapeObject::Group(s) => s.caption.as_ref(),
            crate::model::shape::ShapeObject::Picture(s) => s.caption.as_ref(),
            crate::model::shape::ShapeObject::Chart(s) => s.caption.as_ref(),
            crate::model::shape::ShapeObject::Ole(s) => s.caption.as_ref(),
        }
    }

    fn shape_caption_slot_mut(
        shape: &mut crate::model::shape::ShapeObject,
    ) -> &mut Option<crate::model::shape::Caption> {
        match shape {
            crate::model::shape::ShapeObject::Line(s) => &mut s.drawing.caption,
            crate::model::shape::ShapeObject::Rectangle(s) => &mut s.drawing.caption,
            crate::model::shape::ShapeObject::Ellipse(s) => &mut s.drawing.caption,
            crate::model::shape::ShapeObject::Arc(s) => &mut s.drawing.caption,
            crate::model::shape::ShapeObject::Polygon(s) => &mut s.drawing.caption,
            crate::model::shape::ShapeObject::Curve(s) => &mut s.drawing.caption,
            crate::model::shape::ShapeObject::Group(s) => &mut s.caption,
            crate::model::shape::ShapeObject::Picture(s) => &mut s.caption,
            crate::model::shape::ShapeObject::Chart(s) => &mut s.caption,
            crate::model::shape::ShapeObject::Ole(s) => &mut s.caption,
        }
    }

    fn caption_direction_str(direction: crate::model::shape::CaptionDirection) -> &'static str {
        match direction {
            crate::model::shape::CaptionDirection::Left => "Left",
            crate::model::shape::CaptionDirection::Right => "Right",
            crate::model::shape::CaptionDirection::Top => "Top",
            crate::model::shape::CaptionDirection::Bottom => "Bottom",
        }
    }

    fn caption_vert_align_str(vert_align: crate::model::shape::CaptionVertAlign) -> &'static str {
        match vert_align {
            crate::model::shape::CaptionVertAlign::Top => "Top",
            crate::model::shape::CaptionVertAlign::Center => "Center",
            crate::model::shape::CaptionVertAlign::Bottom => "Bottom",
        }
    }

    fn shape_caption_field(shape: &crate::model::shape::ShapeObject) -> String {
        let caption = Self::shape_caption_ref(shape);
        let caption_text = caption
            .and_then(|caption| caption.paragraphs.first())
            .map(|para| super::super::helpers::json_escape(&para.text))
            .unwrap_or_default();
        format!(
            ",\"hasCaption\":{},\"captionDirection\":\"{}\",\"captionVertAlign\":\"{}\",\
             \"captionWidth\":{},\"captionSpacing\":{},\"captionMaxWidth\":{},\"captionIncludeMargin\":{},\
             \"captionText\":\"{}\"",
            caption.is_some(),
            caption.map_or("Bottom", |cap| Self::caption_direction_str(cap.direction)),
            caption.map_or("Top", |cap| Self::caption_vert_align_str(cap.vert_align)),
            caption.map_or(0u32, |cap| cap.width),
            caption.map_or(0i16, |cap| cap.spacing),
            caption.map_or(0u32, |cap| cap.max_width),
            caption.map_or(false, |cap| cap.include_margin),
            caption_text
        )
    }

    fn finish_new_shape_caption(caption: &mut crate::model::shape::Caption) {
        if let Some(para) = caption.paragraphs.first_mut() {
            if !para.text.is_empty() {
                return;
            }
            para.text = "그림  ".to_string();
            para.char_offsets = vec![0, 1, 2, 11];
            para.char_count = 13;
        }
    }

    fn set_shape_caption_literal_text(caption: &mut crate::model::shape::Caption, text: String) {
        if caption.paragraphs.is_empty() {
            caption
                .paragraphs
                .push(crate::model::paragraph::Paragraph::default());
        }
        let para = &mut caption.paragraphs[0];
        para.text = text;
        let char_len = para.text.chars().count();
        para.char_offsets = (0..char_len).map(|i| i as u32).collect();
        para.char_count = char_len as u32 + 1;
        para.controls.clear();
        para.ctrl_data_records.clear();
        para.field_ranges.clear();
        para.has_para_text = true;
    }

    fn apply_shape_caption_props(shape: &mut crate::model::shape::ShapeObject, props_json: &str) {
        let caption_text = Self::json_str_field_any(props_json, &["captionText", "caption_text"]);
        let has_caption = Self::json_bool_field_any(props_json, &["hasCaption", "has_caption"]);
        if has_caption == Some(false) && caption_text.is_none() {
            *Self::shape_caption_slot_mut(shape) = None;
            shape.common_mut().attr &= !(1 << 29);
            return;
        }
        if has_caption != Some(true) && caption_text.is_none() {
            return;
        }

        let object_width = shape.common().width;
        let wants_auto_number_caption = has_caption == Some(true) && caption_text.is_none();
        let caption_created;
        {
            let caption_slot = Self::shape_caption_slot_mut(shape);
            caption_created = caption_slot.is_none();
            let caption = caption_slot.get_or_insert_with(|| {
                let mut cap = crate::model::shape::Caption::default();
                cap.paragraphs
                    .push(crate::model::paragraph::Paragraph::default());
                cap.max_width = object_width;
                if wants_auto_number_caption {
                    cap.paragraphs[0]
                        .controls
                        .push(crate::model::control::Control::AutoNumber(
                            crate::model::control::AutoNumber {
                                number_type: crate::model::control::AutoNumberType::Picture,
                                ..Default::default()
                            },
                        ));
                }
                cap
            });
            if let Some(v) =
                Self::json_str_field_any(props_json, &["captionDirection", "caption_direction"])
            {
                caption.direction = match v.as_str() {
                    "Left" => crate::model::shape::CaptionDirection::Left,
                    "Right" => crate::model::shape::CaptionDirection::Right,
                    "Top" => crate::model::shape::CaptionDirection::Top,
                    _ => crate::model::shape::CaptionDirection::Bottom,
                };
            }
            if let Some(v) =
                Self::json_str_field_any(props_json, &["captionVertAlign", "caption_vert_align"])
            {
                caption.vert_align = match v.as_str() {
                    "Center" => crate::model::shape::CaptionVertAlign::Center,
                    "Bottom" => crate::model::shape::CaptionVertAlign::Bottom,
                    _ => crate::model::shape::CaptionVertAlign::Top,
                };
            }
            if let Some(v) =
                Self::json_u32_field_any(props_json, &["captionWidth", "caption_width"])
            {
                caption.width = v;
            }
            if let Some(v) =
                Self::json_i16_field_any(props_json, &["captionSpacing", "caption_spacing"])
            {
                caption.spacing = v;
            }
            if let Some(v) =
                Self::json_u32_field_any(props_json, &["captionMaxWidth", "caption_max_width"])
            {
                caption.max_width = v;
            }
            if let Some(v) = Self::json_bool_field_any(
                props_json,
                &["captionIncludeMargin", "caption_include_margin"],
            ) {
                caption.include_margin = v;
            }
            if let Some(text) = caption_text {
                Self::set_shape_caption_literal_text(caption, text);
            } else if caption_created {
                Self::finish_new_shape_caption(caption);
            }
        }
        shape.common_mut().attr |= 1 << 29;
    }

    /// 그림 컨트롤을 문단에서 삭제한다 (네이티브).
    pub fn delete_picture_control_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<String, HwpError> {
        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        let section = &mut self.document.sections[section_idx];
        if parent_para_idx >= section.paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "부모 문단 인덱스 {} 범위 초과",
                parent_para_idx
            )));
        }
        let para = &mut section.paragraphs[parent_para_idx];
        if control_idx >= para.controls.len() {
            return Err(HwpError::RenderError(format!(
                "컨트롤 인덱스 {} 범위 초과",
                control_idx
            )));
        }
        // 그림 컨트롤인지 확인
        if !matches!(
            &para.controls[control_idx],
            crate::model::control::Control::Picture(_)
        ) {
            return Err(HwpError::RenderError(
                "지정된 컨트롤이 그림이 아닙니다".to_string(),
            ));
        }

        // 컨트롤이 차지하는 갭의 시작 위치를 찾아 char_offsets 조정
        let text_chars: Vec<char> = para.text.chars().collect();
        let mut ci = 0usize;
        let mut prev_end: u32 = 0;
        let mut gap_start: Option<u32> = None;
        'outer: for i in 0..text_chars.len() {
            let offset = if i < para.char_offsets.len() {
                para.char_offsets[i]
            } else {
                prev_end
            };
            while prev_end + 8 <= offset && ci < para.controls.len() {
                if ci == control_idx {
                    gap_start = Some(prev_end);
                    break 'outer;
                }
                ci += 1;
                prev_end += 8;
            }
            let char_size: u32 = if text_chars[i] == '\t' {
                8
            } else if text_chars[i].len_utf16() == 2 {
                2
            } else {
                1
            };
            prev_end = offset + char_size;
        }
        if gap_start.is_none() {
            while ci < para.controls.len() {
                if ci == control_idx {
                    gap_start = Some(prev_end);
                    break;
                }
                ci += 1;
                prev_end += 8;
            }
        }

        // char_offsets 조정
        if let Some(gs) = gap_start {
            let threshold = gs + 8;
            for offset in para.char_offsets.iter_mut() {
                if *offset >= threshold {
                    *offset -= 8;
                }
            }
        }

        // 컨트롤 및 ctrl_data_record 제거
        para.controls.remove(control_idx);
        if control_idx < para.ctrl_data_records.len() {
            para.ctrl_data_records.remove(control_idx);
        }

        // char_count 갱신
        if para.char_count >= 8 {
            para.char_count -= 8;
        }

        // line_segs 재계산: 그림 높이가 반영된 line_segs를 텍스트 기반으로 리셋
        Self::reflow_paragraph_line_segs_after_control_delete(para, &self.styles, self.dpi);

        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: parent_para_idx,
            ctrl: control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// 머리말/꼬리말 내부 그림 컨트롤을 삭제한다.
    pub fn delete_header_footer_picture_control_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let inner_paras: &mut Vec<crate::model::paragraph::Paragraph> = match outer_ctrl {
                crate::model::control::Control::Header(h) => &mut h.paragraphs,
                crate::model::control::Control::Footer(f) => &mut f.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            if inner_control_idx >= para.controls.len() {
                return Err(HwpError::RenderError(format!(
                    "내부 컨트롤 인덱스 {} 범위 초과",
                    inner_control_idx
                )));
            }
            if !matches!(&para.controls[inner_control_idx], Control::Picture(_)) {
                return Err(HwpError::RenderError(
                    "지정된 내부 컨트롤이 그림이 아닙니다".to_string(),
                ));
            }

            let text_chars: Vec<char> = para.text.chars().collect();
            let mut ci = 0usize;
            let mut prev_end: u32 = 0;
            let mut gap_start: Option<u32> = None;
            'outer: for i in 0..text_chars.len() {
                let offset = if i < para.char_offsets.len() {
                    para.char_offsets[i]
                } else {
                    prev_end
                };
                while prev_end + 8 <= offset && ci < para.controls.len() {
                    if ci == inner_control_idx {
                        gap_start = Some(prev_end);
                        break 'outer;
                    }
                    ci += 1;
                    prev_end += 8;
                }
                let char_size: u32 = if text_chars[i] == '\t' {
                    8
                } else if text_chars[i].len_utf16() == 2 {
                    2
                } else {
                    1
                };
                prev_end = offset + char_size;
            }
            if gap_start.is_none() {
                while ci < para.controls.len() {
                    if ci == inner_control_idx {
                        gap_start = Some(prev_end);
                        break;
                    }
                    ci += 1;
                    prev_end += 8;
                }
            }

            if let Some(gs) = gap_start {
                let threshold = gs + 8;
                for offset in para.char_offsets.iter_mut() {
                    if *offset >= threshold {
                        *offset -= 8;
                    }
                }
            }

            para.controls.remove(inner_control_idx);
            if inner_control_idx < para.ctrl_data_records.len() {
                para.ctrl_data_records.remove(inner_control_idx);
            }
            if para.char_count >= 8 {
                para.char_count -= 8;
            }
            Self::reflow_paragraph_line_segs_after_control_delete(para, &self.styles, self.dpi);
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: outer_para_idx,
            ctrl: outer_control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// 머리말/꼬리말 내부 Shape/OLE/Chart 컨트롤을 삭제한다.
    pub fn delete_header_footer_shape_control_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let inner_paras: &mut Vec<crate::model::paragraph::Paragraph> = match outer_ctrl {
                crate::model::control::Control::Header(h) => &mut h.paragraphs,
                crate::model::control::Control::Footer(f) => &mut f.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            if inner_control_idx >= para.controls.len() {
                return Err(HwpError::RenderError(format!(
                    "내부 컨트롤 인덱스 {} 범위 초과",
                    inner_control_idx
                )));
            }
            if !matches!(&para.controls[inner_control_idx], Control::Shape(_)) {
                return Err(HwpError::RenderError(
                    "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                ));
            }

            let text_chars: Vec<char> = para.text.chars().collect();
            let mut ci = 0usize;
            let mut prev_end: u32 = 0;
            let mut gap_start: Option<u32> = None;
            'outer: for i in 0..text_chars.len() {
                let offset = if i < para.char_offsets.len() {
                    para.char_offsets[i]
                } else {
                    prev_end
                };
                while prev_end + 8 <= offset && ci < para.controls.len() {
                    if ci == inner_control_idx {
                        gap_start = Some(prev_end);
                        break 'outer;
                    }
                    ci += 1;
                    prev_end += 8;
                }
                let char_size: u32 = if text_chars[i] == '\t' {
                    8
                } else if text_chars[i].len_utf16() == 2 {
                    2
                } else {
                    1
                };
                prev_end = offset + char_size;
            }
            if gap_start.is_none() {
                while ci < para.controls.len() {
                    if ci == inner_control_idx {
                        gap_start = Some(prev_end);
                        break;
                    }
                    ci += 1;
                    prev_end += 8;
                }
            }

            if let Some(gs) = gap_start {
                let threshold = gs + 8;
                for offset in para.char_offsets.iter_mut() {
                    if *offset >= threshold {
                        *offset -= 8;
                    }
                }
            }

            para.controls.remove(inner_control_idx);
            if inner_control_idx < para.ctrl_data_records.len() {
                para.ctrl_data_records.remove(inner_control_idx);
            }
            if para.char_count >= 8 {
                para.char_count -= 8;
            }
            Self::reflow_paragraph_line_segs_after_control_delete(para, &self.styles, self.dpi);
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: outer_para_idx,
            ctrl: outer_control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// 컨트롤 삭제 후 문단의 line_segs를 재계산한다.
    ///
    /// 그림/도형 삭제 시 문단의 line_segs에 컨트롤 높이가 그대로 남아,
    /// 레이아웃이 갱신되지 않는 문제를 방지한다.
    pub(crate) fn reflow_paragraph_line_segs_after_control_delete(
        para: &mut Paragraph,
        styles: &crate::renderer::style_resolver::ResolvedStyleSet,
        dpi: f64,
    ) {
        // 남은 컨트롤 중 가장 큰 높이 계산
        let max_remaining_ctrl_height = para
            .controls
            .iter()
            .map(|ctrl| match ctrl {
                Control::Picture(pic) => pic.common.height as i32,
                Control::Shape(shape) => shape.common().height as i32,
                Control::Equation(eq) => eq.common.height as i32,
                _ => 0,
            })
            .max()
            .unwrap_or(0);

        if max_remaining_ctrl_height > 0 {
            // 아직 컨트롤이 남아있으면 가장 큰 컨트롤 높이로 설정
            if let Some(ls) = para.line_segs.first_mut() {
                ls.line_height = max_remaining_ctrl_height;
                ls.text_height = max_remaining_ctrl_height;
                ls.baseline_distance = (max_remaining_ctrl_height * 850) / 1000;
            }
        } else if para.text.is_empty() {
            // 텍스트도 컨트롤도 없음 → 기본 텍스트 높이로 리셋
            if let Some(ls) = para.line_segs.first_mut() {
                ls.line_height = 1000;
                ls.text_height = 1000;
                ls.baseline_distance = 850;
                ls.line_spacing = 600;
            }
        } else {
            // 텍스트가 있으면 reflow_line_segs로 재계산
            let seg_width = para.line_segs.first().map(|s| s.segment_width).unwrap_or(0);
            let available_width_px = crate::renderer::hwpunit_to_px(seg_width, dpi);
            crate::renderer::composer::reflow_line_segs(para, available_width_px, styles, dpi);
        }
    }

    /// 커서 위치에 새 표를 삽입한다 (네이티브).
    ///
    /// 1. PageDef에서 편집 영역 폭 계산
    /// 2. 균등 열 폭으로 row_count × col_count 셀 생성
    /// 3. Table + Paragraph 조립
    /// 4. 커서 위치에 삽입 (빈 문단이면 교체, 아니면 분할 후 삽입)
    /// 5. 표 아래에 빈 문단 추가 (HWP 표준)
    pub fn create_table_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
        row_count: u16,
        col_count: u16,
    ) -> Result<String, HwpError> {
        use crate::model::paragraph::{CharShapeRef, LineSeg};
        use crate::model::style::{BorderFill, BorderLine, BorderLineType, DiagonalLine, Fill};
        use crate::model::table::{Cell, Table, TablePageBreak};

        // 유효성 검사
        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과 (총 {}개)",
                section_idx,
                self.document.sections.len()
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }
        if row_count == 0 || col_count == 0 || col_count > 256 {
            return Err(HwpError::RenderError(format!(
                "행/열 수 범위 오류 (행={}, 열={}, 열은 1~256)",
                row_count, col_count
            )));
        }

        // --- 1. 편집 영역 폭 계산 ---
        let pd = &self.document.sections[section_idx].section_def.page_def;
        let outer_margin_lr: i32 = 283 * 2; // outer_margin left + right (~2mm)
        let content_width =
            (pd.width as i32 - pd.margin_left as i32 - pd.margin_right as i32 - outer_margin_lr)
                .max(7200) as u32;

        // --- 2. 한컴 기본값 기반 셀 생성 (blank_h_saved.hwp 참조) ---
        let col_width = content_width / col_count as u32;
        // 한컴 기본: 셀 패딩 L=510 R=510 T=141 B=141
        let cell_pad = crate::model::Padding {
            left: 510,
            right: 510,
            top: 141,
            bottom: 141,
        };
        // 한컴 기본: 셀 높이 = top + bottom padding (빈 셀 최소 높이)
        let cell_height: u32 = (cell_pad.top + cell_pad.bottom) as u32;
        // 한컴 기본: 행 렌더링 높이 = padding_top + line_height(1000) + padding_bottom
        let rendered_row_height: u32 = cell_pad.top as u32 + 1000 + cell_pad.bottom as u32;
        let total_width = col_width * col_count as u32;
        let total_height = rendered_row_height * row_count as u32;

        // BorderFill: 실선 테두리가 있는 기존 항목 재사용, 없으면 새로 생성
        let cell_border_fill_id = {
            let existing = self.document.doc_info.border_fills.iter().position(|bf| {
                bf.borders
                    .iter()
                    .all(|b| b.line_type == BorderLineType::Solid && b.width >= 1)
            });
            if let Some(idx) = existing {
                (idx + 1) as u16 // 1-based
            } else {
                // 실선 BorderFill이 없으면 새로 생성
                let solid_border = BorderLine {
                    line_type: BorderLineType::Solid,
                    width: 1,
                    color: 0,
                };
                let new_bf = BorderFill {
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
                };
                self.document.doc_info.border_fills.push(new_bf);
                self.document.doc_info.raw_stream = None;
                self.document.doc_info.border_fills.len() as u16 // 1-based
            }
        };

        // 커서 위치 문단의 속성을 기본값으로 상속 (한컴 동작 일치)
        let current_para = &self.document.sections[section_idx].paragraphs[para_idx];
        let default_char_shape_id: u32 = current_para
            .char_shapes
            .first()
            .map(|cs| cs.char_shape_id)
            .unwrap_or(0);
        let default_para_shape_id: u16 = current_para.para_shape_id;

        // 셀 목록 생성
        let mut cells = Vec::with_capacity((row_count as usize) * (col_count as usize));
        for r in 0..row_count {
            for c in 0..col_count {
                let mut cell = Cell::new_empty(c, r, col_width, cell_height, cell_border_fill_id);
                cell.padding = cell_pad;
                cell.vertical_align = crate::model::table::VerticalAlign::Center; // 한컴 기본값
                                                                                  // 셀 문단 보정: char_count_msb, raw_header_extra, para/char shape
                for cp in &mut cell.paragraphs {
                    cp.char_count_msb = true;
                    cp.para_shape_id = default_para_shape_id;
                    if cp.raw_header_extra.len() < 10 {
                        let mut rhe = vec![0u8; 10];
                        rhe[0..2].copy_from_slice(&1u16.to_le_bytes()); // n_char_shapes=1
                        rhe[4..6].copy_from_slice(&1u16.to_le_bytes()); // n_line_segs=1
                        cp.raw_header_extra = rhe;
                    }
                    // line_segs 보정: new_empty()의 기본 LineSeg는 line_height=0이므로 항상 교체
                    let seg_w = (col_width as i32) - 141 - 141; // 셀 폭 - 좌우 패딩
                    cp.line_segs = vec![LineSeg {
                        text_start: 0,
                        line_height: 1000,
                        text_height: 1000,
                        baseline_distance: 850,
                        line_spacing: 600,
                        segment_width: seg_w,
                        tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                        ..Default::default()
                    }];
                }
                // raw_list_extra: 빈 벡터 (cell.width 필드가 LIST_HEADER에 직렬화됨)
                cell.raw_list_extra = Vec::new();
                cells.push(cell);
            }
        }

        // --- 3. Table 구조체 조립 (한컴 기본 속성값) ---
        let row_sizes: Vec<i16> = (0..row_count).map(|_| col_count as i16).collect();

        // raw_ctrl_data: CommonObjAttr 바이너리 (파서 호환)
        // 바이트 레이아웃: flags(4) + v_offset(4) + h_offset(4) + width(4) + height(4)
        //                 + z_order(4) + margin_l(2) + margin_r(2) + margin_t(2) + margin_b(2)
        //                 + instance_id(4) = 36바이트 (+ 여유 2바이트 = 38)
        // vert=Para(2), horz=Para(3), wrap=TopAndBottom(1)
        // width_criterion=Absolute(4), height_criterion=Absolute(2)
        let flags: u32 = (2 << 3) | (3 << 8) | (4 << 15) | (2 << 18) | (1 << 21);
        let outer_margin: i16 = 283; // ~1mm
        let mut raw_ctrl_data = vec![0u8; 38];
        raw_ctrl_data[common_obj_offsets::FLAGS].copy_from_slice(&flags.to_le_bytes());
        // vertical_offset/horizontal_offset/z_order = 0
        raw_ctrl_data[common_obj_offsets::WIDTH].copy_from_slice(&total_width.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::HEIGHT].copy_from_slice(&total_height.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_LEFT].copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_RIGHT]
            .copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_TOP].copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_BOTTOM]
            .copy_from_slice(&outer_margin.to_le_bytes());
        // instance_id (해시 기반, 비-0 필수)
        let instance_id: u32 = {
            let mut h: u32 = 0x7c150000;
            h = h.wrapping_add(row_count as u32 * 0x1000);
            h = h.wrapping_add(col_count as u32 * 0x100);
            h = h.wrapping_add(total_width);
            h = h.wrapping_add(total_height.wrapping_mul(0x1b));
            if h == 0 {
                h = 0x7c154b69;
            }
            h
        };
        raw_ctrl_data[common_obj_offsets::INSTANCE_ID].copy_from_slice(&instance_id.to_le_bytes());

        let mut table = Table {
            attr: 0x082A2210, // 한컴 기본값 (blank_h_saved.hwp)
            row_count,
            col_count,
            cell_spacing: 0,
            padding: crate::model::Padding {
                left: 510,
                right: 510,
                top: 141,
                bottom: 141,
            },
            row_sizes,
            border_fill_id: cell_border_fill_id, // 한컴: 표와 셀이 같은 BorderFill 사용
            zones: Vec::new(),
            cells,
            cell_grid: Vec::new(),
            page_break: TablePageBreak::None,
            repeat_header: false,
            caption: None,
            common: crate::model::shape::CommonObjAttr {
                treat_as_char: false,
                text_wrap: crate::model::shape::TextWrap::TopAndBottom,
                vert_rel_to: crate::model::shape::VertRelTo::Para,
                horz_rel_to: crate::model::shape::HorzRelTo::Para,
                vert_align: crate::model::shape::VertAlign::Top,
                horz_align: crate::model::shape::HorzAlign::Left,
                width: total_width,
                height: total_height,
                ..Default::default()
            },
            outer_margin_left: 283,
            outer_margin_right: 283,
            outer_margin_top: 283,
            outer_margin_bottom: 283,
            raw_ctrl_data,
            raw_table_record_attr: 0x00000006, // 한컴 기본값 (bit1=셀분리금지, bit2=repeat_header)
            raw_table_record_extra: vec![0u8; 2],
            dirty: true,
            local_resize_rows: Vec::new(),
            local_resize_cols: Vec::new(),
            local_resize_cell_widths: Vec::new(),
            local_resize_cell_heights: Vec::new(),
        };
        table.rebuild_grid();

        // --- 4. Table을 포함하는 Paragraph 생성 ---
        // para_shape_id: 커서 위치 문단의 값 상속 (한컴 동작 일치)
        let table_para_shape_id = default_para_shape_id;

        let mut table_raw_header_extra = vec![0u8; 10];
        table_raw_header_extra[0..2].copy_from_slice(&1u16.to_le_bytes());
        table_raw_header_extra[4..6].copy_from_slice(&1u16.to_le_bytes());

        let table_para = Paragraph {
            text: String::new(),
            char_count: 9, // 확장 제어문자(8 code units) + 문단끝(1)
            control_mask: 0x00000800,
            char_offsets: vec![],
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: default_char_shape_id,
            }],
            line_segs: vec![LineSeg {
                text_start: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: 0, // 한컴 표준: 표 문단의 segment_width는 0
                tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                ..Default::default()
            }],
            para_shape_id: table_para_shape_id,
            style_id: 0,
            controls: vec![Control::Table(Box::new(table))],
            ctrl_data_records: vec![None],
            has_para_text: true,
            raw_header_extra: table_raw_header_extra,
            char_count_msb: false,
            ..Default::default()
        };

        let make_empty_neighbor_para = || {
            let mut empty_raw_header_extra = vec![0u8; 10];
            empty_raw_header_extra[0..2].copy_from_slice(&1u16.to_le_bytes());
            empty_raw_header_extra[4..6].copy_from_slice(&1u16.to_le_bytes());
            Paragraph {
                text: String::new(),
                char_count: 1,
                char_count_msb: false,
                control_mask: 0,
                para_shape_id: default_para_shape_id,
                style_id: 0,
                char_shapes: vec![CharShapeRef {
                    start_pos: 0,
                    char_shape_id: default_char_shape_id,
                }],
                line_segs: vec![LineSeg {
                    text_start: 0,
                    line_height: 1000,
                    text_height: 1000,
                    baseline_distance: 850,
                    line_spacing: 600,
                    segment_width: content_width as i32,
                    tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                    ..Default::default()
                }],
                has_para_text: false,
                raw_header_extra: empty_raw_header_extra,
                ..Default::default()
            }
        };

        // --- 5. 커서 위치에 삽입 ---
        self.document.sections[section_idx].raw_stream = None;

        let para = &self.document.sections[section_idx].paragraphs[para_idx];
        let is_empty_para = para.text.is_empty() && para.controls.is_empty();
        let is_structure_only_empty_para = Self::is_structure_only_empty_paragraph(para);

        let insert_para_idx;
        let table_control_idx;
        if is_empty_para {
            // 빈 문단이면 UI에서 넘어온 offset과 무관하게 현재 줄을 표 host로 사용한다.
            self.document.sections[section_idx].paragraphs[para_idx] = table_para;
            insert_para_idx = para_idx;
            table_control_idx = 0;
        } else if is_structure_only_empty_para {
            // blank2010 첫 문단처럼 SectionDef/ColumnDef만 있는 빈 줄은 구조 컨트롤을
            // 보존하되, 줄 배치는 표 host 문단 기준으로 교체해 표 위 빈 줄을 만들지 않는다.
            let old_para = self.document.sections[section_idx].paragraphs[para_idx].clone();
            let mut merged_para = table_para;
            let table_control = merged_para
                .controls
                .pop()
                .ok_or_else(|| HwpError::RenderError("표 컨트롤 생성 실패".to_string()))?;
            let table_ctrl_data = merged_para.ctrl_data_records.pop().unwrap_or(None);

            merged_para.controls = old_para.controls;
            merged_para.ctrl_data_records = old_para.ctrl_data_records;
            while merged_para.ctrl_data_records.len() < merged_para.controls.len() {
                merged_para.ctrl_data_records.push(None);
            }
            table_control_idx = merged_para.controls.len();
            merged_para.controls.push(table_control);
            merged_para.ctrl_data_records.push(table_ctrl_data);
            merged_para.char_count = merged_para.controls.len() as u32 * 8 + 1;
            merged_para.control_mask = old_para.control_mask | 0x0000_0800;
            merged_para.has_para_text = true;

            self.document.sections[section_idx].paragraphs[para_idx] = merged_para;
            insert_para_idx = para_idx;
        } else if char_offset == 0 && para.controls.is_empty() {
            // 문단 맨 앞이면 바로 앞에 삽입
            self.document.sections[section_idx]
                .paragraphs
                .insert(para_idx, table_para);
            insert_para_idx = para_idx;
            table_control_idx = 0;
        } else {
            // 문단 중간이면 분할 후 삽입
            if char_offset > 0 && !para.text.is_empty() {
                let new_para =
                    self.document.sections[section_idx].paragraphs[para_idx].split_at(char_offset);
                self.document.sections[section_idx]
                    .paragraphs
                    .insert(para_idx + 1, new_para);
                // 표 문단은 분할된 뒤에 삽입
                self.document.sections[section_idx]
                    .paragraphs
                    .insert(para_idx + 1, table_para);
                insert_para_idx = para_idx + 1;
                table_control_idx = 0;
            } else {
                // char_offset == 0이지만 컨트롤이 있는 경우 → 뒤에 삽입
                self.document.sections[section_idx]
                    .paragraphs
                    .insert(para_idx + 1, table_para);
                insert_para_idx = para_idx + 1;
                table_control_idx = 0;
            }
        }

        // 표 아래에 빈 문단 추가 (HWP 표준, 한컴 blank_h_saved.hwp 참조)
        self.document.sections[section_idx]
            .paragraphs
            .insert(insert_para_idx + 1, make_empty_neighbor_para());

        // --- 6. 스타일 갱신 + 리플로우 + 페이지네이션 ---
        // 새 BorderFill 추가 시 styles.border_styles 갱신이 필요하므로 rebuild_section 사용
        self.rebuild_section(section_idx);

        self.event_log.push(DocumentEvent::TableRowInserted {
            section: section_idx,
            para: insert_para_idx,
            ctrl: table_control_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{}",
            insert_para_idx, table_control_idx
        )))
    }

    pub(crate) fn default_table_paragraph_pair(
        &mut self,
        section_idx: usize,
        row_count: u16,
        col_count: u16,
        default_char_shape_id: u32,
        default_para_shape_id: u16,
    ) -> Result<(Paragraph, Paragraph), HwpError> {
        use crate::model::paragraph::{CharShapeRef, LineSeg};
        use crate::model::style::{BorderFill, BorderLine, BorderLineType, DiagonalLine, Fill};
        use crate::model::table::{Cell, Table, TablePageBreak};

        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과 (총 {}개)",
                section_idx,
                self.document.sections.len()
            )));
        }
        if row_count == 0 || col_count == 0 || col_count > 256 {
            return Err(HwpError::RenderError(format!(
                "행/열 수 범위 오류 (행={}, 열={}, 열은 1~256)",
                row_count, col_count
            )));
        }

        let pd = &self.document.sections[section_idx].section_def.page_def;
        let outer_margin_lr: i32 = 283 * 2;
        let content_width =
            (pd.width as i32 - pd.margin_left as i32 - pd.margin_right as i32 - outer_margin_lr)
                .max(7200) as u32;
        let col_width = content_width / col_count as u32;
        let cell_pad = crate::model::Padding {
            left: 510,
            right: 510,
            top: 141,
            bottom: 141,
        };
        let cell_height: u32 = (cell_pad.top + cell_pad.bottom) as u32;
        let rendered_row_height: u32 = cell_pad.top as u32 + 1000 + cell_pad.bottom as u32;
        let total_width = col_width * col_count as u32;
        let total_height = rendered_row_height * row_count as u32;

        let cell_border_fill_id = {
            let existing = self.document.doc_info.border_fills.iter().position(|bf| {
                bf.borders
                    .iter()
                    .all(|b| b.line_type == BorderLineType::Solid && b.width >= 1)
            });
            if let Some(idx) = existing {
                (idx + 1) as u16
            } else {
                let solid_border = BorderLine {
                    line_type: BorderLineType::Solid,
                    width: 1,
                    color: 0,
                };
                let new_bf = BorderFill {
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
                };
                self.document.doc_info.border_fills.push(new_bf);
                self.document.doc_info.raw_stream = None;
                self.document.doc_info.border_fills.len() as u16
            }
        };

        let mut cells = Vec::with_capacity((row_count as usize) * (col_count as usize));
        for r in 0..row_count {
            for c in 0..col_count {
                let mut cell = Cell::new_empty(c, r, col_width, cell_height, cell_border_fill_id);
                cell.padding = cell_pad;
                cell.vertical_align = crate::model::table::VerticalAlign::Center;
                for cp in &mut cell.paragraphs {
                    cp.char_count_msb = true;
                    cp.para_shape_id = default_para_shape_id;
                    if cp.raw_header_extra.len() < 10 {
                        let mut rhe = vec![0u8; 10];
                        rhe[0..2].copy_from_slice(&1u16.to_le_bytes());
                        rhe[4..6].copy_from_slice(&1u16.to_le_bytes());
                        cp.raw_header_extra = rhe;
                    }
                    let seg_w = (col_width as i32) - 141 - 141;
                    cp.line_segs = vec![LineSeg {
                        text_start: 0,
                        line_height: 1000,
                        text_height: 1000,
                        baseline_distance: 850,
                        line_spacing: 600,
                        segment_width: seg_w,
                        tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                        ..Default::default()
                    }];
                }
                cell.raw_list_extra = Vec::new();
                cells.push(cell);
            }
        }

        let row_sizes: Vec<i16> = (0..row_count).map(|_| col_count as i16).collect();
        let flags: u32 = (2 << 3) | (3 << 8) | (4 << 15) | (2 << 18) | (1 << 21);
        let outer_margin: i16 = 283;
        let mut raw_ctrl_data = vec![0u8; 38];
        raw_ctrl_data[common_obj_offsets::FLAGS].copy_from_slice(&flags.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::WIDTH].copy_from_slice(&total_width.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::HEIGHT].copy_from_slice(&total_height.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_LEFT].copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_RIGHT]
            .copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_TOP].copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_BOTTOM]
            .copy_from_slice(&outer_margin.to_le_bytes());
        let instance_id: u32 = {
            let mut h: u32 = 0x7c150000;
            h = h.wrapping_add(row_count as u32 * 0x1000);
            h = h.wrapping_add(col_count as u32 * 0x100);
            h = h.wrapping_add(total_width);
            h = h.wrapping_add(total_height.wrapping_mul(0x1b));
            if h == 0 {
                h = 0x7c154b69;
            }
            h
        };
        raw_ctrl_data[common_obj_offsets::INSTANCE_ID].copy_from_slice(&instance_id.to_le_bytes());

        let mut table = Table {
            attr: 0x082A2210,
            row_count,
            col_count,
            cell_spacing: 0,
            padding: cell_pad,
            row_sizes,
            border_fill_id: cell_border_fill_id,
            zones: Vec::new(),
            cells,
            cell_grid: Vec::new(),
            page_break: TablePageBreak::None,
            repeat_header: false,
            caption: None,
            common: crate::model::shape::CommonObjAttr {
                treat_as_char: false,
                text_wrap: crate::model::shape::TextWrap::TopAndBottom,
                vert_rel_to: crate::model::shape::VertRelTo::Para,
                horz_rel_to: crate::model::shape::HorzRelTo::Para,
                vert_align: crate::model::shape::VertAlign::Top,
                horz_align: crate::model::shape::HorzAlign::Left,
                width: total_width,
                height: total_height,
                ..Default::default()
            },
            outer_margin_left: 283,
            outer_margin_right: 283,
            outer_margin_top: 283,
            outer_margin_bottom: 283,
            raw_ctrl_data,
            raw_table_record_attr: 0x00000006,
            raw_table_record_extra: vec![0u8; 2],
            dirty: true,
            local_resize_rows: Vec::new(),
            local_resize_cols: Vec::new(),
            local_resize_cell_widths: Vec::new(),
            local_resize_cell_heights: Vec::new(),
        };
        table.rebuild_grid();

        let mut table_raw_header_extra = vec![0u8; 10];
        table_raw_header_extra[0..2].copy_from_slice(&1u16.to_le_bytes());
        table_raw_header_extra[4..6].copy_from_slice(&1u16.to_le_bytes());

        let table_para = Paragraph {
            text: String::new(),
            char_count: 9,
            control_mask: 0x00000800,
            char_offsets: vec![],
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: default_char_shape_id,
            }],
            line_segs: vec![LineSeg {
                text_start: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: 0,
                tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                ..Default::default()
            }],
            para_shape_id: default_para_shape_id,
            style_id: 0,
            controls: vec![Control::Table(Box::new(table))],
            ctrl_data_records: vec![None],
            has_para_text: true,
            raw_header_extra: table_raw_header_extra,
            char_count_msb: false,
            ..Default::default()
        };

        let mut empty_raw_header_extra = vec![0u8; 10];
        empty_raw_header_extra[0..2].copy_from_slice(&1u16.to_le_bytes());
        empty_raw_header_extra[4..6].copy_from_slice(&1u16.to_le_bytes());
        let empty_para = Paragraph {
            text: String::new(),
            char_count: 1,
            char_count_msb: false,
            control_mask: 0,
            para_shape_id: default_para_shape_id,
            style_id: 0,
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: default_char_shape_id,
            }],
            line_segs: vec![LineSeg {
                text_start: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: content_width as i32,
                tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                ..Default::default()
            }],
            has_para_text: false,
            raw_header_extra: empty_raw_header_extra,
            ..Default::default()
        };

        Ok((table_para, empty_para))
    }

    pub(crate) fn insert_table_paragraph_into_paragraphs(
        paragraphs: &mut Vec<Paragraph>,
        para_idx: usize,
        char_offset: usize,
        table_para: Paragraph,
        empty_para: Paragraph,
    ) -> Result<(usize, usize), HwpError> {
        let para = paragraphs
            .get(para_idx)
            .ok_or_else(|| HwpError::RenderError(format!("문단 {} 범위 초과", para_idx)))?;
        let is_empty_para = para.text.is_empty() && para.controls.is_empty();
        let insert_para_idx = if is_empty_para {
            paragraphs[para_idx] = table_para;
            para_idx
        } else if char_offset == 0 && para.controls.is_empty() {
            paragraphs.insert(para_idx, table_para);
            para_idx
        } else if char_offset > 0 && !para.text.is_empty() {
            let new_para = paragraphs[para_idx].split_at(char_offset);
            paragraphs.insert(para_idx + 1, new_para);
            paragraphs.insert(para_idx + 1, table_para);
            para_idx + 1
        } else {
            paragraphs.insert(para_idx + 1, table_para);
            para_idx + 1
        };
        paragraphs.insert(insert_para_idx + 1, empty_para);
        Ok((insert_para_idx, 0))
    }

    pub(crate) fn create_table_by_cell_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        path: &[(usize, usize, usize)],
        char_offset: usize,
        row_count: u16,
        col_count: u16,
    ) -> Result<String, HwpError> {
        if path.is_empty() {
            return Err(HwpError::RenderError(
                "cell_path가 비어있습니다".to_string(),
            ));
        }
        let target_para_idx = path.last().map(|entry| entry.2).unwrap_or(0);
        let (default_char_shape_id, default_para_shape_id) = {
            let target_para = self.resolve_paragraph_by_path(section_idx, parent_para_idx, path)?;
            (
                target_para
                    .char_shapes
                    .first()
                    .map(|shape| shape.char_shape_id)
                    .unwrap_or(0),
                target_para.para_shape_id,
            )
        };
        let (table_para, empty_para) = self.default_table_paragraph_pair(
            section_idx,
            row_count,
            col_count,
            default_char_shape_id,
            default_para_shape_id,
        )?;
        let (insert_para_idx, table_control_idx) = {
            let paragraphs =
                self.get_cell_paragraphs_mut_by_path(section_idx, parent_para_idx, path)?;
            Self::insert_table_paragraph_into_paragraphs(
                paragraphs,
                target_para_idx,
                char_offset,
                table_para,
                empty_para,
            )?
        };
        let mut table_path = path.to_vec();
        if let Some(last) = table_path.last_mut() {
            last.2 = insert_para_idx;
        }
        table_path.push((table_control_idx, 0, 0));
        let cell_path_json = serde_json::Value::Array(
            table_path
                .iter()
                .map(|(control, cell, para)| serde_json::json!([control, cell, para]))
                .collect(),
        )
        .to_string();

        self.document.sections[section_idx].raw_stream = None;
        self.rebuild_section(section_idx);
        self.event_log.push(DocumentEvent::TableRowInserted {
            section: section_idx,
            para: parent_para_idx,
            ctrl: path[0].0,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"cell_path\":{},\"cellPath\":{}",
            insert_para_idx, table_control_idx, cell_path_json, cell_path_json
        )))
    }

    /// 커서 위치에 표를 삽입한다 (확장, JSON 옵션).
    ///
    /// 기본 create_table_native의 확장판으로, treat_as_char(인라인) 등 세부 속성을 지정할 수 있다.
    /// treat_as_char=true인 경우:
    ///   - 별도 문단을 생성하지 않고 기존 문단의 controls에 표를 추가
    ///   - 텍스트 흐름에 8 UTF-16 코드유닛 자리를 삽입
    ///   - 표 아래 빈 문단 미생성
    pub fn create_table_ex_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
        row_count: u16,
        col_count: u16,
        treat_as_char: bool,
        col_widths_hu: Option<&[u32]>,
        row_heights_hu: Option<&[u32]>,
    ) -> Result<String, HwpError> {
        use crate::model::paragraph::{CharShapeRef, LineSeg};
        use crate::model::style::{BorderFill, BorderLine, BorderLineType, DiagonalLine, Fill};
        use crate::model::table::{Cell, Table, TablePageBreak};

        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }
        if row_count == 0 || col_count == 0 || col_count > 256 {
            return Err(HwpError::RenderError(format!(
                "행/열 수 범위 오류 (행={}, 열={})",
                row_count, col_count
            )));
        }

        if !treat_as_char {
            return self.create_table_native(
                section_idx,
                para_idx,
                char_offset,
                row_count,
                col_count,
            );
        }

        // ── 인라인 TAC 표 생성 ──

        let pd = &self.document.sections[section_idx].section_def.page_def;
        let outer_margin: i16 = 283;
        let outer_margin_lr = (outer_margin * 2) as i32;
        let content_width =
            (pd.width as i32 - pd.margin_left as i32 - pd.margin_right as i32 - outer_margin_lr)
                .max(7200) as u32;

        // 열 폭 결정
        let col_ws: Vec<u32> = if let Some(widths) = col_widths_hu {
            if widths.len() == col_count as usize {
                widths.to_vec()
            } else {
                let w = content_width / col_count as u32;
                vec![w; col_count as usize]
            }
        } else {
            let w = content_width / col_count as u32;
            vec![w; col_count as usize]
        };
        let total_width: u32 = col_ws.iter().sum();

        let cell_pad = crate::model::Padding {
            left: 510,
            right: 510,
            top: 141,
            bottom: 141,
        };
        let min_row_height: u32 = cell_pad.top as u32 + 1000 + cell_pad.bottom as u32;
        let row_heights: Vec<u32> = if let Some(heights) = row_heights_hu {
            if heights.len() == row_count as usize {
                heights.iter().map(|h| (*h).max(min_row_height)).collect()
            } else {
                vec![min_row_height; row_count as usize]
            }
        } else {
            vec![min_row_height; row_count as usize]
        };
        let total_height: u32 = row_heights.iter().sum();

        // BorderFill
        let cell_border_fill_id = {
            let existing = self.document.doc_info.border_fills.iter().position(|bf| {
                bf.borders
                    .iter()
                    .all(|b| b.line_type == BorderLineType::Solid && b.width >= 1)
            });
            if let Some(idx) = existing {
                (idx + 1) as u16
            } else {
                let solid_border = BorderLine {
                    line_type: BorderLineType::Solid,
                    width: 1,
                    color: 0,
                };
                let new_bf = BorderFill {
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
                };
                self.document.doc_info.border_fills.push(new_bf);
                self.document.doc_info.raw_stream = None;
                self.document.doc_info.border_fills.len() as u16
            }
        };

        let current_para = &self.document.sections[section_idx].paragraphs[para_idx];
        let default_char_shape_id: u32 = current_para
            .char_shapes
            .first()
            .map(|cs| cs.char_shape_id)
            .unwrap_or(0);
        let default_para_shape_id: u16 = current_para.para_shape_id;

        // 셀 생성
        let mut cells = Vec::with_capacity((row_count as usize) * (col_count as usize));
        for r in 0..row_count {
            let row_height = row_heights[r as usize];
            for c in 0..col_count {
                let col_w = col_ws[c as usize];
                let mut cell = Cell::new_empty(c, r, col_w, row_height, cell_border_fill_id);
                cell.padding = cell_pad;
                cell.vertical_align = crate::model::table::VerticalAlign::Center;
                for cp in &mut cell.paragraphs {
                    cp.char_count_msb = true;
                    cp.para_shape_id = default_para_shape_id;
                    if cp.raw_header_extra.len() < 10 {
                        let mut rhe = vec![0u8; 10];
                        rhe[0..2].copy_from_slice(&1u16.to_le_bytes());
                        rhe[4..6].copy_from_slice(&1u16.to_le_bytes());
                        cp.raw_header_extra = rhe;
                    }
                    let seg_w = (col_w as i32) - 141 - 141;
                    let text_height =
                        row_height.saturating_sub((cell_pad.top + cell_pad.bottom) as u32);
                    cp.line_segs = vec![LineSeg {
                        text_start: 0,
                        line_height: text_height as i32,
                        text_height: text_height as i32,
                        baseline_distance: (text_height as f64 * 0.85) as i32,
                        line_spacing: 600,
                        segment_width: seg_w,
                        tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                        ..Default::default()
                    }];
                }
                cell.raw_list_extra = Vec::new();
                cells.push(cell);
            }
        }

        // Table 구조체
        let row_sizes: Vec<i16> = (0..row_count).map(|_| col_count as i16).collect();
        // raw_ctrl_data: treat_as_char + vert=Page(0) + horz=Para(3) + wrap=TopAndBottom(1)
        #[allow(clippy::identity_op)]
        let flags: u32 = (1 << 0) /* treat_as_char */
            | (0 << 3) /* vert=Page */
            | (3 << 8) /* horz=Para */
            | (4 << 15) /* width_criterion=Absolute */
            | (2 << 18) /* height_criterion=Absolute */
            | (1 << 21) /* wrap=TopAndBottom */;
        let mut raw_ctrl_data = vec![0u8; 38];
        raw_ctrl_data[common_obj_offsets::FLAGS].copy_from_slice(&flags.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::WIDTH].copy_from_slice(&total_width.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::HEIGHT].copy_from_slice(&total_height.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_LEFT].copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_RIGHT]
            .copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_TOP].copy_from_slice(&outer_margin.to_le_bytes());
        raw_ctrl_data[common_obj_offsets::MARGIN_BOTTOM]
            .copy_from_slice(&outer_margin.to_le_bytes());
        let instance_id: u32 = {
            let mut h: u32 = 0x7c160000;
            h = h.wrapping_add(row_count as u32 * 0x1000);
            h = h.wrapping_add(col_count as u32 * 0x100);
            h = h.wrapping_add(total_width);
            if h == 0 {
                h = 0x7c164b69;
            }
            h
        };
        raw_ctrl_data[common_obj_offsets::INSTANCE_ID].copy_from_slice(&instance_id.to_le_bytes());

        let mut table = Table {
            attr: 0x04000006,
            row_count,
            col_count,
            cell_spacing: 0,
            padding: cell_pad,
            row_sizes,
            border_fill_id: cell_border_fill_id,
            zones: Vec::new(),
            cells,
            cell_grid: Vec::new(),
            page_break: TablePageBreak::RowBreak,
            repeat_header: false,
            caption: None,
            common: crate::model::shape::CommonObjAttr {
                treat_as_char: true,
                text_wrap: crate::model::shape::TextWrap::TopAndBottom,
                vert_rel_to: crate::model::shape::VertRelTo::Page,
                horz_rel_to: crate::model::shape::HorzRelTo::Para,
                vert_align: crate::model::shape::VertAlign::Top,
                horz_align: crate::model::shape::HorzAlign::Left,
                width: total_width,
                height: total_height,
                ..Default::default()
            },
            outer_margin_left: outer_margin,
            outer_margin_right: outer_margin,
            outer_margin_top: outer_margin,
            outer_margin_bottom: outer_margin,
            raw_ctrl_data,
            raw_table_record_attr: 0x04000006,
            raw_table_record_extra: vec![0u8; 2],
            dirty: true,
            local_resize_rows: Vec::new(),
            local_resize_cols: Vec::new(),
            local_resize_cell_widths: Vec::new(),
            local_resize_cell_heights: Vec::new(),
        };
        table.rebuild_grid();

        // ── 기존 문단에 인라인 삽입 ──
        self.document.sections[section_idx].raw_stream = None;
        let para = &mut self.document.sections[section_idx].paragraphs[para_idx];

        // controls에 표 추가
        let ctrl_idx = para.controls.len();
        para.controls.push(Control::Table(Box::new(table)));
        para.ctrl_data_records.push(None);

        // char_offsets에 8 UTF-16 코드유닛 갭 삽입
        // 확장 제어문자는 8 코드유닛을 차지
        let insert_utf16_pos = if char_offset < para.char_offsets.len() {
            para.char_offsets[char_offset]
        } else if !para.char_offsets.is_empty() {
            let last_idx = para.char_offsets.len() - 1;
            let last_char_len = para
                .text
                .chars()
                .nth(last_idx)
                .map(|c| c.len_utf16() as u32)
                .unwrap_or(1);
            para.char_offsets[last_idx] + last_char_len
        } else {
            0
        };

        // 이후 char_offsets를 8만큼 shift
        for offset in para.char_offsets.iter_mut() {
            if *offset >= insert_utf16_pos {
                *offset += 8;
            }
        }

        // char_count 갱신 (확장 제어문자 8 + 기존)
        para.char_count += 8;

        // LINE_SEG 갱신: 표 높이를 반영
        if let Some(seg) = para.line_segs.first_mut() {
            let new_lh = (total_height as i32).max(seg.line_height);
            if new_lh > seg.line_height {
                seg.line_height = new_lh;
                seg.text_height = new_lh;
                seg.baseline_distance = (new_lh as f64 * 0.85) as i32;
            }
        }

        // rebuild
        self.rebuild_section(section_idx);

        self.event_log.push(DocumentEvent::TableRowInserted {
            section: section_idx,
            para: para_idx,
            ctrl: ctrl_idx,
        });
        // 표 바로 뒤의 논리적 오프셋 계산
        let logical_after = super::super::helpers::text_to_logical_offset(
            &self.document.sections[section_idx].paragraphs[para_idx],
            char_offset,
        ) + 1;
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"logicalOffset\":{}",
            para_idx, ctrl_idx, logical_after
        )))
    }

    /// 커서 위치에 그림을 삽입한다 (네이티브).
    ///
    /// - `cell_path` 가 비어있으면 본문 paragraph 에 inline (treat_as_char=true) 삽입.
    /// - `cell_path` 가 있으면 표 셀 영역에 floating picture (tac=false, wrap=Square,
    ///   Page-relative offset) 로 삽입한다. 셀 자체는 비어있는 채로 유지되어 cursor
    ///   클릭이 정상 동작 (#1151). 한컴 2022 의 셀 이미지 삽입 패턴과 동일
    ///   (incellpicture.hwp 검증).
    ///
    /// `paper_offset_x_hu / paper_offset_y_hu`: 셀 floating 분기에서 사용할 paper-relative
    /// 좌표 (HWPUNIT). `None` 이면 셀 좌상단 (`compute_cell_page_offset`) 을 default 로 사용
    /// — 기존 동작 + API caller 호환. studio drag 좌표 기반 호출은 `Some` 으로 전달.
    /// 본문 inline 분기 (cell_path 비어있음) 는 본 매개변수를 사용하지 않는다.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_picture_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
        cell_path: &[(usize, usize, usize)],
        image_data: &[u8],
        width: u32,
        height: u32,
        natural_width_px: u32,
        natural_height_px: u32,
        extension: &str,
        description: &str,
        paper_offset_x_hu: Option<i32>,
        paper_offset_y_hu: Option<i32>,
    ) -> Result<String, HwpError> {
        use crate::model::bin_data::{
            BinData, BinDataCompression, BinDataContent, BinDataStatus, BinDataType,
        };
        use crate::model::image::{CropInfo, ImageAttr, ImageEffect, Picture};
        use crate::model::paragraph::{CharShapeRef, LineSeg};
        use crate::model::shape::{CommonObjAttr, HorzRelTo, ShapeComponentAttr, VertRelTo};
        // 유효성 검사
        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과 (총 {}개)",
                section_idx,
                self.document.sections.len()
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }
        if image_data.is_empty() {
            return Err(HwpError::RenderError(
                "이미지 데이터가 비어 있습니다".to_string(),
            ));
        }
        // cell_path 가 있으면 경로가 유효한지 사전 검증한다.
        //
        // 표 셀 picture 는 한컴 정합상 표 sibling floating 으로 삽입하지만,
        // 글상자(text_box) 내부 picture 는 글상자 문단의 control 로 들어가야 한다.
        // 기존 resolve_cell_by_path 는 마지막 엔트리가 표일 때만 성공하므로
        // 먼저 표/글상자를 구분한다.
        let cell_path_is_textbox = if !cell_path.is_empty() {
            let section = &self.document.sections[section_idx];
            let is_textbox = Self::cell_path_terminates_at_textbox(section, para_idx, cell_path)?;
            if !is_textbox {
                self.resolve_cell_by_path(section_idx, para_idx, cell_path)?;
            }
            is_textbox
        } else {
            false
        };

        // --- 1. BinDataContent 추가 ---
        let next_id = self.document.bin_data_content.len() as u16 + 1;
        self.document.bin_data_content.push(BinDataContent {
            id: next_id,
            data: image_data.to_vec(),
            extension: extension.to_string(),
        });

        // --- 2. BinData 메타데이터 추가 ---
        // attr: bits 0-3=1(Embedding), bits 4-5=0(Default), bits 8-9=1(Success)
        let bin_attr: u16 = 0x0101;
        self.document.doc_info.bin_data_list.push(BinData {
            raw_data: None,
            attr: bin_attr,
            data_type: BinDataType::Embedding,
            compression: BinDataCompression::Default,
            status: BinDataStatus::Success,
            abs_path: None,
            rel_path: None,
            storage_id: next_id,
            extension: Some(extension.to_string()),
        });
        self.document.doc_info.raw_stream = None; // DocInfo 재직렬화

        // --- 공통 자원 ---
        let shape_attr = ShapeComponentAttr {
            original_width: width,
            original_height: height,
            current_width: width,
            current_height: height,
            local_file_version: 1,
            render_sx: 1.0,
            render_sy: 1.0,
            ..Default::default()
        };
        let bx = [0i32, 0, width as i32, 0];
        let by = [width as i32, height as i32, 0, height as i32];
        let crop = CropInfo {
            left: 0,
            top: 0,
            right: (natural_width_px * 75) as i32,
            bottom: (natural_height_px * 75) as i32,
        };
        let image_attr = ImageAttr {
            bin_data_id: next_id,
            brightness: 0,
            contrast: 0,
            effect: ImageEffect::RealPic,
            transparency: 0,
            external_path: None,
        };

        if !cell_path.is_empty() {
            if cell_path_is_textbox {
                // === 글상자 내부 picture 분기 (#1322 maintainer fix) ===
                // hitTest 의 글상자 sentinel path (`cellIdx=0`) 가 넘어온 경우에는
                // Picture 를 body paragraph 의 sibling 으로 띄우지 않고, 실제 text_box
                // paragraph 안에 삽입한다. 글상자 내부 좌표계는 text_box content box
                // 기준이므로 caller 가 전달한 offset 은 Para-relative 로 해석한다.
                let (offset_x_hu, offset_y_hu) = match (paper_offset_x_hu, paper_offset_y_hu) {
                    (Some(x), Some(y)) => (x, y),
                    _ => (0, 0),
                };

                // CommonObjAttr (text_box 내부 floating):
                //   bits 3-4=vert_rel_to(2=Para), bits 8-10=horz_rel_to(3=Para),
                //   bits 15-17=width_criterion(4=Absolute),
                //   bits 18-20=height_criterion(2=Absolute),
                //   bits 21-23=text_wrap(0=Square)
                let common_attr: u32 = (2 << 3) | (3 << 8) | (4 << 15) | (2 << 18);
                let common = CommonObjAttr {
                    ctrl_id: 0x67736F20,
                    attr: common_attr,
                    treat_as_char: false,
                    vert_rel_to: VertRelTo::Para,
                    horz_rel_to: HorzRelTo::Para,
                    text_wrap: crate::model::shape::TextWrap::Square,
                    horizontal_offset: offset_x_hu.max(0) as u32,
                    vertical_offset: offset_y_hu.max(0) as u32,
                    width,
                    height,
                    z_order: 1,
                    description: description.to_string(),
                    ..Default::default()
                };
                let pic = Picture {
                    common,
                    shape_attr,
                    border_x: bx,
                    border_y: by,
                    crop,
                    image_attr,
                    ..Default::default()
                };

                let (new_ctrl_idx, logical_after) = {
                    let section = &mut self.document.sections[section_idx];
                    section.raw_stream = None;
                    let target_para =
                        Self::resolve_cell_paragraph_mut(section, para_idx, cell_path)?;
                    let new_ctrl_idx = target_para.controls.len();
                    target_para.controls.push(Control::Picture(Box::new(pic)));
                    target_para.ctrl_data_records.push(None);
                    target_para.control_mask |= 0x00000800;
                    let logical_positions =
                        super::super::helpers::find_logical_control_positions(target_para);
                    let logical_after = logical_positions
                        .get(new_ctrl_idx)
                        .copied()
                        .unwrap_or_else(|| target_para.text.chars().count())
                        + 1;
                    (new_ctrl_idx, logical_after)
                };

                self.mark_section_dirty(section_idx);
                self.recompose_section(section_idx);
                self.paginate_if_needed();
                self.invalidate_page_tree_cache();

                self.event_log.push(DocumentEvent::PictureInserted {
                    section: section_idx,
                    para: para_idx,
                });
                return Ok(super::super::helpers::json_ok_with(&format!(
                    "\"paraIdx\":{},\"controlIdx\":{},\"inner_control\":{},\"innerControl\":{},\"cellPathTarget\":true,\"logicalOffset\":{}",
                    para_idx, new_ctrl_idx, new_ctrl_idx, new_ctrl_idx, logical_after
                )));
            }

            // === 셀 floating picture 분기 (#1151 v2 — 한컴 패턴 정합) ===
            // Picture 는 표가 들어있는 paragraph 의 sibling control 로 append 된다.
            // tac=false, wrap=Square (어울림), horz/vert_rel_to=Paper, offset 은 사용자 클릭/드래그 위치.
            // [Task #1151 v8] 결함 A fix: 한컴 native default 가 Paper (incellpicture.hwp dump
            // 확인 — horz_rel_to=Paper offset=11845, vert_rel_to=Paper offset=15595).
            // [Task #1151 v8] 결함 C fix: 사용자가 클릭/드래그한 좌표 (paper-relative HU) 사용 —
            // 한컴 native 동작 정합. caller (studio) 가 None 전달 시 셀 좌상단 default.
            let (offset_x_hu, offset_y_hu) = match (paper_offset_x_hu, paper_offset_y_hu) {
                (Some(x), Some(y)) => (x, y),
                _ => self.compute_cell_page_offset(section_idx, para_idx, cell_path),
            };

            // CommonObjAttr (floating):
            //   bits 3-4=vert_rel_to(0=Paper), bits 8-10=horz_rel_to(0=Paper),
            //   bits 15-17=width_criterion(4=Absolute), bits 18-20=height_criterion(2=Absolute),
            //   bits 21-23=text_wrap(0=Square)
            let common_attr: u32 = (4 << 15) | (2 << 18);
            let common = CommonObjAttr {
                ctrl_id: 0x67736F20,
                attr: common_attr,
                treat_as_char: false,
                vert_rel_to: VertRelTo::Paper,
                horz_rel_to: HorzRelTo::Paper,
                text_wrap: crate::model::shape::TextWrap::Square,
                horizontal_offset: offset_x_hu.max(0) as u32,
                vertical_offset: offset_y_hu.max(0) as u32,
                width,
                height,
                z_order: 1,
                description: description.to_string(),
                ..Default::default()
            };
            let pic = Picture {
                common,
                shape_attr,
                border_x: bx,
                border_y: by,
                crop,
                image_attr,
                ..Default::default()
            };

            // table 같은 paragraph 의 sibling control 로 append.
            self.document.sections[section_idx].raw_stream = None;
            let parent = &mut self.document.sections[section_idx].paragraphs[para_idx];
            let new_ctrl_idx = parent.controls.len();
            parent.controls.push(Control::Picture(Box::new(pic)));
            parent.ctrl_data_records.push(None);
            let logical_positions = super::super::helpers::find_logical_control_positions(parent);
            let logical_after = logical_positions
                .get(new_ctrl_idx)
                .copied()
                .unwrap_or_else(|| parent.text.chars().count())
                + 1;

            // outer table dirty 마킹 (재측정 유도)
            let outer_ctrl = cell_path[0].0;
            if let Some(Control::Table(t)) = self.document.sections[section_idx].paragraphs
                [para_idx]
                .controls
                .get_mut(outer_ctrl)
            {
                t.dirty = true;
            }
            self.mark_section_dirty(section_idx);
            self.paginate_if_needed();
            // [Task #1151 v9 결함 F] page tree cache invalidate — v5 와 동일 결함 (다른
            // setter 들은 모두 호출하나 본 insert path 의 셀 분기만 누락). 두 picture
            // 연속 insert + toggle 시 cache stale → studio 화면 불일치.
            self.invalidate_page_tree_cache();

            self.event_log.push(DocumentEvent::PictureInserted {
                section: section_idx,
                para: para_idx,
            });
            return Ok(super::super::helpers::json_ok_with(&format!(
                "\"paraIdx\":{},\"controlIdx\":{},\"logicalOffset\":{}",
                para_idx, new_ctrl_idx, logical_after
            )));
        }

        // === 본문 floating picture 분기 (Task #1151 v9 결함 E — 셀 분기와 동일 패턴) ===
        //
        // 한컴 native 동작 (사용자 시연 2026-05-30): 본문 picture 신규 삽입 시
        // 글자처럼 취급 default = **미체크** (tac=false, floating). 셀 안 picture
        // 와 동일. 이전 rhwp 본문 path 는 새 paragraph 생성 + inline glyph (tac=true)
        // 로 만들어 한컴 default 와 불일치 — 재설계하여 셀 분기와 통합.
        let (offset_x_hu, offset_y_hu) = match (paper_offset_x_hu, paper_offset_y_hu) {
            (Some(x), Some(y)) => (x, y),
            _ => (0, 0),
        };

        // CommonObjAttr (floating, 셀 분기와 동일):
        //   bits 3-4=vert_rel_to(0=Paper), bits 8-10=horz_rel_to(0=Paper),
        //   bits 15-17=width_criterion(4=Absolute), bits 18-20=height_criterion(2=Absolute),
        //   bits 21-23=text_wrap(0=Square)
        let common_attr: u32 = (4 << 15) | (2 << 18);
        let common = CommonObjAttr {
            ctrl_id: 0x67736F20, // "gso " — GenShape
            attr: common_attr,
            treat_as_char: false,
            vert_rel_to: VertRelTo::Paper,
            horz_rel_to: HorzRelTo::Paper,
            text_wrap: crate::model::shape::TextWrap::Square,
            horizontal_offset: offset_x_hu.max(0) as u32,
            vertical_offset: offset_y_hu.max(0) as u32,
            width,
            height,
            z_order: 1,
            description: description.to_string(),
            ..Default::default()
        };

        let pic = Picture {
            common,
            shape_attr,
            border_x: bx,
            border_y: by,
            crop,
            image_attr,
            ..Default::default()
        };

        // 현재 paragraph 의 sibling control 로 append (새 paragraph 생성 X).
        self.document.sections[section_idx].raw_stream = None;
        let parent = &mut self.document.sections[section_idx].paragraphs[para_idx];
        let new_ctrl_idx = parent.controls.len();
        parent.controls.push(Control::Picture(Box::new(pic)));
        parent.ctrl_data_records.push(None);
        let logical_positions = super::super::helpers::find_logical_control_positions(parent);
        let logical_after = logical_positions
            .get(new_ctrl_idx)
            .copied()
            .unwrap_or_else(|| parent.text.chars().count())
            + 1;

        self.mark_section_dirty(section_idx);
        self.paginate_if_needed();
        // [Task #1151 v9 결함 F] page tree cache invalidate (v5 패턴).
        self.invalidate_page_tree_cache();

        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: para_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"logicalOffset\":{}",
            para_idx, new_ctrl_idx, logical_after
        )))
    }

    /// 머리말/꼬리말 내부 문단에 그림을 삽입한다.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_header_footer_picture_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        char_offset: usize,
        image_data: &[u8],
        width: u32,
        height: u32,
        natural_width_px: u32,
        natural_height_px: u32,
        extension: &str,
        description: &str,
        para_offset_x_hu: Option<i32>,
        para_offset_y_hu: Option<i32>,
    ) -> Result<String, HwpError> {
        use crate::model::bin_data::{
            BinData, BinDataCompression, BinDataContent, BinDataStatus, BinDataType,
        };
        use crate::model::image::{CropInfo, ImageAttr, ImageEffect, Picture};
        use crate::model::shape::{CommonObjAttr, HorzRelTo, ShapeComponentAttr, VertRelTo};

        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과 (총 {}개)",
                section_idx,
                self.document.sections.len()
            )));
        }
        if image_data.is_empty() {
            return Err(HwpError::RenderError(
                "이미지 데이터가 비어 있습니다".to_string(),
            ));
        }
        {
            let section = &self.document.sections[section_idx];
            let outer_para = section.paragraphs.get(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para.controls.get(outer_control_idx).ok_or_else(|| {
                HwpError::RenderError(format!(
                    "외부 컨트롤 인덱스 {} 범위 초과",
                    outer_control_idx
                ))
            })?;
            let inner_paras: &[Paragraph] = match outer_ctrl {
                Control::Header(header) => &header.paragraphs,
                Control::Footer(footer) => &footer.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            if char_offset > inner_para.text.chars().count() {
                return Err(HwpError::RenderError(format!(
                    "문자 오프셋 {} 범위 초과",
                    char_offset
                )));
            }
        }

        let next_id = self.document.bin_data_content.len() as u16 + 1;
        self.document.bin_data_content.push(BinDataContent {
            id: next_id,
            data: image_data.to_vec(),
            extension: extension.to_string(),
        });

        self.document.doc_info.bin_data_list.push(BinData {
            raw_data: None,
            attr: 0x0101,
            data_type: BinDataType::Embedding,
            compression: BinDataCompression::Default,
            status: BinDataStatus::Success,
            abs_path: None,
            rel_path: None,
            storage_id: next_id,
            extension: Some(extension.to_string()),
        });
        self.document.doc_info.raw_stream = None;

        let shape_attr = ShapeComponentAttr {
            original_width: width,
            original_height: height,
            current_width: width,
            current_height: height,
            local_file_version: 1,
            render_sx: 1.0,
            render_sy: 1.0,
            ..Default::default()
        };
        let bx = [0i32, 0, width as i32, 0];
        let by = [width as i32, height as i32, 0, height as i32];
        let crop = CropInfo {
            left: 0,
            top: 0,
            right: (natural_width_px * 75) as i32,
            bottom: (natural_height_px * 75) as i32,
        };
        let image_attr = ImageAttr {
            bin_data_id: next_id,
            brightness: 0,
            contrast: 0,
            effect: ImageEffect::RealPic,
            transparency: 0,
            external_path: None,
        };
        let common_attr: u32 = (2 << 3) | (3 << 8) | (4 << 15) | (2 << 18);
        let common = CommonObjAttr {
            ctrl_id: 0x67736F20,
            attr: common_attr,
            treat_as_char: false,
            vert_rel_to: VertRelTo::Para,
            horz_rel_to: HorzRelTo::Para,
            text_wrap: crate::model::shape::TextWrap::Square,
            horizontal_offset: para_offset_x_hu.unwrap_or(0).max(0) as u32,
            vertical_offset: para_offset_y_hu.unwrap_or(0).max(0) as u32,
            width,
            height,
            z_order: 1,
            description: description.to_string(),
            ..Default::default()
        };
        let pic = Picture {
            common,
            shape_attr,
            border_x: bx,
            border_y: by,
            crop,
            image_attr,
            ..Default::default()
        };

        let (new_ctrl_idx, logical_after, scope) = {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let (inner_paras, scope): (&mut Vec<Paragraph>, &str) = match outer_ctrl {
                Control::Header(header) => (&mut header.paragraphs, "header"),
                Control::Footer(footer) => (&mut footer.paragraphs, "footer"),
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let new_ctrl_idx = inner_para.controls.len();
            inner_para.controls.push(Control::Picture(Box::new(pic)));
            inner_para.ctrl_data_records.push(None);
            inner_para.control_mask |= 0x00000800;
            let logical_positions =
                super::super::helpers::find_logical_control_positions(inner_para);
            let logical_after = logical_positions
                .get(new_ctrl_idx)
                .copied()
                .unwrap_or_else(|| inner_para.text.chars().count())
                + 1;
            (new_ctrl_idx, logical_after, scope.to_string())
        };

        self.document.sections[section_idx].raw_stream = None;
        self.mark_section_dirty(section_idx);
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: outer_para_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"container_scope\":\"{}\",\"hf_para\":{},\"inner_control\":{},\"logicalOffset\":{}",
            outer_para_idx,
            outer_control_idx,
            scope,
            inner_para_idx,
            new_ctrl_idx,
            logical_after
        )))
    }

    /// 표 셀의 page-relative 좌상단 좌표를 HWPUNIT 단위로 계산 (#1151 floating).
    ///
    /// render tree 를 순회하여 cell_path 와 매칭되는 TableCell 노드를 찾고
    /// bbox.x / bbox.y (px) 를 HWPUNIT 으로 환산 (× 75).
    ///
    /// 매칭 실패 / 페이지 미빌드 시 (0, 0) fallback — picture 가 페이지 좌상단에
    /// 떠도 사용자가 드래그로 위치 조정 가능.
    pub(crate) fn compute_cell_page_offset(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path: &[(usize, usize, usize)],
    ) -> (i32, i32) {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};

        if cell_path.is_empty() {
            return (0, 0);
        }

        fn matches_cell_run(
            node: &RenderNode,
            parent_para: usize,
            path: &[(usize, usize, usize)],
        ) -> bool {
            if let RenderNodeType::TextRun(ref tr) = node.node_type {
                return tr.cell_context.as_ref().is_some_and(|ctx| {
                    ctx.parent_para_index == parent_para
                        && ctx.path.len() == path.len()
                        && ctx
                            .path
                            .iter()
                            .zip(path.iter())
                            .all(|(a, b)| a.control_index == b.0 && a.cell_index == b.1)
                });
            }
            for child in &node.children {
                if matches!(child.node_type, RenderNodeType::Table(_)) {
                    continue;
                }
                if matches_cell_run(child, parent_para, path) {
                    return true;
                }
            }
            false
        }

        fn find_cell(
            node: &RenderNode,
            parent_para: usize,
            path: &[(usize, usize, usize)],
        ) -> Option<(f64, f64)> {
            if let RenderNodeType::Table(_) = node.node_type {
                if matches_cell_run(node, parent_para, path) {
                    let target_cell = path.last().unwrap().1;
                    for child in node.children.iter() {
                        if let RenderNodeType::TableCell(ref tc) = child.node_type {
                            if tc.model_cell_index == Some(target_cell as u32) {
                                return Some((child.bbox.x, child.bbox.y));
                            }
                        }
                    }
                }
            }
            for child in &node.children {
                if let Some(found) = find_cell(child, parent_para, path) {
                    return Some(found);
                }
            }
            None
        }

        let total_pages = self.page_count();
        for p in 0..total_pages {
            if let Ok(tree) = self.build_page_tree(p) {
                if let Some((px, py)) = find_cell(&tree.root, parent_para_idx, cell_path) {
                    // px → HWPUNIT (1px = 75 HWPUNIT at 96 DPI 가정).
                    // 단, section_idx 가 의미 있는 단위 정합을 위해 section 자체의
                    // 보정은 호출 측 (Picture.horz/vert_rel_to=Page) 가 처리.
                    let _ = section_idx;
                    return ((px * 75.0) as i32, (py * 75.0) as i32);
                }
            }
        }
        (0, 0)
    }

    /// 표의 모든 셀 bbox를 반환한다 (네이티브).
    pub(crate) fn get_table_cell_bboxes_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<String, HwpError> {
        self.get_table_cell_bboxes_from_page(section_idx, parent_para_idx, control_idx, 0)
    }

    /// page_hint부터 탐색하여 표의 셀 bbox를 반환한다 (네이티브).
    /// page_hint에서 못 찾으면 앞쪽도 탐색한다 (페이지 분할된 표 대응).
    pub(crate) fn get_table_cell_bboxes_from_page(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        page_hint: usize,
    ) -> Result<String, HwpError> {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};

        // 렌더 트리에서 해당 표 노드를 찾아 셀 bbox를 수집
        fn find_table_cells(
            node: &RenderNode,
            sec: usize,
            ppi: usize,
            ci: usize,
            page_idx: usize,
            result: &mut Vec<String>,
        ) -> bool {
            if let RenderNodeType::Table(ref tn) = node.node_type {
                if tn.section_index == Some(sec)
                    && tn.para_index == Some(ppi)
                    && tn.control_index == Some(ci)
                {
                    for (_child_idx, child) in node.children.iter().enumerate() {
                        if let RenderNodeType::TableCell(ref cn) = child.node_type {
                            // cellIdx: 모델의 cells 배열에서 (row, col)로 검색한 인덱스
                            let model_cell_idx = cn.model_cell_index.unwrap_or(0) as usize;
                            result.push(format!(
                                "{{\"cellIdx\":{},\"row\":{},\"col\":{},\"rowSpan\":{},\"colSpan\":{},\"pageIndex\":{},\"x\":{:.1},\"y\":{:.1},\"w\":{:.1},\"h\":{:.1}}}",
                                model_cell_idx, cn.row, cn.col, cn.row_span, cn.col_span,
                                page_idx,
                                child.bbox.x, child.bbox.y, child.bbox.width, child.bbox.height
                            ));
                        }
                    }
                    return true; // 찾음
                }
            }
            for child in &node.children {
                if find_table_cells(child, sec, ppi, ci, page_idx, result) {
                    return true;
                }
            }
            false
        }

        let mut cells = Vec::new();
        let total_pages = self.page_count() as usize;
        let start = page_hint.min(total_pages.saturating_sub(1));

        // page_hint부터 뒤쪽 탐색
        let mut found = false;
        for page_num in start..total_pages {
            let tree = self.build_page_tree_cached(page_num as u32)?;
            if find_table_cells(
                &tree.root,
                section_idx,
                parent_para_idx,
                control_idx,
                page_num,
                &mut cells,
            ) {
                found = true;
            } else if found {
                break;
            }
        }

        // page_hint에서 못 찾았으면 앞쪽 탐색 (페이지 분할 표가 hint 이전 페이지에서 시작될 수 있음)
        if !found && start > 0 {
            for page_num in (0..start).rev() {
                let tree = self.build_page_tree_cached(page_num as u32)?;
                if find_table_cells(
                    &tree.root,
                    section_idx,
                    parent_para_idx,
                    control_idx,
                    page_num,
                    &mut cells,
                ) {
                    found = true;
                    // 이 페이지에서 찾음 — hint까지 다시 정방향 탐색하여 누락된 페이지 수집
                    for fwd in (page_num + 1)..=start {
                        let tree2 = self.build_page_tree_cached(fwd as u32)?;
                        if !find_table_cells(
                            &tree2.root,
                            section_idx,
                            parent_para_idx,
                            control_idx,
                            fwd,
                            &mut cells,
                        ) {
                            break;
                        }
                    }
                    break;
                }
            }
        }

        Ok(format!("[{}]", cells.join(",")))
    }

    // ── 글상자(Shape) CRUD ─────────────────────────────────

    /// CommonObjAttr → JSON 문자열 (Shape/Picture 공용 속성)
    fn common_obj_attr_to_json(c: &crate::model::shape::CommonObjAttr) -> String {
        let vert_rel = match c.vert_rel_to {
            crate::model::shape::VertRelTo::Paper => "Paper",
            crate::model::shape::VertRelTo::Page => "Page",
            crate::model::shape::VertRelTo::Para => "Para",
        };
        let vert_align = match c.vert_align {
            crate::model::shape::VertAlign::Top => "Top",
            crate::model::shape::VertAlign::Center => "Center",
            crate::model::shape::VertAlign::Bottom => "Bottom",
            crate::model::shape::VertAlign::Inside => "Inside",
            crate::model::shape::VertAlign::Outside => "Outside",
        };
        let horz_rel = match c.horz_rel_to {
            crate::model::shape::HorzRelTo::Paper => "Paper",
            crate::model::shape::HorzRelTo::Page => "Page",
            crate::model::shape::HorzRelTo::Column => "Column",
            crate::model::shape::HorzRelTo::Para => "Para",
        };
        let horz_align = match c.horz_align {
            crate::model::shape::HorzAlign::Left => "Left",
            crate::model::shape::HorzAlign::Center => "Center",
            crate::model::shape::HorzAlign::Right => "Right",
            crate::model::shape::HorzAlign::Inside => "Inside",
            crate::model::shape::HorzAlign::Outside => "Outside",
        };
        let text_wrap = match c.text_wrap {
            crate::model::shape::TextWrap::Square => "Square",
            crate::model::shape::TextWrap::Tight => "Tight",
            crate::model::shape::TextWrap::Through => "Through",
            crate::model::shape::TextWrap::TopAndBottom => "TopAndBottom",
            crate::model::shape::TextWrap::BehindText => "BehindText",
            crate::model::shape::TextWrap::InFrontOfText => "InFrontOfText",
        };
        let text_flow = Self::text_flow_json_name(c.text_flow);
        let numbering_type = Self::object_numbering_type_json_name(c.numbering_type);
        let width_criterion = Self::size_criterion_json_name(c.width_criterion);
        let height_criterion = Self::size_criterion_json_name(c.height_criterion);
        let desc_escaped = super::super::helpers::json_escape(&c.description);
        let dropcap_style =
            super::super::helpers::json_escape(c.dropcap_style.as_deref().unwrap_or("None"));
        let href = super::super::helpers::json_escape(c.href.as_deref().unwrap_or(""));
        format!(
            "\"width\":{},\"height\":{},\"treatAsChar\":{},\
             \"vertRelTo\":\"{}\",\"vertAlign\":\"{}\",\
             \"horzRelTo\":\"{}\",\"horzAlign\":\"{}\",\
             \"vertOffset\":{},\"horzOffset\":{},\
             \"textWrap\":\"{}\",\"restrictInPage\":{},\"allowOverlap\":{},\"sizeProtect\":{},\
             \"textFlow\":\"{}\",\"numberingType\":\"{}\",\"numberingTypeExplicit\":{},\
             \"lock\":{},\"widthCriterion\":\"{}\",\"heightCriterion\":\"{}\",\"href\":\"{}\",\
             \"zOrder\":{},\"instanceId\":{},\"instId\":{},\
             \"outerMarginLeft\":{},\"outerMarginTop\":{},\
             \"outerMarginRight\":{},\"outerMarginBottom\":{},\
             \"description\":\"{}\",\"dropcapStyle\":\"{}\"",
            c.width,
            c.height,
            c.treat_as_char,
            vert_rel,
            vert_align,
            horz_rel,
            horz_align,
            c.vertical_offset,
            c.horizontal_offset,
            text_wrap,
            c.flow_with_text,
            c.allow_overlap,
            c.size_protect,
            text_flow,
            numbering_type,
            c.numbering_type_explicit,
            c.lock,
            width_criterion,
            height_criterion,
            href,
            c.z_order,
            c.instance_id,
            c.inst_id,
            c.margin.left,
            c.margin.top,
            c.margin.right,
            c.margin.bottom,
            desc_escaped,
            dropcap_style,
        )
    }

    fn json_string_or_null(value: Option<&str>) -> String {
        serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
    }

    fn ole_shape_metadata_field(shape: &crate::model::shape::ShapeObject) -> String {
        let crate::model::shape::ShapeObject::Ole(ole) = shape else {
            return String::new();
        };
        format!(
            ",\"binDataId\":{},\"extentX\":{},\"extentY\":{},\"objectType\":{},\"drawAspect\":{},\"eqBaseLine\":{},\"hasMoniker\":{}",
            ole.bin_data_id,
            ole.extent_x,
            ole.extent_y,
            Self::json_string_or_null(ole.hwpx_object_type.as_deref()),
            Self::json_string_or_null(ole.hwpx_draw_aspect.as_deref()),
            Self::json_string_or_null(ole.hwpx_eq_base_line.as_deref()),
            Self::json_string_or_null(ole.hwpx_has_moniker.as_deref()),
        )
    }

    fn sync_ole_raw_tag_metadata(ole: &mut crate::model::shape::OleShape) {
        if ole.raw_tag_data.len() >= 16 {
            ole.raw_tag_data[4..8].copy_from_slice(&ole.extent_x.to_le_bytes());
            ole.raw_tag_data[8..12].copy_from_slice(&ole.extent_y.to_le_bytes());
            ole.raw_tag_data[12..16].copy_from_slice(&ole.bin_data_id.to_le_bytes());
        }
    }

    fn requested_ole_bin_data_id(props_json: &str) -> Option<u32> {
        Self::json_u32_field_any(
            props_json,
            &["binDataId", "bin_data_id", "binaryItemId", "binary_item_id"],
        )
    }

    fn validate_ole_bin_data_id_for_shape(
        &self,
        shape: &crate::model::shape::ShapeObject,
        requested_bin_data_id: Option<u32>,
    ) -> Result<(), HwpError> {
        let bin_data_id_exists = requested_bin_data_id
            .map(|bin_data_id| self.ole_bin_data_id_exists(bin_data_id))
            .unwrap_or(false);
        Self::validate_requested_ole_bin_data_id_for_shape(
            shape,
            requested_bin_data_id,
            bin_data_id_exists,
        )
    }

    fn ole_bin_data_id_exists(&self, bin_data_id: u32) -> bool {
        self.document
            .bin_data_content
            .iter()
            .any(|content| content.id as u32 == bin_data_id)
    }

    fn validate_requested_ole_bin_data_id_for_shape(
        shape: &crate::model::shape::ShapeObject,
        requested_bin_data_id: Option<u32>,
        bin_data_id_exists: bool,
    ) -> Result<(), HwpError> {
        let Some(bin_data_id) = requested_bin_data_id else {
            return Ok(());
        };
        if !matches!(shape, crate::model::shape::ShapeObject::Ole(_)) {
            return Ok(());
        }
        if bin_data_id == 0 {
            return Err(HwpError::RenderError(
                "OLE binDataId must reference an existing BinData entry: 0".to_string(),
            ));
        }
        if bin_data_id_exists {
            Ok(())
        } else {
            Err(HwpError::RenderError(format!(
                "OLE binDataId must reference an existing BinData entry: {}",
                bin_data_id
            )))
        }
    }

    fn apply_ole_shape_metadata_props(
        shape: &mut crate::model::shape::ShapeObject,
        props_json: &str,
    ) {
        let crate::model::shape::ShapeObject::Ole(ole) = shape else {
            return;
        };
        let mut raw_tag_needs_sync = false;
        if let Some(v) = Self::json_u32_field_any(
            props_json,
            &["binDataId", "bin_data_id", "binaryItemId", "binary_item_id"],
        ) {
            ole.bin_data_id = v;
            raw_tag_needs_sync = true;
        }
        if let Some(v) = Self::json_i32_field_any(props_json, &["extentX", "extent_x"]) {
            ole.extent_x = v.max(0);
            raw_tag_needs_sync = true;
        }
        if let Some(v) = Self::json_i32_field_any(props_json, &["extentY", "extent_y"]) {
            ole.extent_y = v.max(0);
            raw_tag_needs_sync = true;
        }
        if let Some(v) = Self::json_str_field_any(props_json, &["objectType", "object_type"]) {
            ole.hwpx_object_type = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(v) = Self::json_str_field_any(props_json, &["drawAspect", "draw_aspect"]) {
            ole.hwpx_draw_aspect = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(v) = Self::json_str_field_any(props_json, &["eqBaseLine", "eq_base_line"]) {
            ole.hwpx_eq_base_line = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(v) = Self::json_str_field_any(props_json, &["hasMoniker", "has_moniker"]) {
            ole.hwpx_has_moniker = if v.is_empty() { None } else { Some(v) };
        }
        if raw_tag_needs_sync {
            Self::sync_ole_raw_tag_metadata(ole);
        }
    }

    fn text_flow_json_name(value: crate::model::shape::TextFlow) -> &'static str {
        match value {
            crate::model::shape::TextFlow::BothSides => "BothSides",
            crate::model::shape::TextFlow::LeftOnly => "LeftOnly",
            crate::model::shape::TextFlow::RightOnly => "RightOnly",
            crate::model::shape::TextFlow::LargestOnly => "LargestOnly",
        }
    }

    fn text_flow_from_json_name(
        value: &str,
        fallback: crate::model::shape::TextFlow,
    ) -> crate::model::shape::TextFlow {
        match value {
            "BothSides" | "BOTH_SIDES" | "both_sides" | "bothSides" | "0" => {
                crate::model::shape::TextFlow::BothSides
            }
            "LeftOnly" | "LEFT_ONLY" | "left_only" | "leftOnly" | "1" => {
                crate::model::shape::TextFlow::LeftOnly
            }
            "RightOnly" | "RIGHT_ONLY" | "right_only" | "rightOnly" | "2" => {
                crate::model::shape::TextFlow::RightOnly
            }
            "LargestOnly" | "LARGEST_ONLY" | "largest_only" | "largestOnly" | "3" => {
                crate::model::shape::TextFlow::LargestOnly
            }
            _ => fallback,
        }
    }

    fn object_numbering_type_json_name(
        value: crate::model::shape::ObjectNumberingType,
    ) -> &'static str {
        match value {
            crate::model::shape::ObjectNumberingType::None => "None",
            crate::model::shape::ObjectNumberingType::Picture => "Picture",
            crate::model::shape::ObjectNumberingType::Table => "Table",
            crate::model::shape::ObjectNumberingType::Equation => "Equation",
        }
    }

    fn object_numbering_type_from_json_name(
        value: &str,
        fallback: crate::model::shape::ObjectNumberingType,
    ) -> crate::model::shape::ObjectNumberingType {
        match value {
            "None" | "NONE" | "none" | "" => crate::model::shape::ObjectNumberingType::None,
            "Picture" | "PICTURE" | "picture" => crate::model::shape::ObjectNumberingType::Picture,
            "Table" | "TABLE" | "table" => crate::model::shape::ObjectNumberingType::Table,
            "Equation" | "EQUATION" | "equation" => {
                crate::model::shape::ObjectNumberingType::Equation
            }
            _ => fallback,
        }
    }

    fn size_criterion_json_name(value: crate::model::shape::SizeCriterion) -> &'static str {
        match value {
            crate::model::shape::SizeCriterion::Paper => "Paper",
            crate::model::shape::SizeCriterion::Page => "Page",
            crate::model::shape::SizeCriterion::Column => "Column",
            crate::model::shape::SizeCriterion::Para => "Para",
            crate::model::shape::SizeCriterion::Absolute => "Absolute",
        }
    }

    fn size_criterion_from_json_name(
        value: &str,
        fallback: crate::model::shape::SizeCriterion,
    ) -> crate::model::shape::SizeCriterion {
        match value {
            "Paper" | "PAPER" | "paper" | "0" => crate::model::shape::SizeCriterion::Paper,
            "Page" | "PAGE" | "page" | "1" => crate::model::shape::SizeCriterion::Page,
            "Column" | "COLUMN" | "column" | "2" => crate::model::shape::SizeCriterion::Column,
            "Para" | "PARA" | "para" | "paragraph" | "3" => {
                crate::model::shape::SizeCriterion::Para
            }
            "Absolute" | "ABSOLUTE" | "absolute" | "4" => {
                crate::model::shape::SizeCriterion::Absolute
            }
            _ => fallback,
        }
    }

    /// JSON → CommonObjAttr 필드 업데이트 (Shape/Picture 공용)
    fn apply_common_obj_attr_from_json(
        c: &mut crate::model::shape::CommonObjAttr,
        props_json: &str,
    ) {
        use super::super::helpers::{json_bool, json_i16, json_str, json_u32};

        fn json_bool_alias(props_json: &str, primary: &str, alias: &str) -> Option<bool> {
            json_bool(props_json, primary).or_else(|| json_bool(props_json, alias))
        }

        fn json_i16_alias(props_json: &str, primary: &str, alias: &str) -> Option<i16> {
            json_i16(props_json, primary).or_else(|| json_i16(props_json, alias))
        }

        fn json_str_alias(props_json: &str, primary: &str, alias: &str) -> Option<String> {
            json_str(props_json, primary).or_else(|| json_str(props_json, alias))
        }

        fn json_u32_alias(props_json: &str, primary: &str, alias: &str) -> Option<u32> {
            json_u32(props_json, primary).or_else(|| json_u32(props_json, alias))
        }

        if let Some(w) = json_u32(props_json, "width") {
            c.width = w.max(MIN_SHAPE_SIZE);
        }
        if let Some(h) = json_u32(props_json, "height") {
            c.height = h.max(MIN_SHAPE_SIZE);
        }
        if let Some(tac) = json_bool_alias(props_json, "treatAsChar", "treat_as_char") {
            c.treat_as_char = tac;
            if tac {
                c.attr |= 0x01;
            } else {
                c.attr &= !0x01;
            }
        }
        if let Some(v) = json_str_alias(props_json, "vertRelTo", "vert_rel_to") {
            c.vert_rel_to = match v.as_str() {
                "Paper" => crate::model::shape::VertRelTo::Paper,
                "Page" => crate::model::shape::VertRelTo::Page,
                "Para" => crate::model::shape::VertRelTo::Para,
                _ => c.vert_rel_to,
            };
        }
        if let Some(v) = json_str_alias(props_json, "horzRelTo", "horz_rel_to") {
            c.horz_rel_to = match v.as_str() {
                "Paper" => crate::model::shape::HorzRelTo::Paper,
                "Page" => crate::model::shape::HorzRelTo::Page,
                "Column" => crate::model::shape::HorzRelTo::Column,
                "Para" => crate::model::shape::HorzRelTo::Para,
                _ => c.horz_rel_to,
            };
        }
        if let Some(v) = json_str_alias(props_json, "vertAlign", "vert_align") {
            c.vert_align = match v.as_str() {
                "Top" => crate::model::shape::VertAlign::Top,
                "Center" => crate::model::shape::VertAlign::Center,
                "Bottom" => crate::model::shape::VertAlign::Bottom,
                _ => c.vert_align,
            };
        }
        if let Some(v) = json_str_alias(props_json, "horzAlign", "horz_align") {
            c.horz_align = match v.as_str() {
                "Left" => crate::model::shape::HorzAlign::Left,
                "Center" => crate::model::shape::HorzAlign::Center,
                "Right" => crate::model::shape::HorzAlign::Right,
                _ => c.horz_align,
            };
        }
        if let Some(v) = json_str_alias(props_json, "textWrap", "text_wrap") {
            c.text_wrap = match v.as_str() {
                "Square" => crate::model::shape::TextWrap::Square,
                "Tight" => crate::model::shape::TextWrap::Tight,
                "Through" => crate::model::shape::TextWrap::Through,
                "TopAndBottom" => crate::model::shape::TextWrap::TopAndBottom,
                "BehindText" => crate::model::shape::TextWrap::BehindText,
                "InFrontOfText" => crate::model::shape::TextWrap::InFrontOfText,
                _ => c.text_wrap,
            };
        }
        if let Some(v) = json_str_alias(props_json, "textFlow", "text_flow") {
            c.text_flow = Self::text_flow_from_json_name(&v, c.text_flow);
        }
        if let Some(v) = json_str_alias(props_json, "numberingType", "numbering_type") {
            c.numbering_type = Self::object_numbering_type_from_json_name(&v, c.numbering_type);
            c.numbering_type_explicit = true;
        }
        if let Some(v) = json_bool_alias(
            props_json,
            "numberingTypeExplicit",
            "numbering_type_explicit",
        ) {
            c.numbering_type_explicit = v;
        }
        if let Some(v) = json_bool_alias(props_json, "lock", "locked") {
            c.lock = v;
        }
        if let Some(v) = json_str_alias(props_json, "widthCriterion", "width_criterion") {
            c.width_criterion = Self::size_criterion_from_json_name(&v, c.width_criterion);
        }
        if let Some(v) = json_str_alias(props_json, "heightCriterion", "height_criterion") {
            c.height_criterion = Self::size_criterion_from_json_name(&v, c.height_criterion);
        }
        if let Some(v) = Self::json_i32_field_any(props_json, &["zOrder", "z_order"]) {
            c.z_order = v;
        }
        if let Some(v) = json_u32_alias(props_json, "instanceId", "instance_id") {
            c.instance_id = v;
        }
        if let Some(v) = json_u32_alias(props_json, "instId", "inst_id") {
            c.inst_id = v;
        }
        if let Some(v) = json_str(props_json, "href") {
            c.href = if v.is_empty() { None } else { Some(v) };
        }
        if let Some(v) = json_bool_alias(props_json, "restrictInPage", "restrict_in_page") {
            c.flow_with_text = v;
            if v {
                c.attr |= 1 << 13;
                c.allow_overlap = false;
                c.attr &= !(1 << 14);
            } else {
                c.attr &= !(1 << 13);
            }
        }
        if let Some(v) = json_bool_alias(props_json, "allowOverlap", "allow_overlap") {
            c.allow_overlap = v;
            if v {
                c.attr |= 1 << 14;
            } else {
                c.attr &= !(1 << 14);
            }
        }
        if let Some(v) = json_bool_alias(props_json, "sizeProtect", "size_protect") {
            c.size_protect = v;
            if v {
                c.attr |= 1 << 20;
            } else {
                c.attr &= !(1 << 20);
            }
        }
        if c.flow_with_text {
            c.allow_overlap = false;
            c.attr &= !(1 << 14);
        }
        if let Some(v) = json_u32_alias(props_json, "vertOffset", "vert_offset") {
            c.vertical_offset = v;
        }
        if let Some(v) = json_u32_alias(props_json, "horzOffset", "horz_offset") {
            c.horizontal_offset = v;
        }
        if let Some(v) = json_str(props_json, "description") {
            c.description = v;
        }
        if let Some(v) = json_str_alias(props_json, "dropcapStyle", "dropcap_style") {
            c.dropcap_style = if v.is_empty() || v == "None" {
                None
            } else {
                Some(v)
            };
        }
        if let Some(v) = json_i16_alias(props_json, "outerMarginLeft", "outer_margin_left") {
            c.margin.left = v;
        }
        if let Some(v) = json_i16_alias(props_json, "outerMarginTop", "outer_margin_top") {
            c.margin.top = v;
        }
        if let Some(v) = json_i16_alias(props_json, "outerMarginRight", "outer_margin_right") {
            c.margin.right = v;
        }
        if let Some(v) = json_i16_alias(props_json, "outerMarginBottom", "outer_margin_bottom") {
            c.margin.bottom = v;
        }
        Self::sync_common_obj_attr_known_bits(c);
    }

    /// 글상자(Shape) 속성 조회 (네이티브).
    pub fn get_shape_properties_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<String, HwpError> {
        let shape = self.resolve_shape_control_ref(section_idx, parent_para_idx, control_idx)?;

        let c = shape.common();
        let common_json = Self::common_obj_attr_to_json(c);

        // TextBox 속성
        let tb_json = if let Some(tb) = get_textbox_from_shape(shape) {
            let va = match tb.vertical_align {
                crate::model::table::VerticalAlign::Top => "Top",
                crate::model::table::VerticalAlign::Center => "Center",
                crate::model::table::VerticalAlign::Bottom => "Bottom",
            };
            format!(
                ",\"tbMarginLeft\":{},\"tbMarginRight\":{},\"tbMarginTop\":{},\"tbMarginBottom\":{},\"tbVerticalAlign\":\"{}\"",
                tb.margin_left, tb.margin_right, tb.margin_top, tb.margin_bottom, va
            )
        } else {
            String::new()
        };

        // 테두리 / 회전 / 채우기 정보
        let drawing = shape.drawing();
        let extra_json = if let Some(d) = drawing {
            let sa = &d.shape_attr;
            let fill = &d.fill;
            let fill_type = match fill.fill_type {
                crate::model::style::FillType::None => "none",
                crate::model::style::FillType::Solid => "solid",
                crate::model::style::FillType::Gradient => "gradient",
                crate::model::style::FillType::Image => "image",
            };
            // borderAttr 비트필드 분해
            let bl = &d.border_line;
            let line_type = bl.attr & 0x3F; // bits 0-5: 선 종류 (0~17)
            let line_end_shape = (bl.attr >> 6) & 0x0F; // bits 6-9: 끝 모양
            let arrow_start = (bl.attr >> 10) & 0x3F; // bits 10-15: 화살표 시작 모양
            let arrow_end = (bl.attr >> 16) & 0x3F; // bits 16-21: 화살표 끝 모양
            let arrow_start_size = (bl.attr >> 22) & 0x0F; // bits 22-25: 화살표 시작 크기
            let arrow_end_size = (bl.attr >> 26) & 0x0F; // bits 26-29: 화살표 끝 크기

            let mut extra = format!(
                ",\"borderColor\":{},\"borderColorHex\":\"{}\",\"borderWidth\":{},\"borderAttr\":{},\"borderOutlineStyle\":{}\
                ,\"lineType\":{},\"lineEndShape\":{}\
                ,\"arrowStart\":{},\"arrowEnd\":{},\"arrowStartSize\":{},\"arrowEndSize\":{}\
                ,\"rotationAngle\":{},\"rotateImage\":{},\"horzFlip\":{},\"vertFlip\":{}\
                ,\"fillType\":\"{}\"",
                bl.color,
                Self::color_ref_to_hex(bl.color),
                bl.width,
                bl.attr,
                bl.outline_style,
                line_type, line_end_shape,
                arrow_start, arrow_end, arrow_start_size, arrow_end_size,
                sa.rotation_angle, sa.rotate_image, sa.horz_flip, sa.vert_flip,
                fill_type
            );
            // 단색 채우기
            if let Some(ref s) = fill.solid {
                extra.push_str(&format!(
                    ",\"fillBgColor\":{},\"fillBgColorHex\":\"{}\",\"fillPatColor\":{},\"fillPatColorHex\":\"{}\",\"fillPatType\":{}",
                    s.background_color,
                    Self::color_ref_to_hex(s.background_color),
                    s.pattern_color,
                    Self::color_ref_to_hex(s.pattern_color),
                    s.pattern_type
                ));
            }
            // 그러데이션 채우기
            if let Some(ref g) = fill.gradient {
                extra.push_str(&format!(
                    ",\"gradientType\":{},\"gradientAngle\":{},\"gradientCenterX\":{},\"gradientCenterY\":{},\"gradientBlur\":{}",
                    g.gradient_type, g.angle, g.center_x, g.center_y, g.blur
                ));
            }
            extra.push_str(&format!(",\"fillAlpha\":{}", fill.alpha));
            // 그림자
            extra.push_str(&format!(",\"shadowType\":{},\"shadowColor\":{},\"shadowOffsetX\":{},\"shadowOffsetY\":{},\"shadowAlpha\":{}",
                d.shadow_type, d.shadow_color, d.shadow_offset_x, d.shadow_offset_y, d.shadow_alpha));
            extra.push_str(&Self::shape_shadow_field(d));
            extra.push_str(&Self::shape_effects_field(d));
            extra.push_str(&Self::shape_raw_hwpx_child_xml_field(d));
            extra.push_str(&format!(
                ",\"scInstId\":{},\"groupLevel\":{}",
                d.inst_id, d.shape_attr.group_level
            ));
            extra
        } else {
            String::new()
        };

        // Rectangle 전용: 모서리 곡률
        let round_json = if let crate::model::shape::ShapeObject::Rectangle(ref rect) = shape {
            format!(",\"roundRate\":{}", rect.round_rate)
        } else {
            String::new()
        };

        // 연결선 타입 + 제어점 좌표 (꺽임/곡선 중간 마커용)
        let connector_json = if let crate::model::shape::ShapeObject::Line(ref line) = shape {
            if let Some(ref conn) = line.connector {
                // type=2 제어점의 평균 좌표 (꺽임 모서리 / 곡선 중간점)
                let ctrl2_pts: Vec<&crate::model::shape::ConnectorControlPoint> = conn
                    .control_points
                    .iter()
                    .filter(|cp| cp.point_type == 2)
                    .collect();
                if !ctrl2_pts.is_empty() {
                    let avg_x: i32 =
                        ctrl2_pts.iter().map(|p| p.x).sum::<i32>() / ctrl2_pts.len() as i32;
                    let avg_y: i32 =
                        ctrl2_pts.iter().map(|p| p.y).sum::<i32>() / ctrl2_pts.len() as i32;
                    format!(
                        ",\"connectorType\":{},\"connectorMidX\":{},\"connectorMidY\":{}",
                        conn.link_type as u32, avg_x, avg_y
                    )
                } else {
                    format!(",\"connectorType\":{}", conn.link_type as u32)
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let caption_json = Self::shape_caption_field(shape);
        let ole_json = Self::ole_shape_metadata_field(shape);

        Ok(format!(
            "{{{}{}{}{}{}{}{}}}",
            common_json, tb_json, extra_json, round_json, connector_json, caption_json, ole_json
        ))
    }

    /// 글상자(Shape) 속성 변경 (네이티브).
    pub fn set_shape_properties_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        use super::super::helpers::{json_bool, json_i32, json_str};

        let requested_ole_bin_data_id = Self::requested_ole_bin_data_id(props_json);
        {
            let shape =
                self.resolve_shape_control_ref(section_idx, parent_para_idx, control_idx)?;
            self.validate_ole_bin_data_id_for_shape(shape, requested_ole_bin_data_id)?;
        }
        let shape = self.resolve_shape_control_mut(section_idx, parent_para_idx, control_idx)?;

        // CommonObjAttr 업데이트
        // 리사이즈 핸들을 반대편으로 끌어당길 때 studio가 width/height=0 을 보내
        // 도형이 렌더러상 사라지는 버그 방어: 최소 크기 clamp.
        let c = shape.common_mut();
        let new_w =
            super::super::helpers::json_u32(props_json, "width").map(|w| w.max(MIN_SHAPE_SIZE));
        let new_h =
            super::super::helpers::json_u32(props_json, "height").map(|h| h.max(MIN_SHAPE_SIZE));
        Self::apply_common_obj_attr_from_json(c, props_json);

        // Polygon/Curve: original_width/height는 생성 시 값으로 유지해야 렌더러의
        // 스케일 팩터(sx = current/original)가 올바르게 동작한다.
        let is_polygon_or_curve = matches!(
            shape,
            crate::model::shape::ShapeObject::Polygon(_)
                | crate::model::shape::ShapeObject::Curve(_)
        );
        let saved_orig_w = if is_polygon_or_curve {
            shape.drawing().map(|d| d.shape_attr.original_width)
        } else {
            None
        };
        let saved_orig_h = if is_polygon_or_curve {
            shape.drawing().map(|d| d.shape_attr.original_height)
        } else {
            None
        };

        // ShapeComponentAttr 크기/회전/채우기 동기화
        if let Some(d) = shape.drawing_mut() {
            if let Some(w) = new_w {
                d.shape_attr.current_width = w;
                d.shape_attr.original_width = w;
            }
            if let Some(h) = new_h {
                d.shape_attr.current_height = h;
                d.shape_attr.original_height = h;
            }
            if let Some(v) = Self::json_u32_field_any(props_json, &["groupLevel", "group_level"]) {
                d.shape_attr.group_level = v.min(u16::MAX as u32) as u16;
            }

            // 회전/기울임
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["rotationAngle", "rotation_angle"])
            {
                d.shape_attr.rotation_angle = v as i16;
            }
            if let Some(v) = Self::json_bool_field_any(props_json, &["rotateImage", "rotate_image"])
            {
                d.shape_attr.rotate_image = v;
                if v {
                    d.shape_attr.flip |= 0x0008_0000;
                } else {
                    d.shape_attr.flip &= !0x0008_0000;
                }
            }
            // 대칭(flip)
            if let Some(v) = Self::json_bool_field_any(props_json, &["horzFlip", "horz_flip"]) {
                d.shape_attr.horz_flip = v;
                if v {
                    d.shape_attr.flip |= 1;
                } else {
                    d.shape_attr.flip &= !1;
                }
            }
            if let Some(v) = Self::json_bool_field_any(props_json, &["vertFlip", "vert_flip"]) {
                d.shape_attr.vert_flip = v;
                if v {
                    d.shape_attr.flip |= 2;
                } else {
                    d.shape_attr.flip &= !2;
                }
            }

            // 테두리 선 — 색상/굵기
            if let Some(v) = Self::json_css_color_ref_field(props_json, "borderColorHex")
                .or_else(|| Self::json_css_color_ref_field(props_json, "border_color_hex"))
            {
                d.border_line.color = v;
            } else if let Some(v) =
                Self::json_i32_field_any(props_json, &["borderColor", "border_color"])
            {
                d.border_line.color = v as u32;
            }
            if let Some(v) = Self::json_i32_field_any(props_json, &["borderWidth", "border_width"])
            {
                d.border_line.width = v;
            }
            if let Some(v) = Self::json_i32_field_any(
                props_json,
                &["borderOutlineStyle", "border_outline_style"],
            ) {
                d.border_line.outline_style = v.clamp(0, u8::MAX as i32) as u8;
            }

            // 테두리 선 — attr 비트필드 개별 필드 업데이트
            {
                let mut attr = Self::json_u32_field_any(props_json, &["borderAttr", "border_attr"])
                    .unwrap_or(d.border_line.attr);
                if let Some(v) = Self::json_i32_field_any(props_json, &["lineType", "line_type"]) {
                    attr = (attr & !0x3F) | ((v as u32) & 0x3F);
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["lineEndShape", "line_end_shape"])
                {
                    attr = (attr & !(0x0F << 6)) | (((v as u32) & 0x0F) << 6);
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["arrowStart", "arrow_start"])
                {
                    attr = (attr & !(0x3F << 10)) | (((v as u32) & 0x3F) << 10);
                }
                if let Some(v) = Self::json_i32_field_any(props_json, &["arrowEnd", "arrow_end"]) {
                    attr = (attr & !(0x3F << 16)) | (((v as u32) & 0x3F) << 16);
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["arrowStartSize", "arrow_start_size"])
                {
                    attr = (attr & !(0x0F << 22)) | (((v as u32) & 0x0F) << 22);
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["arrowEndSize", "arrow_end_size"])
                {
                    attr = (attr & !(0x0F << 26)) | (((v as u32) & 0x0F) << 26);
                }
                d.border_line.attr = attr;
            }

            // 채우기 (단색)
            if let Some(v) = Self::json_str_field_any(props_json, &["fillType", "fill_type"]) {
                d.fill.fill_type = match v.as_str() {
                    "solid" => crate::model::style::FillType::Solid,
                    "gradient" => crate::model::style::FillType::Gradient,
                    "image" => crate::model::style::FillType::Image,
                    _ => crate::model::style::FillType::None,
                };
            }
            if let Some(v) = Self::json_css_color_ref_field(props_json, "fillBgColorHex")
                .or_else(|| Self::json_css_color_ref_field(props_json, "fill_bg_color_hex"))
            {
                let solid = d.fill.solid.get_or_insert_with(|| {
                    crate::model::style::SolidFill {
                        pattern_type: -1, // -1 = 단색 채우기 (0은 채우기 없음)
                        ..Default::default()
                    }
                });
                solid.background_color = v;
            } else if let Some(v) =
                Self::json_i32_field_any(props_json, &["fillBgColor", "fill_bg_color"])
            {
                let solid = d.fill.solid.get_or_insert_with(|| {
                    crate::model::style::SolidFill {
                        pattern_type: -1, // -1 = 단색 채우기 (0은 채우기 없음)
                        ..Default::default()
                    }
                });
                solid.background_color = v as u32;
            }
            if let Some(v) = Self::json_css_color_ref_field(props_json, "fillPatColorHex")
                .or_else(|| Self::json_css_color_ref_field(props_json, "fill_pat_color_hex"))
            {
                let solid = d
                    .fill
                    .solid
                    .get_or_insert_with(|| crate::model::style::SolidFill {
                        pattern_type: -1,
                        ..Default::default()
                    });
                solid.pattern_color = v;
            } else if let Some(v) =
                Self::json_i32_field_any(props_json, &["fillPatColor", "fill_pat_color"])
            {
                let solid = d
                    .fill
                    .solid
                    .get_or_insert_with(|| crate::model::style::SolidFill {
                        pattern_type: -1,
                        ..Default::default()
                    });
                solid.pattern_color = v as u32;
            }
            if let Some(v) = Self::json_i32_field_any(props_json, &["fillPatType", "fill_pat_type"])
            {
                let solid = d
                    .fill
                    .solid
                    .get_or_insert_with(|| crate::model::style::SolidFill {
                        pattern_type: -1,
                        ..Default::default()
                    });
                solid.pattern_type = v;
            }
            if let Some(v) = Self::json_i32_field_any(props_json, &["fillAlpha", "fill_alpha"]) {
                d.fill.alpha = v as u8;
            }

            // 채우기 (그라디언트)
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientType", "gradient_type"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.gradient_type = v as i16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientAngle", "gradient_angle"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.angle = v as i16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientCenterX", "gradient_center_x"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.center_x = v as i16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientCenterY", "gradient_center_y"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.center_y = v as i16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientBlur", "gradient_blur"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.blur = v as i16;
            }

            // 그림자
            if let Some(v) = Self::json_u32_field_any(props_json, &["shadowType", "shadow_type"]) {
                d.shadow_type = v;
            }
            if let Some(v) = Self::json_css_color_ref_field_any(
                props_json,
                &["shadowColorHex", "shadow_color_hex"],
            ) {
                d.shadow_color = v;
            } else if let Some(v) =
                Self::json_i32_field_any(props_json, &["shadowColor", "shadow_color"])
            {
                d.shadow_color = v as u32;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["shadowOffsetX", "shadow_offset_x"])
            {
                d.shadow_offset_x = v;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["shadowOffsetY", "shadow_offset_y"])
            {
                d.shadow_offset_y = v;
            }
            if let Some(v) = Self::json_u32_field_any(props_json, &["shadowAlpha", "shadow_alpha"])
            {
                d.shadow_alpha = v.min(u8::MAX as u32) as u8;
            }
            Self::apply_shape_shadow_props(d, props_json);
            Self::apply_shape_raw_hwpx_child_xml_props(d, props_json);
            Self::apply_shape_effects_props(d, props_json);

            // TextBox 속성 업데이트
            if let Some(ref mut tb) = d.text_box {
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["tbMarginLeft", "tb_margin_left"])
                {
                    tb.margin_left = v as i16;
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["tbMarginRight", "tb_margin_right"])
                {
                    tb.margin_right = v as i16;
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["tbMarginTop", "tb_margin_top"])
                {
                    tb.margin_top = v as i16;
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["tbMarginBottom", "tb_margin_bottom"])
                {
                    tb.margin_bottom = v as i16;
                }
                if let Some(v) =
                    Self::json_str_field_any(props_json, &["tbVerticalAlign", "tb_vertical_align"])
                {
                    tb.vertical_align = match v.as_str() {
                        "Top" => crate::model::table::VerticalAlign::Top,
                        "Center" => crate::model::table::VerticalAlign::Center,
                        "Bottom" => crate::model::table::VerticalAlign::Bottom,
                        _ => tb.vertical_align,
                    };
                }
            }
        }

        // Rectangle 곡률
        if let crate::model::shape::ShapeObject::Rectangle(ref mut rect) = shape {
            if let Some(v) = Self::json_i32_field_any(props_json, &["roundRate", "round_rate"]) {
                rect.round_rate = v as u8;
            }
        }

        Self::apply_ole_shape_metadata_props(shape, props_json);

        // Rectangle 좌표 동기화
        if let crate::model::shape::ShapeObject::Rectangle(ref mut rect) = shape {
            let w = rect.common.width as i32;
            let h = rect.common.height as i32;
            rect.x_coords = [0, w, w, 0];
            rect.y_coords = [0, 0, h, h];
        }

        // Polygon/Curve: original_width/height 복원 (생성 시 값 유지 → 렌더러 스케일 팩터 정상화)
        if let Some(d) = shape.drawing_mut() {
            if let Some(w) = saved_orig_w {
                d.shape_attr.original_width = w;
            }
            if let Some(h) = saved_orig_h {
                d.shape_attr.original_height = h;
            }
        }

        Self::apply_shape_caption_props(shape, props_json);

        // Group 리사이즈: original_width 유지, current_width만 변경 (렌더러가 스케일 적용)
        // 한컴 방식: 자식은 변경하지 않고, 컨테이너의 current/original 비율로 스케일 결정
        if let crate::model::shape::ShapeObject::Group(ref mut group) = shape {
            if let Some(nw) = new_w {
                group.shape_attr.current_width = nw;
                // original_width는 유지 (스케일 기준)
            }
            if let Some(nh) = new_h {
                group.shape_attr.current_height = nh;
            }
            // 회전 중심 갱신
            group.shape_attr.rotation_center.x = (group.common.width / 2) as i32;
            group.shape_attr.rotation_center.y = (group.common.height / 2) as i32;
            // raw_rendering 초기화 → 직렬화 시 스케일 행렬 재생성
            group.shape_attr.raw_rendering = Vec::new();
        }

        // 리플로우 + 렌더 트리 캐시 무효화
        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: parent_para_idx,
            ctrl: control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// [Task #1138] Shape 속성 → JSON. get_shape_properties_native +
    /// get_cell_shape_properties_by_path_native 공유.
    fn format_shape_props_inner(
        shape: &crate::model::shape::ShapeObject,
    ) -> Result<String, HwpError> {
        let c = shape.common();
        let common_json = Self::common_obj_attr_to_json(c);

        // TextBox 속성
        let tb_json = if let Some(tb) = get_textbox_from_shape(shape) {
            let va = match tb.vertical_align {
                crate::model::table::VerticalAlign::Top => "Top",
                crate::model::table::VerticalAlign::Center => "Center",
                crate::model::table::VerticalAlign::Bottom => "Bottom",
            };
            format!(
                ",\"tbMarginLeft\":{},\"tbMarginRight\":{},\"tbMarginTop\":{},\"tbMarginBottom\":{},\"tbVerticalAlign\":\"{}\"",
                tb.margin_left, tb.margin_right, tb.margin_top, tb.margin_bottom, va
            )
        } else {
            String::new()
        };

        // 테두리 / 회전 / 채우기 정보
        let drawing = shape.drawing();
        let extra_json = if let Some(d) = drawing {
            let sa = &d.shape_attr;
            let fill = &d.fill;
            let fill_type = match fill.fill_type {
                crate::model::style::FillType::None => "none",
                crate::model::style::FillType::Solid => "solid",
                crate::model::style::FillType::Gradient => "gradient",
                crate::model::style::FillType::Image => "image",
            };
            let bl = &d.border_line;
            let line_type = bl.attr & 0x3F;
            let line_end_shape = (bl.attr >> 6) & 0x0F;
            let arrow_start = (bl.attr >> 10) & 0x3F;
            let arrow_end = (bl.attr >> 16) & 0x3F;
            let arrow_start_size = (bl.attr >> 22) & 0x0F;
            let arrow_end_size = (bl.attr >> 26) & 0x0F;

            let mut extra = format!(
                ",\"borderColor\":{},\"borderColorHex\":\"{}\",\"borderWidth\":{},\"borderAttr\":{},\"borderOutlineStyle\":{}\
                ,\"lineType\":{},\"lineEndShape\":{}\
                ,\"arrowStart\":{},\"arrowEnd\":{},\"arrowStartSize\":{},\"arrowEndSize\":{}\
                ,\"rotationAngle\":{},\"rotateImage\":{},\"horzFlip\":{},\"vertFlip\":{}\
                ,\"fillType\":\"{}\"",
                bl.color,
                Self::color_ref_to_hex(bl.color),
                bl.width,
                bl.attr,
                bl.outline_style,
                line_type, line_end_shape,
                arrow_start, arrow_end, arrow_start_size, arrow_end_size,
                sa.rotation_angle, sa.rotate_image, sa.horz_flip, sa.vert_flip,
                fill_type
            );
            if let Some(ref s) = fill.solid {
                extra.push_str(&format!(
                    ",\"fillBgColor\":{},\"fillBgColorHex\":\"{}\",\"fillPatColor\":{},\"fillPatColorHex\":\"{}\",\"fillPatType\":{}",
                    s.background_color,
                    Self::color_ref_to_hex(s.background_color),
                    s.pattern_color,
                    Self::color_ref_to_hex(s.pattern_color),
                    s.pattern_type
                ));
            }
            if let Some(ref g) = fill.gradient {
                extra.push_str(&format!(
                    ",\"gradientType\":{},\"gradientAngle\":{},\"gradientCenterX\":{},\"gradientCenterY\":{},\"gradientBlur\":{}",
                    g.gradient_type, g.angle, g.center_x, g.center_y, g.blur
                ));
            }
            extra.push_str(&format!(",\"fillAlpha\":{}", fill.alpha));
            extra.push_str(&format!(",\"shadowType\":{},\"shadowColor\":{},\"shadowOffsetX\":{},\"shadowOffsetY\":{},\"shadowAlpha\":{}",
                d.shadow_type, d.shadow_color, d.shadow_offset_x, d.shadow_offset_y, d.shadow_alpha));
            extra.push_str(&Self::shape_shadow_field(d));
            extra.push_str(&Self::shape_effects_field(d));
            extra.push_str(&Self::shape_raw_hwpx_child_xml_field(d));
            extra.push_str(&format!(
                ",\"scInstId\":{},\"groupLevel\":{}",
                d.inst_id, d.shape_attr.group_level
            ));
            extra
        } else {
            String::new()
        };

        let round_json = if let crate::model::shape::ShapeObject::Rectangle(ref rect) = shape {
            format!(",\"roundRate\":{}", rect.round_rate)
        } else {
            String::new()
        };

        let connector_json = if let crate::model::shape::ShapeObject::Line(ref line) = shape {
            if let Some(ref conn) = line.connector {
                let ctrl2_pts: Vec<&crate::model::shape::ConnectorControlPoint> = conn
                    .control_points
                    .iter()
                    .filter(|cp| cp.point_type == 2)
                    .collect();
                if !ctrl2_pts.is_empty() {
                    let avg_x: i32 =
                        ctrl2_pts.iter().map(|p| p.x).sum::<i32>() / ctrl2_pts.len() as i32;
                    let avg_y: i32 =
                        ctrl2_pts.iter().map(|p| p.y).sum::<i32>() / ctrl2_pts.len() as i32;
                    format!(
                        ",\"connectorType\":{},\"connectorMidX\":{},\"connectorMidY\":{}",
                        conn.link_type as u32, avg_x, avg_y
                    )
                } else {
                    format!(",\"connectorType\":{}", conn.link_type as u32)
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let caption_json = Self::shape_caption_field(shape);
        let ole_json = Self::ole_shape_metadata_field(shape);

        Ok(format!(
            "{{{}{}{}{}{}{}{}}}",
            common_json, tb_json, extra_json, round_json, connector_json, caption_json, ole_json
        ))
    }

    /// [Task #1138] Shape 속성 JSON 적용 (mutation only). 후처리 (recompose /
    /// paginate / cache invalidate / event log) 는 호출자 책임.
    /// set_shape_properties_native + set_cell_shape_properties_by_path_native 공유.
    fn apply_shape_props_inner(shape: &mut crate::model::shape::ShapeObject, props_json: &str) {
        use super::super::helpers::{json_bool, json_i32, json_str};

        let c = shape.common_mut();
        let new_w =
            super::super::helpers::json_u32(props_json, "width").map(|w| w.max(MIN_SHAPE_SIZE));
        let new_h =
            super::super::helpers::json_u32(props_json, "height").map(|h| h.max(MIN_SHAPE_SIZE));
        Self::apply_common_obj_attr_from_json(c, props_json);

        let is_polygon_or_curve = matches!(
            shape,
            crate::model::shape::ShapeObject::Polygon(_)
                | crate::model::shape::ShapeObject::Curve(_)
        );
        let saved_orig_w = if is_polygon_or_curve {
            shape.drawing().map(|d| d.shape_attr.original_width)
        } else {
            None
        };
        let saved_orig_h = if is_polygon_or_curve {
            shape.drawing().map(|d| d.shape_attr.original_height)
        } else {
            None
        };

        if let Some(d) = shape.drawing_mut() {
            if let Some(w) = new_w {
                d.shape_attr.current_width = w;
                d.shape_attr.original_width = w;
            }
            if let Some(h) = new_h {
                d.shape_attr.current_height = h;
                d.shape_attr.original_height = h;
            }
            if let Some(v) = Self::json_u32_field_any(props_json, &["groupLevel", "group_level"]) {
                d.shape_attr.group_level = v.min(u16::MAX as u32) as u16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["rotationAngle", "rotation_angle"])
            {
                d.shape_attr.rotation_angle = v as i16;
            }
            if let Some(v) = Self::json_bool_field_any(props_json, &["rotateImage", "rotate_image"])
            {
                d.shape_attr.rotate_image = v;
                if v {
                    d.shape_attr.flip |= 0x0008_0000;
                } else {
                    d.shape_attr.flip &= !0x0008_0000;
                }
            }
            if let Some(v) = Self::json_bool_field_any(props_json, &["horzFlip", "horz_flip"]) {
                d.shape_attr.horz_flip = v;
                if v {
                    d.shape_attr.flip |= 1;
                } else {
                    d.shape_attr.flip &= !1;
                }
            }
            if let Some(v) = Self::json_bool_field_any(props_json, &["vertFlip", "vert_flip"]) {
                d.shape_attr.vert_flip = v;
                if v {
                    d.shape_attr.flip |= 2;
                } else {
                    d.shape_attr.flip &= !2;
                }
            }
            if let Some(v) = Self::json_css_color_ref_field(props_json, "borderColorHex")
                .or_else(|| Self::json_css_color_ref_field(props_json, "border_color_hex"))
            {
                d.border_line.color = v;
            } else if let Some(v) =
                Self::json_i32_field_any(props_json, &["borderColor", "border_color"])
            {
                d.border_line.color = v as u32;
            }
            if let Some(v) = Self::json_i32_field_any(props_json, &["borderWidth", "border_width"])
            {
                d.border_line.width = v;
            }
            if let Some(v) = Self::json_i32_field_any(
                props_json,
                &["borderOutlineStyle", "border_outline_style"],
            ) {
                d.border_line.outline_style = v.clamp(0, u8::MAX as i32) as u8;
            }
            {
                let mut attr = Self::json_u32_field_any(props_json, &["borderAttr", "border_attr"])
                    .unwrap_or(d.border_line.attr);
                if let Some(v) = Self::json_i32_field_any(props_json, &["lineType", "line_type"]) {
                    attr = (attr & !0x3F) | ((v as u32) & 0x3F);
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["lineEndShape", "line_end_shape"])
                {
                    attr = (attr & !(0x0F << 6)) | (((v as u32) & 0x0F) << 6);
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["arrowStart", "arrow_start"])
                {
                    attr = (attr & !(0x3F << 10)) | (((v as u32) & 0x3F) << 10);
                }
                if let Some(v) = Self::json_i32_field_any(props_json, &["arrowEnd", "arrow_end"]) {
                    attr = (attr & !(0x3F << 16)) | (((v as u32) & 0x3F) << 16);
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["arrowStartSize", "arrow_start_size"])
                {
                    attr = (attr & !(0x0F << 22)) | (((v as u32) & 0x0F) << 22);
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["arrowEndSize", "arrow_end_size"])
                {
                    attr = (attr & !(0x0F << 26)) | (((v as u32) & 0x0F) << 26);
                }
                d.border_line.attr = attr;
            }
            if let Some(v) = Self::json_str_field_any(props_json, &["fillType", "fill_type"]) {
                d.fill.fill_type = match v.as_str() {
                    "solid" => crate::model::style::FillType::Solid,
                    "gradient" => crate::model::style::FillType::Gradient,
                    "image" => crate::model::style::FillType::Image,
                    _ => crate::model::style::FillType::None,
                };
            }
            if let Some(v) = Self::json_css_color_ref_field(props_json, "fillBgColorHex")
                .or_else(|| Self::json_css_color_ref_field(props_json, "fill_bg_color_hex"))
            {
                let solid = d
                    .fill
                    .solid
                    .get_or_insert_with(|| crate::model::style::SolidFill {
                        pattern_type: -1,
                        ..Default::default()
                    });
                solid.background_color = v;
            } else if let Some(v) =
                Self::json_i32_field_any(props_json, &["fillBgColor", "fill_bg_color"])
            {
                let solid = d
                    .fill
                    .solid
                    .get_or_insert_with(|| crate::model::style::SolidFill {
                        pattern_type: -1,
                        ..Default::default()
                    });
                solid.background_color = v as u32;
            }
            if let Some(v) = Self::json_css_color_ref_field(props_json, "fillPatColorHex")
                .or_else(|| Self::json_css_color_ref_field(props_json, "fill_pat_color_hex"))
            {
                let solid = d
                    .fill
                    .solid
                    .get_or_insert_with(|| crate::model::style::SolidFill {
                        pattern_type: -1,
                        ..Default::default()
                    });
                solid.pattern_color = v;
            } else if let Some(v) =
                Self::json_i32_field_any(props_json, &["fillPatColor", "fill_pat_color"])
            {
                let solid = d
                    .fill
                    .solid
                    .get_or_insert_with(|| crate::model::style::SolidFill {
                        pattern_type: -1,
                        ..Default::default()
                    });
                solid.pattern_color = v as u32;
            }
            if let Some(v) = Self::json_i32_field_any(props_json, &["fillPatType", "fill_pat_type"])
            {
                let solid = d
                    .fill
                    .solid
                    .get_or_insert_with(|| crate::model::style::SolidFill {
                        pattern_type: -1,
                        ..Default::default()
                    });
                solid.pattern_type = v;
            }
            if let Some(v) = Self::json_i32_field_any(props_json, &["fillAlpha", "fill_alpha"]) {
                d.fill.alpha = v as u8;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientType", "gradient_type"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.gradient_type = v as i16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientAngle", "gradient_angle"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.angle = v as i16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientCenterX", "gradient_center_x"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.center_x = v as i16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientCenterY", "gradient_center_y"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.center_y = v as i16;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["gradientBlur", "gradient_blur"])
            {
                let grad = d.fill.gradient.get_or_insert_with(Default::default);
                grad.blur = v as i16;
            }
            if let Some(v) = Self::json_u32_field_any(props_json, &["shadowType", "shadow_type"]) {
                d.shadow_type = v;
            }
            if let Some(v) = Self::json_css_color_ref_field_any(
                props_json,
                &["shadowColorHex", "shadow_color_hex"],
            ) {
                d.shadow_color = v;
            } else if let Some(v) =
                Self::json_i32_field_any(props_json, &["shadowColor", "shadow_color"])
            {
                d.shadow_color = v as u32;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["shadowOffsetX", "shadow_offset_x"])
            {
                d.shadow_offset_x = v;
            }
            if let Some(v) =
                Self::json_i32_field_any(props_json, &["shadowOffsetY", "shadow_offset_y"])
            {
                d.shadow_offset_y = v;
            }
            if let Some(v) = Self::json_u32_field_any(props_json, &["shadowAlpha", "shadow_alpha"])
            {
                d.shadow_alpha = v.min(u8::MAX as u32) as u8;
            }
            Self::apply_shape_shadow_props(d, props_json);
            Self::apply_shape_raw_hwpx_child_xml_props(d, props_json);
            Self::apply_shape_effects_props(d, props_json);
            if let Some(ref mut tb) = d.text_box {
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["tbMarginLeft", "tb_margin_left"])
                {
                    tb.margin_left = v as i16;
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["tbMarginRight", "tb_margin_right"])
                {
                    tb.margin_right = v as i16;
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["tbMarginTop", "tb_margin_top"])
                {
                    tb.margin_top = v as i16;
                }
                if let Some(v) =
                    Self::json_i32_field_any(props_json, &["tbMarginBottom", "tb_margin_bottom"])
                {
                    tb.margin_bottom = v as i16;
                }
                if let Some(v) =
                    Self::json_str_field_any(props_json, &["tbVerticalAlign", "tb_vertical_align"])
                {
                    tb.vertical_align = match v.as_str() {
                        "Top" => crate::model::table::VerticalAlign::Top,
                        "Center" => crate::model::table::VerticalAlign::Center,
                        "Bottom" => crate::model::table::VerticalAlign::Bottom,
                        _ => tb.vertical_align,
                    };
                }
            }
        }

        if let crate::model::shape::ShapeObject::Rectangle(ref mut rect) = shape {
            if let Some(v) = Self::json_i32_field_any(props_json, &["roundRate", "round_rate"]) {
                rect.round_rate = v as u8;
            }
        }

        Self::apply_ole_shape_metadata_props(shape, props_json);

        if let crate::model::shape::ShapeObject::Rectangle(ref mut rect) = shape {
            let w = rect.common.width as i32;
            let h = rect.common.height as i32;
            rect.x_coords = [0, w, w, 0];
            rect.y_coords = [0, 0, h, h];
        }

        if let Some(d) = shape.drawing_mut() {
            if let Some(w) = saved_orig_w {
                d.shape_attr.original_width = w;
            }
            if let Some(h) = saved_orig_h {
                d.shape_attr.original_height = h;
            }
        }

        Self::apply_shape_caption_props(shape, props_json);

        if let crate::model::shape::ShapeObject::Group(ref mut group) = shape {
            if let Some(nw) = new_w {
                group.shape_attr.current_width = nw;
            }
            if let Some(nh) = new_h {
                group.shape_attr.current_height = nh;
            }
            group.shape_attr.rotation_center.x = (group.common.width / 2) as i32;
            group.shape_attr.rotation_center.y = (group.common.height / 2) as i32;
            group.shape_attr.raw_rendering = Vec::new();
        }
    }

    /// [Task #1138] 표 셀 내 Shape 속성 조회 (by_path).
    /// [Task #1151 v4] 셀 안 picture 속성 조회 (cell_path 기반).
    /// `get_cell_shape_properties_by_path_native` Picture 버전.
    pub fn get_cell_picture_properties_by_path_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        // [Task #1171] 표 셀과 글상자(Shape text_box) 를 모두 처리하는 resolver 사용.
        // (기존 resolve_cell_by_path 는 마지막 세그먼트가 표 셀이어야 했음.)
        let cell_para = self.resolve_paragraph_by_path(section_idx, parent_para_idx, &path)?;
        let ctrl = cell_para.controls.get(inner_control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
        })?;
        let pic = match ctrl {
            Control::Picture(p) => p,
            _ => {
                return Err(HwpError::RenderError(
                    "지정된 셀 내 컨트롤이 그림이 아닙니다".to_string(),
                ))
            }
        };
        Self::format_picture_properties_json(pic)
    }

    pub fn get_cell_shape_properties_by_path_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        let cell_para = self.resolve_paragraph_by_path(section_idx, parent_para_idx, &path)?;
        let ctrl = cell_para.controls.get(inner_control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
        })?;
        let shape_ref = match ctrl {
            Control::Shape(s) => s.as_ref(),
            _ => {
                return Err(HwpError::RenderError(
                    "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                ))
            }
        };
        Self::format_shape_props_inner(shape_ref)
    }

    pub fn get_shape_group_child_properties_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
        group_child_path_json: &str,
    ) -> Result<String, HwpError> {
        let child_path = Self::parse_group_child_path_json(group_child_path_json)?;
        let shape = if let Some(cell_path_json) = cell_path_json {
            let path = Self::parse_cell_path_json(cell_path_json)?;
            let cell_para = self.resolve_paragraph_by_path(section_idx, parent_para_idx, &path)?;
            let inner_control_idx = inner_control_idx
                .ok_or_else(|| HwpError::RenderError("inner_control 이 필요합니다".to_string()))?;
            let ctrl = cell_para.controls.get(inner_control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
            })?;
            match ctrl {
                Control::Shape(shape) => shape.as_ref(),
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                    ))
                }
            }
        } else if let Some(inner_para_idx) = inner_para_idx {
            let section = self.document.sections.get(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", parent_para_idx))
            })?;
            let outer_ctrl = outer_para.controls.get(control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 컨트롤 인덱스 {} 범위 초과", control_idx))
            })?;
            let inner_paras: &[Paragraph] = match outer_ctrl {
                Control::Header(header) => &header.paragraphs,
                Control::Footer(footer) => &footer.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let inner_control_idx = inner_control_idx
                .ok_or_else(|| HwpError::RenderError("inner_control 이 필요합니다".to_string()))?;
            let inner_ctrl = inner_para.controls.get(inner_control_idx).ok_or_else(|| {
                HwpError::RenderError(format!(
                    "내부 컨트롤 인덱스 {} 범위 초과",
                    inner_control_idx
                ))
            })?;
            match inner_ctrl {
                Control::Shape(shape) => shape.as_ref(),
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                    ))
                }
            }
        } else {
            self.resolve_shape_control_ref(section_idx, parent_para_idx, control_idx)?
        };
        let child = Self::shape_group_child_ref(shape, &child_path)?;
        Self::format_shape_props_inner(child)
    }

    pub fn get_shape_group_child_picture_properties_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
        group_child_path_json: &str,
    ) -> Result<String, HwpError> {
        let child_path = Self::parse_group_child_path_json(group_child_path_json)?;
        let shape = if let Some(cell_path_json) = cell_path_json {
            let path = Self::parse_cell_path_json(cell_path_json)?;
            let cell_para = self.resolve_paragraph_by_path(section_idx, parent_para_idx, &path)?;
            let inner_control_idx = inner_control_idx
                .ok_or_else(|| HwpError::RenderError("inner_control 이 필요합니다".to_string()))?;
            let ctrl = cell_para.controls.get(inner_control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
            })?;
            match ctrl {
                Control::Shape(shape) => shape.as_ref(),
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                    ))
                }
            }
        } else if let Some(inner_para_idx) = inner_para_idx {
            let section = self.document.sections.get(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", parent_para_idx))
            })?;
            let outer_ctrl = outer_para.controls.get(control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 컨트롤 인덱스 {} 범위 초과", control_idx))
            })?;
            let inner_paras: &[Paragraph] = match outer_ctrl {
                Control::Header(header) => &header.paragraphs,
                Control::Footer(footer) => &footer.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let inner_para = inner_paras.get(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let inner_control_idx = inner_control_idx
                .ok_or_else(|| HwpError::RenderError("inner_control 이 필요합니다".to_string()))?;
            let inner_ctrl = inner_para.controls.get(inner_control_idx).ok_or_else(|| {
                HwpError::RenderError(format!(
                    "내부 컨트롤 인덱스 {} 범위 초과",
                    inner_control_idx
                ))
            })?;
            match inner_ctrl {
                Control::Shape(shape) => shape.as_ref(),
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                    ))
                }
            }
        } else {
            self.resolve_shape_control_ref(section_idx, parent_para_idx, control_idx)?
        };
        let picture = Self::shape_group_child_picture_ref(shape, &child_path)?;
        Self::format_picture_properties_json(picture)
    }

    /// [Task #1138] 표 셀 내 Shape 속성 변경 (by_path).
    /// [Task #1151 v4] 셀 안 picture 속성 변경 (cell_path 기반).
    ///
    /// `set_cell_shape_properties_by_path_native` 와 동일 패턴 — 셀 path 순회 후
    /// inner_control_idx 의 Picture 에 대해 `apply_picture_props_inner` 적용.
    /// v2 의 tac 토글 마이그레이션 path 는 본 셀 안 picture path 에서는 적용되지
    /// 않는다 (셀 안 inline picture 는 이미 셀 안 위치에 있고, 한컴은 inline→floating
    /// 자동 변환을 별도 path 로 처리. 본 PR 의 v2 scope 는 floating→inline 만).
    pub fn set_cell_picture_properties_by_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        use super::super::helpers::{json_bool, json_i32};

        let path = Self::parse_cell_path_json(cell_path_json)?;
        let restrict_change = json_bool(props_json, "restrictInPage");
        let restrict_enabled_by_this_call = restrict_change.unwrap_or(false);
        let clamp_horz =
            restrict_enabled_by_this_call || json_i32(props_json, "horzOffset").is_some();
        let clamp_vert =
            restrict_enabled_by_this_call || json_i32(props_json, "vertOffset").is_some();
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let current_para = Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
            let ctrl = current_para
                .controls
                .get_mut(inner_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
                })?;
            let pic = match ctrl {
                Control::Picture(p) => p,
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 셀 내 컨트롤이 그림이 아닙니다".to_string(),
                    ))
                }
            };
            Self::apply_picture_props_inner(pic, props_json);
        }
        let section = &mut self.document.sections[section_idx];
        Self::clamp_direct_owner_cell_picture_offsets(
            section,
            parent_para_idx,
            &path,
            inner_control_idx,
            clamp_horz,
            clamp_vert,
        )?;
        Self::sync_direct_owner_cell_for_picture(
            section,
            parent_para_idx,
            &path,
            inner_control_idx,
        )?;
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        let outer_table_ctrl = path.first().unwrap().0;
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: parent_para_idx,
            ctrl: outer_table_ctrl,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// [Task #1171 / PR #1254] 표 셀/글상자 내부 Picture 삭제 (cell_path 기반).
    pub fn delete_cell_picture_control_by_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let para = Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
            if inner_control_idx >= para.controls.len() {
                return Err(HwpError::RenderError(format!(
                    "셀 내 컨트롤 {} 범위 초과",
                    inner_control_idx
                )));
            }
            if !matches!(&para.controls[inner_control_idx], Control::Picture(_)) {
                return Err(HwpError::RenderError(
                    "지정된 셀 내 컨트롤이 그림이 아닙니다".to_string(),
                ));
            }

            let text_chars: Vec<char> = para.text.chars().collect();
            let mut ci = 0usize;
            let mut prev_end: u32 = 0;
            let mut gap_start: Option<u32> = None;
            'outer: for i in 0..text_chars.len() {
                let offset = if i < para.char_offsets.len() {
                    para.char_offsets[i]
                } else {
                    prev_end
                };
                while prev_end + 8 <= offset && ci < para.controls.len() {
                    if ci == inner_control_idx {
                        gap_start = Some(prev_end);
                        break 'outer;
                    }
                    ci += 1;
                    prev_end += 8;
                }
                let char_size: u32 = if text_chars[i] == '\t' {
                    8
                } else if text_chars[i].len_utf16() == 2 {
                    2
                } else {
                    1
                };
                prev_end = offset + char_size;
            }
            if gap_start.is_none() {
                while ci < para.controls.len() {
                    if ci == inner_control_idx {
                        gap_start = Some(prev_end);
                        break;
                    }
                    ci += 1;
                    prev_end += 8;
                }
            }

            if let Some(gs) = gap_start {
                let threshold = gs + 8;
                for offset in para.char_offsets.iter_mut() {
                    if *offset >= threshold {
                        *offset -= 8;
                    }
                }
            }

            para.controls.remove(inner_control_idx);
            if inner_control_idx < para.ctrl_data_records.len() {
                para.ctrl_data_records.remove(inner_control_idx);
            }
            if para.char_count >= 8 {
                para.char_count -= 8;
            }
            Self::reflow_paragraph_line_segs_after_control_delete(para, &self.styles, self.dpi);
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        let outer_ctrl = path.first().unwrap().0;
        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: parent_para_idx,
            ctrl: outer_ctrl,
        });
        Ok("{\"ok\":true}".to_string())
    }

    pub fn set_cell_shape_properties_by_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        let requested_ole_bin_data_id = Self::requested_ole_bin_data_id(props_json);
        let requested_ole_bin_data_id_exists = requested_ole_bin_data_id
            .map(|bin_data_id| self.ole_bin_data_id_exists(bin_data_id))
            .unwrap_or(false);
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let current_para = Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
            let ctrl = current_para
                .controls
                .get_mut(inner_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
                })?;
            let shape = match ctrl {
                Control::Shape(s) => s.as_mut(),
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                    ))
                }
            };
            Self::validate_requested_ole_bin_data_id_for_shape(
                shape,
                requested_ole_bin_data_id,
                requested_ole_bin_data_id_exists,
            )?;
            Self::apply_shape_props_inner(shape, props_json);
        }
        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        let outer_table_ctrl = path.first().unwrap().0;
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: parent_para_idx,
            ctrl: outer_table_ctrl,
        });
        Ok("{\"ok\":true}".to_string())
    }

    pub fn set_shape_group_child_properties_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
        group_child_path_json: &str,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let child_path = Self::parse_group_child_path_json(group_child_path_json)?;
        let requested_ole_bin_data_id = Self::requested_ole_bin_data_id(props_json);
        let requested_ole_bin_data_id_exists = requested_ole_bin_data_id
            .map(|bin_data_id| self.ole_bin_data_id_exists(bin_data_id))
            .unwrap_or(false);
        let mut event_ctrl = control_idx;
        {
            if let Some(cell_path_json) = cell_path_json {
                let path = Self::parse_cell_path_json(cell_path_json)?;
                event_ctrl = path.first().unwrap().0;
                let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                })?;
                let current_para =
                    Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
                let inner_control_idx = inner_control_idx.ok_or_else(|| {
                    HwpError::RenderError("inner_control 이 필요합니다".to_string())
                })?;
                let ctrl = current_para
                    .controls
                    .get_mut(inner_control_idx)
                    .ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "셀 내 컨트롤 {} 범위 초과",
                            inner_control_idx
                        ))
                    })?;
                let shape = match ctrl {
                    Control::Shape(shape) => shape.as_mut(),
                    _ => {
                        return Err(HwpError::RenderError(
                            "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                        ))
                    }
                };
                let child = Self::shape_group_child_mut(shape, &child_path)?;
                Self::validate_requested_ole_bin_data_id_for_shape(
                    child,
                    requested_ole_bin_data_id,
                    requested_ole_bin_data_id_exists,
                )?;
                Self::apply_shape_props_inner(child, props_json);
            } else if let Some(inner_para_idx) = inner_para_idx {
                let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                })?;
                let outer_para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", parent_para_idx))
                })?;
                let outer_ctrl = outer_para.controls.get_mut(control_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("외부 컨트롤 인덱스 {} 범위 초과", control_idx))
                })?;
                let inner_paras: &mut Vec<Paragraph> = match outer_ctrl {
                    Control::Header(header) => &mut header.paragraphs,
                    Control::Footer(footer) => &mut footer.paragraphs,
                    _ => {
                        return Err(HwpError::RenderError(
                            "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                        ))
                    }
                };
                let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
                })?;
                let inner_control_idx = inner_control_idx.ok_or_else(|| {
                    HwpError::RenderError("inner_control 이 필요합니다".to_string())
                })?;
                let inner_ctrl =
                    inner_para
                        .controls
                        .get_mut(inner_control_idx)
                        .ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "내부 컨트롤 인덱스 {} 범위 초과",
                                inner_control_idx
                            ))
                        })?;
                let shape = match inner_ctrl {
                    Control::Shape(shape) => shape.as_mut(),
                    _ => {
                        return Err(HwpError::RenderError(
                            "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                        ))
                    }
                };
                let child = Self::shape_group_child_mut(shape, &child_path)?;
                Self::validate_requested_ole_bin_data_id_for_shape(
                    child,
                    requested_ole_bin_data_id,
                    requested_ole_bin_data_id_exists,
                )?;
                Self::apply_shape_props_inner(child, props_json);
            } else {
                let shape =
                    self.resolve_shape_control_mut(section_idx, parent_para_idx, control_idx)?;
                let child = Self::shape_group_child_mut(shape, &child_path)?;
                Self::validate_requested_ole_bin_data_id_for_shape(
                    child,
                    requested_ole_bin_data_id,
                    requested_ole_bin_data_id_exists,
                )?;
                Self::apply_shape_props_inner(child, props_json);
            }
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: parent_para_idx,
            ctrl: event_ctrl,
        });
        Ok("{\"ok\":true}".to_string())
    }

    pub fn set_shape_group_child_picture_properties_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
        group_child_path_json: &str,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let child_path = Self::parse_group_child_path_json(group_child_path_json)?;
        let mut event_ctrl = control_idx;
        if let Some(cell_path_json) = cell_path_json {
            let path = Self::parse_cell_path_json(cell_path_json)?;
            event_ctrl = path.first().unwrap().0;
        }

        let caption_created = {
            let shape = self.shape_group_parent_shape_mut(
                section_idx,
                parent_para_idx,
                control_idx,
                cell_path_json,
                inner_para_idx,
                inner_control_idx,
            )?;
            let picture = Self::shape_group_child_picture_mut(shape, &child_path)?;
            Self::apply_picture_props_inner(picture, props_json)
        };

        if caption_created {
            crate::parser::assign_auto_numbers(&mut self.document);
            let shape = self.shape_group_parent_shape_mut(
                section_idx,
                parent_para_idx,
                control_idx,
                cell_path_json,
                inner_para_idx,
                inner_control_idx,
            )?;
            let picture = Self::shape_group_child_picture_mut(shape, &child_path)?;
            Self::finish_new_picture_caption(picture);
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: parent_para_idx,
            ctrl: event_ctrl,
        });

        if caption_created {
            let shape = self.shape_group_parent_shape_ref(
                section_idx,
                parent_para_idx,
                control_idx,
                cell_path_json,
                inner_para_idx,
                inner_control_idx,
            )?;
            let picture = Self::shape_group_child_picture_ref(shape, &child_path)?;
            let char_offset = picture
                .caption
                .as_ref()
                .and_then(|caption| caption.paragraphs.first())
                .map_or(0, |para| para.text.chars().count());
            Ok(format!(
                "{{\"ok\":true,\"captionCharOffset\":{}}}",
                char_offset
            ))
        } else {
            Ok("{\"ok\":true}".to_string())
        }
    }

    /// 표 셀/글상자 내부 Shape/OLE/Chart 삭제 (cell_path 기반).
    pub fn delete_cell_shape_control_by_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let para = Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
            if inner_control_idx >= para.controls.len() {
                return Err(HwpError::RenderError(format!(
                    "셀 내 컨트롤 {} 범위 초과",
                    inner_control_idx
                )));
            }
            if !matches!(&para.controls[inner_control_idx], Control::Shape(_)) {
                return Err(HwpError::RenderError(
                    "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                ));
            }

            let text_chars: Vec<char> = para.text.chars().collect();
            let mut ci = 0usize;
            let mut prev_end: u32 = 0;
            let mut gap_start: Option<u32> = None;
            'outer: for i in 0..text_chars.len() {
                let offset = if i < para.char_offsets.len() {
                    para.char_offsets[i]
                } else {
                    prev_end
                };
                while prev_end + 8 <= offset && ci < para.controls.len() {
                    if ci == inner_control_idx {
                        gap_start = Some(prev_end);
                        break 'outer;
                    }
                    ci += 1;
                    prev_end += 8;
                }
                let char_size: u32 = if text_chars[i] == '\t' {
                    8
                } else if text_chars[i].len_utf16() == 2 {
                    2
                } else {
                    1
                };
                prev_end = offset + char_size;
            }
            if gap_start.is_none() {
                while ci < para.controls.len() {
                    if ci == inner_control_idx {
                        gap_start = Some(prev_end);
                        break;
                    }
                    ci += 1;
                    prev_end += 8;
                }
            }

            if let Some(gs) = gap_start {
                let threshold = gs + 8;
                for offset in para.char_offsets.iter_mut() {
                    if *offset >= threshold {
                        *offset -= 8;
                    }
                }
            }

            para.controls.remove(inner_control_idx);
            if inner_control_idx < para.ctrl_data_records.len() {
                para.ctrl_data_records.remove(inner_control_idx);
            }
            if para.char_count >= 8 {
                para.char_count -= 8;
            }
            Self::reflow_paragraph_line_segs_after_control_delete(para, &self.styles, self.dpi);
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        let outer_ctrl = path.first().unwrap().0;
        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: parent_para_idx,
            ctrl: outer_ctrl,
        });
        Ok("{\"ok\":true}".to_string())
    }

    pub fn delete_shape_group_child_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
        group_child_path_json: &str,
    ) -> Result<String, HwpError> {
        let child_path = Self::parse_group_child_path_json(group_child_path_json)?;
        let mut event_ctrl = control_idx;
        {
            if let Some(cell_path_json) = cell_path_json {
                let path = Self::parse_cell_path_json(cell_path_json)?;
                event_ctrl = path.first().unwrap().0;
                let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                })?;
                let current_para =
                    Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
                let inner_control_idx = inner_control_idx.ok_or_else(|| {
                    HwpError::RenderError("inner_control 이 필요합니다".to_string())
                })?;
                let ctrl = current_para
                    .controls
                    .get_mut(inner_control_idx)
                    .ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "셀 내 컨트롤 {} 범위 초과",
                            inner_control_idx
                        ))
                    })?;
                let shape = match ctrl {
                    Control::Shape(shape) => shape.as_mut(),
                    _ => {
                        return Err(HwpError::RenderError(
                            "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                        ))
                    }
                };
                Self::remove_shape_group_child(shape, &child_path)?;
            } else if let Some(inner_para_idx) = inner_para_idx {
                let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                })?;
                let outer_para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", parent_para_idx))
                })?;
                let outer_ctrl = outer_para.controls.get_mut(control_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("외부 컨트롤 인덱스 {} 범위 초과", control_idx))
                })?;
                let inner_paras: &mut Vec<Paragraph> = match outer_ctrl {
                    Control::Header(header) => &mut header.paragraphs,
                    Control::Footer(footer) => &mut footer.paragraphs,
                    _ => {
                        return Err(HwpError::RenderError(
                            "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                        ))
                    }
                };
                let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
                })?;
                let inner_control_idx = inner_control_idx.ok_or_else(|| {
                    HwpError::RenderError("inner_control 이 필요합니다".to_string())
                })?;
                let inner_ctrl =
                    inner_para
                        .controls
                        .get_mut(inner_control_idx)
                        .ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "내부 컨트롤 인덱스 {} 범위 초과",
                                inner_control_idx
                            ))
                        })?;
                let shape = match inner_ctrl {
                    Control::Shape(shape) => shape.as_mut(),
                    _ => {
                        return Err(HwpError::RenderError(
                            "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                        ))
                    }
                };
                Self::remove_shape_group_child(shape, &child_path)?;
            } else {
                let shape =
                    self.resolve_shape_control_mut(section_idx, parent_para_idx, control_idx)?;
                Self::remove_shape_group_child(shape, &child_path)?;
            }
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: parent_para_idx,
            ctrl: event_ctrl,
        });
        Ok("{\"ok\":true}".to_string())
    }

    pub fn change_shape_group_child_z_order_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
        group_child_path_json: &str,
        operation: &str,
    ) -> Result<String, HwpError> {
        let child_path = Self::parse_group_child_path_json(group_child_path_json)?;
        let mut event_ctrl = control_idx;
        let (old_idx, new_idx, child_count);
        {
            let reordered = if let Some(cell_path_json) = cell_path_json {
                let path = Self::parse_cell_path_json(cell_path_json)?;
                event_ctrl = path.first().unwrap().0;
                let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                })?;
                let current_para =
                    Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
                let inner_control_idx = inner_control_idx.ok_or_else(|| {
                    HwpError::RenderError("inner_control 이 필요합니다".to_string())
                })?;
                let ctrl = current_para
                    .controls
                    .get_mut(inner_control_idx)
                    .ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "셀 내 컨트롤 {} 범위 초과",
                            inner_control_idx
                        ))
                    })?;
                let shape = match ctrl {
                    Control::Shape(shape) => shape.as_mut(),
                    _ => {
                        return Err(HwpError::RenderError(
                            "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                        ))
                    }
                };
                Self::reorder_shape_group_child(shape, &child_path, operation)?
            } else if let Some(inner_para_idx) = inner_para_idx {
                let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                })?;
                let outer_para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", parent_para_idx))
                })?;
                let outer_ctrl = outer_para.controls.get_mut(control_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("외부 컨트롤 인덱스 {} 범위 초과", control_idx))
                })?;
                let inner_paras: &mut Vec<Paragraph> = match outer_ctrl {
                    Control::Header(header) => &mut header.paragraphs,
                    Control::Footer(footer) => &mut footer.paragraphs,
                    _ => {
                        return Err(HwpError::RenderError(
                            "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                        ))
                    }
                };
                let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
                })?;
                let inner_control_idx = inner_control_idx.ok_or_else(|| {
                    HwpError::RenderError("inner_control 이 필요합니다".to_string())
                })?;
                let inner_ctrl =
                    inner_para
                        .controls
                        .get_mut(inner_control_idx)
                        .ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "내부 컨트롤 인덱스 {} 범위 초과",
                                inner_control_idx
                            ))
                        })?;
                let shape = match inner_ctrl {
                    Control::Shape(shape) => shape.as_mut(),
                    _ => {
                        return Err(HwpError::RenderError(
                            "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                        ))
                    }
                };
                Self::reorder_shape_group_child(shape, &child_path, operation)?
            } else {
                let shape =
                    self.resolve_shape_control_mut(section_idx, parent_para_idx, control_idx)?;
                Self::reorder_shape_group_child(shape, &child_path, operation)?
            };
            old_idx = reordered.0;
            new_idx = reordered.1;
            child_count = reordered.2;
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: parent_para_idx,
            ctrl: event_ctrl,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"groupChildIndex\":{},\"oldGroupChildIndex\":{},\"childCount\":{}",
            new_idx, old_idx, child_count
        )))
    }

    pub fn insert_shape_group_child_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_path_json: Option<&str>,
        inner_para_idx: Option<usize>,
        inner_control_idx: Option<usize>,
        parent_group_child_path_json: Option<&str>,
        child_index: Option<usize>,
        width: u32,
        height: u32,
        horz_offset: u32,
        vert_offset: u32,
        treat_as_char: bool,
        text_wrap_str: &str,
        shape_type: &str,
        line_flip_x: bool,
        line_flip_y: bool,
        polygon_points: &[crate::model::Point],
    ) -> Result<String, HwpError> {
        if width == 0 && height == 0 {
            return Err(HwpError::RenderError(
                "폭과 높이가 모두 0입니다".to_string(),
            ));
        }
        let parent_path = parent_group_child_path_json
            .map(Self::parse_group_child_path_json)
            .transpose()?;
        let parent_path_ref = parent_path.as_deref();
        let mut event_ctrl = control_idx;
        let (insert_idx, child_count);
        {
            let inserted = if let Some(cell_path_json) = cell_path_json {
                let path = Self::parse_cell_path_json(cell_path_json)?;
                event_ctrl = path.first().unwrap().0;
                let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                })?;
                let current_para =
                    Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
                let default_char_shape_id = current_para
                    .char_shapes
                    .first()
                    .map(|cs| cs.char_shape_id)
                    .unwrap_or(0);
                let default_para_shape_id = current_para.para_shape_id;
                let child = Self::build_shape_object_for_insert(
                    default_char_shape_id,
                    default_para_shape_id,
                    0,
                    width,
                    height,
                    horz_offset,
                    vert_offset,
                    treat_as_char,
                    text_wrap_str,
                    shape_type,
                    line_flip_x,
                    line_flip_y,
                    polygon_points,
                );
                let inner_control_idx = inner_control_idx.ok_or_else(|| {
                    HwpError::RenderError("inner_control 이 필요합니다".to_string())
                })?;
                let ctrl = current_para
                    .controls
                    .get_mut(inner_control_idx)
                    .ok_or_else(|| {
                        HwpError::RenderError(format!(
                            "셀 내 컨트롤 {} 범위 초과",
                            inner_control_idx
                        ))
                    })?;
                let shape = match ctrl {
                    Control::Shape(shape) => shape.as_mut(),
                    _ => {
                        return Err(HwpError::RenderError(
                            "지정된 셀 내 컨트롤이 Shape이 아닙니다".to_string(),
                        ))
                    }
                };
                Self::insert_shape_group_child(shape, parent_path_ref, child, child_index)?
            } else if let Some(inner_para_idx) = inner_para_idx {
                let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                })?;
                let outer_para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", parent_para_idx))
                })?;
                let outer_ctrl = outer_para.controls.get_mut(control_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("외부 컨트롤 인덱스 {} 범위 초과", control_idx))
                })?;
                let inner_paras: &mut Vec<Paragraph> = match outer_ctrl {
                    Control::Header(header) => &mut header.paragraphs,
                    Control::Footer(footer) => &mut footer.paragraphs,
                    _ => {
                        return Err(HwpError::RenderError(
                            "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                        ))
                    }
                };
                let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                    HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
                })?;
                let default_char_shape_id = inner_para
                    .char_shapes
                    .first()
                    .map(|cs| cs.char_shape_id)
                    .unwrap_or(0);
                let default_para_shape_id = inner_para.para_shape_id;
                let child = Self::build_shape_object_for_insert(
                    default_char_shape_id,
                    default_para_shape_id,
                    0,
                    width,
                    height,
                    horz_offset,
                    vert_offset,
                    treat_as_char,
                    text_wrap_str,
                    shape_type,
                    line_flip_x,
                    line_flip_y,
                    polygon_points,
                );
                let inner_control_idx = inner_control_idx.ok_or_else(|| {
                    HwpError::RenderError("inner_control 이 필요합니다".to_string())
                })?;
                let inner_ctrl =
                    inner_para
                        .controls
                        .get_mut(inner_control_idx)
                        .ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "내부 컨트롤 인덱스 {} 범위 초과",
                                inner_control_idx
                            ))
                        })?;
                let shape = match inner_ctrl {
                    Control::Shape(shape) => shape.as_mut(),
                    _ => {
                        return Err(HwpError::RenderError(
                            "지정된 내부 컨트롤이 Shape이 아닙니다".to_string(),
                        ))
                    }
                };
                Self::insert_shape_group_child(shape, parent_path_ref, child, child_index)?
            } else {
                let default_char_shape_id;
                let default_para_shape_id;
                {
                    let section = self.document.sections.get(section_idx).ok_or_else(|| {
                        HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
                    })?;
                    let para = section.paragraphs.get(parent_para_idx).ok_or_else(|| {
                        HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
                    })?;
                    default_char_shape_id = para
                        .char_shapes
                        .first()
                        .map(|cs| cs.char_shape_id)
                        .unwrap_or(0);
                    default_para_shape_id = para.para_shape_id;
                }
                let child = Self::build_shape_object_for_insert(
                    default_char_shape_id,
                    default_para_shape_id,
                    0,
                    width,
                    height,
                    horz_offset,
                    vert_offset,
                    treat_as_char,
                    text_wrap_str,
                    shape_type,
                    line_flip_x,
                    line_flip_y,
                    polygon_points,
                );
                let shape =
                    self.resolve_shape_control_mut(section_idx, parent_para_idx, control_idx)?;
                Self::insert_shape_group_child(shape, parent_path_ref, child, child_index)?
            };
            insert_idx = inserted.0;
            child_count = inserted.1;
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: parent_para_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"groupChildIndex\":{},\"childCount\":{},\"controlIdx\":{}",
            insert_idx, child_count, event_ctrl
        )))
    }

    /// 글상자(Shape) 삭제 (네이티브).
    ///
    /// delete_picture_control_native()와 동일한 패턴.
    pub fn delete_shape_control_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<String, HwpError> {
        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        let section = &mut self.document.sections[section_idx];
        if parent_para_idx >= section.paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                parent_para_idx
            )));
        }
        let para = &mut section.paragraphs[parent_para_idx];
        if control_idx >= para.controls.len() {
            return Err(HwpError::RenderError(format!(
                "컨트롤 인덱스 {} 범위 초과",
                control_idx
            )));
        }
        if !matches!(&para.controls[control_idx], Control::Shape(_)) {
            return Err(HwpError::RenderError(
                "지정된 컨트롤이 Shape이 아닙니다".to_string(),
            ));
        }

        // char_offsets 조정 (delete_picture_control_native와 동일)
        let text_chars: Vec<char> = para.text.chars().collect();
        let mut ci = 0usize;
        let mut prev_end: u32 = 0;
        let mut gap_start: Option<u32> = None;
        'outer: for i in 0..text_chars.len() {
            let offset = if i < para.char_offsets.len() {
                para.char_offsets[i]
            } else {
                prev_end
            };
            while prev_end + 8 <= offset && ci < para.controls.len() {
                if ci == control_idx {
                    gap_start = Some(prev_end);
                    break 'outer;
                }
                ci += 1;
                prev_end += 8;
            }
            let char_size: u32 = if text_chars[i] == '\t' {
                8
            } else if text_chars[i].len_utf16() == 2 {
                2
            } else {
                1
            };
            prev_end = offset + char_size;
        }
        if gap_start.is_none() {
            while ci < para.controls.len() {
                if ci == control_idx {
                    gap_start = Some(prev_end);
                    break;
                }
                ci += 1;
                prev_end += 8;
            }
        }
        if let Some(gs) = gap_start {
            let threshold = gs + 8;
            for offset in para.char_offsets.iter_mut() {
                if *offset >= threshold {
                    *offset -= 8;
                }
            }
        }

        para.controls.remove(control_idx);
        if control_idx < para.ctrl_data_records.len() {
            para.ctrl_data_records.remove(control_idx);
        }
        if para.char_count >= 8 {
            para.char_count -= 8;
        }

        // line_segs 재계산: 도형 높이가 반영된 line_segs를 텍스트 기반으로 리셋
        Self::reflow_paragraph_line_segs_after_control_delete(para, &self.styles, self.dpi);

        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: parent_para_idx,
            ctrl: control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// 커서 위치에 글상자(Rectangle + TextBox)를 삽입한다 (네이티브).
    pub fn create_shape_control_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
        width: u32,
        height: u32,
        horz_offset: u32,
        vert_offset: u32,
        treat_as_char: bool,
        text_wrap_str: &str,
        shape_type: &str,
        line_flip_x: bool,
        line_flip_y: bool,
        polygon_points: &[crate::model::Point],
    ) -> Result<String, HwpError> {
        use crate::model::paragraph::{CharShapeRef, LineSeg};
        use crate::model::shape::*;
        use crate::model::style::{Fill, ShapeBorderLine};

        // 유효성 검사
        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }
        if width == 0 && height == 0 {
            return Err(HwpError::RenderError(
                "폭과 높이가 모두 0입니다".to_string(),
            ));
        }

        let text_wrap = match text_wrap_str {
            "Square" => TextWrap::Square,
            "Tight" => TextWrap::Tight,
            "Through" => TextWrap::Through,
            "TopAndBottom" => TextWrap::TopAndBottom,
            "BehindText" => TextWrap::BehindText,
            "InFrontOfText" => TextWrap::InFrontOfText,
            _ => TextWrap::InFrontOfText,
        };

        // 커서 위치 문단의 속성 상속
        let current_para = &self.document.sections[section_idx].paragraphs[para_idx];
        let default_char_shape_id: u32 = current_para
            .char_shapes
            .first()
            .map(|cs| cs.char_shape_id)
            .unwrap_or(0);
        let default_para_shape_id: u16 = current_para.para_shape_id;

        // 편집 영역 폭
        let pd = &self.document.sections[section_idx].section_def.page_def;
        let content_width =
            (pd.width as i32 - pd.margin_left as i32 - pd.margin_right as i32).max(7200) as u32;

        // attr 비트 계산
        // 도형(line/ellipse/rectangle) 및 floating 글상자: 한컴 기본값 0x046A4000
        //   Paper/Top/Paper/Left/InFrontOfText + 절대크기 + allow_overlap + bit26
        // inline 글상자(treat_as_char=true): Para/Top/Column/Left/Square = 0x0A0210
        // [Task #1280 v2] 삽입 글상자는 한컴 정답값 floating(treat_as_char=false)+글앞으로(InFrontOfText).
        //   권위 샘플 samples/textbox-under-image.hwp 실측: 글상자 배치=글앞으로/Paper/Paper/false.
        //   serializer(control.rs:1768)는 common.attr!=0 이면 그대로 직렬화하므로 attr 와 enum 필드를
        //   함께 정합시킨다. treat_as_char=true 인 inline 글상자는 #1280 본편 동작을 그대로 보존.
        let inline_textbox = shape_type == "textbox" && treat_as_char;
        let mut attr: u32 = if inline_textbox { 0x0A0210 } else { 0x046A4000 };
        if treat_as_char {
            attr |= 0x01;
        }

        // --- 빈 문단 (글상자 내부용) ---
        let tb_inner_width = width.saturating_sub(1020); // 양쪽 여백 510+510
        let mut inner_raw_header_extra = vec![0u8; 10];
        inner_raw_header_extra[0..2].copy_from_slice(&1u16.to_le_bytes());
        inner_raw_header_extra[4..6].copy_from_slice(&1u16.to_le_bytes());
        let inner_para = Paragraph {
            text: String::new(),
            char_count: 1,
            char_count_msb: true,
            control_mask: 0,
            para_shape_id: default_para_shape_id,
            style_id: 0,
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: default_char_shape_id,
            }],
            line_segs: vec![LineSeg {
                text_start: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: tb_inner_width as i32,
                tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                ..Default::default()
            }],
            has_para_text: false,
            raw_header_extra: inner_raw_header_extra,
            ..Default::default()
        };

        // --- 도형 구조 조립 ---
        let w_i = width as i32;
        let h_i = height as i32;
        let new_z_order = self.max_shape_z_order_in_section(section_idx) + 1;

        // ctrl_id 결정
        let is_connector = shape_type.starts_with("connector-");
        let ctrl_id: u32 = match shape_type {
            "line"
            | "connector-straight"
            | "connector-stroke"
            | "connector-arc"
            | "connector-straight-arrow"
            | "connector-stroke-arrow"
            | "connector-arc-arrow" => {
                if is_connector {
                    0x24636f6c
                } else {
                    0x246c696e
                }
            } // '$col' or '$lin'
            "ellipse" => 0x24656c6c, // '$ell'
            "polygon" => 0x24706f6c, // '$pol'
            "arc" => 0x24617263,     // '$arc'
            _ => 0x24726563,         // '$rec' (rectangle, textbox)
        };

        // instance_id 생성: 고유 해시 (z_order 기반 + 위치/크기)
        let instance_id: u32 = {
            let mut h: u32 = 0x7de30000;
            h = h.wrapping_add(new_z_order as u32 * 0x100);
            h = h.wrapping_add(horz_offset.wrapping_mul(3));
            h = h.wrapping_add(vert_offset.wrapping_mul(7));
            h = h.wrapping_add(width);
            h = h.wrapping_add(height.wrapping_mul(0x1b));
            h |= 0x40000000; // bit30 설정 (한컴 호환)
            if h == 0 {
                h = 0x7de34b69;
            }
            h
        };

        let common = CommonObjAttr {
            ctrl_id,
            attr,
            vertical_offset: vert_offset,
            horizontal_offset: horz_offset,
            width,
            height,
            z_order: new_z_order,
            instance_id,
            margin: if shape_type == "textbox" {
                crate::model::Padding {
                    left: 283,
                    right: 283,
                    top: 283,
                    bottom: 283,
                }
            } else {
                crate::model::Padding {
                    left: 0,
                    right: 0,
                    top: 0,
                    bottom: 0,
                }
            },
            treat_as_char,
            // [Task #1280 v2] inline 글상자만 Para/Column(본문 기준), floating 글상자·도형은 Paper.
            vert_rel_to: if inline_textbox {
                VertRelTo::Para
            } else {
                VertRelTo::Paper
            },
            vert_align: VertAlign::Top,
            horz_rel_to: if inline_textbox {
                HorzRelTo::Column
            } else {
                HorzRelTo::Paper
            },
            horz_align: HorzAlign::Left,
            text_wrap,
            description: match shape_type {
                "line" => "선입니다.".to_string(),
                "ellipse" => "타원입니다.".to_string(),
                "rectangle" => "사각형입니다.".to_string(),
                "textbox" => "글상자입니다.".to_string(),
                "polygon" => "다각형입니다.".to_string(),
                "arc" => "호입니다.".to_string(),
                "connector-straight" => "직선 연결선입니다.".to_string(),
                "connector-stroke" => "꺾인 연결선입니다.".to_string(),
                "connector-arc" => "곡선 연결선입니다.".to_string(),
                _ => "그리기 개체.".to_string(),
            },
            ..Default::default()
        };

        let has_textbox = shape_type == "textbox";
        let has_fill = shape_type != "line" && !is_connector;

        let drawing = DrawingObjAttr {
            shape_attr: ShapeComponentAttr {
                ctrl_id,
                is_two_ctrl_id: true,
                original_width: width,
                original_height: height,
                current_width: width,
                current_height: height,
                local_file_version: 1,
                flip: 0x00080000, // 한컴 기본값
                rotation_center: crate::model::Point {
                    x: (width / 2) as i32,
                    y: (height / 2) as i32,
                },
                ..Default::default()
            },
            border_line: ShapeBorderLine {
                color: 0,
                width: 33,
                attr: 0xD1000041,
                outline_style: 0,
            },
            fill: if has_fill {
                Fill {
                    fill_type: crate::model::style::FillType::Solid,
                    solid: Some(crate::model::style::SolidFill {
                        background_color: 0x00FFFFFF,
                        pattern_color: 0,
                        pattern_type: -1,
                    }),
                    gradient: None,
                    image: None,
                    alpha: 0,
                }
            } else {
                Fill::default()
            },
            text_box: if has_textbox {
                Some(TextBox {
                    list_attr: 0x20,
                    vertical_all: false,
                    vertical_align: crate::model::table::VerticalAlign::Top,
                    margin_left: 283,
                    margin_right: 283,
                    margin_top: 283,
                    margin_bottom: 283,
                    max_width: width,
                    raw_list_header_extra: vec![0u8; 13],
                    paragraphs: vec![inner_para],
                })
            } else {
                None
            },
            // inst_id: 한컴 SubjectID 기준 = (CTRL_HEADER instance_id & 0x3FFFFFFF) + 1
            inst_id: (instance_id & 0x3FFFFFFF) + 1,
            ..Default::default()
        };

        let shape_obj = match shape_type {
            "line"
            | "connector-straight"
            | "connector-stroke"
            | "connector-arc"
            | "connector-straight-arrow"
            | "connector-stroke-arrow"
            | "connector-arc-arrow" => {
                // 드래그 방향에 따라 시작/끝점 결정
                let (sx, sy, ex, ey) = match (line_flip_x, line_flip_y) {
                    (false, false) => (0, 0, w_i, h_i), // 좌상→우하
                    (false, true) => (0, h_i, w_i, 0),  // 좌하→우상
                    (true, false) => (w_i, 0, 0, h_i),  // 우상→좌하
                    (true, true) => (w_i, h_i, 0, 0),   // 우하→좌상
                };
                let connector = if is_connector {
                    use crate::model::shape::{ConnectorControlPoint, ConnectorData, LinkLineType};
                    let link_type = match shape_type {
                        "connector-straight" => LinkLineType::StraightNoArrow,
                        "connector-straight-arrow" => LinkLineType::StraightOneWay,
                        "connector-stroke" => LinkLineType::StrokeNoArrow,
                        "connector-stroke-arrow" => LinkLineType::StrokeOneWay,
                        "connector-arc" => LinkLineType::ArcNoArrow,
                        "connector-arc-arrow" => LinkLineType::ArcOneWay,
                        _ => LinkLineType::StraightNoArrow,
                    };
                    // 꺽인/곡선 연결선: 한컴 호환 제어점 생성
                    // 구조: 시작앵커(type=3) + 중간점(type=2) + 끝앵커(type=26)
                    let control_points = match link_type {
                        LinkLineType::StrokeNoArrow
                        | LinkLineType::StrokeOneWay
                        | LinkLineType::StrokeBoth
                        | LinkLineType::ArcNoArrow
                        | LinkLineType::ArcOneWay
                        | LinkLineType::ArcBoth => {
                            vec![
                                ConnectorControlPoint {
                                    x: sx,
                                    y: sy,
                                    point_type: 3,
                                }, // 시작 앵커
                                ConnectorControlPoint {
                                    x: ex,
                                    y: sy,
                                    point_type: 2,
                                }, // 중간 (직각 꺾임)
                                ConnectorControlPoint {
                                    x: ex,
                                    y: ey,
                                    point_type: 26,
                                }, // 끝 앵커
                            ]
                        }
                        _ => Vec::new(),
                    };
                    Some(ConnectorData {
                        link_type,
                        start_subject_id: 0,
                        start_subject_index: 0,
                        end_subject_id: 0,
                        end_subject_index: 0,
                        control_points,
                        raw_trailing: vec![0x1a, 0, 0, 0, 0, 0], // 한컴 호환 패딩
                    })
                } else {
                    None
                };
                ShapeObject::Line(LineShape {
                    common,
                    drawing,
                    start: crate::model::Point { x: sx, y: sy },
                    end: crate::model::Point { x: ex, y: ey },
                    started_right_or_bottom: if is_connector {
                        false
                    } else {
                        line_flip_x || line_flip_y
                    },
                    connector,
                    raw_trailing: Vec::new(),
                })
            }
            "ellipse" => ShapeObject::Ellipse(EllipseShape {
                common,
                drawing,
                attr: 0,
                center: crate::model::Point {
                    x: w_i / 2,
                    y: h_i / 2,
                },
                axis1: crate::model::Point { x: w_i, y: h_i / 2 },
                axis2: crate::model::Point { x: w_i / 2, y: h_i },
                start1: crate::model::Point { x: w_i, y: h_i / 2 },
                end1: crate::model::Point { x: w_i, y: h_i / 2 },
                start2: crate::model::Point { x: w_i, y: h_i / 2 },
                end2: crate::model::Point { x: w_i, y: h_i / 2 },
                raw_trailing: Vec::new(),
            }),
            "polygon" => {
                let points = if !polygon_points.is_empty() {
                    polygon_points.to_vec()
                } else {
                    // 기본 삼각형 (bbox 내접)
                    vec![
                        crate::model::Point { x: w_i / 2, y: 0 },
                        crate::model::Point { x: w_i, y: h_i },
                        crate::model::Point { x: 0, y: h_i },
                    ]
                };
                ShapeObject::Polygon(PolygonShape {
                    common,
                    drawing,
                    points,
                    raw_trailing: Vec::new(),
                })
            }
            "arc" => {
                // 사각형에 내접하는 타원의 1/4 호 (우상 사분면)
                // center: bbox 중심, axis1: 우측 중앙, axis2: 상단 중앙
                ShapeObject::Arc(ArcShape {
                    common,
                    drawing,
                    arc_type: 0, // 0=Arc
                    center: crate::model::Point {
                        x: w_i / 2,
                        y: h_i / 2,
                    },
                    axis1: crate::model::Point { x: w_i, y: h_i / 2 },
                    axis2: crate::model::Point { x: w_i / 2, y: 0 },
                    raw_trailing: Vec::new(),
                })
            }
            _ => ShapeObject::Rectangle(RectangleShape {
                common,
                drawing,
                round_rate: 0,
                x_coords: [0, w_i, w_i, 0],
                y_coords: [0, 0, h_i, h_i],
                raw_trailing: Vec::new(),
            }),
        };

        // --- 기존 문단에 인라인 컨트롤로 삽입 ---
        self.document.sections[section_idx].raw_stream = None;

        let insert_para_idx = para_idx;
        let insert_ctrl_idx;
        {
            let paragraph = &mut self.document.sections[section_idx].paragraphs[para_idx];

            // 컨트롤 삽입 위치 결정 (char_offset 기준)
            let insert_idx = {
                let positions =
                    crate::document_core::helpers::find_control_text_positions(paragraph);
                let mut idx = paragraph.controls.len();
                for (i, &pos) in positions.iter().enumerate() {
                    if pos > char_offset {
                        idx = i;
                        break;
                    }
                }
                idx
            };

            // 컨트롤 추가
            paragraph
                .controls
                .insert(insert_idx, Control::Shape(Box::new(shape_obj)));
            paragraph.ctrl_data_records.insert(insert_idx, None);

            // char_offsets에 raw offset 삽입
            if !paragraph.char_offsets.is_empty() {
                let raw_offset = if insert_idx > 0 && insert_idx <= paragraph.char_offsets.len() {
                    paragraph.char_offsets[insert_idx - 1] + 8
                } else if !paragraph.char_offsets.is_empty() {
                    let first = paragraph.char_offsets[0];
                    if first >= 8 {
                        first - 8
                    } else {
                        0
                    }
                } else {
                    (char_offset * 2) as u32
                };
                paragraph.char_offsets.insert(insert_idx, raw_offset);
            }

            // 삽입된 컨트롤 이후의 char_offsets를 8만큼 증가 (텍스트 매핑 유지)
            for co in paragraph.char_offsets.iter_mut().skip(insert_idx + 1) {
                *co += 8;
            }

            // char_count 갱신 (확장 컨트롤 = 8 code units)
            paragraph.char_count += 8;

            // control_mask에 GSO 비트 설정
            paragraph.control_mask |= 0x00000800;
            // has_para_text 보장
            paragraph.has_para_text = true;
            insert_ctrl_idx = insert_idx;
        }

        // 리플로우 + 페이지네이션
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: insert_para_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{}",
            insert_para_idx, insert_ctrl_idx
        )))
    }

    /// 글상자(Shape) z-order 변경 (네이티브).
    /// operation: "front" | "back" | "forward" | "backward"
    pub fn change_shape_z_order_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        operation: &str,
    ) -> Result<String, HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;

        // 구역 내 모든 Shape의 (z_order, para_idx, ctrl_idx) 수집
        let mut shape_infos: Vec<(i32, usize, usize)> = Vec::new();
        for (pi, para) in section.paragraphs.iter().enumerate() {
            for (ci, ctrl) in para.controls.iter().enumerate() {
                if let Control::Shape(shape) = ctrl {
                    shape_infos.push((shape.z_order(), pi, ci));
                }
            }
        }

        // (z_order, para_idx, ctrl_idx) 기준 정렬 — 렌더링 순서와 동일
        shape_infos.sort();

        let target_pos = shape_infos
            .iter()
            .position(|&(_, pi, ci)| pi == para_idx && ci == control_idx)
            .ok_or_else(|| HwpError::RenderError("대상 Shape를 찾을 수 없습니다".to_string()))?;
        let current_z = shape_infos[target_pos].0;
        let last_pos = shape_infos.len() - 1;

        // (대상 새 z_order, 이웃 변경 정보 Option<(para_idx, ctrl_idx, 새 z_order)>)
        let changes: Option<(i32, Option<(usize, usize, i32)>)> = match operation {
            "front" => {
                if target_pos == last_pos {
                    None // 이미 맨 앞
                } else {
                    let max_z = shape_infos[last_pos].0;
                    Some((max_z + 1, None))
                }
            }
            "back" => {
                if target_pos == 0 {
                    None // 이미 맨 뒤
                } else {
                    let min_z = shape_infos[0].0;
                    Some((min_z - 1, None))
                }
            }
            "forward" => {
                if target_pos >= last_pos {
                    None // 이미 맨 앞
                } else {
                    let neighbor = shape_infos[target_pos + 1];
                    if current_z == neighbor.0 {
                        // 같은 z_order — 대상만 +1하여 이웃 위로 이동
                        Some((current_z + 1, None))
                    } else {
                        // 다른 z_order — 이웃과 z_order 교환
                        Some((neighbor.0, Some((neighbor.1, neighbor.2, current_z))))
                    }
                }
            }
            "backward" => {
                if target_pos == 0 {
                    None // 이미 맨 뒤
                } else {
                    let neighbor = shape_infos[target_pos - 1];
                    if current_z == neighbor.0 {
                        // 같은 z_order — 대상만 -1하여 이웃 아래로 이동
                        Some((current_z - 1, None))
                    } else {
                        // 다른 z_order — 이웃과 z_order 교환
                        Some((neighbor.0, Some((neighbor.1, neighbor.2, current_z))))
                    }
                }
            }
            _ => {
                return Err(HwpError::RenderError(format!(
                    "알 수 없는 operation: {}",
                    operation
                )))
            }
        };

        let (new_z, neighbor_change) = match changes {
            Some(c) => c,
            None => {
                return Ok(super::super::helpers::json_ok_with(&format!(
                    "\"zOrder\":{}",
                    current_z
                )))
            }
        };

        // z_order 변경: 대상 + 이웃
        {
            let section = &mut self.document.sections[section_idx];
            if let Control::Shape(shape) = &mut section.paragraphs[para_idx].controls[control_idx] {
                shape.common_mut().z_order = new_z;
            }
            if let Some((n_pi, n_ci, n_z)) = neighbor_change {
                if let Control::Shape(shape) = &mut section.paragraphs[n_pi].controls[n_ci] {
                    shape.common_mut().z_order = n_z;
                }
            }
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        Ok(super::super::helpers::json_ok_with(&format!(
            "\"zOrder\":{}",
            new_z
        )))
    }

    /// 연결선의 SubjectID를 갱신한다 (연결선 생성 후 호출)
    pub fn update_connector_subject_ids(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        start_subject_id: u32,
        start_subject_index: u32,
        end_subject_id: u32,
        end_subject_index: u32,
    ) {
        if let Some(section) = self.document.sections.get_mut(section_idx) {
            if let Some(para) = section.paragraphs.get_mut(para_idx) {
                if let Some(Control::Shape(ref mut shape)) = para.controls.get_mut(control_idx) {
                    if let ShapeObject::Line(ref mut line) = shape.as_mut() {
                        if let Some(ref mut conn) = line.connector {
                            conn.start_subject_id = start_subject_id;
                            conn.start_subject_index = start_subject_index;
                            conn.end_subject_id = end_subject_id;
                            conn.end_subject_index = end_subject_index;
                        }
                    }
                }
            }
        }
    }

    /// 연결선 제어점을 연결점 방향에 따라 재계산한다.
    /// start_idx/end_idx: 0=상, 1=우, 2=하, 3=좌
    pub fn recalculate_connector_routing(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        start_idx: u32,
        end_idx: u32,
    ) {
        use crate::model::shape::ConnectorControlPoint;

        let section = match self.document.sections.get_mut(section_idx) {
            Some(s) => s,
            None => return,
        };
        let para = match section.paragraphs.get_mut(para_idx) {
            Some(p) => p,
            None => return,
        };
        let ctrl = match para.controls.get_mut(control_idx) {
            Some(c) => c,
            None => return,
        };

        let line = match ctrl {
            Control::Shape(ref mut s) => match s.as_mut() {
                ShapeObject::Line(ref mut l) => l,
                _ => return,
            },
            _ => return,
        };

        let conn = match &mut line.connector {
            Some(c) => c,
            None => return,
        };

        let sx = line.start.x;
        let sy = line.start.y;
        let ex = line.end.x;
        let ey = line.end.y;
        let w = line.common.width as i32;
        let h = line.common.height as i32;

        // 직선 연결선: 제어점 불필요
        if !conn.link_type.is_stroke() && !conn.link_type.is_arc() {
            conn.control_points.clear();
            return;
        }

        // 연결점 방향: 0=상, 1=우, 2=하, 3=좌
        if conn.link_type.is_arc() {
            // ─── 곡선 연결선: 파워포인트 스타일 S곡선 ───
            // ctrl1: 시작점에서 시작 방향으로 중간지점까지 뻗음
            // ctrl2: 끝점에서 끝 방향으로 중간지점까지 뻗음
            // → 중간지점에서 위아래(또는 좌우)가 반전되는 S자
            // 한컴 공식: 수평 연결(우/좌)은 midX 기준, 수직 연결(상/하)은 midY 기준
            // ctrl1 = (midX, startY) / (startX, midY), ctrl2 = (midX, endY) / (endX, midY)
            let mid_x = (sx + ex) / 2;
            let mid_y = (sy + ey) / 2;
            let start_is_horz = start_idx == 1 || start_idx == 3; // 우/좌
            let end_is_horz = end_idx == 1 || end_idx == 3;

            let (c1x, c1y, c2x, c2y) = if start_is_horz && end_is_horz {
                // 우↔좌: midX 기준 S곡선
                (mid_x, sy, mid_x, ey)
            } else if !start_is_horz && !end_is_horz {
                // 상↔하: midY 기준 S곡선
                (sx, mid_y, ex, mid_y)
            } else if start_is_horz {
                // 우/좌 → 상/하: 수평 출발 → midX까지, 수직 진입 → midY까지
                (mid_x, sy, ex, mid_y)
            } else {
                // 상/하 → 우/좌: 수직 출발 → midY까지, 수평 진입 → midX까지
                (sx, mid_y, mid_x, ey)
            };

            conn.control_points = vec![
                ConnectorControlPoint {
                    x: sx,
                    y: sy,
                    point_type: 3,
                }, // 시작 앵커
                ConnectorControlPoint {
                    x: c1x,
                    y: c1y,
                    point_type: 2,
                }, // 베지어 ctrl1
                ConnectorControlPoint {
                    x: c2x,
                    y: c2y,
                    point_type: 2,
                }, // 베지어 ctrl2
                ConnectorControlPoint {
                    x: ex,
                    y: ey,
                    point_type: 26,
                }, // 끝 앵커
            ];
        } else {
            // ─── 꺽인 연결선: 직각 꺾임점 ───
            let mut pts = Vec::new();
            pts.push(ConnectorControlPoint {
                x: sx,
                y: sy,
                point_type: 3,
            });

            match (start_idx, end_idx) {
                (1, 3) | (3, 1) => {
                    let mid_x = (sx + ex) / 2;
                    pts.push(ConnectorControlPoint {
                        x: mid_x,
                        y: sy,
                        point_type: 2,
                    });
                    pts.push(ConnectorControlPoint {
                        x: mid_x,
                        y: ey,
                        point_type: 2,
                    });
                }
                (2, 0) | (0, 2) => {
                    let mid_y = (sy + ey) / 2;
                    pts.push(ConnectorControlPoint {
                        x: sx,
                        y: mid_y,
                        point_type: 2,
                    });
                    pts.push(ConnectorControlPoint {
                        x: ex,
                        y: mid_y,
                        point_type: 2,
                    });
                }
                (1, 0) | (1, 2) | (3, 0) | (3, 2) => {
                    pts.push(ConnectorControlPoint {
                        x: ex,
                        y: sy,
                        point_type: 2,
                    });
                }
                (0, 1) | (0, 3) | (2, 1) | (2, 3) => {
                    pts.push(ConnectorControlPoint {
                        x: sx,
                        y: ey,
                        point_type: 2,
                    });
                }
                _ => {
                    let mid_x = (sx + ex) / 2;
                    pts.push(ConnectorControlPoint {
                        x: mid_x,
                        y: sy,
                        point_type: 2,
                    });
                    pts.push(ConnectorControlPoint {
                        x: mid_x,
                        y: ey,
                        point_type: 2,
                    });
                }
            }

            pts.push(ConnectorControlPoint {
                x: ex,
                y: ey,
                point_type: 26,
            });
            conn.control_points = pts;
        }
    }

    /// 구역 내 모든 연결선을 스캔하여 연결된 도형의 현재 위치에 맞게 갱신한다.
    pub fn update_connectors_in_section(&mut self, section_idx: usize) {
        let section = match self.document.sections.get(section_idx) {
            Some(s) => s,
            None => return,
        };

        // 1) SC inst_id → 연결점 좌표 맵 구축 (SubjectID = drawing.inst_id)
        let mut conn_points: std::collections::HashMap<u32, [(i32, i32); 4]> =
            std::collections::HashMap::new();
        for para in &section.paragraphs {
            for ctrl in &para.controls {
                let (common, inst_id, _is_line) = match ctrl {
                    Control::Shape(s) => {
                        let sc_inst = s.drawing().map(|d| d.inst_id).unwrap_or(0);
                        (
                            s.common(),
                            sc_inst,
                            matches!(s.as_ref(), ShapeObject::Line(_)),
                        )
                    }
                    Control::Picture(p) => (&p.common, 0u32, false),
                    _ => continue,
                };
                if _is_line {
                    continue;
                }
                let x = common.horizontal_offset as i32;
                let y = common.vertical_offset as i32;
                let w = common.width as i32;
                let h = common.height as i32;
                let cx = x + w / 2;
                let cy = y + h / 2;
                let pts = [(cx, y), (x + w, cy), (cx, y + h), (x, cy)];
                // SC inst_id (= SubjectID) 등록
                if inst_id != 0 {
                    conn_points.insert(inst_id, pts);
                }
                // CTRL_HEADER instance_id로도 등록 (폴백)
                if common.instance_id != 0 {
                    conn_points.insert(common.instance_id, pts);
                    conn_points.insert((common.instance_id & 0x3FFFFFFF) + 1, pts);
                }
            }
        }

        // 2) 커넥터 찾기 및 좌표 갱신
        let section = match self.document.sections.get_mut(section_idx) {
            Some(s) => s,
            None => return,
        };
        for para in &mut section.paragraphs {
            for ctrl in &mut para.controls {
                let line = match ctrl {
                    Control::Shape(ref mut s) => match s.as_mut() {
                        ShapeObject::Line(ref mut l) if l.connector.is_some() => l,
                        _ => continue,
                    },
                    _ => continue,
                };

                let conn = line.connector.as_ref().unwrap();
                let start_pts = conn_points.get(&conn.start_subject_id);
                let end_pts = conn_points.get(&conn.end_subject_id);

                // 연결된 도형을 찾지 못하면 건너뜀 (연결 끊어진 상태)
                if start_pts.is_none() || end_pts.is_none() {
                    continue;
                }

                let si = conn.start_subject_index as usize;
                let ei = conn.end_subject_index as usize;
                let (gsx, gsy) = start_pts.unwrap()[si.min(3)];
                let (gex, gey) = end_pts.unwrap()[ei.min(3)];

                // 커넥터 bbox 재계산
                let min_x = gsx.min(gex);
                let min_y = gsy.min(gey);
                let max_x = gsx.max(gex);
                let max_y = gsy.max(gey);
                let new_w = (max_x - min_x).max(1) as u32;
                let new_h = (max_y - min_y).max(1) as u32;

                line.common.horizontal_offset = min_x as u32;
                line.common.vertical_offset = min_y as u32;
                line.common.width = new_w;
                line.common.height = new_h;

                // 로컬 시작/끝 좌표
                line.start.x = gsx - min_x;
                line.start.y = gsy - min_y;
                line.end.x = gex - min_x;
                line.end.y = gey - min_y;

                // shape_attr 동기화
                line.drawing.shape_attr.current_width = new_w;
                line.drawing.shape_attr.original_width = new_w;
                line.drawing.shape_attr.current_height = new_h;
                line.drawing.shape_attr.original_height = new_h;
                line.drawing.shape_attr.rotation_center.x = new_w as i32 / 2;
                line.drawing.shape_attr.rotation_center.y = new_h as i32 / 2;
                line.drawing.shape_attr.raw_rendering = Vec::new();
            }
        }

        // 3) 제어점 재계산 (인덱스 수집 후 별도 루프 — borrow checker 대응)
        let mut routing_targets: Vec<(usize, usize, u32, u32)> = Vec::new();
        {
            let section = match self.document.sections.get(section_idx) {
                Some(s) => s,
                None => return,
            };
            for (pi, para) in section.paragraphs.iter().enumerate() {
                for (ci, ctrl) in para.controls.iter().enumerate() {
                    if let Control::Shape(ref s) = ctrl {
                        if let ShapeObject::Line(ref l) = s.as_ref() {
                            if let Some(ref c) = l.connector {
                                if c.link_type.is_stroke() || c.link_type.is_arc() {
                                    routing_targets.push((
                                        pi,
                                        ci,
                                        c.start_subject_index,
                                        c.end_subject_index,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        for (pi, ci, si, ei) in routing_targets {
            self.recalculate_connector_routing(section_idx, pi, ci, si, ei);
        }
    }

    /// 직선 끝점 이동: 글로벌 좌표(HWPUNIT)로 시작/끝점을 직접 설정
    pub fn move_line_endpoint_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        start_x: i32,
        start_y: i32,
        end_x: i32,
        end_y: i32,
    ) -> Result<String, HwpError> {
        let section = self
            .document
            .sections
            .get_mut(section_idx)
            .ok_or_else(|| HwpError::RenderError("구역 범위 초과".to_string()))?;
        let para = section
            .paragraphs
            .get_mut(para_idx)
            .ok_or_else(|| HwpError::RenderError("문단 범위 초과".to_string()))?;
        let ctrl = para
            .controls
            .get_mut(control_idx)
            .ok_or_else(|| HwpError::RenderError("컨트롤 범위 초과".to_string()))?;
        let line = match ctrl {
            Control::Shape(ref mut s) => match s.as_mut() {
                ShapeObject::Line(ref mut l) => l,
                _ => return Err(HwpError::RenderError("직선이 아닙니다".to_string())),
            },
            _ => return Err(HwpError::RenderError("Shape이 아닙니다".to_string())),
        };

        let min_x = start_x.min(end_x);
        let min_y = start_y.min(end_y);
        let w = (start_x - end_x).abs().max(1);
        let h = (start_y - end_y).abs().max(0);

        line.common.horizontal_offset = min_x as u32;
        line.common.vertical_offset = min_y as u32;
        line.common.width = w as u32;
        line.common.height = h.max(1) as u32;
        line.start.x = start_x - min_x;
        line.start.y = start_y - min_y;
        line.end.x = end_x - min_x;
        line.end.y = end_y - min_y;

        line.drawing.shape_attr.current_width = w as u32;
        line.drawing.shape_attr.original_width = w as u32;
        line.drawing.shape_attr.current_height = h.max(1) as u32;
        line.drawing.shape_attr.original_height = h.max(1) as u32;
        line.drawing.shape_attr.rotation_center.x = w / 2;
        line.drawing.shape_attr.rotation_center.y = h / 2;
        line.drawing.shape_attr.raw_rendering = Vec::new();

        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.update_connectors_in_section(section_idx);

        Ok("{\"ok\":true}".to_string())
    }

    /// 도형 내부 좌표만 스케일 (common/shape_attr은 변경하지 않음)
    fn scale_shape_coords(child: &mut crate::model::shape::ShapeObject, sx: f64, sy: f64) {
        use crate::model::shape::ShapeObject as SO;
        fn sp(v: i32, s: f64) -> i32 {
            (v as f64 * s).round() as i32
        }
        match child {
            SO::Line(ref mut s) => {
                s.start.x = sp(s.start.x, sx);
                s.start.y = sp(s.start.y, sy);
                s.end.x = sp(s.end.x, sx);
                s.end.y = sp(s.end.y, sy);
            }
            SO::Rectangle(ref mut s) => {
                let w = s.common.width as i32;
                let h = s.common.height as i32;
                s.x_coords = [0, w, w, 0];
                s.y_coords = [0, 0, h, h];
            }
            SO::Ellipse(ref mut s) => {
                s.center.x = sp(s.center.x, sx);
                s.center.y = sp(s.center.y, sy);
                s.axis1.x = sp(s.axis1.x, sx);
                s.axis1.y = sp(s.axis1.y, sy);
                s.axis2.x = sp(s.axis2.x, sx);
                s.axis2.y = sp(s.axis2.y, sy);
                s.start1.x = sp(s.start1.x, sx);
                s.start1.y = sp(s.start1.y, sy);
                s.end1.x = sp(s.end1.x, sx);
                s.end1.y = sp(s.end1.y, sy);
                s.start2.x = sp(s.start2.x, sx);
                s.start2.y = sp(s.start2.y, sy);
                s.end2.x = sp(s.end2.x, sx);
                s.end2.y = sp(s.end2.y, sy);
            }
            SO::Arc(ref mut s) => {
                s.center.x = sp(s.center.x, sx);
                s.center.y = sp(s.center.y, sy);
                s.axis1.x = sp(s.axis1.x, sx);
                s.axis1.y = sp(s.axis1.y, sy);
                s.axis2.x = sp(s.axis2.x, sx);
                s.axis2.y = sp(s.axis2.y, sy);
            }
            SO::Polygon(ref mut s) => {
                for p in &mut s.points {
                    p.x = sp(p.x, sx);
                    p.y = sp(p.y, sy);
                }
            }
            SO::Curve(ref mut s) => {
                for p in &mut s.points {
                    p.x = sp(p.x, sx);
                    p.y = sp(p.y, sy);
                }
            }
            _ => {}
        }
    }

    /// 그룹 자식 개체들을 비례 스케일 (크기/위치/도형좌표 포함)
    fn scale_group_children(children: &mut [crate::model::shape::ShapeObject], sx: f64, sy: f64) {
        use crate::model::shape::ShapeObject as SO;
        fn sp(v: i32, s: f64) -> i32 {
            (v as f64 * s).round() as i32
        }

        for child in children.iter_mut() {
            // CommonObjAttr 스케일
            let c = child.common_mut();
            c.horizontal_offset = (c.horizontal_offset as f64 * sx) as u32;
            c.vertical_offset = (c.vertical_offset as f64 * sy) as u32;
            c.width = ((c.width as f64 * sx).round().max(1.0)) as u32;
            c.height = ((c.height as f64 * sy).round().max(1.0)) as u32;
            let new_horz = c.horizontal_offset;
            let new_vert = c.vertical_offset;
            let new_cw = c.width;
            let new_ch = c.height;

            // 도형별 좌표 스케일
            match child {
                SO::Line(ref mut s) => {
                    s.start.x = sp(s.start.x, sx);
                    s.start.y = sp(s.start.y, sy);
                    s.end.x = sp(s.end.x, sx);
                    s.end.y = sp(s.end.y, sy);
                }
                SO::Rectangle(ref mut s) => {
                    let w = new_cw as i32;
                    let h = new_ch as i32;
                    s.x_coords = [0, w, w, 0];
                    s.y_coords = [0, 0, h, h];
                }
                SO::Ellipse(ref mut s) => {
                    s.center.x = sp(s.center.x, sx);
                    s.center.y = sp(s.center.y, sy);
                    s.axis1.x = sp(s.axis1.x, sx);
                    s.axis1.y = sp(s.axis1.y, sy);
                    s.axis2.x = sp(s.axis2.x, sx);
                    s.axis2.y = sp(s.axis2.y, sy);
                    s.start1.x = sp(s.start1.x, sx);
                    s.start1.y = sp(s.start1.y, sy);
                    s.end1.x = sp(s.end1.x, sx);
                    s.end1.y = sp(s.end1.y, sy);
                    s.start2.x = sp(s.start2.x, sx);
                    s.start2.y = sp(s.start2.y, sy);
                    s.end2.x = sp(s.end2.x, sx);
                    s.end2.y = sp(s.end2.y, sy);
                }
                SO::Arc(ref mut s) => {
                    s.center.x = sp(s.center.x, sx);
                    s.center.y = sp(s.center.y, sy);
                    s.axis1.x = sp(s.axis1.x, sx);
                    s.axis1.y = sp(s.axis1.y, sy);
                    s.axis2.x = sp(s.axis2.x, sx);
                    s.axis2.y = sp(s.axis2.y, sy);
                }
                SO::Polygon(ref mut s) => {
                    for p in &mut s.points {
                        p.x = sp(p.x, sx);
                        p.y = sp(p.y, sy);
                    }
                }
                SO::Curve(ref mut s) => {
                    for p in &mut s.points {
                        p.x = sp(p.x, sx);
                        p.y = sp(p.y, sy);
                    }
                }
                SO::Group(ref mut g) => {
                    g.shape_attr.current_width = new_cw;
                    g.shape_attr.original_width = new_cw;
                    g.shape_attr.current_height = new_ch;
                    g.shape_attr.original_height = new_ch;
                    Self::scale_group_children(&mut g.children, sx, sy);
                }
                SO::Picture(_) => {} // 그림은 크기만 변경
                SO::Chart(_) => {}   // 차트: 크기만 변경, 내부 좌표 스케일 없음 (Task #195 단계 2)
                SO::Ole(_) => {}     // OLE: 크기만 변경
            }

            // shape_attr 동기화
            let sa = match child {
                SO::Line(s) => &mut s.drawing.shape_attr,
                SO::Rectangle(s) => &mut s.drawing.shape_attr,
                SO::Ellipse(s) => &mut s.drawing.shape_attr,
                SO::Arc(s) => &mut s.drawing.shape_attr,
                SO::Polygon(s) => &mut s.drawing.shape_attr,
                SO::Curve(s) => &mut s.drawing.shape_attr,
                SO::Group(g) => &mut g.shape_attr,
                SO::Picture(p) => &mut p.shape_attr,
                SO::Chart(c) => &mut c.drawing.shape_attr,
                SO::Ole(o) => &mut o.drawing.shape_attr,
            };
            sa.offset_x = new_horz as i32;
            sa.offset_y = new_vert as i32;
            sa.current_width = new_cw;
            sa.original_width = new_cw;
            sa.current_height = new_ch;
            sa.original_height = new_ch;
            sa.render_tx = new_horz as f64;
            sa.render_ty = new_vert as f64;
            sa.raw_rendering = Vec::new();
        }
    }

    /// 구역 내 모든 Shape의 z_order 최대값을 반환 (새 Shape 생성 시 사용)
    fn max_shape_z_order_in_section(&self, section_idx: usize) -> i32 {
        self.document
            .sections
            .get(section_idx)
            .map(|section| {
                section
                    .paragraphs
                    .iter()
                    .flat_map(|p| p.controls.iter())
                    .filter_map(|ctrl| {
                        if let Control::Shape(shape) = ctrl {
                            Some(shape.z_order())
                        } else {
                            None
                        }
                    })
                    .max()
                    .unwrap_or(-1)
            })
            .unwrap_or(-1)
    }

    fn max_shape_z_order_in_paragraphs(paragraphs: &[Paragraph]) -> i32 {
        paragraphs
            .iter()
            .flat_map(|p| p.controls.iter())
            .filter_map(|ctrl| {
                if let Control::Shape(shape) = ctrl {
                    Some(shape.z_order())
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(-1)
    }

    fn shape_component_attr_mut(
        shape: &mut ShapeObject,
    ) -> &mut crate::model::shape::ShapeComponentAttr {
        match shape {
            ShapeObject::Line(s) => &mut s.drawing.shape_attr,
            ShapeObject::Rectangle(s) => &mut s.drawing.shape_attr,
            ShapeObject::Ellipse(s) => &mut s.drawing.shape_attr,
            ShapeObject::Arc(s) => &mut s.drawing.shape_attr,
            ShapeObject::Polygon(s) => &mut s.drawing.shape_attr,
            ShapeObject::Curve(s) => &mut s.drawing.shape_attr,
            ShapeObject::Group(g) => &mut g.shape_attr,
            ShapeObject::Picture(p) => &mut p.shape_attr,
            ShapeObject::Chart(c) => &mut c.drawing.shape_attr,
            ShapeObject::Ole(o) => &mut o.drawing.shape_attr,
        }
    }

    fn build_shape_object_for_insert(
        default_char_shape_id: u32,
        default_para_shape_id: u16,
        new_z_order: i32,
        width: u32,
        height: u32,
        horz_offset: u32,
        vert_offset: u32,
        treat_as_char: bool,
        text_wrap_str: &str,
        shape_type: &str,
        line_flip_x: bool,
        line_flip_y: bool,
        polygon_points: &[crate::model::Point],
    ) -> ShapeObject {
        use crate::model::paragraph::{CharShapeRef, LineSeg};
        use crate::model::shape::*;
        use crate::model::style::{Fill, ShapeBorderLine};

        let text_wrap = match text_wrap_str {
            "Square" => TextWrap::Square,
            "Tight" => TextWrap::Tight,
            "Through" => TextWrap::Through,
            "TopAndBottom" => TextWrap::TopAndBottom,
            "BehindText" => TextWrap::BehindText,
            "InFrontOfText" => TextWrap::InFrontOfText,
            _ => TextWrap::InFrontOfText,
        };
        let inline_textbox = shape_type == "textbox" && treat_as_char;
        let mut attr: u32 = if inline_textbox { 0x0A0210 } else { 0x046A4000 };
        if treat_as_char {
            attr |= 0x01;
        }

        let tb_inner_width = width.saturating_sub(1020);
        let mut inner_raw_header_extra = vec![0u8; 10];
        inner_raw_header_extra[0..2].copy_from_slice(&1u16.to_le_bytes());
        inner_raw_header_extra[4..6].copy_from_slice(&1u16.to_le_bytes());
        let inner_para = Paragraph {
            text: String::new(),
            char_count: 1,
            char_count_msb: true,
            control_mask: 0,
            para_shape_id: default_para_shape_id,
            style_id: 0,
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: default_char_shape_id,
            }],
            line_segs: vec![LineSeg {
                text_start: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: tb_inner_width as i32,
                tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                ..Default::default()
            }],
            has_para_text: false,
            raw_header_extra: inner_raw_header_extra,
            ..Default::default()
        };

        let w_i = width as i32;
        let h_i = height as i32;
        let is_connector = shape_type.starts_with("connector-");
        let ctrl_id: u32 = match shape_type {
            "line"
            | "connector-straight"
            | "connector-stroke"
            | "connector-arc"
            | "connector-straight-arrow"
            | "connector-stroke-arrow"
            | "connector-arc-arrow" => {
                if is_connector {
                    0x24636f6c
                } else {
                    0x246c696e
                }
            }
            "ellipse" => 0x24656c6c,
            "polygon" => 0x24706f6c,
            "arc" => 0x24617263,
            _ => 0x24726563,
        };

        let instance_id: u32 = {
            let mut h: u32 = 0x7de30000;
            h = h.wrapping_add(new_z_order as u32 * 0x100);
            h = h.wrapping_add(horz_offset.wrapping_mul(3));
            h = h.wrapping_add(vert_offset.wrapping_mul(7));
            h = h.wrapping_add(width);
            h = h.wrapping_add(height.wrapping_mul(0x1b));
            h |= 0x40000000;
            if h == 0 {
                h = 0x7de34b69;
            }
            h
        };

        let common = CommonObjAttr {
            ctrl_id,
            attr,
            vertical_offset: vert_offset,
            horizontal_offset: horz_offset,
            width,
            height,
            z_order: new_z_order,
            instance_id,
            margin: if shape_type == "textbox" {
                crate::model::Padding {
                    left: 283,
                    right: 283,
                    top: 283,
                    bottom: 283,
                }
            } else {
                crate::model::Padding {
                    left: 0,
                    right: 0,
                    top: 0,
                    bottom: 0,
                }
            },
            treat_as_char,
            vert_rel_to: if inline_textbox {
                VertRelTo::Para
            } else {
                VertRelTo::Paper
            },
            vert_align: VertAlign::Top,
            horz_rel_to: if inline_textbox {
                HorzRelTo::Column
            } else {
                HorzRelTo::Paper
            },
            horz_align: HorzAlign::Left,
            text_wrap,
            description: match shape_type {
                "line" => "선입니다.".to_string(),
                "ellipse" => "타원입니다.".to_string(),
                "rectangle" => "사각형입니다.".to_string(),
                "textbox" => "글상자입니다.".to_string(),
                "polygon" => "다각형입니다.".to_string(),
                "arc" => "호입니다.".to_string(),
                "connector-straight" => "직선 연결선입니다.".to_string(),
                "connector-stroke" => "꺾인 연결선입니다.".to_string(),
                "connector-arc" => "곡선 연결선입니다.".to_string(),
                _ => "그리기 개체.".to_string(),
            },
            ..Default::default()
        };

        let has_textbox = shape_type == "textbox";
        let has_fill = shape_type != "line" && !is_connector;
        let drawing = DrawingObjAttr {
            shape_attr: ShapeComponentAttr {
                ctrl_id,
                is_two_ctrl_id: true,
                original_width: width,
                original_height: height,
                current_width: width,
                current_height: height,
                local_file_version: 1,
                flip: 0x00080000,
                rotation_center: crate::model::Point {
                    x: (width / 2) as i32,
                    y: (height / 2) as i32,
                },
                ..Default::default()
            },
            border_line: ShapeBorderLine {
                color: 0,
                width: 33,
                attr: 0xD1000041,
                outline_style: 0,
            },
            fill: if has_fill {
                Fill {
                    fill_type: crate::model::style::FillType::Solid,
                    solid: Some(crate::model::style::SolidFill {
                        background_color: 0x00FFFFFF,
                        pattern_color: 0,
                        pattern_type: -1,
                    }),
                    gradient: None,
                    image: None,
                    alpha: 0,
                }
            } else {
                Fill::default()
            },
            text_box: if has_textbox {
                Some(TextBox {
                    list_attr: 0x20,
                    vertical_all: false,
                    vertical_align: crate::model::table::VerticalAlign::Top,
                    margin_left: 283,
                    margin_right: 283,
                    margin_top: 283,
                    margin_bottom: 283,
                    max_width: width,
                    raw_list_header_extra: vec![0u8; 13],
                    paragraphs: vec![inner_para],
                })
            } else {
                None
            },
            inst_id: (instance_id & 0x3FFFFFFF) + 1,
            ..Default::default()
        };

        match shape_type {
            "line"
            | "connector-straight"
            | "connector-stroke"
            | "connector-arc"
            | "connector-straight-arrow"
            | "connector-stroke-arrow"
            | "connector-arc-arrow" => {
                let (sx, sy, ex, ey) = match (line_flip_x, line_flip_y) {
                    (false, false) => (0, 0, w_i, h_i),
                    (false, true) => (0, h_i, w_i, 0),
                    (true, false) => (w_i, 0, 0, h_i),
                    (true, true) => (w_i, h_i, 0, 0),
                };
                let connector = if is_connector {
                    use crate::model::shape::{ConnectorControlPoint, ConnectorData, LinkLineType};
                    let link_type = match shape_type {
                        "connector-straight" => LinkLineType::StraightNoArrow,
                        "connector-straight-arrow" => LinkLineType::StraightOneWay,
                        "connector-stroke" => LinkLineType::StrokeNoArrow,
                        "connector-stroke-arrow" => LinkLineType::StrokeOneWay,
                        "connector-arc" => LinkLineType::ArcNoArrow,
                        "connector-arc-arrow" => LinkLineType::ArcOneWay,
                        _ => LinkLineType::StraightNoArrow,
                    };
                    let control_points = match link_type {
                        LinkLineType::StrokeNoArrow
                        | LinkLineType::StrokeOneWay
                        | LinkLineType::StrokeBoth
                        | LinkLineType::ArcNoArrow
                        | LinkLineType::ArcOneWay
                        | LinkLineType::ArcBoth => vec![
                            ConnectorControlPoint {
                                x: sx,
                                y: sy,
                                point_type: 3,
                            },
                            ConnectorControlPoint {
                                x: ex,
                                y: sy,
                                point_type: 2,
                            },
                            ConnectorControlPoint {
                                x: ex,
                                y: ey,
                                point_type: 26,
                            },
                        ],
                        _ => Vec::new(),
                    };
                    Some(ConnectorData {
                        link_type,
                        start_subject_id: 0,
                        start_subject_index: 0,
                        end_subject_id: 0,
                        end_subject_index: 0,
                        control_points,
                        raw_trailing: vec![0x1a, 0, 0, 0, 0, 0],
                    })
                } else {
                    None
                };
                ShapeObject::Line(LineShape {
                    common,
                    drawing,
                    start: crate::model::Point { x: sx, y: sy },
                    end: crate::model::Point { x: ex, y: ey },
                    started_right_or_bottom: if is_connector {
                        false
                    } else {
                        line_flip_x || line_flip_y
                    },
                    connector,
                    raw_trailing: Vec::new(),
                })
            }
            "ellipse" => ShapeObject::Ellipse(EllipseShape {
                common,
                drawing,
                attr: 0,
                center: crate::model::Point {
                    x: w_i / 2,
                    y: h_i / 2,
                },
                axis1: crate::model::Point { x: w_i, y: h_i / 2 },
                axis2: crate::model::Point { x: w_i / 2, y: h_i },
                start1: crate::model::Point { x: w_i, y: h_i / 2 },
                end1: crate::model::Point { x: w_i, y: h_i / 2 },
                start2: crate::model::Point { x: w_i, y: h_i / 2 },
                end2: crate::model::Point { x: w_i, y: h_i / 2 },
                raw_trailing: Vec::new(),
            }),
            "polygon" => {
                let points = if !polygon_points.is_empty() {
                    polygon_points.to_vec()
                } else {
                    vec![
                        crate::model::Point { x: w_i / 2, y: 0 },
                        crate::model::Point { x: w_i, y: h_i },
                        crate::model::Point { x: 0, y: h_i },
                    ]
                };
                ShapeObject::Polygon(PolygonShape {
                    common,
                    drawing,
                    points,
                    raw_trailing: Vec::new(),
                })
            }
            "arc" => ShapeObject::Arc(ArcShape {
                common,
                drawing,
                arc_type: 0,
                center: crate::model::Point {
                    x: w_i / 2,
                    y: h_i / 2,
                },
                axis1: crate::model::Point { x: w_i, y: h_i / 2 },
                axis2: crate::model::Point { x: w_i / 2, y: 0 },
                raw_trailing: Vec::new(),
            }),
            _ => ShapeObject::Rectangle(RectangleShape {
                common,
                drawing,
                round_rate: 0,
                x_coords: [0, w_i, w_i, 0],
                y_coords: [0, 0, h_i, h_i],
                raw_trailing: Vec::new(),
            }),
        }
    }

    fn insert_shape_object_at_char_offset(
        para: &mut Paragraph,
        char_offset: usize,
        shape_obj: ShapeObject,
    ) -> Result<usize, HwpError> {
        let text_len = para.text.chars().count();
        if char_offset > text_len {
            return Err(HwpError::RenderError(format!(
                "char_offset {} 범위 초과 (문단 길이 {})",
                char_offset, text_len
            )));
        }

        let insert_idx = {
            let positions = crate::document_core::helpers::find_control_text_positions(para);
            let mut idx = para.controls.len();
            for (i, &pos) in positions.iter().enumerate() {
                if pos > char_offset {
                    idx = i;
                    break;
                }
            }
            idx
        };

        para.controls
            .insert(insert_idx, Control::Shape(Box::new(shape_obj)));
        while para.ctrl_data_records.len() < insert_idx {
            para.ctrl_data_records.push(None);
        }
        para.ctrl_data_records.insert(insert_idx, None);

        if !para.char_offsets.is_empty() {
            let raw_offset = if insert_idx > 0 && insert_idx <= para.char_offsets.len() {
                para.char_offsets[insert_idx - 1] + 8
            } else if !para.char_offsets.is_empty() {
                para.char_offsets[0].saturating_sub(8)
            } else {
                (char_offset * 2) as u32
            };
            para.char_offsets.insert(insert_idx, raw_offset);
        }
        for co in para.char_offsets.iter_mut().skip(insert_idx + 1) {
            *co += 8;
        }

        para.char_count += 8;
        para.control_mask |= 0x00000800;
        para.has_para_text = true;
        Ok(insert_idx)
    }

    /// 표 셀/글상자 내부 문단에 Shape 컨트롤을 삽입한다.
    pub fn create_cell_shape_control_by_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        char_offset: usize,
        width: u32,
        height: u32,
        horz_offset: u32,
        vert_offset: u32,
        treat_as_char: bool,
        text_wrap_str: &str,
        shape_type: &str,
        line_flip_x: bool,
        line_flip_y: bool,
        polygon_points: &[crate::model::Point],
    ) -> Result<String, HwpError> {
        if width == 0 && height == 0 {
            return Err(HwpError::RenderError(
                "폭과 높이가 모두 0입니다".to_string(),
            ));
        }

        let path = Self::parse_cell_path_json(cell_path_json)?;
        let outer_ctrl = path[0].0;
        let outer_cell = path[0].1;
        let insert_ctrl_idx = {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let target_para = Self::resolve_cell_paragraph_mut(section, parent_para_idx, &path)?;
            let default_char_shape_id = target_para
                .char_shapes
                .first()
                .map(|cs| cs.char_shape_id)
                .unwrap_or(0);
            let default_para_shape_id = target_para.para_shape_id;
            let new_z_order = target_para
                .controls
                .iter()
                .filter_map(|ctrl| {
                    if let Control::Shape(shape) = ctrl {
                        Some(shape.z_order())
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(-1)
                + 1;
            let shape_obj = Self::build_shape_object_for_insert(
                default_char_shape_id,
                default_para_shape_id,
                new_z_order,
                width,
                height,
                horz_offset,
                vert_offset,
                treat_as_char,
                text_wrap_str,
                shape_type,
                line_flip_x,
                line_flip_y,
                polygon_points,
            );
            Self::insert_shape_object_at_char_offset(target_para, char_offset, shape_obj)?
        };

        self.mark_cell_control_dirty(section_idx, parent_para_idx, outer_ctrl);
        self.document.sections[section_idx].raw_stream = None;
        self.mark_section_dirty(section_idx);
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::CellTextChanged {
            section: section_idx,
            para: parent_para_idx,
            ctrl: outer_ctrl,
            cell: outer_cell,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"inner_control\":{},\"innerControlIdx\":{},\"logicalOffset\":{}",
            parent_para_idx, outer_ctrl, insert_ctrl_idx, insert_ctrl_idx, char_offset
        )))
    }

    /// 머리말/꼬리말 내부 문단에 Shape 컨트롤을 삽입한다.
    pub fn create_header_footer_shape_control_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        char_offset: usize,
        width: u32,
        height: u32,
        horz_offset: u32,
        vert_offset: u32,
        treat_as_char: bool,
        text_wrap_str: &str,
        shape_type: &str,
        line_flip_x: bool,
        line_flip_y: bool,
        polygon_points: &[crate::model::Point],
    ) -> Result<String, HwpError> {
        if width == 0 && height == 0 {
            return Err(HwpError::RenderError(
                "폭과 높이가 모두 0입니다".to_string(),
            ));
        }

        let (scope, insert_ctrl_idx) = {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let (scope, inner_paras): (&'static str, &mut Vec<Paragraph>) = match outer_ctrl {
                Control::Header(header) => ("header", &mut header.paragraphs),
                Control::Footer(footer) => ("footer", &mut footer.paragraphs),
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let new_z_order = Self::max_shape_z_order_in_paragraphs(inner_paras) + 1;
            let inner_para = inner_paras.get_mut(inner_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("내부 문단 인덱스 {} 범위 초과", inner_para_idx))
            })?;
            let default_char_shape_id = inner_para
                .char_shapes
                .first()
                .map(|cs| cs.char_shape_id)
                .unwrap_or(0);
            let default_para_shape_id = inner_para.para_shape_id;
            let shape_obj = Self::build_shape_object_for_insert(
                default_char_shape_id,
                default_para_shape_id,
                new_z_order,
                width,
                height,
                horz_offset,
                vert_offset,
                treat_as_char,
                text_wrap_str,
                shape_type,
                line_flip_x,
                line_flip_y,
                polygon_points,
            );
            let insert_ctrl_idx =
                Self::insert_shape_object_at_char_offset(inner_para, char_offset, shape_obj)?;
            (scope, insert_ctrl_idx)
        };

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: outer_para_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"container_scope\":\"{}\",\"hf_para\":{},\"inner_control\":{},\"innerControlIdx\":{},\"logicalOffset\":{}",
            outer_para_idx, outer_control_idx, scope, inner_para_idx, insert_ctrl_idx, insert_ctrl_idx, char_offset
        )))
    }

    fn group_shape_controls_in_paragraphs(
        paragraphs: &mut Vec<Paragraph>,
        targets: &[(usize, usize)],
        new_z_order: i32,
    ) -> Result<(usize, usize), HwpError> {
        use crate::model::shape::*;

        if targets.len() < 2 {
            return Err(HwpError::RenderError(
                "묶기 위해서는 2개 이상의 개체가 필요합니다".to_string(),
            ));
        }

        let mut children: Vec<ShapeObject> = Vec::new();
        let mut group_min_x: i32 = i32::MAX;
        let mut group_min_y: i32 = i32::MAX;
        let mut group_max_x: i32 = i32::MIN;
        let mut group_max_y: i32 = i32::MIN;
        let mut first_common: Option<CommonObjAttr> = None;

        for &(pi, ci) in targets {
            if pi >= paragraphs.len() {
                return Err(HwpError::RenderError(format!(
                    "문단 인덱스 {} 범위 초과",
                    pi
                )));
            }
            if ci >= paragraphs[pi].controls.len() {
                return Err(HwpError::RenderError(format!(
                    "컨트롤 인덱스 {} 범위 초과 (문단 {})",
                    ci, pi
                )));
            }
            let ctrl = &paragraphs[pi].controls[ci];
            let (common, shape_obj) = match ctrl {
                Control::Shape(s) => {
                    let c = s.common().clone();
                    (c, (**s).clone())
                }
                Control::Picture(p) => {
                    let c = p.common.clone();
                    (c, ShapeObject::Picture(p.clone()))
                }
                _ => {
                    return Err(HwpError::RenderError(format!(
                        "컨트롤 ({},{})은 Shape/Picture가 아닙니다",
                        pi, ci
                    )))
                }
            };

            let x1 = common.horizontal_offset as i32;
            let y1 = common.vertical_offset as i32;
            let x2 = x1 + common.width as i32;
            let y2 = y1 + common.height as i32;
            group_min_x = group_min_x.min(x1);
            group_min_y = group_min_y.min(y1);
            group_max_x = group_max_x.max(x2);
            group_max_y = group_max_y.max(y2);

            if first_common.is_none() {
                first_common = Some(common);
            }
            children.push(shape_obj);
        }

        let group_w = (group_max_x - group_min_x).max(1) as u32;
        let group_h = (group_max_y - group_min_y).max(1) as u32;
        let fc = first_common.unwrap();

        for child in &mut children {
            let new_horz = ((child.common().horizontal_offset as i32 - group_min_x).max(0)) as u32;
            let new_vert = ((child.common().vertical_offset as i32 - group_min_y).max(0)) as u32;
            child.common_mut().horizontal_offset = new_horz;
            child.common_mut().vertical_offset = new_vert;

            let sa = Self::shape_component_attr_mut(child);
            sa.offset_x = new_horz as i32;
            sa.offset_y = new_vert as i32;
            sa.group_level = 1;
            sa.is_two_ctrl_id = false;
            sa.raw_rendering = Vec::new();
            sa.render_tx = new_horz as f64;
            sa.render_ty = new_vert as f64;
            sa.render_sx = 1.0;
            sa.render_sy = 1.0;
            sa.render_b = 0.0;
            sa.render_c = 0.0;
        }

        let group = GroupShape {
            common: CommonObjAttr {
                ctrl_id: 0x24636f6e,
                attr: fc.attr,
                vertical_offset: group_min_y as u32,
                horizontal_offset: group_min_x as u32,
                width: group_w,
                height: group_h,
                z_order: new_z_order,
                margin: fc.margin.clone(),
                treat_as_char: fc.treat_as_char,
                vert_rel_to: fc.vert_rel_to,
                vert_align: fc.vert_align,
                horz_rel_to: fc.horz_rel_to,
                horz_align: fc.horz_align,
                text_wrap: fc.text_wrap,
                description: "묶음 개체입니다.".to_string(),
                ..Default::default()
            },
            shape_attr: ShapeComponentAttr {
                ctrl_id: 0x24636f6e,
                is_two_ctrl_id: true,
                original_width: group_w,
                original_height: group_h,
                current_width: group_w,
                current_height: group_h,
                local_file_version: 1,
                flip: 0x00080000,
                rotation_center: crate::model::Point {
                    x: (group_w / 2) as i32,
                    y: (group_h / 2) as i32,
                },
                ..Default::default()
            },
            children,
            raw_component_extra: Vec::new(),
            caption: None,
        };
        let group_obj = ShapeObject::Group(group);

        let mut sorted_targets: Vec<(usize, usize)> = targets.to_vec();
        sorted_targets.sort_by(|a, b| b.cmp(a));
        let insert_target = *targets.iter().min().unwrap();

        for &(pi, ci) in &sorted_targets {
            let para = &mut paragraphs[pi];
            let text_chars: Vec<char> = para.text.chars().collect();
            let mut ctrl_ci = 0usize;
            let mut prev_end: u32 = 0;
            let mut gap_start: Option<u32> = None;
            'outer: for i in 0..text_chars.len() {
                let offset = if i < para.char_offsets.len() {
                    para.char_offsets[i]
                } else {
                    prev_end
                };
                while prev_end + 8 <= offset && ctrl_ci < para.controls.len() {
                    if ctrl_ci == ci {
                        gap_start = Some(prev_end);
                        break 'outer;
                    }
                    ctrl_ci += 1;
                    prev_end += 8;
                }
                let char_size: u32 = if text_chars[i] == '\t' {
                    8
                } else if text_chars[i].len_utf16() == 2 {
                    2
                } else {
                    1
                };
                prev_end = offset + char_size;
            }
            if gap_start.is_none() {
                while ctrl_ci < para.controls.len() {
                    if ctrl_ci == ci {
                        gap_start = Some(prev_end);
                        break;
                    }
                    ctrl_ci += 1;
                    prev_end += 8;
                }
            }
            if let Some(gs) = gap_start {
                let threshold = gs + 8;
                for offset in para.char_offsets.iter_mut() {
                    if *offset >= threshold {
                        *offset -= 8;
                    }
                }
            }

            para.controls.remove(ci);
            if ci < para.ctrl_data_records.len() {
                para.ctrl_data_records.remove(ci);
            }
            if para.char_count >= 8 {
                para.char_count -= 8;
            }
        }

        let (insert_pi, insert_ci_orig) = insert_target;
        let removed_before = sorted_targets
            .iter()
            .filter(|&&(pi, ci)| pi == insert_pi && ci < insert_ci_orig)
            .count();
        let insert_ci = insert_ci_orig - removed_before;

        {
            let para = &mut paragraphs[insert_pi];
            let ctrl_insert = insert_ci.min(para.controls.len());
            para.controls
                .insert(ctrl_insert, Control::Shape(Box::new(group_obj)));
            let cdr_insert = ctrl_insert.min(para.ctrl_data_records.len());
            para.ctrl_data_records.insert(cdr_insert, None);

            if !para.char_offsets.is_empty() {
                for co in para.char_offsets.iter_mut() {
                    *co += 8;
                }
            }
            para.char_count += 8;
            para.control_mask |= 0x00000800;
            para.has_para_text = true;
        }

        Ok((insert_pi, insert_ci))
    }

    fn ungroup_shape_control_in_paragraphs(
        paragraphs: &mut Vec<Paragraph>,
        para_idx: usize,
        control_idx: usize,
    ) -> Result<(), HwpError> {
        use crate::model::shape::*;

        if para_idx >= paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }
        let para = &mut paragraphs[para_idx];
        if control_idx >= para.controls.len() {
            return Err(HwpError::RenderError(format!(
                "컨트롤 인덱스 {} 범위 초과",
                control_idx
            )));
        }

        match &para.controls[control_idx] {
            Control::Shape(s) => match s.as_ref() {
                ShapeObject::Group(_) => {}
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 컨트롤이 GroupShape이 아닙니다".to_string(),
                    ))
                }
            },
            _ => {
                return Err(HwpError::RenderError(
                    "지정된 컨트롤이 Shape이 아닙니다".to_string(),
                ))
            }
        };

        let group_ctrl = para.controls.remove(control_idx);
        if control_idx < para.ctrl_data_records.len() {
            para.ctrl_data_records.remove(control_idx);
        }
        if para.char_count >= 8 {
            para.char_count -= 8;
        }

        let group_shape = match group_ctrl {
            Control::Shape(s) => match *s {
                ShapeObject::Group(g) => g,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };

        let group_x = group_shape.common.horizontal_offset as i32;
        let group_y = group_shape.common.vertical_offset as i32;
        let gsa = &group_shape.shape_attr;
        let group_sx = if gsa.original_width > 0 {
            gsa.current_width as f64 / gsa.original_width as f64
        } else {
            1.0
        };
        let group_sy = if gsa.original_height > 0 {
            gsa.current_height as f64 / gsa.original_height as f64
        } else {
            1.0
        };

        let mut insert_idx = control_idx;
        for mut child in group_shape.children {
            {
                let sa = child.shape_attr();
                let sa_w = sa.original_width;
                let sa_h = sa.original_height;
                let sa_ox = sa.offset_x;
                let sa_oy = sa.offset_y;
                let c = child.common_mut();
                if c.width == 0 && sa_w > 0 {
                    c.width = sa_w;
                }
                if c.height == 0 && sa_h > 0 {
                    c.height = sa_h;
                }
                if c.horizontal_offset == 0 && sa_ox > 0 {
                    c.horizontal_offset = sa_ox as u32;
                }
                if c.vertical_offset == 0 && sa_oy > 0 {
                    c.vertical_offset = sa_oy as u32;
                }
            }
            {
                let c = child.common_mut();
                c.horizontal_offset =
                    (group_x + (c.horizontal_offset as f64 * group_sx) as i32) as u32;
                c.vertical_offset = (group_y + (c.vertical_offset as f64 * group_sy) as i32) as u32;
                c.width = ((c.width as f64 * group_sx).round().max(1.0)) as u32;
                c.height = ((c.height as f64 * group_sy).round().max(1.0)) as u32;
                c.vert_rel_to = group_shape.common.vert_rel_to;
                c.vert_align = group_shape.common.vert_align;
                c.horz_rel_to = group_shape.common.horz_rel_to;
                c.horz_align = group_shape.common.horz_align;
                c.text_wrap = group_shape.common.text_wrap;
                c.attr = group_shape.common.attr;
                c.treat_as_char = group_shape.common.treat_as_char;
            }
            if group_sx != 1.0 || group_sy != 1.0 {
                Self::scale_shape_coords(&mut child, group_sx, group_sy);
            }
            let final_w = child.common().width;
            let final_h = child.common().height;
            {
                let sa = Self::shape_component_attr_mut(&mut child);
                if sa.group_level > 0 {
                    sa.group_level -= 1;
                }
                sa.offset_x = 0;
                sa.offset_y = 0;
                sa.render_tx = 0.0;
                sa.render_ty = 0.0;
                sa.current_width = final_w;
                sa.original_width = final_w;
                sa.current_height = final_h;
                sa.original_height = final_h;
                sa.is_two_ctrl_id = true;
                sa.raw_rendering = Vec::new();
            }

            para.controls
                .insert(insert_idx, Control::Shape(Box::new(child)));
            para.ctrl_data_records.insert(insert_idx, None);
            para.char_count += 8;
            para.control_mask |= 0x00000800;
            para.has_para_text = true;
            insert_idx += 1;
        }

        let children_count = insert_idx - control_idx;
        if children_count > 1 && !para.char_offsets.is_empty() {
            let net_delta = ((children_count - 1) * 8) as u32;
            for co in para.char_offsets.iter_mut() {
                *co += net_delta;
            }
        }

        Ok(())
    }

    // ─── 개체 묶기/풀기 API ──────────────────────────────

    /// 선택된 개체들을 GroupShape로 묶는다.
    /// targets: [(para_idx, control_idx), ...] — 같은 구역 내 Shape 또는 Picture
    /// 반환: JSON `{"ok":true, "paraIdx":N, "controlIdx":N}`
    pub fn group_shapes_native(
        &mut self,
        section_idx: usize,
        targets: &[(usize, usize)],
    ) -> Result<String, HwpError> {
        use crate::model::control::Control;
        use crate::model::shape::*;

        if targets.len() < 2 {
            return Err(HwpError::RenderError(
                "묶기 위해서는 2개 이상의 개체가 필요합니다".to_string(),
            ));
        }
        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }

        // 1) 대상 개체들을 ShapeObject로 수집 (인덱스 유효성 검사 포함)
        let section = &self.document.sections[section_idx];
        let mut children: Vec<ShapeObject> = Vec::new();
        let mut group_min_x: i32 = i32::MAX;
        let mut group_min_y: i32 = i32::MAX;
        let mut group_max_x: i32 = i32::MIN;
        let mut group_max_y: i32 = i32::MIN;
        let mut first_common: Option<CommonObjAttr> = None;

        for &(pi, ci) in targets {
            if pi >= section.paragraphs.len() {
                return Err(HwpError::RenderError(format!(
                    "문단 인덱스 {} 범위 초과",
                    pi
                )));
            }
            if ci >= section.paragraphs[pi].controls.len() {
                return Err(HwpError::RenderError(format!(
                    "컨트롤 인덱스 {} 범위 초과 (문단 {})",
                    ci, pi
                )));
            }
            let ctrl = &section.paragraphs[pi].controls[ci];
            let (common, shape_obj) = match ctrl {
                Control::Shape(s) => {
                    let c = s.common().clone();
                    (c, (**s).clone())
                }
                Control::Picture(p) => {
                    let c = p.common.clone();
                    (c, ShapeObject::Picture(p.clone()))
                }
                _ => {
                    return Err(HwpError::RenderError(format!(
                        "컨트롤 ({},{})은 Shape/Picture가 아닙니다",
                        pi, ci
                    )))
                }
            };

            // 합산 bbox 계산 (HWPUNIT 기준 — horizontal_offset, vertical_offset, width, height)
            let x1 = common.horizontal_offset as i32;
            let y1 = common.vertical_offset as i32;
            let x2 = x1 + common.width as i32;
            let y2 = y1 + common.height as i32;
            group_min_x = group_min_x.min(x1);
            group_min_y = group_min_y.min(y1);
            group_max_x = group_max_x.max(x2);
            group_max_y = group_max_y.max(y2);

            if first_common.is_none() {
                first_common = Some(common);
            }
            children.push(shape_obj);
        }

        let group_w = (group_max_x - group_min_x).max(1) as u32;
        let group_h = (group_max_y - group_min_y).max(1) as u32;
        let fc = first_common.unwrap();

        // 2) 자식 개체의 offset/render 좌표를 그룹 로컬 좌표로 변환
        for child in &mut children {
            // 그룹 내 로컬 좌표 계산
            let new_horz = ((child.common().horizontal_offset as i32 - group_min_x).max(0)) as u32;
            let new_vert = ((child.common().vertical_offset as i32 - group_min_y).max(0)) as u32;
            child.common_mut().horizontal_offset = new_horz;
            child.common_mut().vertical_offset = new_vert;

            // shape_attr: 렌더링에 사용되는 render_tx/ty와 offset_x/y 설정
            let sa = match child {
                ShapeObject::Line(s) => &mut s.drawing.shape_attr,
                ShapeObject::Rectangle(s) => &mut s.drawing.shape_attr,
                ShapeObject::Ellipse(s) => &mut s.drawing.shape_attr,
                ShapeObject::Arc(s) => &mut s.drawing.shape_attr,
                ShapeObject::Polygon(s) => &mut s.drawing.shape_attr,
                ShapeObject::Curve(s) => &mut s.drawing.shape_attr,
                ShapeObject::Group(g) => &mut g.shape_attr,
                ShapeObject::Picture(p) => &mut p.shape_attr,
                ShapeObject::Chart(c) => &mut c.drawing.shape_attr,
                ShapeObject::Ole(o) => &mut o.drawing.shape_attr,
            };
            sa.offset_x = new_horz as i32;
            sa.offset_y = new_vert as i32;
            sa.group_level = 1;
            sa.is_two_ctrl_id = false; // 그룹 자식은 ctrl_id 1번만
            sa.raw_rendering = Vec::new(); // 새로 생성 (직렬화 시 재계산)
                                           // 렌더러가 사용하는 변환 행렬 값 설정
            sa.render_tx = new_horz as f64;
            sa.render_ty = new_vert as f64;
            sa.render_sx = 1.0;
            sa.render_sy = 1.0;
            sa.render_b = 0.0;
            sa.render_c = 0.0;
        }

        // 3) GroupShape 조립
        let new_z_order = self.max_shape_z_order_in_section(section_idx) + 1;
        let group = GroupShape {
            common: CommonObjAttr {
                ctrl_id: 0x24636f6e, // '$con' — 그룹 컨테이너
                attr: fc.attr,
                vertical_offset: group_min_y as u32,
                horizontal_offset: group_min_x as u32,
                width: group_w,
                height: group_h,
                z_order: new_z_order,
                margin: fc.margin.clone(),
                treat_as_char: fc.treat_as_char,
                vert_rel_to: fc.vert_rel_to,
                vert_align: fc.vert_align,
                horz_rel_to: fc.horz_rel_to,
                horz_align: fc.horz_align,
                text_wrap: fc.text_wrap,
                description: "묶음 개체입니다.".to_string(),
                ..Default::default()
            },
            shape_attr: ShapeComponentAttr {
                ctrl_id: 0x24636f6e, // '$con'
                is_two_ctrl_id: true,
                original_width: group_w,
                original_height: group_h,
                current_width: group_w,
                current_height: group_h,
                local_file_version: 1,
                flip: 0x00080000,
                rotation_center: crate::model::Point {
                    x: (group_w / 2) as i32,
                    y: (group_h / 2) as i32,
                },
                ..Default::default()
            },
            children,
            raw_component_extra: Vec::new(),
            caption: None,
        };

        let group_obj = ShapeObject::Group(group);

        // 4) 원래 개체들을 문단에서 제거 (큰 인덱스부터 제거해야 인덱스 밀림 방지)
        let mut sorted_targets: Vec<(usize, usize)> = targets.to_vec();
        sorted_targets.sort_by(|a, b| b.cmp(a)); // 역순 정렬

        // 첫 번째 삽입 위치 (원래 개체 중 가장 앞에 있는 것)
        let insert_target = *targets.iter().min().unwrap();

        for &(pi, ci) in &sorted_targets {
            let para = &mut self.document.sections[section_idx].paragraphs[pi];

            // char_offsets 조정
            let text_chars: Vec<char> = para.text.chars().collect();
            let mut ctrl_ci = 0usize;
            let mut prev_end: u32 = 0;
            let mut gap_start: Option<u32> = None;
            'outer: for i in 0..text_chars.len() {
                let offset = if i < para.char_offsets.len() {
                    para.char_offsets[i]
                } else {
                    prev_end
                };
                while prev_end + 8 <= offset && ctrl_ci < para.controls.len() {
                    if ctrl_ci == ci {
                        gap_start = Some(prev_end);
                        break 'outer;
                    }
                    ctrl_ci += 1;
                    prev_end += 8;
                }
                let char_size: u32 = if text_chars[i] == '\t' {
                    8
                } else if text_chars[i].len_utf16() == 2 {
                    2
                } else {
                    1
                };
                prev_end = offset + char_size;
            }
            if gap_start.is_none() {
                while ctrl_ci < para.controls.len() {
                    if ctrl_ci == ci {
                        gap_start = Some(prev_end);
                        break;
                    }
                    ctrl_ci += 1;
                    prev_end += 8;
                }
            }
            if let Some(gs) = gap_start {
                let threshold = gs + 8;
                for offset in para.char_offsets.iter_mut() {
                    if *offset >= threshold {
                        *offset -= 8;
                    }
                }
            }

            para.controls.remove(ci);
            if ci < para.ctrl_data_records.len() {
                para.ctrl_data_records.remove(ci);
            }
            if para.char_count >= 8 {
                para.char_count -= 8;
            }
        }

        // 5) 삽입 위치 인덱스 재계산 (제거 후 인덱스가 변했을 수 있음)
        //    insert_target의 para에서 그보다 앞에서 제거된 개체 수만큼 보정
        let (insert_pi, insert_ci_orig) = insert_target;
        let removed_before = sorted_targets
            .iter()
            .filter(|&&(pi, ci)| pi == insert_pi && ci < insert_ci_orig)
            .count();
        let insert_ci = insert_ci_orig - removed_before;

        // 6) GroupShape를 문단에 삽입
        {
            let para = &mut self.document.sections[section_idx].paragraphs[insert_pi];

            // controls/ctrl_data_records 삽입 (범위 보정)
            let ctrl_insert = insert_ci.min(para.controls.len());
            para.controls
                .insert(ctrl_insert, Control::Shape(Box::new(group_obj)));
            let cdr_insert = ctrl_insert.min(para.ctrl_data_records.len());
            para.ctrl_data_records.insert(cdr_insert, None);

            // char_offsets: 텍스트 문자 매핑이므로 컨트롤 인덱스와 무관
            // 기존 char_offsets에서 마지막 gap 위치 다음에 8바이트 추가
            if !para.char_offsets.is_empty() {
                // 모든 기존 char_offsets를 8씩 증가 (컨트롤이 앞에 삽입되므로)
                for co in para.char_offsets.iter_mut() {
                    *co += 8;
                }
            }
            para.char_count += 8;
            para.control_mask |= 0x00000800;
            para.has_para_text = true;
        }

        // 7) 리플로우 + 페이지네이션
        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: insert_pi,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{}",
            insert_pi, insert_ci
        )))
    }

    /// 머리말/꼬리말 내부 Shape/Picture 컨트롤을 GroupShape로 묶는다.
    pub fn group_header_footer_shapes_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        targets: &[(usize, usize)],
    ) -> Result<String, HwpError> {
        let (insert_pi, insert_ci);
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let inner_paras: &mut Vec<Paragraph> = match outer_ctrl {
                Control::Header(header) => &mut header.paragraphs,
                Control::Footer(footer) => &mut footer.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            let new_z_order = Self::max_shape_z_order_in_paragraphs(inner_paras) + 1;
            (insert_pi, insert_ci) =
                Self::group_shape_controls_in_paragraphs(inner_paras, targets, new_z_order)?;
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: outer_para_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"innerParaIdx\":{},\"innerControlIdx\":{}",
            outer_para_idx, outer_control_idx, insert_pi, insert_ci
        )))
    }

    /// GroupShape를 풀어 자식 개체들을 개별 Shape/Picture로 복원한다.
    /// 스펙: 한 단계만 풀기 (중첩 그룹은 유지), 자식 cnt 1 감소
    pub fn ungroup_shape_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
    ) -> Result<String, HwpError> {
        use crate::model::control::Control;
        use crate::model::shape::*;

        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        let section = &mut self.document.sections[section_idx];
        if para_idx >= section.paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }
        let para = &mut section.paragraphs[para_idx];
        if control_idx >= para.controls.len() {
            return Err(HwpError::RenderError(format!(
                "컨트롤 인덱스 {} 범위 초과",
                control_idx
            )));
        }

        // GroupShape 추출
        match &para.controls[control_idx] {
            Control::Shape(s) => match s.as_ref() {
                ShapeObject::Group(_) => {}
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 컨트롤이 GroupShape이 아닙니다".to_string(),
                    ))
                }
            },
            _ => {
                return Err(HwpError::RenderError(
                    "지정된 컨트롤이 Shape이 아닙니다".to_string(),
                ))
            }
        };
        // GroupShape를 꺼냄
        let group_ctrl = para.controls.remove(control_idx);
        if control_idx < para.ctrl_data_records.len() {
            para.ctrl_data_records.remove(control_idx);
        }
        if para.char_count >= 8 {
            para.char_count -= 8;
        }

        let group_shape = match group_ctrl {
            Control::Shape(s) => match *s {
                ShapeObject::Group(g) => g,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };

        // 그룹의 글로벌 좌표
        let group_x = group_shape.common.horizontal_offset as i32;
        let group_y = group_shape.common.vertical_offset as i32;
        // 그룹 스케일 (리사이즈된 경우)
        let gsa = &group_shape.shape_attr;
        let group_sx = if gsa.original_width > 0 {
            gsa.current_width as f64 / gsa.original_width as f64
        } else {
            1.0
        };
        let group_sy = if gsa.original_height > 0 {
            gsa.current_height as f64 / gsa.original_height as f64
        } else {
            1.0
        };

        // 자식들을 개별 컨트롤로 복원
        let mut insert_idx = control_idx;
        for mut child in group_shape.children {
            // 파일에서 로드한 그룹 자식은 common이 기본값(0) → shape_attr에서 복원
            {
                let sa = child.shape_attr();
                let sa_w = sa.original_width;
                let sa_h = sa.original_height;
                let sa_ox = sa.offset_x;
                let sa_oy = sa.offset_y;
                let c = child.common_mut();
                if c.width == 0 && sa_w > 0 {
                    c.width = sa_w;
                }
                if c.height == 0 && sa_h > 0 {
                    c.height = sa_h;
                }
                if c.horizontal_offset == 0 && sa_ox > 0 {
                    c.horizontal_offset = sa_ox as u32;
                }
                if c.vertical_offset == 0 && sa_oy > 0 {
                    c.vertical_offset = sa_oy as u32;
                }
            }
            // 자식의 로컬 좌표를 글로벌 좌표로 변환 (그룹 스케일 적용)
            {
                let c = child.common_mut();
                c.horizontal_offset =
                    (group_x + (c.horizontal_offset as f64 * group_sx) as i32) as u32;
                c.vertical_offset = (group_y + (c.vertical_offset as f64 * group_sy) as i32) as u32;
                c.width = ((c.width as f64 * group_sx).round().max(1.0)) as u32;
                c.height = ((c.height as f64 * group_sy).round().max(1.0)) as u32;
                c.vert_rel_to = group_shape.common.vert_rel_to;
                c.vert_align = group_shape.common.vert_align;
                c.horz_rel_to = group_shape.common.horz_rel_to;
                c.horz_align = group_shape.common.horz_align;
                c.text_wrap = group_shape.common.text_wrap;
                c.attr = group_shape.common.attr;
                c.treat_as_char = group_shape.common.treat_as_char;
            }
            // 도형별 좌표에 그룹 스케일 적용
            if group_sx != 1.0 || group_sy != 1.0 {
                Self::scale_shape_coords(&mut child, group_sx, group_sy);
            }
            // shape_attr 갱신 (common 값 확정 후)
            let final_w = child.common().width;
            let final_h = child.common().height;
            {
                let sa = match &mut child {
                    ShapeObject::Line(s) => &mut s.drawing.shape_attr,
                    ShapeObject::Rectangle(s) => &mut s.drawing.shape_attr,
                    ShapeObject::Ellipse(s) => &mut s.drawing.shape_attr,
                    ShapeObject::Arc(s) => &mut s.drawing.shape_attr,
                    ShapeObject::Polygon(s) => &mut s.drawing.shape_attr,
                    ShapeObject::Curve(s) => &mut s.drawing.shape_attr,
                    ShapeObject::Group(g) => &mut g.shape_attr,
                    ShapeObject::Picture(p) => &mut p.shape_attr,
                    ShapeObject::Chart(c) => &mut c.drawing.shape_attr,
                    ShapeObject::Ole(o) => &mut o.drawing.shape_attr,
                };
                if sa.group_level > 0 {
                    sa.group_level -= 1;
                }
                sa.offset_x = 0;
                sa.offset_y = 0;
                sa.render_tx = 0.0;
                sa.render_ty = 0.0;
                sa.current_width = final_w;
                sa.original_width = final_w;
                sa.current_height = final_h;
                sa.original_height = final_h;
                sa.is_two_ctrl_id = true;
                sa.raw_rendering = Vec::new();
            }

            // 문단에 삽입
            para.controls
                .insert(insert_idx, Control::Shape(Box::new(child)));
            para.ctrl_data_records.insert(insert_idx, None);
            para.char_count += 8;
            para.control_mask |= 0x00000800;
            para.has_para_text = true;
            insert_idx += 1;
        }

        // char_offsets: 그룹 1개 → 자식 N개, net 변화 = (N-1) * 8
        let children_count = insert_idx - control_idx;
        if children_count > 1 && !para.char_offsets.is_empty() {
            let net_delta = ((children_count - 1) * 8) as u32;
            for co in para.char_offsets.iter_mut() {
                *co += net_delta;
            }
        }

        // 리플로우 + 페이지네이션
        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: para_idx,
            ctrl: control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// 머리말/꼬리말 내부 GroupShape를 한 단계 풀어 개별 Shape 컨트롤로 복원한다.
    pub fn ungroup_header_footer_shape_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            let inner_paras: &mut Vec<Paragraph> = match outer_ctrl {
                Control::Header(header) => &mut header.paragraphs,
                Control::Footer(footer) => &mut footer.paragraphs,
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            };
            Self::ungroup_shape_control_in_paragraphs(
                inner_paras,
                inner_para_idx,
                inner_control_idx,
            )?;
        }

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: outer_para_idx,
            ctrl: outer_control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    // ─── 수식 속성 API ──────────────────────────────────

    /// 수식 컨트롤의 속성을 조회한다 (네이티브).
    /// 표 셀 내 또는 본문의 수식 컨트롤을 찾아 불변 참조를 반환한다.
    fn find_equation_ref(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_idx: Option<usize>,
        cell_para_idx: Option<usize>,
    ) -> Result<&crate::model::control::Equation, HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;

        let ctrl = if let (Some(ci), Some(cpi)) = (cell_idx, cell_para_idx) {
            // 표 셀 내 수식
            let para = section.paragraphs.get(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?;
            let table = match para.controls.get(control_idx) {
                Some(Control::Table(t)) => t,
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 컨트롤이 표가 아닙니다".to_string(),
                    ))
                }
            };
            let cell = table
                .cells
                .get(ci)
                .ok_or_else(|| HwpError::RenderError(format!("셀 인덱스 {} 범위 초과", ci)))?;
            let cell_para = cell.paragraphs.get(cpi).ok_or_else(|| {
                HwpError::RenderError(format!("셀 문단 인덱스 {} 범위 초과", cpi))
            })?;
            // 셀 문단의 첫 번째 수식 컨트롤을 찾는다
            cell_para
                .controls
                .iter()
                .find(|c| matches!(c, Control::Equation(_)))
                .ok_or_else(|| {
                    HwpError::RenderError("셀 문단에 수식 컨트롤이 없습니다".to_string())
                })?
        } else {
            // 본문 수식
            let para = section.paragraphs.get(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?;
            para.controls.get(control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
            })?
        };

        match ctrl {
            Control::Equation(e) => Ok(e),
            _ => Err(HwpError::RenderError(
                "지정된 컨트롤이 수식이 아닙니다".to_string(),
            )),
        }
    }

    /// 표 셀 내 또는 본문의 수식 컨트롤을 찾아 가변 참조를 반환한다.
    fn find_equation_mut(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_idx: Option<usize>,
        cell_para_idx: Option<usize>,
    ) -> Result<&mut crate::model::control::Equation, HwpError> {
        let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;

        let ctrl = if let (Some(ci), Some(cpi)) = (cell_idx, cell_para_idx) {
            // 표 셀 내 수식
            let para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?;
            let table = match para.controls.get_mut(control_idx) {
                Some(Control::Table(t)) => t,
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 컨트롤이 표가 아닙니다".to_string(),
                    ))
                }
            };
            let cell = table
                .cells
                .get_mut(ci)
                .ok_or_else(|| HwpError::RenderError(format!("셀 인덱스 {} 범위 초과", ci)))?;
            let cell_para = cell.paragraphs.get_mut(cpi).ok_or_else(|| {
                HwpError::RenderError(format!("셀 문단 인덱스 {} 범위 초과", cpi))
            })?;
            cell_para
                .controls
                .iter_mut()
                .find(|c| matches!(c, Control::Equation(_)))
                .ok_or_else(|| {
                    HwpError::RenderError("셀 문단에 수식 컨트롤이 없습니다".to_string())
                })?
        } else {
            // 본문 수식
            let para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
            })?;
            para.controls.get_mut(control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
            })?
        };

        match ctrl {
            Control::Equation(e) => Ok(e),
            _ => Err(HwpError::RenderError(
                "지정된 컨트롤이 수식이 아닙니다".to_string(),
            )),
        }
    }

    fn find_note_equation_ref(
        &self,
        kind: &str,
        section_idx: usize,
        parent_para_idx: usize,
        note_control_idx: usize,
        note_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<&crate::model::control::Equation, HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let para = section.paragraphs.get(parent_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
        })?;
        let note_para = match para.controls.get(note_control_idx) {
            Some(Control::Footnote(note)) if kind == "footnote" => {
                note.paragraphs.get(note_para_idx)
            }
            Some(Control::Endnote(note)) if kind == "endnote" => note.paragraphs.get(note_para_idx),
            _ => None,
        }
        .ok_or_else(|| {
            HwpError::RenderError(format!(
                "각주/미주 문단을 찾을 수 없습니다: kind={} sec={} para={} ctrl={} note_para={}",
                kind, section_idx, parent_para_idx, note_control_idx, note_para_idx
            ))
        })?;
        match note_para.controls.get(inner_control_idx) {
            Some(Control::Equation(eq)) => Ok(eq),
            _ => Err(HwpError::RenderError(format!(
                "각주/미주 내부 컨트롤 {}은 수식이 아닙니다",
                inner_control_idx
            ))),
        }
    }

    fn find_note_equation_mut(
        &mut self,
        kind: &str,
        section_idx: usize,
        parent_para_idx: usize,
        note_control_idx: usize,
        note_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<&mut crate::model::control::Equation, HwpError> {
        let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let para = section.paragraphs.get_mut(parent_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", parent_para_idx))
        })?;
        let note_para = match para.controls.get_mut(note_control_idx) {
            Some(Control::Footnote(note)) if kind == "footnote" => {
                note.paragraphs.get_mut(note_para_idx)
            }
            Some(Control::Endnote(note)) if kind == "endnote" => {
                note.paragraphs.get_mut(note_para_idx)
            }
            _ => None,
        }
        .ok_or_else(|| {
            HwpError::RenderError(format!(
                "각주/미주 문단을 찾을 수 없습니다: kind={} sec={} para={} ctrl={} note_para={}",
                kind, section_idx, parent_para_idx, note_control_idx, note_para_idx
            ))
        })?;
        match note_para.controls.get_mut(inner_control_idx) {
            Some(Control::Equation(eq)) => Ok(eq),
            _ => Err(HwpError::RenderError(format!(
                "각주/미주 내부 컨트롤 {}은 수식이 아닙니다",
                inner_control_idx
            ))),
        }
    }

    fn equation_properties_json(eq: &crate::model::control::Equation) -> String {
        let common_json = Self::common_obj_attr_to_json(&eq.common);
        let script_escaped = super::super::helpers::json_escape(&eq.script);
        let font_name_escaped = super::super::helpers::json_escape(&eq.font_name);
        let line_mode_escaped = super::super::helpers::json_escape(&eq.line_mode);

        format!(
            concat!(
                "{{{},\"script\":\"{}\",\"fontSize\":{},\"color\":{},",
                "\"baseline\":{},\"fontName\":\"{}\",\"lineMode\":\"{}\",",
                "\"hasCaption\":false,\"captionDirection\":\"None\",",
                "\"captionWidth\":0,\"captionSpacing\":0}}"
            ),
            common_json,
            script_escaped,
            eq.font_size,
            eq.color,
            eq.baseline,
            font_name_escaped,
            line_mode_escaped,
        )
    }

    fn apply_equation_properties(
        eq: &mut crate::model::control::Equation,
        dpi: f64,
        props_json: &str,
    ) {
        use super::super::helpers::{json_i32, json_str, json_u32};
        use crate::renderer::equation::layout::EqLayout;
        use crate::renderer::equation::parser::EqParser;
        use crate::renderer::equation::tokenizer::tokenize;
        use crate::renderer::hwpunit_to_px;

        fn json_u32_alias(props_json: &str, primary: &str, alias: &str) -> Option<u32> {
            json_u32(props_json, primary).or_else(|| json_u32(props_json, alias))
        }

        fn json_str_alias(props_json: &str, primary: &str, alias: &str) -> Option<String> {
            json_str(props_json, primary).or_else(|| json_str(props_json, alias))
        }

        if let Some(s) = json_str(props_json, "script") {
            eq.script = s;
        }
        if let Some(fs) = json_u32_alias(props_json, "fontSize", "font_size") {
            eq.font_size = fs;
        }
        if let Some(c) = json_u32(props_json, "color") {
            eq.color = c;
        }
        if let Some(bl) = json_i32(props_json, "baseline") {
            eq.baseline = bl as i16;
        }
        if let Some(fn_) = json_str_alias(props_json, "fontName", "font_name") {
            eq.font_name = fn_;
        }
        if let Some(line_mode) = json_str_alias(props_json, "lineMode", "line_mode") {
            eq.line_mode = line_mode;
        }
        Self::apply_common_obj_attr_from_json(&mut eq.common, props_json);

        let font_size_px = hwpunit_to_px(eq.font_size as i32, dpi);
        let tokens = tokenize(&eq.script);
        let ast = EqParser::new(tokens).parse();
        let layout_box = EqLayout::new(font_size_px).layout(&ast);
        let new_w = crate::renderer::px_to_hwpunit(layout_box.width, dpi).max(0) as u32;
        let new_h = crate::renderer::px_to_hwpunit(layout_box.height, dpi).max(0) as u32;
        eq.common.width = new_w;
        eq.common.height = new_h;
    }

    pub fn get_equation_properties_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_idx: Option<usize>,
        cell_para_idx: Option<usize>,
    ) -> Result<String, HwpError> {
        let eq = self.find_equation_ref(
            section_idx,
            parent_para_idx,
            control_idx,
            cell_idx,
            cell_para_idx,
        )?;

        Ok(Self::equation_properties_json(eq))
    }

    /// 수식 컨트롤의 속성을 변경한다 (네이티브).
    pub fn set_equation_properties_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_idx: Option<usize>,
        cell_para_idx: Option<usize>,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let dpi = self.dpi;
        let eq = self.find_equation_mut(
            section_idx,
            parent_para_idx,
            control_idx,
            cell_idx,
            cell_para_idx,
        )?;
        Self::apply_equation_properties(eq, dpi, props_json);

        // 표 셀 내 수식인 경우 표 dirty 플래그 설정
        if cell_idx.is_some() {
            if let Some(Control::Table(t)) = self.document.sections[section_idx].paragraphs
                [parent_para_idx]
                .controls
                .get_mut(control_idx)
            {
                t.dirty = true;
            }
        }

        // 재조판
        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        Ok(super::super::helpers::json_ok())
    }

    /// 표 셀/글상자 경로 내부 문단의 수식 컨트롤 속성을 변경한다.
    pub fn set_equation_properties_in_cell_by_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path: &[(usize, usize, usize)],
        inner_control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        if cell_path.is_empty() {
            return Err(HwpError::RenderError(
                "cell_path 가 비어있습니다".to_string(),
            ));
        }
        let outer_ctrl = cell_path[0].0;
        let outer_cell = cell_path[0].1;
        let dpi = self.dpi;
        {
            let cell_para =
                self.get_cell_paragraph_mut_by_path(section_idx, parent_para_idx, cell_path)?;
            let ctrl = cell_para
                .controls
                .get_mut(inner_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!("셀 내 컨트롤 {} 범위 초과", inner_control_idx))
                })?;
            let eq = match ctrl {
                Control::Equation(eq) => eq,
                _ => {
                    return Err(HwpError::RenderError(
                        "지정된 셀 내부 컨트롤이 수식이 아닙니다".to_string(),
                    ))
                }
            };
            Self::apply_equation_properties(eq, dpi, props_json);
        }

        self.mark_cell_control_dirty(section_idx, parent_para_idx, outer_ctrl);
        self.document.sections[section_idx].raw_stream = None;
        self.mark_section_dirty(section_idx);
        self.paginate_if_needed();
        self.event_log.push(DocumentEvent::CellTextChanged {
            section: section_idx,
            para: parent_para_idx,
            ctrl: outer_ctrl,
            cell: outer_cell,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"innerControl\":{}",
            inner_control_idx
        )))
    }

    /// 직접 표 셀 문단의 특정 수식 컨트롤 속성을 변경한다.
    pub fn set_equation_properties_in_cell_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_idx: usize,
        cell_para_idx: usize,
        inner_control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let cell_path = [(control_idx, cell_idx, cell_para_idx)];
        let result = self.set_equation_properties_in_cell_by_path_native(
            section_idx,
            parent_para_idx,
            &cell_path,
            inner_control_idx,
            props_json,
        )?;
        self.reflow_cell_paragraph(
            section_idx,
            parent_para_idx,
            control_idx,
            cell_idx,
            cell_para_idx,
        );
        Ok(result)
    }

    pub fn get_note_equation_properties_native(
        &self,
        kind: &str,
        section_idx: usize,
        parent_para_idx: usize,
        note_control_idx: usize,
        note_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let eq = self.find_note_equation_ref(
            kind,
            section_idx,
            parent_para_idx,
            note_control_idx,
            note_para_idx,
            inner_control_idx,
        )?;
        Ok(Self::equation_properties_json(eq))
    }

    pub fn set_note_equation_properties_native(
        &mut self,
        kind: &str,
        section_idx: usize,
        parent_para_idx: usize,
        note_control_idx: usize,
        note_para_idx: usize,
        inner_control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let dpi = self.dpi;
        let eq = self.find_note_equation_mut(
            kind,
            section_idx,
            parent_para_idx,
            note_control_idx,
            note_para_idx,
            inner_control_idx,
        )?;
        Self::apply_equation_properties(eq, dpi, props_json);

        let section = &mut self.document.sections[section_idx];
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        Ok(super::super::helpers::json_ok())
    }

    fn header_footer_apply_to_u8(apply_to: crate::model::header_footer::HeaderFooterApply) -> u8 {
        match apply_to {
            crate::model::header_footer::HeaderFooterApply::Both => 0,
            crate::model::header_footer::HeaderFooterApply::Even => 1,
            crate::model::header_footer::HeaderFooterApply::Odd => 2,
        }
    }

    /// 머리말/꼬리말 문단에 수식 컨트롤을 삽입한다.
    pub fn insert_equation_in_header_footer_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        char_offset: usize,
        script: &str,
        font_size: u32,
        color: u32,
    ) -> Result<String, HwpError> {
        let equation = Self::equation_control(script, font_size, color);
        let (is_header, apply_to, insert_idx) = {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            match outer_ctrl {
                Control::Header(header) => {
                    let inner_para =
                        header.paragraphs.get_mut(inner_para_idx).ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "머리말 문단 인덱스 {} 범위 초과",
                                inner_para_idx
                            ))
                        })?;
                    let insert_idx = Self::insert_equation_control_in_paragraph(
                        inner_para,
                        char_offset,
                        equation,
                    );
                    (
                        true,
                        Self::header_footer_apply_to_u8(header.apply_to),
                        insert_idx,
                    )
                }
                Control::Footer(footer) => {
                    let inner_para =
                        footer.paragraphs.get_mut(inner_para_idx).ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "꼬리말 문단 인덱스 {} 범위 초과",
                                inner_para_idx
                            ))
                        })?;
                    let insert_idx = Self::insert_equation_control_in_paragraph(
                        inner_para,
                        char_offset,
                        equation,
                    );
                    (
                        false,
                        Self::header_footer_apply_to_u8(footer.apply_to),
                        insert_idx,
                    )
                }
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            }
        };

        self.reflow_hf_paragraph(section_idx, is_header, apply_to, inner_para_idx);
        self.document.sections[section_idx].raw_stream = None;
        self.mark_section_dirty(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: outer_para_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"hfParaIdx\":{}",
            outer_para_idx, insert_idx, inner_para_idx
        )))
    }

    /// 머리말/꼬리말 내부 수식 컨트롤의 속성을 변경한다.
    pub fn set_header_footer_equation_properties_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let dpi = self.dpi;
        let (is_header, apply_to) = {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            match outer_ctrl {
                Control::Header(header) => {
                    let inner_para =
                        header.paragraphs.get_mut(inner_para_idx).ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "머리말 문단 인덱스 {} 범위 초과",
                                inner_para_idx
                            ))
                        })?;
                    let ctrl = inner_para
                        .controls
                        .get_mut(inner_control_idx)
                        .ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "내부 컨트롤 인덱스 {} 범위 초과",
                                inner_control_idx
                            ))
                        })?;
                    let eq = match ctrl {
                        Control::Equation(eq) => eq,
                        _ => {
                            return Err(HwpError::RenderError(
                                "지정된 내부 컨트롤이 수식이 아닙니다".to_string(),
                            ))
                        }
                    };
                    Self::apply_equation_properties(eq, dpi, props_json);
                    (true, Self::header_footer_apply_to_u8(header.apply_to))
                }
                Control::Footer(footer) => {
                    let inner_para =
                        footer.paragraphs.get_mut(inner_para_idx).ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "꼬리말 문단 인덱스 {} 범위 초과",
                                inner_para_idx
                            ))
                        })?;
                    let ctrl = inner_para
                        .controls
                        .get_mut(inner_control_idx)
                        .ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "내부 컨트롤 인덱스 {} 범위 초과",
                                inner_control_idx
                            ))
                        })?;
                    let eq = match ctrl {
                        Control::Equation(eq) => eq,
                        _ => {
                            return Err(HwpError::RenderError(
                                "지정된 내부 컨트롤이 수식이 아닙니다".to_string(),
                            ))
                        }
                    };
                    Self::apply_equation_properties(eq, dpi, props_json);
                    (false, Self::header_footer_apply_to_u8(footer.apply_to))
                }
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            }
        };

        self.reflow_hf_paragraph(section_idx, is_header, apply_to, inner_para_idx);
        self.document.sections[section_idx].raw_stream = None;
        self.mark_section_dirty(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureResized {
            section: section_idx,
            para: outer_para_idx,
            ctrl: outer_control_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"innerControl\":{}",
            inner_control_idx
        )))
    }

    /// 머리말/꼬리말 내부 수식 컨트롤을 삭제한다.
    pub fn delete_header_footer_equation_control_native(
        &mut self,
        section_idx: usize,
        outer_para_idx: usize,
        outer_control_idx: usize,
        inner_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let (is_header, apply_to) = {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let outer_para = section.paragraphs.get_mut(outer_para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("외부 문단 인덱스 {} 범위 초과", outer_para_idx))
            })?;
            let outer_ctrl = outer_para
                .controls
                .get_mut(outer_control_idx)
                .ok_or_else(|| {
                    HwpError::RenderError(format!(
                        "외부 컨트롤 인덱스 {} 범위 초과",
                        outer_control_idx
                    ))
                })?;
            match outer_ctrl {
                Control::Header(header) => {
                    let inner_para =
                        header.paragraphs.get_mut(inner_para_idx).ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "머리말 문단 인덱스 {} 범위 초과",
                                inner_para_idx
                            ))
                        })?;
                    Self::delete_equation_control_from_paragraph(inner_para, inner_control_idx)?;
                    (true, Self::header_footer_apply_to_u8(header.apply_to))
                }
                Control::Footer(footer) => {
                    let inner_para =
                        footer.paragraphs.get_mut(inner_para_idx).ok_or_else(|| {
                            HwpError::RenderError(format!(
                                "꼬리말 문단 인덱스 {} 범위 초과",
                                inner_para_idx
                            ))
                        })?;
                    Self::delete_equation_control_from_paragraph(inner_para, inner_control_idx)?;
                    (false, Self::header_footer_apply_to_u8(footer.apply_to))
                }
                _ => {
                    return Err(HwpError::RenderError(
                        "외부 컨트롤이 머리말/꼬리말이 아닙니다".to_string(),
                    ))
                }
            }
        };

        self.reflow_hf_paragraph(section_idx, is_header, apply_to, inner_para_idx);
        self.document.sections[section_idx].raw_stream = None;
        self.mark_section_dirty(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: outer_para_idx,
            ctrl: outer_control_idx,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"innerControl\":{}",
            inner_control_idx
        )))
    }

    /// 수식 스크립트를 SVG로 렌더링하여 반환한다 (미리보기 전용).
    pub fn render_equation_preview_native(
        &self,
        script: &str,
        font_size_hwpunit: u32,
        color: u32,
    ) -> Result<String, HwpError> {
        use crate::renderer::equation::layout::EqLayout;
        use crate::renderer::equation::parser::EqParser;
        use crate::renderer::equation::svg_render::{eq_color_to_svg, render_equation_svg};
        use crate::renderer::equation::tokenizer::tokenize;

        let font_size_px = crate::renderer::hwpunit_to_px(font_size_hwpunit as i32, self.dpi);
        let tokens = tokenize(script);
        let ast = EqParser::new(tokens).parse();
        let layout_box = EqLayout::new(font_size_px).layout(&ast);
        let color_str = eq_color_to_svg(color);
        let svg_fragment = render_equation_svg(&layout_box, &color_str, font_size_px);

        let w = layout_box.width;
        let h = layout_box.height;
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {:.2} {:.2}\" width=\"{:.2}\" height=\"{:.2}\">{}</svg>",
            w, h, w, h, svg_fragment,
        );
        Ok(svg)
    }

    fn delete_equation_control_from_paragraph(
        para: &mut Paragraph,
        control_idx: usize,
    ) -> Result<(), HwpError> {
        if control_idx >= para.controls.len() {
            return Err(HwpError::RenderError(format!(
                "컨트롤 인덱스 {} 범위 초과",
                control_idx
            )));
        }
        if !matches!(&para.controls[control_idx], Control::Equation(_)) {
            return Err(HwpError::RenderError(
                "지정된 컨트롤이 수식이 아닙니다".to_string(),
            ));
        }

        let text_chars: Vec<char> = para.text.chars().collect();
        let mut ci = 0usize;
        let mut prev_end: u32 = 0;
        let mut gap_start: Option<u32> = None;
        'outer: for i in 0..text_chars.len() {
            let offset = if i < para.char_offsets.len() {
                para.char_offsets[i]
            } else {
                prev_end
            };
            while prev_end + 8 <= offset && ci < para.controls.len() {
                if ci == control_idx {
                    gap_start = Some(prev_end);
                    break 'outer;
                }
                ci += 1;
                prev_end += 8;
            }
            let char_size: u32 = if text_chars[i] == '\t' {
                8
            } else if text_chars[i].len_utf16() == 2 {
                2
            } else {
                1
            };
            prev_end = offset + char_size;
        }
        if gap_start.is_none() {
            while ci < para.controls.len() {
                if ci == control_idx {
                    gap_start = Some(prev_end);
                    break;
                }
                ci += 1;
                prev_end += 8;
            }
        }

        if let Some(gs) = gap_start {
            let threshold = gs + 8;
            for offset in para.char_offsets.iter_mut() {
                if *offset >= threshold {
                    *offset -= 8;
                }
            }
        }

        para.controls.remove(control_idx);
        if control_idx < para.ctrl_data_records.len() {
            para.ctrl_data_records.remove(control_idx);
        }
        if para.char_count >= 8 {
            para.char_count -= 8;
        }
        Ok(())
    }

    /// 수식(Equation) 컨트롤을 문단에서 삭제한다.
    pub fn delete_equation_control_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
    ) -> Result<String, HwpError> {
        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        let section = &mut self.document.sections[section_idx];
        if parent_para_idx >= section.paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                parent_para_idx
            )));
        }
        let para = &mut section.paragraphs[parent_para_idx];
        Self::delete_equation_control_from_paragraph(para, control_idx)?;

        Self::reflow_paragraph_line_segs_after_control_delete(para, &self.styles, self.dpi);
        section.raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();

        self.event_log.push(DocumentEvent::PictureDeleted {
            section: section_idx,
            para: parent_para_idx,
            ctrl: control_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// 표 셀/글상자 경로 내부 문단의 수식 컨트롤을 삭제한다.
    pub fn delete_equation_control_in_cell_by_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path: &[(usize, usize, usize)],
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        if cell_path.is_empty() {
            return Err(HwpError::RenderError(
                "cell_path 가 비어있습니다".to_string(),
            ));
        }
        let outer_ctrl = cell_path[0].0;
        let outer_cell = cell_path[0].1;
        {
            let cell_para =
                self.get_cell_paragraph_mut_by_path(section_idx, parent_para_idx, cell_path)?;
            Self::delete_equation_control_from_paragraph(cell_para, inner_control_idx)?;
        }

        self.mark_cell_control_dirty(section_idx, parent_para_idx, outer_ctrl);
        self.document.sections[section_idx].raw_stream = None;
        self.mark_section_dirty(section_idx);
        self.paginate_if_needed();
        self.event_log.push(DocumentEvent::CellTextChanged {
            section: section_idx,
            para: parent_para_idx,
            ctrl: outer_ctrl,
            cell: outer_cell,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"innerControl\":{}",
            inner_control_idx
        )))
    }

    /// 직접 표 셀 문단의 수식 컨트롤을 삭제한다.
    pub fn delete_equation_control_in_cell_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_idx: usize,
        cell_para_idx: usize,
        inner_control_idx: usize,
    ) -> Result<String, HwpError> {
        let cell_path = [(control_idx, cell_idx, cell_para_idx)];
        let result = self.delete_equation_control_in_cell_by_path_native(
            section_idx,
            parent_para_idx,
            &cell_path,
            inner_control_idx,
        )?;
        self.reflow_cell_paragraph(
            section_idx,
            parent_para_idx,
            control_idx,
            cell_idx,
            cell_para_idx,
        );
        Ok(result)
    }

    // ─── 각주 삽입/삭제 API ──────────────────────────────

    fn footnote_shape_number_format_code(format: crate::model::footnote::NumberFormat) -> u8 {
        crate::model::footnote::FootnoteShape::number_format_attr_code(format) as u8
    }

    fn footnote_shape_number_format_from_str(
        value: &str,
        fallback: crate::model::footnote::NumberFormat,
    ) -> crate::model::footnote::NumberFormat {
        crate::model::footnote::FootnoteShape::number_format_from_name(value, fallback)
    }

    fn footnote_shape_number_format_name(
        format: crate::model::footnote::NumberFormat,
    ) -> &'static str {
        use crate::model::footnote::NumberFormat;
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

    fn footnote_numbering_name(
        numbering: crate::model::footnote::FootnoteNumbering,
    ) -> &'static str {
        use crate::model::footnote::FootnoteNumbering;
        match numbering {
            FootnoteNumbering::Continue => "continue",
            FootnoteNumbering::RestartSection => "restartSection",
            FootnoteNumbering::RestartPage => "restartPage",
        }
    }

    fn footnote_numbering_from_str(
        value: &str,
        fallback: crate::model::footnote::FootnoteNumbering,
    ) -> crate::model::footnote::FootnoteNumbering {
        use crate::model::footnote::FootnoteNumbering;
        match value {
            "continue" | "CONTINUOUS" | "continuous" => FootnoteNumbering::Continue,
            "restartSection" | "ON_SECTION" | "RESTART_SECTION" | "onSection" => {
                FootnoteNumbering::RestartSection
            }
            "restartPage" | "ON_PAGE" | "RESTART_PAGE" | "onPage" => FootnoteNumbering::RestartPage,
            _ => fallback,
        }
    }

    fn footnote_placement_name(
        placement: crate::model::footnote::FootnotePlacement,
    ) -> &'static str {
        use crate::model::footnote::FootnotePlacement;
        match placement {
            FootnotePlacement::EachColumn => "documentEnd",
            FootnotePlacement::BelowText => "sectionEnd",
            FootnotePlacement::RightColumn => "rightColumn",
        }
    }

    fn footnote_placement_from_str(
        value: &str,
        fallback: crate::model::footnote::FootnotePlacement,
    ) -> crate::model::footnote::FootnotePlacement {
        use crate::model::footnote::FootnotePlacement;
        match value {
            "documentEnd" | "eachColumn" => FootnotePlacement::EachColumn,
            "sectionEnd" | "belowText" => FootnotePlacement::BelowText,
            "rightColumn" => FootnotePlacement::RightColumn,
            _ => fallback,
        }
    }

    fn encode_footnote_shape_attr(shape: &crate::model::footnote::FootnoteShape) -> u32 {
        shape.encode_attr()
    }

    fn first_char_or_nul(value: &str) -> char {
        value.chars().next().unwrap_or('\0')
    }

    fn json_escape_note_char(ch: char) -> String {
        if ch == '\0' {
            String::new()
        } else {
            crate::document_core::helpers::json_escape(&ch.to_string())
        }
    }

    fn hwpunit16_from_json(json: &str, key: &str) -> Option<i16> {
        crate::document_core::helpers::json_i32(json, key)
            .map(|v| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16)
    }

    fn make_note_inner_paragraph(
        number_type: crate::model::control::AutoNumberType,
        number: u16,
        format: u8,
        prefix_char: char,
        suffix_char: char,
        default_char_shape_id: u32,
        para_shape_id: u16,
        style_id: u8,
    ) -> crate::model::paragraph::Paragraph {
        use crate::model::paragraph::{CharShapeRef, LineSeg, Paragraph};

        let auto_num = crate::model::control::AutoNumber {
            number_type,
            format,
            superscript: false,
            number,
            assigned_number: number,
            user_symbol: '\0',
            prefix_char,
            suffix_char,
        };

        Paragraph {
            text: "  ".to_string(),
            char_count: 10,
            char_count_msb: true,
            control_mask: 1u32 << 0x12,
            char_offsets: vec![0, 8],
            para_shape_id,
            style_id,
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: default_char_shape_id,
            }],
            controls: vec![crate::model::control::Control::AutoNumber(auto_num)],
            line_segs: vec![LineSeg {
                text_start: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: 0,
                tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                ..Default::default()
            }],
            has_para_text: true,
            ..Default::default()
        }
    }

    fn endnote_style_defaults(&self, section_idx: usize, para_idx: usize) -> (u32, u16, u8) {
        let section = &self.document.sections[section_idx];

        for para in &section.paragraphs {
            for ctrl in &para.controls {
                if let Control::Endnote(en) = ctrl {
                    if let Some(ep) = en.paragraphs.first() {
                        let char_shape_id = ep
                            .char_shapes
                            .first()
                            .map(|cs| cs.char_shape_id)
                            .unwrap_or(0);
                        return (char_shape_id, ep.para_shape_id, ep.style_id);
                    }
                }
            }
        }

        for (idx, style) in self.document.doc_info.styles.iter().enumerate() {
            if style.local_name == "미주" || style.english_name.eq_ignore_ascii_case("Endnote") {
                return (
                    style.char_shape_id as u32,
                    style.para_shape_id,
                    idx.min(u8::MAX as usize) as u8,
                );
            }
        }

        let current_para = &section.paragraphs[para_idx];
        (
            current_para
                .char_shapes
                .first()
                .map(|cs| cs.char_shape_id)
                .unwrap_or(0),
            current_para.para_shape_id,
            current_para.style_id,
        )
    }

    fn sync_endnote_control_with_shape(
        endnote: &mut crate::model::footnote::Endnote,
        number_format_code: u8,
        prefix_char: char,
        suffix_char: char,
    ) {
        use crate::model::control::{AutoNumberType, Control};

        endnote.before_decoration_letter = if prefix_char == '\0' {
            0
        } else {
            prefix_char as u16
        };
        endnote.after_decoration_letter = if suffix_char == '\0' {
            0
        } else {
            suffix_char as u16
        };
        endnote.number_shape = number_format_code as u32;

        for para in &mut endnote.paragraphs {
            for ctrl in &mut para.controls {
                if let Control::AutoNumber(auto_num) = ctrl {
                    if auto_num.number_type == AutoNumberType::Endnote {
                        auto_num.format = number_format_code;
                        auto_num.prefix_char = prefix_char;
                        auto_num.suffix_char = suffix_char;
                        auto_num.number = endnote.number;
                        auto_num.assigned_number = endnote.number;
                    }
                }
            }
        }
    }

    fn renumber_paragraph_endnotes_with_shape(
        paragraphs: &mut [crate::model::paragraph::Paragraph],
        next_number: &mut u16,
        number_format_code: u8,
        prefix_char: char,
        suffix_char: char,
    ) {
        for para in paragraphs {
            for ctrl in &mut para.controls {
                match ctrl {
                    Control::Endnote(endnote) => {
                        endnote.number = *next_number;
                        Self::sync_endnote_control_with_shape(
                            endnote,
                            number_format_code,
                            prefix_char,
                            suffix_char,
                        );
                        *next_number = next_number.saturating_add(1);
                    }
                    Control::Table(table) => {
                        for cell in &mut table.cells {
                            Self::renumber_paragraph_endnotes_with_shape(
                                &mut cell.paragraphs,
                                next_number,
                                number_format_code,
                                prefix_char,
                                suffix_char,
                            );
                        }
                    }
                    Control::Shape(shape) => {
                        if let Some(text_box) =
                            shape.drawing_mut().and_then(|d| d.text_box.as_mut())
                        {
                            Self::renumber_paragraph_endnotes_with_shape(
                                &mut text_box.paragraphs,
                                next_number,
                                number_format_code,
                                prefix_char,
                                suffix_char,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// 각주를 삽입한다.
    /// 커서 위치에 각주 컨트롤을 추가하고 빈 문단 1개를 생성한다.
    /// 반환: JSON `{"ok":true, "paraIdx":N, "controlIdx":N, "footnoteNumber":N}`
    pub fn insert_footnote_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
    ) -> Result<String, HwpError> {
        use crate::model::footnote::Footnote;
        use crate::model::paragraph::{CharShapeRef, LineSeg, Paragraph};

        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }

        // 각주 번호: 삽입 위치 이전의 모든 각주 수 + 1
        // 본문 문단 + 표 셀 + 글상자 내부의 각주를 모두 포함
        let footnote_number = {
            let mut count = 0u16;
            let section = &self.document.sections[section_idx];
            for (pi, para) in section.paragraphs.iter().enumerate() {
                let is_before = pi < para_idx;
                let is_same = pi == para_idx;
                // 본문 문단의 각주
                for (ci, ctrl) in para.controls.iter().enumerate() {
                    match ctrl {
                        Control::Footnote(_) => {
                            if is_before {
                                count += 1;
                            } else if is_same {
                                let positions =
                                    crate::document_core::helpers::find_control_text_positions(
                                        para,
                                    );
                                let pos = positions.get(ci).copied().unwrap_or(usize::MAX);
                                if pos <= char_offset {
                                    count += 1;
                                }
                            }
                        }
                        // 표 셀 내 각주
                        Control::Table(table) if is_before || is_same => {
                            for cell in &table.cells {
                                for cp in &cell.paragraphs {
                                    count +=
                                        cp.controls
                                            .iter()
                                            .filter(|c| matches!(c, Control::Footnote(_)))
                                            .count() as u16;
                                }
                            }
                        }
                        // 글상자 내 각주
                        Control::Shape(shape) if is_before || is_same => {
                            if let Some(text_box) =
                                shape.drawing().and_then(|d| d.text_box.as_ref())
                            {
                                for tp in &text_box.paragraphs {
                                    count +=
                                        tp.controls
                                            .iter()
                                            .filter(|c| matches!(c, Control::Footnote(_)))
                                            .count() as u16;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            count + 1
        };

        // 각주 내부 문단 생성: 기존 각주의 스타일을 참조하여 동일한 스타일 적용
        // 기존 각주가 없으면 본문 문단 스타일 사용
        let (default_char_shape_id, default_para_shape_id) = {
            let section = &self.document.sections[section_idx];
            let mut found = None;
            // 본문 문단의 각주에서 스타일 참조
            'outer: for para in &section.paragraphs {
                for ctrl in &para.controls {
                    if let Control::Footnote(fn_) = ctrl {
                        if let Some(fp) = fn_.paragraphs.first() {
                            found = Some((
                                fp.char_shapes
                                    .first()
                                    .map(|cs| cs.char_shape_id)
                                    .unwrap_or(0),
                                fp.para_shape_id,
                            ));
                            break 'outer;
                        }
                    }
                    // 표 셀 내 각주에서도 참조
                    if let Control::Table(table) = ctrl {
                        for cell in &table.cells {
                            for cp in &cell.paragraphs {
                                for cc in &cp.controls {
                                    if let Control::Footnote(fn_) = cc {
                                        if let Some(fp) = fn_.paragraphs.first() {
                                            found = Some((
                                                fp.char_shapes
                                                    .first()
                                                    .map(|cs| cs.char_shape_id)
                                                    .unwrap_or(0),
                                                fp.para_shape_id,
                                            ));
                                            break 'outer;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            found.unwrap_or_else(|| {
                let current_para = &section.paragraphs[para_idx];
                (
                    current_para
                        .char_shapes
                        .first()
                        .map(|cs| cs.char_shape_id)
                        .unwrap_or(0),
                    current_para.para_shape_id,
                )
            })
        };

        // [Task #1058 reopen Round 5] 신규 각주 inner paragraph 한컴 contract 정합:
        //   - style_id = 11 (각주 style, 한컴 DocInfo 기본 각주 style ID)
        //   - para_shape_id = 0 (각주 default ParaShape)
        //   - controls = [AutoNumber] (각주 번호 inline 컨트롤, char index 0 위치)
        //   - text = "  " (placeholder space ×2, AutoNumber 가 두 space 사이 8 cu 차지)
        //   - char_offsets = [0, 8] (첫 space pos 0, AutoNumber anchor 점유 pos 0~7, 두 번째 space pos 8)
        //   - char_count = 10 (2 placeholder + 8 AutoNumber inline ctrl)
        //   - has_para_text = true
        // 한컴 정답지 samples/footnote-01.hwp 의 각주 inner_para 와 동일한 contract.
        // 사용자 입력은 두 placeholder 뒤 (char_offset=2) 부터 시작 — insert_text_at 의
        // 일반 분기가 char_offsets[i] = base + sum(widths) 시프트 (jump 8 보존).
        let auto_num = crate::model::control::AutoNumber {
            number_type: crate::model::control::AutoNumberType::Footnote,
            format: 0, // Digit
            superscript: false,
            number: footnote_number,
            assigned_number: footnote_number,
            user_symbol: '\0',
            prefix_char: '\0',
            suffix_char: ')',
        };
        let inner_para = Paragraph {
            text: "  ".to_string(), // placeholder space ×2 (정답지 정합)
            char_count: 10,         // 2 placeholder + 8 (AutoNumber inline ctrl)
            char_count_msb: true,
            control_mask: 1u32 << 0x12, // bit 18 (AutoNumber)
            char_offsets: vec![0, 8],   // AutoNumber 가 두 space 사이 8 cu 차지
            para_shape_id: 0,
            style_id: 11, // 각주 style
            char_shapes: vec![CharShapeRef {
                start_pos: 0,
                char_shape_id: default_char_shape_id,
            }],
            controls: vec![crate::model::control::Control::AutoNumber(auto_num)],
            line_segs: vec![LineSeg {
                text_start: 0,
                line_height: 1000,
                text_height: 1000,
                baseline_distance: 850,
                line_spacing: 600,
                segment_width: 0,
                tag: LineSeg::TAG_SINGLE_SEGMENT_LINE,
                ..Default::default()
            }],
            has_para_text: true,
            ..Default::default()
        };
        // default_para_shape_id 변수가 위에서 unused 가 되지 않도록 (caller paragraph 의 ps 정보는
        // 본 본문 paragraph 의 contract 보존 — 각주 본문은 ps_id=0 사용)
        let _ = default_para_shape_id;

        let footnote = Footnote {
            number: footnote_number,
            paragraphs: vec![inner_para],
            // [Task #1050] HWP5 CTRL_FOOTNOTE 한컴 default
            after_decoration_letter: 0x0029, // ')'
            ..Default::default()
        };

        // 문단에 각주 컨트롤 삽입
        self.document.sections[section_idx].raw_stream = None;
        let paragraph = &mut self.document.sections[section_idx].paragraphs[para_idx];

        // 삽입 위치 결정 (char_offset 기준)
        let insert_idx = {
            let positions = crate::document_core::helpers::find_control_text_positions(paragraph);
            let mut idx = paragraph.controls.len();
            for (i, &pos) in positions.iter().enumerate() {
                if pos > char_offset {
                    idx = i;
                    break;
                }
            }
            idx
        };

        paragraph
            .controls
            .insert(insert_idx, Control::Footnote(Box::new(footnote)));
        paragraph.ctrl_data_records.insert(insert_idx, None);

        // char_offsets 조정: char_offset 위치에 8바이트 갭 생성
        // char_offsets[i]는 텍스트 i번째 문자의 UTF-16 오프셋 (컨트롤은 갭으로 표현)
        // 주의: char_offset은 텍스트 기준 인덱스이지만, char_offsets 배열 길이는 text.chars().count()
        // text에 포함되지 않는 제어 문자(cc - text_len 차이)가 있을 수 있으므로 범위 확인
        if !paragraph.char_offsets.is_empty() {
            let text_len = paragraph.text.chars().count();
            let safe_offset = char_offset.min(text_len);
            for co in paragraph.char_offsets[safe_offset..].iter_mut() {
                *co += 8;
            }
        }
        paragraph.char_count += 8;
        paragraph.control_mask |= 1u32 << 0x0011; // 각주/미주 비트
        paragraph.has_para_text = true;

        // 전체 각주 순서 번호 재계산 (1부터 순차)
        // 본문 문단 + 표 셀 + 글상자 내부의 각주를 모두 포함
        {
            let mut num = 1u16;
            for pi in 0..self.document.sections[section_idx].paragraphs.len() {
                for ci in 0..self.document.sections[section_idx].paragraphs[pi]
                    .controls
                    .len()
                {
                    match &mut self.document.sections[section_idx].paragraphs[pi].controls[ci] {
                        Control::Footnote(ref mut fn_) => {
                            fn_.number = num;
                            num += 1;
                        }
                        Control::Table(ref mut table) => {
                            for cell in &mut table.cells {
                                for cp in &mut cell.paragraphs {
                                    for cc in &mut cp.controls {
                                        if let Control::Footnote(ref mut fn_) = cc {
                                            fn_.number = num;
                                            num += 1;
                                        }
                                    }
                                }
                            }
                        }
                        Control::Shape(ref mut shape) => {
                            if let Some(text_box) =
                                shape.drawing_mut().and_then(|d| d.text_box.as_mut())
                            {
                                for tp in &mut text_box.paragraphs {
                                    for tc in &mut tp.controls {
                                        if let Control::Footnote(ref mut fn_) = tc {
                                            fn_.number = num;
                                            num += 1;
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // 각주 내부 문단 리플로우
        self.reflow_footnote_paragraph(section_idx, para_idx, insert_idx, 0);

        // 본문 문단 리플로우 (각주 마커 폭으로 인한 줄넘김 변경 반영)
        {
            use crate::renderer::composer::reflow_line_segs;
            use crate::renderer::hwpunit_to_px;
            let page_def = &self.document.sections[section_idx].section_def.page_def;
            let text_width =
                page_def.width as i32 - page_def.margin_left as i32 - page_def.margin_right as i32;
            let available_width = hwpunit_to_px(text_width, self.dpi);
            let para_style = self.styles.para_styles.get(
                self.document.sections[section_idx].paragraphs[para_idx].para_shape_id as usize,
            );
            let margin_left = para_style.map(|s| s.margin_left).unwrap_or(0.0);
            let margin_right = para_style.map(|s| s.margin_right).unwrap_or(0.0);
            let final_width = (available_width - margin_left - margin_right).max(0.0);
            let body_para = &mut self.document.sections[section_idx].paragraphs[para_idx];
            reflow_line_segs(body_para, final_width, &self.styles, self.dpi);
        }

        // 리플로우 + 페이지네이션
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: para_idx,
        });
        Ok(format!(
            "{{\"ok\":true,\"paraIdx\":{},\"controlIdx\":{},\"footnoteNumber\":{}}}",
            para_idx, insert_idx, footnote_number
        ))
    }

    /// 미주를 삽입한다.
    /// 커서 위치에 미주 컨트롤을 추가하고 빈 미주 문단 1개를 생성한다.
    /// 반환: JSON `{"ok":true, "paraIdx":N, "controlIdx":N, "endnoteNumber":N}`
    pub fn insert_endnote_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
    ) -> Result<String, HwpError> {
        use crate::model::footnote::Endnote;

        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }

        let shape = self.document.sections[section_idx]
            .section_def
            .endnote_shape
            .clone();
        let start_number = shape.start_number.max(1);
        let number_format_code = Self::footnote_shape_number_format_code(shape.number_format);
        let endnote_number = {
            let mut count = 0u16;
            let section = &self.document.sections[section_idx];
            for (pi, para) in section.paragraphs.iter().enumerate() {
                let is_before = pi < para_idx;
                let is_same = pi == para_idx;
                for (ci, ctrl) in para.controls.iter().enumerate() {
                    match ctrl {
                        Control::Endnote(_) => {
                            if is_before {
                                count += 1;
                            } else if is_same {
                                let positions =
                                    crate::document_core::helpers::find_control_text_positions(
                                        para,
                                    );
                                let pos = positions.get(ci).copied().unwrap_or(usize::MAX);
                                if pos <= char_offset {
                                    count += 1;
                                }
                            }
                        }
                        Control::Table(table) if is_before || is_same => {
                            for cell in &table.cells {
                                for cp in &cell.paragraphs {
                                    count +=
                                        cp.controls
                                            .iter()
                                            .filter(|c| matches!(c, Control::Endnote(_)))
                                            .count() as u16;
                                }
                            }
                        }
                        Control::Shape(shape) if is_before || is_same => {
                            if let Some(text_box) =
                                shape.drawing().and_then(|d| d.text_box.as_ref())
                            {
                                for tp in &text_box.paragraphs {
                                    count +=
                                        tp.controls
                                            .iter()
                                            .filter(|c| matches!(c, Control::Endnote(_)))
                                            .count() as u16;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            start_number.saturating_add(count)
        };

        let (default_char_shape_id, para_shape_id, style_id) =
            self.endnote_style_defaults(section_idx, para_idx);
        let prefix_char = if shape.prefix_char == '\0' {
            '\0'
        } else {
            shape.prefix_char
        };
        let suffix_char = if shape.suffix_char == '\0' {
            ')'
        } else {
            shape.suffix_char
        };

        let inner_para = Self::make_note_inner_paragraph(
            crate::model::control::AutoNumberType::Endnote,
            endnote_number,
            number_format_code,
            prefix_char,
            suffix_char,
            default_char_shape_id,
            para_shape_id,
            style_id,
        );

        let endnote = Endnote {
            number: endnote_number,
            paragraphs: vec![inner_para],
            before_decoration_letter: prefix_char as u16,
            after_decoration_letter: suffix_char as u16,
            number_shape: number_format_code as u32,
            ..Default::default()
        };

        self.document.sections[section_idx].raw_stream = None;
        let paragraph = &mut self.document.sections[section_idx].paragraphs[para_idx];

        let insert_idx = {
            let positions = crate::document_core::helpers::find_control_text_positions(paragraph);
            let mut idx = paragraph.controls.len();
            for (i, &pos) in positions.iter().enumerate() {
                if pos > char_offset {
                    idx = i;
                    break;
                }
            }
            idx
        };

        paragraph
            .controls
            .insert(insert_idx, Control::Endnote(Box::new(endnote)));
        paragraph.ctrl_data_records.insert(insert_idx, None);

        if !paragraph.char_offsets.is_empty() {
            let text_len = paragraph.text.chars().count();
            let safe_offset = char_offset.min(text_len);
            for co in paragraph.char_offsets[safe_offset..].iter_mut() {
                *co += 8;
            }
        }
        paragraph.char_count += 8;
        paragraph.control_mask |= 1u32 << 0x0011;
        paragraph.has_para_text = true;

        let mut next_number = start_number;
        Self::renumber_paragraph_endnotes_with_shape(
            &mut self.document.sections[section_idx].paragraphs,
            &mut next_number,
            number_format_code,
            prefix_char,
            suffix_char,
        );

        self.reflow_footnote_paragraph(section_idx, para_idx, insert_idx, 0);

        {
            use crate::renderer::composer::reflow_line_segs;
            use crate::renderer::hwpunit_to_px;
            let page_def = &self.document.sections[section_idx].section_def.page_def;
            let text_width =
                page_def.width as i32 - page_def.margin_left as i32 - page_def.margin_right as i32;
            let available_width = hwpunit_to_px(text_width, self.dpi);
            let para_style = self.styles.para_styles.get(
                self.document.sections[section_idx].paragraphs[para_idx].para_shape_id as usize,
            );
            let margin_left = para_style.map(|s| s.margin_left).unwrap_or(0.0);
            let margin_right = para_style.map(|s| s.margin_right).unwrap_or(0.0);
            let final_width = (available_width - margin_left - margin_right).max(0.0);
            let body_para = &mut self.document.sections[section_idx].paragraphs[para_idx];
            reflow_line_segs(body_para, final_width, &self.styles, self.dpi);
        }

        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: para_idx,
        });
        Ok(format!(
            "{{\"ok\":true,\"paraIdx\":{},\"controlIdx\":{},\"endnoteNumber\":{}}}",
            para_idx, insert_idx, endnote_number
        ))
    }

    /// 현재 구역의 미주 모양을 조회한다.
    pub fn get_endnote_shape_native(&self, section_idx: usize) -> Result<String, HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let shape = &section.section_def.endnote_shape;
        let separator_enabled = shape.separator_length != 0
            || shape.separator_line_type != 0
            || shape.separator_line_width != 0;
        let separator_color =
            crate::document_core::helpers::clipboard_color_to_css(shape.separator_color);

        Ok(format!(
            concat!(
                "{{\"ok\":true,",
                "\"numberFormat\":\"{}\",",
                "\"userChar\":\"{}\",",
                "\"prefixChar\":\"{}\",",
                "\"suffixChar\":\"{}\",",
                "\"startNumber\":{},",
                "\"separatorEnabled\":{},",
                "\"separatorLength\":{},",
                "\"separatorMarginTop\":{},",
                "\"separatorMarginBottom\":{},",
                "\"noteSpacing\":{},",
                "\"separatorLineType\":{},",
                "\"separatorLineWidth\":{},",
                "\"separatorColor\":\"{}\",",
                "\"numberCodeSuperscript\":{},",
                "\"printInlineAfterText\":{},",
                "\"numbering\":\"{}\",",
                "\"placement\":\"{}\"",
                "}}"
            ),
            Self::footnote_shape_number_format_name(shape.number_format),
            Self::json_escape_note_char(shape.user_char),
            Self::json_escape_note_char(shape.prefix_char),
            Self::json_escape_note_char(shape.suffix_char),
            shape.start_number,
            if separator_enabled { "true" } else { "false" },
            shape.separator_length,
            shape.separator_above_margin_hu(),
            shape.separator_below_margin_hu(),
            shape.between_notes_margin_hu(),
            shape.separator_line_type,
            shape.separator_line_width,
            separator_color,
            if shape.number_code_superscript {
                "true"
            } else {
                "false"
            },
            if shape.print_inline_after_text {
                "true"
            } else {
                "false"
            },
            Self::footnote_numbering_name(shape.numbering),
            Self::footnote_placement_name(shape.placement),
        ))
    }

    /// 현재 구역의 미주 모양을 적용한다.
    pub fn apply_endnote_shape_native(
        &mut self,
        section_idx: usize,
        props_json: &str,
    ) -> Result<String, HwpError> {
        let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let shape = &mut section.section_def.endnote_shape;
        shape.raw_hwpx_children = None;

        if let Some(v) = crate::document_core::helpers::json_str(props_json, "numberFormat") {
            shape.number_format =
                Self::footnote_shape_number_format_from_str(&v, shape.number_format);
        }
        if let Some(v) = crate::document_core::helpers::json_str(props_json, "userChar") {
            shape.user_char = Self::first_char_or_nul(&v);
        }
        if let Some(v) = crate::document_core::helpers::json_str(props_json, "prefixChar") {
            shape.prefix_char = Self::first_char_or_nul(&v);
        }
        if let Some(v) = crate::document_core::helpers::json_str(props_json, "suffixChar") {
            shape.suffix_char = Self::first_char_or_nul(&v);
        }
        if let Some(v) = crate::document_core::helpers::json_u16(props_json, "startNumber") {
            shape.start_number = v.max(1);
        }
        if let Some(v) = Self::hwpunit16_from_json(props_json, "separatorLength") {
            shape.separator_length = v.max(0);
        }
        if let Some(v) = Self::hwpunit16_from_json(props_json, "separatorMarginTop") {
            let above = v.max(0);
            // HWP5 저장본은 구분선 위 값을 fallback 슬롯에 보관하는 경우가 있어 함께 갱신한다.
            shape.separator_margin_top = above;
            shape.separator_margin_bottom = above;
        }
        if let Some(v) = Self::hwpunit16_from_json(props_json, "separatorMarginBottom") {
            shape.note_spacing = v.max(0);
        }
        if let Some(v) = Self::hwpunit16_from_json(props_json, "noteSpacing") {
            shape.raw_unknown = v.max(0) as u16;
        }
        if let Some(v) = crate::document_core::helpers::json_u8(props_json, "separatorLineType") {
            shape.separator_line_type = v;
        }
        if let Some(v) = crate::document_core::helpers::json_u8(props_json, "separatorLineWidth") {
            shape.separator_line_width = v;
        }
        if let Some(v) = crate::document_core::helpers::json_color(props_json, "separatorColor") {
            shape.separator_color = v;
        }
        if let Some(v) = crate::document_core::helpers::json_str(props_json, "numbering") {
            shape.numbering = Self::footnote_numbering_from_str(&v, shape.numbering);
        }
        if let Some(v) = crate::document_core::helpers::json_str(props_json, "placement") {
            shape.placement = Self::footnote_placement_from_str(&v, shape.placement);
        }
        if let Some(v) =
            crate::document_core::helpers::json_bool(props_json, "numberCodeSuperscript")
        {
            shape.number_code_superscript = v;
        }
        if let Some(v) =
            crate::document_core::helpers::json_bool(props_json, "printInlineAfterText")
        {
            shape.print_inline_after_text = v;
        }
        if let Some(false) =
            crate::document_core::helpers::json_bool(props_json, "separatorEnabled")
        {
            shape.separator_length = 0;
            shape.separator_line_type = 0;
            shape.separator_line_width = 0;
        }
        shape.attr = Self::encode_footnote_shape_attr(shape);
        let start_number = shape.start_number.max(1);
        let number_format_code = Self::footnote_shape_number_format_code(shape.number_format);
        let prefix_char = shape.prefix_char;
        let suffix_char = shape.suffix_char;
        let mut next_number = start_number;
        Self::renumber_paragraph_endnotes_with_shape(
            &mut section.paragraphs,
            &mut next_number,
            number_format_code,
            prefix_char,
            suffix_char,
        );
        section.raw_stream = None;

        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        Ok(super::super::helpers::json_ok())
    }

    fn equation_control(script: &str, font_size: u32, color: u32) -> Control {
        use crate::model::control::Equation;
        use crate::model::shape::CommonObjAttr;
        use crate::parser::tags::CTRL_EQUATION;

        let equation = Equation {
            common: CommonObjAttr {
                ctrl_id: CTRL_EQUATION,
                treat_as_char: true,
                width: 0,
                height: 0,
                ..Default::default()
            },
            script: script.to_string(),
            font_size,
            color,
            font_name: "HYhwpEQ".to_string(),
            ..Default::default()
        };
        Control::Equation(Box::new(equation))
    }

    fn insert_equation_control_in_paragraph(
        paragraph: &mut Paragraph,
        char_offset: usize,
        equation: Control,
    ) -> usize {
        let insert_idx = {
            let positions = crate::document_core::helpers::find_control_text_positions(paragraph);
            let mut idx = paragraph.controls.len();
            for (i, &pos) in positions.iter().enumerate() {
                if pos > char_offset {
                    idx = i;
                    break;
                }
            }
            idx
        };

        paragraph.controls.insert(insert_idx, equation);
        paragraph.ctrl_data_records.insert(insert_idx, None);

        if !paragraph.char_offsets.is_empty() {
            let text_len = paragraph.text.chars().count();
            let safe_offset = char_offset.min(text_len);
            for co in paragraph.char_offsets[safe_offset..].iter_mut() {
                *co += 8;
            }
        }
        paragraph.char_count += 8;
        paragraph.control_mask |= 1u32 << 11;
        paragraph.has_para_text = true;
        insert_idx
    }

    /// 본문 문단에 수식을 삽입한다.
    /// 커서 위치에 수식 컨트롤을 추가한다.
    /// 반환: JSON `{"ok":true, "paraIdx":N, "controlIdx":N}`
    pub fn insert_equation_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
        script: &str,
        font_size: u32,
        color: u32,
    ) -> Result<String, HwpError> {
        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }

        self.document.sections[section_idx].raw_stream = None;
        let equation = Self::equation_control(script, font_size, color);
        let paragraph = &mut self.document.sections[section_idx].paragraphs[para_idx];
        let insert_idx =
            Self::insert_equation_control_in_paragraph(paragraph, char_offset, equation);

        // 본문 문단 리플로우
        {
            use crate::renderer::composer::reflow_line_segs;
            use crate::renderer::hwpunit_to_px;
            let page_def = &self.document.sections[section_idx].section_def.page_def;
            let text_width =
                page_def.width as i32 - page_def.margin_left as i32 - page_def.margin_right as i32;
            let available_width = hwpunit_to_px(text_width, self.dpi);
            let para_style = self.styles.para_styles.get(
                self.document.sections[section_idx].paragraphs[para_idx].para_shape_id as usize,
            );
            let margin_left = para_style.map(|s| s.margin_left).unwrap_or(0.0);
            let margin_right = para_style.map(|s| s.margin_right).unwrap_or(0.0);
            let final_width = (available_width - margin_left - margin_right).max(0.0);
            let body_para = &mut self.document.sections[section_idx].paragraphs[para_idx];
            reflow_line_segs(body_para, final_width, &self.styles, self.dpi);
        }

        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        self.event_log.push(DocumentEvent::PictureInserted {
            section: section_idx,
            para: para_idx,
        });
        Ok(format!(
            "{{\"ok\":true,\"paraIdx\":{},\"controlIdx\":{}}}",
            para_idx, insert_idx
        ))
    }

    /// 표 셀/글상자 경로 내부 문단에 수식을 삽입한다.
    pub fn insert_equation_in_cell_by_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path: &[(usize, usize, usize)],
        char_offset: usize,
        script: &str,
        font_size: u32,
        color: u32,
    ) -> Result<String, HwpError> {
        if cell_path.is_empty() {
            return Err(HwpError::RenderError(
                "cell_path 가 비어있습니다".to_string(),
            ));
        }

        let outer_ctrl = cell_path[0].0;
        let outer_cell = cell_path[0].1;
        let cell_para_idx = cell_path.last().map(|entry| entry.2).unwrap_or(0);
        let equation = Self::equation_control(script, font_size, color);
        let insert_idx = {
            let cell_para =
                self.get_cell_paragraph_mut_by_path(section_idx, parent_para_idx, cell_path)?;
            Self::insert_equation_control_in_paragraph(cell_para, char_offset, equation)
        };

        self.mark_cell_control_dirty(section_idx, parent_para_idx, outer_ctrl);
        self.document.sections[section_idx].raw_stream = None;
        self.mark_section_dirty(section_idx);
        self.paginate_if_needed();

        self.event_log.push(DocumentEvent::CellTextChanged {
            section: section_idx,
            para: parent_para_idx,
            ctrl: outer_ctrl,
            cell: outer_cell,
        });
        Ok(super::super::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{},\"cellParaIdx\":{}",
            parent_para_idx, insert_idx, cell_para_idx
        )))
    }

    /// 직접 표 셀 문단에 수식을 삽입한다.
    pub fn insert_equation_in_cell_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        control_idx: usize,
        cell_idx: usize,
        cell_para_idx: usize,
        char_offset: usize,
        script: &str,
        font_size: u32,
        color: u32,
    ) -> Result<String, HwpError> {
        let cell_path = [(control_idx, cell_idx, cell_para_idx)];
        let result = self.insert_equation_in_cell_by_path_native(
            section_idx,
            parent_para_idx,
            &cell_path,
            char_offset,
            script,
            font_size,
            color,
        )?;
        self.reflow_cell_paragraph(
            section_idx,
            parent_para_idx,
            control_idx,
            cell_idx,
            cell_para_idx,
        );
        Ok(result)
    }
}

#[cfg(test)]
mod resize_clamp_tests {
    use super::*;
    use crate::model::document::{Document, Section, SectionDef};
    use crate::model::page::PageDef;

    fn make_test_core() -> DocumentCore {
        let mut doc = Document::default();
        doc.sections.push(Section {
            section_def: SectionDef {
                page_def: PageDef {
                    width: 59528,
                    height: 84188,
                    margin_left: 8504,
                    margin_right: 8504,
                    margin_top: 5668,
                    margin_bottom: 4252,
                    margin_header: 4252,
                    margin_footer: 4252,
                    ..Default::default()
                },
                ..Default::default()
            },
            paragraphs: vec![Paragraph::default()],
            raw_stream: None,
        });
        let mut core = DocumentCore::new_empty();
        // set_document이 composed/styles/pagination 벡터를 일관되게 초기화한다.
        core.set_document(doc);
        core
    }

    fn create_rectangle(core: &mut DocumentCore) -> (usize, usize) {
        let res = core
            .create_shape_control_native(
                0,
                0,
                0,
                9000,
                6750,
                0,
                0,
                false,
                "InFrontOfText",
                "rectangle",
                false,
                false,
                &[],
            )
            .expect("create rectangle");
        let para_idx = res
            .split("\"paraIdx\":")
            .nth(1)
            .and_then(|s| s.split(',').next())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let ctrl_idx = res
            .split("\"controlIdx\":")
            .nth(1)
            .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        (para_idx, ctrl_idx)
    }

    fn shape_common<'a>(
        core: &'a DocumentCore,
        para: usize,
        ctrl: usize,
    ) -> &'a crate::model::shape::CommonObjAttr {
        let c = &core.document.sections[0].paragraphs[para].controls[ctrl];
        match c {
            Control::Shape(s) => s.common(),
            _ => panic!("expected shape"),
        }
    }

    /// 리사이즈 핸들을 반대편 너머로 잡아끌 때 studio가 width=0 을 보내도
    /// 도형 공통 크기는 MIN_SHAPE_SIZE 이상을 유지해야 한다.
    #[test]
    fn resize_to_zero_width_clamps_to_min() {
        let mut core = make_test_core();
        let (para, ctrl) = create_rectangle(&mut core);

        core.set_shape_properties_native(0, para, ctrl, r#"{"width":0,"height":0}"#)
            .expect("resize to 0");

        let common = shape_common(&core, para, ctrl);
        assert!(
            common.width >= MIN_SHAPE_SIZE,
            "width clamped: {}",
            common.width
        );
        assert!(
            common.height >= MIN_SHAPE_SIZE,
            "height clamped: {}",
            common.height
        );
    }

    /// Rectangle은 common.width/height 를 기반으로 x_coords/y_coords 를 재계산한다.
    /// 0으로 내려가면 [0,0,0,0]이 되어 화면에서 사라졌던 버그 방어.
    #[test]
    fn rectangle_coords_nonzero_after_shrink_to_zero() {
        let mut core = make_test_core();
        let (para, ctrl) = create_rectangle(&mut core);

        core.set_shape_properties_native(0, para, ctrl, r#"{"width":0,"height":0}"#)
            .expect("resize to 0");

        let ctrl_ref = &core.document.sections[0].paragraphs[para].controls[ctrl];
        if let Control::Shape(shape) = ctrl_ref {
            if let ShapeObject::Rectangle(rect) = shape.as_ref() {
                assert_ne!(rect.x_coords, [0, 0, 0, 0], "Rectangle x_coords collapsed");
                assert_ne!(rect.y_coords, [0, 0, 0, 0], "Rectangle y_coords collapsed");
            } else {
                panic!("expected Rectangle variant");
            }
        }
    }

    /// 반복된 0-resize 후에도 원상 복구 가능한 양의 크기로 리사이즈할 수 있어야 한다.
    /// (사용자 보고 시나리오: 핸들 여러 번 클릭 → 도형 소실 → 되돌리기 불가)
    #[test]
    fn repeated_zero_resize_does_not_corrupt_state() {
        let mut core = make_test_core();
        let (para, ctrl) = create_rectangle(&mut core);

        for _ in 0..5 {
            core.set_shape_properties_native(0, para, ctrl, r#"{"width":0,"height":0}"#)
                .expect("repeated resize");
        }
        core.set_shape_properties_native(0, para, ctrl, r#"{"width":12000,"height":8000}"#)
            .expect("restore");

        let common = shape_common(&core, para, ctrl);
        assert_eq!(common.width, 12000);
        assert_eq!(common.height, 8000);
    }
}

impl crate::document_core::DocumentCore {
    /// 숨은 설명(hidden comment)을 본문 문단에 삽입한다.
    ///
    /// 커서 위치에 HiddenComment 컨트롤을 추가하고 숨은 설명 본문 문단 1개를 생성한다.
    /// 반환: JSON `{"ok":true,"paraIdx":N,"controlIdx":N}`
    pub fn insert_hidden_comment_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
        text: &str,
    ) -> Result<String, crate::error::HwpError> {
        use crate::error::HwpError;
        use crate::model::control::{Control, HiddenComment};
        use crate::model::paragraph::{CharShapeRef, Paragraph};

        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }

        let mut comment_para = Paragraph::new_empty();
        comment_para.has_para_text = true;
        if !text.is_empty() {
            comment_para.char_shapes.push(CharShapeRef {
                start_pos: 0,
                char_shape_id: 0,
            });
            comment_para.insert_text_at(0, text);
        }

        let comment = HiddenComment {
            paragraphs: vec![comment_para],
        };

        self.document.sections[section_idx].raw_stream = None;
        let paragraph = &mut self.document.sections[section_idx].paragraphs[para_idx];
        let insert_idx = {
            let positions = crate::document_core::helpers::find_control_text_positions(paragraph);
            let mut idx = paragraph.controls.len();
            for (i, &pos) in positions.iter().enumerate() {
                if pos > char_offset {
                    idx = i;
                    break;
                }
            }
            idx
        };

        paragraph
            .controls
            .insert(insert_idx, Control::HiddenComment(Box::new(comment)));
        if paragraph.ctrl_data_records.len() < insert_idx {
            paragraph.ctrl_data_records.resize(insert_idx, None);
        }
        paragraph.ctrl_data_records.insert(insert_idx, None);

        if !paragraph.char_offsets.is_empty() {
            let text_len = paragraph.text.chars().count();
            let safe_offset = char_offset.min(text_len);
            for offset in paragraph.char_offsets[safe_offset..].iter_mut() {
                *offset += 8;
            }
        }
        paragraph.char_count += 8;
        paragraph.control_mask |= 1u32 << 0x000F;
        paragraph.has_para_text = true;

        self.reflow_paragraph(section_idx, para_idx);
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"paraIdx\":{},\"controlIdx\":{}",
            para_idx, insert_idx
        )))
    }

    /// 숨은 설명(hidden comment) 본문 문단 목록을 조회한다.
    pub fn get_hidden_comment_info_native(
        &self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
    ) -> Result<String, crate::error::HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let para = section
            .paragraphs
            .get(para_idx)
            .ok_or_else(|| HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", para_idx)))?;
        let comment = Self::hidden_comment_control_ref(para, control_idx)?;
        Ok(Self::format_hidden_comment_info(comment))
    }

    /// cell_path가 가리키는 표 셀/글상자 문단 안의 숨은 설명 본문 문단 목록을 조회한다.
    pub fn get_hidden_comment_info_by_cell_path_native(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
    ) -> Result<String, crate::error::HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        let para = self.resolve_paragraph_by_path(section_idx, parent_para_idx, &path)?;
        let comment = Self::hidden_comment_control_ref(para, inner_control_idx)?;
        Ok(Self::format_hidden_comment_info(comment))
    }

    fn format_hidden_comment_info(comment: &crate::model::control::HiddenComment) -> String {
        let texts: Vec<String> = comment
            .paragraphs
            .iter()
            .map(|p| {
                format!(
                    "\"{}\"",
                    crate::document_core::helpers::json_escape(&p.text)
                )
            })
            .collect();
        let total_len: usize = comment
            .paragraphs
            .iter()
            .map(|p| p.text.chars().count())
            .sum();
        format!(
            "{{\"ok\":true,\"paraCount\":{},\"totalTextLen\":{},\"texts\":[{}]}}",
            comment.paragraphs.len(),
            total_len,
            texts.join(",")
        )
    }

    /// 숨은 설명(hidden comment) 내부 문단에 텍스트를 삽입한다.
    pub fn insert_text_in_hidden_comment_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        hidden_para_idx: usize,
        char_offset: usize,
        text: &str,
    ) -> Result<String, crate::error::HwpError> {
        let new_chars_count = text.chars().count();
        {
            let hidden_para = self.get_hidden_comment_paragraph_mut(
                section_idx,
                para_idx,
                control_idx,
                hidden_para_idx,
            )?;
            hidden_para.insert_text_at(char_offset, text);
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::TextInserted {
            section: section_idx,
            para: para_idx,
            offset: char_offset,
            len: new_chars_count,
        });
        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"charOffset\":{}",
            char_offset + new_chars_count
        )))
    }

    /// cell_path가 가리키는 표 셀/글상자 문단 안의 숨은 설명 내부 문단에 텍스트를 삽입한다.
    pub fn insert_text_in_hidden_comment_by_cell_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
        hidden_para_idx: usize,
        char_offset: usize,
        text: &str,
    ) -> Result<String, crate::error::HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        let new_chars_count = text.chars().count();
        {
            let hidden_para = self.get_hidden_comment_paragraph_by_cell_path_mut(
                section_idx,
                parent_para_idx,
                &path,
                inner_control_idx,
                hidden_para_idx,
            )?;
            hidden_para.insert_text_at(char_offset, text);
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::TextInserted {
            section: section_idx,
            para: parent_para_idx,
            offset: char_offset,
            len: new_chars_count,
        });
        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"charOffset\":{}",
            char_offset + new_chars_count
        )))
    }

    /// 숨은 설명(hidden comment) 내부 문단에서 텍스트를 삭제한다.
    pub fn delete_text_in_hidden_comment_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        hidden_para_idx: usize,
        char_offset: usize,
        count: usize,
    ) -> Result<String, crate::error::HwpError> {
        {
            let hidden_para = self.get_hidden_comment_paragraph_mut(
                section_idx,
                para_idx,
                control_idx,
                hidden_para_idx,
            )?;
            hidden_para.delete_text_at(char_offset, count);
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::TextDeleted {
            section: section_idx,
            para: para_idx,
            offset: char_offset,
            count,
        });
        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"charOffset\":{}",
            char_offset
        )))
    }

    /// cell_path가 가리키는 표 셀/글상자 문단 안의 숨은 설명 내부 문단에서 텍스트를 삭제한다.
    pub fn delete_text_in_hidden_comment_by_cell_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
        hidden_para_idx: usize,
        char_offset: usize,
        count: usize,
    ) -> Result<String, crate::error::HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        {
            let hidden_para = self.get_hidden_comment_paragraph_by_cell_path_mut(
                section_idx,
                parent_para_idx,
                &path,
                inner_control_idx,
                hidden_para_idx,
            )?;
            hidden_para.delete_text_at(char_offset, count);
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::TextDeleted {
            section: section_idx,
            para: parent_para_idx,
            offset: char_offset,
            count,
        });
        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"charOffset\":{}",
            char_offset
        )))
    }

    /// 숨은 설명(hidden comment) 내부 문단을 분할한다.
    pub fn split_paragraph_in_hidden_comment_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        hidden_para_idx: usize,
        char_offset: usize,
    ) -> Result<String, crate::error::HwpError> {
        let new_para = {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let para = section.paragraphs.get_mut(para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", para_idx))
            })?;
            let ctrl = para.controls.get_mut(control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
            })?;
            let Control::HiddenComment(comment) = ctrl else {
                return Err(HwpError::RenderError(format!(
                    "컨트롤 {}은 숨은 설명이 아닙니다",
                    control_idx
                )));
            };
            if hidden_para_idx >= comment.paragraphs.len() {
                return Err(HwpError::RenderError(format!(
                    "숨은 설명 문단 인덱스 {} 범위 초과",
                    hidden_para_idx
                )));
            }
            comment.paragraphs[hidden_para_idx].split_at(char_offset)
        };

        let new_para_idx = hidden_para_idx + 1;
        {
            let ctrl =
                &mut self.document.sections[section_idx].paragraphs[para_idx].controls[control_idx];
            if let Control::HiddenComment(comment) = ctrl {
                comment.paragraphs.insert(new_para_idx, new_para);
            }
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::ParagraphSplit {
            section: section_idx,
            para: para_idx,
            offset: char_offset,
        });
        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"hiddenParaIndex\":{},\"charOffset\":0",
            new_para_idx
        )))
    }

    /// cell_path가 가리키는 표 셀/글상자 문단 안의 숨은 설명 내부 문단을 분할한다.
    pub fn split_paragraph_in_hidden_comment_by_cell_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
        hidden_para_idx: usize,
        char_offset: usize,
    ) -> Result<String, crate::error::HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        let new_para_idx = hidden_para_idx + 1;
        {
            let comment = self.get_hidden_comment_by_cell_path_mut(
                section_idx,
                parent_para_idx,
                &path,
                inner_control_idx,
            )?;
            if hidden_para_idx >= comment.paragraphs.len() {
                return Err(HwpError::RenderError(format!(
                    "숨은 설명 문단 인덱스 {} 범위 초과",
                    hidden_para_idx
                )));
            }
            let new_para = comment.paragraphs[hidden_para_idx].split_at(char_offset);
            comment.paragraphs.insert(new_para_idx, new_para);
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::ParagraphSplit {
            section: section_idx,
            para: parent_para_idx,
            offset: char_offset,
        });
        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"hiddenParaIndex\":{},\"charOffset\":0",
            new_para_idx
        )))
    }

    /// 숨은 설명(hidden comment) 내부 문단을 이전 문단과 병합한다.
    pub fn merge_paragraph_in_hidden_comment_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        hidden_para_idx: usize,
    ) -> Result<String, crate::error::HwpError> {
        if hidden_para_idx == 0 {
            return Err(HwpError::RenderError(
                "첫 번째 숨은 설명 문단은 이전 문단과 병합할 수 없습니다".to_string(),
            ));
        }

        let merge_offset = {
            let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
                HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
            })?;
            let para = section.paragraphs.get_mut(para_idx).ok_or_else(|| {
                HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", para_idx))
            })?;
            let ctrl = para.controls.get_mut(control_idx).ok_or_else(|| {
                HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
            })?;
            let Control::HiddenComment(comment) = ctrl else {
                return Err(HwpError::RenderError(format!(
                    "컨트롤 {}은 숨은 설명이 아닙니다",
                    control_idx
                )));
            };
            if hidden_para_idx >= comment.paragraphs.len() {
                return Err(HwpError::RenderError(format!(
                    "숨은 설명 문단 인덱스 {} 범위 초과",
                    hidden_para_idx
                )));
            }
            let merge_offset = comment.paragraphs[hidden_para_idx - 1].text.chars().count();
            let removed = comment.paragraphs.remove(hidden_para_idx);
            comment.paragraphs[hidden_para_idx - 1].merge_from(&removed);
            merge_offset
        };

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::ParagraphMerged {
            section: section_idx,
            para: para_idx,
        });
        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"hiddenParaIndex\":{},\"charOffset\":{}",
            hidden_para_idx - 1,
            merge_offset
        )))
    }

    /// cell_path가 가리키는 표 셀/글상자 문단 안의 숨은 설명 내부 문단을 이전 문단과 병합한다.
    pub fn merge_paragraph_in_hidden_comment_by_cell_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
        hidden_para_idx: usize,
    ) -> Result<String, crate::error::HwpError> {
        if hidden_para_idx == 0 {
            return Err(HwpError::RenderError(
                "첫 번째 숨은 설명 문단은 이전 문단과 병합할 수 없습니다".to_string(),
            ));
        }

        let path = Self::parse_cell_path_json(cell_path_json)?;
        let merge_offset = {
            let comment = self.get_hidden_comment_by_cell_path_mut(
                section_idx,
                parent_para_idx,
                &path,
                inner_control_idx,
            )?;
            if hidden_para_idx >= comment.paragraphs.len() {
                return Err(HwpError::RenderError(format!(
                    "숨은 설명 문단 인덱스 {} 범위 초과",
                    hidden_para_idx
                )));
            }
            let merge_offset = comment.paragraphs[hidden_para_idx - 1].text.chars().count();
            let removed = comment.paragraphs.remove(hidden_para_idx);
            comment.paragraphs[hidden_para_idx - 1].merge_from(&removed);
            merge_offset
        };

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::ParagraphMerged {
            section: section_idx,
            para: parent_para_idx,
        });
        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"hiddenParaIndex\":{},\"charOffset\":{}",
            hidden_para_idx - 1,
            merge_offset
        )))
    }

    /// 숨은 설명(hidden comment) 내부 문단에 글자 서식을 적용한다.
    pub fn apply_char_format_in_hidden_comment_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        hidden_para_idx: usize,
        start_offset: usize,
        end_offset: usize,
        props_json: &str,
    ) -> Result<String, crate::error::HwpError> {
        let mut mods = parse_char_shape_mods(props_json);
        if json_has_border_keys(props_json) {
            let bf_id = self.create_border_fill_from_json(props_json);
            mods.border_fill_id = Some(bf_id);
        }

        let base_id = self
            .get_hidden_comment_paragraph_ref(section_idx, para_idx, control_idx, hidden_para_idx)?
            .char_shape_id_at(start_offset)
            .unwrap_or(0);
        let new_id = self.document.find_or_create_char_shape(base_id, &mods);
        {
            let hidden_para = self.get_hidden_comment_paragraph_mut(
                section_idx,
                para_idx,
                control_idx,
                hidden_para_idx,
            )?;
            hidden_para.apply_char_shape_range(start_offset, end_offset, new_id);
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::CharFormatChanged {
            section: section_idx,
            para: para_idx,
            start: start_offset,
            end: end_offset,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// cell_path가 가리키는 표 셀/글상자 문단 안의 숨은 설명 내부 문단에 글자 서식을 적용한다.
    pub fn apply_char_format_in_hidden_comment_by_cell_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
        hidden_para_idx: usize,
        start_offset: usize,
        end_offset: usize,
        props_json: &str,
    ) -> Result<String, crate::error::HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        let mut mods = parse_char_shape_mods(props_json);
        if json_has_border_keys(props_json) {
            let bf_id = self.create_border_fill_from_json(props_json);
            mods.border_fill_id = Some(bf_id);
        }

        let base_id = self
            .get_hidden_comment_paragraph_by_cell_path_ref(
                section_idx,
                parent_para_idx,
                &path,
                inner_control_idx,
                hidden_para_idx,
            )?
            .char_shape_id_at(start_offset)
            .unwrap_or(0);
        let new_id = self.document.find_or_create_char_shape(base_id, &mods);
        {
            let hidden_para = self.get_hidden_comment_paragraph_by_cell_path_mut(
                section_idx,
                parent_para_idx,
                &path,
                inner_control_idx,
                hidden_para_idx,
            )?;
            hidden_para.apply_char_shape_range(start_offset, end_offset, new_id);
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::CharFormatChanged {
            section: section_idx,
            para: parent_para_idx,
            start: start_offset,
            end: end_offset,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// 숨은 설명(hidden comment) 내부 문단에 문단 서식을 적용한다.
    pub fn apply_para_format_in_hidden_comment_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        hidden_para_idx: usize,
        props_json: &str,
    ) -> Result<String, crate::error::HwpError> {
        let base_id = self
            .get_hidden_comment_paragraph_ref(section_idx, para_idx, control_idx, hidden_para_idx)?
            .para_shape_id;
        let mut mods = parse_para_shape_mods(props_json);

        if json_has_tab_keys(props_json) {
            let base_tab_def_id = self
                .document
                .doc_info
                .para_shapes
                .get(base_id as usize)
                .map(|ps| ps.tab_def_id)
                .unwrap_or(0);
            let new_td = build_tab_def_from_json(
                props_json,
                base_tab_def_id,
                &self.document.doc_info.tab_defs,
            );
            let new_tab_id = self.document.find_or_create_tab_def(new_td);
            mods.tab_def_id = Some(new_tab_id);
        }

        if json_has_border_keys(props_json) {
            let bf_id = self.create_border_fill_from_json(props_json);
            mods.border_fill_id = Some(bf_id);
        }
        if let Some(arr) = parse_json_i16_array(props_json, "borderSpacing", 4) {
            mods.border_spacing = Some([arr[0], arr[1], arr[2], arr[3]]);
        }

        let new_id = self.document.find_or_create_para_shape(base_id, &mods);
        {
            let hidden_para = self.get_hidden_comment_paragraph_mut(
                section_idx,
                para_idx,
                control_idx,
                hidden_para_idx,
            )?;
            hidden_para.para_shape_id = new_id;
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::ParaFormatChanged {
            section: section_idx,
            para: para_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    /// cell_path가 가리키는 표 셀/글상자 문단 안의 숨은 설명 내부 문단에 문단 서식을 적용한다.
    pub fn apply_para_format_in_hidden_comment_by_cell_path_native(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        cell_path_json: &str,
        inner_control_idx: usize,
        hidden_para_idx: usize,
        props_json: &str,
    ) -> Result<String, crate::error::HwpError> {
        let path = Self::parse_cell_path_json(cell_path_json)?;
        let base_id = self
            .get_hidden_comment_paragraph_by_cell_path_ref(
                section_idx,
                parent_para_idx,
                &path,
                inner_control_idx,
                hidden_para_idx,
            )?
            .para_shape_id;
        let mut mods = parse_para_shape_mods(props_json);

        if json_has_tab_keys(props_json) {
            let base_tab_def_id = self
                .document
                .doc_info
                .para_shapes
                .get(base_id as usize)
                .map(|ps| ps.tab_def_id)
                .unwrap_or(0);
            let new_td = build_tab_def_from_json(
                props_json,
                base_tab_def_id,
                &self.document.doc_info.tab_defs,
            );
            let new_tab_id = self.document.find_or_create_tab_def(new_td);
            mods.tab_def_id = Some(new_tab_id);
        }

        if json_has_border_keys(props_json) {
            let bf_id = self.create_border_fill_from_json(props_json);
            mods.border_fill_id = Some(bf_id);
        }
        if let Some(arr) = parse_json_i16_array(props_json, "borderSpacing", 4) {
            mods.border_spacing = Some([arr[0], arr[1], arr[2], arr[3]]);
        }

        let new_id = self.document.find_or_create_para_shape(base_id, &mods);
        {
            let hidden_para = self.get_hidden_comment_paragraph_by_cell_path_mut(
                section_idx,
                parent_para_idx,
                &path,
                inner_control_idx,
                hidden_para_idx,
            )?;
            hidden_para.para_shape_id = new_id;
        }

        self.document.sections[section_idx].raw_stream = None;
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();
        self.event_log.push(DocumentEvent::ParaFormatChanged {
            section: section_idx,
            para: parent_para_idx,
        });
        Ok("{\"ok\":true}".to_string())
    }

    fn hidden_comment_control_ref(
        para: &Paragraph,
        control_idx: usize,
    ) -> Result<&crate::model::control::HiddenComment, crate::error::HwpError> {
        let ctrl = para.controls.get(control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
        })?;
        let Control::HiddenComment(comment) = ctrl else {
            return Err(HwpError::RenderError(format!(
                "컨트롤 {}은 숨은 설명이 아닙니다",
                control_idx
            )));
        };
        Ok(comment.as_ref())
    }

    fn hidden_comment_control_mut(
        para: &mut Paragraph,
        control_idx: usize,
    ) -> Result<&mut crate::model::control::HiddenComment, crate::error::HwpError> {
        let ctrl = para.controls.get_mut(control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
        })?;
        let Control::HiddenComment(comment) = ctrl else {
            return Err(HwpError::RenderError(format!(
                "컨트롤 {}은 숨은 설명이 아닙니다",
                control_idx
            )));
        };
        Ok(comment.as_mut())
    }

    fn get_hidden_comment_by_cell_path_mut(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        path: &[(usize, usize, usize)],
        inner_control_idx: usize,
    ) -> Result<&mut crate::model::control::HiddenComment, crate::error::HwpError> {
        let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let para = Self::resolve_cell_paragraph_mut(section, parent_para_idx, path)?;
        Self::hidden_comment_control_mut(para, inner_control_idx)
    }

    fn get_hidden_comment_paragraph_by_cell_path_mut(
        &mut self,
        section_idx: usize,
        parent_para_idx: usize,
        path: &[(usize, usize, usize)],
        inner_control_idx: usize,
        hidden_para_idx: usize,
    ) -> Result<&mut Paragraph, crate::error::HwpError> {
        let comment = self.get_hidden_comment_by_cell_path_mut(
            section_idx,
            parent_para_idx,
            path,
            inner_control_idx,
        )?;
        comment.paragraphs.get_mut(hidden_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!(
                "숨은 설명 문단 인덱스 {} 범위 초과",
                hidden_para_idx
            ))
        })
    }

    fn get_hidden_comment_paragraph_by_cell_path_ref(
        &self,
        section_idx: usize,
        parent_para_idx: usize,
        path: &[(usize, usize, usize)],
        inner_control_idx: usize,
        hidden_para_idx: usize,
    ) -> Result<&Paragraph, crate::error::HwpError> {
        let para = self.resolve_paragraph_by_path(section_idx, parent_para_idx, path)?;
        let comment = Self::hidden_comment_control_ref(para, inner_control_idx)?;
        comment.paragraphs.get(hidden_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!(
                "숨은 설명 문단 인덱스 {} 범위 초과",
                hidden_para_idx
            ))
        })
    }

    fn get_hidden_comment_paragraph_mut(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        hidden_para_idx: usize,
    ) -> Result<&mut Paragraph, crate::error::HwpError> {
        let section = self.document.sections.get_mut(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let para = section
            .paragraphs
            .get_mut(para_idx)
            .ok_or_else(|| HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", para_idx)))?;
        let ctrl = para.controls.get_mut(control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
        })?;
        let Control::HiddenComment(comment) = ctrl else {
            return Err(HwpError::RenderError(format!(
                "컨트롤 {}은 숨은 설명이 아닙니다",
                control_idx
            )));
        };
        comment.paragraphs.get_mut(hidden_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!(
                "숨은 설명 문단 인덱스 {} 범위 초과",
                hidden_para_idx
            ))
        })
    }

    fn get_hidden_comment_paragraph_ref(
        &self,
        section_idx: usize,
        para_idx: usize,
        control_idx: usize,
        hidden_para_idx: usize,
    ) -> Result<&Paragraph, crate::error::HwpError> {
        let section = self.document.sections.get(section_idx).ok_or_else(|| {
            HwpError::RenderError(format!("구역 인덱스 {} 범위 초과", section_idx))
        })?;
        let para = section
            .paragraphs
            .get(para_idx)
            .ok_or_else(|| HwpError::RenderError(format!("문단 인덱스 {} 범위 초과", para_idx)))?;
        let ctrl = para.controls.get(control_idx).ok_or_else(|| {
            HwpError::RenderError(format!("컨트롤 인덱스 {} 범위 초과", control_idx))
        })?;
        let Control::HiddenComment(comment) = ctrl else {
            return Err(HwpError::RenderError(format!(
                "컨트롤 {}은 숨은 설명이 아닙니다",
                control_idx
            )));
        };
        comment.paragraphs.get(hidden_para_idx).ok_or_else(|| {
            HwpError::RenderError(format!(
                "숨은 설명 문단 인덱스 {} 범위 초과",
                hidden_para_idx
            ))
        })
    }

    pub fn insert_new_number_native(
        &mut self,
        section_idx: usize,
        para_idx: usize,
        char_offset: usize,
        start_num: u16,
    ) -> Result<String, crate::error::HwpError> {
        use crate::error::HwpError;
        use crate::model::control::{AutoNumberType, Control, NewNumber};

        if section_idx >= self.document.sections.len() {
            return Err(HwpError::RenderError(format!(
                "구역 인덱스 {} 범위 초과",
                section_idx
            )));
        }
        if para_idx >= self.document.sections[section_idx].paragraphs.len() {
            return Err(HwpError::RenderError(format!(
                "문단 인덱스 {} 범위 초과",
                para_idx
            )));
        }

        let new_number = NewNumber {
            number_type: AutoNumberType::Page,
            number: start_num,
        };

        self.document.sections[section_idx].raw_stream = None;
        let paragraph = &mut self.document.sections[section_idx].paragraphs[para_idx];

        let insert_idx = {
            let positions = crate::document_core::helpers::find_control_text_positions(paragraph);
            let mut idx = paragraph.controls.len();
            for (i, &pos) in positions.iter().enumerate() {
                if pos > char_offset {
                    idx = i;
                    break;
                }
            }
            idx
        };

        paragraph
            .controls
            .insert(insert_idx, Control::NewNumber(new_number));
        paragraph.ctrl_data_records.insert(insert_idx, None);

        if !paragraph.char_offsets.is_empty() {
            let text_len = paragraph.text.chars().count();
            let safe_offset = char_offset.min(text_len);
            for co in paragraph.char_offsets[safe_offset..].iter_mut() {
                *co += 8;
            }
        }
        paragraph.char_count += 8;
        paragraph.control_mask |= 1u32 << 0x0012;
        paragraph.has_para_text = true;

        self.reflow_paragraph(section_idx, para_idx);
        self.recompose_section(section_idx);
        self.paginate_if_needed();
        self.invalidate_page_tree_cache();

        Ok(crate::document_core::helpers::json_ok_with(&format!(
            "\"controlIdx\":{}",
            insert_idx
        )))
    }
}

#[cfg(test)]
mod issue_1151_cell_picture_insert_tests {
    //! Issue #1151: 표 셀 안 이미지 삽입이 항상 표 밖 본문 문단에 들어가는 결함.
    //!
    //! v2 설계 — 한컴 정합 floating picture 접근:
    //! 셀 안 삽입 (cell_path 비어있지 않음) 시 picture 는 셀 내부 paragraph 에
    //! inline 삽입되지 않고, 표가 있는 같은 paragraph 의 sibling control 로
    //! floating (tac=false) 삽입된다. 셀 자체는 비어있는 채로 유지되어 사용자가
    //! 클릭으로 cursor 를 셀에 위치시킬 수 있다.

    use super::*;
    use crate::model::document::{Document, Section, SectionDef};
    use crate::model::page::PageDef;

    fn make_test_core() -> DocumentCore {
        let mut doc = Document::default();
        doc.sections.push(Section {
            section_def: SectionDef {
                page_def: PageDef {
                    width: 59528,
                    height: 84188,
                    margin_left: 8504,
                    margin_right: 8504,
                    margin_top: 5668,
                    margin_bottom: 4252,
                    margin_header: 4252,
                    margin_footer: 4252,
                    ..Default::default()
                },
                ..Default::default()
            },
            paragraphs: vec![Paragraph::default()],
            raw_stream: None,
        });
        let mut core = DocumentCore::new_empty();
        core.set_document(doc);
        core
    }

    fn minimal_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x00, 0x00, 0x00,
            0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    fn collect_picture_transparencies(doc: &Document) -> Vec<u8> {
        let mut values = Vec::new();
        for section in &doc.sections {
            collect_picture_transparencies_from_paragraphs(&section.paragraphs, &mut values);
        }
        values
    }

    fn collect_picture_transparencies_from_paragraphs(
        paragraphs: &[Paragraph],
        values: &mut Vec<u8>,
    ) {
        for para in paragraphs {
            for control in &para.controls {
                collect_picture_transparencies_from_control(control, values);
            }
        }
    }

    fn collect_picture_transparencies_from_control(control: &Control, values: &mut Vec<u8>) {
        match control {
            Control::Picture(pic) => {
                values.push(pic.image_attr.clamped_transparency());
                if let Some(caption) = &pic.caption {
                    collect_picture_transparencies_from_paragraphs(&caption.paragraphs, values);
                }
            }
            Control::Table(table) => {
                for cell in &table.cells {
                    collect_picture_transparencies_from_paragraphs(&cell.paragraphs, values);
                }
            }
            Control::Shape(shape) => collect_picture_transparencies_from_shape(shape, values),
            Control::Header(header) => {
                collect_picture_transparencies_from_paragraphs(&header.paragraphs, values);
            }
            Control::Footer(footer) => {
                collect_picture_transparencies_from_paragraphs(&footer.paragraphs, values);
            }
            Control::Footnote(footnote) => {
                collect_picture_transparencies_from_paragraphs(&footnote.paragraphs, values);
            }
            Control::Endnote(endnote) => {
                collect_picture_transparencies_from_paragraphs(&endnote.paragraphs, values);
            }
            _ => {}
        }
    }

    fn collect_picture_transparencies_from_shape(
        shape: &crate::model::shape::ShapeObject,
        values: &mut Vec<u8>,
    ) {
        match shape {
            crate::model::shape::ShapeObject::Picture(pic) => {
                values.push(pic.image_attr.clamped_transparency());
                if let Some(caption) = &pic.caption {
                    collect_picture_transparencies_from_paragraphs(&caption.paragraphs, values);
                }
            }
            crate::model::shape::ShapeObject::Group(group) => {
                for child in &group.children {
                    collect_picture_transparencies_from_shape(child, values);
                }
                if let Some(caption) = &group.caption {
                    collect_picture_transparencies_from_paragraphs(&caption.paragraphs, values);
                }
            }
            _ => {
                if let Some(drawing) = shape.drawing() {
                    if let Some(text_box) = &drawing.text_box {
                        collect_picture_transparencies_from_paragraphs(
                            &text_box.paragraphs,
                            values,
                        );
                    }
                    if let Some(caption) = &drawing.caption {
                        collect_picture_transparencies_from_paragraphs(&caption.paragraphs, values);
                    }
                }
            }
        }
    }

    fn parse_idx(res: &str, key: &str) -> usize {
        res.split(&format!("\"{}\":", key))
            .nth(1)
            .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("missing {key} in {res}"))
    }

    #[test]
    fn issue1151_insert_picture_into_table_cell_is_floating_sibling() {
        let mut core = make_test_core();

        // 1×1 표 생성
        let table_res = core
            .create_table_native(0, 0, 0, 1, 1)
            .expect("create 1x1 table");
        let table_para_idx = parse_idx(&table_res, "paraIdx");
        let table_ctrl_idx = parse_idx(&table_res, "controlIdx");

        let cell_path: Vec<(usize, usize, usize)> = vec![(table_ctrl_idx, 0, 0)];
        let image = minimal_png();
        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            5000,
            5000,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert picture (floating)");

        // 셀 안은 그대로 비어있어야 한다 (floating 은 셀에 들어가지 않음).
        let table_ctrl =
            &core.document.sections[0].paragraphs[table_para_idx].controls[table_ctrl_idx];
        let table = match table_ctrl {
            Control::Table(t) => t,
            _ => panic!("expected Control::Table"),
        };
        let cell0_para0 = &table.cells[0].paragraphs[0];
        assert!(
            cell0_para0
                .controls
                .iter()
                .all(|c| !matches!(c, Control::Picture(_))),
            "cell 안에 picture 가 들어가면 안 된다 (floating 방식). got: {:?}",
            cell0_para0.controls
        );

        // table 같은 paragraph 의 sibling control 로 Picture 가 존재해야 한다.
        let parent_para = &core.document.sections[0].paragraphs[table_para_idx];
        let picture = parent_para
            .controls
            .iter()
            .find_map(|c| match c {
                Control::Picture(p) => Some(p.as_ref()),
                _ => None,
            })
            .expect("expected sibling Picture in parent paragraph");

        // floating 속성 검증
        assert!(
            !picture.common.treat_as_char,
            "floating picture 는 treat_as_char=false 여야 한다"
        );
        assert!(
            matches!(
                picture.common.text_wrap,
                crate::model::shape::TextWrap::Square
            ),
            "floating picture wrap=Square (어울림) 이어야 한다. got: {:?}",
            picture.common.text_wrap
        );
    }

    #[test]
    fn issue1151_v9_insert_picture_body_floating_default() {
        // [Task #1151 v9 결함 E] 한컴 native 정합: 본문 picture 신규 삽입 시 default =
        // tac=false (floating, 글자처럼 미체크). 셀 분기와 동일 패턴.
        let mut core = make_test_core();
        let image = minimal_png();
        core.insert_picture_native(
            0,
            0,
            0,
            &[], // 빈 cell_path → 본문 floating (v9 fix 후)
            &image,
            5000,
            5000,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert picture body");

        let body_para = &core.document.sections[0].paragraphs[0];
        let pic_in_body = body_para.controls.iter().find_map(|c| match c {
            Control::Picture(p) => Some(p.as_ref()),
            _ => None,
        });
        let picture = pic_in_body.expect("expected Picture in body paragraph (sibling control)");

        // 한컴 native 정합: tac=false, rel_to=Paper, wrap=Square
        assert!(
            !picture.common.treat_as_char,
            "본문 picture default = tac=false (한컴 native 정합, v9 결함 E fix)"
        );
        assert!(
            matches!(
                picture.common.horz_rel_to,
                crate::model::shape::HorzRelTo::Paper
            ),
            "본문 picture horz_rel_to = Paper (셀 분기와 동일)"
        );
        assert!(
            matches!(
                picture.common.vert_rel_to,
                crate::model::shape::VertRelTo::Paper
            ),
            "본문 picture vert_rel_to = Paper"
        );
        assert!(matches!(
            picture.common.text_wrap,
            crate::model::shape::TextWrap::Square
        ));

        // 새 paragraph 생성 안 함 — 기존 paragraph 의 sibling control 로 append
        assert_eq!(
            core.document.sections[0].paragraphs.len(),
            1,
            "본문 picture 삽입 시 새 paragraph 생성 안 함 (sibling control)"
        );
    }

    #[test]
    fn issue1452_insert_picture_returns_logical_offset_after_picture() {
        let mut core = make_test_core();
        core.insert_text_native(0, 0, 0, "abc")
            .expect("insert text");

        let image = minimal_png();
        let result = core
            .insert_picture_native(
                0,
                0,
                3,
                &[],
                &image,
                5000,
                5000,
                1,
                1,
                "png",
                "test",
                None,
                None,
            )
            .expect("insert picture body");

        assert_eq!(parse_idx(&result, "paraIdx"), 0);
        assert_eq!(parse_idx(&result, "controlIdx"), 0);
        assert_eq!(
            parse_idx(&result, "logicalOffset"),
            4,
            "본문 텍스트 'abc' 뒤에 그림 1개를 넣으면 그림 뒤 커서 offset은 4여야 한다: {result}"
        );
    }

    #[test]
    fn issue1452_enter_after_dropped_inline_picture_keeps_next_para_below_picture() {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};

        fn collect_image_bboxes(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Image(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for child in &node.children {
                collect_image_bboxes(child, out);
            }
        }

        fn collect_para_end_runs(
            node: &RenderNode,
            out: &mut Vec<(usize, Option<usize>, f64, f64, f64, f64)>,
        ) {
            if let RenderNodeType::TextRun(run) = &node.node_type {
                if run.is_para_end {
                    if let Some(para_idx) = run.para_index {
                        out.push((
                            para_idx,
                            run.char_start,
                            node.bbox.x,
                            node.bbox.y,
                            node.bbox.width,
                            node.bbox.height,
                        ));
                    }
                }
            }
            for child in &node.children {
                collect_para_end_runs(child, out);
            }
        }

        let mut core = make_test_core();
        let image = minimal_png();
        let pic_w = 30000u32;
        let pic_h = 9000u32;

        let result = core
            .insert_picture_native(
                0,
                0,
                0,
                &[],
                &image,
                pic_w,
                pic_h,
                1,
                1,
                "png",
                "drop",
                None,
                None,
            )
            .expect("insert dropped picture");
        let ctrl_idx = parse_idx(&result, "controlIdx");
        let logical_offset = parse_idx(&result, "logicalOffset");

        core.set_picture_properties_native(0, 0, ctrl_idx, r#"{"treatAsChar":true}"#)
            .expect("dropped picture becomes treat-as-char");
        core.split_paragraph_native(0, 0, logical_offset)
            .expect("Enter after dropped picture");

        assert_eq!(
            core.document.sections[0].paragraphs.len(),
            2,
            "그림 뒤 Enter 는 새 빈 문단을 만들어야 한다"
        );
        assert_eq!(
            core.document.sections[0].paragraphs[0].line_segs[0].line_height, pic_h as i32,
            "TAC 그림만 남은 첫 문단은 그림 높이를 줄 높이로 유지해야 한다"
        );
        assert!(
            core.document.sections[0].paragraphs[1].line_segs[0].line_height < pic_h as i32 / 2,
            "새 빈 문단은 그림 높이를 물려받지 않고 기본 줄 높이로 시작해야 한다"
        );

        let tree = core.build_page_tree(0).expect("build page tree");
        let mut images = Vec::new();
        collect_image_bboxes(&tree.root, &mut images);
        assert_eq!(images.len(), 1, "drop 그림 ImageNode 1개 필요");

        let mut para_ends = Vec::new();
        collect_para_end_runs(&tree.root, &mut para_ends);
        let image = images[0];
        let image_right = image.0 + image.2;
        let image_bottom = image.1 + image.3;
        let para0_end = para_ends
            .iter()
            .find(|(para_idx, _, _, _, _, _)| *para_idx == 0)
            .expect("첫 문단 끝 표시");
        let para1_end = para_ends
            .iter()
            .find(|(para_idx, _, _, _, _, _)| *para_idx == 1)
            .expect("새 빈 문단 끝 표시");

        assert_eq!(
            para0_end.1,
            Some(logical_offset),
            "첫 문단 끝 표시는 그림 뒤 logical offset에 놓여야 한다"
        );
        assert!(
            para0_end.2 >= image_right - 0.5,
            "첫 문단부호 x는 그림 뒤에 있어야 한다: mark_x={}, image_right={}",
            para0_end.2,
            image_right
        );
        assert!(
            para1_end.3 >= image_bottom - 0.5,
            "새 빈 문단부호는 그림 아래 줄에 있어야 한다: mark_y={}, image_bottom={}",
            para1_end.3,
            image_bottom
        );
    }

    #[test]
    fn issue1452_picture_text_wrap_updates_hwp_attr_bits() {
        let mut core = make_test_core();
        let image = minimal_png();
        core.insert_picture_native(
            0,
            0,
            0,
            &[],
            &image,
            5000,
            5000,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert picture body");

        {
            let pic = match &mut core.document.sections[0].paragraphs[0].controls[0] {
                Control::Picture(p) => p.as_mut(),
                _ => panic!("expected picture"),
            };
            pic.common.attr |= 1 << 30;
        }

        let cases = [
            (
                "InFrontOfText",
                crate::model::shape::TextWrap::InFrontOfText,
                3u32,
            ),
            (
                "BehindText",
                crate::model::shape::TextWrap::BehindText,
                2u32,
            ),
            (
                "TopAndBottom",
                crate::model::shape::TextWrap::TopAndBottom,
                1u32,
            ),
            ("Square", crate::model::shape::TextWrap::Square, 0u32),
        ];

        for (name, expected_wrap, expected_bits) in cases {
            let json = format!(r#"{{"textWrap":"{}"}}"#, name);
            core.set_picture_properties_native(0, 0, 0, &json)
                .unwrap_or_else(|err| panic!("set textWrap={name} failed: {err}"));
            let pic = match &core.document.sections[0].paragraphs[0].controls[0] {
                Control::Picture(p) => p.as_ref(),
                _ => panic!("expected picture"),
            };
            assert_eq!(pic.common.text_wrap, expected_wrap);
            assert_eq!(
                (pic.common.attr >> 21) & 0x07,
                expected_bits,
                "HWP 저장용 attr textWrap bit가 stale이면 안 된다: {name}"
            );
            assert_ne!(
                pic.common.attr & (1 << 30),
                0,
                "알 수 없는 원본 attr 비트는 보존되어야 한다"
            );
        }
    }

    #[test]
    fn issue1452_picture_transparency_props_roundtrip() {
        let mut core = make_test_core();
        let image = minimal_png();
        core.insert_picture_native(
            0,
            0,
            0,
            &[],
            &image,
            5000,
            5000,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert picture body");

        core.set_picture_properties_native(0, 0, 0, r#"{"transparency":50}"#)
            .expect("set transparency");
        let props = core
            .get_picture_properties_native(0, 0, 0)
            .expect("get picture properties");
        assert!(
            props.contains(r#""transparency":50"#),
            "그림 속성 JSON은 투명도 50%를 반환해야 한다: {props}"
        );

        let pic = match &core.document.sections[0].paragraphs[0].controls[0] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!("expected picture"),
        };
        assert_eq!(pic.image_attr.clamped_transparency(), 50);
        assert!((pic.image_attr.opacity() - 0.5).abs() < f64::EPSILON);

        core.set_picture_properties_native(0, 0, 0, r#"{"transparency":200}"#)
            .expect("set clamped transparency");
        let pic = match &core.document.sections[0].paragraphs[0].controls[0] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!("expected picture"),
        };
        assert_eq!(
            pic.image_attr.clamped_transparency(),
            100,
            "속성 API로 들어온 범위 밖 투명도는 0~100으로 clamp되어야 한다"
        );
    }

    #[test]
    fn issue1452_picture_transparency_samples_parse_as_ui_percent() {
        for path in ["samples/투명도0-50.hwp", "samples/투명도0-50.hwpx"] {
            let data =
                std::fs::read(path).unwrap_or_else(|err| panic!("fixture 읽기 실패 {path}: {err}"));
            let core =
                DocumentCore::from_bytes(&data).unwrap_or_else(|err| panic!("parse {path}: {err}"));
            let transparencies = collect_picture_transparencies(&core.document);
            assert!(
                transparencies.len() >= 2,
                "샘플에는 최소 두 개의 그림이 있어야 한다: {path}, got {transparencies:?}"
            );
            assert_eq!(
                &transparencies[..2],
                &[0, 50],
                "샘플 첫 번째/두 번째 그림 투명도는 각각 0%, 50%여야 한다: {path}"
            );
        }
    }

    #[test]
    fn issue1452_picture_transparency_samples_render_once_with_opacity() {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};

        fn collect_images(node: &RenderNode, out: &mut Vec<(Option<usize>, Option<usize>, f64)>) {
            if let RenderNodeType::Image(img) = &node.node_type {
                out.push((img.para_index, img.control_index, img.opacity));
            }
            for child in &node.children {
                collect_images(child, out);
            }
        }

        for path in ["samples/투명도0-50.hwp", "samples/투명도0-50.hwpx"] {
            let data =
                std::fs::read(path).unwrap_or_else(|err| panic!("fixture 읽기 실패 {path}: {err}"));
            let core =
                DocumentCore::from_bytes(&data).unwrap_or_else(|err| panic!("parse {path}: {err}"));
            let tree = core
                .build_page_tree(0)
                .unwrap_or_else(|err| panic!("render tree {path}: {err}"));
            let mut images = Vec::new();
            collect_images(&tree.root, &mut images);

            assert_eq!(
                images.len(),
                2,
                "투명도 샘플의 그림은 두 번만 렌더되어야 한다: {path}, got {images:?}"
            );

            let mut identities = images
                .iter()
                .map(|(para, control, _)| (*para, *control))
                .collect::<Vec<_>>();
            identities.sort_unstable();
            identities.dedup();
            assert_eq!(
                identities.len(),
                2,
                "같은 그림 control 이 중복 렌더되면 안 된다: {path}, got {images:?}"
            );

            let mut opacities = images
                .iter()
                .map(|(_, _, opacity)| (opacity * 100.0).round() as i32)
                .collect::<Vec<_>>();
            opacities.sort_unstable();
            assert_eq!(
                opacities,
                vec![50, 100],
                "렌더 트리 불투명도는 투명도 0/50%를 100/50%로 보존해야 한다: {path}"
            );
        }
    }

    #[test]
    fn issue1452_enter_after_second_tac_picture_keeps_both_pictures() {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};

        fn collect_images(node: &RenderNode, out: &mut Vec<(Option<usize>, Option<usize>, f64)>) {
            if let RenderNodeType::Image(img) = &node.node_type {
                out.push((img.para_index, img.control_index, img.opacity));
            }
            for child in &node.children {
                collect_images(child, out);
            }
        }

        let data = std::fs::read("samples/투명도0-50.hwp")
            .expect("fixture 읽기 실패 samples/투명도0-50.hwp");
        let mut core = DocumentCore::from_bytes(&data).expect("parse samples/투명도0-50.hwp");

        core.split_paragraph_native(0, 0, 2)
            .expect("두 번째 TAC 그림 뒤 Enter");

        assert_eq!(
            core.document.sections[0].paragraphs.len(),
            2,
            "그림 뒤 Enter 는 새 빈 문단을 만들어야 한다"
        );
        assert!(
            core.document.sections[0].paragraphs[0].line_segs.len() >= 2,
            "원래 문단은 두 TAC 그림 줄을 유지해야 한다: {:?}",
            core.document.sections[0].paragraphs[0].line_segs
        );

        let tree = core.build_page_tree(0).expect("build page tree");
        let mut images = Vec::new();
        collect_images(&tree.root, &mut images);
        assert_eq!(
            images.len(),
            2,
            "Enter 후에도 두 그림이 모두 렌더되어야 한다: {images:?}"
        );

        let mut identities = images
            .iter()
            .map(|(para, control, _)| (*para, *control))
            .collect::<Vec<_>>();
        identities.sort_unstable();
        identities.dedup();
        assert_eq!(
            identities.len(),
            2,
            "두 그림 control 이 각각 렌더되어야 한다: {images:?}"
        );
    }

    #[test]
    fn issue1151_invalid_cell_path_returns_error() {
        let mut core = make_test_core();
        let _ = core
            .create_table_native(0, 0, 0, 1, 1)
            .expect("create table");
        let bad_path: Vec<(usize, usize, usize)> = vec![(0, 5, 0)]; // cell 5 는 1×1 표에 없음
        let image = minimal_png();
        let res = core.insert_picture_native(
            0, 0, 0, &bad_path, &image, 5000, 5000, 1, 1, "png", "test", None, None,
        );
        assert!(
            res.is_err(),
            "out-of-range cell path → Err 기대, got {res:?}"
        );
    }
}

#[cfg(test)]
mod issue_1151_v2_tac_toggle_tests {
    //! Issue #1151 v2: floating picture → "글자처럼 취급" 토글 시 한컴 정합 (H1).
    //!
    //! 한컴 산출물 분석 (samples/tac-verify/scenario-{a,b,c,d}-after.hwp) 결과:
    //! tac false→true 토글 시 picture 의 control 위치는 불변이고, 4 가지 필드만
    //! 갱신된다. (a) treat_as_char=true, (b) horz/vert_rel_to=Para, (c) h/v_offset=0,
    //! (d) parent paragraph 의 line_segs[0] 의 line_height = picture height,
    //!     text_height = picture height, baseline_distance = round(lh*0.85).
    //! paragraph.text / char_offsets / paragraph 수 변화 없음.

    use super::*;
    use crate::model::document::{Document, Section, SectionDef};
    use crate::model::image::{ImageAttr, Picture};
    use crate::model::page::PageDef;
    use crate::model::paragraph::LineSeg;
    use crate::model::shape::{CommonObjAttr, HorzRelTo, ShapeComponentAttr, TextWrap, VertRelTo};

    fn make_test_core() -> DocumentCore {
        let mut doc = Document::default();
        doc.sections.push(Section {
            section_def: SectionDef {
                page_def: PageDef {
                    width: 59528,
                    height: 84188,
                    margin_left: 8504,
                    margin_right: 8504,
                    margin_top: 5668,
                    margin_bottom: 4252,
                    margin_header: 4252,
                    margin_footer: 4252,
                    ..Default::default()
                },
                ..Default::default()
            },
            paragraphs: vec![Paragraph::default()],
            raw_stream: None,
        });
        let mut core = DocumentCore::new_empty();
        core.set_document(doc);
        core
    }

    fn minimal_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x00, 0x00, 0x00,
            0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    fn parse_idx(res: &str, key: &str) -> usize {
        res.split(&format!("\"{}\":", key))
            .nth(1)
            .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("missing {key} in {res}"))
    }

    /// 본문 (또는 임의 paragraph) 에 floating picture 를 직접 push 한다.
    /// 한컴이 만든 floating picture 의 model 상태 (tac=false, Paper-relative, offset 있음)
    /// 와 동등.
    fn push_body_floating_picture(
        para: &mut Paragraph,
        width_hu: u32,
        height_hu: u32,
        offset_h: u32,
        offset_v: u32,
        bin_id: u16,
    ) -> usize {
        let common_attr: u32 = (1 << 3) | (1 << 8) | (4 << 15) | (2 << 18);
        let pic = Picture {
            common: CommonObjAttr {
                ctrl_id: 0x67736F20,
                attr: common_attr,
                treat_as_char: false,
                vert_rel_to: VertRelTo::Paper,
                horz_rel_to: HorzRelTo::Paper,
                text_wrap: TextWrap::Square,
                horizontal_offset: offset_h,
                vertical_offset: offset_v,
                width: width_hu,
                height: height_hu,
                z_order: 0,
                ..Default::default()
            },
            shape_attr: ShapeComponentAttr {
                original_width: width_hu,
                original_height: height_hu,
                current_width: width_hu,
                current_height: height_hu,
                ..Default::default()
            },
            border_x: [0i32, 0, width_hu as i32, 0],
            border_y: [width_hu as i32, height_hu as i32, 0, height_hu as i32],
            image_attr: ImageAttr {
                bin_data_id: bin_id,
                ..Default::default()
            },
            ..Default::default()
        };
        let idx = para.controls.len();
        para.controls.push(Control::Picture(Box::new(pic)));
        para.ctrl_data_records.push(None);
        idx
    }

    /// 한컴 산출물에서 관찰된 baseline 비율: lh × 0.85 (round).
    fn expected_baseline(lh: i32) -> i32 {
        (lh as f64 * 0.85).round() as i32
    }

    // ─── Scenario A 등가 ───────────────────────────────────────────────
    #[test]
    fn tac_toggle_table_sibling_floating_to_inline() {
        let mut core = make_test_core();

        // 1×1 표 생성
        let table_res = core
            .create_table_native(0, 0, 0, 1, 1)
            .expect("create 1x1 table");
        let table_para_idx = parse_idx(&table_res, "paraIdx");
        let table_ctrl_idx = parse_idx(&table_res, "controlIdx");

        // 셀 안 floating picture 삽입 (v1 path, h=5331 HU)
        let cell_path: Vec<(usize, usize, usize)> = vec![(table_ctrl_idx, 0, 0)];
        let image = minimal_png();
        let pic_w = 5977u32;
        let pic_h = 5331u32;
        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            pic_w,
            pic_h,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert floating picture in cell");

        // picture 는 표 sibling 위치 (= 마지막 control)
        let pic_ctrl_idx = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;
        let before_paragraph_count = core.document.sections[0].paragraphs.len();
        let before_controls_count = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len();

        // tac false→true 토글
        let res = core.set_picture_properties_native(
            0,
            table_para_idx,
            pic_ctrl_idx,
            r#"{"treatAsChar":true}"#,
        );
        assert!(res.is_ok(), "set_picture_properties_native failed: {res:?}");

        let para = &core.document.sections[0].paragraphs[table_para_idx];
        let pic = match &para.controls[pic_ctrl_idx] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!("picture not at expected ctrl_idx"),
        };

        // (1) picture 위치 / paragraph 수 불변
        assert_eq!(para.controls.len(), before_controls_count);
        assert_eq!(
            core.document.sections[0].paragraphs.len(),
            before_paragraph_count
        );

        // (2) 4 필드 갱신
        assert!(pic.common.treat_as_char, "treat_as_char true");
        assert_eq!(pic.common.attr & 0x01, 0x01, "attr 비트 0 셋");
        assert!(
            matches!(pic.common.horz_rel_to, HorzRelTo::Para),
            "horz_rel_to=Para, got {:?}",
            pic.common.horz_rel_to
        );
        assert!(
            matches!(pic.common.vert_rel_to, VertRelTo::Para),
            "vert_rel_to=Para, got {:?}",
            pic.common.vert_rel_to
        );
        assert_eq!(pic.common.horizontal_offset, 0, "h_offset=0");
        assert_eq!(pic.common.vertical_offset, 0, "v_offset=0");

        // (3) LINE_SEG[0] 갱신
        let seg = &para.line_segs[0];
        assert_eq!(
            seg.line_height, pic_h as i32,
            "line_segs[0].line_height = picture height"
        );
        assert_eq!(
            seg.text_height, pic_h as i32,
            "line_segs[0].text_height = picture height"
        );
        assert_eq!(
            seg.baseline_distance,
            expected_baseline(pic_h as i32),
            "line_segs[0].baseline_distance = round(lh*0.85)"
        );

        // (4) text / char_offsets 불변 (sentinel char 추가하지 않음)
        assert_eq!(para.text, "");
        assert_eq!(para.char_offsets.len(), 0);
    }

    // ─── [Task #1151 v8 결함 A regression] v1 셀 floating 의 초기 rel_to=Paper ─
    //
    // 사용자 한컴 native 시연 (2026-05-30): 한컴이 셀 안 picture 신규 삽입 시
    // 가로/세로 기준 = "종이" (Paper). v1 plan 의 incellpicture.hwp dump 분석 정합.
    // v1 구현이 Page 로 잘못 설정한 결함을 정정.
    #[test]
    fn v8_cell_floating_picture_uses_paper_rel_to() {
        let mut core = make_test_core();
        let table_res = core
            .create_table_native(0, 0, 0, 1, 1)
            .expect("create 1x1 table");
        let table_para_idx = parse_idx(&table_res, "paraIdx");
        let table_ctrl_idx = parse_idx(&table_res, "controlIdx");
        let cell_path: Vec<(usize, usize, usize)> = vec![(table_ctrl_idx, 0, 0)];
        let image = minimal_png();
        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            5977,
            5331,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert floating picture in cell");
        let pic_ctrl_idx = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;
        let para = &core.document.sections[0].paragraphs[table_para_idx];
        let pic = match &para.controls[pic_ctrl_idx] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!("not Picture"),
        };

        // (A) typed field 가 Paper
        assert!(
            matches!(pic.common.horz_rel_to, HorzRelTo::Paper),
            "horz_rel_to = Paper (한컴 native default), got {:?}",
            pic.common.horz_rel_to
        );
        assert!(
            matches!(pic.common.vert_rel_to, VertRelTo::Paper),
            "vert_rel_to = Paper, got {:?}",
            pic.common.vert_rel_to
        );

        // (B) attr 비트 정합 — bit 3-4 (vert) = 0, bit 8-10 (horz) = 0 (둘 다 Paper)
        let bits_vert = (pic.common.attr >> 3) & 0b11;
        let bits_horz = (pic.common.attr >> 8) & 0b111;
        assert_eq!(bits_vert, 0, "attr bits 3-4 = Paper(0)");
        assert_eq!(bits_horz, 0, "attr bits 8-10 = Paper(0)");

        // (C) tac=false, wrap=Square 그대로
        assert!(!pic.common.treat_as_char);
        assert!(matches!(
            pic.common.text_wrap,
            crate::model::shape::TextWrap::Square
        ));
    }

    // ─── [Task #1151 v9 결함 D regression v2] 큰 picture 2 장 wrap 시나리오 ───
    //
    // 사용자 시연 (2026-05-30 후속): 큰 picture 2 장 (page 폭 초과) 글자처럼 토글 시
    // 한컴 native 는 wrap (다음 line). Stage 23 fix 첫 버전은 pic_y 결정이 pic_x wrap
    // 처리 전이라 wrap 후 line_top_y 가 갱신됐어도 pic_y 가 wrap 전 값 → 두 picture
    // 같은 위치 겹침. Fix: pic_y 결정을 pic_x 뒤로 옮김 (wrap 후 state 반영).
    #[test]
    fn v9_two_large_pictures_wrap_to_next_line() {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};
        fn collect_image_bboxes(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Image(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for child in &node.children {
                collect_image_bboxes(child, out);
            }
        }

        let mut core = make_test_core();
        let table_res = core.create_table_native(0, 0, 0, 1, 1).expect("table");
        let table_para_idx = parse_idx(&table_res, "paraIdx");
        let table_ctrl_idx = parse_idx(&table_res, "controlIdx");
        let cell_path: Vec<(usize, usize, usize)> = vec![(table_ctrl_idx, 0, 0)];
        let image = minimal_png();

        // 큰 picture 2 장 — 각 80mm × 60mm (22680 × 17010 HU)
        // page 본문 폭 ≈ 150mm. 두 picture 합 160mm > 150mm → wrap 발생해야 함.
        let pic_w = 22680u32;
        let pic_h = 17010u32;

        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            pic_w,
            pic_h,
            1,
            1,
            "png",
            "p1",
            None,
            None,
        )
        .expect("insert pic1");
        let pic1_ctrl = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;
        core.set_picture_properties_native(0, table_para_idx, pic1_ctrl, r#"{"treatAsChar":true}"#)
            .expect("toggle pic1");

        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            pic_w,
            pic_h,
            1,
            1,
            "png",
            "p2",
            None,
            None,
        )
        .expect("insert pic2");
        let pic2_ctrl = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;
        core.set_picture_properties_native(0, table_para_idx, pic2_ctrl, r#"{"treatAsChar":true}"#)
            .expect("toggle pic2");

        let tree = core.build_page_tree_cached(0).expect("build");
        let mut images = vec![];
        collect_image_bboxes(&tree.root, &mut images);
        let pic_h_px = pic_h as f64 * 96.0 / 7200.0;

        assert_eq!(images.len(), 2, "두 picture 모두 render 되어야 함");
        let (x1, y1, _, _) = images[0];
        let (x2, y2, _, _) = images[1];

        // (A) 둘째 picture y 가 첫 picture y + pic_h 만큼 진행 (wrap)
        let y_diff = y2 - y1;
        assert!(
            (y_diff - pic_h_px).abs() < 1.0,
            "wrap: y_diff {:.2} ≈ pic_h {:.2} (한 picture height 만큼 진행) — got y1={}, y2={}",
            y_diff,
            pic_h_px,
            y1,
            y2
        );

        // (B) x 동일 (wrap 후 둘째 picture 가 새 line 의 좌측에서 시작)
        assert!(
            (x1 - x2).abs() < 1.0,
            "wrap: x 동일 (둘 다 새 line 의 좌측) — got x1={}, x2={}",
            x1,
            x2
        );
    }

    #[test]
    fn v9_two_tac_pictures_horizontal_distribute() {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};
        fn collect_image_bboxes(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Image(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for child in &node.children {
                collect_image_bboxes(child, out);
            }
        }

        let mut core = make_test_core();
        let table_res = core.create_table_native(0, 0, 0, 1, 1).expect("table");
        let table_para_idx = parse_idx(&table_res, "paraIdx");
        let table_ctrl_idx = parse_idx(&table_res, "controlIdx");
        let cell_path: Vec<(usize, usize, usize)> = vec![(table_ctrl_idx, 0, 0)];
        let image = minimal_png();

        // picture 1 삽입 + tac
        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            5670,
            5670,
            1,
            1,
            "png",
            "test1",
            None,
            None,
        )
        .expect("insert pic1");
        let pic1_ctrl = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;
        core.set_picture_properties_native(0, table_para_idx, pic1_ctrl, r#"{"treatAsChar":true}"#)
            .expect("toggle pic1");

        // picture 2 삽입 + tac
        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            5670,
            5670,
            1,
            1,
            "png",
            "test2",
            None,
            None,
        )
        .expect("insert pic2");
        let pic2_ctrl = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;
        core.set_picture_properties_native(0, table_para_idx, pic2_ctrl, r#"{"treatAsChar":true}"#)
            .expect("toggle pic2");

        // render tree 의 image bbox 검증
        let tree = core.build_page_tree_cached(0).expect("build page 0");
        let mut images = vec![];
        collect_image_bboxes(&tree.root, &mut images);
        assert_eq!(images.len(), 2, "두 picture 모두 render 되어야 함");

        let (x1, y1, w1, _h1) = images[0];
        let (x2, y2, _w2, _h2) = images[1];

        // (A) y 동일 (한 line) — 가로 분배 정합
        assert!(
            (y1 - y2).abs() < 0.5,
            "두 picture y 동일 (가로 분배) — got y1={}, y2={}",
            y1,
            y2
        );

        // (B) x 다름 (가로 누적) — pic2 x = pic1 x + pic1 width
        assert!(
            x2 > x1 + 0.5,
            "두 picture x 다름 (가로 누적) — got x1={}, x2={}",
            x1,
            x2
        );
        assert!(
            (x2 - (x1 + w1)).abs() < 0.5,
            "pic2 x ≈ pic1 x + pic1 width — got x1={}, x2={}, w1={}",
            x1,
            x2,
            w1
        );
    }

    // ─── Scenario D 등가 ───────────────────────────────────────────────
    #[test]
    fn tac_toggle_body_floating_to_inline() {
        let mut core = make_test_core();
        let pic_h = 19019u32;
        let pic_w = 20863u32;
        let ctrl_idx = {
            let para = &mut core.document.sections[0].paragraphs[0];
            push_body_floating_picture(para, pic_w, pic_h, 13428, 13568, 1)
        };

        let res = core.set_picture_properties_native(0, 0, ctrl_idx, r#"{"treatAsChar":true}"#);
        assert!(res.is_ok(), "set_picture_properties_native failed: {res:?}");

        let para = &core.document.sections[0].paragraphs[0];
        let pic = match &para.controls[ctrl_idx] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!("picture not at expected ctrl_idx"),
        };

        assert!(pic.common.treat_as_char);
        assert!(matches!(pic.common.horz_rel_to, HorzRelTo::Para));
        assert!(matches!(pic.common.vert_rel_to, VertRelTo::Para));
        assert_eq!(pic.common.horizontal_offset, 0);
        assert_eq!(pic.common.vertical_offset, 0);

        let seg = &para.line_segs[0];
        assert_eq!(seg.line_height, pic_h as i32);
        assert_eq!(seg.text_height, pic_h as i32);
        assert_eq!(seg.baseline_distance, expected_baseline(pic_h as i32));

        assert_eq!(para.text, "");
        assert_eq!(para.char_offsets.len(), 0);
    }

    // ─── Scenario C 등가 ───────────────────────────────────────────────
    #[test]
    fn tac_toggle_3x3_center_cell_floating_to_inline() {
        let mut core = make_test_core();

        let table_res = core
            .create_table_native(0, 0, 0, 3, 3)
            .expect("create 3x3 table");
        let table_para_idx = parse_idx(&table_res, "paraIdx");
        let table_ctrl_idx = parse_idx(&table_res, "controlIdx");

        // (1,1) 중앙 셀의 cell_path: (outer_ctrl_idx, row, col)
        let cell_path: Vec<(usize, usize, usize)> = vec![(table_ctrl_idx, 1, 1)];
        let image = minimal_png();
        let pic_w = 5434u32;
        let pic_h = 4847u32;
        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            pic_w,
            pic_h,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert floating picture in center cell");

        let pic_ctrl_idx = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;
        let res = core.set_picture_properties_native(
            0,
            table_para_idx,
            pic_ctrl_idx,
            r#"{"treatAsChar":true}"#,
        );
        assert!(res.is_ok(), "set_picture_properties_native failed: {res:?}");

        let para = &core.document.sections[0].paragraphs[table_para_idx];
        let pic = match &para.controls[pic_ctrl_idx] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!("picture not at expected ctrl_idx"),
        };
        assert!(pic.common.treat_as_char);
        assert_eq!(pic.common.horizontal_offset, 0);
        assert_eq!(pic.common.vertical_offset, 0);
        assert_eq!(para.line_segs[0].line_height, pic_h as i32);
        assert_eq!(
            para.line_segs[0].baseline_distance,
            expected_baseline(pic_h as i32)
        );
    }

    // ─── [Task #1151 v5] v1 path → tac toggle → page tree cache invalidate 검증 ─
    //
    // 사용자 보고 (2026-05-30): "rhwp 신규 표 + 셀 안 이미지 → tac 토글 시
    // 시각 변화 없음". 진단 결과 model + composer + paragraph_layout 모두 정상
    // 동작 (picture 가 표 아래 정확 위치 156.9 px 에 inline 렌더) 인데, studio
    // 가 stale page tree 받음. root cause: set_picture_properties_native 의
    // invalidate_page_tree_cache 호출 누락 — 다른 picture/shape setter (셀 picture
    // by_path / 셀 shape by_path / header-footer / shape 등) 는 모두 호출.
    //
    // 본 테스트는 v1 path → tac toggle 후 build_page_render_tree 가 picture 가
    // 표 아래로 이동한 새 위치로 ImageNode 를 emit 하는지 검증 — cache 갱신 정합.
    #[test]
    fn v5_tac_toggle_invalidates_page_tree_and_emits_inline_picture_below_table() {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};
        fn collect_image_bboxes(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Image(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for child in &node.children {
                collect_image_bboxes(child, out);
            }
        }
        fn collect_table_bboxes(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Table(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for child in &node.children {
                collect_table_bboxes(child, out);
            }
        }

        let mut core = make_test_core();
        let table_res = core
            .create_table_native(0, 0, 0, 1, 1)
            .expect("create 1x1 table");
        let table_para_idx = parse_idx(&table_res, "paraIdx");
        let table_ctrl_idx = parse_idx(&table_res, "controlIdx");
        let cell_path: Vec<(usize, usize, usize)> = vec![(table_ctrl_idx, 0, 0)];
        let image = minimal_png();
        let pic_w = 5977u32;
        let pic_h = 5331u32;
        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            pic_w,
            pic_h,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert floating picture in cell");
        let pic_ctrl_idx = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;

        // toggle 전: build_page_tree_cached 호출 → cache 채움.
        let tree_before = core
            .build_page_tree_cached(0)
            .expect("build_page_tree_cached pre-toggle");
        let mut image_before: Vec<(f64, f64, f64, f64)> = vec![];
        collect_image_bboxes(&tree_before.root, &mut image_before);
        assert_eq!(image_before.len(), 1, "toggle 전 ImageNode 1 개 필요");
        let (_x0, y_before, _w0, _h0) = image_before[0];

        // tac false → true 토글
        core.set_picture_properties_native(
            0,
            table_para_idx,
            pic_ctrl_idx,
            r#"{"treatAsChar":true}"#,
        )
        .expect("toggle");

        // toggle 후: build_page_tree_cached 다시 호출. fix 적용 시 invalidate_page_tree_cache
        // 가 작동하여 새 tree 반환 (picture 위치 = 표 아래). fix 미적용 시 stale cache 반환.
        let tree_after = core
            .build_page_tree_cached(0)
            .expect("build_page_tree_cached post-toggle");
        let mut image_after: Vec<(f64, f64, f64, f64)> = vec![];
        collect_image_bboxes(&tree_after.root, &mut image_after);
        let mut table_after: Vec<(f64, f64, f64, f64)> = vec![];
        collect_table_bboxes(&tree_after.root, &mut table_after);

        assert_eq!(image_after.len(), 1, "toggle 후 ImageNode 1 개 필요");
        assert_eq!(table_after.len(), 1, "toggle 후 Table 1 개 필요");
        let (_x_a, y_after, _w_a, _h_a) = image_after[0];
        let (_tx, ty, _tw, th) = table_after[0];
        let table_bottom = ty + th;

        // (A) cache invalidate 검증: toggle 전후 picture y 가 다름 (stale cache 아님).
        assert!(
            (y_before - y_after).abs() > 0.5,
            "FAIL: page tree cache invalidate 누락 — toggle 후에도 picture y 동일 (before={}, after={})",
            y_before,
            y_after
        );

        // (B) toggle 후 picture 가 표 아래 위치 (한컴 정합).
        assert!(
            y_after > table_bottom,
            "FAIL: picture 가 표 아래에 미배치 — picture y={}, table bottom={}",
            y_after,
            table_bottom
        );
    }

    // ─── [Task #1151 v6] 한컴 정합 (scenario-a-after.hwp) render tree baseline ──
    //
    // v6 root cause 진단 베이스라인 — 한컴 정합 model 의 render tree 가 표를
    // 정확한 셀 size 로 그리고 picture 가 표 아래에 배치됨을 확인. v6 fix
    // (Table::update_ctrl_dimensions 가 self.common 동기화) 가 적용된 후 rhwp
    // v1 path + 셀 size 조절 + tac toggle 의 render tree 가 이 baseline 과 같은
    // 패턴 (image y > table bottom) 을 따르는지가 v6 fix 정합 기준.
    #[test]
    fn v6_render_tree_scenario_a_after_baseline() {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};
        fn collect_image(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Image(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for c in &node.children {
                collect_image(c, out);
            }
        }
        fn collect_table(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Table(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for c in &node.children {
                collect_table(c, out);
            }
        }

        let bytes = std::fs::read("samples/tac-verify/scenario-a-after.hwp")
            .expect("read scenario-a-after.hwp");
        let doc = crate::parser::parse_hwp(&bytes).expect("parse scenario-a-after.hwp");
        let mut core = DocumentCore::new_empty();
        core.set_document(doc);
        let tree = core.build_page_tree_cached(0).expect("build page 0");
        let mut images = vec![];
        let mut tables = vec![];
        collect_image(&tree.root, &mut images);
        collect_table(&tree.root, &mut tables);

        // baseline 단언: 표 와 picture 가 분리되어 표 아래에 picture 배치
        assert_eq!(tables.len(), 1, "한컴 정합 표 1개");
        assert_eq!(images.len(), 1, "한컴 정합 picture 1개");
        let (_tx, ty, _tw, th) = tables[0];
        let (_ix, iy, _iw, _ih) = images[0];
        assert!(
            iy > ty + th,
            "한컴 baseline: picture 가 표 아래 (iy={}, table_bottom={})",
            iy,
            ty + th
        );
    }

    // ─── [Task #1151 v6 regression] rhwp v1 path + 셀 height 조절 + tac toggle ─
    //
    // Root cause: Table::update_ctrl_dimensions 가 raw_ctrl_data 만 갱신하고
    // self.common.width / self.common.height 는 동기화하지 않아 paragraph_layout 의
    // v3 helper 가 stale 값 (cell 조절 전) 을 사용 → picture 가 표 아래로 충분히
    // 안 밀려나고 표 박스 안에 들어감 (사용자 보고 2026-05-30).
    //
    // Fix: update_ctrl_dimensions 에서 self.common.width / height 동기화.
    // 검증: cell.height = 11498 조절 후 tac toggle → table.common.height == 11498
    // 및 picture y > table bottom.
    #[test]
    fn v6_resize_cell_then_tac_toggle_picture_below_table() {
        use crate::renderer::render_tree::{RenderNode, RenderNodeType};
        fn collect_image(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Image(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for c in &node.children {
                collect_image(c, out);
            }
        }
        fn collect_table(node: &RenderNode, out: &mut Vec<(f64, f64, f64, f64)>) {
            if matches!(node.node_type, RenderNodeType::Table(_)) {
                out.push((node.bbox.x, node.bbox.y, node.bbox.width, node.bbox.height));
            }
            for c in &node.children {
                collect_table(c, out);
            }
        }

        let mut core = make_test_core();
        let table_res = core.create_table_native(0, 0, 0, 1, 1).expect("table");
        let table_para_idx = parse_idx(&table_res, "paraIdx");
        let table_ctrl_idx = parse_idx(&table_res, "controlIdx");

        // 셀 height 를 한컴 정합 size (12498 HU) 와 유사하게 조절.
        // default cell.height = 1282 → delta = 12498 - 1282 = 11216
        core.resize_table_cells_native(
            0,
            table_para_idx,
            table_ctrl_idx,
            r#"[{"cellIdx":0,"heightDelta":11216}]"#,
        )
        .expect("resize cell");

        // v6 fix 1: resize 후 table.common.height 가 cell.height 와 동기화
        let table =
            match &core.document.sections[0].paragraphs[table_para_idx].controls[table_ctrl_idx] {
                Control::Table(t) => t,
                _ => panic!(),
            };
        assert_eq!(
            table.common.height, 11498,
            "v6 fix: table.common.height 가 cell 조절 후 동기화 (raw_ctrl_data 뿐 아니라 self.common 도)"
        );
        assert_eq!(table.cells[0].height, 11498);

        // picture 삽입 (v1 path)
        let cell_path: Vec<(usize, usize, usize)> = vec![(table_ctrl_idx, 0, 0)];
        let image = minimal_png();
        core.insert_picture_native(
            0,
            table_para_idx,
            0,
            &cell_path,
            &image,
            5977,
            5331,
            1,
            1,
            "png",
            "test",
            None,
            None,
        )
        .expect("insert");
        let pic_ctrl_idx = core.document.sections[0].paragraphs[table_para_idx]
            .controls
            .len()
            - 1;

        // tac toggle
        core.set_picture_properties_native(
            0,
            table_para_idx,
            pic_ctrl_idx,
            r#"{"treatAsChar":true}"#,
        )
        .expect("toggle");

        // v6 fix 2: render tree 의 picture 가 표 box 아래에 배치되는지 확인.
        let tree = core.build_page_tree_cached(0).expect("build page 0");
        let mut images = vec![];
        let mut tables = vec![];
        collect_image(&tree.root, &mut images);
        collect_table(&tree.root, &mut tables);
        assert_eq!(tables.len(), 1);
        assert_eq!(images.len(), 1);
        let (_tx, ty, _tw, th) = tables[0];
        let (_ix, iy, _iw, _ih) = images[0];
        assert!(
            iy > ty + th,
            "v6 fix: picture 가 표 아래 (iy={}, table_bottom={}) — table.common.height 동기화 정합",
            iy,
            ty + th
        );
    }

    // ─── 이미 tac=true 인 picture 의 다른 속성 변경 — migration 미진입 ─────
    #[test]
    fn tac_toggle_when_already_tac_true_no_migration() {
        let mut core = make_test_core();
        let pic_h = 5000u32;
        let ctrl_idx = {
            let para = &mut core.document.sections[0].paragraphs[0];
            push_body_floating_picture(para, 5000, pic_h, 1000, 1000, 1)
        };

        // 먼저 tac=true 로 마이그레이션
        core.set_picture_properties_native(0, 0, ctrl_idx, r#"{"treatAsChar":true}"#)
            .expect("first migration");
        let lh_after_first = core.document.sections[0].paragraphs[0].line_segs[0].line_height;

        // 두 번째 호출: tac 변경 없이 다른 속성 변경 — migration 미진입
        core.set_picture_properties_native(0, 0, ctrl_idx, r#"{"brightness":50}"#)
            .expect("second call no-op for migration");

        let para = &core.document.sections[0].paragraphs[0];
        // line_height 가 더 자라지 않아야 함 (이미 picture height 인 채로 유지)
        assert_eq!(para.line_segs[0].line_height, lh_after_first);
        // brightness 는 적용됨
        let pic = match &para.controls[ctrl_idx] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!(),
        };
        assert_eq!(pic.image_attr.brightness, 50);
    }

    // ─── tac=true → false 토글 — 빈 그림 문단 LINE_SEG 재구성 ──────────
    #[test]
    fn tac_toggle_true_to_false_restores_empty_picture_para_line_seg() {
        let mut core = make_test_core();
        let pic_h = 5000u32;
        let ctrl_idx = {
            let para = &mut core.document.sections[0].paragraphs[0];
            push_body_floating_picture(para, 5000, pic_h, 1000, 1000, 1)
        };
        // 먼저 tac=true 로
        core.set_picture_properties_native(0, 0, ctrl_idx, r#"{"treatAsChar":true}"#)
            .expect("forward migration");
        let lh_after_forward = core.document.sections[0].paragraphs[0].line_segs[0].line_height;
        assert_eq!(lh_after_forward, pic_h as i32);

        // tac=false 로 — 빈 그림 전용 문단에는 더 이상 inline 슬롯이 없으므로 기본 빈 줄로 복원.
        core.set_picture_properties_native(0, 0, ctrl_idx, r#"{"treatAsChar":false}"#)
            .expect("reverse toggle");
        let para = &core.document.sections[0].paragraphs[0];
        assert_eq!(para.line_segs.len(), 1);
        assert_eq!(
            para.line_segs[0].line_height, 1000,
            "남은 TAC 개체가 없으면 기본 빈 줄 높이로 복원"
        );
        assert_eq!(
            para.line_segs[0].baseline_distance, 850,
            "기본 빈 줄 기준선으로 복원"
        );
        let pic = match &para.controls[ctrl_idx] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!(),
        };
        assert!(!pic.common.treat_as_char, "tac 비트는 false 로 토글");
    }

    // ─── 빈 line_segs paragraph 의 토글 — line_seg 신설 ────────────────
    #[test]
    fn tac_toggle_with_empty_line_segs_creates_new_seg() {
        let mut core = make_test_core();
        let pic_h = 7000u32;
        let ctrl_idx = {
            let para = &mut core.document.sections[0].paragraphs[0];
            para.line_segs.clear(); // 빈 line_segs 강제
            push_body_floating_picture(para, 7000, pic_h, 1000, 1000, 1)
        };
        core.set_picture_properties_native(0, 0, ctrl_idx, r#"{"treatAsChar":true}"#)
            .expect("migration");

        let para = &core.document.sections[0].paragraphs[0];
        assert!(
            !para.line_segs.is_empty(),
            "빈 line_segs 였다면 신설되어야 한다"
        );
        let seg = &para.line_segs[0];
        assert_eq!(seg.line_height, pic_h as i32);
        assert_eq!(seg.text_height, pic_h as i32);
        assert_eq!(seg.baseline_distance, expected_baseline(pic_h as i32));
    }

    // LineSeg 빈 케이스 직접 검증용 (별도 helper 미사용 check)
    #[test]
    #[allow(dead_code)]
    fn _lineseg_default_for_test() {
        let seg = LineSeg::default();
        assert_eq!(seg.line_height, 0);
    }

    // ═══════════════════════════════════════════════════════════════════
    //  통합 검증 (Stage 2): 한컴 산출물 정합
    //
    //  samples/tac-verify/scenario-{a,b,c,d}-before.hwp 를 rhwp 가 파싱한 후
    //  set_picture_properties_native 로 tac false→true 토글한 결과가
    //  scenario-{a,b,c,d}-after.hwp 의 model 과 dump 동치인지 검증한다.
    //  v2 fix 가 만든 model 이 한컴이 만든 model 과 양방향 정합임을 보장.
    // ═══════════════════════════════════════════════════════════════════

    /// 양방향 정합 검증의 공통 단언 — paragraph 0.0 의 picture / line_segs 비교.
    fn assert_toggle_matches_hancom(scenario: &str) {
        let before_bytes =
            std::fs::read(format!("samples/tac-verify/scenario-{scenario}-before.hwp"))
                .expect("read before.hwp");
        let after_bytes =
            std::fs::read(format!("samples/tac-verify/scenario-{scenario}-after.hwp"))
                .expect("read after.hwp");

        let before_doc = crate::parser::parse_hwp(&before_bytes).expect("parse before");
        let after_doc = crate::parser::parse_hwp(&after_bytes).expect("parse after");

        let mut core = DocumentCore::new_empty();
        core.set_document(before_doc);

        // picture 위치 찾기 (paragraph 0.0 의 첫 Picture control)
        let pic_ctrl_idx = core.document.sections[0].paragraphs[0]
            .controls
            .iter()
            .position(|c| matches!(c, Control::Picture(_)))
            .unwrap_or_else(|| panic!("scenario-{scenario}-before: no Picture control"));

        core.set_picture_properties_native(0, 0, pic_ctrl_idx, r#"{"treatAsChar":true}"#)
            .expect("toggle");

        // 토글된 picture
        let toggled_para = &core.document.sections[0].paragraphs[0];
        let toggled_pic = match &toggled_para.controls[pic_ctrl_idx] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!("not Picture after toggle"),
        };

        // 한컴 after 의 picture
        let after_para = &after_doc.sections[0].paragraphs[0];
        let after_pic_ctrl_idx = after_para
            .controls
            .iter()
            .position(|c| matches!(c, Control::Picture(_)))
            .unwrap_or_else(|| panic!("scenario-{scenario}-after: no Picture control"));
        let after_pic = match &after_para.controls[after_pic_ctrl_idx] {
            Control::Picture(p) => p.as_ref(),
            _ => panic!(),
        };

        // (a) picture 4 필드 비교
        assert_eq!(
            toggled_pic.common.treat_as_char, after_pic.common.treat_as_char,
            "scenario-{scenario}: treat_as_char mismatch"
        );
        assert_eq!(
            toggled_pic.common.horizontal_offset, after_pic.common.horizontal_offset,
            "scenario-{scenario}: horizontal_offset mismatch"
        );
        assert_eq!(
            toggled_pic.common.vertical_offset, after_pic.common.vertical_offset,
            "scenario-{scenario}: vertical_offset mismatch"
        );
        assert_eq!(
            toggled_pic.common.horz_rel_to as u8, after_pic.common.horz_rel_to as u8,
            "scenario-{scenario}: horz_rel_to mismatch"
        );
        assert_eq!(
            toggled_pic.common.vert_rel_to as u8, after_pic.common.vert_rel_to as u8,
            "scenario-{scenario}: vert_rel_to mismatch"
        );

        // (b) line_segs[0] 비교
        let toggled_seg = &toggled_para.line_segs[0];
        let after_seg = &after_para.line_segs[0];
        assert_eq!(
            toggled_seg.line_height, after_seg.line_height,
            "scenario-{scenario}: line_height mismatch"
        );
        assert_eq!(
            toggled_seg.text_height, after_seg.text_height,
            "scenario-{scenario}: text_height mismatch"
        );
        assert_eq!(
            toggled_seg.baseline_distance, after_seg.baseline_distance,
            "scenario-{scenario}: baseline_distance mismatch (round(lh*0.85) 정합)"
        );

        // (c) paragraph 수 / picture 위치 불변
        assert_eq!(
            core.document.sections[0].paragraphs.len(),
            after_doc.sections[0].paragraphs.len(),
            "scenario-{scenario}: paragraph count mismatch"
        );
        assert_eq!(
            pic_ctrl_idx, after_pic_ctrl_idx,
            "scenario-{scenario}: picture control_idx mismatch"
        );

        // (d) paragraph.text 불변
        assert_eq!(
            toggled_para.text, after_para.text,
            "scenario-{scenario}: paragraph.text mismatch"
        );
    }

    #[test]
    fn integration_tac_toggle_matches_hancom_scenario_a() {
        assert_toggle_matches_hancom("a");
    }

    #[test]
    fn integration_tac_toggle_matches_hancom_scenario_b() {
        assert_toggle_matches_hancom("b");
    }

    #[test]
    fn integration_tac_toggle_matches_hancom_scenario_c() {
        assert_toggle_matches_hancom("c");
    }

    #[test]
    fn integration_tac_toggle_matches_hancom_scenario_d() {
        assert_toggle_matches_hancom("d");
    }
}

#[cfg(test)]
mod issue_1280_textbox_creation_tests {
    //! Issue #1280: rhwp-studio가 삽입한 글상자가 text_box 없는 Rectangle로 생성되어
    //! 커서 진입·타이핑·붙여넣기가 모두 실패하던 결함.
    //!
    //! 근본 결함은 프런트(`input-handler.ts`)가 `shapeType: 'rectangle'`을 전달한 것이고,
    //! 백엔드 `create_shape_control_native`는 `shape_type == "textbox"`일 때 text_box(내부 문단)를
    //! 정상 구성한다. 본 테스트는 그 백엔드 계약(글상자=text_box 있음, 사각형=없음)을 고정하여
    //! 프런트 수정과 함께 회귀를 막는다.

    use super::*;
    use crate::model::document::{Document, Section, SectionDef};
    use crate::model::page::PageDef;

    fn make_test_core() -> DocumentCore {
        let mut doc = Document::default();
        doc.sections.push(Section {
            section_def: SectionDef {
                page_def: PageDef {
                    width: 59528,
                    height: 84188,
                    margin_left: 8504,
                    margin_right: 8504,
                    margin_top: 5668,
                    margin_bottom: 4252,
                    margin_header: 4252,
                    margin_footer: 4252,
                    ..Default::default()
                },
                ..Default::default()
            },
            paragraphs: vec![Paragraph::default()],
            raw_stream: None,
        });
        let mut core = DocumentCore::new_empty();
        core.set_document(doc);
        core
    }

    fn parse_idx(res: &str, key: &str) -> usize {
        res.split(&format!("\"{}\":", key))
            .nth(1)
            .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("missing {key} in {res}"))
    }

    fn minimal_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x00, 0x00, 0x00,
            0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    /// 도형 생성 후 (para_idx, ctrl_idx) 반환. 글상자는 한컴 기본값과 동일하게 treat_as_char=true.
    fn create_shape(core: &mut DocumentCore, shape_type: &str) -> (usize, usize) {
        let treat_as_char = shape_type == "textbox";
        // 인자: section_idx, para_idx, char_offset, width, height, horz_offset, vert_offset,
        // treat_as_char, text_wrap_str, shape_type, line_flip_x, line_flip_y, polygon_points
        let res = core
            .create_shape_control_native(
                0,
                0,
                0,
                21600,
                7200,
                0,
                0,
                treat_as_char,
                "TopAndBottom",
                shape_type,
                false,
                false,
                &[],
            )
            .unwrap_or_else(|e| panic!("create {shape_type} failed: {e:?}"));
        (parse_idx(&res, "paraIdx"), parse_idx(&res, "controlIdx"))
    }

    fn textbox_of<'a>(
        core: &'a DocumentCore,
        para_idx: usize,
        ctrl_idx: usize,
    ) -> Option<&'a crate::model::shape::TextBox> {
        match &core.document.sections[0].paragraphs[para_idx].controls[ctrl_idx] {
            Control::Shape(s) => crate::document_core::helpers::get_textbox_from_shape(s.as_ref()),
            other => panic!("expected Control::Shape, got {other:?}"),
        }
    }

    fn common_of<'a>(
        core: &'a DocumentCore,
        para_idx: usize,
        ctrl_idx: usize,
    ) -> &'a crate::model::shape::CommonObjAttr {
        match &core.document.sections[0].paragraphs[para_idx].controls[ctrl_idx] {
            Control::Shape(s) => s.common(),
            other => panic!("expected Control::Shape, got {other:?}"),
        }
    }

    /// 글상자를 직접 인자로 생성(treat_as_char/text_wrap 명시). (para_idx, ctrl_idx) 반환.
    fn create_textbox_with(
        core: &mut DocumentCore,
        treat_as_char: bool,
        text_wrap: &str,
    ) -> (usize, usize) {
        let res = core
            .create_shape_control_native(
                0,
                0,
                0,
                21600,
                7200,
                1000,
                2000,
                treat_as_char,
                text_wrap,
                "textbox",
                false,
                false,
                &[],
            )
            .unwrap_or_else(|e| panic!("create textbox failed: {e:?}"));
        (parse_idx(&res, "paraIdx"), parse_idx(&res, "controlIdx"))
    }

    /// [Task #1280 v2] 삽입 글상자를 floating(treat_as_char=false)+InFrontOfText 로 만들면
    /// 한컴 정답값(Paper/Paper/글앞으로)으로 생성되고 text_box 는 그대로 유지된다.
    /// 권위 샘플 samples/textbox-under-image.hwp 실측 정합.
    #[test]
    fn create_floating_textbox_is_in_front_paper() {
        use crate::model::shape::{HorzRelTo, TextWrap, VertRelTo};
        let mut core = make_test_core();
        let (para, ctrl) = create_textbox_with(&mut core, false, "InFrontOfText");

        // text_box 유지 (글상자 기능 보존 — floating 에서도)
        assert!(
            textbox_of(&core, para, ctrl).is_some(),
            "floating 글상자도 text_box 를 가져야 한다"
        );

        let c = common_of(&core, para, ctrl);
        assert!(!c.treat_as_char, "floating: treat_as_char=false");
        assert_eq!(
            c.text_wrap,
            TextWrap::InFrontOfText,
            "글앞으로(InFrontOfText)"
        );
        assert_eq!(c.vert_rel_to, VertRelTo::Paper, "vert_rel_to=Paper");
        assert_eq!(c.horz_rel_to, HorzRelTo::Paper, "horz_rel_to=Paper");
        // 직렬화 attr 비트 정합 (serializer 는 common.attr!=0 이면 그대로 사용).
        assert_eq!(c.attr & 0x01, 0, "attr bit0(treat_as_char)=0");
        assert_eq!((c.attr >> 3) & 0x03, 0, "attr bit3-4(vert_rel_to)=Paper(0)");
        assert_eq!((c.attr >> 8) & 0x03, 0, "attr bit8-9(horz_rel_to)=Paper(0)");
        assert_eq!(
            (c.attr >> 21) & 0x07,
            3,
            "attr bit21-23(text_wrap)=InFrontOfText(3)"
        );
    }

    /// inline 글상자(treat_as_char=true)는 #1280 본편 배치(Para/Column)를 그대로 보존한다(회귀 가드).
    #[test]
    fn create_inline_textbox_preserves_para_column() {
        use crate::model::shape::{HorzRelTo, VertRelTo};
        let mut core = make_test_core();
        let (para, ctrl) = create_textbox_with(&mut core, true, "Square");
        let c = common_of(&core, para, ctrl);
        assert!(c.treat_as_char, "inline: treat_as_char=true");
        assert_eq!(c.vert_rel_to, VertRelTo::Para, "inline vert_rel_to=Para");
        assert_eq!(
            c.horz_rel_to,
            HorzRelTo::Column,
            "inline horz_rel_to=Column"
        );
    }

    /// floating 글상자에도 텍스트 입력이 정상 동작(#1280 본편 회귀 없음).
    #[test]
    fn insert_text_into_floating_textbox() {
        let mut core = make_test_core();
        let (para, ctrl) = create_textbox_with(&mut core, false, "InFrontOfText");
        core.insert_text_in_cell_native(0, para, ctrl, 0, 0, 0, "플로팅")
            .expect("floating 글상자 텍스트 입력 성공");
        let tb = textbox_of(&core, para, ctrl).expect("text_box 존재");
        assert_eq!(
            tb.paragraphs[0].text, "플로팅",
            "floating 글상자 내부 텍스트 보존"
        );
    }

    /// 글상자 안에서 이미지 배치 영역을 드래그한 경우, 그림은 body sibling 이 아니라
    /// text_box 내부 paragraph 의 Picture control 로 들어가야 한다.
    #[test]
    fn insert_picture_into_textbox_uses_textbox_paragraph_control() {
        use crate::model::shape::{HorzRelTo, TextWrap, VertRelTo};

        let mut core = make_test_core();
        let (para, ctrl) = create_textbox_with(&mut core, false, "InFrontOfText");
        let body_control_count_before = core.document.sections[0].paragraphs[para].controls.len();
        let cell_path = vec![(ctrl, 0, 0)];
        let image = minimal_png();

        core.insert_picture_native(
            0,
            para,
            0,
            &cell_path,
            &image,
            5000,
            4000,
            1,
            1,
            "png",
            "textbox picture",
            Some(750),
            Some(1500),
        )
        .expect("글상자 내부 picture 삽입 성공");

        let body = &core.document.sections[0].paragraphs[para];
        assert_eq!(
            body.controls.len(),
            body_control_count_before,
            "글상자 내부 삽입은 body sibling control 을 추가하면 안 된다"
        );

        let tb = textbox_of(&core, para, ctrl).expect("글상자 text_box 존재");
        let picture = tb.paragraphs[0]
            .controls
            .iter()
            .find_map(|c| match c {
                Control::Picture(p) => Some(p.as_ref()),
                _ => None,
            })
            .expect("글상자 내부 문단에 Picture control 이 있어야 한다");

        assert!(!picture.common.treat_as_char);
        assert_eq!(picture.common.horz_rel_to, HorzRelTo::Para);
        assert_eq!(picture.common.vert_rel_to, VertRelTo::Para);
        assert_eq!(picture.common.text_wrap, TextWrap::Square);
        assert_eq!(picture.common.horizontal_offset, 750);
        assert_eq!(picture.common.vertical_offset, 1500);
        assert_eq!(picture.common.width, 5000);
        assert_eq!(picture.common.height, 4000);
    }

    #[test]
    fn create_textbox_has_textbox() {
        let mut core = make_test_core();
        let (para, ctrl) = create_shape(&mut core, "textbox");
        assert!(
            textbox_of(&core, para, ctrl).is_some(),
            "글상자(shape_type=textbox)는 text_box를 가져야 한다 (#1280)"
        );
    }

    #[test]
    fn create_rectangle_has_no_textbox() {
        let mut core = make_test_core();
        let (para, ctrl) = create_shape(&mut core, "rectangle");
        assert!(
            textbox_of(&core, para, ctrl).is_none(),
            "일반 사각형(shape_type=rectangle)은 text_box가 없어야 한다 (글상자/사각형 경로 분리)"
        );
    }

    #[test]
    fn insert_text_into_created_textbox() {
        let mut core = make_test_core();
        let (para, ctrl) = create_shape(&mut core, "textbox");

        // 글상자 내부(cell_idx=0 무시, cell_para_idx=0, char_offset=0)에 텍스트 삽입.
        // 수정 전 프런트 경로에서는 text_box가 없어 "지정된 Shape 컨트롤에 텍스트 박스가 없습니다"로 실패했다.
        core.insert_text_in_cell_native(0, para, ctrl, 0, 0, 0, "테스트")
            .expect("글상자에 텍스트 입력이 성공해야 한다 (#1280)");

        let tb = textbox_of(&core, para, ctrl).expect("글상자 text_box 존재");
        assert_eq!(
            tb.paragraphs[0].text, "테스트",
            "글상자 내부 첫 문단에 입력 텍스트가 보존되어야 한다"
        );
    }

    /// #1280 이슈가 기대 동작에 명시한 "글상자 안 붙여넣기"를 실측한다.
    /// 본문 텍스트를 copy_selection 으로 복사한 뒤 글상자 안에 paste_internal_in_cell 로 붙여넣는다.
    /// 수정 전(text_box 없는 Rectangle)이면 이 경로가 "글상자 없음"(clipboard.rs:512)으로 실패한다.
    ///
    /// 이미지/컨트롤 붙여넣기는 merge_from 이 controls 를 병합하지 않아 조용히 누락되던
    /// 별개 결함(#1323)이 있었으며, merge_from 보강으로 해소되었다.
    /// 회귀 테스트는 `paste_picture_into_textbox` 참고.
    #[test]
    fn paste_text_into_textbox() {
        let mut core = make_test_core();

        // 1. 본문에 텍스트 입력 후 선택 영역 복사 → 내부 클립보드에 텍스트 적재(controls 없음)
        core.insert_text_native(0, 0, 0, "복사원본")
            .expect("본문 텍스트 입력");
        core.copy_selection_native(0, 0, 0, 0, 4)
            .expect("본문 텍스트 복사");

        // 2. 글상자 생성
        let (tb_para, tb_ctrl) = create_shape(&mut core, "textbox");

        // 3. 글상자 안에 붙여넣기 (cell_idx=0, cell_para_idx=0, char_offset=0)
        core.paste_internal_in_cell_native(0, tb_para, tb_ctrl, 0, 0, 0)
            .expect("글상자에 붙여넣기가 성공해야 한다 (#1280; 수정 전엔 \"글상자 없음\")");

        // 4. 글상자 내부 첫 문단에 붙여넣은 텍스트가 들어갔는지 확인
        let tb = textbox_of(&core, tb_para, tb_ctrl).expect("글상자 text_box 존재");
        assert!(
            tb.paragraphs.iter().any(|p| p.text.contains("복사원본")),
            "붙여넣기 후 글상자 내부 문단에 복사한 텍스트가 있어야 한다"
        );
    }

    /// #1323: 글상자 안 이미지(그림 컨트롤) 붙여넣기 회귀 테스트.
    /// 본문 그림을 copy_control 로 복사한 뒤 글상자 안에 paste_internal_in_cell 로
    /// 붙여넣는다. merge_from 이 controls 를 병합하지 않던 수정 전에는 그림이
    /// 에러 없이 조용히 누락되었다.
    #[test]
    fn paste_picture_into_textbox() {
        let mut core = make_test_core();

        // 1. 본문에 그림 삽입 (BinData 등록 포함)
        let res = core
            .insert_picture_native(
                0,
                0,
                0,
                &[],
                &minimal_png(),
                5000,
                5000,
                1,
                1,
                "png",
                "",
                None,
                None,
            )
            .expect("본문 그림 삽입");
        let pic_para = parse_idx(&res, "paraIdx");
        let pic_ctrl = parse_idx(&res, "controlIdx");

        // 2. 그림 복사 → 내부 클립보드
        core.copy_control_native(0, pic_para, &[], pic_ctrl)
            .expect("그림 복사");

        // 3. 글상자 생성 + 안에 붙여넣기 (cell_idx=0 무시, cell_para_idx=0, char_offset=0)
        let (tb_para, tb_ctrl) = create_shape(&mut core, "textbox");
        core.paste_internal_in_cell_native(0, tb_para, tb_ctrl, 0, 0, 0)
            .expect("글상자에 그림 붙여넣기");

        // 4. 글상자 내부 문단에 그림 컨트롤 보존 확인
        let tb = textbox_of(&core, tb_para, tb_ctrl).expect("글상자 text_box 존재");
        let pic_count: usize = tb
            .paragraphs
            .iter()
            .map(|p| {
                p.controls
                    .iter()
                    .filter(|c| matches!(c, Control::Picture(_)))
                    .count()
            })
            .sum();
        assert_eq!(
            pic_count, 1,
            "글상자 안에 붙여넣은 그림 컨트롤이 보존되어야 한다 (#1323)"
        );
    }
}
