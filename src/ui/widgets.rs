//! 界面通用组件：卡片、区块标题、统计徽标、键值行
//!
//! 抽出来的目的是让三个面板共享同一套视觉规则（圆角、间距、配色语义），
//! 避免每个面板各写一套 `Frame` 造成样式漂移。

use crate::ui::theme::Colors;
use eframe::egui;

/// 一张卡片：带标题、浅色描边与圆角，用于把一组控件框起来
pub fn card<R>(
    ui: &mut egui::Ui,
    colors: &Colors,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(colors.card_fill)
        .stroke(egui::Stroke::new(1.0_f32, colors.soft_border))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .color(colors.title_color),
            );
            ui.add_space(4.0);
            add_contents(ui)
        })
        .inner
}

/// 小号区块标题（卡片内部用）
pub fn section_label(ui: &mut egui::Ui, colors: &Colors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .small()
            .strong()
            .color(colors.text_secondary),
    );
}

/// 统计徽标：底色块 + 数值，用于"读入源音符 62033"这类指标
///
/// 用 `horizontal_wrapped` 而非 `horizontal`：数值过长时（比如输出路径）能换行，不会撑宽所在栏。
pub fn badge(ui: &mut egui::Ui, colors: &Colors, label: &str, value: &str) {
    egui::Frame::new()
        .fill(colors.badge_fill)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(label)
                        .small()
                        .color(colors.text_secondary),
                );
                ui.label(
                    egui::RichText::new(value)
                        .strong()
                        .color(colors.text_primary),
                );
            });
        });
}

/// 键值行（用在两列网格里）
///
/// 值默认**截断**（不换行）：报告里有长不可断的内容（"共用通道 16 ×6"这类清单），
/// 若按内容计算宽度会把整个面板撑宽。需要完整显示的长文本用 [`key_value_row_wide`]。
pub fn key_value_row(ui: &mut egui::Ui, colors: &Colors, key: &str, value: &str) {
    key_value_row_with_mode(ui, colors, key, value, egui::TextWrapMode::Truncate);
}

/// 键值行（值允许换行，用于文件路径这类需要看全的内容）
pub fn key_value_row_wide(ui: &mut egui::Ui, colors: &Colors, key: &str, value: &str) {
    key_value_row_with_mode(ui, colors, key, value, egui::TextWrapMode::Wrap);
}

/// 键值行的实现：指定值的换行模式
fn key_value_row_with_mode(
    ui: &mut egui::Ui,
    colors: &Colors,
    key: &str,
    value: &str,
    wrap_mode: egui::TextWrapMode,
) {
    ui.label(
        egui::RichText::new(key)
            .small()
            .color(colors.text_secondary),
    );
    ui.add(egui::Label::new(value.to_string()).wrap_mode(wrap_mode));
    ui.end_row();
}

/// 多行文本行：整行显示一段可能很长的文本（如共用通道清单），允许换行
pub fn multi_value_row(ui: &mut egui::Ui, colors: &Colors, key: &str, value: &str) {
    ui.label(
        egui::RichText::new(key)
            .small()
            .color(colors.text_secondary),
    );
    ui.end_row();
    ui.label(egui::RichText::new(value).small().color(colors.text_primary));
    ui.end_row();
}

/// 错误提示
pub fn error_text(ui: &mut egui::Ui, colors: &Colors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .color(colors.error_text)
            .strong(),
    );
}

/// 警告提示
pub fn warning_text(ui: &mut egui::Ui, colors: &Colors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .color(colors.warning_text)
            .strong(),
    );
}

/// 成功提示
pub fn success_text(ui: &mut egui::Ui, colors: &Colors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .color(colors.success_text)
            .strong(),
    );
}
