//! 同时发声分组与组内归一化
//!
//! 分组口径是**起点**：起点相差小于量化容差的音符算"同时发声"，必须在同一组里一起决策；
//! 起点不同的音符各成一组。时值互相重叠、但起点不同的音符**不需要**塞进同一组——
//! 分配是按时间先后逐组做的，后一组做决策时已经知道"每件乐器此刻还占着哪些音高"
//! （见 `split::run` 的占用表），所以不会把两个音叠到同一件单音乐器上。
//!
//! （早期实现按"时间重叠连通"分组，会把一整串琶音并成一组，带来三个后果：
//!   ① 同音去重会误杀同一组里前后相隔很远的重复音；
//!   ② 容量判定拿"整串音符数"去比"同时音上限"，本不需要合并的地方也被合并；
//!   ③ 写出时整组音符被统一挪到组首时刻、音尾却各保留原值，音符被拉长成一坨和弦。
//!   现在这三件事都不会发生。）
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
    /// 因与组内同音高的音重复而被合并掉的音符数（同一个时刻的重复音）
    pub pitch_merged: usize,
    /// 因超过此刻的同时发音容量、被并进组内八度伙伴的音符数
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
    /// 编制里每件乐器的可演奏音域（用于剔除"谁都演奏不了"的音）
    pub playable_ranges: &'a [PitchRange],
}

/// 按**起始 tick** 把音符分成"同时发声"的组
///
/// 规则：与当前组的锚点起始 tick 相差小于容差就并进当前组，否则另起一组；
/// 容差为 `None` 时要求起始 tick 完全相同。
///
/// 判定只看起点、不看时值：一条长音和它之后陆续起音的旋律音分属不同的组，
/// 它们之间的"同时发声"关系由分配阶段的乐器占用表处理。
pub fn group_by_onset(notes: &[SourceNote], tolerance: OnsetTolerance) -> Vec<Vec<SourceNote>> {
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
                match tolerance {
                    Some(tolerance_ticks) => {
                        note.start_tick.saturating_sub(anchor_tick) < tolerance_ticks
                    }
                    None => note.start_tick == anchor_tick,
                }
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

/// 单个音符组参与分配的音符数上限（安全阀）
///
/// 正常编制（≤8 件乐器、弓弦最多 2 音）不会有这么大的同时音簇；
/// 只有音频转 midi 产生的大音簇才会撞上。超过这个值就按力度保留最大的若干个，
/// 其余丢弃并计入报告（不假装能做精确求解）。
pub const MAX_GROUP_NOTES: usize = 16;

/// 组内归一化
///
/// `available_capacity` 是**此刻**编制还能腾出的同时发音位总数
/// （Σ 每件乐器上限 − 它当前仍占着的音数），由调用方按组起点算好传进来。
///
/// 顺序：
/// 1. 八度折叠进编制并集音域；
/// 2. 剔除"折叠后仍然谁都演奏不了"的音（并入集音域不等于落在某件乐器的音域内）；
/// 3. 同音去重（同一个时刻的同音重复，力度取大、结束取晚）；
/// 4. 音符数超过 `available_capacity` 时按"最难安置优先"做八度合并（不丢音）；
/// 5. 仍超过容量则丢弃：**先丢音域外/超出可演奏范围的**，其次丢力度小的。
pub fn normalize_group(
    group: &[SourceNote],
    context: &NormalizeContext<'_>,
    available_capacity: usize,
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

        // 同音去重：本组是"同时发声"的一组，所以同音高的音确实是同一个时刻的重复音
        if let Some(existing) = folded_notes
            .iter_mut()
            .find(|existing_note| existing_note.key == folded_key)
        {
            existing.velocity = existing.velocity.max(note.velocity);
            existing.end_tick = existing.end_tick.max(note.end_tick);
            stats.pitch_merged += 1;
            continue;
        }

        folded_notes.push(GroupNote {
            key: folded_key,
            velocity: note.velocity,
            end_tick: note.end_tick,
        });
    }

    // 目标容量：此刻还腾得出的同时发音位与安全阀取小（超大音簇不会拖垮求解器）
    let target_capacity = available_capacity.min(MAX_GROUP_NOTES);
    if target_capacity == 0 {
        // 此刻一件乐器都腾不出来：本组的音无论怎么分都会超出上限，只能丢弃
        stats.dropped += folded_notes.len();
        return Vec::new();
    }

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
/// 只在**同一时刻的音符数超过此刻的容量**时才会被调用，所以合并掉的音是
/// "这一刻实在放不下"的音，不会误伤前面或后面起音的音符。
///
/// "最难安置"的定义：该音在组内所有音高里，能找到的八度伙伴最少——
/// 找不到伙伴的音先被跳过（无法合并，只能留到丢弃阶段）。
fn merge_by_octave(
    mut notes: Vec<GroupNote>,
    capacity: usize,
    stats: &mut NormalizeStats,
) -> Vec<GroupNote> {
    let mut merged_flags: Vec<bool> = vec![false; notes.len()];

    // 先处理"八度伙伴最少"的音（等价于最难安置）
    let mut placement_order: Vec<usize> = (0..notes.len()).collect();
    placement_order.sort_by_key(|&note_index| octave_partner_count(&notes, note_index));

    for &note_index in &placement_order {
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
