//! 乐器预置表与编制
//!
//! 音域与舒适区的数据依据见 `documentation/乐器音域舒适区数据表.md`。
//! 本文件是**唯一**的乐器数据落点：改数据只改这里，并同步改那份文档。
//!
//! 关键口径：
//! - 全部音高都是**发声音高**（concert pitch）；
//! - `playable` 是硬边界（范围外不成组合），取"所有专业都能做到 / 管弦乐可写"的口径；
//! - `comfortable` 是代价 0 区间，超出后按距离扣分；
//! - `max_polyphony` 是硬上限（弓弦 2、管乐 1、打击乐 1），不开放用户修改。

/// 一个闭区间音域（发声音高，midi 编号）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchRange {
    pub lo: i32,
    pub hi: i32,
}

impl PitchRange {
    pub const fn new(lo: i32, hi: i32) -> Self {
        PitchRange { lo, hi }
    }

    /// 该音高是否落在区间内
    pub fn contains(&self, key: i32) -> bool {
        key >= self.lo && key <= self.hi
    }

    /// 区间中点（用于同代价时的细分排序）
    pub fn center(&self) -> f64 {
        (self.lo as f64 + self.hi as f64) / 2.0
    }
}

/// 舒适区的可信度：有来源 vs 推断（UI 上要区分，不与有来源的等权使用）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComfortSource {
    /// 来源明确给出了 tessitura / practical range
    Source,
    /// 按配器常识推断
    Inferred,
}

impl ComfortSource {
    /// i18n 词典键
    pub fn i18n_key(&self) -> &'static str {
        match self {
            ComfortSource::Source => "ui.ensemble.comfort_source.source",
            ComfortSource::Inferred => "ui.ensemble.comfort_source.inferred",
        }
    }
}

/// 乐器族
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentFamily {
    Woodwind,
    Brass,
    Strings,
    Percussion,
}

impl InstrumentFamily {
    /// i18n 词典键
    pub fn i18n_key(&self) -> &'static str {
        match self {
            InstrumentFamily::Woodwind => "family.woodwind",
            InstrumentFamily::Brass => "family.brass",
            InstrumentFamily::Strings => "family.strings",
            InstrumentFamily::Percussion => "family.percussion",
        }
    }
}

/// 一件乐器的完整定义（全部字段都是发声音高口径）
#[derive(Debug, Clone, Copy)]
pub struct InstrumentSpec {
    /// 稳定标识（i18n 键后缀、预设引用、去重用）
    pub id: &'static str,
    /// 输出轨英文名（写进 midi）
    pub track_name: &'static str,
    /// 乐器族
    pub family: InstrumentFamily,
    /// GM 音色号（0 基 = 写入文件的 `Cn` 字节值）
    pub gm_program: u8,
    /// 可演奏范围（硬边界）
    pub playable: PitchRange,
    /// 舒适区（代价 0 区间）
    pub comfortable: PitchRange,
    /// 舒适区的可信度
    pub comfort_source: ComfortSource,
    /// 最多同时发几个音（硬上限）
    pub max_polyphony: u8,
    /// 叠音惩罚权重（≥ 1.0）
    pub polyphony_alpha: f64,
    /// 叠音是否必须落在相邻弦（弓弦 = true；首版只作标记，不参与判定）
    pub requires_adjacent_strings: bool,
    /// 移调偏移 Δ = 发声 − 记谱（只用于显示与输入可疑检测）
    pub sounding_offset: i8,
    /// 可选型号变体（如低音单簧管的低 C / 低 E♭）
    pub tuning_options: &'static [TuningOption],
}

/// 同一件乐器的型号变体：整体覆盖 playable / comfortable
#[derive(Debug, Clone, Copy)]
pub struct TuningOption {
    /// 稳定标识（UI 下拉用）
    pub id: &'static str,
    /// 显示名（英文，UI 走 i18n）
    pub label: &'static str,
    /// 覆盖后的可演奏范围
    pub playable: PitchRange,
    /// 覆盖后的舒适区
    pub comfortable: PitchRange,
}

impl InstrumentSpec {
    /// 应用某个型号变体（`variant_id` 为空或找不到时原样返回）
    pub fn with_variant(&self, variant_id: &str) -> InstrumentSpec {
        let Some(variant) = self
            .tuning_options
            .iter()
            .find(|option| option.id == variant_id)
        else {
            return *self;
        };
        InstrumentSpec {
            playable: variant.playable,
            comfortable: variant.comfortable,
            ..*self
        }
    }
}

/// 编制里的一件乐器 = 预置定义 + 用户微调 + 显示序号
#[derive(Debug, Clone)]
pub struct EnsembleSlot {
    /// 指向 [`INSTRUMENT_POOL`] 的下标
    pub spec_index: usize,
    /// 同名乐器的序号：0 → "Violin 1"，1 → "Violin 2"
    pub ordinal: usize,
    /// 生效的型号变体 id（空串 = 默认型号）
    pub variant_id: String,
    /// 生效的可演奏范围
    pub playable: PitchRange,
    /// 生效的舒适区
    pub comfortable: PitchRange,
    /// 生效的叠音惩罚权重
    pub polyphony_alpha: f64,
    /// 是否参与本次分配
    pub enabled: bool,
}

impl EnsembleSlot {
    /// 写进 midi 的轨道名（带序号）
    pub fn track_name(&self) -> String {
        let spec = self.spec();
        if self.ordinal == 0 {
            spec.track_name.to_string()
        } else {
            format!("{} {}", spec.track_name, self.ordinal + 1)
        }
    }

    /// 指向的预置定义
    pub fn spec(&self) -> &'static InstrumentSpec {
        &INSTRUMENT_POOL[self.spec_index]
    }

    /// 最多同时发几个音
    pub fn max_polyphony(&self) -> u8 {
        self.spec().max_polyphony
    }

    /// GM 音色号（0 基）
    pub fn gm_program(&self) -> u8 {
        self.spec().gm_program
    }

    /// 本槽位在**启用清单**里的输出通道（0 基）
    ///
    /// 通道不存字段、每次从这里现算：存下来就会变成"可以被改动的旧值"，
    /// 一旦哪条路径忘了刷新（比如编制增删之后），写出的文件里就会有几件乐器
    /// 挤在同一个通道上、互相覆盖 GM 音色。现算就没有这个失效机会。
    ///
    /// `active_position` 是本槽位在启用清单（[`SplitRequest::ensemble`] 过滤 `enabled` 之后）
    /// 里的下标，所以通道永远从 1 号开始连续排、中间不留空。
    ///
    /// [`SplitRequest::ensemble`]: crate::split::run::SplitRequest::ensemble
    pub fn default_output_channel(&self, active_position: usize) -> u8 {
        assign_channel(active_position)
    }
}

/// 编制预设：一键填入某套常用编制
#[derive(Debug, Clone, Copy)]
pub struct EnsemblePreset {
    /// 稳定标识
    pub id: &'static str,
    /// i18n 词典键
    pub label_key: &'static str,
    /// 组成该预设的乐器 id（同一 id 出现多次表示同名多把）
    pub instrument_ids: &'static [&'static str],
}

/// 叠音惩罚权重的默认值
///
/// 注意：**"只要存在单音方案就绝不叠音"不靠这个值实现**——那是求解器的叠音数主键
/// （见 `split::assign`）。这里只影响"反正都得叠音时叠在谁身上"。
pub const DEFAULT_POLYPHONY_ALPHA: f64 = 3.0;

/// 低音单簧管的两个型号下限（现代低 C 型 / 常见的低 E♭ 型）
const BASS_CLARINET_VARIANTS: &[TuningOption] = &[
    TuningOption {
        id: "low_c",
        label: "Low C",
        playable: PitchRange::new(34, 82),
        comfortable: PitchRange::new(43, 77),
    },
    TuningOption {
        id: "low_eb",
        label: "Low E-flat",
        playable: PitchRange::new(37, 82),
        comfortable: PitchRange::new(43, 77),
    },
];

/// 预置乐器池（数据见 `documentation/乐器音域舒适区数据表.md`）
///
/// 顺序即 UI 里的显示顺序（按乐器族分组，族内由低到高／由常用到少见）。
/// 改动这里的任何数值都必须同步改那份文档。
pub const INSTRUMENT_POOL: &[InstrumentSpec] = &[
    // ── 木管 ──────────────────────────────────────────────────────────
    InstrumentSpec {
        id: "piccolo",
        track_name: "Piccolo",
        family: InstrumentFamily::Woodwind,
        gm_program: 72,
        playable: PitchRange::new(72, 108),
        comfortable: PitchRange::new(84, 108),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 12,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "flute",
        track_name: "Flute",
        family: InstrumentFamily::Woodwind,
        gm_program: 73,
        playable: PitchRange::new(59, 96),
        comfortable: PitchRange::new(60, 96),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "alto_flute",
        track_name: "Alto Flute",
        family: InstrumentFamily::Woodwind,
        gm_program: 73,
        playable: PitchRange::new(55, 91),
        comfortable: PitchRange::new(55, 79),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -5,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "oboe",
        track_name: "Oboe",
        family: InstrumentFamily::Woodwind,
        gm_program: 68,
        playable: PitchRange::new(58, 93),
        comfortable: PitchRange::new(60, 87),
        comfort_source: ComfortSource::Source,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "english_horn",
        track_name: "English Horn",
        family: InstrumentFamily::Woodwind,
        gm_program: 69,
        playable: PitchRange::new(52, 84),
        comfortable: PitchRange::new(59, 79),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -7,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "clarinet_eb",
        track_name: "E-flat Clarinet",
        family: InstrumentFamily::Woodwind,
        gm_program: 71,
        playable: PitchRange::new(55, 96),
        comfortable: PitchRange::new(58, 91),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 3,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "clarinet_bb",
        track_name: "Clarinet in B-flat",
        family: InstrumentFamily::Woodwind,
        gm_program: 71,
        playable: PitchRange::new(50, 94),
        comfortable: PitchRange::new(53, 79),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -2,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "clarinet_a",
        track_name: "Clarinet in A",
        family: InstrumentFamily::Woodwind,
        gm_program: 71,
        playable: PitchRange::new(49, 93),
        comfortable: PitchRange::new(52, 79),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -3,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "bass_clarinet",
        track_name: "Bass Clarinet",
        family: InstrumentFamily::Woodwind,
        gm_program: 71,
        playable: PitchRange::new(34, 82),
        comfortable: PitchRange::new(43, 77),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -14,
        tuning_options: BASS_CLARINET_VARIANTS,
    },
    InstrumentSpec {
        id: "bassoon",
        track_name: "Bassoon",
        family: InstrumentFamily::Woodwind,
        gm_program: 70,
        playable: PitchRange::new(34, 88),
        comfortable: PitchRange::new(36, 72),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "contrabassoon",
        track_name: "Contrabassoon",
        family: InstrumentFamily::Woodwind,
        gm_program: 70,
        playable: PitchRange::new(22, 62),
        comfortable: PitchRange::new(22, 58),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -12,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "soprano_sax",
        track_name: "Soprano Sax",
        family: InstrumentFamily::Woodwind,
        gm_program: 64,
        playable: PitchRange::new(56, 88),
        comfortable: PitchRange::new(58, 84),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -2,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "alto_sax",
        track_name: "Alto Sax",
        family: InstrumentFamily::Woodwind,
        gm_program: 65,
        playable: PitchRange::new(49, 81),
        comfortable: PitchRange::new(53, 74),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -9,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "tenor_sax",
        track_name: "Tenor Sax",
        family: InstrumentFamily::Woodwind,
        gm_program: 66,
        playable: PitchRange::new(44, 76),
        comfortable: PitchRange::new(48, 69),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -14,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "baritone_sax",
        track_name: "Baritone Sax",
        family: InstrumentFamily::Woodwind,
        gm_program: 67,
        playable: PitchRange::new(36, 69),
        comfortable: PitchRange::new(43, 64),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -21,
        tuning_options: &[],
    },
    // ── 铜管 ──────────────────────────────────────────────────────────
    InstrumentSpec {
        id: "horn",
        track_name: "Horn in F",
        family: InstrumentFamily::Brass,
        gm_program: 60,
        playable: PitchRange::new(36, 76),
        comfortable: PitchRange::new(43, 67),
        comfort_source: ComfortSource::Inferred,
        // 圆号极少数情况可双音，按 2 处理（与弦乐同档）
        max_polyphony: 2,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -7,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "trumpet_bb",
        track_name: "Trumpet in B-flat",
        family: InstrumentFamily::Brass,
        gm_program: 56,
        playable: PitchRange::new(52, 82),
        comfortable: PitchRange::new(55, 82),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -2,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "cornet_bb",
        track_name: "Cornet in B-flat",
        family: InstrumentFamily::Brass,
        gm_program: 56,
        playable: PitchRange::new(52, 82),
        comfortable: PitchRange::new(55, 82),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: -2,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "trombone_tenor",
        track_name: "Tenor Trombone",
        family: InstrumentFamily::Brass,
        gm_program: 57,
        playable: PitchRange::new(40, 77),
        comfortable: PitchRange::new(41, 74),
        comfort_source: ComfortSource::Source,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "trombone_bass",
        track_name: "Bass Trombone",
        family: InstrumentFamily::Brass,
        gm_program: 57,
        playable: PitchRange::new(34, 77),
        comfortable: PitchRange::new(41, 74),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "tuba",
        track_name: "Tuba",
        family: InstrumentFamily::Brass,
        gm_program: 58,
        playable: PitchRange::new(22, 72),
        comfortable: PitchRange::new(29, 60),
        comfort_source: ComfortSource::Source,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "euphonium",
        track_name: "Euphonium",
        family: InstrumentFamily::Brass,
        gm_program: 58,
        playable: PitchRange::new(22, 74),
        comfortable: PitchRange::new(40, 67),
        comfort_source: ComfortSource::Source,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    // ── 弦乐 ──────────────────────────────────────────────────────────
    InstrumentSpec {
        id: "violin",
        track_name: "Violin",
        family: InstrumentFamily::Strings,
        gm_program: 40,
        playable: PitchRange::new(55, 100),
        comfortable: PitchRange::new(55, 88),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 2,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        // 双音必须落在相邻弦；首版只作标记，不参与判定
        requires_adjacent_strings: true,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "viola",
        track_name: "Viola",
        family: InstrumentFamily::Strings,
        gm_program: 41,
        playable: PitchRange::new(48, 88),
        comfortable: PitchRange::new(48, 84),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 2,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: true,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "cello",
        track_name: "Cello",
        family: InstrumentFamily::Strings,
        gm_program: 42,
        playable: PitchRange::new(36, 88),
        comfortable: PitchRange::new(36, 72),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 2,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: true,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "contrabass",
        track_name: "Contrabass",
        family: InstrumentFamily::Strings,
        gm_program: 43,
        playable: PitchRange::new(28, 67),
        comfortable: PitchRange::new(28, 55),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 2,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: true,
        sounding_offset: -12,
        tuning_options: &[],
    },
    // ── 有音高打击乐（首版按连续音域近似） ──────────────────────────────
    InstrumentSpec {
        id: "timpani",
        track_name: "Timpani",
        family: InstrumentFamily::Percussion,
        gm_program: 47,
        playable: PitchRange::new(36, 72),
        comfortable: PitchRange::new(38, 69),
        comfort_source: ComfortSource::Source,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "xylophone",
        track_name: "Xylophone",
        family: InstrumentFamily::Percussion,
        gm_program: 13,
        playable: PitchRange::new(65, 108),
        comfortable: PitchRange::new(65, 96),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 12,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "glockenspiel",
        track_name: "Glockenspiel",
        family: InstrumentFamily::Percussion,
        gm_program: 9,
        playable: PitchRange::new(72, 100),
        comfortable: PitchRange::new(76, 96),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 24,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "vibraphone",
        track_name: "Vibraphone",
        family: InstrumentFamily::Percussion,
        gm_program: 11,
        playable: PitchRange::new(53, 89),
        comfortable: PitchRange::new(55, 84),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "tubular_bells",
        track_name: "Tubular Bells",
        family: InstrumentFamily::Percussion,
        gm_program: 14,
        playable: PitchRange::new(72, 89),
        comfortable: PitchRange::new(72, 86),
        comfort_source: ComfortSource::Inferred,
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 0,
        tuning_options: &[],
    },
    InstrumentSpec {
        id: "celesta",
        track_name: "Celesta",
        family: InstrumentFamily::Percussion,
        gm_program: 8,
        playable: PitchRange::new(60, 108),
        comfortable: PitchRange::new(60, 96),
        comfort_source: ComfortSource::Inferred,
        // 键盘乐器物理上可同时 10 音；本工具按单声部分配用途限制为 1
        max_polyphony: 1,
        polyphony_alpha: DEFAULT_POLYPHONY_ALPHA,
        requires_adjacent_strings: false,
        sounding_offset: 12,
        tuning_options: &[],
    },
];

/// 编制预设
pub const ENSEMBLE_PRESETS: &[EnsemblePreset] = &[
    EnsemblePreset {
        id: "string_quartet",
        label_key: "ui.ensemble.preset.string_quartet",
        instrument_ids: &["violin", "violin", "viola", "cello"],
    },
    EnsemblePreset {
        id: "wind_quintet",
        label_key: "ui.ensemble.preset.wind_quintet",
        instrument_ids: &["flute", "oboe", "clarinet_bb", "bassoon", "horn"],
    },
    EnsemblePreset {
        id: "double_winds",
        label_key: "ui.ensemble.preset.double_winds",
        instrument_ids: &[
            "flute",
            "flute",
            "oboe",
            "oboe",
            "clarinet_bb",
            "clarinet_bb",
            "bassoon",
            "bassoon",
        ],
    },
];

/// midi 通道总数
pub const MIDI_CHANNEL_COUNT: usize = 16;

/// GM 打击乐保留通道（索引，1 基的 10 号通道）
pub const PERCUSSION_CHANNEL: usize = 9;

/// 能各自独占一个通道的乐器数上限（16 个通道去掉打击乐保留位）
pub const MAX_SEPARATE_CHANNELS: usize = MIDI_CHANNEL_COUNT - 1;

/// 编制上限（含同名多把）
///
/// **直接等于可用通道数** [`MAX_SEPARATE_CHANNELS`]：midi 只有 16 个通道，索引 9 是
/// GM 打击乐保留位，所以最多 15 件乐器能各自独占一个通道。上限就定 15，这样每件乐器
/// 都能拿到独立通道，不需要考虑复用通道带来的 GM 音色互相覆盖问题。
///
/// （若将来放宽这个上限，就必须处理通道复用，`crowded_channels` 的提示也会重新变得必要。）
pub const MAX_ENSEMBLE_SIZE: usize = MAX_SEPARATE_CHANNELS;

/// 按乐器 id 查预置定义
pub fn find_instrument(id: &str) -> Option<&'static InstrumentSpec> {
    INSTRUMENT_POOL.iter().find(|spec| spec.id == id)
}

/// 编排出输出通道：按**启用清单里的位次**从 0 递增，**跳过索引 9（1 基的 10 号通道，GM 打击乐保留位）**
///
/// 通道由位次直接推导，所以用户不需要（也无法）手动设置通道，更不会从源 midi 里
/// 沿用原轨的通道号：第 1 件 → 1 号通道，第 9 件 → 9 号通道，第 10 件 → **11 号通道**（跳过 10），
/// 第 11 件 → 12 号通道 …… 到第 15 件 → 16 号通道正好用完 15 个可用通道。
///
/// 编制上限 [`MAX_ENSEMBLE_SIZE`] 已经与可用通道数（[`MAX_SEPARATE_CHANNELS`]）对齐，
/// 所以正常情况下**不会发生通道复用**；[`crowded_channels`] 只是防御性检查。
pub fn assign_channel(slot_position: usize) -> u8 {
    if slot_position < PERCUSSION_CHANNEL {
        slot_position as u8
    } else {
        (slot_position + 1).min(MIDI_CHANNEL_COUNT - 1) as u8
    }
}

/// 找出被多件乐器共用的通道（这些乐器在同一个 midi 端口上会共用 GM 音色）
///
/// 入参是**启用清单的件数**而不是槽位表：通道本来就由位次现算（见 [`assign_channel`]），
/// 所以"会不会撞通道"只取决于件数，不再依赖任何存下来的通道字段。
/// 返回 `(通道号, 该通道上的乐器数)`，按通道升序。
pub fn crowded_channels(active_instrument_count: usize) -> Vec<(u8, usize)> {
    let mut usage: Vec<usize> = vec![0; MIDI_CHANNEL_COUNT];
    for slot_position in 0..active_instrument_count {
        usage[assign_channel(slot_position) as usize] += 1;
    }
    usage
        .into_iter()
        .enumerate()
        .filter(|(_, count)| *count > 1)
        .map(|(channel, count)| (channel as u8, count))
        .collect()
}

/// 把一组乐器 id 变成编制槽位（数据缺失时跳过并返回提示）
pub fn build_ensemble(instrument_ids: &[&str]) -> (Vec<EnsembleSlot>, Vec<String>) {
    let mut slots: Vec<EnsembleSlot> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for &instrument_id in instrument_ids {
        if slots.len() >= MAX_ENSEMBLE_SIZE {
            warnings.push(format!("ensemble size limit reached, skipped: {}", instrument_id));
            continue;
        }
        let Some(spec_index) = INSTRUMENT_POOL.iter().position(|spec| spec.id == instrument_id) else {
            warnings.push(format!("unknown instrument id: {}", instrument_id));
            continue;
        };
        let spec = &INSTRUMENT_POOL[spec_index];
        let ordinal = slots
            .iter()
            .filter(|slot| slot.spec_index == spec_index)
            .count();
        slots.push(EnsembleSlot {
            spec_index,
            ordinal,
            variant_id: String::new(),
            playable: spec.playable,
            comfortable: spec.comfortable,
            polyphony_alpha: spec.polyphony_alpha,
            enabled: true,
        });
    }

    (slots, warnings)
}

/// 编制并集音域（八度折叠的落点范围）
pub fn ensemble_union_range(ensemble: &[EnsembleSlot]) -> Option<PitchRange> {
    let mut union: Option<PitchRange> = None;
    for slot in ensemble.iter().filter(|slot| slot.enabled) {
        union = Some(match union {
            None => slot.playable,
            Some(current) => PitchRange::new(
                current.lo.min(slot.playable.lo),
                current.hi.max(slot.playable.hi),
            ),
        });
    }
    union
}

/// 编制总同时发音容量（Σ max_polyphony）
///
/// 这是"一件乐器都没被占用时"的容量。实际判定用的是**此刻**腾得出的位数
/// （总容量 − 每件乐器仍占着的音数，见 `split::run`），因为同一件乐器上一刻
/// 还在响的音会占掉它的位子。挑编制时按这个总数估算仍然有效：
/// 只要它不小于"音频里同时响的音最多有几个"，就一定不会因为放不下而合并或丢音。
pub fn ensemble_capacity(ensemble: &[EnsembleSlot]) -> usize {
    ensemble
        .iter()
        .filter(|slot| slot.enabled)
        .map(|slot| slot.max_polyphony() as usize)
        .sum()
}
