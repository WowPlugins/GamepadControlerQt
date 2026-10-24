// =====================================================================
// main.rs —— 程序入口（Tauri）
//
// 装配顺序：
//   1. 构建共享状态（AppCore + LookRunner + XInputGamepadSource）
//   2. 从 exe 同目录加载配置文件（无则生成默认）
//   3. 启动 Tauri：主窗口（自动创建）+ 悬浮窗（setup 中创建，默认隐藏）
// 退出：RunEvent::Exit 统一钩子——停止映射释放注入 + 自动保存配置
//
// 数据流：XInputGamepadSource → AppCore(SteamInput→Mapper) → SendInput
//          └─ 前端(WebView) ── invoke 命令 ──> 后端(commands.rs)
// =====================================================================

// 【Rust 语法】内部属性（crate 级属性，以 #! 开头作用于整个 crate）：指定 Windows 子系统为 "windows"，编译为 GUI 程序，不弹出控制台窗口
#![windows_subsystem = "windows"]

// 【Rust 语法】mod 声明子模块：Rust 会在同目录自动查找对应文件（commands.rs / core 目录 / ui 目录）
mod commands; // 声明 Tauri IPC 命令模块（对应 commands.rs）
mod core; // 声明核心模块（映射引擎、配置管理、输入类型）
mod ui; // 声明 UI 模块（共享状态 AppShared 等）

// 【Rust 语法】use 导入：把外部 crate / 标准库的类型引入当前作用域，简化调用路径；花括号内为多个项
use std::sync::atomic::AtomicBool; // 原子布尔：跨线程安全地读写布尔标志
use std::sync::{Arc, Mutex}; // Arc（引用计数共享所有权）+ Mutex（互斥锁，提供多线程安全可变访问）
use tauri::Manager; // Tauri 的 Manager trait：提供 state() / get_webview_window() 等实例方法
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS}; // Windows API：获取最近错误码 + "已存在"错误常量
use windows::Win32::System::Threading::CreateMutexW; // Windows API：创建命名互斥体，用于单实例检测
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, MrmPlatformVersion_Windows10_0_0_0, SW_RESTORE, SetForegroundWindow, ShowWindow}; // Windows API：查找窗口、置顶聚焦、控制显隐

/// 已有实例运行时聚焦其主窗口（按主窗口标题查找并恢复/置顶）
// 【Rust 语法】fn 定义函数：无参数、无返回值；调用 Windows 原生 API（FFI）是不安全操作，必须放在 unsafe 块中
fn focus_existing_main_window() { // 已有实例时聚焦其主窗口
    unsafe { // 进入不安全块，允许调用原生 Windows API
        // 按窗口标题查找已有实例的主窗口（FindWindowW 返回 Result，失败即未找到）
        // 【Rust 语法】let-else 语句：模式匹配 Result，匹配 Ok(hwnd) 则绑定 hwnd，否则执行 else 分支
        let Ok(hwnd) = FindWindowW(None, windows::core::w!("Gamepad 键鼠映射")) else { // 按窗口标题查找主窗口
            return; // 未找到主窗口则不做聚焦
        }; // let-else 语句结束
        // 【Rust 语法】HWND 是包装结构体，.0 访问内部原始句柄；is_null() 判断句柄是否为空
        if !hwnd.0.is_null() { // 句柄非空（说明窗口存在）才继续处理
            ShowWindow(hwnd, SW_RESTORE); // 最小化则恢复显示
            let _ = SetForegroundWindow(hwnd); // 置顶聚焦
        } // if 结束
    } // unsafe 块结束
} // 函数结束

/// 全局共享状态：跨线程共享的核心 + 悬浮窗显隐标志 + 悬浮窗透明度
// 【Rust 语法】struct 结构体：pub 公开声明，含具名字段；该类型经 .manage() 注入 Tauri 状态，命令中可用 State<AppState> 获取
pub struct AppState { // 全局共享状态结构体
    pub shared: Arc<ui::shared::AppShared>, // 核心共享对象：Arc 使其可在多个线程间共享所有权
    pub overlay_visible: Arc<AtomicBool>, // 悬浮窗显隐标志：AtomicBool 原子布尔，无需锁即可跨线程读写
} // 结构体定义结束

// 【Rust 语法】fn main 程序入口：crate 根函数，程序从此处开始执行
fn main() { // 程序主入口
    // ---- 0. 单进程限制：命名互斥体检测是否已有实例在运行 ----
    // 【Rust 语法】let 变量绑定 + 块表达式：= 右侧是代码块，块的最后一行（handle）即为表达式的值
    let _single_instance = { // 变量名以下划线开头：避免"未使用变量"警告，仅用于持有互斥体句柄
        // 创建命名互斥体；若已存在（ERROR_ALREADY_EXISTS）说明已有实例在运行。
        // 名称位于 Global 命名空间，多个进程共享（w! 宏只能接收字符串字面量）
        let handle = unsafe { // 在 unsafe 块内调用创建互斥体的 API
            CreateMutexW(None, true, windows::core::w!("Global\\GamepadControlerTauriSingleInstance")) // 创建命名互斥体并请求初始所有权
        }; // unsafe 块结束，返回值绑定到 handle
        // 【Rust 语法】比较运算：== 返回布尔；GetLastError 返回上次调用失败的错误码
        let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS; // 互斥体已存在则说明已有实例在运行
        if already_running { // 若已有实例在运行
            // 已有实例 → 聚焦其主窗口后直接退出本实例
            focus_existing_main_window(); // 聚焦已有实例的主窗口
            return; // 直接退出当前（第二个）实例
        } // if 结束
        // 首次实例：持有互斥体句柄直到进程退出（否则句柄释放后互斥体会被销毁）
        handle // 块表达式的值：把句柄交给 _single_instance 长期持有
    }; // 块表达式结束
    let _ = _single_instance; // 显式消费变量，避免未使用警告（让句柄存活到 main 结束）

    // ---- 1. 构建共享状态并加载配置 ----
    // 【Rust 语法】:: 路径调用：ui::shared::AppShared::new() 是模块内类型上的关联函数；Arc::new 把对象放堆上并共享所有权
    let shared = Arc::new(ui::shared::AppShared::new()); // 构建跨线程共享的核心状态
    { // 【Rust 语法】独立代码块：限定变量作用域，使锁在块结束时自动释放
        let profile = core::config_manager::load(); // 从 exe 同目录加载配置文件，返回 ControllerProfile
        // 【Rust 语法】if-let 模式匹配：尝试获取 Mutex 锁；Ok 则解出可变引用 core，Err（锁中毒）则整块跳过
        if let Ok(mut core) = shared.core.lock() { // 加锁成功后取得核心的可变访问
            core.load_profile(profile); // 把加载的配置载入核心
        } // if-let 结束（此处锁自动释放）
    } // 作用域块结束

    // ---- 2. 启动 Tauri ----
    // 【Rust 语法】方法链：Builder::default() 构造默认构建器，后续逐方法配置
    tauri::Builder::default() // 创建 Tauri 应用构建器
        // 【Rust 语法】.manage()：把 AppState 实例注入 Tauri 状态管理，命令中可用 State<AppState> 获取
        .manage(AppState { // 注入全局共享状态
            shared, // 字段简写：等价于 shared: shared，把共享状态移入
            overlay_visible: Arc::new(AtomicBool::new(false)), // 新建原子布尔并初始化为 false（悬浮窗默认隐藏）
        }) // AppState 结构体字面量结束
        // 【Rust 语法】属性宏 tauri::generate_handler!：把命令函数列表编译为前端 invoke 的分发器
        .invoke_handler(tauri::generate_handler![ // 注册所有前端可调用的 IPC 命令
            commands::get_snapshot, // 主窗口整体快照
            commands::get_overlay_snapshot, // 悬浮窗快照
            commands::start_mapping, // 开始映射
            commands::stop_mapping, // 停止映射
            commands::add_operation_set, // 新增操作集
            commands::rename_operation_set, // 重命名操作集
            commands::copy_operation_set, // 复制操作集
            commands::delete_operation_set, // 删除操作集
            commands::switch_operation_set, // 切换操作集
            commands::adjust_setting, // 调整全局设置
            commands::save_config, // 保存配置
            commands::reset_config, // 重置配置
            commands::toggle_overlay, // 切换悬浮窗显隐
            commands::set_overlay_opacity, // 设置悬浮窗透明度
            commands::quit_app, // 退出应用
            commands::get_layer_edit_snapshot, // 层编辑快照
            commands::get_mapping, // 读取按键映射
            commands::set_mapping, // 写入按键映射
            commands::clear_mapping, // 清空按键映射
            commands::toggle_sub, // 切换子命令
            commands::open_app, // 打开应用
            commands::rename_layer, // 重命名层集
            commands::change_layer_trigger_button, // 改变层的激活按钮
            commands::set_long_press, // 配置长按动作
            commands::set_long_press_ms, // 设置长按判定阈值
            commands::toggle_long_press_sub, // 切换长按子命令
            commands::pick_wow_exe, // 浏览选择 WoW 可执行文件
            commands::set_wow_exe, // 从历史记录选择 WoW 可执行文件
            commands::get_overlay_opacity,
        ]) // 命令数组宏结束
        // 【Rust 语法】闭包：|app| 为参数列表；在应用初始化完成后、运行前回调，用于创建额外窗口
        .setup(|app| { 
            // setup 钩子：创建悬浮窗
            // 悬浮窗：无边框透明置顶小窗，默认隐藏；前端 overlay.html 负责渲染（圆角由 CSS 实现）
            let _overlay = tauri::WebviewWindowBuilder::new( // 创建 Webview 窗口构建器（句柄无需使用，前缀 _ 避免未用警告）
                app, // 传入应用句柄
                "overlay", // 窗口标签名，供 get_webview_window("overlay") 查找
                tauri::WebviewUrl::App("overlay.html".into()), // 加载前端打包资源中的 overlay.html
            ) // WebviewWindowBuilder::new 调用结束
            .title("手柄悬浮窗") // 设置窗口标题
            .inner_size(340.0, 220.0) // 设置窗口内部尺寸为 320x220
            .resizable(false) // 禁止窗口缩放
            .decorations(false) // 无系统边框与标题栏
            .transparent(true) // 启用窗口透明背景
            .always_on_top(true) // 窗口始终置顶
            .skip_taskbar(true) // 不在任务栏显示
            .visible(false) // 默认隐藏，由 toggle_overlay 控制显隐
            // 【Rust 语法】? 错误传播：Result 为 Ok 则取值，Err 则从 setup 闭包提前返回该错误
            .build()?; // 构建窗口，失败则 ? 传播错误
            Ok(()) // 【Rust 语法】Ok 包装单元类型 ()：表示 setup 处理成功
        }) // setup 闭包结束
        // 【Rust 语法】generate_context! 宏：编译期生成应用上下文（含前端资源）；expect 在 Err 时 panic 并打印信息
        .build(tauri::generate_context!()) // 构建 Tauri 应用（内嵌前端资源）
        .expect("error while running tauri application") // 构建失败则崩溃并提示错误
        // 【Rust 语法】闭包：参数 app_handle（应用句柄）与 event（运行事件枚举）
        .run(|app_handle, event| { // 启动应用事件循环
            // 【Rust 语法】match 匹配枚举：主窗口点 X → 退出整个程序；应用退出 → 清理
            match event {
                // 主窗口点 X：exit(0) 会销毁包括 overlay 在内的所有窗口 →
                // 窗口表清空 → 触发 ExitRequested → Exit（下方清理分支照常执行）
                tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { .. }, .. } if label == "main" => {
                    app_handle.exit(0); // 以退出码 0 结束整个应用事件循环
                }
                tauri::RunEvent::Exit => { // 应用退出前：停止映射释放全部注入（防止卡键）+ 自动保存配置文件
                    let state = app_handle.state::<AppState>(); // 从应用句柄取回 manage 注入的 AppState
                    state.shared.stop_mapping(); // 释放所有按键注入，防止卡键
                    // 【Rust 语法】块表达式：块内最后一行作为值赋给 profile
                    let profile = { // 取当前配置快照
                        // 【Rust 语法】unwrap()：从 Result 取出 Ok 值，Err（锁中毒）则 panic
                        let core = state.shared.core.lock().unwrap(); // 对核心加锁
                        core.steam.profile.clone() // 取当前配置快照
                    }; // 块结束，profile 拿到配置副本（此处锁已释放）
                    core::config_manager::save(&profile); // 自动保存到配置文件
                }
                _ => {} // 其余运行事件不处理
            } // match 结束
        }); // run 调用与闭包结束
} // main 函数结束
