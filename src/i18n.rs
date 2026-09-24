//! 中英双语 i18n 模块
//!
//! 机制照搬 `string_flow_rust`，保持两个项目的行为一致：
//! - 词典为外部 JSON 文件（`asset/i18n/<lang>.json`），运行时加载；代码内嵌 `zh-CN` / `en-US`
//!   作为兜底，词典文件缺失也能启动。
//! - **新增语言 = 在 `asset/i18n/` 放一个 JSON 文件**（含 `__meta__` 元信息），无需改代码；
//!   顶栏语言菜单会自动列出运行时发现的全部语言。
//! - 语言设置持久化到工作目录下的 `config.json`：值为 `"system"`（跟随系统）或语言码。
//! - 代码中统一用 [`t!`] 取词：`t!("key")` / `t!("key", arg1, arg2)`。词典值中的 `{}`
//!   为占位符，按位置替换；词典缺失 key 时回退到 `zh-CN`，再缺失则原样返回 key。

use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

/// 默认（兜底）语言
pub const DEFAULT_LANG: &str = "zh-CN";
/// “跟随系统”在 config.json 中的取值
pub const SYSTEM_SETTING: &str = "system";
/// 词典目录（相对工作目录）
const I18N_DIR: &str = "asset/i18n";
/// 配置文件（相对工作目录）
const CONFIG_FILE: &str = "config.json";

/// 一种可用语言的元信息（显示在语言菜单里）
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LangMeta {
    pub code: String,
    pub native_name: String,
    pub english_name: String,
}

struct I18nState {
    /// 用户设置："system" 或语言码
    setting: String,
    /// 当前生效的语言码（setting == "system" 时为检测结果）
    resolved: String,
    /// 生效词典：zh-CN 为基底，叠加当前语言覆盖
    dict: Arc<HashMap<String, String>>,
    /// 全部可用语言（内嵌 + 运行时发现）
    metas: HashMap<String, LangMeta>,
}

static STATE: OnceLock<RwLock<I18nState>> = OnceLock::new();

fn state() -> &'static RwLock<I18nState> {
    STATE.get_or_init(|| RwLock::new(build_state()))
}

/// 初始化 i18n（在创建 UI 之前调用一次即可；内部为惰性构建，重复调用无害）
pub fn init() {
    let _ = state();
}

/// 当前生效的语言码（如 "zh-CN" / "en-US"）
pub fn current_language() -> String {
    let state_guard = state().read().unwrap_or_else(|e| e.into_inner());
    state_guard.resolved.clone()
}

/// 当前用户设置："system" 或语言码
pub fn current_setting() -> String {
    let state_guard = state().read().unwrap_or_else(|e| e.into_inner());
    state_guard.setting.clone()
}

/// 手动切换语言（立即生效并持久化到 config.json）
pub fn set_language(code: &str) {
    let mut state_guard = state().write().unwrap_or_else(|e| e.into_inner());
    if !state_guard.metas.contains_key(code) {
        return;
    }
    state_guard.setting = code.to_string();
    state_guard.resolved = code.to_string();
    state_guard.dict = Arc::new(build_dict(code));
    drop(state_guard);
    save_setting(code);
}

/// 切换为“跟随系统”（立即生效并持久化到 config.json）
pub fn set_follow_system() {
    let mut state_guard = state().write().unwrap_or_else(|e| e.into_inner());
    let resolved = detect_system_language(&state_guard.metas);
    state_guard.setting = SYSTEM_SETTING.to_string();
    state_guard.resolved = resolved.clone();
    state_guard.dict = Arc::new(build_dict(&resolved));
    drop(state_guard);
    save_setting(SYSTEM_SETTING);
}

/// 全部可用语言（zh-CN、en-US 优先，其余按语言码排序）
pub fn available_languages() -> Vec<LangMeta> {
    let state_guard = state().read().unwrap_or_else(|e| e.into_inner());
    let mut language_list: Vec<LangMeta> = state_guard.metas.values().cloned().collect();
    language_list.sort_by(|meta_a, meta_b| {
        rank(meta_a)
            .cmp(&rank(meta_b))
            .then_with(|| meta_a.code.cmp(&meta_b.code))
    });
    language_list
}

fn rank(meta: &LangMeta) -> u8 {
    match meta.code.as_str() {
        "zh-CN" => 0,
        "en-US" => 1,
        _ => 2,
    }
}

/// 取词：返回本地化字符串（`{}` 占位符按位置替换）
pub fn tr(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    let state_guard = state().read().unwrap_or_else(|e| e.into_inner());
    let template = state_guard.dict.get(key).map(String::as_str).unwrap_or(key);
    if args.is_empty() {
        return template.to_string();
    }
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    let mut arg_index = 0;
    loop {
        match rest.find("{}") {
            Some(position) => {
                out.push_str(&rest[..position]);
                if arg_index < args.len() {
                    out.push_str(&args[arg_index].to_string());
                } else {
                    out.push_str("{}");
                }
                arg_index += 1;
                rest = &rest[position + 2..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

/// 取词宏：`t!("key")` 或 `t!("key", arg1, arg2)`，用法与 `format!` 一致
#[macro_export]
macro_rules! t {
    ($key:expr $(, $arg:expr)* $(,)?) => {{
        let args: &[&dyn ::core::fmt::Display] = &[$( &($arg) ),*];
        $crate::i18n::tr($key, args)
    }};
}

// ────────────────────────────── 内部实现 ──────────────────────────────

fn build_state() -> I18nState {
    // 1) 收集可用语言（内嵌 zh-CN / en-US + 运行时发现的词典文件）
    let mut metas: HashMap<String, LangMeta> = HashMap::new();
    insert_meta_from_json(&mut metas, include_str!("../asset/i18n/zh-CN.json"));
    insert_meta_from_json(&mut metas, include_str!("../asset/i18n/en-US.json"));
    if let Ok(entries) = std::fs::read_dir(I18N_DIR) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|extension| extension == "json").unwrap_or(false) {
                if let Ok(json_text) = std::fs::read_to_string(&path) {
                    insert_meta_from_json(&mut metas, &json_text);
                }
            }
        }
    }

    // 2) 读取持久化的语言设置
    let setting = load_setting(&metas);

    // 3) 解析生效语言
    let resolved = match setting.as_str() {
        SYSTEM_SETTING => detect_system_language(&metas),
        configured_code => configured_code.to_string(),
    };

    // 4) 构建生效词典
    let dict = Arc::new(build_dict(&resolved));

    I18nState {
        setting,
        resolved,
        dict,
        metas,
    }
}

fn insert_meta_from_json(metas: &mut HashMap<String, LangMeta>, json_text: &str) {
    if let Ok(Value::Object(json_object)) = serde_json::from_str::<Value>(json_text) {
        if let Some(Value::Object(meta)) = json_object.get("__meta__") {
            let get_field = |field_name: &str| {
                meta.get(field_name)
                    .and_then(|json_value| json_value.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let code = get_field("code");
            if code.is_empty() {
                return;
            }
            metas.insert(
                code.clone(),
                LangMeta {
                    code,
                    native_name: get_field("native_name"),
                    english_name: get_field("english_name"),
                },
            );
        }
    }
}

fn load_setting(metas: &HashMap<String, LangMeta>) -> String {
    if let Ok(json_text) = std::fs::read_to_string(CONFIG_FILE) {
        if let Ok(config_json) = serde_json::from_str::<Value>(&json_text) {
            if let Some(language) = config_json
                .get("language")
                .and_then(|language_value| language_value.as_str())
            {
                if language == SYSTEM_SETTING || metas.contains_key(language) {
                    return language.to_string();
                }
            }
        }
    }
    SYSTEM_SETTING.to_string()
}

/// 检测系统语言：精确匹配语言码，其次按前两位语言码前缀匹配，最后回退默认语言
fn detect_system_language(metas: &HashMap<String, LangMeta>) -> String {
    let locale = sys_locale::get_locale()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace('_', "-");
    if locale.is_empty() {
        return DEFAULT_LANG.to_string();
    }
    let normalize = |code: &str| code.to_ascii_lowercase().replace('_', "-");

    for code in metas.keys() {
        if normalize(code) == locale {
            return code.clone();
        }
    }
    let language_prefix: String = locale.chars().take(2).collect();
    if language_prefix.len() == 2 {
        for code in metas.keys() {
            if normalize(code).starts_with(&language_prefix) {
                return code.clone();
            }
        }
    }
    DEFAULT_LANG.to_string()
}

/// 构建生效词典：zh-CN 为基底，叠加目标语言（内嵌 + 运行时文件覆盖）
fn build_dict(resolved: &str) -> HashMap<String, String> {
    let mut dict = load_dict(include_str!("../asset/i18n/zh-CN.json"));

    let embedded = match resolved {
        "zh-CN" => None,
        "en-US" => Some(include_str!("../asset/i18n/en-US.json")),
        _ => None,
    };
    if let Some(embedded_text) = embedded {
        for (key, value) in load_dict(embedded_text) {
            dict.insert(key, value);
        }
    }

    let runtime_path = Path::new(I18N_DIR).join(format!("{}.json", resolved));
    if let Ok(runtime_text) = std::fs::read_to_string(&runtime_path) {
        for (key, value) in load_dict(&runtime_text) {
            dict.insert(key, value);
        }
    }

    dict
}

fn load_dict(json_text: &str) -> HashMap<String, String> {
    let mut dict = HashMap::new();
    if let Ok(Value::Object(json_object)) = serde_json::from_str::<Value>(json_text) {
        for (key, value) in json_object {
            if key == "__meta__" {
                continue;
            }
            if let Some(translation) = value.as_str() {
                dict.insert(key, translation.to_string());
            }
        }
    }
    dict
}

fn save_setting(setting: &str) {
    let json = serde_json::json!({ "language": setting });
    let result = serde_json::to_string_pretty(&json)
        .map_err(|e| e.to_string())
        .and_then(|text| std::fs::write(CONFIG_FILE, text).map_err(|e| e.to_string()));
    if let Err(error) = result {
        eprintln!("{}", crate::t!("log.i18n_save_failed", error));
    }
}
