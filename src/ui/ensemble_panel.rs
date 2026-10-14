//! 编制面板：乐器池勾选、编制表、预设

use crate::split::eval::EvaluationReport;
use crate::split::instrument::{
    ENSEMBLE_PRESETS, INSTRUMENT_POOL, EnsembleSlot, InstrumentFamily, MAX_ENSEMBLE_SIZE,
    PitchRange, build_ensemble, ensemble_capacity, ensemble_union_range,
};
use crate::split::pitch::key_name;
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
                                // 通道不显示：它由本槽位在启用清单里的位次现算（跳过打击乐保留的
                                // 10 号通道），用户不需要手动管它，也不会沿用源 midi 的通道号
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

        ui.add_space(8.0);

        // ---- 卡片 4：编制汇总（实时） ----
        //
        // 每一帧都由当前编制现算，所以增删乐器立刻反映在数字上。
        // 作用是与源轨评估的数字对照：容量够不够、音域盖不盖得住。
        let summary = ensemble_summary(&self.data.ensemble);
        widgets::card(ui, colors, &crate::t!("ui.ensemble.summary"), |ui| {
            if self.data.ensemble.is_empty() {
                ui.label(
                    egui::RichText::new(crate::t!("ui.ensemble.summary_empty"))
                        .small()
                        .color(colors.text_secondary),
                );
                return;
            }

            // 与源轨评估的对照：只在评估过之后才有意义
            let target = self.evaluation.as_ref();
            let value_color = match target {
                Some(target) if !coverage_verdict(target, &summary).all_ok() => {
                    colors.warning_text
                }
                _ => colors.text_primary,
            };

            egui::Grid::new("ensemble_summary_grid")
                .num_columns(2)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    widgets::key_value_row_colored(
                        ui,
                        colors,
                        &crate::t!("ui.ensemble.summary_polyphony"),
                        &crate::t!(
                            "ui.ensemble.summary_polyphony_value",
                            summary.capacity,
                            summary.instrument_count
                        ),
                        value_color,
                    );
                    widgets::key_value_row_colored(
                        ui,
                        colors,
                        &crate::t!("ui.ensemble.summary_range"),
                        &format!(
                            "{}（{}–{}）",
                            crate::split::pitch::key_range_text(
                                summary.lowest_key,
                                summary.highest_key
                            ),
                            summary.lowest_key.unwrap_or_default(),
                            summary.highest_key.unwrap_or_default()
                        ),
                        value_color,
                    );
                });

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(ensemble_compare_text(target, &summary))
                    .strong()
                    .color(value_color),
            );
        });

        for warning in &self.warnings {
            ui.add_space(6.0);
            widgets::warning_text(ui, colors, warning);
        }
    }
}

/// 编制汇总的原始数字（界面上直接用这几项，不做别的加工）
struct EnsembleSummary {
    /// 件数（只是显示用，容量与音域都不看它）
    instrument_count: usize,
    /// 全编制最多能同时发几个音（Σ 每件乐器的同时发音上限）
    capacity: usize,
    /// 并集音域下界
    lowest_key: Option<i32>,
    /// 并集音域上界
    highest_key: Option<i32>,
}

/// 由当前编制算出汇总数字
///
/// 口径：全部取**发声音高**。件数用编制长度（与 `ensemble_capacity` 的过滤口径一致：
/// `enabled` 为假的槽位不参与分轨，也不参与这里的计数）。
fn ensemble_summary(ensemble: &[EnsembleSlot]) -> EnsembleSummary {
    let active: Vec<EnsembleSlot> = ensemble
        .iter()
        .filter(|slot| slot.enabled)
        .cloned()
        .collect();
    let union: Option<PitchRange> = ensemble_union_range(&active);
    EnsembleSummary {
        instrument_count: active.len(),
        capacity: ensemble_capacity(&active),
        lowest_key: union.map(|range| range.lo),
        highest_key: union.map(|range| range.hi),
    }
}

/// 编制与源轨评估的对照结果（供文案与配色共用同一套判定）
struct CoverageVerdict {
    /// 同时音容量够用
    polyphony_ok: bool,
    /// 低音够低
    low_ok: bool,
    /// 高音够高
    high_ok: bool,
}

impl CoverageVerdict {
    /// 三项全过才算"盖得住源轨"
    fn all_ok(&self) -> bool {
        self.polyphony_ok && self.low_ok && self.high_ok
    }
}

/// 逐项判定编制能否覆盖源轨的要求
fn coverage_verdict(target: &EvaluationReport, summary: &EnsembleSummary) -> CoverageVerdict {
    CoverageVerdict {
        polyphony_ok: summary.capacity >= target.max_onset_notes,
        // 编制并集下界比源轨最低音还高 → 低音区没人演得了
        low_ok: match (summary.lowest_key, target.lowest_key) {
            (Some(lowest), Some(target_low)) => lowest <= target_low,
            _ => true,
        },
        high_ok: match (summary.highest_key, target.highest_key) {
            (Some(highest), Some(target_high)) => highest >= target_high,
            _ => true,
        },
    }
}

/// 编制与源轨评估的对照文字
///
/// 没评估过就提示先评估；评估过则逐项说清"同时音够不够、低音够不够低、高音够不够高"。
/// 音域只比并集边界，不逐音检查——真正"谁都演不了"的音由分轨阶段的归一化处理并计入报告。
fn ensemble_compare_text(target: Option<&EvaluationReport>, summary: &EnsembleSummary) -> String {
    let Some(target) = target else {
        return crate::t!("ui.ensemble.summary_no_target");
    };

    let verdict = coverage_verdict(target, summary);
    let mut lines: Vec<String> = Vec::new();

    if verdict.polyphony_ok {
        lines.push(crate::t!(
            "ui.ensemble.compare_polyphony_ok",
            summary.capacity,
            target.max_onset_notes,
            summary.capacity - target.max_onset_notes
        ));
    } else {
        lines.push(crate::t!(
            "ui.ensemble.compare_polyphony_short",
            target.max_onset_notes,
            summary.capacity
        ));
    }

    if verdict.low_ok && verdict.high_ok {
        lines.push(crate::t!("ui.ensemble.compare_range_ok"));
    }
    // 音域缺口要点名到具体音，用户才知道往哪个方向补乐器
    if !verdict.low_ok {
        if let (Some(lowest), Some(target_low)) = (summary.lowest_key, target.lowest_key) {
            lines.push(crate::t!(
                "ui.ensemble.compare_low_short",
                key_name(target_low),
                target_low - lowest,
                key_name(lowest)
            ));
        }
    }
    if !verdict.high_ok {
        if let (Some(highest), Some(target_high)) = (summary.highest_key, target.highest_key) {
            lines.push(crate::t!(
                "ui.ensemble.compare_high_short",
                key_name(target_high),
                highest - target_high,
                key_name(highest)
            ));
        }
    }

    lines.join("　")
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
