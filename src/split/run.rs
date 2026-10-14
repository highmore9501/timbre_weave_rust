//! 分轨总流程：读源轨 → 分组 → 归一化 → 分配 → 写出 → 汇总

use crate::midi::grouping::{
    MAX_GROUP_NOTES, NormalizeContext, NormalizeStats, group_by_onset, normalize_group,
};
use crate::midi::reader::{load_midi_file, read_track_notes};
use crate::midi::writer::{
    OutputNote, VoiceTrackSpec, build_conductor_track, build_voice_track, write_split_midi,
};
use crate::split::assign::{DEFAULT_POOL_SIZE, assign_group};
use crate::split::cost::range_cost;
use crate::split::instrument::{EnsembleSlot, crowded_channels, ensemble_union_range};
use crate::split::report::{SlotReport, SplitReport, StageTimings};

/// 分轨请求
#[derive(Debug, Clone)]
pub struct SplitRequest {
    /// 源 midi 文件路径
    pub midi_path: String,
    /// 作为源轨的轨道下标
    pub track_indices: Vec<usize>,
    /// 使用通道（-1 = 全部通道）
    pub channel_filter: i32,
    /// 起始 tick 容差（ticks；`None` = 要求起始 tick 完全相同）
    pub onset_tolerance: Option<u64>,
    /// 本次编制
    pub ensemble: Vec<EnsembleSlot>,
    /// 候选池大小（越大越接近穷举，代价是更慢）
    pub pool_size: usize,
}

impl SplitRequest {
    /// 候选池大小的实际取值
    fn effective_pool_size(&self) -> usize {
        if self.pool_size == 0 {
            DEFAULT_POOL_SIZE
        } else {
            self.pool_size
        }
    }
}

/// 分轨错误
#[derive(Debug)]
pub enum SplitError {
    /// 源轨里没有可用音符
    Empty,
    /// 没有启用任何乐器
    NoInstrument,
    /// midi 读写失败
    Io(String),
}

impl std::fmt::Display for SplitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitError::Empty => write!(formatter, "{}", crate::t!("msg.split_empty")),
            SplitError::NoInstrument => write!(formatter, "{}", crate::t!("msg.no_instrument_selected")),
            SplitError::Io(message) => write!(formatter, "{}", message),
        }
    }
}

impl std::error::Error for SplitError {}

/// 一条待写出的声部音符
#[derive(Debug, Clone)]
struct VoiceOutputNote {
    key: i32,
    velocity: u8,
    start_tick: u64,
    end_tick: u64,
}

/// 执行一次分轨
pub fn run_split(request: &SplitRequest) -> Result<SplitReport, SplitError> {
    let overall_start = std::time::Instant::now();

    let active_ensemble: Vec<EnsembleSlot> = request
        .ensemble
        .iter()
        .filter(|slot| slot.enabled)
        .cloned()
        .collect();
    if active_ensemble.is_empty() {
        return Err(SplitError::NoInstrument);
    }

    let Some(union_range) = ensemble_union_range(&active_ensemble) else {
        return Err(SplitError::NoInstrument);
    };

    // === 1. 读源轨（计时：解析 + 收集音符） ===
    let stage_start = std::time::Instant::now();
    let loaded = load_midi_file(&request.midi_path).map_err(SplitError::Io)?;
    let source_notes = read_track_notes(loaded.smf(), &request.track_indices, request.channel_filter);
    let read_micros = stage_start.elapsed().as_secs_f64() * 1_000_000.0;
    if source_notes.is_empty() {
        return Err(SplitError::Empty);
    }

    crate::log_info!(crate::t!(
        "log.split.start",
        request.track_indices.len(),
        source_notes.len(),
        active_ensemble.len()
    ));

    // === 2. 分组（计时） ===
    let stage_start = std::time::Instant::now();
    let groups = group_by_onset(&source_notes, request.onset_tolerance);
    let grouping_micros = stage_start.elapsed().as_secs_f64() * 1_000_000.0;

    // 编制里每件乐器的可演奏音域（归一化阶段用来剔除"谁都演奏不了"的音）
    let playable_ranges: Vec<crate::split::instrument::PitchRange> = active_ensemble
        .iter()
        .map(|slot| slot.playable)
        .collect();

    // === 3. 逐组归一化 + 分配（分别计时） ===
    let normalize_context = NormalizeContext {
        union_lo: union_range.lo,
        union_hi: union_range.hi,
        playable_ranges: &playable_ranges,
    };
    let mut normalize_stats = NormalizeStats::default();
    let mut voice_notes: Vec<Vec<VoiceOutputNote>> = vec![Vec::new(); active_ensemble.len()];
    let mut slot_doubled_notes: Vec<usize> = vec![0; active_ensemble.len()];
    // 每件乐器"此刻仍占着"的音（结束 tick, 音高）：分配完一组把新音挂上，进下一组前清掉已经结束的。
    // 这就是取代"按时间重叠连通分组"的那份时间信息。
    let mut slot_pending: Vec<Vec<(u64, i32)>> = vec![Vec::new(); active_ensemble.len()];
    let mut normalize_micros: f64 = 0.0;
    let mut assign_micros: f64 = 0.0;
    let mut report = SplitReport {
        source_notes: source_notes.len(),
        onset_groups: groups.len(),
        ..SplitReport::default()
    };

    for group in &groups {
        let anchor_tick = group[0].start_tick;

        // 先摘掉此刻已经结束的音，剩下的就是与本组同时发声、必须算进同时发音数的占用
        let occupied_keys: Vec<Vec<i32>> = slot_pending
            .iter_mut()
            .map(|pending| {
                pending.retain(|(end_tick, _key)| *end_tick > anchor_tick);
                pending.iter().map(|(_end_tick, key)| *key).collect()
            })
            .collect();
        let available_capacity: usize = active_ensemble
            .iter()
            .enumerate()
            .map(|(slot_index, slot)| {
                (slot.max_polyphony() as usize).saturating_sub(occupied_keys[slot_index].len())
            })
            .sum();

        let stage_start = std::time::Instant::now();
        let normalized = normalize_group(
            group,
            &normalize_context,
            available_capacity,
            &mut normalize_stats,
        );
        normalize_micros += stage_start.elapsed().as_secs_f64() * 1_000_000.0;

        let stage_start = std::time::Instant::now();
        let assignment_result = assign_group(
            &normalized,
            &active_ensemble,
            &occupied_keys,
            request.effective_pool_size(),
        );
        assign_micros += stage_start.elapsed().as_secs_f64() * 1_000_000.0;

        match assignment_result {
            Some(assignment) => {
                // 候选池诊断：只有真的发生过剪枝/强制安置才提示，不静默降级
                report.pool_expansions += assignment.diagnostics.expansions;
                report.pool_peak_size = report.pool_peak_size.max(assignment.diagnostics.peak_pool_size);
                if assignment.diagnostics.saturated {
                    report.pool_saturated_groups += 1;
                }
                report.forced_notes += assignment.diagnostics.forced_notes;
                if assignment.doubled_notes > 0 {
                    report.doubling_groups += 1;
                    report.doubled_notes += assignment.doubled_notes;
                }
                for (slot_index, note_indices) in assignment.voice_note_indices.iter().enumerate() {
                    if !note_indices.is_empty() {
                        // 这一刻该乐器本来还占着音 → 本组这几个音全算叠音；
                        // 否则只有第二个音起才算叠音（弦乐双音）。
                        let held_notes = occupied_keys[slot_index].len();
                        slot_doubled_notes[slot_index] += if held_notes > 0 {
                            note_indices.len()
                        } else {
                            note_indices.len() - 1
                        };
                    }
                    for &note_index in note_indices {
                        let note = normalized[note_index];
                        voice_notes[slot_index].push(VoiceOutputNote {
                            key: note.key,
                            velocity: note.velocity,
                            start_tick: anchor_tick,
                            end_tick: note.end_tick,
                        });
                        slot_pending[slot_index].push((note.end_tick, note.key));
                    }
                }
            }
            None => {
                // 这条分支理论上不可达：归一化已按"此刻腾得出的容量"卡过、且每个音都有乐器能演。
                // 真到了这里，把整组按力度丢音（**不**硬塞给某件乐器——那会突破 max_polyphony，
                // 正是"单声部乐器出现叠音"的根源）。
                if !report.used_fallback {
                    crate::log_warn!(crate::t!("log.split.unassignable_group"));
                }
                report.used_fallback = true;
                report.unplaced_notes += normalized.len();
            }
        }
    }

    report.octave_folded = normalize_stats.octave_folded;
    report.pitch_merged = normalize_stats.pitch_merged;
    report.octave_merged = normalize_stats.octave_merged;
    report.dropped_notes = normalize_stats.dropped;

    // 安全阀提示：超大音簇会被截断（normalize_group 已经在统计里记了丢弃数）
    if groups.iter().any(|group| group.len() > MAX_GROUP_NOTES) {
        crate::log_warn!(crate::t!("log.split.overflow"));
    }
    // 候选池剪枝提示：出现过说明结果不保证全局最优，把池调大可以换精度
    if report.pool_saturated_groups > 0 {
        crate::log_warn!(crate::t!(
            "log.split.pool_saturated",
            report.pool_saturated_groups,
            request.effective_pool_size()
        ));
    }
    // 强制安置提示：有音被塞进音域外的乐器（说明编制偏小或音域不合）
    if report.forced_notes > 0 {
        crate::log_warn!(crate::t!("log.split.forced_notes", report.forced_notes));
    }

    // === 4. 构建并写出 midi ===
    let conductor_track = build_conductor_track(loaded.smf());
    let mut voice_tracks: Vec<midly::Track<'static>> = Vec::with_capacity(active_ensemble.len());
    let mut slot_reports: Vec<SlotReport> = Vec::with_capacity(active_ensemble.len());

    for (slot_index, slot) in active_ensemble.iter().enumerate() {
        let spec = VoiceTrackSpec {
            track_name: slot.track_name(),
            instrument_name: slot.track_name(),
            gm_program: slot.gm_program(),
            channel: slot.channel,
        };
        let output_notes: Vec<OutputNote> = voice_notes[slot_index]
            .iter()
            .map(|note| OutputNote {
                key: note.key.clamp(0, 127) as u8,
                velocity: note.velocity,
                start_tick: note.start_tick,
                end_tick: note.end_tick,
            })
            .collect();
        let (track, written) = build_voice_track(&spec, &output_notes);

        slot_reports.push(SlotReport {
            track_name: spec.track_name,
            note_count: written,
            share: 0.0,
            lowest_key: output_notes.iter().map(|note| note.key as i32).min(),
            highest_key: output_notes.iter().map(|note| note.key as i32).max(),
            doubled_notes: slot_doubled_notes[slot_index],
            outside_comfort: voice_notes[slot_index]
                .iter()
                .filter(|note| range_cost(slot, note.key).is_some_and(|cost| cost > 0.0))
                .count(),
        });

        report.written_notes += written;
        voice_tracks.push(track);
    }

    // 计算各乐器占比
    if report.written_notes > 0 {
        for slot_report in slot_reports.iter_mut() {
            slot_report.share = slot_report.note_count as f64 / report.written_notes as f64;
        }
    }
    report.slots = slot_reports;

    // 通道复用检查：编制超过 15 件时必然发生（midi 只有 15 个可用通道）
    report.shared_channels = crowded_channels(&active_ensemble)
        .into_iter()
        .map(|(channel, count)| (channel + 1, count))
        .collect();
    if !report.shared_channels.is_empty() {
        let detail: Vec<String> = report
            .shared_channels
            .iter()
            .map(|(channel, count)| format!("{}号通道×{}", channel, count))
            .collect();
        crate::log_warn!(crate::t!("log.split.shared_channels", detail.join("、")));
    }

    let output_path = split_output_path(&request.midi_path);
    if let Some(parent) = std::path::Path::new(&output_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| SplitError::Io(error.to_string()))?;
        }
    }

    // 计时：构建事件 + 序列化 + 落盘
    let stage_start = std::time::Instant::now();
    let file_bytes = write_split_midi(conductor_track, voice_tracks, loaded.smf().header.timing)
        .map_err(SplitError::Io)?;
    std::fs::write(&output_path, file_bytes).map_err(|error| SplitError::Io(error.to_string()))?;
    let write_micros = stage_start.elapsed().as_secs_f64() * 1_000_000.0;
    report.output_path = output_path;

    report.timings = StageTimings {
        read_micros,
        grouping_micros,
        normalize_micros,
        assign_micros,
        write_micros,
        total_micros: overall_start.elapsed().as_secs_f64() * 1_000_000.0,
    };

    // === 5. 日志 ===
    crate::log_info!(crate::t!(
        "log.split.stats",
        report.onset_groups,
        report.written_notes,
        report.octave_folded,
        report.pitch_merged,
        report.octave_merged,
        report.dropped_notes
    ));
    crate::log_info!(crate::t!("log.split.slots", report.slot_counts_text()));
    crate::log_info!(crate::t!("log.split.timings", report.timings.summary_text()));
    if report.doubling_groups > 0 {
        crate::log_info!(crate::t!(
            "log.split.doubling",
            report.doubling_groups,
            report.doubled_notes
        ));
    }
    crate::log_ok!(crate::t!("log.split.output", &report.output_path));

    Ok(report)
}

/// 输出路径：与源文件同目录，`<源文件名去扩展名>_split.mid`
pub fn split_output_path(original_path: &str) -> String {
    let last_separator = original_path.rfind(|c| c == '/' || c == '\\');
    let dot_index = original_path
        .rfind('.')
        .filter(|dot| last_separator.is_none_or(|separator| *dot > separator));
    match dot_index {
        Some(dot) => format!("{}_split{}", &original_path[..dot], &original_path[dot..]),
        None => format!("{}_split.mid", original_path),
    }
}
