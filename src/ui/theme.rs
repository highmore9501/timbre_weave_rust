//! 颜色主题（深色 / 浅色）
//!
//! 结构与 `string_flow_rust` 的 `ui/theme.rs` 一致（同一套 `Colors` 字段 + `ThemeManager`
//! 应用 `egui::Style`），但**配色换成冷色调**：青绿 / 蓝青为主色，与那边的暖棕色调区分开。
//!
//! 主题选择持久化到工作目录的 `config.json` 的 `theme` 字段（与 `language` 共存）。

use eframe::egui;
use serde_json::Value;

/// 配置文件（与 i18n 共用同一个 config.json）
const CONFIG_FILE: &str = "config.json";

/// 主题类型
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeType {
    Light,
    Dark,
}

impl ThemeType {
    /// 稳定标识（写进 config.json）
    pub fn id(&self) -> &'static str {
        match self {
            ThemeType::Light => "light",
            ThemeType::Dark => "dark",
        }
    }

    /// 从 config.json 里的字符串还原
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "light" => Some(ThemeType::Light),
            "dark" => Some(ThemeType::Dark),
            _ => None,
        }
    }

    /// i18n 词典键
    pub fn i18n_key(&self) -> &'static str {
        match self {
            ThemeType::Light => "ui.theme.light",
            ThemeType::Dark => "ui.theme.dark",
        }
    }

    /// 另一种主题（用于切换按钮）
    pub fn toggled(&self) -> Self {
        match self {
            ThemeType::Light => ThemeType::Dark,
            ThemeType::Dark => ThemeType::Light,
        }
    }
}

/// 主题管理器
pub struct ThemeManager {
    pub theme_type: ThemeType,
}

impl ThemeManager {
    /// 从 config.json 读取主题（缺失或非法时回退深色）
    pub fn load_from_config() -> Self {
        let theme_type = std::fs::read_to_string(CONFIG_FILE)
            .ok()
            .and_then(|json_text| serde_json::from_str::<Value>(&json_text).ok())
            .and_then(|config_json| {
                config_json
                    .get("theme")
                    .and_then(|theme_value| theme_value.as_str())
                    .and_then(ThemeType::from_id)
            })
            .unwrap_or(ThemeType::Dark);
        ThemeManager { theme_type }
    }

    /// 切换主题并持久化
    pub fn toggle(&mut self, ctx: &egui::Context) {
        self.theme_type = self.theme_type.toggled();
        save_theme(self.theme_type);
        self.apply_theme(ctx);
    }

    /// 把主题应用到 egui 上下文
    pub fn apply_theme(&self, ctx: &egui::Context) {
        let colors = self.get_colors();
        let mut style = (*ctx.style()).clone();

        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.menu_margin = egui::Margin::same(8);
        style.spacing.indent = 16.0;
        style.spacing.slider_width = 160.0;
        style.spacing.text_edit_width = 200.0;
        style.visuals = self.build_visuals(colors);

        ctx.set_style(style);
    }

    /// 当前主题的配色
    pub fn get_colors(&self) -> Colors {
        match self.theme_type {
            ThemeType::Light => Colors::light(),
            ThemeType::Dark => Colors::dark(),
        }
    }

    fn build_visuals(&self, colors: Colors) -> egui::Visuals {
        let mut visuals = match self.theme_type {
            ThemeType::Light => egui::Visuals::light(),
            ThemeType::Dark => egui::Visuals::dark(),
        };

        visuals.override_text_color = Some(colors.text_primary);
        visuals.panel_fill = colors.panel_fill;
        visuals.window_fill = colors.window_fill;
        visuals.faint_bg_color = colors.panel_fill;
        visuals.extreme_bg_color = colors.card_fill;
        visuals.code_bg_color = colors.card_fill_alt;
        visuals.hyperlink_color = colors.accent;
        visuals.selection.bg_fill = colors.accent_soft;
        visuals.selection.stroke = egui::Stroke::new(1.0_f32, colors.accent);
        visuals.window_stroke = egui::Stroke::new(1.0_f32, colors.border);

        visuals.widgets.noninteractive.bg_fill = colors.card_fill;
        visuals.widgets.noninteractive.weak_bg_fill = colors.card_fill_alt;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, colors.border);
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, colors.text_secondary);
        visuals.widgets.noninteractive.corner_radius = 10.0.into();

        visuals.widgets.inactive.bg_fill = colors.card_fill_alt;
        visuals.widgets.inactive.weak_bg_fill = colors.card_fill;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, colors.border);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, colors.text_primary);
        visuals.widgets.inactive.corner_radius = 8.0.into();

        visuals.widgets.hovered.bg_fill = colors.accent_soft;
        visuals.widgets.hovered.weak_bg_fill = colors.card_fill_alt;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, colors.accent);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, colors.text_primary);
        visuals.widgets.hovered.corner_radius = 8.0.into();

        visuals.widgets.active.bg_fill = colors.accent;
        visuals.widgets.active.weak_bg_fill = colors.accent;
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, colors.accent);
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, colors.text_on_accent);
        visuals.widgets.active.corner_radius = 8.0.into();

        visuals.widgets.open.bg_fill = colors.card_fill_alt;
        visuals.widgets.open.weak_bg_fill = colors.card_fill_alt;
        visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0_f32, colors.accent);
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0_f32, colors.text_primary);
        visuals.widgets.open.corner_radius = 8.0.into();

        visuals
    }
}

/// 一套配色
#[derive(Clone, Copy)]
pub struct Colors {
    pub panel_fill: egui::Color32,
    pub window_fill: egui::Color32,
    pub card_fill: egui::Color32,
    pub card_fill_alt: egui::Color32,
    pub accent: egui::Color32,
    pub accent_soft: egui::Color32,
    pub border: egui::Color32,
    pub text_primary: egui::Color32,
    pub text_secondary: egui::Color32,
    pub text_on_accent: egui::Color32,
    /// 统计徽标底色
    pub badge_fill: egui::Color32,
    /// 更淡的底色（表头 / 次级块）
    pub subtle_fill: egui::Color32,
    /// 更淡的边线
    pub soft_border: egui::Color32,
    /// 强调色的填充块
    pub accent_fill: egui::Color32,
    /// 标题文字色
    pub title_color: egui::Color32,
    /// 警告色（黄偏橙，深浅主题都看得清）
    pub warning_text: egui::Color32,
    /// 成功色
    pub success_text: egui::Color32,
    /// 错误色
    pub error_text: egui::Color32,
    /// 乐器族标签色：木管
    pub family_woodwind: egui::Color32,
    /// 乐器族标签色：铜管
    pub family_brass: egui::Color32,
    /// 乐器族标签色：弦乐
    pub family_strings: egui::Color32,
    /// 乐器族标签色：打击乐
    pub family_percussion: egui::Color32,
}

impl Colors {
    /// 浅色主题：冷调的雾青 + 蓝绿主色
    pub fn light() -> Self {
        Self {
            panel_fill: egui::Color32::from_rgb(238, 243, 245),
            window_fill: egui::Color32::from_rgb(247, 251, 252),
            card_fill: egui::Color32::from_rgb(252, 255, 255),
            card_fill_alt: egui::Color32::from_rgb(230, 239, 242),
            accent: egui::Color32::from_rgb(26, 122, 138),
            accent_soft: egui::Color32::from_rgb(205, 231, 235),
            border: egui::Color32::from_rgb(196, 214, 219),
            text_primary: egui::Color32::from_rgb(30, 44, 48),
            text_secondary: egui::Color32::from_rgb(94, 116, 122),
            text_on_accent: egui::Color32::from_rgb(244, 252, 253),
            badge_fill: egui::Color32::from_rgb(219, 235, 238),
            subtle_fill: egui::Color32::from_rgb(243, 249, 250),
            soft_border: egui::Color32::from_rgb(209, 226, 230),
            accent_fill: egui::Color32::from_rgb(205, 231, 235),
            title_color: egui::Color32::from_rgb(18, 92, 105),
            warning_text: egui::Color32::from_rgb(158, 104, 12),
            success_text: egui::Color32::from_rgb(28, 116, 84),
            error_text: egui::Color32::from_rgb(176, 58, 52),
            family_woodwind: egui::Color32::from_rgb(29, 132, 110),
            family_brass: egui::Color32::from_rgb(176, 122, 30),
            family_strings: egui::Color32::from_rgb(46, 104, 176),
            family_percussion: egui::Color32::from_rgb(140, 74, 158),
        }
    }

    /// 深色主题：深青灰底 + 亮青主色
    pub fn dark() -> Self {
        Self {
            panel_fill: egui::Color32::from_rgb(22, 28, 31),
            window_fill: egui::Color32::from_rgb(28, 35, 38),
            card_fill: egui::Color32::from_rgb(34, 42, 46),
            card_fill_alt: egui::Color32::from_rgb(44, 55, 60),
            accent: egui::Color32::from_rgb(94, 202, 214),
            accent_soft: egui::Color32::from_rgb(40, 74, 80),
            border: egui::Color32::from_rgb(66, 84, 90),
            text_primary: egui::Color32::from_rgb(226, 238, 240),
            text_secondary: egui::Color32::from_rgb(158, 180, 186),
            text_on_accent: egui::Color32::from_rgb(16, 32, 36),
            badge_fill: egui::Color32::from_rgb(46, 68, 74),
            subtle_fill: egui::Color32::from_rgb(40, 50, 55),
            soft_border: egui::Color32::from_rgb(62, 78, 84),
            accent_fill: egui::Color32::from_rgb(38, 84, 92),
            title_color: egui::Color32::from_rgb(140, 226, 236),
            warning_text: egui::Color32::from_rgb(240, 196, 108),
            success_text: egui::Color32::from_rgb(126, 214, 168),
            error_text: egui::Color32::from_rgb(244, 138, 128),
            family_woodwind: egui::Color32::from_rgb(110, 214, 180),
            family_brass: egui::Color32::from_rgb(232, 190, 110),
            family_strings: egui::Color32::from_rgb(130, 178, 246),
            family_percussion: egui::Color32::from_rgb(206, 154, 234),
        }
    }
}

/// 把主题写进 config.json（保留 language 等既有字段）
fn save_theme(theme_type: ThemeType) {
    let mut config_json = std::fs::read_to_string(CONFIG_FILE)
        .ok()
        .and_then(|json_text| serde_json::from_str::<Value>(&json_text).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    if let Value::Object(config_object) = &mut config_json {
        config_object.insert(
            "theme".to_string(),
            Value::String(theme_type.id().to_string()),
        );
    }

    if let Ok(text) = serde_json::to_string_pretty(&config_json) {
        if let Err(error) = std::fs::write(CONFIG_FILE, text) {
            eprintln!("{}", crate::t!("log.theme_save_failed", error));
        }
    }
}
