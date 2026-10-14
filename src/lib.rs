//! timbre_weave —— 把 midi 音符按演奏可行性分配到自选乐器编制
//!
//! 模块划分：
//! - [`i18n`]：中英双语词典（机制照搬 `string_flow_rust`）
//! - [`logging`]：终端 + UI 双通道日志
//! - [`midi`]：源 midi 读取、同时发声分组与归一化、多轨写出
//! - [`split`]：乐器预置表、代价函数、精确分配求解器、汇总报告
//! - [`ui`]：eframe/egui 界面

pub mod i18n;
pub mod logging;
pub mod midi;
pub mod split;
pub mod ui;
