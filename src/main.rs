use eframe::egui;
use std::sync::Arc;
use timbre_weave::ui::app::TimbreWeaveApp;

/// 图标尺寸（正方形）
const ICON_SIZE: usize = 256;

pub fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    // 初始化 i18n（加载词典与语言设置）
    timbre_weave::i18n::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([960.0, 640.0])
            .with_icon(build_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "Timbre Weave",
        options,
        Box::new(|creation_context| {
            setup_custom_fonts(&creation_context.egui_ctx);
            // 主题在 App::update 里每帧应用，这里只负责字体
            Ok(Box::new(TimbreWeaveApp::default()))
        }),
    )
}

/// 程序化生成窗口图标：深青底 + 三道渐变青色的"弦"
///
/// 不从外部引入图片素材（避免把别的项目的 logo 拷进来），
/// 配色与 [`timbre_weave::ui::theme`] 的深色主题一致。
fn build_icon() -> egui::IconData {
    let mut rgba = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
    let background = (22u8, 28u8, 31u8);
    // 三道弦的颜色（由深到亮的青）
    let strings = [(46u8, 92u8, 100u8), (94u8, 202u8, 214u8), (140u8, 226u8, 236u8)];
    // 弦的纵向位置（相对高度的比例）与粗细
    let string_positions = [0.34f32, 0.52f32, 0.70f32];
    let string_half_thickness = [7.0f32, 6.0f32, 5.0f32];

    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let offset = (y * ICON_SIZE + x) * 4;
            let in_circle = is_inside_circle(x, y);
            if !in_circle {
                // 圆外透明
                continue;
            }

            let mut pixel = (background.0, background.1, background.2);

            // 按行叠加"弦"
            let relative_y = y as f32 / ICON_SIZE as f32;
            for (string_index, &string_y) in string_positions.iter().enumerate() {
                let distance = (relative_y - string_y).abs() * ICON_SIZE as f32;
                if distance <= string_half_thickness[string_index] {
                    // 弦的中段更亮，两端略暗，做出渐变感
                    let relative_x = x as f32 / ICON_SIZE as f32;
                    let glow = 1.0 - (relative_x - 0.5).abs() * 0.7;
                    let color = strings[string_index];
                    pixel = (
                        (color.0 as f32 * glow) as u8,
                        (color.1 as f32 * glow) as u8,
                        (color.2 as f32 * glow) as u8,
                    );
                    break;
                }
            }

            rgba[offset] = pixel.0;
            rgba[offset + 1] = pixel.1;
            rgba[offset + 2] = pixel.2;
            rgba[offset + 3] = 255;
        }
    }

    egui::IconData {
        rgba,
        width: ICON_SIZE as u32,
        height: ICON_SIZE as u32,
    }
}

/// 该像素是否落在圆形图标区域内（边缘做一点抗锯齿：外部留 1 像素透明）
fn is_inside_circle(x: usize, y: usize) -> bool {
    let center = ICON_SIZE as f32 / 2.0 - 0.5;
    let dx = x as f32 - center;
    let dy = y as f32 - center;
    let radius = center + 0.5;
    dx * dx + dy * dy <= radius * radius
}

/// 装载中文字体（与 `string_flow_rust` 用同一套 HarmonyOS Sans）
fn setup_custom_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    fonts.font_data.insert(
        "HarmonyOS_Sans".to_owned(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../asset/font/HarmonyOS_Sans_SC_Regular.ttf"
        ))),
    );

    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "HarmonyOS_Sans".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("HarmonyOS_Sans".to_owned());

    ctx.set_fonts(fonts);
}
