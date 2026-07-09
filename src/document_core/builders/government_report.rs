//! Government/business report DocumentCore builder for MCP regeneration tests.
//!
//! The builder intentionally uses the existing editing commands instead of
//! hand-assembling every low-level record. This keeps generated sessions on the
//! same path that MCP clients exercise when they edit real documents.

use crate::DocumentCore;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct GovernmentReport {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
    #[serde(default)]
    pub period: Option<String>,
    #[serde(default)]
    pub confidentiality: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default, alias = "pageFooter")]
    pub page_footer: Option<String>,
    #[serde(default)]
    pub rows: Vec<GovernmentReportRow>,
    #[serde(default)]
    pub sections: Vec<GovernmentReportSection>,
    #[serde(default)]
    pub footer: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GovernmentReportRow {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GovernmentReportSection {
    pub title: String,
    #[serde(default, alias = "pageBreakBefore")]
    pub page_break_before: bool,
    #[serde(default)]
    pub paragraphs: Vec<String>,
    #[serde(default)]
    pub tables: Vec<GovernmentReportTable>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GovernmentReportTable {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub headers: Vec<String>,
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
}

pub fn parse_report_str(text: &str) -> Result<GovernmentReport, String> {
    serde_json::from_str(text).map_err(|e| format!("invalid government report JSON: {e}"))
}

pub fn parse_report_value(value: Value) -> Result<GovernmentReport, String> {
    serde_json::from_value(value).map_err(|e| format!("invalid government report object: {e}"))
}

pub fn build_government_report(report: &GovernmentReport) -> Result<DocumentCore, String> {
    let mut core = DocumentCore::new_empty();
    core.create_blank_document_native()
        .map_err(|e| e.to_string())?;

    set_paragraph_text(&mut core, 0, &report.title)?;
    apply_char_format(
        &mut core,
        0,
        &report.title,
        r#"{"fontSize":1800,"bold":true}"#,
    )?;

    if let Some(subtitle) = non_empty(report.subtitle.as_deref()) {
        let para = append_paragraph(&mut core, subtitle)?;
        apply_char_format(&mut core, para, subtitle, r#"{"fontSize":1300}"#)?;
    }

    let metadata = metadata_line(report);
    if !metadata.is_empty() {
        append_paragraph(&mut core, &metadata)?;
    }

    if let Some(summary) = non_empty(report.summary.as_deref()) {
        append_paragraph(&mut core, summary)?;
    }

    if let Some(header) = non_empty(report.header.as_deref()) {
        create_repeating_text(&mut core, true, header)?;
    }
    if let Some(page_footer) = non_empty(report.page_footer.as_deref()) {
        create_repeating_text(&mut core, false, page_footer)?;
    }

    create_summary_table(&mut core, &report.rows)?;

    for (idx, section) in report.sections.iter().enumerate() {
        append_report_section(&mut core, section, idx > 0)?;
    }

    if let Some(footer) = non_empty(report.footer.as_deref()) {
        append_paragraph(&mut core, footer)?;
    }

    Ok(core)
}

pub fn report_table_count(report: &GovernmentReport) -> usize {
    1 + report
        .sections
        .iter()
        .map(|section| section.tables.len())
        .sum::<usize>()
}

pub fn has_repeating_header(report: &GovernmentReport) -> bool {
    non_empty(report.header.as_deref()).is_some()
}

pub fn has_repeating_footer(report: &GovernmentReport) -> bool {
    non_empty(report.page_footer.as_deref()).is_some()
}

fn metadata_line(report: &GovernmentReport) -> String {
    let mut parts = Vec::new();
    if let Some(value) = non_empty(report.organization.as_deref()) {
        parts.push(format!("기관: {value}"));
    }
    if let Some(value) = non_empty(report.period.as_deref()) {
        parts.push(format!("기간: {value}"));
    }
    if let Some(value) = non_empty(report.confidentiality.as_deref()) {
        parts.push(format!("분류: {value}"));
    }
    parts.join(" | ")
}

fn append_report_section(
    core: &mut DocumentCore,
    section: &GovernmentReportSection,
    allow_page_break: bool,
) -> Result<(), String> {
    let title_para = if section.page_break_before && allow_page_break {
        append_page_break_paragraph(core, &section.title)?
    } else {
        append_paragraph(core, &section.title)?
    };
    apply_char_format(
        core,
        title_para,
        &section.title,
        r#"{"fontSize":1500,"bold":true}"#,
    )?;

    for paragraph in &section.paragraphs {
        if let Some(text) = non_empty(Some(paragraph.as_str())) {
            append_paragraph(core, text)?;
        }
    }

    for table in &section.tables {
        if let Some(title) = non_empty(table.title.as_deref()) {
            let para = append_paragraph(core, title)?;
            apply_char_format(core, para, title, r#"{"fontSize":1200,"bold":true}"#)?;
        }
        create_matrix_table(core, &table.headers, &table.rows)?;
    }

    Ok(())
}

fn create_summary_table(
    core: &mut DocumentCore,
    rows: &[GovernmentReportRow],
) -> Result<(), String> {
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            vec![
                row.label.clone(),
                row.value.clone(),
                row.unit.clone().unwrap_or_default(),
            ]
        })
        .collect();
    create_matrix_table(
        core,
        &["항목".to_string(), "값".to_string(), "단위".to_string()],
        &table_rows,
    )
}

fn create_matrix_table(
    core: &mut DocumentCore,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<(), String> {
    let col_count = matrix_col_count(headers, rows);
    let header_row_count: usize = if headers.is_empty() { 0 } else { 1 };
    let row_count = header_row_count.saturating_add(rows.len()).max(1);
    if row_count > u16::MAX as usize {
        return Err(format!(
            "too many rows for government report table: {row_count}"
        ));
    }
    if col_count > u16::MAX as usize {
        return Err(format!(
            "too many columns for government report table: {col_count}"
        ));
    }

    let table_para = append_paragraph(core, "")?;
    let (table_para, table_control) =
        create_table_at(core, table_para, row_count as u16, col_count as u16)?;

    let mut row_offset = 0;
    if !headers.is_empty() {
        for (col, text) in headers.iter().enumerate() {
            set_table_cell(core, table_para, table_control, col, text)?;
        }
        row_offset = 1;
    }

    for (row_idx, row) in rows.iter().enumerate() {
        for col in 0..col_count {
            let text = row.get(col).map(String::as_str).unwrap_or("");
            if !text.is_empty() {
                let cell_idx = (row_idx + row_offset) * col_count + col;
                set_table_cell(core, table_para, table_control, cell_idx, text)?;
            }
        }
    }

    Ok(())
}

fn matrix_col_count(headers: &[String], rows: &[Vec<String>]) -> usize {
    headers
        .len()
        .max(rows.iter().map(Vec::len).max().unwrap_or(0))
        .max(1)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
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

fn set_paragraph_text(core: &mut DocumentCore, para_idx: usize, text: &str) -> Result<(), String> {
    if !text.is_empty() {
        core.insert_text_native(0, para_idx, 0, text)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn append_paragraph(core: &mut DocumentCore, text: &str) -> Result<usize, String> {
    let para_idx = core
        .document
        .sections
        .first()
        .map(|section| section.paragraphs.len())
        .unwrap_or(0);
    core.insert_paragraph_native(0, para_idx)
        .map_err(|e| e.to_string())?;
    set_paragraph_text(core, para_idx, text)?;
    Ok(para_idx)
}

fn append_page_break_paragraph(core: &mut DocumentCore, text: &str) -> Result<usize, String> {
    let blank_para = append_paragraph(core, "")?;
    let result = core
        .insert_page_break_native(0, blank_para, 0)
        .map_err(|e| e.to_string())?;
    let para_idx = parse_json_field::<usize>(&result, "paraIdx")?;
    set_paragraph_text(core, para_idx, text)?;
    Ok(para_idx)
}

fn create_repeating_text(
    core: &mut DocumentCore,
    is_header: bool,
    text: &str,
) -> Result<(), String> {
    core.create_header_footer_native(0, is_header, 0)
        .map_err(|e| e.to_string())?;
    core.insert_text_in_header_footer_native(0, is_header, 0, 0, 0, text)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn apply_char_format(
    core: &mut DocumentCore,
    para_idx: usize,
    text: &str,
    props: &str,
) -> Result<(), String> {
    let len = text.chars().count();
    if len > 0 {
        core.apply_char_format_native(0, para_idx, 0, len, props)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn create_table_at(
    core: &mut DocumentCore,
    table_para: usize,
    row_count: u16,
    col_count: u16,
) -> Result<(usize, usize), String> {
    let table_result = core
        .create_table_native(0, table_para, 0, row_count, col_count)
        .map_err(|e| e.to_string())?;
    let table_para = parse_json_field::<usize>(&table_result, "paraIdx")?;
    let table_control = parse_json_field::<usize>(&table_result, "controlIdx")?;
    Ok((table_para, table_control))
}

fn set_table_cell(
    core: &mut DocumentCore,
    table_para: usize,
    table_control: usize,
    cell_idx: usize,
    text: &str,
) -> Result<(), String> {
    if !text.is_empty() {
        core.insert_text_in_cell_native(0, table_para, table_control, cell_idx, 0, 0, text)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
