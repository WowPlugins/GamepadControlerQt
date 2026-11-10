// =====================================================================
// commands.rs —— Tauri IPC 命令（前端 WebView <-> 后端）
//
// 前端通过 window.__TAURI__.core.invoke("命令名", 参数) 调用；
// 所有命令运行在 Tauri 主线程，内部短时 lock 共享 core。
// =====================================================================

// 【Rust 语法】use 导入：crate:: 表示从本 crate 根开始定位；::* 通配导入模块全部公开项；花括号内为多个具名项
use crate::core::config_manager; // 导入配置管理模块（保存 / 加载 / 重置）
use crate::core::input_types::*; // 通配导入手柄输入类型：按钮枚举与显示名转换函数
use crate::core::mapping_types::{ActionType, KeyMapping, KeyboardRule, MappedAction, StateCond, StateLogic}; // 导入映射动作类型、按键映射、键盘规则结构体、状态条件
use crate::AppState; // 导入主程序定义的全局共享状态结构体
use serde::{Deserialize, Serialize}; // serde 序列化框架：派生 trait 的基础（JSON 与结构体互转）
use std::sync::atomic::Ordering; // 原子操作的内存序（用于原子标志的读写）
use tauri::{AppHandle, Manager, State}; // tauri 类型：应用句柄、窗口管理 trait、命令状态包装

// ---------------------------------------------------------------------
// 主窗口快照
// ---------------------------------------------------------------------

// 【Rust 语法】derive 派生宏：自动为结构体实现 Serialize trait，使其可序列化为 JSON 返回前端
#[derive(Serialize)]
// 【Rust 语法】struct 结构体：pub 公开声明；字段默认私有；描述一个操作集的基本信息
pub struct SetInfo { // 操作集信息
    id: String, // 操作集唯一标识
    name: String, // 操作集显示名称
} // 结构体结束

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct LayerInfo { // 层信息
    id: String, // 层唯一标识
    name: String, // 层显示名称
    active: bool, // 该层当前是否激活
} // 结构体结束

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
// 【Rust 语法】struct 结构体：含多种类型字段，一次命令调用返回主窗口全部状态
pub struct Snapshot { // 主窗口整体状态快照
    connected: bool, // 手柄是否已连接
    running: bool, // 映射是否运行中
    active_set_id: String, // 当前激活操作集的 id
    active_set_name: String, // 当前激活操作集的名称
    layer_name: String, // 当前激活层的名称
    sets: Vec<SetInfo>, // 【Rust 语法】Vec<T> 泛型动态数组：全部操作集列表
    layers: Vec<LayerInfo>, // 当前操作集内全部层列表
    mouse_toggle: Option<String>, // 【Rust 语法】Option<T> 枚举：Some 表示有鼠标长按锁存，None 表示没有
    deadzone: f32, // 摇杆死区（f32：单精度浮点）
    look_sensitivity: f32, // 视角灵敏度
    look_smoothing: f32, // 视角平滑
    look_acceleration: f32, // 视角加速度
    overlay_opacity: f32, // 悬浮窗透明度
    wow_exe: String, // 目标游戏（WoW）可执行文件路径
    wow_exe_history: Vec<String>, // 历史选择过的 WoW 路径（最近优先）
    state_read_enabled: bool, // 是否启用角色状态读取
    state_read_interval_ms: u32, // 状态读取周期（毫秒）
    state_pixel_x: i32, // 像素锚点 X
    state_pixel_y: i32, // 像素锚点 Y
    state_bits: u16, // 当前 10 个状态的位图（bit0..bit9）
    debug_log: bool, // 调试日志开关状态
} // 结构体结束

// ---------------------------------------------------------------------
// 悬浮窗快照
// ---------------------------------------------------------------------

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct MappingRow { // 悬浮窗中一行按键映射
    button: String, // 手柄按钮的显示名
    desc: String, // 映射动作的描述文本
    held: bool, // 该按钮当前是否被按住
} // 结构体结束

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct PressedRow { // 悬浮窗中一个「按下按键 + 其映射功能」条目
    button: String, // 手柄按钮的显示名
    desc: String, // 该按钮当前生效的映射描述（长按已触发时显示长按动作）
} // 结构体结束

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct OverlaySnapshot { // 悬浮窗整体快照
    set_name: String, // 当前操作集名称
    layer_name: String, // 当前层名称
    connected: bool, // 手柄连接状态
    pressed: Vec<String>, // 当前被按下的按钮显示名集合
    pressed_rows: Vec<PressedRow>, // 按下的按钮与其映射功能（收起状态也显示功能）
    mouse_toggle: Option<String>, // 鼠标左右键锁存文案（含 WASD 自动锁定 / 交换锁存；None = 无锁存）
    auto_right_lock: bool, // 是否为 WASD 左摇杆自动锁定的右键
    mappings: Vec<MappingRow>, // 当前层按键映射行列表
    /// 当前操作集内映射最多的层的映射数，前端据此用占位行固定展开高度（切层不伸缩）
    max_mappings: usize, // 最大映射数
    /// 悬浮窗卡片背景透明度（0.2 ~ 1.0），前端据此设置背景 alpha
    opacity: f32, // 透明度数值
    /// 悬浮窗展开/收起切换计数：每次手柄触发 +1，前端据此翻转展开状态
    expand_rev: u64, // 切换计数
} // 结构体结束

// ---------------------------------------------------------------------
// 层编辑快照
// ---------------------------------------------------------------------

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct SwitchTarget { // "切换层"动作的可选目标层
    id: String, // 目标层 id
    name: String, // 目标层名称
    display: String, // 显示文本（含"公共层"等逻辑名）
} // 结构体结束

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct ButtonGridItem { // 层编辑界面按钮网格的一个格子
    name: String, // 按钮的枚举名（供前端回传识别）
    display: String, // 按钮显示名
    pressed: bool, // 是否被按住（用于实时高亮）
} // 结构体结束

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct LayerEditSnapshot<'r> { // 层编辑页整体快照
    layer_name: String, // 被编辑层的名称
    trigger_button: Option<&'r str>, // 触发层切换的按钮（公共层无指向本层的切换映射时为 ""）
    switch_targets: Vec<SwitchTarget>, // 可切换目标列表
    buttons: Vec<ButtonGridItem>, // 按钮网格数据
} // 结构体结束

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct ActionView { // 单个动作的展示视图（长按动作复用）
    key_code: i32, // 键盘键码（Android KeyCode 常量）
    kind: String, // 动作类型字符串
    desc: String, // 动作描述
    subs: Vec<String>, // 子命令键码（组合键）列表
    mouse_button: Option<MouseButton>, // 鼠标键
    layer_name: Option<String>, // 目标层名称（切换层动作时使用）
} // 结构体结束

// 【Rust 语法】derive 派生宏：自动实现 Serialize（序列化）trait
#[derive(Serialize)]
pub struct MappingView { // 单个按键映射的展示视图
    key_code: i32, // 键盘键码（存储 Android KeyCode 常量，运行时再转换为 Windows 虚拟键码）
    kind: String, // 动作类型字符串（keyboard / mouse / ...）
    desc: String, // 动作描述
    subs: Vec<String>, // 子命令键码（组合键）列表
    sub_on_layer_hold: bool, // 切层保持开关：切到本层时是否把点击子命令修饰键一起按下保持
    has_mapping: bool, // 是否已有映射
    mouse_button: Option<MouseButton>, // 鼠标键（Some 表示有，None 表示无）
    layer_name: Option<String>, // 目标层名称（切换层动作时使用）
    long_press: Option<ActionView>, // 长按动作（None 表示未配置长按）
    long_press_ms: u32, // 长按判定阈值（毫秒）
    state_conditions: Vec<StateCondView>, // 状态条件列表
    state_logic: String, // 条件组合方式（or / and）
} // 结构体结束

/// 状态条件的展示/提交视图
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")] // JSON 字段使用 camelCase（stateId / expected）
pub struct StateCondView {
    pub state_id: String, // 状态 id（stealthed / chatOpen / ...）
    pub expected: bool,   // 期望值
}

// 【Rust 语法】impl 实现块：为结构体添加关联方法，通过 MappingView::none() 调用（无需实例）
impl MappingView {
    // 【Rust 语法】关联函数 + Self 返回类型：不接收 self 参数，Self 即 MappingView 自身类型
    fn none() -> Self { // 构造一个"无映射"占位视图
        Self { // 结构体字面量初始化
            key_code: 0, // 无映射的键码为 0
            kind: "keyboard".into(), // 【Rust 语法】.into() 类型转换：把 &str 字面量转换为 String
            desc: "（无映射）".into(), // 空映射的描述文本
            subs: Vec::new(), // 空子命令列表
            sub_on_layer_hold: true, // 切层保持开关默认开
            has_mapping: false, // 标记为无映射
            mouse_button: None, // 无鼠标键
            layer_name: None, // 无目标层名称
            long_press: None, // 无长按动作
            long_press_ms: KeyMapping::DEFAULT_LONG_PRESS_MS, // 长按阈值取默认值
            state_conditions: Vec::new(), // 无条件
            state_logic: "or".into(), // 默认逻辑或
        } // Self 字面量结束
    } // none 函数结束
} // impl 块结束

// 把动作转换为前端展示视图
fn action_view(a: &MappedAction, subs: &[i32]) -> ActionView {
    ActionView {
        key_code: a.key_code,
        kind: kind_str(a),
        desc: describe_action(a),
        subs: subs.iter().map(|&k| key_code_to_name(k)).collect(),
        mouse_button: Some(a.mouse_button),
        layer_name: a.layer_name.clone(),
    }
}

// ---------------------------------------------------------------------
// 层查找辅助（与 gpui 版 layer_edit.rs 一致）
// ---------------------------------------------------------------------
// 【Rust 语法】生命周期参数 + 引用返回：<'a> 声明生命周期 a；输入 profile 与返回值 Option<&'a ...> 共享同一生命周期，保证返回的引用仍然有效
fn find_layer_ref<'a>( // 按 id 查找层，返回不可变引用
    profile: &'a crate::core::mapping_types::ControllerProfile, // 【Rust 语法】&'a T 带生命周期注解的不可变借用：只读访问控制器配置
    id: &str, // 【Rust 语法】&str 字符串切片：只借用不拥有所有权，作为层 id 入参
) -> Option<&'a crate::core::mapping_types::OperationLayer> { // 返回可选的操作层引用（找不到则为 None）
    if id == "Common" { // 特殊 id "Common" 表示公共层
        profile.common_layer() // 返回公共层的引用
    } else { // 否则按 id 在层列表中查找
        // 【Rust 语法】迭代器链：into_iter() 生成迭代器，find 接收闭包 |l| 判断条件，返回首个匹配项
        profile.layers().into_iter().find(|l| l.id == id) // 遍历层列表，找到 id 相同的层
    } // else 结束
} // 函数结束

// 【Rust 语法】可变借用 &'a mut：允许调用方修改被借用的数据；同一时刻只能存在一个可变借用
fn find_layer_mut<'a>( // 按 id 查找层，返回可变引用
    profile: &'a mut crate::core::mapping_types::ControllerProfile, // 对控制器配置的可变借用（可修改）
    id: &str, // 层 id 入参（&str 借用）
) -> Option<&'a mut crate::core::mapping_types::OperationLayer> { // 返回可选的层可变引用
    if id == "Common" { // 公共层特殊处理
        profile.common_layer_mut() // 返回公共层的可变引用
    } else { // 其他层按 id 查找
        profile.layers_mut().into_iter().find(|l| l.id == id) // 遍历层列表找 id 相同者（可变迭代）
    } // else 结束
} // 函数结束

// ---------------------------------------------------------------------
// 动作描述 / 类型串（与 gpui 版 layer_edit.rs 一致）
// ---------------------------------------------------------------------
// 【Rust 语法】函数：接收 &MappedAction 不可变借用，返回拥有所有权的 String
fn describe_action(a: &MappedAction) -> String { // 把映射动作转成人类可读的描述文本
    // 【Rust 语法】match 模式匹配：按枚举值逐一分支执行；r#type 是原始标识符（type 是关键字，加 r# 转义）
    match a.r#type { // 按动作类型分发
        // 【Rust 语法】format! 格式化宏：把表达式值嵌入字符串模板生成 String
        ActionType::KeyboardKey => format!("键盘: {}", key_code_to_name(a.key_code)), // 键盘键：拼出"键盘: 键名"
        ActionType::MouseClick => format!("鼠标: {}", mouse_button_display_name(a.mouse_button)), // 鼠标点击
        ActionType::MouseToggle => { // 鼠标长按锁存：花括号分支块可含多行
            format!("鼠标长按: {}", mouse_button_display_name(a.mouse_button)) // 拼出"鼠标长按: 键名"
        } // 分支块结束
        ActionType::WheelUp => "滚轮上".to_string(), // 【Rust 语法】.to_string()：把 &str 转为 String；滚轮上
        ActionType::WheelDown => "滚轮下".to_string(), // 滚轮下
        ActionType::SwitchLayer => { // 切换层动作：分支块
            // 【Rust 语法】clone() 复制 String；unwrap_or_default 取 Some 值，None 时返回默认空串
            format!("切换到层: {}", a.layer_name.clone().unwrap_or_default()) // 拼出"切换到层: 名称"
        } // 分支块结束
        ActionType::SwitchLayerAndKey => format!( // 切层+按键：目标层与按键一起展示
            "切层+按键: {} → {}",
            a.layer_name.clone().unwrap_or_default(),
            key_code_to_name(a.key_code)
        ),
        ActionType::LookAround => "视角控制（右摇杆）".to_string(), // 视角控制（右摇杆）
        ActionType::MouseMove => "鼠标移动（左摇杆）".to_string(), // 鼠标移动（左摇杆）
        ActionType::InteractionKey => "交互键".to_string(), // 交互键
        ActionType::ReleaseMouseToggle => "释放鼠标锁存".to_string(), // 释放鼠标左右键锁存
        ActionType::SwapMouseToggle => "交换鼠标左右键锁存".to_string(), // 交换左右键锁存
        ActionType::FineLook => "鼠标精细移动".to_string(), // 右摇杆精细移动
        ActionType::FastLook => "鼠标快速移动".to_string(), // 右摇杆快速移动
        ActionType::TurnAround => format!("快速转身 {}°", a.key_code.abs() * 90 / 400), // 快速转身角度
        ActionType::ToggleOverlayExpand => "切换悬浮窗展开/收起".to_string(), // 悬浮窗展开状态切换
        ActionType::StateMacro => format!("解锁右键+居中+按键: {}", key_code_to_name(a.key_code)), // 解锁+居中+发送组合键
        _ => "（未知）".to_string(), // 【Rust 语法】通配符 _ 分支：兜底匹配所有未列出的枚举值
    } // match 结束
} // 函数结束

fn kind_str(a: &MappedAction) -> String { // 返回动作类型字符串，供前端区分渲染方式
    match a.r#type { // 按动作类型匹配
        ActionType::KeyboardKey => "keyboard", // 键盘键
        ActionType::MouseClick => "mouse", // 鼠标点击
        ActionType::MouseToggle => "mousetoggle", // 鼠标长按
        ActionType::WheelUp => "wheelup", // 滚轮上
        ActionType::WheelDown => "wheeldown", // 滚轮下
        ActionType::SwitchLayer => "switchlayer", // 切换层
        ActionType::SwitchLayerAndKey => "switchlayerandkey", // 切层+按键
        ActionType::LookAround => "lookaround", // 视角控制
        ActionType::MouseMove => "mousemove", // 鼠标移动
        ActionType::InteractionKey => "interactionkey", // 交互键
        ActionType::ReleaseMouseToggle => "releasemousetoggle", // 释放鼠标锁存
        ActionType::SwapMouseToggle => "swapmousetoggle", // 交换左右键锁存
        ActionType::FineLook => "finelook", // 精细移动
        ActionType::FastLook => "fastlook", // 快速移动
        ActionType::TurnAround => "turnaround", // 快速转身
        ActionType::ToggleOverlayExpand => "toggleoverlayexpand", // 切换悬浮窗展开/收起
        ActionType::StateMacro => "statemacro", // 状态宏
        _ => "keyboard", // 未知类型兜底为 keyboard
    } // match 结束
    .to_string() // 把匹配出的 &str 转为 String（match 是表达式，可直接接方法）
} // 函数结束

/// 按类型字符串构建动作（主动作与长按动作共用）
fn build_action(
    kind: &str,
    key_code: Option<i32>,
    mouse_button: Option<String>,
    layer_name: Option<String>,
) -> Option<MappedAction> {
    match kind { // 按类型字符串构建动作
        "keyboard" => key_code.map(MappedAction::keyboard_key), // 键盘：键码存在则构建
        "mouse" => mouse_button // 鼠标点击
            .as_deref() // 【Rust 语法】as_deref：Option<String> → Option<&str>
            .and_then(mouse_button_from_name) // 解析按钮名
            .map(MappedAction::mouse_click), // 构建鼠标点击动作
        "mousetoggle" => mouse_button // 鼠标长按
            .as_deref()
            .and_then(mouse_button_from_name)
            .map(MappedAction::mouse_toggle), // 构建鼠标长按动作
        "wheelup" => Some(MappedAction::wheel_up()), // 滚轮上
        "wheeldown" => Some(MappedAction::wheel_down()), // 滚轮下
        "switchlayer" => layer_name.map(|n| MappedAction::switch_layer(&n)), // 切换层
        // 切层+按键：需要目标层；按键可后选（0 表示只切层不按键）
        "switchlayerandkey" => layer_name
            .map(|n| MappedAction::switch_layer_and_key(&n, key_code.unwrap_or(0))),
        "lookaround" => Some(MappedAction::look_around()), // 视角控制
        "mousemove" => Some(MappedAction::mouse_move()), // 鼠标移动
        "interactionkey" => Some(MappedAction::interaction_key()), // 交互键（需求1）
        "releasemousetoggle" => Some(MappedAction::release_mouse_toggle()), // 释放鼠标锁存（需求4）
        "swapmousetoggle" => Some(MappedAction::swap_mouse_toggle()), // 交换左右键锁存（需求5）
        "finelook" => Some(MappedAction::fine_look()), // 精细移动（需求7）
        "fastlook" => Some(MappedAction::fast_look()), // 快速移动（精细移动的反向）
        // 快速转身：keyCode 承载有符号像素（正=右，负=左），缺省 800（约 180°）
        "turnaround" => Some(MappedAction::turn_around(key_code.filter(|v| *v != 0).unwrap_or(800))),
        "toggleoverlayexpand" => Some(MappedAction::toggle_overlay_expand()), // 切换悬浮窗展开/收起
        "statemacro" => Some(MappedAction::state_macro(key_code.unwrap_or(0))), // 状态宏：未选键时先建空动作，选键后再更新
        _ => None, // 未知类型不生成动作
    } // match 结束
} // 函数结束

// ---------------------------------------------------------------------
// 主窗口：整体快照
// ---------------------------------------------------------------------
// 【Rust 语法】属性宏 #[tauri::command]：把函数注册为前端可 invoke 的 IPC 命令，参数自动从 JSON 反序列化
#[tauri::command]
// 【Rust 语法】State<'_, AppState>：Tauri 状态注入，'_ 表示省略具体生命周期；返回值 Snapshot 自动序列化为 JSON
pub fn get_snapshot(state: State<'_, AppState>) -> Snapshot { // 主窗口整体快照命令
    let running = state.shared.running.load(Ordering::SeqCst); // 读取映射运行标志（SeqCst 为最强内存序）
    let core = state.shared.core.lock().unwrap(); // 对共享核心加锁，取得不可变访问（语句结束自动释放）
    let connected = core.connected; // 手柄连接状态
    let layer_name = core.steam.active_layer_name().to_string(); // 当前激活层名称
    let active_set_id = core.steam.profile.active_operation_set_id.clone(); // 复制当前操作集 id
    let active_set_name = core.steam.profile.active_operation_set_name(); // 取当前操作集名称
    // 【Rust 语法】类型标注 + 迭代器：: Vec<SetInfo> 显式标注收集目标类型
    let sets: Vec<SetInfo> = core // 收集所有操作集信息
        .steam // 访问映射引擎 steam
        .profile // 访问控制器配置
        .operation_sets // 操作集容器
        .iter() // 【Rust 语法】iter()：产生元素的不可变迭代器
        .map(|s| SetInfo { // 【Rust 语法】闭包 + map：对每个元素执行转换
            id: s.id.clone(), // 复制操作集 id
            name: s.name.clone(), // 复制操作集名称
        }) // 闭包返回 SetInfo
        .collect(); // 【Rust 语法】collect()：把迭代器收集为 Vec（类型由变量标注推断）
    let layers: Vec<LayerInfo> = core // 收集层信息
        .steam // 访问映射引擎
        .profile // 访问配置
        .layers() // 取当前操作集的层列表
        .iter() // 生成迭代器
        .map(|l| LayerInfo { // 逐层转换
            id: l.id.clone(), // 复制层 id
            name: l.name.clone(), // 复制层名称
            active: core.steam.is_layer_active(&l.id), // 查询该层是否激活
        }) // 闭包返回 LayerInfo
        .collect(); // 收集为 Vec<LayerInfo>
    let mouse_toggle = core.mapper.lock_summary(); // 当前左右键锁存状态文案（需求1：左右键都判断）
    let gs = &core.steam.profile.global_settings; // 借用全局设置引用，避免重复写长路径
    Snapshot { // 构造快照结构体
        connected, // 连接状态
        running, // 运行标志
        active_set_id, // 当前操作集 id
        active_set_name, // 当前操作集名称
        layer_name, // 当前层名称
        sets, // 操作集列表
        layers, // 层列表
        mouse_toggle, // 鼠标长按锁存（Option）
        deadzone: gs.deadzone, // 死区
        look_sensitivity: gs.look_sensitivity, // 视角灵敏度
        look_smoothing: gs.look_smoothing, // 视角平滑
        look_acceleration: gs.look_acceleration, // 视角加速度
        overlay_opacity: gs.overlay_opacity,
        wow_exe: gs.wow_exe.clone(), // 目标游戏 exe 路径
        wow_exe_history: gs.wow_exe_history.clone(), // 历史路径列表
        state_read_enabled: gs.state_read_enabled, // 是否启用状态读取
        state_read_interval_ms: gs.state_read_interval_ms, // 读取周期
        state_pixel_x: gs.state_pixel_x, // 像素锚点 X
        state_pixel_y: gs.state_pixel_y, // 像素锚点 Y
        state_bits: state.shared.states.load(), // 当前状态位图
        debug_log: gs.debug_log, // 调试日志开关
    } // Snapshot 字面量结束
} // 函数结束

// ---------------------------------------------------------------------
// 主窗口：程序版本号（取 Cargo.toml 包版本，编译时注入）
// ---------------------------------------------------------------------
#[tauri::command]
pub fn get_version() -> String { // 版本号命令
    format!("v{}", env!("CARGO_PKG_VERSION")) // 如 v0.0.79
} // 结束 get_version 函数

// ---------------------------------------------------------------------
// 悬浮窗：整体快照（含当前层映射列表）
// ---------------------------------------------------------------------
// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn get_overlay_snapshot(state: State<'_, AppState>) -> OverlaySnapshot { // 悬浮窗整体快照命令
    let core = state.shared.core.lock().unwrap(); // 对共享核心加锁
    let set_name = core.steam.profile.active_operation_set_name(); // 当前操作集名称
    let layer_name = core.steam.active_layer_name().to_string(); // 当前层名称
    let connected = core.connected; // 手柄连接状态
    let mut pressed: Vec<String> = core // 【Rust 语法】mut 可变绑定：变量后续需修改
        .steam // 访问映射引擎
        .held_buttons() // 获取当前被按住的按钮集合
        .iter() // 生成迭代器
        .map(|b| controller_button_display_name(*b).to_string()) // 把按钮枚举转成显示名
        .collect(); // 收集为 Vec<String>
    pressed.sort(); // 排序，保证前端显示顺序稳定
    // 按下的按钮对应的映射功能：长按已触发时显示长按动作，否则显示主动作
    let mut pressed_rows: Vec<PressedRow> = core
        .steam
        .held_buttons() // 当前按住的按钮集合
        .iter()
        .map(|b| {
            let triggered_long = core.mapper.long_press_buttons.contains(&TriggerId::Pad(*b)); // 是否已触发长按
            let desc = core
                .steam
                .get_effective_mapping(*b) // 查询当前生效映射
                .map(|m| {
                    if triggered_long {
                        if let Some(lp) = &m.long_press {
                            return describe_action(lp); // 长按已生效：显示长按动作
                        }
                    }
                    describe_action(&m.action) // 未配置长按或尚未触发：显示主动作
                })
                .unwrap_or_else(|| "（无映射）".to_string()); // 该按钮在公共层也无映射
            PressedRow {
                button: controller_button_display_name(*b).to_string(), // 按钮显示名
                desc, // 映射功能描述
            }
        })
        .collect();
    pressed_rows.sort_by(|a, b| a.button.cmp(&b.button)); // 按按钮名排序，避免抖动
    let mouse_toggle = core.mapper.lock_summary(); // 鼠标左右键锁存文案（含 WASD 自动锁定 / 交换锁存）
    let auto_right_lock = core.mapper.auto_locked_right; // 是否为 WASD 左摇杆自动锁定的右键
    // 当前层映射列表：最后激活的操作层，否则公共层
    let active_layers = core.steam.get_active_layers(); // 获取当前激活的层引用数组
    let layer_ref = if active_layers.is_empty() { // 【Rust 语法】if 表达式：分支都返回值，赋给 layer_ref
        core.steam.profile.common_layer() // 无激活层时用公共层
    } else { // 有激活层时
        active_layers.last().copied() // 【Rust 语法】last() 返回 Option<&T>，copied() 解引用并复制为 Option<T>
    }; // if 表达式结束
    let held = core.steam.held_buttons(); // 当前按住的按钮集合（用于判断 held 状态）
    let mut mappings: Vec<MappingRow> = Vec::new(); // 新建可变空向量，收集映射行
    if let Some(lr) = layer_ref { // 层引用存在才处理
        for b in all_controller_buttons() { // 【Rust 语法】for 循环：遍历手柄全部按钮
            if let Some(m) = lr.get_mapping(b) { // 该按钮存在映射才处理
                mappings.push(MappingRow { // 【Rust 语法】Vec::push：向向量尾部追加元素
                    button: controller_button_display_name(b).to_string(), // 按钮显示名
                    desc: m.describe(), // 映射描述
                    held: held.contains(&b), // 按钮是否被按住
                }); // MappingRow 构造结束并压入向量
            } // 内层 if-let 结束
        } // for 循环结束
    } // 外层 if-let 结束
    let opacity = (*core).steam.profile.global_settings.overlay_opacity; // 读取透明度
    let expand_rev = core.mapper.overlay_expand_rev; // 悬浮窗展开/收起切换计数（手柄触发时递增）
    // 当前操作集所有层（公共层+操作层）中映射数的最大值：前端据此固定展开高度
    let max_mappings = core.steam.profile.active_set() // 当前激活操作集
        .map(|s| { // 计算该操作集内的最大映射数
            std::iter::once(&s.common_layer) // 公共层
                .chain(s.layers.iter()) // 各操作层
                .map(|l| l.button_mappings.len()) // 每层映射数
                .max() // 取最大
                .unwrap_or(0) // 无层时回退 0
        })
        .unwrap_or(0); // 无操作集时回退 0
    OverlaySnapshot { // 构造悬浮窗快照
        set_name, // 操作集名称
        layer_name, // 层名称
        connected, // 连接状态
        pressed, // 按住的按钮列表
        pressed_rows, // 按住的按钮 + 映射功能
        mouse_toggle, // 鼠标锁存文案（None = 无锁存）
        auto_right_lock, // 是否 WASD 自动锁定右键
        mappings, // 映射行列表
        max_mappings, // 最大映射数（用于固定展开高度）
        opacity, // 透明度
        expand_rev, // 展开/收起切换计数
    } // OverlaySnapshot 字面量结束
} // 函数结束

// ---------------------------------------------------------------------
// 启停
// ---------------------------------------------------------------------
// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn start_mapping(state: State<'_, AppState>) { // 开始映射命令（无返回值）
    state.shared.start_mapping(); // 调用共享状态的启动映射方法
}

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn stop_mapping(state: State<'_, AppState>) { // 停止映射命令
    state.shared.stop_mapping(); // 调用共享状态的停止映射方法（释放注入）
}

// ---------------------------------------------------------------------
// 操作集管理
// ---------------------------------------------------------------------
// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn add_operation_set(state: State<'_, AppState>) -> String { // 新增操作集命令，返回新 id
    let mut core = state.shared.core.lock().unwrap(); // 加锁并取得可变访问（mut 因为要修改核心）
    core.add_operation_set() // 新增操作集并返回其 id（块尾表达式即函数返回值）
}

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn rename_operation_set(state: State<'_, AppState>, set_id: String, name: String) -> bool { // 重命名操作集命令（参数由前端传入）
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    core.rename_operation_set(&set_id, &name) // 传引用调用重命名方法，返回是否成功
}

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn copy_operation_set(state: State<'_, AppState>, set_id: String, name: String) -> bool { // 复制操作集命令
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    core.copy_operation_set(&set_id, &name) // 调用复制方法，返回是否成功
}

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn delete_operation_set(state: State<'_, AppState>, set_id: String) -> bool { // 删除操作集命令
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    core.delete_operation_set(&set_id) // 调用删除方法，返回是否成功
}

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn switch_operation_set(state: State<'_, AppState>, set_id: String) -> bool { // 切换操作集命令
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    core.switch_operation_set(&set_id) // 调用切换方法，返回是否成功
}

// ---------------------------------------------------------------------
// 全局设置
// ---------------------------------------------------------------------
// 【Rust 语法】derive 派生宏：实现 Deserialize（反序列化），命令参数从 JSON 还原为枚举
#[derive(Deserialize)]
// 【Rust 语法】serde 属性 rename_all：把 Rust 驼峰枚举名映射为 snake_case 字符串（前端传 deadzone 等）
#[serde(rename_all = "snake_case")]
// 【Rust 语法】enum 枚举：无字段的 C 风格枚举，表示可调整的全局设置项
pub enum SettingKey { // 全局设置项枚举
    Deadzone, // 摇杆死区
    LookSensitivity, // 视角灵敏度
    LookSmoothing, // 视角平滑
    LookAcceleration, // 视角加速度
} // 枚举结束

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn adjust_setting(state: State<'_, AppState>, key: SettingKey, delta: f32) { // 调整全局设置命令：key 指定项，delta 为增量
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    let mut gs = core.steam.profile.global_settings.clone(); // 复制一份全局设置，避免在锁内长期借用
    match key { // 按设置项分支处理
        SettingKey::Deadzone => gs.deadzone = (gs.deadzone + delta).clamp(0.0, 0.5), // 【Rust 语法】clamp：把值限制在 [min, max] 区间；死区增减并限幅 0~0.5
        SettingKey::LookSensitivity => { // 灵敏度分支块
            gs.look_sensitivity = (gs.look_sensitivity + delta).clamp(0.05, 2.0) // 限幅 0.05~2.0
        } // 分支块结束
        SettingKey::LookSmoothing => { // 平滑分支块
            gs.look_smoothing = (gs.look_smoothing + delta).clamp(0.0, 0.95) // 限幅 0~0.95
        } // 分支块结束
        SettingKey::LookAcceleration => { // 加速度分支块
            gs.look_acceleration = (gs.look_acceleration + delta).clamp(0.5, 3.0) // 限幅 0.5~3.0
        } // 分支块结束
    } // match 结束
    core.update_global_settings(gs); // 用修改后的设置更新核心
    drop(core); // 先释放锁，sync 内部还要再加一次
    state.shared.sync_state_config(); // 同步到像素读取线程
}

// 开关角色状态读取
#[tauri::command]
pub fn set_state_read_enabled(state: State<'_, AppState>, enabled: bool) {
    {
        let mut core = state.shared.core.lock().unwrap();
        let mut gs = core.steam.profile.global_settings.clone();
        gs.state_read_enabled = enabled;
        core.update_global_settings(gs);
    }
    state.shared.sync_state_config();
    crate::dbg_log!("状态读取开关 -> {}", if enabled { "开启" } else { "关闭" });
}

// 设置状态读取周期（毫秒，限幅 10~2000）
#[tauri::command]
pub fn set_state_read_interval(state: State<'_, AppState>, ms: u32) {
    {
        let mut core = state.shared.core.lock().unwrap();
        let mut gs = core.steam.profile.global_settings.clone();
        gs.state_read_interval_ms = ms.clamp(10, 2000);
        core.update_global_settings(gs);
    }
    state.shared.sync_state_config();
    crate::dbg_log!("状态读取周期 -> {}ms", ms.clamp(10, 2000));
}

// 设置像素锚点坐标（与插件里 SLStateDB.pixel_x/pixel_y 一致）
#[tauri::command]
pub fn set_state_pixel_pos(state: State<'_, AppState>, x: i32, y: i32) {
    {
        let mut core = state.shared.core.lock().unwrap();
        let mut gs = core.steam.profile.global_settings.clone();
        gs.state_pixel_x = x.max(0);
        gs.state_pixel_y = y.max(0);
        core.update_global_settings(gs);
    }
    state.shared.sync_state_config();
    crate::dbg_log!("像素锚点 -> ({}, {})", x.max(0), y.max(0));
}

// 设置调试日志开关（开启时分配控制台窗口并开始输出日志）
#[tauri::command]
pub fn set_debug_log(state: State<'_, AppState>, enabled: bool) {
    let (read_enabled, interval, px, py) = {
        let mut core = state.shared.core.lock().unwrap(); // 加锁修改全局设置
        let mut gs = core.steam.profile.global_settings.clone(); // 克隆一份设置
        gs.debug_log = enabled; // 更新调试日志开关
        let info = (
            gs.state_read_enabled,
            gs.state_read_interval_ms,
            gs.state_pixel_x,
            gs.state_pixel_y,
        ); // 顺带取出像素读取配置，便于开启时在日志里说明当前采集参数
        core.update_global_settings(gs); // 写回配置
        info
    };
    if enabled {
        crate::core::debug_log::set_enabled(true); // 先打开开关（后续日志才能输出）
        crate::dbg_log!("调试日志已开启");
        crate::dbg_log!(
            "状态读取 启用={} 周期={}ms 锚点=({}, {})",
            read_enabled,
            interval,
            px,
            py
        );
        if !read_enabled {
            crate::dbg_log!("提示：角色状态读取未启用，不会有像素读取日志（请先开启状态读取）");
        }
    } else {
        crate::dbg_log!("调试日志已关闭"); // 关闭前输出最后一条
        crate::core::debug_log::set_enabled(false); // 再关闭开关并收起控制台
    }
}

// ---------------------------------------------------------------------
// 物理键盘组合键规则
// ---------------------------------------------------------------------

/// 单条键盘规则的展示视图
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardRuleView {
    id: u32,                             // 规则 id
    enabled: bool,                       // 是否启用
    ctrl: bool,                          // 是否要求 Ctrl
    shift: bool,                         // 是否要求 Shift
    alt: bool,                           // 是否要求 Alt
    win: bool,                           // 是否要求 Win
    key_code: i32,                       // 主键 Android KeyCode
    key_name: String,                    // 主键名（如 F1）
    combo_text: String,                  // 组合键文本（如 Ctrl + Shift + F1）
    kind: String,                        // 动作类型字符串
    action_key_code: i32,                // 动作目标键码（keyboard / statemacro 使用）
    desc: String,                        // 动作描述
    subs: Vec<String>,                   // 子命令键名列表
    mouse_button: Option<MouseButton>,   // 鼠标键（Some 表示有）
    state_conditions: Vec<StateCondView>, // 状态条件列表
    state_logic: String,                 // or / and
}

/// 键盘规则列表整体视图
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardRulesView {
    enabled: bool,                  // 物理键盘映射总开关
    rules: Vec<KeyboardRuleView>,   // 规则列表
}

/// 把一条键盘规则转成前端视图
fn keyboard_rule_view(r: &KeyboardRule) -> KeyboardRuleView {
    // 拼组合键文本：修饰键在前，主键在后
    let mut parts: Vec<String> = Vec::new();
    if r.ctrl {
        parts.push("Ctrl".to_string());
    }
    if r.shift {
        parts.push("Shift".to_string());
    }
    if r.alt {
        parts.push("Alt".to_string());
    }
    if r.win {
        parts.push("Win".to_string());
    }
    parts.push(key_code_to_name(r.key_code));
    KeyboardRuleView {
        id: r.id,
        enabled: r.enabled,
        ctrl: r.ctrl,
        shift: r.shift,
        alt: r.alt,
        win: r.win,
        key_code: r.key_code,
        key_name: key_code_to_name(r.key_code),
        combo_text: parts.join(" + "),
        kind: kind_str(&r.mapping.action),
        action_key_code: r.mapping.action.key_code,
        desc: describe_action(&r.mapping.action),
        subs: r
            .mapping
            .sub_commands
            .iter()
            .map(|&k| key_code_to_name(k))
            .collect(),
        mouse_button: Some(r.mapping.action.mouse_button),
        state_conditions: r
            .mapping
            .state_conditions
            .iter()
            .map(|c| StateCondView {
                state_id: c.state_id.clone(),
                expected: c.expected,
            })
            .collect(),
        state_logic: r.mapping.state_logic.as_str().to_string(),
    }
}

// 读取全部键盘规则
#[tauri::command]
pub fn get_keyboard_rules(state: State<'_, AppState>) -> KeyboardRulesView {
    let core = state.shared.core.lock().unwrap();
    KeyboardRulesView {
        enabled: core.steam.profile.global_settings.keyboard_rules_enabled,
        rules: core
            .steam
            .profile
            .keyboard_rules
            .iter()
            .map(keyboard_rule_view)
            .collect(),
    }
}

// 新增一条空白规则（默认主键 F1、未启用），返回新规则 id
#[tauri::command]
pub fn add_keyboard_rule(state: State<'_, AppState>) -> u32 {
    let mut core = state.shared.core.lock().unwrap();
    // 规则 id 取当前最大值 +1（删除后不复用，避免与"已吞键记录"冲突）
    let id = core
        .steam
        .profile
        .keyboard_rules
        .iter()
        .map(|r| r.id)
        .max()
        .unwrap_or(0)
        + 1;
    core.steam.profile.keyboard_rules.push(KeyboardRule {
        id,
        enabled: false, // 新规则默认不启用，配好组合键与动作后由用户打开
        ctrl: false,
        shift: false,
        alt: false,
        win: false,
        key_code: android_key::F1, // 默认主键 F1
        mapping: KeyMapping::new(MappedAction::keyboard_key(0)), // 空动作（键码 0 表示未设置）
    });
    core.profile_rev += 1;
    drop(core); // 先释放锁，sync 内部还要再加锁
    state.shared.sync_keyboard();
    id
}

// 删除一条键盘规则
#[tauri::command]
pub fn delete_keyboard_rule(state: State<'_, AppState>, id: u32) {
    let mut core = state.shared.core.lock().unwrap();
    let before = core.steam.profile.keyboard_rules.len();
    core.steam.profile.keyboard_rules.retain(|r| r.id != id);
    if core.steam.profile.keyboard_rules.len() != before {
        core.profile_rev += 1;
    }
    drop(core);
    state.shared.sync_keyboard();
}

// 启用 / 停用某条规则
#[tauri::command]
pub fn set_keyboard_rule_enabled(state: State<'_, AppState>, id: u32, enabled: bool) {
    let mut core = state.shared.core.lock().unwrap();
    if let Some(r) = core.steam.profile.keyboard_rules.iter_mut().find(|r| r.id == id) {
        r.enabled = enabled;
        core.profile_rev += 1;
    }
    drop(core);
    state.shared.sync_keyboard();
}

// 物理键盘映射总开关
#[tauri::command]
pub fn set_keyboard_rules_enabled(state: State<'_, AppState>, enabled: bool) {
    let mut core = state.shared.core.lock().unwrap();
    let mut gs = core.steam.profile.global_settings.clone();
    gs.keyboard_rules_enabled = enabled;
    core.update_global_settings(gs);
    drop(core);
    state.shared.sync_keyboard();
}

// 设置规则的组合键（修饰键 + 主键）
#[tauri::command]
pub fn set_keyboard_rule_combo(
    state: State<'_, AppState>,
    id: u32,
    ctrl: bool,
    shift: bool,
    alt: bool,
    win: bool,
    key_code: i32,
) {
    let mut core = state.shared.core.lock().unwrap();
    if let Some(r) = core.steam.profile.keyboard_rules.iter_mut().find(|r| r.id == id) {
        r.ctrl = ctrl;
        r.shift = shift;
        r.alt = alt;
        r.win = win;
        r.key_code = key_code;
        core.profile_rev += 1;
    }
    drop(core);
    state.shared.sync_keyboard();
}

// 设置规则的目标动作（键盘 / 鼠标 / 滚轮 / 状态宏等）
#[tauri::command]
pub fn set_keyboard_rule_action(
    state: State<'_, AppState>,
    id: u32,
    kind: String,
    key_code: Option<i32>,
    mouse_button: Option<String>,
) {
    // 键盘规则没有"切换层"概念，layer_name 固定传 None
    let Some(action) = (if kind == "none" {
        // 未设置：置为空键盘动作（键码 0），相当于清除该规则的动作
        Some(MappedAction::keyboard_key(0))
    } else {
        build_action(&kind, key_code, mouse_button, None)
    }) else {
        return;
    };
    let mut core = state.shared.core.lock().unwrap();
    if let Some(r) = core.steam.profile.keyboard_rules.iter_mut().find(|r| r.id == id) {
        let main_code = action.key_code;
        r.mapping.action = action;
        // 子命令与主动作重复时移除，避免组合键自己按自己
        r.mapping.sub_commands.retain(|k| *k != main_code);
        core.profile_rev += 1;
    }
    drop(core);
    state.shared.sync_keyboard();
}

// 切换规则的子命令（组合键）
#[tauri::command]
pub fn toggle_keyboard_rule_sub(state: State<'_, AppState>, id: u32, key_code: i32) {
    let mut core = state.shared.core.lock().unwrap();
    if let Some(r) = core.steam.profile.keyboard_rules.iter_mut().find(|r| r.id == id) {
        if let Some(pos) = r.mapping.sub_commands.iter().position(|&k| k == key_code) {
            r.mapping.sub_commands.remove(pos); // 已存在则取消
        } else if r.mapping.sub_commands.len() < KeyMapping::MAX_SUB_COMMANDS {
            r.mapping.sub_commands.push(key_code); // 未满则追加
        }
        core.profile_rev += 1;
    }
    drop(core);
    state.shared.sync_keyboard();
}

// 设置规则的状态条件（conditions 为空表示始终生效）
#[tauri::command]
pub fn set_keyboard_rule_state_conditions(
    state: State<'_, AppState>,
    id: u32,
    conditions: Vec<StateCondView>,
    logic: String,
) {
    let mut core = state.shared.core.lock().unwrap();
    if let Some(r) = core.steam.profile.keyboard_rules.iter_mut().find(|r| r.id == id) {
        r.mapping.state_conditions = conditions
            .into_iter()
            .map(|c| StateCond {
                state_id: c.state_id,
                expected: c.expected,
            })
            .collect();
        r.mapping.state_logic = StateLogic::from_str(&logic);
        core.profile_rev += 1;
    }
    drop(core);
    state.shared.sync_keyboard();
}

// ---------------------------------------------------------------------
// 配置保存 / 重置
// ---------------------------------------------------------------------
// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn save_config(state: State<'_, AppState>) { // 保存配置命令
    // 【Rust 语法】块表达式：锁在块内使用，块结束自动释放锁后再做文件写入
    let profile = { // 取当前配置快照
        let core = state.shared.core.lock().unwrap(); // 对核心加锁
        core.steam.profile.clone() // 复制配置快照
    }; // 块结束，锁在此释放
    config_manager::save(&profile); // 把快照写入配置文件
}

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn reset_config(state: State<'_, AppState>) { // 重置配置命令
    config_manager::reset_to_default(); // 把磁盘配置重置为默认值
    let def = config_manager::load(); // 重新加载默认配置
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    core.load_profile(def); // 把默认配置载入核心（替换当前配置）
    drop(core); // 释放锁后再同步读取配置
    state.shared.sync_state_config(); // 让像素读取恢复默认参数
    state.shared.sync_keyboard(); // 让键盘规则快照恢复默认
    crate::core::debug_log::set_enabled(false); // 重置后关闭调试日志（默认关闭）
}

// ---------------------------------------------------------------------
// 悬浮窗显隐 / 退出
// ---------------------------------------------------------------------
// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn toggle_overlay(app: AppHandle, state: State<'_, AppState>) -> bool { // 切换悬浮窗显隐命令，返回新状态
    // 【Rust 语法】let-else 语句：取到 overlay 窗口则绑定 win，否则走 else 分支
    let Some(win) = app.get_webview_window("overlay") else { // 按标签查找 overlay 窗口
        return false; // 窗口不存在则返回失败
    }; // let-else 结束
    let new = !state.overlay_visible.load(Ordering::SeqCst); // 【Rust 语法】! 取反：读取当前显隐标志后取反
    state.overlay_visible.store(new, Ordering::SeqCst); // 写回新的显隐状态
    if new { // 若变为可见
        let _ = win.show(); // 显示窗口；let _ 丢弃 Result
    } else { // 否则
        let _ = win.hide(); // 隐藏窗口
    } // if-else 结束
    new // 返回新状态（函数最后一个表达式）
} // 函数结束

/// 设置悬浮窗卡片背景透明度（0.2 ~ 1.0），由主窗口滑杆调用
// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn set_overlay_opacity(state: State<'_, AppState>, opacity: f32) { // 设置悬浮窗透明度命令
    let mut o = state.shared.core.lock().unwrap(); // 加锁取得 MutexGuard（可变守卫）
    (*o).steam.profile.global_settings.overlay_opacity = opacity.clamp(0.2, 1.0); // 写入内部值；透明度限幅 0.2~1.0
} // 函数结束

#[tauri::command]
pub fn get_overlay_opacity(state: State<'_, AppState>) -> f32 {
    let core = state.shared.core.lock().unwrap();
    (*core).steam.profile.global_settings.overlay_opacity
}

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn quit_app(app: AppHandle) { // 退出应用命令
    let state = app.state::<AppState>(); // 【Rust 语法】泛型方法 state::<T>()：从应用句柄取回注入的 AppState
    state.shared.stop_mapping(); // 停止映射并释放全部注入
    app.exit(0); // 退出应用，退出码 0
}

// ---------------------------------------------------------------------
// 层编辑
// ---------------------------------------------------------------------
// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn get_layer_edit_snapshot(state: State<'_, AppState>, layer_id: String) -> LayerEditSnapshot<'_> { // 层编辑快照命令
    let core = state.shared.core.lock().unwrap(); // 对核心加锁

    let layer = find_layer_ref(&core.steam.profile, &layer_id).unwrap(); // 查找层引用
    // “激活按钮”= 公共层中按下可切到本层的按键。匹配条件与运行时解析一致
    // （steam_input.rs 先按 id 再按 name 解析 layerName），故命中层 id 或 name 均算
    let trigger_button = find_layer_ref(&core.steam.profile, "Common")
        .and_then(|l| l.button_mappings.iter().find(|(_, m)| {
            matches!( // 切层或切层+按键都可作为激活按钮
                m.action.r#type,
                ActionType::SwitchLayer | ActionType::SwitchLayerAndKey
            )
                && matches!(
                    m.action.layer_name.as_deref(),
                    Some(n) if n == layer.id || n == layer.name
                )
        }))
        .map(|(b, _)| *b); // Option<ControllerButton>：公共层无切到本层的映射时为 None
    let switch_targets: Vec<SwitchTarget> = core // 收集可切换目标
        .steam // 访问映射引擎
        .profile // 访问配置
        .layers() // 层列表
        .iter() // 生成迭代器
        .map(|l| SwitchTarget { // 逐层转换
            id: l.id.clone(), // 目标层 id
            name: l.name.clone(), // 目标层名称
            display: layer_display_name(&l.name), // 层显示名（含公共层逻辑名）
        }) // 闭包返回 SwitchTarget
        .collect(); // 收集为 Vec<SwitchTarget>
    let held = core.steam.held_buttons(); // 当前按住的按钮集合
    let buttons: Vec<ButtonGridItem> = all_controller_buttons() // 收集按钮网格数据
        .into_iter() // 【Rust 语法】into_iter()：消费所有权生成迭代器
        .map(|b| ButtonGridItem { // 逐个按钮转换
            name: controller_button_name(b).to_string(), // 按钮枚举名
            display: controller_button_display_name(b).to_string(), // 按钮显示名
            pressed: held.contains(&b), // 是否被按住
        }) // 闭包返回 ButtonGridItem
        .collect(); // 收集为 Vec<ButtonGridItem>
    LayerEditSnapshot { // 构造层编辑快照
        layer_name: layer.name.clone(), // 层名称
        trigger_button: if let Some(b) = trigger_button { Some(controller_button_name(b)) } else { None }, // 触发按键（公共层无切到本层的映射时为 None）
        switch_targets, // 切换目标列表
        buttons, // 按钮网格数据
    } // LayerEditSnapshot 字面量结束
} // 函数结束

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn get_mapping(state: State<'_, AppState>, layer_id: String, button: String) -> MappingView { // 读取单个按键映射命令
    let Some(b) = controller_button_from_name(&button) else { // 【Rust 语法】let-else：按钮名转枚举，失败则走 else
        return MappingView::none(); // 按钮名无效则返回无映射视图
    }; // let-else 结束
    let core = state.shared.core.lock().unwrap(); // 对核心加锁
    let Some(layer) = find_layer_ref(&core.steam.profile, &layer_id) else { // 【Rust 语法】let-else：查层失败也走 else
        return MappingView::none(); // 层不存在则返回无映射视图
    }; // let-else 结束
    match layer.get_mapping(b) { // 【Rust 语法】match 匹配 Option：按该按钮的映射结果分支
        Some(m) => MappingView { // 有映射则构造视图
            key_code: m.action.key_code, // 键盘键码（存储 Android KeyCode 常量，运行时再转换为 Windows 虚拟键码）
            kind: kind_str(&m.action), // 动作类型字符串
            desc: describe_action(&m.action), // 动作描述
            subs: m.sub_commands.iter().map(|&k| key_code_to_name(k)).collect(), // 【Rust 语法】闭包 |&k| 解构引用；子命令键码转名称
            sub_on_layer_hold: m.sub_on_layer_hold, // 切层保持开关状态
            has_mapping: true, // 标记有映射
            mouse_button: Some(m.action.mouse_button), // 鼠标键（Some 表示有，None 表示无）
            layer_name: m.action.layer_name.clone(), // 目标层名称（切换层动作时使用）
            long_press: m.long_press.as_ref().map(|lp| action_view(lp, &m.long_press_subs)), // 长按动作视图
            long_press_ms: m.long_press_ms, // 长按阈值
            state_conditions: m
                .state_conditions
                .iter()
                .map(|c| StateCondView {
                    state_id: c.state_id.clone(),
                    expected: c.expected,
                })
                .collect(), // 状态条件列表
            state_logic: m.state_logic.as_str().to_string(), // or / and
        }, // 视图构造结束
        None => MappingView::none(), // 无映射则返回空视图
    } // match 结束
} // 函数结束

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn set_mapping( // 写入按键映射命令（多参数，含多个可选参数）
    state: State<'_, AppState>, // 共享状态
    layer_id: String, // 目标层 id
    button: String, // 按钮名
    kind: String, // 动作类型字符串
    key_code: Option<i32>, // 【Rust 语法】Option<i32>：可空的整数（键盘键码）
    mouse_button: Option<String>, // 可空的鼠标按钮名
    layer_name: Option<String>, // 可空的切换层目标名
) { // 参数表结束，函数无返回值
    let Some(b) = controller_button_from_name(&button) else { // 按钮名转枚举
        return; // 无效则直接返回
    }; // let-else 结束
    let Some(action) = build_action(&kind, key_code, mouse_button, layer_name) else { // 【Rust 语法】let-else：动作构建失败则返回
        return; // 无有效动作则直接返回
    }; // let-else 结束
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    if let Some(layer) = find_layer_mut(&mut core.steam.profile, &layer_id) { // 【Rust 语法】if-let：可变借用配置查找层
        // 写入主动作时保留已有子命令与长按配置（长按独立配置，不因更换主动作而丢失）
        let main_code = action.key_code; // 先取出键码，避免下面与动作移动冲突
        if let Some(m) = layer.button_mappings.get_mut(&b) { // 已存在映射则原地更新
            m.action = action; // 只替换主动作
            m.sub_commands.retain(|k| *k != main_code); // 子命令与主动作重复时移除
            core.profile_rev += 1; // 配置版本号自增，通知前端刷新
        } else { // 无映射则新建
            layer.button_mappings.insert(b, KeyMapping::new(action)); // 【Rust 语法】HashMap::insert：写入该按钮映射
            core.profile_rev += 1; // 配置版本号自增
        } // else 结束
    } // if-let 结束
} // 函数结束

// 状态条件配置：写入某按键映射的生效条件（conditions 为空表示始终生效）
#[tauri::command]
pub fn set_state_conditions(
    state: State<'_, AppState>,
    layer_id: String,
    button: String,
    conditions: Vec<StateCondView>,
    logic: String,
) {
    let Some(b) = controller_button_from_name(&button) else {
        return; // 按钮名无效
    };
    let mut core = state.shared.core.lock().unwrap();
    if let Some(layer) = find_layer_mut(&mut core.steam.profile, &layer_id) {
        if let Some(m) = layer.button_mappings.get_mut(&b) {
            m.state_conditions = conditions
                .into_iter()
                .map(|c| StateCond {
                    state_id: c.state_id,
                    expected: c.expected,
                })
                .collect();
            m.state_logic = StateLogic::from_str(&logic);
            core.profile_rev += 1;
        }
    }
}

// 长按动作配置：kind 为 None 表示取消长按
#[tauri::command]
pub fn set_long_press(
    state: State<'_, AppState>,
    layer_id: String,
    button: String,
    kind: Option<String>,
    key_code: Option<i32>,
    mouse_button: Option<String>,
    layer_name: Option<String>,
) {
    let Some(b) = controller_button_from_name(&button) else {
        return; // 按钮名无效
    };
    let action = kind.as_deref().and_then(|k| build_action(k, key_code, mouse_button, layer_name));
    let mut core = state.shared.core.lock().unwrap();
    if let Some(layer) = find_layer_mut(&mut core.steam.profile, &layer_id) {
        if let Some(m) = layer.button_mappings.get_mut(&b) { // 长按依附于已有主动作
            let same_kind = match (&m.long_press, &action) { // 长按类型是否未变
                (Some(old), Some(new)) => old.r#type == new.r#type, // 类型相同则保留子命令
                _ => false, // 新建或取消时清空子命令
            };
            if let Some(a) = action {
                m.long_press = Some(a); // 写入长按动作
                if !same_kind {
                    m.long_press_subs.clear(); // 类型变化时清空长按子命令
                }
            } else {
                m.long_press = None; // 取消长按
                m.long_press_subs.clear(); // 清理长按子命令
            }
            core.profile_rev += 1;
        }
    }
} // 函数结束

// 设置长按判定阈值（毫秒，限幅 100~5000）
#[tauri::command]
pub fn set_long_press_ms(state: State<'_, AppState>, layer_id: String, button: String, ms: u32) {
    let Some(b) = controller_button_from_name(&button) else {
        return;
    };
    let mut core = state.shared.core.lock().unwrap();
    if let Some(layer) = find_layer_mut(&mut core.steam.profile, &layer_id) {
        if let Some(m) = layer.button_mappings.get_mut(&b) {
            m.long_press_ms = ms.clamp(100, 5000); // 阈值限幅
            core.profile_rev += 1;
        }
    }
} // 函数结束

// 切换长按动作的子命令（仅长按已配置时有效）
#[tauri::command]
pub fn toggle_long_press_sub(state: State<'_, AppState>, layer_id: String, button: String, key_code: i32) {
    let Some(b) = controller_button_from_name(&button) else {
        return;
    };
    let mut core = state.shared.core.lock().unwrap();
    if let Some(layer) = find_layer_mut(&mut core.steam.profile, &layer_id) {
        if let Some(m) = layer.button_mappings.get_mut(&b) {
            if m.long_press.is_some() {
                if let Some(pos) = m.long_press_subs.iter().position(|&k| k == key_code) {
                    m.long_press_subs.remove(pos); // 已存在则移除（取消勾选）
                } else if m.long_press_subs.len() < KeyMapping::MAX_SUB_COMMANDS {
                    m.long_press_subs.push(key_code); // 未满则追加
                }
                core.profile_rev += 1;
            }
        }
    }
} // 函数结束

// ---------------------------------------------------------------------
// 目标游戏（WoW）exe 选择
// ---------------------------------------------------------------------
/// 弹出系统文件选择对话框，返回选中的 exe 完整路径
fn pick_exe_dialog() -> Option<String> {
    use std::mem::size_of;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OPENFILENAMEW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY,
        OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
    };

    let mut buf = [0u16; 1024]; // 存放用户选择的路径
    // 过滤器字符串：成对出现（显示名 \0 通配符 \0），最后以空串结尾
    let filter: Vec<u16> = "可执行文件 (*.exe)\0*.exe\0所有文件 (*.*)\0*.*\0\0"
        .encode_utf16()
        .collect();
    let title: Vec<u16> = "选择游戏可执行文件".encode_utf16().chain([0u16]).collect();

    let mut ofn = OPENFILENAMEW::default();
    ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    ofn.lpstrFilter = PCWSTR(filter.as_ptr());
    ofn.lpstrFile = PWSTR(buf.as_mut_ptr());
    ofn.nMaxFile = buf.len() as u32;
    ofn.lpstrTitle = PCWSTR(title.as_ptr());
    ofn.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_HIDEREADONLY;

    if !unsafe { GetOpenFileNameW(&mut ofn) }.as_bool() {
        return None; // 用户取消
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
} // 函数结束

/// 写入目标 exe 并维护历史记录（最近使用优先，最多 10 条）
fn apply_wow_exe(core: &mut crate::core::AppCore, path: &str) {
    let gs = &mut core.steam.profile.global_settings;
    gs.wow_exe = path.to_string();
    gs.wow_exe_history.retain(|p| p != path); // 去重
    gs.wow_exe_history.insert(0, path.to_string()); // 最近使用排最前
    gs.wow_exe_history.truncate(10); // 限制条数
    core.profile_rev += 1;
} // 函数结束

// 浏览选择 WoW exe（成功后自动写入并加入历史）
#[tauri::command]
pub fn pick_wow_exe(state: State<'_, AppState>) -> Option<String> {
    let path = pick_exe_dialog()?; // 弹出对话框；取消则返回 None
    {
        let mut core = state.shared.core.lock().unwrap();
        apply_wow_exe(&mut core, &path);
    }
    state.shared.sync_state_config(); // 同步给像素读取线程（按新 exe 定位游戏窗口）
    Some(path)
} // 函数结束

// 从历史记录中选择 WoW exe
#[tauri::command]
pub fn set_wow_exe(state: State<'_, AppState>, path: String) {
    {
        let mut core = state.shared.core.lock().unwrap();
        apply_wow_exe(&mut core, &path);
    }
    state.shared.sync_state_config(); // 同步给像素读取线程（按新 exe 定位游戏窗口）
} // 函数结束

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn clear_mapping(state: State<'_, AppState>, layer_id: String, button: String) { // 清空按键映射命令
    let Some(b) = controller_button_from_name(&button) else { // 按钮名转枚举
        return; // 无效则返回
    }; // let-else 结束
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    if let Some(layer) = find_layer_mut(&mut core.steam.profile, &layer_id) { // 可变借用查找层
        layer.button_mappings.remove(&b); // 删除该按钮的映射
        core.profile_rev += 1; // 配置版本号自增
    } // if-let 结束
} // 函数结束

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn toggle_sub(state: State<'_, AppState>, layer_id: String, button: String, key_code: i32) { // 切换子命令（组合键）命令
    let Some(b) = controller_button_from_name(&button) else { // 按钮名转枚举
        return; // 无效则返回
    }; // let-else 结束
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    if let Some(layer) = find_layer_mut(&mut core.steam.profile, &layer_id) { // 可变借用查找层
        if let Some(m) = layer.button_mappings.get_mut(&b) { // 【Rust 语法】get_mut：可变借用该按钮的映射
            // 【Rust 语法】position：闭包找首个匹配索引，返回 Option<usize>
            if let Some(pos) = m.sub_commands.iter().position(|&k| k == key_code) { // 子命令已存在
                m.sub_commands.remove(pos); // 移除该子命令（取消）
            } else if m.sub_commands.len() < KeyMapping::MAX_SUB_COMMANDS { // 不存在且未满
                m.sub_commands.push(key_code); // 追加该子命令
            } // else-if 结束
        } // 内层 if-let 结束
        core.profile_rev += 1; // 配置版本号自增
    } // 外层 if-let 结束
} // 函数结束

/// 设置「切层保持」开关：切到本层时是否把该键点击的子命令修饰键一起按下保持。
/// 仅作用于点击设置，不影响长按设置。
#[tauri::command]
pub fn set_sub_on_layer_hold(
    state: State<'_, AppState>,
    layer_id: String,
    button: String,
    on: bool,
) {
    let Some(b) = controller_button_from_name(&button) else { // 按钮名转枚举
        return; // 无效则返回
    }; // let-else 结束
    let mut core = state.shared.core.lock().unwrap(); // 加锁取可变访问
    if let Some(layer) = find_layer_mut(&mut core.steam.profile, &layer_id) { // 可变借用查找层
        if let Some(m) = layer.button_mappings.get_mut(&b) { // 可变借用该按钮的映射
            m.sub_on_layer_hold = on; // 写入开关状态
            core.profile_rev += 1; // 配置版本号自增
        } // 内层 if-let 结束
    } // 外层 if-let 结束
} // 函数结束

//
#[tauri::command]
pub fn open_app(app: AppHandle, _: State<'_, AppState>) {
    // 【Rust 语法】let-else 语句：取到 overlay 窗口则绑定 win，否则走 else 分支
    let Some(win) = app.get_webview_window("main") else { // 按标签查找 main 窗口
        return; // 窗口不存在则返回
    }; // let-else 结束
    // 【Rust 语法】主窗口可能被最小化：show 不会取消最小化，须先 unminimize 恢复
    if win.is_minimized().unwrap_or(false) { // 判断窗口是否处于最小化状态（失败按非最小化处理）
        win.unminimize().unwrap(); // 取消最小化，恢复窗口显示
    }
    win.show().unwrap(); // 显示窗口
    win.set_focus().unwrap(); // 设置焦点
}

#[tauri::command]
pub fn rename_layer(state: State<'_, AppState>, layer_id: String, layer_name: String) {
    let mut core = state.shared.core.lock().unwrap();
    let layer = find_layer_mut(&mut core.steam.profile, &layer_id).unwrap();
    layer.name = layer_name;
    core.profile_rev += 1;
}

// 【Rust 语法】属性宏 #[tauri::command]：注册为 Tauri IPC 命令
#[tauri::command]
pub fn change_layer_trigger_button(state: State<'_, AppState>, layer_id: String, button_name: String) { // 修改层的「激活按钮」命令
    // 语义：在公共层写入一条「切层」映射（公共层的 button_name 按钮 -> 切到 layer_id 对应层），
    // 与 get_layer_edit_snapshot 中扫描触发按键的规则保持一致。
    // 注意：这里不能先 `let core = state.shared.core.lock()` 再调用 set_mapping：
    //   1) 先加锁会借用 state，之后把 state 移动进 set_mapping 会报 E0505
    //      （cannot move out of `state` because it is borrowed）；
    //   2) std::sync::Mutex 不可重入，重入加锁会直接死锁。
    // 故下方的清理逻辑放在独立块内（块结束即释放锁），随后再把 state 移动给 set_mapping。

    // 公共层恒为激活层（steam_input::activate_layer 会忽略 "Common"），不存在「激活按钮」：
    // 直接返回，避免把公共层上该按键原本的映射覆盖成一条永不生效的切层动作。
    if layer_id.is_empty() || layer_id == "Common" { // 非法目标（空 / 公共层）
        return; // 不做任何修改
    } // 非法目标分支结束

    // 1) 先清除公共层中指向本层的旧「切层」映射：前端下拉框是单选控件，同一层只应保留一个激活按钮；
    //    否则改选后旧按钮仍能激活本层，且 get_layer_edit_snapshot 依赖 HashMap 迭代顺序扫描，回显不确定。
    // 【Rust 语法】块表达式：锁在块内使用，块结束自动释放锁后再调用 set_mapping
    {
        let mut core = state.shared.core.lock().unwrap(); // 对核心加锁（仅在本块内持有）
        // 旧配置可能按「层名称」写入切层映射，故同时匹配 id 与 name
        let target_name = find_layer_ref(&core.steam.profile, &layer_id) // 查目标层（不可变借用）
            .map(|l| l.name.clone()) // 命中则克隆名称（避免与后续可变借用冲突）
            .unwrap_or_default(); // 层不存在则空串（仅按 id 匹配）
        // 【Rust 语法】let-else 语句：取到公共层则绑定 common，否则执行 else 分支
        let Some(common) = find_layer_mut(&mut core.steam.profile, "Common") else { // 取公共层可变引用
            return; // 无公共层（异常配置）则放弃
        }; // let-else 结束
        // 【Rust 语法】HashMap::retain(闭包)：闭包返回 true 的项保留，false 的项移除；此处移除指向本层的切层映射
        common.button_mappings.retain(|_, m| {
            !(matches!( // 动作是「切层」或「切层+按键」
                m.action.r#type,
                ActionType::SwitchLayer | ActionType::SwitchLayerAndKey
            )
                && matches!( // 且目标层就是本层（id 或名称命中）
                    m.action.layer_name.as_deref(),
                    Some(n) if n == layer_id.as_str() || n == target_name.as_str()
                ))
        }); // retain 结束
        core.profile_rev += 1; // 配置版本号自增，通知前端刷新
    } // 块结束，锁在此释放

    // 2) 写入新的激活按钮映射
    set_mapping( // 复用「写入按键映射」命令（内部自带加锁与 profile_rev 自增）
        state, // 共享状态（按值移动，此处未持有任何借用）
        "Common".to_string(), // 映射写在公共层（激活按钮由公共层驱动切层）
        button_name, // 触发按钮名
        "switchlayer".to_string(), // 动作类型：切换层
        None, // 键盘键码（切层动作不需要）
        None, // 鼠标键（切层动作不需要）
        Some(layer_id), // 切层目标：被编辑的层 id
    ); // set_mapping 调用结束
} // 函数结束