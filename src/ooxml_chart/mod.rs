//! OOXML 차트 (DrawingML) 파싱 및 SVG 렌더링
//!
//! HWP 파일 내 OLE 개체의 `OOXMLChartContents` 스트림 또는 HWPX `Chart/chartN.xml`은
//! Microsoft OOXML DrawingML 차트 XML로 저장된다. 이 모듈은 해당 XML을 파싱하여
//! 데이터 모델로 변환한 뒤, 네이티브 SVG 차트로 렌더링한다.
//!
//! ## 지원 범위
//! - `c:barChart` (세로/가로 막대)
//! - `c:lineChart` (꺾은선, stacked/percentStacked grouping semantic·렌더 근사)
//! - `c:pieChart`·`c:doughnutChart` (원형/도넛; 도넛은 원형 렌더 근사 + hole size semantic 보존)
//! - `c:scatterChart` (분산형; line 렌더러로 근사)
//! - `c:stockChart` (주식형; HLC/OHLC 기본 glyph, high-low line 및 up/down bar 스타일 렌더링, 데이터 캐시 편집)
//! - `c:bar3DChart`·`c:pie3DChart`·`c:ofPieChart` — **2D 근사 라우팅** (C1a #1453):
//!   3D막대→평면 막대, 3D원형/ofPie→단일 원형. ofPie 보조 플롯 타입/간격/크기와
//!   연결선 스타일은 semantic get/set으로 보존한다. 입체감·보조플롯 실제 분리 렌더링은
//!   미표현(후속 C2).
//! - **콤보 차트** (barChart + lineChart 혼합) — 시리즈별 타입 보존
//! - **이중 Y축** (primary + secondary) — 시리즈별 축 그룹 매핑
//!
//! ## 범위 외
//! - 3D 입체감·ofPie 보조플롯 분리 렌더링, 영역, 애니메이션, 세밀 스타일

pub mod edit;
pub mod parser;
pub mod renderer;

/// OOXML 차트 데이터 모델
#[derive(Debug, Clone, Default)]
pub struct OoxmlChart {
    /// 주 차트 타입 (콤보인 경우 첫 번째 plotType이 들어감; 렌더러는 시리즈별 타입 우선)
    pub chart_type: OoxmlChartType,
    pub title: Option<String>,
    pub series: Vec<OoxmlSeries>,
    pub categories: Vec<String>,
    /// 시리즈 중 하나라도 보조축을 쓰면 true
    pub has_secondary_axis: bool,
    /// 막대(bar/bar3D) 또는 라인(line) plot의 `c:grouping`
    /// (clustered/standard/stacked/percentStacked).
    pub grouping: BarGrouping,
    /// 막대 차트 항목 간 간격 (`c:barChart/c:gapWidth`, 보통 0..500).
    pub bar_gap_width: Option<u32>,
    /// 막대 차트 시리즈 겹침 (`c:barChart/c:overlap`, 보통 -100..100).
    pub bar_overlap: Option<i32>,
    /// 3D 막대 차트 깊이 간격 (`c:bar3DChart/c:gapDepth`, 보통 0..500).
    pub bar_3d_gap_depth: Option<u32>,
    /// 3D 막대 차트 모양 (`c:bar3DChart/c:shape`; box/cone/cylinder/pyramid 등).
    pub bar_3d_shape: Option<String>,
    /// 라인 차트 곡선 보간 여부 (`c:lineChart/c:ser/c:smooth` 또는 chart-level `c:smooth`).
    pub line_smooth: Option<bool>,
    /// 라인 차트 표식 표시 여부 (`c:lineChart/c:marker`).
    pub line_marker_visible: Option<bool>,
    /// 라인 차트 표식 크기 (`c:lineChart/c:ser/c:marker/c:size`, 보통 2..72).
    pub line_marker_size: Option<u32>,
    /// 라인 차트 표식 모양 (`c:lineChart/c:ser/c:marker/c:symbol`).
    pub line_marker_symbol: Option<ChartMarkerSymbol>,
    /// 라인 차트 표식 채움 색상 (`c:lineChart/c:ser/c:marker/c:spPr/a:solidFill`, RGB).
    pub line_marker_fill_color: Option<u32>,
    /// 라인 차트 표식 선 색상 (`c:lineChart/c:ser/c:marker/c:spPr/a:ln/...`, RGB).
    pub line_marker_line_color: Option<u32>,
    /// 라인 차트 표식 선 두께 (`c:lineChart/c:ser/c:marker/c:spPr/a:ln@w`, EMU).
    pub line_marker_line_width: Option<u32>,
    /// 원형 차트 첫 조각 시작 각도 (`c:pieChart/c:firstSliceAng`, 0..360).
    pub pie_first_slice_angle: Option<u16>,
    /// 원형 차트 조각 분리 정도 (`c:pieChart/c:ser/c:explosion`, 0..400).
    pub pie_explosion: Option<u32>,
    /// 원본 plot이 원형대원형/원형대막대 차트인지 여부 (`c:ofPieChart`).
    pub has_of_pie_chart: bool,
    /// 원본 plot이 도넛 차트인지 여부 (`c:doughnutChart`).
    pub has_doughnut_chart: bool,
    /// 도넛 차트 구멍 크기 (`c:doughnutChart/c:holeSize`, 보통 10..90).
    pub doughnut_hole_size: Option<u32>,
    /// 원형대원형/원형대막대 보조 플롯 타입 (`c:ofPieChart/c:ofPieType`).
    pub pie_of_pie_type: Option<OfPieType>,
    /// 원형대원형/원형대막대 간격 (`c:ofPieChart/c:gapWidth`, 보통 0..500).
    pub pie_of_pie_gap_width: Option<u32>,
    /// 원형대원형/원형대막대 보조 플롯 크기 (`c:ofPieChart/c:secondPieSize`, 보통 5..200).
    pub pie_of_pie_second_size: Option<u32>,
    /// 원형대원형/원형대막대 연결선 색상 (`c:ofPieChart/c:serLines/c:spPr/a:ln/...`, RGB).
    pub pie_of_pie_ser_line_color: Option<u32>,
    /// 원형대원형/원형대막대 연결선 두께 (`c:ofPieChart/c:serLines/c:spPr/a:ln@w`, EMU).
    pub pie_of_pie_ser_line_width: Option<u32>,
    /// 분산형 차트 스타일 (`c:scatterChart/c:scatterStyle`).
    pub scatter_style: Option<ScatterStyle>,
    /// 분산형 차트 곡선 보간 여부 (`c:scatterChart/c:ser/c:smooth`).
    pub scatter_smooth: Option<bool>,
    /// 분산형 차트 표식 크기 (`c:scatterChart/c:ser/c:marker/c:size`, 보통 2..72).
    pub scatter_marker_size: Option<u32>,
    /// 분산형 차트 표식 모양 (`c:scatterChart/c:ser/c:marker/c:symbol`).
    pub scatter_marker_symbol: Option<ChartMarkerSymbol>,
    /// 분산형 차트 표식 채움 색상 (`c:scatterChart/c:ser/c:marker/c:spPr/a:solidFill`, RGB).
    pub scatter_marker_fill_color: Option<u32>,
    /// 분산형 차트 표식 선 색상 (`c:scatterChart/c:ser/c:marker/c:spPr/a:ln/...`, RGB).
    pub scatter_marker_line_color: Option<u32>,
    /// 분산형 차트 표식 선 두께 (`c:scatterChart/c:ser/c:marker/c:spPr/a:ln@w`, EMU).
    pub scatter_marker_line_width: Option<u32>,
    /// 시리즈 추세선 종류 (`c:ser/c:trendline/c:trendlineType`).
    pub trendline_type: Option<ChartTrendlineType>,
    /// 다항식 추세선 차수 (`c:ser/c:trendline/c:order`, 보통 2..6).
    pub trendline_order: Option<u32>,
    /// 이동 평균 추세선 기간 (`c:ser/c:trendline/c:period`, 보통 2..255).
    pub trendline_period: Option<u32>,
    /// 시리즈 추세선 수식 표시 여부 (`c:ser/c:trendline/c:dispEq`).
    pub trendline_display_equation: Option<bool>,
    /// 시리즈 추세선 R 제곱값 표시 여부 (`c:ser/c:trendline/c:dispRSqr`).
    pub trendline_display_r_squared: Option<bool>,
    /// 시리즈 추세선 선 색상 (`c:ser/c:trendline/c:spPr/a:ln/a:solidFill/a:srgbClr`, RGB).
    pub trendline_line_color: Option<u32>,
    /// 시리즈 추세선 선 두께 (`c:ser/c:trendline/c:spPr/a:ln@w`, EMU).
    pub trendline_line_width: Option<u32>,
    /// 오차 막대 방향 (`c:ser/c:errBars/c:errDir`).
    pub error_bar_direction: Option<ChartErrorBarDirection>,
    /// 오차 막대 표시 유형 (`c:ser/c:errBars/c:errBarType`).
    pub error_bar_type: Option<ChartErrorBarType>,
    /// 오차 막대 값 유형 (`c:ser/c:errBars/c:errValType`).
    pub error_bar_value_type: Option<ChartErrorBarValueType>,
    /// 오차 막대 값 (`c:ser/c:errBars/c:val`).
    pub error_bar_value: Option<f64>,
    /// 오차 막대 end cap 숨김 여부 (`c:ser/c:errBars/c:noEndCap`).
    pub error_bar_no_end_cap: Option<bool>,
    /// 오차 막대 선 색상 (`c:ser/c:errBars/c:spPr/a:ln/a:solidFill/a:srgbClr`, RGB).
    pub error_bar_line_color: Option<u32>,
    /// 오차 막대 선 두께 (`c:ser/c:errBars/c:spPr/a:ln@w`, EMU).
    pub error_bar_line_width: Option<u32>,
    /// 주식형 차트 up/down bar gap width (`c:upDownBars/c:gapWidth`, 보통 0..500).
    pub stock_up_down_bar_gap_width: Option<u32>,
    /// 주식형 상승 bar 채움 색상 (`c:upBars/c:spPr/a:solidFill/a:srgbClr`, RGB).
    pub stock_up_bar_fill_color: Option<u32>,
    /// 주식형 하락 bar 채움 색상 (`c:downBars/c:spPr/a:solidFill/a:srgbClr`, RGB).
    pub stock_down_bar_fill_color: Option<u32>,
    /// 주식형 상승 bar 선 색상 (`c:upBars/c:spPr/a:ln/a:solidFill/a:srgbClr`, RGB).
    pub stock_up_bar_line_color: Option<u32>,
    /// 주식형 하락 bar 선 색상 (`c:downBars/c:spPr/a:ln/a:solidFill/a:srgbClr`, RGB).
    pub stock_down_bar_line_color: Option<u32>,
    /// 주식형 상승 bar 선 두께 (`c:upBars/c:spPr/a:ln@w`, EMU).
    pub stock_up_bar_line_width: Option<u32>,
    /// 주식형 하락 bar 선 두께 (`c:downBars/c:spPr/a:ln@w`, EMU).
    pub stock_down_bar_line_width: Option<u32>,
    /// 주식형 high-low line 색상 (`c:hiLowLines/c:spPr/a:ln/a:solidFill/a:srgbClr`, RGB).
    pub stock_hi_low_line_color: Option<u32>,
    /// 주식형 high-low line 두께 (`c:hiLowLines/c:spPr/a:ln@w`, EMU).
    pub stock_hi_low_line_width: Option<u32>,
    /// 데이터 레이블 위치 (`c:dLbls/c:dLblPos`).
    pub data_label_position: Option<ChartDataLabelPosition>,
    /// 데이터 레이블 값 표시 여부 (`c:dLbls/c:showVal`).
    pub data_labels_show_value: Option<bool>,
    /// 데이터 레이블 카테고리명 표시 여부 (`c:dLbls/c:showCatName`).
    pub data_labels_show_category_name: Option<bool>,
    /// 데이터 레이블 시리즈명 표시 여부 (`c:dLbls/c:showSerName`).
    pub data_labels_show_series_name: Option<bool>,
    /// 데이터 레이블 백분율 표시 여부 (`c:dLbls/c:showPercent`).
    pub data_labels_show_percent: Option<bool>,
    /// 데이터 레이블 범례 키 표시 여부 (`c:dLbls/c:showLegendKey`).
    pub data_labels_show_legend_key: Option<bool>,
    /// 차트 제목이 플롯 영역 위에 겹쳐 표시되는지 여부 (`c:title/c:overlay`).
    pub title_overlay: Option<bool>,
    /// 차트가 1904 날짜 시스템을 사용하는지 여부 (`c:chartSpace/c:date1904`).
    pub date_1904: Option<bool>,
    /// 차트 스타일 번호 (`c:chartSpace/c:style`; c14 AlternateContent 값은 보통 +100).
    pub chart_style: Option<u32>,
    /// 차트 영역 채움 색상 (`c:chartSpace/c:spPr/a:solidFill/a:srgbClr`, RGB).
    pub chart_area_fill_color: Option<u32>,
    /// 플롯 영역 채움 색상 (`c:plotArea/c:spPr/a:solidFill/a:srgbClr`, RGB).
    pub plot_area_fill_color: Option<u32>,
    /// 차트 영역 모서리를 둥글게 표시하는지 여부 (`c:chartSpace/c:roundedCorners`).
    pub rounded_corners: Option<bool>,
    /// 자동 생성된 차트 제목이 삭제되었는지 여부 (`c:chart/c:autoTitleDeleted`).
    pub auto_title_deleted: Option<bool>,
    /// 같은 plot의 데이터 요소별 색상을 다르게 적용하는지 여부 (`c:*Chart/c:varyColors`).
    pub vary_colors: Option<bool>,
    /// 3D 차트 X축 회전 각도 (`c:view3D/c:rotX`).
    pub view_3d_rotation_x: Option<i32>,
    /// 3D 차트 Y축 회전 각도 (`c:view3D/c:rotY`).
    pub view_3d_rotation_y: Option<i32>,
    /// 3D 차트 원근 값 (`c:view3D/c:perspective`).
    pub view_3d_perspective: Option<u32>,
    /// 3D 차트 직각축 사용 여부 (`c:view3D/c:rAngAx`).
    pub view_3d_right_angle_axes: Option<bool>,
    /// 3D 차트 높이 비율 (`c:view3D/c:hPercent`).
    pub view_3d_height_percent: Option<u32>,
    /// 3D 차트 깊이 비율 (`c:view3D/c:depthPercent`).
    pub view_3d_depth_percent: Option<u32>,
    /// 빈 데이터 표시 방식 (`c:dispBlanksAs`).
    pub display_blanks_as: Option<ChartDisplayBlanksAs>,
    /// 숨겨진 행/열 데이터를 차트에 표시할지 여부 (`c:showHiddenData`).
    pub show_hidden_data: Option<bool>,
    /// 표시된 셀만 차트에 그릴지 여부 (`c:plotVisOnly`).
    pub plot_visible_only: Option<bool>,
    /// 차트 데이터 테이블 가로 테두리 표시 여부 (`c:plotArea/c:dTable/c:showHorzBorder`).
    pub data_table_show_horizontal_border: Option<bool>,
    /// 차트 데이터 테이블 세로 테두리 표시 여부 (`c:plotArea/c:dTable/c:showVertBorder`).
    pub data_table_show_vertical_border: Option<bool>,
    /// 차트 데이터 테이블 외곽선 표시 여부 (`c:plotArea/c:dTable/c:showOutline`).
    pub data_table_show_outline: Option<bool>,
    /// 차트 데이터 테이블 범례 키 표시 여부 (`c:plotArea/c:dTable/c:showKeys`).
    pub data_table_show_keys: Option<bool>,
    /// 범례 위치 (`c:legend/c:legendPos`).
    pub legend_position: Option<ChartLegendPosition>,
    /// 범례가 플롯 영역 위에 겹쳐 표시되는지 여부 (`c:legend/c:overlay`).
    pub legend_overlay: Option<bool>,
    /// 카테고리 축 표시 여부 (`c:catAx/c:delete`, `val="1"`이면 숨김).
    pub category_axis_visible: Option<bool>,
    /// 값 축 표시 여부 (`c:valAx/c:delete`, `val="1"`이면 숨김).
    pub value_axis_visible: Option<bool>,
    /// 카테고리 축 제목 (`c:catAx/c:title`).
    pub category_axis_title: Option<String>,
    /// 값 축 제목 (`c:valAx/c:title`).
    pub value_axis_title: Option<String>,
    /// 카테고리 축 위치 (`c:catAx/c:axPos`).
    pub category_axis_position: Option<AxisPosition>,
    /// 값 축 위치 (`c:valAx/c:axPos`).
    pub value_axis_position: Option<AxisPosition>,
    /// 카테고리 축 눈금 라벨 위치 (`c:catAx/c:tickLblPos`).
    pub category_axis_label_position: Option<AxisLabelPosition>,
    /// 값 축 눈금 라벨 위치 (`c:valAx/c:tickLblPos`).
    pub value_axis_label_position: Option<AxisLabelPosition>,
    /// 카테고리 축 자동 설정 여부 (`c:catAx/c:auto`).
    pub category_axis_auto: Option<bool>,
    /// 카테고리 축 라벨 정렬 (`c:catAx/c:lblAlgn`).
    pub category_axis_label_alignment: Option<AxisLabelAlignment>,
    /// 카테고리 축 라벨 offset (`c:catAx/c:lblOffset`).
    pub category_axis_label_offset: Option<u32>,
    /// 카테고리 축 tick mark skip (`c:catAx/c:tickMarkSkip`).
    pub category_axis_tick_mark_skip: Option<u32>,
    /// 카테고리 축 다중 레벨 라벨 비활성 여부 (`c:catAx/c:noMultiLvlLbl`).
    pub category_axis_no_multi_level_labels: Option<bool>,
    /// 카테고리 축 값 방향 (`c:catAx/c:scaling/c:orientation`).
    pub category_axis_orientation: Option<AxisOrientation>,
    /// 값 축 값 방향 (`c:valAx/c:scaling/c:orientation`).
    pub value_axis_orientation: Option<AxisOrientation>,
    /// 카테고리 축 교차 위치 (`c:catAx/c:crosses`).
    pub category_axis_crosses: Option<AxisCrosses>,
    /// 카테고리 축 수치 교차값 (`c:catAx/c:crossesAt`).
    pub category_axis_crosses_at: Option<f64>,
    /// 값 축 교차 위치 (`c:valAx/c:crosses`).
    pub value_axis_crosses: Option<AxisCrosses>,
    /// 값 축 수치 교차값 (`c:valAx/c:crossesAt`).
    pub value_axis_crosses_at: Option<f64>,
    /// 값 축 막대/카테고리 교차 위치 (`c:valAx/c:crossBetween`).
    pub value_axis_cross_between: Option<AxisCrossBetween>,
    /// 카테고리 축 주 눈금 표시 방식 (`c:catAx/c:majorTickMark`).
    pub category_axis_major_tick_mark: Option<AxisTickMark>,
    /// 카테고리 축 보조 눈금 표시 방식 (`c:catAx/c:minorTickMark`).
    pub category_axis_minor_tick_mark: Option<AxisTickMark>,
    /// 카테고리 축 선 색상 (`c:catAx/c:spPr/a:ln/a:solidFill/a:srgbClr`, RGB).
    pub category_axis_line_color: Option<u32>,
    /// 카테고리 축 선 두께 (`c:catAx/c:spPr/a:ln@w`, EMU).
    pub category_axis_line_width: Option<u32>,
    /// 카테고리 축 주 gridline 선 색상 (`c:catAx/c:majorGridlines/c:spPr/a:ln/...`, RGB).
    pub category_axis_major_grid_line_color: Option<u32>,
    /// 카테고리 축 주 gridline 선 두께 (`c:catAx/c:majorGridlines/c:spPr/a:ln@w`, EMU).
    pub category_axis_major_grid_line_width: Option<u32>,
    /// 카테고리 축 보조 gridline 선 색상 (`c:catAx/c:minorGridlines/c:spPr/a:ln/...`, RGB).
    pub category_axis_minor_grid_line_color: Option<u32>,
    /// 카테고리 축 보조 gridline 선 두께 (`c:catAx/c:minorGridlines/c:spPr/a:ln@w`, EMU).
    pub category_axis_minor_grid_line_width: Option<u32>,
    /// 값 축 주 눈금 표시 방식 (`c:valAx/c:majorTickMark`).
    pub value_axis_major_tick_mark: Option<AxisTickMark>,
    /// 값 축 보조 눈금 표시 방식 (`c:valAx/c:minorTickMark`).
    pub value_axis_minor_tick_mark: Option<AxisTickMark>,
    /// 값 축 선 색상 (`c:valAx/c:spPr/a:ln/a:solidFill/a:srgbClr`, RGB).
    pub value_axis_line_color: Option<u32>,
    /// 값 축 선 두께 (`c:valAx/c:spPr/a:ln@w`, EMU).
    pub value_axis_line_width: Option<u32>,
    /// 값 축 주 gridline 선 색상 (`c:valAx/c:majorGridlines/c:spPr/a:ln/...`, RGB).
    pub value_axis_major_grid_line_color: Option<u32>,
    /// 값 축 주 gridline 선 두께 (`c:valAx/c:majorGridlines/c:spPr/a:ln@w`, EMU).
    pub value_axis_major_grid_line_width: Option<u32>,
    /// 값 축 보조 gridline 선 색상 (`c:valAx/c:minorGridlines/c:spPr/a:ln/...`, RGB).
    pub value_axis_minor_grid_line_color: Option<u32>,
    /// 값 축 보조 gridline 선 두께 (`c:valAx/c:minorGridlines/c:spPr/a:ln@w`, EMU).
    pub value_axis_minor_grid_line_width: Option<u32>,
    /// 값 축 로그 눈금 밑 (`c:valAx/c:scaling/c:logBase`).
    pub value_axis_log_base: Option<f64>,
    /// 값 축 표시 단위 (`c:valAx/c:dispUnits/c:builtInUnit`).
    pub value_axis_display_unit: Option<AxisDisplayUnit>,
    /// 값 축 최소값 (`c:valAx/c:scaling/c:min`).
    pub value_axis_minimum: Option<f64>,
    /// 값 축 최대값 (`c:valAx/c:scaling/c:max`).
    pub value_axis_maximum: Option<f64>,
    /// 값 축 주 눈금 단위 (`c:valAx/c:majorUnit`).
    pub value_axis_major_unit: Option<f64>,
    /// 값 축 보조 눈금 단위 (`c:valAx/c:minorUnit`).
    pub value_axis_minor_unit: Option<f64>,
    /// 카테고리 축 숫자/날짜 표시 형식 (`c:catAx/c:numFmt/@formatCode`).
    pub category_axis_number_format: Option<String>,
    /// 카테고리 축 숫자/날짜 표시 형식의 원본 연결 여부 (`c:catAx/c:numFmt/@sourceLinked`).
    pub category_axis_number_format_source_linked: Option<bool>,
    /// 값 축 숫자 표시 형식 (`c:valAx/c:numFmt/@formatCode`).
    pub value_axis_number_format: Option<String>,
    /// 값 축 숫자 표시 형식의 원본 연결 여부 (`c:valAx/c:numFmt/@sourceLinked`).
    pub value_axis_number_format_source_linked: Option<bool>,
}

/// 막대/라인 차트 그룹화 방식 (`c:grouping`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BarGrouping {
    /// 묶은(side-by-side). `clustered`/`standard` 흡수.
    #[default]
    Clustered,
    /// 누적 (시리즈를 카테고리별로 쌓음).
    Stacked,
    /// 백분율 누적 (카테고리 합을 100%로 정규화).
    PercentStacked,
}

/// 차트 범례 위치 (`c:legendPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartLegendPosition {
    Right,
    Left,
    Top,
    Bottom,
    TopRight,
}

/// 축 위치 (`c:axPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisPosition {
    Bottom,
    Left,
    Top,
    Right,
}

/// 축 눈금 라벨 위치 (`c:tickLblPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisLabelPosition {
    NextTo,
    High,
    Low,
    None,
}

/// 카테고리 축 라벨 정렬 (`c:lblAlgn`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisLabelAlignment {
    Center,
    Left,
    Right,
}

/// 축 값 방향 (`c:scaling/c:orientation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisOrientation {
    MinMax,
    MaxMin,
}

/// 축 교차 위치 (`c:crosses`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisCrosses {
    AutoZero,
    Min,
    Max,
}

/// 값 축 막대/카테고리 교차 위치 (`c:crossBetween`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisCrossBetween {
    Between,
    MidCategory,
}

/// 축 눈금 표시 방식 (`c:majorTickMark`, `c:minorTickMark`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisTickMark {
    Cross,
    In,
    Out,
    None,
}

/// 값 축 표시 단위 (`c:valAx/c:dispUnits/c:builtInUnit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisDisplayUnit {
    Hundreds,
    Thousands,
    TenThousands,
    HundredThousands,
    Millions,
    TenMillions,
    HundredMillions,
    Billions,
    Trillions,
}

/// 차트 데이터 레이블 위치 (`c:dLblPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartDataLabelPosition {
    BestFit,
    Bottom,
    Center,
    InsideBase,
    InsideEnd,
    Left,
    OutsideEnd,
    Right,
    Top,
}

/// 빈 데이터 표시 방식 (`c:dispBlanksAs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartDisplayBlanksAs {
    Gap,
    Span,
    Zero,
}

/// 분산형 차트 스타일 (`c:scatterStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScatterStyle {
    Line,
    LineMarker,
    Marker,
    Smooth,
    SmoothMarker,
}

/// 라인/분산형 차트 표식 모양 (`c:marker/c:symbol`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartMarkerSymbol {
    Circle,
    Dash,
    Diamond,
    Dot,
    None,
    Picture,
    Plus,
    Square,
    Star,
    Triangle,
    X,
}

/// 시리즈 추세선 종류 (`c:trendlineType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartTrendlineType {
    Linear,
    Exponential,
    Logarithmic,
    MovingAverage,
    Polynomial,
    Power,
}

/// 차트 오차 막대 방향 (`c:errDir`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartErrorBarDirection {
    X,
    Y,
}

/// 차트 오차 막대 표시 유형 (`c:errBarType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartErrorBarType {
    Both,
    Plus,
    Minus,
}

/// 차트 오차 막대 값 유형 (`c:errValType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartErrorBarValueType {
    FixedValue,
    Percentage,
    StandardDeviation,
    StandardError,
}

/// 원형대원형/원형대막대 보조 플롯 종류 (`c:ofPieType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfPieType {
    Pie,
    Bar,
}

/// 차트 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OoxmlChartType {
    /// 세로 막대 (barDir=col)
    Column,
    /// 가로 막대 (barDir=bar)
    Bar,
    /// 꺾은선
    Line,
    /// 원형
    Pie,
    /// 분산형
    Scatter,
    /// 주식형
    Stock,
    #[default]
    Unknown,
}

impl OoxmlChartType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Column => "세로 막대",
            Self::Bar => "가로 막대",
            Self::Line => "꺾은선",
            Self::Pie => "원형",
            Self::Scatter => "분산형",
            Self::Stock => "주식형",
            Self::Unknown => "미지원",
        }
    }
}

/// 데이터 시리즈 (막대 한 묶음 또는 선 하나)
#[derive(Debug, Clone, Default)]
pub struct OoxmlSeries {
    pub name: String,
    pub values: Vec<f64>,
    /// RGB 색상 (`0xRRGGBB`), 파서가 확정 못하면 None (렌더러가 기본 팔레트 적용)
    pub color: Option<u32>,
    /// 시리즈 선 색상 (`c:ser/c:spPr/a:ln/a:solidFill/a:srgbClr`, RGB).
    pub line_color: Option<u32>,
    /// 시리즈 선 두께 (`c:ser/c:spPr/a:ln@w`, EMU).
    pub line_width: Option<u32>,
    /// 시리즈 본인의 차트 타입 (콤보 차트에서 바/라인 구분용)
    pub series_type: OoxmlChartType,
    /// 이 시리즈가 속한 플롯의 c:axId 값 목록 (parser 내부에서 axis 분류에 사용)
    pub axis_ids: Vec<String>,
    /// 0 = 기본축(왼쪽/아래), 1 = 보조축(오른쪽/위)
    pub axis_group: u8,
    /// 숫자 포맷 코드 (예: "#,##0")
    pub format_code: Option<String>,
}

impl OoxmlChart {
    /// 파싱 입력: OOXMLChartContents 원본 바이트 (UTF-8 XML)
    pub fn parse(xml: &[u8]) -> Option<Self> {
        parser::parse_chart_xml(xml)
    }

    /// 주어진 영역에 SVG 조각으로 렌더링한다.
    /// 반환값은 `<g>...</g>` 또는 여러 요소로 구성된 SVG 문자열 조각.
    pub fn render_svg(&self, x: f64, y: f64, w: f64, h: f64) -> String {
        renderer::render_chart_svg(self, x, y, w, h)
    }

    /// 시리즈가 여러 타입을 섞어 쓰는지 (콤보 차트) 여부
    pub fn is_combo(&self) -> bool {
        let mut types: std::collections::HashSet<OoxmlChartType> = std::collections::HashSet::new();
        for s in &self.series {
            if s.series_type != OoxmlChartType::Unknown {
                types.insert(s.series_type);
            }
        }
        types.len() > 1
    }
}
