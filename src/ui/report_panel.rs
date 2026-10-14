//! 执行与报告面板

use crate::split::run::{SplitRequest, run_split};
use crate::ui::app::TimbreWeaveApp;
use crate::ui::theme::Colors;
use crate::ui::widgets;
use eframe::egui;

impl TimbreWeaveApp {
    /// 绘制右侧"执行与报告"栏
    pub fn show_report_panel(&mut self, ui: &mut egui::Ui, colors: &Colors) {
        ui.label(
            egui::RichText::new(crate::t!("ui.report.title"))
                .heading()
                .color(colors.title_color),
        );
        ui.label(
            egui::RichText::new(crate::t!("ui.report.subtitle"))
                .small()
                .color(colors.text_secondary),
        );
        ui.add_space(8.0);

        // ---- 卡片 1：参数与执行 ----
        widgets::card(ui, colors, &crate::t!("ui.report.run_card"), |ui| {
            let mut pool_size = self.data.pool_size;
            let pool_response = ui.add(
                egui::Slider::new(&mut pool_size, crate::split::assign::MIN_POOL_SIZE..=2048)
                    .logarithmic(true)
                    .text(crate::t!("ui.report.pool_size")),
            );
            if pool_response.changed() {
                self.data.pool_size = pool_size;
            }
            ui.label(
                egui::RichText::new(crate::t!("ui.report.pool_size_hint"))
                    .small()
                    .color(colors.text_secondary),
            );

            ui.add_space(6.0);
            let run_button = egui::Button::new(
                egui::RichText::new(crate::t!("ui.report.run"))
                    .strong()
                    .color(colors.text_on_accent),
            )
            .fill(colors.accent);
            if ui.add(run_button).clicked() {
                self.run_split();
            }
        });

        ui.add_space(8.0);

        if let Some(message) = &self.info_message {
            widgets::success_text(ui, colors, message);
            ui.add_space(6.0);
        }
        if let Some(error) = &self.error_message {
            widgets::error_text(ui, colors, error);
            ui.add_space(6.0);
        }

        let Some(report) = &self.report else {
            widgets::card(ui, colors, &crate::t!("ui.report.result_card"), |ui| {
                ui.label(
                    egui::RichText::new(crate::t!("ui.report.no_result"))
                        .small()
                        .color(colors.text_secondary),
                );
            });
            return;
        };

        // ---- 卡片 2：处理时间 ----
        //
        // 只显示总耗时：分阶段耗时是调试用的，对挑编制没有帮助（它仍然照常统计并写进日志）。
        widgets::card(ui, colors, &crate::t!("ui.report.timing_card"), |ui| {
            widgets::badge(
                ui,
                colors,
                &crate::t!("ui.report.total_time"),
                &crate::split::report::StageTimings::format_micros(report.timings.total_micros),
            );
        });

        ui.add_space(8.0);

        // ---- 卡片 3：统计汇总 ----
        widgets::card(ui, colors, &crate::t!("ui.report.summary_card"), |ui| {
            ui.horizontal_wrapped(|ui| {
                widgets::badge(
                    ui,
                    colors,
                    &crate::t!("ui.report.source_notes"),
                    &report.source_notes.to_string(),
                );
                widgets::badge(
                    ui,
                    colors,
                    &crate::t!("ui.report.written_notes"),
                    &report.written_notes.to_string(),
                );
                widgets::badge(
                    ui,
                    colors,
                    &crate::t!("ui.report.onset_groups"),
                    &report.onset_groups.to_string(),
                );
            });
            ui.add_space(6.0);

            egui::Grid::new("report_summary_grid")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    widgets::key_value_row_wide(
                        ui,
                        colors,
                        &crate::t!("ui.report.output_path"),
                        &report.output_path,
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.octave_folded"),
                        &report.octave_folded.to_string(),
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.pitch_merged"),
                        &report.pitch_merged.to_string(),
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.octave_merged"),
                        &report.octave_merged.to_string(),
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.dropped_notes"),
                        &report.dropped_notes.to_string(),
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.forced_notes"),
                        &report.forced_notes.to_string(),
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.doubled_notes"),
                        &report.doubled_notes.to_string(),
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.pool_expansions"),
                        &report.pool_expansions.to_string(),
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.pool_peak_size"),
                        &report.pool_peak_size.to_string(),
                    );
                    widgets::key_value_row(
                        ui,
                        colors,
                        &crate::t!("ui.report.pool_saturated"),
                        &report.pool_saturated_groups.to_string(),
                    );
                    if !report.shared_channels.is_empty() {
                        let detail: Vec<String> = report
                            .shared_channels
                            .iter()
                            .map(|(channel, count)| format!("{} ×{}", channel, count))
                            .collect();
                        widgets::multi_value_row(
                            ui,
                            colors,
                            &crate::t!("ui.report.shared_channels"),
                            &detail.join("  "),
                        );
                    }
                });
        });

        if report.dropped_notes > 0 {
            ui.add_space(6.0);
            widgets::warning_text(
                ui,
                colors,
                &crate::t!("ui.report.dropped_warning", report.dropped_notes),
            );
        }
        if report.forced_notes > 0 {
            ui.add_space(6.0);
            widgets::warning_text(
                ui,
                colors,
                &crate::t!("ui.report.forced_warning", report.forced_notes),
            );
        }
        if report.pool_saturated_groups > 0 {
            ui.add_space(6.0);
            widgets::warning_text(
                ui,
                colors,
                &crate::t!(
                    "ui.report.pool_saturated_warning",
                    report.pool_saturated_groups
                ),
            );
        }
        if !report.shared_channels.is_empty() {
            ui.add_space(6.0);
            let total_shared: usize = report
                .shared_channels
                .iter()
                .map(|(_, count)| *count)
                .sum();
            widgets::warning_text(
                ui,
                colors,
                &crate::t!("ui.report.shared_channels_warning", total_shared),
            );
        }

        ui.add_space(8.0);

        // ---- 卡片 4：每件乐器的音符数 ----
        widgets::card(ui, colors, &crate::t!("ui.report.slot_card"), |ui| {
            egui::Grid::new("report_slot_grid")
                .num_columns(6)
                .striped(true)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    widgets::section_label(ui, colors, &crate::t!("ui.report.slot"));
                    widgets::section_label(ui, colors, &crate::t!("ui.report.note_count"));
                    widgets::section_label(ui, colors, &crate::t!("ui.report.share"));
                    widgets::section_label(ui, colors, &crate::t!("ui.report.used_range"));
                    widgets::section_label(ui, colors, &crate::t!("ui.report.doubled_notes"));
                    widgets::section_label(ui, colors, &crate::t!("ui.report.outside_comfort"));
                    ui.end_row();

                    for slot in &report.slots {
                        ui.label(slot.track_name.clone());
                        ui.label(slot.note_count.to_string());
                        ui.label(format!("{:.1}%", slot.share * 100.0));
                        ui.label(match (slot.lowest_key, slot.highest_key) {
                            (Some(lowest), Some(highest)) => format!("{}–{}", lowest, highest),
                            _ => "-".to_string(),
                        });
                        // 叠音只应出现在上限为 2 的乐器上；单声部乐器这一列必须是 0
                        if slot.doubled_notes > 0 {
                            ui.label(
                                egui::RichText::new(slot.doubled_notes.to_string())
                                    .color(colors.family_strings),
                            );
                        } else {
                            ui.label("0");
                        }
                        ui.label(slot.outside_comfort.to_string());
                        ui.end_row();
                    }
                });
        });
    }

    /// 执行一次分轨
    fn run_split(&mut self) {
        self.report = None;
        self.error_message = None;
        self.info_message = None;

        let Some(midi_path) = self.data.selected_midi_path.clone() else {
            self.error_message = Some(crate::t!("msg.no_file_selected"));
            return;
        };
        if self.data.source_tracks.is_empty() {
            self.error_message = Some(crate::t!("msg.no_track_selected"));
            return;
        }
        if self.data.ensemble.is_empty() {
            self.error_message = Some(crate::t!("msg.no_instrument_selected"));
            return;
        }

        // 量化档位 → 起始 tick 容差（需要每拍 tick 数，所以这里读一次文件头）
        let onset_tolerance = match crate::midi::reader::load_midi_file(&midi_path) {
            Ok(loaded) => self.data.quantization.onset_tolerance(loaded.ticks_per_beat),
            Err(message) => {
                self.error_message = Some(message);
                return;
            }
        };

        let request = SplitRequest {
            midi_path: midi_path.clone(),
            track_indices: self.data.source_tracks.clone(),
            channel_filter: self.data.channel_filter,
            onset_tolerance,
            ensemble: self.data.ensemble.clone(),
            pool_size: self.data.pool_size,
        };

        match run_split(&request) {
            Ok(report) => {
                crate::log_ok!(crate::t!("msg.split_success", &report.output_path));
                self.report = Some(report);
            }
            Err(error) => {
                let text = error.to_string();
                crate::log_err!(crate::t!("msg.split_failed", &text));
                self.error_message = Some(text);
            }
        }
    }
}
