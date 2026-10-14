//! 源 midi 面板：选文件、扫描轨道、勾选源轨、量化与通道

use crate::ui::app::TimbreWeaveApp;
use crate::ui::theme::Colors;
use crate::ui::widgets;
use eframe::egui;

/// 起始 tick 量化档位（用于判定"同时发声"）
///
/// 取"每拍 tick 数 × 分数"作为容差；与旧项目动画解析用的是同一个用户可见参数口径。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Quantization {
    /// 不量化：要求起始 tick 完全相同
    None,
    OneQuarter,
    OneEighth,
    OneSixteenth,
    OneThirtySecond,
}

impl Quantization {
    /// 全部可选档位（顺序即下拉菜单显示顺序）
    pub const ALL: [Quantization; 5] = [
        Quantization::None,
        Quantization::OneQuarter,
        Quantization::OneEighth,
        Quantization::OneSixteenth,
        Quantization::OneThirtySecond,
    ];

    /// 相对每拍的分数（`None` = 不量化）
    pub fn to_fraction(&self) -> Option<f64> {
        match self {
            Quantization::None => None,
            Quantization::OneQuarter => Some(0.25),
            Quantization::OneEighth => Some(0.125),
            Quantization::OneSixteenth => Some(0.0625),
            Quantization::OneThirtySecond => Some(0.03125),
        }
    }

    /// 显示文本
    pub fn display_text(&self) -> String {
        match self {
            Quantization::None => crate::t!("ui.midi.quantization.none"),
            Quantization::OneQuarter => crate::t!("ui.midi.quantization.quarter"),
            Quantization::OneEighth => crate::t!("ui.midi.quantization.eighth"),
            Quantization::OneSixteenth => crate::t!("ui.midi.quantization.sixteenth"),
            Quantization::OneThirtySecond => crate::t!("ui.midi.quantization.thirty_second"),
        }
    }

    /// 换算成起始 tick 容差（需要文件头的"每拍 tick 数"）
    ///
    /// 分轨与评估共用这一个换算，两边的"同时发声"口径才不会漂移。
    pub fn onset_tolerance(&self, ticks_per_beat: u16) -> crate::midi::grouping::OnsetTolerance {
        self.to_fraction()
            .map(|fraction| (ticks_per_beat as f64 * fraction).round() as u64)
    }
}

impl TimbreWeaveApp {
    /// 绘制左侧"源 MIDI"栏
    pub fn show_midi_panel(&mut self, ui: &mut egui::Ui, colors: &Colors) {
        ui.label(
            egui::RichText::new(crate::t!("ui.midi.title"))
                .heading()
                .color(colors.title_color),
        );
        ui.label(
            egui::RichText::new(crate::t!("ui.midi.subtitle"))
                .small()
                .color(colors.text_secondary),
        );
        ui.add_space(8.0);

        // ---- 卡片 1：选文件与扫描 ----
        let has_file = self.data.selected_midi_path.is_some();
        widgets::card(ui, colors, &crate::t!("ui.midi.file_card"), |ui| {
            ui.horizontal(|ui| {
                if ui.button(crate::t!("ui.midi.select_file")).clicked() {
                    self.pick_midi_file();
                }
                if ui
                    .add_enabled(has_file, egui::Button::new(crate::t!("ui.midi.scan")))
                    .clicked()
                {
                    self.scan_tracks();
                }
            });

            match &self.data.selected_midi_path {
                Some(path) => {
                    // 路径可能很长：允许换行，避免撑宽左栏
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(path)
                                .small()
                                .color(colors.text_secondary),
                        )
                        .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                }
                None => {
                    ui.label(
                        egui::RichText::new(crate::t!("ui.midi.no_file"))
                            .small()
                            .color(colors.text_secondary),
                    );
                }
            }
        });

        ui.add_space(8.0);

        // ---- 卡片 2：通道与量化 ----
        widgets::card(ui, colors, &crate::t!("ui.midi.options_card"), |ui| {
            egui::Grid::new("midi_option_grid")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(crate::t!("ui.midi.use_channel"));
                    egui::ComboBox::from_id_salt("channel_filter")
                        .selected_text(if self.data.channel_filter == -1 {
                            crate::t!("ui.midi.use_channel.all")
                        } else {
                            (self.data.channel_filter + 1).to_string()
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    self.data.channel_filter == -1,
                                    crate::t!("ui.midi.use_channel.all"),
                                )
                                .clicked()
                            {
                                self.data.channel_filter = -1;
                                // 通道过滤变了，评估结果里的音符数就不再对应当前设置
                                self.evaluation = None;
                            }
                            for channel_index in 0..16 {
                                if ui
                                    .selectable_label(
                                        self.data.channel_filter == channel_index,
                                        (channel_index + 1).to_string(),
                                    )
                                    .clicked()
                                {
                                    self.data.channel_filter = channel_index;
                                    self.evaluation = None;
                                }
                            }
                        });
                    ui.end_row();

                    ui.label(crate::t!("ui.midi.quantization"));
                    egui::ComboBox::from_id_salt("quantization")
                        .selected_text(self.data.quantization.display_text())
                        .show_ui(ui, |ui| {
                            for quantization in Quantization::ALL {
                                if ui
                                    .selectable_label(
                                        self.data.quantization == quantization,
                                        quantization.display_text(),
                                    )
                                    .clicked()
                                {
                                    self.data.quantization = quantization;
                                    // 量化档位决定"多近算同时发声"，评估结果随之失效
                                    self.evaluation = None;
                                }
                            }
                        });
                    ui.end_row();
                });
        });

        ui.add_space(8.0);

        // ---- 卡片 3：轨道列表 ----
        let scanned_count = self.scanned_tracks.len();
        let selected_count = self.data.source_tracks.len();
        widgets::card(
            ui,
            colors,
            &crate::t!("ui.midi.track_card", scanned_count, selected_count),
            |ui| {
                if self.scanned_tracks.is_empty() {
                    ui.label(
                        egui::RichText::new(crate::t!("ui.midi.no_tracks"))
                            .small()
                            .color(colors.text_secondary),
                    );
                    return;
                }

                egui::ScrollArea::vertical()
                    .id_salt("source_track_scroll")
                    .max_height(320.0)
                    .show(ui, |ui| {
                        egui::Grid::new("source_track_grid")
                            .num_columns(4)
                            .striped(true)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                widgets::section_label(ui, colors, &crate::t!("ui.midi.track_index"));
                                widgets::section_label(ui, colors, &crate::t!("ui.midi.track_name"));
                                widgets::section_label(ui, colors, &crate::t!("ui.midi.track_notes"));
                                widgets::section_label(
                                    ui,
                                    colors,
                                    &crate::t!("ui.midi.track_is_source"),
                                );
                                ui.end_row();

                                for track in &self.scanned_tracks {
                                    let track_index = track.track_index;
                                    ui.label((track_index + 1).to_string());
                                    ui.label(track.track_name.clone());
                                    ui.label(track.note_count.to_string());
                                    let mut is_source =
                                        self.data.source_tracks.contains(&track_index);
                                    if ui.checkbox(&mut is_source, "").changed() {
                                        if is_source {
                                            self.data.source_tracks.push(track_index);
                                        } else {
                                            self.data
                                                .source_tracks
                                                .retain(|index| *index != track_index);
                                        }
                                        // 勾选变了，上一次的评估结果不再对应当前选择
                                        self.evaluation = None;
                                    }
                                    ui.end_row();
                                }
                            });
                    });

                // 勾完源轨后先评估一下，再挑编制：挑乐器的两个依据（同时音容量、音域）
                // 就是评估给出的数字
                ui.add_space(6.0);
                if ui
                    .add_enabled(
                        selected_count > 0,
                        egui::Button::new(crate::t!("ui.midi.evaluate")),
                    )
                    .on_hover_text(crate::t!("ui.midi.evaluate_hint"))
                    .clicked()
                {
                    self.evaluate_source_tracks();
                }
            },
        );

        ui.add_space(8.0);

        // ---- 卡片 4：评估结果（点过"评估"才出现） ----
        if let Some(evaluation) = &self.evaluation {
            widgets::card(ui, colors, &crate::t!("ui.midi.eval_card"), |ui| {
                egui::Grid::new("eval_grid")
                    .num_columns(2)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        widgets::key_value_row(
                            ui,
                            colors,
                            &crate::t!("ui.midi.eval_notes"),
                            &evaluation.note_count.to_string(),
                        );
                        widgets::key_value_row(
                            ui,
                            colors,
                            &crate::t!("ui.midi.eval_onset_peak"),
                            &evaluation.max_onset_notes.to_string(),
                        );
                        widgets::key_value_row(
                            ui,
                            colors,
                            &crate::t!("ui.midi.eval_overlap_peak"),
                            &evaluation.overlap_peak.to_string(),
                        );

                        ui.label(
                            egui::RichText::new(crate::t!("ui.midi.eval_range"))
                                .small()
                                .color(colors.text_secondary),
                        );
                        match (evaluation.lowest_key, evaluation.highest_key) {
                            (Some(lowest), Some(highest)) => {
                                ui.label(format!(
                                    "{}（{}–{}）",
                                    crate::split::pitch::key_range_text(Some(lowest), Some(highest)),
                                    lowest,
                                    highest
                                ));
                            }
                            _ => {
                                ui.label("-");
                            }
                        }
                        ui.end_row();
                    });

                // 说明文本：让用户知道这两个"同时音"数字各自是什么意思
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(crate::t!("ui.midi.eval_onset_note"))
                        .small()
                        .color(colors.text_secondary),
                );
                ui.label(
                    egui::RichText::new(crate::t!("ui.midi.eval_overlap_note"))
                        .small()
                        .color(colors.text_secondary),
                );

                if !evaluation.onset_size_counts.is_empty() {
                    ui.add_space(4.0);
                    widgets::section_label(ui, colors, &crate::t!("ui.midi.eval_distribution"));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(evaluation.onset_size_text())
                                .small()
                                .color(colors.text_primary),
                        )
                        .wrap_mode(egui::TextWrapMode::Wrap),
                    );
                }
            });

            ui.add_space(8.0);
        }

        // ---- 提示信息 ----
        if let Some(message) = &self.info_message {
            widgets::success_text(ui, colors, message);
        }
        if let Some(error) = &self.error_message {
            widgets::error_text(ui, colors, error);
        }
    }
}
