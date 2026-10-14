//! 候选择优求解器（候选池 / beam search）
//!
//! ## 为什么不用"一次性精确枚举"
//!
//! 旧写法是逐组枚举"音符 → 乐器"的全部子集划分（掩码 + 记忆化，`O(3ⁿ·m)`）。
//! 小组合没问题，但**没有工作量上界**：n = 16、m = 8 时约 4300 万次子集访问，
//! 一个音频转 midi 的大音簇就能把 UI 线程卡死。多乐器 + 多同时音时组合数会爆炸。
//!
//! ## 现在的做法（参考 `key_ripple_rust` 的 `RecorderPool`）
//!
//! 维护一个**固定大小的候选池**：池里每条是"已经分配完前若干个音"的**部分解**，
//! 带着累计得分。推进时由池里每条各自展开"下一个音分给谁"，然后把全部展开结果
//! 按分层得分排序，**只保留最好的 K 条**继续往下走。
//!
//! 这样每一步的工作量都被 K 卡住（`K × 乐器数` 次展开），总工作量上界是
//! `音符数 × K × 乐器数`，与组合总数无关。
//!
//! 之所以能在我们这里成立：**代价函数是逐乐器累加分解的**——某件乐器的代价只取决于
//! 它最终分到的音集，与分配顺序无关。所以"到目前为止累计得分最小"就是正确的剪枝依据，
//! 和 key_ripple 里 `current_entropy + 本步增量` 是同一个道理。
//!
//! ## 硬约束：`max_polyphony` 绝不可超（按时刻算）
//!
//! 单声部乐器（木管 / 铜管 / 打击乐）的 `max_polyphony = 1`，**任何情况下都不会拿到第二个音**。
//! 展开时"该乐器是否还有空位"是第一道判断，两趟展开（见下）都遵守它。
//!
//! 空位的口径是**这一刻**：调用方把"每件乐器此刻仍占着的音高"（上一组留下、还没结束的音）
//! 一起传进来，占用数 + 本组已分给它的音数 必须小于 `max_polyphony`。
//! 因为分组只按起点划分（同一时刻的音符才在同一组），所以"占着"指的就是与本组同时发声。
//! 这也是本模块能取代"把时值重叠的音符并成一组"那种做法的原因：
//! 时间关系由这里的占用表处理，组不必再为一个长音拖着一长串后来的音符。
//!
//! ## 两趟展开：让自己永远不会失败
//!
//! 第一趟只考虑"这件乐器真的能演奏这个音"（`range_cost` 不是 `None`）。
//! 如果**所有候选**都找不到落点（典型情形：这个音只有 Piccolo 能演，但本组里 Piccolo 已被占用），
//! 就进入第二趟：允许把它塞给"还有空位、且距离最近"的乐器，无论是否在音域内，
//! 并计入 [`PoolDiagnostics::forced_notes`]。
//!
//! 这样做的好处是**求解器不会失败**，于是上层不需要"整组按最近乐器乱塞"的危险兜底
//! （那正是"单声部乐器出现叠音"的根源）。放不下的情况由归一化阶段的折叠 / 合并 / 丢弃负责
//! ——检测到的不可安置音符在归一化时就被丢掉并计数，根本不会走到这里。
//!
//! ## 分层的四个键（词典序）
//!
//! 1. **叠音音符总数**最少 —— 硬保证"只要存在单音方案就绝不叠音"；
//! 2. 代价和最小（这一层才决定"某个音该摊给别的乐器、还是用第二根弦"）；
//! 3. 超出舒适区的半音距离之和最小；
//! 4. 舒适区中心距离平方和最小。
//!
//! 池内同分时按状态键排序收敛，结果确定可复现，**不使用随机数**。
//!
//! ## 与精确解的偏差
//!
//! K 足够大时结果与穷举一致（K = 候选总数时池等价于穷举）。默认 K 取较大值，
//! 只有极端的组才可能被剪枝；一旦发生，[`PoolDiagnostics::saturated`] 会置位并写进报告，
//! 不会静默降级。

use crate::midi::grouping::GroupNote;
use crate::split::cost::{comfort_distance, range_cost, subset_cost};
use crate::split::instrument::EnsembleSlot;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

/// 候选池默认大小（key_ripple 用的是 100；我们每个候选的展开代价更小，取大一些更稳）
pub const DEFAULT_POOL_SIZE: usize = 256;

/// 候选池的最小可用大小（再小就退化成贪心）
pub const MIN_POOL_SIZE: usize = 8;

/// "音域外"音符在**增量**代价里的罚分
///
/// [`crate::split::cost::subset_cost`] 对音域外的音返回 `INFINITY`（第一趟展开的硬约束靠它），
/// 但增量是"收下之后 − 收下之前"，两个 `INFINITY` 相减会得到 `NaN`。
/// 这里换成一个有限的、比任何正常组合都大的罚分（正常组合最多 `MAX_PENALTY × alpha × 音数`）。
const OUT_OF_RANGE_INCREMENT: f64 = 64.0;

/// 一个分配方案的分层得分（比较时先比前面的字段）
#[derive(Debug, Clone, Copy)]
struct LayeredScore {
    /// 叠音音符总数（主键）
    doubled_notes: usize,
    /// 代价和（次键）
    cost: f64,
    /// 超出舒适区的半音距离之和（三键）
    outside_distance: i32,
    /// 舒适区中心距离平方和（四键）
    center_deviation: f64,
}

impl LayeredScore {
    /// 空方案
    fn zero() -> Self {
        LayeredScore {
            doubled_notes: 0,
            cost: 0.0,
            outside_distance: 0,
            center_deviation: 0.0,
        }
    }

    /// 累加另一个得分
    fn plus(self, other: LayeredScore) -> Self {
        LayeredScore {
            doubled_notes: self.doubled_notes + other.doubled_notes,
            cost: self.cost + other.cost,
            outside_distance: self.outside_distance + other.outside_distance,
            center_deviation: self.center_deviation + other.center_deviation,
        }
    }
}

/// 分层比较：先比叠音数，再比代价，再比超出舒适区距离，最后比中心偏离
///
/// 直接返回 [`Ordering`]，池内排序与选优都复用它，避免两处比较逻辑漂移。
fn score_order(candidate: &LayeredScore, incumbent: &LayeredScore) -> Ordering {
    match candidate.doubled_notes.cmp(&incumbent.doubled_notes) {
        Ordering::Equal => {}
        other => return other,
    }
    if (candidate.cost - incumbent.cost).abs() > f64::EPSILON {
        return candidate
            .cost
            .partial_cmp(&incumbent.cost)
            .unwrap_or(Ordering::Equal);
    }
    match candidate.outside_distance.cmp(&incumbent.outside_distance) {
        Ordering::Equal => {}
        other => return other,
    }
    if (candidate.center_deviation - incumbent.center_deviation).abs() > f64::EPSILON {
        return candidate
            .center_deviation
            .partial_cmp(&incumbent.center_deviation)
            .unwrap_or(Ordering::Equal);
    }
    Ordering::Equal
}

/// 部分解的去重 / 排序键：每件乐器已经收下几个音（前缀，只含已用到的乐器）
///
/// 这个键唯一决定了"接下来还能怎么分"（每件乐器的剩余空位），
/// 所以两个键相同的部分解可以合并（保留得分更好的那条）。
type StateKey = Vec<(usize, u8)>;

/// 池中的一个部分解
#[derive(Debug, Clone)]
struct Candidate {
    /// `assignments[instrument]` = 该乐器分到的音符下标（升序）
    assignments: Vec<Vec<usize>>,
    /// 累计分层得分
    score: LayeredScore,
    /// 去重 / 排序键
    state_key: StateKey,
    /// 该部分解里有多少个音是被"强制塞进音域外乐器"的
    forced_notes: usize,
}

impl Candidate {
    /// 空部分解（一个音都还没分）
    fn empty(instrument_count: usize) -> Self {
        Candidate {
            assignments: vec![Vec::new(); instrument_count],
            score: LayeredScore::zero(),
            state_key: Vec::new(),
            forced_notes: 0,
        }
    }
}

/// 堆里的包装：`Ord` 按"最差优先"定义，于是 `peek()` 拿到池里最差的一条
#[derive(Debug, Clone)]
struct PoolEntry {
    candidate: Candidate,
}

impl PartialEq for PoolEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for PoolEntry {}

impl PartialOrd for PoolEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PoolEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // 得分差 → 差的那个在堆顶（方便 pop 掉）
        match score_order(&self.candidate.score, &other.candidate.score) {
            Ordering::Equal => {}
            Ordering::Less => return Ordering::Greater,
            Ordering::Greater => return Ordering::Less,
        }
        // 得分打平时用状态键定序，保证堆行为确定（大键在堆顶）
        self.candidate.state_key.cmp(&other.candidate.state_key)
    }
}

/// 候选池求解的诊断信息（写进报告，避免"被剪枝了但没人知道"）
#[derive(Debug, Clone, Copy, Default)]
pub struct PoolDiagnostics {
    /// 展开出的候选总数
    pub expansions: usize,
    /// 池内保留过的最大条数
    pub peak_pool_size: usize,
    /// 池曾经到达上限（说明发生过剪枝，结果不保证全局最优）
    pub saturated: bool,
    /// 被"强制塞进音域外乐器"的音符数（第一趟无人能演 → 第二趟找最近有空位的乐器）
    pub forced_notes: usize,
}

/// 求解结果
#[derive(Debug, Clone, Default)]
pub struct GroupAssignment {
    /// `voice_note_indices[slot_index]` = 该乐器分到的音符在组内的下标
    pub voice_note_indices: Vec<Vec<usize>>,
    /// 叠音音符总数
    pub doubled_notes: usize,
    /// 代价和
    pub total_cost: f64,
    /// 求解诊断
    pub diagnostics: PoolDiagnostics,
}

/// 用候选池求解一个音符组的分配方案
///
/// `occupied_keys[slot_index]` 是这一刻该乐器**仍占着**的音高（上一组留下、此刻还没结束的），
/// 它们与本组的音同时发声，所以要计入该乐器的同时发音数。
///
/// 返回 `None` 只有一种可能：**本组音符数超过此刻还能腾出的同时发音位总数**
/// （这是硬约束，必须由归一化阶段的合并/丢弃来满足，见 `midi::grouping::normalize_group`）。
/// 除此之外求解器一定能给出把每个音都安置好的方案——必要时允许"强制塞进音域外乐器"，
/// 并计入诊断的 `forced_notes`。
pub fn assign_group(
    notes: &[GroupNote],
    ensemble: &[EnsembleSlot],
    occupied_keys: &[Vec<i32>],
    pool_size: usize,
) -> Option<GroupAssignment> {
    if notes.is_empty() {
        return Some(GroupAssignment {
            voice_note_indices: vec![Vec::new(); ensemble.len()],
            ..GroupAssignment::default()
        });
    }

    // 硬约束：此刻腾得出的同时发音位必须够（不够就该在归一化阶段合并/丢音，不应该走到这里）
    let free_capacity: usize = ensemble
        .iter()
        .enumerate()
        .map(|(instrument_index, slot)| {
            let occupied = occupied_keys.get(instrument_index).map_or(0, Vec::len);
            (slot.max_polyphony() as usize).saturating_sub(occupied)
        })
        .sum();
    if notes.len() > free_capacity {
        return None;
    }

    let pool_capacity = pool_size.max(MIN_POOL_SIZE).max(1);
    let mut diagnostics = PoolDiagnostics::default();

    // 池里最开始只有一条空解
    let mut pool: BinaryHeap<PoolEntry> = BinaryHeap::with_capacity(pool_capacity + 1);
    pool.push(PoolEntry {
        candidate: Candidate::empty(ensemble.len()),
    });

    let mut expansions: Vec<Candidate> = Vec::with_capacity(pool_capacity * ensemble.len());
    let mut seen_keys: HashSet<StateKey> = HashSet::new();

    // 逐音符推进：每个音都必须落到某件乐器上（不丢音）
    for note_index in 0..notes.len() {
        expansions.clear();
        seen_keys.clear();

        // 当前池里的全部部分解（逐个展开）
        let current_candidates: Vec<Candidate> =
            pool.drain().map(|entry| entry.candidate).collect();

        // ---- 第一趟：只考虑真的能演奏这个音的乐器 ----
        for candidate in &current_candidates {
            for instrument_index in 0..ensemble.len() {
                if !can_place(candidate, instrument_index, ensemble, occupied_keys) {
                    continue;
                }
                if range_cost(&ensemble[instrument_index], notes[note_index].key).is_none() {
                    continue;
                }
                expansions.push(expand_candidate(
                    candidate,
                    instrument_index,
                    note_index,
                    &ensemble[instrument_index],
                    notes,
                    occupied_keys,
                    false,
                ));
            }
        }

        // ---- 第二趟：第一趟全灭时才启用"强制安置" ----
        // 典型情形：这个音只有 Piccolo 能演，但本组里 Piccolo 已被占用。
        // 与其让整组失败（进而让上层乱塞、把单声部乐器塞成叠音），
        // 不如把它交给"还有空位、距离最近"的乐器，并明确计数。
        if expansions.is_empty() {
            for candidate in &current_candidates {
                let mut nearest: Option<(i32, usize)> = None;
                for instrument_index in 0..ensemble.len() {
                    if !can_place(candidate, instrument_index, ensemble, occupied_keys) {
                        continue;
                    }
                    let distance = range_distance(&ensemble[instrument_index], notes[note_index].key);
                    if nearest.is_none_or(|(best_distance, _)| distance < best_distance) {
                        nearest = Some((distance, instrument_index));
                    }
                }
                if let Some((_, instrument_index)) = nearest {
                    expansions.push(expand_candidate(
                        candidate,
                        instrument_index,
                        note_index,
                        &ensemble[instrument_index],
                        notes,
                        occupied_keys,
                        true,
                    ));
                }
            }
        }

        diagnostics.expansions += expansions.len();

        // 到这里 expansions 仍然为空，只有一种可能：所有乐器都被塞满了。
        // 归一化阶段保证了组内音数 ≤ 总容量，所以这是不可达的防御分支。
        if expansions.is_empty() {
            return None;
        }

        // 按分层得分排序，只保留最好的 K 条
        expansions.sort_by(|candidate_a, candidate_b| {
            score_order(&candidate_a.score, &candidate_b.score)
                .then_with(|| candidate_a.state_key.cmp(&candidate_b.state_key))
        });

        for candidate in expansions.drain(..) {
            // 同一个状态键的部分解合并：只留排序后最先出现（得分最优）的那条
            if !seen_keys.insert(candidate.state_key.clone()) {
                continue;
            }
            if pool.len() >= pool_capacity {
                diagnostics.saturated = true;
                break;
            }
            pool.push(PoolEntry { candidate });
        }

        diagnostics.peak_pool_size = diagnostics.peak_pool_size.max(pool.len());
    }

    // 池里得分最好的那条就是结果
    let best = pool
        .into_iter()
        .min_by(|entry_a, entry_b| score_order(&entry_a.candidate.score, &entry_b.candidate.score))
        .map(|entry| entry.candidate)?;

    diagnostics.forced_notes = best.forced_notes;

    Some(GroupAssignment {
        voice_note_indices: best.assignments,
        doubled_notes: best.score.doubled_notes,
        total_cost: best.score.cost,
        diagnostics,
    })
}

/// 该乐器在这个部分解里是否还有空位
///
/// 空位按"这一刻"算：此刻仍占着的音数（`occupied_keys`）+ 本组已分给它的音数
/// 必须小于 `max_polyphony`（硬上限）。
fn can_place(
    candidate: &Candidate,
    instrument_index: usize,
    ensemble: &[EnsembleSlot],
    occupied_keys: &[Vec<i32>],
) -> bool {
    let occupied = occupied_keys.get(instrument_index).map_or(0, Vec::len);
    let limit = ensemble[instrument_index].max_polyphony() as usize;
    occupied + candidate.assignments[instrument_index].len() < limit
}

/// 音符到可演奏音域的距离（域内为 0），用于第二趟挑"最近的乐器"
fn range_distance(slot: &EnsembleSlot, key: i32) -> i32 {
    if key < slot.playable.lo {
        slot.playable.lo - key
    } else if key > slot.playable.hi {
        key - slot.playable.hi
    } else {
        0
    }
}

/// 把"第 `note_index` 个音分给第 `instrument_index` 件乐器"，生成新的部分解
fn expand_candidate(
    candidate: &Candidate,
    instrument_index: usize,
    note_index: usize,
    slot: &EnsembleSlot,
    notes: &[GroupNote],
    occupied_keys: &[Vec<i32>],
    is_forced: bool,
) -> Candidate {
    let key = notes[note_index].key;

    // 该乐器收下这个音**之前**的音集：此刻仍占着的音 + 本组已分给它的音
    // （两者都与这个音同时发声，所以代价与"叠音"判定都要把它们算上）
    let mut keys_before: Vec<i32> = occupied_keys
        .get(instrument_index)
        .map_or(Vec::new(), |held| held.clone());
    keys_before.extend(
        candidate.assignments[instrument_index]
            .iter()
            .map(|&assigned_index| notes[assigned_index].key),
    );

    // 增量代价 = 收下之后的整集代价 − 收下之前的整集代价
    let cost_before = subset_cost(slot, &keys_before);
    let mut keys_after = keys_before.clone();
    keys_after.push(key);
    let cost_after = subset_cost(slot, &keys_after);
    // 音集里有"音域外"的音时 subset_cost 给的是 INFINITY，两个 INFINITY 相减是 NaN，
    // 一旦混进候选池的得分比较，整池排序就会失效，所以这里换成一个有限的大罚分
    let incremental_cost = if cost_before.is_finite() && cost_after.is_finite() {
        cost_after - cost_before
    } else {
        OUT_OF_RANGE_INCREMENT
    };

    let center = slot.comfortable.center();
    let incremental = LayeredScore {
        // 收下这个音之前乐器就不是空的 → 这个音是叠音（和仍在响的音，或本组同伙）
        doubled_notes: if keys_before.is_empty() { 0 } else { 1 },
        cost: incremental_cost,
        outside_distance: comfort_distance(slot, key),
        center_deviation: (key as f64 - center).powi(2),
    };

    let mut assignments = candidate.assignments.clone();
    assignments[instrument_index].push(note_index);
    let state_key = build_state_key(&assignments);

    Candidate {
        assignments,
        score: candidate.score.plus(incremental),
        state_key,
        forced_notes: candidate.forced_notes + if is_forced { 1 } else { 0 },
    }
}

/// 构造状态键：每件乐器已收的音符数，去掉没用到的乐器（前缀）
fn build_state_key(assignments: &[Vec<usize>]) -> StateKey {
    let mut state_key: StateKey = Vec::new();
    for (instrument_index, note_indices) in assignments.iter().enumerate() {
        if note_indices.is_empty() {
            continue;
        }
        state_key.push((instrument_index, note_indices.len() as u8));
    }
    state_key
}
