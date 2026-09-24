//! 源 midi 读取：把选中的轨道解析成绝对 tick 的音符列表
//!
//! 收集口径与 `string_flow_rust` 的既有实现保持一致（换项目但不换语义）：
//! - `velocity == 0` 的 NoteOn 视作 NoteOff；
//! - 同一 (channel, key) 再次 NoteOn 时先把上一个音收尾，避免同音重叠；
//! - 轨道结束时仍在发声的音符以轨尾时间收尾；
//! - 时值为 0 的音符直接丢弃。
//!
//! 注意 midi 文件里的音高**一律是发声音高**（concert pitch）：
//! SMF 没有移调语义，Cubase 的 Display Transpose 只影响显示、不影响播放。

use midly::{Smf, TrackEventKind};

/// 从源 midi 读入的一条音符（绝对 tick）
#[derive(Debug, Clone, Copy)]
pub struct SourceNote {
    /// midi 音高（发声音高）
    pub key: i32,
    /// 力度
    pub velocity: u8,
    /// 绝对起始 tick
    pub start_tick: u64,
    /// 绝对结束 tick
    pub end_tick: u64,
}

/// 正在发声的源音符（用于收尾）
#[derive(Debug, Clone, Copy)]
struct ActiveSourceNote {
    key: u8,
    channel: u8,
    velocity: u8,
    start_tick: u64,
}

/// 一个已解析的 midi 文件
///
/// `midly::Smf` 借用输入缓冲，所以这里必须自己持有字节缓冲；
/// 析构顺序由 [`Drop`] 保证（先释放 `smf`，再释放 `bytes`）。
pub struct LoadedMidi {
    /// 原始文件字节：`smf` 借用它，所以必须持有；字段顺序不能调整
    #[allow(dead_code)]
    bytes: Vec<u8>,
    /// 解析结果
    smf: Smf<'static>,
    /// 每拍 tick 数（非 metrical timing 时回退 480）
    pub ticks_per_beat: u16,
}

impl LoadedMidi {
    /// 访问解析后的 smf
    pub fn smf(&self) -> &Smf<'static> {
        &self.smf
    }

    /// 轨道数
    pub fn track_count(&self) -> usize {
        self.smf.tracks.len()
    }
}

impl Drop for LoadedMidi {
    fn drop(&mut self) {
        // 先手动丢弃借用 bytes 的 smf，避免字段析构顺序变化时出现悬垂引用
        let empty: Smf<'static> = Smf {
            header: self.smf.header,
            tracks: Vec::new(),
        };
        let _ = std::mem::replace(&mut self.smf, empty);
    }
}

/// 一条轨道的扫描摘要
#[derive(Debug, Clone, Default)]
pub struct TrackInfo {
    /// 轨道下标
    pub track_index: usize,
    /// 轨道名（优先用 TrackName meta，其次 InstrumentName，最后退化为 "Track N"）
    pub track_name: String,
    /// 该轨上的音符数（按 `channel_filter` 过滤后）
    pub note_count: usize,
}

/// 扫描全部轨道，返回每条轨的摘要
///
/// 走的是与 [`read_track_notes`] 完全相同的收集口径，
/// 所以"扫描时看到的音符数"与"实际参与分配的音符数"一致。
pub fn scan_tracks(smf: &Smf<'_>, channel_filter: i32) -> Vec<TrackInfo> {
    let mut track_list: Vec<TrackInfo> = Vec::with_capacity(smf.tracks.len());

    for track_index in 0..smf.tracks.len() {
        let notes = read_track_notes(smf, &[track_index], channel_filter);
        track_list.push(TrackInfo {
            track_index,
            track_name: track_display_name(smf, track_index),
            note_count: notes.len(),
        });
    }

    track_list
}

/// 取轨道显示名
fn track_display_name(smf: &Smf<'_>, track_index: usize) -> String {
    let mut track_name: Option<String> = None;
    let mut instrument_name: Option<String> = None;

    for event in &smf.tracks[track_index] {
        match &event.kind {
            TrackEventKind::Meta(midly::MetaMessage::TrackName(bytes)) => {
                if track_name.is_none() {
                    track_name = Some(decode_text(bytes));
                }
            }
            TrackEventKind::Meta(midly::MetaMessage::InstrumentName(bytes)) => {
                if instrument_name.is_none() {
                    instrument_name = Some(decode_text(bytes));
                }
            }
            _ => {}
        }
    }

    track_name
        .or(instrument_name)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("Track {}", track_index + 1))
}

/// meta 文本解码（midi 的文本字段多为 latin-1 或 utf-8，做一次宽松回退）
fn decode_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|byte| *byte as char).collect(),
    }
}

/// 读取并解析一个 midi 文件
pub fn load_midi_file(path: &str) -> Result<LoadedMidi, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;

    // SAFETY: `bytes` 的所有权交给返回值，并在本类型的 Drop 里保证
    // “先丢弃 smf、再释放 bytes”，因此 smf 里的借用不会超过 bytes 的生命周期。
    // 指向的数据在移动过程中不会变化（Vec 的堆缓冲地址稳定）。
    let smf: Smf<'static> = unsafe { std::mem::transmute(Smf::parse(&bytes).map_err(|e| e.to_string())?) };
    let ticks_per_beat = match smf.header.timing {
        midly::Timing::Metrical(ticks) => ticks.as_int(),
        _ => 480,
    };

    Ok(LoadedMidi {
        bytes,
        smf,
        ticks_per_beat,
    })
}

/// 读取指定轨道上的全部音符（绝对 tick）
///
/// `track_indices` 是要作为源轨的轨道下标；`channel_filter` 为 `-1` 表示使用全部通道。
pub fn read_track_notes(
    smf: &Smf<'_>,
    track_indices: &[usize],
    channel_filter: i32,
) -> Vec<SourceNote> {
    let mut notes: Vec<SourceNote> = Vec::new();

    for &track_index in track_indices {
        if track_index >= smf.tracks.len() {
            continue;
        }

        let track = &smf.tracks[track_index];
        let mut absolute_tick: u64 = 0;
        let mut active: Vec<ActiveSourceNote> = Vec::new();

        for event in track {
            absolute_tick += event.delta.as_int() as u64;

            let TrackEventKind::Midi { channel, message } = event.kind else {
                continue;
            };
            let channel_value = channel.as_int();
            if channel_filter != -1 && channel_filter != channel_value as i32 {
                continue;
            }

            match message {
                midly::MidiMessage::NoteOn { key, vel } => {
                    let key_value = key.as_int();
                    if vel.as_int() > 0 {
                        close_active_source_note(
                            &mut active,
                            &mut notes,
                            channel_value,
                            key_value,
                            absolute_tick,
                        );
                        active.push(ActiveSourceNote {
                            key: key_value,
                            channel: channel_value,
                            velocity: vel.as_int(),
                            start_tick: absolute_tick,
                        });
                    } else {
                        close_active_source_note(
                            &mut active,
                            &mut notes,
                            channel_value,
                            key_value,
                            absolute_tick,
                        );
                    }
                }
                midly::MidiMessage::NoteOff { key, .. } => {
                    close_active_source_note(
                        &mut active,
                        &mut notes,
                        channel_value,
                        key.as_int(),
                        absolute_tick,
                    );
                }
                _ => {}
            }
        }

        // 轨尾兜底：仍在发声的音符以轨道末尾时间结束
        for note in active {
            if absolute_tick > note.start_tick {
                notes.push(SourceNote {
                    key: note.key as i32,
                    velocity: note.velocity,
                    start_tick: note.start_tick,
                    end_tick: absolute_tick,
                });
            }
        }
    }

    notes
}

/// 收尾一个正在发声的源音符（找不到就忽略）
fn close_active_source_note(
    active: &mut Vec<ActiveSourceNote>,
    notes: &mut Vec<SourceNote>,
    channel: u8,
    key: u8,
    end_tick: u64,
) {
    let Some(position) = active
        .iter()
        .position(|note| note.key == key && note.channel == channel)
    else {
        return;
    };
    let note = active.remove(position);
    if end_tick > note.start_tick {
        notes.push(SourceNote {
            key: note.key as i32,
            velocity: note.velocity,
            start_tick: note.start_tick,
            end_tick,
        });
    }
}
