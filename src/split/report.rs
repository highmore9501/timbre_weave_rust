//! 分轨汇总报告

/// 各阶段耗时（内部以**微秒**累计，避免 release 下每阶段不足 1ms 被截断成 0）
#[derive(Debug, Clone, Copy, Default)]
pub struct StageTimings {
    /// 读取与解析源 midi
    pub read_micros: f64,
    /// 按起点分组（同时发声的音符归为一组）
    pub grouping_micros: f64,
    /// 组内归一化（八度折叠 / 同音去重 / 八度合并 / 丢弃）
    pub normalize_micros: f64,
    /// 候选择优求解
    pub assign_micros: f64,
    /// 构建并写出 midi 文件
    pub write_micros: f64,
    /// 全流程总耗时
    pub total_micros: f64,
}

impl StageTimings {
    /// 格式化成人类可读的时长（ms / s）
    pub fn format_micros(micros: f64) -> String {
        let millis = micros / 1000.0;
        if millis < 1000.0 {
            if millis < 10.0 {
                format!("{:.2} ms", millis)
            } else {
                format!("{:.0} ms", millis)
            }
        } else {
            format!("{:.2} s", millis / 1000.0)
        }
    }

    /// 格式化成一行（日志用）
    pub fn summary_text(&self) -> String {
        format!(
            "解析 {} / 分组 {} / 归一化 {} / 求解 {} / 写出 {} / 合计 {}",
            Self::format_micros(self.read_micros),
            Self::format_micros(self.grouping_micros),
            Self::format_micros(self.normalize_micros),
            Self::format_micros(self.assign_micros),
            Self::format_micros(self.write_micros),
            Self::format_micros(self.total_micros)
        )
    }
}

/// 单件乐器的统计（需求方明确要求：每件乐器分到多少音符）
#[derive(Debug, Clone, Default)]
pub struct SlotReport {
    /// 写进 midi 的轨道名
    pub track_name: String,
    /// 该乐器的音符数
    pub note_count: usize,
    /// 占写回音符总数的比例
    pub share: f64,
    /// 实际用到的最低音（没有音符时为 `None`）
    pub lowest_key: Option<i32>,
    /// 实际用到的最高音
    pub highest_key: Option<i32>,
    /// 该乐器参与叠音的音符数
    pub doubled_notes: usize,
    /// 该乐器拿到的音里超出自身舒适区的数量
    pub outside_comfort: usize,
}

/// 一次分轨的汇总
#[derive(Debug, Clone, Default)]
pub struct SplitReport {
    /// 输出文件路径
    pub output_path: String,
    /// 读入的源音符数
    pub source_notes: usize,
    /// 音符组数（同一个时刻发声的音符算一组）
    pub onset_groups: usize,
    /// 写回的音符数
    pub written_notes: usize,
    /// 音域外八度折叠数
    pub octave_folded: usize,
    /// 同音去重数（同一个时刻的重复音被合掉的数量）
    pub pitch_merged: usize,
    /// 八度合并数（同一时刻放不下、被并进八度伙伴的数量）
    pub octave_merged: usize,
    /// 丢弃数（正常为 0）
    pub dropped_notes: usize,
    /// 出现叠音的音符组数
    pub doubling_groups: usize,
    /// 叠音音符总数
    pub doubled_notes: usize,
    /// 是否走了"求解器无解 → 整组丢音"这条（理论上不可达的防御分支）
    pub used_fallback: bool,
    /// 被"强制塞进音域外乐器"的音符数（正常为 0）
    pub forced_notes: usize,
    /// 因组内不可安置而整组丢弃的音符数（防御分支专用，正常为 0）
    pub unplaced_notes: usize,
    /// 候选池展开出的候选总数（跨全部音符组累计）
    pub pool_expansions: usize,
    /// 候选池出现过的最大条数
    pub pool_peak_size: usize,
    /// 候选池到达上限（发生过剪枝）的音符组数
    pub pool_saturated_groups: usize,
    /// 各阶段耗时
    pub timings: StageTimings,
    /// 被多件乐器共用的通道：(通道号 1 基, 该通道上的乐器数)
    ///
    /// 编制超过 15 件时必然出现（midi 只有 15 个可用通道）。共通道的乐器在同一个
    /// midi 端口上会互相覆盖 GM 音色，所以要在报告里提示。
    pub shared_channels: Vec<(u8, usize)>,
    /// 每件乐器一行
    pub slots: Vec<SlotReport>,
}

impl SplitReport {
    /// 把每个槽位的音符数拼成一行（日志用）
    pub fn slot_counts_text(&self) -> String {
        self.slots
            .iter()
            .map(|slot| format!("{}={}", slot.track_name, slot.note_count))
            .collect::<Vec<String>>()
            .join(" ")
    }
}
