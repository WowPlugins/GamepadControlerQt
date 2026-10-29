// =====================================================================
// config.rs —— 配置序列化 / 反序列化（JSON）
//
// 等效 Qt 版 ControllerConfig.h/.cpp。
// JSON 格式与安卓版（version=2）同版本号，同一份配置文件可在
// Windows 版与安卓版之间互换使用。
//
// 顶层结构（新格式，含操作集）：
//   {
//     "version": 2,
//     "globalSettings": { ... },
//     "activeOperationSet": "Set1",
//     "operationSets": [ { "id","name","commonLayer","layers" } ]
//   }
// 兼容：旧 v2 配置（顶层直接 commonLayer/layers）加载时自动包装成
// 单个「默认操作集」，实现无缝升级。
//
// 容错策略：对无法识别的动作类型/按钮名/超限子命令，采取"跳过该条
// 映射"的宽松策略；只有 JSON 语法错误、根节点不是对象、版本号不匹配
// 才返回错误。
// =====================================================================

// 【Rust 语法】`use` 语句：把其他模块中的项引入当前作用域；`crate::core::input_types` 是绝对路径，指向本 crate 内 core 子模块下的 input_types 模块；末尾的 `*`（通配符）表示导入该模块所有公开项。
use crate::core::input_types::*;
// 【Rust 语法】同样用 `use` 导入 `crate::core::mapping_types` 模块的全部公开项（ControllerProfile、OperationSet、MappedAction 等类型都来自这里）。
use crate::core::mapping_types::*;
// 【Rust 语法】`use ...::{a, b}` 花括号形式：同时从 serde_json 导入 `json`（宏，用于快速构造 JSON 值）和 `Value`（枚举，表示任意 JSON 值）。
use serde_json::{json, Value};

// 【Rust 语法】`pub const`：声明公开的编译期常量；`i64 = 2` 是类型注解（64 位有符号整数）和初始值；常量的名字约定用全大写下划线分隔。
pub const CONFIG_VERSION: i64 = 2;

// ---- 动作 type 的字符串常量（与配置文件/安卓版一一对应）----
// 【Rust 语法】`const ... : &str`：字符串字面量类型是 `&str`（字符串切片/借用），`"keyboard"` 与配置文件里的 type 字段值一一对应。
const K_TYPE_KEYBOARD: &str = "keyboard";
const K_TYPE_MOUSE: &str = "mouse";
const K_TYPE_MOUSE_TOGGLE: &str = "mouseToggle";
const K_TYPE_WHEEL_UP: &str = "wheelUp";
const K_TYPE_WHEEL_DOWN: &str = "wheelDown";
const K_TYPE_SWITCH_LAYER: &str = "switchLayer";
const K_TYPE_MOUSE_MOVE: &str = "mouseMove";
const K_TYPE_LOOK_AROUND: &str = "lookAround";
const K_TYPE_TOGGLE_MAPPING: &str = "toggleMapping";
const K_TYPE_TOGGLE_ON_SCREEN_KEYBOARD: &str = "toggleOnScreenKeyboard";
const K_TYPE_TOGGLE_OVERLAY: &str = "toggleOverlay";
const K_TYPE_INTERACTION_KEY: &str = "interactionKey";
const K_TYPE_RELEASE_MOUSE_TOGGLE: &str = "releaseMouseToggle";
const K_TYPE_SWAP_MOUSE_TOGGLE: &str = "swapMouseToggle";
const K_TYPE_FINE_LOOK: &str = "fineLook";
const K_TYPE_TOGGLE_OVERLAY_EXPAND: &str = "toggleOverlayExpand";

// =====================================================================
// 序列化：ControllerProfile -> JSON 文本
// =====================================================================
// 【Rust 语法】`pub fn`：定义公开函数；`profile: &ControllerProfile` 是参数，`&` 表示借用（不获取所有权，只读）；`-> Value` 是返回值类型。函数体最后一个不加分号的表达式就是返回值。
pub fn profile_to_json(profile: &ControllerProfile) -> Value {
    // 【Rust 语法】`let mut`：声明可变变量（后续可修改）；`json!` 是 serde_json 提供的宏，`{ "key": expr }` 语法会按字面构造出一个 JSON 对象（Value::Object）。
    let mut gs = json!({
        // 将 profile 里的全局设置字段逐一映射为 JSON 键值；冒号左侧是 JSON 字符串键，右侧是从结构体字段读出的值。
        "deadzone": profile.global_settings.deadzone,
        "lookSensitivity": profile.global_settings.look_sensitivity,
        "cursorSpeed": profile.global_settings.cursor_speed,
        "lookSmoothing": profile.global_settings.look_smoothing,
        "lookAcceleration": profile.global_settings.look_acceleration,
        "invertLookX": profile.global_settings.invert_look_x,
        "invertLookY": profile.global_settings.invert_look_y,
        "overlayX": profile.global_settings.overlay_x,
        "overlayY": profile.global_settings.overlay_y,
        "overlayScale": profile.global_settings.overlay_scale,
        "mainWindowX": profile.global_settings.main_window_x,
        "mainWindowY": profile.global_settings.main_window_y,
        "releaseOnForegroundChange": profile.global_settings.release_on_foreground_change,
        "confirmOnClose": profile.global_settings.confirm_on_close,
        "overlayOpacity": profile.global_settings.overlay_opacity,
        "wowExe": profile.global_settings.wow_exe,
        "wowExeHistory": profile.global_settings.wow_exe_history,
    });
    // 【Rust 语法】`as_object_mut()`：把 Value 转成 `Option<&mut Map>`（可变借用其中的对象）；`expect("object")`：若是 Some 则取出值，若是 None 则 panic 并输出提示信息（这里对象必然存在）。
    gs.as_object_mut().expect("object");

    // 【Rust 语法】`Vec::new()`：创建空的动态数组；`mut` 使其可变以便后续 push；变量类型由后面的 push 自动推断为 Vec<Value>。
    let mut sets = Vec::new();
    // 【Rust 语法】`for ... in ...`：for 循环；`&profile.operation_sets` 是借用迭代，每次 `set` 都是 `&OperationSet`（引用，不移动所有权）。
    for set in &profile.operation_sets {
        // 每层循环内新建一个空 Vec，用于收集该操作集的所有层 JSON。
        let mut layers = Vec::new();
        // 嵌套 for：遍历当前操作集的 layers 数组（同样是借用）。
        for layer in &set.layers {
            // push 向 Vec 尾部追加元素；`layer_to_json` 把单个层结构体转成 Value。
            layers.push(layer_to_json(layer));
        }
        // 用 json! 宏组装一个操作集的 JSON 对象并 push 进 sets。
        sets.push(json!({
            "id": set.id,
            "name": set.name,
            // 公共层同样调用 layer_to_json 转换（传入借用）。
            "commonLayer": layer_to_json(&set.common_layer),
            "layers": layers,
        }));
    }

    // 【Rust 语法】`json!` 宏的最后一个表达式直接作为函数返回值（没有分号）；字段值可以是变量（gs/sets）或常量（CONFIG_VERSION）。
    json!({
        "version": CONFIG_VERSION,
        "globalSettings": gs,
        "activeOperationSet": profile.active_operation_set_id,
        "operationSets": sets,
    })
}

// 【Rust 语法】返回类型 `-> String`：返回堆分配的、可变长 UTF-8 字符串（区别于 &str）。
pub fn profile_to_json_string(profile: &ControllerProfile) -> String {
    // 【Rust 语法】`to_string_pretty` 输出带缩进的 JSON 文本；`unwrap_or_default()`：若返回 Result::Err（序列化失败）则用默认值（空字符串）兜底，不会 panic。
    serde_json::to_string_pretty(&profile_to_json(profile)).unwrap_or_default()
}

// =====================================================================
// 反序列化：JSON 文本 -> ControllerProfile
// 失败（语法错误/非对象根/版本不匹配）返回 Err(msg)。
// =====================================================================
// 【Rust 语法】`Result<ControllerProfile, String>`：Rust 的错误处理枚举，Ok 携带成功值，Err 携带错误信息（这里是 String）；`text: &str` 接收文本的借用。
pub fn profile_from_json_str(text: &str) -> Result<ControllerProfile, String> {
    // 【Rust 语法】`let root: Value = ...`：显式类型注解，告诉编译器解析目标类型是 Value；`map_err(|e| ...)` 把底层解析错误转换成 String；`|e|` 是闭包（匿名函数）语法，`{}` 内的 format! 拼出错误消息。
    let root: Value =
        serde_json::from_str(text).map_err(|e| format!("Invalid JSON: {}", e))?;
    // 【Rust 语法】`?` 运算符：若前面是 Ok 则取出值并继续，若是 Err 则提前从当前函数返回该 Err；这里转发的是 Result<ControllerProfile,String>，直接作为函数返回值（最后一个表达式无分号）。
    profile_from_json_value(&root)
}

// 【Rust 语法】`&Value`：接收 JSON 值引用的另一个入口函数，同样返回 Result。
pub fn profile_from_json_value(root: &Value) -> Result<ControllerProfile, String> {
    // 【Rust 语法】`if !expr`：取反判断；`is_object()` 判断 Value 是否为 JSON 对象。
    if !root.is_object() {
        // 【Rust 语法】`return Err(...)`：提前返回错误分支；`.to_string()` 把字符串字面量转换成 String 类型（Result 的 Err 需要 String）。
        return Err("Root is not an object".to_string());
    }
    // 【Rust 语法】`Option` 链式调用：`get("version")` 返回 Option<&Value>，`and_then(|v| v.as_i64())` 若存在则尝试转 i64（闭包返回 Option），`unwrap_or(1)` 若为 None 用默认值 1。
    let version = root.get("version").and_then(|v| v.as_i64()).unwrap_or(1);
    // 【Rust 语法】`!=` 判断版本号不匹配；`format!` 宏做字符串插值，`{}` 是占位符。
    if version != CONFIG_VERSION {
        return Err(format!("Unsupported config version: {}", version));
    }

    // 【Rust 语法】结构体字面量初始化：`ControllerProfile { 字段: 值, ... }`；`mut` 使变量可变，便于后续修改字段。
    let mut profile = ControllerProfile {
        operation_sets: Vec::new(),
        active_operation_set_id: String::new(),
        global_settings: GlobalSettings::default(),
    };

    // ---- 全局设置：可缺省，逐字段带默认值读取 ----
    // 【Rust 语法】`if let Some(gs) = ...`：模式匹配简化写法——若 Option 是 Some 则解包出变量 gs 并进入分支，否则整个分支跳过。
    if let Some(gs) = root.get("globalSettings").and_then(|v| v.as_object()) {
        // 用默认值构造一个新的 GlobalSettings。
        let mut s = GlobalSettings::default();
        // 从 gs 对象里读取 "deadzone" 字段，缺省时用 0.15；num 是下方定义的辅助函数。
        s.deadzone = num(gs, "deadzone", 0.15);
        s.look_sensitivity = num(gs, "lookSensitivity", 0.5);
        s.cursor_speed = num(gs, "cursorSpeed", 1.0);
        s.look_smoothing = num(gs, "lookSmoothing", 0.5);
        s.look_acceleration = num(gs, "lookAcceleration", 1.5);
        s.invert_look_x = bool_(gs, "invertLookX", false);
        s.invert_look_y = bool_(gs, "invertLookY", false);
        s.overlay_x = int_(gs, "overlayX", -1);
        s.overlay_y = int_(gs, "overlayY", -1);
        // `as f64` 是把 f32 数值强制转换成 f64（Rust 不允许隐式数值转换）。
        s.overlay_scale = num(gs, "overlayScale", 1.0) as f64;
        s.main_window_x = int_(gs, "mainWindowX", -1);
        s.main_window_y = int_(gs, "mainWindowY", -1);
        s.release_on_foreground_change = bool_(gs, "releaseOnForegroundChange", true);
        s.confirm_on_close = bool_(gs, "confirmOnClose", true);
        s.overlay_opacity = num(gs, "overlayOpacity", 0.8);
        s.wow_exe = gs.get("wowExe").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if let Some(Value::Array(arr)) = gs.get("wowExeHistory") {
            for v in arr {
                if let Some(p) = v.as_str() {
                    if !p.is_empty() && !s.wow_exe_history.iter().any(|x| x == p) {
                        s.wow_exe_history.push(p.to_string());
                    }
                }
            }
        }
        // 把组装好的全局设置赋回 profile。
        profile.global_settings = s;
    }

    // ---- 操作集：新格式 operationSets 数组；旧 v2 格式兼容包装 ----
    // 取出顶层 "operationSets" 字段（可能不存在，返回 Option<&Value>）。
    let sets_val = root.get("operationSets");
    // 【Rust 语法】`match` 表达式：对 sets_val 这个 Option 做模式匹配，分支结果会赋给 parsed_sets；match 本身也是表达式。
    let parsed_sets = match sets_val {
        // 【Rust 语法】匹配 arm + 守卫：`Some(Value::Array(arr))` 匹配"有值且是数组"并解包出 arr；`if !arr.is_empty()` 是守卫条件，不满足则落到下一分支。
        Some(Value::Array(arr)) if !arr.is_empty() => {
            let mut sets = Vec::new();
            // 遍历数组中的每个元素（借用）。
            for v in arr {
                // 跳过不是对象的元素（宽松容错）。
                if !v.is_object() {
                    continue;
                }
                // 【Rust 语法】`as_object()` 返回 Option<&Map>，`unwrap()` 直接取出（前面已确认是对象，不会 panic）。
                let so = v.as_object().unwrap();
                // 【Rust 语法】结构体字面量：`id`/`name` 字段用 `get().and_then().unwrap_or("").to_string()` 读取——取字符串，缺省为空串再转成 String（因为结构体字段是 String 类型）。
                let mut set = OperationSet {
                    id: so.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    name: so.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    common_layer: OperationLayer::new("Common"),
                    layers: Vec::new(),
                };
                // 若 id 为空，则回退到用 name 作为 id。
                if set.id.is_empty() {
                    // 【Rust 语法】if-else 作为表达式：if 分支和 else 分支的最终值都会赋给 set.id。
                    set.id = if set.name.is_empty() {
                        "Set".to_string()
                    } else {
                        // 【Rust 语法】`.clone()`：复制一份 String（避免把 name 的所有权移动走）。
                        set.name.clone()
                    };
                }
                // 本操作集的公共层
                // 【Rust 语法】match 匹配 `Some(Value::Object(_))`：`_` 通配符表示忽略对象内部内容，只判断"是对象"。
                set.common_layer = match so.get("commonLayer") {
                    Some(Value::Object(_)) => {
                        // 是对象就调用 parse_layer 解析为公共层（true 表示公共层）。
                        parse_layer(so.get("commonLayer").unwrap(), true)
                    }
                    // 缺省或不是对象则新建空公共层。
                    _ => OperationLayer::new("Common"),
                };
                // 本操作集的操作层数组
                // 【Rust 语法】`if let Some(Value::Array(larr)) = ...`：匹配"存在且是数组"并解包出 larr。
                if let Some(Value::Array(larr)) = so.get("layers") {
                    // 遍历该操作集的操作层。
                    for lv in larr {
                        // 只解析是对象的元素。
                        if lv.is_object() {
                            // push 解析出的操作层（false 表示非公共层）。
                            set.layers.push(parse_layer(lv, false));
                        }
                    }
                }
                // 把完整的操作集加入 sets 数组。
                sets.push(set);
            }
            // match 分支的返回值：sets（无分号，作为本分支表达式结果）。
            sets
        }
        // 【Rust 语法】`_ =>`：通配分支，匹配所有其他情况（None 或空数组）。
        _ => {
            // ---- 旧 v2 格式兼容：包装为单个「默认操作集」 ----
            // 构造一个默认操作集（id 固定为 "Set1"，name 为中文"默认操作集"）。
            let mut set = OperationSet {
                id: "Set1".to_string(),
                name: "默认操作集".to_string(),
                common_layer: OperationLayer::new("Common"),
                layers: Vec::new(),
            };
            // 从旧格式顶层读取公共层；是对象才解析，否则用空公共层。
            set.common_layer = match root.get("commonLayer") {
                Some(Value::Object(_)) => parse_layer(root.get("commonLayer").unwrap(), true),
                _ => OperationLayer::new("Common"),
            };
            // 从旧格式顶层读取 layers 数组。
            if let Some(Value::Array(arr)) = root.get("layers") {
                // 逐个解析为操作层。
                for v in arr {
                    if v.is_object() {
                        set.layers.push(parse_layer(v, false));
                    }
                }
            }
            // 【Rust 语法】`vec![set]`：用 vec! 宏快速构造只含一个元素的 Vec，作为本分支返回值。
            vec![set]
        }
    };
    // 把解析出的操作集列表赋给 profile。
    profile.operation_sets = parsed_sets;

    // ---- 恢复上次激活的操作集；缺失时回退到第一个 ----
    // 读取 "activeOperationSet" 字段为字符串，缺省为空串。
    let active = root.get("activeOperationSet").and_then(|v| v.as_str()).unwrap_or("");
    // 【Rust 语法】`!a && !b`：逻辑非和逻辑与；先尝试设置激活集，失败且操作集非空时回退。
    if !profile.set_active_operation_set(active)
        && !profile.operation_sets.is_empty()
    {
        // 【Rust 语法】`operation_sets[0]`：按下标访问 Vec 元素；`.id.clone()` 复制第一个集的 id 字符串。
        profile.active_operation_set_id = profile.operation_sets[0].id.clone();
    }

    // ---- 兜底保证：至少保留一个有效操作集 ----
    // 若操作集列表仍为空。
    if profile.operation_sets.is_empty() {
        // 【Rust 语法】`let fallback = ...`：调用关联函数（静态方法）create_empty 创建空操作集；`push(fallback)` 把其所有权移入 Vec。
        let fallback = OperationSet::create_empty("Set1", "默认操作集");
        profile.operation_sets.push(fallback);
        profile.active_operation_set_id = "Set1".to_string();
    }

    // 【Rust 语法】`Ok(profile)`：Result 的成功变体，把 profile 包装为 Ok 返回（无分号，函数返回值）。
    Ok(profile)
}

// =====================================================================
// 层 / 映射 / 动作的序列化与解析
// =====================================================================

// 【Rust 语法】私有函数 `fn`（无 pub，仅本模块可用）；`match action.r#type` 中 `r#` 是原始标识符语法——因为 `type` 是 Rust 关键字，必须用 `r#type` 才能作为字段名。
fn action_to_json(action: &MappedAction) -> Value {
    // 【Rust 语法】match 对枚举 ActionType 做穷尽匹配；`=>` 左侧是模式，右侧是分支表达式。
    match action.r#type {
        // 键盘键：输出 { "type": "keyboard", "keyCode": ... }
        ActionType::KeyboardKey => json!({ "type": K_TYPE_KEYBOARD, "keyCode": action.key_code }),
        // 鼠标点击：输出按钮名（经 mouse_button_name 转字符串）。
        ActionType::MouseClick => json!({ "type": K_TYPE_MOUSE, "button": mouse_button_name(action.mouse_button) }),
        // 鼠标开关（按住切换）：同样输出按钮名。
        ActionType::MouseToggle => json!({ "type": K_TYPE_MOUSE_TOGGLE, "button": mouse_button_name(action.mouse_button) }),
        // 滚轮上：只输出 type。
        ActionType::WheelUp => json!({ "type": K_TYPE_WHEEL_UP }),
        // 滚轮下：只输出 type。
        ActionType::WheelDown => json!({ "type": K_TYPE_WHEEL_DOWN }),
        // 【Rust 语法】`.clone().unwrap_or_default()`：layer_name 是 Option<String>，先 clone 副本再解包，None 时用默认空字符串。
        ActionType::SwitchLayer => json!({ "type": K_TYPE_SWITCH_LAYER, "layerName": action.layer_name.clone().unwrap_or_default() }),
        // 鼠标移动：只输出 type。
        ActionType::MouseMove => json!({ "type": K_TYPE_MOUSE_MOVE }),
        // 视角环绕：只输出 type。
        ActionType::LookAround => json!({ "type": K_TYPE_LOOK_AROUND }),
        // 开关映射：只输出 type。
        ActionType::ToggleMapping => json!({ "type": K_TYPE_TOGGLE_MAPPING }),
        // 开关屏幕键盘：只输出 type。
        ActionType::ToggleOnScreenKeyboard => json!({ "type": K_TYPE_TOGGLE_ON_SCREEN_KEYBOARD }),
        // 开关叠加层：只输出 type。
        ActionType::ToggleOverlay => json!({ "type": K_TYPE_TOGGLE_OVERLAY }),
        // 交互键：只输出 type。
        ActionType::InteractionKey => json!({ "type": K_TYPE_INTERACTION_KEY }),
        // 释放鼠标左右键锁存：只输出 type。
        ActionType::ReleaseMouseToggle => json!({ "type": K_TYPE_RELEASE_MOUSE_TOGGLE }),
        // 交换鼠标左右键锁存：只输出 type。
        ActionType::SwapMouseToggle => json!({ "type": K_TYPE_SWAP_MOUSE_TOGGLE }),
        // 鼠标精细移动：只输出 type。
        ActionType::FineLook => json!({ "type": K_TYPE_FINE_LOOK }),
        ActionType::ToggleOverlayExpand => json!({ "type": K_TYPE_TOGGLE_OVERLAY_EXPAND }),
    }
}

// 【Rust 语法】返回 `Option<MappedAction>`：解析成功返回 Some(动作)，失败（未知类型/字段缺失）返回 None。
fn parse_action(obj: &Value) -> Option<MappedAction> {
    // 读取 "type" 字段字符串，缺省为空串。
    let type_str = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    // 用字符串匹配动作类型常量。
    match type_str {
        // 【Rust 语法】`as i32`：把 i64 强制转换（窄化）为 i32；`unwrap_or(0)` 缺省为 0。
        K_TYPE_KEYBOARD => Some(MappedAction::keyboard_key(obj.get("keyCode").and_then(|v| v.as_i64()).unwrap_or(0) as i32)),
        K_TYPE_MOUSE => {
            // 【Rust 语法】`?` 运算符用在 Option 上：mouse_button_from_name 返回 Option，若为 None 则整个函数提前返回 None。
            let b = mouse_button_from_name(obj.get("button").and_then(|v| v.as_str()).unwrap_or(""))?;
            Some(MappedAction::mouse_click(b))
        }
        K_TYPE_MOUSE_TOGGLE => {
            let b = mouse_button_from_name(obj.get("button").and_then(|v| v.as_str()).unwrap_or(""))?;
            Some(MappedAction::mouse_toggle(b))
        }
        K_TYPE_WHEEL_UP => Some(MappedAction::wheel_up()),
        K_TYPE_WHEEL_DOWN => Some(MappedAction::wheel_down()),
        K_TYPE_SWITCH_LAYER => {
            // 读取层名。
            let name = obj.get("layerName").and_then(|v| v.as_str()).unwrap_or("");
            // 层名为空则解析失败。
            if name.is_empty() {
                return None;
            }
            Some(MappedAction::switch_layer(name))
        }
        K_TYPE_MOUSE_MOVE => Some(MappedAction::mouse_move()),
        K_TYPE_LOOK_AROUND => Some(MappedAction::look_around()),
        K_TYPE_TOGGLE_MAPPING => Some(MappedAction::toggle_mapping()),
        K_TYPE_TOGGLE_ON_SCREEN_KEYBOARD => Some(MappedAction::toggle_on_screen_keyboard()),
        K_TYPE_TOGGLE_OVERLAY => Some(MappedAction::toggle_overlay()),
        K_TYPE_INTERACTION_KEY => Some(MappedAction::interaction_key()),
        K_TYPE_RELEASE_MOUSE_TOGGLE => Some(MappedAction::release_mouse_toggle()),
        K_TYPE_SWAP_MOUSE_TOGGLE => Some(MappedAction::swap_mouse_toggle()),
        K_TYPE_FINE_LOOK => Some(MappedAction::fine_look()),
        K_TYPE_TOGGLE_OVERLAY_EXPAND => Some(MappedAction::toggle_overlay_expand()),
        _ => None, // 未知类型
    }
}

// 【Rust 语法】把 KeyMapping（按键映射）转成 JSON 对象。
fn mapping_to_json(mapping: &KeyMapping) -> Value {
    // json! 宏组装：action 字段递归转换，subCommands 字段直接使用 Vec 值。
    json!({
        "action": action_to_json(&mapping.action),
        "subCommands": mapping.sub_commands,
        // 长按动作（None 时输出 null，保持文件可读）
        "longPress": mapping.long_press.as_ref().map(|a| action_to_json(a)),
        "longPressSubCommands": mapping.long_press_subs,
        "longPressMs": mapping.long_press_ms,
    })
}

// 【Rust 语法】解析 JSON 为 KeyMapping，返回 Option；失败返回 None。
fn parse_mapping(obj: &Value) -> Option<KeyMapping> {
    // 【Rust 语法】`obj.get("action")?`：若取不到 "action" 字段，`?` 让函数直接返回 None。
    let action_val = obj.get("action")?;
    // 校验 action 必须是对象。
    if !action_val.is_object() {
        return None;
    }
    // 解析 action；失败则 `?` 提前返回 None。
    let action = parse_action(action_val)?;

    // 【Rust 语法】`Vec<i32>`：显式指定元素类型为 i32 的空数组。
    let mut subs: Vec<i32> = Vec::new();
    // 若存在 "subCommands" 且是数组。
    if let Some(Value::Array(arr)) = obj.get("subCommands") {
        // 遍历每个子命令元素。
        for v in arr {
            // 若是整数则收集（非整数跳过）。
            if let Some(n) = v.as_i64() {
                subs.push(n as i32);
            }
        }
    }
    // 校验子命令数量是否超过上限。
    if subs.len() > KeyMapping::MAX_SUB_COMMANDS {
        return None; // 子命令超限，跳过
    }
    // 长按动作：可选；存在且是对象才解析（非法长按动作忽略，不影响主动作）
    let long_press = match obj.get("longPress") {
        Some(v) if v.is_object() => parse_action(v),
        _ => None,
    };
    let mut long_subs: Vec<i32> = Vec::new();
    if let Some(Value::Array(arr)) = obj.get("longPressSubCommands") {
        for v in arr {
            if let Some(n) = v.as_i64() {
                long_subs.push(n as i32);
            }
        }
    }
    if long_subs.len() > KeyMapping::MAX_SUB_COMMANDS {
        long_subs.truncate(KeyMapping::MAX_SUB_COMMANDS);
    }
    let long_press_ms = obj
        .get("longPressMs")
        .and_then(|v| v.as_i64())
        .map(|v| v.clamp(100, 5000) as u32)
        .unwrap_or(KeyMapping::DEFAULT_LONG_PRESS_MS);
    // 【Rust 语法】结构体字面量：`action` 是字段名简写——等价于 `action: action`，即变量名与字段名相同可省略冒号部分。
    Some(KeyMapping {
        action,
        sub_commands: subs,
        long_press,
        long_press_subs: long_subs,
        long_press_ms,
    })
}

// 【Rust 语法】把 OperationLayer（操作层）转成 JSON 对象。
fn layer_to_json(layer: &OperationLayer) -> Value {
    // 【Rust 语法】`serde_json::Map::new()`：serde_json 提供的字符串→Value 有序映射容器（类似 HashMap）。
    let mut mappings = serde_json::Map::new();
    // 【Rust 语法】for 循环解构：`(button, mapping)` 同时取出 HashMap 的键和值；`&layer.button_mappings` 借用迭代。
    for (button, mapping) in &layer.button_mappings {
        // insert 往映射中插入键值对；`*button` 解引用取出按钮枚举值。
        mappings.insert(
            // 键是按钮名的字符串；`to_string()` 把 &str 转成 String。
            controller_button_name(*button).to_string(),
            mapping_to_json(mapping),
        );
    }
    // 新建一个 JSON 对象映射，用于存放层的各字段。
    let mut obj = serde_json::Map::new();
    // id 为空时回退写 name，保证老数据也有可读 id
    // 【Rust 语法】`Value::String(...)`：显式构造 JSON 字符串值；内部 `if layer.id.is_empty() {...} else {...}` 是 if-else 表达式。
    obj.insert(
        "id".to_string(),
        Value::String(if layer.id.is_empty() {
            layer.name.clone()
        } else {
            layer.id.clone()
        }),
    );
    // 写入层名。
    obj.insert("name".to_string(), Value::String(layer.name.clone()));
    // 若有触发按键字段则写入。
    if layer.has_trigger_button {
        // 写入触发按钮名。
        obj.insert(
            "triggerButton".to_string(),
            Value::String(controller_button_name(layer.trigger_button).to_string()),
        );
    }
    // 写入按钮映射表（mappings 转成 Value::Object）。
    obj.insert("buttonMappings".to_string(), Value::Object(mappings));
    // 【Rust 语法】`Value::Object(obj)`：把 Map 包装成 Value 对象，作为函数返回值（无分号）。
    Value::Object(obj)
}

/// 解析一个层。is_common 指示是否公共层（公共层不解析 triggerButton）。
// 【Rust 语法】函数参数 `v: &Value`（JSON 引用）与 `is_common: bool`（是否公共层），返回 OperationLayer（拥有所有权）。
fn parse_layer(v: &Value, is_common: bool) -> OperationLayer {
    // 【Rust 语法】match 解包 Option<&Map>：Some(o) 取出对象引用，None 时提前返回新建的公共层。
    let obj = match v.as_object() {
        Some(o) => o,
        None => return OperationLayer::new("Common"),
    };
    // 先按公共层默认名创建可变层对象。
    let mut layer = OperationLayer::new("Common");
    // 读取 "name" 字段字符串赋给层名。
    layer.name = obj.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
    // id：优先读取持久化的 id；缺失时回退到 name
    // 读取 "id" 字段字符串赋给层 id。
    layer.id = obj.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
    // 若 id 为空则回退生成。
    if layer.id.is_empty() {
        // 公共层用 "Common"，操作层用层名，通过 if-else 表达式选择。
        layer.id = if is_common {
            "Common".to_string()
        } else {
            layer.name.clone()
        };
    }

    // 触发按键：仅操作层有
    // 仅非公共层处理触发按键。
    if !is_common {
        // 若存在 "triggerButton" 字符串。
        if let Some(tb) = obj.get("triggerButton").and_then(|x| x.as_str()) {
            // 若按钮名能映射到合法按钮枚举。
            if let Some(btn) = controller_button_from_name(tb) {
                // 标记有触发按键。
                layer.has_trigger_button = true;
                // 记录触发按键。
                layer.trigger_button = btn;
            }
        }
    }

    // 按钮映射表：逐条解析，非法条目直接跳过
    // 若存在 "buttonMappings" 且是对象。
    if let Some(Value::Object(mappings)) = obj.get("buttonMappings") {
        // 遍历映射的每个 按钮名→映射值 对。
        for (name, mval) in mappings {
            // 按钮名合法才继续。
            if let Some(btn) = controller_button_from_name(name) {
                // 映射值必须是对象。
                if mval.is_object() {
                    // 解析映射成功才插入。
                    if let Some(mapping) = parse_mapping(mval) {
                        // 写入按钮→映射 的关系。
                        layer.button_mappings.insert(btn, mapping);
                    }
                }
            }
        }
    }
    // 返回组装好的层（无分号，尾表达式）。
    layer
}

// ---- 数值读取辅助 ----
// 【Rust 语法】`m: &serde_json::Map<String, Value>`：接收 JSON 对象映射的引用；`key: &str` 是查找键；`def: f32` 是缺省值；返回 f32。
fn num(m: &serde_json::Map<String, Value>, key: &str, def: f32) -> f32 {
    // 【Rust 语法】Option 链式处理：`get(key)` 取字段 → `and_then(|v| v.as_f64())` 转浮点 → `map(|x| x as f32)` 强转 f32 → `unwrap_or(def)` 缺省兜底；无分号是返回值。
    m.get(key)
        .and_then(|v| v.as_f64())
        .map(|x| x as f32)
        .unwrap_or(def)
}
// 【Rust 语法】`int_`：读取整数；注意函数名带下划线尾缀，避免与关键字 `int` 冲突（Rust 中没有 int 关键字，这里是命名习惯避免歧义）。
fn int_(m: &serde_json::Map<String, Value>, key: &str, def: i32) -> i32 {
    // 取字段 → 转 i64 → 强转 i32 → 缺省兜底。
    m.get(key)
        .and_then(|v| v.as_i64())
        .map(|x| x as i32)
        .unwrap_or(def)
}
// 【Rust 语法】`bool_`：读取布尔值；`as_bool()` 返回 Option<bool>。
fn bool_(m: &serde_json::Map<String, Value>, key: &str, def: bool) -> bool {
    // 取字段 → 转 bool → 缺省兜底。
    m.get(key).and_then(|v| v.as_bool()).unwrap_or(def)
}
