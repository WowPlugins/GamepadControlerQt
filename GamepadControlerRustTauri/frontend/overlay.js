/* =====================================================================
   overlay.js —— 悬浮窗前端逻辑（原生 JS + Tauri invoke）
   每 50ms 轮询 get_overlay_snapshot 渲染；点击标题或按钮切换展开，
   展开时调用 setSize 加高窗口容纳映射列表。
   ===================================================================== */

// 解构出 Tauri invoke（调用后端 Rust 命令）
const { invoke } = window.__TAURI__.core;
// 解构出 getCurrentWindow（调整悬浮窗大小）与 LogicalSize（逻辑像素尺寸）
const { getCurrentWindow, LogicalSize } = window.__TAURI__.window;

// ---------------------------------------------------------------------
// 前端状态
// ---------------------------------------------------------------------
let expanded = false; // 悬浮窗是否处于展开状态
let prevKey = null; // 上一次渲染内容对比串，避免重复渲染
let lastFitH = 0; // 上次贴合高度，高度没变就不 setSize，避免频繁调整
let lastFitW = 0; // 上次贴合宽度（收起/展开宽度不同）
let animating = false; // 展开/收起尺寸动画进行中，fitSize 跳过避免干扰分步生长
let collapsing = false; // 收起动画中：映射区由 setExpanded 控制 display，render 跳过避免截断淡出
let lastExpandRev = null; // 上次已处理的「切换展开」计数（手柄触发时后端递增，前端据此翻转）

const MIN_H = 24;  // 收起最小高度（防御下限；实际由 fitSize 精确贴合迷你卡片）
const MAX_H = 900; // 展开最大高度（内容完整显示的防御上限；悬浮窗高度随内容自适应，不裁剪）
const W = 300;     // 展开态宽度
const MINI_W = 160; // 收起态宽度（迷你圆角矩形）

// ---------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------
// esc：HTML 转义，防止特殊字符破坏页面结构
function esc(s) {
  // 依次替换 & < > " 为对应 HTML 实体
  return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// $：按 id 获取 DOM 元素的简写
function $(id) { return document.getElementById(id); }

// 字段级更新辅助：值没变就不写 DOM，避免轮询渲染时无意义重绘导致文字闪烁
function setText(id, v) { // 设置文本：相同则跳过
  const el = $(id);
  if (el.textContent !== v) el.textContent = v;
}
function setHTML(id, v) { // 设置 HTML：相同则跳过（避免重建 DOM 闪烁）
  const el = $(id);
  if (el.innerHTML !== v) el.innerHTML = v;
}
function setStyle(id, prop, v) { // 设置内联样式：相同则跳过
  const el = $(id);
  if (el.style[prop] !== v) el.style[prop] = v;
}

// ---------------------------------------------------------------------
// 轮询
// ---------------------------------------------------------------------
let ticking = false; // 轮询重入锁，防止上一次 invoke 未返回时又发起一次
async function tick() {
  if (ticking) return; // 上一次轮询未结束则跳过
  ticking = true; // 标记轮询进行中
  try {
    const snap = await invoke('get_overlay_snapshot'); // 调用后端命令获取悬浮窗快照
    if (lastExpandRev === null) { // 首次取快照
      lastExpandRev = snap.expand_rev; // 只记录基线，避免启动时误翻转
    } else if (snap.expand_rev !== lastExpandRev) { // 手柄触发了展开/收起切换
      lastExpandRev = snap.expand_rev; // 更新基线
      if (!animating) setExpanded(!expanded); // 动画进行中则跳过，避免打断高度动画
    }
    const key = JSON.stringify(snap) + '|' + expanded; // 组合对比串（含展开状态）
    if (key !== prevKey) { // 有变化才渲染
      prevKey = key; // 记录本次内容
      render(snap); // 渲染悬浮窗
    }
  } catch (e) {
    console.error('overlay tick:', e); // 出错打印日志（不中断定时器）
  }
  ticking = false; // 轮询结束，解除锁
}
setInterval(tick, 50); // 每 50ms 轮询一次

// ---------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------
function render(snap) {
  // 标题（值相同则跳过，按键时不会闪）
  setText('ov-set', '操作集: ' + esc(snap.set_name)); // 当前操作集名称
  setText('ov-layer', '当前层: ' + esc(snap.layer_name)); // 当前层名称
  setText('ov-toggle', expanded ? '收起' : '展开'); // 展开/收起按钮文案

  // 连接状态
  const dotCls = 'dot ' + (snap.connected ? 'ok' : 'off'); // 状态圆点类名
  if ($('ov-dot').className !== dotCls) $('ov-dot').className = dotCls; // 有变化才改
  if ($('ov-mini-dot').className !== dotCls) $('ov-mini-dot').className = dotCls; // 迷你条连接圆点同步
  setText('ov-conn', snap.connected ? '手柄已连接' : '手柄未连接'); // 连接状态文字
  setText('ov-mini-text', snap.layer_name || snap.set_name || '--'); // 迷你条显示当前层名（无层名时退回操作集名）

  // 按下按键（仅按键内容变化才重建 chips）：显示「按键名 → 映射功能」，收起状态也能看到功能
  const pressedRows = snap.pressed_rows || []; // 按下的按钮 + 映射描述（后端提供）
  setHTML('ov-pressed', pressedRows.length === 0
    ? '<span class="overlay-faint">无按键按下</span>' // 无按键时显示提示
    : pressedRows.map((r) => '<span class="press-chip">' + esc(r.button) // 按钮显示名
        + (r.desc ? '<span class="press-desc">→ ' + esc(r.desc) + '</span>' : '') // 映射功能（淡色小字）
        + '</span>').join(''));

  // L3 锁存警示（边框变橙 + 警示条）；ov-warn 固定高度占位，锁存切换只换内容不改变悬浮窗高度
  const lockText = snap.mouse_toggle || ''; // 鼠标左右键锁存文案（空串 = 无锁存，含 WASD 自动锁定 / 交换锁存）
  const autoLock = !!snap.auto_right_lock; // 右键是否为 WASD 左摇杆自动锁定
  const mouseToggle = lockText !== ''; // 是否有鼠标锁存
  const leftLocked = lockText.indexOf('左键') >= 0; // 文案里含左键 = 左键也被锁存
  let warnText = '鼠标长按锁存中，再按一次解除'; // 默认：由手柄按钮建立的锁存
  if (autoLock) { // 自动锁定的右键不能靠「再按一次」解除，文案要指明解除方式
    warnText = (leftLocked ? '左右键锁定中（右键自动）' : '鼠标右键锁定中（WASD 自动）');
  }
  setHTML('ov-warn', mouseToggle // 锁存时橙色警示条，平时弱化占位（高度恒定，避免悬浮窗抖动）
    ? '<div class="overlay-warn">⚠ ' + esc(warnText) + '</div>'
    : '<span class="overlay-dim">长按锁存: 无</span>');
  $('overlay-card').classList.toggle('mouse-toggle', mouseToggle); // 切换卡片橙色边框/内光晕样式（锁定状态）
  // 收起态隐藏锁存文字行（锁定状态改由边框颜色表示），展开态才显示文字警示
  setStyle('ov-warn', 'display', expanded ? 'flex' : 'none');

  // 卡片背景透明度跟随设置（透明度越低越透明，配合透明窗口）
  const op = typeof snap.opacity === 'number' ? snap.opacity : 0.85; // 取快照中的透明度，缺省 0.85
  const bg = 'rgba(43, 45, 49, ' + op + ')'; // 深色背景色（卡片与 body 同色，深色区域统一）
  if (document.body.style.background !== bg) document.body.style.background = bg; // body 兜底背景
  setStyle('overlay-card', 'background', bg); // 卡片背景

  // 展开：映射列表
  // 收起动画中 display 由 setExpanded 控制（映射区脱离文档流淡出），此处跳过避免截断淡出
  if (!collapsing) setStyle('ov-mappings', 'display', expanded ? 'flex' : 'none'); // 展开时显示映射区
  if (expanded) { // 仅展开时渲染映射列表
    setText('ov-map-title', '当前层映射: ' + esc(snap.layer_name)); // 映射列表标题
    const maxN = snap.max_mappings || 0; // 当前操作集映射最多的层的行数（后端计算）：用于占位固定展开高度
    const rows = snap.mappings.map((m) => // 遍历每条映射生成行
      '<div class="overlay-row">' + // 一行容器
      '<span class="ov-acc' + (m.held ? ' held' : '') + '">' + esc(m.button) + '</span>' + // 手柄按键名（按住中变橙色）
      '<span class="ov-dim">→</span>' + // 箭头分隔
      '<span class="ov-text">' + esc(m.desc) + '</span>' + // 映射动作描述
      '</div>'
    );
    for (let i = snap.mappings.length; i < maxN; i++) { // 不可见占位行补齐到 maxN
      rows.push('<div class="overlay-row ov-pad"></div>'); // 撑高占位：切层后映射行数不足时高度保持固定
    }
    setHTML('ov-map-list', rows.length === 0 // 无任何行时显示空提示
      ? '<span class="overlay-faint">（无映射）</span>'
      : rows.join(''));
  }
  fitSize(); // 内容变化后贴合窗口宽高（有守卫，尺寸没变不会重复 setSize）
}

// 贴合窗口宽高：展开态用宽尺寸、收起态用迷你宽度，高度始终贴合卡片
async function fitSize() {
  if (animating) return; // 展开/收起高度动画进行中，跳过避免干扰分步生长
  await new Promise((r) => requestAnimationFrame(r)); // 等布局完成再测量
  const card = $('overlay-card');
  // 窗口高度贴合卡片高度（无 shell 边距补偿，卡片铺满窗口）
  const h = Math.max(MIN_H, Math.min(card.offsetHeight, MAX_H)); // 夹在上下限之间
  const w = expanded ? W : MINI_W; // 展开/收起宽度
  if (h === lastFitH && w === lastFitW) return; // 尺寸没变则跳过
  lastFitH = h;             // 记录本次高度
  lastFitW = w;             // 记录本次宽度
  await getCurrentWindow().setSize(new LogicalSize(w, h));
}

// 分步调整窗口宽高到目标尺寸：平滑"生长/收缩"动画（多帧 setSize + 缓动）。
// 未传 targetArg 时按当前内容高度计算（展开用）；传了则用传入值（收起用收缩目标）。
// targetW 未传时按展开状态取宽度（展开 W / 收起 MINI_W）。
async function animateHeight(targetArg, targetW) {
  await new Promise((r) => requestAnimationFrame(r)); // 等布局完成再测量
  const card = $('overlay-card');
  const target = targetArg != null // 目标高度：传入值优先，否则按当前内容高度
    ? targetArg
    : Math.max(MIN_H, Math.min(card.offsetHeight, MAX_H)); // 夹在上下限（贴合卡片，无 shell 边距）
  const targetWv = targetW != null ? targetW : (expanded ? W : MINI_W); // 目标宽度
  if (target === lastFitH && targetWv === lastFitW) return; // 尺寸没变则跳过
  const from = lastFitH > 0 ? lastFitH : target; // 首帧无基线则直接用目标（避免从 0 生长）
  const win = getCurrentWindow(); // 窗口句柄
  const steps = 4; // 分步数：4 帧约 70ms 的快速平滑过渡（步数过多会拖慢且不流畅）
  for (let i = 1; i <= steps; i++) { // 逐帧生长/收缩
    const t = i / steps; // 进度 0~1
    const eased = t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2; // easeInOutQuad 缓动
    const h = Math.round(from + (target - from) * eased); // 当前帧高度
    lastFitH = h; // 记录中间高度（供守卫对比）
    lastFitW = targetWv; // 记录目标宽度
    await win.setSize(new LogicalSize(targetWv, h)); // 调整窗口宽高
    await new Promise((r) => setTimeout(r, 16)); // 帧间隔
  }
  lastFitH = target; // 最终精确到位
  lastFitW = targetWv; // 最终宽度到位
}

// ---------------------------------------------------------------------
// 展开 / 收起（同时调整窗口大小）
// ---------------------------------------------------------------------
async function setExpanded(v) {
  expanded = v;             // 更新展开状态
  prevKey = null;           // 强制重绘
  animating = true;         // 展开/收起全程禁止 fitSize 干预（render 内会调用）
  try {
    if (v) {
      // 展开：恢复完整布局 -> 映射区透明占位 -> 窗口分步生长 -> 淡入滑入归位
      $('overlay-card').classList.remove('collapsed'); // 恢复完整布局（迷你条隐藏）
      $('ov-warn').style.display = 'flex'; // 恢复锁存文字行（参与高度计算）
      $('ov-mappings').style.display = 'flex'; // 显示（文档流，撑高卡片参与布局）
      $('ov-mappings').style.opacity = '0'; // 透明占位
      $('ov-mappings').style.transform = 'translateY(10px) scale(0.98)'; // 起始下移微缩
      try {
        const snap = await invoke('get_overlay_snapshot'); // 获取快照
        render(snap);       // 渲染内容（占位，参与布局）
      } catch (e) { console.error('overlay render:', e); }
      await animateHeight(); // 窗口分步生长到展开高度
      requestAnimationFrame(() => { // 下一帧淡入滑入（触发 transition）
        $('ov-mappings').style.opacity = '1'; // 淡入
        $('ov-mappings').style.transform = 'translateY(0) scale(1)'; // 上移归位、还原缩放
      });
    } else {
      // 收起：映射区淡出滑出 → 切换为迷你布局 → 窗口收缩为迷你圆角矩形
      collapsing = true; // 收起动画中：禁止 render 把映射区 display 设 none（否则淡出被截断）
      $('ov-warn').style.display = 'none'; // 锁存文字行收起（锁定状态由边框颜色表示）
      $('ov-mappings').style.opacity = '0'; // 淡出
      $('ov-mappings').style.transform = 'translateY(10px) scale(0.98)'; // 下移微缩滑出
      await new Promise((r) => setTimeout(r, 200)); // 等淡出/滑出过渡完成（0.2s）
      $('overlay-card').classList.add('collapsed'); // 切换迷你布局（卡片立即收紧为一行）
      $('ov-mappings').style.display = 'none'; // 隐藏映射区
      $('ov-mappings').style.opacity = ''; // 清理内联透明度（恢复 CSS 默认，下次展开重新设置）
      $('ov-mappings').style.transform = ''; // 清理位移/缩放
      collapsing = false; // 恢复 render 对 display 的控制
      const miniH = Math.max(MIN_H, $('overlay-card').offsetHeight); // 迷你卡片实际高度
      await animateHeight(miniH, MINI_W); // 窗口宽高收缩到迷你尺寸
    }
  } finally {
    animating = false;      // 动画结束，恢复 fitSize 自动贴合
  }
}

// ---------------------------------------------------------------------
// 事件绑定
// ---------------------------------------------------------------------
$('ov-toggle').addEventListener('click', (e) => { // 展开/收起按钮
  e.stopPropagation(); // 阻止事件冒泡到标题，避免重复触发
  setExpanded(!expanded); // 切换展开状态
});
// 点击标题区域切换展开/收起（不影响拖动）
$('ov-title').addEventListener('click', (e) => { // 标题行点击
  if (e.target === $('ov-toggle')) return; // 点的是展开按钮则跳过（避免重复）
  setExpanded(!expanded); // 切换展开状态
});

// 收起态迷你条：单击展开；拖动窗口时不触发展开
let miniDownX = 0; // 按下起点 X
let miniDownY = 0; // 按下起点 Y
let miniMoved = false; // 是否已判定为拖动窗口

$('ov-mini').addEventListener('mousedown', (e) => { // 按下
  if (e.button !== 0 || expanded) return; // 仅收起态左键
  miniMoved = false; // 重置拖动标记
  miniDownX = e.screenX; // 记录按下起点
  miniDownY = e.screenY;
});
$('ov-mini').addEventListener('mousemove', (e) => { // 移动
  if (Math.abs(e.screenX - miniDownX) > 5 || Math.abs(e.screenY - miniDownY) > 5) { // 超过阈值视为拖动
    miniMoved = true; // 标记为拖动：松开时不展开
  }
});
$('ov-mini').addEventListener('mouseup', () => { // 松开
  if (!miniMoved && !expanded) setExpanded(true); // 单击：展开
});

$('openApp').addEventListener('click', (e) => {
  e.stopPropagation(); // 阻止事件冒泡到标题，避免重复触发
  invoke('open_app'); // 打开应用
});
