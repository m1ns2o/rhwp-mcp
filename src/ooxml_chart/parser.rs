//! OOXML 차트 XML 파서
//!
//! DrawingML 차트 XML을 `OoxmlChart` 데이터 모델로 변환한다.
//! 의도적으로 관대한 파서: 알 수 없는 태그는 무시하고 지원 범위 데이터만 추출.
//!
//! ## 콤보/이중축 지원
//! - 여러 `<c:barChart>`, `<c:lineChart>`가 한 차트 안에 공존 가능
//! - 각 plot 블록의 `<c:axId val="...">`를 수집 → 시리즈에 복사
//! - `<c:valAx>`에서 `<c:axId>`와 `<c:axPos>` 수집 → axId→primary/secondary 매핑 생성
//! - 파싱 완료 시 시리즈의 axis_ids를 primary/secondary 집합과 비교해 axis_group 지정

use super::{
    AxisCrossBetween, AxisCrosses, AxisDisplayUnit, AxisLabelAlignment, AxisLabelPosition,
    AxisOrientation, AxisPosition, AxisTickMark, BarGrouping, ChartDataLabelPosition,
    ChartDisplayBlanksAs, ChartErrorBarDirection, ChartErrorBarType, ChartErrorBarValueType,
    ChartLegendPosition, ChartMarkerSymbol, ChartTrendlineType, OfPieType, OoxmlChart,
    OoxmlChartType, OoxmlSeries, ScatterStyle,
};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashMap;

/// 파싱 진행 시 문맥(현재 어떤 태그 트리에 있는지) 추적
#[derive(Default)]
struct ParseState {
    cur_series: Option<OoxmlSeries>,
    cur_text_buf: String,
    in_tx: bool,
    in_cat: bool,
    in_val: bool,
    in_chart_title: bool,
    in_axis_title: Option<AxisKind>,
    in_v: bool,
    in_a_t: bool,
    in_sp_pr: bool,      // c:spPr — 시리즈/figure의 shape properties
    in_solid_fill: bool, // a:solidFill
    in_ln: bool,         // a:ln (stroke)
    in_chart_space_sp_pr: bool,
    in_plot_area_sp_pr: bool,
    axis_sp_pr: Option<AxisKind>,
    axis_gridlines: Option<AxisGridLineKind>,
    axis_gridline_sp_pr: Option<AxisGridLineKind>,
    in_marker: bool,    // c:marker — line/scatter series marker properties
    marker_sp_pr: bool, // c:marker/c:spPr
    in_num_cache: bool, // c:numCache — formatCode 파싱
    in_trendline: bool,
    trendline_sp_pr: bool,
    in_error_bars: bool,
    error_bar_sp_pr: bool,
    in_of_pie_chart: bool,
    in_doughnut_chart: bool,
    in_bar_3d_chart: bool,
    in_plot_area: bool,
    in_data_table: bool,
    in_view_3d: bool,
    in_up_down_bars: bool,
    in_hi_low_lines: bool,
    hi_low_lines_sp_pr: bool,
    in_ser_lines: bool,
    ser_lines_sp_pr: bool,
    stock_bar_kind: Option<StockBarKind>,
    stock_bar_sp_pr: Option<StockBarKind>,
    in_data_labels: bool,
    in_data_label_point: bool,
    bar_dir: Option<BarDir>,
    // 현재 파싱 중인 plot 블록 (barChart/lineChart/pieChart/scatterChart/stockChart) 안에 있는지
    cur_plot_type: Option<OoxmlChartType>,
    // 현재 plot 블록에서 누적되는 axId (plot 종료 시 해당 plot의 모든 시리즈에 복사)
    cur_plot_ax_ids: Vec<String>,
    // 현재 plot이 시작된 시점의 chart.series.len() — plot 종료 시 이 시점 이후 시리즈에 axIds 할당
    cur_plot_series_start: usize,
    // c:valAx 블록 내에서 수집 중인 axId, axPos
    in_cat_ax: bool,
    cur_cat_ax_visible: Option<bool>,
    in_val_ax: bool,
    cur_val_ax_id: Option<String>,
    cur_val_ax_pos: Option<String>,
    cur_val_ax_visible: Option<bool>,
    in_val_ax_scaling: bool,
    in_val_ax_display_units: bool,
    // axId → axPos 매핑 (l/r/t/b)
    val_ax_map: HashMap<String, String>,
    in_legend: bool,
}

#[derive(Clone, Copy)]
enum BarDir {
    Bar,
    Col,
}

#[derive(Clone, Copy)]
enum AxisKind {
    Category,
    Value,
}

#[derive(Clone, Copy)]
enum AxisGridLineKind {
    CategoryMajor,
    CategoryMinor,
    ValueMajor,
    ValueMinor,
}

#[derive(Clone, Copy)]
enum StockBarKind {
    Up,
    Down,
}

/// OOXML 차트 XML 파싱 진입점
pub fn parse_chart_xml(xml: &[u8]) -> Option<OoxmlChart> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);

    let mut chart = OoxmlChart::default();
    let mut state = ParseState::default();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => handle_start(e, &mut chart, &mut state),
            Ok(Event::Empty(ref e)) => {
                handle_start(e, &mut chart, &mut state);
                handle_end(e.local_name().as_ref(), &mut chart, &mut state);
            }
            Ok(Event::End(ref e)) => handle_end(e.local_name().as_ref(), &mut chart, &mut state),
            Ok(Event::Text(t)) => {
                if state.in_v || state.in_a_t {
                    let s = t.decode().unwrap_or_default();
                    state.cur_text_buf.push_str(&s);
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    if chart.series.is_empty() && chart.title.is_none() {
        return None;
    }

    // 가로/세로 막대 최종 분기 (chart_type이 Column 상태면 barDir로 확정)
    if matches!(
        chart.chart_type,
        OoxmlChartType::Column | OoxmlChartType::Bar
    ) {
        if let Some(BarDir::Bar) = state.bar_dir {
            chart.chart_type = OoxmlChartType::Bar;
        } else {
            chart.chart_type = OoxmlChartType::Column;
        }
    }
    // 시리즈별 series_type이 Column인데 chart_type이 Bar인 경우도 동기화
    for s in chart.series.iter_mut() {
        if matches!(s.series_type, OoxmlChartType::Column | OoxmlChartType::Bar) {
            s.series_type = if matches!(state.bar_dir, Some(BarDir::Bar)) {
                OoxmlChartType::Bar
            } else {
                OoxmlChartType::Column
            };
        }
    }

    // 축 매핑 결정
    // primary: pos="l" (세로 막대/라인의 좌측 Y) 또는 pos="b"가 아닌 첫 valAx
    // secondary: primary가 아닌 나머지
    let mut primary_axid: Option<String> = None;
    let mut secondary_axid: Option<String> = None;
    // 순회 순서를 안정적으로 하기 위해 정렬
    let mut entries: Vec<(String, String)> = state
        .val_ax_map
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (axid, pos) in &entries {
        match pos.as_str() {
            "l" | "b" => {
                if primary_axid.is_none() {
                    primary_axid = Some(axid.clone());
                } else if secondary_axid.is_none() {
                    secondary_axid = Some(axid.clone());
                }
            }
            "r" | "t" => {
                if secondary_axid.is_none() {
                    secondary_axid = Some(axid.clone());
                } else if primary_axid.is_none() {
                    primary_axid = Some(axid.clone());
                }
            }
            _ => {
                if primary_axid.is_none() {
                    primary_axid = Some(axid.clone());
                } else if secondary_axid.is_none() {
                    secondary_axid = Some(axid.clone());
                }
            }
        }
    }

    // 시리즈 axis_group 지정
    for s in chart.series.iter_mut() {
        let is_secondary = match (&secondary_axid, &primary_axid) {
            (Some(sec), _) if s.axis_ids.iter().any(|a| a == sec) => true,
            (_, Some(pri)) if s.axis_ids.iter().any(|a| a == pri) => false,
            _ => false,
        };
        s.axis_group = if is_secondary { 1 } else { 0 };
        if is_secondary {
            chart.has_secondary_axis = true;
        }
    }

    Some(chart)
}

fn handle_start(e: &quick_xml::events::BytesStart, chart: &mut OoxmlChart, st: &mut ParseState) {
    let name = e.local_name();
    let name_bytes = name.as_ref();
    match name_bytes {
        b"barChart" => {
            chart.chart_type = OoxmlChartType::Column; // barDir로 세분
            st.cur_plot_type = Some(OoxmlChartType::Column);
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"plotArea" => st.in_plot_area = true,
        b"dTable" => {
            if st.in_plot_area {
                st.in_data_table = true;
            }
        }
        b"lineChart" => {
            if chart.chart_type == OoxmlChartType::Unknown {
                chart.chart_type = OoxmlChartType::Line;
            }
            st.cur_plot_type = Some(OoxmlChartType::Line);
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"pieChart" => {
            chart.chart_type = OoxmlChartType::Pie;
            st.cur_plot_type = Some(OoxmlChartType::Pie);
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"scatterChart" => {
            if chart.chart_type == OoxmlChartType::Unknown {
                chart.chart_type = OoxmlChartType::Scatter;
            }
            st.cur_plot_type = Some(OoxmlChartType::Scatter);
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"stockChart" => {
            if chart.chart_type == OoxmlChartType::Unknown {
                chart.chart_type = OoxmlChartType::Stock;
            }
            st.cur_plot_type = Some(OoxmlChartType::Stock);
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"bar3DChart" => {
            // 3D 막대 — 2D 근사(C1a #1453). barDir 핸들러가 col/bar를 그대로 채워
            // 파싱 종료 후처리가 Column↔Bar를 확정한다.
            chart.chart_type = OoxmlChartType::Column;
            st.cur_plot_type = Some(OoxmlChartType::Column);
            st.in_bar_3d_chart = true;
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"pie3DChart" => {
            // 3D 원형 — 단일 원형으로 2D 근사(C1a #1453).
            chart.chart_type = OoxmlChartType::Pie;
            st.cur_plot_type = Some(OoxmlChartType::Pie);
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"doughnutChart" => {
            // 도넛 — 원형으로 2D 근사하되 hole size semantic은 보존한다.
            chart.chart_type = OoxmlChartType::Pie;
            chart.has_doughnut_chart = true;
            st.cur_plot_type = Some(OoxmlChartType::Pie);
            st.in_doughnut_chart = true;
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"ofPieChart" => {
            // 원형대원형/원형대막대 — 단일 원형으로 2D 근사하되 보조 플롯 설정은 보존.
            chart.chart_type = OoxmlChartType::Pie;
            chart.has_of_pie_chart = true;
            st.cur_plot_type = Some(OoxmlChartType::Pie);
            st.in_of_pie_chart = true;
            st.cur_plot_ax_ids.clear();
            st.cur_plot_series_start = chart.series.len();
        }
        b"ofPieType" => {
            if st.in_of_pie_chart {
                if let Some(val) = attr_val(e, "val") {
                    chart.pie_of_pie_type = of_pie_type(&val);
                }
            }
        }
        b"serLines" => {
            if st.in_of_pie_chart {
                st.in_ser_lines = true;
            }
        }
        b"holeSize" => {
            if st.in_doughnut_chart && chart.doughnut_hole_size.is_none() {
                chart.doughnut_hole_size = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"barDir" => {
            if let Some(val) = attr_val(e, "val") {
                st.bar_dir = match val.as_str() {
                    "bar" => Some(BarDir::Bar),
                    "col" => Some(BarDir::Col),
                    _ => None,
                };
            }
        }
        b"scatterStyle" => {
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Scatter)) {
                if let Some(val) = attr_val(e, "val") {
                    chart.scatter_style = scatter_style(&val);
                }
            }
        }
        b"trendline" => {
            if st.cur_series.is_some()
                && matches!(
                    st.cur_plot_type,
                    Some(OoxmlChartType::Line | OoxmlChartType::Scatter)
                )
            {
                st.in_trendline = true;
            }
        }
        b"trendlineType" => {
            if st.in_trendline && chart.trendline_type.is_none() {
                chart.trendline_type = attr_val(e, "val").and_then(|val| trendline_type(&val));
            }
        }
        b"order" => {
            if st.in_trendline && chart.trendline_order.is_none() {
                chart.trendline_order = attr_val(e, "val").and_then(|val| val.parse::<u32>().ok());
            }
        }
        b"period" => {
            if st.in_trendline && chart.trendline_period.is_none() {
                chart.trendline_period = attr_val(e, "val").and_then(|val| val.parse::<u32>().ok());
            }
        }
        b"dispEq" => {
            if st.in_trendline && chart.trendline_display_equation.is_none() {
                chart.trendline_display_equation = attr_ooxml_bool(e);
            }
        }
        b"dispRSqr" => {
            if st.in_trendline && chart.trendline_display_r_squared.is_none() {
                chart.trendline_display_r_squared = attr_ooxml_bool(e);
            }
        }
        b"errBars" => {
            if st.cur_series.is_some()
                && matches!(
                    st.cur_plot_type,
                    Some(OoxmlChartType::Line | OoxmlChartType::Scatter)
                )
            {
                st.in_error_bars = true;
            }
        }
        b"errDir" => {
            if st.in_error_bars && chart.error_bar_direction.is_none() {
                chart.error_bar_direction =
                    attr_val(e, "val").and_then(|val| error_bar_direction(&val));
            }
        }
        b"errBarType" => {
            if st.in_error_bars && chart.error_bar_type.is_none() {
                chart.error_bar_type = attr_val(e, "val").and_then(|val| error_bar_type(&val));
            }
        }
        b"errValType" => {
            if st.in_error_bars && chart.error_bar_value_type.is_none() {
                chart.error_bar_value_type =
                    attr_val(e, "val").and_then(|val| error_bar_value_type(&val));
            }
        }
        b"noEndCap" => {
            if st.in_error_bars && chart.error_bar_no_end_cap.is_none() {
                chart.error_bar_no_end_cap = attr_ooxml_bool(e);
            }
        }
        b"grouping" => {
            // 막대(bar/bar3D)와 라인(line) plot의 grouping을 채택한다.
            if matches!(
                st.cur_plot_type,
                Some(OoxmlChartType::Column | OoxmlChartType::Bar | OoxmlChartType::Line)
            ) {
                if let Some(val) = attr_val(e, "val") {
                    chart.grouping = match val.as_str() {
                        "stacked" => BarGrouping::Stacked,
                        "percentStacked" => BarGrouping::PercentStacked,
                        _ => BarGrouping::Clustered,
                    };
                }
            }
        }
        b"gapWidth" => {
            if st.in_of_pie_chart && chart.pie_of_pie_gap_width.is_none() {
                chart.pie_of_pie_gap_width = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
            if st.in_up_down_bars && chart.stock_up_down_bar_gap_width.is_none() {
                chart.stock_up_down_bar_gap_width =
                    attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
            if matches!(
                st.cur_plot_type,
                Some(OoxmlChartType::Column | OoxmlChartType::Bar)
            ) && chart.bar_gap_width.is_none()
            {
                chart.bar_gap_width = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"secondPieSize" => {
            if st.in_of_pie_chart && chart.pie_of_pie_second_size.is_none() {
                chart.pie_of_pie_second_size = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"overlap" => {
            if matches!(
                st.cur_plot_type,
                Some(OoxmlChartType::Column | OoxmlChartType::Bar)
            ) && chart.bar_overlap.is_none()
            {
                chart.bar_overlap = attr_val(e, "val").and_then(|val| parse_i32(&val));
            }
        }
        b"gapDepth" => {
            if st.in_bar_3d_chart && chart.bar_3d_gap_depth.is_none() {
                chart.bar_3d_gap_depth = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"shape" => {
            if st.in_bar_3d_chart && chart.bar_3d_shape.is_none() {
                chart.bar_3d_shape = attr_val(e, "val");
            }
        }
        b"upDownBars" => st.in_up_down_bars = true,
        b"hiLowLines" => {
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Stock)) {
                st.in_hi_low_lines = true;
            }
        }
        b"dLbls" => st.in_data_labels = true,
        b"dLbl" => {
            if st.in_data_labels {
                st.in_data_label_point = true;
            }
        }
        b"dLblPos" => {
            if st.in_data_labels && !st.in_data_label_point && chart.data_label_position.is_none() {
                chart.data_label_position =
                    attr_val(e, "val").and_then(|val| data_label_position(&val));
            }
        }
        b"showVal" => {
            if st.in_data_labels
                && !st.in_data_label_point
                && chart.data_labels_show_value.is_none()
            {
                chart.data_labels_show_value = attr_ooxml_bool(e);
            }
        }
        b"showCatName" => {
            if st.in_data_labels
                && !st.in_data_label_point
                && chart.data_labels_show_category_name.is_none()
            {
                chart.data_labels_show_category_name = attr_ooxml_bool(e);
            }
        }
        b"showSerName" => {
            if st.in_data_labels
                && !st.in_data_label_point
                && chart.data_labels_show_series_name.is_none()
            {
                chart.data_labels_show_series_name = attr_ooxml_bool(e);
            }
        }
        b"showPercent" => {
            if st.in_data_labels
                && !st.in_data_label_point
                && chart.data_labels_show_percent.is_none()
            {
                chart.data_labels_show_percent = attr_ooxml_bool(e);
            }
        }
        b"showLegendKey" => {
            if st.in_data_labels
                && !st.in_data_label_point
                && chart.data_labels_show_legend_key.is_none()
            {
                chart.data_labels_show_legend_key = attr_ooxml_bool(e);
            }
        }
        b"overlay" => {
            if st.in_chart_title && chart.title_overlay.is_none() {
                chart.title_overlay = attr_ooxml_bool(e);
            } else if st.in_legend && chart.legend_overlay.is_none() {
                chart.legend_overlay = attr_ooxml_bool(e);
            }
        }
        b"date1904" => {
            if chart.date_1904.is_none() {
                chart.date_1904 = attr_ooxml_bool(e);
            }
        }
        b"style" => {
            if let Some(value) = attr_val(e, "val").and_then(|val| parse_u32(&val)) {
                chart.chart_style = Some(normalize_chart_style(value));
            }
        }
        b"roundedCorners" => {
            if chart.rounded_corners.is_none() {
                chart.rounded_corners = attr_ooxml_bool(e);
            }
        }
        b"autoTitleDeleted" => {
            if chart.auto_title_deleted.is_none() {
                chart.auto_title_deleted = attr_ooxml_bool(e);
            }
        }
        b"varyColors" => {
            if st.cur_plot_type.is_some() && chart.vary_colors.is_none() {
                chart.vary_colors = attr_ooxml_bool(e);
            }
        }
        b"rAngAx" => {
            if st.in_view_3d && chart.view_3d_right_angle_axes.is_none() {
                chart.view_3d_right_angle_axes = attr_ooxml_bool(e);
            }
        }
        b"rotX" => {
            if st.in_view_3d && chart.view_3d_rotation_x.is_none() {
                chart.view_3d_rotation_x = attr_val(e, "val").and_then(|val| parse_i32(&val));
            }
        }
        b"rotY" => {
            if st.in_view_3d && chart.view_3d_rotation_y.is_none() {
                chart.view_3d_rotation_y = attr_val(e, "val").and_then(|val| parse_i32(&val));
            }
        }
        b"perspective" => {
            if st.in_view_3d && chart.view_3d_perspective.is_none() {
                chart.view_3d_perspective = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"hPercent" => {
            if st.in_view_3d && chart.view_3d_height_percent.is_none() {
                chart.view_3d_height_percent = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"depthPercent" => {
            if st.in_view_3d && chart.view_3d_depth_percent.is_none() {
                chart.view_3d_depth_percent = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"dispBlanksAs" => {
            if chart.display_blanks_as.is_none() {
                chart.display_blanks_as =
                    attr_val(e, "val").and_then(|val| display_blanks_as(&val));
            }
        }
        b"showHiddenData" => {
            if chart.show_hidden_data.is_none() {
                chart.show_hidden_data = attr_ooxml_bool(e);
            }
        }
        b"plotVisOnly" => {
            if chart.plot_visible_only.is_none() {
                chart.plot_visible_only = attr_ooxml_bool(e);
            }
        }
        b"showHorzBorder" => {
            if st.in_data_table && chart.data_table_show_horizontal_border.is_none() {
                chart.data_table_show_horizontal_border = attr_ooxml_bool(e);
            }
        }
        b"showVertBorder" => {
            if st.in_data_table && chart.data_table_show_vertical_border.is_none() {
                chart.data_table_show_vertical_border = attr_ooxml_bool(e);
            }
        }
        b"showOutline" => {
            if st.in_data_table && chart.data_table_show_outline.is_none() {
                chart.data_table_show_outline = attr_ooxml_bool(e);
            }
        }
        b"showKeys" => {
            if st.in_data_table && chart.data_table_show_keys.is_none() {
                chart.data_table_show_keys = attr_ooxml_bool(e);
            }
        }
        b"upBars" => {
            if st.in_up_down_bars {
                st.stock_bar_kind = Some(StockBarKind::Up);
            }
        }
        b"downBars" => {
            if st.in_up_down_bars {
                st.stock_bar_kind = Some(StockBarKind::Down);
            }
        }
        b"marker" => {
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Line))
                && st.cur_series.is_none()
                && chart.line_marker_visible.is_none()
            {
                chart.line_marker_visible = attr_ooxml_bool(e);
            }
            st.in_marker = true;
        }
        b"symbol" => {
            if st.in_marker && st.cur_series.is_some() {
                if let Some(symbol) = attr_val(e, "val").and_then(|val| marker_symbol(&val)) {
                    match st.cur_plot_type {
                        Some(OoxmlChartType::Line) if chart.line_marker_symbol.is_none() => {
                            chart.line_marker_symbol = Some(symbol);
                        }
                        Some(OoxmlChartType::Scatter) if chart.scatter_marker_symbol.is_none() => {
                            chart.scatter_marker_symbol = Some(symbol);
                        }
                        _ => {}
                    }
                }
            }
        }
        b"size" => {
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Line))
                && st.in_marker
                && st.cur_series.is_some()
                && chart.line_marker_size.is_none()
            {
                chart.line_marker_size = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Scatter))
                && st.in_marker
                && st.cur_series.is_some()
                && chart.scatter_marker_size.is_none()
            {
                chart.scatter_marker_size = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"smooth" => {
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Line)) && chart.line_smooth.is_none()
            {
                chart.line_smooth = attr_val(e, "val").map(|val| val != "0");
            }
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Scatter))
                && chart.scatter_smooth.is_none()
            {
                chart.scatter_smooth = attr_val(e, "val").map(|val| val != "0");
            }
        }
        b"firstSliceAng" => {
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Pie))
                && chart.pie_first_slice_angle.is_none()
            {
                chart.pie_first_slice_angle = attr_val(e, "val").and_then(|val| parse_u16(&val));
            }
        }
        b"explosion" => {
            if matches!(st.cur_plot_type, Some(OoxmlChartType::Pie))
                && st.cur_series.is_some()
                && chart.pie_explosion.is_none()
            {
                chart.pie_explosion = attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"legend" => st.in_legend = true,
        b"legendPos" => {
            if st.in_legend {
                if let Some(val) = attr_val(e, "val") {
                    chart.legend_position = match val.as_str() {
                        "r" => Some(ChartLegendPosition::Right),
                        "l" => Some(ChartLegendPosition::Left),
                        "t" => Some(ChartLegendPosition::Top),
                        "b" => Some(ChartLegendPosition::Bottom),
                        "tr" => Some(ChartLegendPosition::TopRight),
                        _ => None,
                    };
                }
            }
        }
        b"delete" => {
            if st.in_cat_ax || st.in_val_ax {
                let visible = attr_val(e, "val").as_deref() != Some("1");
                if st.in_cat_ax {
                    st.cur_cat_ax_visible = Some(visible);
                }
                if st.in_val_ax {
                    st.cur_val_ax_visible = Some(visible);
                }
            }
        }
        b"tickLblPos" => {
            if st.in_cat_ax || st.in_val_ax {
                if let Some(val) = attr_val(e, "val").and_then(|val| axis_label_position(&val)) {
                    if st.in_cat_ax && chart.category_axis_label_position.is_none() {
                        chart.category_axis_label_position = Some(val);
                    }
                    if st.in_val_ax && chart.value_axis_label_position.is_none() {
                        chart.value_axis_label_position = Some(val);
                    }
                }
            }
        }
        b"auto" => {
            if st.in_cat_ax && chart.category_axis_auto.is_none() {
                chart.category_axis_auto = attr_ooxml_bool(e);
            }
        }
        b"lblAlgn" => {
            if st.in_cat_ax && chart.category_axis_label_alignment.is_none() {
                chart.category_axis_label_alignment =
                    attr_val(e, "val").and_then(|val| axis_label_alignment(&val));
            }
        }
        b"lblOffset" => {
            if st.in_cat_ax && chart.category_axis_label_offset.is_none() {
                chart.category_axis_label_offset =
                    attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"tickMarkSkip" => {
            if st.in_cat_ax && chart.category_axis_tick_mark_skip.is_none() {
                chart.category_axis_tick_mark_skip =
                    attr_val(e, "val").and_then(|val| parse_u32(&val));
            }
        }
        b"noMultiLvlLbl" => {
            if st.in_cat_ax && chart.category_axis_no_multi_level_labels.is_none() {
                chart.category_axis_no_multi_level_labels = attr_ooxml_bool(e);
            }
        }
        b"orientation" => {
            if st.in_cat_ax || st.in_val_ax {
                if let Some(val) = attr_val(e, "val").and_then(|val| axis_orientation(&val)) {
                    if st.in_cat_ax && chart.category_axis_orientation.is_none() {
                        chart.category_axis_orientation = Some(val);
                    }
                    if st.in_val_ax && chart.value_axis_orientation.is_none() {
                        chart.value_axis_orientation = Some(val);
                    }
                }
            }
        }
        b"crossBetween" => {
            if st.in_val_ax && chart.value_axis_cross_between.is_none() {
                chart.value_axis_cross_between =
                    attr_val(e, "val").and_then(|val| axis_cross_between(&val));
            }
        }
        b"crosses" => {
            if st.in_cat_ax || st.in_val_ax {
                if let Some(val) = attr_val(e, "val").and_then(|val| axis_crosses(&val)) {
                    if st.in_cat_ax && chart.category_axis_crosses.is_none() {
                        chart.category_axis_crosses = Some(val);
                    }
                    if st.in_val_ax && chart.value_axis_crosses.is_none() {
                        chart.value_axis_crosses = Some(val);
                    }
                }
            }
        }
        b"crossesAt" => {
            if st.in_cat_ax && chart.category_axis_crosses_at.is_none() {
                chart.category_axis_crosses_at =
                    attr_val(e, "val").and_then(|val| parse_finite_f64(&val));
            }
            if st.in_val_ax && chart.value_axis_crosses_at.is_none() {
                chart.value_axis_crosses_at =
                    attr_val(e, "val").and_then(|val| parse_finite_f64(&val));
            }
        }
        b"majorTickMark" => {
            if st.in_cat_ax || st.in_val_ax {
                if let Some(val) = attr_val(e, "val").and_then(|val| axis_tick_mark(&val)) {
                    if st.in_cat_ax && chart.category_axis_major_tick_mark.is_none() {
                        chart.category_axis_major_tick_mark = Some(val);
                    }
                    if st.in_val_ax && chart.value_axis_major_tick_mark.is_none() {
                        chart.value_axis_major_tick_mark = Some(val);
                    }
                }
            }
        }
        b"minorTickMark" => {
            if st.in_cat_ax || st.in_val_ax {
                if let Some(val) = attr_val(e, "val").and_then(|val| axis_tick_mark(&val)) {
                    if st.in_cat_ax && chart.category_axis_minor_tick_mark.is_none() {
                        chart.category_axis_minor_tick_mark = Some(val);
                    }
                    if st.in_val_ax && chart.value_axis_minor_tick_mark.is_none() {
                        chart.value_axis_minor_tick_mark = Some(val);
                    }
                }
            }
        }
        b"majorGridlines" => {
            st.axis_gridlines = if st.in_cat_ax {
                Some(AxisGridLineKind::CategoryMajor)
            } else if st.in_val_ax {
                Some(AxisGridLineKind::ValueMajor)
            } else {
                None
            };
        }
        b"minorGridlines" => {
            st.axis_gridlines = if st.in_cat_ax {
                Some(AxisGridLineKind::CategoryMinor)
            } else if st.in_val_ax {
                Some(AxisGridLineKind::ValueMinor)
            } else {
                None
            };
        }
        b"numFmt" => {
            if st.in_cat_ax {
                if chart.category_axis_number_format.is_none() {
                    chart.category_axis_number_format = attr_val(e, "formatCode");
                }
                if chart.category_axis_number_format_source_linked.is_none() {
                    chart.category_axis_number_format_source_linked =
                        attr_val(e, "sourceLinked").and_then(|val| parse_ooxml_bool(&val));
                }
            }
            if st.in_val_ax {
                if chart.value_axis_number_format.is_none() {
                    chart.value_axis_number_format = attr_val(e, "formatCode");
                }
                if chart.value_axis_number_format_source_linked.is_none() {
                    chart.value_axis_number_format_source_linked =
                        attr_val(e, "sourceLinked").and_then(|val| parse_ooxml_bool(&val));
                }
            }
        }
        b"scaling" => {
            if st.in_val_ax {
                st.in_val_ax_scaling = true;
            }
        }
        b"logBase" => {
            if st.in_val_ax && st.in_val_ax_scaling && chart.value_axis_log_base.is_none() {
                chart.value_axis_log_base =
                    attr_val(e, "val").and_then(|val| parse_finite_f64(&val));
            }
        }
        b"min" => {
            if st.in_val_ax && st.in_val_ax_scaling && chart.value_axis_minimum.is_none() {
                chart.value_axis_minimum =
                    attr_val(e, "val").and_then(|val| parse_finite_f64(&val));
            }
        }
        b"max" => {
            if st.in_val_ax && st.in_val_ax_scaling && chart.value_axis_maximum.is_none() {
                chart.value_axis_maximum =
                    attr_val(e, "val").and_then(|val| parse_finite_f64(&val));
            }
        }
        b"majorUnit" => {
            if st.in_val_ax && chart.value_axis_major_unit.is_none() {
                chart.value_axis_major_unit =
                    attr_val(e, "val").and_then(|val| parse_finite_f64(&val));
            }
        }
        b"minorUnit" => {
            if st.in_val_ax && chart.value_axis_minor_unit.is_none() {
                chart.value_axis_minor_unit =
                    attr_val(e, "val").and_then(|val| parse_finite_f64(&val));
            }
        }
        b"dispUnits" => {
            if st.in_val_ax {
                st.in_val_ax_display_units = true;
            }
        }
        b"builtInUnit" => {
            if st.in_val_ax && st.in_val_ax_display_units && chart.value_axis_display_unit.is_none()
            {
                chart.value_axis_display_unit =
                    attr_val(e, "val").and_then(|val| axis_display_unit(&val));
            }
        }
        b"ser" => {
            let mut ser = OoxmlSeries::default();
            if let Some(t) = st.cur_plot_type {
                ser.series_type = t;
            }
            st.cur_series = Some(ser);
        }
        b"tx" => st.in_tx = true,
        b"cat" | b"xVal" => st.in_cat = true,
        b"val" if st.in_error_bars => {
            if chart.error_bar_value.is_none() {
                chart.error_bar_value = attr_val(e, "val").and_then(|val| val.parse::<f64>().ok());
            }
        }
        b"val" | b"yVal" => st.in_val = true,
        b"title" => {
            st.in_chart_title = !st.in_plot_area && !st.in_cat_ax && !st.in_val_ax;
            st.in_axis_title = if st.in_cat_ax {
                Some(AxisKind::Category)
            } else if st.in_val_ax {
                Some(AxisKind::Value)
            } else {
                None
            };
        }
        b"view3D" => st.in_view_3d = true,
        b"v" => {
            st.in_v = true;
            st.cur_text_buf.clear();
        }
        b"t" => {
            st.in_a_t = true;
            st.cur_text_buf.clear();
        }
        b"spPr" => {
            st.in_sp_pr = true;
            if st.in_marker {
                st.marker_sp_pr = true;
            } else if st.in_hi_low_lines {
                st.hi_low_lines_sp_pr = true;
            } else if st.in_ser_lines {
                st.ser_lines_sp_pr = true;
            } else if st.in_trendline {
                st.trendline_sp_pr = true;
            } else if st.in_error_bars {
                st.error_bar_sp_pr = true;
            } else if let Some(kind) = st.stock_bar_kind {
                st.stock_bar_sp_pr = Some(kind);
            } else if let Some(kind) = st.axis_gridlines {
                st.axis_gridline_sp_pr = Some(kind);
            } else if st.in_cat_ax {
                st.axis_sp_pr = Some(AxisKind::Category);
            } else if st.in_val_ax {
                st.axis_sp_pr = Some(AxisKind::Value);
            } else if st.in_plot_area && st.cur_plot_type.is_none() && st.cur_series.is_none() {
                st.in_plot_area_sp_pr = true;
            } else if !st.in_chart_title
                && !st.in_legend
                && !st.in_view_3d
                && !st.in_plot_area
                && st.cur_plot_type.is_none()
                && st.cur_series.is_none()
            {
                st.in_chart_space_sp_pr = true;
            }
        }
        b"solidFill" => st.in_solid_fill = true,
        b"ln" => {
            st.in_ln = true;
            if st.marker_sp_pr {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    set_marker_line_width(chart, st.cur_plot_type, width);
                }
            } else if st.trendline_sp_pr {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    if chart.trendline_line_width.is_none() {
                        chart.trendline_line_width = Some(width);
                    }
                }
            } else if st.error_bar_sp_pr {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    if chart.error_bar_line_width.is_none() {
                        chart.error_bar_line_width = Some(width);
                    }
                }
            } else if st.hi_low_lines_sp_pr {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    if chart.stock_hi_low_line_width.is_none() {
                        chart.stock_hi_low_line_width = Some(width);
                    }
                }
            } else if st.ser_lines_sp_pr {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    if chart.pie_of_pie_ser_line_width.is_none() {
                        chart.pie_of_pie_ser_line_width = Some(width);
                    }
                }
            } else if let Some(kind) = st.stock_bar_sp_pr {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    set_stock_bar_line_width(chart, kind, width);
                }
            } else if let Some(kind) = st.axis_gridline_sp_pr {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    set_axis_grid_line_width(chart, kind, width);
                }
            } else if let Some(axis) = st.axis_sp_pr {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    match axis {
                        AxisKind::Category if chart.category_axis_line_width.is_none() => {
                            chart.category_axis_line_width = Some(width);
                        }
                        AxisKind::Value if chart.value_axis_line_width.is_none() => {
                            chart.value_axis_line_width = Some(width);
                        }
                        _ => {}
                    }
                }
            } else if is_plain_series_sp_pr(st) {
                if let Some(width) = attr_val(e, "w").and_then(|val| parse_u32(&val)) {
                    if let Some(ser) = st.cur_series.as_mut() {
                        if ser.line_width.is_none() {
                            ser.line_width = Some(width);
                        }
                    }
                }
            }
        }
        b"srgbClr" => {
            if st.in_chart_space_sp_pr && st.in_solid_fill && !st.in_ln {
                if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                    if chart.chart_area_fill_color.is_none() {
                        chart.chart_area_fill_color = Some(rgb);
                    }
                }
            } else if st.in_plot_area_sp_pr && st.in_solid_fill && !st.in_ln {
                if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                    if chart.plot_area_fill_color.is_none() {
                        chart.plot_area_fill_color = Some(rgb);
                    }
                }
            } else if st.marker_sp_pr {
                if st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                        if st.in_ln {
                            set_marker_line_color(chart, st.cur_plot_type, rgb);
                        } else {
                            set_marker_fill_color(chart, st.cur_plot_type, rgb);
                        }
                    }
                }
            } else if st.trendline_sp_pr {
                if st.in_ln && st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                        if chart.trendline_line_color.is_none() {
                            chart.trendline_line_color = Some(rgb);
                        }
                    }
                }
            } else if st.error_bar_sp_pr {
                if st.in_ln && st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                        if chart.error_bar_line_color.is_none() {
                            chart.error_bar_line_color = Some(rgb);
                        }
                    }
                }
            } else if st.hi_low_lines_sp_pr {
                if st.in_ln && st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                        if chart.stock_hi_low_line_color.is_none() {
                            chart.stock_hi_low_line_color = Some(rgb);
                        }
                    }
                }
            } else if st.ser_lines_sp_pr {
                if st.in_ln && st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                        if chart.pie_of_pie_ser_line_color.is_none() {
                            chart.pie_of_pie_ser_line_color = Some(rgb);
                        }
                    }
                }
            } else if let Some(kind) = st.stock_bar_sp_pr {
                if st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                        if st.in_ln {
                            set_stock_bar_line_color(chart, kind, rgb);
                        } else {
                            set_stock_bar_fill_color(chart, kind, rgb);
                        }
                    }
                }
            } else if st.in_ln && st.in_solid_fill {
                if let Some(rgb) = attr_val(e, "val").and_then(|val| parse_rgb_hex(&val)) {
                    if let Some(kind) = st.axis_gridline_sp_pr {
                        set_axis_grid_line_color(chart, kind, rgb);
                    } else if let Some(axis) = st.axis_sp_pr {
                        set_axis_line_color(chart, axis, rgb);
                    }
                }
            }
            if is_plain_series_sp_pr(st) && (st.in_solid_fill || st.in_ln) {
                if let Some(val) = attr_val(e, "val") {
                    if let Some(rgb) = parse_rgb_hex(&val) {
                        if let Some(ser) = st.cur_series.as_mut() {
                            if st.in_ln {
                                if ser.line_color.is_none() {
                                    ser.line_color = Some(rgb);
                                }
                            } else if ser.color.is_none() {
                                ser.color = Some(rgb);
                            }
                        }
                    }
                }
            }
        }
        b"schemeClr" => {
            if st.in_chart_space_sp_pr && st.in_solid_fill && !st.in_ln {
                if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                    if chart.chart_area_fill_color.is_none() {
                        chart.chart_area_fill_color = Some(rgb);
                    }
                }
            } else if st.in_plot_area_sp_pr && st.in_solid_fill && !st.in_ln {
                if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                    if chart.plot_area_fill_color.is_none() {
                        chart.plot_area_fill_color = Some(rgb);
                    }
                }
            } else if st.marker_sp_pr {
                if st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                        if st.in_ln {
                            set_marker_line_color(chart, st.cur_plot_type, rgb);
                        } else {
                            set_marker_fill_color(chart, st.cur_plot_type, rgb);
                        }
                    }
                }
            } else if st.trendline_sp_pr {
                if st.in_ln && st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                        if chart.trendline_line_color.is_none() {
                            chart.trendline_line_color = Some(rgb);
                        }
                    }
                }
            } else if st.error_bar_sp_pr {
                if st.in_ln && st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                        if chart.error_bar_line_color.is_none() {
                            chart.error_bar_line_color = Some(rgb);
                        }
                    }
                }
            } else if st.hi_low_lines_sp_pr {
                if st.in_ln && st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                        if chart.stock_hi_low_line_color.is_none() {
                            chart.stock_hi_low_line_color = Some(rgb);
                        }
                    }
                }
            } else if st.ser_lines_sp_pr {
                if st.in_ln && st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                        if chart.pie_of_pie_ser_line_color.is_none() {
                            chart.pie_of_pie_ser_line_color = Some(rgb);
                        }
                    }
                }
            } else if let Some(kind) = st.stock_bar_sp_pr {
                if st.in_solid_fill {
                    if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                        if st.in_ln {
                            set_stock_bar_line_color(chart, kind, rgb);
                        } else {
                            set_stock_bar_fill_color(chart, kind, rgb);
                        }
                    }
                }
            } else if st.in_ln && st.in_solid_fill {
                if let Some(rgb) = attr_val(e, "val").and_then(|val| scheme_color(&val)) {
                    if let Some(kind) = st.axis_gridline_sp_pr {
                        set_axis_grid_line_color(chart, kind, rgb);
                    } else if let Some(axis) = st.axis_sp_pr {
                        set_axis_line_color(chart, axis, rgb);
                    }
                }
            }
            if is_plain_series_sp_pr(st) && (st.in_solid_fill || st.in_ln) {
                if let Some(val) = attr_val(e, "val") {
                    if let Some(rgb) = scheme_color(&val) {
                        if let Some(ser) = st.cur_series.as_mut() {
                            if st.in_ln {
                                if ser.line_color.is_none() {
                                    ser.line_color = Some(rgb);
                                }
                            } else if ser.color.is_none() {
                                ser.color = Some(rgb);
                            }
                        }
                    }
                }
            }
        }
        b"numCache" => st.in_num_cache = true,
        b"formatCode" => {
            // <c:formatCode>#,##0</c:formatCode> — 텍스트 노드로 옴
            st.cur_text_buf.clear();
            st.in_v = true; // 텍스트 누적 플래그 재활용 (handle_end에서 분기)
        }
        b"axId" => {
            if let Some(val) = attr_val(e, "val") {
                if st.in_val_ax {
                    st.cur_val_ax_id = Some(val.clone());
                } else if st.cur_plot_type.is_some() {
                    st.cur_plot_ax_ids.push(val);
                }
            }
        }
        b"axPos" => {
            if st.in_cat_ax || st.in_val_ax {
                if let Some(val) = attr_val(e, "val") {
                    if let Some(position) = axis_position(&val) {
                        if st.in_cat_ax && chart.category_axis_position.is_none() {
                            chart.category_axis_position = Some(position);
                        }
                        if st.in_val_ax && chart.value_axis_position.is_none() {
                            chart.value_axis_position = Some(position);
                        }
                    }
                    if st.in_val_ax {
                        st.cur_val_ax_pos = Some(val);
                    }
                }
            }
        }
        b"catAx" => {
            st.in_cat_ax = true;
            st.cur_cat_ax_visible = None;
        }
        b"valAx" => {
            st.in_val_ax = true;
            st.cur_val_ax_id = None;
            st.cur_val_ax_pos = None;
            st.cur_val_ax_visible = None;
        }
        _ => {}
    }
}

fn handle_end(name: &[u8], chart: &mut OoxmlChart, st: &mut ParseState) {
    match name {
        b"v" => {
            st.in_v = false;
            let text = std::mem::take(&mut st.cur_text_buf);
            if let Some(ser) = st.cur_series.as_mut() {
                if st.in_tx {
                    if ser.name.is_empty() {
                        ser.name = text;
                    }
                } else if st.in_cat {
                    if chart.series.is_empty() {
                        chart.categories.push(text);
                    }
                } else if st.in_val {
                    if let Ok(v) = text.parse::<f64>() {
                        ser.values.push(v);
                    } else {
                        ser.values.push(0.0);
                    }
                }
            }
        }
        b"formatCode" => {
            st.in_v = false;
            let text = std::mem::take(&mut st.cur_text_buf);
            if !text.is_empty() {
                if let Some(ser) = st.cur_series.as_mut() {
                    if ser.format_code.is_none() {
                        ser.format_code = Some(text);
                    }
                }
            }
        }
        b"t" => {
            st.in_a_t = false;
            let text = std::mem::take(&mut st.cur_text_buf);
            if !text.is_empty() {
                if st.in_chart_title {
                    match chart.title.as_mut() {
                        Some(s) => s.push_str(&text),
                        None => chart.title = Some(text),
                    }
                } else if let Some(axis) = st.in_axis_title {
                    let target = match axis {
                        AxisKind::Category => &mut chart.category_axis_title,
                        AxisKind::Value => &mut chart.value_axis_title,
                    };
                    match target.as_mut() {
                        Some(s) => s.push_str(&text),
                        None => *target = Some(text),
                    }
                }
            }
        }
        b"tx" => st.in_tx = false,
        b"cat" | b"xVal" => st.in_cat = false,
        b"val" | b"yVal" => st.in_val = false,
        b"title" => {
            st.in_chart_title = false;
            st.in_axis_title = None;
        }
        b"legend" => st.in_legend = false,
        b"view3D" => st.in_view_3d = false,
        b"plotArea" => st.in_plot_area = false,
        b"dTable" => st.in_data_table = false,
        b"ser" => {
            if let Some(ser) = st.cur_series.take() {
                // axIds는 plot 종료 시 일괄 할당 (XML 구조상 axId가 ser 뒤에 옴)
                chart.series.push(ser);
            }
        }
        b"spPr" => {
            st.in_sp_pr = false;
            st.in_chart_space_sp_pr = false;
            st.in_plot_area_sp_pr = false;
            st.hi_low_lines_sp_pr = false;
            st.ser_lines_sp_pr = false;
            st.marker_sp_pr = false;
            st.trendline_sp_pr = false;
            st.error_bar_sp_pr = false;
            st.stock_bar_sp_pr = None;
            st.axis_sp_pr = None;
            st.axis_gridline_sp_pr = None;
        }
        b"solidFill" => st.in_solid_fill = false,
        b"ln" => st.in_ln = false,
        b"hiLowLines" => st.in_hi_low_lines = false,
        b"serLines" => st.in_ser_lines = false,
        b"upBars" | b"downBars" => st.stock_bar_kind = None,
        b"upDownBars" => st.in_up_down_bars = false,
        b"dLbl" => st.in_data_label_point = false,
        b"dLbls" => st.in_data_labels = false,
        b"majorGridlines" | b"minorGridlines" => st.axis_gridlines = None,
        b"marker" => st.in_marker = false,
        b"trendline" => st.in_trendline = false,
        b"errBars" => st.in_error_bars = false,
        b"numCache" => st.in_num_cache = false,
        b"scaling" => st.in_val_ax_scaling = false,
        b"dispUnits" => st.in_val_ax_display_units = false,
        b"catAx" => {
            st.in_cat_ax = false;
            let visible = st.cur_cat_ax_visible.take().unwrap_or(true);
            chart.category_axis_visible =
                Some(chart.category_axis_visible.unwrap_or(false) || visible);
        }
        b"valAx" => {
            st.in_val_ax = false;
            let visible = st.cur_val_ax_visible.take().unwrap_or(true);
            chart.value_axis_visible = Some(chart.value_axis_visible.unwrap_or(false) || visible);
            if let (Some(id), Some(pos)) = (st.cur_val_ax_id.take(), st.cur_val_ax_pos.take()) {
                st.val_ax_map.insert(id, pos);
            } else if let Some(id) = st.cur_val_ax_id.take() {
                st.val_ax_map.insert(id, String::new());
                st.cur_val_ax_pos = None;
            }
        }
        b"barChart" | b"lineChart" | b"pieChart" | b"scatterChart" | b"stockChart"
        | b"bar3DChart" | b"pie3DChart" | b"doughnutChart" | b"ofPieChart" => {
            if name == b"ofPieChart" {
                st.in_of_pie_chart = false;
            }
            if name == b"doughnutChart" {
                st.in_doughnut_chart = false;
            }
            if name == b"bar3DChart" {
                st.in_bar_3d_chart = false;
            }
            // plot 종료 — 이 plot에 속한 시리즈에 axIds 복사
            let start = st.cur_plot_series_start;
            for ser in chart.series.iter_mut().skip(start) {
                ser.axis_ids = st.cur_plot_ax_ids.clone();
            }
            st.cur_plot_type = None;
            st.cur_plot_ax_ids.clear();
        }
        _ => {}
    }
}

fn attr_val(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == key.as_bytes() {
            return Some(String::from_utf8_lossy(attr.value.as_ref()).to_string());
        }
    }
    None
}

fn axis_label_position(value: &str) -> Option<AxisLabelPosition> {
    match value {
        "nextTo" => Some(AxisLabelPosition::NextTo),
        "high" => Some(AxisLabelPosition::High),
        "low" => Some(AxisLabelPosition::Low),
        "none" => Some(AxisLabelPosition::None),
        _ => None,
    }
}

fn axis_position(value: &str) -> Option<AxisPosition> {
    match value {
        "b" => Some(AxisPosition::Bottom),
        "l" => Some(AxisPosition::Left),
        "t" => Some(AxisPosition::Top),
        "r" => Some(AxisPosition::Right),
        _ => None,
    }
}

fn axis_label_alignment(value: &str) -> Option<AxisLabelAlignment> {
    match value {
        "ctr" => Some(AxisLabelAlignment::Center),
        "l" => Some(AxisLabelAlignment::Left),
        "r" => Some(AxisLabelAlignment::Right),
        _ => None,
    }
}

fn axis_orientation(value: &str) -> Option<AxisOrientation> {
    match value {
        "minMax" => Some(AxisOrientation::MinMax),
        "maxMin" => Some(AxisOrientation::MaxMin),
        _ => None,
    }
}

fn axis_cross_between(value: &str) -> Option<AxisCrossBetween> {
    match value {
        "between" => Some(AxisCrossBetween::Between),
        "midCat" => Some(AxisCrossBetween::MidCategory),
        _ => None,
    }
}

fn axis_crosses(value: &str) -> Option<AxisCrosses> {
    match value {
        "autoZero" => Some(AxisCrosses::AutoZero),
        "min" => Some(AxisCrosses::Min),
        "max" => Some(AxisCrosses::Max),
        _ => None,
    }
}

fn axis_tick_mark(value: &str) -> Option<AxisTickMark> {
    match value {
        "cross" => Some(AxisTickMark::Cross),
        "in" => Some(AxisTickMark::In),
        "out" => Some(AxisTickMark::Out),
        "none" => Some(AxisTickMark::None),
        _ => None,
    }
}

fn axis_display_unit(value: &str) -> Option<AxisDisplayUnit> {
    match value {
        "hundreds" => Some(AxisDisplayUnit::Hundreds),
        "thousands" => Some(AxisDisplayUnit::Thousands),
        "tenThousands" => Some(AxisDisplayUnit::TenThousands),
        "hundredThousands" => Some(AxisDisplayUnit::HundredThousands),
        "millions" => Some(AxisDisplayUnit::Millions),
        "tenMillions" => Some(AxisDisplayUnit::TenMillions),
        "hundredMillions" => Some(AxisDisplayUnit::HundredMillions),
        "billions" => Some(AxisDisplayUnit::Billions),
        "trillions" => Some(AxisDisplayUnit::Trillions),
        _ => None,
    }
}

fn data_label_position(value: &str) -> Option<ChartDataLabelPosition> {
    match value {
        "bestFit" => Some(ChartDataLabelPosition::BestFit),
        "b" => Some(ChartDataLabelPosition::Bottom),
        "ctr" => Some(ChartDataLabelPosition::Center),
        "inBase" => Some(ChartDataLabelPosition::InsideBase),
        "inEnd" => Some(ChartDataLabelPosition::InsideEnd),
        "l" => Some(ChartDataLabelPosition::Left),
        "outEnd" => Some(ChartDataLabelPosition::OutsideEnd),
        "r" => Some(ChartDataLabelPosition::Right),
        "t" => Some(ChartDataLabelPosition::Top),
        _ => None,
    }
}

fn display_blanks_as(value: &str) -> Option<ChartDisplayBlanksAs> {
    match value {
        "gap" => Some(ChartDisplayBlanksAs::Gap),
        "span" => Some(ChartDisplayBlanksAs::Span),
        "zero" => Some(ChartDisplayBlanksAs::Zero),
        _ => None,
    }
}

fn set_axis_line_color(chart: &mut OoxmlChart, axis: AxisKind, rgb: u32) {
    match axis {
        AxisKind::Category if chart.category_axis_line_color.is_none() => {
            chart.category_axis_line_color = Some(rgb);
        }
        AxisKind::Value if chart.value_axis_line_color.is_none() => {
            chart.value_axis_line_color = Some(rgb);
        }
        _ => {}
    }
}

fn set_axis_grid_line_width(chart: &mut OoxmlChart, kind: AxisGridLineKind, width: u32) {
    match kind {
        AxisGridLineKind::CategoryMajor if chart.category_axis_major_grid_line_width.is_none() => {
            chart.category_axis_major_grid_line_width = Some(width);
        }
        AxisGridLineKind::CategoryMinor if chart.category_axis_minor_grid_line_width.is_none() => {
            chart.category_axis_minor_grid_line_width = Some(width);
        }
        AxisGridLineKind::ValueMajor if chart.value_axis_major_grid_line_width.is_none() => {
            chart.value_axis_major_grid_line_width = Some(width);
        }
        AxisGridLineKind::ValueMinor if chart.value_axis_minor_grid_line_width.is_none() => {
            chart.value_axis_minor_grid_line_width = Some(width);
        }
        _ => {}
    }
}

fn set_axis_grid_line_color(chart: &mut OoxmlChart, kind: AxisGridLineKind, rgb: u32) {
    match kind {
        AxisGridLineKind::CategoryMajor if chart.category_axis_major_grid_line_color.is_none() => {
            chart.category_axis_major_grid_line_color = Some(rgb);
        }
        AxisGridLineKind::CategoryMinor if chart.category_axis_minor_grid_line_color.is_none() => {
            chart.category_axis_minor_grid_line_color = Some(rgb);
        }
        AxisGridLineKind::ValueMajor if chart.value_axis_major_grid_line_color.is_none() => {
            chart.value_axis_major_grid_line_color = Some(rgb);
        }
        AxisGridLineKind::ValueMinor if chart.value_axis_minor_grid_line_color.is_none() => {
            chart.value_axis_minor_grid_line_color = Some(rgb);
        }
        _ => {}
    }
}

fn set_stock_bar_fill_color(chart: &mut OoxmlChart, kind: StockBarKind, rgb: u32) {
    match kind {
        StockBarKind::Up if chart.stock_up_bar_fill_color.is_none() => {
            chart.stock_up_bar_fill_color = Some(rgb);
        }
        StockBarKind::Down if chart.stock_down_bar_fill_color.is_none() => {
            chart.stock_down_bar_fill_color = Some(rgb);
        }
        _ => {}
    }
}

fn set_stock_bar_line_color(chart: &mut OoxmlChart, kind: StockBarKind, rgb: u32) {
    match kind {
        StockBarKind::Up if chart.stock_up_bar_line_color.is_none() => {
            chart.stock_up_bar_line_color = Some(rgb);
        }
        StockBarKind::Down if chart.stock_down_bar_line_color.is_none() => {
            chart.stock_down_bar_line_color = Some(rgb);
        }
        _ => {}
    }
}

fn set_stock_bar_line_width(chart: &mut OoxmlChart, kind: StockBarKind, width: u32) {
    match kind {
        StockBarKind::Up if chart.stock_up_bar_line_width.is_none() => {
            chart.stock_up_bar_line_width = Some(width);
        }
        StockBarKind::Down if chart.stock_down_bar_line_width.is_none() => {
            chart.stock_down_bar_line_width = Some(width);
        }
        _ => {}
    }
}

fn set_marker_fill_color(chart: &mut OoxmlChart, plot_type: Option<OoxmlChartType>, rgb: u32) {
    match plot_type {
        Some(OoxmlChartType::Line) if chart.line_marker_fill_color.is_none() => {
            chart.line_marker_fill_color = Some(rgb);
        }
        Some(OoxmlChartType::Scatter) if chart.scatter_marker_fill_color.is_none() => {
            chart.scatter_marker_fill_color = Some(rgb);
        }
        _ => {}
    }
}

fn set_marker_line_color(chart: &mut OoxmlChart, plot_type: Option<OoxmlChartType>, rgb: u32) {
    match plot_type {
        Some(OoxmlChartType::Line) if chart.line_marker_line_color.is_none() => {
            chart.line_marker_line_color = Some(rgb);
        }
        Some(OoxmlChartType::Scatter) if chart.scatter_marker_line_color.is_none() => {
            chart.scatter_marker_line_color = Some(rgb);
        }
        _ => {}
    }
}

fn set_marker_line_width(chart: &mut OoxmlChart, plot_type: Option<OoxmlChartType>, width: u32) {
    match plot_type {
        Some(OoxmlChartType::Line) if chart.line_marker_line_width.is_none() => {
            chart.line_marker_line_width = Some(width);
        }
        Some(OoxmlChartType::Scatter) if chart.scatter_marker_line_width.is_none() => {
            chart.scatter_marker_line_width = Some(width);
        }
        _ => {}
    }
}

fn is_plain_series_sp_pr(st: &ParseState) -> bool {
    st.in_sp_pr
        && st.cur_series.is_some()
        && !st.in_marker
        && !st.marker_sp_pr
        && !st.in_trendline
        && !st.in_error_bars
        && !st.error_bar_sp_pr
        && !st.hi_low_lines_sp_pr
        && !st.ser_lines_sp_pr
        && st.stock_bar_sp_pr.is_none()
        && st.axis_gridline_sp_pr.is_none()
        && st.axis_sp_pr.is_none()
        && !st.in_chart_space_sp_pr
        && !st.in_plot_area_sp_pr
}

fn scatter_style(value: &str) -> Option<ScatterStyle> {
    match value {
        "line" => Some(ScatterStyle::Line),
        "lineMarker" => Some(ScatterStyle::LineMarker),
        "marker" => Some(ScatterStyle::Marker),
        "smooth" => Some(ScatterStyle::Smooth),
        "smoothMarker" => Some(ScatterStyle::SmoothMarker),
        _ => None,
    }
}

fn marker_symbol(value: &str) -> Option<ChartMarkerSymbol> {
    match value {
        "circle" => Some(ChartMarkerSymbol::Circle),
        "dash" => Some(ChartMarkerSymbol::Dash),
        "diamond" => Some(ChartMarkerSymbol::Diamond),
        "dot" => Some(ChartMarkerSymbol::Dot),
        "none" => Some(ChartMarkerSymbol::None),
        "picture" => Some(ChartMarkerSymbol::Picture),
        "plus" => Some(ChartMarkerSymbol::Plus),
        "square" => Some(ChartMarkerSymbol::Square),
        "star" => Some(ChartMarkerSymbol::Star),
        "triangle" => Some(ChartMarkerSymbol::Triangle),
        "x" => Some(ChartMarkerSymbol::X),
        _ => None,
    }
}

fn trendline_type(value: &str) -> Option<ChartTrendlineType> {
    match value {
        "linear" => Some(ChartTrendlineType::Linear),
        "exp" => Some(ChartTrendlineType::Exponential),
        "log" => Some(ChartTrendlineType::Logarithmic),
        "movingAvg" => Some(ChartTrendlineType::MovingAverage),
        "poly" => Some(ChartTrendlineType::Polynomial),
        "power" => Some(ChartTrendlineType::Power),
        _ => None,
    }
}

fn error_bar_direction(value: &str) -> Option<ChartErrorBarDirection> {
    match value {
        "x" => Some(ChartErrorBarDirection::X),
        "y" => Some(ChartErrorBarDirection::Y),
        _ => None,
    }
}

fn error_bar_type(value: &str) -> Option<ChartErrorBarType> {
    match value {
        "both" => Some(ChartErrorBarType::Both),
        "plus" => Some(ChartErrorBarType::Plus),
        "minus" => Some(ChartErrorBarType::Minus),
        _ => None,
    }
}

fn error_bar_value_type(value: &str) -> Option<ChartErrorBarValueType> {
    match value {
        "fixedVal" => Some(ChartErrorBarValueType::FixedValue),
        "percentage" => Some(ChartErrorBarValueType::Percentage),
        "stdDev" => Some(ChartErrorBarValueType::StandardDeviation),
        "stdErr" => Some(ChartErrorBarValueType::StandardError),
        _ => None,
    }
}

fn of_pie_type(value: &str) -> Option<OfPieType> {
    match value {
        "pie" => Some(OfPieType::Pie),
        "bar" => Some(OfPieType::Bar),
        _ => None,
    }
}

fn parse_finite_f64(value: &str) -> Option<f64> {
    let parsed = value.parse::<f64>().ok()?;
    if parsed.is_finite() {
        Some(parsed)
    } else {
        None
    }
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse::<u32>().ok()
}

fn normalize_chart_style(value: u32) -> u32 {
    if (101..=148).contains(&value) {
        value - 100
    } else {
        value
    }
}

fn parse_u16(value: &str) -> Option<u16> {
    value.parse::<u16>().ok()
}

fn parse_i32(value: &str) -> Option<i32> {
    value.parse::<i32>().ok()
}

fn parse_ooxml_bool(value: &str) -> Option<bool> {
    match value {
        "1" | "true" | "TRUE" => Some(true),
        "0" | "false" | "FALSE" => Some(false),
        _ => None,
    }
}

fn attr_ooxml_bool(e: &quick_xml::events::BytesStart) -> Option<bool> {
    attr_val(e, "val")
        .as_deref()
        .map(parse_ooxml_bool)
        .unwrap_or(Some(true))
}

fn parse_rgb_hex(s: &str) -> Option<u32> {
    let t = s.trim().trim_start_matches('#');
    if t.len() != 6 {
        return None;
    }
    u32::from_str_radix(t, 16).ok()
}

/// 테마 색상 이름 → RGB (Office 2016 기본 + HWP 스타일 102 근사)
/// accent1~6, dk1, lt1, dk2, lt2 등
fn scheme_color(name: &str) -> Option<u32> {
    match name {
        "accent1" => Some(0x70AD47), // 녹색 (HWP style 102 차트의 1번 시리즈)
        "accent2" => Some(0x4472C4), // 파랑 (2번 시리즈)
        "accent3" => Some(0xED7D31), // 주황
        "accent4" => Some(0xFFC000), // 노랑
        "accent5" => Some(0x5B9BD5), // 하늘
        "accent6" => Some(0xA5A5A5), // 회색
        "dk1" | "tx1" => Some(0x000000),
        "lt1" | "bg1" => Some(0xFFFFFF),
        "dk2" | "tx2" => Some(0x44546A),
        "lt2" | "bg2" => Some(0xE7E6E6),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR_XML: &str = r#"<?xml version="1.0"?>
<c:chartSpace xmlns:c="x" xmlns:a="y">
<c:chart>
  <c:title><c:tx><c:rich><a:p><a:r><a:t>Title A</a:t></a:r></a:p></c:rich></c:tx></c:title>
  <c:plotArea>
    <c:barChart>
      <c:barDir val="col"/>
      <c:ser>
        <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt></c:strCache></c:strRef></c:tx>
        <c:cat><c:strRef><c:strCache>
          <c:pt idx="0"><c:v>Seoul</c:v></c:pt>
          <c:pt idx="1"><c:v>Busan</c:v></c:pt>
        </c:strCache></c:strRef></c:cat>
        <c:val><c:numRef><c:numCache>
          <c:pt idx="0"><c:v>100</c:v></c:pt>
          <c:pt idx="1"><c:v>80</c:v></c:pt>
        </c:numCache></c:numRef></c:val>
      </c:ser>
    </c:barChart>
  </c:plotArea>
</c:chart>
</c:chartSpace>"#;

    #[test]
    fn test_parse_bar_chart() {
        let c = parse_chart_xml(BAR_XML.as_bytes()).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Column);
        assert_eq!(c.title.as_deref(), Some("Title A"));
        assert_eq!(c.series.len(), 1);
        assert_eq!(c.series[0].series_type, OoxmlChartType::Column);
        assert_eq!(c.series[0].values, vec![100.0, 80.0]);
        assert_eq!(c.categories, vec!["Seoul", "Busan"]);
    }

    #[test]
    fn test_parse_combo_dual_axis() {
        let xml = r#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea>
<c:barChart><c:barDir val="col"/><c:ser>
  <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>금액</c:v></c:pt></c:strCache></c:strRef></c:tx>
  <c:spPr><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></c:spPr>
  <c:val><c:numRef><c:numCache><c:formatCode>#,##0</c:formatCode>
    <c:pt idx="0"><c:v>1000</c:v></c:pt><c:pt idx="1"><c:v>2000</c:v></c:pt>
  </c:numCache></c:numRef></c:val>
</c:ser><c:axId val="AX1"/><c:axId val="AX2"/></c:barChart>
<c:lineChart><c:ser>
  <c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>건수</c:v></c:pt></c:strCache></c:strRef></c:tx>
  <c:spPr><a:ln><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></a:ln></c:spPr>
  <c:val><c:numRef><c:numCache>
    <c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>20</c:v></c:pt>
  </c:numCache></c:numRef></c:val>
</c:ser><c:axId val="AX3"/><c:axId val="AX4"/></c:lineChart>
<c:valAx><c:axId val="AX2"/><c:axPos val="l"/></c:valAx>
<c:valAx><c:axId val="AX4"/><c:axPos val="r"/></c:valAx>
</c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml.as_bytes()).expect("parse OK");
        assert_eq!(c.series.len(), 2);
        assert_eq!(c.series[0].name, "금액");
        assert_eq!(c.series[0].series_type, OoxmlChartType::Column);
        assert_eq!(c.series[0].color, Some(0x70AD47));
        assert_eq!(c.series[0].axis_group, 0);
        assert_eq!(c.series[0].format_code.as_deref(), Some("#,##0"));
        assert_eq!(c.series[1].name, "건수");
        assert_eq!(c.series[1].series_type, OoxmlChartType::Line);
        assert_eq!(c.series[1].color, Some(0x4472C4));
        assert_eq!(c.series[1].axis_group, 1);
        assert!(c.has_secondary_axis);
        assert!(c.is_combo());
    }

    #[test]
    fn test_parse_horizontal_bar() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="bar"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>5</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Bar);
    }

    #[test]
    fn test_parse_pie_chart() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:pieChart><c:ser><c:explosion val="25"/><c:val><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:val></c:ser><c:firstSliceAng val="45"/></c:pieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Pie);
        assert_eq!(c.series[0].values, vec![30.0, 70.0]);
        assert_eq!(c.pie_explosion, Some(25));
        assert_eq!(c.pie_first_slice_angle, Some(45));
    }

    #[test]
    fn test_parse_malformed() {
        assert!(parse_chart_xml(b"not xml").is_none());
    }

    // --- C1a (#1453): 3D막대·3D원형·ofPie 라우팅 ---

    #[test]
    fn test_parse_bar3d_col() {
        // bar3DChart + barDir=col → Column (세로 3D 막대 2D 근사)
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:bar3DChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>100</c:v></c:pt><c:pt idx="1"><c:v>80</c:v></c:pt></c:numCache></c:val></c:ser></c:bar3DChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Column);
        assert_eq!(c.series[0].values, vec![100.0, 80.0]);
    }

    #[test]
    fn test_parse_bar3d_bar() {
        // bar3DChart + barDir=bar → Bar (가로 3D 막대 2D 근사)
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:bar3DChart><c:barDir val="bar"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>5</c:v></c:pt></c:numCache></c:val></c:ser></c:bar3DChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Bar);
    }

    #[test]
    fn test_parse_pie3d() {
        // pie3DChart → Pie (3D 원형 2D 근사)
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:pie3DChart><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:val></c:ser></c:pie3DChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Pie);
        assert_eq!(c.series[0].values, vec![30.0, 70.0]);
    }

    #[test]
    fn test_parse_ofpie() {
        // ofPieChart → Pie 렌더 근사 + ofPie semantic 설정 보존.
        let xml = br##"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:ofPieChart><c:ofPieType val="pie"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>40</c:v></c:pt><c:pt idx="1"><c:v>25</c:v></c:pt><c:pt idx="2"><c:v>35</c:v></c:pt></c:numCache></c:val></c:ser><c:gapWidth val="100"/><c:secondPieSize val="75"/><c:serLines><c:spPr><a:ln w="22225"><a:solidFill><a:srgbClr val="123456"/></a:solidFill></a:ln></c:spPr></c:serLines></c:ofPieChart></c:plotArea></c:chart></c:chartSpace>"##;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Pie);
        assert!(c.has_of_pie_chart);
        assert_eq!(c.series[0].values, vec![40.0, 25.0, 35.0]);
        assert_eq!(c.pie_of_pie_type, Some(OfPieType::Pie));
        assert_eq!(c.pie_of_pie_gap_width, Some(100));
        assert_eq!(c.pie_of_pie_second_size, Some(75));
        assert_eq!(c.pie_of_pie_ser_line_color, Some(0x123456));
        assert_eq!(c.pie_of_pie_ser_line_width, Some(22225));
    }

    #[test]
    fn test_parse_doughnut_chart_hole_size() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:doughnutChart><c:ser><c:explosion val="8"/><c:val><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:val></c:ser><c:firstSliceAng val="45"/><c:holeSize val="55"/></c:doughnutChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Pie);
        assert!(c.has_doughnut_chart);
        assert_eq!(c.series[0].values, vec![30.0, 70.0]);
        assert_eq!(c.pie_explosion, Some(8));
        assert_eq!(c.pie_first_slice_angle, Some(45));
        assert_eq!(c.doughnut_hole_size, Some(55));
    }

    #[test]
    fn test_parse_stock_chart() {
        let xml = r#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:stockChart>
<c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>고가</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>1 월</c:v></c:pt><c:pt idx="1"><c:v>2 월</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>55</c:v></c:pt><c:pt idx="1"><c:v>57</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>
<c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>저가</c:v></c:pt></c:strCache></c:strRef></c:tx><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>11</c:v></c:pt><c:pt idx="1"><c:v>12</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>
<c:hiLowLines/><c:axId val="1"/><c:axId val="2"/></c:stockChart><c:valAx><c:axId val="2"/><c:axPos val="l"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml.as_bytes()).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Stock);
        assert_eq!(c.categories, vec!["1 월", "2 월"]);
        assert_eq!(c.series.len(), 2);
        assert_eq!(c.series[0].name, "고가");
        assert_eq!(c.series[0].series_type, OoxmlChartType::Stock);
        assert_eq!(c.series[0].values, vec![55.0, 57.0]);
        assert_eq!(c.series[1].name, "저가");
        assert_eq!(c.series[1].series_type, OoxmlChartType::Stock);
        assert_eq!(c.series[1].values, vec![11.0, 12.0]);
    }

    #[test]
    fn test_parse_stock_up_down_bars() {
        let xml = br##"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:stockChart>
<c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>
<c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>15</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>
<c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>8</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>
<c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>13</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>
<c:upDownBars><c:gapWidth val="75"/><c:upBars><c:spPr><a:solidFill><a:srgbClr val="00B050"/></a:solidFill><a:ln w="19050"><a:solidFill><a:srgbClr val="006100"/></a:solidFill></a:ln></c:spPr></c:upBars><c:downBars><c:spPr><a:solidFill><a:srgbClr val="C00000"/></a:solidFill><a:ln w="25400"><a:solidFill><a:srgbClr val="660000"/></a:solidFill></a:ln></c:spPr></c:downBars></c:upDownBars>
</c:stockChart></c:plotArea></c:chart></c:chartSpace>"##;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Stock);
        assert_eq!(c.stock_up_down_bar_gap_width, Some(75));
        assert_eq!(c.stock_up_bar_fill_color, Some(0x00B050));
        assert_eq!(c.stock_up_bar_line_color, Some(0x006100));
        assert_eq!(c.stock_up_bar_line_width, Some(19050));
        assert_eq!(c.stock_down_bar_fill_color, Some(0xC00000));
        assert_eq!(c.stock_down_bar_line_color, Some(0x660000));
        assert_eq!(c.stock_down_bar_line_width, Some(25400));
    }

    #[test]
    fn test_parse_stock_hi_low_lines_style() {
        let xml = br##"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:stockChart>
<c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>
<c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>15</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser>
<c:hiLowLines><c:spPr><a:ln w="22225"><a:solidFill><a:srgbClr val="123456"/></a:solidFill></a:ln></c:spPr></c:hiLowLines>
</c:stockChart></c:plotArea></c:chart></c:chartSpace>"##;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Stock);
        assert_eq!(c.stock_hi_low_line_color, Some(0x123456));
        assert_eq!(c.stock_hi_low_line_width, Some(22225));
    }

    // --- C1a Part B (#1453): 막대 누적 grouping 파싱 ---

    fn bar_xml_with_grouping(plot: &str, grouping: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:{plot}><c:barDir val="col"/><c:grouping val="{grouping}"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:{plot}></c:plotArea></c:chart></c:chartSpace>"#
        )
    }

    #[test]
    fn test_parse_grouping_stacked() {
        let c = parse_chart_xml(bar_xml_with_grouping("barChart", "stacked").as_bytes())
            .expect("parse OK");
        assert_eq!(c.grouping, BarGrouping::Stacked);
    }

    #[test]
    fn test_parse_grouping_percent_stacked() {
        // bar3DChart 경로에서도 grouping 파싱
        let c = parse_chart_xml(bar_xml_with_grouping("bar3DChart", "percentStacked").as_bytes())
            .expect("parse OK");
        assert_eq!(c.grouping, BarGrouping::PercentStacked);
    }

    #[test]
    fn test_parse_grouping_clustered_default() {
        // clustered 명시 → Clustered. grouping 없는 차트도 기본 Clustered.
        let c = parse_chart_xml(bar_xml_with_grouping("barChart", "clustered").as_bytes())
            .expect("parse OK");
        assert_eq!(c.grouping, BarGrouping::Clustered);
        let c2 = parse_chart_xml(BAR_XML.as_bytes()).expect("parse OK");
        assert_eq!(c2.grouping, BarGrouping::Clustered);
    }

    #[test]
    fn test_parse_grouping_line_stacked() {
        // line plot의 grouping도 semantic 값으로 보존하고 렌더러에서 누적 근사에 사용한다.
        let xml = r#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:lineChart><c:grouping val="stacked"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml.as_bytes()).expect("parse OK");
        assert_eq!(c.grouping, BarGrouping::Stacked);
    }

    #[test]
    fn test_parse_bar_gap_and_overlap() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:gapWidth val="150"/><c:overlap val="-25"/></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.bar_gap_width, Some(150));
        assert_eq!(c.bar_overlap, Some(-25));
    }

    #[test]
    fn test_parse_line_marker_and_smooth() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:marker><c:symbol val="diamond"/><c:size val="7"/><c:spPr><a:solidFill><a:srgbClr val="F4B183"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="5B9BD5"/></a:solidFill></a:ln></c:spPr></c:marker><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:numRef></c:val><c:smooth val="1"/></c:ser><c:marker val="1"/></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Line);
        assert_eq!(c.line_marker_size, Some(7));
        assert_eq!(c.line_marker_symbol, Some(ChartMarkerSymbol::Diamond));
        assert_eq!(c.line_marker_fill_color, Some(0xF4B183));
        assert_eq!(c.line_marker_line_color, Some(0x5B9BD5));
        assert_eq!(c.line_marker_line_width, Some(12700));
        assert_eq!(c.line_marker_visible, Some(true));
        assert_eq!(c.line_smooth, Some(true));
    }

    #[test]
    fn test_parse_scatter_style_marker_and_smooth() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:scatterChart><c:scatterStyle val="line"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Y1</c:v></c:pt></c:strCache></c:strRef></c:tx><c:marker><c:symbol val="square"/><c:size val="7"/><c:spPr><a:solidFill><a:srgbClr val="FFD966"/></a:solidFill><a:ln w="19050"><a:solidFill><a:srgbClr val="70AD47"/></a:solidFill></a:ln></c:spPr></c:marker><c:xVal><c:numRef><c:numCache><c:pt idx="0"><c:v>0.7</c:v></c:pt><c:pt idx="1"><c:v>1.8</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numRef><c:numCache><c:pt idx="0"><c:v>2.7</c:v></c:pt><c:pt idx="1"><c:v>3.2</c:v></c:pt></c:numCache></c:numRef></c:yVal><c:smooth val="0"/></c:ser></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_type, OoxmlChartType::Scatter);
        assert_eq!(c.scatter_style, Some(ScatterStyle::Line));
        assert_eq!(c.scatter_marker_size, Some(7));
        assert_eq!(c.scatter_marker_symbol, Some(ChartMarkerSymbol::Square));
        assert_eq!(c.scatter_marker_fill_color, Some(0xFFD966));
        assert_eq!(c.scatter_marker_line_color, Some(0x70AD47));
        assert_eq!(c.scatter_marker_line_width, Some(19050));
        assert_eq!(c.scatter_smooth, Some(false));
        assert_eq!(c.categories, vec!["0.7", "1.8"]);
        assert_eq!(c.series[0].name, "Y1");
        assert_eq!(c.series[0].values, vec![2.7, 3.2]);
        assert_eq!(c.series[0].series_type, OoxmlChartType::Scatter);
    }

    #[test]
    fn test_parse_legend_position() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart></c:plotArea><c:legend><c:legendPos val="b"/><c:overlay val="0"/></c:legend></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.legend_position, Some(ChartLegendPosition::Bottom));
        assert_eq!(c.legend_overlay, Some(false));
    }

    #[test]
    fn test_parse_title_and_legend_overlay() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:title><c:tx><c:rich><a:p xmlns:a="y"><a:r><a:t>Title</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="1"/></c:title><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:title><c:tx><c:rich><a:p xmlns:a="y"><a:r><a:t>Category Axis</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title></c:catAx><c:valAx><c:axId val="20"/><c:title><c:tx><c:rich><a:p xmlns:a="y"><a:r><a:t>Value Axis</a:t></a:r></a:p></c:rich></c:tx></c:title></c:valAx></c:plotArea><c:legend><c:legendPos val="r"/><c:overlay val="0"/></c:legend></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.title.as_deref(), Some("Title"));
        assert_eq!(c.category_axis_title.as_deref(), Some("Category Axis"));
        assert_eq!(c.value_axis_title.as_deref(), Some("Value Axis"));
        assert_eq!(c.title_overlay, Some(true));
        assert_eq!(c.legend_overlay, Some(false));
    }

    #[test]
    fn test_parse_axis_visibility() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:delete val="1"/><c:axPos val="b"/></c:catAx><c:valAx><c:axId val="20"/><c:delete val="0"/><c:axPos val="l"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_visible, Some(false));
        assert_eq!(c.value_axis_visible, Some(true));
        assert_eq!(c.category_axis_position, Some(AxisPosition::Bottom));
        assert_eq!(c.value_axis_position, Some(AxisPosition::Left));
    }

    #[test]
    fn test_parse_axis_label_position() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:delete val="0"/><c:tickLblPos val="low"/></c:catAx><c:valAx><c:axId val="20"/><c:delete val="0"/><c:tickLblPos val="high"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_label_position, Some(AxisLabelPosition::Low));
        assert_eq!(c.value_axis_label_position, Some(AxisLabelPosition::High));
    }

    #[test]
    fn test_parse_axis_orientation() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:scaling><c:orientation val="maxMin"/></c:scaling></c:catAx><c:valAx><c:axId val="20"/><c:scaling><c:orientation val="minMax"/></c:scaling></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_orientation, Some(AxisOrientation::MaxMin));
        assert_eq!(c.value_axis_orientation, Some(AxisOrientation::MinMax));
    }

    #[test]
    fn test_parse_category_axis_label_controls() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/><c:tickMarkSkip val="1"/><c:noMultiLvlLbl val="0"/></c:catAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_auto, Some(true));
        assert_eq!(
            c.category_axis_label_alignment,
            Some(AxisLabelAlignment::Center)
        );
        assert_eq!(c.category_axis_label_offset, Some(100));
        assert_eq!(c.category_axis_tick_mark_skip, Some(1));
        assert_eq!(c.category_axis_no_multi_level_labels, Some(false));
    }

    #[test]
    fn test_parse_value_axis_cross_between() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:crossBetween val="midCat"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(
            c.value_axis_cross_between,
            Some(AxisCrossBetween::MidCategory)
        );
    }

    #[test]
    fn test_parse_axis_crosses() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:crosses val="min"/><c:crossesAt val="2"/></c:catAx><c:valAx><c:axId val="20"/><c:crosses val="max"/><c:crossesAt val="1.5"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_crosses, Some(AxisCrosses::Min));
        assert_eq!(c.category_axis_crosses_at, Some(2.0));
        assert_eq!(c.value_axis_crosses, Some(AxisCrosses::Max));
        assert_eq!(c.value_axis_crosses_at, Some(1.5));
    }

    #[test]
    fn test_parse_axis_tick_marks() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:majorTickMark val="in"/><c:minorTickMark val="cross"/></c:catAx><c:valAx><c:axId val="20"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_major_tick_mark, Some(AxisTickMark::In));
        assert_eq!(c.category_axis_minor_tick_mark, Some(AxisTickMark::Cross));
        assert_eq!(c.value_axis_major_tick_mark, Some(AxisTickMark::Out));
        assert_eq!(c.value_axis_minor_tick_mark, Some(AxisTickMark::None));
    }

    #[test]
    fn test_parse_axis_line_style() {
        let xml = br##"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:spPr><a:ln w="9525"><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:ln></c:spPr></c:catAx><c:valAx><c:axId val="20"/><c:spPr><a:ln w="19050"><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></a:ln></c:spPr></c:valAx></c:plotArea></c:chart></c:chartSpace>"##;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_line_color, Some(0x112233));
        assert_eq!(c.category_axis_line_width, Some(9525));
        assert_eq!(c.value_axis_line_color, Some(0x4472C4));
        assert_eq!(c.value_axis_line_width, Some(19050));
    }

    #[test]
    fn test_parse_axis_grid_line_style() {
        let xml = br##"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:majorGridlines><c:spPr><a:ln w="6350"><a:solidFill><a:srgbClr val="99AA00"/></a:solidFill></a:ln></c:spPr></c:majorGridlines></c:catAx><c:valAx><c:axId val="20"/><c:minorGridlines><c:spPr><a:ln w="12700"><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></a:ln></c:spPr></c:minorGridlines></c:valAx></c:plotArea></c:chart></c:chartSpace>"##;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_major_grid_line_color, Some(0x99AA00));
        assert_eq!(c.category_axis_major_grid_line_width, Some(6350));
        assert_eq!(c.value_axis_minor_grid_line_color, Some(0x4472C4));
        assert_eq!(c.value_axis_minor_grid_line_width, Some(12700));
    }

    #[test]
    fn test_parse_value_axis_scale() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:scaling><c:logBase val="10"/><c:orientation val="minMax"/><c:max val="12"/><c:min val="0"/></c:scaling><c:majorUnit val="3"/><c:minorUnit val="1.5"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.value_axis_log_base, Some(10.0));
        assert_eq!(c.value_axis_minimum, Some(0.0));
        assert_eq!(c.value_axis_maximum, Some(12.0));
        assert_eq!(c.value_axis_major_unit, Some(3.0));
        assert_eq!(c.value_axis_minor_unit, Some(1.5));
    }

    #[test]
    fn test_parse_value_axis_display_unit() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:dispUnits><c:builtInUnit val="millions"/></c:dispUnits></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.value_axis_display_unit, Some(AxisDisplayUnit::Millions));
    }

    #[test]
    fn test_parse_value_axis_number_format() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:numFmt formatCode="yyyy-mm" sourceLinked="0"/></c:catAx><c:valAx><c:axId val="20"/><c:numFmt formatCode="General" sourceLinked="1"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.category_axis_number_format.as_deref(), Some("yyyy-mm"));
        assert_eq!(c.category_axis_number_format_source_linked, Some(false));
        assert_eq!(c.value_axis_number_format.as_deref(), Some("General"));
        assert_eq!(c.value_axis_number_format_source_linked, Some(true));
    }

    #[test]
    fn test_parse_data_labels() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:dLbls><c:dLblPos val="outEnd"/><c:showVal val="1"/><c:showCatName val="0"/><c:showSerName val="1"/><c:showPercent val="0"/><c:showLegendKey val="1"/></c:dLbls><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(
            c.data_label_position,
            Some(ChartDataLabelPosition::OutsideEnd)
        );
        assert_eq!(c.data_labels_show_value, Some(true));
        assert_eq!(c.data_labels_show_category_name, Some(false));
        assert_eq!(c.data_labels_show_series_name, Some(true));
        assert_eq!(c.data_labels_show_percent, Some(false));
        assert_eq!(c.data_labels_show_legend_key, Some(true));
    }

    #[test]
    fn test_parse_chart_display_options() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:date1904 val="1"/><c:roundedCorners val="1"/><c:chart><c:autoTitleDeleted val="1"/><c:plotArea><c:barChart><c:barDir val="col"/><c:varyColors val="1"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart></c:plotArea><c:dispBlanksAs val="span"/><c:showHiddenData val="1"/><c:plotVisOnly val="0"/></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.date_1904, Some(true));
        assert_eq!(c.rounded_corners, Some(true));
        assert_eq!(c.auto_title_deleted, Some(true));
        assert_eq!(c.vary_colors, Some(true));
        assert_eq!(c.display_blanks_as, Some(ChartDisplayBlanksAs::Span));
        assert_eq!(c.show_hidden_data, Some(true));
        assert_eq!(c.plot_visible_only, Some(false));
    }

    #[test]
    fn test_parse_chart_data_table_flags() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart><c:dTable><c:showHorzBorder val="1"/><c:showVertBorder val="0"/><c:showOutline val="1"/><c:showKeys val="0"/></c:dTable></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.data_table_show_horizontal_border, Some(true));
        assert_eq!(c.data_table_show_vertical_border, Some(false));
        assert_eq!(c.data_table_show_outline, Some(true));
        assert_eq!(c.data_table_show_keys, Some(false));
    }

    #[test]
    fn test_parse_chart_trendline_fields() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:trendline><c:spPr><a:ln w="22225"><a:solidFill><a:srgbClr val="ABCDEF"/></a:solidFill></a:ln></c:spPr><c:trendlineType val="poly"/><c:order val="3"/><c:period val="5"/><c:dispEq val="1"/><c:dispRSqr val="0"/></c:trendline><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.trendline_type, Some(ChartTrendlineType::Polynomial));
        assert_eq!(c.trendline_order, Some(3));
        assert_eq!(c.trendline_period, Some(5));
        assert_eq!(c.trendline_display_equation, Some(true));
        assert_eq!(c.trendline_display_r_squared, Some(false));
        assert_eq!(c.trendline_line_color, Some(0xABCDEF));
        assert_eq!(c.trendline_line_width, Some(22225));
    }

    #[test]
    fn test_parse_chart_error_bar_fields() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:errBars><c:spPr><a:ln w="31750"><a:solidFill><a:srgbClr val="CC5500"/></a:solidFill></a:ln></c:spPr><c:errDir val="y"/><c:errBarType val="both"/><c:errValType val="fixedVal"/><c:noEndCap val="1"/><c:val val="1.25"/></c:errBars><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.error_bar_direction, Some(ChartErrorBarDirection::Y));
        assert_eq!(c.error_bar_type, Some(ChartErrorBarType::Both));
        assert_eq!(
            c.error_bar_value_type,
            Some(ChartErrorBarValueType::FixedValue)
        );
        assert_eq!(c.error_bar_no_end_cap, Some(true));
        assert_eq!(c.error_bar_value, Some(1.25));
        assert_eq!(c.error_bar_line_color, Some(0xCC5500));
        assert_eq!(c.error_bar_line_width, Some(31750));
    }

    #[test]
    fn test_parse_chart_style_from_alternate_content() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:mc="m" xmlns:c14="y"><mc:AlternateContent><mc:Choice Requires="c14"><c14:style val="102"/></mc:Choice><mc:Fallback><c:style val="2"/></mc:Fallback></mc:AlternateContent><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_style, Some(2));
    }

    #[test]
    fn test_parse_chart_area_fill_color() {
        let xml = br##"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart></c:plotArea></c:chart><c:spPr><a:solidFill><a:srgbClr val="E6F0FA"/></a:solidFill><a:ln><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:ln></c:spPr></c:chartSpace>"##;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.chart_area_fill_color, Some(0xE6F0FA));
    }

    #[test]
    fn test_parse_plot_area_fill_color() {
        let xml = br##"<?xml version="1.0"?><c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser></c:barChart><c:spPr><a:solidFill><a:srgbClr val="F1E4D6"/></a:solidFill><a:ln><a:solidFill><a:srgbClr val="112233"/></a:solidFill></a:ln></c:spPr></c:plotArea></c:chart></c:chartSpace>"##;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.plot_area_fill_color, Some(0xF1E4D6));
    }

    #[test]
    fn test_parse_chart_view_3d() {
        let xml = br#"<?xml version="1.0"?><c:chartSpace xmlns:c="x"><c:chart><c:view3D><c:rAngAx val="1"/><c:rotX val="15"/><c:rotY val="20"/><c:perspective val="30"/><c:hPercent val="100"/><c:depthPercent val="120"/></c:view3D><c:plotArea><c:bar3DChart><c:barDir val="col"/><c:ser><c:val><c:numCache><c:pt idx="0"><c:v>3</c:v></c:pt></c:numCache></c:val></c:ser><c:shape val="box"/><c:gapDepth val="150"/></c:bar3DChart></c:plotArea></c:chart></c:chartSpace>"#;
        let c = parse_chart_xml(xml).expect("parse OK");
        assert_eq!(c.view_3d_right_angle_axes, Some(true));
        assert_eq!(c.view_3d_rotation_x, Some(15));
        assert_eq!(c.view_3d_rotation_y, Some(20));
        assert_eq!(c.view_3d_perspective, Some(30));
        assert_eq!(c.view_3d_height_percent, Some(100));
        assert_eq!(c.view_3d_depth_percent, Some(120));
        assert_eq!(c.bar_3d_gap_depth, Some(150));
        assert_eq!(c.bar_3d_shape.as_deref(), Some("box"));
    }
}
