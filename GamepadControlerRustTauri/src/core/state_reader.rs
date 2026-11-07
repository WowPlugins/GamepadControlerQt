// =====================================================================
// state_reader.rs —— 角色状态读取（像素点方案）
//
// WoW 插件 SLState 会在游戏窗口左上角绘制一排色块：
//   [锚点蓝][校验青][S0][S1]...[S9]   每块 6x6，间隔 2（单位为游戏物理像素）
// 本模块用 GetDC + BitBlt 抓取游戏窗口左上角的一小条区域，解码出 10 个状态位。
//
// 关键点：
//   - 色块是相对「游戏窗口客户区左上角」摆放的（插件用 UIParent TOPLEFT 定位），
//     不是屏幕左上角，因此先按 wow_exe 找到游戏窗口，再取其客户区屏幕坐标。
//   - 游戏若被系统 DPI 缩放（高分屏 + 兼容性里的 DPI 设置），6x6/间隔 2 会被
//     整体放大（如 125% → 7.5 像素）。这里不写死尺寸，而是在捕获图里自动
//     测量锚点色块的实际大小与步长，再按测量结果采样各状态块。
//
// 线程模型：
//   PixelReader 独立线程按配置周期（默认 50ms）采集一次，结果写入
//   Arc<StateBits>（原子 u16），映射线程读取时无锁。
// =====================================================================

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, ClientToScreen, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::IsIconic;

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

// 插件侧的色块几何（游戏物理像素）：6x6，步长 8
const BLOCK: i32 = 6;
const STEP: i32 = 8;
const BLOCK_COUNT: i32 = 12; // 锚点 + 校验 + 10 个状态

// 每次抓取的区域大小：起点为锚点坐标前 12 像素，512x96 足以覆盖
// 2 倍 DPI 缩放后的整排色块，并容忍锚点坐标的偏差
const CAP_W: i32 = 512;
const CAP_H: i32 = 96;
const PAD: i32 = 12;

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
    /// 目标游戏 exe 路径（空 = 以屏幕左上角为基准）
    pub exe: Mutex<String>,
}

impl Default for StateReadConfig {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            interval_ms: AtomicU32::new(50),
            x: AtomicI32::new(10),
            y: AtomicI32::new(10),
            exe: Mutex::new(String::new()),
        }
    }
}

// ---------------------------------------------------------------------
// 像素访问与颜色判定
// ---------------------------------------------------------------------

/// 从 32bpp 顶向下 DIB 中取一个像素（内存序为 BGRA）
#[inline]
fn pixel(buf: &[u8], x: i32, y: i32) -> (u8, u8, u8) {
    let i = ((y * CAP_W + x) * 4) as usize;
    (buf[i + 2], buf[i + 1], buf[i]) // R, G, B
}

/// 纯蓝锚点（RGB 0,0,255）：蓝通道明显高于红/绿
#[inline]
fn is_blue(c: (u8, u8, u8)) -> bool {
    let (r, g, b) = (c.0 as i32, c.1 as i32, c.2 as i32);
    b >= 110 && b - r >= 70 && b - g >= 70
}

/// 青色校验块（RGB 0,255,255）：绿、蓝都明显高于红
#[inline]
fn is_cyan(c: (u8, u8, u8)) -> bool {
    let (r, g, b) = (c.0 as i32, c.1 as i32, c.2 as i32);
    g >= 110 && b >= 110 && g - r >= 60 && b - r >= 60
}

/// 在捕获图中寻找锚点色块并测量实际几何
/// 返回 (锚点左上 x, 左上 y, 块宽, 块高, 步长)
fn find_anchor(buf: &[u8]) -> Option<(i32, i32, i32, i32, i32)> {
    for y in 0..CAP_H {
        for x in 0..CAP_W {
            if !is_blue(pixel(buf, x, y)) {
                continue; // 不是蓝色，继续扫描
            }
            // 水平延展得到块宽
            let mut bw = 0;
            while x + bw < CAP_W && is_blue(pixel(buf, x + bw, y)) {
                bw += 1;
            }
            // 垂直延展得到块高
            let mut bh = 0;
            while y + bh < CAP_H && is_blue(pixel(buf, x, y + bh)) {
                bh += 1;
            }
            // 必须是近似正方形的小色块（缩放后 6~12 像素）
            if bw < 3 || bh < 3 || bw > 24 || bh > 24 || (bw - bh).abs() > 2 {
                continue;
            }
            // 块中心行向右找青色校验块，两点间距即步长
            let cy = y + bh / 2;
            let mut cx = x + bw;
            while cx < CAP_W && !is_cyan(pixel(buf, cx, cy)) {
                cx += 1;
            }
            if cx >= CAP_W {
                continue;
            }
            let step = cx - x;
            // 步长应约为块宽的 4/3（6:8），并留出缩放取整的余量
            if step < bw + 1 || step > bw * 2 + 6 {
                continue;
            }
            return Some((x, y, bw, bh, step));
        }
    }
    None
}

/// 首次成功解码时打印一次完整的采样详情，之后只在状态变化时输出（避免按采集周期刷屏）
static FIRST_OK: AtomicBool = AtomicBool::new(true);

/// 失败类日志限流：最多每秒输出一次，避免按 20Hz 采集时刷屏
static LAST_FAIL_MS: AtomicU64 = AtomicU64::new(0);

fn fail_log_allowed() -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_FAIL_MS.load(Ordering::Relaxed);
    if now.saturating_sub(last) >= 3000 {
        LAST_FAIL_MS.store(now, Ordering::Relaxed);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------
// 抓取与解码
// ---------------------------------------------------------------------

/// 抓取一条色块并解码为状态位；未找到锚点/校验时返回 None（说明当前没有插件在绘制）
fn read_states(origin_x: i32, origin_y: i32, px: i32, py: i32) -> Option<u16> {
    let log_on = crate::core::debug_log::enabled(); // 日志开关（关闭时完全不做格式化）
    let cap_x = origin_x + (px - PAD).max(0);
    let cap_y = origin_y + (py - PAD).max(0);
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            crate::dbg_log!("像素读取 原点=({},{}) → GetDC 失败，跳过本次采集", origin_x, origin_y);
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));

        // 32bpp 顶向下 DIB：像素数据可直接按内存读取，避免逐点 GetPixel 的开销
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = CAP_W;
        bmi.bmiHeader.biHeight = -CAP_H; // 负高度 = 从上到下存储
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB.0;

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut result = None;
        match CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) if !bits.is_null() => {
                let old = SelectObject(mem, HGDIOBJ(bmp.0));
                if BitBlt(mem, 0, 0, CAP_W, CAP_H, Some(screen), cap_x, cap_y, SRCCOPY).is_ok() {
                    // 【Rust 语法】from_raw_parts：按 DIB 内存布局构造只读切片（宽*高*4 字节）
                    let buf = std::slice::from_raw_parts(bits as *const u8, (CAP_W * CAP_H * 4) as usize);
                    result = decode(buf, cap_x, cap_y, origin_x, origin_y, px, py, log_on);
                } else {
                    crate::dbg_log!(
                        "像素读取 区域=({},{}) {}x{} → BitBlt 失败",
                        cap_x,
                        cap_y,
                        CAP_W,
                        CAP_H
                    );
                }
                SelectObject(mem, old);
                let _ = DeleteObject(HGDIOBJ(bmp.0));
            }
            _ => {
                crate::dbg_log!("像素读取 → 创建 DIB 位图失败");
            }
        }

        let _ = DeleteDC(mem);
        ReleaseDC(Some(HWND(std::ptr::null_mut())), screen);
        result
    }
}

/// 从捕获的位图数据中定位色块并解码 10 个状态位
fn decode(
    buf: &[u8],
    cap_x: i32,
    cap_y: i32,
    origin_x: i32,
    origin_y: i32,
    px: i32,
    py: i32,
    log_on: bool,
) -> Option<u16> {
    let Some((ax, ay, bw, bh, step)) = find_anchor(buf) else {
        if log_on && fail_log_allowed() {
            // 失败时按 1 倍缩放采样一遍期望位置，方便判断是坐标偏差还是缩放/全屏独占问题
            let rel_x = px - cap_x;
            let rel_y = py - cap_y;
            let mut diag = String::new();
            for i in 0..BLOCK_COUNT {
                let x = rel_x + STEP * i + BLOCK / 2;
                let y = rel_y + BLOCK / 2;
                if x >= 0 && x < CAP_W && y >= 0 && y < CAP_H {
                    let c = pixel(buf, x, y);
                    diag.push_str(&format!("{}:RGB({},{},{}) ", i, c.0, c.1, c.2));
                }
            }
            // 统计捕获区域的亮度：全黑通常意味着独占全屏无法用 BitBlt 抓取
            let mut non_black = 0u32;
            let mut max_sum = 0u32;
            for y in 0..CAP_H {
                for x in 0..CAP_W {
                    let c = pixel(buf, x, y);
                    let s = c.0 as u32 + c.1 as u32 + c.2 as u32;
                    if s > 12 {
                        non_black += 1;
                    }
                    if s > max_sum {
                        max_sum = s;
                    }
                }
            }
            crate::dbg_log!(
                "像素读取 原点=({},{}) 区域=({},{}) {}x{} 基准=({},{}) → 未找到蓝色锚点（非黑像素={}/{} 最亮={}）；按 1 倍缩放的期望采样点=[{}]",
                origin_x,
                origin_y,
                cap_x,
                cap_y,
                CAP_W,
                CAP_H,
                origin_x + px,
                origin_y + py,
                non_black,
                CAP_W * CAP_H,
                max_sum,
                diag.trim_end()
            );
        }
        return None;
    };

    let cy = ay + bh / 2; // 整排色块的垂直中心
    let anchor = pixel(buf, ax + bw / 2, cy);
    let verify = pixel(buf, ax + step + bw / 2, cy);
    if !is_cyan(verify) {
        if log_on && fail_log_allowed() {
            crate::dbg_log!(
                "像素读取 原点=({},{}) 基准=({},{}) 找到蓝色块=({},{}) {}x{} 步长={} 但其右侧不是青色校验块 RGB({},{},{})",
                origin_x,
                origin_y,
                origin_x + px,
                origin_y + py,
                ax,
                ay,
                bw,
                bh,
                step,
                verify.0,
                verify.1,
                verify.2
            );
        }
        return None;
    }

    let mut bits = 0u16;
    let mut detail = String::new(); // 每个色块的判定明细（仅日志开启时填充）
    for i in 0..10i32 {
        let x = ax + step * (2 + i) + bw / 2; // 第 i 个状态块中心
        if x < 0 || x >= CAP_W {
            if log_on {
                crate::dbg_log!("像素读取 第 {} 个状态块超出捕获区域，读取失败", i);
            }
            return None;
        }
        let c = pixel(buf, x, cy);
        // 真 = 绿（G 明显高于 R）；假 = 红（R 高于 G）
        let truthy = c.1 as i32 > c.0 as i32 + 16;
        if truthy {
            bits |= 1u16 << i;
        }
        if log_on && FIRST_OK.load(Ordering::Relaxed) {
            detail.push_str(&format!(
                "{}={}(RGB {},{},{}), ",
                state_display_name(STATE_IDS[i as usize]),
                if truthy { "真" } else { "假" },
                c.0,
                c.1,
                c.2
            ));
        }
    }

    if log_on && FIRST_OK.swap(false, Ordering::Relaxed) {
        let names: Vec<&str> = (0..10)
            .filter(|i| bits & (1u16 << i) != 0)
            .map(|i| state_display_name(STATE_IDS[i as usize]))
            .collect();
        crate::dbg_log!(
            "像素读取 首次成功 原点=({},{}) 基准=({},{}) 锚点=RGB({},{},{}) 块={}x{} 步长={} 校验=RGB({},{},{}) 位=0b{:010b} 状态=[{}] 明细=[{}]",
            origin_x,
            origin_y,
            origin_x + px,
            origin_y + py,
            anchor.0,
            anchor.1,
            anchor.2,
            bw,
            bh,
            step,
            verify.0,
            verify.1,
            verify.2,
            bits,
            if names.is_empty() { "无".to_string() } else { names.join("/") },
            detail.trim_end_matches(", ")
        );
    }
    Some(bits)
}

/// 状态位 → 可读文本（如 "潜行中/战斗中"，无状态时返回 "无"）
fn bits_text(v: u16) -> String {
    let names: Vec<&str> = (0..10)
        .filter(|i| v & (1u16 << i) != 0)
        .map(|i| state_display_name(STATE_IDS[i as usize]))
        .collect();
    if names.is_empty() {
        "无".to_string()
    } else {
        names.join("/")
    }
}

/// 取游戏窗口客户区左上角的屏幕坐标（窗口最小化/无效时返回 None）
fn game_client_origin(hwnd: isize) -> Option<(i32, i32)> {
    if hwnd == 0 {
        return None;
    }
    let h = HWND(hwnd as *mut core::ffi::c_void);
    unsafe {
        if IsIconic(h).as_bool() {
            return None; // 最小化时没有可读取的画面
        }
        let mut pt = POINT { x: 0, y: 0 };
        if !ClientToScreen(h, &mut pt).as_bool() {
            return None;
        }
        Some((pt.x, pt.y))
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
            let mut was_enabled = false; // 上周期采集开关，用于只在状态切换时打日志
            let mut last_bits: Option<u16> = None; // 上次解出的状态位（变化时才打日志）
            let mut hwnd: isize = 0; // 缓存的游戏窗口句柄
            let mut last_lookup: Option<Instant> = None; // 上次查找窗口的时间
            let mut window_ok: Option<bool> = None; // 上次窗口是否找到（避免刷屏）
            while running.load(Ordering::SeqCst) {
                let on = cfg.enabled.load(Ordering::SeqCst);
                if on != was_enabled {
                    crate::dbg_log!("像素采集 {}", if on { "已启动" } else { "已停止" });
                    was_enabled = on;
                }
                if on {
                    let exe = cfg
                        .exe
                        .lock()
                        .map(|s| s.clone())
                        .unwrap_or_default();
                    // 基准原点：优先用游戏窗口客户区左上角（插件把色块画在客户区左上角）
                    let (ox, oy) = if exe.trim().is_empty() {
                        (0, 0) // 未配置游戏 exe：退回屏幕左上角
                    } else {
                        // 每 2 秒或窗口失效时重新查找（EnumWindows 开销较大，不能每周期都做）
                        let need_lookup = hwnd == 0
                            || !crate::core::wow_sender::is_window_valid(hwnd)
                            || last_lookup.map_or(true, |t| t.elapsed() >= Duration::from_secs(2));
                        if need_lookup {
                            hwnd = crate::core::wow_sender::find_window_by_exe(&exe).unwrap_or(0);
                            last_lookup = Some(Instant::now());
                        }
                        match game_client_origin(hwnd) {
                            Some((x, y)) => {
                                if window_ok != Some(true) {
                                    crate::dbg_log!("像素采集 已定位游戏窗口 客户区原点=({},{})", x, y);
                                    window_ok = Some(true);
                                }
                                (x, y)
                            }
                            None => {
                                // 找不到游戏窗口时回退屏幕左上角（全屏且窗口在 (0,0) 时仍可用）
                                if window_ok != Some(false) {
                                    crate::dbg_log!(
                                        "像素采集 未找到游戏窗口（exe={}），回退屏幕左上角原点",
                                        exe
                                    );
                                    window_ok = Some(false);
                                }
                                (0, 0)
                            }
                        }
                    };
                    let x = cfg.x.load(Ordering::SeqCst);
                    let y = cfg.y.load(Ordering::SeqCst);
                    match read_states(ox, oy, x, y) {
                        Some(v) => {
                            bits.store(v);
                            // 状态位变化时单独打印一条，便于在日志里快速定位状态切换
                            if last_bits != Some(v) {
                                crate::dbg_log!(
                                    "角色状态 变化 {} → {}（位=0b{:010b}）",
                                    last_bits.map(bits_text).unwrap_or_else(|| "初始".to_string()),
                                    bits_text(v),
                                    v
                                );
                                last_bits = Some(v);
                            }
                        }
                        None => {
                            bits.store(0); // 插件未绘制（例如切出游戏）时清零
                            if last_bits.is_some() && last_bits != Some(0) {
                                crate::dbg_log!("角色状态 变化 {} → 无（插件未绘制，读取失败）", last_bits.map(bits_text).unwrap_or_default());
                                last_bits = Some(0);
                            }
                        }
                    }
                    let ms = cfg.interval_ms.load(Ordering::SeqCst).max(10) as u64;
                    thread::sleep(Duration::from_millis(ms));
                } else {
                    bits.store(0);
                    last_bits = None; // 关闭采集后重置，重新开启时打印一次「初始 → ...」
                    FIRST_OK.store(true, Ordering::Relaxed); // 下次启动采集时重新打印一次详情
                    window_ok = None;
                    hwnd = 0;
                    last_lookup = None;
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
