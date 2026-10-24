// =====================================================================
// app.rs —— 应用核心组合层（AppCore）
//
// 组合 SteamInput（映射引擎）+ MapperState（注入状态）+ 注入器 + 视角
// 状态，对外提供统一入口：
//   - 手柄轮询线程：handle_source_event（按钮/摇杆/连接事件）
//   - UI 线程：配置管理（操作集增删改、层编辑、全局设置、启停）
//
// 并发模型：
//   AppCore 整体由 Arc<Mutex<AppCore>> 共享；手柄线程与 UI 线程都需
//   获取锁后再操作。临界区都是短操作（映射查询 + 注入调用），不会
//   长时间阻塞。注入器内部自带 Mutex，look 线程不经 AppCore 锁。
// =====================================================================

// 【Rust 语法】`use` 导入语句：`crate::core::injector::InputInjector` 绝对路径指向本 crate 内 core 模块的 injector 子模块中的 InputInjector 结构体。
use crate::core::injector::InputInjector;
// 【Rust 语法】`use ...::{A, B}`：同时导入 mapper 模块下的 LookState 和 MapperState 两个类型。
use crate::core::mapper::{LookState, MapperState};
// 【Rust 语法】一次导入三个类型：ControllerProfile（配置）、GlobalSettings（全局设置）、OperationSet（操作集）。
use crate::core::mapping_types::{ControllerProfile, GlobalSettings, OperationSet};
// 【Rust 语法】导入 steam_input 模块下的 ButtonDispatch（按钮分发枚举）和 SteamInput（映射引擎）。
use crate::core::steam_input::{ButtonDispatch, SteamInput};
// 【Rust 语法】导入 UiEvent（UI 事件枚举，用于向前端发送事件）。
use crate::core::UiEvent;
// 【Rust 语法】导入 SourceEvent（手柄事件源枚举：连接/按钮/摇杆）。
use crate::core::xinput_source::SourceEvent;
// 【Rust 语法】`std::sync::mpsc::Sender`：标准库 mpsc 多生产者单消费者通道的发送端，用于跨线程发送 UI 事件。
use std::sync::mpsc::Sender;
// 【Rust 语法】`std::sync::Arc`：原子引用计数智能指针，允许多个线程共享同一份数据的所有权。
use std::sync::Arc;

// 【Rust 语法】`pub struct AppCore { ... }`：定义公开结构体，花括号内是字段列表（每个字段 名: 类型）。这是 Rust 组合（组合而非继承）模式的体现。
pub struct AppCore {
    // pub 字段：映射引擎 SteamInput，对外可见。
    pub steam: SteamInput,
    // pub 字段：注入状态（按键/鼠标状态跟踪）。
    pub mapper: MapperState,
    // 【Rust 语法】`Arc<InputInjector>`：注入器被 Arc 包裹，便于在多线程间共享（look 线程等不需要 AppCore 锁也能用它）。
    pub injector: Arc<InputInjector>,
    // 【Rust 语法】`Arc<LookState>`：视角状态同样用 Arc 共享（look 线程独立使用）。
    pub look: Arc<LookState>,
    /// 手柄当前是否连接（UI 轮询显示）
    // pub 字段：手柄连接标志，UI 据此轮询显示。
    pub connected: bool,
    /// 配置修订号：任何配置修改（操作集/层/全局设置）都会递增，
    /// UI 据此判断是否需要重新渲染列表等。
    // pub 字段：u64 无符号 64 位整数，作为配置修订版本号。
    pub profile_rev: u64,
    // 【Rust 语法】私有字段（无 pub，外部不可见）：`Sender<UiEvent>` 是 mpsc 通道发送端，UI 事件从这里发出。
    event_tx: Sender<UiEvent>,
}

// 【Rust 语法】`impl AppCore { ... }`：为 AppCore 类型定义方法块；块内的函数称为关联函数/方法，可通过 `实例.方法()` 调用。
impl AppCore {
    // 【Rust 语法】`pub fn new(...) -> Self`：构造函数约定写法——`Self` 是当前类型 AppCore 的别名；`Sender<UiEvent>` 参数按值接收发送端。
    pub fn new(event_tx: Sender<UiEvent>) -> Self {
        // 【Rust 语法】`Arc::new(...)`：把新建的 InputInjector 放进 Arc（引用计数 1），随后可被多线程共享。
        let injector = Arc::new(InputInjector::new());
        // 【Rust 语法】`Self { ... }`：结构体字面量构造 AppCore 实例并作为返回值。
        Self {
            // 创建 SteamInput 引擎，传入事件发送端的克隆副本（每个线程/持有者一份）。
            steam: SteamInput::new(event_tx.clone()),
            // 使用 Default trait 的默认值构造 MapperState（`::default()` 是 trait 提供的关联函数）。
            mapper: MapperState::default(),
            // 用默认值构造 LookState 并放进 Arc。
            look: Arc::new(LookState::default()),
            // 字段简写：`injector` 等价于 `injector: injector`（变量名与字段名相同可省略）。
            injector,
            // 初始状态：手柄未连接。
            connected: false,
            // 初始修订号为 0。
            profile_rev: 0,
            // 字段简写：把 event_tx 存入结构体。
            event_tx,
        }
    }

    // 【Rust 语法】私有方法 `fn emit(&self, ...)`：`&self` 表示借用 self（只读，不修改）；向事件通道发送事件。
    fn emit(&self, e: UiEvent) {
        // 无界 mpsc 通道的 send 不阻塞（receiver 断开时返回 Err，忽略即可）
        // 【Rust 语法】`let _ = ...`：显式丢弃返回值（send 返回 Result，用 _ 忽略）；若接收端已断开，send 返回 Err，这里直接忽略。
        let _ = self.event_tx.send(e);
    }

    // -----------------------------------------------------------------
    // 手柄事件入口（轮询线程回调）
    // -----------------------------------------------------------------
    // 【Rust 语法】`pub fn handle_source_event(&mut self, ...)`：`&mut self` 表示可变借用（可修改自身状态）；参数 event 是 SourceEvent 枚举，按值传入。
    pub fn handle_source_event(&mut self, event: SourceEvent) {
        // 【Rust 语法】`match event`：对枚举做穷尽匹配，按事件种类分发处理。
        match event {
            // 连接状态变化事件：解包出布尔值 connected。
            SourceEvent::Connected(connected) => {
                // 调用内部处理连接变化的方法。
                self.handle_connected(connected);
            }
            // 按钮事件：解包出按钮和按下状态。
            SourceEvent::Button(button, is_pressed) => {
                // 交给 SteamInput 引擎处理按钮事件，返回按钮分发结果枚举。
                let dispatch = self.steam.handle_button_event(button, is_pressed);
                // 对分发结果再次 match。
                match dispatch {
                    // 【Rust 语法】匹配结构体变体：`ButtonDispatch::Execute { is_pressed, mapping }` 解构出内部命名字段。
                    ButtonDispatch::Execute {
                        is_pressed,
                        mapping,
                        layer_id,
                    } => {
                        // 调用 mapper 处理按钮注入（传入按钮、状态、映射、来源层、注入器、事件发送端与视角状态）。
                        self.mapper.handle_button(
                            button,
                            is_pressed,
                            // `&mapping` 传入映射的借用。
                            &mapping,
                            &layer_id,
                            // 【Rust 语法】`injector.as_ref()`：把 Arc<InputInjector> 转成 &InputInjector 引用传给方法。
                            self.injector.as_ref(),
                            &self.event_tx,
                            self.look.as_ref(),
                        );
                    }
                    // 松开激活层按钮：释放该层内产生的鼠标左右键锁存（需求6）
                    ButtonDispatch::DeactivateLayer(layer_id) => {
                        self.mapper.release_layer_mouse(
                            &layer_id,
                            self.injector.as_ref(),
                            &self.event_tx,
                        );
                        // 层切换时退出精细移动，避免灵敏度残留
                        self.look.set_fine_look(false);
                    }
                    // 开关映射的分发：向外发出 ToggleMappingRequested 事件。
                    ButtonDispatch::ToggleMapping => {
                        self.emit(UiEvent::ToggleMappingRequested);
                    }
                    // 开关叠加层：发出 ToggleOverlayRequested 事件。
                    ButtonDispatch::ToggleOverlay => {
                        self.emit(UiEvent::ToggleOverlayRequested);
                    }
                    // 开关屏幕键盘：Windows 版无此功能，分支留空。
                    ButtonDispatch::ToggleOnScreenKeyboard => {
                        // Windows 版无屏幕键盘，忽略
                    }
                    // 无操作分支：空块 {}。
                    ButtonDispatch::None => {}
                }
            }
            // 摇杆事件：解包出摇杆编号和 x/y 坐标。
            SourceEvent::Stick(stick, x, y) => {
                // 【Rust 语法】`let (stick, x, y) = ...`：元组解构——把函数返回的三元组同时赋给三个变量（变量名遮蔽外部旧值）。
                let (stick, x, y) = self.steam.handle_stick_input(stick, x, y);
                let wow_exe = self.steam.profile.global_settings.wow_exe.clone(); // 目标游戏 exe（WASD 直发用）
                // 链式方法调用：让 mapper 处理摇杆输入（传入注入器、视角状态引用与目标 exe）。
                self.mapper.handle_stick(
                    stick,
                    x,
                    y,
                    self.injector.as_ref(),
                    self.look.as_ref(),
                    &wow_exe,
                );
            }
            // 轮询心跳：检查待判定的长按是否达到阈值（需求2）
            SourceEvent::Tick => {
                self.mapper.check_long_press(
                    self.injector.as_ref(),
                    &self.event_tx,
                    self.look.as_ref(),
                );
            }
        }
    }

    // 【Rust 语法】私有方法 `fn handle_connected(&mut self, ...)`：处理连接状态变化。
    fn handle_connected(&mut self, connected: bool) {
        // 更新本地连接标志。
        self.connected = connected;
        // 向 UI 发出连接状态事件。
        self.emit(UiEvent::Connected(connected));
        // 若为断开连接。
        if !connected {
            // 断开：释放全部注入（含 MouseToggle 锁存）+ 清空层栈与按持记录
            // 链式调用 release_all_inputs 释放全部注入（注入器、事件发送端、视角状态）。
            self.mapper
                .release_all_inputs(self.injector.as_ref(), &self.event_tx, self.look.as_ref());
            // 清空 SteamInput 的层栈。
            self.steam.deactivate_all_layers();
            // 清空按持按钮记录，防止卡键。
            self.steam.clear_held_buttons();
        }
    }

    // -----------------------------------------------------------------
    // 启停
    // -----------------------------------------------------------------
    /// 开始映射：更新视角设置参数
    // 【Rust 语法】`pub fn start_mapping(&mut self)`：公开方法，`&mut self` 允许修改自身。
    pub fn start_mapping(&mut self) {
        // 【Rust 语法】`&self.steam.profile.global_settings`：取全局设置的引用（避免整体 clone），gs 是 &GlobalSettings。
        let gs = &self.steam.profile.global_settings;
        // 链式调用：用全局设置里的灵敏度/平滑/加速度更新视角状态。
        self.look
            .update_settings(gs.look_sensitivity, gs.look_smoothing, gs.look_acceleration);
    }

    /// 停止映射：释放全部注入 + 清空层栈
    pub fn stop_mapping(&mut self) {
        // 释放全部注入。
        self.mapper
            .release_all_inputs(self.injector.as_ref(), &self.event_tx, self.look.as_ref());
        // 清空层栈。
        self.steam.deactivate_all_layers();
        // 清空按持记录。
        self.steam.clear_held_buttons();
    }

    /// 前台切换导致停止时调用（仅释放注入，保持手柄连接状态）
    pub fn release_all_inputs(&mut self) {
        // 释放全部注入（保持连接状态，只停止映射效果）。
        self.mapper
            .release_all_inputs(self.injector.as_ref(), &self.event_tx, self.look.as_ref());
        // 清空层栈。
        self.steam.deactivate_all_layers();
        // 清空按持记录。
        self.steam.clear_held_buttons();
    }

    // -----------------------------------------------------------------
    // 配置管理（UI 线程调用）
    // -----------------------------------------------------------------

    /// 整体加载配置（启动 / 重置默认）
    // 【Rust 语法】参数 `profile: ControllerProfile` 按值（拥有所有权）传入，加载后由内部持有。
    pub fn load_profile(&mut self, profile: ControllerProfile) {
        // 把配置加载进 SteamInput 引擎。
        self.steam.load_profile(profile);
        // 修订号自增（`+= 1` 是复合赋值运算符），通知 UI 配置已变化。
        self.profile_rev += 1;
        // 【Rust 语法】`.clone()`：复制全局设置（避免借用冲突，因后面要同时借用 look 和 steam）。
        let gs = self.steam.profile.global_settings.clone();
        // 用新配置更新视角参数。
        self.look.update_settings(gs.look_sensitivity, gs.look_smoothing, gs.look_acceleration);
    }

    /// 更新全局设置（界面滑块实时调整）
    pub fn update_global_settings(&mut self, settings: GlobalSettings) {
        // 把新的全局设置写入 SteamInput。
        self.steam.set_global_settings(settings);
        // 修订号自增。
        self.profile_rev += 1;
        // 复制当前全局设置。
        let gs = self.steam.profile.global_settings.clone();
        // 同步更新视角参数。
        self.look
            .update_settings(gs.look_sensitivity, gs.look_smoothing, gs.look_acceleration);
    }

    /// 切换操作集（按 id），成功返回 true
    // 【Rust 语法】参数 `set_id: &str`（字符串借用）→ 返回 `bool`。
    pub fn switch_operation_set(&mut self, set_id: &str) -> bool {
        // 【Rust 语法】if-else 作为表达式：条件为真返回 true 分支，否则 false 分支。
        if self.steam.switch_operation_set(set_id) {
            // 切换成功，修订号自增。
            self.profile_rev += 1;
            true
        } else {
            false
        }
    }

    /// 新增操作集：自动生成 id，默认名"操作集 N"（防重名）。
    /// 返回新集 id。
    // 【Rust 语法】返回值 `-> String`：返回新操作集的 id（拥有的字符串）。
    pub fn add_operation_set(&mut self) -> String {
        // 生成一个不重复的 id。
        let id = self.steam.profile.unique_operation_set_id();
        // 【Rust 语法】`format!` 宏拼字符串；`id.trim_start_matches("Set")` 去掉 id 前缀 "Set" 得到数字部分；`unique_set_name` 保证显示名不重名。
        let name = unique_set_name(&self.steam.profile, &format!("操作集{}", id.trim_start_matches("Set")));
        // 添加前先清空层栈，防止 Vec 扩容导致旧层引用失效（用 id 更安全）
        // 添加操作集前先清空层栈，防止 Vec 扩容使已激活层的指针悬垂。
        self.steam.deactivate_all_layers();
        // 【Rust 语法】`let set = ...`：调用关联函数 create_empty 创建空操作集。
        let set = OperationSet::create_empty(&id, &name);
        // push 把操作集加入列表（转移所有权）。
        self.steam.profile.operation_sets.push(set);
        // 修订号自增。
        self.profile_rev += 1;
        // 通知操作集列表已变化。
        self.steam.notify_operation_set_changed();
        // 返回新 id（尾表达式，无分号）。
        id
    }

    /// 复制操作集为新操作集：直接以新名字创建。
    // 【Rust 语法】两个 &str 参数，返回 bool 表示是否成功。
    pub fn copy_operation_set(&mut self, src_id: &str, new_name: &str) -> bool {
        // 【Rust 语法】`match ... { Some(i) => i, None => return false }`：若找到则取出下标，找不到提前返回 false。
        let src_idx = match self
            .steam
            .profile
            .operation_sets
            // 【Rust 语法】`.iter()` 生成迭代器，`.position(|s| s.id == src_id)` 用闭包判断条件，返回 Option<usize>（第一个匹配下标）。
            .iter()
            .position(|s| s.id == src_id)
        {
            Some(i) => i,
            None => return false,
        };
        // 生成新 id。
        let new_id = self.steam.profile.unique_operation_set_id();
        // 【Rust 语法】`operation_sets[src_idx]` 按下标访问，`.clone()` 深拷贝该操作集（保留其全部层数据）。
        let mut copy = self.steam.profile.operation_sets[src_idx].clone();
        // 覆盖复制体的 id 为新 id。
        copy.id = new_id;
        // 【Rust 语法】`new_name.to_string()`：把 &str 转换成拥有的 String 赋给 name。
        copy.name = new_name.to_string();
        // 复制前先清空层栈（安全措施）。
        self.steam.deactivate_all_layers();
        // 把复制体加入列表。
        self.steam.profile.operation_sets.push(copy);
        // 修订号自增。
        self.profile_rev += 1;
        // 通知操作集列表已变化。
        self.steam.notify_operation_set_changed();
        // 返回成功。
        true
    }

    /// 重命名操作集（仅改显示名，不影响运行时逻辑）
    pub fn rename_operation_set(&mut self, set_id: &str, new_name: &str) -> bool {
        // 【Rust 语法】`.iter_mut()` 生成可变迭代器，`.find(闭包)` 找到第一个匹配元素，返回 Option<&mut OperationSet>（可变引用）。
        let set = match self
            .steam
            .profile
            .operation_sets
            .iter_mut()
            .find(|s| s.id == set_id)
        {
            // 找到则取出可变引用。
            Some(s) => s,
            None => return false,
        };
        // 【Rust 语法】`new_name.trim()` 去除首尾空白字符；若为空则拒绝重命名。
        if new_name.trim().is_empty() {
            return false;
        }
        // 修改显示名。
        set.name = new_name.to_string();
        // 修订号自增。
        self.profile_rev += 1;
        // 通知操作集列表已变化。
        self.steam.notify_operation_set_changed();
        // 返回成功。
        true
    }

    /// 删除操作集：至少保留一个；删除后回退到第一个集。返回是否成功。
    pub fn delete_operation_set(&mut self, set_id: &str) -> bool {
        // 【Rust 语法】`.len()` 获取 Vec 长度；若只剩一个操作集则禁止删除（至少保留一个）。
        if self.steam.profile.operation_sets.len() <= 1 {
            return false;
        }
        // 查找待删除操作集的下标；`.unwrap_or(0)` 找不到时默认删除第一个。
        let pos = self
            .steam
            .profile
            .operation_sets
            .iter()
            .position(|s| s.id == set_id)
            .unwrap_or(0);
        // 删除前先清空层栈（安全措施）。
        self.steam.deactivate_all_layers();
        // 【Rust 语法】`Vec::remove(pos)`：按下标删除元素并返回（其余元素前移）。
        self.steam.profile.operation_sets.remove(pos);
        // 若删除的是当前激活集，回退到第一个
        // 【Rust 语法】`.clone()`：复制当前激活集 id（避免后续操作冲突）。
        let active_id = self.steam.profile.active_operation_set_id.clone();
        // 若原激活集已不存在（set_active_operation_set 返回 false）。
        if !self.steam.profile.set_active_operation_set(&active_id) {
            // 【Rust 语法】`if let Some(first) = ...`：`.first()` 返回 Option<&OperationSet>，Some 时解包出第一个集。
            if let Some(first) = self.steam.profile.operation_sets.first() {
                // 回退：激活第一个操作集。
                self.steam.profile.active_operation_set_id = first.id.clone();
            }
        }
        // 修订号自增。
        self.profile_rev += 1;
        // 通知操作集列表已变化。
        self.steam.notify_operation_set_changed();
        // 返回成功。
        true
    }

    /// 当前激活操作集 id
    // 【Rust 语法】返回值 `-> &str`：返回借用，不转移所有权。
    pub fn active_operation_set_id(&self) -> &str {
        // 【Rust 语法】`&self.steam.profile.active_operation_set_id`：取 String 的引用；返回 &str 时 String 会自动解引用强制转换（deref coercion）。
        &self.steam.profile.active_operation_set_id
    }
}

/// 生成不与现有操作集重名的显示名（追加数字后缀）
// 【Rust 语法】模块级私有函数：参数为配置借用和基础名，返回 String。
fn unique_set_name(profile: &ControllerProfile, base: &str) -> String {
    // 【Rust 语法】`std::collections::HashSet<&str>`：哈希集合容器，元素是 &str 引用；`iter().map(闭包).collect()` 是迭代器管道：遍历操作集 → 映射出每个名字的 &str → 收集进 HashSet。
    let used: std::collections::HashSet<&str> =
        profile.operation_sets.iter().map(|s| s.name.as_str()).collect();
    // 【Rust 语法】`.contains(base)` 判断集合中是否已有该名；若没有则直接用基础名。
    if !used.contains(base) {
        return base.to_string();
    }
    // 从序号 2 开始尝试。
    let mut n = 2;
    // 【Rust 语法】`loop { ... }`：无限循环，靠内部 return 退出。
    loop {
        // 【Rust 语法】`format!("{} {}", base, n)`：字符串插值，生成"基础名 序号"格式的候选名。
        let candidate = format!("{} {}", base, n);
        // 若候选名不重名。
        if !used.contains(candidate.as_str()) {
            // 返回该候选名（`as_str()` 把 String 转成 &str 用于查询）。
            return candidate;
        }
        // 序号递增，继续尝试。
        n += 1;
    }
}
