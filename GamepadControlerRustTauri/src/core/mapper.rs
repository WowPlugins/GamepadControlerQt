// =====================================================================
// mapper.rs —— 键鼠映射器
//
// 等效 Qt 版 KeyboardMouseMapper.h/.cpp。
// 职责：
//   - 执行 SteamInput 分发的按钮/摇杆事件，转换为键鼠注入
//   - 按钮映射：键盘（含子命令组合键）、鼠标单击、鼠标长按锁存（MouseToggle）
//   - 长按：按住超过阈值后释放短按动作、改执行长按动作
//   - 交互键 / 释放鼠标锁存 / 交换鼠标左右键锁存 / 鼠标精细移动
//   - 松开时按「已注入状态」精确释放（不依赖当前层映射，避免切层卡死）
//   - 左摇杆 -> WASD 8 方向移动（阈值 0.5），并按需拦截直发 WoW
//   - 右摇杆 -> 固定 125Hz（LOOK_TICK_MS=8ms）平滑视角移动循环
//
// 线程模型：
//   - 手柄轮询线程：handle_button / handle_stick / check_long_press 直接执行
//     （与 UI 线程通过 AppCore 的同一把锁串行化）
//   - UI 线程：release_all_inputs（stop / 前台切换 / 手柄断开）
//   - look 线程：独立 std::thread，固定节拍读取右摇杆原子量并注入鼠标移动
// =====================================================================

// 【Rust 语法】use 导入语句：导入本 crate 内 core 模块下的 InputInjector（注入器接口，封装 Windows SendInput）。
use crate::core::injector::InputInjector;
// 【Rust 语法】use 花括号导入：从同一模块按需导入多个符号（android_key 键码常量、手柄按钮/摇杆/鼠标键枚举）。
use crate::core::input_types::{android_key, ControllerButton, ControllerStick, MouseButton};
// 【Rust 语法】use 花括号导入：导入映射引擎的动作类型枚举 ActionType 与按键映射结构体 KeyMapping。
use crate::core::mapping_types::{ActionType, KeyMapping, MappedAction};
// 【Rust 语法】use 导入语句：导入 UI 事件类型 UiEvent（用于向 UI 线程发通知）。
use crate::core::UiEvent;
// 【Rust 语法】use 导入语句：导入按 exe 定位窗口并投递键盘消息的辅助模块。
use crate::core::wow_sender;
// 【Rust 语法】use 嵌套导入：HashMap（键值映射）与 HashSet（不重复元素集合）。
use std::collections::{HashMap, HashSet};
// 【Rust 语法】use 嵌套导入：原子类型 AtomicBool/AtomicU32 与内存序 Ordering，用于多线程无锁共享数据。
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
// 【Rust 语法】use 导入语句：mpsc（多生产者单消费者）通道的发送端 Sender，用于向 UI 线程发送事件。
use std::sync::mpsc::Sender;
// 【Rust 语法】use 导入语句：Arc<T> 原子引用计数智能指针，让多个线程共享同一份数据的所有权。
use std::sync::Arc;
// 【Rust 语法】use 导入语句：线程模块，用于创建/管理后台线程（std::thread）。
use std::thread;
// 【Rust 语法】use 嵌套导入：Duration 时间段、Instant 时刻（用于计时与固定节拍控制）。
use std::time::{Duration, Instant};

// ---- 视角控制常量 ----
// 【Rust 语法】const 定义编译期常量：全大写命名，类型标注为 f32（单精度浮点数）。
const LOOK_SPEED_PX_PER_SEC: f32 = 480.0; // 满幅摇杆每秒像素位移
const LOOK_SMOOTH_TAU_MAX: f32 = 0.048; // 最大时间常数（smoothing=1 时）
const LOOK_TICK_MS: u64 = 8; // 节拍周期（125Hz）
/// 精细移动时右摇杆灵敏度的缩放系数（需求7）
const FINE_LOOK_SCALE: f32 = 0.25;
/// 交互键点击的间隔（毫秒），给游戏识别按下/抬起留出时间
const INTERACTION_CLICK_GAP_MS: u64 = 15;
/// 解锁鼠标长按后的等待时间（毫秒）：让游戏先处理完抬起事件，再做移动光标/点击等后续操作
const UNLOCK_SETTLE_MS: u64 = 50;

// ---------------------------------------------------------------------
// LookState —— 右摇杆原子状态（look 线程读取，主线程/手柄线程写入）
// 使用 AtomicU32 存储 f32 位模式（Rust 无原生 AtomicF32）。
// ---------------------------------------------------------------------
// 【Rust 语法】struct 结构体：不派生任何 trait；字段全为原子类型（内部可变，无需 &mut 也能跨线程修改）。
pub struct LookState {
    pub latest_x: AtomicU32, // 【Rust 语法】AtomicU32：原子无符号 32 位整数，多线程可无锁安全读写；此处存右摇杆最新 X 的位模式
    pub latest_y: AtomicU32, // 右摇杆最新 Y 值（f32 的位模式）
    pub sensitivity: AtomicU32, // 视角灵敏度（位模式存储）
    pub smoothing: AtomicU32, // 平滑度（位模式存储）
    pub acceleration: AtomicU32, // 加速度（位模式存储）
    pub running: AtomicBool, // 【Rust 语法】AtomicBool：原子布尔，标记 look 线程是否在运行
    /// 精细移动开关（需求7：按住映射了「鼠标精细移动」的手柄键时为 true）
    pub fine_look: AtomicBool,
} // 结束 LookState 结构体定义

// 【Rust 语法】自由函数（不在 impl 内，属于模块级函数）：把 f32 以位模式写入 AtomicU32。
fn store_f32(a: &AtomicU32, v: f32) {
    // 【Rust 语法】store：写入原子值；`v.to_bits()` 把 f32 转为 u32 位模式；Ordering::Relaxed 为最宽松内存序（无跨线程顺序保证需求）。
    a.store(v.to_bits(), Ordering::Relaxed);
} // 结束 store_f32 函数
// 【Rust 语法】自由函数：从 AtomicU32 读出位模式并还原为 f32。
fn load_f32(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed)) // 【Rust 语法】load 读取原子值；from_bits 把 u32 位模式还原为 f32
} // 结束 load_f32 函数

// 【Rust 语法】为 LookState 实现 Default trait（特性）：提供一套默认实例（手动实现，因原子类型无法自动派生默认值）。
impl Default for LookState {
    fn default() -> Self { // 实现 Default trait 要求的 default 方法
        Self { // 构造 LookState 默认实例
            latest_x: AtomicU32::new(0.0f32.to_bits()), // 【Rust 语法】AtomicU32::new(初值) 创建原子量；0.0f32 浮点字面量转位模式
            latest_y: AtomicU32::new(0.0f32.to_bits()), // Y 初始 0
            sensitivity: AtomicU32::new(0.5f32.to_bits()), // 灵敏度默认 0.5
            smoothing: AtomicU32::new(0.5f32.to_bits()), // 平滑度默认 0.5
            acceleration: AtomicU32::new(1.5f32.to_bits()), // 加速度默认 1.5
            running: AtomicBool::new(false), // 初始标记为未运行
            fine_look: AtomicBool::new(false), // 初始未进入精细移动
        } // 结束 Self 结构体字面量
    } // 结束 default 函数
} // 结束 impl Default for LookState 块

// 【Rust 语法】impl 块：为 LookState 实现方法。
impl LookState {
    // 【Rust 语法】方法：`&self` 不可变借用即可（原子字段具备内部可变性，无需 &mut）。
    pub fn update_settings(&self, sensitivity: f32, smoothing: f32, acceleration: f32) {
        store_f32(&self.sensitivity, sensitivity); // 写入新的灵敏度
        store_f32(&self.smoothing, smoothing); // 写入新的平滑度
        store_f32(&self.acceleration, acceleration); // 写入新的加速度
    } // 结束 update_settings 函数

    /// 设置/清除精细移动状态
    pub fn set_fine_look(&self, on: bool) {
        self.fine_look.store(on, Ordering::Relaxed);
    } // 结束 set_fine_look 函数
} // 结束 impl LookState 块

// ---------------------------------------------------------------------
// PendingLongPress —— 待判定的长按
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(Clone)]：长按动作需要复制到待判定表中，故派生 Clone。
#[derive(Clone)]
struct PendingLongPress {
    started: Instant,          // 按下时刻
    threshold_ms: u32,         // 判定阈值（毫秒）
    action: MappedAction,      // 满足阈值后要执行的长按动作
    subs: Vec<i32>,            // 长按动作的子命令键码
    layer_id: String,          // 该映射所在的层（释放层内鼠标时使用）
} // 结束 PendingLongPress 结构体

/// 鼠标键是否为左/右键（需求6 只释放左右键锁存）
fn is_left_right(mb: MouseButton) -> bool {
    matches!(mb, MouseButton::Left | MouseButton::Right)
}

/// 按通道发送左摇杆按键：hwnd 有效时直发目标窗口，否则走 SendInput
fn send_stick_key(kc: i32, down: bool, hwnd: Option<isize>, injector: &InputInjector) {
    match hwnd {
        Some(h) => wow_sender::send_key(h, kc, down), // 直发 WoW 窗口消息
        None => {
            if down {
                injector.send_key_down(kc); // 常规注入按下
            } else {
                injector.send_key_up(kc); // 常规注入抬起
            }
        }
    }
}

// ---------------------------------------------------------------------
// MapperState —— 当前注入状态（按下时记录，松开时按状态精确释放）
// 与 SteamInput 位于同一把锁（AppCore）内，避免并发 down/up 不对称。
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(Default)]：自动为所有字段实现默认值（HashMap/HashSet 默认空、f32 默认 0.0，均满足 Default）。
#[derive(Default)]
// 【Rust 语法】struct 结构体：记录各手柄按钮当前已注入的键鼠状态，供松开时精确释放。
pub struct MapperState {
    /// 按钮 -> 主键 keyCode
    pub pressed_main_keys: HashMap<ControllerButton, i32>, // 当前按下的主键记录（手柄按钮 -> 键码）
    /// 按钮 -> 已按下的子命令
    pub pressed_sub_keys: HashMap<ControllerButton, Vec<i32>>, // 已按下的组合键子命令记录
    /// 按钮 -> 鼠标键
    pub pressed_mouse_buttons: HashMap<ControllerButton, MouseButton>, // 当前按下的鼠标键记录
    /// 按钮 -> 鼠标单击来源层 id（需求6：释放层时只释放该层内产生的鼠标锁存）
    pub pressed_mouse_layer: HashMap<ControllerButton, String>,
    /// WASD 当前按下的 keyCode
    pub left_stick_pressed_keys: HashSet<i32>, // 左摇杆 WASD 当前按下的键码集合
    /// 左摇杆按键是否正通过 PostMessage 直发目标窗口
    pub stick_keys_via_post: bool,
    /// 长按保持（MouseToggle）的鼠标键：按钮 -> 鼠标键
    pub toggled_mouse_buttons: HashMap<ControllerButton, MouseButton>, // MouseToggle 锁存的鼠标键记录
    /// 按钮 -> 鼠标锁存来源层 id（需求6）
    pub toggled_mouse_layer: HashMap<ControllerButton, String>,
    /// 无按钮来源的额外锁存（交换左右键锁存后产生）
    pub extra_toggled: HashSet<MouseButton>,
    /// WASD 拦截自动锁定的鼠标右键
    pub auto_locked_right: bool,
    /// 自动锁定前保存的光标位置（解锁时还原）
    pub auto_lock_saved_pos: Option<(i32, i32)>,
    /// 待判定的长按：按钮 -> 长按信息
    pub pending_long_press: HashMap<ControllerButton, PendingLongPress>,
    /// 已触发长按的按钮（松开时按长按动作释放）
    pub long_press_buttons: HashSet<ControllerButton>,
    /// 缓存的 WoW 窗口句柄（0 = 无，isize 保证状态可跨线程）
    pub wow_hwnd: isize,
    /// 缓存窗口对应的 exe 路径
    pub wow_hwnd_exe: String,
    /// 平滑状态（仅 look 线程使用）
    pub smoothed_look_x: f32, // 视角 EMA 平滑后的 X 值
    pub smoothed_look_y: f32, // 视角 EMA 平滑后的 Y 值
} // 结束 MapperState 结构体定义

/// 判断是否为修饰键（Alt/Ctrl/Shift）：
/// 组合键注入/释放时修饰键必须「最先按下、最后释放」，否则游戏识别不到组合键。
// 【Rust 语法】模块级私有函数（不在 impl 内，属于模块顶层）；matches! 宏判断 code 是否命中任一修饰键键码。
fn is_modifier_key(code: i32) -> bool {
    matches!(
        code, // 待判断的键码
        android_key::ALT_LEFT // 左 Alt
            | android_key::ALT_RIGHT // 右 Alt
            | android_key::CTRL_LEFT // 左 Ctrl
            | android_key::CTRL_RIGHT // 右 Ctrl
            | android_key::SHIFT_LEFT // 左 Shift
            | android_key::SHIFT_RIGHT // 右 Shift
    ) // matches! 调用结束，其值为 bool
}

// 【Rust 语法】impl 块：为 MapperState 实现方法。
impl MapperState {
    /// 释放全部注入状态（物理按键/鼠标键 + 所有保持状态，含 MouseToggle 锁存）。
    /// 供 stop 和手柄断开时调用，避免 toggle 保持的鼠标键在断开后卡死。
    // 【Rust 语法】方法：`&mut self` 可变借用；参数均用 `&` 引用（借用，不转移所有权）：注入器、事件发送端、视角状态。
    pub fn release_all_inputs(
        &mut self, // 可变借用自身（需要清空记录字段）
        injector: &InputInjector, // 注入器引用（只读）
        event_tx: &Sender<UiEvent>, // 事件通道发送端引用（只读）
        look: &LookState, // 视角状态引用（只读，用于清零摇杆值）
    ) { // 结束参数列表，函数体开始
        injector.release_all(); // 让注入器一次性松开所有物理按键/鼠标键
        // 先复制再清空，逐个通知 UI 解除锁存提示
        // 【Rust 语法】drain() 方法：取出并清空 HashMap 的全部键值对并返回迭代器；collect() 把迭代器收集为 Vec（元组向量）。
        let toggled: Vec<(ControllerButton, MouseButton)> =
            self.toggled_mouse_buttons.drain().collect(); // 把全部锁存记录取出并收集为向量
        // 【Rust 语法】for 循环 + 元组解构：每次迭代把 (按钮, 鼠标键) 元组拆解到 button、mb 两个变量。
        for (button, mb) in toggled {
            // 【Rust 语法】`let _ = ...`：显式忽略表达式返回的 Result（发送失败也无需处理）。
            let _ = event_tx.send(UiEvent::MouseToggleChanged { // 通知 UI：该鼠标键的锁存已解除
                button, // 手柄按钮
                mb, // 鼠标键
                active: false, // 标记为非激活（解除锁存）
            }); // 结束事件构造与发送
        } // 结束 for 循环
        self.toggled_mouse_layer.clear(); // 清空锁存来源层记录
        self.extra_toggled.clear(); // 清空无来源锁存（注入器已统一松开）
        self.pressed_mouse_layer.clear(); // 清空鼠标单击来源层记录
        self.auto_locked_right = false; // 清除自动锁定标记
        self.auto_lock_saved_pos = None; // 清除自动锁定前保存的光标位置
        self.pending_long_press.clear(); // 清空待判定长按
        self.long_press_buttons.clear(); // 清空已触发长按记录
        self.stick_keys_via_post = false; // 复位左摇杆投递通道标记
        self.pressed_main_keys.clear(); // 清空主键记录
        self.pressed_sub_keys.clear(); // 清空子命令记录
        self.pressed_mouse_buttons.clear(); // 清空鼠标键记录
        self.left_stick_pressed_keys.clear(); // 清空左摇杆按键记录
        self.toggled_mouse_buttons.clear(); // 清空锁存记录
        look.set_fine_look(false); // 退出精细移动
        store_f32(&look.latest_x, 0.0); // 清零右摇杆 X 值
        store_f32(&look.latest_y, 0.0); // 清零右摇杆 Y 值
        self.smoothed_look_x = 0.0; // 复位平滑 X
        self.smoothed_look_y = 0.0; // 复位平滑 Y
    } // 结束 release_all_inputs 函数

    /// 按「已注入状态」释放某按钮的全部注入（子命令逆序 -> 主键 -> 鼠标）。
    /// 注意：不处理 MouseToggle 锁存（toggle 由用户主动锁存，松开不改变状态）。
    // 【Rust 语法】方法：`&mut self` 可变借用（需要从记录中移除条目）。
    pub fn release_button_injection(&mut self, button: ControllerButton, injector: &InputInjector) {
        // 释放子命令与主键（顺序与注入相反：非修饰子命令 -> 主键 -> 修饰子命令）
        // 【组合键顺序】修饰键（Alt/Ctrl/Shift）必须最后释放，与注入时的"最先按下"对称，
        // 否则 Alt 先于主键松开，游戏（如魔兽）识别不到完整组合键。
        if let Some(subs) = self.pressed_sub_keys.remove(&button) {
            // 【Rust 语法】`subs.iter().rev()`：反向迭代引用；`sub` 为元素引用 &i32，`*sub` 解引用取 i32 值。
            for sub in subs.iter().rev() { // 非修饰子命令先释放
                if !is_modifier_key(*sub) { // 非修饰键
                    injector.send_key_up(*sub); // 逐个发送键抬起事件
                } // 结束 if 判断
            } // 结束 for 循环
            // 释放主键
            if let Some(main) = self.pressed_main_keys.remove(&button) {
                injector.send_key_up(main); // 发送主键抬起
            } // 结束 if let 分支
            // 修饰子命令最后释放
            for sub in subs.iter().rev() { // 修饰键最后释放
                if is_modifier_key(*sub) { // 修饰键
                    injector.send_key_up(*sub); // 逐个发送键抬起事件
                } // 结束 if 判断
            } // 结束 for 循环
        } else if let Some(main) = self.pressed_main_keys.remove(&button) { // 无子命令时直接释放主键
            injector.send_key_up(main); // 发送主键抬起
        } // 结束 if let 分支
        // 释放鼠标（不处理长按保持的）
        if let Some(mb) = self.pressed_mouse_buttons.remove(&button) {
            injector.send_mouse_up(mb); // 发送鼠标键抬起
            self.pressed_mouse_layer.remove(&button); // 清理来源层标记
        } // 结束 if let 分支
    } // 结束 release_button_injection 函数

    /// 按钮命中映射：按下执行动作并记录注入状态；松开精确释放。
    // 【Rust 语法】方法：`&mut self` 可变借用；mapping/injector/event_tx 均为引用（只读借用注入器与事件通道）。
    pub fn handle_button(
        &mut self, // 可变借用自身（需要记录/释放注入状态）
        button: ControllerButton, // 触发的手柄按钮
        is_pressed: bool, // 是否按下（true 按下 / false 松开）
        mapping: &KeyMapping, // 命中的按键映射（只读借用）
        layer_id: &str, // 映射所在层 id（需求6：记录鼠标锁存来源）
        injector: &InputInjector, // 注入器引用（只读）
        event_tx: &Sender<UiEvent>, // 事件通道发送端引用（只读）
        look: &LookState, // 视角状态引用（精细移动开关）
    ) { // 结束参数列表，函数体开始
        if !is_pressed { // 松开分支
            let fired_long = self.long_press_buttons.remove(&button); // 该按钮是否已触发过长按
            let had_pending = self.pending_long_press.remove(&button).is_some(); // 是否还有未判定的长按
            // 配置了长按但未超阈值就松开：此时才执行短按动作（按下记录 + 立即释放 = 一次点按），
            // 这样长按时不会先误触发短按。切层动作在按下时已执行，跳过以免重复切层。
            if had_pending && !fired_long && mapping.action.r#type != ActionType::SwitchLayer {
                self.execute_press(button, &mapping.action, &mapping.sub_commands, layer_id, injector, event_tx, look);
            }
            // 精细移动：按「本次实际执行的动作」复位——长按触发时用长按动作类型，否则用主动作类型
            let acted = if fired_long { mapping.long_press.as_ref() } else { Some(&mapping.action) };
            if acted.map_or(false, |a| a.r#type == ActionType::FineLook) {
                look.set_fine_look(false); // 释放时恢复正常灵敏度
            }
            self.release_button_injection(button, injector); // 按已注入状态精确释放
            return; // 提前结束函数
        } // 结束 if 判断

        if let Some(long_action) = &mapping.long_press {
            // 配置了长按动作：按下先不执行短按，等松开（未超阈值）或 Tick 判定超阈值后再决定，
            // 避免长按时先触发一次短按（点击）。
            if mapping.action.r#type == ActionType::SwitchLayer {
                self.execute_press(button, &mapping.action, &mapping.sub_commands, layer_id, injector, event_tx, look); // 切层必须按下立即生效
            }
            let threshold = mapping.long_press_ms.clamp(100, 5000); // 阈值限幅 100~5000ms
            self.pending_long_press.insert(
                button,
                PendingLongPress {
                    started: Instant::now(), // 记录按下时刻
                    threshold_ms: threshold,
                    action: long_action.clone(), // 长按动作
                    subs: mapping.long_press_subs.clone(), // 长按子命令
                    layer_id: layer_id.to_string(), // 来源层
                },
            );
            return; // 短按延后到松手时执行
        }

        // 未配置长按：按下立即执行短按动作
        self.execute_press(button, &mapping.action, &mapping.sub_commands, layer_id, injector, event_tx, look);
    } // 结束 handle_button 函数

    /// 长按检查（由轮询线程每个 Tick 调用）：满足阈值的按钮切换为长按动作
    pub fn check_long_press(
        &mut self,
        injector: &InputInjector,
        event_tx: &Sender<UiEvent>,
        look: &LookState,
    ) {
        if self.pending_long_press.is_empty() { // 无待判定项时直接返回
            return;
        }
        let now = Instant::now(); // 当前时刻
        // 收集已满足阈值的按钮（先收集再处理，避免遍历时修改容器）
        let fired: Vec<ControllerButton> = self
            .pending_long_press
            .iter()
            .filter(|(_, p)| now.duration_since(p.started).as_millis() as u64 >= p.threshold_ms as u64)
            .map(|(b, _)| *b)
            .collect();
        for button in fired { // 逐个切换为长按动作
            if let Some(p) = self.pending_long_press.remove(&button) {
                self.release_button_injection(button, injector); // 先释放按下时已注入的状态（配置长按后短按未注入，通常无内容）
                self.long_press_buttons.insert(button); // 标记为长按已触发
                let layer = p.layer_id.clone(); // 先克隆层 id，避免与 &mut self 借用冲突
                self.execute_press(button, &p.action, &p.subs, &layer, injector, event_tx, look); // 执行长按动作
            } // 结束 if let 分支
        } // 结束 for 循环
    } // 结束 check_long_press 函数

    /// 执行一次按下动作（短按与长按共用）
    fn execute_press(
        &mut self,
        button: ControllerButton,
        action: &MappedAction,
        subs: &[i32],
        layer_id: &str,
        injector: &InputInjector,
        event_tx: &Sender<UiEvent>,
        look: &LookState,
    ) {
        // 【Rust 语法】match 模式匹配：按动作类型分发到对应处理逻辑。
        match action.r#type {
            ActionType::KeyboardKey => { // 键盘按键动作
                self.handle_keyboard_key(button, action.key_code, subs, injector); // 处理组合键按下（主键+子命令）
            } // 结束键盘按键分支
            ActionType::MouseClick => { // 鼠标单击动作
                self.handle_mouse_click(button, action.mouse_button, layer_id, injector); // 处理鼠标按下
            } // 结束鼠标单击分支
            ActionType::MouseToggle => { // 鼠标长按锁存动作
                self.handle_mouse_toggle(button, action.mouse_button, layer_id, injector, event_tx); // 处理锁存切换
            } // 结束鼠标长按锁存分支
            ActionType::WheelUp => self.handle_wheel(button, 1, subs, injector), // 滚轮上滚（支持子命令组合键，如 Alt+滚轮）
            ActionType::WheelDown => self.handle_wheel(button, -1, subs, injector), // 滚轮下滚（支持子命令组合键）
            ActionType::InteractionKey => self.handle_interaction_key(injector), // 交互键（需求1）
            ActionType::ReleaseMouseToggle => self.handle_release_mouse_toggle(injector, event_tx), // 释放鼠标锁存（需求4）
            ActionType::SwapMouseToggle => self.handle_swap_mouse_toggle(injector, event_tx), // 交换左右键锁存（需求5）
            ActionType::FineLook => look.set_fine_look(true), // 精细移动：按下进入（需求7）
            // 【Rust 语法】模式 `|`（或）匹配：多个变体共享同一分支代码。
            ActionType::SwitchLayer // 切换层动作
            | ActionType::ToggleMapping // 切换映射
            | ActionType::ToggleOnScreenKeyboard // 切换屏幕键盘
            | ActionType::ToggleOverlay => {} // 由 SteamInput 引擎处理，此处不注入
            ActionType::MouseMove | ActionType::LookAround => {} // 摇杆动作在 handle_stick 中处理
        } // 结束 match 表达式
    } // 结束 execute_press 函数

    /// 键盘映射：按下组合键。
    /// 【组合键顺序】无论配置中修饰键（Alt/Ctrl/Shift）是主键还是子命令，
    /// 统一「修饰键最先按下 -> 主键 -> 非修饰子命令」，与真实键盘一致：
    /// 先按修饰键再按主键，游戏（如魔兽）才能识别为组合键（如 Alt+3）。
    // 【Rust 语法】私有方法（无 pub 修饰）：`&[i32]` 是切片类型，可看作对数组/向量元素的借用视图。
    fn handle_keyboard_key(
        &mut self, // 可变借用自身（需要记录主键/子命令按下状态）
        button: ControllerButton, // 触发的手柄按钮
        main_key_code: i32, // 主键键码
        subs: &[i32], // 子命令键码切片（只读借用）
        injector: &InputInjector, // 注入器引用（只读）
    ) { // 结束参数列表，函数体开始
        if self.pressed_main_keys.contains_key(&button) { // 若该按钮的主键已按下
            return; // 已按下，忽略重复
        } // 结束 if 判断
        // 【Rust 语法】`subs.len().min(...)`：取长度与上限二者中的较小值（限制子命令数量）。
        let n = subs.len().min(KeyMapping::MAX_SUB_COMMANDS);
        // 【Rust 语法】Vec::with_capacity(n)：预分配容量避免多次扩容；`let mut` 声明可变绑定。
        let mut valid_subs: Vec<i32> = Vec::with_capacity(n);
        // 【Rust 语法】`0..n` 范围表达式：生成从 0 到 n-1 的整数序列，for 逐个迭代。
        for i in 0..n {
            if subs[i] == main_key_code { // 若子命令与主键相同
                continue; // 避免子命令与主键重复，跳过该索引
            } // 结束 if 判断
            valid_subs.push(subs[i]); // 收集有效的子命令键码
        } // 结束 for 循环
        // ① 修饰子命令最先按下（Alt/Ctrl/Shift 优先于主键）
        for &sub in &valid_subs { // 遍历修饰子命令
            if is_modifier_key(sub) { // 是修饰键
                injector.send_key_down(sub); // 最先按下（形成组合键的修饰部分）
            } // 结束 if 判断
        } // 结束 for 循环
        injector.send_key_down(main_key_code); // ② 再按下主键（若主键本身是修饰键，此时修饰键也已排在最前）
        // ③ 非修饰子命令最后按下
        for &sub in &valid_subs { // 遍历非修饰子命令
            if !is_modifier_key(sub) { // 非修饰键
                injector.send_key_down(sub); // 最后按下
            } // 结束 if 判断
        } // 结束 for 循环
        self.pressed_main_keys.insert(button, main_key_code); // 记录主键已按下
        self.pressed_sub_keys.insert(button, valid_subs); // 记录子命令已按下
    } // 结束 handle_keyboard_key 函数

    /// 滚轮滚动：支持子命令组合键（如 Alt+滚轮）。
    /// 【组合键顺序】与 handle_keyboard_key 一致：修饰键（Alt/Ctrl/Shift）最先按下，
    /// 保证游戏识别到组合键；滚轮为瞬时事件，松开时由 release_button_injection 释放子命令。
    // 【Rust 语法】私有方法：`&mut self` 可变借用（需记录子命令按下状态）。
    fn handle_wheel(
        &mut self, // 可变借用自身（需要记录子命令按下状态）
        button: ControllerButton, // 触发的手柄按钮
        dir: i32, // 滚动方向（1 上滚 / -1 下滚）
        subs: &[i32], // 子命令键码切片（只读借用）
        injector: &InputInjector, // 注入器引用（只读）
    ) { // 结束参数列表，函数体开始
        if self.pressed_sub_keys.contains_key(&button) { // 该按钮的子命令已在按住中
            return; // 忽略重复（滚轮为瞬时事件，按下一次滚动一格）
        } // 结束 if 判断
        // 【Rust 语法】`subs.len().min(...)`：取长度与上限二者中的较小值（限制子命令数量）。
        let n = subs.len().min(KeyMapping::MAX_SUB_COMMANDS);
        // 【Rust 语法】`subs[..n].to_vec()`：切片取前 n 个元素并复制为 Vec<i32>（子命令键码列表）。
        let valid_subs: Vec<i32> = subs[..n].to_vec(); // 收集子命令键码
        // ① 修饰子命令最先按下（Alt/Ctrl/Shift 优先于滚轮事件）
        for &sub in &valid_subs { // 遍历修饰子命令
            if is_modifier_key(sub) { // 是修饰键
                injector.send_key_down(sub); // 最先按下（形成组合键的修饰部分）
            } // 结束 if 判断
        } // 结束 for 循环
        injector.send_mouse_wheel(dir); // ② 发送滚轮事件（此时修饰键已按下）
        // ③ 非修饰子命令后按下
        for &sub in &valid_subs { // 遍历非修饰子命令
            if !is_modifier_key(sub) { // 非修饰键
                injector.send_key_down(sub); // 后按下
            } // 结束 if 判断
        } // 结束 for 循环
        self.pressed_sub_keys.insert(button, valid_subs); // 记录子命令已按下（松开时自动释放）
    } // 结束 handle_wheel 函数

    /// 鼠标单击：按下/松开跟随手柄
    // 【Rust 语法】私有方法：`&mut self` 可变借用。
    fn handle_mouse_click(
        &mut self, // 可变借用自身（需要记录鼠标键按下状态）
        button: ControllerButton, // 触发的手柄按钮
        mb: MouseButton, // 要按下的鼠标键
        layer_id: &str, // 映射所在层 id
        injector: &InputInjector, // 注入器引用（只读）
    ) { // 结束参数列表，函数体开始
        if self.pressed_mouse_buttons.contains_key(&button) { // 若该按钮的鼠标键已按下
            return; // 已按下则忽略重复
        } // 结束 if 判断
        injector.send_mouse_down(mb); // 注入鼠标键按下
        self.pressed_mouse_buttons.insert(button, mb); // 记录按下状态
        self.pressed_mouse_layer.insert(button, layer_id.to_string()); // 记录来源层（需求6）
    } // 结束 handle_mouse_click 函数

    /// 鼠标长按锁存（MouseToggle）：每次按下切换保持状态。
    /// 松开手柄键时 release_button_injection 不处理该记录（保持锁存状态）。
    // 【Rust 语法】私有方法：`&mut self` 可变借用；event_tx 为事件通道发送端引用。
    fn handle_mouse_toggle(
        &mut self, // 可变借用自身（需要读写锁存记录）
        button: ControllerButton, // 触发的手柄按钮
        mb: MouseButton, // 要锁存的鼠标键
        layer_id: &str, // 映射所在层 id
        injector: &InputInjector, // 注入器引用（只读）
        event_tx: &Sender<UiEvent>, // 事件通道发送端引用（只读）
    ) { // 结束参数列表，函数体开始
        if self.toggled_mouse_buttons.contains_key(&button) { // 若当前已锁存，则本次按下为"解除锁存"
            injector.send_mouse_up(mb); // 抬起鼠标键
            self.toggled_mouse_buttons.remove(&button); // 移除锁存记录
            self.toggled_mouse_layer.remove(&button); // 移除来源层记录
            let _ = event_tx.send(UiEvent::MouseToggleChanged { // 通知 UI：锁存已解除
                button, // 手柄按钮
                mb, // 鼠标键
                active: false, // 标记为非激活
            }); // 结束事件构造与发送
        } else { // 否则本次按下为"建立锁存"
            injector.send_mouse_down(mb); // 按下并保持鼠标键（锁存）
            self.toggled_mouse_buttons.insert(button, mb); // 记录锁存状态
            self.toggled_mouse_layer.insert(button, layer_id.to_string()); // 记录来源层（需求6）
            let _ = event_tx.send(UiEvent::MouseToggleChanged { // 通知 UI：已建立锁存
                button, // 手柄按钮
                mb, // 鼠标键
                active: true, // 标记为激活
            }); // 结束事件构造与发送
        } // 结束 else 分支
    } // 结束 handle_mouse_toggle 函数

    // -----------------------------------------------------------------
    // 需求1：交互键
    // -----------------------------------------------------------------
    /// 记录鼠标左右键锁存状态与光标位置 -> 解锁 -> 移到屏幕中央右键点击一次
    /// -> 光标移回原位 -> 恢复锁存。
    fn handle_interaction_key(&mut self, injector: &InputInjector) {
        // a. 记录左右键锁存状态（用户要求：左右键都要判断）与当前光标位置
        let left_locked = self.is_mouse_locked(MouseButton::Left);
        let right_locked = self.is_mouse_locked(MouseButton::Right);
        let (saved_x, saved_y) = injector.get_cursor_pos();

        // b. 已锁存则先解锁（锁存记录保留，最后按同样状态恢复）
        if left_locked {
            injector.send_mouse_up(MouseButton::Left); // 临时松开左键
        }
        if right_locked {
            injector.send_mouse_up(MouseButton::Right); // 临时松开右键
        }
        if left_locked || right_locked {
            thread::sleep(Duration::from_millis(UNLOCK_SETTLE_MS)); // 解锁后等待，让游戏先处理抬起事件
        }
        let (w, h) = injector.screen_size(); // 主屏尺寸
        injector.set_cursor_pos(w / 2, h / 2); // 光标移到屏幕中央
        thread::sleep(Duration::from_millis(INTERACTION_CLICK_GAP_MS)); // 等待光标就位
        injector.send_mouse_down(MouseButton::Right); // 右键按下
        injector.send_mouse_up(MouseButton::Right); // 右键抬起（完成一次点击）
        thread::sleep(Duration::from_millis(INTERACTION_CLICK_GAP_MS)); // 等待点击生效

        // c. 光标移回原位置，再恢复锁存状态
        injector.set_cursor_pos(saved_x, saved_y);
        if left_locked {
            injector.send_mouse_down(MouseButton::Left); // 恢复左键锁存
        }
        if right_locked {
            injector.send_mouse_down(MouseButton::Right); // 恢复右键锁存
        }
    } // 结束 handle_interaction_key 函数

    // -----------------------------------------------------------------
    // 需求4：释放鼠标按键长按锁存
    // -----------------------------------------------------------------
    fn handle_release_mouse_toggle(&mut self, injector: &InputInjector, event_tx: &Sender<UiEvent>) {
        // 释放所有由手柄按钮建立的锁存
        let mut released = false; // 是否实际释放过鼠标锁存（决定是否需要等待）
        let toggled: Vec<(ControllerButton, MouseButton)> = self.toggled_mouse_buttons.drain().collect();
        for (button, mb) in toggled {
            injector.send_mouse_up(mb); // 松开锁存的鼠标键
            let _ = event_tx.send(UiEvent::MouseToggleChanged { button, mb, active: false });
            released = true; // 有锁存被释放
        }
        self.toggled_mouse_layer.clear(); // 清理来源层记录
        // 释放交换/自动产生的额外锁存
        let extras: Vec<MouseButton> = self.extra_toggled.drain().collect();
        for mb in extras {
            injector.send_mouse_up(mb);
            released = true; // 有额外锁存被释放
        }
        released |= self.auto_locked_right; // WASD 拦截自动锁定的右键也算释放
        self.release_auto_right_lock(injector); // 释放 WASD 拦截自动锁定的右键
        if released {
            thread::sleep(Duration::from_millis(UNLOCK_SETTLE_MS)); // 解锁后等待，让游戏先处理抬起事件
        }
        // 需求4：释放锁存后把鼠标移动到屏幕正中央（含 WASD 拦截还原光标之后再次居中）
        let (w, h) = injector.screen_size(); // 主屏尺寸
        injector.set_cursor_pos(w / 2, h / 2); // 光标移到屏幕中央
    } // 结束 handle_release_mouse_toggle 函数

    // -----------------------------------------------------------------
    // 需求5：交换鼠标左右键长按锁存
    // -----------------------------------------------------------------
    fn handle_swap_mouse_toggle(&mut self, injector: &InputInjector, event_tx: &Sender<UiEvent>) {
        // 1) 记录交换前的左右键锁存状态
        let left_locked = self.is_mouse_locked(MouseButton::Left);
        let right_locked = self.is_mouse_locked(MouseButton::Right);
        if !left_locked && !right_locked {
            return; // 都没锁定，无需交换
        }

        // 2) 全部释放现有锁存（含自动锁定，释放后光标还原）
        let toggled: Vec<(ControllerButton, MouseButton)> = self.toggled_mouse_buttons.drain().collect();
        for (_, mb) in &toggled {
            injector.send_mouse_up(*mb);
        }
        let extras: Vec<MouseButton> = self.extra_toggled.drain().collect();
        for mb in extras {
            injector.send_mouse_up(mb);
        }
        self.release_auto_right_lock(injector);

        // 3) 按「交换后的状态」重新锁存：左 <-> 右
        if right_locked {
            injector.send_mouse_down(MouseButton::Left); // 原右键锁存 -> 改为左键
            self.extra_toggled.insert(MouseButton::Left);
        }
        if left_locked {
            injector.send_mouse_down(MouseButton::Right); // 原左键锁存 -> 改为右键
            self.extra_toggled.insert(MouseButton::Right);
        }

        // 4) 原按钮的锁存记录改写成交换后的鼠标键，保证再次按下该按钮能正确解除
        for (button, mb) in toggled {
            let swapped = match mb {
                MouseButton::Left => MouseButton::Right,
                MouseButton::Right => MouseButton::Left,
                other => other,
            };
            self.toggled_mouse_buttons.insert(button, swapped);
            let _ = event_tx.send(UiEvent::MouseToggleChanged {
                button,
                mb: swapped,
                active: true,
            });
        }
    } // 结束 handle_swap_mouse_toggle 函数

    /// 判断某个鼠标键当前是否处于锁存状态（左右键都需要判断）
    fn is_mouse_locked(&self, mb: MouseButton) -> bool {
        if mb == MouseButton::Right && self.auto_locked_right {
            return true; // WASD 拦截自动锁定的右键
        }
        if self.extra_toggled.contains(&mb) {
            return true; // 交换左右键后产生的无来源锁存
        }
        self.toggled_mouse_buttons.values().any(|b| *b == mb) // 由手柄按钮建立的锁存
    } // 结束 is_mouse_locked 函数

    /// 释放 WASD 拦截自动锁定的右键（并还原光标位置）
    fn release_auto_right_lock(&mut self, injector: &InputInjector) {
        if !self.auto_locked_right {
            return; // 未自动锁定
        }
        // 若用户另有右键锁存（手动长按 / 交换产生），不能替用户解锁
        let other_lock = self
            .toggled_mouse_buttons
            .values()
            .any(|b| *b == MouseButton::Right)
            || self.extra_toggled.contains(&MouseButton::Right);
        if other_lock {
            self.auto_locked_right = false; // 仅清除自动标记，保留用户锁存
            self.auto_lock_saved_pos = None;
            return;
        }
        injector.send_mouse_up(MouseButton::Right); // 松开自动锁定的右键
        self.auto_locked_right = false;
        if let Some((x, y)) = self.auto_lock_saved_pos.take() {
            injector.set_cursor_pos(x, y); // 光标还原到锁定前位置
        }
    } // 结束 release_auto_right_lock 函数

    // -----------------------------------------------------------------
    // 需求6：释放某操作层内产生的鼠标左右键锁存
    // -----------------------------------------------------------------
    /// 层停用时调用：仅释放该层内点击/长按过的鼠标左右键。
    pub fn release_layer_mouse(
        &mut self,
        layer_id: &str,
        injector: &InputInjector,
        event_tx: &Sender<UiEvent>,
    ) {
        if layer_id.is_empty() || layer_id == "Common" {
            return; // 公共层不参与释放（公共层的锁存由用户主动解除）
        }
        // 1) 该层内按下未松开的鼠标单击
        let pressed: Vec<ControllerButton> = self
            .pressed_mouse_layer
            .iter()
            .filter(|(_, l)| l.as_str() == layer_id)
            .map(|(b, _)| *b)
            .collect();
        for button in pressed {
            if let Some(mb) = self.pressed_mouse_buttons.get(&button).copied() {
                if is_left_right(mb) {
                    injector.send_mouse_up(mb); // 松开左右键
                    self.pressed_mouse_buttons.remove(&button);
                }
            }
            self.pressed_mouse_layer.remove(&button); // 清理来源层标记
        }
        // 2) 该层内建立的鼠标长按锁存
        let toggled: Vec<(ControllerButton, MouseButton)> = self
            .toggled_mouse_layer
            .iter()
            .filter(|(_, l)| l.as_str() == layer_id)
            .map(|(b, l)| (*b, l.clone()))
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|(b, _)| self.toggled_mouse_buttons.get(&b).map(|mb| (b, *mb)))
            .collect();
        for (button, mb) in toggled {
            if is_left_right(mb) {
                injector.send_mouse_up(mb); // 松开左右键锁存
            }
            self.toggled_mouse_buttons.remove(&button);
            self.toggled_mouse_layer.remove(&button);
            let _ = event_tx.send(UiEvent::MouseToggleChanged {
                button,
                mb,
                active: false,
            });
        }
    } // 结束 release_layer_mouse 函数

    /// 当前锁存状态的展示文案（主窗口 / 悬浮窗使用）
    pub fn lock_summary(&self) -> Option<String> {
        let mut names: Vec<&str> = Vec::new();
        if self.is_mouse_locked(MouseButton::Left) {
            names.push("鼠标左键");
        }
        if self.is_mouse_locked(MouseButton::Right) {
            names.push("鼠标右键");
        }
        if names.is_empty() {
            None
        } else {
            Some(format!("长按锁存: {}", names.join(" + ")))
        }
    } // 结束 lock_summary 函数

    // -----------------------------------------------------------------
    // 左摇杆 WASD / 右摇杆视角
    // -----------------------------------------------------------------
    /// 摇杆处理：
    ///   - 右摇杆：仅记录最新值到原子量（look 线程按固定节拍读取并平滑发送）
    ///   - 左摇杆：WASD 8 方向移动（阈值 0.5），与上一次按键状态做差集
    ///     需求3：WASD 触发时若右键未锁存，则把光标移到屏幕外左下角并锁定右键，
    ///     WASD 直接投递给 WoW 窗口（PostMessage）。
    ///     注意：右键锁存是「永久」的——摇杆回中/WASD 释放都不解锁、不还原光标，
    ///     只有用户主动解除（释放锁存动作/交换锁存/停止映射/退出程序）时才释放。
    // 【Rust 语法】方法：`&mut self` 可变借用（需修改左摇杆按键集合）。
    pub fn handle_stick(
        &mut self, // 可变借用自身（需要维护左摇杆按键集合）
        stick: ControllerStick, // 摇杆标识（左/右）
        x: f32, // 摇杆 X 轴归一化值（-1 ~ 1）
        y: f32, // 摇杆 Y 轴归一化值（-1 ~ 1）
        injector: &InputInjector, // 注入器引用（只读）
        look: &LookState, // 视角状态引用（右摇杆时写入最新值）
        wow_exe: &str, // 目标游戏 exe 路径（WASD 直发用）
    ) { // 结束参数列表，函数体开始
        if stick == ControllerStick::RightStick { // 右摇杆分支
            store_f32(&look.latest_x, x); // 仅记录最新 X 值到原子量
            store_f32(&look.latest_y, y); // 仅记录最新 Y 值到原子量
            return; // 右摇杆不做按键处理，立即返回
        } // 结束右摇杆分支

        // 左摇杆 -> WASD 8 方向（阈值 0.5）
        // 注意：XInput 的 Y 轴向上为正（向上推 => y>0），判定要跟物理方向一致。
        // 【Rust 语法】const 局部常量：在函数体内声明的常量（此处为摇杆判定阈值）。
        const THRESHOLD: f32 = 0.5;
        let up = y > THRESHOLD; // 摇杆向上（y 为正）-> W
        let down = y < -THRESHOLD; // 摇杆向下（y 为负）-> S
        let left = x < -THRESHOLD; // 摇杆向左（x 为负）-> A
        let right = x > THRESHOLD; // 摇杆向右（x 为正）-> D

        let mut target: HashSet<i32> = HashSet::new(); // 本轮目标按键集合（显式类型标注）
        if up { // 摇杆向上
            target.insert(android_key::W); // 向上：加入 W 键
        } // 结束 if 判断
        if down { // 摇杆向下
            target.insert(android_key::S); // 向下：加入 S 键
        } // 结束 if 判断
        if left { // 摇杆向左
            target.insert(android_key::A); // 向左：加入 A 键
        } // 结束 if 判断
        if right { // 摇杆向右
            target.insert(android_key::D); // 向右：加入 D 键
        } // 结束 if 判断

        // 需求3：WASD 触发拦截
        // 排除场景：鼠标左键处于锁存状态（例如按住左键拖拽/交互）时不自动锁右键、不挪光标，
        // WASD 继续走常规 SendInput，避免打断左键操作。
        if !target.is_empty()
            && !self.is_mouse_locked(MouseButton::Right)
            && !self.is_mouse_locked(MouseButton::Left)
        { // 左右键均未锁存才自动锁定右键
            let (w, h) = injector.screen_size();
            self.auto_lock_saved_pos = Some(injector.get_cursor_pos()); // 记录锁定前光标位置
            // 屏幕左下角、屏幕外 2/3 处（系统会把越界坐标钳制到屏幕边缘）
            injector.set_cursor_pos(-(w * 2 / 3), h + h * 2 / 3);
            injector.send_mouse_down(MouseButton::Right); // 锁定鼠标右键
            self.auto_locked_right = true;
        } // 结束拦截分支

        // 右键锁存时把 WASD 直发目标窗口，否则走常规 SendInput
        let hwnd = if self.is_mouse_locked(MouseButton::Right) {
            self.resolve_wow_hwnd(wow_exe)
        } else {
            None
        };
        let via_post = hwnd.is_some();
        // 投递通道变化：先用旧通道松开全部按键，再用新通道重新按下
        if !self.left_stick_pressed_keys.is_empty() && self.stick_keys_via_post != via_post {
            let old_hwnd = if self.stick_keys_via_post {
                Some(self.wow_hwnd)
            } else {
                None
            };
            let old: Vec<i32> = self.left_stick_pressed_keys.drain().collect();
            for kc in old {
                send_stick_key(kc, false, old_hwnd, injector); // 旧通道松开
            }
        }
        self.stick_keys_via_post = via_post;

        // 先收集需要释放的键再统一处理，避免遍历时修改容器
        // 【Rust 语法】迭代器链：iter() 引用迭代 -> copied() 拷贝为值 -> filter(闭包) 过滤 -> collect() 收集为 Vec。
        let to_release: Vec<i32> = self
            .left_stick_pressed_keys // 左摇杆当前按下的键集合
            .iter() // 迭代集合元素引用
            .copied() // 把 &i32 拷贝为 i32 值
            .filter(|kc| !target.contains(kc)) // 保留"上一轮按下但本轮不再需要"的键（差集）
            .collect(); // 收集为 Vec<i32>
        for kc in to_release { // 遍历待释放的键
            send_stick_key(kc, false, hwnd, injector); // 松开不再需要的键
            self.left_stick_pressed_keys.remove(&kc); // 从按下记录中移除
        } // 结束 for 循环
        // 计算需要按下的键（新按下的）
        for kc in target { // 【Rust 语法】for kc in target：按值遍历集合（Set 已不再使用）
            if !self.left_stick_pressed_keys.contains(&kc) { // 若该键尚未按下
                send_stick_key(kc, true, hwnd, injector); // 注入按下
                self.left_stick_pressed_keys.insert(kc); // 记录已按下
            } // 结束 if 判断
        } // 结束 for 循环

        // 摇杆回中/WASD 释放不解锁：右键锁存保持到用户主动解除（需求3 修正）
    } // 结束 handle_stick 函数

    /// 解析并缓存目标游戏窗口句柄（仅在按键状态变化时执行查找）
    fn resolve_wow_hwnd(&mut self, wow_exe: &str) -> Option<isize> {
        if self.wow_hwnd != 0
            && self.wow_hwnd_exe == wow_exe
            && wow_sender::is_window_valid(self.wow_hwnd)
        {
            return Some(self.wow_hwnd); // 缓存仍然有效
        }
        let found = wow_sender::find_window_by_exe(wow_exe); // 按 exe 路径查找窗口
        self.wow_hwnd_exe = wow_exe.to_string();
        self.wow_hwnd = found.unwrap_or(0);
        found
    } // 结束 resolve_wow_hwnd 函数
} // 结束 impl MapperState 块

// ---------------------------------------------------------------------
// LookRunner —— 视角控制线程（125Hz）
// ---------------------------------------------------------------------
// 【Rust 语法】struct 结构体：负责管理独立的视角控制后台线程。
pub struct LookRunner {
    pub state: Arc<LookState>, // 【Rust 语法】Arc<LookState>：与手柄线程/UI 线程共享视角状态（引用计数共享）
    pub injector: Arc<InputInjector>, // 共享的注入器（Arc 包装）
    handle: Option<thread::JoinHandle<()>>, // 【Rust 语法】Option<JoinHandle<()>>：后台线程句柄；() 表示该线程不返回值
} // 结束 LookRunner 结构体定义

// 【Rust 语法】impl 块：为 LookRunner 实现方法。
impl LookRunner {
    /// 注入器与视角状态均由 AppCore 持有，通过 Arc 共享，
    /// 保证手柄线程（写 latest_x/y）与 look 线程（读）看到同一份状态。
    // 【Rust 语法】关联函数（无 self）：构造器；参数为 Arc 智能指针（克隆引用计数即可共享同一份数据，不复制底层内容）。
    pub fn new(injector: Arc<InputInjector>, state: Arc<LookState>) -> Self {
        Self { // 构造 LookRunner 实例
            state, // 【Rust 语法】字段简写：同名变量直接赋值给字段
            injector, // 字段简写：注入器
            handle: None, // 初始未启动线程，句柄为 None
        } // 结束 Self 结构体字面量
    } // 结束 new 函数

    /// 开始映射：启动 look 线程
    // 【Rust 语法】方法：`&mut self` 可变借用（需要写入线程句柄字段）。
    pub fn start(&mut self) {
        if self.state.running.load(Ordering::SeqCst) { // 若已处于运行状态则直接返回
            return; // 已运行则直接返回
        } // 结束 if 判断
        self.state.running.store(true, Ordering::SeqCst); // 置运行标记（SeqCst 全序内存序，保证跨线程可见）
        // 【Rust 语法】Arc::clone(&...)：克隆引用计数得到新的 Arc，把共享数据的所有权带入新线程。
        let state = Arc::clone(&self.state); // 克隆视角状态引用计数，供线程共享
        let injector = Arc::clone(&self.injector); // 克隆注入器引用计数，供线程共享
        // 【Rust 语法】thread::spawn(闭包)：在新系统线程中执行闭包；`move` 把闭包捕获的变量所有权移入线程。
        self.handle = Some(thread::spawn(move || {
            // timeBeginPeriod(1) 提高系统计时器分辨率，保证节拍准确
            // 【Rust 语法】Instant::now() 获取当前时刻；`let mut` 声明可变绑定。
            let mut last_tick = Instant::now(); // 记录上一次节拍的时刻
            let mut smoothed_x = 0.0f32; // 平滑 X 初始为 0
            let mut smoothed_y = 0.0f32; // 平滑 Y 初始为 0
            while state.running.load(Ordering::SeqCst) { // 循环直到 running 被置为 false（停止）
                let tick_start = Instant::now(); // 记录本次节拍开始时刻
                // 【Rust 语法】as_secs_f32() 转秒（f32 类型）；clamp(0.001, 0.05) 把数值钳制到区间内（防止除零/过大）。
                let dt = (tick_start - last_tick).as_secs_f32().clamp(0.001, 0.05); // 距上一帧的时间差（秒）
                last_tick = tick_start; // 更新上一帧时刻
                process_look_tick(&state, &injector, dt, &mut smoothed_x, &mut smoothed_y); // 执行一次视角节拍处理（传入可变引用，原地震用）

                // 【Rust 语法】elapsed() 返回 Duration；as_millis() 转毫秒（u128）；`as i64` 类型转换。
                let elapsed = tick_start.elapsed().as_millis() as i64; // 本次节拍处理已消耗的毫秒数
                let sleep_ms = LOOK_TICK_MS as i64 - elapsed; // 距离目标周期还剩余的睡眠时间
                if sleep_ms > 0 { // 若剩余时间大于 0 则睡眠补齐
                    thread::sleep(Duration::from_millis(sleep_ms as u64)); // 睡眠补齐剩余时间，保证固定节拍
                } // 结束 if 判断
            } // 结束 while 循环
        })); // 结束闭包与 thread::spawn 调用
    } // 结束 start 函数

    /// 停止映射：停 look 线程
    // 【Rust 语法】方法：`&mut self` 可变借用（需取出并消费线程句柄）。
    pub fn stop(&mut self) {
        self.state.running.store(false, Ordering::SeqCst); // 置停止标记，循环将退出
        if let Some(h) = self.handle.take() { // 【Rust 语法】take()：取出 Option 中的值并原地置为 None（消费句柄）
            let _ = h.join(); // 【Rust 语法】join()：阻塞等待线程结束；`let _ =` 忽略其返回结果
        } // 结束 if let 分支
    } // 结束 stop 函数
} // 结束 impl LookRunner 块

// 【Rust 语法】impl Drop：实现析构 trait（特性），当 LookRunner 实例被销毁（如离开作用域）时自动调用 drop 方法。
impl Drop for LookRunner {
    fn drop(&mut self) { // 析构函数：对象销毁时被自动调用
        self.stop(); // 销毁时自动停止 look 线程，避免线程泄漏
    } // 结束 drop 函数
} // 结束 impl Drop for LookRunner 块

/// look 线程单次节拍：右摇杆 -> 平滑 -> 位移 -> 注入鼠标移动。
/// 处理流水线：
///   1. 幅值钳制：mag 超过 1 时归一化
///   2. 加速度曲线：pow(mag, accel)，推得越深位移越大
///   3. 时间常数 EMA 平滑：alpha = 1-exp(-dt/tau)，tau = smoothing*0.048s
///   4. 位移积分：dx = 平滑值 × 灵敏度 × 480px/s × dt
/// 精细移动开启时灵敏度额外乘 FINE_LOOK_SCALE。
// 【Rust 语法】自由函数：参数含借用 `&`（只读）与可变借用 `&mut`（smoothed_x/y 会被原地震用修改）。
fn process_look_tick(
    state: &LookState, // 视角状态（只读借用，含摇杆最新值等）
    injector: &InputInjector, // 注入器（只读借用）
    dt: f32, // 时间步长（秒）
    smoothed_x: &mut f32, // 平滑 X 的可变引用（跨帧累积状态）
    smoothed_y: &mut f32, // 平滑 Y 的可变引用（跨帧累积状态）
) { // 结束参数列表，函数体开始
    let mut rx = load_f32(&state.latest_x); // 读取右摇杆最新 X（声明可变，后续可能被归一化）
    let mut ry = load_f32(&state.latest_y); // 读取右摇杆最新 Y
    // 精细移动（需求7）：灵敏度乘以缩放系数
    let fine = state.fine_look.load(Ordering::Relaxed);
    let sens = load_f32(&state.sensitivity) * if fine { FINE_LOOK_SCALE } else { 1.0 };
    let smoothing = load_f32(&state.smoothing); // 读取平滑度
    let accel = load_f32(&state.acceleration).clamp(0.5, 3.0); // 读取加速度并钳制在 [0.5, 3.0] 区间

    // 幅值钳制：摇杆输入理论上 <=1，但小数误差可能略超，归一化处理
    let mut mag = (rx * rx + ry * ry).sqrt(); // 计算输入向量幅值（平方和开根号）
    if mag > 1.0 { // 幅值超过 1 则归一化
        rx /= mag; // 归一化 X 分量
        ry /= mag; // 归一化 Y 分量
        mag = 1.0; // 幅值钳制为 1
    } // 结束幅值钳制分支

    // 加速曲线：幅值 -> 更高幅值（推得越深，输出增长越快）
    if rx != 0.0 || ry != 0.0 { // 摇杆有实际输入时
        let curve = mag.powf(accel); // 【Rust 语法】powf(指数)：浮点求幂，计算幅值的 accel 次方
        let scale = curve / mag; // 计算缩放系数
        rx *= scale; // 对 X 分量应用加速缩放
        ry *= scale; // 对 Y 分量应用加速缩放
    } // 结束加速曲线分支

    // 时间常数 EMA 平滑（低通滤波，消除摇杆抖动）
    // 【Rust 语法】clamp：限制数值范围；tau 为滤波时间常数。
    let tau = smoothing.clamp(0.0, 0.95) * LOOK_SMOOTH_TAU_MAX; // 由平滑度换算时间常数
    // 【Rust 语法】if/else 表达式（可作为值使用）：tau<=0 时 alpha=1（无平滑），否则按指数衰减公式计算滤波系数。
    let alpha = if tau <= 0.0 { 1.0 } else { 1.0 - (-dt / tau).exp() };
    *smoothed_x = *smoothed_x * (1.0 - alpha) + rx * alpha; // 【Rust 语法】`*` 解引用可变借用以写入调用方变量；EMA 更新平滑 X
    *smoothed_y = *smoothed_y * (1.0 - alpha) + ry * alpha; // EMA 更新平滑 Y

    // 位移积分：480px/秒 × 灵敏度 × dt（亚像素由注入器余量累积补发）
    // Y 轴取反：XInput 右摇杆向上推 => ry>0，而鼠标向上移动需要 dy<0（屏幕 Y 向下为正）。
    let dx = *smoothed_x * sens * LOOK_SPEED_PX_PER_SEC * dt; // 计算 X 轴位移（像素）
    let dy = -*smoothed_y * sens * LOOK_SPEED_PX_PER_SEC * dt; // 计算 Y 轴位移（取反，因屏幕坐标 Y 向下为正）
    if dx != 0.0 || dy != 0.0 { // 只有存在位移时才注入
        injector.send_mouse_move(dx, dy); // 向系统注入相对鼠标移动
    } // 结束 if 判断
} // 结束 process_look_tick 函数
