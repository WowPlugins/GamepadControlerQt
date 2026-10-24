/* =====================================================================
   edit.js —— 独立「编辑层」窗口（WebviewWindow "edit-win"）
   层 id 通过 URL hash 传入（edit.html#<layerId>）；主窗口复用该窗口
   时会广播 'edit-layer' 事件携带新的层 id。每 50ms 轮询渲染。
   ===================================================================== */

// 解构出 Tauri invoke（调用后端 Rust 命令）
const { invoke } = window.__TAURI__.core;
// 解构出 getCurrentWindow（用于关闭本窗口）
const { getCurrentWindow } = window.__TAURI__.window;
// 解构出 listen（用于接收主窗口广播的层 id）
const { listen } = window.__TAURI__.event;

// ---------------------------------------------------------------------
// 常量（与后端 core/input_types.rs android_key 一致）
// ---------------------------------------------------------------------
// COMMON_KEYS：可映射的键盘键列表（补全，含字母/数字/功能键/特殊键/方向键/符号键/小键盘），
// [Android 键码, 显示名]（显示名与后端 key_code_to_name 一致，用于子命令勾选态匹配）
const COMMON_KEYS = [
  // 字母 A-Z
  [29, 'A'], [30, 'B'], [31, 'C'], [32, 'D'], [33, 'E'], [34, 'F'], [35, 'G'], [36, 'H'],
  [37, 'I'], [38, 'J'], [39, 'K'], [40, 'L'], [41, 'M'], [42, 'N'], [43, 'O'], [44, 'P'],
  [45, 'Q'], [46, 'R'], [47, 'S'], [48, 'T'], [49, 'U'], [50, 'V'], [51, 'W'], [52, 'X'],
  [53, 'Y'], [54, 'Z'],
  // 数字 0-9
  [7, '0'], [8, '1'], [9, '2'], [10, '3'], [11, '4'], [12, '5'], [13, '6'], [14, '7'],
  [15, '8'], [16, '9'],
  // 功能键 F1-F12
  [131, 'F1'], [132, 'F2'], [133, 'F3'], [134, 'F4'], [135, 'F5'], [136, 'F6'],
  [137, 'F7'], [138, 'F8'], [139, 'F9'], [140, 'F10'], [141, 'F11'], [142, 'F12'],
  // 修饰键
  [59, 'Shift'], [113, 'Ctrl'], [57, 'Alt'],
  // 特殊键
  [62, 'Space'], [66, 'Enter'], [61, 'Tab'], [111, 'Esc'], [67, 'Backspace'],
  [124, 'Insert'], [123, 'Home'], [122, 'End'], [92, 'PageUp'], [93, 'PageDown'],
  // 方向键
  [19, '↑'], [20, '↓'], [21, '←'], [22, '→'],
  // 符号键
  [68, '`'], [69, '-'], [70, '='], [71, '['], [72, ']'], [73, '\\'], [74, ';'],
  [75, "'"], [55, ','], [56, '.'], [76, '/'],
  // 锁键
  [115, 'CapsLock'], [143, 'NumLock'], [116, 'ScrollLock'],
  // 小键盘 0-9
  [144, 'Num0'], [145, 'Num1'], [146, 'Num2'], [147, 'Num3'], [148, 'Num4'],
  [149, 'Num5'], [150, 'Num6'], [151, 'Num7'], [152, 'Num8'], [153, 'Num9'],
];

// SUB_COMMAND_KEYS：子命令可选的特殊功能按键（仅键盘动作显示），
// 只提供键盘上方的特殊功能按键，不含普通字母/数字
const SUB_COMMAND_KEYS = [
  // 修饰键
  [59, 'Shift'], [113, 'Ctrl'], [57, 'Alt'],
  // 常用特殊键
  [66, 'Enter'], [61, 'Tab'], [111, 'Esc'], [62, 'Space'],
  // 方向键
  [19, '↑'], [20, '↓'], [21, '←'], [22, '→'],
  // 编辑/导航键
  [67, 'Backspace'], [124, 'Insert'], [123, 'Home'], [122, 'End'],
  [92, 'PageUp'], [93, 'PageDown'], [115, 'CapsLock'],
  // 功能键 F1-F12
  [131, 'F1'], [132, 'F2'], [133, 'F3'], [134, 'F4'], [135, 'F5'], [136, 'F6'],
  [137, 'F7'], [138, 'F8'], [139, 'F9'], [140, 'F10'], [141, 'F11'], [142, 'F12'],
];

// KINDS：动作类型列表，[后端枚举名, 中文显示名]
const KINDS = [
  ['none', '未设置'], // 未设置：清除该按键的映射（含长按）
  ['keyboard', '键盘'], // 键盘按键
  ['mouse', '鼠标点击'], // 鼠标点击
  ['mousetoggle', '鼠标长按'], // 鼠标长按锁存
  ['wheelup', '滚轮上'], // 鼠标滚轮上滚
  ['wheeldown', '滚轮下'], // 鼠标滚轮下滚
  ['switchlayer', '切层'], // 切换层
  ['lookaround', '视角控制'], // 右摇杆视角控制
  ['mousemove', '鼠标移动'], // 鼠标移动
  ['interactionkey', '交互键'], // 交互键：临时解锁锁存，屏幕中央左右键各点一次
  ['releasemousetoggle', '释放鼠标锁存'], // 释放鼠标左右键长按锁存
  ['swapmousetoggle', '交换左右键锁存'], // 交换鼠标左右键长按锁存
  ['finelook', '鼠标精细移动'], // 按住时右摇杆低灵敏度
];

// LONG_PRESS_KINDS：可作为「长按动作」的类型（切层由映射引擎处理，未设置无长按，均不能作为长按动作）
const LONG_PRESS_KINDS = KINDS.filter(([k]) => k !== 'switchlayer' && k !== 'none');

// MOUSE_BUTTONS：可映射的鼠标按键列表，[枚举名, 中文显示名]
const MOUSE_BUTTONS = [
  ['LEFT', '鼠标左键'], // 左键
  ['RIGHT', '鼠标右键'], // 右键
  ['MIDDLE', '鼠标中键'], // 中键
  ['FORWARD', '鼠标前进键'], // 前进键（侧键）
  ['BACK', '鼠标后退键'], // 后退键（侧键）
];

// NO_TARGET_KINDS：无需选择额外目标的动作类型，选中即生效
const NO_TARGET_KINDS = ['none', 'wheelup', 'wheeldown', 'lookaround', 'mousemove', 'interactionkey', 'releasemousetoggle', 'swapmousetoggle', 'finelook'];

// ---------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------
// esc：HTML 转义，防止特殊字符破坏页面结构
function esc(s) {
  // 依次替换 & < > " 为对应 HTML 实体
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
// 状态：层 id 取自 URL hash（edit.html#<layerId>）
// ---------------------------------------------------------------------
let editLayerId = decodeURIComponent(location.hash.replace(/^#/, '')) || 'Common'; // 当前编辑的层 id，默认公共层
let selectedButton = 'A'; // 当前选中的手柄按键
let editKind = 'none'; // 当前选中的动作类型（无映射时为"未设置"）
let editLongKind = null; // 当前选中的长按动作类型（null = 未配置长按）
let currentLongPressMs = 500; // 当前按键的长按判定阈值（毫秒，用于加减按钮）
let prevEdit = null; // 上一次渲染内容对比串，避免重复渲染
let prevSelectKey = null; // 上一次「激活按钮」下拉框内容指纹，避免重建 DOM 把展开中的下拉框重置
let renameLayerMode = false; // 是否正在重命名操作集
let renameLayerId = null; // 当前重命名的操作集 id
let renameLayerValue = ''; // 当前重命名的名称值

// 主窗口复用本窗口并切换目标层时，通过事件推送新的层 id
listen('edit-layer', (e) => { // 注册 edit-layer 事件监听
  const id = (e.payload && e.payload.layerId) || 'Common'; // 读取事件携带的层 id，缺省为公共层
  editLayerId = id; // 更新当前编辑的层 id
  selectedButton = 'A'; // 重置选中按键为 A
  editKind = 'none'; // 重置动作类型为未设置
  editLongKind = null; // 重置长按动作类型为未配置
  prevEdit = null; // 清空对比串，强制下一轮重新渲染
  prevSelectKey = null; // 清空下拉框指纹，切换层后强制重建选项
  // 读取新层 A 键的现有映射类型
  invoke('get_mapping', { layerId: id, button: 'A' }).then((map) => { // 查询 A 键当前映射
    if (map.has_mapping) editKind = map.kind; else editKind = 'none'; // 有映射则按原类型显示，否则为未设置
    editLongKind = map.long_press ? map.long_press.kind : null; // 同步长按动作类型
    prevEdit = null; // 再次强制下一轮重新渲染
  });
});

// ---------------------------------------------------------------------
// 轮询
// ---------------------------------------------------------------------
let ticking = false; // 轮询重入锁，防止上一次 invoke 未返回时又发起一次
async function tick() {
  if (ticking) return; // 上一次轮询未结束则跳过
  ticking = true; // 标记轮询进行中
  try {
    const info = await invoke('get_layer_edit_snapshot', { layerId: editLayerId }); // 获取当前层编辑快照（按键网格/层名等）
    const map = await invoke('get_mapping', { layerId: editLayerId, button: selectedButton }); // 获取当前选中按键的映射
    const key = JSON.stringify(info) + '|' + JSON.stringify(map); // 组合成对比串
    if (key !== prevEdit) { // 有变化才渲染
      prevEdit = key; // 记录本次内容
      renderEdit(info, map); // 渲染编辑界面
    }
  } catch (e) {
    console.error('edit tick:', e); // 出错打印日志（不中断定时器）
  }
  ticking = false; // 轮询结束，解除锁
}
setInterval(tick, 50); // 每 50ms 轮询一次

// ---------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------
function renderEdit(info, map) {
  const btnDisplay = (info.buttons.find((b) => b.name === selectedButton) || {}).display || selectedButton; // 当前选中按键的显示名
  $('edit-title').textContent = '编辑层: ' + info.layer_name; // 标题显示层名
  $('edit-current').textContent = '当前: ' + btnDisplay + ' (' + map.desc + ')'; // 显示当前按键与其映射描述
  // 公共层恒为激活层，不存在"激活按钮"：隐藏该控件（写入公共层的切层映射是无效动作，
  // 还会把公共层上该按键原有的映射覆盖掉，故后端也做了同样的拦截）
  const isCommon = editLayerId === 'Common'; // 是否正在编辑公共层
  $('edit-layer-select-label').style.display = isCommon ? 'none' : ''; // 标签显隐
  $('edit-layer-select').style.display = isCommon ? 'none' : ''; // 下拉框显隐

  // 激活按钮下拉框：仅当"按键集合 / 选中的激活按钮"变化时才重建 DOM。
  // 重建 innerHTML 会重置（并关闭）正在展开的下拉框，而手柄按键按下/松开会不断触发本函数重渲染，
  // 表现为"选完没反应"，故先用指纹比对跳过无变化的重建。
  const selectKey = info.buttons.map((b) => b.name).join(',') + '|' + (info.trigger_button || '');
  if (selectKey !== prevSelectKey) { // 内容有变化才重建
    prevSelectKey = selectKey; // 记录本次指纹
    const sel = $('edit-layer-select'); // 激活按钮下拉框
    // 未设置激活按钮时补一个占位项（value 为空串，提交时由 layerActiveButton 忽略）
    const placeholder = info.trigger_button ? '' : '<option class="disabled" value="">未设置激活按钮</option>';
    sel.innerHTML = placeholder + info.buttons.map((b) => // 遍历所有手柄按键生成选项
      '<option value="' + b.name + '"' + (b.name === info.trigger_button ? 'selected' : '') + '>' + esc(b.display) + '</option>'
    ).join('');
    sel.value = info.trigger_button || ''; // 同步选中项（无激活按钮时回到占位项）
  }

  // 手柄按钮网格
  $('edit-grid').innerHTML = info.buttons.map((b) => // 遍历所有手柄按键生成网格按钮
    '<div class="pad-btn' + // 按键按钮基础样式
    (b.name === selectedButton ? ' sel' : '') + // 选中的按键加 sel 高亮
    (b.pressed && b.name !== selectedButton ? ' pressed' : '') + '" ' + // 物理按下的其他按键加 pressed 样式
    'onclick="App.selectButton(' + q(b.name) + ')">' + esc(b.display) + '</div>' // 点击切换选中按键（单引号实参）
  ).join('');

  // 动作类型 chips
  $('edit-kinds').innerHTML = KINDS.map(([k, label]) => // 遍历动作类型生成 chip
    '<div class="chip' + (k === editKind ? ' active' : '') + '" ' + // 当前类型加 active 高亮
    'onclick="App.setKind(' + q(k) + ')">' + label + '</div>' // 点击切换动作类型
  ).join('');

  // 目标区
  $('edit-target').innerHTML = targetHTML(info, editKind, map); // 按动作类型渲染目标选择区

  // 子命令（键盘/滚轮动作提供组合键；其余动作类型不提供）
  const showSubs = editKind === 'keyboard' || editKind === 'wheelup' || editKind === 'wheeldown'; // 是否显示子命令区（键盘/滚轮支持组合键）
  $('edit-subs').style.display = showSubs ? '' : 'none'; // 隐藏/显示子命令 chips 容器
  $('edit-subs-label').style.display = showSubs ? '' : 'none'; // 隐藏/显示子命令标题
  if (showSubs) { // 仅键盘动作渲染子命令
    const subSet = new Set(map.subs); // 已勾选的子命令集合（用于判断 active）
    $('edit-subs').innerHTML = SUB_COMMAND_KEYS.map(([code, label]) => // 遍历特殊功能按键生成子命令 chip
      '<div class="chip sub' + (subSet.has(label) ? ' active' : '') + '" ' + // 已勾选的子命令加 active
      'onclick="App.toggleSub(' + code + ')">' + label + '</div>' // 点击切换子命令（数字实参无需引号）
    ).join('');
    $('edit-subs-label').textContent = // 子命令标题，显示当前已选数量
      '子命令（最多3个，当前: ' + (map.subs.length ? map.subs.join(' + ') : '无子命令') + '）';
  }

  renderLongPress(info, map); // 渲染长按动作配置区（需求2）
}

// renderLongPress：渲染「长按动作」配置区（仅在当前按键已有主动作时可用）
function renderLongPress(info, map) {
  const hasMapping = map.has_mapping && editKind !== 'none'; // 无主动作（或已选未设置）时长按无依附对象
  const lp = map.long_press || null; // 已配置的长按动作（ActionView）或 null
  $('edit-long-label').style.display = hasMapping ? '' : 'none'; // 标题显隐
  $('edit-long-kinds').style.display = hasMapping ? '' : 'none'; // 类型 chips 显隐
  $('edit-long-target').style.display = hasMapping ? '' : 'none'; // 目标区显隐
  $('edit-long-subs-label').style.display = 'none'; // 先隐藏，下面按条件显示
  $('edit-long-subs').style.display = 'none';
  $('edit-long-ms-row').style.display = lp ? '' : 'none'; // 阈值行仅在已配置长按时显示
  $('edit-long-clear').style.display = lp ? '' : 'none'; // 取消长按按钮仅在已配置时显示
  if (!hasMapping) return; // 无主动作则整块隐藏

  // 长按动作类型 chips
  $('edit-long-kinds').innerHTML = LONG_PRESS_KINDS.map(([k, label]) => // 遍历可选类型
    '<div class="chip' + (k === editLongKind ? ' active' : '') + '" ' + // 当前类型加 active
    'onclick="App.setLongKind(' + q(k) + ')">' + label + '</div>' // 点击切换长按类型
  ).join('');

  // 长按目标区：未选类型时给出提示
  $('edit-long-target').innerHTML = editLongKind
    ? targetHTML(info, editLongKind, lp || {}, true) // 复用目标选择区，长按样式
    : '<div class="faint">选择长按动作类型后配置目标；未选择则无长按</div>';

  // 长按子命令（仅键盘/滚轮且已配置长按时显示）
  const showLongSubs = lp && (editLongKind === 'keyboard' || editLongKind === 'wheelup' || editLongKind === 'wheeldown');
  $('edit-long-subs-label').style.display = showLongSubs ? '' : 'none';
  $('edit-long-subs').style.display = showLongSubs ? '' : 'none';
  if (showLongSubs) {
    const longSubSet = new Set(lp.subs); // 已勾选的长按子命令
    $('edit-long-subs').innerHTML = SUB_COMMAND_KEYS.map(([code, label]) =>
      '<div class="chip sub' + (longSubSet.has(label) ? ' active' : '') + '" ' +
      'onclick="App.toggleLongSub(' + code + ')">' + label + '</div>'
    ).join('');
    $('edit-long-subs-label').textContent =
      '长按子命令（最多3个，当前: ' + (lp.subs.length ? lp.subs.join(' + ') : '无子命令') + '）';
  }

  currentLongPressMs = map.long_press_ms || 500; // 记录当前阈值，供加减按钮计算
  $('edit-long-ms').textContent = currentLongPressMs + 'ms'; // 显示长按阈值
}

// targetHTML：根据动作类型返回目标选择区 HTML
// kind：动作类型；act：动作视图（含 key_code / mouse_button / layer_name）；isLong：是否为长按动作
function targetHTML(info, kind, act, isLong) {
  const a = act || {}; // 防御：未配置时视为空对象
  const preset = isLong ? 'Long' : ''; // 长按使用独立的处理器前缀（setLongKey / setLongMouse / setLongLayer）
  if (kind === 'none') { // 未设置：无主动作可配置
    return '<div class="faint">未设置：当前按键无映射，选择其他类型后即可配置</div>';
  }
  if (kind === 'keyboard') { // 键盘动作 → 显示按键选择
    return '<div class="dim">选择按键</div><div class="chips-row">' +
      COMMON_KEYS.map(([code, label]) => // 遍历按键生成 chip
        '<div class="chip ' + (code === a.key_code ? ' active' : '') + '" ' + // 当前按键加 active 高亮
        'onclick="App.set' + preset + 'Key(' + code + ')">' + label + '</div>' // 点击设置键盘映射（数字实参）
      ).join('') + '</div>';
  }
  if (kind === 'mouse' || kind === 'mousetoggle') { // 鼠标动作 → 显示鼠标键选择
    return '<div class="dim">选择鼠标键</div><div class="chips-row">' +
      MOUSE_BUTTONS.map(([name, label]) => // 遍历鼠标键生成 chip
        '<div class="chip' + (name.toLowerCase() === (a.mouse_button || '').toLowerCase() ? ' active' : '') + '" ' + // 当前鼠标键加 active 高亮
        'onclick="App.set' + preset + 'Mouse(' + q(name) + ')">' + label + '</div>' // 点击设置鼠标映射（单引号实参）
      ).join('') + '</div>';
  }
  if (kind === 'switchlayer') { // 切层动作 → 显示目标层选择
    return '<div class="dim">选择目标层</div><div class="chips-row">' +
      info.switch_targets.map((t) => // 遍历可切换的目标层生成 chip
        '<div class="chip' + (t.name.toLowerCase() === (a.layer_name || '').toLowerCase() ? ' active' : '') + '" ' + // 当前目标层加 active 高亮
        'onclick="App.set' + preset + 'Layer(' + q(t.name) + ')">' + esc(t.display) + '</div>' // 点击设置切层映射
      ).join('') + '</div>';
  }
  return '<div class="faint">该动作无需额外目标，点击上方类型即生效</div>'; // 无需目标的动作提示
}

// ---------------------------------------------------------------------
// 动作
// ---------------------------------------------------------------------
// App：编辑窗口动作集合，供内联 onclick 调用
const App = {
  selectButton(name) { // 切换选中手柄按键
    selectedButton = name; // 更新选中按键
    invoke('get_mapping', { layerId: editLayerId, button: name }).then((map) => { // 查询新按键的映射
      editKind = map.has_mapping ? map.kind : 'none'; // 有映射则按其类型显示，否则显示未设置
      editLongKind = map.long_press ? map.long_press.kind : null; // 同步长按动作类型
      prevEdit = null; // 强制下一轮重新渲染
    });
  },
  setKind(k) { // 切换动作类型
    editKind = k; // 更新动作类型
    if (k === 'none') { // 未设置：清除该按键的映射（含长按与子命令）
      invoke('clear_mapping', { layerId: editLayerId, button: selectedButton });
    } else if (NO_TARGET_KINDS.includes(k)) { // 若该类型无需额外目标则直接生效
      invoke('set_mapping', { layerId: editLayerId, button: selectedButton, kind: k, keyCode: null, mouseButton: null, layerName: null }); // 直接写入映射
    }
    prevEdit = null; // 强制下一轮重新渲染
  },
  setKey(code) { // 设置键盘映射
    invoke('set_mapping', { layerId: editLayerId, button: selectedButton, kind: 'keyboard', keyCode: code, mouseButton: null, layerName: null }); // 写入键盘键码映射
  },
  setMouse(name) { // 设置鼠标按键映射
    invoke('set_mapping', { layerId: editLayerId, button: selectedButton, kind: editKind, mouseButton: name, keyCode: null, layerName: null }); // 写入鼠标键映射
  },
  setLayer(name) { // 设置切层映射
    invoke('set_mapping', { layerId: editLayerId, button: selectedButton, kind: 'switchlayer', layerName: name, keyCode: null, mouseButton: null }); // 写入目标层映射
  },
  clearMapping() { // 清除当前按键映射
    invoke('clear_mapping', { layerId: editLayerId, button: selectedButton });
  },
  toggleSub(code) { // 切换子命令（追加/移除）
    invoke('toggle_sub', { layerId: editLayerId, button: selectedButton, keyCode: code });
  },
  // ---- 长按动作（需求2） ----
  setLongKind(k) { // 切换长按动作类型
    editLongKind = k; // 更新长按类型
    if (NO_TARGET_KINDS.includes(k)) { // 无需额外目标则直接写入
      invoke('set_long_press', { layerId: editLayerId, button: selectedButton, kind: k, keyCode: null, mouseButton: null, layerName: null });
    }
    prevEdit = null; // 强制重新渲染
  },
  setLongKey(code) { // 设置长按键盘动作
    invoke('set_long_press', { layerId: editLayerId, button: selectedButton, kind: 'keyboard', keyCode: code, mouseButton: null, layerName: null });
  },
  setLongMouse(name) { // 设置长按鼠标动作
    invoke('set_long_press', { layerId: editLayerId, button: selectedButton, kind: editLongKind, mouseButton: name, keyCode: null, layerName: null });
  },
  setLongLayer(name) { // 设置长按切层动作
    invoke('set_long_press', { layerId: editLayerId, button: selectedButton, kind: 'switchlayer', layerName: name, keyCode: null, mouseButton: null });
  },
  toggleLongSub(code) { // 切换长按子命令
    invoke('toggle_long_press_sub', { layerId: editLayerId, button: selectedButton, keyCode: code });
  },
  adjustLongMs(delta) { // 调整长按阈值（步长 50ms，限幅 100~5000）
    const next = Math.min(5000, Math.max(100, currentLongPressMs + delta)); // 计算新阈值
    currentLongPressMs = next; // 记录到前端状态
    $('edit-long-ms').textContent = next + 'ms'; // 立即更新显示
    invoke('set_long_press_ms', { layerId: editLayerId, button: selectedButton, ms: next }); // 通知后端
  },
  clearLongPress() { // 取消长按动作
    editLongKind = null; // 清空前端类型
    invoke('set_long_press', { layerId: editLayerId, button: selectedButton, kind: null, keyCode: null, mouseButton: null, layerName: null });
    prevEdit = null; // 强制重新渲染
  },
  close() { getCurrentWindow().close(); }, // 关闭本窗口
  renameLayer() {
    renameLayerMode = true; // 标记为重命名模式
    renameLayerId = editLayerId; // 记录要重命名的操作层
    renderRenameLayerRow(); // 显示输入行
    $('rename-layer-input').focus(); // 聚焦输入框
  },
  renameLayerOk() {
    invoke('rename_layer', {layerId: renameLayerId, layerName: $('rename-layer-input').value})
    this.renameLayerCancel();
  },
  renameLayerCancel() {
    renameLayerMode = false; // 取消重命名模式
    renameLayerValue = ''; // 清空输入值
    renderRenameLayerRow();
  },
  layerActiveButton(layerId, keyName) {
    if (!keyName) return; // 占位项（"未设置激活按钮"）的 value 为空串，不提交
    // 【坑】Tauri 2 的 #[tauri::command] 默认把 Rust 参数名转成 camelCase 再匹配
    // （tauri-macros 的 argument_case 默认 ArgumentCase::Camel），
    // 因此后端 layer_id / button_name 必须写成 layerId / buttonName；
    // 传 layer_id / button_name 会以「invalid args ... missing required key」失败，
    // 且 invoke 返回的 Promise 被拒绝，不加 catch 时表现为"点了没反应"。
    invoke('change_layer_trigger_button', { layerId: layerId, buttonName: keyName })
      .catch((e) => console.error('change_layer_trigger_button:', e)); // 打印失败原因，避免再次静默无反应
  },
};

// renderRenameLayerRow：根据 renameLayerMode 决定重命名输入行的显示与内容
function renderRenameLayerRow() {
  const rr = $('rename-layer-row'); // 输入行容器
  if (renameLayerMode) { // 处于重命名模式时显示
    rr.style.display = 'flex'; // 显示输入行
    $('rename-layer-hint').textContent = '重命名操作层'; // 提示文字
    const inp = $('rename-layer-input'); // 输入框
    if (document.activeElement !== inp) inp.value = renameLayerValue; // 输入框未被聚焦时才回填，避免覆盖正在输入的内容
  } else {
    rr.classList.add('hide');
    setTimeout(() => {
      rr.style.display = 'none'; // 非编辑模式隐藏输入行
      rr.classList.remove('hide');
    }, 220);
  }
}

// ---------------------------------------------------------------------
// 静态按钮绑定 + 点击波纹
// ---------------------------------------------------------------------
$('edit-rename-layer').addEventListener('click', () => App.renameLayer()); // 重命名层按钮，点击后显示重命名输入行
$('rename-layer-ok').addEventListener('click', () => App.renameLayerOk()); // 确认重命名按钮，点击后提交重命名请求
$('rename-layer-cancel').addEventListener('click', () => App.renameLayerCancel()); // 取消重命名按钮，点击后隐藏输入行
$('edit-close').addEventListener('click', () => App.close()); // 关闭按钮
$('edit-clear').addEventListener('click', () => App.clearMapping()); // 清除映射按钮
$('edit-long-clear').addEventListener('click', () => App.clearLongPress()); // 取消长按动作按钮
$('edit-layer-select').addEventListener('change', (e) => App.layerActiveButton(editLayerId, e.target.value));

document.addEventListener('pointerdown', (e) => { // 监听指针按下（事件委托，适用于动态生成的元素）
  const el = e.target.closest('.btn, .chip, .pad-btn, .mini'); // 命中可点击元素
  if (!el) return; // 未命中则忽略
  const rect = el.getBoundingClientRect(); // 获取元素位置与尺寸
  const d = Math.max(rect.width, rect.height); // 波纹直径取宽高较大值
  const span = document.createElement('span'); // 创建波纹元素
  span.className = 'ripple'; // 应用波纹样式
  span.style.width = d + 'px'; // 波纹宽度
  span.style.height = d + 'px'; // 波纹高度
  span.style.left = (e.clientX - rect.left - d / 2) + 'px'; // 波纹水平位置（以点击点为圆心）
  span.style.top = (e.clientY - rect.top - d / 2) + 'px'; // 波纹垂直位置
  el.appendChild(span); // 挂入波纹节点
  setTimeout(() => span.remove(), 600); // 动画结束后移除
});

// ---------------------------------------------------------------------
// 窗口显示策略：
//   - 带 ?prewarm=1（应用启动时预热创建）：保持隐藏，用户点击后由 main.js 调用 show() 显示
//   - 不带 prewarm（用户点击后才新建）：首帧渲染完成后自动显示，避免打开瞬间白屏闪动
// ---------------------------------------------------------------------
if (!new URLSearchParams(location.search).has('prewarm')) { // 非预热窗口才自动显示
  (async () => {
    try {
      await tick(); // 立即先执行一轮渲染
    } catch (e) {
      console.error('首帧渲染失败:', e); // 渲染出错打印日志
    }
    getCurrentWindow().show(); // 内容就绪后再显示窗口，杜绝白屏
  })();
  // 兜底：若首帧渲染异常导致未显示，1.5 秒后强制显示，避免窗口"打不开"
  setTimeout(() => getCurrentWindow().show(), 1500);
}
