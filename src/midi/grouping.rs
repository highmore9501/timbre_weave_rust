//! 时间重叠分组与组内归一化
//!
//! 与旧的四重奏分轨不同：那边的分组只看起始 tick，因为它允许任意叠加；
//! 本项目的乐器有**同时发音硬上限**，所以必须按"时间区间重叠"求连通分量，
//! 否则"起始不同、时值重叠"的音符会被分到不同组、各自独立决策，
//! 双双落到同一个单音乐器上，输出就是非法的。
//!
//! 归一化顺序：音域外八度折叠 → 同音去重 → 超容量时八度合并 → 仍超容量则丢弃。

use crate::midi::reader::SourceNote;
use crate::split::instrument::PitchRange;

/// 分组时使用的起始 tick 容差（`None` = 要求起始 tick 完全相同）
pub type OnsetTolerance = Option<u64>;

/// 组内归一化之后的音符（起始时间统一取所在组的锚点）
#[derive(Debug, Clone, Copy)]
pub struct GroupNote {
    /// 折叠后的音高（发声音高）
    pub key: i32,
    /// 力度（合并时取最大）
    pub velocity: u8,
    /// 结束 tick（合并时取最晚）
    pub end_tick: u64,
}

/// 归一化过程中累计的统计
#[derive(Debug, Clone, Copy, Default)]
pub struct NormalizeStats {
    /// 因落在编制并集音域之外而被八度折叠的音符数
    pub octave_folded: usize,
    /// 因同音重复或八度替换被合并掉的音符数
    pub octave_merged: usize,
    /// 被丢弃的音符数
    pub dropped: usize,
}

/// 归一化所需的编制信息
#[derive(Debug, Clone, Copy)]
pub struct NormalizeContext<'a> {
    /// 编制并集音域下界
    pub union_lo: i32,
    /// 编制并集音域上界
    pub union_hi: i32,
    /// 该编制的总同时发音容量（Σ max_polyphony）
    pub total_capacity: usize,
    /// 编制里每件乐器的可演奏音域（用于剔除"谁都演奏不了"的音）
    pub playable_ranges: &'a [PitchRange],
}

/// 按时间重叠与起始容差把音符分成"必须一起决策"的组
///
/// 规则：与当前组的锚点起始 tick 相差小于容差，**或者与组内任一音符在时间上重叠**
/// （`a.start < b.end && b.start < a.end`），就并进当前组。
pub fn group_by_overlap(notes: &[SourceNote], tolerance: OnsetTolerance) -> Vec<Vec<SourceNote>> {
    let mut sorted_notes: Vec<SourceNote> = notes.to_vec();
    sorted_notes.sort_by(|note_a, note_b| {
        note_a
            .start_tick
            .cmp(&note_b.start_tick)
            .then_with(|| note_a.key.cmp(&note_b.key))
    });

    let mut groups: Vec<Vec<SourceNote>> = Vec::new();

    for note in sorted_notes {
        let belongs_to_last = match groups.last() {
            Some(group) => {
                let anchor_tick = group[0].start_tick;
                let near_anchor = match tolerance {
                    Some(tolerance_ticks) => {
                        note.start_tick.saturating_sub(anchor_tick) < tolerance_ticks
                    }
                    None => note.start_tick == anchor_tick,
                };
                near_anchor
                    || group
                        .iter()
                        .any(|existing| overlaps(existing, &note))
            }
            None => false,
        };

        if belongs_to_last {
            if let Some(last_group) = groups.last_mut() {
                last_group.push(note);
            }
        } else {
            groups.push(vec![note]);
        }
    }

    groups
}

/// 两条音符在时间上是否重叠（半开区间相交）
fn overlaps(note_a: &SourceNote, note_b: &SourceNote) -> bool {
    note_a.start_tick < note_b.end_tick && note_b.start_tick < note_a.end_tick
}

/// 单个音符组参与分配的音符数上限（安全阀）
///
/// 正常编制（≤8 件乐器、弓弦最多 2 音）不会有这么大的同时音簇；
/// 只有音频转 midi 产生的大音簇才会撞上。超过这个值就按力度保留最大的若干个，
/// 其余丢弃并计入报告（不假装能做精确求解）。
pub const MAX_GROUP_NOTES: usize = 16;

/// 组内归一化
///
/// 按"折叠 → 合并 → 丢弃"的顺序，保证输出的组内音符数**绝不超过编制的总同时发音容量**：
/// 1. 八度折叠进编制并集音域；
/// 2. 同音去重（力度取大、结束取晚）；
/// 3. 剔除"折叠后仍然谁都演奏不了"的音（并入集音域不等于落在某件乐器的音域内）；
/// 4. 超过总容量时按"最难安置优先"做八度合并（不丢音）；
/// 5. 仍超过容量则丢弃：**先丢音域外/超出可演奏范围的**，其次丢力度小的。
pub fn normalize_group(
    group: &[SourceNote],
    context: &NormalizeContext<'_>,
    stats: &mut NormalizeStats,
) -> Vec<GroupNote> {
    let mut folded_notes: Vec<GroupNote> = Vec::with_capacity(group.len());

    for note in group {
        let folded_key = fold_into_range(note.key, context.union_lo, context.union_hi);
        if folded_key != note.key {
            stats.octave_folded += 1;
        }

        // 折叠后仍然没有任何乐器能演奏这个音 → 直接丢弃（不能让求解器失败）
        let is_playable = context
            .playable_ranges
            .iter()
            .any(|range| range.contains(folded_key));
        if !is_playable {
            stats.dropped += 1;
            continue;
        }

        // 同音去重：与已收下的同音高合并（力度取大、结束取晚）
        if let Some(existing) = folded_notes.iter_mut().find(|item| item.key == folded_key) {
            existing.velocity = existing.velocity.max(note.velocity);
            existing.end_tick = existing.end_tick.max(note.end_tick);
            stats.octave_merged += 1;
            continue;
        }

        folded_notes.push(GroupNote {
            key: folded_key,
            velocity: note.velocity,
            end_tick: note.end_tick,
        });
    }

    // 目标容量：编制总容量与安全阀取小（超大音簇不会拖垮求解器）
    let target_capacity = context.total_capacity.min(MAX_GROUP_NOTES).max(1);

    // 超容量：先尝试八度合并（不丢音）
    if folded_notes.len() > target_capacity {
        folded_notes = merge_by_octave(folded_notes, target_capacity, stats);
    }

    // 仍超容量：丢弃。优先丢掉"最不该保留"的音：
    // 先是超出可演奏范围的（音域距离最大的），其次力度小的。
    if folded_notes.len() > target_capacity {
        let ranges = context.playable_ranges;
        folded_notes.sort_by(|note_a, note_b| {
            let distance_a = playable_distance(ranges, note_a.key);
            let distance_b = playable_distance(ranges, note_b.key);
            // 距离大的排前面（先被丢），距离相同时力度小的排前面
            distance_b
                .cmp(&distance_a)
                .then_with(|| note_a.velocity.cmp(&note_b.velocity))
        });
        stats.dropped += folded_notes.len() - target_capacity;
        folded_notes.truncate(target_capacity);
        // 恢复按音高升序，便于日志阅读与状态键稳定
        folded_notes.sort_by(|note_a, note_b| note_a.key.cmp(&note_b.key));
    }

    folded_notes
}

/// 该音到"编制里任何一件乐器音域"的最近距离（落在某件乐器音域内为 0）
fn playable_distance(ranges: &[PitchRange], key: i32) -> i32 {
    ranges
        .iter()
        .map(|range| {
            if key < range.lo {
                range.lo - key
            } else if key > range.hi {
                key - range.hi
            } else {
                0
            }
        })
        .min()
        .unwrap_or(i32::MAX)
}

/// 八度折叠：把音高移进 `[range_lo, range_hi]`（区间跨度不足八度时取最近的合法八度）
fn fold_into_range(mut key: i32, range_lo: i32, range_hi: i32) -> i32 {
    if range_hi - range_lo < 12 {
        // 跨度不足一个八度：折不到里面，只能取最近的边界音
        return key.clamp(range_lo, range_hi);
    }
    while key < range_lo {
        key += 12;
    }
    while key > range_hi {
        key -= 12;
    }
    key
}

/// 按"最难安置优先"把音符折叠成组内已有音高的八度重复并合并
///
/// "最难安置"的定义：该音在组内所有音高里，能找到的八度伙伴最少、
/// 且与其它音的音程关系最不常见——这里用"能否找到八度伙伴"作为主判据，
/// 找不到伙伴的音先被跳过（无法合并，只能留到丢弃阶段）。
fn merge_by_octave(
    mut notes: Vec<GroupNote>,
    capacity: usize,
    stats: &mut NormalizeStats,
) -> Vec<GroupNote> {
    let mut merged_flags: Vec<bool> = vec![false; notes.len()];

    // 先处理"八度伙伴最少"的音（等价于最难安置）
    let mut order: Vec<usize> = (0..notes.len()).collect();
    order.sort_by_key(|&note_index| octave_partner_count(&notes, note_index));

    for &note_index in &order {
        if notes.len() <= capacity {
            break;
        }
        if merged_flags[note_index] {
            continue;
        }
        let partner = (0..notes.len()).find(|&other_index| {
            other_index != note_index
                && !merged_flags[other_index]
                && (notes[other_index].key - notes[note_index].key) % 12 == 0
        });
        let Some(partner_index) = partner else {
            continue;
        };

        // 合并进伙伴（力度取大、结束取晚），并把它标记为已用掉
        let donor = notes[note_index];
        notes[partner_index].velocity = notes[partner_index].velocity.max(donor.velocity);
        notes[partner_index].end_tick = notes[partner_index].end_tick.max(donor.end_tick);
        merged_flags[note_index] = true;
        stats.octave_merged += 1;
    }

    notes
        .into_iter()
        .enumerate()
        .filter_map(|(note_index, note)| (!merged_flags[note_index]).then_some(note))
        .collect()
}

/// 该音在组内有多少个八度伙伴（用于"最难安置优先"排序）
fn octave_partner_count(notes: &[GroupNote], note_index: usize) -> usize {
    notes
        .iter()
        .enumerate()
        .filter(|(other_index, other)| {
            *other_index != note_index && (other.key - notes[note_index].key) % 12 == 0
        })
        .count()
}
