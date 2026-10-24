// =====================================================================
// steam_input.rs —— 映射引擎
//
// 等效 Qt 版 SteamInput.h/.cpp。
// 职责：
//   - 维护当前激活的操作层栈（公共层始终激活、优先级最低）
//   - 按键查询：按「最后激活的操作层 -> ... -> 公共层」顺序查找有效映射
//   - 分发按钮/摇杆输入，并负责层切换动作的运行时处理
//
// 线程模型：
//   - 手柄轮询线程调用 handle_button_event / handle_stick_input（&mut self）
//   - UI 线程通过同一把锁修改配置（操作集切换 / 层编辑 / 全局设置）
//   - 状态变化通过 UI 事件通道（Sender<UiEvent>）通知界面/悬浮窗
// =====================================================================

// 【Rust 语法】use 语句导入类型：花括号分组一次引入 ControllerButton、ControllerStick、Vector2 三个类型。
use crate::core::input_types::{ControllerButton, ControllerStick, Vector2};
// 【Rust 语法】use 可跨行书写：多个导入名称用逗号分隔、换行排列，末尾分号结束。
use crate::core::mapping_types::{
    ActionType, ControllerProfile, KeyMapping, OperationLayer,
};
use crate::core::UiEvent; // 导入 UiEvent 类型
use std::collections::{HashMap, HashSet}; // 【Rust 语法】嵌套花括号：从 std::collections 一次导入 HashMap 与 HashSet 两个容器
use std::sync::mpsc::Sender; // 【Rust 语法】mpsc = 多生产者单消费者消息通道；Sender 为通道发送端类型

/// 按钮事件分发结果（由外层 mapper 决定执行注入）
// 【Rust 语法】#[derive(Debug, Clone)] 是属性宏（derive 派生宏）：让编译器自动为类型实现 Debug（调试打印）
// 与 Clone（克隆）两个 trait，免去手写 impl 代码。Rust 用 #[...] 属性给编译器附加信息。
#[derive(Debug, Clone)]
// 【Rust 语法】pub enum 定义枚举：成员称为「变体」；比 C++ 的 enum class 更强大，因为变体可以携带数据。
pub enum ButtonDispatch {
    /// 无动作（松开切层键、无映射按下等）
    None, // 【Rust 语法】单元变体：不携带任何数据（类似 C 的枚举常量）
    /// 交给映射器执行注入
    // 【Rust 语法】结构体变体：大括号内是命名字段，相当于枚举成员携带一个匿名结构体。
    Execute {
        is_pressed: bool, // 是否按下
        mapping: KeyMapping, // 需要执行注入的键映射
        layer_id: String, // 映射命中的层 id（需求6：记录鼠标锁存来源层）
    },
    /// 松开激活层的按钮：该层已停用，需释放该层内产生的鼠标左右键锁存（需求6）
    DeactivateLayer(String),
    /// 切换映射启停请求（ToggleMapping）
    ToggleMapping, // 切换映射启停请求
    /// 切换屏幕键盘请求（ToggleOnScreenKeyboard）
    ToggleOnScreenKeyboard, // 切换屏幕键盘请求
    /// 切换悬浮窗请求（ToggleOverlay）
    ToggleOverlay, // 切换悬浮窗请求
} // 枚举定义结束

/// SteamInput —— 映射引擎
// 【Rust 语法】pub struct 定义公开结构体；带 pub 的字段对外可见，私有字段外部不可直接访问。
pub struct SteamInput {
    /// 当前配置（操作集列表 + 当前激活操作集 + 全局设置）
    pub profile: ControllerProfile, // 公开字段：当前配置
    /// UI 事件通道（通知界面/悬浮窗状态变化）
    event_tx: Sender<UiEvent>, // 【Rust 语法】Sender<UiEvent> 泛型：发送 UiEvent 类型消息的通道发送端（私有字段）
    /// 已激活操作层 id（按下顺序，后加入的优先级更高）
    active_layers: Vec<String>, // 【Rust 语法】Vec<String> 动态数组（类似 Qt 的 QVector<std::string>）；String 为可变 UTF-8 字符串
    /// 记录「哪个按键激活了哪个层」，松开该按键时停用对应层
    button_triggered_layers: HashMap<ControllerButton, String>, // 【Rust 语法】HashMap<K, V> 键值映射（类似 QHash）
    /// 当前物理按下的手柄按键集合
    held_buttons: HashSet<ControllerButton>, // 手柄按键集合
    /// 当前激活层 id（未激活任何操作层时为 "Common"）
    active_layer_name: String, // 当前激活层名
} // 结构体定义结束

// 【Rust 语法】impl SteamInput { ... }：为 SteamInput 定义实现块（类似 C++ 类内成员函数定义）。
impl SteamInput {
    // 【Rust 语法】pub fn new(event_tx: Sender<UiEvent>) -> Self：关联函数（构造函数约定），
    // 接收通道发送端参数，返回 Self（即 SteamInput）。
    pub fn new(event_tx: Sender<UiEvent>) -> Self { // 构造映射引擎
        Self { // 构造 Self 结构体
            profile: ControllerProfile::create_default(), // 【Rust 语法】:: 调用类型关联函数：创建默认配置（类似 C++ 静态方法）
            event_tx, // 【Rust 语法】字段简写：`event_tx: event_tx` 可省略为 `event_tx`
            active_layers: Vec::new(), // 空激活层栈
            button_triggered_layers: HashMap::new(), // 空触发层记录
            held_buttons: HashSet::new(), // 空按持集合
            active_layer_name: "Common".to_string(), // 【Rust 语法】"..." 是字符串字面量（类型 &str）；
            // .to_string() 方法把 &str 转为拥有所有权的 String。
        } // Self 构造结束
    } // new 结束

    /// 整体替换配置（启动加载配置、重置默认时调用），同时清空所有激活层
    // 【Rust 语法】&mut self：对当前实例的可变借用（方法可修改内部状态）。
    pub fn load_profile(&mut self, new_profile: ControllerProfile) { // 加载配置
        self.profile = new_profile; // 整体替换 profile 字段
        self.deactivate_all_layers(); // 清空所有已激活层（防止旧配置的层映射残留）
        self.emit(UiEvent::ProfileChanged); // 通知 UI：配置已变更
    } // 方法结束

    /// 仅更新全局设置（界面滑块实时调整时调用，不重置已激活层）
    pub fn set_global_settings(&mut self, settings: crate::core::mapping_types::GlobalSettings) { // 更新全局设置
        self.profile.global_settings = settings; // 直接替换全局设置字段
        self.emit(UiEvent::ProfileChanged); // 通知 UI 配置变更
    } // 方法结束

    // -----------------------------------------------------------------
    // 操作集管理
    // -----------------------------------------------------------------

    /// 切换当前操作集（按 id）：清空已激活层栈（旧操作集内的层不再有效）。
    /// id 无效返回 false（不产生任何副作用）。
    // 【Rust 语法】参数 &str 是字符串的不可变引用（借用，不拥有数据）；-> bool 返回布尔。
    pub fn switch_operation_set(&mut self, set_id: &str) -> bool { // 切换操作集
        if !self.profile.set_active_operation_set(set_id) { // 尝试把指定 id 设为当前操作集，失败（不存在）则进入分支
            return false; // 操作集不存在，直接返回失败
        } // 失败分支结束
        if self.profile.active_operation_set_id != set_id { // 防御性检查：设置后 id 应一致
            return false; // 防御
        } // 防御分支结束
        self.deactivate_all_layers(); // 清空激活层（旧操作集内的层全部失效）
        self.emit(UiEvent::ProfileChanged); // 通知 UI 配置变更
        self.emit(UiEvent::OperationSetChanged(self.profile.active_operation_set_name())); // 【Rust 语法】枚举变体携带数据：
        // 把当前操作集名称作为参数包进事件变体，再发送给 UI。
        true // 最后一个表达式作为返回值：切换成功
    } // 方法结束

    /// 操作集结构变化后统一通知（新增/删除/复制/重命名后调用）。
    /// 调用方须先 deactivate_all_layers()。
    pub fn notify_operation_set_changed(&mut self) { // 通知操作集变化
        self.emit(UiEvent::ProfileChanged); // 通知 UI 配置变更
        self.emit(UiEvent::OperationSetChanged(self.profile.active_operation_set_name())); // 通知当前操作集名变化
    } // 方法结束

    // -----------------------------------------------------------------
    // 层管理
    // -----------------------------------------------------------------

    /// 激活一个操作层（追加到栈顶，优先级最高）。
    /// 忽略空 id/Common（公共层不可"激活"）；重复激活同一层被忽略。
    pub fn activate_layer(&mut self, layer_id: &str) { // 激活层
        if layer_id.is_empty() || layer_id == "Common" { // 【Rust 语法】|| 为逻辑或；is_empty() 判断字符串是否为空
            return; // 空 id 或 Common 层不可激活
        } // 非法 id 分支结束
        if !self.profile.find_layer(layer_id).is_some() { // 【Rust 语法】find_layer 返回 Option<&OperationLayer>；Option 表示「可能有值（Some）或没有（None）」；
        // .is_some() 判断是否为 Some，! 取反表示「层不存在」。
            return; // 层不存在（当前操作集内）
        } // 层不存在分支结束
        if !self.active_layers.contains(&layer_id.to_string()) { // 【Rust 语法】Vec::contains 判断是否含某元素（参数传引用 &）；to_string() 把 &str 转 String
            self.active_layers.push(layer_id.to_string()); // 【Rust 语法】Vec::push 在尾部追加元素（栈顶，优先级最高）
            self.update_active_layer_name(); // 更新当前激活层名
        } // 新层分支结束
    } // 方法结束

    /// 停用一个操作层（从栈中移除所有匹配项）
    pub fn deactivate_layer(&mut self, layer_id: &str) { // 停用层
        let before = self.active_layers.len(); // 【Rust 语法】let 绑定变量；len() 返回元素个数（记录移除前长度）
        self.active_layers.retain(|id| id != layer_id); // 【Rust 语法】Vec::retain(闭包)：保留满足闭包条件的元素、移除其余；
        // 闭包 |id| id != layer_id 是匿名函数（类似 C++ lambda）：参数为每个元素的引用，返回 bool 决定去留。
        if self.active_layers.len() != before { // 长度变化说明确实移除了元素
            self.update_active_layer_name(); // 更新激活层名
        } // 变化分支结束
    } // 方法结束

    /// 停用所有操作层，回到公共层；同时清空触发层记录
    pub fn deactivate_all_layers(&mut self) { // 停用全部层
        self.active_layers.clear(); // 清空激活层栈
        self.button_triggered_layers.clear(); // 清空触发层记录
        self.update_active_layer_name(); // 更新激活层名（回落为 Common）
    } // 方法结束

    /// 指定层（按 id）当前是否激活
    pub fn is_layer_active(&self, id: &str) -> bool { // 判断层是否激活
        self.active_layers.iter().any(|l| l == id) // 【Rust 语法】迭代器适配器 .any(闭包)：只要有一个元素满足闭包条件即返回 true
    } // 方法结束

    /// 当前激活层 id（未激活任何操作层时为 "Common"）
    pub fn active_layer_name(&self) -> &str { // 返回当前激活层名
        &self.active_layer_name // 【Rust 语法】& 借用：返回内部 String 的引用 &str（借用 self 的生命周期，调用方不拥有数据）
    } // 方法结束

    /// 当前物理按下的手柄按键集合
    pub fn held_buttons(&self) -> &HashSet<ControllerButton> { // 返回按持集合引用
        &self.held_buttons // 返回内部集合的不可变引用（避免拷贝）
    } // 方法结束

    /// 清空物理按持记录（手柄断开/停止映射时调用，避免状态残留）
    pub fn clear_held_buttons(&mut self) { // 清空按持记录
        self.held_buttons.clear(); // 清空集合
    } // 方法结束

    /// 当前激活层列表（按激活顺序，公共层不在其中）
    // 【Rust 语法】返回类型 Vec<&OperationLayer>：存放 OperationLayer 引用的动态数组（借用而非复制数据）。
    pub fn get_active_layers(&self) -> Vec<&OperationLayer> { // 获取激活层列表
        self.active_layers // 从激活层 id 列表出发
            .iter() // 【Rust 语法】.iter() 生成元素的迭代器
            .filter_map(|id| self.profile.find_layer(id)) // 【Rust 语法】filter_map(闭包)：闭包返回 Option；Some 解包保留、None 过滤掉（层可能已被删除）
            .collect() // 【Rust 语法】collect() 把迭代器收集为容器；目标类型由函数返回类型 Vec<&OperationLayer> 推断
    } // 方法结束

    // -----------------------------------------------------------------
    // 查询
    // -----------------------------------------------------------------

    /// 查询按钮在当前层栈下的有效映射：
    /// 从最后激活的操作层开始，逐层回退到公共层，返回第一个命中。
    // 【Rust 语法】返回类型 Option<KeyMapping>：可能为 Some(映射) 或 None（无映射）；调用方用模式匹配解包。
    pub fn get_effective_mapping(&self, button: ControllerButton) -> Option<KeyMapping> { // 查询有效映射
        self.get_effective_mapping_with_layer(button) // 复用带层 id 的查询，丢弃层 id
            .map(|(m, _)| m) // 【Rust 语法】Option::map：取出元组中的映射部分
    } // 方法结束

    /// 同 get_effective_mapping，但额外返回命中映射所在的层 id（需求6 记录鼠标锁存来源层）。
    pub fn get_effective_mapping_with_layer(
        &self,
        button: ControllerButton,
    ) -> Option<(KeyMapping, String)> {
        for id in self.active_layers.iter().rev() { // 【Rust 语法】.rev() 反转迭代器（从最后激活的层/栈顶开始）；for 循环遍历
            if let Some(layer) = self.profile.find_layer(id) { // 【Rust 语法】if let 模式匹配：若 find_layer 返回 Some 则解包为 layer 进入分支；None 则跳过
                if let Some(m) = layer.get_mapping(button) { // 若该层存在此按钮的映射
                    return Some((m.clone(), id.clone())); // 同时返回映射与层 id
                } // 命中分支结束
            } // 层存在分支结束
        } // 循环结束
        self.profile // 操作层栈未命中，回退到公共层
            .common_layer() // 获取公共层（返回 Option<&OperationLayer>）
            .and_then(|cl| cl.get_mapping(button)) // 【Rust 语法】Option::and_then(闭包)：Some 时调用闭包（闭包也返回 Option），并自动扁平化嵌套的 Option
            .map(|m| (m.clone(), "Common".to_string())) // 公共层映射的来源层记为 Common
    } // 方法结束

    // -----------------------------------------------------------------
    // 输入入口（由手柄读取源调用）
    // -----------------------------------------------------------------

    /// 按钮按下/松开事件；SwitchLayer 动作在此处理（按住激活/松开回退），
    /// 其余动作通过返回值交给映射器执行。
    pub fn handle_button_event(&mut self, button: ControllerButton, is_pressed: bool) -> ButtonDispatch { // 按钮事件入口
        if is_pressed { // 按下事件
            self.held_buttons.insert(button); // 记录到按持集合
        } else { // 松开事件
            self.held_buttons.remove(&button); // 从按持集合移除
        } // 分支结束

        // 松开时：若该按键激活了某个层，停用该层并通知映射器释放该层内产生的鼠标锁存
        if !is_pressed { // 仅在松开时处理层停用
            if let Some(triggered_id) = self.button_triggered_layers.remove(&button) { // 【Rust 语法】HashMap::remove 返回 Option<V>；
            // if let Some(...) 解包：若该按钮之前激活过某层则取出其 id。
                self.deactivate_layer(&triggered_id); // 停用该按钮激活的层
                return ButtonDispatch::DeactivateLayer(triggered_id); // 通知映射器释放该层内产生的鼠标左右键锁存（需求6）
            } // 触发层分支结束
        } // 松开分支结束

        // 查询当前层栈下的有效映射；无映射则不产生任何事件
        let Some((mapping, layer_id)) = self.get_effective_mapping_with_layer(button) else { // 【Rust 语法】let-else 语句：若 Option 为 Some 则解包为 mapping/layer_id 继续执行；
        // 若为 None 则必须执行 else 块并离开（此处 else 块内用 return 返回）。
            // 松开时：即使该按钮在「松开时刻」的层栈下已无映射，仍要广播松开事件，
            // 让映射器按「已注入状态」精确释放（防止此前注入的按键卡死）。
            if !is_pressed { // 松开场景
                return ButtonDispatch::Execute { // 返回执行事件
                    is_pressed, // 【Rust 语法】字段简写：is_pressed: is_pressed
                    mapping: KeyMapping::new(crate::core::mapping_types::MappedAction::mouse_move()), // 虚拟空动作（仅用于触发精确释放）
                    layer_id: String::new(), // 无映射来源层
                }; // Execute 变体构造结束
            } // 松开分支结束
            return ButtonDispatch::None; // 按下且无映射：无动作
        }; // 【Rust 语法】let-else 结束：此语句之后 mapping / layer_id 已绑定

        // 切换层动作由引擎处理：按住激活目标层并记录触发按钮
        if mapping.action.r#type == ActionType::SwitchLayer { // 【Rust 语法】r#type 原始标识符访问字段（type 是关键字）；== 比较枚举值
            if is_pressed { // 仅在按下时激活层
                let target = mapping.action.layer_name.clone().unwrap_or_default(); // 【Rust 语法】Option::unwrap_or_default()：
                // Some 取内部值，None 取该类型默认值（String 默认空串）。
                // 先按 id 查找；找不到则按 name 查找（兼容旧配置）
                let target_id = self // 解析目标层 id
                    .profile // 访问当前配置
                    .find_layer(&target) // 先按 id 查找目标层
                    .map(|l| l.id.clone()) // 【Rust 语法】Option::map(闭包)：Some 时把内部值转换（取出层 id 克隆）
                    .or_else(|| { // 【Rust 语法】Option::or_else(闭包)：None 时执行闭包尝试备选方案
                        self.profile // 备选方案：按名称查找
                            .layers() // 获取当前操作集的所有层
                            .into_iter() // 【Rust 语法】into_iter() 生成取得元素所有权的迭代器
                            .find(|l| l.name == target) // 【Rust 语法】Iterator::find(闭包)：返回第一个满足条件的元素（Option）
                            .map(|l| l.id.clone()) // 取找到层的 id 克隆
                    }); // or_else 闭包结束
                if let Some(tid) = target_id { // 若成功解析出目标层 id
                    if !self.is_layer_active(&tid) { // 目标层尚未激活
                        self.activate_layer(&tid); // 激活目标层
                        self.button_triggered_layers.insert(button, tid); // 【Rust 语法】HashMap::insert：记录「该按钮 → 激活的层」，松开时据此停用
                    } // 未激活分支结束
                } // 有目标分支结束
            } // 按下分支结束
            return ButtonDispatch::None; // 切换层动作不交给映射器执行注入
        } // SwitchLayer 分支结束

        // 切换类动作：仅在按下时触发，松开忽略
        if is_pressed { // 仅按下时处理切换类动作
            match mapping.action.r#type { // 匹配动作类型
                ActionType::ToggleMapping => return ButtonDispatch::ToggleMapping, // 切换映射启停
                ActionType::ToggleOnScreenKeyboard => { // 切换屏幕键盘
                    return ButtonDispatch::ToggleOnScreenKeyboard; // 返回切换请求
                } // 屏幕键盘分支结束
                ActionType::ToggleOverlay => return ButtonDispatch::ToggleOverlay, // 切换悬浮窗
                _ => {} // 【Rust 语法】通配分支 + 空块：其他动作类型忽略
            } // match 结束
        } // 按下分支结束

        // 其余动作（键盘/鼠标/视角等）交给映射器执行
        ButtonDispatch::Execute { // 返回执行事件
            is_pressed, // 字段简写
            mapping, // 字段简写：把查询到的映射交给映射器
            layer_id, // 映射命中层 id（需求6）
        } // Execute 变体构造结束
    } // 方法结束

    /// 摇杆输入入口：先应用全局死区（缩放式），再返回处理后值。
    // 【Rust 语法】多行函数签名：参数逐个换行，末尾 -> (ControllerStick, f32, f32) 表示返回三元组。
    pub fn handle_stick_input( // 摇杆输入入口
        &mut self, // 可变借用 self
        stick: ControllerStick, // 摇杆标识（左/右摇杆）
        x: f32, // 摇杆原始 X 值
        y: f32, // 摇杆原始 Y 值
    ) -> (ControllerStick, f32, f32) { // 返回三元组（摇杆标识 + 处理后的 X/Y）
        let dz = self.profile.global_settings.deadzone; // 读取全局死区值
        let d = Vector2::new(x, y).with_deadzone(dz); // 【Rust 语法】链式方法调用：构造二维向量并对死区做缩放处理
        let mut out_x = d.x; // 【Rust 语法】let mut 可变绑定：初始化输出 X
        let mut out_y = d.y; // 初始化输出 Y
        if stick == ControllerStick::RightStick { // 仅对右摇杆（视角）应用反转设置
            if self.profile.global_settings.invert_look_x { // 水平反转开启
                out_x = -out_x; // X 取反
            } // 反转分支结束
            if self.profile.global_settings.invert_look_y { // 垂直反转开启
                out_y = -out_y; // Y 取反
            } // 反转分支结束
        } // 右摇杆分支结束
        (stick, out_x, out_y) // 最后一个表达式作为返回值：返回三元组（无需 return）
    } // 方法结束

    // -----------------------------------------------------------------
    // 私有辅助
    // -----------------------------------------------------------------

    /// 重新计算当前激活层名（栈顶层的显示名；无激活层时为 "Common"），
    /// 变化时发出 LayerChanged 事件。
    fn update_active_layer_name(&mut self) { // 私有方法：更新激活层名
        let name = match self.active_layers.last() { // 【Rust 语法】Vec::last() 返回 Option<&T>（栈顶元素引用）；用 match 匹配该 Option
            Some(id) => self // 栈顶有元素（Some）
                .profile // 访问配置
                .find_layer(id) // 查找该层
                .map(|l| l.name.clone()) // 取层名克隆
                .unwrap_or_else(|| "Common".to_string()), // 【Rust 语法】Option::unwrap_or_else(闭包)：None 时执行闭包得到默认值
            None => "Common".to_string(), // 无激活层时显示名默认 Common
        }; // match 结束，name 已绑定
        if name != self.active_layer_name { // 层名发生变化
            let changed = name.clone(); // 克隆一份供事件使用
            self.active_layer_name = name; // 更新内部层名
            self.emit(UiEvent::LayerChanged(changed)); // 通知 UI：激活层已变更
        } // 变化分支结束
    } // 方法结束

    fn emit(&self, event: UiEvent) { // 私有方法：发送 UI 事件
        // 无界 mpsc 通道的 send 不阻塞（receiver 断开时返回 Err，忽略即可）
        let _ = self.event_tx.send(event); // 【Rust 语法】send 返回 Result；let _ = 用通配符 _ 显式丢弃返回值，避免 unused 警告
    } // 方法结束
} // impl SteamInput 块结束
