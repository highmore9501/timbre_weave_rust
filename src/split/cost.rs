//! 单件乐器的代价函数

use crate::split::instrument::EnsembleSlot;

/// 超出舒适区、到达可演奏边界时的最大罚分
/// （沿用旧四重奏实现里"音域两端罚分 2"的量级）
pub const MAX_PENALTY: f64 = 2.0;

/// 单音对单乐器的代价
///
/// - 可演奏范围外 → `None`（硬约束，不成组合）；
/// - 舒适区内 → `0`；
/// - 舒适区外、可演奏范围内 → 按"超出距离 / 该侧剩余空间"线性升到 [`MAX_PENALTY`]。
///
/// 归一化（除以该侧剩余空间）让"同样的相对深度"在音域宽窄不同的乐器上罚分相当。
pub fn range_cost(instrument: &EnsembleSlot, key: i32) -> Option<f64> {
    if key < instrument.playable.lo || key > instrument.playable.hi {
        return None;
    }
    if key >= instrument.comfortable.lo && key <= instrument.comfortable.hi {
        return Some(0.0);
    }

    let (distance, span) = if key < instrument.comfortable.lo {
        (
            instrument.comfortable.lo - key,
            instrument.comfortable.lo - instrument.playable.lo,
        )
    } else {
        (
            key - instrument.comfortable.hi,
            instrument.playable.hi - instrument.comfortable.hi,
        )
    };

    // 退化情形（舒适区贴着可演奏边界）：直接给最大罚分，避免除零
    if span <= 0 {
        return Some(MAX_PENALTY);
    }
    Some(MAX_PENALTY * distance as f64 / span as f64)
}

/// 音集对单乐器的代价（`keys.len() ≤ max_polyphony` 由调用方保证）
///
/// - 1 个音：该音的音域代价；
/// - 多个音：`Σ音域代价 × alpha × 音数`（沿用旧四重奏的"和弦难度"写法）。
///
/// 注意叠音惩罚只是**次键**里的系数——"只要存在单音方案就绝不叠音"
/// 由 [`crate::split::assign`] 的叠音数主键保证，不依赖 `alpha` 的大小。
pub fn subset_cost(instrument: &EnsembleSlot, keys: &[i32]) -> f64 {
    match keys.len() {
        0 => 0.0,
        1 => range_cost(instrument, keys[0]).unwrap_or(f64::INFINITY),
        note_count => {
            let sum: f64 = keys
                .iter()
                .map(|&key| range_cost(instrument, key).unwrap_or(f64::INFINITY))
                .sum();
            sum * instrument.polyphony_alpha * note_count as f64
        }
    }
}

/// 该音在编制里最小的单音代价（用于剪枝下界与"最难安置优先"排序）
pub fn best_range_cost(ensemble: &[EnsembleSlot], key: i32) -> f64 {
    ensemble
        .iter()
        .filter(|slot| slot.enabled)
        .filter_map(|slot| range_cost(slot, key))
        .fold(f64::INFINITY, f64::min)
}

/// 该音超出乐器舒适区的半音距离（舒适区内为 0）
pub fn comfort_distance(instrument: &EnsembleSlot, key: i32) -> i32 {
    if key < instrument.comfortable.lo {
        instrument.comfortable.lo - key
    } else if key > instrument.comfortable.hi {
        key - instrument.comfortable.hi
    } else {
        0
    }
}

/// 音符到可演奏音域的距离（域内为 0），仅用于兜底取"最近的乐器"
pub fn range_distance(instrument: &EnsembleSlot, key: i32) -> i32 {
    if key < instrument.playable.lo {
        instrument.playable.lo - key
    } else if key > instrument.playable.hi {
        key - instrument.playable.hi
    } else {
        0
    }
}
