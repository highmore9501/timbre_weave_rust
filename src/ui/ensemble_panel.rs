//! 编制面板：乐器池勾选、编制表、预设

use crate::split::instrument::{
    ENSEMBLE_PRESETS, INSTRUMENT_POOL, InstrumentFamily, MAX_ENSEMBLE_SIZE, build_ensemble,
};
use crate::ui::app::TimbreWeaveApp;
use crate::ui::theme::Colors;
use crate::ui::widgets;
use eframe::egui;

impl TimbreWeaveApp {
    /// 绘制中间"编制"栏
    pub fn show_ensemble_panel(&mut self, ui: &mut egui::Ui, colors: &Colors) {
        ui.label(
            egui::RichText::new(crate::t!("ui.ensemble.title"))
                .heading()
                .color(colors.title_color),
        );
        ui.label(
            egui::RichText::new(crate::t!("ui.ensemble.subtitle", MAX_ENSEMBLE_SIZE))
                .small()
                .color(colors.text_secondary),
        );
        ui.add_space(8.0);

        // ---- 卡片 1：编制预设 ----
        widgets::card(ui, colors, &crate::t!("ui.ensemble.preset"), |ui| {
            ui.horizontal_wrapped(|ui| {
                for preset in ENSEMBLE_PRESETS {
                    if ui.button(crate::t!(preset.label_key)).clicked() {
                        let (slots, warnings) = build_ensemble(preset.instrument_ids);
                        self.data.ensemble = slots;
                        self.warnings = warnings;
                    }
                }
                if ui.button(crate::t!("ui.ensemble.preset.clear")).clicked() {
                    self.data.ensemble.clear();
                    self.warnings.clear();
                }
            });
        });

        ui.add_space(8.0);

        // ---- 卡片 2：乐器池 ----
        let selected_total = self.data.ensemble.len();
        let pool_title = crate::t!("ui.ensemble.pool_with_count", selected_total, MAX_ENSEMBLE_SIZE);
        widgets::card(ui, colors, &pool_title, |ui| {
            if selected_total >= MAX_ENSEMBLE_SIZE {
                widgets::warning_text(
                    ui,
                    colors,
                    &crate::t!("ui.ensemble.full", MAX_ENSEMBLE_SIZE),
                );
                ui.add_space(4.0);
            }

            egui::ScrollArea::vertical()
                .id_salt("instrument_pool")
                .max_height(300.0)
                .show(ui, |ui| {
                    let mut current_family: Option<InstrumentFamily> = None;
                    for (spec_index, spec) in INSTRUMENT_POOL.iter().enumerate() {
                        // 族变化时插一条族标题，视觉上分组
                        if current_family != Some(spec.family) {
                            if current_family.is_some() {
                                ui.add_space(6.0);
                            }
                            ui.label(
                                egui::RichText::new(crate::t!(spec.family.i18n_key()))
                                    .small()
                                    .strong()
                                    .color(family_color(colors, spec.family)),
                            );
                            current_family = Some(spec.family);
                        }

                        ui.horizontal(|ui| {
                            // 同一个乐器可以加多把（如两把小提琴）：只受编制上限约束
                            let instance_count = self
                                .data
                                .ensemble
                                .iter()
                                .filter(|slot| slot.spec_index == spec_index)
                                .count();
                            let can_add = self.data.ensemble.len() < MAX_ENSEMBLE_SIZE;
                            if ui
                                .add_enabled(can_add, egui::Button::new("+"))
                                .on_hover_text(crate::t!("ui.ensemble.add_hint"))
                                .clicked()
                            {
                                let (mut added, warnings) = build_ensemble(&[spec.id]);
                                self.data.ensemble.append(&mut added);
                                self.warnings = warnings;
                            }
                            ui.label(crate::t!("instrument.{}", spec.id));
                            if instance_count > 0 {
                                ui.label(
                                    egui::RichText::new(format!("×{}", instance_count))
                                        .small()
                                        .strong()
                                        .color(colors.success_text),
                                );
                            }
                            let family_name = crate::t!(spec.family.i18n_key());
                            let comfort_tag = crate::t!(spec.comfort_source.i18n_key());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  {}–{}  {}",
                                    family_name, spec.playable.lo, spec.playable.hi, comfort_tag
                                ))
                                .small()
                                .color(colors.text_secondary),
                            );
                            // 移调乐器额外标出记谱音域：演奏者心里想的是记谱音域，
                            // 而 midi 与音域判断用的都是发声音高（written = sounding − Δ）
                            if spec.sounding_offset != 0 {
                                let written_lo = spec.playable.lo - spec.sounding_offset as i32;
                                let written_hi = spec.playable.hi - spec.sounding_offset as i32;
                                ui.label(
                                    egui::RichText::new(format!(
                                        "({} {written_lo}–{written_hi})",
                                        crate::t!("ui.ensemble.written_range")
                                    ))
                                    .small()
                                    .color(colors.text_secondary),
                                );
                            }
                        });
                    }
                });
        });

        ui.add_space(8.0);

        // ---- 卡片 3：已选编制 ----
        widgets::card(ui, colors, &crate::t!("ui.ensemble.selected"), |ui| {
            if self.data.ensemble.is_empty() {
                ui.label(
                    egui::RichText::new(crate::t!("ui.ensemble.empty"))
                        .small()
                        .color(colors.text_secondary),
                );
                return;
            }

            let mut remove_index: Option<usize> = None;
            egui::ScrollArea::vertical()
                .id_salt("ensemble_scroll")
                .max_height(260.0)
                .show(ui, |ui| {
                    egui::Grid::new("ensemble_grid")
                        .num_columns(6)
                        .striped(true)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            widgets::section_label(ui, colors, &crate::t!("ui.ensemble.slot"));
                            widgets::section_label(ui, colors, &crate::t!("ui.ensemble.gm_program"));
                            widgets::section_label(
                                ui,
                                colors,
                                &crate::t!("ui.ensemble.max_polyphony"),
                            );
                            widgets::section_label(ui, colors, &crate::t!("ui.ensemble.playable"));
                            widgets::section_label(ui, colors, &crate::t!("ui.ensemble.comfortable"));
                            ui.end_row();

                            for (slot_index, slot) in self.data.ensemble.iter().enumerate() {
                                // 通道不显示：它由编制序号自动推导（跳过打击乐保留的 10 号通道），
                                // 用户不需要手动管它
                                ui.label(slot.track_name());
                                // GM 音色号内部存 0 基，这里按惯例显示 1 基并标注
                                ui.label(format!("{}", slot.gm_program() as u16 + 1));
                                // 同时发音硬上限：1 = 单声部，2 = 可双音
                                ui.label(slot.max_polyphony().to_string());
                                ui.label(format!("{}–{}", slot.playable.lo, slot.playable.hi));
                                ui.label(format!("{}–{}", slot.comfortable.lo, slot.comfortable.hi));
                                if ui.button(crate::t!("ui.ensemble.remove")).clicked() {
                                    remove_index = Some(slot_index);
                                }
                                ui.end_row();
                            }
                        });
                });

            if let Some(slot_index) = remove_index {
                self.data.ensemble.remove(slot_index);
                // 重新编号通道与同名序号
                self.reindex_ensemble();
            }
        });

        for warning in &self.warnings {
            ui.add_space(6.0);
            widgets::warning_text(ui, colors, warning);
        }
    }
}

/// 乐器族对应的标签色
fn family_color(colors: &Colors, family: InstrumentFamily) -> egui::Color32 {
    match family {
        InstrumentFamily::Woodwind => colors.family_woodwind,
        InstrumentFamily::Brass => colors.family_brass,
        InstrumentFamily::Strings => colors.family_strings,
        InstrumentFamily::Percussion => colors.family_percussion,
    }
}
