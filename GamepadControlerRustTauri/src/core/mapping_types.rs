// =====================================================================
// mapping_types.rs —— 映射数据模型
//
// 等效 Qt 版 MappingTypes.h（MappedAction / KeyMapping / OperationLayer /
// GlobalSettings / OperationSet / ControllerProfile）。
//
// 三层结构总览（从高到低）：
//   操作集（OperationSet）—— 最顶层容器，由 1 个公共层 + 最多 10 个
//     操作层组成；切换操作集时，其下所有操作层整体切换。
//   公共层（common_layer）—— 始终激活，优先度最低，作为兜底映射；
//   操作层（layers）—— 通过公共层的 SwitchLayer 映射按住激活、松开回退。
//  按键查询顺序（get_effective_mapping）：最后激活的操作层 -> ... -> 公共层。
// =====================================================================

// 【Rust 语法】use 导入语句：`crate::` 表示当前 crate 的根模块；`*` 为通配符，把 input_types 模块中所有公开（pub）符号引入当前作用域，方便直接使用（如 MouseButton、android_key）。
use crate::core::input_types::*;
// 【Rust 语法】use 导入语句：从标准库 std::collections 模块导入 HashMap（哈希映射/字典容器）。
use std::collections::HashMap;

// 【Rust 语法】const 定义编译期常量：值在编译期确定、不可被修改；`pub` 表示对外公开；类型 `usize` 为无符号整数（位数随平台 32/64 位而变化）。
/// 单个操作集内最多操作层数
pub const K_MAX_LAYERS_PER_SET: usize = 10;

// ---------------------------------------------------------------------
// MappedAction —— 映射动作
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(...)] 属性宏（派生宏）：让编译器自动为下方类型实现所列 trait（特性）——
// Debug 调试格式化输出、Clone 可克隆、Copy 可逐位复制（低成本拷贝）、PartialEq 可判相等、Eq 完全等价。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// 【Rust 语法】enum 枚举类型：定义一组离散的取值（称为"变体"）；此处每个变体都不携带附加数据（单元变体）。
pub enum ActionType {
    KeyboardKey, // 键盘按键动作
    MouseClick, // 鼠标单击动作
    SwitchLayer, // 切换操作层动作
    MouseMove, // 鼠标移动动作
    LookAround, // 视角控制动作（右摇杆）
    MouseToggle, // 鼠标长按锁存动作
    WheelUp, // 滚轮上滚
    WheelDown, // 滚轮下滚
    ToggleMapping, // 切换映射总开关
    ToggleOnScreenKeyboard, // 切换屏幕键盘
    ToggleOverlay, // 切换悬浮窗
    InteractionKey, // 交互键（临时解锁鼠标锁存 -> 屏幕中心左右键点击 -> 恢复）
    ReleaseMouseToggle, // 释放鼠标左右键长按锁存
    SwapMouseToggle, // 交换鼠标左右键长按锁存
    FineLook, // 鼠标精细移动（右摇杆低灵敏度，按住生效）
} // 结束 ActionType 枚举定义

// MappedAction 含 Option<String>（layer_name），不能 Copy，只 Clone。
// 【Rust 语法】#[derive(...)] 派生宏：实现 Debug / Clone / PartialEq / Eq（因字段含 String，无法实现 Copy）。
#[derive(Debug, Clone, PartialEq, Eq)]
// 【Rust 语法】struct 结构体：把多个命名字段组合成一种新类型；字段前加 `pub` 表示对外可见可读写。
pub struct MappedAction {
    // 【Rust 语法】原始标识符 r#type：`type` 是 Rust 的保留关键字，用 `r#` 前缀才能把它当作普通字段名使用。
    pub r#type: ActionType, // 动作类型（ActionType 枚举的一个变体）
    pub key_code: i32, // 键盘键码（存储 Android KeyCode 常量，运行时再转换为 Windows 虚拟键码）
    pub mouse_button: MouseButton, // 鼠标键（左/右/中键），非鼠标动作时取默认值
    pub layer_name: Option<String>, // 【Rust 语法】Option<String>：枚举，要么是 None（无值）要么是 Some(字符串)；此处仅切换层动作使用，存目标层名
} // 结束 MappedAction 结构体定义

// 【Rust 语法】impl 块：为 MappedAction 实现方法（函数）；块内 `Self` 代指该类型本身。
impl MappedAction {
    // 【Rust 语法】关联函数（无 self 参数）：类似"静态构造器"，用 `MappedAction::keyboard_key(...)` 调用。
    pub fn keyboard_key(code: i32) -> Self { // 构造"键盘按键"动作，参数 code 为要按下的键码
        // 【Rust 语法】结构体字面量：按字段名给定值，创建并返回一个 MappedAction 实例（作为函数最后表达式即为返回值）。
        Self {
            r#type: ActionType::KeyboardKey, // 动作类型设为键盘按键
            key_code: code, // 键码使用传入的 code
            mouse_button: MouseButton::Left, // 鼠标键字段填默认值 Left（本动作不使用）
            layer_name: None, // 不涉及层切换，填 None
        } // 结束 Self 结构体字面量
    } // 结束 keyboard_key 函数
    pub fn mouse_click(b: MouseButton) -> Self { // 构造"鼠标单击"动作，参数 b 为要点击的鼠标键
        Self { // 构造 MouseClick 动作实例
            r#type: ActionType::MouseClick, // 动作类型设为鼠标单击
            key_code: 0, // 不使用键盘键码，填 0
            mouse_button: b, // 鼠标键使用传入的 b
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 mouse_click 函数
    pub fn mouse_toggle(b: MouseButton) -> Self { // 构造"鼠标长按锁存"动作，参数 b 为要锁存的鼠标键
        Self { // 构造 MouseToggle 动作实例
            r#type: ActionType::MouseToggle, // 动作类型设为鼠标长按锁存
            key_code: 0, // 不使用键盘键码
            mouse_button: b, // 鼠标键使用传入的 b
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 mouse_toggle 函数
    pub fn wheel_up() -> Self { // 构造"滚轮上滚"动作
        Self { // 构造 WheelUp 动作实例
            r#type: ActionType::WheelUp, // 动作类型设为滚轮上滚
            key_code: 0, // 不使用键盘键码
            mouse_button: MouseButton::Left, // 鼠标键填默认值 Left
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 wheel_up 函数
    pub fn wheel_down() -> Self { // 构造"滚轮下滚"动作
        Self { // 构造 WheelDown 动作实例
            r#type: ActionType::WheelDown, // 动作类型设为滚轮下滚
            key_code: 0, // 不使用键盘键码
            mouse_button: MouseButton::Left, // 鼠标键填默认值 Left
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 wheel_down 函数
    pub fn switch_layer(name: &str) -> Self { // 构造"切换操作层"动作，参数 name 为目标层名
        Self { // 构造 SwitchLayer 动作实例
            r#type: ActionType::SwitchLayer, // 动作类型设为切换操作层
            key_code: 0, // 不使用键盘键码
            mouse_button: MouseButton::Left, // 鼠标键填默认值 Left
            layer_name: Some(name.to_string()), // 【Rust 语法】Some(...) 将值包装成 Option 的"有值"形态；to_string() 把 &str 借用字符串复制为拥有的 String
        } // 结束 Self 结构体字面量
    } // 结束 switch_layer 函数
    pub fn mouse_move() -> Self { // 构造"鼠标移动"动作
        Self { // 构造 MouseMove 动作实例
            r#type: ActionType::MouseMove, // 动作类型设为鼠标移动
            key_code: 0, // 不使用键盘键码
            mouse_button: MouseButton::Left, // 鼠标键填默认值 Left
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 mouse_move 函数
    pub fn look_around() -> Self { // 构造"视角控制"动作
        Self { // 构造 LookAround 动作实例
            r#type: ActionType::LookAround, // 动作类型设为视角控制
            key_code: 0, // 不使用键盘键码
            mouse_button: MouseButton::Left, // 鼠标键填默认值 Left
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 look_around 函数
    pub fn toggle_mapping() -> Self { // 构造"切换映射总开关"动作
        Self { // 构造 ToggleMapping 动作实例
            r#type: ActionType::ToggleMapping, // 动作类型设为切换映射
            key_code: 0, // 不使用键盘键码
            mouse_button: MouseButton::Left, // 鼠标键填默认值 Left
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 toggle_mapping 函数
    pub fn toggle_on_screen_keyboard() -> Self { // 构造"切换屏幕键盘"动作
        Self { // 构造 ToggleOnScreenKeyboard 动作实例
            r#type: ActionType::ToggleOnScreenKeyboard, // 动作类型设为切换屏幕键盘
            key_code: 0, // 不使用键盘键码
            mouse_button: MouseButton::Left, // 鼠标键填默认值 Left
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 toggle_on_screen_keyboard 函数
    pub fn toggle_overlay() -> Self { // 构造"切换悬浮窗"动作
        Self { // 构造 ToggleOverlay 动作实例
            r#type: ActionType::ToggleOverlay, // 动作类型设为切换悬浮窗
            key_code: 0, // 不使用键盘键码
            mouse_button: MouseButton::Left, // 鼠标键填默认值 Left
            layer_name: None, // 无层切换
        } // 结束 Self 结构体字面量
    } // 结束 toggle_overlay 函数
    pub fn interaction_key() -> Self { // 构造"交互键"动作
        Self {
            r#type: ActionType::InteractionKey,
            key_code: 0,
            mouse_button: MouseButton::Left,
            layer_name: None,
        }
    }
    pub fn release_mouse_toggle() -> Self { // 构造"释放鼠标锁存"动作
        Self {
            r#type: ActionType::ReleaseMouseToggle,
            key_code: 0,
            mouse_button: MouseButton::Left,
            layer_name: None,
        }
    }
    pub fn swap_mouse_toggle() -> Self { // 构造"交换鼠标左右键锁存"动作
        Self {
            r#type: ActionType::SwapMouseToggle,
            key_code: 0,
            mouse_button: MouseButton::Left,
            layer_name: None,
        }
    }
    pub fn fine_look() -> Self { // 构造"鼠标精细移动"动作
        Self {
            r#type: ActionType::FineLook,
            key_code: 0,
            mouse_button: MouseButton::Left,
            layer_name: None,
        }
    }
} // 结束 impl MappedAction 块

// ---------------------------------------------------------------------
// KeyMapping —— 单个按键映射
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(...)] 派生宏：实现 Debug / Clone / PartialEq / Eq（字段均为可 Clone 类型，故可自动派生）。
#[derive(Debug, Clone, PartialEq, Eq)]
// 【Rust 语法】struct 结构体：单个按键的完整映射定义（主动作 + 若干附加子命令键）。
pub struct KeyMapping {
    pub action: MappedAction, // 主要动作（按下立即执行）
    // 【Rust 语法】Vec<i32> 为动态可变长度数组（向量）；此处存放组合键的附加键码。
    pub sub_commands: Vec<i32>, // Android KeyCode，最多 MAX_SUB_COMMANDS 个
    /// 长按动作：按住超过 long_press_ms 后替换主动作（None 表示不支持长按）
    pub long_press: Option<MappedAction>,
    /// 长按动作的子命令
    pub long_press_subs: Vec<i32>,
    /// 长按判定阈值（毫秒），默认 500
    pub long_press_ms: u32,
} // 结束 KeyMapping 结构体定义

impl Default for KeyMapping {
    fn default() -> Self {
        Self {
            action: MappedAction::mouse_move(),
            sub_commands: Vec::new(),
            long_press: None,
            long_press_subs: Vec::new(),
            long_press_ms: KeyMapping::DEFAULT_LONG_PRESS_MS,
        }
    }
}

// 【Rust 语法】impl 块：为 KeyMapping 实现方法。
impl KeyMapping {
    // 【Rust 语法】关联常量：与类型绑定、不属于某个实例的常量，通过 `KeyMapping::MAX_SUB_COMMANDS` 访问。
    pub const MAX_SUB_COMMANDS: usize = 3;
    /// 长按判定默认阈值（毫秒）
    pub const DEFAULT_LONG_PRESS_MS: u32 = 500;

    /// 构造一条只有主动作的映射
    pub fn new(action: MappedAction) -> Self {
        Self {
            action,
            sub_commands: Vec::new(),
            long_press: None,
            long_press_subs: Vec::new(),
            long_press_ms: Self::DEFAULT_LONG_PRESS_MS,
        }
    }

    /// 生成可读描述字符串，如 "W+ALT"
    // 【Rust 语法】方法定义：`&self` 表示对自身实例的不可变借用（只读访问，不转移所有权）。
    pub fn describe(&self) -> String {
        // 【Rust 语法】let mut 绑定：`mut` 声明该变量可变（Rust 变量默认不可变）；`Vec::new()` 创建空动态数组，类型由上下文推断为 Vec<String>。
        let mut parts: Vec<String> = Vec::new();
        // 【Rust 语法】match 表达式：按 `self.action.r#type` 的值做模式匹配，命中哪个分支就执行哪个分支的右侧表达式。
        match self.action.r#type {
            ActionType::KeyboardKey => parts.push(key_code_to_name(self.action.key_code)), // 键盘按键：追加键名描述
            ActionType::MouseClick => parts.push(mouse_button_display_name(self.action.mouse_button).to_string()), // 鼠标单击：追加鼠标键名
            ActionType::MouseToggle => parts.push(format!("长按{}", mouse_button_display_name(self.action.mouse_button))), // 鼠标长按锁存：追加"长按键名"描述
            ActionType::WheelUp => parts.push("滚轮上滚".to_string()), // 滚轮上滚描述
            ActionType::WheelDown => parts.push("滚轮下滚".to_string()), // 滚轮下滚描述
            ActionType::SwitchLayer => parts.push(format!("切换→{}", self.action.layer_name.as_deref().unwrap_or(""))), // 切换层：追加"切换→层名"（层名为空时显示空串）
            ActionType::MouseMove => parts.push("鼠标移动".to_string()), // 鼠标移动描述
            ActionType::LookAround => parts.push("视角控制".to_string()), // 视角控制描述
            ActionType::ToggleMapping => parts.push("切换映射".to_string()), // 切换映射描述
            ActionType::ToggleOnScreenKeyboard => parts.push("切换屏幕键盘".to_string()), // 切换屏幕键盘描述
            ActionType::ToggleOverlay => parts.push("切换悬浮窗".to_string()), // 切换悬浮窗描述
            ActionType::InteractionKey => parts.push("交互键".to_string()), // 交互键描述
            ActionType::ReleaseMouseToggle => parts.push("释放鼠标锁存".to_string()), // 释放锁存描述
            ActionType::SwapMouseToggle => parts.push("交换鼠标左右键锁存".to_string()), // 交换锁存描述
            ActionType::FineLook => parts.push("鼠标精细移动".to_string()), // 精细移动描述
        } // 结束 match 表达式
        // 【Rust 语法】for ... in 循环：`&self.sub_commands` 是对数组的不可变借用（不转移所有权），每次迭代 `sub` 绑定为元素引用 &i32。
        for sub in &self.sub_commands {
            // 【Rust 语法】解引用 `*sub`：从引用中取出值，把 &i32 解引用为 i32 再传给函数。
            parts.push(key_code_to_name(*sub));
        } // 结束 for 循环
        parts.join("+") // 【Rust 语法】最后一个表达式即函数返回值；join 方法用 "+" 连接字符串数组并返回 String
    } // 结束 describe 函数
} // 结束 impl KeyMapping 块

// ---------------------------------------------------------------------
// OperationLayer —— 操作层（一组按键映射）
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(Debug, Clone)]：实现调试输出与克隆（字段 HashMap 也支持 Clone，故可派生）。
#[derive(Debug, Clone)]
// 【Rust 语法】struct 结构体：一个操作层，承载一组"手柄按钮 -> 按键映射"的对应关系。
pub struct OperationLayer {
    /// 唯一标识符，固定不变（如 "Layer1"、"Common"），运行时逻辑只认 id
    pub id: String, // 【Rust 语法】String 为拥有所有权的 UTF-8 字符串（可增长、可修改）
    /// 显示名称，用户可修改
    pub name: String, // 层的显示名称（用于 UI 展示）
    pub has_trigger_button: bool, // 是否配置了触发按键（仅 UI 展示用）
    pub trigger_button: ControllerButton, // 触发按键（手柄按钮）
    /// 按钮 -> 映射
    pub button_mappings: HashMap<ControllerButton, KeyMapping>, // 【Rust 语法】HashMap<K,V> 哈希表：键为手柄按钮，值为对应的按键映射
} // 结束 OperationLayer 结构体定义

// 【Rust 语法】impl 块：为 OperationLayer 实现方法。
impl OperationLayer {
    // 【Rust 语法】关联函数（无 self）：构造一个新层；参数 `&str` 是对字符串的借用（借用方不拥有数据）。
    pub fn new(id: &str) -> Self {
        Self { // 构造 OperationLayer 实例
            // 【Rust 语法】`to_string()` 把 &str（借用字符串）复制转换为自有 String。
            id: id.to_string(), // 层 id 设为传入的 id
            name: id.to_string(), // 初始显示名与 id 相同
            has_trigger_button: false, // 默认没有触发按键
            trigger_button: ControllerButton::A, // 触发按键给默认值 A
            button_mappings: HashMap::new(), // 初始为空映射表
        } // 结束 Self 结构体字面量
    } // 结束 new 函数

    // 【Rust 语法】方法：`&self` 不可变借用；返回值 `Option<&KeyMapping>` 表示"可能没有（None）或返回映射的借用"。
    pub fn get_mapping(&self, b: ControllerButton) -> Option<&KeyMapping> {
        self.button_mappings.get(&b) // 【Rust 语法】HashMap::get 按键查询，返回 Option<&V>；此处作为最后一个表达式直接返回
    } // 结束 get_mapping 函数
} // 结束 impl OperationLayer 块

// ---------------------------------------------------------------------
// GlobalSettings —— 全局设置
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(Debug, Clone)]：实现调试输出与克隆。
#[derive(Debug, Clone)]
// 【Rust 语法】struct 结构体：汇总程序的全局配置项。
pub struct GlobalSettings {
    pub deadzone: f32, // 摇杆死区（f32 单精度浮点，避免轻微偏移误触发）
    pub look_sensitivity: f32, // 视角灵敏度
    pub cursor_speed: f32, // 鼠标光标速度
    pub look_smoothing: f32, // 视角平滑度
    pub look_acceleration: f32, // 视角加速度
    pub invert_look_x: bool, // 视角 X 轴是否反转（bool 布尔型）
    pub invert_look_y: bool, // 视角 Y 轴是否反转
    pub overlay_x: i32, // 悬浮窗 X 坐标（i32 有符号 32 位整数）
    pub overlay_y: i32, // 悬浮窗 Y 坐标
    pub overlay_scale: f64, // 悬浮窗缩放比例（f64 双精度浮点）
    pub main_window_x: i32, // 主窗口 X 坐标
    pub main_window_y: i32, // 主窗口 Y 坐标
    pub release_on_foreground_change: bool, // 前台窗口切换时是否自动释放按键
    pub confirm_on_close: bool, // 关闭程序时是否弹出确认框
    pub overlay_opacity: f32, // 悬浮窗的明度
    /// 目标游戏（WoW）可执行文件路径：WASD 拦截后按键直接投递给该进程
    pub wow_exe: String,
    /// 历史选择过的 WoW 可执行文件路径（最近优先，数量有限）
    pub wow_exe_history: Vec<String>,
} // 结束 GlobalSettings 结构体定义

// 【Rust 语法】为 GlobalSettings 实现 Default trait（特性）：提供一套默认值，之后可用 `GlobalSettings::default()` 便捷创建实例。
impl Default for GlobalSettings {
    fn default() -> Self { // 实现 Default trait 要求的 default 方法，返回带默认值的实例
        Self { // 构造 GlobalSettings 默认实例
            deadzone: 0.15, // 死区默认 0.15
            look_sensitivity: 0.5, // 灵敏度默认 0.5
            cursor_speed: 1.0, // 光标速度默认 1.0
            look_smoothing: 0.5, // 平滑度默认 0.5
            look_acceleration: 1.5, // 加速度默认 1.5
            invert_look_x: false, // 默认不反转 X 轴
            invert_look_y: false, // 默认不反转 Y 轴
            overlay_x: -1, // 悬浮窗坐标默认 -1（表示尚未定位）
            overlay_y: -1, // 悬浮窗坐标默认 -1
            overlay_scale: 1.0, // 缩放默认 1.0（原始大小）
            main_window_x: -1, // 主窗口坐标默认 -1（未定位）
            main_window_y: -1, // 主窗口坐标默认 -1
            release_on_foreground_change: true, // 默认前台切换时释放按键
            confirm_on_close: true, // 默认关闭时需确认
            overlay_opacity: 0.8, // 悬浮窗透明度默认 0.8
            wow_exe: String::new(), // 默认未选择目标程序
            wow_exe_history: Vec::new(), // 默认无历史记录
        } // 结束 Self 结构体字面量
    } // 结束 default 函数
} // 结束 impl Default for GlobalSettings 块

// ---------------------------------------------------------------------
// OperationSet —— 操作集（最顶层容器）
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(Debug, Clone)]：实现调试输出与克隆。
#[derive(Debug, Clone)]
// 【Rust 语法】struct 结构体：操作集是映射配置的最顶层容器。
pub struct OperationSet {
    pub id: String, // 操作集唯一 id
    pub name: String, // 操作集显示名称
    pub common_layer: OperationLayer, // 公共层（始终激活、优先度最低，作为兜底映射）
    pub layers: Vec<OperationLayer>, // 【Rust 语法】Vec<OperationLayer> 动态数组：操作层列表
} // 结束 OperationSet 结构体定义

// 【Rust 语法】impl 块：为 OperationSet 实现方法。
impl OperationSet {
    /// 创建一个全新的空操作集：空公共层 + K_MAX_LAYERS_PER_SET 个空操作层
    // 【Rust 语法】关联函数（无 self）：构造器；`mut` 使局部变量可变（默认不可变）。
    pub fn create_empty(set_id: &str, set_name: &str) -> Self {
        // 【Rust 语法】let mut 绑定 + 调用关联函数 `OperationLayer::new(...)`。
        let mut common = OperationLayer::new("Common"); // 创建公共层，其 id 为 "Common"
        common.name = "Common".to_string(); // 公共层显示名也设为 "Common"
        // 【Rust 语法】数组字面量 `[...]`：固定长度数组，元素为 &str 字符串字面量（数量固定为 10）。
        let layer_ids = [ // 预置 10 个操作层的 id 列表
            "Layer1", "Layer2", "Layer3", "Layer4", "Layer5", // 前 5 个层的 id
            "Layer6", "Layer7", "Layer8", "Layer9", "Layer10", // 后 5 个层的 id
        ]; // 结束层 id 数组
        // 【Rust 语法】迭代器方法链：`iter()` 生成元素引用迭代器 -> `map(闭包)` 逐元素转换 -> `collect()` 收集为集合。
        let layers = layer_ids
            // 【Rust 语法】iter()：对数组取引用迭代器，产生 &str 引用。
            .iter()
            // 【Rust 语法】map 接收闭包 `|id| { ... }`：闭包是匿名函数，`id` 是迭代到的每个元素（&str 类型）。
            .map(|id| {
                let mut l = OperationLayer::new(id); // 为每个 id 创建一个新操作层
                l.name = layer_display_name(id); // 显示名改用可读名称（如"层1"）
                l // 【Rust 语法】闭包内最后一个表达式 l 作为闭包返回值
            }) // 结束 map 闭包
            // 【Rust 语法】collect()：把迭代器收拢为集合；具体集合类型由 `let layers` 的目标类型推断为 Vec<OperationLayer>。
            .collect();
        Self { // 构造操作集实例
            id: set_id.to_string(), // 操作集 id
            name: set_name.to_string(), // 操作集名称
            common_layer: common, // 公共层
            layers, // 【Rust 语法】字段简写语法：字段名与局部变量同名时可省略 `layers: layers` 直接写 `layers`
        } // 结束 Self 结构体字面量
    } // 结束 create_empty 函数
} // 结束 impl OperationSet 块

// ---------------------------------------------------------------------
// ControllerProfile —— 完整配置
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(Debug, Clone)]：实现调试输出与克隆。
#[derive(Debug, Clone)]
// 【Rust 语法】struct 结构体：整个手柄配置文件的根容器。
pub struct ControllerProfile {
    /// 操作集列表（至少 1 个）
    pub operation_sets: Vec<OperationSet>, // 【Rust 语法】Vec<OperationSet>：操作集列表
    /// 当前激活的操作集 id
    pub active_operation_set_id: String, // 当前处于激活状态的操作集 id
    pub global_settings: GlobalSettings, // 全局设置
} // 结束 ControllerProfile 结构体定义

// 【Rust 语法】impl 块：为 ControllerProfile 实现方法。
impl ControllerProfile {
    /// 当前激活的操作集（无有效激活集时回退第一个；空则 None）
    // 【Rust 语法】方法：`&self` 不可变借用；返回 `Option<&OperationSet>`（操作集的只读借用，可能为 None）。
    pub fn active_set(&self) -> Option<&OperationSet> {
        self.operation_sets // 【Rust 语法】迭代器链：self.operation_sets 是 Vec<OperationSet>
            .iter() // 迭代操作集元素引用
            .find(|s| s.id == self.active_operation_set_id) // 【Rust 语法】find 接收闭包：返回第一个使闭包为 true 的元素（此处按 id 匹配）
            .or_else(|| self.operation_sets.first()) // 【Rust 语法】or_else 接收闭包：找不到时回退到第一个操作集（first() 返回 Option）
    } // 结束 active_set 函数

    // 【Rust 语法】方法：`&mut self` 可变借用（可修改自身字段）；返回 `Option<&mut OperationSet>`（可变借用）。
    pub fn active_set_mut(&mut self) -> Option<&mut OperationSet> {
        let id = self.active_operation_set_id.clone(); // 先复制出 id，避免借用冲突
        // 【Rust 语法】if let 模式匹配：若 position(...) 返回 Some(pos) 则把内部值绑定到 pos 并进入分支；否则走 else。
        if let Some(pos) = self.operation_sets.iter().position(|s| s.id == id) {
            Some(&mut self.operation_sets[pos]) // 按位置返回对应操作集的可变借用
        } else { // 未找到时的 else 分支
            self.operation_sets.first_mut() // 回退到第一个操作集（可变版本）
        } // 结束 else 分支
    } // 结束 active_set_mut 函数

    /// 当前激活操作集的公共层
    // 【Rust 语法】方法：`&self` 不可变借用；`map(闭包)` 把 Option<&OperationSet> 变换为 Option<&OperationLayer>。
    pub fn common_layer(&self) -> Option<&OperationLayer> {
        self.active_set().map(|s| &s.common_layer) // 取激活操作集的公共层字段引用
    } // 结束 common_layer 函数

    /// 当前激活操作集的公共层（可变）
    // 【Rust 语法】方法：`&mut self` 可变借用；map 闭包里取公共层的可变引用 `&mut s.common_layer`。
    pub fn common_layer_mut(&mut self) -> Option<&mut OperationLayer> {
        self.active_set_mut().map(|s| &mut s.common_layer) // 取激活操作集公共层的可变引用
    } // 结束 common_layer_mut 函数

    /// 当前激活操作集的操作层
    // 【Rust 语法】方法：`&self` 不可变借用；返回 `Vec<&OperationLayer>`（层引用的向量）。
    pub fn layers(&self) -> Vec<&OperationLayer> {
        // 【Rust 语法】match 匹配 Option：Some(s) 取出内部值绑定到 s，None 分支处理"无值"情况。
        match self.active_set() {
            Some(s) => s.layers.iter().collect(), // 把各层引用收集成 Vec<&OperationLayer>
            None => Vec::new(), // 无激活集时返回空向量
        } // 结束 match 表达式
    } // 结束 layers 函数

    // 【Rust 语法】方法：`&mut self` 可变借用；返回层可变引用的向量。
    pub fn layers_mut(&mut self) -> Vec<&mut OperationLayer> {
        // 【Rust 语法】match 匹配 Option<&mut OperationSet>。
        match self.active_set_mut() {
            Some(s) => s.layers.iter_mut().collect(), // iter_mut() 产生可变引用迭代器并收集成向量
            None => Vec::new(), // 无激活集时返回空向量
        } // 结束 match 表达式
    } // 结束 layers_mut 函数

    /// 当前激活操作集的显示名
    // 【Rust 语法】方法：`&self` 不可变借用；返回值 String（新创建的字符串）。
    pub fn active_operation_set_name(&self) -> String {
        self.active_set().map(|s| s.name.clone()).unwrap_or_default() // 【Rust 语法】map 取出名称克隆，unwrap_or_default() 在 None 时返回 String 默认值（空串）
    } // 结束 active_operation_set_name 函数

    /// 按 id 设置当前激活操作集；无效 id 返回 false
    // 【Rust 语法】方法：`&mut self` 可变借用；返回 bool 表示是否设置成功。
    pub fn set_active_operation_set(&mut self, id: &str) -> bool {
        // 【Rust 语法】any(闭包)：只要集合中任一元素满足闭包条件即返回 true。
        if self.operation_sets.iter().any(|s| s.id == id) {
            self.active_operation_set_id = id.to_string(); // 更新激活集 id
            true // 成功则返回 true
        } else { // 未找到时的 else 分支
            false // id 不存在则返回 false
        } // 结束 else 分支
    } // 结束 set_active_operation_set 函数

    /// 生成一个不与现有操作集重复的新 id（"Set1"、"Set2"...）
    // 【Rust 语法】方法：`&self` 不可变借用；返回新 String。
    pub fn unique_operation_set_id(&self) -> String {
        let mut max = 0; // 记录现有操作集编号中的最大值
        // 【Rust 语法】for ... in 循环：`&self.operation_sets` 是对列表的不可变借用，`s` 每次为操作集引用 &OperationSet。
        for s in &self.operation_sets {
            // 【Rust 语法】if let 模式匹配：strip_prefix("Set") 返回 Option<&str>，若以 "Set" 开头则取出剩余部分绑定到 rest。
            if let Some(rest) = s.id.strip_prefix("Set") {
                // 【Rust 语法】`parse::<i32>()` 把字符串解析为 i32，返回 Result；`if let Ok(n)` 仅在解析成功时取出数值 n。
                if let Ok(n) = rest.parse::<i32>() {
                    if n > max { // 若比当前最大值大则更新
                        max = n; // 更新最大值
                    } // 结束 if n > max 判断
                } // 结束 if let Ok(n) 分支
            } // 结束 if let Some(rest) 分支
        } // 结束 for 循环
        format!("Set{}", max + 1) // 【Rust 语法】format! 宏：按格式模板生成字符串并返回 String；编号取 max+1 保证不重复
    } // 结束 unique_operation_set_id 函数

    /// 按 id 查找层（仅当前激活操作集内，含公共层）
    // 【Rust 语法】方法：`&self` 不可变借用；返回 Option<&OperationLayer>。
    pub fn find_layer(&self, id: &str) -> Option<&OperationLayer> {
        // 【Rust 语法】`?` 运算符：若 active_set() 返回 None，则整个函数立即返回 None；若为 Some 则解包出内部值绑定给 set。
        let set = self.active_set()?;
        if set.common_layer.id == id { // 若 id 命中公共层
            return Some(&set.common_layer); // 命中的是公共层，直接返回其引用
        } // 结束 if 判断
        set.layers.iter().find(|l| l.id == id) // 否则在操作层列表中查找 id 匹配的层引用
    } // 结束 find_layer 函数

    /// 由触发按键找操作层（当前激活操作集内，仅供 UI 展示）
    // 【Rust 语法】方法：`&self` 不可变借用；返回 Option<&OperationLayer>。
    pub fn find_layer_by_trigger(&self, b: ControllerButton) -> Option<&OperationLayer> {
        let set = self.active_set()?; // 无激活集则借 `?` 提前返回 None
        set.layers // 取操作层列表引用
            .iter() // 迭代层引用
            .find(|l| l.has_trigger_button && l.trigger_button == b) // 查找"配置了触发按键且触发键等于 b"的层
    } // 结束 find_layer_by_trigger 函数

    /// 生成默认配置（WoW 预设：1 个"默认操作集"，含公共层 + 10 个操作层）
    // 【Rust 语法】关联函数（无 self）：静态构造器，返回完整默认配置。
    pub fn create_default() -> Self {
        let mut p = Self { // 构造 ControllerProfile 实例（p 声明为 mut，后续要修改其公共层）
            // 【Rust 语法】vec![...] 宏：创建 Vec 并直接填充给定元素。
            operation_sets: vec![OperationSet::create_empty("Set1", "默认操作集")], // 初始含 1 个名为"默认操作集"的 Set1
            active_operation_set_id: "Set1".to_string(), // 默认激活 Set1
            global_settings: GlobalSettings::default(), // 全局设置取默认值
        }; // 结束 Self 结构体字面量
        // 【Rust 语法】expect("...")：Option/Result 为 None/Err 时以给定信息 panic；此处确定存在默认集，不会失败，安全解包。
        let common = p.common_layer_mut().expect("default set exists"); // 获取激活操作集公共层的可变引用
        // ---- 公共层默认映射 ----
        common.button_mappings.insert( // 【Rust 语法】HashMap::insert(键, 值)：向映射表插入一对键值
            ControllerButton::A, // A 键
            KeyMapping::new(MappedAction::keyboard_key(android_key::SPACE)), // A 键 -> 键盘空格键
        ); // 结束 insert 调用
        common.button_mappings.insert( // 插入 B 键映射
            ControllerButton::B, // B 键
            KeyMapping::new(MappedAction::mouse_click(MouseButton::Right)), // B 键 -> 鼠标右键
        ); // 结束 insert 调用
        common.button_mappings.insert( // 插入 X 键映射
            ControllerButton::X, // X 键
            KeyMapping::new(MappedAction::mouse_click(MouseButton::Left)), // X 键 -> 鼠标左键
        ); // 结束 insert 调用
        common.button_mappings.insert( // 插入 Y 键映射
            ControllerButton::Y, // Y 键
            KeyMapping::new(MappedAction::keyboard_key(android_key::I)), // Y 键 -> 键盘 I 键
        ); // 结束 insert 调用
        common.button_mappings.insert( // 插入 Menu 键映射
            ControllerButton::Menu, // Menu 键
            KeyMapping::new(MappedAction::keyboard_key(android_key::ESCAPE)), // Menu 键 -> ESC 键
        ); // 结束 insert 调用
        common.button_mappings.insert( // 插入 Options 键映射
            ControllerButton::Options, // Options 键
            KeyMapping::new(MappedAction::keyboard_key(android_key::M)), // Options 键 -> M 键
        ); // 结束 insert 调用
        // 右摇杆按压 -> 视角控制
        common.button_mappings.insert( // 插入右摇杆按压映射
            ControllerButton::RightStickClick, // 右摇杆按压
            KeyMapping::new(MappedAction::look_around()), // 右摇杆按压 -> 视角控制
        ); // 结束 insert 调用
        p // 【Rust 语法】最后一个表达式 p 作为函数返回值
    } // 结束 create_default 函数
} // 结束 impl ControllerProfile 块
