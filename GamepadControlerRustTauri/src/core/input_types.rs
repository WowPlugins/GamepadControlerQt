// =====================================================================
// input_types.rs —— 手柄/鼠标/键盘基础类型
//
// 与 Qt 版（InputTypes.h）保持一致：
//  - KeyCode 沿用 Android KeyEvent 常量，保证配置文件格式兼容；
//  - 手柄按钮/鼠标按钮/摇杆统一为本地枚举，避免暴露 Windows XInput
//    或 Android KeyEvent 的底层差异。
// =====================================================================

// ---------------------------------------------------------------------
// Android KeyEvent keycode 常量（配置文件中保存的按键值）
// 运行时通过 injector::android_key_code_to_windows_vk() 转换为 Windows
// 虚拟键码（VK）后再注入系统。
// ---------------------------------------------------------------------
// 【Rust 语法】#[allow(non_upper_case_globals)]：允许常量名非大写，避免编译告警
#[allow(non_upper_case_globals)]
// 【Rust 语法】pub mod：声明公开模块 android_key，里面定义一组按键常量
pub mod android_key {
    // 字母 A-Z: 29..54
    // 【Rust 语法】pub const：公开常量，i32 为 32 位有符号整数；值即 Android KeyEvent 键码
    pub const A: i32 = 29; // 字母 A 的键码
    pub const B: i32 = 30; // 字母 B 的键码
    pub const C: i32 = 31; // 字母 C 的键码
    pub const D: i32 = 32; // 字母 D 的键码
    pub const E: i32 = 33; // 字母 E 的键码
    pub const F: i32 = 34; // 字母 F 的键码
    pub const G: i32 = 35; // 字母 G 的键码
    pub const H: i32 = 36; // 字母 H 的键码
    pub const I: i32 = 37; // 字母 I 的键码
    pub const J: i32 = 38; // 字母 J 的键码
    pub const K: i32 = 39; // 字母 K 的键码
    pub const L: i32 = 40; // 字母 L 的键码
    pub const M: i32 = 41; // 字母 M 的键码
    pub const N: i32 = 42; // 字母 N 的键码
    pub const O: i32 = 43; // 字母 O 的键码
    pub const P: i32 = 44; // 字母 P 的键码
    pub const Q: i32 = 45; // 字母 Q 的键码
    pub const R: i32 = 46; // 字母 R 的键码
    pub const S: i32 = 47; // 字母 S 的键码
    pub const T: i32 = 48; // 字母 T 的键码
    pub const U: i32 = 49; // 字母 U 的键码
    pub const V: i32 = 50; // 字母 V 的键码
    pub const W: i32 = 51; // 字母 W 的键码
    pub const X: i32 = 52; // 字母 X 的键码
    pub const Y: i32 = 53; // 字母 Y 的键码
    pub const Z: i32 = 54; // 字母 Z 的键码
    // 数字 0-9: 7..16
    pub const N0: i32 = 7; // 数字键 0 的键码
    pub const N1: i32 = 8; // 数字键 1 的键码
    pub const N2: i32 = 9; // 数字键 2 的键码
    pub const N3: i32 = 10; // 数字键 3 的键码
    pub const N4: i32 = 11; // 数字键 4 的键码
    pub const N5: i32 = 12; // 数字键 5 的键码
    pub const N6: i32 = 13; // 数字键 6 的键码
    pub const N7: i32 = 14; // 数字键 7 的键码
    pub const N8: i32 = 15; // 数字键 8 的键码
    pub const N9: i32 = 16; // 数字键 9 的键码
    // 功能键 F1-F12: 131..142
    pub const F1: i32 = 131; // 功能键 F1 的键码
    pub const F2: i32 = 132; // 功能键 F2 的键码
    pub const F3: i32 = 133; // 功能键 F3 的键码
    pub const F4: i32 = 134; // 功能键 F4 的键码
    pub const F5: i32 = 135; // 功能键 F5 的键码
    pub const F6: i32 = 136; // 功能键 F6 的键码
    pub const F7: i32 = 137; // 功能键 F7 的键码
    pub const F8: i32 = 138; // 功能键 F8 的键码
    pub const F9: i32 = 139; // 功能键 F9 的键码
    pub const F10: i32 = 140; // 功能键 F10 的键码
    pub const F11: i32 = 141; // 功能键 F11 的键码
    pub const F12: i32 = 142; // 功能键 F12 的键码
    // 修饰键
    pub const SHIFT_LEFT: i32 = 59; // 左 Shift 键码
    pub const SHIFT_RIGHT: i32 = 60; // 右 Shift 键码
    pub const CTRL_LEFT: i32 = 113; // 左 Ctrl 键码
    pub const CTRL_RIGHT: i32 = 114; // 右 Ctrl 键码
    pub const ALT_LEFT: i32 = 57; // 左 Alt 键码
    pub const ALT_RIGHT: i32 = 58; // 右 Alt 键码
    // 特殊键
    pub const SPACE: i32 = 62; // 空格键码
    pub const ENTER: i32 = 66; // 回车键码
    pub const TAB: i32 = 61; // Tab 键码
    pub const ESCAPE: i32 = 111; // Esc 键码
    pub const BACK: i32 = 4; // 返回键（退格）键码
    pub const DEL: i32 = 67; // 删除键码
    pub const INSERT: i32 = 124; // 插入键码
    pub const HOME: i32 = 123; // Home 键码
    pub const PAGE_UP: i32 = 92; // 向上翻页键码
    pub const PAGE_DOWN: i32 = 93; // 向下翻页键码
    pub const MOVE_END: i32 = 122; // End 键码
    // 方向键（键盘方向键码，与手柄 DPad 无关）
    pub const DPAD_UP: i32 = 19; // 键盘上方向键码
    pub const DPAD_DOWN: i32 = 20; // 键盘下方向键码
    pub const DPAD_LEFT: i32 = 21; // 键盘左方向键码
    pub const DPAD_RIGHT: i32 = 22; // 键盘右方向键码
    // 符号键
    pub const MINUS: i32 = 69; // 减号 - 键码
    pub const EQUALS: i32 = 70; // 等号 = 键码
    pub const LEFT_BRACKET: i32 = 71; // 左方括号 [ 键码
    pub const RIGHT_BRACKET: i32 = 72; // 右方括号 ] 键码
    pub const BACKSLASH: i32 = 73; // 反斜杠 \ 键码
    pub const SEMICOLON: i32 = 74; // 分号 ; 键码
    pub const APOSTROPHE: i32 = 75; // 单引号 ' 键码
    pub const COMMA: i32 = 55; // 逗号 , 键码
    pub const PERIOD: i32 = 56; // 句点 . 键码
    pub const SLASH: i32 = 76; // 斜杠 / 键码
    pub const GRAVE: i32 = 68; // 反引号 ` 键码
    // 锁键
    pub const CAPS_LOCK: i32 = 115; // 大写锁定键码
    pub const NUM_LOCK: i32 = 143; // 数字锁定键码
    pub const SCROLL_LOCK: i32 = 116; // 滚动锁定键码
    // 小键盘 0-9: 144..153
    pub const NUMPAD_0: i32 = 144; // 小键盘 0 键码
    pub const NUMPAD_1: i32 = 145; // 小键盘 1 键码
    pub const NUMPAD_2: i32 = 146; // 小键盘 2 键码
    pub const NUMPAD_3: i32 = 147; // 小键盘 3 键码
    pub const NUMPAD_4: i32 = 148; // 小键盘 4 键码
    pub const NUMPAD_5: i32 = 149; // 小键盘 5 键码
    pub const NUMPAD_6: i32 = 150; // 小键盘 6 键码
    pub const NUMPAD_7: i32 = 151; // 小键盘 7 键码
    pub const NUMPAD_8: i32 = 152; // 小键盘 8 键码
    pub const NUMPAD_9: i32 = 153; // 小键盘 9 键码
} // 模块 android_key 定义结束

// ---------------------------------------------------------------------
// ControllerButton —— 手柄物理按键的统一枚举
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(...)]：自动派生 Debug（调试输出）、Clone（克隆）、Copy（按位复制）、
// PartialEq（== 比较）、Eq（全等）、Hash（可哈希）六个 trait
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
// 【Rust 语法】pub enum：公开枚举，各变体为无数据字段的"单元变体"，代表手柄物理按键
pub enum ControllerButton {
    A, // 按键 A
    B, // 按键 B
    X, // 按键 X
    Y, // 按键 Y
    LeftShoulder,        // LB
    RightShoulder,       // RB
    LeftTriggerClick,    // LT
    RightTriggerClick,   // RT
    LeftStickClick,      // L3
    RightStickClick,     // R3
    Menu,                // START
    Options,             // BACK
    Guide, // Xbox 中央 Home 键
    DpadUp, // 十字键上
    DpadDown, // 十字键下
    DpadLeft, // 十字键左
    DpadRight, // 十字键右
    TouchpadClick, // 触控板点击
} // 枚举 ControllerButton 定义结束

// ---------------------------------------------------------------------
// TriggerId —— 映射触发源（手柄按键 / 物理键盘规则）
// 映射器的按下/松开状态表统一以本枚举为 key，两种输入源共用同一套
// 注入与释放逻辑。
// ---------------------------------------------------------------------
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerId {
    Pad(ControllerButton), // 手柄按键触发
    Kb(u32),               // 物理键盘规则触发（值为规则 id）
}

// ---------------------------------------------------------------------
// ControllerStick —— 摇杆
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(...)]：为摇杆枚举派生 Debug / Clone / Copy / PartialEq / Eq / Hash
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControllerStick {
    LeftStick, // 左摇杆
    RightStick, // 右摇杆
    DpadAsStick, // 十字键模拟为摇杆
} // 枚举 ControllerStick 定义结束

// 鼠标按键（名称用大写，与安卓版枚举名一致，保证配置文件兼容）
// 【Rust 语法】#[derive(...)]：为鼠标按键枚举派生 Debug / Clone / Copy / PartialEq / Eq / Hash
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum MouseButton {
    Left, // 鼠标左键
    Right, // 鼠标右键
    Middle, // 鼠标中键
    Forward, // 鼠标前进键
    Back, // 鼠标后退键
} // 枚举 MouseButton 定义结束

// ---------------------------------------------------------------------
// Vector2 —— 二维向量（摇杆输入）
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(...)]：派生 Debug / Clone / Copy（注意没有 PartialEq，浮点不便比较）
#[derive(Debug, Clone, Copy)]
// 【Rust 语法】pub struct：公开结构体，含两个 pub 浮点字段 x、y
pub struct Vector2 {
    pub x: f32, // 横坐标（摇杆 x 轴）
    pub y: f32, // 纵坐标（摇杆 y 轴）
} // 结构体 Vector2 定义结束

// 【Rust 语法】impl 块：为 Vector2 实现方法
impl Vector2 {
    // 【Rust 语法】pub fn new 关联函数：不接收 self 的构造函数；-> Self 返回 Vector2 本身
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y } // 【Rust 语法】字段简写初始化：x, y 等价于 x: x, y: y
    } // 构造函数 new 结束

    // 【Rust 语法】&self：不可变借用自身（只读方法）；返回向量的模长
    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt() // 勾股定理求模长：sqrt(x²+y²)，sqrt 是 f32 的方法
    }

    // 归一化：长度不足 1e-6 视为零向量
    pub fn normalized(&self) -> Self {
        let m = self.magnitude(); // 先求当前模长
        if m < 1e-6 { // 模长过小（接近零向量）时避免除零
            return Self { x: 0.0, y: 0.0 }; // 返回零向量
        } // 结束"模长过小"分支
        Self { // 否则把每个分量除以模长，得到单位向量
            x: self.x / m, // x 分量归一化
            y: self.y / m, // y 分量归一化
        } // 结构体字面量结束
    } // 方法 normalized 结束

    // 死区缩放：(mag - deadzone) / (1 - deadzone)
    pub fn with_deadzone(&self, deadzone: f32) -> Self {
        let m = self.magnitude(); // 求当前模长
        if m <= deadzone { // 模长不超过死区阈值则视为无输入
            return Self { x: 0.0, y: 0.0 }; // 返回零向量
        } // 结束"死区内"分支
        let scale = (m - deadzone) / (1.0 - deadzone); // 计算缩放比例：把 [deadzone,1] 线性映射到 [0,1]
        Self {
            x: self.x * scale, // x 分量按比例缩放
            y: self.y * scale, // y 分量按比例缩放
        } // 结构体字面量结束
    } // 方法 with_deadzone 结束
} // impl Vector2 结束

// ---------------------------------------------------------------------
// 名称映射：配置持久化名 / 界面显示名
// ---------------------------------------------------------------------

/// 手柄按钮 -> 序列化名（大写英文，写入配置文件，与安卓版一致）
// 【Rust 语法】&'static str：返回字符串字面量引用（生命周期为 'static，即整个程序运行期间都有效）
pub fn controller_button_name(b: ControllerButton) -> &'static str {
    // 【Rust 语法】match 表达式：按值匹配枚举的每个变体；匹配分支的值即整个表达式的返回值
    match b {
        ControllerButton::A => "A", // A 键 -> 序列化名 "A"
        ControllerButton::B => "B", // B 键 -> "B"
        ControllerButton::X => "X", // X 键 -> "X"
        ControllerButton::Y => "Y", // Y 键 -> "Y"
        ControllerButton::LeftShoulder => "LB", // 左肩键 -> "LB"
        ControllerButton::RightShoulder => "RB", // 右肩键 -> "RB"
        ControllerButton::LeftTriggerClick => "LT", // 左扳机 -> "LT"
        ControllerButton::RightTriggerClick => "RT", // 右扳机 -> "RT"
        ControllerButton::LeftStickClick => "L3", // 左摇杆按下 -> "L3"
        ControllerButton::RightStickClick => "R3", // 右摇杆按下 -> "R3"
        ControllerButton::Menu => "MENU", // 菜单键 -> "MENU"
        ControllerButton::Options => "OPTIONS", // 视图键 -> "OPTIONS"
        ControllerButton::Guide => "GUIDE", // Home 键 -> "GUIDE"
        ControllerButton::DpadUp => "DPAD_UP", // 方向键上 -> "DPAD_UP"
        ControllerButton::DpadDown => "DPAD_DOWN", // 方向键下 -> "DPAD_DOWN"
        ControllerButton::DpadLeft => "DPAD_LEFT", // 方向键左 -> "DPAD_LEFT"
        ControllerButton::DpadRight => "DPAD_RIGHT", // 方向键右 -> "DPAD_RIGHT"
        ControllerButton::TouchpadClick => "TOUCHPAD_CLICK", // 触控板点击 -> "TOUCHPAD_CLICK"
    } // match 结束
} // 函数 controller_button_name 结束

/// 序列化名 -> 手柄按钮；解析失败返回 None
// 【Rust 语法】返回类型 Option<ControllerButton>：Rust 的"可选值"类型，Some=解析成功 / None=未找到
pub fn controller_button_from_name(name: &str) -> Option<ControllerButton> {
    all_controller_buttons() // 获取所有按钮的列表
        .into_iter() // 【Rust 语法】into_iter：把 Vec 变成"按值"迭代器（消费所有权）
        .find(|&b| controller_button_name(b) == name) // 【Rust 语法】迭代器 find + 闭包：返回第一个满足条件的元素，封装为 Option
} // 函数 controller_button_from_name 结束

/// 手柄按钮 -> 中文显示名（仅 UI 使用）
pub fn controller_button_display_name(b: ControllerButton) -> &'static str {
    match b { // 按按钮枚举分支，返回中文显示名
        ControllerButton::A => "A键", // A 键显示名
        ControllerButton::B => "B键", // B 键显示名
        ControllerButton::X => "X键", // X 键显示名
        ControllerButton::Y => "Y键", // Y 键显示名
        ControllerButton::LeftShoulder => "LB肩键", // 左肩键显示名
        ControllerButton::RightShoulder => "RB肩键", // 右肩键显示名
        ControllerButton::LeftTriggerClick => "LT扳机", // 左扳机显示名
        ControllerButton::RightTriggerClick => "RT扳机", // 右扳机显示名
        ControllerButton::LeftStickClick => "L3摇杆按下", // 左摇杆按下显示名
        ControllerButton::RightStickClick => "R3摇杆按下", // 右摇杆按下显示名
        ControllerButton::Menu => "菜单键", // 菜单键显示名
        ControllerButton::Options => "视图键", // 视图键显示名
        ControllerButton::Guide => "Home键", // Home 键显示名
        ControllerButton::DpadUp => "方向键上", // 方向键上显示名
        ControllerButton::DpadDown => "方向键下", // 方向键下显示名
        ControllerButton::DpadLeft => "方向键左", // 方向键左显示名
        ControllerButton::DpadRight => "方向键右", // 方向键右显示名
        ControllerButton::TouchpadClick => "触控板点击", // 触控板点击显示名
    } // match 结束
} // 函数 controller_button_display_name 结束

/// 所有可映射的手柄按钮（固定显示顺序）
// 【Rust 语法】返回类型 Vec<ControllerButton>：存放按钮的动态数组
pub fn all_controller_buttons() -> Vec<ControllerButton> {
    vec![ // 【Rust 语法】vec! 宏：创建并填充 Vec（动态数组）
        ControllerButton::A, // 添加 A 键
        ControllerButton::B, // 添加 B 键
        ControllerButton::X, // 添加 X 键
        ControllerButton::Y, // 添加 Y 键
        ControllerButton::LeftShoulder, // 添加左肩键
        ControllerButton::RightShoulder, // 添加右肩键
        ControllerButton::LeftTriggerClick, // 添加左扳机
        ControllerButton::RightTriggerClick, // 添加右扳机
        ControllerButton::LeftStickClick, // 添加左摇杆按下
        ControllerButton::RightStickClick, // 添加右摇杆按下
        ControllerButton::Menu, // 添加菜单键
        ControllerButton::Options, // 添加视图键
        ControllerButton::DpadUp, // 添加方向键上
        ControllerButton::DpadDown, // 添加方向键下
        ControllerButton::DpadLeft, // 添加方向键左
        ControllerButton::DpadRight, // 添加方向键右
        ControllerButton::Guide, // 添加 Home 键
        ControllerButton::TouchpadClick, // 添加触控板点击
    ] // vec! 宏填充结束
} // 函数 all_controller_buttons 结束

/// 鼠标键 -> 序列化名（大写，与安卓版一致）
pub fn mouse_button_name(b: MouseButton) -> &'static str {
    match b { // 按鼠标键枚举分支，返回大写序列化名
        MouseButton::Left => "LEFT", // 左键 -> "LEFT"
        MouseButton::Right => "RIGHT", // 右键 -> "RIGHT"
        MouseButton::Middle => "MIDDLE", // 中键 -> "MIDDLE"
        MouseButton::Forward => "FORWARD", // 前进键 -> "FORWARD"
        MouseButton::Back => "BACK", // 后退键 -> "BACK"
    } // match 结束
} // 函数 mouse_button_name 结束

/// 鼠标键名 -> 枚举（兼容大小写）
// 【Rust 语法】&str 与 String：参数为字符串引用 &str；Option<MouseButton> 作为返回值
pub fn mouse_button_from_name(name: &str) -> Option<MouseButton> {
    let upper = name.to_ascii_uppercase(); // 把字符串转成大写 ASCII 形式，实现大小写不敏感匹配
    match upper.as_str() { // 【Rust 语法】match 匹配字符串：as_str() 把 String 转成 &str 引用以便分支比较
        "LEFT" => Some(MouseButton::Left), // "LEFT" -> 左键（Some 包裹）
        "RIGHT" => Some(MouseButton::Right), // "RIGHT" -> 右键
        "MIDDLE" => Some(MouseButton::Middle), // "MIDDLE" -> 中键
        "FORWARD" => Some(MouseButton::Forward), // "FORWARD" -> 前进键
        "BACK" => Some(MouseButton::Back), // "BACK" -> 后退键
        _ => None, // 【Rust 语法】_ 通配符分支：匹配以上都不满足的情况，返回 None（解析失败）
    } // match 结束
} // 函数 mouse_button_from_name 结束

/// 鼠标键 -> 中文显示名
pub fn mouse_button_display_name(b: MouseButton) -> &'static str {
    match b { // 按鼠标键枚举分支，返回中文显示名
        MouseButton::Left => "鼠标左键", // 左键显示名
        MouseButton::Right => "鼠标右键", // 右键显示名
        MouseButton::Middle => "鼠标中键", // 中键显示名
        MouseButton::Forward => "鼠标前进键", // 前进键显示名
        MouseButton::Back => "鼠标后退键", // 后退键显示名
    } // match 结束
} // 函数 mouse_button_display_name 结束

/// Android KeyCode -> 可读名称（与安卓版 keyCodeToName 一致）
// 【Rust 语法】String 是堆分配的动态字符串；函数把按键码转成可读名称字符串
pub fn key_code_to_name(key_code: i32) -> String {
    // 字母 A-Z：29..54
    // 【Rust 语法】(29..=54) 是含右端点的范围类型；contains(&key_code) 判断数值是否在区间内
    if (29..=54).contains(&key_code) {
        // 【Rust 语法】b'A' 是字节字面量（u8）；(key_code - 29) as u8 转成字节；+ 算出字符的 ASCII 码；as char 转字符；to_string() 转字符串
        return ((b'A' + (key_code - 29) as u8) as char).to_string();
    } // 结束字母区间判断
    // 数字 0-9：7..16
    if (7..=16).contains(&key_code) {
        return ((b'0' + (key_code - 7) as u8) as char).to_string(); // 由键码偏移算出数字字符并转字符串
    } // 结束数字区间判断
    // F1-F12：131..142
    if (131..=142).contains(&key_code) {
        return format!("F{}", key_code - 131 + 1); // 【Rust 语法】format! 宏：格式化生成字符串 "F" + 序号
    } // 结束 F 键区间判断
    // 小键盘 0-9：144..153
    if (144..=153).contains(&key_code) {
        return format!("Num{}", key_code - 144); // 格式化生成 "Num0"~"Num9"
    } // 结束小键盘区间判断
    match key_code { // 其余键码用 match 逐一映射为名称
        android_key::SPACE => "Space".to_string(), // 空格键 -> "Space"
        android_key::ENTER => "Enter".to_string(), // 回车 -> "Enter"
        android_key::TAB => "Tab".to_string(), // Tab -> "Tab"
        android_key::ESCAPE => "Esc".to_string(), // Esc -> "Esc"
        android_key::BACK => "Back".to_string(), // 返回键 -> "Back"
        android_key::DEL => "Backspace".to_string(), // 删除 -> "Backspace"
        android_key::INSERT => "Insert".to_string(), // 插入 -> "Insert"
        android_key::HOME => "Home".to_string(), // Home -> "Home"
        android_key::PAGE_UP => "PageUp".to_string(), // 上翻页 -> "PageUp"
        android_key::PAGE_DOWN => "PageDown".to_string(), // 下翻页 -> "PageDown"
        android_key::MOVE_END => "End".to_string(), // End -> "End"
        // 【Rust 语法】| 在 match 模式中表示"或"：两个键码映射到同一名称
        android_key::SHIFT_LEFT | android_key::SHIFT_RIGHT => "Shift".to_string(), // 左右 Shift 统一为 "Shift"
        android_key::CTRL_LEFT | android_key::CTRL_RIGHT => "Ctrl".to_string(), // 左右 Ctrl 统一为 "Ctrl"
        android_key::ALT_LEFT | android_key::ALT_RIGHT => "Alt".to_string(), // 左右 Alt 统一为 "Alt"
        android_key::DPAD_UP => "↑".to_string(), // 上方向键 -> "↑"
        android_key::DPAD_DOWN => "↓".to_string(), // 下方向键 -> "↓"
        android_key::DPAD_LEFT => "←".to_string(), // 左方向键 -> "←"
        android_key::DPAD_RIGHT => "→".to_string(), // 右方向键 -> "→"
        android_key::MINUS => "-".to_string(), // 减号 -> "-"
        android_key::EQUALS => "=".to_string(), // 等号 -> "="
        android_key::LEFT_BRACKET => "[".to_string(), // 左方括号 -> "["
        android_key::RIGHT_BRACKET => "]".to_string(), // 右方括号 -> "]"
        android_key::BACKSLASH => "\\".to_string(), // 反斜杠 -> "\"（字符串中转义为 \\）
        android_key::SEMICOLON => ";".to_string(), // 分号 -> ";"
        android_key::APOSTROPHE => "'".to_string(), // 单引号 -> "'"
        android_key::COMMA => ",".to_string(), // 逗号 -> ","
        android_key::PERIOD => ".".to_string(), // 句点 -> "."
        android_key::SLASH => "/".to_string(), // 斜杠 -> "/"
        android_key::GRAVE => "`".to_string(), // 反引号 -> "`"
        android_key::CAPS_LOCK => "CapsLock".to_string(), // 大写锁定 -> "CapsLock"
        android_key::NUM_LOCK => "NumLock".to_string(), // 数字锁定 -> "NumLock"
        android_key::SCROLL_LOCK => "ScrollLock".to_string(), // 滚动锁定 -> "ScrollLock"
        _ => format!("Key({})", key_code), // 【Rust 语法】_ 兜底分支：未定义的键码显示为 "Key(键码)"
    } // match 结束
} // 函数 key_code_to_name 结束

/// 层名 -> 带预设中文别名的展示名（WoW 预设）
pub fn layer_display_name(layer_name: &str) -> String {
    let alias = match layer_name { // 【Rust 语法】match 匹配 &str；把匹配到的中文别名赋给变量 alias
        "Layer1" => "战斗", // 层 1 别名"战斗"
        "Layer2" => "骑乘", // 层 2 别名"骑乘"
        "Layer3" => "瞄准", // 层 3 别名"瞄准"
        "Layer4" => "拾取", // 层 4 别名"拾取"
        "Layer5" => "潜行", // 层 5 别名"潜行"
        "Layer6" => "钓鱼", // 层 6 别名"钓鱼"
        "Layer7" => "对战", // 层 7 别名"对战"
        "Layer8" => "团本", // 层 8 别名"团本"
        "Layer9" => "旅行", // 层 9 别名"旅行"
        "Layer10" => "自定义", // 层 10 别名"自定义"
        _ => return layer_name.to_string(), // 未匹配到预设别名时直接返回原始层名
    }; // match 结束（结果赋值给变量 alias）
    format!("{} {}", layer_name, alias) // 把原始层名与中文别名拼接成 "Layer1 战斗" 这样的展示名
} // 函数 layer_display_name 结束
