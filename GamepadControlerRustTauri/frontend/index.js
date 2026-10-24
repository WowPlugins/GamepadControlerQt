/* =====================================================================
   main.js —— 主窗口前端逻辑（原生 JS + Tauri invoke）
   每 50ms 轮询后端快照刷新界面；数据无变化时不重建 DOM 避免闪烁。
   层编辑、使用说明均为独立窗口（edit.html / help.html），
   由本文件通过 WebviewWindow 创建并复用。
   ===================================================================== */

// 解构出 Tauri 的 invoke 命令调用函数（用于调用后端 Rust 命令）
const { invoke } = window.__TAURI__.core;
// 解构出 getCurrentWindow（用于首帧渲染完成后显示主窗口）
const { getCurrentWindow } = window.__TAURI__.window;
// 解构出 WebviewWindow 类（用于创建 / 查找独立窗口）
const { WebviewWindow } = window.__TAURI__.webviewWindow;
// 解构出 emit 事件发送函数（用于向编辑窗口广播层 id）
const { emit } = window.__TAURI__.event;

// ---------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------
// esc：对字符串做 HTML 转义，防止特殊字符破坏页面结构或引发注入
function esc(s) {
  // 依次替换 & < > " 为对应的 HTML 实体
  return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// 生成单引号包裹的 JS 字符串字面量，用于双引号 onclick 属性内的实参
// （JSON.stringify 会生成双引号，嵌进双引号 onclick 属性会断裂 HTML，故改用单引号）
function q(s) {
  // 先转义反斜杠，再转义单引号，最后用单引号包裹，得到安全的 JS 字符串字面量
  return "'" + String(s).replace(/\\/g, '\\\\').replace(/'/g, "\\'") + "'";
}

// $：按 id 获取 DOM 元素的简写
function $(id) { return document.getElementById(id); }

// ---------------------------------------------------------------------
// 前端状态
// ---------------------------------------------------------------------
let overlayVisible = false; // 悬浮窗当前是否显示（用于切换按钮文案）
let renameMode = null;         // 'rename' | 'copy' | null —— 重命名/复制输入行的模式
let renameSetId = null;        // 正在重命名/复制的操作集 id
let renameValue = '';          // 输入框当前内容（避免轮询重绘时覆盖用户输入）
let activeSetId = '';          // 当前激活操作集 id
let activeSetName = '';        // 当前激活操作集名称
let running = false;           // 映射是否正在运行（决定开始/停止按钮行为）
let overlayOpacity = null;     // 悬浮窗透明度（0.2 ~ 1.0），与后端一致
let prevWowKey = null;         // 上一次目标 exe 下拉框指纹，避免轮询重建把展开中的下拉框重置

let prevMain = null;           // 上一次渲染的快照 JSON 串，用于对比避免重复渲染

// ---------------------------------------------------------------------
// 轮询
// ---------------------------------------------------------------------
let ticking = false; // 轮询重入锁，防止上一次 invoke 未返回时又发起一次
async function tick() {
  if (ticking) return; // 若上一次轮询还没结束则跳过本次
  ticking = true;      // 标记轮询进行中
  try {
    const snap = await invoke('get_snapshot'); // 调用后端命令获取主界面完整快照
    const key = JSON.stringify(snap);          // 将快照序列化为字符串便于比对
    if (key !== prevMain) {                    // 仅当快照有变化时才重新渲染
      prevMain = key;                          // 记录本次快照
      renderMain(snap);                        // 重新渲染主视图
    }
  } catch (e) {
    console.error('tick:', e); // 轮询出错时打印日志（不中断定时器）
  }
  ticking = false; // 轮询结束，解除锁
}
setInterval(tick, 50); // 每 50ms 轮询一次后端

// ---------------------------------------------------------------------
// 主视图渲染
// ---------------------------------------------------------------------
function renderMain(snap) {
  running = snap.running;             // 同步映射运行状态到前端变量
  activeSetId = snap.active_set_id;   // 同步当前操作集 id
  activeSetName = snap.active_set_name; // 同步当前操作集名称
  overlayOpacity = snap.overlay_opacity;

  // 状态
  const statusText = snap.connected // 根据连接与运行状态拼装状态文案
    ? (snap.running ? '● 已连接 · 映射运行中' : '● 已连接 · 已停止')
    : '○ 手柄未连接';
  $('status-text').textContent = statusText; // 写入状态文字
  $('status-dot').className = 'dot ' + (snap.connected ? 'ok' : 'off'); // 切换状态圆点样式

  // 重命名输入行
  renderRenameRow(); // 根据 renameMode 显示/隐藏输入行

  // 操作集 chips
  $('set-chips').innerHTML = snap.sets.map((s) => // 遍历所有操作集生成 chip 按钮
    '<div class="chip' + (s.id === snap.active_set_id ? ' active' : '') + '" ' + // 当前操作集加 active 高亮
    'onclick="App.switchSet(' + q(s.id) + ')">' + esc(s.name) + '</div>' // 点击切换操作集（q 生成单引号实参）
  ).join('');

  // 层列表（点击打开独立「编辑层」窗口）
  const layers = [{ id: 'Common', name: '公共层', active: false }].concat(snap.layers); // 公共层固定排最前，再接操作层
  $('layer-list').innerHTML = layers.map((l) => // 遍历所有层生成按钮
    '<div class="layer-btn' + (l.active ? ' active' : '') + '" ' + // 当前层加 active 高亮
    'onclick="App.openEdit(' + q(l.id) + ')">' + esc(l.name) + '</div>' // 点击打开对应层的编辑窗口
  ).join('');

  // 当前信息
  $('info-col').innerHTML =
    '<div class="info-line">当前操作集: ' + esc(snap.active_set_name) + '</div>' + // 当前操作集行
    '<div class="info-line">当前层: ' + esc(snap.layer_name) + '</div>' + // 当前层行
    '<div class="info-sub" style="color:' + (snap.mouse_toggle ? '#f0a34a' : '#6c727c') + '">' + // 锁存警示，激活时橙色
    (snap.mouse_toggle ? esc(snap.mouse_toggle) : '无长按锁存') + '</div>'; // 显示锁存按键或“无长按锁存”

  // 全局设置
  const settings = [ // 全局设置项列表：[显示名, 当前值, 后端键名]
    ['死区', snap.deadzone, 'deadzone'],
    ['视角灵敏度', snap.look_sensitivity, 'look_sensitivity'],
    ['视角平滑', snap.look_smoothing, 'look_smoothing'],
    ['视角加速', snap.look_acceleration, 'look_acceleration'],
  ];
  $('settings-col').innerHTML = settings.map(([label, val, key]) => // 为每项生成一行调整控件
    '<div class="slider-row">' + // 一行容器
    '<div class="slider-label">' + label + '</div>' + // 设置项名称
    '<button class="mini" onclick="App.adjust(\'' + key + '\',-0.01)">−</button>' + // 减小按钮（注意此处用转义单引号）
    '<div class="slider-val">' + val.toFixed(2) + '</div>' + // 当前数值（保留两位小数）
    '<button class="mini" onclick="App.adjust(\'' + key + '\',0.01)">+</button>' + // 增大按钮
    '</div>'
  ).join('');

  // 目标游戏 exe（下拉框仅在内容变化时重建，避免展开中被轮询刷新关闭）
  const wowHistory = snap.wow_exe_history || []; // 历史路径列表
  const wowKey = (snap.wow_exe || '') + '|' + wowHistory.join(';'); // 指纹
  if (wowKey !== prevWowKey) { // 内容变化才重建
    prevWowKey = wowKey; // 记录指纹
    const sel = $('wow-exe-select'); // 下拉框
    const placeholder = wowHistory.length ? '选择历史路径' : '未选择（点浏览添加）'; // 占位项文案
    sel.innerHTML = '<option class="disabled" value="">' + placeholder + '</option>' + // 占位项
      wowHistory.map((p) => // 遍历历史路径
        '<option value="' + esc(p) + '"' + (p === snap.wow_exe ? ' selected' : '') + '>' + esc(p) + '</option>'
      ).join('');
    sel.value = snap.wow_exe || ''; // 同步当前选中项
    $('wow-exe-current').textContent = snap.wow_exe ? '当前: ' + snap.wow_exe : '当前: 未选择（WASD 走普通注入）'; // 路径显示
  }

  // 开始/停止 + 悬浮窗按钮
  const toggle = $('btn-toggle'); // 获取开始/停止按钮元素
  toggle.textContent = snap.running ? '停止映射' : '开始映射'; // 按运行状态切换按钮文字
  toggle.className = 'btn-toggle ' + (snap.running ? 'running' : 'stopped'); // 切换按钮颜色样式（运行红/停止青）
  $('btn-overlay').textContent = overlayVisible ? '关闭悬浮窗' : '显示悬浮窗'; // 悬浮窗按钮文案
  $('opacity-val').textContent = overlayOpacity;
}

// renderRenameRow：根据 renameMode 决定重命名/复制输入行的显示与内容
function renderRenameRow() {
  const rr = $('rename-row'); // 输入行容器
  if (renameMode) { // 处于重命名/复制模式时显示
    rr.style.display = 'flex'; // 显示输入行
    $('rename-hint').textContent = renameMode === 'rename' ? '重命名操作集' : '复制为新操作集'; // 提示文字
    const inp = $('rename-input'); // 输入框
    if (document.activeElement !== inp) inp.value = renameValue; // 输入框未被聚焦时才回填，避免覆盖正在输入的内容
  } else {
    rr.style.display = 'none'; // 非编辑模式隐藏输入行
  }
}

// ---------------------------------------------------------------------
// 独立窗口：编辑层 / 使用说明
// ---------------------------------------------------------------------
// 编辑层窗口：固定标签 'edit-win'。首次打开通过 URL hash 传入层 id
// （edit.html#<layerId>）；窗口已存在时复用，并用 'edit-layer' 事件
// 推送新的层 id，避免整窗刷新。
function openEditWindow(id) {
  WebviewWindow.getByLabel('edit-win').then((existing) => { // getByLabel 是异步方法，返回 Promise
    if (existing) { // 编辑窗口已存在 → 复用
      emit('edit-layer', { layerId: id }); // 先通过事件把新的层 id 推送给编辑窗口（隐藏时也会立即处理）
      existing.show(); // 显示窗口（预热创建的窗口此前处于隐藏状态）
      existing.setFocus(); // 将编辑窗口置顶聚焦
    } else { // 编辑窗口不存在 → 新建
      const win = new WebviewWindow('edit-win', { // 创建固定标签为 edit-win 的独立窗口
        url: 'edit.html#' + encodeURIComponent(id), // URL 带 hash 传入层 id
        title: '编辑层', // 窗口标题
        width: 780,      // 窗口宽度
        height: 620,     // 窗口高度
        minWidth: 680,   // 最小宽度
        minHeight: 520,  // 最小高度
        center: true,    // 创建后居中
        backgroundColor: '#26282c', // 窗口背景色（避免打开瞬间白屏闪动）
        visible: false,  // 先隐藏，页面首帧渲染完成后由 edit.js 调用 show() 再显示，杜绝白屏
      });
      win.once('tauri://error', (e) => console.error('编辑窗口创建失败:', e)); // 监听创建失败事件并打印
    }
  });
}

// openHelpWindow：打开「使用说明」独立窗口（不存在则新建，已存在则复用聚焦）
function openHelpWindow() {
  WebviewWindow.getByLabel('help-win').then((existing) => { // 异步查找 help-win 窗口
    if (existing) { // 已存在 → 复用
      existing.show(); // 显示窗口（预热创建的窗口此前处于隐藏状态）
      existing.setFocus(); // 置顶聚焦
      return; // 直接返回，不新建
    }
    const win = new WebviewWindow('help-win', { // 创建使用说明窗口
      url: 'help.html',   // 加载帮助页面
      title: '使用说明',  // 窗口标题
      width: 660,         // 窗口宽度
      height: 740,        // 窗口高度
      minWidth: 500,      // 最小宽度
      minHeight: 420,     // 最小高度
      center: true,       // 创建后居中
      backgroundColor: '#26282c', // 窗口背景色（避免打开瞬间白屏闪动）
      visible: false,     // 先隐藏，页面渲染完成后由 help.js 调用 show() 再显示，杜绝白屏
    });
    win.once('tauri://error', (e) => console.error('使用说明窗口创建失败:', e)); // 监听创建失败事件并打印
  });
}

// prewarmWindows：应用启动后预创建并加载编辑层 / 使用说明窗口（保持隐藏）。
// 用户点击时 WebView2 已加载完毕，可瞬间显示，避免首次打开等待窗口加载。
function prewarmWindows() {
  WebviewWindow.getByLabel('edit-win').then((existing) => { // 异步检查编辑窗口是否已存在
    if (!existing) { // 不存在则预创建（隐藏加载）
      new WebviewWindow('edit-win', { // 参数与 openEditWindow 保持一致
        url: 'edit.html?prewarm=1#Common', // 带 prewarm 标记 + 初始层 id（edit.js 检测 prewarm 不自动显示）
        title: '编辑层',
        width: 780,
        height: 620,
        minWidth: 680,
        minHeight: 520,
        center: true,
        backgroundColor: '#26282c',
        visible: false, // 保持隐藏，用户点击时由 openEditWindow 调 show() 显示
      });
    }
  });
  WebviewWindow.getByLabel('help-win').then((existing) => { // 异步检查帮助窗口是否已存在
    if (!existing) { // 不存在则预创建（隐藏加载）
      new WebviewWindow('help-win', { // 参数与 openHelpWindow 保持一致
        url: 'help.html?prewarm=1', // 带 prewarm 标记（help.js 检测 prewarm 不自动显示）
        title: '使用说明',
        width: 660,
        height: 740,
        minWidth: 500,
        minHeight: 420,
        center: true,
        backgroundColor: '#26282c',
        visible: false, // 保持隐藏，用户点击时由 openHelpWindow 调 show() 显示
      });
    }
  });
}

// ---------------------------------------------------------------------
// 动作
// ---------------------------------------------------------------------
// App：全局动作集合，供内联 onclick 及事件绑定调用
const App = {
  // ---- 操作集 ----
  switchSet(id) { invoke('switch_operation_set', { setId: id }); }, // 切换当前操作集
  addSet() { invoke('add_operation_set'); }, // 新增一个操作集
  startCopy() { // 进入“复制为新操作集”编辑模式
    renameMode = 'copy'; // 标记为复制模式
    renameSetId = activeSetId; // 记录要复制的源操作集
    renameValue = activeSetName + ' - 副本'; // 预填默认新名称
    renderRenameRow(); // 显示输入行
    $('rename-input').focus(); // 聚焦输入框
    $('rename-input').select(); // 全选输入框文本方便直接改名
  },
  startRename() { // 进入“重命名操作集”编辑模式
    renameMode = 'rename'; // 标记为重命名模式
    renameSetId = activeSetId; // 记录要重命名的操作集
    renameValue = ''; // 清空预填，让用户重新输入
    renderRenameRow(); // 显示输入行
    $('rename-input').focus(); // 聚焦输入框
  },
  commitRename() { // 确认重命名/复制
    const name = renameValue.trim(); // 去掉首尾空格
    if (!name || !renameMode) return; // 名称为空或不在编辑模式则忽略
    if (renameMode === 'rename') { // 重命名模式
      invoke('rename_operation_set', { setId: renameSetId, name }); // 调用后端重命名
    } else { // 复制模式
      invoke('copy_operation_set', { setId: renameSetId, name }); // 调用后端复制为新操作集
    }
    renameMode = null; // 退出编辑模式
    renderRenameRow(); // 隐藏输入行
  },
  cancelRename() { renameMode = null; renderRenameRow(); }, // 取消编辑：退出模式并隐藏输入行
  deleteSet() { invoke('delete_operation_set', { setId: activeSetId }); }, // 删除当前操作集

  // ---- 启停 / 配置 ----
  toggleMapping() { invoke(running ? 'stop_mapping' : 'start_mapping'); }, // 按当前状态启/停映射
  saveConfig() { invoke('save_config'); }, // 保存配置到配置文件
  resetConfig() { invoke('reset_config'); }, // 重置为默认配置

  // ---- 悬浮窗 / 退出 ----
  toggleOverlay() { invoke('toggle_overlay').then((v) => { overlayVisible = v; }); }, // 切换悬浮窗并同步显示状态
  quit() { invoke('quit_app'); }, // 退出程序

  // ---- 设置 ----
  adjust(key, delta) { invoke('adjust_setting', { key, delta }); }, // 微调指定全局设置项
  adjustOpacity(delta) { // 调整悬浮窗透明度
    overlayOpacity = Math.min(1, Math.max(0.2, overlayOpacity + delta)); // 限制在 0.2~1.0 之间
    const v = $('opacity-val'); // 透明度数值显示元素
    if (v) v.textContent = Math.round(overlayOpacity * 100) + '%'; // 更新为百分比显示
    invoke('set_overlay_opacity', { opacity: overlayOpacity }); // 通知后端应用透明度
  },

  // ---- 目标游戏 exe ----
  pickWow() { // 浏览选择目标游戏 exe
    invoke('pick_wow_exe')
      .then(() => { prevWowKey = null; }) // 成功后清空指纹，强制刷新下拉框
      .catch((e) => console.error('pick_wow_exe:', e));
  },
  selectWow(path) { // 从历史记录中选择目标游戏 exe
    if (!path) return; // 占位项不提交
    prevWowKey = null; // 强制刷新下拉框
    invoke('set_wow_exe', { path }).catch((e) => console.error('set_wow_exe:', e));
  },

  // ---- 独立窗口 ----
  openEdit(id) { openEditWindow(id); }, // 打开指定层的编辑窗口
  openHelp() { openHelpWindow(); }, // 打开使用说明窗口
};

// ---------------------------------------------------------------------
// 静态按钮绑定
// ---------------------------------------------------------------------
$('btn-add').addEventListener('click', () => App.addSet()); // 添加操作集
$('btn-copy').addEventListener('click', () => App.startCopy()); // 复制操作集
$('btn-rename').addEventListener('click', () => App.startRename()); // 重命名操作集
$('btn-delete').addEventListener('click', () => App.deleteSet()); // 删除操作集
$('btn-toggle').addEventListener('click', () => App.toggleMapping()); // 开始/停止映射
$('btn-save').addEventListener('click', () => App.saveConfig()); // 保存配置
$('btn-reset').addEventListener('click', () => App.resetConfig()); // 重置默认配置
$('btn-overlay').addEventListener('click', () => App.toggleOverlay()); // 显示/关闭悬浮窗
$('btn-help').addEventListener('click', () => App.openHelp()); // 打开使用说明
$('btn-wow').addEventListener('click', () => App.pickWow()); // 浏览选择目标游戏 exe
$('wow-exe-select').addEventListener('change', (e) => App.selectWow(e.target.value)); // 从历史记录选择目标 exe
$('btn-quit').addEventListener('click', () => App.quit()); // 退出程序
$('rename-ok').addEventListener('click', () => App.commitRename()); // 确认重命名/复制
$('rename-cancel').addEventListener('click', () => App.cancelRename()); // 取消重命名/复制
$('rename-input').addEventListener('input', (e) => { renameValue = e.target.value; }); // 输入框内容同步到变量
$('rename-input').addEventListener('keydown', (e) => { // 输入框键盘事件
  if (e.key === 'Enter') App.commitRename(); // 回车确认
  if (e.key === 'Escape') App.cancelRename(); // Esc 取消
});

// ---------------------------------------------------------------------
// 点击波纹动效（全局委托：按钮 / chip / 层 / 手柄按键 / 设置小按钮）
// ---------------------------------------------------------------------
document.addEventListener('pointerdown', (e) => { // 监听任意指针按下（事件委托，适用于动态生成的元素）
  const el = e.target.closest('.btn, .chip, .layer-btn, .pad-btn, .mini, .btn-toggle, .ov-toggle'); // 命中可点击元素
  if (!el) return; // 未命中则忽略
  const rect = el.getBoundingClientRect(); // 获取元素位置与尺寸
  const d = Math.max(rect.width, rect.height); // 波纹直径取宽高较大值
  const span = document.createElement('span'); // 创建波纹元素
  span.className = 'ripple'; // 应用波纹样式类
  span.style.width = d + 'px'; // 波纹宽度
  span.style.height = d + 'px'; // 波纹高度
  span.style.left = (e.clientX - rect.left - d / 2) + 'px'; // 波纹水平位置（以点击点为圆心）
  span.style.top = (e.clientY - rect.top - d / 2) + 'px'; // 波纹垂直位置
  el.appendChild(span); // 将波纹挂到元素内
  setTimeout(() => span.remove(), 600); // 动画结束后移除波纹节点
});

// ---------------------------------------------------------------------
// 首帧渲染完成后显示主窗口（主窗口以 visible:false 创建，避免冷启动白屏闪动）
// ---------------------------------------------------------------------
(async () => {
  try {
    await tick(); // 立即先执行一轮渲染
  } catch (e) {
    console.error('首帧渲染失败:', e); // 渲染出错打印日志
  }
  getCurrentWindow().show(); // 内容就绪后再显示窗口，杜绝白屏
  prewarmWindows(); // 预热编辑层 / 使用说明窗口（隐藏加载，点击时瞬间显示）
})();
// 兜底：若首帧渲染异常导致未显示，1.5 秒后强制显示，避免窗口"打不开"
setTimeout(() => getCurrentWindow().show(), 1500);
