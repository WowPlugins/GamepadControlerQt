// =====================================================================
// xinput_source.rs —— XInput 手柄读取源
//
// 等效 Qt 版 XInputGamepadSource.h/.cpp（Windows XInput，不用 QtGamepad）。
// 内部独立线程以固定周期（默认 8ms = 125Hz）轮询 XInputGetState，
// 确保应用在后台/非焦点时仍能正常轮询。
//
// 关键设计：
//   1. 连接防抖：连续 MAX_CONNECTION_FAILS 次轮询失败才判定断开，
//      避免 USB 短暂通信错误导致界面闪烁。
//   2. 释放兜底：断开（或 stop）时，把所有仍处于"按下"状态的按钮
//      强制发一遍松开事件，避免按键/鼠标键卡死。
// =====================================================================

// 【Rust 语法】use 语句：导入其它模块/包中的符号，相当于 C++ 的 using/import
use crate::core::input_types::{ControllerButton, ControllerStick}; // 从本 crate 的 core::input_types 模块导入手柄按钮与摇杆两个枚举
use std::collections::HashMap; // 导入标准库 HashMap（哈希表），用于记录每个按钮上一次的按下状态
use std::sync::atomic::{AtomicBool, Ordering}; // 导入原子布尔类型与内存顺序，用于跨线程安全地读写"运行中"标志
use std::sync::Arc; // 导入 Arc（原子引用计数智能指针），让同一份数据可被多个线程共享
use std::thread::{self, JoinHandle}; // 导入线程模块自身与线程句柄类型，用于启动/等待轮询线程
use std::time::Duration; // 导入 Duration 时间段类型，用于设置轮询线程的休眠时长
use windows::Win32::Foundation::ERROR_SUCCESS; // 导入 Windows 的 HRESULT"成功"常量，用于判断 XInputGetState 是否成功
use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_STATE}; // 导入 Windows XInput 的 API 函数与状态结构体

/// 手柄源事件（回调给上层）
// 【Rust 语法】#[derive(...)]：自动为枚举派生 trait 实现——Debug（调试输出）、Clone（克隆）、Copy（按位复制）
#[derive(Debug, Clone, Copy)]
// 【Rust 语法】enum 枚举：一种可携带不同类型数据（变体）的类型，这里用作手柄事件的统一载体
pub enum SourceEvent {
    /// 连接状态变化（connected=true 已连接）
    Connected(bool), // 变体：携带一个 bool，true=已连接 / false=断开
    /// 按钮按下/松开
    Button(ControllerButton, bool), // 变体：携带按钮枚举与按下状态 bool
    /// 摇杆输入（x,y 归一化到 [-1,1]，未应用死区）
    Stick(ControllerStick, f32, f32), // 变体：携带摇杆枚举与 x、y 两个归一化浮点坐标
    /// 轮询心跳（每次成功轮询末尾发出，用于驱动长按计时判定）
    Tick, // 变体：不携带数据
}

/// 手柄源事件回调（上层为 AppCore 的共享锁）
// 【Rust 语法】type 别名 + dyn Fn：给"可调用的闭包类型"起别名；dyn Fn(...) 是 trait 对象（动态分发）；
// Arc 使闭包可跨线程共享；Send+Sync 是线程安全约束（可移动/可共享引用）
pub type SourceCallback = Arc<dyn Fn(SourceEvent) + Send + Sync>; // 回调类型：接收 SourceEvent、无返回值、可跨线程调用的闭包

/// XInput 按钮位定义（wButtons 位掩码，与 XINPUT_GAMEPAD_BUTTON_FLAGS 一致）
// 【Rust 语法】const：声明编译期常量；u16 是无符号 16 位整数，对应 Windows 的 WORD 类型
const BIT_A: u16 = 0x1000; // A 键对应的位掩码（十六进制）
const BIT_B: u16 = 0x2000; // B 键对应的位掩码
const BIT_X: u16 = 0x4000; // X 键对应的位掩码
const BIT_Y: u16 = 0x8000; // Y 键对应的位掩码
const BIT_DPAD_UP: u16 = 0x0001; // 十字键"上"对应的位掩码
const BIT_DPAD_DOWN: u16 = 0x0002; // 十字键"下"对应的位掩码
const BIT_DPAD_LEFT: u16 = 0x0004; // 十字键"左"对应的位掩码
const BIT_DPAD_RIGHT: u16 = 0x0008; // 十字键"右"对应的位掩码
const BIT_START: u16 = 0x0010; // START（菜单）键对应的位掩码
const BIT_BACK: u16 = 0x0020; // BACK（视图）键对应的位掩码
const BIT_LEFT_THUMB: u16 = 0x0040; // 左摇杆按下（L3）对应的位掩码
const BIT_RIGHT_THUMB: u16 = 0x0080; // 右摇杆按下（R3）对应的位掩码
const BIT_LEFT_SHOULDER: u16 = 0x0100; // 左肩键（LB）对应的位掩码
const BIT_RIGHT_SHOULDER: u16 = 0x0200; // 右肩键（RB）对应的位掩码

/// XInput 按钮位映射：XInput wButtons 位 -> 统一按钮枚举
// 【Rust 语法】数组常量：[(u16, ControllerButton); 14] 表示"包含 14 个元素的数组，元素类型为 (u16, ControllerButton) 二元元组"
const BUTTON_DEFS: [(u16, ControllerButton); 14] = [
    (BIT_DPAD_UP, ControllerButton::DpadUp), // 位掩码 -> 十字键上
    (BIT_DPAD_DOWN, ControllerButton::DpadDown), // 位掩码 -> 十字键下
    (BIT_DPAD_LEFT, ControllerButton::DpadLeft), // 位掩码 -> 十字键左
    (BIT_DPAD_RIGHT, ControllerButton::DpadRight), // 位掩码 -> 十字键右
    (BIT_START, ControllerButton::Menu), // 位掩码 -> 菜单键
    (BIT_BACK, ControllerButton::Options), // 位掩码 -> 视图键
    (BIT_LEFT_THUMB, ControllerButton::LeftStickClick), // 位掩码 -> 左摇杆按下
    (BIT_RIGHT_THUMB, ControllerButton::RightStickClick), // 位掩码 -> 右摇杆按下
    (BIT_LEFT_SHOULDER, ControllerButton::LeftShoulder), // 位掩码 -> 左肩键
    (BIT_RIGHT_SHOULDER, ControllerButton::RightShoulder), // 位掩码 -> 右肩键
    (BIT_A, ControllerButton::A), // 位掩码 -> A 键
    (BIT_B, ControllerButton::B), // 位掩码 -> B 键
    (BIT_X, ControllerButton::X), // 位掩码 -> X 键
    (BIT_Y, ControllerButton::Y), // 位掩码 -> Y 键
]; // 按钮位映射数组定义结束

/// 摇杆原始值(SHORT) -> 归一化浮点 -1.0 ~ 1.0
// 【Rust 语法】fn：定义函数；参数 value: i16 为有符号 16 位整数（XInput 摇杆原始值范围 -32768~32767）；-> f32 表示返回 32 位浮点
fn axis_to_float(value: i16) -> f32 {
    // 【Rust 语法】const 在函数体内声明局部常量：MAX 为 32767.0（i16 正向上限）
    const MAX: f32 = 32767.0; // 定义归一化除数的最大值
    let v = value as f32; // 【Rust 语法】let 声明不可变局部变量；as 是类型转换运算符，把 i16 转成 f32
    if v > MAX { // 【Rust 语法】if 条件判断，无括号包裹条件
        return 1.0; // 超过正向上限则饱和为 1.0（防止极端值越界）
    } // 结束"超过上限"分支
    if v < -MAX { // 小于负向上限时进入分支
        return -1.0; // 低于负向下限则饱和为 -1.0
    } // 结束"低于下限"分支
    v / MAX // 【Rust 语法】表达式作为返回值：末尾无分号的行即是函数的返回值（等价于 return v / MAX）
} // 函数 axis_to_float 结束

// 【Rust 语法】const：最大连接失败次数，u32 为无符号 32 位整数
const MAX_CONNECTION_FAILS: u32 = 3; // 连续失败达到 3 次才判定手柄断开（连接防抖）

// ---------------------------------------------------------------------
// Poller —— 轮询状态机（在轮询线程内独占使用）
// 持有跨轮询的状态：上一按钮状态 / 连接状态 / 失败计数。
// ---------------------------------------------------------------------
// 【Rust 语法】struct：定义结构体（数据聚合类型），字段之间用逗号分隔
struct Poller {
    player_index: u32, // 手柄玩家编号（XInput 0~3，这里固定为 0）
    connected: bool, // 当前是否处于已连接状态
    prev_button_states: HashMap<ControllerButton, bool>, // 【Rust 语法】泛型 HashMap<K,V>：记录每个按钮上一次的按下状态，用于检测变化
    connection_fail_count: u32, // 连续读取失败的次数计数
    callback: SourceCallback, // 事件回调闭包，轮询到变化时回调上层
} // 结构体 Poller 定义结束

// 【Rust 语法】impl 块：为 Poller 结构体实现方法（相当于 C++ 的成员函数）
impl Poller {
    // 【Rust 语法】关联函数 new：不接收 self 的构造函数；-> Self 表示返回类型就是本结构体 Poller
    fn new(player_index: u32, callback: SourceCallback) -> Self {
        // 【Rust 语法】struct 字面量初始化：字段名: 值；Self { ... } 构造当前结构体实例
        Self {
            player_index, // 保存玩家编号（字段简写：同名变量可直接省略 "player_index: player_index"）
            connected: false, // 初始状态为未连接
            prev_button_states: HashMap::new(), // 创建空的按钮状态哈希表
            connection_fail_count: 0, // 初始失败计数为 0
            callback, // 保存回调闭包（简写语法）
        } // 结构体字面量初始化结束
    } // 构造函数 new 结束

    /// 单次轮询手柄状态
    // 【Rust 语法】&mut self：接收结构体的可变借用，表示该方法会修改自身状态
    fn poll(&mut self) {
        let mut state = XINPUT_STATE::default(); // 【Rust 语法】let mut 可变变量；::default() 是关联函数调用，用默认值创建 Windows 状态结构体
        // 【Rust 语法】unsafe 块：调用外部 FFI（C 接口）函数属于不安全操作，需要显式标记；
        // &mut state 是对 state 的可变借用，作为出参接收手柄状态
        let result = unsafe { XInputGetState(self.player_index, &mut state) }; // 调用 XInputGetState 读取手柄状态，返回 HRESULT 结果码

        if result == ERROR_SUCCESS.0 { // 【Rust 语法】ERROR_SUCCESS.0：访问 Windows 元组结构体 .0 字段拿到数值，与返回码比较判断是否成功
            // ---- 连接成功 ----
            self.connection_fail_count = 0; // 只要有成功就读，就视为在线
            if !self.connected { // 【Rust 语法】! 为逻辑取反；判断之前是否未连接
                self.connected = true; // 标记为已连接
                (self.callback)(SourceEvent::Connected(true)); // 调用回调（括号包裹 self.callback 后加参数调用），通知上层连接成功
            } // 结束"从未连接到已连接"的过渡分支
            let pad = state.Gamepad; // 【Rust 语法】字段访问：取出结构体中的 Gamepad 子结构体（含按键位掩码与摇杆数据）

            // 数字按键：遍历映射表，逐位检查按下状态并对比上次
            // 【Rust 语法】for ... in 循环：遍历数组的每个元素；(bit, button) 是元组解构，把 (u16, ControllerButton) 拆开
            for (bit, button) in BUTTON_DEFS {
                let pressed = (pad.wButtons.0 & bit) != 0; // 【Rust 语法】位与运算 &：取出 wButtons 掩码中该按钮对应的位是否为 1；.0 访问元组字段
                let prev = *self.prev_button_states.get(&button).unwrap_or(&false); // 【Rust 语法】get 返回 Option<&bool>；unwrap_or 提供缺省值；* 解引用取到 bool 值
                if pressed != prev { // 按下状态与上一次不同，说明发生边沿变化
                    self.prev_button_states.insert(button, pressed); // 更新记录的最新状态
                    (self.callback)(SourceEvent::Button(button, pressed)); // 回调上层：发出按钮按下/松开事件
                } // 结束"状态变化"判断分支
            } // 结束按键遍历循环

            // 模拟扳机（LT/RT 是 0~255 的模拟量）：半程以上（>=128）视为"按下"
            self.update_trigger(ControllerButton::LeftTriggerClick, pad.bLeftTrigger >= 128); // 左扳机：数值 >=128 视为按下
            self.update_trigger(ControllerButton::RightTriggerClick, pad.bRightTrigger >= 128); // 右扳机：数值 >=128 视为按下

            // 摇杆：归一化后发射（死区由 SteamInput 统一处理）
            // 【Rust 语法】多行函数调用：参数在括号内分行书写，缩进继续
            (self.callback)(SourceEvent::Stick( // 回调上层：发出左摇杆事件
                ControllerStick::LeftStick, // 摇杆枚举：左摇杆
                axis_to_float(pad.sThumbLX), // x 轴：原始短整数转归一化浮点
                axis_to_float(pad.sThumbLY), // y 轴：原始短整数转归一化浮点
            )); // 左摇杆事件调用结束
            (self.callback)(SourceEvent::Stick( // 回调上层：发出右摇杆事件
                ControllerStick::RightStick, // 摇杆枚举：右摇杆
                axis_to_float(pad.sThumbRX), // x 轴：原始短整数转归一化浮点
                axis_to_float(pad.sThumbRY), // y 轴：原始短整数转归一化浮点
            )); // 右摇杆事件调用结束
            (self.callback)(SourceEvent::Tick); // 每次成功轮询末尾发出心跳（驱动长按计时）
        } else { // 读取失败走此分支
            // ---- 读取失败（可能短暂抖动，也可能真正断开）----
            self.connection_fail_count += 1; // 失败计数加一
            if self.connection_fail_count >= MAX_CONNECTION_FAILS && self.connected { // 连续失败达到阈值，且当前仍标记为连接中
                self.connected = false; // 判定为断开，更新连接标志
                // 释放所有已按下的按钮，避免 heldButtons 堆积 / 键鼠卡死
                // 【Rust 语法】let pressed: Vec<...>：显式标注变量类型为 Vec（动态数组）；
                // 迭代器链：iter() 遍历 -> filter 按闭包过滤 -> map 转换 -> collect 收集为 Vec
                let pressed: Vec<ControllerButton> = self
                    .prev_button_states // 对哈希表进行迭代
                    .iter() // 取得迭代器（元素为 (&ControllerButton, &bool)）
                    .filter(|(_, &p)| p) // 【Rust 语法】闭包 |(_, &p)| p：模式解构出按键与状态值，只保留按下(p=true)的项
                    .map(|(b, _)| *b) // 【Rust 语法】map 转换：从引用解出按钮值 *b，忽略状态
                    .collect(); // 把迭代器收集成 Vec<ControllerButton>
                for b in pressed { // 遍历所有需要释放的按钮
                    self.prev_button_states.insert(b, false); // 更新状态为未按下
                    (self.callback)(SourceEvent::Button(b, false)); // 回调上层：发出松开事件，防止卡键
                } // 结束释放遍历循环
                self.prev_button_states.clear(); // 清空状态表，回到初始状态
                (self.callback)(SourceEvent::Connected(false)); // 回调上层：通知断开
            } // 结束"判定为断开"分支
        } // 结束 else（读取失败）分支
    } // 函数 poll 结束

    // 【Rust 语法】方法 update_trigger：处理模拟扳机键的按下状态变化
    fn update_trigger(&mut self, button: ControllerButton, pressed: bool) {
        let prev = *self.prev_button_states.get(&button).unwrap_or(&false); // 读取该按钮上一次的状态（无记录则视为 false）
        if pressed != prev { // 状态发生变化时才处理
            self.prev_button_states.insert(button, pressed); // 更新最新状态
            (self.callback)(SourceEvent::Button(button, pressed)); // 回调上层：发出按钮事件
        } // 结束"状态变化"判断分支
    } // 方法 update_trigger 结束
} // impl Poller 结束

// 【Rust 语法】pub struct：公开结构体，供模块外部（AppCore）使用
pub struct XInputGamepadSource {
    player_index: u32, // 手柄玩家编号
    running: Arc<AtomicBool>, // 【Rust 语法】Arc<AtomicBool>：跨线程共享的原子布尔，作为线程"是否运行中"的开关
    poll_interval_ms: u64, // 轮询间隔（毫秒），默认 8ms 对应 125Hz
    handle: Option<JoinHandle<()>>, // 【Rust 语法】Option<JoinHandle<()>>：线程句柄的可选值（Some=线程在运行 / None=无线程）
    callback: SourceCallback, // 事件回调闭包
} // 结构体 XInputGamepadSource 定义结束

impl XInputGamepadSource {
    // 【Rust 语法】pub fn new：公开的构造函数；Self 即 XInputGamepadSource 本身
    pub fn new(callback: SourceCallback) -> Self {
        Self {
            player_index: 0, // 默认使用 0 号手柄
            running: Arc::new(AtomicBool::new(false)), // 【Rust 语法】Arc::new + AtomicBool::new：创建初始为 false 的原子开关
            poll_interval_ms: 8, // 轮询间隔 8ms
            handle: None, // 初始没有线程句柄
            callback, // 保存回调（简写）
        } // 结构体字面量初始化结束
    } // 构造函数 new 结束

    /// 启动轮询（重置连接失败计数）
    // 【Rust 语法】&mut self：可变借用，因为要写入 handle / running 字段
    pub fn start(&mut self) {
        if self.running.load(Ordering::SeqCst) { // 【Rust 语法】原子 load：以"顺序一致性"内存序读取当前值；若已在运行则直接返回，避免重复启动
            return; // 已在运行，直接返回
        } // 结束"已在运行"判断分支
        self.running.store(true, Ordering::SeqCst); // 原子 store：以顺序一致性内存序写入 true，标记开始运行
        // 【Rust 语法】Arc::clone：增加引用计数，把同一份数据分享给线程（不深拷贝数据本身）
        let running = Arc::clone(&self.running); // 复制运行开关的引用给线程用
        let poll_interval_ms = self.poll_interval_ms; // 拷贝轮询间隔（u64 为 Copy 类型，直接复制）
        let player_index = self.player_index; // 拷贝玩家编号
        let callback = Arc::clone(&self.callback); // 复制回调闭包的 Arc 引用给线程用
        // 【Rust 语法】thread::spawn(move || {...})：创建新线程；move 关键字把捕获的变量所有权移入闭包；
        // 返回值是 JoinHandle<()>，用 Some 包装存入 handle 字段
        self.handle = Some(thread::spawn(move || {
            let mut poller = Poller::new(player_index, callback); // 在线程内创建轮询状态机
            poller.poll(); // 立即轮询一次，快速反馈连接状态
            while running.load(Ordering::SeqCst) { // 【Rust 语法】while 循环：只要运行标志为 true 就持续轮询
                thread::sleep(Duration::from_millis(poll_interval_ms)); // 休眠一个轮询周期，控制 125Hz 频率
                poller.poll(); // 执行一次轮询
            } // 结束轮询循环
        })); // thread::spawn 调用结束
    } // 方法 start 结束

    /// 停止轮询并清理（通知上层释放所有注入）
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst); // 原子写入 false：通知轮询线程退出循环
        // 【Rust 语法】if let Some(h) = ...：模式匹配 Option；take() 把 Option 取走并留下 None；若存在线程句柄则 h 绑定其值
        if let Some(h) = self.handle.take() {
            let _ = h.join(); // 【Rust 语法】join()：等待线程结束；let _ = 忽略返回值；join 返回 Result，等待成功即 Ok
        } // 结束"存在线程句柄"分支
        // 无论是否曾连接都发一次断开事件，上层据此 release_all_inputs
        // （幂等，重复调用无害；确保退出/停止时无残留键鼠注入）。
        (self.callback)(SourceEvent::Connected(false)); // 回调上层：通知断开，触发释放所有按键
    } // 方法 stop 结束
} // impl XInputGamepadSource 结束

// 【Rust 语法】impl Drop for：实现 Drop trait，定义对象被销毁（drop）时的清理逻辑（类似 C++ 析构函数）
impl Drop for XInputGamepadSource {
    fn drop(&mut self) { // Drop trait 要求实现的方法签名：接收 &mut self
        self.stop(); // 析构时自动停止轮询并释放
    } // drop 方法结束
} // impl Drop 结束
