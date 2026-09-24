//! 主窗口与全局状态

use crate::midi::reader::{TrackInfo, load_midi_file, scan_tracks};
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
            self.error_message = None;
            self.info_message = None;
        }
    }

    /// 扫描所选 midi 的全部轨道
    pub fn scan_tracks(&mut self) {
        self.scanned_tracks.clear();
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
        // 左右两栏用**固定宽度（exact_width）+ 禁止拖拽**：报告里有长不可断的字符串
        // （输出文件全路径、共用通道清单），如果让 SidePanel 按内容自适应宽度，
        // 右栏会被撑宽并把中间栏挤没。固定宽度后超长文本由换行/裁切处理。
        let colors = self.colors();
        let side_panel_frame = |fill: egui::Color32| {
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::same(10))
        };

        egui::SidePanel::left("source_panel")
            .resizable(false)
            .exact_width(380.0)
            .frame(side_panel_frame(colors.panel_fill))
            .show(ctx, |ui| {
                // 内容超出栏宽时裁切而不是撑开栏宽
                ui.set_clip_rect(ui.max_rect());
                self.show_midi_panel(ui, &colors);
            });

        egui::SidePanel::right("report_panel")
            .resizable(false)
            .exact_width(440.0)
            .frame(side_panel_frame(colors.panel_fill))
            .show(ctx, |ui| {
                ui.set_clip_rect(ui.max_rect());
                self.show_report_panel(ui, &colors);
            });

        egui::CentralPanel::default()
            .frame(side_panel_frame(colors.panel_fill))
            .show(ctx, |ui| self.show_ensemble_panel(ui, &colors));
    }
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
