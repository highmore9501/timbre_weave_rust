//! 音高显示：把 midi 编号写成音名
//!
//! 界面上一律以**发声音高**为准（与乐器数据、源 midi 读数同一口径），
//! 音名只做显示用途，不参与任何判定。
//!
//! 八度编号按科学音高记号法（SPN）：`60 = C4`、`69 = A4`（标准音 A440）、
//! `0 = C-1`、`127 = G9`。

/// 一个八度里的十二个音名（用升号写法，与 midi 的黑键编号对应）
const PITCH_CLASS_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// midi 编号 → 音名（如 60 → "C4"、69 → "A4"）
pub fn key_name(key: i32) -> String {
    // 用 rem_euclid 而不是 `%`：负数（0 以下的越界音）也能落到 0–11
    let pitch_class = key.rem_euclid(12) as usize;
    let octave = key.div_euclid(12) - 1;
    format!("{}{}", PITCH_CLASS_NAMES[pitch_class], octave)
}

/// 音域文本（如 "C4–C6"）；两端都没有音符时返回 "-"
pub fn key_range_text(lowest_key: Option<i32>, highest_key: Option<i32>) -> String {
    match (lowest_key, highest_key) {
        (Some(lowest), Some(highest)) => format!("{}–{}", key_name(lowest), key_name(highest)),
        _ => "-".to_string(),
    }
}
