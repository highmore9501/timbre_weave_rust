//! 主窗口与全局状态

use crate::midi::reader::{TrackInfo, load_midi_file, scan_tracks};
use crate::split::eval::{EvaluationReport, evaluate_tracks};
use crate::split::instrument::{EnsembleSlot, INSTRUMENT_POOL, assign_channel};
use crate::split::report::SplitReport;
use crate::ui::midi_panel::Quantization;
use crate::ui::theme::{Colors, ThemeManager};
use eframe::egui;

/// 一次分轨需要的全部输入
#[derive(Debug, Clone)]
pub struct SplitInputData {
    /// 已选择的 midi 文件完整路径
    pub selected_midi_path: Option<String>,
    /// 作为源轨的轨道下标
    pub source_tracks: Vec<usize>,
    /// 使用通道（-1 = 全部通道）
    pub channel_filter: i32,
    /// 起始 tick 量化档位（决定"多近算同时发声"）
    pub quantization: Quantization,
    /// 候选池大小（越大越接近穷举，代价是更慢）
    pub pool_size: usize,
    /// 本次编制
    pub ensemble: Vec<EnsembleSlot>,
}

impl Default for SplitInputData {
    fn default() -> Self {
        Self {
            selected_midi_path: None,
            source_tracks: Vec::new(),
            channel_filter: -1,
            quantization: Quantization::OneSixteenth,
            pool_size: crate::split::assign::DEFAULT_POOL_SIZE,
            ensemble: Vec::new(),
        }
    }
}

/// 应用主结构
pub struct TimbreWeaveApp {
    /// 分轨输入
    pub data: SplitInputData,
    /// 扫描到的轨道摘要
    pub scanned_tracks: Vec<TrackInfo>,
    /// 最近一次源轨评估结果（勾选轨道后点"评估"得到）
    pub evaluation: Option<EvaluationReport>,
    /// 最近一次分轨报告
    pub report: Option<SplitReport>,
    /// 提示信息（成功 / 说明）
    pub info_message: Option<String>,
    /// 错误信息
    pub error_message: Option<String>,
    /// 非致命警告（乐器池数据缺失、编制超限等）
    pub warnings: Vec<String>,
    /// 主题管理器
    pub theme: ThemeManager,
}

impl Default for TimbreWeaveApp {
    fn default() -> Self {
        Self {
            data: SplitInputData::default(),
            scanned_tracks: Vec::new(),
            evaluation: None,
            report: None,
            info_message: None,
            error_message: None,
            warnings: Vec::new(),
            theme: ThemeManager::load_from_config(),
        }
    }
}

impl TimbreWeaveApp {
    /// 当前主题配色
    pub fn colors(&self) -> Colors {
        self.theme.get_colors()
    }

    /// 打开文件对话框选择 midi
    pub fn pick_midi_file(&mut self) {
        let picked = rfd::FileDialog::new()
            .add_filter("MIDI", &["mid", "midi"])
            .pick_file();
        if let Some(path) = picked {
            self.data.selected_midi_path = Some(path.to_string_lossy().to_string());
            self.scanned_tracks.clear();
            self.data.source_tracks.clear();
            self.evaluation = None;
            self.error_message = None;
            self.info_message = None;
        }
    }

    /// 扫描所选 midi 的全部轨道
    pub fn scan_tracks(&mut self) {
        self.scanned_tracks.clear();
        self.evaluation = None;
        self.error_message = None;
        self.info_message = None;

        let Some(path) = self.data.selected_midi_path.clone() else {
            self.error_message = Some(crate::t!("msg.no_file_selected"));
            return;
        };

        match load_midi_file(&path) {
            Ok(loaded) => {
                self.scanned_tracks = scan_tracks(loaded.smf(), self.data.channel_filter);
                self.info_message = Some(crate::t!(
                    "msg.scan_done",
                    self.scanned_tracks.len()
                ));
            }
            Err(message) => {
                self.error_message = Some(message);
            }
        }
    }

    /// 评估当前勾选的源轨：音符总数、最大同时发声数、最低音与最高音
    ///
    /// 只读不写：不产生任何文件，也不改动 [`SplitInputData`] 里的任何选择。
    pub fn evaluate_source_tracks(&mut self) {
        self.evaluation = None;
        self.error_message = None;
        self.info_message = None;

        let Some(path) = self.data.selected_midi_path.clone() else {
            self.error_message = Some(crate::t!("msg.no_file_selected"));
            return;
        };
        if self.data.source_tracks.is_empty() {
            self.error_message = Some(crate::t!("msg.no_track_selected"));
            return;
        }

        let loaded = match load_midi_file(&path) {
            Ok(loaded) => loaded,
            Err(message) => {
                self.error_message = Some(message);
                return;
            }
        };
        // 同时发声的口径与分轨一致：用当前量化档位换算出起始 tick 容差
        let tolerance = self.data.quantization.onset_tolerance(loaded.ticks_per_beat);

        match evaluate_tracks(
            &loaded,
            &self.data.source_tracks,
            self.data.channel_filter,
            tolerance,
        ) {
            Ok(report) => {
                crate::log_info!(crate::t!(
                    "log.eval.done",
                    report.note_count,
                    report.max_onset_notes,
                    report.overlap_peak
                ));
                self.evaluation = Some(report);
            }
            Err(message) => {
                self.error_message = Some(message);
            }
        }
    }

    /// 编制变动后重排同名序号与输出通道
    pub fn reindex_ensemble(&mut self) {
        let mut ordinal_by_spec: Vec<usize> = vec![0; INSTRUMENT_POOL.len()];
        for (slot_position, slot) in self.data.ensemble.iter_mut().enumerate() {
            slot.ordinal = ordinal_by_spec[slot.spec_index];
            ordinal_by_spec[slot.spec_index] += 1;
            slot.channel = assign_channel(slot_position);
        }
    }
}

impl eframe::App for TimbreWeaveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 主题每次启动与切换时应用；这里按需重设，保证窗口重建后样式仍在
        self.theme.apply_theme(ctx);

        egui::TopBottomPanel::top("top_panel")
            .frame(
                egui::Frame::new()
                    .fill(self.colors().window_fill)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::t!("app.title"))
                            .heading()
                            .strong()
                            .color(self.colors().title_color),
                    );
                    ui.label(
                        egui::RichText::new(crate::t!("app.subtitle"))
                            .small()
                            .color(self.colors().text_secondary),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // 先把配色取出来（Copy 类型），避免与下面的 &mut self.theme 撞借用
                        let top_bar_colors = self.colors();
                        show_language_menu(ui, &top_bar_colors);
                        show_theme_toggle(ui, &mut self.theme, ctx, &top_bar_colors);
                    });
                });
            });

        // 三栏分栏：左=源 midi，中=编制，右=执行与报告
        //
        // 三栏**等宽**（各占可用宽度的 1/3）。默认布局里左右是固定宽度的 SidePanel、中间是
        // CentralPanel，窗口拉大时多出来的宽度全归中间栏，左右两栏纹丝不动；所以这里先算后画：
        // 拿这一帧的可用宽度除以三当目标栏宽，三栏都按这个数排布。
        //
        // 宽度口径（见 egui 的 `panel.rs` / `frame.rs`）：
        // - `SidePanel::exact_width` 是**外框宽度**，内容区要再让出内边距与描边
        //   （`frame.total_margin()`）；
        // - 中间栏只能在它那整块画布上把**内容宽度**钉住，且要让出同样一份外框，
        //   这样三栏的内容区才真正等宽（栏间是面板背景色，外框那几像素看不出来，
        //   但内容区一旦不等宽，卡片右边就会差出一大截）。
        let colors = self.colors();
        // 面板外框左右各占的宽度：内边距 + 描边（数值与 [`panel_frame`] 一一对应）
        const PANEL_FRAME_MARGIN: f32 = 1.0 + 10.0;
        // 窄窗口兜底：再挤就没法看了，此时三栏按这个下限排成一条横向长条（超出部分看不见）
        const MIN_COLUMN_WIDTH: f32 = 300.0;

        let available_width = ctx.available_rect().width();
        let column_width = (available_width / 3.0).max(MIN_COLUMN_WIDTH);
        // 中间栏「内容」的目标宽度：让出与左右两栏相同的外框
        let central_content_width = (column_width - PANEL_FRAME_MARGIN * 2.0).max(0.0);

        egui::SidePanel::left("source_panel")
            .resizable(false)
            .exact_width(column_width)
            .frame(panel_frame(colors.panel_fill))
            .show(ctx, |ui| {
                // 内容超出栏宽时裁切而不是撑开栏宽
                ui.set_clip_rect(ui.max_rect());
                self.show_midi_panel(ui, &colors);
            });

        egui::SidePanel::right("report_panel")
            .resizable(false)
            .exact_width(column_width)
            .frame(panel_frame(colors.panel_fill))
            .show(ctx, |ui| {
                ui.set_clip_rect(ui.max_rect());
                self.show_report_panel(ui, &colors);
            });

        egui::CentralPanel::default()
            .frame(panel_frame(colors.panel_fill))
            .show(ctx, |ui| {
                // 把内容宽度钉在等宽口径上（`set_width` = 同时设最小与最大宽度）
                ui.set_width(central_content_width);
                self.show_ensemble_panel(ui, &colors);
            });
    }
}

/// 三栏面板的统一外框：同一份背景色 + 同一套留白
///
/// 三个面板必须用**同一个**外框，否则栏与栏之间的视觉分界会漂移；
/// `update` 里的常量 `PANEL_FRAME_MARGIN` 就是这份外框左右各占的宽度。
fn panel_frame(fill: egui::Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .inner_margin(egui::Margin::same(10))
}

/// 顶栏主题切换按钮
fn show_theme_toggle(
    ui: &mut egui::Ui,
    theme: &mut ThemeManager,
    ctx: &egui::Context,
    colors: &Colors,
) {
    let next_theme = theme.theme_type.toggled();
    let label = format!(
        "{} {}",
        crate::t!("ui.theme.label"),
        crate::t!(next_theme.i18n_key())
    );
    if ui
        .button(egui::RichText::new(label).color(colors.accent))
        .on_hover_text(crate::t!("ui.theme.hint"))
        .clicked()
    {
        theme.toggle(ctx);
    }
}

/// 顶栏语言菜单
fn show_language_menu(ui: &mut egui::Ui, colors: &Colors) {
    let current = crate::i18n::current_setting();
    let selected_text = if current == crate::i18n::SYSTEM_SETTING {
        format!(
            "{} ({})",
            crate::t!("ui.language.follow_system"),
            crate::i18n::current_language()
        )
    } else {
        crate::i18n::current_language()
    };

    egui::ComboBox::from_id_salt("language_menu")
        .selected_text(egui::RichText::new(selected_text).color(colors.text_primary))
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(
                    current == crate::i18n::SYSTEM_SETTING,
                    crate::t!("ui.language.follow_system"),
                )
                .clicked()
            {
                crate::i18n::set_follow_system();
                ui.ctx().request_repaint();
            }
            for meta in crate::i18n::available_languages() {
                let selected = current == meta.code;
                if ui.selectable_label(selected, &meta.native_name).clicked() {
                    crate::i18n::set_language(&meta.code);
                    ui.ctx().request_repaint();
                }
            }
        });
}
