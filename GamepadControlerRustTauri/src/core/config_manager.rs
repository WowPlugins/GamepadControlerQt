// =====================================================================
// config_manager.rs —— 配置文件读写管理
//
// 等效 Qt 版 ConfigManager.h/.cpp。
// 配置文件固定放在可执行文件同目录（绿色便携，不写注册表/用户目录），
// 文件名与安卓版保持一致（steamlike_config.json）。
//
// 加载策略：
//   - 文件存在且可正常解析 -> 返回解析结果
//   - 文件不存在 / 解析失败 / 版本不匹配 -> 回退到默认配置并保存
// =====================================================================

// 【Rust 语法】use 语句：从本 crate 的 core::config 模块导入配置序列化相关函数（JSON 与配置互转）
use crate::core::config;
// 【Rust 语法】use 语句：从 core::mapping_types 模块导入 ControllerProfile 类型（控制器配置档案）
use crate::core::mapping_types::ControllerProfile;
// 【Rust 语法】use 语句：从标准库 std::path 导入 PathBuf 类型（跨平台文件路径）
use std::path::PathBuf;
// 【Rust 语法】use 语句：导入 Mutex（互斥锁），用于串行化配置文件写入
use std::sync::Mutex;

// 【Rust 语法】const 定义编译期常量；&str 是不可变的字符串切片类型（只读借用），此处存放配置文件名
const CONFIG_FILE_NAME: &str = "steamlike_config.json";

/// 配置文件完整路径（exe 目录 + "steamlike_config.json"）
pub fn config_file_path() -> PathBuf { // 【Rust 语法】pub fn 定义公开函数，-> PathBuf 指定返回类型；函数体最后一个表达式即返回值
    // 【Rust 语法】let 绑定变量；std::env::current_exe() 返回 Result<PathBuf, Error>，获取当前可执行文件的完整路径
    let exe_dir = std::env::current_exe()
        .ok() // 【Rust 语法】Result::ok() 把 Result 转为 Option：Ok(v)->Some(v)、Err->None，出错时不 panic
        .and_then(|p| p.parent().map(|d| d.to_path_buf())) // 【Rust 语法】and_then 链式处理 Option；闭包 |p| 取父目录，再用 map 转为 PathBuf
        .unwrap_or_else(|| PathBuf::from(".")); // 【Rust 语法】unwrap_or_else：None 时执行无参闭包 || 兜底为当前目录 "."
    exe_dir.join(CONFIG_FILE_NAME) // PathBuf::join 把文件名拼接进目录，得到最终配置路径并返回
}

/// 配置文件是否存在
pub fn has_config_file() -> bool { // 【Rust 语法】返回类型为 bool；函数体只有一行表达式，直接作为返回值
    config_file_path().exists() // PathBuf::exists() 检查文件是否存在
}

/// 加载配置：文件不存在或解析失败时回退到默认配置并保存
pub fn load() -> ControllerProfile { // 【Rust 语法】返回类型为 ControllerProfile；本模块的核心加载逻辑
    let path = config_file_path(); // 取得配置文件完整路径
    if path.exists() { // 【Rust 语法】if 表达式（Rust 中 if 是表达式而非语句）；文件存在才尝试读取
        // 【Rust 语法】if let Ok(text) = ...：对 Result 做模式匹配，读取成功（Ok）时把文本绑定到 text
        if let Ok(text) = std::fs::read_to_string(&path) {
            // 【Rust 语法】if let Ok(profile) = ...：解析成功（Ok）时把 ControllerProfile 绑定到 profile
            if let Ok(profile) = config::profile_from_json_str(&text) {
                return profile; // return 显式返回解析出的配置
            }
        }
    }
    let def = ControllerProfile::create_default(); // 走到这说明读取/解析失败，创建默认配置（关联函数 = 类似静态方法）
    save(&def); // 【Rust 语法】&def 为不可变借用（不转移所有权），把默认配置保存到文件
    def // 返回默认配置（最后一个表达式，无 return 也能返回）
}

/// 保存配置到文件，成功返回 true
pub fn save(profile: &ControllerProfile) -> bool { // 【Rust 语法】参数 profile: &ControllerProfile 为不可变借用，不拥有数据
    // 写入锁：自动保存线程与退出保存可能同时写文件，串行化避免内容交错损坏
    static SAVE_LOCK: Mutex<()> = Mutex::new(()); // 【Rust 语法】函数内 static：全局只初始化一次
    let _guard = SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner()); // 锁中毒时也继续（取出内部值）
    let text = config::profile_to_json_string(profile); // 把配置对象序列化成 JSON 字符串
    std::fs::write(config_file_path(), text).is_ok() // 写入文件；Result::is_ok() 判断是否成功并作为返回值
}

/// 重置为默认配置并保存
pub fn reset_to_default() { // 【Rust 语法】无参数、无 -> 返回类型，隐式返回单元类型 ()
    let def = ControllerProfile::create_default(); // 创建一份默认配置
    save(&def); // 复用 save 函数把默认配置落盘
}
