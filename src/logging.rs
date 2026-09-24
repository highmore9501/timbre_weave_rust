//! 全局日志通道
//!
//! 与 `string_flow_rust` 的行为保持一致：日志同时输出到终端（标准输出）与所有订阅者
//! （UI 控制台面板），severity 用于 UI 着色。
//!
//! 使用方式：流水线内部用 [`crate::log_info!`]（普通信息）、[`crate::log_ok!`]（成功）、
//! [`crate::log_warn!`]（警告）、[`crate::log_err!`]（错误）宏记录日志，用法与 `println!` 一致。

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

/// 日志级别
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 普通信息
    Info,
    /// 成功
    Ok,
    /// 警告
    Warn,
    /// 错误
    Error,
}

/// 全局订阅者列表（sender 端），`log` 时向所有存活订阅者广播
static SUBSCRIBERS: OnceLock<Mutex<Vec<Sender<(Severity, String)>>>> = OnceLock::new();

fn subscribers() -> &'static Mutex<Vec<Sender<(Severity, String)>>> {
    SUBSCRIBERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 订阅全局日志，返回消息接收端（通常由 UI 控制台面板持有）
///
/// 订阅端被丢弃后，下次 [`log_with`] 时对应的 sender 会被自动清理，无需手动退订。
pub fn subscribe() -> Receiver<(Severity, String)> {
    let (sender, receiver) = mpsc::channel();
    subscribers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(sender);
    receiver
}

/// 记录一条指定级别的日志：打印到标准输出，并广播给所有订阅者
pub fn log_with(severity: Severity, msg: &str) {
    println!("{}", msg);
    let mut subscriber_list = subscribers().lock().unwrap_or_else(|e| e.into_inner());
    // 清理已断开（Receiver 被丢弃）的订阅者
    subscriber_list.retain(|sender| sender.send((severity, msg.to_string())).is_ok());
}

/// 记录一条普通日志（等同 [`log_with`] 的 `Severity::Info`）
pub fn log(msg: &str) {
    log_with(Severity::Info, msg);
}

/// 便捷日志宏：用法与 `println!` 一致（支持格式化参数），
/// 也接受单个已格式化的表达式（如 `log_info!(crate::t!("key"))`），
/// 把消息同时输出到终端和所有 UI 订阅者。
#[macro_export]
macro_rules! log_info {
    () => {
        $crate::logging::log("")
    };
    ($msg:expr) => {
        $crate::logging::log(&($msg))
    };
    ($($arg:tt)*) => {
        $crate::logging::log(&format!($($arg)*))
    };
}

/// 成功日志宏（UI 控制台显示为绿色）
#[macro_export]
macro_rules! log_ok {
    () => {
        $crate::logging::log_with($crate::logging::Severity::Ok, "")
    };
    ($msg:expr) => {
        $crate::logging::log_with($crate::logging::Severity::Ok, &($msg))
    };
    ($($arg:tt)*) => {
        $crate::logging::log_with($crate::logging::Severity::Ok, &format!($($arg)*))
    };
}

/// 警告日志宏（UI 控制台显示为黄色）
#[macro_export]
macro_rules! log_warn {
    () => {
        $crate::logging::log_with($crate::logging::Severity::Warn, "")
    };
    ($msg:expr) => {
        $crate::logging::log_with($crate::logging::Severity::Warn, &($msg))
    };
    ($($arg:tt)*) => {
        $crate::logging::log_with($crate::logging::Severity::Warn, &format!($($arg)*))
    };
}

/// 错误日志宏（UI 控制台显示为红色）
#[macro_export]
macro_rules! log_err {
    () => {
        $crate::logging::log_with($crate::logging::Severity::Error, "")
    };
    ($msg:expr) => {
        $crate::logging::log_with($crate::logging::Severity::Error, &($msg))
    };
    ($($arg:tt)*) => {
        $crate::logging::log_with($crate::logging::Severity::Error, &format!($($arg)*))
    };
}
