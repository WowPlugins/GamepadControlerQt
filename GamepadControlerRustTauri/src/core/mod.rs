// =====================================================================
// core —— 核心逻辑模块
//
// 数据流（单方向）：
//   XInputGamepadSource → SteamInput → KeyboardMouseMapper → InputInjector
//                      (轮询线程)        (映射引擎)          (注入)
//
// AppCore 组合 SteamInput + MapperState + 注入器 + 视角状态，并由手柄
// 轮询线程与 UI 线程通过同一把 Mutex 串行访问，保证并发安全。
// =====================================================================

// 【Rust 语法】pub mod：声明公开子模块，其他模块可通过 crate::core::app 访问该子模块
pub mod app;
// 【Rust 语法】pub mod：声明公开子模块 config（JSON 配置序列化）
pub mod config;
// 【Rust 语法】pub mod：声明公开子模块 config_manager（配置文件读写管理）
pub mod config_manager;
// 【Rust 语法】pub mod：声明公开子模块 input_types（手柄/鼠标输入类型定义）
pub mod input_types;
// 【Rust 语法】pub mod：声明公开子模块 injector（按键注入器）
pub mod injector;
// 【Rust 语法】pub mod：声明公开子模块 mapper（键鼠映射执行器）
pub mod mapper;
// 【Rust 语法】pub mod：声明公开子模块 mapping_types（映射数据结构定义）
pub mod mapping_types;
// 【Rust 语法】pub mod：声明公开子模块 steam_input（SteamInput 映射引擎）
pub mod steam_input;
// 【Rust 语法】pub mod：声明公开子模块 xinput_source（XInput 手柄读取源）
pub mod xinput_source;
// 【Rust 语法】pub mod：声明公开子模块 wow_sender（按 exe 定位目标窗口并投递键盘消息）
pub mod wow_sender;

// 【Rust 语法】pub use 重导出：把 app::AppCore 提升到 core 模块顶层，外部可直接用 crate::core::AppCore 引用
pub use app::AppCore;

// ---------------------------------------------------------------------
// UiEvent —— 核心 -> UI 的事件（经 std mpsc 通道非阻塞发送）
// UI 线程周期性地 drain 通道，再驱动 gpui 重绘对应界面元素。
// ---------------------------------------------------------------------
// 【Rust 语法】#[derive(...)] 派生宏：自动为 UiEvent 实现 Debug（格式化调试输出）和 Clone（克隆复制）trait
#[derive(Debug, Clone)]
// 【Rust 语法】pub enum：定义公开枚举类型；枚举的每个成员称为变体（variant）
pub enum UiEvent {
    /// 当前激活层显示名变化（切层时）
    // 【Rust 语法】枚举变体可携带数据：LayerChanged 携带一个 String
    LayerChanged(String),
    /// 当前操作集显示名变化（切换/重命名操作集时）
    // 【Rust 语法】枚举变体可携带数据：OperationSetChanged 携带一个 String
    OperationSetChanged(String),
    /// 配置整体变化（层编辑/操作集结构变化/全局设置变化）
    // 【Rust 语法】无数据变体：仅作为标记，不携带任何值
    ProfileChanged,
    /// 手柄连接状态变化
    // 【Rust 语法】枚举变体携带 bool：true 表示已连接，false 表示已断开
    Connected(bool),
    /// 鼠标长按锁存状态变化（悬浮窗橙色警示 / 边框高亮）
    // 【Rust 语法】struct 风格变体：用命名大括号携带多个不同类型字段
    MouseToggleChanged {
        button: input_types::ControllerButton, // 触发锁存的手柄按钮
        mb: input_types::MouseButton, // 被锁存的鼠标按键
        active: bool, // 锁存状态是否激活
    },
    /// 手柄侧发起 ToggleMapping 请求（切映射启停）
    // 【Rust 语法】无数据变体：仅作为事件标记
    ToggleMappingRequested,
    /// 手柄侧发起 ToggleOverlay 请求（悬浮窗显隐）
    // 【Rust 语法】无数据变体：仅作为事件标记
    ToggleOverlayRequested,
}
