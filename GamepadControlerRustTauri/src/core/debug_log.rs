// =====================================================================
// debug_log.rs —— 调试日志开关
//
// 默认关闭。开启时若当前没有控制台，则调用 AllocConsole 分配一个控制台窗口，
// 之后所有日志通过 println! 输出到该控制台；关闭时只隐藏窗口（保留句柄，避免写入失效）。
// 日志点统一使用 crate::dbg_log! 宏：开关关闭时连格式化都不会执行。
// =====================================================================

use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::System::Console::{AllocConsole, GetConsoleWindow, SetConsoleOutputCP};
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOW, ShowWindow};

// 日志总开关（原子量，任意线程可读写）
static ENABLED: AtomicBool = AtomicBool::new(false);
// 控制台是否由本程序分配（关闭时只隐藏窗口，不释放，避免其它线程正在写入时句柄失效）
static CONSOLE_OWNED: AtomicBool = AtomicBool::new(false);

/// 日志开关是否打开
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 设置日志开关：打开时分配/显示控制台，关闭时隐藏本程序分配的控制台
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::SeqCst);
    // 【Rust 语法】unsafe 块：AllocConsole/ShowWindow 为原生 API 调用
    unsafe {
        if on {
            if !CONSOLE_OWNED.load(Ordering::SeqCst) {
                // 首次打开：分配控制台（已存在控制台时 AllocConsole 会失败，此时直接复用现有控制台）
                if AllocConsole().is_ok() {
                    CONSOLE_OWNED.store(true, Ordering::SeqCst);
                }
            }
            // 控制台输出代码页设为 UTF-8，避免中文日志乱码
            let _ = SetConsoleOutputCP(65001);
            // 已分配但被隐藏过：重新显示
            let hwnd = GetConsoleWindow();
            if !hwnd.0.is_null() {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
        } else if CONSOLE_OWNED.load(Ordering::SeqCst) {
            // 关闭时只隐藏控制台窗口，保留句柄，避免其它线程写入时句柄失效
            let hwnd = GetConsoleWindow();
            if !hwnd.0.is_null() {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
}

/// 输出一行日志（调用方应先用 enabled() 或 dbg_log! 宏做开关判断）
pub fn print(args: std::fmt::Arguments) {
    println!("[调试] {}", args);
}

/// 调试日志宏：开关打开时才格式化并输出
#[macro_export]
macro_rules! dbg_log {
    ($($arg:tt)*) => {{
        if $crate::core::debug_log::enabled() {
            $crate::core::debug_log::print(format_args!($($arg)*));
        }
    }};
}
