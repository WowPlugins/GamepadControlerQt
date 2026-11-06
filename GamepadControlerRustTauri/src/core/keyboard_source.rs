// =====================================================================
// keyboard_source.rs —— 物理键盘组合键触发源
//
// 用 WH_KEYBOARD_LL 低级键盘钩子监听全局按键：
//   - 命中规则（修饰键集合严格相等 + 主键一致 + 状态条件满足）时吞掉该键，
//     并把「按下/松开」事件投递给 worker 线程执行映射动作；
//   - 未命中规则的按键原样放行给游戏。
//
// 线程模型：
//   - 钩子线程：SetWindowsHookExW + GetMessageW 消息循环，回调里只做
//     轻量匹配（绝不锁 AppCore，低级钩子超时会被系统摘除）；
//   - worker 线程：收到事件后再取 AppCore 锁执行注入（状态宏内部有
//     50ms 等待，不能放在钩子回调里）。
// =====================================================================

use crate::core::app::AppCore;
use crate::core::injector::android_key_code_to_windows_vk;
use crate::core::mapping_types::{ControllerProfile, KeyMapping};
use crate::core::state_reader::StateBits;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock, RwLockReadGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    KBDLLHOOKSTRUCT, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HC_ACTION, LLKHF_INJECTED, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_KEYUP,
    WM_QUIT, WM_SYSKEYUP,
};

// ---- 修饰键位掩码（与 CompiledRule.mods 对应） ----
const MOD_CTRL: u8 = 1 << 0; // 按住 Ctrl
const MOD_SHIFT: u8 = 1 << 1; // 按住 Shift
const MOD_ALT: u8 = 1 << 2; // 按住 Alt
const MOD_WIN: u8 = 1 << 3; // 按住 Win

/// 编译后的规则（钩子回调里直接匹配，避免每次都转换 Android 键码）
struct CompiledRule {
    id: u32,                // 规则 id（投递给 worker / 作为 TriggerId::Kb）
    mods: u8,               // 要求的修饰键位掩码
    main_vk: u16,           // 主键 Windows 虚拟键码
    mapping: KeyMapping,    // 动作与状态条件
}

/// 钩子 -> worker 的事件
enum KbEvent {
    Press(u32),   // 规则按下
    Release(u32), // 规则松开
    Quit,         // 停止 worker
}

/// 钩子回调共享上下文（静态常驻，重启监听时只替换规则快照与通道）
struct HookCtx {
    rules: RwLock<Arc<Vec<CompiledRule>>>, // 当前规则快照（sync 时整体替换）
    states: Arc<StateBits>,                // 角色状态位（状态条件判定）
    tx: Mutex<Sender<KbEvent>>,            // 到 worker 线程的事件通道
    swallowed: Mutex<HashMap<u32, u32>>,   // 已吞下的主键 VK -> 规则 id（keyup 需要同样吞掉）
}

impl HookCtx {
    /// 取规则快照（锁中毒时也能继续用，钩子回调里不允许 panic）
    fn rules(&self) -> RwLockReadGuard<'_, Arc<Vec<CompiledRule>>> {
        self.rules.read().unwrap_or_else(|e| e.into_inner())
    }

    /// 取吞吐记录锁
    fn swallowed(&self) -> MutexGuard<'_, HashMap<u32, u32>> {
        self.swallowed.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 取通道锁
    fn tx(&self) -> MutexGuard<'_, Sender<KbEvent>> {
        self.tx.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 全局钩子上下文（只初始化一次）
static HOOK_CTX: OnceLock<HookCtx> = OnceLock::new();

/// 读取当前按住的修饰键集合
fn current_mods() -> u8 {
    // 判断某个虚拟键当前是否被按住（最高位 0x8000 表示按下）
    let held = |vk: u16| unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 };
    let mut mods = 0u8;
    if held(VK_CONTROL.0) {
        mods |= MOD_CTRL;
    }
    if held(VK_SHIFT.0) {
        mods |= MOD_SHIFT;
    }
    if held(VK_MENU.0) {
        mods |= MOD_ALT;
    }
    if held(VK_LWIN.0) || held(VK_RWIN.0) {
        mods |= MOD_WIN;
    }
    mods
}

/// 前台窗口是否属于本进程（在 keymacro 设置页里按键时不触发映射）
fn foreground_is_self() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

/// 低级键盘钩子回调（在钩子线程执行，必须尽快返回）
unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // 非 HC_ACTION 交给下一个钩子
    if code != HC_ACTION as i32 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let ctx = match HOOK_CTX.get() {
        Some(c) => c,
        None => return CallNextHookEx(None, code, wparam, lparam),
    };
    // 取出按键信息（lparam 指向 KBDLLHOOKSTRUCT）
    let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    // 本程序 SendInput 注入的键：直接放行，避免自己触发自己形成循环
    if info.flags.contains(LLKHF_INJECTED) || foreground_is_self() {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let vk = info.vkCode; // u32 虚拟键码
    let is_up = wparam.0 == WM_KEYUP as usize || wparam.0 == WM_SYSKEYUP as usize;

    if is_up {
        // 松开：只有按下时被吞掉的主键才需要吞掉抬起（否则游戏收到无配对的 keyup）
        if let Some(rule_id) = ctx.swallowed().remove(&vk) {
            let _ = ctx.tx().send(KbEvent::Release(rule_id));
            return LRESULT(1);
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // 按下：连发（按住不放会重复 keydown）已被吞过则继续吞，且不重复投递
    if ctx.swallowed().contains_key(&vk) {
        return LRESULT(1);
    }

    // 规则匹配：修饰键集合严格相等 + 主键一致 + 状态条件满足
    let mods = current_mods();
    let rules = ctx.rules().clone();
    for rule in rules.iter() {
        if rule.main_vk as u32 != vk || rule.mods != mods {
            continue;
        }
        if !rule.mapping.state_allows(|id| ctx.states.get(id)) {
            continue; // 状态条件不满足：当作没命中，放行给游戏
        }
        ctx.swallowed().insert(vk, rule.id);
        let _ = ctx.tx().send(KbEvent::Press(rule.id));
        return LRESULT(1);
    }

    CallNextHookEx(None, code, wparam, lparam)
}

/// 把配置里的键盘规则编译成钩子可直接匹配的快照
pub fn compile_rules(profile: &ControllerProfile) -> Arc<Vec<CompiledRule>> {
    let mut out = Vec::new();
    // 总开关关闭时返回空快照（等同于全部放行）
    if profile.global_settings.keyboard_rules_enabled {
        for rule in profile.keyboard_rules.iter() {
            if !rule.enabled {
                continue; // 单条规则未启用
            }
            let main_vk = android_key_code_to_windows_vk(rule.key_code);
            // 主键未配置、或主键本身是修饰键（不符合"修饰键 + 主键"语义）时跳过
            if main_vk == 0 || is_modifier_vk(main_vk) {
                continue;
            }
            let mods = (rule.ctrl as u8) * MOD_CTRL
                | (rule.shift as u8) * MOD_SHIFT
                | (rule.alt as u8) * MOD_ALT
                | (rule.win as u8) * MOD_WIN;
            out.push(CompiledRule {
                id: rule.id,
                mods,
                main_vk,
                mapping: rule.mapping.clone(),
            });
        }
    }
    Arc::new(out)
}

/// 是否为修饰键的虚拟键码（主键不能是修饰键）
fn is_modifier_vk(vk: u16) -> bool {
    matches!(
        vk,
        0xA0..=0xA5 | 0x10 | 0x11 | 0x12 | 0x5B | 0x5C // L/R Shift/Ctrl/Alt、通用修饰键、Win
    )
}

/// 钩子线程主循环：安装钩子 -> 消息循环 -> 卸载钩子
fn run_hook_loop(running: Arc<AtomicBool>, thread_id: Arc<AtomicU32>) {
    unsafe {
        // 提前创建线程消息队列：保证 stop() 的 PostThreadMessageW 一定能送达，
        // 且 thread_id 只在队列已存在后才对外发布，避免停止时错过唤醒导致 join 卡死。
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);

        let hmod = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(_) => return, // 拿不到模块句柄，无法安装钩子
        };
        let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(HINSTANCE(hmod.0)), 0) {
            Ok(h) => h,
            Err(_) => return, // 安装失败（权限等原因）时直接退出
        };
        // stop() 需要线程 id 才能投递 WM_QUIT 唤醒阻塞中的 GetMessageW
        thread_id.store(GetCurrentThreadId(), Ordering::SeqCst);

        while running.load(Ordering::SeqCst) {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                break; // 0 = WM_QUIT，-1 = 错误
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(hook);
    }
}

/// worker 线程主循环：把按键事件转成 AppCore 调用
fn run_worker(rx: Receiver<KbEvent>, core: Arc<Mutex<AppCore>>, running: Arc<AtomicBool>) {
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(KbEvent::Quit) => break,
            Ok(event) => {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                // 取 AppCore 锁：最多重试 ~100ms，拿不到就丢弃该事件，
                // 保证 stop() 不会因为主线程持有锁而卡死
                let mut guard = None;
                for _ in 0..100 {
                    if let Ok(g) = core.try_lock() {
                        guard = Some(g);
                        break;
                    }
                    if !running.load(Ordering::SeqCst) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                if let Some(mut core) = guard {
                    match event {
                        KbEvent::Press(id) => core.handle_keyboard_event(id, true),
                        KbEvent::Release(id) => core.handle_keyboard_event(id, false),
                        KbEvent::Quit => {}
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// 物理键盘监听器（由 AppShared 持有，跟随映射启停）
pub struct KeyboardListener {
    running: Arc<AtomicBool>,           // 监听是否在运行
    thread_id: Arc<AtomicU32>,          // 钩子线程 id（0 = 尚未就绪）
    hook_thread: Option<JoinHandle<()>>, // 钩子线程句柄
    worker_thread: Option<JoinHandle<()>>, // worker 线程句柄
    ctx: &'static HookCtx,              // 全局钩子上下文
}

impl KeyboardListener {
    pub fn new(states: Arc<StateBits>) -> Self {
        // 全局上下文只初始化一次；未启动时通道接收端已丢弃，发送会静默失败
        let ctx = HOOK_CTX.get_or_init(|| {
            let (tx, _rx) = mpsc::channel();
            HookCtx {
                rules: RwLock::new(Arc::new(Vec::new())),
                states,
                tx: Mutex::new(tx),
                swallowed: Mutex::new(HashMap::new()),
            }
        });
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread_id: Arc::new(AtomicU32::new(0)),
            hook_thread: None,
            worker_thread: None,
            ctx,
        }
    }

    /// 刷新规则快照（配置变化时调用；总开关关闭时快照为空）
    pub fn sync(&self, profile: &ControllerProfile) {
        let compiled = compile_rules(profile);
        *self
            .ctx
            .rules
            .write()
            .unwrap_or_else(|e| e.into_inner()) = compiled;
    }

    /// 启动钩子线程与 worker 线程（重复调用无副作用）
    pub fn start(&mut self, core: Arc<Mutex<AppCore>>) {
        if self.hook_thread.is_some() {
            return;
        }
        // 每次启动重建事件通道，避免上一轮残留事件
        let (tx, rx) = mpsc::channel();
        *self.ctx.tx() = tx;
        self.ctx.swallowed().clear();
        self.running.store(true, Ordering::SeqCst);
        self.thread_id.store(0, Ordering::SeqCst);

        let worker_running = Arc::clone(&self.running);
        self.worker_thread = Some(thread::spawn(move || {
            run_worker(rx, core, worker_running);
        }));

        let hook_running = Arc::clone(&self.running);
        let hook_tid = Arc::clone(&self.thread_id);
        self.hook_thread = Some(thread::spawn(move || {
            run_hook_loop(hook_running, hook_tid);
        }));
    }

    /// 停止监听：先退出钩子线程（唤醒消息循环），再结束 worker
    pub fn stop(&mut self) {
        if self.hook_thread.is_none() {
            return;
        }
        self.running.store(false, Ordering::SeqCst);
        // 唤醒阻塞在 GetMessageW 的钩子线程
        let tid = self.thread_id.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Some(h) = self.hook_thread.take() {
            let _ = h.join();
        }
        // 通知 worker 退出并回收
        let _ = self.ctx.tx().send(KbEvent::Quit);
        if let Some(h) = self.worker_thread.take() {
            let _ = h.join();
        }
        self.ctx.swallowed().clear();
    }

    /// 当前是否在监听（UI 展示用）
    pub fn is_running(&self) -> bool {
        self.hook_thread.is_some()
    }
}

impl Drop for KeyboardListener {
    fn drop(&mut self) {
        self.stop();
    }
}
