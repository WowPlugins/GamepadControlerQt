/* =====================================================================
   log.js —— 独立「调试日志」窗口（WebviewWindow "log-win"）
   打开时先读取后端日志文件（debug.log）展示已有内容，
   之后每 250ms 增量拉取内存日志缓冲，追加新产生的日志。
   ===================================================================== */

// 解构出 Tauri invoke（调用后端 Rust 命令）
const { invoke } = window.__TAURI__.core;
// 解构出 getCurrentWindow（用于关闭 / 显示本窗口）
const { getCurrentWindow } = window.__TAURI__.window;

let lastSeq = 0; // 已拉取到的最新日志序号
let autoScroll = true; // 是否自动滚动到底部
let ready = false; // 日志文件加载完成前跳过增量轮询
const MAX_DOM_LINES = 3000; // 前端最多保留的日志行数（与后端缓冲一致）

// $：按 id 获取 DOM 元素的简写
function $(id) { return document.getElementById(id); }

// classify：按日志文字粗略分类，用于颜色区分
function classify(text) {
  if (/错误|失败|error/i.test(text)) return 'error'; // 错误类
  if (/警告|提示|warn/i.test(text)) return 'warn'; // 警告/提示类
  return 'info'; // 普通信息
}

// updateEmpty：根据是否存在日志行切换空状态提示
function updateEmpty() {
  const empty = $('log-empty');
  if (!empty) return; // 空状态占位不存在（已有日志行）时无需处理
  const hasLines = $('log-view').querySelector('.log-line') !== null;
  empty.style.display = hasLines ? 'none' : 'block';
}

// appendEntries：把新增日志追加到滚动区
function appendEntries(entries) {
  if (!entries.length) return;
  const view = $('log-view');
  // 追加前判断是否已在底部，避免用户向上翻看时被强制拉回
  const atBottom = view.scrollHeight - view.scrollTop - view.clientHeight < 24;
  const frag = document.createDocumentFragment();
  for (const e of entries) {
    const line = document.createElement('div');
    line.className = 'log-line ' + classify(e.text);
    const time = document.createElement('span');
    time.className = 'log-time';
    time.textContent = e.time; // 本地时间
    const text = document.createElement('span');
    text.className = 'log-text';
    text.textContent = e.text; // textContent 避免日志内容被当作 HTML
    line.appendChild(time);
    line.appendChild(text);
    frag.appendChild(line);
  }
  view.appendChild(frag);
  // 超出上限时移除最旧的行，避免长时间运行后 DOM 无限增长
  const lines = view.querySelectorAll('.log-line');
  if (lines.length > MAX_DOM_LINES) {
    for (let i = 0; i < lines.length - MAX_DOM_LINES; i++) lines[i].remove();
  }
  updateEmpty();
  if (autoScroll || atBottom) view.scrollTop = view.scrollHeight; // 自动滚到底部
}

// renderStatus：刷新开关状态、条数与提示
function renderStatus(page) {
  const status = $('log-status');
  status.textContent = page.enabled ? '已开启' : '已关闭';
  status.className = 'chip log-status' + (page.enabled ? ' active' : ' off');
  $('log-count').textContent = page.total + ' 条' + (page.dropped ? '（已丢弃 ' + page.dropped + ' 条）' : '');
  $('log-tip').textContent = page.enabled ? '实时追加中' : '日志已关闭，仍可查看历史记录';
}

// loadFromFile：窗口打开时直接展示日志文件内容（程序启动时已清空旧日志）
async function loadFromFile() {
  const lines = await invoke('read_log_file'); // 后端读取 debug.log 的最后若干行
  const view = $('log-view');
  const empty = $('log-empty'); // 保留空状态占位（有日志时由 updateEmpty 隐藏）
  view.innerHTML = ''; // 丢掉旧内容
  if (empty) view.appendChild(empty);
  const frag = document.createDocumentFragment();
  for (const raw of lines) {
    const line = document.createElement('div');
    line.className = 'log-line ' + classify(raw);
    const m = /^(\d{2}:\d{2}:\d{2}\.\d{3})\s{2}(.*)$/.exec(raw); // 拆分「时间  正文」
    const time = document.createElement('span');
    time.className = 'log-time';
    time.textContent = m ? m[1] : '';
    const text = document.createElement('span');
    text.className = 'log-text';
    text.textContent = m ? m[2] : raw; // textContent 避免日志内容被当作 HTML
    line.appendChild(time);
    line.appendChild(text);
    frag.appendChild(line);
  }
  view.appendChild(frag);
  updateEmpty();
  view.scrollTop = view.scrollHeight; // 打开时定位到最新一条
  // 文件内容即已展示部分：把序号对齐到内存缓冲末尾，后续 tick 只追加新日志
  const page = await invoke('get_logs', { after: 0 });
  lastSeq = page.next;
  renderStatus(page);
}

// tick：增量拉取日志
async function tick() {
  if (!ready) return; // 文件尚未加载完成
  try {
    const page = await invoke('get_logs', { after: lastSeq }); // 只取新日志
    lastSeq = page.next; // 记录最新序号
    appendEntries(page.entries || []); // 追加渲染
    renderStatus(page); // 刷新状态栏
  } catch (e) {
    console.error('get_logs:', e); // 拉取失败不中断定时器
  }
}

// copyAll：复制当前窗口内全部日志到系统剪贴板
async function copyAll() {
  const lines = Array.from($('log-view').querySelectorAll('.log-line')).map((line) => {
    const time = line.querySelector('.log-time')?.textContent || ''; // 时间列
    const text = line.querySelector('.log-text')?.textContent || ''; // 日志正文
    return time + '  ' + text; // 用两个空格分隔时间与正文
  });
  if (!lines.length) {
    flashCopied('暂无日志');
    return;
  }
  const content = lines.join('\n');
  let ok = false;
  try {
    await navigator.clipboard.writeText(content); // 首选：异步剪贴板 API
    ok = true;
  } catch (e) {
    // 回退：WebView 不支持剪贴板 API 时用隐藏 textarea + execCommand
    const ta = document.createElement('textarea');
    ta.value = content;
    ta.style.position = 'fixed';
    ta.style.left = '-9999px';
    document.body.appendChild(ta);
    ta.select();
    try {
      // 使用现代 Clipboard API 替换已弃用的 document.execCommand
      const selectedText = window.getSelection().toString();
      ok = selectedText ? await navigator.clipboard.writeText(selectedText).then(() => true).catch(() => false) : false;
    } catch (err) {
      ok = false;
    }
    ta.remove();
  }
  flashCopied(ok ? '已拷贝' : '拷贝失败');
}

// flashCopied：按钮临时显示操作结果，1.2 秒后恢复
function flashCopied(msg) {
  const btn = $('log-copy');
  const original = '拷贝全部';
  btn.textContent = msg;
  clearTimeout(flashCopied.timer);
  flashCopied.timer = setTimeout(() => { btn.textContent = original; }, 1200);
}

// clearLogs：清空前端与后端日志
function clearLogs() {
  invoke('clear_logs')
    .then(() => {
      $('log-view').innerHTML = '<div id="log-empty" class="log-empty faint">暂无日志，开启「调试日志」后这里会实时显示</div>';
      $('log-count').textContent = '0 条';
      updateEmpty();
    })
    .catch((e) => console.error('clear_logs:', e));
}

// closeWindow：关闭本窗口
function closeWindow() {
  getCurrentWindow().close().catch((e) => console.error('关闭日志窗口失败:', e)); // 失败时打印原因，避免按钮看起来“没反应”
}

// ---- 事件绑定 ----
$('log-copy').addEventListener('click', copyAll);
$('log-clear').addEventListener('click', clearLogs);
$('log-close').addEventListener('click', closeWindow);
$('log-auto-scroll').addEventListener('change', () => {
  autoScroll = $('log-auto-scroll').checked;
  if (autoScroll) $('log-view').scrollTop = $('log-view').scrollHeight;
});

// ---- 窗口显示策略 ----
setInterval(tick, 250); // 每 250ms 增量拉取一次

(async () => {
  try {
    await loadFromFile(); // 打开窗口先展示日志文件已有内容
  } catch (e) {
    console.error('读取日志文件失败:', e); // 读取失败不影响后续实时日志
  }
  ready = true; // 允许增量轮询
  await tick(); // 补上加载文件期间新增的日志
  getCurrentWindow().show(); // 内容就绪后再显示
})();
setTimeout(() => getCurrentWindow().show(), 1500); // 兜底显示
