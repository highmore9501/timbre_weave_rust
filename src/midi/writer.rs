//! 多轨 midi 写出（Format 1：0 号指挥轨 + 每件乐器一条轨）
//!
//! 依据 SMF 规范与 DAW 惯例：
//! - 0 号轨是 global tempo track，只放 timing/meta，**不放通道事件**；
//! - 每条乐器轨单通道，事件序为 `TrackName` → `InstrumentName` → `ProgramChange`
//!   → 音符事件 → `EndOfTrack`；
//! - 音高一律写**发声音高**（midi 没有移调语义）。

use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, Track, TrackEvent, TrackEventKind};
use std::collections::HashSet;

/// 待写出的一个音符
#[derive(Debug, Clone, Copy)]
pub struct OutputNote {
    /// midi 音高（发声音高）
    pub key: u8,
    /// 力度
    pub velocity: u8,
    /// 绝对起始 tick
    pub start_tick: u64,
    /// 绝对结束 tick
    pub end_tick: u64,
}

/// 一条乐器轨的写出参数
#[derive(Debug, Clone)]
pub struct VoiceTrackSpec {
    /// 轨道名（如 "Violin 1"）
    pub track_name: String,
    /// 乐器名（`InstrumentName` meta）
    pub instrument_name: String,
    /// 0 基 GM 音色号
    pub gm_program: u8,
    /// 输出通道（0–15，9 为 GM 打击乐保留位）
    pub channel: u8,
}

/// 轨内事件（同 tick 时按 kind 排序：Program < NoteOff < NoteOn）
enum VoiceEvent {
    Program,
    NoteOff { key: u8 },
    NoteOn { key: u8, velocity: u8 },
}

/// 把音符写进一条乐器轨，返回（轨道事件, 实际写出的音符数）
///
/// 轨道名 / 乐器名要写进 `TrackName` / `InstrumentName` meta，而 `midly` 的
/// `Track<'static>` 要求事件借用 `'static` 数据。这里是**刻意**的小体量泄漏：
/// 每次分轨每条轨最多泄漏两个短字符串（编制上限 8 条轨），换取整条写出链路的生命周期简单。
pub fn build_voice_track(spec: &VoiceTrackSpec, notes: &[OutputNote]) -> (Track<'static>, usize) {
    let mut sorted_notes: Vec<OutputNote> = notes.to_vec();
    sorted_notes.sort_by(|note_a, note_b| {
        note_a
            .start_tick
            .cmp(&note_b.start_tick)
            .then_with(|| note_a.key.cmp(&note_b.key))
    });

    // 同轨同音高重触发：先把上一个音收尾，避免重叠
    let mut last_index_by_key: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();
    for note_index in 0..sorted_notes.len() {
        let key = sorted_notes[note_index].key;
        if let Some(&previous_index) = last_index_by_key.get(&key) {
            if sorted_notes[note_index].start_tick < sorted_notes[previous_index].end_tick {
                sorted_notes[previous_index].end_tick = sorted_notes[note_index].start_tick;
            }
        }
        last_index_by_key.insert(key, note_index);
    }

    let mut timed_events: Vec<(u64, u8, VoiceEvent)> = Vec::with_capacity(sorted_notes.len() * 2 + 3);
    timed_events.push((0, 0, VoiceEvent::Program));
    for note in &sorted_notes {
        if note.end_tick <= note.start_tick {
            // 被裁剪成零时值的音符没有意义，跳过
            continue;
        }
        timed_events.push((
            note.start_tick,
            2,
            VoiceEvent::NoteOn {
                key: note.key,
                velocity: note.velocity,
            },
        ));
        timed_events.push((note.end_tick, 1, VoiceEvent::NoteOff { key: note.key }));
    }
    timed_events.sort_by(|event_a, event_b| {
        event_a
            .0
            .cmp(&event_b.0)
            .then_with(|| event_a.1.cmp(&event_b.1))
    });

    let written_notes = timed_events
        .iter()
        .filter(|(_, _, event)| matches!(event, VoiceEvent::NoteOn { .. }))
        .count();

    let channel = midly::num::u4::new(spec.channel);
    // 见本函数文档：这里刻意把两个短字符串泄漏成 'static，保证写出链路生命周期简单
    let track_name: &'static [u8] = Box::leak(spec.track_name.clone().into_bytes().into_boxed_slice());
    let instrument_name: &'static [u8] =
        Box::leak(spec.instrument_name.clone().into_bytes().into_boxed_slice());

    let mut track: Track<'static> = Vec::with_capacity(timed_events.len() + 4);
    track.push(TrackEvent {
        delta: midly::num::u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::TrackName(track_name)),
    });
    track.push(TrackEvent {
        delta: midly::num::u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::InstrumentName(instrument_name)),
    });

    let mut last_tick: u64 = 0;
    for (tick, _, event) in timed_events {
        let delta = tick.saturating_sub(last_tick) as u32;
        last_tick = tick;

        let message = match event {
            VoiceEvent::Program => MidiMessage::ProgramChange {
                program: midly::num::u7::new(spec.gm_program),
            },
            VoiceEvent::NoteOn { key, velocity } => MidiMessage::NoteOn {
                key: midly::num::u7::new(key),
                vel: midly::num::u7::new(velocity),
            },
            VoiceEvent::NoteOff { key } => MidiMessage::NoteOff {
                key: midly::num::u7::new(key),
                vel: midly::num::u7::new(0),
            },
        };

        track.push(TrackEvent {
            delta: midly::num::u28::new(delta),
            kind: TrackEventKind::Midi { channel, message },
        });
    }

    track.push(TrackEvent {
        delta: midly::num::u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });

    (track, written_notes)
}

/// 构建 0 号指挥轨：汇总源文件全部轨道上的 tempo / 拍号 / 调号（按绝对 tick 排序去重）
///
/// 只重建这三类 meta 事件（它们都不借用外部数据），所以输出轨道是 `'static`，
/// 不需要把源缓冲的生命周期带出去。
pub fn build_conductor_track(smf: &Smf<'_>) -> Track<'static> {
    let mut meta_events: Vec<(u64, TrackEventKind<'static>)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for track in &smf.tracks {
        let mut absolute_tick: u64 = 0;
        for event in track {
            absolute_tick += event.delta.as_int() as u64;

            let owned_kind: TrackEventKind<'static> = match event.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(microseconds_per_beat)) => {
                    TrackEventKind::Meta(MetaMessage::Tempo(microseconds_per_beat))
                }
                TrackEventKind::Meta(MetaMessage::TimeSignature(
                    numerator,
                    denominator_power,
                    clocks_per_click,
                    thirty_seconds_per_quarter,
                )) => TrackEventKind::Meta(MetaMessage::TimeSignature(
                    numerator,
                    denominator_power,
                    clocks_per_click,
                    thirty_seconds_per_quarter,
                )),
                TrackEventKind::Meta(MetaMessage::KeySignature(sharps_or_flats, is_minor)) => {
                    TrackEventKind::Meta(MetaMessage::KeySignature(sharps_or_flats, is_minor))
                }
                _ => continue,
            };

            // 多轨里可能出现重复的同一 meta，去重避免指挥轨里出现重复事件
            let signature = format!("{}:{:?}", absolute_tick, owned_kind);
            if seen.insert(signature) {
                meta_events.push((absolute_tick, owned_kind));
            }
        }
    }

    meta_events.sort_by_key(|(tick, _)| *tick);

    let mut track: Track<'static> = Vec::with_capacity(meta_events.len() + 1);
    let mut last_tick: u64 = 0;
    for (tick, kind) in meta_events {
        track.push(TrackEvent {
            delta: midly::num::u28::new(tick.saturating_sub(last_tick) as u32),
            kind,
        });
        last_tick = tick;
    }
    track.push(TrackEvent {
        delta: midly::num::u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    track
}

/// 组装完整的输出文件字节
pub fn write_split_midi(
    conductor_track: Track<'static>,
    voice_tracks: Vec<Track<'static>>,
    timing: Timing,
) -> Result<Vec<u8>, String> {
    let mut tracks: Vec<Track<'static>> = Vec::with_capacity(voice_tracks.len() + 1);
    tracks.push(conductor_track);
    tracks.extend(voice_tracks);

    let smf = Smf {
        header: midly::Header {
            format: Format::Parallel,
            timing,
        },
        tracks,
    };

    let mut buffer = Vec::new();
    smf.write_std(&mut buffer).map_err(|error| error.to_string())?;
    Ok(buffer)
}
