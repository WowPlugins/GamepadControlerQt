// =====================================================================
// injector.rs —— Windows 本机键鼠注入器（SendInput 实现）
//
// 等效 Qt 版 InputInjector.h/.cpp（WindowsInputInjector）。
// 使用 SendInput() 直接向系统注入键盘/鼠标事件，无跨进程桥接。
//
// 线程安全：所有注入方法内部通过 Mutex 保护按键状态，
// 可安全地从 GUI 线程（按钮/左摇杆）与 look 线程（右摇杆）并发调用。
//
// 说明：
//  - 注入目标为当前前台窗口（即用户正在操作的任意程序，本机使用场景）；
//  - KeyCode 入参为 Android KeyCode，内部转为 VK/扫描码后注入。
// =====================================================================

// 【Rust 语法】use 语句：把其他模块/库中的名称导入当前作用域（类似 C++ 的 using / include 后使用全名）。
// crate::core::input_types 是本项目内部模块路径；{android_key, MouseButton} 为花括号分组导入，一次引入多个项。
use crate::core::input_types::{android_key, MouseButton};
// 【Rust 语法】std::collections::HashSet：标准库哈希集合（元素唯一、无序），类似 C++ 的 std::unordered_set。
use std::collections::HashSet;
// 【Rust 语法】std::sync::Mutex：标准库互斥锁，用于多线程下互斥访问共享数据，类似 C++ 的 std::mutex。
use std::sync::Mutex;
// 【Rust 语法】glob 通配导入（::*）：一次性引入 windows crate 中 KeyboardAndMouse 模块的全部公开项。
// windows crate 是微软官方 Windows API 的 Rust 绑定库（在内部对 FFI/unsafe 调用做了安全封装）。
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CYSCREEN,
};

// windows crate 0.62 未导出这些 XButton / 滚轮常量，自行定义（与 Windows SDK 一致）
// 【Rust 语法】const 声明编译期常量：类型必须显式标注；u32 为无符号 32 位整数。
const XBUTTON1: u32 = 1; // 第 4 鼠标键（后退）
const XBUTTON2: u32 = 2; // 第 5 鼠标键（前进）
// 【Rust 语法】i32 为有符号 32 位整数；WHEEL_DELTA 是 Windows 规定的滚轮一格标准步进值。
const WHEEL_DELTA: i32 = 120; // 滚轮一格

// ---------------------------------------------------------------------
// Android KeyCode -> Windows 虚拟键码（VK）
// 复用安卓版 BridgeInputInjector 的映射表，未知键返回 0。
// ---------------------------------------------------------------------
// 【Rust 语法】pub fn 定义公开函数（pub 对外可见）；参数 code 类型为 i32；-> u16 声明返回类型（无符号 16 位整数）。
// 函数体用 {} 包裹；可用 return 显式返回，也可把最后一个表达式作为返回值。
pub fn android_key_code_to_windows_vk(code: i32) -> u16 { // 函数体开始
    // 字母 A-Z: 29..54 -> VK_A(0x41)..VK_Z
    // 【Rust 语法】(29..=54) 是闭区间范围 RangeInclusive；.contains(&code) 判断范围是否包含该值，参数需传引用 &code。
    if (29..=54).contains(&code) { // 若 code 落在字母 A-Z 区间
        // 【Rust 语法】return 立即返回；0x41 为十六进制字面量；(code - 29) as u16 用 as 做类型转换（i32 → u16）。
        return 0x41 + (code - 29) as u16; // VK_A 加偏移量得到对应字母的虚拟键码
    } // 字母区间分支结束
    // 数字 0-9: 7..16 -> VK_0(0x30)..VK_9
    if (7..=16).contains(&code) { // 若 code 落在数字区间
        return 0x30 + (code - 7) as u16; // VK_0 加偏移得到对应数字键
    } // 数字区间分支结束
    // F1-F12: 131..142 -> VK_F1(0x70)..VK_F12
    if (131..=142).contains(&code) { // 若 code 落在 F1~F12 区间
        return 0x70 + (code - 131) as u16; // VK_F1 加偏移得到对应功能键
    } // F 键区间分支结束
    // 小键盘 0-9: 144..153 -> VK_NUMPAD0(0x60)..VK_NUMPAD9
    if (144..=153).contains(&code) { // 若 code 落在小键盘数字区间
        return 0x60 + (code - 144) as u16; // VK_NUMPAD0 加偏移得到对应小键盘数字
    } // 小键盘区间分支结束
    // 【Rust 语法】match 模式匹配表达式（比 C++ switch 更强大）：对 code 逐一匹配 `模式 => 表达式` 分支。
    // 分支命中后直接采用右侧表达式的值作为 match 结果，无需 break；_ 为通配分支（兜底，类似 default）。
    match code { // 按 Android KeyCode 查表
        android_key::SHIFT_LEFT => 0xA0, // 左 Shift -> VK_LSHIFT
        android_key::SHIFT_RIGHT => 0xA1, // 右 Shift -> VK_RSHIFT
        android_key::CTRL_LEFT => 0xA2, // 左 Ctrl -> VK_LCONTROL
        android_key::CTRL_RIGHT => 0xA3, // 右 Ctrl -> VK_RCONTROL
        android_key::ALT_LEFT => 0xA4, // 左 Alt -> VK_LMENU
        android_key::ALT_RIGHT => 0xA5, // 右 Alt -> VK_RMENU
        android_key::SPACE => 0x20, // 空格 -> VK_SPACE
        android_key::ENTER => 0x0D, // 回车 -> VK_RETURN
        android_key::TAB => 0x09, // Tab 制表键 -> VK_TAB
        android_key::ESCAPE => 0x1B, // Esc -> VK_ESCAPE
        android_key::BACK => 0x08, // 退格 -> VK_BACK
        android_key::DEL => 0x2E, // 删除 -> VK_DELETE
        android_key::INSERT => 0x2D, // 插入 -> VK_INSERT
        android_key::HOME => 0x24, // Home -> VK_HOME
        android_key::PAGE_UP => 0x21, // PageUp -> VK_PRIOR
        android_key::PAGE_DOWN => 0x22, // PageDown -> VK_NEXT
        android_key::MOVE_END => 0x23, // End -> VK_END
        android_key::DPAD_UP => 0x26, // 方向键上 -> VK_UP
        android_key::DPAD_DOWN => 0x28, // 方向键下 -> VK_DOWN
        android_key::DPAD_LEFT => 0x25, // 方向键左 -> VK_LEFT
        android_key::DPAD_RIGHT => 0x27, // 方向键右 -> VK_RIGHT
        android_key::MINUS => 0xBD, // 减号/下划线 -> VK_OEM_MINUS
        android_key::EQUALS => 0xBB, // 等号/加号 -> VK_OEM_PLUS
        android_key::LEFT_BRACKET => 0xDB, // 左方括号 -> VK_OEM_4
        android_key::RIGHT_BRACKET => 0xDD, // 右方括号 -> VK_OEM_6
        android_key::BACKSLASH => 0xDC, // 反斜杠/竖线 -> VK_OEM_5
        android_key::SEMICOLON => 0xBA, // 分号/冒号 -> VK_OEM_1
        android_key::APOSTROPHE => 0xDE, // 单引号/双引号 -> VK_OEM_7
        android_key::COMMA => 0xBC, // 逗号/小于号 -> VK_OEM_COMMA
        android_key::PERIOD => 0xBE, // 句号/大于号 -> VK_OEM_PERIOD
        android_key::SLASH => 0xBF, // 斜杠/问号 -> VK_OEM_2
        android_key::GRAVE => 0xC0, // 反引号/波浪号 -> VK_OEM_3
        android_key::CAPS_LOCK => 0x14, // 大写锁定 -> VK_CAPITAL
        android_key::NUM_LOCK => 0x90, // 数字锁定 -> VK_NUMLOCK
        android_key::SCROLL_LOCK => 0x91, // 滚动锁定 -> VK_SCROLL
        _ => 0, // 未知键返回 0（表示无效）
    } // 【Rust 语法】match 表达式结束；其整个表达式的值将作为函数返回值
} // 函数结束

// ---------------------------------------------------------------------
// Android KeyCode -> Windows 扫描码（硬件码）
// 硬编码映射，不依赖 MapVirtualKey（后台线程可能无正确键盘布局上下文）。
// 扫描码用于 KEYEVENTF_SCANCODE 模式，DirectInput / Raw Input 游戏主要识别此码。
// ---------------------------------------------------------------------
// 【Rust 语法】pub fn 公开函数，参数 code: i32，返回 u16；扫描码是键盘的物理硬件码（与布局无关）。
pub fn android_key_code_to_windows_scan_code(code: i32) -> u16 { // 函数体开始
    // 注意：KEYEVENTF_SCANCODE 模式下 Windows 只认 wScan（硬件扫描码），
    // 必须对应键盘真实物理位置，不能按连续值推算！
    // 字母 A-Z: 29..54（QWERTY 物理扫描码，非连续，必须查表）
    if (29..=54).contains(&code) { // 若为字母 A-Z 区间
        // 【Rust 语法】函数体内可声明 const 定长数组：[u16; 26] 表示「元素为 u16、长度固定 26」；
        // 数组按下标取值，下标类型必须为 usize（见下方 as usize）。
        const SC: [u16; 26] = [
            0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, // A-M
            0x31, 0x18, 0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C, // N-Z
        ]; // 数组字面量结束
        // 【Rust 语法】SC[下标] 取数组元素；usize 是平台相关的无符号索引类型，用 as usize 把 i32 转成下标。
        return SC[(code - 29) as usize]; // 用 (code-29) 作为 0..25 下标查表返回对应扫描码
    } // 字母扫描码分支结束
    // 数字 0-9: 7..16（0 在 9 之后，非连续）
    if (7..=16).contains(&code) { // 若为数字区间
        const SC: [u16; 10] = [0x0B, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A]; // 数字键物理扫描码（1-9,0）
        return SC[(code - 7) as usize]; // 用 (code-7) 作下标查表
    } // 数字扫描码分支结束
    // F1-F10: 131..140 -> 0x3B..0x44（连续）
    // F11/F12 物理扫描码不与 F1-F10 连续（F11=0x57, F12=0x58），单独处理
    if (131..=140).contains(&code) { // 若为 F1~F10 区间
        return 0x3B + (code - 131) as u16; // F1 扫描码 0x3B 加偏移（F1-F10 连续）
    } // F1-F10 分支结束
    if code == android_key::F11 { // 单独判断 F11
        return 0x57; // F11 物理扫描码
    } // F11 分支结束
    if code == android_key::F12 { // 单独判断 F12
        return 0x58; // F12 物理扫描码
    } // F12 分支结束
    // 小键盘 0-9: 144..153（按数字键盘物理布局，非连续）
    if (144..=153).contains(&code) { // 若为小键盘数字区间
        const SC: [u16; 10] = [0x52, 0x4F, 0x50, 0x51, 0x4B, 0x4C, 0x4D, 0x47, 0x48, 0x49]; // 小键盘物理扫描码
        return SC[(code - 144) as usize]; // 用 (code-144) 作下标查表
    } // 小键盘扫描码分支结束
    match code { // 其余按键按映射表匹配
        android_key::SHIFT_LEFT => 0x2A, // 左 Shift 扫描码
        android_key::SHIFT_RIGHT => 0x36, // 右 Shift 扫描码
        android_key::CTRL_LEFT => 0x1D, // 左 Ctrl 扫描码
        android_key::CTRL_RIGHT => 0x1D, // E0 扩展
        android_key::ALT_LEFT => 0x38, // 左 Alt 扫描码
        android_key::ALT_RIGHT => 0x38, // E0 扩展
        android_key::SPACE => 0x39, // 空格扫描码
        android_key::ENTER => 0x1C, // 回车扫描码
        android_key::TAB => 0x0F, // Tab 扫描码
        android_key::ESCAPE => 0x01, // Esc 扫描码
        android_key::BACK => 0x0E, // 退格扫描码
        android_key::DEL => 0x53,    // E0 扩展
        android_key::INSERT => 0x52, // E0 扩展
        android_key::HOME => 0x47,   // E0 扩展
        android_key::PAGE_UP => 0x49, // E0 扩展
        android_key::PAGE_DOWN => 0x51, // E0 扩展
        android_key::MOVE_END => 0x4F, // E0 扩展
        android_key::DPAD_UP => 0x48, // 上方向键扫描码
        android_key::DPAD_DOWN => 0x50, // 下方向键扫描码
        android_key::DPAD_LEFT => 0x4B, // 左方向键扫描码
        android_key::DPAD_RIGHT => 0x4D, // 右方向键扫描码
        android_key::MINUS => 0x0C, // 减号扫描码
        android_key::EQUALS => 0x0D, // 等号扫描码
        android_key::LEFT_BRACKET => 0x1A, // 左方括号扫描码
        android_key::RIGHT_BRACKET => 0x1B, // 右方括号扫描码
        android_key::BACKSLASH => 0x2B, // 反斜杠扫描码
        android_key::SEMICOLON => 0x27, // 分号扫描码
        android_key::APOSTROPHE => 0x28, // 单引号扫描码
        android_key::COMMA => 0x33, // 逗号扫描码
        android_key::PERIOD => 0x34, // 句号扫描码
        android_key::SLASH => 0x35, // 斜杠扫描码
        android_key::GRAVE => 0x29, // 反引号扫描码
        android_key::CAPS_LOCK => 0x3A, // 大写锁定扫描码
        android_key::NUM_LOCK => 0x45, // E0 扩展（Numpad）
        android_key::SCROLL_LOCK => 0x46, // 滚动锁定扫描码
        _ => 0, // 未知键返回 0
    } // match 结束，其值作为函数返回值
} // 函数结束

/// 判断 Android KeyCode 是否为扩展键（需要 KEYEVENTF_EXTENDEDKEY）
/// 【pub(crate)】仅在本 crate 内可见：wow_sender 组装 PostMessage 的 lParam 时复用同一判定。
pub(crate) fn is_extended_key(code: i32) -> bool { // 函数体开始
    // 【Rust 语法】matches! 宏：判断表达式是否匹配给定模式，返回 bool，等价于 `match 表达式 { 模式 => true, _ => false }`。
    // 模式中 | 表示「或」（多个备选模式之一命中即 true），无需写成多个 match 分支。
    matches!(
        code,
        android_key::DPAD_UP // 方向键上属于扩展键
            | android_key::DPAD_DOWN // 方向键下
            | android_key::DPAD_LEFT // 方向键左
            | android_key::DPAD_RIGHT // 方向键右
            | android_key::INSERT // 插入键
            | android_key::DEL // 删除键
            | android_key::HOME // Home 键
            | android_key::MOVE_END // End 键
            | android_key::PAGE_UP // PageUp 键
            | android_key::PAGE_DOWN // PageDown 键
            | android_key::CTRL_RIGHT // 右 Ctrl
            | android_key::ALT_RIGHT // 右 Alt
            | android_key::NUM_LOCK // 数字锁定键
    ) // matches! 调用结束，其值为 bool
} // 函数结束

/// 鼠标按键对应的 SendInput 事件标志（按下/松开）与 XButton 数据
// 【Rust 语法】返回类型是元组 (A, B, C)：用圆括号括起的多个值的组合；MOUSE_EVENT_FLAGS 为 windows crate
// 的位标志类型（支持 | 位或运算）；u32 为 X 键编号数据。
fn mouse_flags_for(b: MouseButton) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS, u32) { // 函数体开始
    // Windows 约定：XBUTTON1=第 4 键（后退）、XBUTTON2=第 5 键（前进），不能弄反。
    match b { // 按鼠标键枚举匹配
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 0), // 左键：按下/松开标志，无 X 键数据
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 0), // 右键
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, 0), // 中键
        MouseButton::Forward => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON2), // 前进键（X 键，携带 XBUTTON2 数据）
        MouseButton::Back => (MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, XBUTTON1), // 后退键（X 键，携带 XBUTTON1 数据）
    } // match 结束，返回三元组
} // 函数结束

/// 注入单个键盘事件（down=true 按下，false 松开）
/// 使用 KEYEVENTF_SCANCODE（物理扫描码）模式：MSDN 规定该模式下 wVk 必须为 0。
// 【Rust 语法】unsafe fn 表示「不安全的函数」：其内部可能进行 Rust 编译器无法证明安全的操作（此处调用
// Windows FFI 注入函数 SendInput）。调用者必须在 unsafe { } 块中才能调用它（见 send_key_down 调用处）。
unsafe fn inject_key(code: i32, down: bool) { // 函数体开始
    let sc = android_key_code_to_windows_scan_code(code); // 【Rust 语法】let 声明不可变绑定：把 Android KeyCode 转为 Windows 扫描码
    if sc == 0 { // 扫描码为 0 表示未知键
        return; // 无法注入，直接返回
    } // 未知键分支结束
    let mut flags = KEYEVENTF_SCANCODE; // 【Rust 语法】let mut 声明可变绑定（默认 let 不可变）；初始标志为「扫描码模式」
    if !down { // 若是松开事件（down 为 false）
        flags |= KEYEVENTF_KEYUP; // 【Rust 语法】|= 是按位或赋值运算符：给标志追加 KEYUP（松开）位
    } // 松开标志分支结束
    if is_extended_key(code) { // 若是扩展键
        flags |= KEYEVENTF_EXTENDEDKEY; // 追加扩展键标志（对应键盘的 E0 前缀键）
    } // 扩展标志分支结束
    // 【Rust 语法】结构体字面量：INPUT { 字段: 值, ... } 构造结构体实例；r#type 是原始标识符
    //（type 是 Rust 关键字，需用 r# 前缀转义为字段名）；windows crate 的 INPUT 是联合体（union），
    // 其 Anonymous 字段为 INPUT_0，内含键盘数据结构 ki: KEYBDINPUT。
    let mut input = INPUT { // 构造一个键盘输入事件结构体
        r#type: INPUT_KEYBOARD, // 事件类型：键盘输入
        Anonymous: INPUT_0 { // 联合体成员：键盘/鼠标数据区
            ki: KEYBDINPUT { // 键盘输入数据结构体
                wVk: VIRTUAL_KEY(0), // 【Rust 语法】VIRTUAL_KEY(0) 是「newtype」元组结构体的构造：包一个 0 值虚拟键码（扫描码模式下必须为 0）
                wScan: sc, // 硬件扫描码（本实现使用扫描码而非虚拟键码）
                dwFlags: flags, // 事件标志（扫描码模式/松开/扩展键）
                time: 0, // 时间戳（0 表示由系统自动填充）
                dwExtraInfo: 0, // 附加信息（未使用）
            }, // KEYBDINPUT 结构体结束
        }, // INPUT_0 联合体成员结束
    }; // INPUT 结构体结束
    SendInput(&[input], std::mem::size_of::<INPUT>() as i32); // 【Rust 语法】FFI 调用 SendInput：&[input] 是数组切片引用（内含 1 个事件），
    // 第二参数 1 为事件个数；该函数返回注入的事件数（此处忽略返回值）。
} // 函数结束

/// 注入单个鼠标按键事件
unsafe fn inject_mouse_button_raw(b: MouseButton, down: bool) { // 函数体开始
    let (df, uf, data) = mouse_flags_for(b); // 【Rust 语法】let 解构元组：同时取出按下标志 df、松开标志 uf、X 键数据 data
    let flag = if down { df } else { uf }; // 【Rust 语法】if 作为表达式使用：按 down 选择按下/松开标志，结果赋给不可变变量 flag
    if flag.0 == 0 { // 【Rust 语法】.0 访问位标志的底层数值；判断标志位是否为 0
        return; // 标志为 0 表示该键没有对应事件，直接返回
    } // 空标志分支结束
    let mut input = INPUT { // 构造鼠标输入事件结构体
        r#type: INPUT_MOUSE, // 事件类型：鼠标输入
        Anonymous: INPUT_0 { // 联合体成员
            mi: MOUSEINPUT { // 鼠标输入数据结构体
                dx: 0, // 相对移动 X（鼠标按键事件不移动）
                dy: 0, // 相对移动 Y
                mouseData: data, // 鼠标数据（X 键编号；滚轮事件则为滚轮量）
                dwFlags: flag, // 事件标志（按下/松开）
                time: 0, // 时间戳
                dwExtraInfo: 0, // 附加信息
            }, // MOUSEINPUT 结构体结束
        }, // INPUT_0 联合体成员结束
    }; // INPUT 结构体结束
    SendInput(&[input], std::mem::size_of::<INPUT>() as i32); // 调用 SendInput 注入一个鼠标事件
} // 函数结束

/// 注入相对鼠标移动事件
unsafe fn inject_mouse_move_raw(dx: i32, dy: i32) { // 函数体开始
    let mut input = INPUT { // 构造鼠标移动事件结构体
        r#type: INPUT_MOUSE, // 事件类型：鼠标输入
        Anonymous: INPUT_0 { // 联合体成员
            mi: MOUSEINPUT { // 鼠标输入数据结构体
                dx, // 【Rust 语法】字段简写：字段名与传入变量同名时可省略 `dx: dx`，直接写 dx
                dy, // 同上简写：dy: dy
                mouseData: 0, // 移动事件无鼠标数据
                dwFlags: MOUSEEVENTF_MOVE, // 事件标志：相对移动
                time: 0, // 时间戳
                dwExtraInfo: 0, // 附加信息
            }, // MOUSEINPUT 结构体结束
        }, // INPUT_0 联合体成员结束
    }; // INPUT 结构体结束
    SendInput(&[input], std::mem::size_of::<INPUT>() as i32); // 调用 SendInput 注入一个鼠标移动事件
} // 函数结束

/// WindowsInputInjector —— SendInput 实现
///
/// 状态记录（pressed_keys_/pressed_buttons_）用于：
///  - 去重：同一键未松开前不会重复注入按下事件
///  - 精确释放：release_all 时遍历释放所有仍按住的键/鼠标键
// 【Rust 语法】pub struct 定义公开结构体（类似 C++ class 的数据成员）；字段默认私有（仅本模块可访问）。
// state 字段类型 Mutex<InjectorState>：用互斥锁包裹内部状态，实现多线程安全的共享可变状态。
pub struct InputInjector {
    state: Mutex<InjectorState>, // 注入状态（互斥锁保护）
} // 结构体定义结束

// 【Rust 语法】struct 定义私有结构体：记录注入器当前的内部状态（无 pub 即模块私有）。
struct InjectorState {
    /// 当前按下的 Android KeyCode
    pressed_keys: HashSet<i32>, // 【Rust 语法】HashSet<i32> 泛型集合：元素类型为 i32
    /// 当前按下的鼠标键
    pressed_buttons: HashSet<MouseButton>, // 鼠标按键集合
    /// 亚像素余量累积（X 方向）
    mouse_remainder_x: f32, // 【Rust 语法】f32 为 32 位单精度浮点类型
    /// 亚像素余量累积（Y 方向）
    mouse_remainder_y: f32, // Y 方向浮点余量
} // 结构体定义结束

// 【Rust 语法】impl 块：为类型实现方法或特征（trait）。`impl Default for InputInjector` 表示实现标准库的
// Default 特征；trait 是 Rust 的接口机制（类似 C++ 的纯虚基类/接口约定）。
impl Default for InputInjector {
    // 【Rust 语法】fn default() -> Self：Self 关键字指代当前实现的类型（此处为 InputInjector）。
    fn default() -> Self { // 特征要求的方法：返回该类型的默认实例
        Self::new() // 【Rust 语法】Self::new() 调用本类型的关联函数 new（类似 C++ 的静态成员函数），返回 Self 实例
    } // 方法结束
} // impl Default 块结束

// 【Rust 语法】impl InputInjector { ... }：为 InputInjector 定义方法；没有 pub 的方法为私有方法。
impl InputInjector {
    // 【Rust 语法】pub fn new() -> Self：公开关联函数（Rust 约定构造函数叫 new）；不接收 self，返回新实例。
    pub fn new() -> Self { // 创建注入器实例
        Self { // 结构体字面量构造 Self
            state: Mutex::new(InjectorState { // 【Rust 语法】Mutex::new(...) 创建互斥锁并装入初始状态
                pressed_keys: HashSet::new(), // 【Rust 语法】HashSet::new() 创建空集合
                pressed_buttons: HashSet::new(), // 创建空鼠标键集合
                mouse_remainder_x: 0.0, // 初始 X 余量为 0.0
                mouse_remainder_y: 0.0, // 初始 Y 余量为 0.0
            }), // InjectorState 构造结束
        } // Self 构造结束
    } // new 结束

    /// 注入能力是否可用（Windows 本机实现恒为 true）
    // 【Rust 语法】&self：对当前实例的不可变借用（只读访问，方法不修改状态）；返回 bool。
    pub fn is_available(&self) -> bool { // 方法体开始
        true // 最后一个表达式作为返回值：本机实现恒为可用
    } // 方法结束

    /// 按下按键（入参为 Android KeyCode；去重后注入）
    pub fn send_key_down(&self, android_key_code: i32) { // 按下按键
        if android_key_code_to_windows_vk(android_key_code) == 0 { // 先判断该键是否有有效的 VK 映射
            return; // 无效键（VK 为 0）直接返回，不注入
        } // 无效键分支结束
        // 【Rust 语法】lock() 获取互斥锁，返回 MutexGuard 智能指针（作用域结束自动释放锁，类似 RAII）；
        // unwrap() 处理 Result：锁中毒（panic）时直接崩溃；mut 表示可变绑定（要修改内部状态）；
        // MutexGuard 支持 Deref 自动解引用访问内部字段。
        let mut st = self.state.lock().unwrap(); // 获取互斥锁
        if !st.pressed_keys.insert(android_key_code) { // 【Rust 语法】HashSet::insert 返回 bool：新插入 true、已存在 false；! 取反进入「已存在」分支
            return; // 已按下，忽略重复
        } // 去重分支结束
        drop(st); // 【Rust 语法】drop() 显式释放值：提前解锁（避免持锁期间调用注入，降低锁竞争）
        unsafe { inject_key(android_key_code, true) }; // 【Rust 语法】unsafe 块：调用 unsafe fn 必须用 unsafe 块包裹；此处注入「按下」事件
    } // 方法结束

    /// 松开按键（只在确实按下过时发送）
    pub fn send_key_up(&self, android_key_code: i32) { // 松开按键
        if android_key_code_to_windows_vk(android_key_code) == 0 { // 无效键判断
            return; // 无效键直接返回
        } // 无效键分支结束
        let mut st = self.state.lock().unwrap(); // 获取互斥锁
        if !st.pressed_keys.remove(&android_key_code) { // 【Rust 语法】HashSet::remove 接收元素引用 &T，返回是否移除成功（此前存在）；! 取反进入「未按下过」分支
            return; // 从未按下过，无需注入松开
        } // 去重分支结束
        drop(st); // 提前解锁
        unsafe { inject_key(android_key_code, false) }; // 注入「松开」事件
    } // 方法结束

    /// 按下鼠标按键（去重）
    pub fn send_mouse_down(&self, button: MouseButton) { // 按下鼠标键
        let mut st = self.state.lock().unwrap(); // 获取互斥锁
        if !st.pressed_buttons.insert(button) { // 已按下则忽略重复
            return; // 已按下，忽略重复
        } // 去重分支结束
        drop(st); // 提前解锁
        unsafe { inject_mouse_button_raw(button, true) }; // 注入鼠标「按下」事件
    } // 方法结束

    /// 松开鼠标按键（只在确实按下过时发送）
    pub fn send_mouse_up(&self, button: MouseButton) { // 松开鼠标键
        let mut st = self.state.lock().unwrap(); // 获取互斥锁
        if !st.pressed_buttons.remove(&button) { // 未按下过则忽略
            return; // 从未按下过，无需注入松开
        } // 去重分支结束
        drop(st); // 提前解锁
        unsafe { inject_mouse_button_raw(button, false) }; // 注入鼠标「松开」事件
    } // 方法结束

    /// 滚动鼠标滚轮（steps>0 上滚、<0 下滚，单位：格）
    pub fn send_mouse_wheel(&self, steps: i32) { // 滚轮方法
        if steps == 0 { // 步数为 0 无需注入
            return; // 直接返回
        } // 零步数分支结束
        let mut input = INPUT { // 构造滚轮事件结构体
            r#type: INPUT_MOUSE, // 事件类型：鼠标输入
            Anonymous: INPUT_0 { // 联合体成员
                mi: MOUSEINPUT { // 鼠标输入数据结构体
                    dx: 0, // 无 X 移动
                    dy: 0, // 无 Y 移动
                    mouseData: (steps * WHEEL_DELTA as i32) as u32, // 1 格 = 120
                    dwFlags: MOUSEEVENTF_WHEEL, // 事件标志：滚轮
                    time: 0, // 时间戳
                    dwExtraInfo: 0, // 附加信息
                }, // MOUSEINPUT 结构体结束
            }, // INPUT_0 联合体成员结束
        }; // INPUT 结构体结束
        unsafe { // 【Rust 语法】unsafe 块：包裹 FFI 调用
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32); // 调用 SendInput 注入滚轮事件
        } // unsafe 块结束
    } // 方法结束

    /// 相对移动鼠标（像素，允许小数）
    /// 亚像素余量累积：小数部分保留，累积满 1px 才补发。
    pub fn send_mouse_move(&self, dx: f32, dy: f32) { // 相对移动方法（参数为浮点）
        let mut st = self.state.lock().unwrap(); // 获取互斥锁
        st.mouse_remainder_x += dx; // 【Rust 语法】+= 复合赋值：把 dx 累加到 X 方向余量
        st.mouse_remainder_y += dy; // 累加到 Y 方向余量
        let ix = st.mouse_remainder_x as i32; // 【Rust 语法】as 类型转换：浮点取整为整数（截断小数）
        let iy = st.mouse_remainder_y as i32; // Y 方向取整
        if ix == 0 && iy == 0 { // 【Rust 语法】&& 为逻辑与；整数部分均为 0 说明不足 1 像素
            return; // 返回（小数余量保留，待下次累积）
        } // 零移动分支结束
        st.mouse_remainder_x -= ix as f32; // 【Rust 语法】-= 复合赋值：减去已发送的整数部分，只保留小数余量
        st.mouse_remainder_y -= iy as f32; // 同上（Y 方向）
        drop(st); // 提前解锁
        unsafe { inject_mouse_move_raw(ix, iy) }; // 注入相对移动事件
    } // 方法结束

    /// 释放所有仍按住的键盘/鼠标键（手柄断开、停止映射、退出时调用）
    pub fn release_all(&self) { // 释放全部输入
        let mut st = self.state.lock().unwrap(); // 获取互斥锁
        // 【Rust 语法】for 循环遍历迭代器；.iter() 返回借用元素的迭代器（类似 C++ 的范围 for）；
        // 模式 &k 把迭代出的 &i32 解引用为 i32 值。
        for &k in st.pressed_keys.iter() { // 遍历仍按住的键
            unsafe { inject_key(k, false) }; // 对每个仍按住的键注入「松开」事件
        } // 循环结束
        st.pressed_keys.clear(); // 【Rust 语法】clear() 清空集合
        for &b in st.pressed_buttons.iter() { // 遍历仍按住的鼠标键
            unsafe { inject_mouse_button_raw(b, false) }; // 对每个注入「松开」事件
        } // 循环结束
        st.pressed_buttons.clear(); // 清空鼠标键集合
        st.mouse_remainder_x = 0.0; // 清零 X 方向余量
        st.mouse_remainder_y = 0.0; // 清零 Y 方向余量
    } // 方法结束

    // -----------------------------------------------------------------
    // 光标位置 / 屏幕尺寸（交互键与 WASD 拦截使用）
    // -----------------------------------------------------------------

    /// 读取当前鼠标光标屏幕坐标
    pub fn get_cursor_pos(&self) -> (i32, i32) {
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut p).is_err() {
                return (0, 0);
            }
        }
        (p.x, p.y)
    }

    /// 把光标移动到指定屏幕坐标（超出屏幕时由系统钳制到边缘）
    pub fn set_cursor_pos(&self, x: i32, y: i32) {
        unsafe {
            let _ = SetCursorPos(x, y);
        }
    }

    /// 主屏幕尺寸（宽, 高）
    pub fn screen_size(&self) -> (i32, i32) {
        unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
    }
} // impl InputInjector 块结束
