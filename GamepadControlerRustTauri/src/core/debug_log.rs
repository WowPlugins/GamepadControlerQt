// =====================================================================
// debug_log.rs —— 调试日志开关、内存日志缓冲与日志文件
//
// 默认关闭。开启后日志同时写入内存环形缓冲和 exe 同目录的 debug.log；
// 「调试日志」窗口打开时直接读取日志文件内容展示，之后增量轮询内存缓冲。
// 程序每次启动会清空上一次运行留下的日志文件。
// 日志点统一使用 crate::dbg_log! 宏：开关关闭时连格式化都不会执行。
// =====================================================================

use serde::Serialize;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::Win32::System::SystemInformation::GetLocalTime;

/// 内存中最多保留的日志条数，超出后丢弃最旧的（日志窗口一次性展示的行数上限）
pub const MAX_LINES: usize = 3000;

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

/// 日志文件路径：exe 同目录 debug.log（与配置文件位置一致）
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// 取日志文件路径（首次调用时解析，后续复用）
fn log_path() -> &'static PathBuf {
    LOG_PATH.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("debug.log")))
            .unwrap_or_else(|| PathBuf::from("debug.log"))
    })
}

/// 程序启动时调用：清空上一次运行留下的日志文件
pub fn reset_file() {
    let _ = File::create(log_path()); // 创建 / 截断为空文件
}

/// 追加一行到日志文件（失败时忽略，日志不能影响主流程）
fn append_file(time: &str, text: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = writeln!(f, "{}  {}", time, text); // 时间与正文用两个空格分隔（日志窗口按此拆分）
    }
}

/// 读取日志文件最后 max 行（日志窗口打开时展示）
pub fn read_tail(max: usize) -> Vec<String> {
    let file = match File::open(log_path()) {
        Ok(f) => f, // 文件存在
        Err(_) => return Vec::new(), // 尚未产生日志
    };
    let mut lines: VecDeque<String> = VecDeque::with_capacity(max.min(MAX_LINES)); // 只保留最后 max 行
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if lines.len() >= max {
            lines.pop_front(); // 超出上限时丢弃最旧的一行
        }
        lines.push_back(line);
    }
    lines.into_iter().collect()
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
    append_file(&time, &text); // 写入日志文件（日志窗口打开时按文件内容展示）
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

/// 清空日志缓冲与日志文件（序号不重置，避免前端增量拉取错位）
pub fn clear() {
    {
        let mut s = store().lock().unwrap_or_else(|e| e.into_inner());
        s.entries.clear();
        s.dropped = 0;
    }
    let _ = File::create(log_path()); // 同时清空日志文件
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
