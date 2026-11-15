// =====================================================================
// debug_log.rs —— 调试日志开关与内存日志缓冲
//
// 默认关闭。开启后日志不再分配控制台窗口，而是写入内存环形缓冲，
// 由前端「调试日志」窗口通过 get_logs 命令轮询展示。
// 日志点统一使用 crate::dbg_log! 宏：开关关闭时连格式化都不会执行。
// =====================================================================

use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::Win32::System::SystemInformation::GetLocalTime;

/// 内存中最多保留的日志条数，超出后丢弃最旧的
const MAX_LINES: usize = 3000;

// 日志总开关（原子量，任意线程可读写）
static ENABLED: AtomicBool = AtomicBool::new(false);

/// 一条日志
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub seq: u64,   // 递增序号，前端据此增量拉取
    pub time: String, // 本地时间 HH:MM:SS.mmm
    pub text: String, // 日志正文
}

/// 日志查询结果
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub entries: Vec<LogEntry>, // 本次新增的日志
    pub next: u64,              // 当前最新序号
    pub total: usize,           // 缓冲区当前条数
    pub dropped: u64,           // 因超出上限丢弃的条数
    pub enabled: bool,          // 日志开关状态
}

/// 日志缓冲
struct LogStore {
    entries: VecDeque<LogEntry>, // 最近的日志
    next_seq: u64,               // 下一条序号
    dropped: u64,                // 已丢弃条数
}

// 全局日志缓冲：OnceLock 保证只初始化一次，Mutex 保证多线程安全
static STORE: OnceLock<Mutex<LogStore>> = OnceLock::new();

/// 获取全局日志缓冲
fn store() -> &'static Mutex<LogStore> {
    STORE.get_or_init(|| {
        Mutex::new(LogStore {
            entries: VecDeque::with_capacity(MAX_LINES),
            next_seq: 0,
            dropped: 0,
        })
    })
}

/// 日志开关是否打开
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 设置日志开关（只切换开关，不再分配/显示控制台）
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::SeqCst);
}

/// 当前本地时间，格式 HH:MM:SS.mmm
fn now_string() -> String {
    let st = unsafe { GetLocalTime() };
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}

/// 输出一行日志（调用方应先用 enabled() 或 dbg_log! 宏做开关判断）
pub fn print(args: std::fmt::Arguments) {
    let time = now_string();
    let text = format!("{}", args);
    let mut s = store().lock().unwrap_or_else(|e| e.into_inner());
    s.next_seq += 1;
    let entry = LogEntry {
        seq: s.next_seq,
        time,
        text,
    };
    if s.entries.len() >= MAX_LINES {
        s.entries.pop_front();
        s.dropped += 1;
    }
    s.entries.push_back(entry);
}

/// 增量读取日志：返回 seq > after 的所有条目
pub fn page(after: u64) -> LogPage {
    let s = store().lock().unwrap_or_else(|e| e.into_inner());
    let entries: Vec<LogEntry> = s
        .entries
        .iter()
        .filter(|e| e.seq > after)
        .cloned()
        .collect();
    LogPage {
        entries,
        next: s.next_seq,
        total: s.entries.len(),
        dropped: s.dropped,
        enabled: enabled(),
    }
}

/// 清空日志缓冲（序号不重置，避免前端增量拉取错位）
pub fn clear() {
    let mut s = store().lock().unwrap_or_else(|e| e.into_inner());
    s.entries.clear();
    s.dropped = 0;
}

/// 调试日志宏：开关打开时才格式化并写入内存缓冲
#[macro_export]
macro_rules! dbg_log {
    ($($arg:tt)*) => {{
        if $crate::core::debug_log::enabled() {
            $crate::core::debug_log::print(format_args!($($arg)*));
        }
    }};
}
