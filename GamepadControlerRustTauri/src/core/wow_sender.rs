// =====================================================================
// wow_sender.rs —— 目标游戏窗口定位 + 键盘消息直接投递
//
// 用途（需求3：WASD 拦截）：
//   右键锁定把光标固定在屏幕外后，手柄左摇杆产生的 WASD 不再走
//   SendInput（会随前台窗口变化），而是按「WoW 可执行文件路径」找到
//   对应进程的顶层窗口，用 PostMessageW 直接投递 WM_KEYDOWN/WM_KEYUP。
//
// 说明：
//   - 窗口查找仅在按键状态变化时触发（结果由 MapperState 缓存）；
//   - 返回值统一用 isize 表示窗口句柄（0 = 无效），避免 !Send 的 HWND
//     进入跨线程共享的核心状态。
// =====================================================================

// 【Rust 语法】use 导入：复用注入器里已有的 Android KeyCode -> 物理扫描码映射
use crate::core::injector::{android_key_code_to_windows_scan_code, is_extended_key};
use windows::core::{BOOL, PWSTR}; // 布尔类型 / 可写宽字符串指针（QueryFullProcessImageNameW 出参）
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM, FALSE, TRUE}; // 窗口句柄 / 关闭句柄 / 回调参数
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
}; // 进程查询 API
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindow, PostMessageW, WM_KEYDOWN, WM_KEYUP,
}; // 窗口枚举 / 消息投递

/// 规范化路径：统一小写、反斜杠，便于与进程完整路径比较
fn normalize_path(path: &str) -> String {
    path.trim().replace('/', "\\").to_lowercase() // 全小写比较，避免盘符/目录大小写差异
}

/// 判断缓存窗口句柄是否仍然有效
pub fn is_window_valid(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false; // 空句柄直接判无效
    }
    unsafe { IsWindow(Some(HWND(hwnd as *mut core::ffi::c_void))).as_bool() } // 调用系统 API 检查窗口是否存在
}

// ---------------------------------------------------------------------
// 按进程完整路径匹配顶层窗口
// ---------------------------------------------------------------------

/// 窗口枚举回调的上下文（通过 lparam 指针传入，避免全局状态）
struct FindContext {
    target: String, // 目标 exe 的规范化完整路径
    found: isize,   // 找到的窗口句柄（0 = 未找到）
}

/// 读取进程完整路径
fn process_image_path(pid: u32) -> Option<String> {
    unsafe {
        // 以「有限查询」权限打开目标进程（无需管理员即可读取路径）
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024]; // 宽字符缓冲区（足够容纳长路径）
        let mut len = buf.len() as u32; // 入参为缓冲区字符数，出参为实际长度
        let ok = QueryFullProcessImageNameW(
            handle, // 进程句柄
            PROCESS_NAME_WIN32,             // 使用 Win32 路径格式
            PWSTR(buf.as_mut_ptr()),         // 输出缓冲区
            &mut len,                        // 长度指针
        )
        .is_ok();
        let _ = CloseHandle(handle); // 无论成功与否都关闭句柄
        if !ok {
            return None; // 读取失败（进程可能已退出）
        }
        Some(String::from_utf16_lossy(&buf[..len as usize])) // 仅取有效长度部分转字符串
    }
}

/// EnumWindows 回调：命中目标进程则记录句柄并停止枚举
unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut FindContext); // 还原上下文指针
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid)); // 取窗口所属进程 id
    if pid != 0 {
        if let Some(path) = process_image_path(pid) {
            if normalize_path(&path) == ctx.target {
                ctx.found = hwnd.0 as isize; // 记录命中的窗口句柄
                return FALSE; // 返回 FALSE 停止枚举
            }
        }
    }
    TRUE // 继续枚举下一个窗口
}

/// 按 exe 完整路径查找第一个顶层窗口（返回 isize 句柄，0 = 未找到）
pub fn find_window_by_exe(exe_path: &str) -> Option<isize> {
    if exe_path.trim().is_empty() {
        return None; // 未配置目标程序
    }
    let mut ctx = FindContext {
        target: normalize_path(exe_path),
        found: 0,
    };
    unsafe {
        // 【Rust 语法】WNDENUMPROC 是 Option<unsafe extern "system" fn(..)>；传 Some(函数指针)
        let _ = EnumWindows(Some(enum_windows_proc), LPARAM(&mut ctx as *mut FindContext as isize));
    }
    if ctx.found != 0 {
        Some(ctx.found)
    } else {
        None
    }
}

// ---------------------------------------------------------------------
// 键盘消息投递
// ---------------------------------------------------------------------

/// 向指定窗口投递一次键盘按下/抬起（Android KeyCode 入参）
pub fn send_key(hwnd: isize, android_key: i32, down: bool) {
    if hwnd == 0 {
        return; // 无有效窗口
    }
    let scan = android_key_code_to_windows_scan_code(android_key); // 转为物理扫描码（游戏按扫描码识别）
    if scan == 0 {
        return; // 未知键
    }
    // lParam 布局：低 16 位为重复次数（1）；16~23 位为扫描码；
    // 第 24 位为扩展键标志；第 30 位为前次键状态；第 31 位为转换位（1 = 抬起）。
    let mut lparam: u32 = 1 | ((scan as u32 & 0xFF) << 16);
    if is_extended_key(android_key) {
        lparam |= 1 << 24; // 扩展键
    }
    if !down {
        lparam |= 0xC000_0000; // 抬起：第 30、31 位
    }
    let msg = if down { WM_KEYDOWN } else { WM_KEYUP };
    unsafe {
        // PostMessageW 只要求窗口存在；失败（窗口已销毁等）时忽略
        let _ = PostMessageW(
            Some(HWND(hwnd as *mut core::ffi::c_void)),
            msg,
            WPARAM(0),
            LPARAM(lparam as isize),
        );
    }
}
