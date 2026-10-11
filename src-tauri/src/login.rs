//! 「外置登录窗口」授权：DeepSeek 开放平台。
//!
//! 为什么不直接调登录接口：登录可能触发短信验证码、滑块、GitHub / LinuxDo
//! 第三方登录等人机验证，服务端接口无法完成。这里开一个真实的 WebView 窗口
//! 让用户自己登录，我们只负责「感知登录成功」并把凭证取回。
//!
//! 两条回传通道：
//! 1. **IPC（主通道）**：注入脚本调用 `deepseek_login_token`，权限在
//!    `capabilities/login.json` 里收窄到 deeplogin 这一个窗口、
//!    platform.deepseek.com 这一个域名。
//! 2. **标题（降级通道）**：脚本把凭证写进 `document.title`，由
//!    `on_document_title_changed` 回调接收 —— 不需要任何 IPC 权限。
//!
//! 取到候选后**必须先验证**：用候选 Token 请求一次平台用量接口，HTTP 200 才采纳。

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::http::Http;

const DEEPSEEK_WINDOW: &str = "deeplogin";
const MAIN_WINDOW: &str = "main";
const DEEPSEEK_LOGIN_URL: &str = "https://platform.deepseek.com/usage";
const TITLE_PREFIX: &str = "TLTOKEN|";
/// 登录进度事件：发给主窗口，避免表单停在「等待登录」上没有任何反馈
const PROGRESS_EVENT: &str = "login-progress";
/// 注入脚本的存活心跳前缀：用来区分「页面里没有凭证」和「脚本 / 回传通道没生效」
const PING_PREFIX: &str = "TLPING|";

/// 单次登录最长等待时间
const TIMEOUT: Duration = Duration::from_secs(10 * 60);
const POLL: Duration = Duration::from_millis(400);

// ---------------------------------------------------------------------------
// 待取走的凭证
// ---------------------------------------------------------------------------

/// 候选凭证必须**全部收下**：注入脚本对每个值只会回传一次，
/// 若像旧实现那样只保留第一个，真正的令牌可能被排在前面的随机串挤掉，
/// 之后再也不会被回传 —— 表现就是「登录成功了却一直等下去」。
struct Pending {
    queue: Mutex<VecDeque<String>>,
    seen: Mutex<HashSet<String>>,
}

fn slots() -> &'static Mutex<HashMap<String, Arc<Pending>>> {
    static S: OnceLock<Mutex<HashMap<String, Arc<Pending>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn slot(label: &str) -> Arc<Pending> {
    let mut map = match slots().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    map.entry(label.to_string())
        .or_insert_with(|| {
            Arc::new(Pending {
                queue: Mutex::new(VecDeque::new()),
                seen: Mutex::new(HashSet::new()),
            })
        })
        .clone()
}

/// 清空上一次登录的残留
fn reset(label: &str) {
    let pending = slot(label);
    {
        let mut q = match pending.queue.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        q.clear();
    }
    let mut s = match pending.seen.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    s.clear();
}

fn submit(label: &str, token: String) {
    let pending = slot(label);
    let mut seen = match pending.seen.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if !seen.insert(token.clone()) {
        return;
    }
    drop(seen);

    let mut queue = match pending.queue.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    queue.push_back(token);
}

/// 一次性取走当前排队的候选凭证
fn drain(label: &str) -> Vec<String> {
    slot(label)
        .queue
        .lock()
        .ok()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 注入脚本
// ---------------------------------------------------------------------------

/// `__LOGIN_CMD__` 占位符在运行时替换成回传命令名：
/// 优先走命令回传，命令不可用（没放开 IPC 权限）时退回标题回传。
const INIT_SCRIPT: &str = r#"
(function () {
  var CMD = '__LOGIN_CMD__';
  var PREFIX = 'TLTOKEN|';
  var sent = {};
  // 标题是降级回传通道，但站点（SPA）在路由切换时会重写 document.title，
  // 把刚写进去的凭证顶掉；这里记住第一个候选值，发现标题被改回去就补写一次。
  var last = null;
  var win = window;
  // document.title changes can be coalesced by WebKit when several values are
  // written in the same turn. Serialize every signal so the Rust title callback
  // cannot miss the actual credential.
  var signalQueue = [];
  function queueSignal(value) {
    if (!value) return;
    signalQueue.push(String(value));
  }
  setInterval(function () {
    var value = signalQueue.shift();
    if (!value) return;
    try { document.title = value; } catch (e) {}
  }, 120);

  // fromHeader：值直接来自 Authorization 请求头，是站点自己在用的真实凭证，
  // 不做字符集猜测，只排空白与控制符。
  function isCandidate(v, fromHeader) {
    if (typeof v !== 'string') return false;
    if (v.length < 12 || v.length > 2048) return false;
    if (/[\s"'\\]/.test(v)) return false;
    if (fromHeader) return true;
    if (!/^[A-Za-z0-9_\-\.\|:\+\/=~]+$/.test(v)) return false;
    // 站点令牌（sk-…）与 JWT（eyJ…）直接收；其它随机串要求足够长
    return /^(sk-|eyJ)/.test(v) || v.length >= 20;
  }

  function push(v, fromHeader) {
    if (!v) return;
    v = String(v).replace(/^Bearer\s+/i, '').trim();
    if (!isCandidate(v, fromHeader) || sent[v]) return;
    sent[v] = 1;
    if (!last) last = v;

    function fallback() {
      queueSignal(PREFIX + v);
    }
    // 主通道：IPC 命令（需要为本站放开 capability）
    try {
      var api = win.__TAURI_INTERNALS__ || win.__TAURI__;
      if (api && api.invoke) {
        var timer = setTimeout(fallback, 800);
        var r = api.invoke(CMD, { token: v });
        if (r && typeof r.then === 'function') {
          r.then(function () { clearTimeout(timer); },
                 function () { clearTimeout(timer); fallback(); });
        } else {
          clearTimeout(timer);
        }
        return;
      }
    } catch (e) {}
    fallback();
  }

  // Headers 实例 / 普通对象 / [[k,v]] 数组都能取到值
  function readHeader(h, name) {
    if (!h) return null;
    try {
      if (typeof h.get === 'function') return h.get(name);
      if (Array.isArray(h)) {
        for (var i = 0; i < h.length; i++) {
          if (String(h[i][0]).toLowerCase() === name) return h[i][1];
        }
        return null;
      }
      for (var k in h) {
        if (Object.prototype.hasOwnProperty.call(h, k) && String(k).toLowerCase() === name) return h[k];
      }
    } catch (e) {}
    return null;
  }

  // 站点自己在发的请求头是最好的凭证来源：Authorization 就是会话令牌。
  function harvest(h) {
    if (!h) return;
    try {
      push(readHeader(h, 'authorization'), true);
    } catch (e) {}
  }

  var origFetch = win.fetch;
  win.fetch = function (input, init) {
    try {
      harvest((init && init.headers) || (input && input.headers));
    } catch (e) {}
    return origFetch.apply(this, arguments);
  };

  var origSetHeader = XMLHttpRequest.prototype.setRequestHeader;
  XMLHttpRequest.prototype.setRequestHeader = function (name, value) {
    try {
      if (String(name).toLowerCase() === 'authorization') push(value, true);
    } catch (e) {}
    return origSetHeader.apply(this, arguments);
  };

  // 已登录时页面把令牌放在 localStorage / sessionStorage 里，直接读即可，
  // 用户甚至不需要做任何操作。
  function walk(o, depth) {
    if (!o || depth > 5) return;
    if (typeof o === 'string') { if (isCandidate(o)) push(o); return; }
    if (Array.isArray(o)) {
      for (var i = 0; i < o.length && i < 50; i++) walk(o[i], depth + 1);
      return;
    }
    if (typeof o !== 'object') return;
    for (var k in o) {
      if (!Object.prototype.hasOwnProperty.call(o, k)) continue;
      var val = o[k];
      if (typeof val === 'string') {
        if (/token|jwt|auth|key|session|secret/i.test(k) || val.length >= 32) push(val);
      } else {
        walk(val, depth + 1);
      }
    }
  }

  function scan(store) {
    try {
      for (var i = 0; i < store.length; i++) {
        var raw = store.getItem(store.key(i));
        if (!raw || raw.length > 8192) continue;
        var c = raw.charAt(0);
        if (c === '{' || c === '[') {
          try { walk(JSON.parse(raw), 0); } catch (e) {}
        } else {
          push(raw);
        }
      }
    } catch (e) {}
  }

  function forceScan() {
    scan(win.localStorage);
    scan(win.sessionStorage);
  }
  setInterval(forceScan, 800);
  forceScan();

  // 脚本存活心跳：让 App 能区分「页面里确实没有凭证」和「脚本 / 回传通道没生效」
  function ping() {
    if (last) return;
    queueSignal('TLPING|1');
  }
  ping();
  setInterval(ping, 5000);

  // 站点页面把标题改回去时补写一次，让后端仍能通过标题通道拿到候选凭证
  setInterval(function () {
    if (!last) return;
    try {
      if (String(win.document.title || '').indexOf(PREFIX) !== 0) {
        queueSignal(PREFIX + last);
      }
    } catch (e) {}
  }, 2000);
})();
"#;

fn init_script(cmd: &str) -> String {
    INIT_SCRIPT.replace("__LOGIN_CMD__", cmd)
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/// 注入脚本回传候选凭证（由 deeplogin 窗口调用，权限已在 capability 里收窄）。
#[tauri::command]
pub fn deepseek_login_token(token: String) -> Result<(), String> {
    let t = token.trim().to_string();
    if t.is_empty() {
        return Err("token 为空".to_string());
    }
    submit(DEEPSEEK_WINDOW, t);
    Ok(())
}

/// 打开 DeepSeek 外置登录窗口，等待用户完成登录并取回平台用量 Token。
#[tauri::command]
pub async fn start_deepseek_login(app: AppHandle) -> Result<String, String> {
    start_login(app).await
}

async fn start_login(app: AppHandle) -> Result<String, String> {
    let label = DEEPSEEK_WINDOW.to_string();

    // 复位，避免上一次的残留
    reset(&label);

    if let Some(old) = app.get_webview_window(&label) {
        let _ = old.close();
    }

    let url: tauri::Url = DEEPSEEK_LOGIN_URL
        .parse()
        .map_err(|_| format!("登录地址非法: {DEEPSEEK_LOGIN_URL}"))?;

    let script = init_script("deepseek_login_token");
    let cb_label = label.clone();
    let cb_app = app.clone();
    let window = WebviewWindowBuilder::new(&app, label.clone(), WebviewUrl::External(url))
        .title("登录 DeepSeek 开放平台")
        .inner_size(1100.0, 800.0)
        .center()
        .resizable(true)
        .initialization_script(script)
        .on_document_title_changed(move |_window, title| {
            let t = title.trim();
            if let Some(tok) = parse_title_token(t) {
                submit(&cb_label, tok);
            } else if t.starts_with(PING_PREFIX) {
                // 脚本心跳：能收到说明注入与标题回传通道都是通的
                emit_progress(&cb_app, "登录窗口已就绪（取凭证脚本已注入），等待平台登录…");
            }
        })
        .build()
        .map_err(|e| format!("打开登录窗口失败: {e}"))?;
    window
        .show()
        .map_err(|e| format!("显示登录窗口失败: {e}"))?;

    let result = wait_for_token(&app).await;

    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.close();
    }
    result
}

fn parse_title_token(title: &str) -> Option<String> {
    title
        .trim()
        .strip_prefix(TITLE_PREFIX)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 越像凭证的越先验证：站点里往往同时存在多个候选，先验证最像的那个可以立刻拿到结果，
/// 而不是先在几十个无关字符串上各花一次请求。
///
/// 要的是**网页会话凭证**，因此 JWT（`eyJ…`）排最前；
/// `sk-…` 是 API Key，会话类接口不认它，排到最后（仍会兜底试一次）。
fn token_priority(token: &str) -> u8 {
    if token.starts_with("eyJ") {
        0
    } else if token.starts_with("sk-") {
        2
    } else {
        1
    }
}

fn emit_progress(app: &AppHandle, msg: &str) {
    if let Some(win) = app.get_webview_window(MAIN_WINDOW) {
        let _ = win.emit(PROGRESS_EVENT, msg.to_string());
    }
}

/// 结束等待时的错误说明：把「捕获到了但都没通过校验」说清楚 ——
/// 只看这一句就能判断是「页面里没有凭证」还是「校验被拒（含原因）」。
fn give_up(reason: &str, captured: usize, last_error: Option<String>) -> String {
    if captured == 0 {
        return format!("{reason}，未捕获到任何凭证，请重试");
    }
    match last_error {
        Some(e) => format!("{reason}：已捕获 {captured} 个候选凭证，但都没通过校验（{e}）"),
        None => format!("{reason}：已捕获 {captured} 个候选凭证，但都没通过校验"),
    }
}

async fn wait_for_token(app: &AppHandle) -> Result<String, String> {
    let http = Http::new()?;
    let label = DEEPSEEK_WINDOW;
    let started = Instant::now();
    let mut tried: HashSet<String> = HashSet::new();
    let mut pending: Vec<String> = Vec::new();
    let mut last_error: Option<String> = None;
    let mut captured: usize = 0;
    let mut last_progress = String::new();
    let mut last_beat: u64 = 0;

    loop {
        // 1) 收下所有候选（注入脚本对每个值只回传一次，不能丢），再按可信度排序
        let incoming = drain(label);
        if !incoming.is_empty() {
            captured += incoming.len();
            pending.extend(incoming);
            pending.sort_by_key(|t| token_priority(t));
        }

        // 2) 逐个验证，越像令牌的越先试
        if let Some(candidate) = pending.first().cloned() {
            pending.remove(0);
            if tried.insert(candidate.clone()) {
                let msg = format!("已捕获 {captured} 个候选凭证，正在验证第 {} 个…", tried.len());
                if msg != last_progress {
                    last_progress = msg.clone();
                    emit_progress(app, &msg);
                }
                match crate::providers::deepseek::verify_platform_token(&http, &candidate).await {
                    Ok(_) => {
                        emit_progress(app, "验证通过，正在回填…");
                        return Ok(candidate);
                    }
                    Err(e) => {
                        // 不是可用的凭证（可能是别的短期令牌），继续验证下一个；
                        // 原因要显示出来，否则界面只会停在「等待登录」上
                        let msg = format!("候选凭证未通过校验（{}），继续等待…", e);
                        if msg != last_progress {
                            last_progress = msg.clone();
                            emit_progress(app, &msg);
                        }
                        last_error = Some(e);
                    }
                }
            }
            continue;
        }

        if app.get_webview_window(label).is_none() {
            return Err(give_up("登录窗口已关闭", captured, last_error));
        }
        if started.elapsed() > TIMEOUT {
            return Err(give_up("等待登录超时", captured, last_error));
        }

        // 心跳：长时间没取到凭证时也要让表单看到「还在等」，而不是停在「等待登录」。
        // 用独立的游标去重，避免与上面的进度文案来回覆盖造成刷屏。
        let waited = started.elapsed().as_secs() / 15 * 15;
        if waited >= 15 && waited != last_beat {
            last_beat = waited;
            emit_progress(
                app,
                &format!("仍在等待登录（已等待 {waited} 秒），请在登录窗口完成登录…"),
            );
        }

        tokio::time::sleep(POLL).await;
    }
}
