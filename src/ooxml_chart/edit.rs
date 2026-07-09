//! Minimal OOXML chart semantic editing.
//!
//! This preserves the original chart XML shape and updates only selected
//! semantic fields: title text, bar chart direction/grouping, line chart grouping/marker
//! style, stock up/down bar style, cached labels, and numeric values in string/number cache blocks.
//! Updated category/value caches are rebuilt so `<c:pt>` nodes and `ptCount`
//! can grow or shrink.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, Write};

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

use super::{
    AxisCrossBetween, AxisCrosses, AxisDisplayUnit, AxisLabelAlignment, AxisLabelPosition,
    AxisOrientation, AxisPosition, AxisTickMark, BarGrouping, ChartDataLabelPosition,
    ChartDisplayBlanksAs, ChartErrorBarDirection, ChartErrorBarType, ChartErrorBarValueType,
    ChartLegendPosition, ChartMarkerSymbol, ChartTrendlineType, OfPieType, OoxmlChart,
    OoxmlChartType, ScatterStyle,
};

#[derive(Debug, Clone, Default)]
pub struct ChartXmlUpdate {
    pub title: Option<String>,
    pub chart_type: Option<OoxmlChartType>,
    pub grouping: Option<BarGrouping>,
    pub bar_gap_width: Option<u32>,
    pub bar_overlap: Option<i32>,
    pub bar_3d_gap_depth: Option<u32>,
    pub bar_3d_shape: Option<String>,
    pub line_smooth: Option<bool>,
    pub line_marker_visible: Option<bool>,
    pub line_marker_size: Option<u32>,
    pub line_marker_symbol: Option<ChartMarkerSymbol>,
    pub line_marker_fill_color: Option<u32>,
    pub line_marker_line_color: Option<u32>,
    pub line_marker_line_width: Option<u32>,
    pub pie_first_slice_angle: Option<u16>,
    pub pie_explosion: Option<u32>,
    pub doughnut_hole_size: Option<u32>,
    pub pie_of_pie_type: Option<OfPieType>,
    pub pie_of_pie_gap_width: Option<u32>,
    pub pie_of_pie_second_size: Option<u32>,
    pub pie_of_pie_ser_line_color: Option<u32>,
    pub pie_of_pie_ser_line_width: Option<u32>,
    pub scatter_style: Option<ScatterStyle>,
    pub scatter_smooth: Option<bool>,
    pub scatter_marker_size: Option<u32>,
    pub scatter_marker_symbol: Option<ChartMarkerSymbol>,
    pub scatter_marker_fill_color: Option<u32>,
    pub scatter_marker_line_color: Option<u32>,
    pub scatter_marker_line_width: Option<u32>,
    pub trendline_type: Option<ChartTrendlineType>,
    pub trendline_order: Option<u32>,
    pub trendline_period: Option<u32>,
    pub trendline_display_equation: Option<bool>,
    pub trendline_display_r_squared: Option<bool>,
    pub trendline_line_color: Option<u32>,
    pub trendline_line_width: Option<u32>,
    pub error_bar_direction: Option<ChartErrorBarDirection>,
    pub error_bar_type: Option<ChartErrorBarType>,
    pub error_bar_value_type: Option<ChartErrorBarValueType>,
    pub error_bar_value: Option<f64>,
    pub error_bar_no_end_cap: Option<bool>,
    pub error_bar_line_color: Option<u32>,
    pub error_bar_line_width: Option<u32>,
    pub stock_up_down_bar_gap_width: Option<u32>,
    pub stock_up_bar_fill_color: Option<u32>,
    pub stock_down_bar_fill_color: Option<u32>,
    pub stock_up_bar_line_color: Option<u32>,
    pub stock_down_bar_line_color: Option<u32>,
    pub stock_up_bar_line_width: Option<u32>,
    pub stock_down_bar_line_width: Option<u32>,
    pub stock_hi_low_line_color: Option<u32>,
    pub stock_hi_low_line_width: Option<u32>,
    pub data_label_position: Option<ChartDataLabelPosition>,
    pub data_labels_show_value: Option<bool>,
    pub data_labels_show_category_name: Option<bool>,
    pub data_labels_show_series_name: Option<bool>,
    pub data_labels_show_percent: Option<bool>,
    pub data_labels_show_legend_key: Option<bool>,
    pub title_overlay: Option<bool>,
    pub date_1904: Option<bool>,
    pub chart_style: Option<u32>,
    pub chart_area_fill_color: Option<u32>,
    pub plot_area_fill_color: Option<u32>,
    pub rounded_corners: Option<bool>,
    pub auto_title_deleted: Option<bool>,
    pub vary_colors: Option<bool>,
    pub view_3d_rotation_x: Option<i32>,
    pub view_3d_rotation_y: Option<i32>,
    pub view_3d_perspective: Option<u32>,
    pub view_3d_right_angle_axes: Option<bool>,
    pub view_3d_height_percent: Option<u32>,
    pub view_3d_depth_percent: Option<u32>,
    pub display_blanks_as: Option<ChartDisplayBlanksAs>,
    pub show_hidden_data: Option<bool>,
    pub plot_visible_only: Option<bool>,
    pub data_table_show_horizontal_border: Option<bool>,
    pub data_table_show_vertical_border: Option<bool>,
    pub data_table_show_outline: Option<bool>,
    pub data_table_show_keys: Option<bool>,
    pub legend_position: Option<ChartLegendPosition>,
    pub legend_overlay: Option<bool>,
    pub category_axis_visible: Option<bool>,
    pub value_axis_visible: Option<bool>,
    pub category_axis_title: Option<String>,
    pub value_axis_title: Option<String>,
    pub category_axis_position: Option<AxisPosition>,
    pub value_axis_position: Option<AxisPosition>,
    pub category_axis_label_position: Option<AxisLabelPosition>,
    pub value_axis_label_position: Option<AxisLabelPosition>,
    pub category_axis_auto: Option<bool>,
    pub category_axis_label_alignment: Option<AxisLabelAlignment>,
    pub category_axis_label_offset: Option<u32>,
    pub category_axis_tick_mark_skip: Option<u32>,
    pub category_axis_no_multi_level_labels: Option<bool>,
    pub category_axis_orientation: Option<AxisOrientation>,
    pub value_axis_orientation: Option<AxisOrientation>,
    pub category_axis_crosses: Option<AxisCrosses>,
    pub category_axis_crosses_at: Option<f64>,
    pub value_axis_crosses: Option<AxisCrosses>,
    pub value_axis_crosses_at: Option<f64>,
    pub value_axis_cross_between: Option<AxisCrossBetween>,
    pub category_axis_major_tick_mark: Option<AxisTickMark>,
    pub category_axis_minor_tick_mark: Option<AxisTickMark>,
    pub category_axis_line_color: Option<u32>,
    pub category_axis_line_width: Option<u32>,
    pub category_axis_major_grid_line_color: Option<u32>,
    pub category_axis_major_grid_line_width: Option<u32>,
    pub category_axis_minor_grid_line_color: Option<u32>,
    pub category_axis_minor_grid_line_width: Option<u32>,
    pub value_axis_major_tick_mark: Option<AxisTickMark>,
    pub value_axis_minor_tick_mark: Option<AxisTickMark>,
    pub value_axis_line_color: Option<u32>,
    pub value_axis_line_width: Option<u32>,
    pub value_axis_major_grid_line_color: Option<u32>,
    pub value_axis_major_grid_line_width: Option<u32>,
    pub value_axis_minor_grid_line_color: Option<u32>,
    pub value_axis_minor_grid_line_width: Option<u32>,
    pub value_axis_log_base: Option<f64>,
    pub value_axis_display_unit: Option<AxisDisplayUnit>,
    pub value_axis_minimum: Option<f64>,
    pub value_axis_maximum: Option<f64>,
    pub value_axis_major_unit: Option<f64>,
    pub value_axis_minor_unit: Option<f64>,
    pub category_axis_number_format: Option<String>,
    pub category_axis_number_format_source_linked: Option<bool>,
    pub value_axis_number_format: Option<String>,
    pub value_axis_number_format_source_linked: Option<bool>,
    pub categories: Option<Vec<String>>,
    pub series: Vec<SeriesXmlUpdate>,
}

#[derive(Debug, Clone, Default)]
pub struct SeriesXmlUpdate {
    pub index: usize,
    pub name: Option<String>,
    pub values: Option<Vec<f64>>,
    pub color: Option<u32>,
    pub line_color: Option<u32>,
    pub line_width: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxisKind {
    Category,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxisGridLineKind {
    CategoryMajor,
    CategoryMinor,
    ValueMajor,
    ValueMinor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StockBarKind {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerFamily {
    Line,
    Scatter,
}

#[derive(Default)]
struct StockBarUpdateState {
    up_down_bars_seen: bool,
    gap_width_updated: bool,
    up_fill_updated: bool,
    down_fill_updated: bool,
    up_line_color_updated: bool,
    down_line_color_updated: bool,
    up_line_width_updated: bool,
    down_line_width_updated: bool,
    hi_low_lines_seen: bool,
    hi_low_line_color_updated: bool,
    hi_low_line_width_updated: bool,
}

impl StockBarUpdateState {
    fn mark_updated(&mut self, update: &ChartXmlUpdate) {
        self.up_down_bars_seen = true;
        if update.stock_up_down_bar_gap_width.is_some() {
            self.gap_width_updated = true;
        }
        if update.stock_up_bar_fill_color.is_some() {
            self.up_fill_updated = true;
        }
        if update.stock_down_bar_fill_color.is_some() {
            self.down_fill_updated = true;
        }
        if update.stock_up_bar_line_color.is_some() {
            self.up_line_color_updated = true;
        }
        if update.stock_down_bar_line_color.is_some() {
            self.down_line_color_updated = true;
        }
        if update.stock_up_bar_line_width.is_some() {
            self.up_line_width_updated = true;
        }
        if update.stock_down_bar_line_width.is_some() {
            self.down_line_width_updated = true;
        }
    }

    fn mark_hi_low_updated(&mut self, update: &ChartXmlUpdate) {
        self.hi_low_lines_seen = true;
        if update.stock_hi_low_line_color.is_some() {
            self.hi_low_line_color_updated = true;
        }
        if update.stock_hi_low_line_width.is_some() {
            self.hi_low_line_width_updated = true;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataLabelField {
    Position,
    ShowValue,
    ShowCategoryName,
    ShowSeriesName,
    ShowPercent,
    ShowLegendKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataTableField {
    ShowHorizontalBorder,
    ShowVerticalBorder,
    ShowOutline,
    ShowKeys,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrendlineField {
    Type,
    Order,
    Period,
    DisplayEquation,
    DisplayRSquared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorBarField {
    Direction,
    Type,
    ValueType,
    Value,
    NoEndCap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayScope {
    Title,
    Legend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View3DField {
    RotationX,
    RotationY,
    Perspective,
    RightAngleAxes,
    HeightPercent,
    DepthPercent,
}

#[derive(Default)]
struct DataLabelUpdateState {
    current_plot_has_data_labels: bool,
    current_position_updated: bool,
    current_show_value_updated: bool,
    current_show_category_name_updated: bool,
    current_show_series_name_updated: bool,
    current_show_percent_updated: bool,
    current_show_legend_key_updated: bool,
    position_updated: bool,
    show_value_updated: bool,
    show_category_name_updated: bool,
    show_series_name_updated: bool,
    show_percent_updated: bool,
    show_legend_key_updated: bool,
}

impl DataLabelUpdateState {
    fn reset_for_plot(&mut self) {
        self.current_plot_has_data_labels = false;
    }

    fn reset_for_data_labels(&mut self) {
        self.current_position_updated = false;
        self.current_show_value_updated = false;
        self.current_show_category_name_updated = false;
        self.current_show_series_name_updated = false;
        self.current_show_percent_updated = false;
        self.current_show_legend_key_updated = false;
    }

    fn mark_updated(&mut self, field: DataLabelField) {
        match field {
            DataLabelField::Position => {
                self.current_position_updated = true;
                self.position_updated = true;
            }
            DataLabelField::ShowValue => {
                self.current_show_value_updated = true;
                self.show_value_updated = true;
            }
            DataLabelField::ShowCategoryName => {
                self.current_show_category_name_updated = true;
                self.show_category_name_updated = true;
            }
            DataLabelField::ShowSeriesName => {
                self.current_show_series_name_updated = true;
                self.show_series_name_updated = true;
            }
            DataLabelField::ShowPercent => {
                self.current_show_percent_updated = true;
                self.show_percent_updated = true;
            }
            DataLabelField::ShowLegendKey => {
                self.current_show_legend_key_updated = true;
                self.show_legend_key_updated = true;
            }
        }
    }

    fn mark_requested_updated(&mut self, update: &ChartXmlUpdate) {
        for field in data_label_requested_fields(update) {
            self.mark_updated(field);
        }
    }

    fn current_updated(&self, field: DataLabelField) -> bool {
        match field {
            DataLabelField::Position => self.current_position_updated,
            DataLabelField::ShowValue => self.current_show_value_updated,
            DataLabelField::ShowCategoryName => self.current_show_category_name_updated,
            DataLabelField::ShowSeriesName => self.current_show_series_name_updated,
            DataLabelField::ShowPercent => self.current_show_percent_updated,
            DataLabelField::ShowLegendKey => self.current_show_legend_key_updated,
        }
    }

    fn updated(&self, field: DataLabelField) -> bool {
        match field {
            DataLabelField::Position => self.position_updated,
            DataLabelField::ShowValue => self.show_value_updated,
            DataLabelField::ShowCategoryName => self.show_category_name_updated,
            DataLabelField::ShowSeriesName => self.show_series_name_updated,
            DataLabelField::ShowPercent => self.show_percent_updated,
            DataLabelField::ShowLegendKey => self.show_legend_key_updated,
        }
    }
}

#[derive(Default)]
struct DataTableUpdateState {
    show_horizontal_border_updated: bool,
    show_vertical_border_updated: bool,
    show_outline_updated: bool,
    show_keys_updated: bool,
}

impl DataTableUpdateState {
    fn mark_updated(&mut self, field: DataTableField) {
        match field {
            DataTableField::ShowHorizontalBorder => self.show_horizontal_border_updated = true,
            DataTableField::ShowVerticalBorder => self.show_vertical_border_updated = true,
            DataTableField::ShowOutline => self.show_outline_updated = true,
            DataTableField::ShowKeys => self.show_keys_updated = true,
        }
    }

    fn updated(&self, field: DataTableField) -> bool {
        match field {
            DataTableField::ShowHorizontalBorder => self.show_horizontal_border_updated,
            DataTableField::ShowVerticalBorder => self.show_vertical_border_updated,
            DataTableField::ShowOutline => self.show_outline_updated,
            DataTableField::ShowKeys => self.show_keys_updated,
        }
    }
}

#[derive(Default)]
struct TrendlineUpdateState {
    current_line_style_updated: bool,
    current_type_updated: bool,
    current_order_updated: bool,
    current_period_updated: bool,
    current_display_equation_updated: bool,
    current_display_r_squared_updated: bool,
    line_style_updated: bool,
    type_updated: bool,
    order_updated: bool,
    period_updated: bool,
    display_equation_updated: bool,
    display_r_squared_updated: bool,
}

impl TrendlineUpdateState {
    fn reset_for_trendline(&mut self) {
        self.current_line_style_updated = false;
        self.current_type_updated = false;
        self.current_order_updated = false;
        self.current_period_updated = false;
        self.current_display_equation_updated = false;
        self.current_display_r_squared_updated = false;
    }

    fn mark_updated(&mut self, field: TrendlineField) {
        match field {
            TrendlineField::Type => {
                self.current_type_updated = true;
                self.type_updated = true;
            }
            TrendlineField::Order => {
                self.current_order_updated = true;
                self.order_updated = true;
            }
            TrendlineField::Period => {
                self.current_period_updated = true;
                self.period_updated = true;
            }
            TrendlineField::DisplayEquation => {
                self.current_display_equation_updated = true;
                self.display_equation_updated = true;
            }
            TrendlineField::DisplayRSquared => {
                self.current_display_r_squared_updated = true;
                self.display_r_squared_updated = true;
            }
        }
    }

    fn mark_line_style_updated(&mut self) {
        self.current_line_style_updated = true;
        self.line_style_updated = true;
    }

    fn current_updated(&self, field: TrendlineField) -> bool {
        match field {
            TrendlineField::Type => self.current_type_updated,
            TrendlineField::Order => self.current_order_updated,
            TrendlineField::Period => self.current_period_updated,
            TrendlineField::DisplayEquation => self.current_display_equation_updated,
            TrendlineField::DisplayRSquared => self.current_display_r_squared_updated,
        }
    }

    fn updated(&self, field: TrendlineField) -> bool {
        match field {
            TrendlineField::Type => self.type_updated,
            TrendlineField::Order => self.order_updated,
            TrendlineField::Period => self.period_updated,
            TrendlineField::DisplayEquation => self.display_equation_updated,
            TrendlineField::DisplayRSquared => self.display_r_squared_updated,
        }
    }
}

#[derive(Default)]
struct ErrorBarUpdateState {
    current_direction_updated: bool,
    current_type_updated: bool,
    current_value_type_updated: bool,
    current_value_updated: bool,
    current_no_end_cap_updated: bool,
    current_line_style_updated: bool,
    direction_updated: bool,
    type_updated: bool,
    value_type_updated: bool,
    value_updated: bool,
    no_end_cap_updated: bool,
    line_style_updated: bool,
}

impl ErrorBarUpdateState {
    fn reset_for_error_bars(&mut self) {
        self.current_direction_updated = false;
        self.current_type_updated = false;
        self.current_value_type_updated = false;
        self.current_value_updated = false;
        self.current_no_end_cap_updated = false;
        self.current_line_style_updated = false;
    }

    fn mark_updated(&mut self, field: ErrorBarField) {
        match field {
            ErrorBarField::Direction => {
                self.current_direction_updated = true;
                self.direction_updated = true;
            }
            ErrorBarField::Type => {
                self.current_type_updated = true;
                self.type_updated = true;
            }
            ErrorBarField::ValueType => {
                self.current_value_type_updated = true;
                self.value_type_updated = true;
            }
            ErrorBarField::Value => {
                self.current_value_updated = true;
                self.value_updated = true;
            }
            ErrorBarField::NoEndCap => {
                self.current_no_end_cap_updated = true;
                self.no_end_cap_updated = true;
            }
        }
    }

    fn mark_line_style_updated(&mut self) {
        self.current_line_style_updated = true;
        self.line_style_updated = true;
    }

    fn current_updated(&self, field: ErrorBarField) -> bool {
        match field {
            ErrorBarField::Direction => self.current_direction_updated,
            ErrorBarField::Type => self.current_type_updated,
            ErrorBarField::ValueType => self.current_value_type_updated,
            ErrorBarField::Value => self.current_value_updated,
            ErrorBarField::NoEndCap => self.current_no_end_cap_updated,
        }
    }

    fn updated(&self, field: ErrorBarField) -> bool {
        match field {
            ErrorBarField::Direction => self.direction_updated,
            ErrorBarField::Type => self.type_updated,
            ErrorBarField::ValueType => self.value_type_updated,
            ErrorBarField::Value => self.value_updated,
            ErrorBarField::NoEndCap => self.no_end_cap_updated,
        }
    }
}

const AXIS_GRID_LINE_KINDS: [AxisGridLineKind; 4] = [
    AxisGridLineKind::CategoryMajor,
    AxisGridLineKind::CategoryMinor,
    AxisGridLineKind::ValueMajor,
    AxisGridLineKind::ValueMinor,
];

#[derive(Default)]
struct GridLineUpdateState {
    category_major_seen: bool,
    category_major_color_updated: bool,
    category_major_width_updated: bool,
    category_minor_seen: bool,
    category_minor_color_updated: bool,
    category_minor_width_updated: bool,
    value_major_seen: bool,
    value_major_color_updated: bool,
    value_major_width_updated: bool,
    value_minor_seen: bool,
    value_minor_color_updated: bool,
    value_minor_width_updated: bool,
}

impl GridLineUpdateState {
    fn mark_seen(&mut self, kind: AxisGridLineKind) {
        match kind {
            AxisGridLineKind::CategoryMajor => self.category_major_seen = true,
            AxisGridLineKind::CategoryMinor => self.category_minor_seen = true,
            AxisGridLineKind::ValueMajor => self.value_major_seen = true,
            AxisGridLineKind::ValueMinor => self.value_minor_seen = true,
        }
    }

    fn is_seen(&self, kind: AxisGridLineKind) -> bool {
        match kind {
            AxisGridLineKind::CategoryMajor => self.category_major_seen,
            AxisGridLineKind::CategoryMinor => self.category_minor_seen,
            AxisGridLineKind::ValueMajor => self.value_major_seen,
            AxisGridLineKind::ValueMinor => self.value_minor_seen,
        }
    }

    fn mark_updated(&mut self, kind: AxisGridLineKind, update: &ChartXmlUpdate) {
        self.mark_seen(kind);
        if axis_grid_line_color_update(kind, update).is_some() {
            match kind {
                AxisGridLineKind::CategoryMajor => self.category_major_color_updated = true,
                AxisGridLineKind::CategoryMinor => self.category_minor_color_updated = true,
                AxisGridLineKind::ValueMajor => self.value_major_color_updated = true,
                AxisGridLineKind::ValueMinor => self.value_minor_color_updated = true,
            }
        }
        if axis_grid_line_width_update(kind, update).is_some() {
            match kind {
                AxisGridLineKind::CategoryMajor => self.category_major_width_updated = true,
                AxisGridLineKind::CategoryMinor => self.category_minor_width_updated = true,
                AxisGridLineKind::ValueMajor => self.value_major_width_updated = true,
                AxisGridLineKind::ValueMinor => self.value_minor_width_updated = true,
            }
        }
    }

    fn is_color_updated(&self, kind: AxisGridLineKind) -> bool {
        match kind {
            AxisGridLineKind::CategoryMajor => self.category_major_color_updated,
            AxisGridLineKind::CategoryMinor => self.category_minor_color_updated,
            AxisGridLineKind::ValueMajor => self.value_major_color_updated,
            AxisGridLineKind::ValueMinor => self.value_minor_color_updated,
        }
    }

    fn is_width_updated(&self, kind: AxisGridLineKind) -> bool {
        match kind {
            AxisGridLineKind::CategoryMajor => self.category_major_width_updated,
            AxisGridLineKind::CategoryMinor => self.category_minor_width_updated,
            AxisGridLineKind::ValueMajor => self.value_major_width_updated,
            AxisGridLineKind::ValueMinor => self.value_minor_width_updated,
        }
    }
}

pub fn update_chart_xml(xml: &[u8], update: &ChartXmlUpdate) -> Result<Vec<u8>, String> {
    let parsed =
        OoxmlChart::parse(xml).ok_or_else(|| "OOXML chart XML을 파싱할 수 없습니다".to_string())?;
    validate_update(&parsed, update)?;

    let series_updates: BTreeMap<usize, SeriesXmlUpdate> = update
        .series
        .iter()
        .cloned()
        .map(|item| (item.index, item))
        .collect();

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(xml.len()));
    let mut buf = Vec::new();
    let mut path: Vec<Vec<u8>> = Vec::new();
    let mut ser_index = 0usize;
    let mut current_ser: Option<usize> = None;
    let mut current_pt_idx: Option<usize> = None;
    let mut tx_pt_seq = 0usize;
    let mut cat_pt_seq = 0usize;
    let mut val_pt_seq = 0usize;
    let mut plot_index = 0usize;
    let mut current_plot: Option<usize> = None;
    let mut bar_dir_updated = false;
    let mut grouping_updated = false;
    let mut grouping_seen_plots: BTreeSet<usize> = BTreeSet::new();
    let mut grouping_updated_plots: BTreeSet<usize> = BTreeSet::new();
    let mut bar_gap_width_updated = false;
    let mut bar_overlap_updated = false;
    let mut bar_3d_gap_depth_updated = false;
    let mut bar_3d_shape_updated = false;
    let mut line_smooth_updated = false;
    let mut line_chart_smooth_updated = false;
    let mut line_smooth_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut line_marker_visible_updated = false;
    let mut line_marker_symbol_updated = false;
    let mut line_marker_size_updated = false;
    let mut line_marker_style_updated = false;
    let mut line_marker_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut line_marker_symbol_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut line_marker_size_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut line_marker_style_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut pie_first_slice_angle_updated = false;
    let mut pie_explosion_updated = false;
    let mut pie_explosion_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut doughnut_hole_size_updated = false;
    let mut pie_of_pie_type_updated = false;
    let mut pie_of_pie_gap_width_updated = false;
    let mut pie_of_pie_second_size_updated = false;
    let mut pie_of_pie_ser_lines_seen = false;
    let mut pie_of_pie_ser_line_color_updated = false;
    let mut pie_of_pie_ser_line_width_updated = false;
    let mut scatter_style_updated = false;
    let mut scatter_smooth_updated = false;
    let mut scatter_smooth_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut scatter_marker_symbol_updated = false;
    let mut scatter_marker_size_updated = false;
    let mut scatter_marker_style_updated = false;
    let mut scatter_marker_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut scatter_marker_symbol_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut scatter_marker_size_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut scatter_marker_style_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut trendline_state = TrendlineUpdateState::default();
    let mut trendline_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut error_bar_state = ErrorBarUpdateState::default();
    let mut error_bar_seen_series: BTreeSet<usize> = BTreeSet::new();
    let mut stock_bar_state = StockBarUpdateState::default();
    let mut data_label_state = DataLabelUpdateState::default();
    let mut title_overlay_updated = false;
    let mut date_1904_updated = false;
    let mut chart_style_updated = false;
    let mut chart_area_fill_color_updated = false;
    let mut plot_area_fill_color_updated = false;
    let mut rounded_corners_updated = false;
    let mut auto_title_deleted_updated = false;
    let mut vary_colors_updated = false;
    let mut view_3d_rotation_x_updated = false;
    let mut view_3d_rotation_y_updated = false;
    let mut view_3d_perspective_updated = false;
    let mut view_3d_right_angle_axes_updated = false;
    let mut view_3d_height_percent_updated = false;
    let mut view_3d_depth_percent_updated = false;
    let mut view_3d_seen = false;
    let mut display_blanks_as_updated = false;
    let mut show_hidden_data_updated = false;
    let mut plot_visible_only_updated = false;
    let mut data_table_seen = false;
    let mut data_table_state = DataTableUpdateState::default();
    let mut legend_position_seen = false;
    let mut legend_position_updated = false;
    let mut legend_overlay_updated = false;
    let mut category_axis_delete_seen = false;
    let mut value_axis_delete_seen = false;
    let mut category_axis_visibility_updated = false;
    let mut value_axis_visibility_updated = false;
    let mut category_axis_title_updated = false;
    let mut value_axis_title_updated = false;
    let mut category_axis_position_seen = false;
    let mut value_axis_position_seen = false;
    let mut category_axis_position_updated = false;
    let mut value_axis_position_updated = false;
    let mut category_axis_label_position_seen = false;
    let mut value_axis_label_position_seen = false;
    let mut category_axis_label_position_updated = false;
    let mut value_axis_label_position_updated = false;
    let mut category_axis_auto_updated = false;
    let mut category_axis_label_alignment_updated = false;
    let mut category_axis_label_offset_updated = false;
    let mut category_axis_tick_mark_skip_updated = false;
    let mut category_axis_no_multi_level_labels_updated = false;
    let mut category_axis_scaling_seen = false;
    let mut value_axis_scaling_seen = false;
    let mut category_axis_orientation_updated = false;
    let mut value_axis_orientation_updated = false;
    let mut category_axis_crosses_updated = false;
    let mut category_axis_crosses_at_updated = false;
    let mut value_axis_crosses_updated = false;
    let mut value_axis_crosses_at_updated = false;
    let mut value_axis_cross_between_updated = false;
    let mut category_axis_major_tick_mark_updated = false;
    let mut category_axis_minor_tick_mark_updated = false;
    let mut category_axis_line_color_updated = false;
    let mut category_axis_line_width_updated = false;
    let mut category_axis_sp_pr_seen = false;
    let mut grid_line_state = GridLineUpdateState::default();
    let mut value_axis_major_tick_mark_updated = false;
    let mut value_axis_minor_tick_mark_updated = false;
    let mut value_axis_line_color_updated = false;
    let mut value_axis_line_width_updated = false;
    let mut value_axis_sp_pr_seen = false;
    let mut value_axis_log_base_updated = false;
    let mut value_axis_display_unit_updated = false;
    let mut value_axis_minimum_updated = false;
    let mut value_axis_maximum_updated = false;
    let mut value_axis_major_unit_updated = false;
    let mut value_axis_minor_unit_updated = false;
    let mut category_axis_number_format_updated = false;
    let mut value_axis_number_format_updated = false;
    let mut series_sp_pr_seen: BTreeSet<usize> = BTreeSet::new();
    let mut series_style_updated: BTreeSet<usize> = BTreeSet::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let local = local_name(e.name().as_ref()).to_vec();
                if is_chart_plot(local.as_slice()) {
                    data_label_state.reset_for_plot();
                    current_plot = Some(plot_index);
                    plot_index += 1;
                }
                if local.as_slice() == b"view3D"
                    && path.last().is_some_and(|p| p.as_slice() == b"chart")
                {
                    view_3d_seen = true;
                }
                if local.as_slice() == b"dLbls" {
                    data_label_state.current_plot_has_data_labels = true;
                    data_label_state.reset_for_data_labels();
                }
                if local.as_slice() == b"dTable" && is_plot_area_parent_path(&path) {
                    data_table_seen = true;
                }
                if let Some(series_index) = trendline_element(local.as_slice(), &path, current_ser)
                {
                    trendline_seen_series.insert(series_index);
                    trendline_state.reset_for_trendline();
                }
                if let Some(series_index) = error_bar_element(local.as_slice(), &path, current_ser)
                {
                    error_bar_seen_series.insert(series_index);
                    error_bar_state.reset_for_error_bars();
                }
                if local.as_slice() == b"axPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    category_axis_position_seen = true;
                }
                if local.as_slice() == b"axPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    value_axis_position_seen = true;
                }
                if local.as_slice() == b"tickLblPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    category_axis_label_position_seen = true;
                }
                if local.as_slice() == b"tickLblPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    value_axis_label_position_seen = true;
                }
                if let Some(axis) = axis_scaling_element_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => category_axis_scaling_seen = true,
                        AxisKind::Value => value_axis_scaling_seen = true,
                    }
                }
                if let Some(axis) = axis_scaling_insertion_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => {
                            write_missing_axis_scaling_with_orientation(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Category,
                                category_axis_scaling_seen,
                                update,
                                &mut category_axis_orientation_updated,
                                &mut value_axis_log_base_updated,
                                &mut value_axis_maximum_updated,
                                &mut value_axis_minimum_updated,
                            )?;
                        }
                        AxisKind::Value => {
                            write_missing_axis_scaling_with_orientation(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Value,
                                value_axis_scaling_seen,
                                update,
                                &mut value_axis_orientation_updated,
                                &mut value_axis_log_base_updated,
                                &mut value_axis_maximum_updated,
                                &mut value_axis_minimum_updated,
                            )?;
                        }
                    }
                }
                if let Some(axis) = value_axis_log_base_insertion_kind(local.as_slice(), &path) {
                    write_missing_value_axis_log_base(
                        &mut writer,
                        e.name().as_ref(),
                        axis,
                        update,
                        &mut value_axis_log_base_updated,
                    )?;
                }
                if let Some(axis) = axis_orientation_insertion_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => {
                            write_missing_axis_orientation(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Category,
                                update,
                                &mut category_axis_orientation_updated,
                            )?;
                        }
                        AxisKind::Value => {
                            write_missing_axis_orientation(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Value,
                                update,
                                &mut value_axis_orientation_updated,
                            )?;
                        }
                    }
                }
                if let Some(axis) = axis_position_insertion_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => {
                            write_missing_axis_visibility_delete(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Category,
                                category_axis_delete_seen,
                                update,
                                &mut category_axis_visibility_updated,
                            )?;
                            write_missing_axis_position(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Category,
                                category_axis_position_seen,
                                update,
                                &mut category_axis_position_updated,
                            )?;
                            if axis_title_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_title(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_title_updated,
                                )?;
                            }
                            if axis_number_format_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_number_format(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_number_format_updated,
                                )?;
                            }
                            if let Some(limit) =
                                axis_tick_mark_insertion_limit(local.as_slice(), &path)
                            {
                                write_missing_axis_tick_marks(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    limit,
                                    update,
                                    &mut category_axis_major_tick_mark_updated,
                                    &mut category_axis_minor_tick_mark_updated,
                                )?;
                            }
                            if axis_label_position_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_label_position(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    category_axis_label_position_seen,
                                    update,
                                    &mut category_axis_label_position_updated,
                                )?;
                            }
                            if axis_crosses_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_crosses(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_crosses_updated,
                                )?;
                            }
                            if axis_crosses_at_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_crosses_at(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_crosses_at_updated,
                                )?;
                            }
                            if let Some(limit) =
                                category_axis_label_control_insertion_limit(local.as_slice(), &path)
                            {
                                write_missing_category_axis_label_controls(
                                    &mut writer,
                                    e.name().as_ref(),
                                    limit,
                                    update,
                                    &mut category_axis_auto_updated,
                                    &mut category_axis_label_alignment_updated,
                                    &mut category_axis_label_offset_updated,
                                    &mut category_axis_tick_mark_skip_updated,
                                    &mut category_axis_no_multi_level_labels_updated,
                                )?;
                            }
                        }
                        AxisKind::Value => {
                            write_missing_axis_visibility_delete(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Value,
                                value_axis_delete_seen,
                                update,
                                &mut value_axis_visibility_updated,
                            )?;
                            write_missing_axis_position(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Value,
                                value_axis_position_seen,
                                update,
                                &mut value_axis_position_updated,
                            )?;
                            if axis_title_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_title(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_title_updated,
                                )?;
                            }
                            if axis_number_format_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_number_format(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_number_format_updated,
                                )?;
                            }
                            if let Some(limit) =
                                axis_tick_mark_insertion_limit(local.as_slice(), &path)
                            {
                                write_missing_axis_tick_marks(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    limit,
                                    update,
                                    &mut value_axis_major_tick_mark_updated,
                                    &mut value_axis_minor_tick_mark_updated,
                                )?;
                            }
                            if axis_label_position_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_label_position(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    value_axis_label_position_seen,
                                    update,
                                    &mut value_axis_label_position_updated,
                                )?;
                            }
                            if axis_crosses_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_crosses(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_crosses_updated,
                                )?;
                            }
                            if axis_crosses_at_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_crosses_at(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_crosses_at_updated,
                                )?;
                            }
                            if is_value_axis_cross_between_insertion_point(local.as_slice(), &path)
                            {
                                write_missing_value_axis_cross_between(
                                    &mut writer,
                                    e.name().as_ref(),
                                    update,
                                    &mut value_axis_cross_between_updated,
                                )?;
                            }
                            if is_value_axis_display_units_insertion_point(local.as_slice(), &path)
                            {
                                write_missing_value_axis_display_units(
                                    &mut writer,
                                    e.name().as_ref(),
                                    update,
                                    &mut value_axis_display_unit_updated,
                                )?;
                            }
                        }
                    }
                }
                if let Some(kind) = axis_grid_line_update_kind(local.as_slice(), &path, update) {
                    write_axis_grid_lines_with_line(&mut writer, e.name().as_ref(), kind, update)?;
                    skip_element(&mut reader, e.name().as_ref())?;
                    grid_line_state.mark_updated(kind, update);
                    buf.clear();
                    continue;
                }
                if local.as_slice() == b"upDownBars" {
                    stock_bar_state.up_down_bars_seen = true;
                    if has_stock_bar_update(update) {
                        write_stock_up_down_bars_with_style(
                            &mut writer,
                            e.name().as_ref(),
                            &parsed,
                            update,
                        )?;
                        skip_element(&mut reader, e.name().as_ref())?;
                        stock_bar_state.mark_updated(update);
                        buf.clear();
                        continue;
                    }
                }
                if is_stock_hi_low_lines(local.as_slice(), &path) {
                    stock_bar_state.hi_low_lines_seen = true;
                    if has_stock_hi_low_line_update(update) {
                        write_stock_hi_low_lines_with_style(
                            &mut writer,
                            e.name().as_ref(),
                            &parsed,
                            update,
                        )?;
                        skip_element(&mut reader, e.name().as_ref())?;
                        stock_bar_state.mark_hi_low_updated(update);
                        buf.clear();
                        continue;
                    }
                }
                if is_of_pie_ser_lines(local.as_slice(), &path) {
                    pie_of_pie_ser_lines_seen = true;
                    if has_of_pie_ser_line_update(update) {
                        write_of_pie_ser_lines_with_style(
                            &mut writer,
                            e.name().as_ref(),
                            &parsed,
                            update,
                        )?;
                        skip_element(&mut reader, e.name().as_ref())?;
                        if update.pie_of_pie_ser_line_color.is_some() {
                            pie_of_pie_ser_line_color_updated = true;
                        }
                        if update.pie_of_pie_ser_line_width.is_some() {
                            pie_of_pie_ser_line_width_updated = true;
                        }
                        buf.clear();
                        continue;
                    }
                }
                if is_stock_chart_ax_id(local.as_slice(), &path)
                    && has_stock_hi_low_line_update(update)
                    && !stock_bar_state.hi_low_lines_seen
                {
                    write_stock_hi_low_lines_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        &parsed,
                        update,
                    )?;
                    stock_bar_state.mark_hi_low_updated(update);
                }
                if let Some(kind) = axis_grid_line_seen_kind(local.as_slice(), &path) {
                    grid_line_state.mark_seen(kind);
                }
                if local.as_slice() == b"delete"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    category_axis_delete_seen = true;
                }
                if local.as_slice() == b"delete"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    value_axis_delete_seen = true;
                }
                if local.as_slice() == b"legendPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"legend")
                {
                    legend_position_seen = true;
                }
                if local.as_slice() == b"axPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    category_axis_position_seen = true;
                }
                if local.as_slice() == b"axPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    value_axis_position_seen = true;
                }
                if local.as_slice() == b"tickLblPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    category_axis_label_position_seen = true;
                }
                if local.as_slice() == b"tickLblPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    value_axis_label_position_seen = true;
                }
                if is_legend_position_insertion_point(local.as_slice(), &path) {
                    write_missing_legend_position(
                        &mut writer,
                        e.name().as_ref(),
                        legend_position_seen,
                        update,
                        &mut legend_position_updated,
                    )?;
                }
                if let Some(axis) = axis_sp_pr_update_kind(local.as_slice(), &path, update) {
                    write_axis_sp_pr_with_line(&mut writer, e.name().as_ref(), axis, update)?;
                    skip_element(&mut reader, e.name().as_ref())?;
                    match axis {
                        AxisKind::Category => {
                            category_axis_sp_pr_seen = true;
                            if update.category_axis_line_color.is_some() {
                                category_axis_line_color_updated = true;
                            }
                            if update.category_axis_line_width.is_some() {
                                category_axis_line_width_updated = true;
                            }
                        }
                        AxisKind::Value => {
                            value_axis_sp_pr_seen = true;
                            if update.value_axis_line_color.is_some() {
                                value_axis_line_color_updated = true;
                            }
                            if update.value_axis_line_width.is_some() {
                                value_axis_line_width_updated = true;
                            }
                        }
                    }
                    buf.clear();
                    continue;
                }
                if local.as_slice() == b"axPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    write_missing_axis_visibility_delete(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        category_axis_delete_seen,
                        update,
                        &mut category_axis_visibility_updated,
                    )?;
                }
                if local.as_slice() == b"axPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    write_missing_axis_visibility_delete(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        value_axis_delete_seen,
                        update,
                        &mut value_axis_visibility_updated,
                    )?;
                }
                if chart_space_sp_pr_update(local.as_slice(), &path, update).is_some() {
                    write_sp_pr_with_fill_preserving_children(
                        &mut reader,
                        &mut writer,
                        &e,
                        update
                            .chart_area_fill_color
                            .expect("chart area fill update"),
                    )?;
                    chart_area_fill_color_updated = true;
                    buf.clear();
                    continue;
                }
                if plot_area_sp_pr_update(local.as_slice(), &path, update).is_some() {
                    write_sp_pr_with_fill_preserving_children(
                        &mut reader,
                        &mut writer,
                        &e,
                        update.plot_area_fill_color.expect("plot area fill update"),
                    )?;
                    plot_area_fill_color_updated = true;
                    buf.clear();
                    continue;
                }
                if is_data_table_insertion_point(local.as_slice(), &path) && !data_table_seen {
                    write_chart_data_table_with_requested_flags(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut data_table_state,
                    )?;
                    data_table_seen = has_data_table_update(update);
                }
                if local.as_slice() == b"extLst" && is_plot_area_parent_path(&path) {
                    write_missing_plot_area_sp_pr(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut plot_area_fill_color_updated,
                    )?;
                }
                if is_chart_space_late_child(local.as_slice()) && is_chart_space_parent_path(&path)
                {
                    write_missing_chart_space_sp_pr(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut chart_area_fill_color_updated,
                    )?;
                }
                if let Some((series_index, fill_color, line_color, line_width)) =
                    series_style_update(
                        local.as_slice(),
                        &path,
                        current_ser,
                        &parsed,
                        &series_updates,
                    )
                {
                    write_series_sp_pr_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        fill_color,
                        line_color,
                        line_width,
                    )?;
                    skip_element(&mut reader, e.name().as_ref())?;
                    series_sp_pr_seen.insert(series_index);
                    series_style_updated.insert(series_index);
                    buf.clear();
                    continue;
                }
                if let Some((field, value)) =
                    data_label_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    data_label_state.mark_updated(field);
                    buf.clear();
                    continue;
                }
                if is_chart_space_flag_insertion_point(local.as_slice(), &path) {
                    write_missing_chart_space_flags(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut date_1904_updated,
                        &mut rounded_corners_updated,
                    )?;
                }
                if is_chart_style_insertion_point(local.as_slice(), &path) {
                    write_missing_chart_style(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut chart_style_updated,
                    )?;
                }
                if is_auto_title_deleted_insertion_point(local.as_slice(), &path) {
                    write_missing_auto_title_deleted(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut auto_title_deleted_updated,
                    )?;
                }
                if is_view_3d_insertion_point(local.as_slice(), &path) && !view_3d_seen {
                    write_view_3d_with_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut view_3d_rotation_x_updated,
                        &mut view_3d_rotation_y_updated,
                        &mut view_3d_perspective_updated,
                        &mut view_3d_right_angle_axes_updated,
                        &mut view_3d_height_percent_updated,
                        &mut view_3d_depth_percent_updated,
                    )?;
                    view_3d_seen = has_view_3d_update(update);
                }
                if is_vary_colors_insertion_point(local.as_slice(), &path) {
                    write_missing_vary_colors(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut vary_colors_updated,
                    )?;
                }
                if let Some(value) = date_1904_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    date_1904_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = chart_style_update_value(local.as_slice(), &path, &e, update) {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    chart_style_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = rounded_corners_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    rounded_corners_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    auto_title_deleted_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    auto_title_deleted_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = vary_colors_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    vary_colors_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some((field, value)) = view_3d_update_value(local.as_slice(), &path, update)
                {
                    write_missing_view_3d_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        view_3d_child_insertion_limit(local.as_slice(), &path),
                        &mut view_3d_rotation_x_updated,
                        &mut view_3d_rotation_y_updated,
                        &mut view_3d_perspective_updated,
                        &mut view_3d_right_angle_axes_updated,
                        &mut view_3d_height_percent_updated,
                        &mut view_3d_depth_percent_updated,
                    )?;
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    mark_view_3d_field_updated(
                        field,
                        &mut view_3d_rotation_x_updated,
                        &mut view_3d_rotation_y_updated,
                        &mut view_3d_perspective_updated,
                        &mut view_3d_right_angle_axes_updated,
                        &mut view_3d_height_percent_updated,
                        &mut view_3d_depth_percent_updated,
                    );
                    buf.clear();
                    continue;
                }
                if let Some(value) = display_blanks_as_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    display_blanks_as_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = show_hidden_data_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    show_hidden_data_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = plot_visible_only_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    plot_visible_only_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some((field, value)) =
                    data_table_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    data_table_state.mark_updated(field);
                    buf.clear();
                    continue;
                }
                if is_trendline_sp_pr(local.as_slice(), &path)
                    && has_trendline_line_style_update(update)
                {
                    write_sp_pr_with_line(
                        &mut writer,
                        e.name().as_ref(),
                        update.trendline_line_color,
                        update.trendline_line_width,
                    )?;
                    skip_element(&mut reader, e.name().as_ref())?;
                    trendline_state.mark_line_style_updated();
                    buf.clear();
                    continue;
                }
                if is_error_bar_sp_pr(local.as_slice(), &path)
                    && has_error_bar_line_style_update(update)
                {
                    write_sp_pr_with_line(
                        &mut writer,
                        e.name().as_ref(),
                        update.error_bar_line_color,
                        update.error_bar_line_width,
                    )?;
                    skip_element(&mut reader, e.name().as_ref())?;
                    error_bar_state.mark_line_style_updated();
                    buf.clear();
                    continue;
                }
                if let Some((field, value)) =
                    trendline_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    trendline_state.mark_updated(field);
                    buf.clear();
                    continue;
                }
                if let Some((field, value)) =
                    error_bar_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    error_bar_state.mark_updated(field);
                    buf.clear();
                    continue;
                }
                if let Some((scope, value)) = overlay_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    match scope {
                        OverlayScope::Title => title_overlay_updated = true,
                        OverlayScope::Legend => legend_overlay_updated = true,
                    }
                    buf.clear();
                    continue;
                }
                if is_overlay_insertion_point(local.as_slice(), &path, OverlayScope::Title) {
                    write_missing_overlay(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        OverlayScope::Title,
                        &mut title_overlay_updated,
                    )?;
                }
                if is_overlay_insertion_point(local.as_slice(), &path, OverlayScope::Legend) {
                    write_missing_overlay(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        OverlayScope::Legend,
                        &mut legend_overlay_updated,
                    )?;
                }
                if let Some(axis) = axis_sp_pr_seen_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => category_axis_sp_pr_seen = true,
                        AxisKind::Value => value_axis_sp_pr_seen = true,
                    }
                }
                if is_grouping_insertion_point(local.as_slice(), &path) {
                    write_missing_grouping(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_plot,
                        &grouping_seen_plots,
                        &mut grouping_updated_plots,
                        &mut grouping_updated,
                    )?;
                }
                if is_grouping_element(local.as_slice(), &path) {
                    if let Some(plot) = current_plot {
                        grouping_seen_plots.insert(plot);
                    }
                }
                if local.as_slice() == b"axId" && is_bar_chart_parent_path(&path) {
                    write_missing_bar_layout_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut bar_gap_width_updated,
                        &mut bar_overlap_updated,
                    )?;
                }
                if local.as_slice() == b"axId" && is_bar_3d_chart_parent_path(&path) {
                    write_missing_bar_3d_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut bar_3d_gap_depth_updated,
                        &mut bar_3d_shape_updated,
                    )?;
                }
                if let Some(bar_dir) = bar_dir_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", bar_dir)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    bar_dir_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(grouping) = grouping_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", grouping)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    if let Some(plot) = current_plot {
                        grouping_updated_plots.insert(plot);
                    }
                    grouping_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = bar_gap_width_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    bar_gap_width_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = bar_overlap_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    bar_overlap_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = bar_3d_gap_depth_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    bar_3d_gap_depth_updated = true;
                    buf.clear();
                    continue;
                }
                if local.as_slice() == b"shape" && is_bar_3d_chart_parent_path(&path) {
                    write_missing_bar_3d_gap_depth(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut bar_3d_gap_depth_updated,
                    )?;
                }
                if let Some(value) = bar_3d_shape_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    bar_3d_shape_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(limit) = line_marker_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_marker_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Line,
                        current_ser,
                        limit,
                        &mut line_marker_symbol_seen_series,
                        &mut line_marker_size_seen_series,
                        &mut line_marker_style_seen_series,
                        &mut line_marker_symbol_updated,
                        &mut line_marker_size_updated,
                        &mut line_marker_style_updated,
                    )?;
                }
                if let Some(series_index) =
                    line_series_marker_element(local.as_slice(), &path, current_ser)
                {
                    line_marker_seen_series.insert(series_index);
                }
                if let Some(limit) = line_series_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_line_series_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        limit,
                        &mut line_marker_seen_series,
                        &mut line_marker_symbol_seen_series,
                        &mut line_marker_size_seen_series,
                        &mut line_marker_style_seen_series,
                        &mut line_marker_symbol_updated,
                        &mut line_marker_size_updated,
                        &mut line_marker_style_updated,
                        &mut line_smooth_seen_series,
                        &mut line_smooth_updated,
                        &mut trendline_seen_series,
                        &mut trendline_state,
                        &mut error_bar_seen_series,
                        &mut error_bar_state,
                    )?;
                }
                if let Some(limit) = line_chart_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_line_chart_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        limit,
                        &mut line_marker_visible_updated,
                        &mut line_chart_smooth_updated,
                        &mut line_smooth_updated,
                    )?;
                }
                if let Some(value) = line_smooth_update_value(local.as_slice(), &path, update) {
                    let smooth_parent_is_line_chart = is_line_chart_path(&path);
                    let smooth_parent_is_series = is_line_series_path(&path);
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    if smooth_parent_is_line_chart {
                        line_chart_smooth_updated = true;
                    } else if smooth_parent_is_series {
                        if let Some(series_index) = current_ser {
                            line_smooth_seen_series.insert(series_index);
                        }
                    }
                    line_smooth_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    line_marker_visible_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    line_marker_visible_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    line_marker_symbol_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    if let Some(series_index) = current_ser {
                        line_marker_symbol_seen_series.insert(series_index);
                    }
                    line_marker_symbol_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = line_marker_size_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    if let Some(series_index) = current_ser {
                        line_marker_size_seen_series.insert(series_index);
                    }
                    line_marker_size_updated = true;
                    buf.clear();
                    continue;
                }
                if is_line_marker_sp_pr(local.as_slice(), &path, update) {
                    write_marker_sp_pr_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Line,
                    )?;
                    skip_element(&mut reader, e.name().as_ref())?;
                    if let Some(series_index) = current_ser {
                        line_marker_style_seen_series.insert(series_index);
                    }
                    line_marker_style_updated = true;
                    buf.clear();
                    continue;
                }
                if is_pie_first_slice_insertion_point(local.as_slice(), &path) {
                    write_missing_pie_first_slice_angle(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut pie_first_slice_angle_updated,
                    )?;
                }
                if is_doughnut_hole_size_insertion_point(local.as_slice(), &path) {
                    write_missing_doughnut_hole_size(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut doughnut_hole_size_updated,
                    )?;
                }
                if is_pie_explosion_insertion_point(local.as_slice(), &path) {
                    write_missing_pie_explosion(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        &mut pie_explosion_seen_series,
                        &mut pie_explosion_updated,
                    )?;
                }
                if let Some(limit) = of_pie_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_of_pie_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        limit,
                        &mut pie_of_pie_type_updated,
                        &mut pie_of_pie_gap_width_updated,
                        &mut pie_of_pie_second_size_updated,
                        &mut pie_of_pie_ser_lines_seen,
                        &mut pie_of_pie_ser_line_color_updated,
                        &mut pie_of_pie_ser_line_width_updated,
                    )?;
                }
                if let Some(value) =
                    pie_first_slice_angle_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    pie_first_slice_angle_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    doughnut_hole_size_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    doughnut_hole_size_updated = true;
                    buf.clear();
                    continue;
                }
                if is_pie_explosion_element(local.as_slice(), &path) {
                    if let Some(series_index) = current_ser {
                        pie_explosion_seen_series.insert(series_index);
                    }
                }
                if let Some(value) = pie_explosion_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    pie_explosion_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = pie_of_pie_type_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    pie_of_pie_type_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    pie_of_pie_gap_width_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    pie_of_pie_gap_width_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    pie_of_pie_second_size_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    pie_of_pie_second_size_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(limit) = scatter_marker_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_marker_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Scatter,
                        current_ser,
                        limit,
                        &mut scatter_marker_symbol_seen_series,
                        &mut scatter_marker_size_seen_series,
                        &mut scatter_marker_style_seen_series,
                        &mut scatter_marker_symbol_updated,
                        &mut scatter_marker_size_updated,
                        &mut scatter_marker_style_updated,
                    )?;
                }
                if let Some(series_index) =
                    scatter_series_marker_element(local.as_slice(), &path, current_ser)
                {
                    scatter_marker_seen_series.insert(series_index);
                }
                if let Some(limit) = scatter_series_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_scatter_series_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        limit,
                        &mut scatter_marker_seen_series,
                        &mut scatter_marker_symbol_seen_series,
                        &mut scatter_marker_size_seen_series,
                        &mut scatter_marker_style_seen_series,
                        &mut scatter_marker_symbol_updated,
                        &mut scatter_marker_size_updated,
                        &mut scatter_marker_style_updated,
                        &mut scatter_smooth_seen_series,
                        &mut scatter_smooth_updated,
                        &mut trendline_seen_series,
                        &mut trendline_state,
                        &mut error_bar_seen_series,
                        &mut error_bar_state,
                    )?;
                }
                if let Some(limit) = scatter_chart_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_scatter_chart_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        limit,
                        &mut scatter_style_updated,
                    )?;
                }
                if let Some(value) = scatter_style_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    scatter_style_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = scatter_smooth_update_value(local.as_slice(), &path, update) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    if let Some(series_index) = current_ser {
                        scatter_smooth_seen_series.insert(series_index);
                    }
                    scatter_smooth_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    scatter_marker_symbol_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    if let Some(series_index) = current_ser {
                        scatter_marker_symbol_seen_series.insert(series_index);
                    }
                    scatter_marker_symbol_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    scatter_marker_size_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    if let Some(series_index) = current_ser {
                        scatter_marker_size_seen_series.insert(series_index);
                    }
                    scatter_marker_size_updated = true;
                    buf.clear();
                    continue;
                }
                if is_scatter_marker_sp_pr(local.as_slice(), &path, update) {
                    write_marker_sp_pr_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Scatter,
                    )?;
                    skip_element(&mut reader, e.name().as_ref())?;
                    if let Some(series_index) = current_ser {
                        scatter_marker_style_seen_series.insert(series_index);
                    }
                    scatter_marker_style_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(position) =
                    legend_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", position)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    legend_position_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_visibility_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_visibility_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_visibility_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_visibility_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_position_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_position_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_label_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_label_position_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_label_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_label_position_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_auto_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_auto_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_label_alignment_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_label_alignment_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_label_offset_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_label_offset_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_tick_mark_skip_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_tick_mark_skip_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) = category_axis_no_multi_level_labels_update_value(
                    local.as_slice(),
                    &path,
                    update,
                ) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_no_multi_level_labels_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_orientation_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_orientation_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_orientation_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_orientation_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_crosses_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_crosses_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_crosses_at_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value.as_str())?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_crosses_at_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_crosses_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_crosses_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_crosses_at_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value.as_str())?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_crosses_at_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_cross_between_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_cross_between_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_major_tick_mark_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_major_tick_mark_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    category_axis_minor_tick_mark_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    category_axis_minor_tick_mark_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_major_tick_mark_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_major_tick_mark_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_minor_tick_mark_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_minor_tick_mark_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_log_base_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_log_base_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_minimum_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_minimum_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_maximum_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_maximum_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_major_unit_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_major_unit_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_minor_unit_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_minor_unit_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(value) =
                    value_axis_display_unit_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    value_axis_display_unit_updated = true;
                    buf.clear();
                    continue;
                }
                if let Some(axis) = axis_number_format_update_kind(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_axis_num_fmt_attrs(&e, axis, update)?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    path.push(local);
                    match axis {
                        AxisKind::Category => category_axis_number_format_updated = true,
                        AxisKind::Value => value_axis_number_format_updated = true,
                    }
                    buf.clear();
                    continue;
                }
                if let Some(axis) = axis_title_rewrite_kind(local.as_slice(), &path, update) {
                    let title = axis_title_update(axis, update).expect("checked");
                    write_chart_title_with_text(&mut reader, &mut writer, &e, title, None)?;
                    match axis {
                        AxisKind::Category => category_axis_title_updated = true,
                        AxisKind::Value => value_axis_title_updated = true,
                    }
                    buf.clear();
                    continue;
                }
                if should_rewrite_chart_title(local.as_slice(), &path, update) {
                    let title = update.title.as_ref().expect("checked");
                    if write_chart_title_with_text(
                        &mut reader,
                        &mut writer,
                        &e,
                        title,
                        update.title_overlay,
                    )? {
                        title_overlay_updated = true;
                    }
                    buf.clear();
                    continue;
                }
                if should_rebuild_category_cache(local.as_slice(), &path, update) {
                    let categories = update.categories.as_ref().expect("checked");
                    write_rebuilt_cache(&mut reader, &mut writer, &e, CacheKind::Text, categories)?;
                    buf.clear();
                    continue;
                }
                if let Some(values) =
                    values_for_rebuild(local.as_slice(), &path, current_ser, &series_updates)
                {
                    let formatted: Vec<String> = values
                        .iter()
                        .map(|value| format_chart_number(*value))
                        .collect();
                    write_rebuilt_cache(
                        &mut reader,
                        &mut writer,
                        &e,
                        CacheKind::Number,
                        &formatted,
                    )?;
                    buf.clear();
                    continue;
                }
                if should_remove_axis_cross_choice(local.as_slice(), &path, update) {
                    skip_element(&mut reader, e.name().as_ref())?;
                    buf.clear();
                    continue;
                }
                if local.as_slice() == b"ser" {
                    current_ser = Some(ser_index);
                    tx_pt_seq = 0;
                    cat_pt_seq = 0;
                    val_pt_seq = 0;
                } else if local.as_slice() == b"pt" {
                    current_pt_idx = pt_index(&e).or_else(|| {
                        if path_contains(&path, b"tx") && path_contains(&path, b"strCache") {
                            let idx = tx_pt_seq;
                            tx_pt_seq += 1;
                            Some(idx)
                        } else if path_contains(&path, b"cat") && path_contains(&path, b"strCache")
                        {
                            let idx = cat_pt_seq;
                            cat_pt_seq += 1;
                            Some(idx)
                        } else if path_contains(&path, b"val") && path_contains(&path, b"numCache")
                        {
                            let idx = val_pt_seq;
                            val_pt_seq += 1;
                            Some(idx)
                        } else {
                            None
                        }
                    });
                }
                writer
                    .write_event(Event::Start(e.into_owned()))
                    .map_err(|e| e.to_string())?;
                path.push(local);
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(e.name().as_ref()).to_vec();
                if is_chart_plot(local.as_slice()) {
                    data_label_state.reset_for_plot();
                }
                if local.as_slice() == b"view3D"
                    && path.last().is_some_and(|p| p.as_slice() == b"chart")
                {
                    view_3d_seen = true;
                }
                if local.as_slice() == b"delete"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    category_axis_delete_seen = true;
                }
                if local.as_slice() == b"delete"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    value_axis_delete_seen = true;
                }
                if local.as_slice() == b"legendPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"legend")
                {
                    legend_position_seen = true;
                }
                if let Some(axis) = axis_scaling_element_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => {
                            category_axis_scaling_seen = true;
                            if has_axis_scaling_child_update(
                                AxisKind::Category,
                                update,
                                category_axis_orientation_updated,
                                value_axis_log_base_updated,
                                value_axis_maximum_updated,
                                value_axis_minimum_updated,
                            ) {
                                write_axis_scaling_with_requested_children(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_orientation_updated,
                                    &mut value_axis_log_base_updated,
                                    &mut value_axis_maximum_updated,
                                    &mut value_axis_minimum_updated,
                                )?;
                                buf.clear();
                                continue;
                            }
                        }
                        AxisKind::Value => {
                            value_axis_scaling_seen = true;
                            if has_axis_scaling_child_update(
                                AxisKind::Value,
                                update,
                                value_axis_orientation_updated,
                                value_axis_log_base_updated,
                                value_axis_maximum_updated,
                                value_axis_minimum_updated,
                            ) {
                                write_axis_scaling_with_requested_children(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_orientation_updated,
                                    &mut value_axis_log_base_updated,
                                    &mut value_axis_maximum_updated,
                                    &mut value_axis_minimum_updated,
                                )?;
                                buf.clear();
                                continue;
                            }
                        }
                    }
                }
                if is_legend_position_insertion_point(local.as_slice(), &path) {
                    write_missing_legend_position(
                        &mut writer,
                        e.name().as_ref(),
                        legend_position_seen,
                        update,
                        &mut legend_position_updated,
                    )?;
                }
                if is_chart_space_flag_insertion_point(local.as_slice(), &path) {
                    write_missing_chart_space_flags(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut date_1904_updated,
                        &mut rounded_corners_updated,
                    )?;
                }
                if is_chart_style_insertion_point(local.as_slice(), &path) {
                    write_missing_chart_style(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut chart_style_updated,
                    )?;
                }
                if is_auto_title_deleted_insertion_point(local.as_slice(), &path) {
                    write_missing_auto_title_deleted(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut auto_title_deleted_updated,
                    )?;
                }
                if is_view_3d_insertion_point(local.as_slice(), &path) && !view_3d_seen {
                    write_view_3d_with_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut view_3d_rotation_x_updated,
                        &mut view_3d_rotation_y_updated,
                        &mut view_3d_perspective_updated,
                        &mut view_3d_right_angle_axes_updated,
                        &mut view_3d_height_percent_updated,
                        &mut view_3d_depth_percent_updated,
                    )?;
                    view_3d_seen = has_view_3d_update(update);
                }
                if is_vary_colors_insertion_point(local.as_slice(), &path) {
                    write_missing_vary_colors(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut vary_colors_updated,
                    )?;
                }
                if let Some(axis) = axis_scaling_insertion_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => {
                            write_missing_axis_scaling_with_orientation(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Category,
                                category_axis_scaling_seen,
                                update,
                                &mut category_axis_orientation_updated,
                                &mut value_axis_log_base_updated,
                                &mut value_axis_maximum_updated,
                                &mut value_axis_minimum_updated,
                            )?;
                        }
                        AxisKind::Value => {
                            write_missing_axis_scaling_with_orientation(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Value,
                                value_axis_scaling_seen,
                                update,
                                &mut value_axis_orientation_updated,
                                &mut value_axis_log_base_updated,
                                &mut value_axis_maximum_updated,
                                &mut value_axis_minimum_updated,
                            )?;
                        }
                    }
                }
                if let Some(axis) = value_axis_log_base_insertion_kind(local.as_slice(), &path) {
                    write_missing_value_axis_log_base(
                        &mut writer,
                        e.name().as_ref(),
                        axis,
                        update,
                        &mut value_axis_log_base_updated,
                    )?;
                }
                if let Some(axis) = axis_orientation_insertion_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => {
                            write_missing_axis_orientation(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Category,
                                update,
                                &mut category_axis_orientation_updated,
                            )?;
                        }
                        AxisKind::Value => {
                            write_missing_axis_orientation(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Value,
                                update,
                                &mut value_axis_orientation_updated,
                            )?;
                        }
                    }
                }
                if let Some(axis) = axis_position_insertion_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => {
                            write_missing_axis_visibility_delete(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Category,
                                category_axis_delete_seen,
                                update,
                                &mut category_axis_visibility_updated,
                            )?;
                            write_missing_axis_position(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Category,
                                category_axis_position_seen,
                                update,
                                &mut category_axis_position_updated,
                            )?;
                            if axis_title_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_title(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_title_updated,
                                )?;
                            }
                            if axis_number_format_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_number_format(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_number_format_updated,
                                )?;
                            }
                            if let Some(limit) =
                                axis_tick_mark_insertion_limit(local.as_slice(), &path)
                            {
                                write_missing_axis_tick_marks(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    limit,
                                    update,
                                    &mut category_axis_major_tick_mark_updated,
                                    &mut category_axis_minor_tick_mark_updated,
                                )?;
                            }
                            if axis_label_position_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_label_position(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    category_axis_label_position_seen,
                                    update,
                                    &mut category_axis_label_position_updated,
                                )?;
                            }
                            if axis_crosses_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_crosses(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_crosses_updated,
                                )?;
                            }
                            if axis_crosses_at_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Category)
                            {
                                write_missing_axis_crosses_at(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Category,
                                    update,
                                    &mut category_axis_crosses_at_updated,
                                )?;
                            }
                            if let Some(limit) =
                                category_axis_label_control_insertion_limit(local.as_slice(), &path)
                            {
                                write_missing_category_axis_label_controls(
                                    &mut writer,
                                    e.name().as_ref(),
                                    limit,
                                    update,
                                    &mut category_axis_auto_updated,
                                    &mut category_axis_label_alignment_updated,
                                    &mut category_axis_label_offset_updated,
                                    &mut category_axis_tick_mark_skip_updated,
                                    &mut category_axis_no_multi_level_labels_updated,
                                )?;
                            }
                        }
                        AxisKind::Value => {
                            write_missing_axis_visibility_delete(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Value,
                                value_axis_delete_seen,
                                update,
                                &mut value_axis_visibility_updated,
                            )?;
                            write_missing_axis_position(
                                &mut writer,
                                e.name().as_ref(),
                                AxisKind::Value,
                                value_axis_position_seen,
                                update,
                                &mut value_axis_position_updated,
                            )?;
                            if axis_title_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_title(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_title_updated,
                                )?;
                            }
                            if axis_number_format_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_number_format(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_number_format_updated,
                                )?;
                            }
                            if let Some(limit) =
                                axis_tick_mark_insertion_limit(local.as_slice(), &path)
                            {
                                write_missing_axis_tick_marks(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    limit,
                                    update,
                                    &mut value_axis_major_tick_mark_updated,
                                    &mut value_axis_minor_tick_mark_updated,
                                )?;
                            }
                            if axis_label_position_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_label_position(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    value_axis_label_position_seen,
                                    update,
                                    &mut value_axis_label_position_updated,
                                )?;
                            }
                            if axis_crosses_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_crosses(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_crosses_updated,
                                )?;
                            }
                            if axis_crosses_at_insertion_kind(local.as_slice(), &path)
                                == Some(AxisKind::Value)
                            {
                                write_missing_axis_crosses_at(
                                    &mut writer,
                                    e.name().as_ref(),
                                    AxisKind::Value,
                                    update,
                                    &mut value_axis_crosses_at_updated,
                                )?;
                            }
                            if is_value_axis_cross_between_insertion_point(local.as_slice(), &path)
                            {
                                write_missing_value_axis_cross_between(
                                    &mut writer,
                                    e.name().as_ref(),
                                    update,
                                    &mut value_axis_cross_between_updated,
                                )?;
                            }
                            if is_value_axis_display_units_insertion_point(local.as_slice(), &path)
                            {
                                write_missing_value_axis_display_units(
                                    &mut writer,
                                    e.name().as_ref(),
                                    update,
                                    &mut value_axis_display_unit_updated,
                                )?;
                            }
                        }
                    }
                }
                if is_empty_value_axis_display_units(local.as_slice(), &path)
                    && update.value_axis_display_unit.is_some()
                    && !value_axis_display_unit_updated
                {
                    write_missing_value_axis_display_units(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut value_axis_display_unit_updated,
                    )?;
                    buf.clear();
                    continue;
                }
                if is_pie_first_slice_insertion_point(local.as_slice(), &path) {
                    write_missing_pie_first_slice_angle(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut pie_first_slice_angle_updated,
                    )?;
                }
                if is_doughnut_hole_size_insertion_point(local.as_slice(), &path) {
                    write_missing_doughnut_hole_size(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut doughnut_hole_size_updated,
                    )?;
                }
                if is_pie_explosion_insertion_point(local.as_slice(), &path) {
                    write_missing_pie_explosion(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        &mut pie_explosion_seen_series,
                        &mut pie_explosion_updated,
                    )?;
                }
                if let Some(limit) = of_pie_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_of_pie_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        limit,
                        &mut pie_of_pie_type_updated,
                        &mut pie_of_pie_gap_width_updated,
                        &mut pie_of_pie_second_size_updated,
                        &mut pie_of_pie_ser_lines_seen,
                        &mut pie_of_pie_ser_line_color_updated,
                        &mut pie_of_pie_ser_line_width_updated,
                    )?;
                }
                if local.as_slice() == b"axId" && is_bar_chart_parent_path(&path) {
                    write_missing_bar_layout_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut bar_gap_width_updated,
                        &mut bar_overlap_updated,
                    )?;
                }
                if local.as_slice() == b"axId" && is_bar_3d_chart_parent_path(&path) {
                    write_missing_bar_3d_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut bar_3d_gap_depth_updated,
                        &mut bar_3d_shape_updated,
                    )?;
                }
                if is_data_table_insertion_point(local.as_slice(), &path) && !data_table_seen {
                    write_chart_data_table_with_requested_flags(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut data_table_state,
                    )?;
                    data_table_seen = has_data_table_update(update);
                }
                if local.as_slice() == b"extLst" && is_plot_area_parent_path(&path) {
                    write_missing_plot_area_sp_pr(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut plot_area_fill_color_updated,
                    )?;
                }
                if is_chart_space_late_child(local.as_slice()) && is_chart_space_parent_path(&path)
                {
                    write_missing_chart_space_sp_pr(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut chart_area_fill_color_updated,
                    )?;
                }
                if local.as_slice() == b"dTable" && is_plot_area_parent_path(&path) {
                    data_table_seen = true;
                }
                if let Some(series_index) = trendline_element(local.as_slice(), &path, current_ser)
                {
                    trendline_seen_series.insert(series_index);
                    trendline_state.reset_for_trendline();
                }
                if let Some(series_index) = error_bar_element(local.as_slice(), &path, current_ser)
                {
                    error_bar_seen_series.insert(series_index);
                    error_bar_state.reset_for_error_bars();
                    if has_error_bar_update(update) {
                        write_chart_error_bars_with_requested_fields(
                            &mut writer,
                            e.name().as_ref(),
                            update,
                            &mut error_bar_state,
                        )?;
                        buf.clear();
                        continue;
                    }
                }
                if local.as_slice() == b"axPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    write_missing_axis_visibility_delete(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        category_axis_delete_seen,
                        update,
                        &mut category_axis_visibility_updated,
                    )?;
                }
                if local.as_slice() == b"axPos"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    write_missing_axis_visibility_delete(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        value_axis_delete_seen,
                        update,
                        &mut value_axis_visibility_updated,
                    )?;
                }
                if let Some(limit) = line_marker_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_marker_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Line,
                        current_ser,
                        limit,
                        &mut line_marker_symbol_seen_series,
                        &mut line_marker_size_seen_series,
                        &mut line_marker_style_seen_series,
                        &mut line_marker_symbol_updated,
                        &mut line_marker_size_updated,
                        &mut line_marker_style_updated,
                    )?;
                }
                if let Some(series_index) =
                    line_series_marker_element(local.as_slice(), &path, current_ser)
                {
                    line_marker_seen_series.insert(series_index);
                    if has_line_marker_children_update(update) {
                        write_marker_with_requested_children(
                            &mut writer,
                            e.name().as_ref(),
                            update,
                            MarkerFamily::Line,
                            &mut line_marker_symbol_updated,
                            &mut line_marker_size_updated,
                            &mut line_marker_style_updated,
                        )?;
                        if update.line_marker_symbol.is_some() {
                            line_marker_symbol_seen_series.insert(series_index);
                        }
                        if update.line_marker_size.is_some() {
                            line_marker_size_seen_series.insert(series_index);
                        }
                        if has_line_marker_style_update(update) {
                            line_marker_style_seen_series.insert(series_index);
                        }
                        buf.clear();
                        continue;
                    }
                }
                if let Some(limit) = line_series_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_line_series_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        limit,
                        &mut line_marker_seen_series,
                        &mut line_marker_symbol_seen_series,
                        &mut line_marker_size_seen_series,
                        &mut line_marker_style_seen_series,
                        &mut line_marker_symbol_updated,
                        &mut line_marker_size_updated,
                        &mut line_marker_style_updated,
                        &mut line_smooth_seen_series,
                        &mut line_smooth_updated,
                        &mut trendline_seen_series,
                        &mut trendline_state,
                        &mut error_bar_seen_series,
                        &mut error_bar_state,
                    )?;
                }
                if let Some(limit) = line_chart_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_line_chart_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        limit,
                        &mut line_marker_visible_updated,
                        &mut line_chart_smooth_updated,
                        &mut line_smooth_updated,
                    )?;
                }
                if is_grouping_insertion_point(local.as_slice(), &path) {
                    write_missing_grouping(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_plot,
                        &grouping_seen_plots,
                        &mut grouping_updated_plots,
                        &mut grouping_updated,
                    )?;
                }
                if is_grouping_element(local.as_slice(), &path) {
                    if let Some(plot) = current_plot {
                        grouping_seen_plots.insert(plot);
                    }
                }
                if let Some(limit) = scatter_marker_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_marker_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Scatter,
                        current_ser,
                        limit,
                        &mut scatter_marker_symbol_seen_series,
                        &mut scatter_marker_size_seen_series,
                        &mut scatter_marker_style_seen_series,
                        &mut scatter_marker_symbol_updated,
                        &mut scatter_marker_size_updated,
                        &mut scatter_marker_style_updated,
                    )?;
                }
                if let Some(series_index) =
                    scatter_series_marker_element(local.as_slice(), &path, current_ser)
                {
                    scatter_marker_seen_series.insert(series_index);
                    if has_scatter_marker_children_update(update) {
                        write_marker_with_requested_children(
                            &mut writer,
                            e.name().as_ref(),
                            update,
                            MarkerFamily::Scatter,
                            &mut scatter_marker_symbol_updated,
                            &mut scatter_marker_size_updated,
                            &mut scatter_marker_style_updated,
                        )?;
                        if update.scatter_marker_symbol.is_some() {
                            scatter_marker_symbol_seen_series.insert(series_index);
                        }
                        if update.scatter_marker_size.is_some() {
                            scatter_marker_size_seen_series.insert(series_index);
                        }
                        if has_scatter_marker_style_update(update) {
                            scatter_marker_style_seen_series.insert(series_index);
                        }
                        buf.clear();
                        continue;
                    }
                }
                if let Some(limit) = scatter_series_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_scatter_series_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        limit,
                        &mut scatter_marker_seen_series,
                        &mut scatter_marker_symbol_seen_series,
                        &mut scatter_marker_size_seen_series,
                        &mut scatter_marker_style_seen_series,
                        &mut scatter_marker_symbol_updated,
                        &mut scatter_marker_size_updated,
                        &mut scatter_marker_style_updated,
                        &mut scatter_smooth_seen_series,
                        &mut scatter_smooth_updated,
                        &mut trendline_seen_series,
                        &mut trendline_state,
                        &mut error_bar_seen_series,
                        &mut error_bar_state,
                    )?;
                }
                if let Some(limit) = scatter_chart_child_insertion_limit(local.as_slice(), &path) {
                    write_missing_scatter_chart_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        limit,
                        &mut scatter_style_updated,
                    )?;
                }
                if should_remove_axis_cross_choice(local.as_slice(), &path, update) {
                    buf.clear();
                    continue;
                }
                if local.as_slice() == b"dTable"
                    && is_plot_area_parent_path(&path)
                    && has_data_table_update(update)
                {
                    write_chart_data_table_with_requested_flags(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut data_table_state,
                    )?;
                } else if let Some(axis) = axis_title_rewrite_kind(local.as_slice(), &path, update)
                {
                    let title = axis_title_update(axis, update).expect("checked");
                    write_axis_title_with_text(&mut writer, e.name().as_ref(), title)?;
                    match axis {
                        AxisKind::Category => category_axis_title_updated = true,
                        AxisKind::Value => value_axis_title_updated = true,
                    }
                } else if let Some(kind) =
                    axis_grid_line_update_kind(local.as_slice(), &path, update)
                {
                    write_axis_grid_lines_with_line(&mut writer, e.name().as_ref(), kind, update)?;
                    grid_line_state.mark_updated(kind, update);
                } else if local.as_slice() == b"dLbls" {
                    data_label_state.current_plot_has_data_labels = true;
                    if has_data_label_update(update) {
                        write_data_labels(&mut writer, e.name().as_ref(), update)?;
                        data_label_state.mark_requested_updated(update);
                    } else {
                        writer
                            .write_event(Event::Empty(e.into_owned()))
                            .map_err(|e| e.to_string())?;
                    }
                } else if local.as_slice() == b"upDownBars" {
                    stock_bar_state.up_down_bars_seen = true;
                    if has_stock_bar_update(update) {
                        write_stock_up_down_bars_with_style(
                            &mut writer,
                            e.name().as_ref(),
                            &parsed,
                            update,
                        )?;
                        stock_bar_state.mark_updated(update);
                    } else {
                        writer
                            .write_event(Event::Empty(e.into_owned()))
                            .map_err(|e| e.to_string())?;
                    }
                } else if is_stock_hi_low_lines(local.as_slice(), &path) {
                    stock_bar_state.hi_low_lines_seen = true;
                    if has_stock_hi_low_line_update(update) {
                        write_stock_hi_low_lines_with_style(
                            &mut writer,
                            e.name().as_ref(),
                            &parsed,
                            update,
                        )?;
                        stock_bar_state.mark_hi_low_updated(update);
                    } else {
                        writer
                            .write_event(Event::Empty(e.into_owned()))
                            .map_err(|e| e.to_string())?;
                    }
                } else if is_of_pie_ser_lines(local.as_slice(), &path) {
                    pie_of_pie_ser_lines_seen = true;
                    if has_of_pie_ser_line_update(update) {
                        write_of_pie_ser_lines_with_style(
                            &mut writer,
                            e.name().as_ref(),
                            &parsed,
                            update,
                        )?;
                        if update.pie_of_pie_ser_line_color.is_some() {
                            pie_of_pie_ser_line_color_updated = true;
                        }
                        if update.pie_of_pie_ser_line_width.is_some() {
                            pie_of_pie_ser_line_width_updated = true;
                        }
                    } else {
                        writer
                            .write_event(Event::Empty(e.into_owned()))
                            .map_err(|e| e.to_string())?;
                    }
                } else if is_stock_chart_ax_id(local.as_slice(), &path)
                    && has_stock_hi_low_line_update(update)
                    && !stock_bar_state.hi_low_lines_seen
                {
                    write_stock_hi_low_lines_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        &parsed,
                        update,
                    )?;
                    stock_bar_state.mark_hi_low_updated(update);
                    writer
                        .write_event(Event::Empty(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                } else if let Some(kind) = axis_grid_line_seen_kind(local.as_slice(), &path) {
                    grid_line_state.mark_seen(kind);
                    writer
                        .write_event(Event::Empty(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                } else if let Some(axis) = axis_sp_pr_update_kind(local.as_slice(), &path, update) {
                    write_axis_sp_pr_with_line(&mut writer, e.name().as_ref(), axis, update)?;
                    match axis {
                        AxisKind::Category => {
                            category_axis_sp_pr_seen = true;
                            if update.category_axis_line_color.is_some() {
                                category_axis_line_color_updated = true;
                            }
                            if update.category_axis_line_width.is_some() {
                                category_axis_line_width_updated = true;
                            }
                        }
                        AxisKind::Value => {
                            value_axis_sp_pr_seen = true;
                            if update.value_axis_line_color.is_some() {
                                value_axis_line_color_updated = true;
                            }
                            if update.value_axis_line_width.is_some() {
                                value_axis_line_width_updated = true;
                            }
                        }
                    }
                } else if let Some(axis) = axis_sp_pr_seen_kind(local.as_slice(), &path) {
                    match axis {
                        AxisKind::Category => category_axis_sp_pr_seen = true,
                        AxisKind::Value => value_axis_sp_pr_seen = true,
                    }
                    writer
                        .write_event(Event::Empty(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                } else if chart_space_sp_pr_update(local.as_slice(), &path, update).is_some() {
                    write_sp_pr_with_fill(
                        &mut writer,
                        e.name().as_ref(),
                        update.chart_area_fill_color,
                    )?;
                    chart_area_fill_color_updated = true;
                } else if plot_area_sp_pr_update(local.as_slice(), &path, update).is_some() {
                    write_sp_pr_with_fill(
                        &mut writer,
                        e.name().as_ref(),
                        update.plot_area_fill_color,
                    )?;
                    plot_area_fill_color_updated = true;
                } else if let Some((series_index, fill_color, line_color, line_width)) =
                    series_style_update(
                        local.as_slice(),
                        &path,
                        current_ser,
                        &parsed,
                        &series_updates,
                    )
                {
                    write_series_sp_pr_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        fill_color,
                        line_color,
                        line_width,
                    )?;
                    series_sp_pr_seen.insert(series_index);
                    series_style_updated.insert(series_index);
                } else if let Some((field, value)) =
                    data_label_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    data_label_state.mark_updated(field);
                } else if let Some(value) = date_1904_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    date_1904_updated = true;
                } else if let Some(value) =
                    chart_style_update_value(local.as_slice(), &path, &e, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    chart_style_updated = true;
                } else if let Some(value) =
                    rounded_corners_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    rounded_corners_updated = true;
                } else if let Some(value) =
                    auto_title_deleted_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    auto_title_deleted_updated = true;
                } else if let Some(value) =
                    vary_colors_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    vary_colors_updated = true;
                } else if local.as_slice() == b"view3D"
                    && path.last().is_some_and(|p| p.as_slice() == b"chart")
                    && has_view_3d_update(update)
                {
                    write_view_3d_with_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut view_3d_rotation_x_updated,
                        &mut view_3d_rotation_y_updated,
                        &mut view_3d_perspective_updated,
                        &mut view_3d_right_angle_axes_updated,
                        &mut view_3d_height_percent_updated,
                        &mut view_3d_depth_percent_updated,
                    )?;
                } else if let Some((field, value)) =
                    view_3d_update_value(local.as_slice(), &path, update)
                {
                    write_missing_view_3d_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        view_3d_child_insertion_limit(local.as_slice(), &path),
                        &mut view_3d_rotation_x_updated,
                        &mut view_3d_rotation_y_updated,
                        &mut view_3d_perspective_updated,
                        &mut view_3d_right_angle_axes_updated,
                        &mut view_3d_height_percent_updated,
                        &mut view_3d_depth_percent_updated,
                    )?;
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    mark_view_3d_field_updated(
                        field,
                        &mut view_3d_rotation_x_updated,
                        &mut view_3d_rotation_y_updated,
                        &mut view_3d_perspective_updated,
                        &mut view_3d_right_angle_axes_updated,
                        &mut view_3d_height_percent_updated,
                        &mut view_3d_depth_percent_updated,
                    );
                } else if let Some(value) =
                    display_blanks_as_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    display_blanks_as_updated = true;
                } else if let Some(value) =
                    show_hidden_data_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    show_hidden_data_updated = true;
                } else if let Some(value) =
                    plot_visible_only_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    plot_visible_only_updated = true;
                } else if let Some((field, value)) =
                    data_table_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    data_table_state.mark_updated(field);
                } else if let Some(series_index) =
                    trendline_element(local.as_slice(), &path, current_ser)
                {
                    trendline_seen_series.insert(series_index);
                    trendline_state.reset_for_trendline();
                    if has_trendline_update(update) {
                        write_chart_trendline_with_requested_fields(
                            &mut writer,
                            e.name().as_ref(),
                            update,
                            &mut trendline_state,
                        )?;
                    } else {
                        writer
                            .write_event(Event::Empty(e.to_owned()))
                            .map_err(|e| e.to_string())?;
                    }
                } else if is_trendline_sp_pr(local.as_slice(), &path)
                    && has_trendline_line_style_update(update)
                {
                    write_sp_pr_with_line(
                        &mut writer,
                        e.name().as_ref(),
                        update.trendline_line_color,
                        update.trendline_line_width,
                    )?;
                    trendline_state.mark_line_style_updated();
                } else if is_error_bar_sp_pr(local.as_slice(), &path)
                    && has_error_bar_line_style_update(update)
                {
                    write_sp_pr_with_line(
                        &mut writer,
                        e.name().as_ref(),
                        update.error_bar_line_color,
                        update.error_bar_line_width,
                    )?;
                    error_bar_state.mark_line_style_updated();
                } else if let Some((field, value)) =
                    trendline_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    trendline_state.mark_updated(field);
                } else if let Some((field, value)) =
                    error_bar_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    error_bar_state.mark_updated(field);
                } else if let Some((scope, value)) =
                    overlay_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    match scope {
                        OverlayScope::Title => title_overlay_updated = true,
                        OverlayScope::Legend => legend_overlay_updated = true,
                    }
                } else if is_overlay_insertion_point(local.as_slice(), &path, OverlayScope::Title) {
                    write_missing_overlay(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        OverlayScope::Title,
                        &mut title_overlay_updated,
                    )?;
                    writer
                        .write_event(Event::Empty(e.to_owned()))
                        .map_err(|e| e.to_string())?;
                } else if is_overlay_insertion_point(local.as_slice(), &path, OverlayScope::Legend)
                {
                    write_missing_overlay(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        OverlayScope::Legend,
                        &mut legend_overlay_updated,
                    )?;
                    writer
                        .write_event(Event::Empty(e.to_owned()))
                        .map_err(|e| e.to_string())?;
                } else if let Some(bar_dir) = bar_dir_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", bar_dir)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    bar_dir_updated = true;
                } else if let Some(grouping) =
                    grouping_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", grouping)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    if let Some(plot) = current_plot {
                        grouping_updated_plots.insert(plot);
                    }
                    grouping_updated = true;
                } else if let Some(value) =
                    bar_gap_width_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    bar_gap_width_updated = true;
                } else if let Some(value) =
                    bar_overlap_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    bar_overlap_updated = true;
                } else if let Some(value) =
                    bar_3d_gap_depth_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    bar_3d_gap_depth_updated = true;
                } else if let Some(value) =
                    bar_3d_shape_update_value(local.as_slice(), &path, update)
                {
                    write_missing_bar_3d_gap_depth(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut bar_3d_gap_depth_updated,
                    )?;
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    bar_3d_shape_updated = true;
                } else if let Some(value) =
                    line_smooth_update_value(local.as_slice(), &path, update)
                {
                    let smooth_parent_is_line_chart = is_line_chart_path(&path);
                    let smooth_parent_is_series = is_line_series_path(&path);
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    if smooth_parent_is_line_chart {
                        line_chart_smooth_updated = true;
                    } else if smooth_parent_is_series {
                        if let Some(series_index) = current_ser {
                            line_smooth_seen_series.insert(series_index);
                        }
                    }
                    line_smooth_updated = true;
                } else if let Some(value) =
                    line_marker_visible_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    line_marker_visible_updated = true;
                } else if let Some(value) =
                    line_marker_symbol_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    if let Some(series_index) = current_ser {
                        line_marker_symbol_seen_series.insert(series_index);
                    }
                    line_marker_symbol_updated = true;
                } else if let Some(value) =
                    line_marker_size_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    if let Some(series_index) = current_ser {
                        line_marker_size_seen_series.insert(series_index);
                    }
                    line_marker_size_updated = true;
                } else if is_line_marker_sp_pr(local.as_slice(), &path, update) {
                    write_marker_sp_pr_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Line,
                    )?;
                    if let Some(series_index) = current_ser {
                        line_marker_style_seen_series.insert(series_index);
                    }
                    line_marker_style_updated = true;
                } else if let Some(value) =
                    pie_first_slice_angle_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    pie_first_slice_angle_updated = true;
                } else if let Some(value) =
                    doughnut_hole_size_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    doughnut_hole_size_updated = true;
                } else if let Some(value) =
                    pie_explosion_update_value(local.as_slice(), &path, update)
                {
                    if let Some(series_index) = current_ser {
                        pie_explosion_seen_series.insert(series_index);
                    }
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    pie_explosion_updated = true;
                } else if let Some(value) =
                    pie_of_pie_type_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    pie_of_pie_type_updated = true;
                } else if let Some(value) =
                    pie_of_pie_gap_width_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    pie_of_pie_gap_width_updated = true;
                } else if let Some(value) =
                    pie_of_pie_second_size_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    pie_of_pie_second_size_updated = true;
                } else if let Some(value) =
                    scatter_style_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    scatter_style_updated = true;
                } else if let Some(value) =
                    scatter_smooth_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    if let Some(series_index) = current_ser {
                        scatter_smooth_seen_series.insert(series_index);
                    }
                    scatter_smooth_updated = true;
                } else if let Some(value) =
                    scatter_marker_symbol_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    if let Some(series_index) = current_ser {
                        scatter_marker_symbol_seen_series.insert(series_index);
                    }
                    scatter_marker_symbol_updated = true;
                } else if let Some(value) =
                    scatter_marker_size_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    if let Some(series_index) = current_ser {
                        scatter_marker_size_seen_series.insert(series_index);
                    }
                    scatter_marker_size_updated = true;
                } else if is_scatter_marker_sp_pr(local.as_slice(), &path, update) {
                    write_marker_sp_pr_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Scatter,
                    )?;
                    if let Some(series_index) = current_ser {
                        scatter_marker_style_seen_series.insert(series_index);
                    }
                    scatter_marker_style_updated = true;
                } else if let Some(position) =
                    legend_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", position)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    legend_position_updated = true;
                } else if let Some(value) =
                    category_axis_visibility_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_visibility_updated = true;
                } else if let Some(value) =
                    value_axis_visibility_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_visibility_updated = true;
                } else if let Some(value) =
                    category_axis_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_position_updated = true;
                } else if let Some(value) =
                    value_axis_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_position_updated = true;
                } else if let Some(value) =
                    category_axis_label_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_label_position_updated = true;
                } else if let Some(value) =
                    value_axis_label_position_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_label_position_updated = true;
                } else if let Some(value) =
                    category_axis_auto_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_auto_updated = true;
                } else if let Some(value) =
                    category_axis_label_alignment_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_label_alignment_updated = true;
                } else if let Some(value) =
                    category_axis_label_offset_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_label_offset_updated = true;
                } else if let Some(value) =
                    category_axis_tick_mark_skip_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_tick_mark_skip_updated = true;
                } else if let Some(value) = category_axis_no_multi_level_labels_update_value(
                    local.as_slice(),
                    &path,
                    update,
                ) {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_no_multi_level_labels_updated = true;
                } else if let Some(value) =
                    category_axis_orientation_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_orientation_updated = true;
                } else if let Some(value) =
                    value_axis_orientation_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_orientation_updated = true;
                } else if let Some(value) =
                    category_axis_crosses_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_crosses_updated = true;
                } else if let Some(value) =
                    category_axis_crosses_at_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value.as_str())?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_crosses_at_updated = true;
                } else if let Some(value) =
                    value_axis_crosses_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_crosses_updated = true;
                } else if let Some(value) =
                    value_axis_crosses_at_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value.as_str())?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_crosses_at_updated = true;
                } else if let Some(value) =
                    value_axis_cross_between_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_cross_between_updated = true;
                } else if let Some(value) =
                    category_axis_major_tick_mark_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_major_tick_mark_updated = true;
                } else if let Some(value) =
                    category_axis_minor_tick_mark_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    category_axis_minor_tick_mark_updated = true;
                } else if let Some(value) =
                    value_axis_major_tick_mark_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_major_tick_mark_updated = true;
                } else if let Some(value) =
                    value_axis_minor_tick_mark_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_minor_tick_mark_updated = true;
                } else if let Some(value) =
                    value_axis_log_base_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_log_base_updated = true;
                } else if let Some(value) =
                    value_axis_minimum_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_minimum_updated = true;
                } else if let Some(value) =
                    value_axis_maximum_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_maximum_updated = true;
                } else if let Some(value) =
                    value_axis_major_unit_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_major_unit_updated = true;
                } else if let Some(value) =
                    value_axis_minor_unit_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", &value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_minor_unit_updated = true;
                } else if let Some(value) =
                    value_axis_display_unit_update_value(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_attr(&e, b"val", value)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    value_axis_display_unit_updated = true;
                } else if let Some(axis) =
                    axis_number_format_update_kind(local.as_slice(), &path, update)
                {
                    let edited = start_with_replaced_axis_num_fmt_attrs(&e, axis, update)?;
                    writer
                        .write_event(Event::Empty(edited))
                        .map_err(|e| e.to_string())?;
                    match axis {
                        AxisKind::Category => category_axis_number_format_updated = true,
                        AxisKind::Value => value_axis_number_format_updated = true,
                    }
                } else {
                    writer
                        .write_event(Event::Empty(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(replacement) =
                    replacement_text(&path, current_ser, current_pt_idx, update, &series_updates)
                {
                    writer
                        .write_event(Event::Text(BytesText::new(&replacement)))
                        .map_err(|e| e.to_string())?;
                } else {
                    writer
                        .write_event(Event::Text(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::CData(e)) => {
                writer
                    .write_event(Event::CData(e.into_owned()))
                    .map_err(|e| e.to_string())?;
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name().as_ref()).to_vec();
                if local.as_slice() == b"dLbls"
                    && path.last().is_some_and(|p| p.as_slice() == b"dLbls")
                    && has_data_label_update(update)
                {
                    write_missing_data_label_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut data_label_state,
                    )?;
                }
                if local.as_slice() == b"dTable"
                    && is_data_table_path(&path)
                    && has_data_table_update(update)
                {
                    write_missing_data_table_flags(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut data_table_state,
                    )?;
                }
                if local.as_slice() == b"errBars"
                    && is_error_bar_path(&path)
                    && has_error_bar_update(update)
                {
                    write_missing_error_bar_fields(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut error_bar_state,
                    )?;
                }
                if local.as_slice() == b"trendline"
                    && is_trendline_path(&path)
                    && has_trendline_update(update)
                {
                    write_missing_trendline_fields(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut trendline_state,
                    )?;
                }
                if local.as_slice() == b"dispUnits" && is_value_axis_display_units_path(&path) {
                    write_missing_value_axis_display_unit(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut value_axis_display_unit_updated,
                    )?;
                }
                if local.as_slice() == b"scaling" && is_category_axis_scaling_path(&path) {
                    write_missing_axis_orientation(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        update,
                        &mut category_axis_orientation_updated,
                    )?;
                }
                if local.as_slice() == b"scaling" && is_value_axis_scaling_path(&path) {
                    let prefix_source = e.name().as_ref().to_vec();
                    write_missing_value_axis_log_base(
                        &mut writer,
                        &prefix_source,
                        AxisKind::Value,
                        update,
                        &mut value_axis_log_base_updated,
                    )?;
                    write_missing_axis_orientation(
                        &mut writer,
                        &prefix_source,
                        AxisKind::Value,
                        update,
                        &mut value_axis_orientation_updated,
                    )?;
                    if let Some(value) = update.value_axis_maximum {
                        if !value_axis_maximum_updated {
                            write_axis_number_empty(&mut writer, &prefix_source, "max", value)?;
                            value_axis_maximum_updated = true;
                        }
                    }
                    if let Some(value) = update.value_axis_minimum {
                        if !value_axis_minimum_updated {
                            write_axis_number_empty(&mut writer, &prefix_source, "min", value)?;
                            value_axis_minimum_updated = true;
                        }
                    }
                }
                if local.as_slice() == b"valAx"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    write_missing_axis_scaling_with_orientation(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        value_axis_scaling_seen,
                        update,
                        &mut value_axis_orientation_updated,
                        &mut value_axis_log_base_updated,
                        &mut value_axis_maximum_updated,
                        &mut value_axis_minimum_updated,
                    )?;
                    write_missing_axis_visibility_delete(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        value_axis_delete_seen,
                        update,
                        &mut value_axis_visibility_updated,
                    )?;
                    write_missing_axis_position(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        value_axis_position_seen,
                        update,
                        &mut value_axis_position_updated,
                    )?;
                    write_missing_axis_title(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        update,
                        &mut value_axis_title_updated,
                    )?;
                    write_missing_axis_number_format(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        update,
                        &mut value_axis_number_format_updated,
                    )?;
                    write_missing_axis_tick_marks(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        2,
                        update,
                        &mut value_axis_major_tick_mark_updated,
                        &mut value_axis_minor_tick_mark_updated,
                    )?;
                    write_missing_axis_label_position(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        value_axis_label_position_seen,
                        update,
                        &mut value_axis_label_position_updated,
                    )?;
                    write_missing_axis_crosses(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        update,
                        &mut value_axis_crosses_updated,
                    )?;
                    write_missing_axis_crosses_at(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        update,
                        &mut value_axis_crosses_at_updated,
                    )?;
                    write_missing_value_axis_cross_between(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut value_axis_cross_between_updated,
                    )?;
                    let prefix_source = e.name().as_ref().to_vec();
                    if let Some(value) = update.value_axis_major_unit {
                        if !value_axis_major_unit_updated {
                            write_axis_number_empty(
                                &mut writer,
                                &prefix_source,
                                "majorUnit",
                                value,
                            )?;
                            value_axis_major_unit_updated = true;
                        }
                    }
                    if let Some(value) = update.value_axis_minor_unit {
                        if !value_axis_minor_unit_updated {
                            write_axis_number_empty(
                                &mut writer,
                                &prefix_source,
                                "minorUnit",
                                value,
                            )?;
                            value_axis_minor_unit_updated = true;
                        }
                    }
                    write_missing_value_axis_display_units(
                        &mut writer,
                        &prefix_source,
                        update,
                        &mut value_axis_display_unit_updated,
                    )?;
                }
                if local.as_slice() == b"catAx"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    write_missing_axis_scaling_with_orientation(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        category_axis_scaling_seen,
                        update,
                        &mut category_axis_orientation_updated,
                        &mut value_axis_log_base_updated,
                        &mut value_axis_maximum_updated,
                        &mut value_axis_minimum_updated,
                    )?;
                    write_missing_axis_visibility_delete(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        category_axis_delete_seen,
                        update,
                        &mut category_axis_visibility_updated,
                    )?;
                    write_missing_axis_position(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        category_axis_position_seen,
                        update,
                        &mut category_axis_position_updated,
                    )?;
                    write_missing_axis_title(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        update,
                        &mut category_axis_title_updated,
                    )?;
                    write_missing_axis_number_format(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        update,
                        &mut category_axis_number_format_updated,
                    )?;
                    write_missing_axis_tick_marks(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        2,
                        update,
                        &mut category_axis_major_tick_mark_updated,
                        &mut category_axis_minor_tick_mark_updated,
                    )?;
                    write_missing_axis_label_position(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        category_axis_label_position_seen,
                        update,
                        &mut category_axis_label_position_updated,
                    )?;
                    for kind in [
                        AxisGridLineKind::CategoryMajor,
                        AxisGridLineKind::CategoryMinor,
                    ] {
                        if !grid_line_state.is_seen(kind) && has_axis_grid_line_update(kind, update)
                        {
                            write_axis_grid_lines_with_line(
                                &mut writer,
                                e.name().as_ref(),
                                kind,
                                update,
                            )?;
                            grid_line_state.mark_updated(kind, update);
                        }
                    }
                }
                if local.as_slice() == b"valAx"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                {
                    for kind in [AxisGridLineKind::ValueMajor, AxisGridLineKind::ValueMinor] {
                        if !grid_line_state.is_seen(kind) && has_axis_grid_line_update(kind, update)
                        {
                            write_axis_grid_lines_with_line(
                                &mut writer,
                                e.name().as_ref(),
                                kind,
                                update,
                            )?;
                            grid_line_state.mark_updated(kind, update);
                        }
                    }
                }
                if local.as_slice() == b"catAx"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                    && !category_axis_sp_pr_seen
                    && has_axis_line_update(AxisKind::Category, update)
                {
                    write_axis_sp_pr_with_line(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        update,
                    )?;
                    if update.category_axis_line_color.is_some() {
                        category_axis_line_color_updated = true;
                    }
                    if update.category_axis_line_width.is_some() {
                        category_axis_line_width_updated = true;
                    }
                }
                if local.as_slice() == b"catAx"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    write_missing_axis_crosses(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        update,
                        &mut category_axis_crosses_updated,
                    )?;
                    write_missing_axis_crosses_at(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Category,
                        update,
                        &mut category_axis_crosses_at_updated,
                    )?;
                }
                if local.as_slice() == b"catAx"
                    && path.last().is_some_and(|p| p.as_slice() == b"catAx")
                {
                    write_missing_category_axis_label_controls(
                        &mut writer,
                        e.name().as_ref(),
                        5,
                        update,
                        &mut category_axis_auto_updated,
                        &mut category_axis_label_alignment_updated,
                        &mut category_axis_label_offset_updated,
                        &mut category_axis_tick_mark_skip_updated,
                        &mut category_axis_no_multi_level_labels_updated,
                    )?;
                }
                if local.as_slice() == b"valAx"
                    && path.last().is_some_and(|p| p.as_slice() == b"valAx")
                    && !value_axis_sp_pr_seen
                    && has_axis_line_update(AxisKind::Value, update)
                {
                    write_axis_sp_pr_with_line(
                        &mut writer,
                        e.name().as_ref(),
                        AxisKind::Value,
                        update,
                    )?;
                    if update.value_axis_line_color.is_some() {
                        value_axis_line_color_updated = true;
                    }
                    if update.value_axis_line_width.is_some() {
                        value_axis_line_width_updated = true;
                    }
                }
                if is_direct_chart_child_path(&path, b"title") {
                    if let Some(value) = update.title_overlay {
                        if !title_overlay_updated {
                            write_chart_empty_with_val(
                                &mut writer,
                                e.name().as_ref(),
                                "overlay",
                                bool_xml_value(value),
                            )?;
                            title_overlay_updated = true;
                        }
                    }
                }
                if is_direct_chart_child_path(&path, b"legend") {
                    write_missing_legend_position(
                        &mut writer,
                        e.name().as_ref(),
                        legend_position_seen,
                        update,
                        &mut legend_position_updated,
                    )?;
                    if let Some(value) = update.legend_overlay {
                        if !legend_overlay_updated {
                            write_chart_empty_with_val(
                                &mut writer,
                                e.name().as_ref(),
                                "overlay",
                                bool_xml_value(value),
                            )?;
                            legend_overlay_updated = true;
                        }
                    }
                }
                if local.as_slice() == b"view3D"
                    && path.last().is_some_and(|p| p.as_slice() == b"view3D")
                {
                    write_missing_view_3d_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        view_3d_field_order().len(),
                        &mut view_3d_rotation_x_updated,
                        &mut view_3d_rotation_y_updated,
                        &mut view_3d_perspective_updated,
                        &mut view_3d_right_angle_axes_updated,
                        &mut view_3d_height_percent_updated,
                        &mut view_3d_depth_percent_updated,
                    )?;
                }
                if local.as_slice() == b"stockChart"
                    && path.last().is_some_and(|p| p.as_slice() == b"stockChart")
                    && has_stock_bar_update(update)
                    && !stock_bar_state.up_down_bars_seen
                {
                    write_stock_up_down_bars_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        &parsed,
                        update,
                    )?;
                    stock_bar_state.mark_updated(update);
                }
                if local.as_slice() == b"stockChart"
                    && path.last().is_some_and(|p| p.as_slice() == b"stockChart")
                    && has_stock_hi_low_line_update(update)
                    && !stock_bar_state.hi_low_lines_seen
                {
                    write_stock_hi_low_lines_with_style(
                        &mut writer,
                        e.name().as_ref(),
                        &parsed,
                        update,
                    )?;
                    stock_bar_state.mark_hi_low_updated(update);
                }
                if is_grouping_chart_parent_path(&path) {
                    write_missing_grouping(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_plot,
                        &grouping_seen_plots,
                        &mut grouping_updated_plots,
                        &mut grouping_updated,
                    )?;
                }
                if is_chart_plot(local.as_slice())
                    && path
                        .last()
                        .is_some_and(|p| p.as_slice() == local.as_slice())
                {
                    write_missing_vary_colors(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut vary_colors_updated,
                    )?;
                    if has_data_label_update(update)
                        && !data_label_state.current_plot_has_data_labels
                    {
                        write_data_labels(&mut writer, e.name().as_ref(), update)?;
                        data_label_state.mark_requested_updated(update);
                    }
                }
                if is_line_marker_path(&path) {
                    write_missing_marker_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Line,
                        current_ser,
                        3,
                        &mut line_marker_symbol_seen_series,
                        &mut line_marker_size_seen_series,
                        &mut line_marker_style_seen_series,
                        &mut line_marker_symbol_updated,
                        &mut line_marker_size_updated,
                        &mut line_marker_style_updated,
                    )?;
                }
                if is_line_series_path(&path) {
                    write_missing_line_series_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        2,
                        &mut line_marker_seen_series,
                        &mut line_marker_symbol_seen_series,
                        &mut line_marker_size_seen_series,
                        &mut line_marker_style_seen_series,
                        &mut line_marker_symbol_updated,
                        &mut line_marker_size_updated,
                        &mut line_marker_style_updated,
                        &mut line_smooth_seen_series,
                        &mut line_smooth_updated,
                        &mut trendline_seen_series,
                        &mut trendline_state,
                        &mut error_bar_seen_series,
                        &mut error_bar_state,
                    )?;
                }
                if is_line_chart_path(&path) {
                    write_missing_line_chart_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        2,
                        &mut line_marker_visible_updated,
                        &mut line_chart_smooth_updated,
                        &mut line_smooth_updated,
                    )?;
                }
                if is_scatter_marker_path(&path) {
                    write_missing_marker_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        MarkerFamily::Scatter,
                        current_ser,
                        3,
                        &mut scatter_marker_symbol_seen_series,
                        &mut scatter_marker_size_seen_series,
                        &mut scatter_marker_style_seen_series,
                        &mut scatter_marker_symbol_updated,
                        &mut scatter_marker_size_updated,
                        &mut scatter_marker_style_updated,
                    )?;
                }
                if is_scatter_series_path(&path) {
                    write_missing_scatter_series_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        2,
                        &mut scatter_marker_seen_series,
                        &mut scatter_marker_symbol_seen_series,
                        &mut scatter_marker_size_seen_series,
                        &mut scatter_marker_style_seen_series,
                        &mut scatter_marker_symbol_updated,
                        &mut scatter_marker_size_updated,
                        &mut scatter_marker_style_updated,
                        &mut scatter_smooth_seen_series,
                        &mut scatter_smooth_updated,
                        &mut trendline_seen_series,
                        &mut trendline_state,
                        &mut error_bar_seen_series,
                        &mut error_bar_state,
                    )?;
                }
                if is_scatter_chart_path(&path) {
                    write_missing_scatter_chart_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        1,
                        &mut scatter_style_updated,
                    )?;
                }
                if is_pie_first_slice_parent_path(&path) {
                    write_missing_pie_first_slice_angle(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut pie_first_slice_angle_updated,
                    )?;
                }
                if is_doughnut_chart_path(&path) {
                    write_missing_doughnut_hole_size(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut doughnut_hole_size_updated,
                    )?;
                }
                if is_pie_series_path(&path) {
                    write_missing_pie_explosion(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        current_ser,
                        &mut pie_explosion_seen_series,
                        &mut pie_explosion_updated,
                    )?;
                }
                if is_of_pie_chart_path(&path) {
                    write_missing_of_pie_children_until(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        3,
                        &mut pie_of_pie_type_updated,
                        &mut pie_of_pie_gap_width_updated,
                        &mut pie_of_pie_second_size_updated,
                        &mut pie_of_pie_ser_lines_seen,
                        &mut pie_of_pie_ser_line_color_updated,
                        &mut pie_of_pie_ser_line_width_updated,
                    )?;
                }
                if is_bar_chart_element(local.as_slice())
                    && path
                        .last()
                        .is_some_and(|p| p.as_slice() == local.as_slice())
                {
                    write_missing_bar_layout_children(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut bar_gap_width_updated,
                        &mut bar_overlap_updated,
                    )?;
                    if local.as_slice() == b"bar3DChart" {
                        write_missing_bar_3d_children(
                            &mut writer,
                            e.name().as_ref(),
                            update,
                            &mut bar_3d_gap_depth_updated,
                            &mut bar_3d_shape_updated,
                        )?;
                    }
                }
                if local.as_slice() == b"plotArea"
                    && path.last().is_some_and(|p| p.as_slice() == b"plotArea")
                {
                    if !data_table_seen {
                        write_chart_data_table_with_requested_flags(
                            &mut writer,
                            e.name().as_ref(),
                            update,
                            &mut data_table_state,
                        )?;
                        data_table_seen = has_data_table_update(update);
                    }
                    write_missing_plot_area_sp_pr(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut plot_area_fill_color_updated,
                    )?;
                }
                if local.as_slice() == b"chartSpace"
                    && path.last().is_some_and(|p| p.as_slice() == b"chartSpace")
                {
                    write_missing_chart_space_flags(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut date_1904_updated,
                        &mut rounded_corners_updated,
                    )?;
                    write_missing_chart_style(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut chart_style_updated,
                    )?;
                    write_missing_chart_space_sp_pr(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut chart_area_fill_color_updated,
                    )?;
                }
                if local.as_slice() == b"chart"
                    && path.last().is_some_and(|p| p.as_slice() == b"chart")
                {
                    write_missing_auto_title_deleted(
                        &mut writer,
                        e.name().as_ref(),
                        update,
                        &mut auto_title_deleted_updated,
                    )?;
                    if !view_3d_seen {
                        write_view_3d_with_children(
                            &mut writer,
                            e.name().as_ref(),
                            update,
                            &mut view_3d_rotation_x_updated,
                            &mut view_3d_rotation_y_updated,
                            &mut view_3d_perspective_updated,
                            &mut view_3d_right_angle_axes_updated,
                            &mut view_3d_height_percent_updated,
                            &mut view_3d_depth_percent_updated,
                        )?;
                        view_3d_seen = has_view_3d_update(update);
                    }
                    if let Some(value) = update.display_blanks_as {
                        if !display_blanks_as_updated {
                            write_chart_empty_with_val(
                                &mut writer,
                                e.name().as_ref(),
                                "dispBlanksAs",
                                display_blanks_as_xml_value(value),
                            )?;
                            display_blanks_as_updated = true;
                        }
                    }
                    if let Some(value) = update.show_hidden_data {
                        if !show_hidden_data_updated {
                            write_chart_empty_with_val(
                                &mut writer,
                                e.name().as_ref(),
                                "showHiddenData",
                                bool_xml_value(value),
                            )?;
                            show_hidden_data_updated = true;
                        }
                    }
                    if let Some(value) = update.plot_visible_only {
                        if !plot_visible_only_updated {
                            write_chart_empty_with_val(
                                &mut writer,
                                e.name().as_ref(),
                                "plotVisOnly",
                                bool_xml_value(value),
                            )?;
                            plot_visible_only_updated = true;
                        }
                    }
                }
                if local.as_slice() == b"ser" && path.last().is_some_and(|p| p.as_slice() == b"ser")
                {
                    if let Some(series_index) = current_ser {
                        if let Some(update) = series_updates.get(&series_index) {
                            if has_series_style_update(update)
                                && !series_sp_pr_seen.contains(&series_index)
                            {
                                let existing = parsed.series.get(series_index);
                                let fill_color =
                                    update.color.or_else(|| existing.and_then(|s| s.color));
                                let line_color = update
                                    .line_color
                                    .or_else(|| existing.and_then(|s| s.line_color));
                                let line_width = update
                                    .line_width
                                    .or_else(|| existing.and_then(|s| s.line_width));
                                write_series_sp_pr_with_style(
                                    &mut writer,
                                    e.name().as_ref(),
                                    fill_color,
                                    line_color,
                                    line_width,
                                )?;
                                series_sp_pr_seen.insert(series_index);
                                series_style_updated.insert(series_index);
                            }
                        }
                    }
                }
                writer
                    .write_event(Event::End(e.into_owned()))
                    .map_err(|e| e.to_string())?;
                if local.as_slice() == b"pt" {
                    current_pt_idx = None;
                }
                if local.as_slice() == b"ser" {
                    current_ser = None;
                    ser_index += 1;
                }
                if is_chart_plot(local.as_slice()) {
                    current_plot = None;
                }
                path.pop();
            }
            Ok(Event::Decl(e)) => writer
                .write_event(Event::Decl(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::PI(e)) => writer
                .write_event(Event::PI(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::Comment(e)) => writer
                .write_event(Event::Comment(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::DocType(e)) => writer
                .write_event(Event::DocType(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::GeneralRef(e)) => writer
                .write_event(Event::GeneralRef(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("OOXML chart XML 읽기 실패: {e}")),
        }
        buf.clear();
    }

    if update.chart_type.is_some() && !bar_dir_updated {
        return Err("chartType 변경 대상 c:barDir 요소를 찾을 수 없습니다".to_string());
    }
    if update.grouping.is_some() && !grouping_updated {
        return Err(
            "grouping 적용 대상 c:barChart/c:bar3DChart/c:lineChart를 찾을 수 없습니다".to_string(),
        );
    }
    if update.bar_gap_width.is_some() && !bar_gap_width_updated {
        return Err("barGapWidth 변경 대상 c:gapWidth 요소를 찾을 수 없습니다".to_string());
    }
    if update.bar_overlap.is_some() && !bar_overlap_updated {
        return Err("barOverlap 변경 대상 c:overlap 요소를 찾을 수 없습니다".to_string());
    }
    if update.bar_3d_gap_depth.is_some() && !bar_3d_gap_depth_updated {
        return Err("bar3DGapDepth 적용 대상 c:bar3DChart를 찾을 수 없습니다".to_string());
    }
    if update.bar_3d_shape.is_some() && !bar_3d_shape_updated {
        return Err("bar3DShape 적용 대상 c:bar3DChart를 찾을 수 없습니다".to_string());
    }
    if update.line_smooth.is_some() && !line_smooth_updated {
        return Err("lineSmooth 적용 대상 c:lineChart를 찾을 수 없습니다".to_string());
    }
    if update.line_marker_visible.is_some() && !line_marker_visible_updated {
        return Err("lineMarkerVisible 적용 대상 c:lineChart를 찾을 수 없습니다".to_string());
    }
    if update.line_marker_symbol.is_some() && !line_marker_symbol_updated {
        return Err("lineMarkerSymbol 적용 대상 c:lineChart/c:ser를 찾을 수 없습니다".to_string());
    }
    if update.line_marker_size.is_some() && !line_marker_size_updated {
        return Err("lineMarkerSize 적용 대상 c:lineChart/c:ser를 찾을 수 없습니다".to_string());
    }
    if has_line_marker_style_update(update) && !line_marker_style_updated {
        return Err("line marker style 적용 대상 c:lineChart/c:ser를 찾을 수 없습니다".to_string());
    }
    if update.pie_first_slice_angle.is_some() && !pie_first_slice_angle_updated {
        return Err(
            "pieFirstSliceAngle 변경 대상 c:pieChart/c:firstSliceAng 요소를 찾을 수 없습니다"
                .to_string(),
        );
    }
    if update.pie_explosion.is_some() && !pie_explosion_updated {
        return Err(
            "pieExplosion 변경 대상 c:pieChart/c:ser/c:explosion 요소를 찾을 수 없습니다"
                .to_string(),
        );
    }
    if update.doughnut_hole_size.is_some() && !doughnut_hole_size_updated {
        return Err("doughnutHoleSize 적용 대상 c:doughnutChart를 찾을 수 없습니다".to_string());
    }
    if update.pie_of_pie_type.is_some() && !pie_of_pie_type_updated {
        return Err("pieOfPieType 적용 대상 c:ofPieChart를 찾을 수 없습니다".to_string());
    }
    if update.pie_of_pie_gap_width.is_some() && !pie_of_pie_gap_width_updated {
        return Err("pieOfPieGapWidth 적용 대상 c:ofPieChart를 찾을 수 없습니다".to_string());
    }
    if update.pie_of_pie_second_size.is_some() && !pie_of_pie_second_size_updated {
        return Err("pieOfPieSecondSize 적용 대상 c:ofPieChart를 찾을 수 없습니다".to_string());
    }
    if update.pie_of_pie_ser_line_color.is_some() && !pie_of_pie_ser_line_color_updated {
        return Err("pieOfPieSerLineColor 적용 대상 c:ofPieChart를 찾을 수 없습니다".to_string());
    }
    if update.pie_of_pie_ser_line_width.is_some() && !pie_of_pie_ser_line_width_updated {
        return Err("pieOfPieSerLineWidth 적용 대상 c:ofPieChart를 찾을 수 없습니다".to_string());
    }
    if update.scatter_style.is_some() && !scatter_style_updated {
        return Err("scatterStyle 적용 대상 c:scatterChart를 찾을 수 없습니다".to_string());
    }
    if update.scatter_smooth.is_some() && !scatter_smooth_updated {
        return Err("scatterSmooth 적용 대상 c:scatterChart/c:ser를 찾을 수 없습니다".to_string());
    }
    if update.scatter_marker_symbol.is_some() && !scatter_marker_symbol_updated {
        return Err(
            "scatterMarkerSymbol 적용 대상 c:scatterChart/c:ser를 찾을 수 없습니다".to_string(),
        );
    }
    if update.scatter_marker_size.is_some() && !scatter_marker_size_updated {
        return Err(
            "scatterMarkerSize 적용 대상 c:scatterChart/c:ser를 찾을 수 없습니다".to_string(),
        );
    }
    if has_scatter_marker_style_update(update) && !scatter_marker_style_updated {
        return Err(
            "scatter marker style 적용 대상 c:scatterChart/c:ser를 찾을 수 없습니다".to_string(),
        );
    }
    ensure_trendline_updates_applied(update, &trendline_state)?;
    ensure_error_bar_updates_applied(update, &error_bar_state)?;
    if update.legend_position.is_some() && !legend_position_updated {
        return Err("legendPosition 변경 대상 c:legendPos 요소를 찾을 수 없습니다".to_string());
    }
    if update.category_axis_visible.is_some() && !category_axis_visibility_updated {
        return Err(
            "categoryAxisVisible 변경 대상 c:catAx/c:delete 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.value_axis_visible.is_some() && !value_axis_visibility_updated {
        return Err(
            "valueAxisVisible 변경 대상 c:valAx/c:delete 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.category_axis_title.is_some() && !category_axis_title_updated {
        return Err("categoryAxisTitle 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_title.is_some() && !value_axis_title_updated {
        return Err("valueAxisTitle 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.category_axis_position.is_some() && !category_axis_position_updated {
        return Err(
            "categoryAxisPosition 변경 대상 c:catAx/c:axPos 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.value_axis_position.is_some() && !value_axis_position_updated {
        return Err(
            "valueAxisPosition 변경 대상 c:valAx/c:axPos 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.category_axis_label_position.is_some() && !category_axis_label_position_updated {
        return Err(
            "categoryAxisLabelPosition 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string(),
        );
    }
    if update.value_axis_label_position.is_some() && !value_axis_label_position_updated {
        return Err("valueAxisLabelPosition 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.category_axis_auto.is_some() && !category_axis_auto_updated {
        return Err("categoryAxisAuto 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string());
    }
    if update.category_axis_label_alignment.is_some() && !category_axis_label_alignment_updated {
        return Err(
            "categoryAxisLabelAlignment 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string(),
        );
    }
    if update.category_axis_label_offset.is_some() && !category_axis_label_offset_updated {
        return Err("categoryAxisLabelOffset 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string());
    }
    if update.category_axis_tick_mark_skip.is_some() && !category_axis_tick_mark_skip_updated {
        return Err("categoryAxisTickMarkSkip 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string());
    }
    if update.category_axis_no_multi_level_labels.is_some()
        && !category_axis_no_multi_level_labels_updated
    {
        return Err(
            "categoryAxisNoMultiLevelLabels 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string(),
        );
    }
    if update.category_axis_orientation.is_some() && !category_axis_orientation_updated {
        return Err("categoryAxisOrientation 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_orientation.is_some() && !value_axis_orientation_updated {
        return Err("valueAxisOrientation 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.category_axis_crosses.is_some() && !category_axis_crosses_updated {
        return Err("categoryAxisCrosses 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string());
    }
    if update.category_axis_crosses_at.is_some() && !category_axis_crosses_at_updated {
        return Err("categoryAxisCrossesAt 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_crosses.is_some() && !value_axis_crosses_updated {
        return Err("valueAxisCrosses 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_crosses_at.is_some() && !value_axis_crosses_at_updated {
        return Err("valueAxisCrossesAt 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_cross_between.is_some() && !value_axis_cross_between_updated {
        return Err("valueAxisCrossBetween 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.category_axis_major_tick_mark.is_some() && !category_axis_major_tick_mark_updated {
        return Err(
            "categoryAxisMajorTickMark 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string(),
        );
    }
    if update.category_axis_minor_tick_mark.is_some() && !category_axis_minor_tick_mark_updated {
        return Err(
            "categoryAxisMinorTickMark 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string(),
        );
    }
    if update.category_axis_line_color.is_some() && !category_axis_line_color_updated {
        return Err("categoryAxisLineColor 변경 대상 c:catAx 요소를 찾을 수 없습니다".to_string());
    }
    if update.category_axis_line_width.is_some() && !category_axis_line_width_updated {
        return Err("categoryAxisLineWidth 변경 대상 c:catAx 요소를 찾을 수 없습니다".to_string());
    }
    ensure_axis_grid_line_updates_applied(update, &grid_line_state)?;
    if update.value_axis_major_tick_mark.is_some() && !value_axis_major_tick_mark_updated {
        return Err("valueAxisMajorTickMark 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_minor_tick_mark.is_some() && !value_axis_minor_tick_mark_updated {
        return Err("valueAxisMinorTickMark 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_line_color.is_some() && !value_axis_line_color_updated {
        return Err("valueAxisLineColor 변경 대상 c:valAx 요소를 찾을 수 없습니다".to_string());
    }
    if update.value_axis_line_width.is_some() && !value_axis_line_width_updated {
        return Err("valueAxisLineWidth 변경 대상 c:valAx 요소를 찾을 수 없습니다".to_string());
    }
    if update.value_axis_log_base.is_some() && !value_axis_log_base_updated {
        return Err("valueAxisLogBase 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_display_unit.is_some() && !value_axis_display_unit_updated {
        return Err("valueAxisDisplayUnit 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_minimum.is_some() && !value_axis_minimum_updated {
        return Err("valueAxisMinimum 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_maximum.is_some() && !value_axis_maximum_updated {
        return Err("valueAxisMaximum 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    if update.value_axis_major_unit.is_some() && !value_axis_major_unit_updated {
        return Err("valueAxisMajorUnit 변경 대상 c:valAx 요소를 찾을 수 없습니다".to_string());
    }
    if update.value_axis_minor_unit.is_some() && !value_axis_minor_unit_updated {
        return Err("valueAxisMinorUnit 변경 대상 c:valAx 요소를 찾을 수 없습니다".to_string());
    }
    if (update.category_axis_number_format.is_some()
        || update.category_axis_number_format_source_linked.is_some())
        && !category_axis_number_format_updated
    {
        return Err("categoryAxisNumberFormat 적용 대상 c:catAx 축을 찾을 수 없습니다".to_string());
    }
    if (update.value_axis_number_format.is_some()
        || update.value_axis_number_format_source_linked.is_some())
        && !value_axis_number_format_updated
    {
        return Err("valueAxisNumberFormat 적용 대상 c:valAx 축을 찾을 수 없습니다".to_string());
    }
    ensure_data_label_updates_applied(update, &data_label_state)?;
    ensure_stock_bar_updates_applied(update, &stock_bar_state)?;
    if update.title_overlay.is_some() && !title_overlay_updated {
        return Err("titleOverlay 변경 대상 c:title 요소를 찾을 수 없습니다".to_string());
    }
    if update.date_1904.is_some() && !date_1904_updated {
        return Err("date1904 적용 대상 c:chartSpace를 찾을 수 없습니다".to_string());
    }
    if update.chart_style.is_some() && !chart_style_updated {
        return Err(
            "chartStyle 변경 대상 c:chartSpace/c:style 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.chart_area_fill_color.is_some() && !chart_area_fill_color_updated {
        return Err(
            "chartAreaFillColor 변경 대상 c:chartSpace 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.plot_area_fill_color.is_some() && !plot_area_fill_color_updated {
        return Err("plotAreaFillColor 변경 대상 c:plotArea 요소를 찾을 수 없습니다".to_string());
    }
    if update.rounded_corners.is_some() && !rounded_corners_updated {
        return Err("roundedCorners 적용 대상 c:chartSpace를 찾을 수 없습니다".to_string());
    }
    if update.auto_title_deleted.is_some() && !auto_title_deleted_updated {
        return Err("autoTitleDeleted 적용 대상 c:chart를 찾을 수 없습니다".to_string());
    }
    if update.vary_colors.is_some() && !vary_colors_updated {
        return Err("varyColors 적용 대상 c:*Chart를 찾을 수 없습니다".to_string());
    }
    if update.view_3d_rotation_x.is_some() && !view_3d_rotation_x_updated {
        return Err("view3DRotationX 적용 대상 c:chart를 찾을 수 없습니다".to_string());
    }
    if update.view_3d_rotation_y.is_some() && !view_3d_rotation_y_updated {
        return Err("view3DRotationY 적용 대상 c:chart를 찾을 수 없습니다".to_string());
    }
    if update.view_3d_perspective.is_some() && !view_3d_perspective_updated {
        return Err("view3DPerspective 적용 대상 c:chart를 찾을 수 없습니다".to_string());
    }
    if update.view_3d_right_angle_axes.is_some() && !view_3d_right_angle_axes_updated {
        return Err("view3DRightAngleAxes 적용 대상 c:chart를 찾을 수 없습니다".to_string());
    }
    if update.view_3d_height_percent.is_some() && !view_3d_height_percent_updated {
        return Err("view3DHeightPercent 적용 대상 c:chart를 찾을 수 없습니다".to_string());
    }
    if update.view_3d_depth_percent.is_some() && !view_3d_depth_percent_updated {
        return Err("view3DDepthPercent 적용 대상 c:chart를 찾을 수 없습니다".to_string());
    }
    if update.display_blanks_as.is_some() && !display_blanks_as_updated {
        return Err("displayBlanksAs 변경 대상 c:chart 요소를 찾을 수 없습니다".to_string());
    }
    if update.show_hidden_data.is_some() && !show_hidden_data_updated {
        return Err("showHiddenData 변경 대상 c:chart 요소를 찾을 수 없습니다".to_string());
    }
    if update.plot_visible_only.is_some() && !plot_visible_only_updated {
        return Err("plotVisibleOnly 변경 대상 c:chart 요소를 찾을 수 없습니다".to_string());
    }
    ensure_data_table_updates_applied(update, &data_table_state)?;
    if update.legend_overlay.is_some() && !legend_overlay_updated {
        return Err("legendOverlay 변경 대상 c:legend 요소를 찾을 수 없습니다".to_string());
    }
    for series in update
        .series
        .iter()
        .filter(|series| has_series_style_update(series))
    {
        if !series_style_updated.contains(&series.index) {
            return Err(format!(
                "series {} style 변경 대상 c:ser 요소를 찾을 수 없습니다",
                series.index
            ));
        }
    }

    Ok(writer.into_inner())
}

fn validate_update(chart: &OoxmlChart, update: &ChartXmlUpdate) -> Result<(), String> {
    if let Some(title) = &update.title {
        if title.is_empty() {
            return Err("title은 비어 있을 수 없습니다".to_string());
        }
    }

    if let Some(value) = update.chart_style {
        if !(1..=48).contains(&value) {
            return Err("chartStyle은 1..48 범위여야 합니다".to_string());
        }
    }
    if let Some(value) = update.plot_area_fill_color {
        if value > 0x00FF_FFFF {
            return Err("plotAreaFillColor는 0x000000..0xFFFFFF 범위여야 합니다".to_string());
        }
    }
    if let Some(value) = update.chart_area_fill_color {
        if value > 0x00FF_FFFF {
            return Err("chartAreaFillColor는 0x000000..0xFFFFFF 범위여야 합니다".to_string());
        }
    }
    if update.category_axis_crosses.is_some() && update.category_axis_crosses_at.is_some() {
        return Err(
            "categoryAxisCrosses와 categoryAxisCrossesAt은 동시에 지정할 수 없습니다".to_string(),
        );
    }
    if update.value_axis_crosses.is_some() && update.value_axis_crosses_at.is_some() {
        return Err(
            "valueAxisCrosses와 valueAxisCrossesAt은 동시에 지정할 수 없습니다".to_string(),
        );
    }
    for (name, value) in [
        ("categoryAxisCrossesAt", update.category_axis_crosses_at),
        ("valueAxisCrossesAt", update.value_axis_crosses_at),
    ] {
        if value.is_some_and(|value| !value.is_finite()) {
            return Err(format!("{name}은 유한한 숫자여야 합니다"));
        }
    }

    if let Some(chart_type) = update.chart_type {
        if !matches!(chart_type, OoxmlChartType::Column | OoxmlChartType::Bar) {
            return Err("chartType 변경은 현재 Column 또는 Bar 막대 차트만 지원합니다".to_string());
        }
        if !matches!(
            chart.chart_type,
            OoxmlChartType::Column | OoxmlChartType::Bar
        ) {
            return Err(format!(
                "chartType 변경은 막대 차트에서만 지원합니다: 현재 {:?}",
                chart.chart_type
            ));
        }
    }

    if update.grouping.is_some()
        && !matches!(
            chart.chart_type,
            OoxmlChartType::Column | OoxmlChartType::Bar | OoxmlChartType::Line
        )
    {
        return Err(format!(
            "grouping 변경은 막대/라인 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }

    if update.bar_gap_width.is_some()
        && !matches!(
            chart.chart_type,
            OoxmlChartType::Column | OoxmlChartType::Bar
        )
    {
        return Err(format!(
            "barGapWidth 변경은 막대 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if let Some(value) = update.bar_gap_width {
        if value > 500 {
            return Err("barGapWidth는 0..500 범위여야 합니다".to_string());
        }
    }

    if update.bar_overlap.is_some()
        && !matches!(
            chart.chart_type,
            OoxmlChartType::Column | OoxmlChartType::Bar
        )
    {
        return Err(format!(
            "barOverlap 변경은 막대 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if let Some(value) = update.bar_overlap {
        if !(-100..=100).contains(&value) {
            return Err("barOverlap은 -100..100 범위여야 합니다".to_string());
        }
    }

    if update.line_smooth.is_some() && !matches!(chart.chart_type, OoxmlChartType::Line) {
        return Err(format!(
            "lineSmooth 변경은 라인 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }

    if update.line_marker_visible.is_some() && !matches!(chart.chart_type, OoxmlChartType::Line) {
        return Err(format!(
            "lineMarkerVisible 변경은 라인 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }

    if has_line_marker_children_update(update) && !matches!(chart.chart_type, OoxmlChartType::Line)
    {
        return Err(format!(
            "line marker 변경은 라인 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if let Some(value) = update.line_marker_size {
        if !(2..=72).contains(&value) {
            return Err("lineMarkerSize는 2..72 범위여야 합니다".to_string());
        }
    }
    if let Some(value) = update.line_marker_line_width {
        if value > 2_000_000 {
            return Err("lineMarkerLineWidth는 0..2000000 EMU 범위여야 합니다".to_string());
        }
    }

    if update.pie_first_slice_angle.is_some() && !matches!(chart.chart_type, OoxmlChartType::Pie) {
        return Err(format!(
            "pieFirstSliceAngle 변경은 원형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if let Some(value) = update.pie_first_slice_angle {
        if value > 360 {
            return Err("pieFirstSliceAngle은 0..360 범위여야 합니다".to_string());
        }
    }

    if update.pie_explosion.is_some() && !matches!(chart.chart_type, OoxmlChartType::Pie) {
        return Err(format!(
            "pieExplosion 변경은 원형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if let Some(value) = update.pie_explosion {
        if value > 400 {
            return Err("pieExplosion은 0..400 범위여야 합니다".to_string());
        }
    }
    if update.doughnut_hole_size.is_some() && !matches!(chart.chart_type, OoxmlChartType::Pie) {
        return Err(format!(
            "doughnutHoleSize 변경은 도넛 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if update.doughnut_hole_size.is_some() && !chart.has_doughnut_chart {
        return Err(
            "doughnutHoleSize 변경은 c:doughnutChart가 있는 차트에서만 지원합니다".to_string(),
        );
    }
    if let Some(value) = update.doughnut_hole_size {
        if !(10..=90).contains(&value) {
            return Err("doughnutHoleSize는 10..90 범위여야 합니다".to_string());
        }
    }

    if (update.pie_of_pie_type.is_some()
        || update.pie_of_pie_gap_width.is_some()
        || update.pie_of_pie_second_size.is_some()
        || update.pie_of_pie_ser_line_color.is_some()
        || update.pie_of_pie_ser_line_width.is_some())
        && !matches!(chart.chart_type, OoxmlChartType::Pie)
    {
        return Err(format!(
            "pieOfPie 변경은 원형대원형/원형대막대 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if (update.pie_of_pie_type.is_some()
        || update.pie_of_pie_gap_width.is_some()
        || update.pie_of_pie_second_size.is_some()
        || update.pie_of_pie_ser_line_color.is_some()
        || update.pie_of_pie_ser_line_width.is_some())
        && !chart.has_of_pie_chart
    {
        return Err(
            "pieOfPie 변경은 c:ofPieChart가 있는 원형대원형/원형대막대 차트에서만 지원합니다"
                .to_string(),
        );
    }
    if let Some(value) = update.pie_of_pie_gap_width {
        if value > 500 {
            return Err("pieOfPieGapWidth는 0..500 범위여야 합니다".to_string());
        }
    }
    if let Some(value) = update.pie_of_pie_second_size {
        if !(5..=200).contains(&value) {
            return Err("pieOfPieSecondSize는 5..200 범위여야 합니다".to_string());
        }
    }
    if let Some(value) = update.pie_of_pie_ser_line_width {
        if value > 2_000_000 {
            return Err("pieOfPieSerLineWidth는 0..2000000 EMU 범위여야 합니다".to_string());
        }
    }

    if update.scatter_style.is_some() && !matches!(chart.chart_type, OoxmlChartType::Scatter) {
        return Err(format!(
            "scatterStyle 변경은 분산형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }

    if update.scatter_smooth.is_some() && !matches!(chart.chart_type, OoxmlChartType::Scatter) {
        return Err(format!(
            "scatterSmooth 변경은 분산형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }

    if has_scatter_marker_children_update(update)
        && !matches!(chart.chart_type, OoxmlChartType::Scatter)
    {
        return Err(format!(
            "scatter marker 변경은 분산형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if let Some(value) = update.scatter_marker_size {
        if !(2..=72).contains(&value) {
            return Err("scatterMarkerSize는 2..72 범위여야 합니다".to_string());
        }
    }
    if let Some(value) = update.scatter_marker_line_width {
        if value > 2_000_000 {
            return Err("scatterMarkerLineWidth는 0..2000000 EMU 범위여야 합니다".to_string());
        }
    }

    if has_trendline_update(update)
        && !matches!(
            chart.chart_type,
            OoxmlChartType::Line | OoxmlChartType::Scatter
        )
    {
        return Err(format!(
            "trendline 변경은 라인/분산형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if let Some(value) = update.trendline_line_width {
        if value > 2_000_000 {
            return Err("trendlineLineWidth는 0..2000000 범위여야 합니다".to_string());
        }
    }
    if has_error_bar_update(update)
        && !matches!(
            chart.chart_type,
            OoxmlChartType::Line | OoxmlChartType::Scatter
        )
    {
        return Err(format!(
            "errorBar 변경은 라인/분산형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if update
        .error_bar_value
        .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err("errorBarValue는 0 이상의 유한한 숫자여야 합니다".to_string());
    }
    if let Some(value) = update.error_bar_line_width {
        if value > 2_000_000 {
            return Err("errorBarLineWidth는 0..2000000 범위여야 합니다".to_string());
        }
    }

    if has_stock_bar_update(update) && !matches!(chart.chart_type, OoxmlChartType::Stock) {
        return Err(format!(
            "stock up/down bar 변경은 주식형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if has_stock_hi_low_line_update(update) && !matches!(chart.chart_type, OoxmlChartType::Stock) {
        return Err(format!(
            "stock hiLowLines 변경은 주식형 차트에서만 지원합니다: 현재 {:?}",
            chart.chart_type
        ));
    }
    if let Some(value) = update.stock_up_down_bar_gap_width {
        if value > 500 {
            return Err("stockUpDownBarGapWidth는 0..500 범위여야 합니다".to_string());
        }
    }
    if let Some(value) = update.stock_hi_low_line_width {
        if value > 2_000_000 {
            return Err("stockHiLowLineWidth는 0..2000000 범위여야 합니다".to_string());
        }
    }

    for (name, value) in [
        ("categoryAxisLineWidth", update.category_axis_line_width),
        (
            "categoryAxisMajorGridLineWidth",
            update.category_axis_major_grid_line_width,
        ),
        (
            "categoryAxisMinorGridLineWidth",
            update.category_axis_minor_grid_line_width,
        ),
        ("valueAxisLineWidth", update.value_axis_line_width),
        (
            "valueAxisMajorGridLineWidth",
            update.value_axis_major_grid_line_width,
        ),
        (
            "valueAxisMinorGridLineWidth",
            update.value_axis_minor_grid_line_width,
        ),
        ("stockUpBarLineWidth", update.stock_up_bar_line_width),
        ("stockDownBarLineWidth", update.stock_down_bar_line_width),
    ] {
        if let Some(value) = value {
            if value > 2_000_000 {
                return Err(format!("{name}는 0..2000000 범위여야 합니다"));
            }
        }
    }

    for (name, value) in [
        ("valueAxisLogBase", update.value_axis_log_base),
        ("valueAxisMinimum", update.value_axis_minimum),
        ("valueAxisMaximum", update.value_axis_maximum),
        ("valueAxisMajorUnit", update.value_axis_major_unit),
        ("valueAxisMinorUnit", update.value_axis_minor_unit),
    ] {
        if let Some(value) = value {
            if !value.is_finite() {
                return Err(format!("{name}은 유한한 숫자여야 합니다"));
            }
        }
    }
    if let Some(value) = update.value_axis_log_base {
        if value < 2.0 {
            return Err("valueAxisLogBase는 2 이상이어야 합니다".to_string());
        }
    }
    for (name, value) in [
        ("valueAxisMajorUnit", update.value_axis_major_unit),
        ("valueAxisMinorUnit", update.value_axis_minor_unit),
    ] {
        if let Some(value) = value {
            if value <= 0.0 {
                return Err(format!("{name}은 0보다 커야 합니다"));
            }
        }
    }
    let effective_min = update.value_axis_minimum.or(chart.value_axis_minimum);
    let effective_max = update.value_axis_maximum.or(chart.value_axis_maximum);
    if let (Some(min), Some(max)) = (effective_min, effective_max) {
        if max <= min {
            return Err("valueAxisMaximum은 valueAxisMinimum보다 커야 합니다".to_string());
        }
    }
    if let Some(format) = &update.value_axis_number_format {
        if format.trim().is_empty() {
            return Err("valueAxisNumberFormat은 빈 문자열일 수 없습니다".to_string());
        }
    }
    if let Some(format) = &update.category_axis_number_format {
        if format.trim().is_empty() {
            return Err("categoryAxisNumberFormat은 빈 문자열일 수 없습니다".to_string());
        }
    }

    let series_updates: BTreeMap<usize, &SeriesXmlUpdate> = update
        .series
        .iter()
        .map(|item| (item.index, item))
        .collect();
    let expected_point_count = update
        .categories
        .as_ref()
        .map(Vec::len)
        .unwrap_or(chart.categories.len());

    if let Some(categories) = &update.categories {
        if categories.is_empty() && !chart.series.is_empty() {
            return Err("categories는 비어 있을 수 없습니다".to_string());
        }
    }

    for series in &update.series {
        let Some(existing) = chart.series.get(series.index) else {
            return Err(format!("series index {} 범위 초과", series.index));
        };
        if let Some(values) = &series.values {
            if values.iter().any(|value| !value.is_finite()) {
                return Err("series values에는 유한한 숫자만 사용할 수 있습니다".to_string());
            }
            if values.is_empty() && !existing.values.is_empty() {
                return Err(format!(
                    "series {} values는 비어 있을 수 없습니다",
                    series.index
                ));
            }
            if values.len() != expected_point_count {
                return Err(format!(
                    "series {} values 길이 불일치: 입력 {}, 기대 {}",
                    series.index,
                    values.len(),
                    expected_point_count
                ));
            }
        }
        if let Some(width) = series.line_width {
            if width > 2_000_000 {
                return Err("series.lineWidth는 0..2000000 EMU 범위여야 합니다".to_string());
            }
        }
    }

    if expected_point_count != chart.categories.len() {
        if update.categories.is_none() && !chart.categories.is_empty() {
            return Err(
                "series values 길이를 바꾸려면 categories도 같은 길이로 제공해야 합니다"
                    .to_string(),
            );
        }
        for (idx, existing) in chart.series.iter().enumerate() {
            if existing.values.is_empty() {
                continue;
            }
            let Some(series_update) = series_updates.get(&idx) else {
                return Err(format!(
                    "category/value point 수를 바꾸려면 series {} values도 제공해야 합니다",
                    idx
                ));
            };
            let Some(values) = &series_update.values else {
                return Err(format!(
                    "category/value point 수를 바꾸려면 series {} values도 제공해야 합니다",
                    idx
                ));
            };
            if values.len() != expected_point_count {
                return Err(format!(
                    "series {} values 길이 불일치: 입력 {}, 기대 {}",
                    idx,
                    values.len(),
                    expected_point_count
                ));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheKind {
    Text,
    Number,
}

fn should_rewrite_chart_title(local: &[u8], path: &[Vec<u8>], update: &ChartXmlUpdate) -> bool {
    local == b"title"
        && update.title.is_some()
        && path.last().is_some_and(|part| part.as_slice() == b"chart")
}

fn axis_title_rewrite_kind(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<AxisKind> {
    if local != b"title" {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") if update.category_axis_title.is_some() => Some(AxisKind::Category),
        Some(b"valAx") if update.value_axis_title.is_some() => Some(AxisKind::Value),
        _ => None,
    }
}

fn axis_title_update<'a>(axis: AxisKind, update: &'a ChartXmlUpdate) -> Option<&'a str> {
    match axis {
        AxisKind::Category => update.category_axis_title.as_deref(),
        AxisKind::Value => update.value_axis_title.as_deref(),
    }
}

fn bar_dir_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"barDir"
        || !path
            .last()
            .is_some_and(|part| matches!(part.as_slice(), b"barChart" | b"bar3DChart"))
    {
        return None;
    }
    match update.chart_type? {
        OoxmlChartType::Column => Some("col"),
        OoxmlChartType::Bar => Some("bar"),
        _ => None,
    }
}

fn grouping_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"grouping"
        || !path.last().is_some_and(|part| {
            matches!(part.as_slice(), b"barChart" | b"bar3DChart" | b"lineChart")
        })
    {
        return None;
    }
    update.grouping.map(grouping_xml_value)
}

fn grouping_xml_value(grouping: BarGrouping) -> &'static str {
    match grouping {
        BarGrouping::Clustered => "clustered",
        BarGrouping::Stacked => "stacked",
        BarGrouping::PercentStacked => "percentStacked",
    }
}

fn is_grouping_chart_parent_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| matches!(part.as_slice(), b"barChart" | b"bar3DChart" | b"lineChart"))
}

fn is_grouping_element(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"grouping" && is_grouping_chart_parent_path(path)
}

fn is_grouping_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    if !is_grouping_chart_parent_path(path) {
        return false;
    }
    matches!(
        local,
        b"varyColors"
            | b"ser"
            | b"dLbls"
            | b"dropLines"
            | b"hiLowLines"
            | b"upDownBars"
            | b"marker"
            | b"smooth"
            | b"gapWidth"
            | b"overlap"
            | b"serLines"
            | b"axId"
            | b"extLst"
    )
}

fn write_missing_grouping<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    current_plot: Option<usize>,
    grouping_seen_plots: &BTreeSet<usize>,
    grouping_updated_plots: &mut BTreeSet<usize>,
    grouping_updated: &mut bool,
) -> Result<(), String> {
    let Some(value) = update.grouping else {
        return Ok(());
    };
    let Some(plot) = current_plot else {
        return Ok(());
    };
    if grouping_seen_plots.contains(&plot) || grouping_updated_plots.contains(&plot) {
        return Ok(());
    }
    write_chart_empty_with_val(
        writer,
        prefix_source_name,
        "grouping",
        grouping_xml_value(value),
    )?;
    grouping_updated_plots.insert(plot);
    *grouping_updated = true;
    Ok(())
}

fn is_bar_chart_element(local: &[u8]) -> bool {
    matches!(local, b"barChart" | b"bar3DChart")
}

fn is_bar_chart_parent_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| is_bar_chart_element(part.as_slice()))
}

fn is_bar_3d_chart_parent_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"bar3DChart")
}

fn write_missing_bar_layout_children<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    bar_gap_width_updated: &mut bool,
    bar_overlap_updated: &mut bool,
) -> Result<(), String> {
    if let Some(value) = update.bar_gap_width {
        if !*bar_gap_width_updated {
            write_chart_empty_with_val(writer, prefix_source_name, "gapWidth", &value.to_string())?;
            *bar_gap_width_updated = true;
        }
    }
    if let Some(value) = update.bar_overlap {
        if !*bar_overlap_updated {
            write_chart_empty_with_val(writer, prefix_source_name, "overlap", &value.to_string())?;
            *bar_overlap_updated = true;
        }
    }
    Ok(())
}

fn write_missing_bar_3d_children<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    bar_3d_gap_depth_updated: &mut bool,
    bar_3d_shape_updated: &mut bool,
) -> Result<(), String> {
    write_missing_bar_3d_gap_depth(writer, prefix_source_name, update, bar_3d_gap_depth_updated)?;
    if let Some(value) = update.bar_3d_shape.as_deref() {
        if !*bar_3d_shape_updated {
            write_chart_empty_with_val(writer, prefix_source_name, "shape", value)?;
            *bar_3d_shape_updated = true;
        }
    }
    Ok(())
}

fn write_missing_bar_3d_gap_depth<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    bar_3d_gap_depth_updated: &mut bool,
) -> Result<(), String> {
    if let Some(value) = update.bar_3d_gap_depth {
        if !*bar_3d_gap_depth_updated {
            write_chart_empty_with_val(writer, prefix_source_name, "gapDepth", &value.to_string())?;
            *bar_3d_gap_depth_updated = true;
        }
    }
    Ok(())
}

fn bar_gap_width_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"gapWidth"
        || !path
            .last()
            .is_some_and(|part| matches!(part.as_slice(), b"barChart" | b"bar3DChart"))
    {
        return None;
    }
    update.bar_gap_width.map(|value| value.to_string())
}

fn bar_overlap_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"overlap"
        || !path
            .last()
            .is_some_and(|part| matches!(part.as_slice(), b"barChart" | b"bar3DChart"))
    {
        return None;
    }
    update.bar_overlap.map(|value| value.to_string())
}

fn bar_3d_gap_depth_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"gapDepth"
        || !path
            .last()
            .is_some_and(|part| part.as_slice() == b"bar3DChart")
    {
        return None;
    }
    update.bar_3d_gap_depth.map(|value| value.to_string())
}

fn bar_3d_shape_update_value<'a>(
    local: &[u8],
    path: &[Vec<u8>],
    update: &'a ChartXmlUpdate,
) -> Option<&'a str> {
    if local != b"shape"
        || !path
            .last()
            .is_some_and(|part| part.as_slice() == b"bar3DChart")
    {
        return None;
    }
    update.bar_3d_shape.as_deref()
}

fn line_smooth_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"smooth"
        || !path_contains(path, b"lineChart")
        || !path
            .last()
            .is_some_and(|part| matches!(part.as_slice(), b"ser" | b"lineChart"))
    {
        return None;
    }
    update
        .line_smooth
        .map(|smooth| if smooth { "1" } else { "0" })
}

fn line_marker_size_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"size"
        || !path_contains(path, b"lineChart")
        || !path_contains(path, b"ser")
        || !path.last().is_some_and(|part| part.as_slice() == b"marker")
    {
        return None;
    }
    update.line_marker_size.map(|value| value.to_string())
}

fn line_marker_symbol_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"symbol" || !is_line_marker_path(path) {
        return None;
    }
    update.line_marker_symbol.map(marker_symbol_xml_value)
}

fn line_marker_visible_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"marker"
        || !path
            .last()
            .is_some_and(|part| part.as_slice() == b"lineChart")
    {
        return None;
    }
    update
        .line_marker_visible
        .map(|visible| if visible { "1" } else { "0" })
}

fn is_line_chart_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"lineChart")
}

fn is_line_series_path(path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| part.as_slice() == b"ser") && path_contains(path, b"lineChart")
}

fn is_line_marker_path(path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| part.as_slice() == b"marker")
        && path_contains(path, b"lineChart")
        && path_contains(path, b"ser")
}

fn is_line_marker_sp_pr(local: &[u8], path: &[Vec<u8>], update: &ChartXmlUpdate) -> bool {
    local == b"spPr" && is_line_marker_path(path) && has_line_marker_style_update(update)
}

fn line_series_marker_element(
    local: &[u8],
    path: &[Vec<u8>],
    current_ser: Option<usize>,
) -> Option<usize> {
    if local == b"marker" && is_line_series_path(path) {
        current_ser
    } else {
        None
    }
}

fn line_marker_child_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !is_line_marker_path(path) {
        return None;
    }
    match local {
        b"size" => Some(1),
        b"spPr" => Some(2),
        b"extLst" => Some(3),
        _ => None,
    }
}

fn line_series_child_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !is_line_series_path(path) {
        return None;
    }
    match local {
        b"dPt" | b"dLbls" | b"errBars" | b"cat" | b"val" | b"smooth" => Some(1),
        b"extLst" => Some(2),
        _ => None,
    }
}

fn line_chart_child_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !is_line_chart_path(path) {
        return None;
    }
    match local {
        b"smooth" => Some(1),
        b"axId" | b"extLst" => Some(2),
        _ => None,
    }
}

fn has_line_marker_children_update(update: &ChartXmlUpdate) -> bool {
    update.line_marker_symbol.is_some()
        || update.line_marker_size.is_some()
        || has_line_marker_style_update(update)
}

fn has_line_marker_style_update(update: &ChartXmlUpdate) -> bool {
    update.line_marker_fill_color.is_some()
        || update.line_marker_line_color.is_some()
        || update.line_marker_line_width.is_some()
}

fn marker_symbol_xml_value(symbol: ChartMarkerSymbol) -> &'static str {
    match symbol {
        ChartMarkerSymbol::Circle => "circle",
        ChartMarkerSymbol::Dash => "dash",
        ChartMarkerSymbol::Diamond => "diamond",
        ChartMarkerSymbol::Dot => "dot",
        ChartMarkerSymbol::None => "none",
        ChartMarkerSymbol::Picture => "picture",
        ChartMarkerSymbol::Plus => "plus",
        ChartMarkerSymbol::Square => "square",
        ChartMarkerSymbol::Star => "star",
        ChartMarkerSymbol::Triangle => "triangle",
        ChartMarkerSymbol::X => "x",
    }
}

fn marker_symbol_update(
    update: &ChartXmlUpdate,
    family: MarkerFamily,
) -> Option<ChartMarkerSymbol> {
    match family {
        MarkerFamily::Line => update.line_marker_symbol,
        MarkerFamily::Scatter => update.scatter_marker_symbol,
    }
}

fn marker_size_update(update: &ChartXmlUpdate, family: MarkerFamily) -> Option<u32> {
    match family {
        MarkerFamily::Line => update.line_marker_size,
        MarkerFamily::Scatter => update.scatter_marker_size,
    }
}

fn marker_fill_color_update(update: &ChartXmlUpdate, family: MarkerFamily) -> Option<u32> {
    match family {
        MarkerFamily::Line => update.line_marker_fill_color,
        MarkerFamily::Scatter => update.scatter_marker_fill_color,
    }
}

fn marker_line_color_update(update: &ChartXmlUpdate, family: MarkerFamily) -> Option<u32> {
    match family {
        MarkerFamily::Line => update.line_marker_line_color,
        MarkerFamily::Scatter => update.scatter_marker_line_color,
    }
}

fn marker_line_width_update(update: &ChartXmlUpdate, family: MarkerFamily) -> Option<u32> {
    match family {
        MarkerFamily::Line => update.line_marker_line_width,
        MarkerFamily::Scatter => update.scatter_marker_line_width,
    }
}

fn has_marker_style_update(update: &ChartXmlUpdate, family: MarkerFamily) -> bool {
    marker_fill_color_update(update, family).is_some()
        || marker_line_color_update(update, family).is_some()
        || marker_line_width_update(update, family).is_some()
}

fn has_marker_children_update(update: &ChartXmlUpdate, family: MarkerFamily) -> bool {
    marker_symbol_update(update, family).is_some()
        || marker_size_update(update, family).is_some()
        || has_marker_style_update(update, family)
}

fn write_marker_with_requested_children<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    family: MarkerFamily,
    marker_symbol_updated: &mut bool,
    marker_size_updated: &mut bool,
    marker_style_updated: &mut bool,
) -> Result<(), String> {
    if !has_marker_children_update(update, family) {
        return Ok(());
    }
    let chart_prefix = element_prefix(prefix_source_name);
    let marker_name = qualified_name(chart_prefix.as_deref(), "marker");
    writer
        .write_event(Event::Start(BytesStart::new(marker_name.as_str())))
        .map_err(|e| e.to_string())?;
    if let Some(symbol) = marker_symbol_update(update, family) {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "symbol",
            marker_symbol_xml_value(symbol),
        )?;
        *marker_symbol_updated = true;
    }
    if let Some(value) = marker_size_update(update, family) {
        write_chart_empty_with_val(writer, prefix_source_name, "size", &value.to_string())?;
        *marker_size_updated = true;
    }
    if has_marker_style_update(update, family) {
        write_marker_sp_pr_with_style(writer, prefix_source_name, update, family)?;
        *marker_style_updated = true;
    }
    writer
        .write_event(Event::End(BytesEnd::new(marker_name.as_str())))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn write_missing_marker_children_until<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    family: MarkerFamily,
    current_ser: Option<usize>,
    limit: u8,
    marker_symbol_seen_series: &mut BTreeSet<usize>,
    marker_size_seen_series: &mut BTreeSet<usize>,
    marker_style_seen_series: &mut BTreeSet<usize>,
    marker_symbol_updated: &mut bool,
    marker_size_updated: &mut bool,
    marker_style_updated: &mut bool,
) -> Result<(), String> {
    let Some(series_index) = current_ser else {
        return Ok(());
    };
    if limit >= 1
        && marker_symbol_update(update, family).is_some()
        && !marker_symbol_seen_series.contains(&series_index)
    {
        let symbol = marker_symbol_update(update, family).unwrap();
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "symbol",
            marker_symbol_xml_value(symbol),
        )?;
        marker_symbol_seen_series.insert(series_index);
        *marker_symbol_updated = true;
    }
    if limit >= 2
        && marker_size_update(update, family).is_some()
        && !marker_size_seen_series.contains(&series_index)
    {
        let value = marker_size_update(update, family).unwrap();
        write_chart_empty_with_val(writer, prefix_source_name, "size", &value.to_string())?;
        marker_size_seen_series.insert(series_index);
        *marker_size_updated = true;
    }
    if limit >= 3
        && has_marker_style_update(update, family)
        && !marker_style_seen_series.contains(&series_index)
    {
        write_marker_sp_pr_with_style(writer, prefix_source_name, update, family)?;
        marker_style_seen_series.insert(series_index);
        *marker_style_updated = true;
    }
    Ok(())
}

fn write_missing_line_series_children_until<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    current_ser: Option<usize>,
    limit: u8,
    line_marker_seen_series: &mut BTreeSet<usize>,
    line_marker_symbol_seen_series: &mut BTreeSet<usize>,
    line_marker_size_seen_series: &mut BTreeSet<usize>,
    line_marker_style_seen_series: &mut BTreeSet<usize>,
    line_marker_symbol_updated: &mut bool,
    line_marker_size_updated: &mut bool,
    line_marker_style_updated: &mut bool,
    line_smooth_seen_series: &mut BTreeSet<usize>,
    line_smooth_updated: &mut bool,
    trendline_seen_series: &mut BTreeSet<usize>,
    trendline_state: &mut TrendlineUpdateState,
    error_bar_seen_series: &mut BTreeSet<usize>,
    error_bar_state: &mut ErrorBarUpdateState,
) -> Result<(), String> {
    let Some(series_index) = current_ser else {
        return Ok(());
    };
    if limit >= 1
        && has_line_marker_children_update(update)
        && !line_marker_seen_series.contains(&series_index)
    {
        write_marker_with_requested_children(
            writer,
            prefix_source_name,
            update,
            MarkerFamily::Line,
            line_marker_symbol_updated,
            line_marker_size_updated,
            line_marker_style_updated,
        )?;
        if update.line_marker_symbol.is_some() {
            line_marker_symbol_seen_series.insert(series_index);
        }
        line_marker_size_seen_series.insert(series_index);
        if has_line_marker_style_update(update) {
            line_marker_style_seen_series.insert(series_index);
        }
        line_marker_seen_series.insert(series_index);
    }
    if limit >= 1 && has_trendline_update(update) && !trendline_seen_series.contains(&series_index)
    {
        write_chart_trendline_with_requested_fields(
            writer,
            prefix_source_name,
            update,
            trendline_state,
        )?;
        trendline_seen_series.insert(series_index);
    }
    if limit >= 1 && has_error_bar_update(update) && !error_bar_seen_series.contains(&series_index)
    {
        write_chart_error_bars_with_requested_fields(
            writer,
            prefix_source_name,
            update,
            error_bar_state,
        )?;
        error_bar_seen_series.insert(series_index);
    }
    if limit >= 2 && !line_smooth_seen_series.contains(&series_index) {
        if let Some(line_smooth) = update.line_smooth {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "smooth",
                bool_xml_value(line_smooth),
            )?;
            line_smooth_seen_series.insert(series_index);
            *line_smooth_updated = true;
        }
    }
    Ok(())
}

fn write_missing_line_chart_children_until<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    limit: u8,
    line_marker_visible_updated: &mut bool,
    line_chart_smooth_updated: &mut bool,
    line_smooth_updated: &mut bool,
) -> Result<(), String> {
    if limit >= 1 {
        if let Some(value) = update.line_marker_visible {
            if !*line_marker_visible_updated {
                write_chart_empty_with_val(
                    writer,
                    prefix_source_name,
                    "marker",
                    bool_xml_value(value),
                )?;
                *line_marker_visible_updated = true;
            }
        }
    }
    if limit >= 2 {
        if let Some(value) = update.line_smooth {
            if !*line_chart_smooth_updated {
                write_chart_empty_with_val(
                    writer,
                    prefix_source_name,
                    "smooth",
                    bool_xml_value(value),
                )?;
                *line_chart_smooth_updated = true;
                *line_smooth_updated = true;
            }
        }
    }
    Ok(())
}

fn pie_first_slice_angle_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"firstSliceAng" || !path_contains_pie_plot(path) {
        return None;
    }
    update.pie_first_slice_angle.map(|value| value.to_string())
}

fn is_pie_first_slice_parent_path(path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| {
        matches!(
            part.as_slice(),
            b"pieChart" | b"pie3DChart" | b"doughnutChart"
        )
    })
}

fn is_pie_first_slice_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    is_pie_first_slice_parent_path(path) && local == b"extLst"
}

fn write_missing_pie_first_slice_angle<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    pie_first_slice_angle_updated: &mut bool,
) -> Result<(), String> {
    if let Some(value) = update.pie_first_slice_angle {
        if !*pie_first_slice_angle_updated {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "firstSliceAng",
                &value.to_string(),
            )?;
            *pie_first_slice_angle_updated = true;
        }
    }
    Ok(())
}

fn doughnut_hole_size_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"holeSize" || !path_contains(path, b"doughnutChart") {
        return None;
    }
    update.doughnut_hole_size.map(|value| value.to_string())
}

fn is_doughnut_chart_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"doughnutChart")
}

fn is_doughnut_hole_size_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    is_doughnut_chart_path(path) && matches!(local, b"extLst")
}

fn write_missing_doughnut_hole_size<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    doughnut_hole_size_updated: &mut bool,
) -> Result<(), String> {
    if let Some(value) = update.doughnut_hole_size {
        if !*doughnut_hole_size_updated {
            write_chart_empty_with_val(writer, prefix_source_name, "holeSize", &value.to_string())?;
            *doughnut_hole_size_updated = true;
        }
    }
    Ok(())
}

fn pie_explosion_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"explosion" || !path_contains_pie_plot(path) || !path_contains(path, b"ser") {
        return None;
    }
    update.pie_explosion.map(|value| value.to_string())
}

fn is_pie_series_path(path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| part.as_slice() == b"ser") && path_contains_pie_plot(path)
}

fn is_pie_explosion_element(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"explosion" && is_pie_series_path(path)
}

fn is_pie_explosion_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    is_pie_series_path(path) && matches!(local, b"dPt" | b"dLbls" | b"cat" | b"val" | b"extLst")
}

fn write_missing_pie_explosion<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    current_ser: Option<usize>,
    pie_explosion_seen_series: &mut BTreeSet<usize>,
    pie_explosion_updated: &mut bool,
) -> Result<(), String> {
    let Some(value) = update.pie_explosion else {
        return Ok(());
    };
    let Some(series_index) = current_ser else {
        return Ok(());
    };
    if pie_explosion_seen_series.contains(&series_index) {
        return Ok(());
    }
    write_chart_empty_with_val(writer, prefix_source_name, "explosion", &value.to_string())?;
    pie_explosion_seen_series.insert(series_index);
    *pie_explosion_updated = true;
    Ok(())
}

fn pie_of_pie_type_update_value<'a>(
    local: &[u8],
    path: &[Vec<u8>],
    update: &'a ChartXmlUpdate,
) -> Option<&'a str> {
    if local != b"ofPieType" || !path_contains(path, b"ofPieChart") {
        return None;
    }
    let value = update.pie_of_pie_type?;
    Some(of_pie_type_xml_value(value))
}

fn of_pie_type_xml_value(value: OfPieType) -> &'static str {
    match value {
        OfPieType::Pie => "pie",
        OfPieType::Bar => "bar",
    }
}

fn pie_of_pie_gap_width_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"gapWidth" || !path_contains(path, b"ofPieChart") {
        return None;
    }
    update.pie_of_pie_gap_width.map(|value| value.to_string())
}

fn pie_of_pie_second_size_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"secondPieSize" || !path_contains(path, b"ofPieChart") {
        return None;
    }
    update.pie_of_pie_second_size.map(|value| value.to_string())
}

fn is_of_pie_chart_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"ofPieChart")
}

fn of_pie_child_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !is_of_pie_chart_path(path) {
        return None;
    }
    match local {
        b"varyColors" | b"ser" | b"dLbls" => Some(1),
        b"gapWidth" => Some(1),
        b"splitType" | b"splitPos" | b"custSplit" | b"secondPieSize" => Some(2),
        b"serLines" | b"extLst" => Some(4),
        _ => None,
    }
}

fn write_missing_of_pie_children_until<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    limit: u8,
    pie_of_pie_type_updated: &mut bool,
    pie_of_pie_gap_width_updated: &mut bool,
    pie_of_pie_second_size_updated: &mut bool,
    pie_of_pie_ser_lines_seen: &mut bool,
    pie_of_pie_ser_line_color_updated: &mut bool,
    pie_of_pie_ser_line_width_updated: &mut bool,
) -> Result<(), String> {
    if limit >= 1 {
        if let Some(value) = update.pie_of_pie_type {
            if !*pie_of_pie_type_updated {
                write_chart_empty_with_val(
                    writer,
                    prefix_source_name,
                    "ofPieType",
                    of_pie_type_xml_value(value),
                )?;
                *pie_of_pie_type_updated = true;
            }
        }
    }
    if limit >= 2 {
        if let Some(value) = update.pie_of_pie_gap_width {
            if !*pie_of_pie_gap_width_updated {
                write_chart_empty_with_val(
                    writer,
                    prefix_source_name,
                    "gapWidth",
                    &value.to_string(),
                )?;
                *pie_of_pie_gap_width_updated = true;
            }
        }
    }
    if limit >= 3 {
        if let Some(value) = update.pie_of_pie_second_size {
            if !*pie_of_pie_second_size_updated {
                write_chart_empty_with_val(
                    writer,
                    prefix_source_name,
                    "secondPieSize",
                    &value.to_string(),
                )?;
                *pie_of_pie_second_size_updated = true;
            }
        }
    }
    if limit >= 4 && has_of_pie_ser_line_update(update) && !*pie_of_pie_ser_lines_seen {
        write_of_pie_ser_lines_with_style(
            writer,
            prefix_source_name,
            &OoxmlChart::default(),
            update,
        )?;
        *pie_of_pie_ser_lines_seen = true;
        if update.pie_of_pie_ser_line_color.is_some() {
            *pie_of_pie_ser_line_color_updated = true;
        }
        if update.pie_of_pie_ser_line_width.is_some() {
            *pie_of_pie_ser_line_width_updated = true;
        }
    }
    Ok(())
}

fn scatter_style_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"scatterStyle"
        || !path
            .last()
            .is_some_and(|part| part.as_slice() == b"scatterChart")
    {
        return None;
    }
    update.scatter_style.map(scatter_style_xml_value)
}

fn scatter_style_xml_value(style: ScatterStyle) -> &'static str {
    match style {
        ScatterStyle::Line => "line",
        ScatterStyle::LineMarker => "lineMarker",
        ScatterStyle::Marker => "marker",
        ScatterStyle::Smooth => "smooth",
        ScatterStyle::SmoothMarker => "smoothMarker",
    }
}

fn scatter_smooth_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"smooth" || !path_contains(path, b"scatterChart") || !path_contains(path, b"ser") {
        return None;
    }
    update
        .scatter_smooth
        .map(|smooth| if smooth { "1" } else { "0" })
}

fn scatter_marker_size_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"size"
        || !path_contains(path, b"scatterChart")
        || !path_contains(path, b"ser")
        || !path.last().is_some_and(|part| part.as_slice() == b"marker")
    {
        return None;
    }
    update.scatter_marker_size.map(|value| value.to_string())
}

fn scatter_marker_symbol_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"symbol" || !is_scatter_marker_path(path) {
        return None;
    }
    update.scatter_marker_symbol.map(marker_symbol_xml_value)
}

fn has_trendline_update(update: &ChartXmlUpdate) -> bool {
    has_trendline_line_style_update(update)
        || update.trendline_type.is_some()
        || update.trendline_order.is_some()
        || update.trendline_period.is_some()
        || update.trendline_display_equation.is_some()
        || update.trendline_display_r_squared.is_some()
}

fn has_trendline_line_style_update(update: &ChartXmlUpdate) -> bool {
    update.trendline_line_color.is_some() || update.trendline_line_width.is_some()
}

fn trendline_element(local: &[u8], path: &[Vec<u8>], current_ser: Option<usize>) -> Option<usize> {
    if local == b"trendline" && (is_line_series_path(path) || is_scatter_series_path(path)) {
        current_ser
    } else {
        None
    }
}

fn is_trendline_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"trendline")
}

fn is_trendline_sp_pr(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"spPr" && is_trendline_path(path)
}

fn trendline_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<(TrendlineField, String)> {
    if !is_trendline_path(path) {
        return None;
    }
    match local {
        b"trendlineType" => update.trendline_type.map(|value| {
            (
                TrendlineField::Type,
                trendline_type_xml_value(value).to_string(),
            )
        }),
        b"order" => update
            .trendline_order
            .map(|value| (TrendlineField::Order, value.to_string())),
        b"period" => update
            .trendline_period
            .map(|value| (TrendlineField::Period, value.to_string())),
        b"dispEq" => update.trendline_display_equation.map(|value| {
            (
                TrendlineField::DisplayEquation,
                bool_xml_value(value).to_string(),
            )
        }),
        b"dispRSqr" => update.trendline_display_r_squared.map(|value| {
            (
                TrendlineField::DisplayRSquared,
                bool_xml_value(value).to_string(),
            )
        }),
        _ => None,
    }
}

fn trendline_requested_fields(update: &ChartXmlUpdate) -> Vec<TrendlineField> {
    let mut fields = Vec::new();
    if update.trendline_type.is_some() {
        fields.push(TrendlineField::Type);
    }
    if update.trendline_order.is_some() {
        fields.push(TrendlineField::Order);
    }
    if update.trendline_period.is_some() {
        fields.push(TrendlineField::Period);
    }
    if update.trendline_display_equation.is_some() {
        fields.push(TrendlineField::DisplayEquation);
    }
    if update.trendline_display_r_squared.is_some() {
        fields.push(TrendlineField::DisplayRSquared);
    }
    fields
}

fn trendline_field_update_value(
    field: TrendlineField,
    update: &ChartXmlUpdate,
) -> Option<(&'static str, String)> {
    match field {
        TrendlineField::Type => update
            .trendline_type
            .map(|value| ("trendlineType", trendline_type_xml_value(value).to_string())),
        TrendlineField::Order => update
            .trendline_order
            .map(|value| ("order", value.to_string())),
        TrendlineField::Period => update
            .trendline_period
            .map(|value| ("period", value.to_string())),
        TrendlineField::DisplayEquation => update
            .trendline_display_equation
            .map(|value| ("dispEq", bool_xml_value(value).to_string())),
        TrendlineField::DisplayRSquared => update
            .trendline_display_r_squared
            .map(|value| ("dispRSqr", bool_xml_value(value).to_string())),
    }
}

fn trendline_type_xml_value(value: ChartTrendlineType) -> &'static str {
    match value {
        ChartTrendlineType::Linear => "linear",
        ChartTrendlineType::Exponential => "exp",
        ChartTrendlineType::Logarithmic => "log",
        ChartTrendlineType::MovingAverage => "movingAvg",
        ChartTrendlineType::Polynomial => "poly",
        ChartTrendlineType::Power => "power",
    }
}

fn has_error_bar_update(update: &ChartXmlUpdate) -> bool {
    has_error_bar_line_style_update(update)
        || update.error_bar_direction.is_some()
        || update.error_bar_type.is_some()
        || update.error_bar_value_type.is_some()
        || update.error_bar_value.is_some()
        || update.error_bar_no_end_cap.is_some()
}

fn has_error_bar_line_style_update(update: &ChartXmlUpdate) -> bool {
    update.error_bar_line_color.is_some() || update.error_bar_line_width.is_some()
}

fn error_bar_element(local: &[u8], path: &[Vec<u8>], current_ser: Option<usize>) -> Option<usize> {
    if local == b"errBars" && (is_line_series_path(path) || is_scatter_series_path(path)) {
        current_ser
    } else {
        None
    }
}

fn is_error_bar_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"errBars")
}

fn is_error_bar_sp_pr(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"spPr" && is_error_bar_path(path)
}

fn error_bar_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<(ErrorBarField, String)> {
    if !is_error_bar_path(path) {
        return None;
    }
    match local {
        b"errDir" => update.error_bar_direction.map(|value| {
            (
                ErrorBarField::Direction,
                error_bar_direction_xml_value(value).to_string(),
            )
        }),
        b"errBarType" => update.error_bar_type.map(|value| {
            (
                ErrorBarField::Type,
                error_bar_type_xml_value(value).to_string(),
            )
        }),
        b"errValType" => update.error_bar_value_type.map(|value| {
            (
                ErrorBarField::ValueType,
                error_bar_value_type_xml_value(value).to_string(),
            )
        }),
        b"val" => update
            .error_bar_value
            .map(|value| (ErrorBarField::Value, format_chart_number(value))),
        b"noEndCap" => update
            .error_bar_no_end_cap
            .map(|value| (ErrorBarField::NoEndCap, bool_xml_value(value).to_string())),
        _ => None,
    }
}

fn error_bar_requested_fields(update: &ChartXmlUpdate) -> Vec<ErrorBarField> {
    let mut fields = Vec::new();
    if update.error_bar_direction.is_some() {
        fields.push(ErrorBarField::Direction);
    }
    if update.error_bar_type.is_some() {
        fields.push(ErrorBarField::Type);
    }
    if update.error_bar_value_type.is_some() {
        fields.push(ErrorBarField::ValueType);
    }
    if update.error_bar_no_end_cap.is_some() {
        fields.push(ErrorBarField::NoEndCap);
    }
    if update.error_bar_value.is_some() {
        fields.push(ErrorBarField::Value);
    }
    fields
}

fn error_bar_field_update_value(
    field: ErrorBarField,
    update: &ChartXmlUpdate,
) -> Option<(&'static str, String)> {
    match field {
        ErrorBarField::Direction => update
            .error_bar_direction
            .map(|value| ("errDir", error_bar_direction_xml_value(value).to_string())),
        ErrorBarField::Type => update
            .error_bar_type
            .map(|value| ("errBarType", error_bar_type_xml_value(value).to_string())),
        ErrorBarField::ValueType => update.error_bar_value_type.map(|value| {
            (
                "errValType",
                error_bar_value_type_xml_value(value).to_string(),
            )
        }),
        ErrorBarField::Value => update
            .error_bar_value
            .map(|value| ("val", format_chart_number(value))),
        ErrorBarField::NoEndCap => update
            .error_bar_no_end_cap
            .map(|value| ("noEndCap", bool_xml_value(value).to_string())),
    }
}

fn error_bar_direction_xml_value(value: ChartErrorBarDirection) -> &'static str {
    match value {
        ChartErrorBarDirection::X => "x",
        ChartErrorBarDirection::Y => "y",
    }
}

fn error_bar_type_xml_value(value: ChartErrorBarType) -> &'static str {
    match value {
        ChartErrorBarType::Both => "both",
        ChartErrorBarType::Plus => "plus",
        ChartErrorBarType::Minus => "minus",
    }
}

fn error_bar_value_type_xml_value(value: ChartErrorBarValueType) -> &'static str {
    match value {
        ChartErrorBarValueType::FixedValue => "fixedVal",
        ChartErrorBarValueType::Percentage => "percentage",
        ChartErrorBarValueType::StandardDeviation => "stdDev",
        ChartErrorBarValueType::StandardError => "stdErr",
    }
}

fn is_scatter_chart_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"scatterChart")
}

fn is_scatter_series_path(path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| part.as_slice() == b"ser")
        && path_contains(path, b"scatterChart")
}

fn is_scatter_marker_path(path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| part.as_slice() == b"marker")
        && path_contains(path, b"scatterChart")
        && path_contains(path, b"ser")
}

fn is_scatter_marker_sp_pr(local: &[u8], path: &[Vec<u8>], update: &ChartXmlUpdate) -> bool {
    local == b"spPr" && is_scatter_marker_path(path) && has_scatter_marker_style_update(update)
}

fn scatter_series_marker_element(
    local: &[u8],
    path: &[Vec<u8>],
    current_ser: Option<usize>,
) -> Option<usize> {
    if local == b"marker" && is_scatter_series_path(path) {
        current_ser
    } else {
        None
    }
}

fn scatter_marker_child_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !is_scatter_marker_path(path) {
        return None;
    }
    match local {
        b"size" => Some(1),
        b"spPr" => Some(2),
        b"extLst" => Some(3),
        _ => None,
    }
}

fn scatter_series_child_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !is_scatter_series_path(path) {
        return None;
    }
    match local {
        b"dPt" | b"dLbls" | b"errBars" | b"xVal" | b"yVal" | b"smooth" => Some(1),
        b"extLst" => Some(2),
        _ => None,
    }
}

fn scatter_chart_child_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !is_scatter_chart_path(path) {
        return None;
    }
    match local {
        b"ser" | b"dLbls" | b"axId" | b"extLst" => Some(1),
        _ => None,
    }
}

fn has_scatter_marker_children_update(update: &ChartXmlUpdate) -> bool {
    update.scatter_marker_symbol.is_some()
        || update.scatter_marker_size.is_some()
        || has_scatter_marker_style_update(update)
}

fn has_scatter_marker_style_update(update: &ChartXmlUpdate) -> bool {
    update.scatter_marker_fill_color.is_some()
        || update.scatter_marker_line_color.is_some()
        || update.scatter_marker_line_width.is_some()
}

fn write_missing_scatter_series_children_until<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    current_ser: Option<usize>,
    limit: u8,
    scatter_marker_seen_series: &mut BTreeSet<usize>,
    scatter_marker_symbol_seen_series: &mut BTreeSet<usize>,
    scatter_marker_size_seen_series: &mut BTreeSet<usize>,
    scatter_marker_style_seen_series: &mut BTreeSet<usize>,
    scatter_marker_symbol_updated: &mut bool,
    scatter_marker_size_updated: &mut bool,
    scatter_marker_style_updated: &mut bool,
    scatter_smooth_seen_series: &mut BTreeSet<usize>,
    scatter_smooth_updated: &mut bool,
    trendline_seen_series: &mut BTreeSet<usize>,
    trendline_state: &mut TrendlineUpdateState,
    error_bar_seen_series: &mut BTreeSet<usize>,
    error_bar_state: &mut ErrorBarUpdateState,
) -> Result<(), String> {
    let Some(series_index) = current_ser else {
        return Ok(());
    };
    if limit >= 1
        && has_scatter_marker_children_update(update)
        && !scatter_marker_seen_series.contains(&series_index)
    {
        write_marker_with_requested_children(
            writer,
            prefix_source_name,
            update,
            MarkerFamily::Scatter,
            scatter_marker_symbol_updated,
            scatter_marker_size_updated,
            scatter_marker_style_updated,
        )?;
        if update.scatter_marker_symbol.is_some() {
            scatter_marker_symbol_seen_series.insert(series_index);
        }
        scatter_marker_size_seen_series.insert(series_index);
        if has_scatter_marker_style_update(update) {
            scatter_marker_style_seen_series.insert(series_index);
        }
        scatter_marker_seen_series.insert(series_index);
    }
    if limit >= 1 && has_trendline_update(update) && !trendline_seen_series.contains(&series_index)
    {
        write_chart_trendline_with_requested_fields(
            writer,
            prefix_source_name,
            update,
            trendline_state,
        )?;
        trendline_seen_series.insert(series_index);
    }
    if limit >= 1 && has_error_bar_update(update) && !error_bar_seen_series.contains(&series_index)
    {
        write_chart_error_bars_with_requested_fields(
            writer,
            prefix_source_name,
            update,
            error_bar_state,
        )?;
        error_bar_seen_series.insert(series_index);
    }
    if limit >= 2 && !scatter_smooth_seen_series.contains(&series_index) {
        if let Some(scatter_smooth) = update.scatter_smooth {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "smooth",
                bool_xml_value(scatter_smooth),
            )?;
            scatter_smooth_seen_series.insert(series_index);
            *scatter_smooth_updated = true;
        }
    }
    Ok(())
}

fn write_missing_scatter_chart_children_until<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    limit: u8,
    scatter_style_updated: &mut bool,
) -> Result<(), String> {
    if limit >= 1 {
        if let Some(value) = update.scatter_style {
            if !*scatter_style_updated {
                write_chart_empty_with_val(
                    writer,
                    prefix_source_name,
                    "scatterStyle",
                    scatter_style_xml_value(value),
                )?;
                *scatter_style_updated = true;
            }
        }
    }
    Ok(())
}

fn has_data_label_update(update: &ChartXmlUpdate) -> bool {
    update.data_label_position.is_some()
        || update.data_labels_show_value.is_some()
        || update.data_labels_show_category_name.is_some()
        || update.data_labels_show_series_name.is_some()
        || update.data_labels_show_percent.is_some()
        || update.data_labels_show_legend_key.is_some()
}

fn data_label_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<(DataLabelField, String)> {
    if !path.last().is_some_and(|part| part.as_slice() == b"dLbls") {
        return None;
    }
    match local {
        b"dLblPos" => update.data_label_position.map(|position| {
            (
                DataLabelField::Position,
                data_label_position_xml_value(position).to_string(),
            )
        }),
        b"showVal" => update
            .data_labels_show_value
            .map(|value| (DataLabelField::ShowValue, bool_xml_value(value).to_string())),
        b"showCatName" => update.data_labels_show_category_name.map(|value| {
            (
                DataLabelField::ShowCategoryName,
                bool_xml_value(value).to_string(),
            )
        }),
        b"showSerName" => update.data_labels_show_series_name.map(|value| {
            (
                DataLabelField::ShowSeriesName,
                bool_xml_value(value).to_string(),
            )
        }),
        b"showPercent" => update.data_labels_show_percent.map(|value| {
            (
                DataLabelField::ShowPercent,
                bool_xml_value(value).to_string(),
            )
        }),
        b"showLegendKey" => update.data_labels_show_legend_key.map(|value| {
            (
                DataLabelField::ShowLegendKey,
                bool_xml_value(value).to_string(),
            )
        }),
        _ => None,
    }
}

fn data_label_requested_fields(update: &ChartXmlUpdate) -> Vec<DataLabelField> {
    let mut fields = Vec::new();
    if update.data_label_position.is_some() {
        fields.push(DataLabelField::Position);
    }
    if update.data_labels_show_value.is_some() {
        fields.push(DataLabelField::ShowValue);
    }
    if update.data_labels_show_category_name.is_some() {
        fields.push(DataLabelField::ShowCategoryName);
    }
    if update.data_labels_show_series_name.is_some() {
        fields.push(DataLabelField::ShowSeriesName);
    }
    if update.data_labels_show_percent.is_some() {
        fields.push(DataLabelField::ShowPercent);
    }
    if update.data_labels_show_legend_key.is_some() {
        fields.push(DataLabelField::ShowLegendKey);
    }
    fields
}

fn data_label_field_update_value(
    field: DataLabelField,
    update: &ChartXmlUpdate,
) -> Option<(&'static str, &'static str)> {
    match field {
        DataLabelField::Position => update
            .data_label_position
            .map(|position| ("dLblPos", data_label_position_xml_value(position))),
        DataLabelField::ShowValue => update
            .data_labels_show_value
            .map(|value| ("showVal", bool_xml_value(value))),
        DataLabelField::ShowCategoryName => update
            .data_labels_show_category_name
            .map(|value| ("showCatName", bool_xml_value(value))),
        DataLabelField::ShowSeriesName => update
            .data_labels_show_series_name
            .map(|value| ("showSerName", bool_xml_value(value))),
        DataLabelField::ShowPercent => update
            .data_labels_show_percent
            .map(|value| ("showPercent", bool_xml_value(value))),
        DataLabelField::ShowLegendKey => update
            .data_labels_show_legend_key
            .map(|value| ("showLegendKey", bool_xml_value(value))),
    }
}

fn data_label_position_xml_value(position: ChartDataLabelPosition) -> &'static str {
    match position {
        ChartDataLabelPosition::BestFit => "bestFit",
        ChartDataLabelPosition::Bottom => "b",
        ChartDataLabelPosition::Center => "ctr",
        ChartDataLabelPosition::InsideBase => "inBase",
        ChartDataLabelPosition::InsideEnd => "inEnd",
        ChartDataLabelPosition::Left => "l",
        ChartDataLabelPosition::OutsideEnd => "outEnd",
        ChartDataLabelPosition::Right => "r",
        ChartDataLabelPosition::Top => "t",
    }
}

fn bool_xml_value(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
}

fn is_chart_space_flag_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    if !is_chart_space_parent_path(path) {
        return false;
    }
    matches!(
        local,
        b"roundedCorners"
            | b"style"
            | b"clrMapOvr"
            | b"pivotSource"
            | b"protection"
            | b"chart"
            | b"spPr"
            | b"txPr"
            | b"externalData"
            | b"printSettings"
            | b"userShapes"
            | b"extLst"
    )
}

fn is_chart_style_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    if !is_chart_space_parent_path(path) {
        return false;
    }
    matches!(
        local,
        b"clrMapOvr"
            | b"pivotSource"
            | b"protection"
            | b"chart"
            | b"spPr"
            | b"txPr"
            | b"externalData"
            | b"printSettings"
            | b"userShapes"
            | b"extLst"
    )
}

fn write_missing_chart_space_flags<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    date_1904_updated: &mut bool,
    rounded_corners_updated: &mut bool,
) -> Result<(), String> {
    if let Some(value) = update.date_1904 {
        if !*date_1904_updated {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "date1904",
                bool_xml_value(value),
            )?;
            *date_1904_updated = true;
        }
    }
    if let Some(value) = update.rounded_corners {
        if !*rounded_corners_updated {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "roundedCorners",
                bool_xml_value(value),
            )?;
            *rounded_corners_updated = true;
        }
    }
    Ok(())
}

fn write_missing_chart_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    chart_style_updated: &mut bool,
) -> Result<(), String> {
    if let Some(value) = update.chart_style {
        if !*chart_style_updated {
            write_chart_empty_with_val(writer, prefix_source_name, "style", &value.to_string())?;
            *chart_style_updated = true;
        }
    }
    Ok(())
}

fn date_1904_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"date1904"
        || !path
            .last()
            .is_some_and(|part| part.as_slice() == b"chartSpace")
    {
        return None;
    }
    update.date_1904.map(bool_xml_value)
}

fn chart_style_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    start: &quick_xml::events::BytesStart,
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"style" || !is_chart_space_style_path(path) {
        return None;
    }
    let value = update.chart_style?;
    let current = attr_local_value(start, b"val").and_then(|value| value.parse::<u32>().ok());
    let prefix = element_prefix(start.name().as_ref());
    let c14_style = prefix.as_deref() == Some("c14") || current.is_some_and(|value| value >= 100);
    Some(if c14_style { value + 100 } else { value }.to_string())
}

fn is_chart_space_style_path(path: &[Vec<u8>]) -> bool {
    path.first()
        .is_some_and(|part| part.as_slice() == b"chartSpace")
        && !path.iter().any(|part| part.as_slice() == b"chart")
}

fn attr_local_value(start: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    start.attributes().flatten().find_map(|attr| {
        if local_name(attr.key.as_ref()) == key {
            Some(String::from_utf8_lossy(attr.value.as_ref()).to_string())
        } else {
            None
        }
    })
}

fn rounded_corners_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"roundedCorners"
        || !path
            .last()
            .is_some_and(|part| part.as_slice() == b"chartSpace")
    {
        return None;
    }
    update.rounded_corners.map(bool_xml_value)
}

fn auto_title_deleted_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"autoTitleDeleted" || !path.last().is_some_and(|part| part.as_slice() == b"chart")
    {
        return None;
    }
    update.auto_title_deleted.map(bool_xml_value)
}

fn is_auto_title_deleted_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    if !path.last().is_some_and(|part| part.as_slice() == b"chart") {
        return false;
    }
    matches!(
        local,
        b"pivotFmts"
            | b"view3D"
            | b"floor"
            | b"sideWall"
            | b"backWall"
            | b"plotArea"
            | b"legend"
            | b"plotVisOnly"
            | b"dispBlanksAs"
            | b"showDLblsOverMax"
            | b"extLst"
    )
}

fn write_missing_auto_title_deleted<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    auto_title_deleted_updated: &mut bool,
) -> Result<(), String> {
    if let Some(value) = update.auto_title_deleted {
        if !*auto_title_deleted_updated {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "autoTitleDeleted",
                bool_xml_value(value),
            )?;
            *auto_title_deleted_updated = true;
        }
    }
    Ok(())
}

fn vary_colors_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"varyColors"
        || !path
            .last()
            .is_some_and(|part| is_chart_plot(part.as_slice()))
    {
        return None;
    }
    update.vary_colors.map(bool_xml_value)
}

fn is_vary_colors_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    if !path
        .last()
        .is_some_and(|part| is_chart_plot(part.as_slice()))
    {
        return false;
    }
    matches!(
        local,
        b"ser"
            | b"dLbls"
            | b"dropLines"
            | b"hiLowLines"
            | b"upDownBars"
            | b"marker"
            | b"smooth"
            | b"gapWidth"
            | b"overlap"
            | b"firstSliceAng"
            | b"holeSize"
            | b"ofPieType"
            | b"secondPieSize"
            | b"splitType"
            | b"splitPos"
            | b"custSplit"
            | b"scatterStyle"
            | b"axId"
            | b"extLst"
    )
}

fn write_missing_vary_colors<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    vary_colors_updated: &mut bool,
) -> Result<(), String> {
    if let Some(value) = update.vary_colors {
        if !*vary_colors_updated {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "varyColors",
                bool_xml_value(value),
            )?;
            *vary_colors_updated = true;
        }
    }
    Ok(())
}

fn has_view_3d_update(update: &ChartXmlUpdate) -> bool {
    update.view_3d_rotation_x.is_some()
        || update.view_3d_rotation_y.is_some()
        || update.view_3d_perspective.is_some()
        || update.view_3d_right_angle_axes.is_some()
        || update.view_3d_height_percent.is_some()
        || update.view_3d_depth_percent.is_some()
}

fn is_view_3d_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    if !path.last().is_some_and(|part| part.as_slice() == b"chart") {
        return false;
    }
    matches!(
        local,
        b"floor"
            | b"sideWall"
            | b"backWall"
            | b"plotArea"
            | b"legend"
            | b"plotVisOnly"
            | b"dispBlanksAs"
            | b"showDLblsOverMax"
            | b"extLst"
    )
}

fn view_3d_field_order() -> [View3DField; 6] {
    [
        View3DField::RightAngleAxes,
        View3DField::RotationX,
        View3DField::RotationY,
        View3DField::Perspective,
        View3DField::HeightPercent,
        View3DField::DepthPercent,
    ]
}

fn view_3d_child_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> usize {
    if !path.last().is_some_and(|part| part.as_slice() == b"view3D") {
        return 0;
    }
    view_3d_field_order()
        .iter()
        .position(|field| view_3d_field_local_name(*field).as_bytes() == local)
        .unwrap_or(0)
}

fn view_3d_field_local_name(field: View3DField) -> &'static str {
    match field {
        View3DField::RightAngleAxes => "rAngAx",
        View3DField::RotationX => "rotX",
        View3DField::RotationY => "rotY",
        View3DField::Perspective => "perspective",
        View3DField::HeightPercent => "hPercent",
        View3DField::DepthPercent => "depthPercent",
    }
}

fn view_3d_field_write_value(field: View3DField, update: &ChartXmlUpdate) -> Option<String> {
    match field {
        View3DField::RotationX => update.view_3d_rotation_x.map(|value| value.to_string()),
        View3DField::RotationY => update.view_3d_rotation_y.map(|value| value.to_string()),
        View3DField::Perspective => update.view_3d_perspective.map(|value| value.to_string()),
        View3DField::RightAngleAxes => update
            .view_3d_right_angle_axes
            .map(|value| bool_xml_value(value).to_string()),
        View3DField::HeightPercent => update.view_3d_height_percent.map(|value| value.to_string()),
        View3DField::DepthPercent => update.view_3d_depth_percent.map(|value| value.to_string()),
    }
}

fn view_3d_field_is_updated(
    field: View3DField,
    rotation_x_updated: bool,
    rotation_y_updated: bool,
    perspective_updated: bool,
    right_angle_axes_updated: bool,
    height_percent_updated: bool,
    depth_percent_updated: bool,
) -> bool {
    match field {
        View3DField::RotationX => rotation_x_updated,
        View3DField::RotationY => rotation_y_updated,
        View3DField::Perspective => perspective_updated,
        View3DField::RightAngleAxes => right_angle_axes_updated,
        View3DField::HeightPercent => height_percent_updated,
        View3DField::DepthPercent => depth_percent_updated,
    }
}

fn mark_view_3d_field_updated(
    field: View3DField,
    rotation_x_updated: &mut bool,
    rotation_y_updated: &mut bool,
    perspective_updated: &mut bool,
    right_angle_axes_updated: &mut bool,
    height_percent_updated: &mut bool,
    depth_percent_updated: &mut bool,
) {
    match field {
        View3DField::RotationX => *rotation_x_updated = true,
        View3DField::RotationY => *rotation_y_updated = true,
        View3DField::Perspective => *perspective_updated = true,
        View3DField::RightAngleAxes => *right_angle_axes_updated = true,
        View3DField::HeightPercent => *height_percent_updated = true,
        View3DField::DepthPercent => *depth_percent_updated = true,
    }
}

fn write_view_3d_with_children<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    rotation_x_updated: &mut bool,
    rotation_y_updated: &mut bool,
    perspective_updated: &mut bool,
    right_angle_axes_updated: &mut bool,
    height_percent_updated: &mut bool,
    depth_percent_updated: &mut bool,
) -> Result<(), String> {
    if !has_view_3d_update(update) {
        return Ok(());
    }
    let chart_prefix = element_prefix(prefix_source_name);
    let name = qualified_name(chart_prefix.as_deref(), "view3D");
    writer
        .write_event(Event::Start(BytesStart::new(name.as_str())))
        .map_err(|e| e.to_string())?;
    write_missing_view_3d_children_until(
        writer,
        prefix_source_name,
        update,
        view_3d_field_order().len(),
        rotation_x_updated,
        rotation_y_updated,
        perspective_updated,
        right_angle_axes_updated,
        height_percent_updated,
        depth_percent_updated,
    )?;
    writer
        .write_event(Event::End(BytesEnd::new(name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_missing_view_3d_children_until<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    limit: usize,
    rotation_x_updated: &mut bool,
    rotation_y_updated: &mut bool,
    perspective_updated: &mut bool,
    right_angle_axes_updated: &mut bool,
    height_percent_updated: &mut bool,
    depth_percent_updated: &mut bool,
) -> Result<(), String> {
    for (index, field) in view_3d_field_order().into_iter().enumerate() {
        if index >= limit {
            break;
        }
        if view_3d_field_is_updated(
            field,
            *rotation_x_updated,
            *rotation_y_updated,
            *perspective_updated,
            *right_angle_axes_updated,
            *height_percent_updated,
            *depth_percent_updated,
        ) {
            continue;
        }
        let Some(value) = view_3d_field_write_value(field, update) else {
            continue;
        };
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            view_3d_field_local_name(field),
            &value,
        )?;
        mark_view_3d_field_updated(
            field,
            rotation_x_updated,
            rotation_y_updated,
            perspective_updated,
            right_angle_axes_updated,
            height_percent_updated,
            depth_percent_updated,
        );
    }
    Ok(())
}

fn view_3d_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<(View3DField, String)> {
    if !path.last().is_some_and(|part| part.as_slice() == b"view3D") {
        return None;
    }
    match local {
        b"rotX" => update
            .view_3d_rotation_x
            .map(|value| (View3DField::RotationX, value.to_string())),
        b"rotY" => update
            .view_3d_rotation_y
            .map(|value| (View3DField::RotationY, value.to_string())),
        b"perspective" => update
            .view_3d_perspective
            .map(|value| (View3DField::Perspective, value.to_string())),
        b"rAngAx" => update.view_3d_right_angle_axes.map(|value| {
            (
                View3DField::RightAngleAxes,
                bool_xml_value(value).to_string(),
            )
        }),
        b"hPercent" => update
            .view_3d_height_percent
            .map(|value| (View3DField::HeightPercent, value.to_string())),
        b"depthPercent" => update
            .view_3d_depth_percent
            .map(|value| (View3DField::DepthPercent, value.to_string())),
        _ => None,
    }
}

fn display_blanks_as_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"dispBlanksAs" || !path.last().is_some_and(|part| part.as_slice() == b"chart") {
        return None;
    }
    update.display_blanks_as.map(display_blanks_as_xml_value)
}

fn show_hidden_data_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"showHiddenData" || !path.last().is_some_and(|part| part.as_slice() == b"chart") {
        return None;
    }
    update.show_hidden_data.map(bool_xml_value)
}

fn plot_visible_only_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"plotVisOnly" || !path.last().is_some_and(|part| part.as_slice() == b"chart") {
        return None;
    }
    update.plot_visible_only.map(bool_xml_value)
}

fn has_data_table_update(update: &ChartXmlUpdate) -> bool {
    update.data_table_show_horizontal_border.is_some()
        || update.data_table_show_vertical_border.is_some()
        || update.data_table_show_outline.is_some()
        || update.data_table_show_keys.is_some()
}

fn data_table_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<(DataTableField, &'static str)> {
    if !path.last().is_some_and(|part| part.as_slice() == b"dTable") {
        return None;
    }
    match local {
        b"showHorzBorder" => update
            .data_table_show_horizontal_border
            .map(|value| (DataTableField::ShowHorizontalBorder, bool_xml_value(value))),
        b"showVertBorder" => update
            .data_table_show_vertical_border
            .map(|value| (DataTableField::ShowVerticalBorder, bool_xml_value(value))),
        b"showOutline" => update
            .data_table_show_outline
            .map(|value| (DataTableField::ShowOutline, bool_xml_value(value))),
        b"showKeys" => update
            .data_table_show_keys
            .map(|value| (DataTableField::ShowKeys, bool_xml_value(value))),
        _ => None,
    }
}

fn data_table_requested_fields(update: &ChartXmlUpdate) -> Vec<DataTableField> {
    let mut fields = Vec::new();
    if update.data_table_show_horizontal_border.is_some() {
        fields.push(DataTableField::ShowHorizontalBorder);
    }
    if update.data_table_show_vertical_border.is_some() {
        fields.push(DataTableField::ShowVerticalBorder);
    }
    if update.data_table_show_outline.is_some() {
        fields.push(DataTableField::ShowOutline);
    }
    if update.data_table_show_keys.is_some() {
        fields.push(DataTableField::ShowKeys);
    }
    fields
}

fn data_table_field_update_value(
    field: DataTableField,
    update: &ChartXmlUpdate,
) -> Option<(&'static str, &'static str)> {
    match field {
        DataTableField::ShowHorizontalBorder => update
            .data_table_show_horizontal_border
            .map(|value| ("showHorzBorder", bool_xml_value(value))),
        DataTableField::ShowVerticalBorder => update
            .data_table_show_vertical_border
            .map(|value| ("showVertBorder", bool_xml_value(value))),
        DataTableField::ShowOutline => update
            .data_table_show_outline
            .map(|value| ("showOutline", bool_xml_value(value))),
        DataTableField::ShowKeys => update
            .data_table_show_keys
            .map(|value| ("showKeys", bool_xml_value(value))),
    }
}

fn is_data_table_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    matches!(local, b"spPr" | b"extLst") && is_plot_area_parent_path(path)
}

fn is_data_table_path(path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| part.as_slice() == b"dTable")
        && path
            .iter()
            .rev()
            .nth(1)
            .is_some_and(|part| part.as_slice() == b"plotArea")
}

fn overlay_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<(OverlayScope, &'static str)> {
    if local != b"overlay" {
        return None;
    }
    if is_direct_chart_child_path(path, b"title") {
        return update
            .title_overlay
            .map(|value| (OverlayScope::Title, bool_xml_value(value)));
    }
    if is_direct_chart_child_path(path, b"legend") {
        return update
            .legend_overlay
            .map(|value| (OverlayScope::Legend, bool_xml_value(value)));
    }
    None
}

fn is_overlay_insertion_point(local: &[u8], path: &[Vec<u8>], scope: OverlayScope) -> bool {
    let parent = match scope {
        OverlayScope::Title => b"title".as_slice(),
        OverlayScope::Legend => b"legend".as_slice(),
    };
    is_direct_chart_child_path(path, parent) && matches!(local, b"spPr" | b"txPr" | b"extLst")
}

fn write_missing_overlay<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    scope: OverlayScope,
    overlay_updated: &mut bool,
) -> Result<(), String> {
    if *overlay_updated {
        return Ok(());
    }
    let value = match scope {
        OverlayScope::Title => update.title_overlay,
        OverlayScope::Legend => update.legend_overlay,
    };
    if let Some(value) = value {
        write_chart_empty_with_val(writer, prefix_source_name, "overlay", bool_xml_value(value))?;
        *overlay_updated = true;
    }
    Ok(())
}

fn display_blanks_as_xml_value(value: ChartDisplayBlanksAs) -> &'static str {
    match value {
        ChartDisplayBlanksAs::Gap => "gap",
        ChartDisplayBlanksAs::Span => "span",
        ChartDisplayBlanksAs::Zero => "zero",
    }
}

fn has_stock_bar_update(update: &ChartXmlUpdate) -> bool {
    update.stock_up_down_bar_gap_width.is_some()
        || update.stock_up_bar_fill_color.is_some()
        || update.stock_down_bar_fill_color.is_some()
        || update.stock_up_bar_line_color.is_some()
        || update.stock_down_bar_line_color.is_some()
        || update.stock_up_bar_line_width.is_some()
        || update.stock_down_bar_line_width.is_some()
}

fn has_stock_hi_low_line_update(update: &ChartXmlUpdate) -> bool {
    update.stock_hi_low_line_color.is_some() || update.stock_hi_low_line_width.is_some()
}

fn has_of_pie_ser_line_update(update: &ChartXmlUpdate) -> bool {
    update.pie_of_pie_ser_line_color.is_some() || update.pie_of_pie_ser_line_width.is_some()
}

fn is_stock_hi_low_lines(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"hiLowLines"
        && path
            .last()
            .is_some_and(|part| part.as_slice() == b"stockChart")
}

fn is_of_pie_ser_lines(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"serLines"
        && path
            .last()
            .is_some_and(|part| part.as_slice() == b"ofPieChart")
}

fn is_stock_chart_ax_id(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"axId"
        && path
            .last()
            .is_some_and(|part| part.as_slice() == b"stockChart")
}

fn legend_position_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"legendPos" || !path.last().is_some_and(|part| part.as_slice() == b"legend") {
        return None;
    }
    update.legend_position.map(legend_position_value)
}

fn legend_position_value(position: ChartLegendPosition) -> &'static str {
    match position {
        ChartLegendPosition::Right => "r",
        ChartLegendPosition::Left => "l",
        ChartLegendPosition::Top => "t",
        ChartLegendPosition::Bottom => "b",
        ChartLegendPosition::TopRight => "tr",
    }
}

fn is_legend_position_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| part.as_slice() == b"legend")
        && matches!(
            local,
            b"layout" | b"overlay" | b"spPr" | b"txPr" | b"extLst"
        )
}

fn write_missing_legend_position<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    seen: bool,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated || seen {
        return Ok(());
    }
    if let Some(position) = update.legend_position {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "legendPos",
            legend_position_value(position),
        )?;
        *updated = true;
    }
    Ok(())
}

fn category_axis_visibility_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"delete" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update
        .category_axis_visible
        .map(axis_visibility_delete_value)
}

fn value_axis_visibility_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"delete" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update.value_axis_visible.map(axis_visibility_delete_value)
}

fn write_missing_axis_visibility_delete<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    seen: bool,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated || seen {
        return Ok(());
    }
    let requested = match axis {
        AxisKind::Category => update.category_axis_visible,
        AxisKind::Value => update.value_axis_visible,
    };
    if let Some(visible) = requested {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "delete",
            axis_visibility_delete_value(visible),
        )?;
        *updated = true;
    }
    Ok(())
}

fn axis_title_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if !matches!(
        local,
        b"numFmt"
            | b"majorTickMark"
            | b"minorTickMark"
            | b"tickLblPos"
            | b"spPr"
            | b"txPr"
            | b"crossAx"
            | b"crosses"
            | b"crossesAt"
            | b"crossBetween"
            | b"auto"
            | b"lblAlgn"
            | b"lblOffset"
            | b"tickLblSkip"
            | b"tickMarkSkip"
            | b"noMultiLvlLbl"
            | b"majorUnit"
            | b"minorUnit"
            | b"dispUnits"
            | b"extLst"
    ) {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => Some(AxisKind::Category),
        Some(b"valAx") => Some(AxisKind::Value),
        _ => None,
    }
}

fn write_missing_axis_title<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated {
        return Ok(());
    }
    if let Some(title) = axis_title_update(axis, update) {
        write_axis_title_with_text(writer, prefix_source_name, title)?;
        *updated = true;
    }
    Ok(())
}

fn axis_position_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if !matches!(
        local,
        b"majorGridlines"
            | b"minorGridlines"
            | b"title"
            | b"numFmt"
            | b"majorTickMark"
            | b"minorTickMark"
            | b"tickLblPos"
            | b"spPr"
            | b"txPr"
            | b"crossAx"
            | b"crosses"
            | b"crossesAt"
            | b"crossBetween"
            | b"auto"
            | b"lblAlgn"
            | b"lblOffset"
            | b"tickLblSkip"
            | b"tickMarkSkip"
            | b"noMultiLvlLbl"
            | b"majorUnit"
            | b"minorUnit"
            | b"dispUnits"
            | b"extLst"
    ) {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => Some(AxisKind::Category),
        Some(b"valAx") => Some(AxisKind::Value),
        _ => None,
    }
}

fn write_missing_axis_position<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    seen: bool,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated || seen {
        return Ok(());
    }
    let requested = match axis {
        AxisKind::Category => update.category_axis_position,
        AxisKind::Value => update.value_axis_position,
    };
    if let Some(position) = requested {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "axPos",
            axis_position_value(position),
        )?;
        *updated = true;
    }
    Ok(())
}

fn axis_label_position_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if !matches!(
        local,
        b"spPr"
            | b"txPr"
            | b"crossAx"
            | b"crosses"
            | b"crossBetween"
            | b"auto"
            | b"lblAlgn"
            | b"lblOffset"
            | b"tickLblSkip"
            | b"tickMarkSkip"
            | b"noMultiLvlLbl"
            | b"majorUnit"
            | b"minorUnit"
            | b"dispUnits"
            | b"extLst"
    ) {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => Some(AxisKind::Category),
        Some(b"valAx") => Some(AxisKind::Value),
        _ => None,
    }
}

fn write_missing_axis_label_position<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    seen: bool,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated || seen {
        return Ok(());
    }
    let requested = match axis {
        AxisKind::Category => update.category_axis_label_position,
        AxisKind::Value => update.value_axis_label_position,
    };
    if let Some(position) = requested {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "tickLblPos",
            axis_label_position_value(position),
        )?;
        *updated = true;
    }
    Ok(())
}

fn axis_crosses_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx")
            if matches!(
                local,
                b"crossesAt"
                    | b"auto"
                    | b"lblAlgn"
                    | b"lblOffset"
                    | b"tickLblSkip"
                    | b"tickMarkSkip"
                    | b"noMultiLvlLbl"
                    | b"extLst"
            ) =>
        {
            Some(AxisKind::Category)
        }
        Some(b"valAx")
            if matches!(
                local,
                b"crossesAt"
                    | b"crossBetween"
                    | b"majorUnit"
                    | b"minorUnit"
                    | b"dispUnits"
                    | b"extLst"
            ) =>
        {
            Some(AxisKind::Value)
        }
        _ => None,
    }
}

fn axis_crosses_at_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx")
            if matches!(
                local,
                b"auto"
                    | b"lblAlgn"
                    | b"lblOffset"
                    | b"tickLblSkip"
                    | b"tickMarkSkip"
                    | b"noMultiLvlLbl"
                    | b"extLst"
            ) =>
        {
            Some(AxisKind::Category)
        }
        Some(b"valAx")
            if matches!(
                local,
                b"crossBetween" | b"majorUnit" | b"minorUnit" | b"dispUnits" | b"extLst"
            ) =>
        {
            Some(AxisKind::Value)
        }
        _ => None,
    }
}

fn is_value_axis_cross_between_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    path.last().is_some_and(|part| part.as_slice() == b"valAx")
        && matches!(
            local,
            b"majorUnit" | b"minorUnit" | b"dispUnits" | b"extLst"
        )
}

fn axis_number_format_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if !matches!(
        local,
        b"majorTickMark"
            | b"minorTickMark"
            | b"tickLblPos"
            | b"spPr"
            | b"txPr"
            | b"crossAx"
            | b"crosses"
            | b"crossesAt"
            | b"crossBetween"
            | b"auto"
            | b"lblAlgn"
            | b"lblOffset"
            | b"tickLblSkip"
            | b"tickMarkSkip"
            | b"noMultiLvlLbl"
            | b"majorUnit"
            | b"minorUnit"
            | b"dispUnits"
            | b"extLst"
    ) {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => Some(AxisKind::Category),
        Some(b"valAx") => Some(AxisKind::Value),
        _ => None,
    }
}

fn write_missing_axis_crosses<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated {
        return Ok(());
    }
    let requested = match axis {
        AxisKind::Category => update.category_axis_crosses,
        AxisKind::Value => update.value_axis_crosses,
    };
    if let Some(value) = requested {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "crosses",
            axis_crosses_value(value),
        )?;
        *updated = true;
    }
    Ok(())
}

fn write_missing_axis_crosses_at<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated {
        return Ok(());
    }
    let requested = match axis {
        AxisKind::Category => update.category_axis_crosses_at,
        AxisKind::Value => update.value_axis_crosses_at,
    };
    if let Some(value) = requested {
        write_axis_number_empty(writer, prefix_source_name, "crossesAt", value)?;
        *updated = true;
    }
    Ok(())
}

fn should_remove_axis_cross_choice(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> bool {
    let axis = match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => AxisKind::Category,
        Some(b"valAx") => AxisKind::Value,
        _ => return false,
    };
    match (axis, local) {
        (AxisKind::Category, b"crosses") => {
            update.category_axis_crosses.is_none() && update.category_axis_crosses_at.is_some()
        }
        (AxisKind::Category, b"crossesAt") => {
            update.category_axis_crosses.is_some() && update.category_axis_crosses_at.is_none()
        }
        (AxisKind::Value, b"crosses") => {
            update.value_axis_crosses.is_none() && update.value_axis_crosses_at.is_some()
        }
        (AxisKind::Value, b"crossesAt") => {
            update.value_axis_crosses.is_some() && update.value_axis_crosses_at.is_none()
        }
        _ => false,
    }
}

fn axis_tick_mark_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !matches!(
        local,
        b"minorTickMark"
            | b"tickLblPos"
            | b"spPr"
            | b"txPr"
            | b"crossAx"
            | b"crosses"
            | b"crossesAt"
            | b"crossBetween"
            | b"auto"
            | b"lblAlgn"
            | b"lblOffset"
            | b"tickLblSkip"
            | b"tickMarkSkip"
            | b"noMultiLvlLbl"
            | b"majorUnit"
            | b"minorUnit"
            | b"dispUnits"
            | b"extLst"
    ) {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") | Some(b"valAx") => {
            if local == b"minorTickMark" {
                Some(1)
            } else {
                Some(2)
            }
        }
        _ => None,
    }
}

fn write_missing_axis_tick_marks<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    limit: u8,
    update: &ChartXmlUpdate,
    major_updated: &mut bool,
    minor_updated: &mut bool,
) -> Result<(), String> {
    if limit >= 1 && !*major_updated {
        let requested = match axis {
            AxisKind::Category => update.category_axis_major_tick_mark,
            AxisKind::Value => update.value_axis_major_tick_mark,
        };
        if let Some(value) = requested {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "majorTickMark",
                axis_tick_mark_value(value),
            )?;
            *major_updated = true;
        }
    }
    if limit >= 2 && !*minor_updated {
        let requested = match axis {
            AxisKind::Category => update.category_axis_minor_tick_mark,
            AxisKind::Value => update.value_axis_minor_tick_mark,
        };
        if let Some(value) = requested {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "minorTickMark",
                axis_tick_mark_value(value),
            )?;
            *minor_updated = true;
        }
    }
    Ok(())
}

fn write_missing_axis_number_format<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated {
        return Ok(());
    }
    let (format, source_linked) = axis_number_format_update(axis, update);
    if format.is_none() && source_linked.is_none() {
        return Ok(());
    }
    write_axis_num_fmt_empty(
        writer,
        prefix_source_name,
        format.unwrap_or("General"),
        source_linked,
    )?;
    *updated = true;
    Ok(())
}

fn write_missing_value_axis_cross_between<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated {
        return Ok(());
    }
    if let Some(value) = update.value_axis_cross_between {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "crossBetween",
            axis_cross_between_value(value),
        )?;
        *updated = true;
    }
    Ok(())
}

fn category_axis_label_control_insertion_limit(local: &[u8], path: &[Vec<u8>]) -> Option<u8> {
    if !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    match local {
        b"lblAlgn" => Some(1),
        b"lblOffset" => Some(2),
        b"tickLblSkip" | b"tickMarkSkip" => Some(3),
        b"noMultiLvlLbl" => Some(4),
        b"extLst" => Some(5),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn write_missing_category_axis_label_controls<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    limit: u8,
    update: &ChartXmlUpdate,
    auto_updated: &mut bool,
    alignment_updated: &mut bool,
    offset_updated: &mut bool,
    tick_mark_skip_updated: &mut bool,
    no_multi_level_labels_updated: &mut bool,
) -> Result<(), String> {
    if limit >= 1 && !*auto_updated {
        if let Some(value) = update.category_axis_auto {
            write_chart_empty_with_val(writer, prefix_source_name, "auto", bool_xml_value(value))?;
            *auto_updated = true;
        }
    }
    if limit >= 2 && !*alignment_updated {
        if let Some(value) = update.category_axis_label_alignment {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "lblAlgn",
                axis_label_alignment_value(value),
            )?;
            *alignment_updated = true;
        }
    }
    if limit >= 3 && !*offset_updated {
        if let Some(value) = update.category_axis_label_offset {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "lblOffset",
                &value.to_string(),
            )?;
            *offset_updated = true;
        }
    }
    if limit >= 4 && !*tick_mark_skip_updated {
        if let Some(value) = update.category_axis_tick_mark_skip {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "tickMarkSkip",
                &value.to_string(),
            )?;
            *tick_mark_skip_updated = true;
        }
    }
    if limit >= 5 && !*no_multi_level_labels_updated {
        if let Some(value) = update.category_axis_no_multi_level_labels {
            write_chart_empty_with_val(
                writer,
                prefix_source_name,
                "noMultiLvlLbl",
                bool_xml_value(value),
            )?;
            *no_multi_level_labels_updated = true;
        }
    }
    Ok(())
}

fn axis_scaling_element_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if local != b"scaling" {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => Some(AxisKind::Category),
        Some(b"valAx") => Some(AxisKind::Value),
        _ => None,
    }
}

fn axis_scaling_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if !matches!(
        local,
        b"delete"
            | b"axPos"
            | b"majorGridlines"
            | b"minorGridlines"
            | b"title"
            | b"numFmt"
            | b"majorTickMark"
            | b"minorTickMark"
            | b"tickLblPos"
            | b"spPr"
            | b"txPr"
            | b"crossAx"
            | b"crosses"
            | b"crossesAt"
            | b"crossBetween"
            | b"auto"
            | b"lblAlgn"
            | b"lblOffset"
            | b"tickLblSkip"
            | b"tickMarkSkip"
            | b"noMultiLvlLbl"
            | b"majorUnit"
            | b"minorUnit"
            | b"dispUnits"
            | b"extLst"
    ) {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => Some(AxisKind::Category),
        Some(b"valAx") => Some(AxisKind::Value),
        _ => None,
    }
}

fn axis_orientation_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if !matches!(local, b"max" | b"min" | b"extLst") {
        return None;
    }
    axis_scaling_path_kind(path)
}

fn value_axis_log_base_insertion_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if !matches!(local, b"orientation" | b"max" | b"min" | b"extLst") {
        return None;
    }
    match axis_scaling_path_kind(path) {
        Some(AxisKind::Value) => Some(AxisKind::Value),
        _ => None,
    }
}

fn is_value_axis_display_units_insertion_point(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"extLst" && path.last().is_some_and(|part| part.as_slice() == b"valAx")
}

fn is_empty_value_axis_display_units(local: &[u8], path: &[Vec<u8>]) -> bool {
    local == b"dispUnits" && path.last().is_some_and(|part| part.as_slice() == b"valAx")
}

fn is_value_axis_display_units_path(path: &[Vec<u8>]) -> bool {
    path.len() >= 2
        && path
            .last()
            .is_some_and(|part| part.as_slice() == b"dispUnits")
        && path[path.len() - 2].as_slice() == b"valAx"
}

fn axis_scaling_path_kind(path: &[Vec<u8>]) -> Option<AxisKind> {
    if !path
        .last()
        .is_some_and(|part| part.as_slice() == b"scaling")
    {
        return None;
    }
    match path
        .get(path.len().saturating_sub(2))
        .map(|part| part.as_slice())
    {
        Some(b"catAx") => Some(AxisKind::Category),
        Some(b"valAx") => Some(AxisKind::Value),
        _ => None,
    }
}

fn axis_orientation_request(axis: AxisKind, update: &ChartXmlUpdate) -> Option<AxisOrientation> {
    match axis {
        AxisKind::Category => update.category_axis_orientation,
        AxisKind::Value => update.value_axis_orientation,
    }
}

fn has_axis_scaling_child_update(
    axis: AxisKind,
    update: &ChartXmlUpdate,
    orientation_updated: bool,
    value_axis_log_base_updated: bool,
    value_axis_maximum_updated: bool,
    value_axis_minimum_updated: bool,
) -> bool {
    axis_orientation_request(axis, update).is_some() && !orientation_updated
        || axis == AxisKind::Value
            && ((update.value_axis_log_base.is_some() && !value_axis_log_base_updated)
                || (update.value_axis_maximum.is_some() && !value_axis_maximum_updated)
                || (update.value_axis_minimum.is_some() && !value_axis_minimum_updated))
}

fn write_missing_axis_orientation<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated {
        return Ok(());
    }
    if let Some(value) = axis_orientation_request(axis, update) {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "orientation",
            axis_orientation_value(value),
        )?;
        *updated = true;
    }
    Ok(())
}

fn write_missing_value_axis_log_base<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if axis != AxisKind::Value || *updated {
        return Ok(());
    }
    if let Some(value) = update.value_axis_log_base {
        write_axis_number_empty(writer, prefix_source_name, "logBase", value)?;
        *updated = true;
    }
    Ok(())
}

fn write_missing_value_axis_display_unit<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated {
        return Ok(());
    }
    if let Some(value) = update.value_axis_display_unit {
        write_chart_empty_with_val(
            writer,
            prefix_source_name,
            "builtInUnit",
            axis_display_unit_value(value),
        )?;
        *updated = true;
    }
    Ok(())
}

fn write_missing_value_axis_display_units<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    updated: &mut bool,
) -> Result<(), String> {
    if *updated || update.value_axis_display_unit.is_none() {
        return Ok(());
    }

    let prefix = element_prefix(prefix_source_name);
    let disp_units_name = qualified_name(prefix.as_deref(), "dispUnits");
    writer
        .write_event(Event::Start(BytesStart::new(disp_units_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_missing_value_axis_display_unit(writer, disp_units_name.as_bytes(), update, updated)?;
    writer
        .write_event(Event::End(BytesEnd::new(disp_units_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_missing_axis_scaling_with_orientation<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    scaling_seen: bool,
    update: &ChartXmlUpdate,
    orientation_updated: &mut bool,
    value_axis_log_base_updated: &mut bool,
    value_axis_maximum_updated: &mut bool,
    value_axis_minimum_updated: &mut bool,
) -> Result<(), String> {
    if scaling_seen
        || !has_axis_scaling_child_update(
            axis,
            update,
            *orientation_updated,
            *value_axis_log_base_updated,
            *value_axis_maximum_updated,
            *value_axis_minimum_updated,
        )
    {
        return Ok(());
    }
    write_axis_scaling_with_requested_children(
        writer,
        prefix_source_name,
        axis,
        update,
        orientation_updated,
        value_axis_log_base_updated,
        value_axis_maximum_updated,
        value_axis_minimum_updated,
    )
}

fn write_axis_scaling_with_requested_children<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    axis: AxisKind,
    update: &ChartXmlUpdate,
    orientation_updated: &mut bool,
    value_axis_log_base_updated: &mut bool,
    value_axis_maximum_updated: &mut bool,
    value_axis_minimum_updated: &mut bool,
) -> Result<(), String> {
    if !has_axis_scaling_child_update(
        axis,
        update,
        *orientation_updated,
        *value_axis_log_base_updated,
        *value_axis_maximum_updated,
        *value_axis_minimum_updated,
    ) {
        return Ok(());
    }

    let prefix = element_prefix(prefix_source_name);
    let scaling_name = qualified_name(prefix.as_deref(), "scaling");
    writer
        .write_event(Event::Start(BytesStart::new(scaling_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_missing_value_axis_log_base(
        writer,
        scaling_name.as_bytes(),
        axis,
        update,
        value_axis_log_base_updated,
    )?;
    write_missing_axis_orientation(
        writer,
        scaling_name.as_bytes(),
        axis,
        update,
        orientation_updated,
    )?;
    if axis == AxisKind::Value {
        if let Some(value) = update.value_axis_maximum {
            if !*value_axis_maximum_updated {
                write_axis_number_empty(writer, scaling_name.as_bytes(), "max", value)?;
                *value_axis_maximum_updated = true;
            }
        }
        if let Some(value) = update.value_axis_minimum {
            if !*value_axis_minimum_updated {
                write_axis_number_empty(writer, scaling_name.as_bytes(), "min", value)?;
                *value_axis_minimum_updated = true;
            }
        }
    }
    writer
        .write_event(Event::End(BytesEnd::new(scaling_name.as_str())))
        .map_err(|e| e.to_string())
}

fn category_axis_position_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"axPos" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update.category_axis_position.map(axis_position_value)
}

fn value_axis_position_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"axPos" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update.value_axis_position.map(axis_position_value)
}

fn category_axis_label_position_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"tickLblPos" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update
        .category_axis_label_position
        .map(axis_label_position_value)
}

fn value_axis_label_position_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"tickLblPos" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update
        .value_axis_label_position
        .map(axis_label_position_value)
}

fn category_axis_auto_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"auto" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update.category_axis_auto.map(bool_xml_value)
}

fn category_axis_label_alignment_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"lblAlgn" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update
        .category_axis_label_alignment
        .map(axis_label_alignment_value)
}

fn category_axis_label_offset_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"lblOffset" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update
        .category_axis_label_offset
        .map(|value| value.to_string())
}

fn category_axis_tick_mark_skip_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"tickMarkSkip" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update
        .category_axis_tick_mark_skip
        .map(|value| value.to_string())
}

fn category_axis_no_multi_level_labels_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"noMultiLvlLbl" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update
        .category_axis_no_multi_level_labels
        .map(bool_xml_value)
}

fn category_axis_orientation_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"orientation" || !is_category_axis_scaling_path(path) {
        return None;
    }
    update.category_axis_orientation.map(axis_orientation_value)
}

fn value_axis_orientation_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"orientation" || !is_value_axis_scaling_path(path) {
        return None;
    }
    update.value_axis_orientation.map(axis_orientation_value)
}

fn category_axis_crosses_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"crosses" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update.category_axis_crosses.map(axis_crosses_value)
}

fn category_axis_crosses_at_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"crossesAt" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update.category_axis_crosses_at.map(format_chart_number)
}

fn value_axis_crosses_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"crosses" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update.value_axis_crosses.map(axis_crosses_value)
}

fn value_axis_crosses_at_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"crossesAt" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update.value_axis_crosses_at.map(format_chart_number)
}

fn value_axis_cross_between_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"crossBetween" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update
        .value_axis_cross_between
        .map(axis_cross_between_value)
}

fn category_axis_major_tick_mark_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"majorTickMark" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update
        .category_axis_major_tick_mark
        .map(axis_tick_mark_value)
}

fn category_axis_minor_tick_mark_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"minorTickMark" || !path.last().is_some_and(|part| part.as_slice() == b"catAx") {
        return None;
    }
    update
        .category_axis_minor_tick_mark
        .map(axis_tick_mark_value)
}

fn value_axis_major_tick_mark_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"majorTickMark" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update.value_axis_major_tick_mark.map(axis_tick_mark_value)
}

fn value_axis_minor_tick_mark_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"minorTickMark" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update.value_axis_minor_tick_mark.map(axis_tick_mark_value)
}

fn value_axis_minimum_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"min" || !is_value_axis_scaling_path(path) {
        return None;
    }
    update.value_axis_minimum.map(format_chart_number)
}

fn value_axis_log_base_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"logBase" || !is_value_axis_scaling_path(path) {
        return None;
    }
    update.value_axis_log_base.map(format_chart_number)
}

fn value_axis_display_unit_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<&'static str> {
    if local != b"builtInUnit" || !is_value_axis_display_units_path(path) {
        return None;
    }
    update.value_axis_display_unit.map(axis_display_unit_value)
}

fn value_axis_maximum_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"max" || !is_value_axis_scaling_path(path) {
        return None;
    }
    update.value_axis_maximum.map(format_chart_number)
}

fn value_axis_major_unit_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"majorUnit" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update.value_axis_major_unit.map(format_chart_number)
}

fn value_axis_minor_unit_update_value(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<String> {
    if local != b"minorUnit" || !path.last().is_some_and(|part| part.as_slice() == b"valAx") {
        return None;
    }
    update.value_axis_minor_unit.map(format_chart_number)
}

fn axis_number_format_update_kind(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<AxisKind> {
    if local != b"numFmt" {
        return None;
    }
    let axis = match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => AxisKind::Category,
        Some(b"valAx") => AxisKind::Value,
        _ => return None,
    };
    let (format, source_linked) = axis_number_format_update(axis, update);
    if format.is_some() || source_linked.is_some() {
        Some(axis)
    } else {
        None
    }
}

fn axis_number_format_update(
    axis: AxisKind,
    update: &ChartXmlUpdate,
) -> (Option<&str>, Option<bool>) {
    match axis {
        AxisKind::Category => (
            update.category_axis_number_format.as_deref(),
            update.category_axis_number_format_source_linked,
        ),
        AxisKind::Value => (
            update.value_axis_number_format.as_deref(),
            update.value_axis_number_format_source_linked,
        ),
    }
}

fn is_value_axis_scaling_path(path: &[Vec<u8>]) -> bool {
    is_axis_scaling_path(path, b"valAx")
}

fn is_category_axis_scaling_path(path: &[Vec<u8>]) -> bool {
    is_axis_scaling_path(path, b"catAx")
}

fn is_axis_scaling_path(path: &[Vec<u8>], axis: &[u8]) -> bool {
    path.len() >= 2
        && path
            .last()
            .is_some_and(|part| part.as_slice() == b"scaling")
        && path[path.len() - 2].as_slice() == axis
}

fn is_direct_chart_child_path(path: &[Vec<u8>], child: &[u8]) -> bool {
    path.len() >= 2
        && path.last().is_some_and(|part| part.as_slice() == child)
        && path[path.len() - 2].as_slice() == b"chart"
}

fn axis_visibility_delete_value(visible: bool) -> &'static str {
    if visible {
        "0"
    } else {
        "1"
    }
}

fn axis_label_position_value(position: AxisLabelPosition) -> &'static str {
    match position {
        AxisLabelPosition::NextTo => "nextTo",
        AxisLabelPosition::High => "high",
        AxisLabelPosition::Low => "low",
        AxisLabelPosition::None => "none",
    }
}

fn axis_position_value(position: AxisPosition) -> &'static str {
    match position {
        AxisPosition::Bottom => "b",
        AxisPosition::Left => "l",
        AxisPosition::Top => "t",
        AxisPosition::Right => "r",
    }
}

fn axis_label_alignment_value(value: AxisLabelAlignment) -> &'static str {
    match value {
        AxisLabelAlignment::Center => "ctr",
        AxisLabelAlignment::Left => "l",
        AxisLabelAlignment::Right => "r",
    }
}

fn axis_orientation_value(value: AxisOrientation) -> &'static str {
    match value {
        AxisOrientation::MinMax => "minMax",
        AxisOrientation::MaxMin => "maxMin",
    }
}

fn axis_crosses_value(value: AxisCrosses) -> &'static str {
    match value {
        AxisCrosses::AutoZero => "autoZero",
        AxisCrosses::Min => "min",
        AxisCrosses::Max => "max",
    }
}

fn axis_cross_between_value(value: AxisCrossBetween) -> &'static str {
    match value {
        AxisCrossBetween::Between => "between",
        AxisCrossBetween::MidCategory => "midCat",
    }
}

fn axis_tick_mark_value(mark: AxisTickMark) -> &'static str {
    match mark {
        AxisTickMark::Cross => "cross",
        AxisTickMark::In => "in",
        AxisTickMark::Out => "out",
        AxisTickMark::None => "none",
    }
}

fn axis_display_unit_value(unit: AxisDisplayUnit) -> &'static str {
    match unit {
        AxisDisplayUnit::Hundreds => "hundreds",
        AxisDisplayUnit::Thousands => "thousands",
        AxisDisplayUnit::TenThousands => "tenThousands",
        AxisDisplayUnit::HundredThousands => "hundredThousands",
        AxisDisplayUnit::Millions => "millions",
        AxisDisplayUnit::TenMillions => "tenMillions",
        AxisDisplayUnit::HundredMillions => "hundredMillions",
        AxisDisplayUnit::Billions => "billions",
        AxisDisplayUnit::Trillions => "trillions",
    }
}

fn axis_sp_pr_update_kind(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<AxisKind> {
    let axis = axis_sp_pr_seen_kind(local, path)?;
    has_axis_line_update(axis, update).then_some(axis)
}

fn chart_space_sp_pr_update(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<u32> {
    if local != b"spPr" || !is_chart_space_parent_path(path) {
        return None;
    }
    update.chart_area_fill_color
}

fn is_chart_space_parent_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"chartSpace")
}

fn is_chart_space_late_child(local: &[u8]) -> bool {
    matches!(
        local,
        b"txPr" | b"externalData" | b"printSettings" | b"userShapes" | b"extLst"
    )
}

fn write_missing_chart_space_sp_pr<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    chart_area_fill_color_updated: &mut bool,
) -> Result<(), String> {
    if update.chart_area_fill_color.is_some() && !*chart_area_fill_color_updated {
        write_sp_pr_with_fill(writer, prefix_source_name, update.chart_area_fill_color)?;
        *chart_area_fill_color_updated = true;
    }
    Ok(())
}

fn plot_area_sp_pr_update(local: &[u8], path: &[Vec<u8>], update: &ChartXmlUpdate) -> Option<u32> {
    if local != b"spPr"
        || !path
            .last()
            .is_some_and(|part| part.as_slice() == b"plotArea")
    {
        return None;
    }
    update.plot_area_fill_color
}

fn is_plot_area_parent_path(path: &[Vec<u8>]) -> bool {
    path.last()
        .is_some_and(|part| part.as_slice() == b"plotArea")
}

fn write_missing_plot_area_sp_pr<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    plot_area_fill_color_updated: &mut bool,
) -> Result<(), String> {
    if update.plot_area_fill_color.is_some() && !*plot_area_fill_color_updated {
        write_sp_pr_with_fill(writer, prefix_source_name, update.plot_area_fill_color)?;
        *plot_area_fill_color_updated = true;
    }
    Ok(())
}

fn axis_sp_pr_seen_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisKind> {
    if local != b"spPr" {
        return None;
    }
    match path.last().map(|part| part.as_slice()) {
        Some(b"catAx") => Some(AxisKind::Category),
        Some(b"valAx") => Some(AxisKind::Value),
        _ => None,
    }
}

fn has_axis_line_update(axis: AxisKind, update: &ChartXmlUpdate) -> bool {
    match axis {
        AxisKind::Category => {
            update.category_axis_line_color.is_some() || update.category_axis_line_width.is_some()
        }
        AxisKind::Value => {
            update.value_axis_line_color.is_some() || update.value_axis_line_width.is_some()
        }
    }
}

fn axis_line_color_update(axis: AxisKind, update: &ChartXmlUpdate) -> Option<u32> {
    match axis {
        AxisKind::Category => update.category_axis_line_color,
        AxisKind::Value => update.value_axis_line_color,
    }
}

fn axis_line_width_update(axis: AxisKind, update: &ChartXmlUpdate) -> Option<u32> {
    match axis {
        AxisKind::Category => update.category_axis_line_width,
        AxisKind::Value => update.value_axis_line_width,
    }
}

impl AxisGridLineKind {
    fn xml_local_name(self) -> &'static str {
        match self {
            AxisGridLineKind::CategoryMajor | AxisGridLineKind::ValueMajor => "majorGridlines",
            AxisGridLineKind::CategoryMinor | AxisGridLineKind::ValueMinor => "minorGridlines",
        }
    }

    fn axis_local_name(self) -> &'static str {
        match self {
            AxisGridLineKind::CategoryMajor | AxisGridLineKind::CategoryMinor => "catAx",
            AxisGridLineKind::ValueMajor | AxisGridLineKind::ValueMinor => "valAx",
        }
    }

    fn color_field_name(self) -> &'static str {
        match self {
            AxisGridLineKind::CategoryMajor => "categoryAxisMajorGridLineColor",
            AxisGridLineKind::CategoryMinor => "categoryAxisMinorGridLineColor",
            AxisGridLineKind::ValueMajor => "valueAxisMajorGridLineColor",
            AxisGridLineKind::ValueMinor => "valueAxisMinorGridLineColor",
        }
    }

    fn width_field_name(self) -> &'static str {
        match self {
            AxisGridLineKind::CategoryMajor => "categoryAxisMajorGridLineWidth",
            AxisGridLineKind::CategoryMinor => "categoryAxisMinorGridLineWidth",
            AxisGridLineKind::ValueMajor => "valueAxisMajorGridLineWidth",
            AxisGridLineKind::ValueMinor => "valueAxisMinorGridLineWidth",
        }
    }
}

fn axis_grid_line_update_kind(
    local: &[u8],
    path: &[Vec<u8>],
    update: &ChartXmlUpdate,
) -> Option<AxisGridLineKind> {
    let kind = axis_grid_line_seen_kind(local, path)?;
    has_axis_grid_line_update(kind, update).then_some(kind)
}

fn axis_grid_line_seen_kind(local: &[u8], path: &[Vec<u8>]) -> Option<AxisGridLineKind> {
    match (path.last().map(|part| part.as_slice()), local) {
        (Some(b"catAx"), b"majorGridlines") => Some(AxisGridLineKind::CategoryMajor),
        (Some(b"catAx"), b"minorGridlines") => Some(AxisGridLineKind::CategoryMinor),
        (Some(b"valAx"), b"majorGridlines") => Some(AxisGridLineKind::ValueMajor),
        (Some(b"valAx"), b"minorGridlines") => Some(AxisGridLineKind::ValueMinor),
        _ => None,
    }
}

fn has_axis_grid_line_update(kind: AxisGridLineKind, update: &ChartXmlUpdate) -> bool {
    axis_grid_line_color_update(kind, update).is_some()
        || axis_grid_line_width_update(kind, update).is_some()
}

fn axis_grid_line_color_update(kind: AxisGridLineKind, update: &ChartXmlUpdate) -> Option<u32> {
    match kind {
        AxisGridLineKind::CategoryMajor => update.category_axis_major_grid_line_color,
        AxisGridLineKind::CategoryMinor => update.category_axis_minor_grid_line_color,
        AxisGridLineKind::ValueMajor => update.value_axis_major_grid_line_color,
        AxisGridLineKind::ValueMinor => update.value_axis_minor_grid_line_color,
    }
}

fn axis_grid_line_width_update(kind: AxisGridLineKind, update: &ChartXmlUpdate) -> Option<u32> {
    match kind {
        AxisGridLineKind::CategoryMajor => update.category_axis_major_grid_line_width,
        AxisGridLineKind::CategoryMinor => update.category_axis_minor_grid_line_width,
        AxisGridLineKind::ValueMajor => update.value_axis_major_grid_line_width,
        AxisGridLineKind::ValueMinor => update.value_axis_minor_grid_line_width,
    }
}

fn ensure_axis_grid_line_updates_applied(
    update: &ChartXmlUpdate,
    state: &GridLineUpdateState,
) -> Result<(), String> {
    for kind in AXIS_GRID_LINE_KINDS {
        if axis_grid_line_color_update(kind, update).is_some() && !state.is_color_updated(kind) {
            return Err(format!(
                "{} 변경 대상 c:{} 요소를 찾을 수 없습니다",
                kind.color_field_name(),
                kind.axis_local_name()
            ));
        }
        if axis_grid_line_width_update(kind, update).is_some() && !state.is_width_updated(kind) {
            return Err(format!(
                "{} 변경 대상 c:{} 요소를 찾을 수 없습니다",
                kind.width_field_name(),
                kind.axis_local_name()
            ));
        }
    }
    Ok(())
}

fn ensure_data_label_updates_applied(
    update: &ChartXmlUpdate,
    state: &DataLabelUpdateState,
) -> Result<(), String> {
    for field in data_label_requested_fields(update) {
        if !state.updated(field) {
            return Err(format!(
                "{} 변경 대상 c:dLbls 요소를 찾거나 삽입할 수 없습니다",
                data_label_field_name(field)
            ));
        }
    }
    Ok(())
}

fn ensure_data_table_updates_applied(
    update: &ChartXmlUpdate,
    state: &DataTableUpdateState,
) -> Result<(), String> {
    for field in data_table_requested_fields(update) {
        if !state.updated(field) {
            return Err(format!(
                "{} 변경 대상 c:plotArea/c:dTable 요소를 찾거나 삽입할 수 없습니다",
                data_table_field_name(field)
            ));
        }
    }
    Ok(())
}

fn ensure_trendline_updates_applied(
    update: &ChartXmlUpdate,
    state: &TrendlineUpdateState,
) -> Result<(), String> {
    if has_trendline_line_style_update(update) && !state.line_style_updated {
        return Err(
            "trendlineLineColor/trendlineLineWidth 변경 대상 c:ser/c:trendline/c:spPr 요소를 찾거나 삽입할 수 없습니다"
                .to_string(),
        );
    }
    for field in trendline_requested_fields(update) {
        if !state.updated(field) {
            return Err(format!(
                "{} 변경 대상 c:ser/c:trendline 요소를 찾거나 삽입할 수 없습니다",
                trendline_field_name(field)
            ));
        }
    }
    Ok(())
}

fn ensure_error_bar_updates_applied(
    update: &ChartXmlUpdate,
    state: &ErrorBarUpdateState,
) -> Result<(), String> {
    if has_error_bar_line_style_update(update) && !state.line_style_updated {
        return Err(
            "errorBarLineColor/errorBarLineWidth 변경 대상 c:ser/c:errBars 요소를 찾거나 삽입할 수 없습니다"
                .to_string(),
        );
    }
    for field in error_bar_requested_fields(update) {
        if !state.updated(field) {
            return Err(format!(
                "{} 변경 대상 c:ser/c:errBars 요소를 찾거나 삽입할 수 없습니다",
                error_bar_field_name(field)
            ));
        }
    }
    Ok(())
}

fn data_label_field_name(field: DataLabelField) -> &'static str {
    match field {
        DataLabelField::Position => "dataLabelPosition",
        DataLabelField::ShowValue => "dataLabelsShowValue",
        DataLabelField::ShowCategoryName => "dataLabelsShowCategoryName",
        DataLabelField::ShowSeriesName => "dataLabelsShowSeriesName",
        DataLabelField::ShowPercent => "dataLabelsShowPercent",
        DataLabelField::ShowLegendKey => "dataLabelsShowLegendKey",
    }
}

fn data_table_field_name(field: DataTableField) -> &'static str {
    match field {
        DataTableField::ShowHorizontalBorder => "dataTableShowHorizontalBorder",
        DataTableField::ShowVerticalBorder => "dataTableShowVerticalBorder",
        DataTableField::ShowOutline => "dataTableShowOutline",
        DataTableField::ShowKeys => "dataTableShowKeys",
    }
}

fn trendline_field_name(field: TrendlineField) -> &'static str {
    match field {
        TrendlineField::Type => "trendlineType",
        TrendlineField::Order => "trendlineOrder",
        TrendlineField::Period => "trendlinePeriod",
        TrendlineField::DisplayEquation => "trendlineDisplayEquation",
        TrendlineField::DisplayRSquared => "trendlineDisplayRSquared",
    }
}

fn error_bar_field_name(field: ErrorBarField) -> &'static str {
    match field {
        ErrorBarField::Direction => "errorBarDirection",
        ErrorBarField::Type => "errorBarType",
        ErrorBarField::ValueType => "errorBarValueType",
        ErrorBarField::Value => "errorBarValue",
        ErrorBarField::NoEndCap => "errorBarNoEndCap",
    }
}

fn ensure_stock_bar_updates_applied(
    update: &ChartXmlUpdate,
    state: &StockBarUpdateState,
) -> Result<(), String> {
    if update.stock_up_down_bar_gap_width.is_some() && !state.gap_width_updated {
        return Err(
            "stockUpDownBarGapWidth 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.stock_up_bar_fill_color.is_some() && !state.up_fill_updated {
        return Err(
            "stockUpBarFillColor 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.stock_down_bar_fill_color.is_some() && !state.down_fill_updated {
        return Err(
            "stockDownBarFillColor 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.stock_up_bar_line_color.is_some() && !state.up_line_color_updated {
        return Err(
            "stockUpBarLineColor 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.stock_down_bar_line_color.is_some() && !state.down_line_color_updated {
        return Err(
            "stockDownBarLineColor 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.stock_up_bar_line_width.is_some() && !state.up_line_width_updated {
        return Err(
            "stockUpBarLineWidth 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.stock_down_bar_line_width.is_some() && !state.down_line_width_updated {
        return Err(
            "stockDownBarLineWidth 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.stock_hi_low_line_color.is_some() && !state.hi_low_line_color_updated {
        return Err(
            "stockHiLowLineColor 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    if update.stock_hi_low_line_width.is_some() && !state.hi_low_line_width_updated {
        return Err(
            "stockHiLowLineWidth 변경 대상 c:stockChart 요소를 찾을 수 없습니다".to_string(),
        );
    }
    Ok(())
}

fn write_axis_sp_pr_with_line<W: Write>(
    writer: &mut Writer<W>,
    axis_source_name: &[u8],
    axis: AxisKind,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    write_sp_pr_with_line(
        writer,
        axis_source_name,
        axis_line_color_update(axis, update),
        axis_line_width_update(axis, update),
    )
}

fn series_style_update(
    local: &[u8],
    path: &[Vec<u8>],
    current_ser: Option<usize>,
    chart: &OoxmlChart,
    series_updates: &BTreeMap<usize, SeriesXmlUpdate>,
) -> Option<(usize, Option<u32>, Option<u32>, Option<u32>)> {
    if local != b"spPr" || !path.last().is_some_and(|part| part.as_slice() == b"ser") {
        return None;
    }
    let series_index = current_ser?;
    let update = series_updates.get(&series_index)?;
    if !has_series_style_update(update) {
        return None;
    }
    let existing = chart.series.get(series_index);
    Some((
        series_index,
        update
            .color
            .or_else(|| existing.and_then(|series| series.color)),
        update
            .line_color
            .or_else(|| existing.and_then(|series| series.line_color)),
        update
            .line_width
            .or_else(|| existing.and_then(|series| series.line_width)),
    ))
}

fn has_series_style_update(update: &SeriesXmlUpdate) -> bool {
    update.color.is_some() || update.line_color.is_some() || update.line_width.is_some()
}

fn write_series_sp_pr_with_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    fill_color: Option<u32>,
    line_color: Option<u32>,
    line_width: Option<u32>,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let sp_pr_name = qualified_name(chart_prefix.as_deref(), "spPr");
    writer
        .write_event(Event::Start(BytesStart::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())?;
    if let Some(color) = fill_color {
        write_solid_fill(writer, color)?;
    }
    if line_color.is_some() || line_width.is_some() {
        write_line_style(writer, line_color, line_width)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_sp_pr_with_fill_preserving_children<R: BufRead, W: Write>(
    reader: &mut Reader<R>,
    writer: &mut Writer<W>,
    start: &quick_xml::events::BytesStart,
    color: u32,
) -> Result<(), String> {
    writer
        .write_event(Event::Start(start.to_owned()))
        .map_err(|e| e.to_string())?;
    write_solid_fill(writer, color)?;

    let mut buf = Vec::new();
    let mut depth = 0usize;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if depth == 0 && is_direct_fill_child(e.name().as_ref()) {
                    skip_element(reader, e.name().as_ref())?;
                } else {
                    depth += 1;
                    writer
                        .write_event(Event::Start(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::Empty(e)) => {
                if !(depth == 0 && is_direct_fill_child(e.name().as_ref())) {
                    writer
                        .write_event(Event::Empty(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::End(e)) => {
                if depth == 0 && local_name(e.name().as_ref()) == b"spPr" {
                    writer
                        .write_event(Event::End(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                    break;
                }
                if depth > 0 {
                    depth -= 1;
                }
                writer
                    .write_event(Event::End(e.into_owned()))
                    .map_err(|e| e.to_string())?;
            }
            Ok(Event::Text(e)) => writer
                .write_event(Event::Text(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::CData(e)) => writer
                .write_event(Event::CData(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::Decl(e)) => writer
                .write_event(Event::Decl(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::PI(e)) => writer
                .write_event(Event::PI(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::Comment(e)) => writer
                .write_event(Event::Comment(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::DocType(e)) => writer
                .write_event(Event::DocType(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::GeneralRef(e)) => writer
                .write_event(Event::GeneralRef(e.into_owned()))
                .map_err(|e| e.to_string())?,
            Ok(Event::Eof) => {
                return Err("c:plotArea/c:spPr가 끝나기 전에 XML이 종료되었습니다".to_string());
            }
            Err(e) => return Err(format!("c:plotArea/c:spPr 읽기 실패: {e}")),
        }
        buf.clear();
    }

    Ok(())
}

fn is_direct_fill_child(name: &[u8]) -> bool {
    matches!(
        local_name(name),
        b"solidFill" | b"noFill" | b"gradFill" | b"pattFill"
    )
}

fn write_sp_pr_with_fill<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    color: Option<u32>,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let sp_pr_name = qualified_name(chart_prefix.as_deref(), "spPr");
    writer
        .write_event(Event::Start(BytesStart::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())?;
    if let Some(color) = color {
        write_solid_fill(writer, color)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_axis_grid_lines_with_line<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    kind: AxisGridLineKind,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let grid_line_name = qualified_name(chart_prefix.as_deref(), kind.xml_local_name());
    writer
        .write_event(Event::Start(BytesStart::new(grid_line_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_sp_pr_with_line(
        writer,
        prefix_source_name,
        axis_grid_line_color_update(kind, update),
        axis_grid_line_width_update(kind, update),
    )?;
    writer
        .write_event(Event::End(BytesEnd::new(grid_line_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_data_labels<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let data_labels_name = qualified_name(chart_prefix.as_deref(), "dLbls");
    writer
        .write_event(Event::Start(BytesStart::new(data_labels_name.as_str())))
        .map_err(|e| e.to_string())?;
    for field in data_label_requested_fields(update) {
        write_data_label_field(writer, prefix_source_name, field, update)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new(data_labels_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_missing_data_label_children<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut DataLabelUpdateState,
) -> Result<(), String> {
    for field in data_label_requested_fields(update) {
        if !state.current_updated(field) {
            write_data_label_field(writer, prefix_source_name, field, update)?;
            state.mark_updated(field);
        }
    }
    Ok(())
}

fn write_data_label_field<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    field: DataLabelField,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let Some((local, value)) = data_label_field_update_value(field, update) else {
        return Ok(());
    };
    write_chart_empty_with_val(writer, prefix_source_name, local, &value)
}

fn write_chart_data_table_with_requested_flags<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut DataTableUpdateState,
) -> Result<(), String> {
    if !has_data_table_update(update) {
        return Ok(());
    }
    let chart_prefix = element_prefix(prefix_source_name);
    let data_table_name = qualified_name(chart_prefix.as_deref(), "dTable");
    writer
        .write_event(Event::Start(BytesStart::new(data_table_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_missing_data_table_flags(writer, prefix_source_name, update, state)?;
    writer
        .write_event(Event::End(BytesEnd::new(data_table_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_missing_data_table_flags<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut DataTableUpdateState,
) -> Result<(), String> {
    for field in data_table_requested_fields(update) {
        if !state.updated(field) {
            write_data_table_field(writer, prefix_source_name, field, update)?;
            state.mark_updated(field);
        }
    }
    Ok(())
}

fn write_data_table_field<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    field: DataTableField,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let Some((local, value)) = data_table_field_update_value(field, update) else {
        return Ok(());
    };
    write_chart_empty_with_val(writer, prefix_source_name, local, value)
}

fn write_chart_trendline_with_requested_fields<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut TrendlineUpdateState,
) -> Result<(), String> {
    if !has_trendline_update(update) {
        return Ok(());
    }
    state.reset_for_trendline();
    let chart_prefix = element_prefix(prefix_source_name);
    let trendline_name = qualified_name(chart_prefix.as_deref(), "trendline");
    writer
        .write_event(Event::Start(BytesStart::new(trendline_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_missing_trendline_fields(writer, prefix_source_name, update, state)?;
    writer
        .write_event(Event::End(BytesEnd::new(trendline_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_missing_trendline_fields<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut TrendlineUpdateState,
) -> Result<(), String> {
    write_missing_trendline_line_style(writer, prefix_source_name, update, state)?;
    for field in trendline_requested_fields(update) {
        if !state.current_updated(field) {
            write_trendline_field(writer, prefix_source_name, field, update)?;
            state.mark_updated(field);
        }
    }
    Ok(())
}

fn write_missing_trendline_line_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut TrendlineUpdateState,
) -> Result<(), String> {
    if has_trendline_line_style_update(update) && !state.current_line_style_updated {
        write_sp_pr_with_line(
            writer,
            prefix_source_name,
            update.trendline_line_color,
            update.trendline_line_width,
        )?;
        state.mark_line_style_updated();
    }
    Ok(())
}

fn write_trendline_field<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    field: TrendlineField,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let Some((local, value)) = trendline_field_update_value(field, update) else {
        return Ok(());
    };
    write_chart_empty_with_val(writer, prefix_source_name, local, &value)
}

fn write_chart_error_bars_with_requested_fields<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut ErrorBarUpdateState,
) -> Result<(), String> {
    if !has_error_bar_update(update) {
        return Ok(());
    }
    state.reset_for_error_bars();
    let chart_prefix = element_prefix(prefix_source_name);
    let error_bars_name = qualified_name(chart_prefix.as_deref(), "errBars");
    writer
        .write_event(Event::Start(BytesStart::new(error_bars_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_missing_error_bar_fields(writer, prefix_source_name, update, state)?;
    writer
        .write_event(Event::End(BytesEnd::new(error_bars_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_missing_error_bar_fields<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut ErrorBarUpdateState,
) -> Result<(), String> {
    write_missing_error_bar_line_style(writer, prefix_source_name, update, state)?;
    for field in error_bar_requested_fields(update) {
        if !state.current_updated(field) {
            write_error_bar_field(writer, prefix_source_name, field, update)?;
            state.mark_updated(field);
        }
    }
    Ok(())
}

fn write_missing_error_bar_line_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    state: &mut ErrorBarUpdateState,
) -> Result<(), String> {
    if has_error_bar_line_style_update(update) && !state.current_line_style_updated {
        write_sp_pr_with_line(
            writer,
            prefix_source_name,
            update.error_bar_line_color,
            update.error_bar_line_width,
        )?;
        state.mark_line_style_updated();
    }
    Ok(())
}

fn write_error_bar_field<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    field: ErrorBarField,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let Some((local, value)) = error_bar_field_update_value(field, update) else {
        return Ok(());
    };
    write_chart_empty_with_val(writer, prefix_source_name, local, &value)
}

fn write_chart_empty_with_val<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    local: &str,
    value: &str,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let name = qualified_name(chart_prefix.as_deref(), local);
    let mut start = BytesStart::new(name.as_str());
    start.push_attribute(("val", value));
    writer
        .write_event(Event::Empty(start))
        .map_err(|e| e.to_string())
}

fn write_stock_up_down_bars_with_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    chart: &OoxmlChart,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let up_down_name = qualified_name(chart_prefix.as_deref(), "upDownBars");
    writer
        .write_event(Event::Start(BytesStart::new(up_down_name.as_str())))
        .map_err(|e| e.to_string())?;

    if let Some(gap_width) = update
        .stock_up_down_bar_gap_width
        .or(chart.stock_up_down_bar_gap_width)
    {
        let gap_name = qualified_name(chart_prefix.as_deref(), "gapWidth");
        let mut gap = BytesStart::new(gap_name.as_str());
        let gap_width = gap_width.to_string();
        gap.push_attribute(("val", gap_width.as_str()));
        writer
            .write_event(Event::Empty(gap))
            .map_err(|e| e.to_string())?;
    }

    if has_stock_bar_effective_style(chart, update, StockBarKind::Up) {
        write_stock_bar_with_style(writer, prefix_source_name, chart, update, StockBarKind::Up)?;
    }
    if has_stock_bar_effective_style(chart, update, StockBarKind::Down) {
        write_stock_bar_with_style(
            writer,
            prefix_source_name,
            chart,
            update,
            StockBarKind::Down,
        )?;
    }

    writer
        .write_event(Event::End(BytesEnd::new(up_down_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_stock_hi_low_lines_with_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    chart: &OoxmlChart,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let hi_low_name = qualified_name(chart_prefix.as_deref(), "hiLowLines");
    writer
        .write_event(Event::Start(BytesStart::new(hi_low_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_sp_pr_with_line(
        writer,
        prefix_source_name,
        update
            .stock_hi_low_line_color
            .or(chart.stock_hi_low_line_color),
        update
            .stock_hi_low_line_width
            .or(chart.stock_hi_low_line_width),
    )?;
    writer
        .write_event(Event::End(BytesEnd::new(hi_low_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_of_pie_ser_lines_with_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    chart: &OoxmlChart,
    update: &ChartXmlUpdate,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let ser_lines_name = qualified_name(chart_prefix.as_deref(), "serLines");
    writer
        .write_event(Event::Start(BytesStart::new(ser_lines_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_sp_pr_with_line(
        writer,
        prefix_source_name,
        update
            .pie_of_pie_ser_line_color
            .or(chart.pie_of_pie_ser_line_color),
        update
            .pie_of_pie_ser_line_width
            .or(chart.pie_of_pie_ser_line_width),
    )?;
    writer
        .write_event(Event::End(BytesEnd::new(ser_lines_name.as_str())))
        .map_err(|e| e.to_string())
}

fn has_stock_bar_effective_style(
    chart: &OoxmlChart,
    update: &ChartXmlUpdate,
    kind: StockBarKind,
) -> bool {
    stock_bar_fill_color(chart, update, kind).is_some()
        || stock_bar_line_color(chart, update, kind).is_some()
        || stock_bar_line_width(chart, update, kind).is_some()
}

fn write_stock_bar_with_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    chart: &OoxmlChart,
    update: &ChartXmlUpdate,
    kind: StockBarKind,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let bar_name = qualified_name(chart_prefix.as_deref(), stock_bar_local_name(kind));
    writer
        .write_event(Event::Start(BytesStart::new(bar_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_stock_bar_sp_pr(
        writer,
        prefix_source_name,
        stock_bar_fill_color(chart, update, kind),
        stock_bar_line_color(chart, update, kind),
        stock_bar_line_width(chart, update, kind),
    )?;
    writer
        .write_event(Event::End(BytesEnd::new(bar_name.as_str())))
        .map_err(|e| e.to_string())
}

fn stock_bar_local_name(kind: StockBarKind) -> &'static str {
    match kind {
        StockBarKind::Up => "upBars",
        StockBarKind::Down => "downBars",
    }
}

fn stock_bar_fill_color(
    chart: &OoxmlChart,
    update: &ChartXmlUpdate,
    kind: StockBarKind,
) -> Option<u32> {
    match kind {
        StockBarKind::Up => update
            .stock_up_bar_fill_color
            .or(chart.stock_up_bar_fill_color),
        StockBarKind::Down => update
            .stock_down_bar_fill_color
            .or(chart.stock_down_bar_fill_color),
    }
}

fn stock_bar_line_color(
    chart: &OoxmlChart,
    update: &ChartXmlUpdate,
    kind: StockBarKind,
) -> Option<u32> {
    match kind {
        StockBarKind::Up => update
            .stock_up_bar_line_color
            .or(chart.stock_up_bar_line_color),
        StockBarKind::Down => update
            .stock_down_bar_line_color
            .or(chart.stock_down_bar_line_color),
    }
}

fn stock_bar_line_width(
    chart: &OoxmlChart,
    update: &ChartXmlUpdate,
    kind: StockBarKind,
) -> Option<u32> {
    match kind {
        StockBarKind::Up => update
            .stock_up_bar_line_width
            .or(chart.stock_up_bar_line_width),
        StockBarKind::Down => update
            .stock_down_bar_line_width
            .or(chart.stock_down_bar_line_width),
    }
}

fn write_stock_bar_sp_pr<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    fill_color: Option<u32>,
    line_color: Option<u32>,
    line_width: Option<u32>,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let sp_pr_name = qualified_name(chart_prefix.as_deref(), "spPr");
    writer
        .write_event(Event::Start(BytesStart::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())?;

    if let Some(color) = fill_color {
        write_solid_fill(writer, color)?;
    }

    write_line_style(writer, line_color, line_width)?;

    writer
        .write_event(Event::End(BytesEnd::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_marker_sp_pr_with_style<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    update: &ChartXmlUpdate,
    family: MarkerFamily,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let sp_pr_name = qualified_name(chart_prefix.as_deref(), "spPr");
    writer
        .write_event(Event::Start(BytesStart::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())?;

    if let Some(color) = marker_fill_color_update(update, family) {
        write_solid_fill(writer, color)?;
    }
    write_line_style(
        writer,
        marker_line_color_update(update, family),
        marker_line_width_update(update, family),
    )?;

    writer
        .write_event(Event::End(BytesEnd::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())
}

fn write_line_style<W: Write>(
    writer: &mut Writer<W>,
    line_color: Option<u32>,
    line_width: Option<u32>,
) -> Result<(), String> {
    if line_color.is_none() && line_width.is_none() {
        return Ok(());
    }
    let mut line = BytesStart::new("a:ln");
    if let Some(width) = line_width {
        let width = width.to_string();
        line.push_attribute(("w", width.as_str()));
    }
    writer
        .write_event(Event::Start(line))
        .map_err(|e| e.to_string())?;
    if let Some(color) = line_color {
        write_solid_fill(writer, color)?;
    }
    writer
        .write_event(Event::End(BytesEnd::new("a:ln")))
        .map_err(|e| e.to_string())
}

fn write_solid_fill<W: Write>(writer: &mut Writer<W>, color: u32) -> Result<(), String> {
    writer
        .write_event(Event::Start(BytesStart::new("a:solidFill")))
        .map_err(|e| e.to_string())?;
    let mut rgb = BytesStart::new("a:srgbClr");
    let color = format!("{:06X}", color & 0x00FF_FFFF);
    rgb.push_attribute(("val", color.as_str()));
    writer
        .write_event(Event::Empty(rgb))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesEnd::new("a:solidFill")))
        .map_err(|e| e.to_string())
}

fn write_sp_pr_with_line<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    color: Option<u32>,
    width: Option<u32>,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let sp_pr_name = qualified_name(chart_prefix.as_deref(), "spPr");
    writer
        .write_event(Event::Start(BytesStart::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())?;

    let mut line = BytesStart::new("a:ln");
    if let Some(width) = width {
        let width = width.to_string();
        line.push_attribute(("w", width.as_str()));
        writer
            .write_event(Event::Start(line))
            .map_err(|e| e.to_string())?;
    } else {
        writer
            .write_event(Event::Start(line))
            .map_err(|e| e.to_string())?;
    }

    if let Some(color) = color {
        write_solid_fill(writer, color)?;
    }

    writer
        .write_event(Event::End(BytesEnd::new("a:ln")))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesEnd::new(sp_pr_name.as_str())))
        .map_err(|e| e.to_string())
}

fn skip_element<R: BufRead>(reader: &mut Reader<R>, target_name: &[u8]) -> Result<(), String> {
    let target = local_name(target_name).to_vec();
    let mut buf = Vec::new();
    let mut depth = 0usize;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(e)) => {
                if depth == 0 && local_name(e.name().as_ref()) == target.as_slice() {
                    return Ok(());
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => {
                return Err("OOXML 요소가 끝나기 전에 XML이 종료되었습니다".to_string());
            }
            Err(e) => return Err(format!("OOXML 요소 건너뛰기 실패: {e}")),
            _ => {}
        }
        buf.clear();
    }
}

fn write_axis_number_empty<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    local: &str,
    value: f64,
) -> Result<(), String> {
    let prefix = element_prefix(prefix_source_name);
    let name = qualified_name(prefix.as_deref(), local);
    let mut start = BytesStart::new(name.as_str());
    let formatted = format_chart_number(value);
    start.push_attribute(("val", formatted.as_str()));
    writer
        .write_event(Event::Empty(start))
        .map_err(|e| e.to_string())
}

fn write_axis_num_fmt_empty<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    format_code: &str,
    source_linked: Option<bool>,
) -> Result<(), String> {
    let prefix = element_prefix(prefix_source_name);
    let name = qualified_name(prefix.as_deref(), "numFmt");
    let mut start = BytesStart::new(name.as_str());
    start.push_attribute(("formatCode", format_code));
    if let Some(source_linked) = source_linked {
        start.push_attribute(("sourceLinked", if source_linked { "1" } else { "0" }));
    }
    writer
        .write_event(Event::Empty(start))
        .map_err(|e| e.to_string())
}

fn start_with_replaced_attr(
    start: &quick_xml::events::BytesStart,
    attr_name: &[u8],
    value: &str,
) -> Result<BytesStart<'static>, String> {
    let mut edited = start.to_owned();
    edited.clear_attributes();
    for attr in start.attributes().flatten() {
        if local_name(attr.key.as_ref()) != attr_name {
            edited.push_attribute((attr.key.as_ref(), attr.value.as_ref()));
        }
    }
    let attr_name = std::str::from_utf8(attr_name)
        .map_err(|_| "OOXML attribute 이름이 UTF-8이 아닙니다".to_string())?;
    edited.push_attribute((attr_name, value));
    Ok(edited)
}

fn start_with_replaced_axis_num_fmt_attrs(
    start: &quick_xml::events::BytesStart,
    axis: AxisKind,
    update: &ChartXmlUpdate,
) -> Result<BytesStart<'static>, String> {
    let mut edited = start.to_owned();
    let (format, source_linked) = axis_number_format_update(axis, update);
    edited.clear_attributes();
    for attr in start.attributes().flatten() {
        let local = local_name(attr.key.as_ref());
        if local == b"formatCode" && format.is_some() {
            continue;
        }
        if local == b"sourceLinked" && source_linked.is_some() {
            continue;
        }
        edited.push_attribute((attr.key.as_ref(), attr.value.as_ref()));
    }
    if let Some(format) = format {
        edited.push_attribute(("formatCode", format));
    }
    if let Some(source_linked) = source_linked {
        edited.push_attribute(("sourceLinked", if source_linked { "1" } else { "0" }));
    }
    Ok(edited)
}

fn should_rebuild_category_cache(local: &[u8], path: &[Vec<u8>], update: &ChartXmlUpdate) -> bool {
    local == b"strCache" && update.categories.is_some() && path_contains(path, b"cat")
}

fn values_for_rebuild<'a>(
    local: &[u8],
    path: &[Vec<u8>],
    current_ser: Option<usize>,
    series_updates: &'a BTreeMap<usize, SeriesXmlUpdate>,
) -> Option<&'a Vec<f64>> {
    if local != b"numCache" || !path_contains(path, b"val") {
        return None;
    }
    let ser_idx = current_ser?;
    series_updates
        .get(&ser_idx)
        .and_then(|series| series.values.as_ref())
}

fn write_chart_title_with_text<R: BufRead, W: Write>(
    reader: &mut Reader<R>,
    writer: &mut Writer<W>,
    start: &quick_xml::events::BytesStart,
    title: &str,
    title_overlay: Option<bool>,
) -> Result<bool, String> {
    let title_name = start.name().as_ref().to_vec();
    let chart_prefix = element_prefix(&title_name);
    writer
        .write_event(Event::Start(start.to_owned()))
        .map_err(|e| e.to_string())?;
    write_title_text_block(writer, chart_prefix.as_deref(), title)?;

    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut skip_depth: Option<usize> = None;
    let mut overlay_updated = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if let Some(skip) = skip_depth.as_mut() {
                    *skip += 1;
                } else if depth == 0 && local_name(e.name().as_ref()) == b"tx" {
                    skip_depth = Some(1);
                } else if depth == 0
                    && local_name(e.name().as_ref()) == b"overlay"
                    && title_overlay.is_some()
                {
                    let edited = start_with_replaced_attr(
                        &e,
                        b"val",
                        bool_xml_value(title_overlay.unwrap()),
                    )?;
                    writer
                        .write_event(Event::Start(edited))
                        .map_err(|e| e.to_string())?;
                    overlay_updated = true;
                    depth += 1;
                } else {
                    writer
                        .write_event(Event::Start(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                    depth += 1;
                }
            }
            Ok(Event::Empty(e)) => {
                if skip_depth.is_none() && !(depth == 0 && local_name(e.name().as_ref()) == b"tx") {
                    if depth == 0 && local_name(e.name().as_ref()) == b"overlay" {
                        let Some(title_overlay) = title_overlay else {
                            writer
                                .write_event(Event::Empty(e.into_owned()))
                                .map_err(|e| e.to_string())?;
                            continue;
                        };
                        let edited =
                            start_with_replaced_attr(&e, b"val", bool_xml_value(title_overlay))?;
                        writer
                            .write_event(Event::Empty(edited))
                            .map_err(|e| e.to_string())?;
                        overlay_updated = true;
                    } else {
                        writer
                            .write_event(Event::Empty(e.into_owned()))
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::Text(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::CData(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::CData(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::End(e)) => {
                if let Some(skip) = skip_depth.as_mut() {
                    *skip -= 1;
                    if *skip == 0 {
                        skip_depth = None;
                    }
                } else if depth == 0 && local_name(e.name().as_ref()) == local_name(&title_name) {
                    if let Some(value) = title_overlay {
                        if !overlay_updated {
                            write_chart_empty_with_val(
                                writer,
                                &title_name,
                                "overlay",
                                bool_xml_value(value),
                            )?;
                            overlay_updated = true;
                        }
                    }
                    writer
                        .write_event(Event::End(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                    break;
                } else {
                    writer
                        .write_event(Event::End(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                    depth = depth.saturating_sub(1);
                }
            }
            Ok(Event::Decl(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::Decl(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::PI(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::PI(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::Comment(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::Comment(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::DocType(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::DocType(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::GeneralRef(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::Eof) => {
                return Err("OOXML chart title이 끝나기 전에 XML이 종료되었습니다".to_string());
            }
            Err(e) => return Err(format!("OOXML chart title 읽기 실패: {e}")),
        }
        buf.clear();
    }

    Ok(overlay_updated)
}

fn write_title_text_block<W: Write>(
    writer: &mut Writer<W>,
    chart_prefix: Option<&str>,
    title: &str,
) -> Result<(), String> {
    let tx_name = qualified_name(chart_prefix, "tx");
    let rich_name = qualified_name(chart_prefix, "rich");
    writer
        .write_event(Event::Start(BytesStart::new(tx_name.as_str())))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::Start(BytesStart::new(rich_name.as_str())))
        .map_err(|e| e.to_string())?;

    write_empty_element(writer, "a:bodyPr")?;
    write_empty_element(writer, "a:lstStyle")?;
    write_start_element(writer, "a:p")?;
    write_start_element(writer, "a:r")?;
    write_start_element(writer, "a:t")?;
    writer
        .write_event(Event::Text(BytesText::new(title)))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesEnd::new("a:t")))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesEnd::new("a:r")))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesEnd::new("a:p")))
        .map_err(|e| e.to_string())?;

    writer
        .write_event(Event::End(BytesEnd::new(rich_name.as_str())))
        .map_err(|e| e.to_string())?;
    writer
        .write_event(Event::End(BytesEnd::new(tx_name.as_str())))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn write_axis_title_with_text<W: Write>(
    writer: &mut Writer<W>,
    prefix_source_name: &[u8],
    title: &str,
) -> Result<(), String> {
    let chart_prefix = element_prefix(prefix_source_name);
    let title_name = qualified_name(chart_prefix.as_deref(), "title");
    writer
        .write_event(Event::Start(BytesStart::new(title_name.as_str())))
        .map_err(|e| e.to_string())?;
    write_title_text_block(writer, chart_prefix.as_deref(), title)?;
    writer
        .write_event(Event::End(BytesEnd::new(title_name.as_str())))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn write_start_element<W: Write>(writer: &mut Writer<W>, name: &str) -> Result<(), String> {
    writer
        .write_event(Event::Start(BytesStart::new(name)))
        .map_err(|e| e.to_string())
}

fn write_empty_element<W: Write>(writer: &mut Writer<W>, name: &str) -> Result<(), String> {
    writer
        .write_event(Event::Empty(BytesStart::new(name)))
        .map_err(|e| e.to_string())
}

fn write_rebuilt_cache<R: BufRead, W: Write>(
    reader: &mut Reader<R>,
    writer: &mut Writer<W>,
    start: &quick_xml::events::BytesStart,
    kind: CacheKind,
    values: &[String],
) -> Result<(), String> {
    let cache_name = start.name().as_ref().to_vec();
    let prefix = element_prefix(&cache_name);
    writer
        .write_event(Event::Start(start.to_owned()))
        .map_err(|e| e.to_string())?;

    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut skip_depth: Option<usize> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if let Some(skip) = skip_depth.as_mut() {
                    *skip += 1;
                } else {
                    let name = e.name();
                    let local = local_name(name.as_ref());
                    if depth == 0 && matches!(local, b"pt" | b"ptCount") {
                        skip_depth = Some(1);
                    } else {
                        writer
                            .write_event(Event::Start(e.into_owned()))
                            .map_err(|e| e.to_string())?;
                        depth += 1;
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                if skip_depth.is_none() {
                    let name = e.name();
                    let local = local_name(name.as_ref());
                    if !(depth == 0 && matches!(local, b"pt" | b"ptCount")) {
                        writer
                            .write_event(Event::Empty(e.into_owned()))
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            Ok(Event::Text(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::Text(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::CData(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::CData(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::End(e)) => {
                if let Some(skip) = skip_depth.as_mut() {
                    *skip -= 1;
                    if *skip == 0 {
                        skip_depth = None;
                    }
                } else if depth == 0 && local_name(e.name().as_ref()) == local_name(&cache_name) {
                    write_cache_points(writer, prefix.as_deref(), kind, values)?;
                    writer
                        .write_event(Event::End(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                    break;
                } else {
                    writer
                        .write_event(Event::End(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                    depth = depth.saturating_sub(1);
                }
            }
            Ok(Event::Decl(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::Decl(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::PI(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::PI(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::Comment(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::Comment(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::DocType(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::DocType(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if skip_depth.is_none() {
                    writer
                        .write_event(Event::GeneralRef(e.into_owned()))
                        .map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::Eof) => {
                return Err("OOXML chart cache가 끝나기 전에 XML이 종료되었습니다".to_string());
            }
            Err(e) => return Err(format!("OOXML chart cache 읽기 실패: {e}")),
        }
        buf.clear();
    }

    Ok(())
}

fn write_cache_points<W: Write>(
    writer: &mut Writer<W>,
    prefix: Option<&str>,
    _kind: CacheKind,
    values: &[String],
) -> Result<(), String> {
    let pt_count_name = qualified_name(prefix, "ptCount");
    let mut pt_count = BytesStart::new(pt_count_name.as_str());
    let count = values.len().to_string();
    pt_count.push_attribute(("val", count.as_str()));
    writer
        .write_event(Event::Empty(pt_count))
        .map_err(|e| e.to_string())?;

    let pt_name = qualified_name(prefix, "pt");
    let v_name = qualified_name(prefix, "v");
    for (idx, value) in values.iter().enumerate() {
        let mut pt = BytesStart::new(pt_name.as_str());
        let idx_attr = idx.to_string();
        pt.push_attribute(("idx", idx_attr.as_str()));
        writer
            .write_event(Event::Start(pt))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Start(BytesStart::new(v_name.as_str())))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::Text(BytesText::new(value)))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesEnd::new(v_name.as_str())))
            .map_err(|e| e.to_string())?;
        writer
            .write_event(Event::End(BytesEnd::new(pt_name.as_str())))
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn replacement_text(
    path: &[Vec<u8>],
    current_ser: Option<usize>,
    current_pt_idx: Option<usize>,
    update: &ChartXmlUpdate,
    series_updates: &BTreeMap<usize, SeriesXmlUpdate>,
) -> Option<String> {
    if !path.last().is_some_and(|name| name.as_slice() == b"v") {
        return None;
    }
    let ser_idx = current_ser?;
    let pt_idx = current_pt_idx?;

    if path_contains(path, b"tx") && path_contains(path, b"strCache") {
        if pt_idx == 0 {
            return series_updates
                .get(&ser_idx)
                .and_then(|series| series.name.clone());
        }
    }

    if path_contains(path, b"cat") && path_contains(path, b"strCache") {
        return update
            .categories
            .as_ref()
            .and_then(|categories| categories.get(pt_idx).cloned());
    }

    if path_contains(path, b"val") && path_contains(path, b"numCache") {
        return series_updates
            .get(&ser_idx)
            .and_then(|series| series.values.as_ref())
            .and_then(|values| values.get(pt_idx))
            .map(|value| format_chart_number(*value));
    }

    None
}

fn pt_index(e: &quick_xml::events::BytesStart) -> Option<usize> {
    for attr in e.attributes().flatten() {
        if local_name(attr.key.as_ref()) == b"idx" {
            let value = std::str::from_utf8(attr.value.as_ref()).ok()?;
            return value.parse().ok();
        }
    }
    None
}

fn path_contains(path: &[Vec<u8>], name: &[u8]) -> bool {
    path.iter().any(|part| part.as_slice() == name)
}

fn path_contains_pie_plot(path: &[Vec<u8>]) -> bool {
    path.iter().any(|part| {
        matches!(
            part.as_slice(),
            b"pieChart" | b"pie3DChart" | b"doughnutChart" | b"ofPieChart"
        )
    })
}

fn is_chart_plot(local: &[u8]) -> bool {
    matches!(
        local,
        b"barChart"
            | b"bar3DChart"
            | b"lineChart"
            | b"pieChart"
            | b"pie3DChart"
            | b"doughnutChart"
            | b"ofPieChart"
            | b"scatterChart"
            | b"stockChart"
    )
}

fn local_name(name: &[u8]) -> &[u8] {
    name.iter()
        .rposition(|byte| *byte == b':')
        .map(|idx| &name[idx + 1..])
        .unwrap_or(name)
}

fn element_prefix(name: &[u8]) -> Option<String> {
    let idx = name.iter().position(|byte| *byte == b':')?;
    std::str::from_utf8(&name[..idx])
        .ok()
        .map(|s| s.to_string())
}

fn qualified_name(prefix: Option<&str>, local: &str) -> String {
    match prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}:{local}"),
        _ => local.to_string(),
    }
}

fn format_chart_number(value: f64) -> String {
    if value.fract().abs() < 1e-9 {
        return format!("{}", value as i64);
    }
    let mut s = format!("{value:.12}");
    while s.contains('.') && s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_existing_series_values_and_labels() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt><c:pt idx="1"><c:v>C2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: Some(vec!["Q1".to_string(), "Q2".to_string()]),
                series: vec![SeriesXmlUpdate {
                    index: 0,
                    name: Some("Edited".to_string()),
                    values: Some(vec![7.0, 8.25]),
                    color: None,
                    line_color: None,
                    line_width: None,
                }],
                ..Default::default()
            },
        )
        .expect("update chart XML");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("<c:v>Edited</c:v>"), "{text}");
        assert!(text.contains("<c:v>Q1</c:v>"), "{text}");
        assert!(text.contains("<c:v>8.25</c:v>"), "{text}");
        assert!(text.contains(r#"<c:ptCount val="2"/>"#), "{text}");
    }

    #[test]
    fn inserts_series_fill_color_when_missing_sp_pr() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:ser><c:idx val="0"/><c:order val="0"/><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                series: vec![SeriesXmlUpdate {
                    index: 0,
                    name: None,
                    values: None,
                    color: Some(0x336699),
                    line_color: None,
                    line_width: None,
                }],
                ..Default::default()
            },
        )
        .expect("update chart XML");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:spPr><a:solidFill><a:srgbClr val="336699"/></a:solidFill></c:spPr>"#
            ),
            "{text}"
        );
        assert!(text.contains("<c:v>1</c:v>"), "{text}");
    }

    #[test]
    fn updates_and_inserts_series_line_style() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:lineChart><c:ser><c:idx val="0"/><c:spPr><a:solidFill><a:srgbClr val="112233"/></a:solidFill><a:ln w="12700"><a:solidFill><a:srgbClr val="445566"/></a:solidFill></a:ln></c:spPr><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:idx val="1"/><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                series: vec![
                    SeriesXmlUpdate {
                        index: 0,
                        line_color: Some(0xABCDEF),
                        line_width: Some(25400),
                        ..Default::default()
                    },
                    SeriesXmlUpdate {
                        index: 1,
                        line_color: Some(0x123456),
                        line_width: Some(33333),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        )
        .expect("update series line style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:spPr><a:solidFill><a:srgbClr val="112233"/></a:solidFill><a:ln w="25400"><a:solidFill><a:srgbClr val="ABCDEF"/></a:solidFill></a:ln></c:spPr>"#),
            "{text}"
        );
        assert!(
            text.contains(r#"<c:spPr><a:ln w="33333"><a:solidFill><a:srgbClr val="123456"/></a:solidFill></a:ln></c:spPr>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse updated chart");
        assert_eq!(parsed.series[0].color, Some(0x112233));
        assert_eq!(parsed.series[0].line_color, Some(0xABCDEF));
        assert_eq!(parsed.series[0].line_width, Some(25400));
        assert_eq!(parsed.series[1].line_color, Some(0x123456));
        assert_eq!(parsed.series[1].line_width, Some(33333));
    }

    #[test]
    fn rebuilds_cache_points_when_lengths_change() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:ptCount val="2"/><c:pt idx="0"><c:v>C1</c:v></c:pt><c:pt idx="1"><c:v>C2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="2"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: Some(vec!["Q1".to_string(), "Q2".to_string(), "Q3".to_string()]),
                series: vec![SeriesXmlUpdate {
                    index: 0,
                    name: None,
                    values: Some(vec![7.0, 8.25, 9.5]),
                    color: None,
                    line_color: None,
                    line_width: None,
                }],
                ..Default::default()
            },
        )
        .expect("update chart XML");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:formatCode>General</c:formatCode>"#),
            "{text}"
        );
        assert_eq!(text.matches(r#"<c:ptCount val="3"/>"#).count(), 2, "{text}");
        assert!(
            text.contains(r#"<c:pt idx="2"><c:v>Q3</c:v></c:pt>"#),
            "{text}"
        );
        assert!(
            text.contains(r#"<c:pt idx="2"><c:v>9.5</c:v></c:pt>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.categories, vec!["Q1", "Q2", "Q3"]);
        assert_eq!(parsed.series[0].values, vec![7.0, 8.25, 9.5]);
    }

    #[test]
    fn inserts_chart_title_text_while_preserving_title_properties() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:title><c:layout/><c:overlay val="0"/><c:txPr><a:p/></c:txPr></c:title><c:plotArea><c:barChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: Some("MCP 차트 제목".to_string()),
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update chart title");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("<a:t>MCP 차트 제목</a:t>"), "{text}");
        assert!(text.contains("<c:title><c:tx>"), "{text}");
        assert!(text.contains(r#"<c:overlay val="0"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.title.as_deref(), Some("MCP 차트 제목"));
        assert_eq!(parsed.series[0].name, "A");
    }

    #[test]
    fn changes_bar_chart_direction_between_column_and_bar() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: Some(OoxmlChartType::Bar),
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update chart type");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:barDir val="bar"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Bar);
        assert_eq!(parsed.series[0].values, vec![1.0]);
    }

    #[test]
    fn changes_bar_grouping() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: Some(BarGrouping::Stacked),
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update chart grouping");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:grouping val="stacked"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.grouping, BarGrouping::Stacked);
        assert_eq!(parsed.chart_type, OoxmlChartType::Column);
    }

    #[test]
    fn changes_line_grouping() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:lineChart><c:grouping val="standard"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt><c:pt idx="1"><c:v>C2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                grouping: Some(BarGrouping::PercentStacked),
                ..Default::default()
            },
        )
        .expect("update line chart grouping");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:grouping val="percentStacked"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.grouping, BarGrouping::PercentStacked);
        assert_eq!(parsed.chart_type, OoxmlChartType::Line);
    }

    #[test]
    fn inserts_bar_grouping_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                grouping: Some(BarGrouping::Stacked),
                ..Default::default()
            },
        )
        .expect("insert bar grouping");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:barDir val="col"/><c:grouping val="stacked"/><c:ser>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Column);
        assert_eq!(parsed.grouping, BarGrouping::Stacked);
    }

    #[test]
    fn inserts_line_grouping_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt><c:pt idx="1"><c:v>C2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                grouping: Some(BarGrouping::PercentStacked),
                ..Default::default()
            },
        )
        .expect("insert line grouping");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:lineChart><c:grouping val="percentStacked"/><c:ser>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Line);
        assert_eq!(parsed.grouping, BarGrouping::PercentStacked);
    }

    #[test]
    fn changes_bar_gap_width_and_overlap() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:gapWidth val="150"/><c:overlap val="0"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: Some(90),
                bar_overlap: Some(-35),
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update bar gap/overlap");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:gapWidth val="90"/>"#), "{text}");
        assert!(text.contains(r#"<c:overlap val="-35"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.bar_gap_width, Some(90));
        assert_eq!(parsed.bar_overlap, Some(-35));
    }

    #[test]
    fn inserts_bar_gap_width_and_overlap_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                bar_gap_width: Some(90),
                bar_overlap: Some(-35),
                ..Default::default()
            },
        )
        .expect("insert bar gap/overlap");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:gapWidth val="90"/><c:overlap val="-35"/><c:axId val="1"/>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.bar_gap_width, Some(90));
        assert_eq!(parsed.bar_overlap, Some(-35));
    }

    #[test]
    fn changes_line_marker_size_and_smooth() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:marker><c:size val="7"/></c:marker><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val><c:smooth val="0"/></c:ser><c:marker val="1"/><c:smooth val="0"/></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: Some(true),
                line_marker_visible: Some(false),
                line_marker_size: Some(11),
                line_marker_symbol: Some(ChartMarkerSymbol::Diamond),
                line_marker_fill_color: Some(0xF4B183),
                line_marker_line_color: Some(0x5B9BD5),
                line_marker_line_width: Some(12700),
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update line style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:symbol val="diamond"/>"#), "{text}");
        assert!(text.contains(r#"<c:size val="11"/>"#), "{text}");
        assert!(text.contains(r#"<a:srgbClr val="F4B183"/>"#), "{text}");
        assert!(
            text.contains(
                r#"<a:ln w="12700"><a:solidFill><a:srgbClr val="5B9BD5"/></a:solidFill></a:ln>"#
            ),
            "{text}"
        );
        assert!(text.contains(r#"<c:marker val="0"/>"#), "{text}");
        assert_eq!(text.matches(r#"<c:smooth val="1"/>"#).count(), 2, "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Line);
        assert_eq!(parsed.line_smooth, Some(true));
        assert_eq!(parsed.line_marker_visible, Some(false));
        assert_eq!(parsed.line_marker_size, Some(11));
        assert_eq!(parsed.line_marker_symbol, Some(ChartMarkerSymbol::Diamond));
        assert_eq!(parsed.line_marker_fill_color, Some(0xF4B183));
        assert_eq!(parsed.line_marker_line_color, Some(0x5B9BD5));
        assert_eq!(parsed.line_marker_line_width, Some(12700));
    }

    #[test]
    fn inserts_line_marker_size_and_smooth_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>B</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                line_smooth: Some(true),
                line_marker_visible: Some(false),
                line_marker_size: Some(11),
                ..Default::default()
            },
        )
        .expect("insert line style fields");
        let text = String::from_utf8(out).expect("utf8");
        assert_eq!(
            text.matches(r#"<c:marker><c:size val="11"/></c:marker>"#)
                .count(),
            2,
            "{text}"
        );
        assert_eq!(text.matches(r#"<c:smooth val="1"/>"#).count(), 3, "{text}");
        assert!(
            text.contains(r#"<c:marker val="0"/><c:smooth val="1"/><c:axId val="1"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Line);
        assert_eq!(parsed.line_smooth, Some(true));
        assert_eq!(parsed.line_marker_visible, Some(false));
        assert_eq!(parsed.line_marker_size, Some(11));
    }

    #[test]
    fn changes_chart_trendline_fields() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:trendline><c:spPr><a:ln w="11111"><a:solidFill><a:srgbClr val="111111"/></a:solidFill></a:ln></c:spPr><c:trendlineType val="exp"/><c:order val="2"/><c:period val="4"/><c:dispEq val="0"/><c:dispRSqr val="1"/></c:trendline><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                trendline_type: Some(ChartTrendlineType::Polynomial),
                trendline_order: Some(3),
                trendline_period: Some(5),
                trendline_display_equation: Some(true),
                trendline_display_r_squared: Some(false),
                trendline_line_color: Some(0x00AA55),
                trendline_line_width: Some(22225),
                ..Default::default()
            },
        )
        .expect("update trendline fields");
        let text = String::from_utf8(out).expect("utf8");
        assert_eq!(text.matches(r#"<c:trendline>"#).count(), 1, "{text}");
        assert!(
            text.contains(r#"<c:spPr><a:ln w="22225"><a:solidFill><a:srgbClr val="00AA55"/></a:solidFill></a:ln></c:spPr>"#),
            "{text}"
        );
        assert!(text.contains(r#"<c:trendlineType val="poly"/>"#), "{text}");
        assert!(text.contains(r#"<c:order val="3"/>"#), "{text}");
        assert!(text.contains(r#"<c:period val="5"/>"#), "{text}");
        assert!(text.contains(r#"<c:dispEq val="1"/>"#), "{text}");
        assert!(text.contains(r#"<c:dispRSqr val="0"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Line);
        assert_eq!(parsed.trendline_type, Some(ChartTrendlineType::Polynomial));
        assert_eq!(parsed.trendline_order, Some(3));
        assert_eq!(parsed.trendline_period, Some(5));
        assert_eq!(parsed.trendline_display_equation, Some(true));
        assert_eq!(parsed.trendline_display_r_squared, Some(false));
        assert_eq!(parsed.trendline_line_color, Some(0x00AA55));
        assert_eq!(parsed.trendline_line_width, Some(22225));
    }

    #[test]
    fn inserts_chart_trendline_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>B</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                trendline_type: Some(ChartTrendlineType::MovingAverage),
                trendline_period: Some(6),
                trendline_display_equation: Some(true),
                trendline_display_r_squared: Some(true),
                trendline_line_color: Some(0x00AA55),
                trendline_line_width: Some(22225),
                ..Default::default()
            },
        )
        .expect("insert trendline fields");
        let text = String::from_utf8(out).expect("utf8");
        assert_eq!(text.matches(r#"<c:trendline>"#).count(), 2, "{text}");
        assert_eq!(text.matches(r#"<a:ln w="22225">"#).count(), 2, "{text}");
        assert_eq!(
            text.matches(r#"<a:srgbClr val="00AA55"/>"#).count(),
            2,
            "{text}"
        );
        assert_eq!(
            text.matches(r#"<c:trendlineType val="movingAvg"/>"#)
                .count(),
            2,
            "{text}"
        );
        assert_eq!(text.matches(r#"<c:period val="6"/>"#).count(), 2, "{text}");
        assert_eq!(text.matches(r#"<c:dispEq val="1"/>"#).count(), 2, "{text}");
        assert_eq!(
            text.matches(r#"<c:dispRSqr val="1"/>"#).count(),
            2,
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.trendline_type,
            Some(ChartTrendlineType::MovingAverage)
        );
        assert_eq!(parsed.trendline_period, Some(6));
        assert_eq!(parsed.trendline_display_equation, Some(true));
        assert_eq!(parsed.trendline_display_r_squared, Some(true));
        assert_eq!(parsed.trendline_line_color, Some(0x00AA55));
        assert_eq!(parsed.trendline_line_width, Some(22225));
    }

    #[test]
    fn changes_chart_error_bar_fields() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:errBars><c:spPr><a:ln w="11111"><a:solidFill><a:srgbClr val="111111"/></a:solidFill></a:ln></c:spPr><c:errDir val="x"/><c:errBarType val="plus"/><c:errValType val="percentage"/><c:noEndCap val="0"/><c:val val="5"/></c:errBars><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                error_bar_direction: Some(ChartErrorBarDirection::Y),
                error_bar_type: Some(ChartErrorBarType::Both),
                error_bar_value_type: Some(ChartErrorBarValueType::FixedValue),
                error_bar_value: Some(1.5),
                error_bar_no_end_cap: Some(true),
                error_bar_line_color: Some(0xCC5500),
                error_bar_line_width: Some(31750),
                ..Default::default()
            },
        )
        .expect("update error-bar fields");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:spPr><a:ln w="31750"><a:solidFill><a:srgbClr val="CC5500"/></a:solidFill></a:ln></c:spPr>"#),
            "{text}"
        );
        assert!(text.contains(r#"<c:errDir val="y"/>"#), "{text}");
        assert!(text.contains(r#"<c:errBarType val="both"/>"#), "{text}");
        assert!(text.contains(r#"<c:errValType val="fixedVal"/>"#), "{text}");
        assert!(text.contains(r#"<c:noEndCap val="1"/>"#), "{text}");
        assert!(text.contains(r#"<c:val val="1.5"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.error_bar_direction, Some(ChartErrorBarDirection::Y));
        assert_eq!(parsed.error_bar_type, Some(ChartErrorBarType::Both));
        assert_eq!(
            parsed.error_bar_value_type,
            Some(ChartErrorBarValueType::FixedValue)
        );
        assert_eq!(parsed.error_bar_value, Some(1.5));
        assert_eq!(parsed.error_bar_no_end_cap, Some(true));
        assert_eq!(parsed.error_bar_line_color, Some(0xCC5500));
        assert_eq!(parsed.error_bar_line_width, Some(31750));
    }

    #[test]
    fn inserts_chart_error_bars_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:lineChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>B</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                error_bar_direction: Some(ChartErrorBarDirection::Y),
                error_bar_type: Some(ChartErrorBarType::Both),
                error_bar_value_type: Some(ChartErrorBarValueType::FixedValue),
                error_bar_value: Some(1.5),
                error_bar_no_end_cap: Some(true),
                error_bar_line_color: Some(0xCC5500),
                error_bar_line_width: Some(31750),
                ..Default::default()
            },
        )
        .expect("insert error-bar fields");
        let text = String::from_utf8(out).expect("utf8");
        assert_eq!(text.matches(r#"<c:errBars>"#).count(), 2, "{text}");
        assert_eq!(text.matches(r#"<a:ln w="31750">"#).count(), 2, "{text}");
        assert_eq!(
            text.matches(r#"<a:srgbClr val="CC5500"/>"#).count(),
            2,
            "{text}"
        );
        assert_eq!(text.matches(r#"<c:errDir val="y"/>"#).count(), 2, "{text}");
        assert_eq!(
            text.matches(r#"<c:errBarType val="both"/>"#).count(),
            2,
            "{text}"
        );
        assert_eq!(
            text.matches(r#"<c:errValType val="fixedVal"/>"#).count(),
            2,
            "{text}"
        );
        assert_eq!(
            text.matches(r#"<c:noEndCap val="1"/>"#).count(),
            2,
            "{text}"
        );
        assert_eq!(text.matches(r#"<c:val val="1.5"/>"#).count(), 2, "{text}");
        assert!(
            text.contains(r#"</c:errBars><c:cat>"#),
            "errBars should be inserted before category/value data: {text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.error_bar_direction, Some(ChartErrorBarDirection::Y));
        assert_eq!(parsed.error_bar_type, Some(ChartErrorBarType::Both));
        assert_eq!(
            parsed.error_bar_value_type,
            Some(ChartErrorBarValueType::FixedValue)
        );
        assert_eq!(parsed.error_bar_value, Some(1.5));
        assert_eq!(parsed.error_bar_no_end_cap, Some(true));
        assert_eq!(parsed.error_bar_line_color, Some(0xCC5500));
        assert_eq!(parsed.error_bar_line_width, Some(31750));
    }

    #[test]
    fn changes_scatter_style_marker_and_smooth() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:scatterChart><c:scatterStyle val="line"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Y1</c:v></c:pt></c:strCache></c:strRef></c:tx><c:marker><c:size val="7"/></c:marker><c:xVal><c:numRef><c:numCache><c:pt idx="0"><c:v>0.7</c:v></c:pt><c:pt idx="1"><c:v>1.8</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numRef><c:numCache><c:pt idx="0"><c:v>2.7</c:v></c:pt><c:pt idx="1"><c:v>3.2</c:v></c:pt></c:numCache></c:numRef></c:yVal><c:smooth val="0"/></c:ser><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Y2</c:v></c:pt></c:strCache></c:strRef></c:tx><c:marker><c:size val="7"/></c:marker><c:xVal><c:numRef><c:numCache><c:pt idx="0"><c:v>0.7</c:v></c:pt><c:pt idx="1"><c:v>1.8</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numRef><c:numCache><c:pt idx="0"><c:v>2.0</c:v></c:pt><c:pt idx="1"><c:v>2.5</c:v></c:pt></c:numCache></c:numRef></c:yVal><c:smooth val="0"/></c:ser></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: Some(ScatterStyle::SmoothMarker),
                scatter_smooth: Some(true),
                scatter_marker_size: Some(11),
                scatter_marker_symbol: Some(ChartMarkerSymbol::Square),
                scatter_marker_fill_color: Some(0xFFD966),
                scatter_marker_line_color: Some(0x70AD47),
                scatter_marker_line_width: Some(19050),
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update scatter style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:scatterStyle val="smoothMarker"/>"#),
            "{text}"
        );
        assert_eq!(text.matches(r#"<c:size val="11"/>"#).count(), 2, "{text}");
        assert_eq!(
            text.matches(r#"<c:symbol val="square"/>"#).count(),
            2,
            "{text}"
        );
        assert_eq!(
            text.matches(r#"<a:srgbClr val="FFD966"/>"#).count(),
            2,
            "{text}"
        );
        assert_eq!(
            text.matches(
                r#"<a:ln w="19050"><a:solidFill><a:srgbClr val="70AD47"/></a:solidFill></a:ln>"#
            )
            .count(),
            2,
            "{text}"
        );
        assert_eq!(text.matches(r#"<c:smooth val="1"/>"#).count(), 2, "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Scatter);
        assert_eq!(parsed.scatter_style, Some(ScatterStyle::SmoothMarker));
        assert_eq!(parsed.scatter_smooth, Some(true));
        assert_eq!(parsed.scatter_marker_size, Some(11));
        assert_eq!(
            parsed.scatter_marker_symbol,
            Some(ChartMarkerSymbol::Square)
        );
        assert_eq!(parsed.scatter_marker_fill_color, Some(0xFFD966));
        assert_eq!(parsed.scatter_marker_line_color, Some(0x70AD47));
        assert_eq!(parsed.scatter_marker_line_width, Some(19050));
        assert_eq!(parsed.categories, vec!["0.7", "1.8"]);
        assert_eq!(parsed.series[0].values, vec![2.7, 3.2]);
    }

    #[test]
    fn inserts_scatter_style_marker_and_smooth_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:scatterChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Y1</c:v></c:pt></c:strCache></c:strRef></c:tx><c:xVal><c:numRef><c:numCache><c:pt idx="0"><c:v>0.7</c:v></c:pt><c:pt idx="1"><c:v>1.8</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numRef><c:numCache><c:pt idx="0"><c:v>2.7</c:v></c:pt><c:pt idx="1"><c:v>3.2</c:v></c:pt></c:numCache></c:numRef></c:yVal></c:ser><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Y2</c:v></c:pt></c:strCache></c:strRef></c:tx><c:xVal><c:numRef><c:numCache><c:pt idx="0"><c:v>0.7</c:v></c:pt><c:pt idx="1"><c:v>1.8</c:v></c:pt></c:numCache></c:numRef></c:xVal><c:yVal><c:numRef><c:numCache><c:pt idx="0"><c:v>2.0</c:v></c:pt><c:pt idx="1"><c:v>2.5</c:v></c:pt></c:numCache></c:numRef></c:yVal></c:ser><c:axId val="1"/></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                scatter_style: Some(ScatterStyle::Marker),
                scatter_smooth: Some(false),
                scatter_marker_size: Some(10),
                ..Default::default()
            },
        )
        .expect("insert scatter style fields");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:scatterStyle val="marker"/><c:ser>"#),
            "{text}"
        );
        assert_eq!(
            text.matches(r#"<c:marker><c:size val="10"/></c:marker>"#)
                .count(),
            2,
            "{text}"
        );
        assert_eq!(text.matches(r#"<c:smooth val="0"/>"#).count(), 2, "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Scatter);
        assert_eq!(parsed.scatter_style, Some(ScatterStyle::Marker));
        assert_eq!(parsed.scatter_smooth, Some(false));
        assert_eq!(parsed.scatter_marker_size, Some(10));
        assert_eq!(parsed.categories, vec!["0.7", "1.8"]);
        assert_eq!(parsed.series[1].values, vec![2.0, 2.5]);
    }

    #[test]
    fn changes_pie_first_slice_angle_and_explosion() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:pieChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Sales</c:v></c:pt></c:strCache></c:strRef></c:tx><c:explosion val="0"/><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:firstSliceAng val="0"/></c:pieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: Some(45),
                pie_explosion: Some(12),
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update pie style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:firstSliceAng val="45"/>"#), "{text}");
        assert!(text.contains(r#"<c:explosion val="12"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Pie);
        assert_eq!(parsed.pie_first_slice_angle, Some(45));
        assert_eq!(parsed.pie_explosion, Some(12));
    }

    #[test]
    fn inserts_pie_first_slice_angle_and_explosion_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:pieChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Sales</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Services</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>20</c:v></c:pt><c:pt idx="1"><c:v>80</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:extLst/></c:pieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                pie_first_slice_angle: Some(90),
                pie_explosion: Some(18),
                ..Default::default()
            },
        )
        .expect("insert pie style fields");
        let text = String::from_utf8(out).expect("utf8");
        assert_eq!(
            text.matches(r#"<c:explosion val="18"/>"#).count(),
            2,
            "{text}"
        );
        assert!(
            text.contains(r#"<c:firstSliceAng val="90"/><c:extLst/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Pie);
        assert_eq!(parsed.pie_first_slice_angle, Some(90));
        assert_eq!(parsed.pie_explosion, Some(18));
    }

    #[test]
    fn changes_and_inserts_doughnut_hole_size() {
        let existing = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:doughnutChart><c:ser><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:firstSliceAng val="0"/><c:holeSize val="40"/></c:doughnutChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            existing,
            &ChartXmlUpdate {
                doughnut_hole_size: Some(65),
                ..Default::default()
            },
        )
        .expect("update doughnut hole size");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:holeSize val="65"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert!(parsed.has_doughnut_chart);
        assert_eq!(parsed.doughnut_hole_size, Some(65));

        let missing = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:doughnutChart><c:ser><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:firstSliceAng val="0"/><c:extLst/></c:doughnutChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            missing,
            &ChartXmlUpdate {
                doughnut_hole_size: Some(55),
                ..Default::default()
            },
        )
        .expect("insert doughnut hole size");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:firstSliceAng val="0"/><c:holeSize val="55"/><c:extLst/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.doughnut_hole_size, Some(55));
    }

    #[test]
    fn changes_of_pie_type_gap_and_second_size() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:ofPieChart><c:ofPieType val="pie"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Sales</c:v></c:pt></c:strCache></c:strRef></c:tx><c:explosion val="0"/><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:gapWidth val="100"/><c:secondPieSize val="75"/></c:ofPieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                pie_of_pie_type: Some(OfPieType::Bar),
                pie_of_pie_gap_width: Some(140),
                pie_of_pie_second_size: Some(85),
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                ..Default::default()
            },
        )
        .expect("update ofPie style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:ofPieType val="bar"/>"#), "{text}");
        assert!(text.contains(r#"<c:gapWidth val="140"/>"#), "{text}");
        assert!(text.contains(r#"<c:secondPieSize val="85"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Pie);
        assert_eq!(parsed.pie_of_pie_type, Some(OfPieType::Bar));
        assert_eq!(parsed.pie_of_pie_gap_width, Some(140));
        assert_eq!(parsed.pie_of_pie_second_size, Some(85));
    }

    #[test]
    fn inserts_of_pie_type_gap_and_second_size_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:ofPieChart><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>Sales</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>Q1</c:v></c:pt><c:pt idx="1"><c:v>Q2</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:extLst/></c:ofPieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                pie_of_pie_type: Some(OfPieType::Bar),
                pie_of_pie_gap_width: Some(140),
                pie_of_pie_second_size: Some(85),
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                ..Default::default()
            },
        )
        .expect("insert ofPie style fields");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:ofPieChart><c:ofPieType val="bar"/><c:ser>"#),
            "{text}"
        );
        assert!(
            text.contains(
                r#"</c:ser><c:gapWidth val="140"/><c:secondPieSize val="85"/><c:extLst/>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Pie);
        assert!(parsed.has_of_pie_chart);
        assert_eq!(parsed.pie_of_pie_type, Some(OfPieType::Bar));
        assert_eq!(parsed.pie_of_pie_gap_width, Some(140));
        assert_eq!(parsed.pie_of_pie_second_size, Some(85));
    }

    #[test]
    fn changes_of_pie_ser_line_style_and_inserts_when_missing() {
        let existing = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:ofPieChart><c:ofPieType val="pie"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:gapWidth val="100"/><c:secondPieSize val="75"/><c:serLines/></c:ofPieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            existing,
            &ChartXmlUpdate {
                pie_of_pie_ser_line_color: Some(0x123456),
                pie_of_pie_ser_line_width: Some(22225),
                ..Default::default()
            },
        )
        .expect("update ofPie serLines style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:serLines><c:spPr><a:ln w="22225"><a:solidFill><a:srgbClr val="123456"/></a:solidFill></a:ln></c:spPr></c:serLines>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert!(parsed.has_of_pie_chart);
        assert_eq!(parsed.pie_of_pie_ser_line_color, Some(0x123456));
        assert_eq!(parsed.pie_of_pie_ser_line_width, Some(22225));

        let missing = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:ofPieChart><c:ofPieType val="pie"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>30</c:v></c:pt><c:pt idx="1"><c:v>70</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:gapWidth val="100"/><c:secondPieSize val="75"/><c:extLst/></c:ofPieChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            missing,
            &ChartXmlUpdate {
                pie_of_pie_ser_line_color: Some(0x654321),
                pie_of_pie_ser_line_width: Some(33333),
                ..Default::default()
            },
        )
        .expect("insert ofPie serLines style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:secondPieSize val="75"/><c:serLines><c:spPr><a:ln w="33333"><a:solidFill><a:srgbClr val="654321"/></a:solidFill></a:ln></c:spPr></c:serLines><c:extLst/>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.pie_of_pie_ser_line_color, Some(0x654321));
        assert_eq!(parsed.pie_of_pie_ser_line_width, Some(33333));
    }

    #[test]
    fn changes_stock_up_down_bar_style_and_inserts_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:stockChart><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>15</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>8</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>13</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/><c:axId val="2"/></c:stockChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                stock_up_down_bar_gap_width: Some(75),
                stock_up_bar_fill_color: Some(0x00B050),
                stock_down_bar_fill_color: Some(0xC00000),
                stock_up_bar_line_color: Some(0x006100),
                stock_down_bar_line_color: Some(0x660000),
                stock_up_bar_line_width: Some(19050),
                stock_down_bar_line_width: Some(25400),
                ..Default::default()
            },
        )
        .expect("update stock up/down style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("<c:upDownBars>"), "{text}");
        assert!(text.contains(r#"<c:gapWidth val="75"/>"#), "{text}");
        assert!(text.contains("<c:upBars>"), "{text}");
        assert!(text.contains("<c:downBars>"), "{text}");
        assert!(text.contains(r#"<a:srgbClr val="00B050"/>"#), "{text}");
        assert!(text.contains(r#"<a:srgbClr val="006100"/>"#), "{text}");
        assert!(text.contains(r#"<a:ln w="19050">"#), "{text}");
        assert!(text.contains(r#"<a:srgbClr val="C00000"/>"#), "{text}");
        assert!(text.contains(r#"<a:srgbClr val="660000"/>"#), "{text}");
        assert!(text.contains(r#"<a:ln w="25400">"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Stock);
        assert_eq!(parsed.stock_up_down_bar_gap_width, Some(75));
        assert_eq!(parsed.stock_up_bar_fill_color, Some(0x00B050));
        assert_eq!(parsed.stock_up_bar_line_color, Some(0x006100));
        assert_eq!(parsed.stock_up_bar_line_width, Some(19050));
        assert_eq!(parsed.stock_down_bar_fill_color, Some(0xC00000));
        assert_eq!(parsed.stock_down_bar_line_color, Some(0x660000));
        assert_eq!(parsed.stock_down_bar_line_width, Some(25400));
    }

    #[test]
    fn changes_stock_hi_low_line_style_and_inserts_when_missing() {
        let existing = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:stockChart><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>15</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:hiLowLines/><c:axId val="1"/></c:stockChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            existing,
            &ChartXmlUpdate {
                stock_hi_low_line_color: Some(0x123456),
                stock_hi_low_line_width: Some(22225),
                ..Default::default()
            },
        )
        .expect("update stock high-low line style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:hiLowLines><c:spPr><a:ln w="22225"><a:solidFill><a:srgbClr val="123456"/></a:solidFill></a:ln></c:spPr></c:hiLowLines>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Stock);
        assert_eq!(parsed.stock_hi_low_line_color, Some(0x123456));
        assert_eq!(parsed.stock_hi_low_line_width, Some(22225));

        let missing = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:stockChart><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>10</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>15</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="1"/></c:stockChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            missing,
            &ChartXmlUpdate {
                stock_hi_low_line_color: Some(0x654321),
                stock_hi_low_line_width: Some(33333),
                ..Default::default()
            },
        )
        .expect("insert stock high-low line style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:hiLowLines><c:spPr><a:ln w="33333"><a:solidFill><a:srgbClr val="654321"/></a:solidFill></a:ln></c:spPr></c:hiLowLines><c:axId val="1"/>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_type, OoxmlChartType::Stock);
        assert_eq!(parsed.stock_hi_low_line_color, Some(0x654321));
        assert_eq!(parsed.stock_hi_low_line_width, Some(33333));
    }

    #[test]
    fn changes_legend_position() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea><c:legend><c:legendPos val="r"/><c:layout/><c:overlay val="0"/></c:legend></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: Some(ChartLegendPosition::Bottom),
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update legend position");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:legendPos val="b"/>"#), "{text}");
        assert!(text.contains(r#"<c:overlay val="0"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.legend_position, Some(ChartLegendPosition::Bottom));
    }

    #[test]
    fn inserts_legend_position_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea><c:legend><c:layout/><c:overlay val="0"/></c:legend></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                legend_position: Some(ChartLegendPosition::Bottom),
                ..Default::default()
            },
        )
        .expect("insert legend position");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:legend><c:legendPos val="b"/><c:layout/><c:overlay val="0"/></c:legend>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.legend_position, Some(ChartLegendPosition::Bottom));
    }

    #[test]
    fn changes_axis_titles() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:title><c:tx><c:rich><a:p xmlns:a="y"><a:r><a:t>Old category</a:t></a:r></a:p></c:rich></c:tx><c:layout/></c:title></c:catAx><c:valAx><c:axId val="20"/><c:title/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_title: Some("분기".to_string()),
                value_axis_title: Some("금액".to_string()),
                ..Default::default()
            },
        )
        .expect("update axis titles");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("<a:t>분기</a:t>"), "{text}");
        assert!(text.contains("<a:t>금액</a:t>"), "{text}");
        assert!(!text.contains("Old category"), "{text}");
        assert!(text.contains("<c:layout/></c:title>"), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_title.as_deref(), Some("분기"));
        assert_eq!(parsed.value_axis_title.as_deref(), Some("금액"));
    }

    #[test]
    fn inserts_axis_titles_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:axPos val="b"/><c:numFmt formatCode="General" sourceLinked="1"/></c:catAx><c:valAx><c:axId val="20"/><c:axPos val="l"/><c:majorTickMark val="out"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_title: Some("분기".to_string()),
                value_axis_title: Some("금액".to_string()),
                ..Default::default()
            },
        )
        .expect("insert axis titles");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(
                r#"<c:axPos val="b"/><c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>분기</a:t></a:r></a:p></c:rich></c:tx></c:title><c:numFmt"#
            ),
            "{text}"
        );
        assert!(
            val_axis.contains(
                r#"<c:axPos val="l"/><c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>금액</a:t></a:r></a:p></c:rich></c:tx></c:title><c:majorTickMark"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_title.as_deref(), Some("분기"));
        assert_eq!(parsed.value_axis_title.as_deref(), Some("금액"));
    }

    #[test]
    fn changes_axis_visibility() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:delete val="0"/><c:axPos val="b"/></c:catAx><c:valAx><c:axId val="20"/><c:delete val="0"/><c:axPos val="l"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: Some(false),
                value_axis_visible: Some(true),
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update axis visibility");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(cat_axis.contains(r#"<c:delete val="1"/>"#), "{text}");
        assert!(val_axis.contains(r#"<c:delete val="0"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_visible, Some(false));
        assert_eq!(parsed.value_axis_visible, Some(true));
    }

    #[test]
    fn inserts_axis_visibility_when_delete_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:axPos val="b"/></c:catAx><c:valAx><c:axId val="20"/><c:axPos val="l"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: Some(false),
                value_axis_visible: Some(true),
                ..Default::default()
            },
        )
        .expect("insert axis visibility");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(r#"<c:axId val="10"/><c:delete val="1"/><c:axPos val="b"/>"#),
            "{text}"
        );
        assert!(
            val_axis.contains(r#"<c:axId val="20"/><c:delete val="0"/><c:axPos val="l"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_visible, Some(false));
        assert_eq!(parsed.value_axis_visible, Some(true));
    }

    #[test]
    fn changes_axis_label_position() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:delete val="0"/><c:tickLblPos val="nextTo"/></c:catAx><c:valAx><c:axId val="20"/><c:delete val="0"/><c:tickLblPos val="nextTo"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: Some(AxisLabelPosition::Low),
                value_axis_label_position: Some(AxisLabelPosition::High),
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update axis label position");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(cat_axis.contains(r#"<c:tickLblPos val="low"/>"#), "{text}");
        assert!(val_axis.contains(r#"<c:tickLblPos val="high"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.category_axis_label_position,
            Some(AxisLabelPosition::Low)
        );
        assert_eq!(
            parsed.value_axis_label_position,
            Some(AxisLabelPosition::High)
        );
    }

    #[test]
    fn inserts_axis_label_positions_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:delete val="0"/><c:axPos val="b"/><c:spPr/></c:catAx><c:valAx><c:axId val="20"/><c:delete val="0"/><c:axPos val="l"/><c:crossAx val="10"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_label_position: Some(AxisLabelPosition::Low),
                value_axis_label_position: Some(AxisLabelPosition::High),
                ..Default::default()
            },
        )
        .expect("insert axis label positions");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(r#"<c:axPos val="b"/><c:tickLblPos val="low"/><c:spPr/>"#),
            "{text}"
        );
        assert!(
            val_axis
                .contains(r#"<c:axPos val="l"/><c:tickLblPos val="high"/><c:crossAx val="10"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.category_axis_label_position,
            Some(AxisLabelPosition::Low)
        );
        assert_eq!(
            parsed.value_axis_label_position,
            Some(AxisLabelPosition::High)
        );
    }

    #[test]
    fn changes_value_axis_cross_between() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:crossBetween val="between"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                value_axis_cross_between: Some(AxisCrossBetween::MidCategory),
                ..Default::default()
            },
        )
        .expect("update value axis crossBetween");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:crossBetween val="midCat"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.value_axis_cross_between,
            Some(AxisCrossBetween::MidCategory)
        );
    }

    #[test]
    fn changes_axis_crosses() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:crosses val="autoZero"/></c:catAx><c:valAx><c:axId val="20"/><c:crosses val="autoZero"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_crosses: Some(AxisCrosses::Min),
                value_axis_crosses: Some(AxisCrosses::Max),
                ..Default::default()
            },
        )
        .expect("update axis crosses");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(cat_axis.contains(r#"<c:crosses val="min"/>"#), "{text}");
        assert!(val_axis.contains(r#"<c:crosses val="max"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_crosses, Some(AxisCrosses::Min));
        assert_eq!(parsed.value_axis_crosses, Some(AxisCrosses::Max));
    }

    #[test]
    fn inserts_axis_crosses_and_cross_between_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:crossAx val="20"/><c:auto val="1"/></c:catAx><c:valAx><c:axId val="20"/><c:crossAx val="10"/><c:majorUnit val="3"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_crosses: Some(AxisCrosses::Min),
                value_axis_crosses: Some(AxisCrosses::Max),
                value_axis_cross_between: Some(AxisCrossBetween::MidCategory),
                ..Default::default()
            },
        )
        .expect("insert axis crosses");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(r#"<c:crossAx val="20"/><c:crosses val="min"/><c:auto val="1"/>"#),
            "{text}"
        );
        assert!(
            val_axis.contains(
                r#"<c:crossAx val="10"/><c:crosses val="max"/><c:crossBetween val="midCat"/><c:majorUnit val="3"/>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_crosses, Some(AxisCrosses::Min));
        assert_eq!(parsed.value_axis_crosses, Some(AxisCrosses::Max));
        assert_eq!(
            parsed.value_axis_cross_between,
            Some(AxisCrossBetween::MidCategory)
        );
    }

    #[test]
    fn changes_axis_crosses_at() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:crosses val="autoZero"/><c:crossesAt val="0"/></c:catAx><c:valAx><c:axId val="20"/><c:crosses val="autoZero"/><c:crossesAt val="0"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_crosses_at: Some(2.0),
                value_axis_crosses_at: Some(1.5),
                ..Default::default()
            },
        )
        .expect("update axis crossesAt");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(!cat_axis.contains("<c:crosses "), "{text}");
        assert!(!val_axis.contains("<c:crosses "), "{text}");
        assert!(cat_axis.contains(r#"<c:crossesAt val="2"/>"#), "{text}");
        assert!(val_axis.contains(r#"<c:crossesAt val="1.5"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_crosses_at, Some(2.0));
        assert_eq!(parsed.value_axis_crosses_at, Some(1.5));
    }

    #[test]
    fn inserts_axis_crosses_at_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:crossAx val="20"/><c:auto val="1"/></c:catAx><c:valAx><c:axId val="20"/><c:crossAx val="10"/><c:majorUnit val="3"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_crosses_at: Some(2.0),
                value_axis_crosses_at: Some(1.5),
                ..Default::default()
            },
        )
        .expect("insert axis crossesAt");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(r#"<c:crossAx val="20"/><c:crossesAt val="2"/><c:auto val="1"/>"#),
            "{text}"
        );
        assert!(
            val_axis
                .contains(r#"<c:crossAx val="10"/><c:crossesAt val="1.5"/><c:majorUnit val="3"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_crosses_at, Some(2.0));
        assert_eq!(parsed.value_axis_crosses_at, Some(1.5));
    }

    #[test]
    fn changes_axis_orientation() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:scaling><c:orientation val="minMax"/></c:scaling></c:catAx><c:valAx><c:axId val="20"/><c:scaling><c:orientation val="minMax"/></c:scaling></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_orientation: Some(AxisOrientation::MaxMin),
                value_axis_orientation: Some(AxisOrientation::MaxMin),
                ..Default::default()
            },
        )
        .expect("update axis orientation");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(r#"<c:orientation val="maxMin"/>"#),
            "{text}"
        );
        assert!(
            val_axis.contains(r#"<c:orientation val="maxMin"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.category_axis_orientation,
            Some(AxisOrientation::MaxMin)
        );
        assert_eq!(parsed.value_axis_orientation, Some(AxisOrientation::MaxMin));
    }

    #[test]
    fn inserts_axis_orientation_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:delete val="0"/></c:catAx><c:valAx><c:axId val="20"/><c:scaling><c:max val="10"/></c:scaling><c:delete val="0"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_orientation: Some(AxisOrientation::MaxMin),
                value_axis_orientation: Some(AxisOrientation::MaxMin),
                ..Default::default()
            },
        )
        .expect("insert axis orientation");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(
                r#"<c:axId val="10"/><c:scaling><c:orientation val="maxMin"/></c:scaling><c:delete val="0"/>"#
            ),
            "{text}"
        );
        assert!(
            val_axis.contains(
                r#"<c:scaling><c:orientation val="maxMin"/><c:max val="10"/></c:scaling>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.category_axis_orientation,
            Some(AxisOrientation::MaxMin)
        );
        assert_eq!(parsed.value_axis_orientation, Some(AxisOrientation::MaxMin));
    }

    #[test]
    fn changes_category_axis_label_controls() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/><c:tickMarkSkip val="1"/><c:noMultiLvlLbl val="0"/></c:catAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_auto: Some(false),
                category_axis_label_alignment: Some(AxisLabelAlignment::Right),
                category_axis_label_offset: Some(250),
                category_axis_tick_mark_skip: Some(2),
                category_axis_no_multi_level_labels: Some(true),
                ..Default::default()
            },
        )
        .expect("update category axis label controls");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:auto val="0"/>"#), "{text}");
        assert!(text.contains(r#"<c:lblAlgn val="r"/>"#), "{text}");
        assert!(text.contains(r#"<c:lblOffset val="250"/>"#), "{text}");
        assert!(text.contains(r#"<c:tickMarkSkip val="2"/>"#), "{text}");
        assert!(text.contains(r#"<c:noMultiLvlLbl val="1"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_auto, Some(false));
        assert_eq!(
            parsed.category_axis_label_alignment,
            Some(AxisLabelAlignment::Right)
        );
        assert_eq!(parsed.category_axis_label_offset, Some(250));
        assert_eq!(parsed.category_axis_tick_mark_skip, Some(2));
        assert_eq!(parsed.category_axis_no_multi_level_labels, Some(true));
    }

    #[test]
    fn inserts_category_axis_label_controls_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:crosses val="autoZero"/><c:tickLblSkip val="3"/><c:extLst/></c:catAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_auto: Some(false),
                category_axis_label_alignment: Some(AxisLabelAlignment::Right),
                category_axis_label_offset: Some(250),
                category_axis_tick_mark_skip: Some(2),
                category_axis_no_multi_level_labels: Some(true),
                ..Default::default()
            },
        )
        .expect("insert category axis label controls");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        assert!(
            cat_axis.contains(
                r#"<c:crosses val="autoZero"/><c:auto val="0"/><c:lblAlgn val="r"/><c:lblOffset val="250"/><c:tickLblSkip val="3"/><c:tickMarkSkip val="2"/><c:noMultiLvlLbl val="1"/><c:extLst/>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_auto, Some(false));
        assert_eq!(
            parsed.category_axis_label_alignment,
            Some(AxisLabelAlignment::Right)
        );
        assert_eq!(parsed.category_axis_label_offset, Some(250));
        assert_eq!(parsed.category_axis_tick_mark_skip, Some(2));
        assert_eq!(parsed.category_axis_no_multi_level_labels, Some(true));
    }

    #[test]
    fn changes_axis_positions() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:axPos val="b"/></c:catAx><c:valAx><c:axId val="20"/><c:axPos val="l"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_position: Some(AxisPosition::Top),
                value_axis_position: Some(AxisPosition::Right),
                ..Default::default()
            },
        )
        .expect("update axis positions");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:axPos val="t"/>"#), "{text}");
        assert!(text.contains(r#"<c:axPos val="r"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_position, Some(AxisPosition::Top));
        assert_eq!(parsed.value_axis_position, Some(AxisPosition::Right));
    }

    #[test]
    fn inserts_axis_positions_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:majorGridlines/></c:catAx><c:valAx><c:axId val="20"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: Some(false),
                value_axis_visible: Some(true),
                category_axis_position: Some(AxisPosition::Top),
                value_axis_position: Some(AxisPosition::Right),
                ..Default::default()
            },
        )
        .expect("insert axis positions");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(
                r#"<c:axId val="10"/><c:delete val="1"/><c:axPos val="t"/><c:majorGridlines/>"#
            ),
            "{text}"
        );
        assert!(
            val_axis.contains(r#"<c:axId val="20"/><c:delete val="0"/><c:axPos val="r"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_visible, Some(false));
        assert_eq!(parsed.value_axis_visible, Some(true));
        assert_eq!(parsed.category_axis_position, Some(AxisPosition::Top));
        assert_eq!(parsed.value_axis_position, Some(AxisPosition::Right));
    }

    #[test]
    fn changes_axis_tick_marks() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/></c:catAx><c:valAx><c:axId val="20"/><c:majorTickMark val="out"/><c:minorTickMark val="none"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: Some(AxisTickMark::In),
                category_axis_minor_tick_mark: Some(AxisTickMark::Cross),
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: Some(AxisTickMark::Cross),
                value_axis_minor_tick_mark: Some(AxisTickMark::In),
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update axis tick marks");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(r#"<c:majorTickMark val="in"/>"#),
            "{text}"
        );
        assert!(
            cat_axis.contains(r#"<c:minorTickMark val="cross"/>"#),
            "{text}"
        );
        assert!(
            val_axis.contains(r#"<c:majorTickMark val="cross"/>"#),
            "{text}"
        );
        assert!(
            val_axis.contains(r#"<c:minorTickMark val="in"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_major_tick_mark, Some(AxisTickMark::In));
        assert_eq!(
            parsed.category_axis_minor_tick_mark,
            Some(AxisTickMark::Cross)
        );
        assert_eq!(parsed.value_axis_major_tick_mark, Some(AxisTickMark::Cross));
        assert_eq!(parsed.value_axis_minor_tick_mark, Some(AxisTickMark::In));
    }

    #[test]
    fn inserts_axis_tick_marks_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:tickLblPos val="nextTo"/></c:catAx><c:valAx><c:axId val="20"/><c:crossAx val="10"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_major_tick_mark: Some(AxisTickMark::In),
                category_axis_minor_tick_mark: Some(AxisTickMark::Cross),
                value_axis_major_tick_mark: Some(AxisTickMark::Cross),
                value_axis_minor_tick_mark: Some(AxisTickMark::In),
                ..Default::default()
            },
        )
        .expect("insert axis tick marks");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(
                r#"<c:majorTickMark val="in"/><c:minorTickMark val="cross"/><c:tickLblPos val="nextTo"/>"#
            ),
            "{text}"
        );
        assert!(
            val_axis.contains(
                r#"<c:majorTickMark val="cross"/><c:minorTickMark val="in"/><c:crossAx val="10"/>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_major_tick_mark, Some(AxisTickMark::In));
        assert_eq!(
            parsed.category_axis_minor_tick_mark,
            Some(AxisTickMark::Cross)
        );
        assert_eq!(parsed.value_axis_major_tick_mark, Some(AxisTickMark::Cross));
        assert_eq!(parsed.value_axis_minor_tick_mark, Some(AxisTickMark::In));
    }

    #[test]
    fn inserts_axis_line_style() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/></c:catAx><c:valAx><c:axId val="20"/><c:spPr><a:ln w="9525"><a:solidFill><a:srgbClr val="777777"/></a:solidFill></a:ln></c:spPr></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: Some(0x112233),
                category_axis_line_width: Some(19050),
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: Some(0x445566),
                value_axis_line_width: Some(25400),
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("insert/update axis line style");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(cat_axis.contains(r#"<c:spPr><a:ln w="19050">"#), "{text}");
        assert!(cat_axis.contains(r#"<a:srgbClr val="112233"/>"#), "{text}");
        assert!(val_axis.contains(r#"<c:spPr><a:ln w="25400">"#), "{text}");
        assert!(val_axis.contains(r#"<a:srgbClr val="445566"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_line_color, Some(0x112233));
        assert_eq!(parsed.category_axis_line_width, Some(19050));
        assert_eq!(parsed.value_axis_line_color, Some(0x445566));
        assert_eq!(parsed.value_axis_line_width, Some(25400));
    }

    #[test]
    fn inserts_axis_grid_line_style() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/></c:catAx><c:valAx><c:axId val="20"/><c:majorGridlines/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_major_grid_line_color: Some(0x99AA00),
                category_axis_major_grid_line_width: Some(6350),
                category_axis_minor_grid_line_color: Some(0xAA5500),
                category_axis_minor_grid_line_width: Some(9525),
                value_axis_major_grid_line_color: Some(0x778899),
                value_axis_major_grid_line_width: Some(12700),
                value_axis_minor_grid_line_color: Some(0x334455),
                value_axis_minor_grid_line_width: Some(15875),
                ..Default::default()
            },
        )
        .expect("insert/update axis gridline style");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("cat axis XML");
        let val_axis = text
            .split("<c:valAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            cat_axis.contains(r#"<c:majorGridlines><c:spPr><a:ln w="6350">"#),
            "{text}"
        );
        assert!(cat_axis.contains(r#"<a:srgbClr val="99AA00"/>"#), "{text}");
        assert!(
            cat_axis.contains(r#"<c:minorGridlines><c:spPr><a:ln w="9525">"#),
            "{text}"
        );
        assert!(cat_axis.contains(r#"<a:srgbClr val="AA5500"/>"#), "{text}");
        assert!(
            val_axis.contains(r#"<c:majorGridlines><c:spPr><a:ln w="12700">"#),
            "{text}"
        );
        assert!(val_axis.contains(r#"<a:srgbClr val="778899"/>"#), "{text}");
        assert!(
            val_axis.contains(r#"<c:minorGridlines><c:spPr><a:ln w="15875">"#),
            "{text}"
        );
        assert!(val_axis.contains(r#"<a:srgbClr val="334455"/>"#), "{text}");
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.category_axis_major_grid_line_color, Some(0x99AA00));
        assert_eq!(parsed.category_axis_major_grid_line_width, Some(6350));
        assert_eq!(parsed.category_axis_minor_grid_line_color, Some(0xAA5500));
        assert_eq!(parsed.category_axis_minor_grid_line_width, Some(9525));
        assert_eq!(parsed.value_axis_major_grid_line_color, Some(0x778899));
        assert_eq!(parsed.value_axis_major_grid_line_width, Some(12700));
        assert_eq!(parsed.value_axis_minor_grid_line_color, Some(0x334455));
        assert_eq!(parsed.value_axis_minor_grid_line_width, Some(15875));
    }

    #[test]
    fn inserts_value_axis_scale() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: Some(0.0),
                value_axis_maximum: Some(12.0),
                value_axis_major_unit: Some(3.0),
                value_axis_minor_unit: Some(1.5),
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: None,
                value_axis_number_format_source_linked: None,
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("insert value axis scale");
        let text = String::from_utf8(out).expect("utf8");
        let val_axis = text
            .split("<c:valAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(val_axis.contains(r#"<c:max val="12"/>"#), "{text}");
        assert!(val_axis.contains(r#"<c:min val="0"/>"#), "{text}");
        assert!(val_axis.contains(r#"<c:majorUnit val="3"/>"#), "{text}");
        assert!(val_axis.contains(r#"<c:minorUnit val="1.5"/>"#), "{text}");

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("reparse edited chart");
        assert_eq!(parsed.value_axis_minimum, Some(0.0));
        assert_eq!(parsed.value_axis_maximum, Some(12.0));
        assert_eq!(parsed.value_axis_major_unit, Some(3.0));
        assert_eq!(parsed.value_axis_minor_unit, Some(1.5));
    }

    #[test]
    fn changes_value_axis_log_base() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:scaling><c:logBase val="2"/><c:orientation val="minMax"/></c:scaling><c:delete val="0"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                value_axis_log_base: Some(10.0),
                ..Default::default()
            },
        )
        .expect("change value axis log base");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:logBase val="10"/>"#), "{text}");

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("reparse edited chart");
        assert_eq!(parsed.value_axis_log_base, Some(10.0));
    }

    #[test]
    fn inserts_value_axis_log_base_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                value_axis_log_base: Some(10.0),
                ..Default::default()
            },
        )
        .expect("insert value axis log base");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:scaling><c:logBase val="10"/><c:orientation val="minMax"/></c:scaling>"#
            ),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("reparse edited chart");
        assert_eq!(parsed.value_axis_log_base, Some(10.0));
    }

    #[test]
    fn changes_value_axis_display_unit() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:dispUnits><c:builtInUnit val="thousands"/></c:dispUnits></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                value_axis_display_unit: Some(AxisDisplayUnit::Millions),
                ..Default::default()
            },
        )
        .expect("change value axis display unit");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:builtInUnit val="millions"/>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("reparse edited chart");
        assert_eq!(
            parsed.value_axis_display_unit,
            Some(AxisDisplayUnit::Millions)
        );
    }

    #[test]
    fn inserts_value_axis_display_unit_into_empty_disp_units() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:dispUnits/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                value_axis_display_unit: Some(AxisDisplayUnit::Millions),
                ..Default::default()
            },
        )
        .expect("insert value axis display unit into empty dispUnits");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:dispUnits><c:builtInUnit val="millions"/></c:dispUnits>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("reparse edited chart");
        assert_eq!(
            parsed.value_axis_display_unit,
            Some(AxisDisplayUnit::Millions)
        );
    }

    #[test]
    fn inserts_value_axis_display_units_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:minorUnit val="1"/><c:extLst/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                value_axis_display_unit: Some(AxisDisplayUnit::Millions),
                ..Default::default()
            },
        )
        .expect("insert value axis display units");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:minorUnit val="1"/><c:dispUnits><c:builtInUnit val="millions"/></c:dispUnits><c:extLst/>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("reparse edited chart");
        assert_eq!(
            parsed.value_axis_display_unit,
            Some(AxisDisplayUnit::Millions)
        );
    }

    #[test]
    fn inserts_value_axis_scale_when_scaling_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:delete val="0"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                value_axis_log_base: Some(10.0),
                value_axis_minimum: Some(0.0),
                value_axis_maximum: Some(12.0),
                ..Default::default()
            },
        )
        .expect("insert value axis scaling");
        let text = String::from_utf8(out).expect("utf8");
        let val_axis = text
            .split("<c:valAx>")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            val_axis.contains(
                r#"<c:axId val="20"/><c:scaling><c:logBase val="10"/><c:max val="12"/><c:min val="0"/></c:scaling><c:delete val="0"/>"#
            ),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("reparse edited chart");
        assert_eq!(parsed.value_axis_log_base, Some(10.0));
        assert_eq!(parsed.value_axis_minimum, Some(0.0));
        assert_eq!(parsed.value_axis_maximum, Some(12.0));
    }

    #[test]
    fn changes_value_axis_number_format() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1234.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:numFmt formatCode="General" sourceLinked="1"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title: None,
                chart_type: None,
                grouping: None,
                bar_gap_width: None,
                bar_overlap: None,
                line_smooth: None,
                line_marker_size: None,
                pie_first_slice_angle: None,
                pie_explosion: None,
                pie_of_pie_type: None,
                pie_of_pie_gap_width: None,
                pie_of_pie_second_size: None,
                pie_of_pie_ser_line_color: None,
                pie_of_pie_ser_line_width: None,
                scatter_style: None,
                scatter_smooth: None,
                scatter_marker_size: None,
                legend_position: None,
                category_axis_title: None,
                value_axis_title: None,
                category_axis_visible: None,
                value_axis_visible: None,
                category_axis_label_position: None,
                value_axis_label_position: None,
                category_axis_major_tick_mark: None,
                category_axis_minor_tick_mark: None,
                category_axis_line_color: None,
                category_axis_line_width: None,
                value_axis_major_tick_mark: None,
                value_axis_minor_tick_mark: None,
                value_axis_line_color: None,
                value_axis_line_width: None,
                value_axis_minimum: None,
                value_axis_maximum: None,
                value_axis_major_unit: None,
                value_axis_minor_unit: None,
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: Some("#,##0.0".to_string()),
                value_axis_number_format_source_linked: Some(false),
                categories: None,
                series: Vec::new(),
                ..Default::default()
            },
        )
        .expect("update value axis number format");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r##"<c:numFmt formatCode="#,##0.0" sourceLinked="0"/>"##),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.value_axis_number_format.as_deref(), Some("#,##0.0"));
        assert_eq!(parsed.value_axis_number_format_source_linked, Some(false));
    }

    #[test]
    fn inserts_value_axis_number_format_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>C1</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1234.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:valAx><c:axId val="20"/><c:majorTickMark val="out"/><c:crossAx val="10"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_number_format: None,
                category_axis_number_format_source_linked: None,
                value_axis_number_format: Some("#,##0.0".to_string()),
                value_axis_number_format_source_linked: Some(false),
                ..Default::default()
            },
        )
        .expect("insert value axis number format");
        let text = String::from_utf8(out).expect("utf8");
        let val_axis = text
            .split("<c:valAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:valAx>").next())
            .expect("value axis XML");
        assert!(
            val_axis.contains(
                r##"<c:axId val="20"/><c:numFmt formatCode="#,##0.0" sourceLinked="0"/><c:majorTickMark val="out"/>"##
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.value_axis_number_format.as_deref(), Some("#,##0.0"));
        assert_eq!(parsed.value_axis_number_format_source_linked, Some(false));
    }

    #[test]
    fn changes_category_axis_number_format() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>2026-01</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1234.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:numFmt formatCode="General" sourceLinked="1"/></c:catAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_number_format: Some("yyyy-mm".to_string()),
                category_axis_number_format_source_linked: Some(false),
                ..Default::default()
            },
        )
        .expect("update category axis number format");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:numFmt formatCode="yyyy-mm" sourceLinked="0"/>"#),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.category_axis_number_format.as_deref(),
            Some("yyyy-mm")
        );
        assert_eq!(
            parsed.category_axis_number_format_source_linked,
            Some(false)
        );
    }

    #[test]
    fn inserts_category_axis_number_format_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:cat><c:strRef><c:strCache><c:pt idx="0"><c:v>2026-01</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1234.5</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:majorTickMark val="out"/><c:tickLblPos val="nextTo"/></c:catAx></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                category_axis_number_format: Some("yyyy-mm".to_string()),
                category_axis_number_format_source_linked: Some(false),
                ..Default::default()
            },
        )
        .expect("insert category axis number format");
        let text = String::from_utf8(out).expect("utf8");
        let cat_axis = text
            .split("<c:catAx")
            .nth(1)
            .and_then(|tail| tail.split("</c:catAx>").next())
            .expect("category axis XML");
        assert!(
            cat_axis.contains(
                r#"<c:axId val="10"/><c:numFmt formatCode="yyyy-mm" sourceLinked="0"/><c:majorTickMark val="out"/>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.category_axis_number_format.as_deref(),
            Some("yyyy-mm")
        );
        assert_eq!(
            parsed.category_axis_number_format_source_linked,
            Some(false)
        );
    }

    #[test]
    fn changes_data_labels_and_inserts_missing_children() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:dLbls><c:showVal val="0"/></c:dLbls><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                data_label_position: Some(ChartDataLabelPosition::OutsideEnd),
                data_labels_show_value: Some(true),
                data_labels_show_category_name: Some(false),
                data_labels_show_series_name: Some(true),
                data_labels_show_percent: Some(false),
                data_labels_show_legend_key: Some(true),
                ..Default::default()
            },
        )
        .expect("update data labels");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:dLblPos val="outEnd"/>"#), "{text}");
        assert!(text.contains(r#"<c:showVal val="1"/>"#), "{text}");
        assert!(text.contains(r#"<c:showCatName val="0"/>"#), "{text}");
        assert!(text.contains(r#"<c:showSerName val="1"/>"#), "{text}");
        assert!(text.contains(r#"<c:showPercent val="0"/>"#), "{text}");
        assert!(text.contains(r#"<c:showLegendKey val="1"/>"#), "{text}");

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.data_label_position,
            Some(ChartDataLabelPosition::OutsideEnd)
        );
        assert_eq!(parsed.data_labels_show_value, Some(true));
        assert_eq!(parsed.data_labels_show_category_name, Some(false));
        assert_eq!(parsed.data_labels_show_series_name, Some(true));
        assert_eq!(parsed.data_labels_show_percent, Some(false));
        assert_eq!(parsed.data_labels_show_legend_key, Some(true));
    }

    #[test]
    fn inserts_data_labels_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:strCache><c:pt idx="0"><c:v>A</c:v></c:pt></c:strCache></c:strRef></c:tx><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                data_label_position: Some(ChartDataLabelPosition::Center),
                data_labels_show_value: Some(true),
                ..Default::default()
            },
        )
        .expect("insert data labels");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:dLbls><c:dLblPos val="ctr"/><c:showVal val="1"/></c:dLbls></c:barChart>"#
            ),
            "{text}"
        );
        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(
            parsed.data_label_position,
            Some(ChartDataLabelPosition::Center)
        );
        assert_eq!(parsed.data_labels_show_value, Some(true));
    }

    #[test]
    fn changes_and_inserts_chart_display_options() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea><c:dispBlanksAs val="gap"/></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                display_blanks_as: Some(ChartDisplayBlanksAs::Span),
                show_hidden_data: Some(true),
                plot_visible_only: Some(false),
                ..Default::default()
            },
        )
        .expect("update display options");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:dispBlanksAs val="span"/>"#), "{text}");
        assert!(text.contains(r#"<c:showHiddenData val="1"/>"#), "{text}");
        assert!(
            text.contains(r#"<c:plotVisOnly val="0"/></c:chart>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.display_blanks_as, Some(ChartDisplayBlanksAs::Span));
        assert_eq!(parsed.show_hidden_data, Some(true));
        assert_eq!(parsed.plot_visible_only, Some(false));
    }

    #[test]
    fn changes_chart_data_table_flags() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart><c:dTable><c:showHorzBorder val="0"/><c:showVertBorder val="1"/><c:showOutline val="0"/><c:showKeys val="0"/></c:dTable></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                data_table_show_horizontal_border: Some(true),
                data_table_show_vertical_border: Some(false),
                data_table_show_outline: Some(true),
                data_table_show_keys: Some(true),
                ..Default::default()
            },
        )
        .expect("update data table flags");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:showHorzBorder val="1"/>"#), "{text}");
        assert!(text.contains(r#"<c:showVertBorder val="0"/>"#), "{text}");
        assert!(text.contains(r#"<c:showOutline val="1"/>"#), "{text}");
        assert!(text.contains(r#"<c:showKeys val="1"/>"#), "{text}");

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.data_table_show_horizontal_border, Some(true));
        assert_eq!(parsed.data_table_show_vertical_border, Some(false));
        assert_eq!(parsed.data_table_show_outline, Some(true));
        assert_eq!(parsed.data_table_show_keys, Some(true));
    }

    #[test]
    fn inserts_chart_data_table_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart><c:extLst/></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                data_table_show_horizontal_border: Some(true),
                data_table_show_vertical_border: Some(false),
                data_table_show_outline: Some(true),
                data_table_show_keys: Some(false),
                ..Default::default()
            },
        )
        .expect("insert data table");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"</c:barChart><c:dTable><c:showHorzBorder val="1"/><c:showVertBorder val="0"/><c:showOutline val="1"/><c:showKeys val="0"/></c:dTable><c:extLst/>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.data_table_show_horizontal_border, Some(true));
        assert_eq!(parsed.data_table_show_vertical_border, Some(false));
        assert_eq!(parsed.data_table_show_outline, Some(true));
        assert_eq!(parsed.data_table_show_keys, Some(false));
    }

    #[test]
    fn changes_chart_space_and_auto_title_flags() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:date1904 val="0"/><c:roundedCorners val="0"/><c:chart><c:autoTitleDeleted val="0"/><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                date_1904: Some(true),
                rounded_corners: Some(true),
                auto_title_deleted: Some(true),
                ..Default::default()
            },
        )
        .expect("update chart-space flags");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:date1904 val="1"/>"#), "{text}");
        assert!(text.contains(r#"<c:roundedCorners val="1"/>"#), "{text}");
        assert!(text.contains(r#"<c:autoTitleDeleted val="1"/>"#), "{text}");

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.date_1904, Some(true));
        assert_eq!(parsed.rounded_corners, Some(true));
        assert_eq!(parsed.auto_title_deleted, Some(true));
    }

    #[test]
    fn inserts_chart_space_title_and_plot_flags_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                date_1904: Some(true),
                rounded_corners: Some(true),
                auto_title_deleted: Some(true),
                vary_colors: Some(true),
                ..Default::default()
            },
        )
        .expect("insert chart-space and plot flags");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:date1904 val="1"/><c:roundedCorners val="1"/><c:chart>"#),
            "{text}"
        );
        assert!(
            text.contains(r#"<c:autoTitleDeleted val="1"/><c:plotArea>"#),
            "{text}"
        );
        assert!(
            text.contains(r#"<c:barDir val="col"/><c:varyColors val="1"/><c:ser>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.date_1904, Some(true));
        assert_eq!(parsed.rounded_corners, Some(true));
        assert_eq!(parsed.auto_title_deleted, Some(true));
        assert_eq!(parsed.vary_colors, Some(true));
    }

    #[test]
    fn changes_chart_style_alternate_content() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:mc="m" xmlns:c14="y"><mc:AlternateContent><mc:Choice Requires="c14"><c14:style val="102"/></mc:Choice><mc:Fallback><c:style val="2"/></mc:Fallback></mc:AlternateContent><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                chart_style: Some(4),
                ..Default::default()
            },
        )
        .expect("update chart style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c14:style val="104"/>"#), "{text}");
        assert!(text.contains(r#"<c:style val="4"/>"#), "{text}");

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_style, Some(4));
    }

    #[test]
    fn inserts_chart_style_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:roundedCorners val="0"/><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                chart_style: Some(5),
                ..Default::default()
            },
        )
        .expect("insert chart style");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:roundedCorners val="0"/><c:style val="5"/><c:chart>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_style, Some(5));
    }

    #[test]
    fn changes_chart_area_fill_color_preserving_line() {
        let xml = br##"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart><c:spPr><a:noFill/><a:ln w="9525"><a:noFill/></a:ln></c:spPr></c:chartSpace>"##;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                chart_area_fill_color: Some(0xE6F0FA),
                ..Default::default()
            },
        )
        .expect("update chart area fill");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:spPr><a:solidFill><a:srgbClr val="E6F0FA"/></a:solidFill><a:ln w="9525"><a:noFill/></a:ln></c:spPr>"#
            ),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_area_fill_color, Some(0xE6F0FA));
    }

    #[test]
    fn inserts_chart_area_fill_color_before_tx_pr_when_sp_pr_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart><c:txPr/></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                chart_area_fill_color: Some(0xE6F0FA),
                ..Default::default()
            },
        )
        .expect("insert chart area fill");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"</c:chart><c:spPr><a:solidFill><a:srgbClr val="E6F0FA"/></a:solidFill></c:spPr><c:txPr/>"#
            ),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.chart_area_fill_color, Some(0xE6F0FA));
    }

    #[test]
    fn changes_plot_area_fill_color_preserving_line() {
        let xml = br##"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart><c:spPr><a:noFill/><a:ln w="9525"><a:noFill/></a:ln></c:spPr></c:plotArea></c:chart></c:chartSpace>"##;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                plot_area_fill_color: Some(0xF1E4D6),
                ..Default::default()
            },
        )
        .expect("update plot area fill");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:spPr><a:solidFill><a:srgbClr val="F1E4D6"/></a:solidFill><a:ln w="9525"><a:noFill/></a:ln></c:spPr>"#
            ),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.plot_area_fill_color, Some(0xF1E4D6));
    }

    #[test]
    fn inserts_plot_area_fill_color_when_sp_pr_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart><c:extLst/></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                plot_area_fill_color: Some(0xF1E4D6),
                ..Default::default()
            },
        )
        .expect("insert plot area fill");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"<c:spPr><a:solidFill><a:srgbClr val="F1E4D6"/></a:solidFill></c:spPr><c:extLst/>"#
            ),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.plot_area_fill_color, Some(0xF1E4D6));
    }

    #[test]
    fn inserts_plot_area_fill_color_at_plot_area_end_when_no_extlst() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                plot_area_fill_color: Some(0xF1E4D6),
                ..Default::default()
            },
        )
        .expect("insert plot area fill at plotArea end");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(
                r#"</c:barChart><c:spPr><a:solidFill><a:srgbClr val="F1E4D6"/></a:solidFill></c:spPr></c:plotArea>"#
            ),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.plot_area_fill_color, Some(0xF1E4D6));
    }

    #[test]
    fn changes_chart_plot_flags() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:varyColors val="0"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                vary_colors: Some(true),
                ..Default::default()
            },
        )
        .expect("update chart plot flags");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:varyColors val="1"/>"#), "{text}");

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.vary_colors, Some(true));
    }

    #[test]
    fn changes_chart_view_3d() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:view3D><c:rAngAx val="1"/><c:rotX val="15"/><c:rotY val="20"/><c:perspective val="30"/><c:hPercent val="100"/><c:depthPercent val="100"/></c:view3D><c:plotArea><c:bar3DChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:shape val="box"/><c:gapDepth val="150"/></c:bar3DChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                view_3d_rotation_x: Some(25),
                view_3d_rotation_y: Some(35),
                view_3d_perspective: Some(45),
                view_3d_right_angle_axes: Some(false),
                view_3d_height_percent: Some(120),
                view_3d_depth_percent: Some(140),
                bar_3d_gap_depth: Some(210),
                bar_3d_shape: Some("cylinder".to_string()),
                ..Default::default()
            },
        )
        .expect("update chart view3D");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains(r#"<c:rAngAx val="0"/>"#), "{text}");
        assert!(text.contains(r#"<c:rotX val="25"/>"#), "{text}");
        assert!(text.contains(r#"<c:rotY val="35"/>"#), "{text}");
        assert!(text.contains(r#"<c:perspective val="45"/>"#), "{text}");
        assert!(text.contains(r#"<c:hPercent val="120"/>"#), "{text}");
        assert!(text.contains(r#"<c:depthPercent val="140"/>"#), "{text}");
        assert!(text.contains(r#"<c:gapDepth val="210"/>"#), "{text}");
        assert!(text.contains(r#"<c:shape val="cylinder"/>"#), "{text}");

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.view_3d_right_angle_axes, Some(false));
        assert_eq!(parsed.view_3d_rotation_x, Some(25));
        assert_eq!(parsed.view_3d_rotation_y, Some(35));
        assert_eq!(parsed.view_3d_perspective, Some(45));
        assert_eq!(parsed.view_3d_height_percent, Some(120));
        assert_eq!(parsed.view_3d_depth_percent, Some(140));
        assert_eq!(parsed.bar_3d_gap_depth, Some(210));
        assert_eq!(parsed.bar_3d_shape.as_deref(), Some("cylinder"));
    }

    #[test]
    fn inserts_chart_view_3d_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:bar3DChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:bar3DChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                view_3d_rotation_x: Some(25),
                view_3d_rotation_y: Some(35),
                view_3d_perspective: Some(45),
                view_3d_right_angle_axes: Some(false),
                view_3d_height_percent: Some(120),
                view_3d_depth_percent: Some(140),
                ..Default::default()
            },
        )
        .expect("insert chart view3D");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:view3D><c:rAngAx val="0"/><c:rotX val="25"/><c:rotY val="35"/><c:perspective val="45"/><c:hPercent val="120"/><c:depthPercent val="140"/></c:view3D><c:plotArea>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.view_3d_right_angle_axes, Some(false));
        assert_eq!(parsed.view_3d_rotation_x, Some(25));
        assert_eq!(parsed.view_3d_rotation_y, Some(35));
        assert_eq!(parsed.view_3d_perspective, Some(45));
        assert_eq!(parsed.view_3d_height_percent, Some(120));
        assert_eq!(parsed.view_3d_depth_percent, Some(140));
    }

    #[test]
    fn inserts_bar_3d_gap_depth_and_shape_when_missing() {
        let xml = br#"<c:chartSpace xmlns:c="x"><c:chart><c:plotArea><c:bar3DChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser><c:axId val="10"/></c:bar3DChart></c:plotArea></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                bar_3d_gap_depth: Some(210),
                bar_3d_shape: Some("cylinder".to_string()),
                ..Default::default()
            },
        )
        .expect("insert bar3D fields");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:gapDepth val="210"/><c:shape val="cylinder"/><c:axId val="10"/>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.bar_3d_gap_depth, Some(210));
        assert_eq!(parsed.bar_3d_shape.as_deref(), Some("cylinder"));
    }

    #[test]
    fn changes_and_inserts_chart_overlays() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Title</a:t></a:r></a:p></c:rich></c:tx></c:title><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea><c:legend><c:legendPos val="r"/><c:layout/><c:overlay val="0"/></c:legend></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title_overlay: Some(true),
                legend_overlay: Some(true),
                ..Default::default()
            },
        )
        .expect("update overlays");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:title><c:tx>"#)
                && text.contains(r#"<c:overlay val="1"/></c:title>"#),
            "{text}"
        );
        assert!(
            text.contains(
                r#"<c:legend><c:legendPos val="r"/><c:layout/><c:overlay val="1"/></c:legend>"#
            ),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.title_overlay, Some(true));
        assert_eq!(parsed.legend_overlay, Some(true));
    }

    #[test]
    fn inserts_chart_overlays_before_formatting_children() {
        let xml = br#"<c:chartSpace xmlns:c="x" xmlns:a="y"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Title</a:t></a:r></a:p></c:rich></c:tx><c:layout/><c:extLst/></c:title><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:val><c:numRef><c:numCache><c:pt idx="0"><c:v>1</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea><c:legend><c:legendPos val="r"/><c:layout/><c:spPr/><c:txPr/></c:legend></c:chart></c:chartSpace>"#;
        let out = update_chart_xml(
            xml,
            &ChartXmlUpdate {
                title_overlay: Some(false),
                legend_overlay: Some(true),
                ..Default::default()
            },
        )
        .expect("insert overlays before formatting children");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.contains(r#"<c:layout/><c:overlay val="0"/><c:extLst/>"#),
            "{text}"
        );
        assert!(
            text.contains(r#"<c:layout/><c:overlay val="1"/><c:spPr/><c:txPr/>"#),
            "{text}"
        );

        let parsed = OoxmlChart::parse(text.as_bytes()).expect("parse edited chart");
        assert_eq!(parsed.title_overlay, Some(false));
        assert_eq!(parsed.legend_overlay, Some(true));
    }
}
