// =====================================================================
// state_reader.rs —— 角色状态读取（像素点方案）
//
// WoW 插件 SLState 会在屏幕左上角绘制一排色块：
//   [锚点蓝][校验青][S0][S1]...[S9]   每块 6x6，间隔 2
// 本模块用 GetDC + BitBlt 抓取这一小条区域，解码出 10 个状态位。
//
// 线程模型：
//   PixelReader 独立线程按配置周期（默认 50ms）采集一次，结果写入
//   Arc<StateBits>（原子 u16），映射线程读取时无锁。
// =====================================================================

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetPixel,
    ReleaseDC, SelectObject, HGDIOBJ, SRCCOPY,
};

/// 可暴露状态 id（顺序必须与插件 STATE_DEFS 一致）
pub const STATE_IDS: [&str; 10] = [
    "stealthed",  // 0 潜行中
    "chatOpen",   // 1 聊天窗打开
    "inCombat",   // 2 战斗中
    "mounted",    // 3 骑乘中
    "hasTarget",  // 4 有目标
    "hasPet",     // 5 有宠物
    "isDead",     // 6 死亡
    "isGhost",    // 7 灵魂状态
    "isFalling",  // 8 下落中
    "isSwimming", // 9 游泳中
];

/// 状态中文名（UI 展示用）
pub fn state_display_name(id: &str) -> &'static str {
    match id {
        "stealthed" => "潜行中",
        "chatOpen" => "聊天窗打开",
        "inCombat" => "战斗中",
        "mounted" => "骑乘中",
        "hasTarget" => "有目标",
        "hasPet" => "有宠物",
        "isDead" => "死亡",
        "isGhost" => "灵魂状态",
        "isFalling" => "下落中",
        "isSwimming" => "游泳中",
        _ => "未知状态",
    }
}

// 色块尺寸（物理像素）：6x6，间隔 2
const BLOCK: i32 = 6;
const STEP: i32 = 8;
const BLOCK_COUNT: i32 = 12; // 锚点 + 校验 + 10 个状态
const CLR_INVALID_VALUE: u32 = 0xFFFF_FFFF; // GetPixel 失败返回值

/// 10 个状态位（bit0..bit9，1 = 真）
#[derive(Default)]
pub struct StateBits {
    bits: AtomicU16,
}

impl StateBits {
    pub fn new() -> Self {
        Self {
            bits: AtomicU16::new(0),
        }
    }

    pub fn store(&self, v: u16) {
        self.bits.store(v, Ordering::SeqCst);
    }

    pub fn load(&self) -> u16 {
        self.bits.load(Ordering::SeqCst)
    }

    /// 按状态 id 查询（未知 id 返回 None）
    pub fn get(&self, id: &str) -> Option<bool> {
        let idx = STATE_IDS.iter().position(|s| *s == id)?;
        Some(self.load() & (1u16 << idx) != 0)
    }

    /// 10 个状态的布尔快照
    pub fn all(&self) -> [bool; 10] {
        let v = self.load();
        let mut out = [false; 10];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = v & (1u16 << i) != 0;
        }
        out
    }
}

/// 读取配置（原子量，便于运行期更新而无需重启线程）
pub struct StateReadConfig {
    pub enabled: AtomicBool,
    pub interval_ms: AtomicU32,
    pub x: AtomicI32,
    pub y: AtomicI32,
}

impl Default for StateReadConfig {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            interval_ms: AtomicU32::new(50),
            x: AtomicI32::new(10),
            y: AtomicI32::new(10),
        }
    }
}

fn channel_r(c: COLORREF) -> i32 {
    (c.0 & 0xFF) as i32
}
fn channel_g(c: COLORREF) -> i32 {
    ((c.0 >> 8) & 0xFF) as i32
}
fn channel_b(c: COLORREF) -> i32 {
    ((c.0 >> 16) & 0xFF) as i32
}

/// 抓取一条色块并解码为状态位；锚点/校验不匹配时返回 None（说明当前没有插件在绘制）
fn read_states(base_x: i32, base_y: i32) -> Option<u16> {
    let log_on = crate::core::debug_log::enabled(); // 日志开关（关闭时完全不做格式化）
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            crate::dbg_log!("像素读取 基准=({},{}) → GetDC 失败，跳过本次采集", base_x, base_y);
            return None;
        }
        let width = STEP * (BLOCK_COUNT - 1) + BLOCK;
        let height = BLOCK;
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, width, height);
        let old = SelectObject(mem, HGDIOBJ(bmp.0));

        let mut result = None;
        if BitBlt(mem, 0, 0, width, height, Some(screen), base_x, base_y, SRCCOPY).is_ok() {
            let anchor = GetPixel(mem, BLOCK / 2, BLOCK / 2);
            let verify = GetPixel(mem, STEP + BLOCK / 2, BLOCK / 2);
            let is_blue = anchor.0 != CLR_INVALID_VALUE
                && channel_b(anchor) > 180
                && channel_r(anchor) < 90
                && channel_g(anchor) < 90;
            let is_cyan = verify.0 != CLR_INVALID_VALUE
                && channel_b(verify) > 140
                && channel_g(verify) > 140
                && channel_r(verify) < 110;
            if is_blue && is_cyan {
                let mut bits = 0u16;
                let mut detail = String::new(); // 每个色块的判定明细（仅日志开启时填充）
                for i in 0..10i32 {
                    let x = STEP * (2 + i) + BLOCK / 2;
                    let c = GetPixel(mem, x, BLOCK / 2);
                    // 绿 = 真；红 = 假（其余颜色按假处理）
                    let truthy = c.0 != CLR_INVALID_VALUE && channel_g(c) > 128 && channel_r(c) < 128;
                    if truthy {
                        bits |= 1u16 << i;
                    }
                    if log_on {
                        detail.push_str(&format!(
                            "{}={}(RGB {},{},{}), ",
                            state_display_name(STATE_IDS[i as usize]),
                            if truthy { "真" } else { "假" },
                            channel_r(c),
                            channel_g(c),
                            channel_b(c)
                        ));
                    }
                }
                if log_on {
                    let names: Vec<&str> = (0..10)
                        .filter(|i| bits & (1u16 << i) != 0)
                        .map(|i| state_display_name(STATE_IDS[i as usize]))
                        .collect();
                    crate::dbg_log!(
                        "像素读取 基准=({},{}) 锚点=RGB({},{},{}) 校验=RGB({},{},{}) 位=0b{:010b} 状态=[{}] 明细=[{}]",
                        base_x,
                        base_y,
                        channel_r(anchor),
                        channel_g(anchor),
                        channel_b(anchor),
                        channel_r(verify),
                        channel_g(verify),
                        channel_b(verify),
                        bits,
                        if names.is_empty() { "无".to_string() } else { names.join("/") },
                        detail.trim_end_matches(", ")
                    );
                }
                result = Some(bits);
            } else if log_on {
                crate::dbg_log!(
                    "像素读取 基准=({},{}) 锚点=RGB({},{},{}) 校验=RGB({},{},{}) → 未检测到插件色块（锚点通过={} 校验通过={}）",
                    base_x,
                    base_y,
                    channel_r(anchor),
                    channel_g(anchor),
                    channel_b(anchor),
                    channel_r(verify),
                    channel_g(verify),
                    channel_b(verify),
                    is_blue,
                    is_cyan
                );
            }
        } else {
            crate::dbg_log!("像素读取 基准=({},{}) → BitBlt 失败", base_x, base_y);
        }

        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        ReleaseDC(Some(HWND(std::ptr::null_mut())), screen);
        result
    }
}

/// 像素读取线程
pub struct PixelReader {
    running: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl PixelReader {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            handle: None,
        }
    }

    /// 启动读取线程（重复调用无副作用）
    pub fn start(&mut self, bits: Arc<StateBits>, cfg: Arc<StateReadConfig>) {
        if self.handle.is_some() {
            return;
        }
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        self.handle = Some(thread::spawn(move || {
            while running.load(Ordering::SeqCst) {
                if cfg.enabled.load(Ordering::SeqCst) {
                    let x = cfg.x.load(Ordering::SeqCst);
                    let y = cfg.y.load(Ordering::SeqCst);
                    match read_states(x, y) {
                        Some(v) => bits.store(v),
                        None => bits.store(0), // 插件未绘制（例如切出游戏）时清零
                    }
                    let ms = cfg.interval_ms.load(Ordering::SeqCst).max(10) as u64;
                    thread::sleep(Duration::from_millis(ms));
                } else {
                    bits.store(0);
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }));
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for PixelReader {
    fn drop(&mut self) {
        self.stop();
    }
}
