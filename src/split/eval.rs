//! 源轨评估：只做统计，不改动任何音符
//!
//! 用户勾完源轨、还没挑乐器时，往往不知道"这套编制够不够用"。
//! 本模块把挑选编制需要的三个数字算出来：
//! - 一共有多少个音符；
//! - 最多有多少个音同时发声（两个口径：同时起音 / 真正重叠）；
//! - 最低音与最高音。
//!
//! 口径与分轨流程保持一致：读音符走 [`read_track_notes`]（同样的通道过滤与收尾规则），
//! 同时起音的分组走 [`group_by_onset`]（同样的量化容差）——这样评估显示的
//! "同时起音最多几个"就是分轨时真正会撞上容量上限的那个数。

use std::collections::BTreeMap;

use crate::midi::grouping::{OnsetTolerance, group_by_onset};
use crate::midi::reader::{LoadedMidi, SourceNote, read_track_notes};

/// 同时起音数分布表的最大键（超出部分归入这个桶，避免音频转 midi 的大音簇刷屏）
const MAX_ONSET_SIZE_BUCKET: usize = 16;

/// 一次评估的结果
#[derive(Debug, Clone, Default)]
pub struct EvaluationReport {
    /// 参与评估的音符总数（按通道过滤之后）
    pub note_count: usize,
    /// 同一量化组里音符数的最大值（"同时起音"口径，与分轨的容量判定一致）
    pub max_onset_notes: usize,
    /// 真正时间重叠的峰值：某一个瞬间最多有几个音在响
    pub overlap_peak: usize,
    /// 最低音（midi 编号；没有音符时为 `None`）
    pub lowest_key: Option<i32>,
    /// 最高音
    pub highest_key: Option<i32>,
    /// 同时起音音符数的分布：键 = 一组的音符数，值 = 这样的时刻有多少个
    ///
    /// 分轨遇到的最大同时发音需求就是这里的最大键；分布本身则告诉用户
    /// "编制要留多少余量"（例如大多数时刻只有 2 个音、偶尔到 5 个）。
    pub onset_size_counts: BTreeMap<usize, usize>,
}

impl EvaluationReport {
    /// 同时起音数分布的一行文本（如 "1 音 ×120  2 音 ×18  5 音 ×1"）
    ///
    /// 键超过 [`MAX_ONSET_SIZE_BUCKET`] 的部分合并显示成 "16 音以上"。
    pub fn onset_size_text(&self) -> String {
        self.onset_size_counts
            .iter()
            .map(|(size, moments)| {
                if *size >= MAX_ONSET_SIZE_BUCKET {
                    format!("{} 音以上 ×{}", size, moments)
                } else {
                    format!("{} 音 ×{}", size, moments)
                }
            })
            .collect::<Vec<String>>()
            .join("  ")
    }
}

/// 评估一组源轨
///
/// 出错只有一种情况：选中的轨道里一个可用音符都没有（与分轨的 [`crate::split::run::SplitError::Empty`] 同义）。
pub fn evaluate_tracks(
    loaded: &LoadedMidi,
    track_indices: &[usize],
    channel_filter: i32,
    tolerance: OnsetTolerance,
) -> Result<EvaluationReport, String> {
    let source_notes = read_track_notes(loaded.smf(), track_indices, channel_filter);
    if source_notes.is_empty() {
        return Err(crate::t!("msg.split_empty"));
    }

    let mut report = EvaluationReport {
        note_count: source_notes.len(),
        lowest_key: source_notes.iter().map(|note| note.key).min(),
        highest_key: source_notes.iter().map(|note| note.key).max(),
        ..EvaluationReport::default()
    };

    // 同时起音：与分轨用同一个分组函数，所以组大小就是分轨要面对的容量需求
    let onset_groups = group_by_onset(&source_notes, tolerance);
    for group in &onset_groups {
        let group_size = group.len();
        report.max_onset_notes = report.max_onset_notes.max(group_size);
        *report.onset_size_counts.entry(group_size).or_insert(0) += 1;
    }

    report.overlap_peak = overlap_peak(&source_notes);

    Ok(report)
}

/// 真正时间重叠的峰值：扫一遍时间轴，看某一个瞬间最多几个音在响
///
/// 做法是经典的"区间最大重叠数"：起音排一个升序、收音排一个升序，
/// 每次走到一个新的起音之前，先把所有"已经结束"的音收掉。
/// 关键在于**结束先于起音处理**：一个音在 tick t 结束、另一个音在 tick t 起音时，
/// 两者并不重叠，不能算作同时发声。
fn overlap_peak(source_notes: &[SourceNote]) -> usize {
    let mut start_events: Vec<u64> = source_notes.iter().map(|note| note.start_tick).collect();
    let mut end_events: Vec<u64> = source_notes.iter().map(|note| note.end_tick).collect();
    start_events.sort_unstable();
    end_events.sort_unstable();

    let mut sounding_count: usize = 0;
    let mut peak_count: usize = 0;
    let mut end_cursor: usize = 0;

    for &start_tick in &start_events {
        while end_cursor < end_events.len() && end_events[end_cursor] <= start_tick {
            sounding_count = sounding_count.saturating_sub(1);
            end_cursor += 1;
        }
        sounding_count += 1;
        peak_count = peak_count.max(sounding_count);
    }

    peak_count
}
