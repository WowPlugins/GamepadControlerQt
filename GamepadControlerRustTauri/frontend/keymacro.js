/* =====================================================================
   keymacro.js —— 独立「物理键盘映射」窗口（WebviewWindow "keymacro-win"）
   左栏规则列表 + 右栏配置：录制组合键、选择动作、子命令、状态条件。
   每 100ms 轮询后端规则快照，内容无变化时不重建 DOM（避免录制时闪烁）。
   ===================================================================== */

// 解构出 Tauri invoke（调用后端 Rust 命令）
const { invoke } = window.__TAURI__.core;
// 解构出 getCurrentWindow（用于关闭 / 显示本窗口）
const { getCurrentWindow } = window.__TAURI__.window;

// ---------------------------------------------------------------------
// 常量（与后端 core/input_types.rs android_key 一致）
// ---------------------------------------------------------------------
// COMMON_KEYS：可映射的键盘键列表，[Android 键码, 显示名]
const COMMON_KEYS = [
  [29, 'A'], [30, 'B'], [31, 'C'], [32, 'D'], [33, 'E'], [34, 'F'], [35, 'G'], [36, 'H'],
  [37, 'I'], [38, 'J'], [39, 'K'], [40, 'L'], [41, 'M'], [42, 'N'], [43, 'O'], [44, 'P'],
  [45, 'Q'], [46, 'R'], [47, 'S'], [48, 'T'], [49, 'U'], [50, 'V'], [51, 'W'], [52, 'X'],
  [53, 'Y'], [54, 'Z'],
  [7, '0'], [8, '1'], [9, '2'], [10, '3'], [11, '4'], [12, '5'], [13, '6'], [14, '7'],
  [15, '8'], [16, '9'],
  [131, 'F1'], [132, 'F2'], [133, 'F3'], [134, 'F4'], [135, 'F5'], [136, 'F6'],
  [137, 'F7'], [138, 'F8'], [139, 'F9'], [140, 'F10'], [141, 'F11'], [142, 'F12'],
  [59, 'Shift'], [113, 'Ctrl'], [57, 'Alt'],
  [62, 'Space'], [66, 'Enter'], [61, 'Tab'], [111, 'Esc'], [67, 'Backspace'],
  [124, 'Insert'], [123, 'Home'], [122, 'End'], [92, 'PageUp'], [93, 'PageDown'],
  [19, '↑'], [20, '↓'], [21, '←'], [22, '→'],
  [68, '`'], [69, '-'], [70, '='], [71, '['], [72, ']'], [73, '\\'], [74, ';'],
  [75, "'"], [55, ','], [56, '.'], [76, '/'],
  [115, 'CapsLock'], [143, 'NumLock'], [116, 'ScrollLock'],
  [144, 'Num0'], [145, 'Num1'], [146, 'Num2'], [147, 'Num3'], [148, 'Num4'],
  [149, 'Num5'], [150, 'Num6'], [151, 'Num7'], [152, 'Num8'], [153, 'Num9'],
];

// SUB_COMMAND_KEYS：子命令可选的特殊功能按键（不含普通字母/数字）
const SUB_COMMAND_KEYS = [
  [59, 'Shift'], [113, 'Ctrl'], [57, 'Alt'],
  [66, 'Enter'], [61, 'Tab'], [111, 'Esc'], [62, 'Space'],
  [19, '↑'], [20, '↓'], [21, '←'], [22, '→'],
  [67, 'Backspace'], [124, 'Insert'], [123, 'Home'], [122, 'End'],
  [92, 'PageUp'], [93, 'PageDown'], [115, 'CapsLock'],
  [131, 'F1'], [132, 'F2'], [133, 'F3'], [134, 'F4'], [135, 'F5'], [136, 'F6'],
  [137, 'F7'], [138, 'F8'], [139, 'F9'], [140, 'F10'], [141, 'F11'], [142, 'F12'],
];

// KINDS：可作为键盘规则动作的类型（不含切层 / 摇杆类动作）
// 每项为 [类型, 显示名, 悬浮说明]
const KINDS = [
  ['none', '未设置', '该规则不执行任何动作（保留组合键但不会拦截按键映射）'],
  ['keyboard', '键盘', '按下触发组合键时发送该按键并保持，松开时释放；可叠加子命令组成新组合键'],
  ['mouse', '鼠标点击', '按下时按下该鼠标键，松开时释放（适合开火、右键等）'],
  ['mousetoggle', '鼠标长按', '按一次按下并锁存该鼠标键，再按一次释放（适合持续引导/按住）'],
  ['wheelup', '滚轮上', '每次按下向上滚动一格；可叠加子命令作为修饰键'],
  ['wheeldown', '滚轮下', '每次按下向下滚动一格；可叠加子命令作为修饰键'],
  ['statemacro', '解锁右键+居中+按键', '先解除右键长按锁存，把光标移到屏幕中央，再按住所选按键/组合键，松开触发键时释放；可配状态条件'],
  ['interactionkey', '交互键', '发送 WoW 的交互键（与目标/物件互动）'],
  ['releasemousetoggle', '释放鼠标锁存', '立即释放当前处于锁存状态的鼠标键'],
  ['swapmousetoggle', '交换左右键锁存', '把锁存中的鼠标键在左键与右键之间交换'],
  ['finelook', '鼠标精细移动', '切换鼠标精细移动模式，降低视角移动速度'],
  ['fastlook', '鼠标快速移动', '切换鼠标快速移动模式，加快视角移动速度'],
  ['toggleoverlayexpand', '悬浮窗展开/收起', '切换悬浮窗的展开/收起状态'],
  ['rightlooklock', '切换右键视角锁定', '按一次：光标移到画面左下角外并按住右键（锁定视角）；再按一次：松开右键、等待游戏退出 mouselook 后把光标移到画面中央；左键锁存中不执行'],
];

// MOUSE_BUTTONS：可映射的鼠标按键，[后端枚举名, 中文显示名]
const MOUSE_BUTTONS = [
  ['LEFT', '鼠标左键'],
  ['RIGHT', '鼠标右键'],
  ['MIDDLE', '鼠标中键'],
  ['FORWARD', '鼠标前进键'],
  ['BACK', '鼠标后退键'],
];

// STATE_OPTIONS：可用的角色状态（与插件 SLState 的 STATE_DEFS 一致）
const STATE_OPTIONS = [
  ['stealthed', '潜行中'], ['chatOpen', '聊天窗打开'], ['inCombat', '战斗中'], ['mounted', '骑乘中'],
  ['hasTarget', '有目标'], ['hasPet', '有宠物'], ['isDead', '死亡'], ['isGhost', '灵魂状态'],
  ['isFalling', '下落中'], ['isSwimming', '游泳中'],
];

// CODE_TO_KEY：KeyboardEvent.code -> [Android 键码, 显示名]，用于录制组合键
const CODE_TO_KEY = {};
(function buildCodeMap() {
  const put = (code, keyCode, label) => { CODE_TO_KEY[code] = [keyCode, label]; };
  for (let i = 0; i < 26; i++) put('Key' + String.fromCharCode(65 + i), 29 + i, String.fromCharCode(65 + i)); // 字母 A-Z
  for (let d = 0; d <= 9; d++) put('Digit' + d, 7 + d, String(d)); // 数字 0-9
  for (let n = 1; n <= 12; n++) put('F' + n, 130 + n, 'F' + n); // 功能键 F1-F12
  put('Space', 62, 'Space'); put('Enter', 66, 'Enter'); put('Tab', 61, 'Tab');
  put('Escape', 111, 'Esc'); put('Backspace', 67, 'Backspace');
  put('Insert', 124, 'Insert'); put('Home', 123, 'Home'); put('End', 122, 'End');
  put('PageUp', 92, 'PageUp'); put('PageDown', 93, 'PageDown');
  put('ArrowUp', 19, '↑'); put('ArrowDown', 20, '↓'); put('ArrowLeft', 21, '←'); put('ArrowRight', 22, '→');
  put('Backquote', 68, '`'); put('Minus', 69, '-'); put('Equal', 70, '=');
  put('BracketLeft', 71, '['); put('BracketRight', 72, ']'); put('Backslash', 73, '\\');
  put('Semicolon', 74, ';'); put('Quote', 75, "'"); put('Comma', 55, ','); put('Period', 56, '.'); put('Slash', 76, '/');
  put('CapsLock', 115, 'CapsLock'); put('NumLock', 143, 'NumLock'); put('ScrollLock', 116, 'ScrollLock');
  for (let n = 0; n <= 9; n++) put('Numpad' + n, 144 + n, 'Num' + n); // 小键盘 0-9
})();

// MODIFIER_CODES：修饰键的 KeyboardEvent.code（录制时只记录状态，不作为主键）
const MODIFIER_CODES = new Set([
  'ControlLeft', 'ControlRight', 'ShiftLeft', 'ShiftRight',
  'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight',
]);

// ---------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------
// esc：HTML 转义，防止特殊字符破坏页面结构
function esc(s) {
  return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// q：生成单引号包裹的 JS 字符串字面量（用于双引号 onclick 属性内的实参）
function q(s) {
  return "'" + String(s).replace(/\\/g, '\\\\').replace(/'/g, "\\'") + "'";
}

// $：按 id 获取 DOM 元素的简写
function $(id) { return document.getElementById(id); }

// ---------------------------------------------------------------------
// 状态
// ---------------------------------------------------------------------
let view = { enabled: true, rules: [] }; // 后端规则快照
let selectedId = null; // 当前选中的规则 id
let recording = false; // 是否正在录制组合键
let recordHint = ''; // 录制过程中的临时提示
let prevKey = ''; // 上一次渲染内容指纹（避免重复重建 DOM）

// effectiveKind：把「键盘动作但键码为 0」视为未设置
function effectiveKind(rule) {
  if (!rule) return 'none';
  return (rule.kind === 'keyboard' && rule.actionKeyCode === 0) ? 'none' : rule.kind;
}

// selectedRule：当前选中的规则对象（不存在返回 null）
function selectedRule() {
  return view.rules.find((r) => r.id === selectedId) || null;
}

// ---------------------------------------------------------------------
// 轮询
// ---------------------------------------------------------------------
let ticking = false; // 轮询重入锁
async function tick() {
  if (ticking) return; // 上一次轮询未结束则跳过
  ticking = true;
  try {
    const data = await invoke('get_keyboard_rules'); // 获取规则列表快照
    // 选中项失效时回落到第一条规则
    if (selectedId === null || !data.rules.some((r) => r.id === selectedId)) {
      selectedId = data.rules.length ? data.rules[0].id : null;
    }
    const key = JSON.stringify(data) + '|' + String(selectedId);
    if (key !== prevKey) { // 有变化才渲染
      prevKey = key;
      view = data;
      if (!recording) render(); // 录制中不重建 DOM，避免打断提示
    }
  } catch (e) {
    console.error('keymacro tick:', e); // 出错打印日志（不中断定时器）
  }
  ticking = false;
}

// ---------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------
function render() {
  const rule = selectedRule();
  hideTip(); // 重建 DOM 前先隐藏悬浮提示（被替换的元素不会再收到 mouseout）

  // 总开关
  const total = $('kb-total');
  total.textContent = '物理键盘映射：' + (view.enabled ? '开' : '关');
  total.className = 'btn' + (view.enabled ? ' active' : '');

  // 规则列表
  $('kb-list').innerHTML = view.rules.map((r) => // 遍历规则生成列表项
    '<div class="layer-btn' + (r.id === selectedId ? ' active' : '') + '" ' +
    'onclick="App.select(' + r.id + ')">' +
    esc(r.comboText) + (r.enabled ? '' : ' · 未启用') + '</div>'
  ).join('') || '<div class="faint">暂无规则，点击上方「添加规则」</div>';

  // 当前规则操作区
  $('kb-current').textContent = rule ? '当前: ' + rule.comboText : '当前: 未选择规则';
  const toggle = $('kb-rule-toggle');
  toggle.textContent = rule && rule.enabled ? '停用规则' : '启用规则';
  toggle.className = 'btn' + (rule && rule.enabled ? ' active' : '');
  toggle.disabled = !rule; // 无规则时不可操作
  $('kb-rule-delete').disabled = !rule;
  $('kb-record').disabled = !rule; // 无规则时不能录制
  $('kb-record').textContent = recording ? '取消录制' : '录制';
  $('kb-record').className = 'btn' + (recording ? ' active' : '');
  $('kb-combo').textContent = rule ? rule.comboText : '未设置';
  $('kb-combo').className = 'kb-combo' + (recording ? ' recording' : '');
  $('kb-record-hint').textContent = recording
    ? (recordHint || '请按下组合键…（Esc 取消）')
    : '点击「录制」后按下组合键；修饰键（Ctrl/Shift/Alt/Win）会自动识别，Esc 取消。';

  if (!rule) { // 无规则时清空右栏
    $('kb-kinds').innerHTML = '';
    $('kb-target').innerHTML = '<div class="faint">请先添加并选择一条规则</div>';
    $('kb-subs').innerHTML = '';
    $('kb-subs-label').style.display = 'none';
    $('kb-states').innerHTML = '';
    $('kb-states-label').textContent = '生效状态条件（不选则始终生效）';
    $('kb-state-logic').innerHTML = '';
    return;
  }

  const kind = effectiveKind(rule);

  // 动作类型 chips（data-tip 提供悬浮说明）
  $('kb-kinds').innerHTML = KINDS.map(([k, label, tip]) =>
    '<div class="chip' + (k === kind ? ' active' : '') + '" ' +
    'data-tip="' + esc(tip) + '" ' +
    'onclick="App.setKind(' + q(k) + ')">' + label + '</div>'
  ).join('');

  // 目标区
  $('kb-target').innerHTML = targetHTML(rule, kind);

  // 子命令（键盘 / 滚轮 / 状态宏支持组合键）
  const showSubs = kind === 'keyboard' || kind === 'wheelup' || kind === 'wheeldown' || kind === 'statemacro';
  $('kb-subs').style.display = showSubs ? '' : 'none';
  $('kb-subs-label').style.display = showSubs ? '' : 'none';
  if (showSubs) {
    const subSet = new Set(rule.subs); // 已勾选的子命令显示名
    $('kb-subs').innerHTML = SUB_COMMAND_KEYS.map(([code, label]) =>
      '<div class="chip sub' + (subSet.has(label) ? ' active' : '') + '" ' +
      'onclick="App.toggleSub(' + code + ')">' + label + '</div>'
    ).join('');
    $('kb-subs-label').textContent =
      '子命令（最多3个，当前: ' + (rule.subs.length ? rule.subs.join(' + ') : '无子命令') + '）';
  }

  renderStates(rule);
}

// targetHTML：根据动作类型返回目标选择区 HTML
function targetHTML(rule, kind) {
  if (kind === 'none') {
    return '<div class="faint">未设置：选择其他类型后即可配置目标</div>';
  }
  if (kind === 'keyboard' || kind === 'statemacro') {
    return '<div class="dim">选择按键</div><div class="chips-row">' +
      COMMON_KEYS.map(([code, label]) =>
        '<div class="chip' + (code === rule.actionKeyCode ? ' active' : '') + '" ' +
        'onclick="App.setKey(' + code + ')">' + label + '</div>'
      ).join('') + '</div>';
  }
  if (kind === 'mouse' || kind === 'mousetoggle') {
    return '<div class="dim">选择鼠标键</div><div class="chips-row">' +
      MOUSE_BUTTONS.map(([name, label]) =>
        '<div class="chip' + (name.toLowerCase() === (rule.mouseButton || '').toLowerCase() ? ' active' : '') + '" ' +
        'onclick="App.setMouse(' + q(name) + ')">' + label + '</div>'
      ).join('') + '</div>';
  }
  return '<div class="faint">该动作无需额外目标，点击上方类型即生效</div>';
}

// renderStates：渲染状态条件区（未选 -> 为真 -> 为假 -> 未选）
function renderStates(rule) {
  const conds = rule.stateConditions || [];
  const byId = {};
  conds.forEach((c) => { byId[c.stateId] = c.expected; });
  $('kb-states').innerHTML = STATE_OPTIONS.map(([id, label]) => {
    let cls = 'chip';
    let suffix = '';
    if (byId[id] === true) { cls += ' active'; suffix = ' = 真'; }
    else if (byId[id] === false) { cls += ' off'; suffix = ' = 假'; }
    return '<div class="' + cls + '" onclick="App.cycleState(' + q(id) + ')">' + label + suffix + '</div>';
  }).join('');
  $('kb-state-logic').innerHTML = [['or', '逻辑或（任一满足）'], ['and', '逻辑与（全部满足）']].map(([k, label]) =>
    '<div class="chip' + (k === (rule.stateLogic || 'or') ? ' active' : '') + '" onclick="App.setLogic(' + q(k) + ')">' + label + '</div>'
  ).join('');
  $('kb-state-logic').style.display = conds.length > 1 ? '' : 'none'; // 多条条件才需要组合方式
  $('kb-states-label').textContent = conds.length
    ? '生效状态条件（当前 ' + conds.length + ' 条）'
    : '生效状态条件（不选则始终生效）';
}

// ---------------------------------------------------------------------
// 动作
// ---------------------------------------------------------------------
const App = {
  select(id) { // 切换选中规则
    selectedId = id;
    prevKey = ''; // 强制下一轮重新渲染
  },
  addRule() { // 新增一条规则并选中
    invoke('add_keyboard_rule').then((id) => {
      selectedId = id;
      prevKey = '';
      tick(); // 立即刷新
    }).catch((e) => console.error('add_keyboard_rule:', e));
  },
  deleteRule() { // 删除当前规则
    if (selectedId === null) return;
    const id = selectedId;
    selectedId = null; // 清空选中，轮询会自动回落到第一条
    prevKey = '';
    invoke('delete_keyboard_rule', { id }).catch((e) => console.error('delete_keyboard_rule:', e));
  },
  toggleRule() { // 启用/停用当前规则
    const rule = selectedRule();
    if (!rule) return;
    invoke('set_keyboard_rule_enabled', { id: rule.id, enabled: !rule.enabled })
      .catch((e) => console.error('set_keyboard_rule_enabled:', e));
  },
  toggleTotal() { // 切换物理键盘映射总开关
    invoke('set_keyboard_rules_enabled', { enabled: !view.enabled })
      .catch((e) => console.error('set_keyboard_rules_enabled:', e));
  },
  startRecord() { // 开始 / 取消录制组合键
    if (selectedId === null) return;
    recording = !recording;
    recordHint = '';
    render();
  },
  setKind(k) { // 切换动作类型
    const rule = selectedRule();
    if (!rule) return;
    // 需要二次选择目标的类型先用「沿用当前值 / 默认值」落库，
    // 否则只改了前端高亮，切到别的规则再回来会回退成旧类型
    let keyCode = null; // 键盘类要发送的键码
    let mouseButton = null; // 鼠标类要按下的键
    if (k === 'keyboard' || k === 'statemacro') keyCode = rule.actionKeyCode || 62; // 键盘/状态宏：沿用当前键码，无则空格
    else if (k === 'mouse' || k === 'mousetoggle') mouseButton = rule.mouseButton || 'LEFT'; // 鼠标类：沿用当前鼠标键，无则左键
    invoke('set_keyboard_rule_action', { id: rule.id, kind: k, keyCode, mouseButton })
      .catch((e) => console.error('set_keyboard_rule_action:', e));
  },
  setKey(code) { // 设置动作目标键
    const rule = selectedRule();
    if (!rule) return;
    invoke('set_keyboard_rule_action', { id: rule.id, kind: effectiveKind(rule), keyCode: code, mouseButton: null })
      .catch((e) => console.error('set_keyboard_rule_action:', e));
  },
  setMouse(name) { // 设置动作目标鼠标键
    const rule = selectedRule();
    if (!rule) return;
    invoke('set_keyboard_rule_action', { id: rule.id, kind: effectiveKind(rule), keyCode: null, mouseButton: name })
      .catch((e) => console.error('set_keyboard_rule_action:', e));
  },
  toggleSub(code) { // 切换子命令（追加/移除）
    const rule = selectedRule();
    if (!rule) return;
    invoke('toggle_keyboard_rule_sub', { id: rule.id, keyCode: code })
      .catch((e) => console.error('toggle_keyboard_rule_sub:', e));
  },
  cycleState(stateId) { // 切换某状态条件：未选 -> 为真 -> 为假 -> 未选
    const rule = selectedRule();
    if (!rule) return;
    const conds = (rule.stateConditions || []).map((c) => ({ stateId: c.stateId, expected: c.expected }));
    const idx = conds.findIndex((c) => c.stateId === stateId);
    if (idx < 0) conds.push({ stateId: stateId, expected: true });
    else if (conds[idx].expected === true) conds[idx].expected = false;
    else conds.splice(idx, 1);
    invoke('set_keyboard_rule_state_conditions', { id: rule.id, conditions: conds, logic: rule.stateLogic || 'or' })
      .catch((e) => console.error('set_keyboard_rule_state_conditions:', e));
  },
  setLogic(logic) { // 切换条件组合方式
    const rule = selectedRule();
    if (!rule) return;
    const conds = (rule.stateConditions || []).map((c) => ({ stateId: c.stateId, expected: c.expected }));
    invoke('set_keyboard_rule_state_conditions', { id: rule.id, conditions: conds, logic })
      .catch((e) => console.error('set_keyboard_rule_state_conditions:', e));
  },
  close() { getCurrentWindow().close(); }, // 关闭本窗口
};

// ---------------------------------------------------------------------
// 组合键录制（window keydown 捕获阶段）
// ---------------------------------------------------------------------
function modsText(e) {
  const parts = [];
  if (e.ctrlKey) parts.push('Ctrl');
  if (e.shiftKey) parts.push('Shift');
  if (e.altKey) parts.push('Alt');
  if (e.metaKey) parts.push('Win');
  return parts;
}

window.addEventListener('keydown', (e) => {
  if (!recording) return; // 非录制状态不拦截
  e.preventDefault(); // 阻止默认行为（避免触发页面快捷键）
  e.stopPropagation(); // 阻止冒泡到页面其它监听
  if (e.repeat) return; // 长按连发不重复处理

  if (e.code === 'Escape') { // Esc 取消录制
    recording = false;
    recordHint = '';
    render();
    return;
  }
  if (MODIFIER_CODES.has(e.code)) { // 只按了修饰键：提示继续按主键
    recordHint = '已按 ' + modsText(e).concat(['…']).join(' + ');
    $('kb-record-hint').textContent = recordHint;
    return;
  }
  const entry = CODE_TO_KEY[e.code]; // 查询主键对应的 Android 键码
  if (!entry) {
    recordHint = '该按键暂不支持，请换一个';
    $('kb-record-hint').textContent = recordHint;
    return;
  }
  const rule = selectedRule();
  if (!rule) { // 理论上录制时必有选中规则
    recording = false;
    render();
    return;
  }
  invoke('set_keyboard_rule_combo', {
    id: rule.id,
    ctrl: e.ctrlKey,
    shift: e.shiftKey,
    alt: e.altKey,
    win: e.metaKey,
    keyCode: entry[0],
  }).catch((err) => console.error('set_keyboard_rule_combo:', err));
  recording = false;
  recordHint = '';
  prevKey = ''; // 强制下一轮刷新显示新组合键
}, true);

// ---------------------------------------------------------------------
// 悬浮提示：悬停带 data-tip 的元素时显示自绘气泡（fixed 定位，不被滚动容器裁剪）
// ---------------------------------------------------------------------
const tipEl = $('kb-tip'); // 提示气泡容器
let tipTarget = null; // 当前正在显示提示的元素

function hideTip() { // 隐藏提示气泡
  if (tipEl.style.display !== 'none') tipEl.style.display = 'none';
  tipTarget = null;
}

function showTip(el) { // 显示提示：优先在元素上方，空间不足时显示在下方
  tipTarget = el;
  tipEl.textContent = el.getAttribute('data-tip');
  tipEl.style.display = 'block'; // 先显示才能测量实际尺寸
  const r = el.getBoundingClientRect(); // 目标元素位置
  const t = tipEl.getBoundingClientRect(); // 气泡尺寸
  let left = r.left + r.width / 2 - t.width / 2; // 水平居中对齐
  left = Math.max(8, Math.min(left, window.innerWidth - t.width - 8)); // 收敛到视口内
  let top = r.top - t.height - 8; // 优先放在上方
  if (top < 8) top = r.bottom + 8; // 上方空间不足则放到下方
  tipEl.style.left = left + 'px';
  tipEl.style.top = top + 'px';
}

document.addEventListener('mouseover', (e) => { // 事件委托：进入带 data-tip 的元素时显示
  const el = e.target.closest && e.target.closest('[data-tip]');
  if (!el || el === tipTarget) return;
  showTip(el);
});
document.addEventListener('mouseout', (e) => { // 离开元素时隐藏
  const el = e.target.closest && e.target.closest('[data-tip]');
  if (el && el === tipTarget) hideTip();
});
window.addEventListener('blur', hideTip); // 窗口失焦时隐藏
document.addEventListener('scroll', hideTip, true); // 任意容器滚动时隐藏（避免气泡错位）

// ---------------------------------------------------------------------
// 静态按钮绑定 + 点击波纹
// ---------------------------------------------------------------------
$('kb-add').addEventListener('click', () => App.addRule());
$('kb-rule-delete').addEventListener('click', () => App.deleteRule());
$('kb-rule-toggle').addEventListener('click', () => App.toggleRule());
$('kb-total').addEventListener('click', () => App.toggleTotal());
$('kb-record').addEventListener('click', () => App.startRecord());
$('kb-close').addEventListener('click', () => App.close());

document.addEventListener('pointerdown', (e) => { // 点击波纹（事件委托）
  const el = e.target.closest('.btn, .chip, .layer-btn');
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const d = Math.max(rect.width, rect.height);
  const span = document.createElement('span');
  span.className = 'ripple';
  span.style.width = d + 'px';
  span.style.height = d + 'px';
  span.style.left = (e.clientX - rect.left - d / 2) + 'px';
  span.style.top = (e.clientY - rect.top - d / 2) + 'px';
  el.appendChild(span);
  setTimeout(() => span.remove(), 600);
});

// ---------------------------------------------------------------------
// 窗口显示策略（与 edit.js 一致）：
//   - 带 ?prewarm=1 的预热窗口：保持隐藏，主窗口点击时 show()
//   - 用户点击后新建：首帧渲染完成再显示，避免白屏闪动
// ---------------------------------------------------------------------
setInterval(tick, 100); // 每 100ms 轮询一次规则快照

if (!new URLSearchParams(location.search).has('prewarm')) { // 非预热窗口才自动显示
  (async () => {
    try {
      await tick(); // 立即先执行一轮渲染
    } catch (e) {
      console.error('首帧渲染失败:', e);
    }
    getCurrentWindow().show(); // 内容就绪后再显示
  })();
  setTimeout(() => getCurrentWindow().show(), 1500); // 兜底显示
} else {
  tick(); // 预热窗口只需先加载数据
}
