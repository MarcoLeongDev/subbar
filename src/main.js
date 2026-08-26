var $ = function(s) { return document.getElementById(s); };

var refreshSec = parseInt(localStorage.getItem('mm_refresh')) || 300;
var markOn5h = localStorage.getItem('mm_mark_5h') !== 'off';
var markOnWeek = localStorage.getItem('mm_mark_week') !== 'off';
var markOnMonth = localStorage.getItem('mm_mark_month') !== 'off';
var lang = localStorage.getItem('mm_lang') || 'en';
var theme = localStorage.getItem('mm_theme') || 'dark';
var endpoint = localStorage.getItem('mm_endpoint') || 'ocg';
var settingsOpen = false;
// SEC-6-6: apiKey holds the plaintext only after the user explicitly reveals
// it via the eye button. Otherwise the IPC returns a redacted form.
var apiKey = '';
var redactedKey = '';
var keyRevealed = false;
var keyInputDirty = false;
// OpenCode Go credential state (mirrors the Minimax key state above).
var ocgWs = '';
var ocgCookie = '';
var ocgRevealed = false;
var ocgDirty = false;
var ocgHasCredentials = false;
var ocgSaveTimer = null;

function invoke(cmd, args) {
  return window.__TAURI__.core.invoke(cmd, args);
}

var dragTargets = new WeakSet();
document.querySelectorAll('button, input, textarea, select, .footer').forEach(function(el) { dragTargets.add(el); });
function onDragStart(e) {
  var t = e.target;
  while (t && t !== this) { if (dragTargets.has(t)) return; t = t.parentElement; }
  e.preventDefault();
  window.__TAURI__.window.getCurrentWindow().startDragging();
}
$('widget').addEventListener('mousedown', onDragStart);
$('settingsPanel').addEventListener('mousedown', onDragStart);

var i18n = {
  en: {title:'SubBar',unit:'s',aes:'Stored in OS keychain',
       errKey:'API key needed\nclick',
       errKeyPrefix:'API key should start with sk-\nclick',
        pill5h:'5h',pillWeek:'Week',pillMonth:'Month',ocgHint:'Enter your OpenCode Go workspace ID & auth cookie (from opencode.ai)'},
  'zh-tw': {title:'SubBar',unit:'秒',aes:'儲存於 OS 鑰匙圈',
        errKey:'需要 API 金鑰\n點擊',
        errKeyPrefix:'API 金鑰應以 sk- 開頭\n點擊',
        pill5h:'5小時',pillWeek:'週',pillMonth:'月',ocgHint:'請輸入 OpenCode Go 工作區 ID 與 auth cookie（來自 opencode.ai）'},
  zh: {title:'SubBar',unit:'秒',aes:'存储于 OS 钥匙串',
        errKey:'需要 API 密钥\n点击',
        errKeyPrefix:'API 密钥应以 sk- 开头\n点击',
        pill5h:'5小时',pillWeek:'周',pillMonth:'月',ocgHint:'请输入 OpenCode Go 工作区 ID 与 auth cookie（来自 opencode.ai）'},
  ja: {title:'SubBar',unit:'秒',aes:'OSキーチェーンに保存',
        errKey:'APIキーが必要です\nクリック',
        errKeyPrefix:'APIキーは sk- で始める必要があります\nクリック',
        pill5h:'5時間',pillWeek:'週間',pillMonth:'月',ocgHint:'OpenCode Go の workspace ID と auth cookie を入力（opencode.ai から）'},
  es: {title:'SubBar',unit:'s',aes:'Almacenado en llavero del SO',
        errKey:'Se necesita clave API\nhaga clic en',
        errKeyPrefix:'La clave API debe comenzar con sk-\nhaga clic en',
        pill5h:'5h',pillWeek:'Semana',pillMonth:'Mes',ocgHint:'Introduce el ID de workspace y auth cookie de OpenCode Go (de opencode.ai)'}
};
function t(k) { return (i18n[lang] || i18n.en)[k] || k; }
function applyLang() {
  document.querySelectorAll('[data-i18n]').forEach(function(el) {
    el.textContent = t(el.dataset.i18n);
  });
}

function showErrorMsg(container, msg) {
  container.textContent = '';
  var d = document.createElement('div');
  d.className = 'error';
  var lines = msg.split('\n');
  for (var i = 0; i < lines.length; i++) {
    if (i > 0) d.appendChild(document.createElement('br'));
    d.appendChild(document.createTextNode(lines[i]));
  }
  container.appendChild(d);
}

function validateApiKeyPrefix(key) {
  if (!key) return true; // Empty key is handled elsewhere (clears the key)
  if (!key.startsWith('sk-')) {
    showErrorMsg($('content'), t('errKeyPrefix'));
    updateKeyState();
    return false;
  }
  return true;
}

function applyTheme() {
  var w = $('widget');
  w.classList.remove('theme-dark','theme-light');
  w.classList.add('theme-' + theme);
  document.querySelectorAll('#segThemeHeader .seg-btn').forEach(function(b) {
    b.classList.toggle('active', b.dataset.val === theme);
  });
}

function getTicks(pillCls) {
  var show;
  if (pillCls === '5h') show = markOn5h;
  else if (pillCls === 'week') show = markOnWeek;
  else if (pillCls === 'month') show = markOnMonth;
  else show = false;
  return show ? [25, 50, 75] : [];
}

function createPill(cls, label, sublabel, pct, ticks) {
  var pill = document.createElement('div');
  pill.className = 'pill pill-' + cls;
  pill.dataset.pill = cls;

  var fill = document.createElement('div');
  fill.className = 'pill-fill';
  fill.style.width = pct + '%';
  pill.appendChild(fill);

  if (ticks.length) {
    var ticksDiv = document.createElement('div');
    ticksDiv.className = 'pill-ticks';
    for (var i = 0; i < ticks.length; i++) {
      var tick = document.createElement('div');
      tick.className = 'tick';
      tick.style.left = ticks[i] + '%';
      ticksDiv.appendChild(tick);
    }
    pill.appendChild(ticksDiv);
  }

  var labelSpan = document.createElement('span');
  labelSpan.className = 'pill-label';
  labelSpan.textContent = label;
  if (sublabel) {
    var subSpan = document.createElement('span');
    subSpan.className = 'pill-sublabel';
    subSpan.textContent = sublabel;
    labelSpan.appendChild(subSpan);
  }
  pill.appendChild(labelSpan);

  var valueSpan = document.createElement('span');
  valueSpan.className = 'pill-value';
  valueSpan.textContent = pct;
  var subPct = document.createElement('span');
  subPct.className = 'pill-sub';
  subPct.textContent = '%';
  valueSpan.appendChild(subPct);
  pill.appendChild(valueSpan);

  return pill;
}

function togglePillMarker(pillCls) {
  var on;
  if (pillCls === '5h') {
    markOn5h = !markOn5h;
    localStorage.setItem('mm_mark_5h', markOn5h ? 'on' : 'off');
    on = markOn5h;
  } else if (pillCls === 'week') {
    markOnWeek = !markOnWeek;
    localStorage.setItem('mm_mark_week', markOnWeek ? 'on' : 'off');
    on = markOnWeek;
  } else if (pillCls === 'month') {
    markOnMonth = !markOnMonth;
    localStorage.setItem('mm_mark_month', markOnMonth ? 'on' : 'off');
    on = markOnMonth;
  }
  var ticks = document.querySelector('.pill-' + pillCls + ' .pill-ticks');
  if (ticks) {
    ticks.style.display = on ? '' : 'none';
  }
}

function timestampLabel(ms) {
  var d = new Date(ms);
  return d.toLocaleTimeString([], {hour:'2-digit',minute:'2-digit',hour12:false});
}

function toggleSettings() {
  settingsOpen = !settingsOpen;
  $('settingsPanel').classList.toggle('show', settingsOpen);
  $('moreBtn').classList.toggle('settings-open', settingsOpen);
  $('moreBtn').style.display = settingsOpen ? 'none' : '';
  if (settingsOpen) {
    refreshKeyDisplay();
    $('refreshSlider').value = refreshSec;
    $('rangeVal').textContent = refreshSec;
    document.querySelectorAll('#segLang .seg-btn').forEach(function(b) {
      b.classList.toggle('active', b.dataset.val === lang);
    });
    document.querySelectorAll('#segEndpoint .seg-btn').forEach(function(b) {
      b.classList.toggle('active', b.dataset.val === endpoint);
    });
    applyTheme();
    fetchUsage(); // Fetch immediately when settings opens
  }
}

// A fixed-length masked string so the password field reads as "filled" when a
// key is stored, without leaking the secret into the DOM as readable text.
function maskedKey() {
  return '••••••••••••';
}

async function refreshKeyDisplay() {
  if (endpoint === 'ocg') {
    await refreshOcgCredsDisplay();
    return;
  }
  // SEC-6-6: default fetch returns redacted form; only the eye toggle fetches plaintext.
  redactedKey = await invoke('get_api_key', { endpoint: endpoint, reveal: false });
  $('apiKeyInput').placeholder = redactedKey || 'sk-cp-...';
  apiKey = '';
  keyRevealed = false;
  keyInputDirty = false;
  // SEC-6-6: fill the field with masked dots (not just a grayed placeholder
  // hint) so it visibly reads as "filled" when a key is stored; the real key
  // stays out of the DOM until the eye reveals it.
  $('apiKeyInput').type = 'password';
  $('apiKeyInput').value = redactedKey ? maskedKey() : '';
  updateKeyState();
}

async function refreshOcgCredsDisplay() {
  // Reveal:true so we can repopulate the fields from the keychain. The cookie
  // lands in a type=password input, so it stays masked in the UI while still
  // being restored after switching endpoints or restarting the app.
  var creds = await invoke('get_ocg_credentials', { reveal: true });
  ocgHasCredentials = !!creds.has_credentials;
  ocgRevealed = false;
  ocgDirty = false;
  if (ocgHasCredentials) {
    ocgWs = creds.workspace_id || '';
    ocgCookie = creds.auth_cookie || '';
    $('ocgWsInput').value = ocgWs;
    $('ocgCookieInput').value = ocgCookie;
  } else {
    // No creds in the backend yet: restore from the localStorage backup if we
    // have one, otherwise keep whatever the user has typed so far.
    var lws = '';
    var lck = '';
    try { lws = localStorage.getItem('ocg_ws') || ''; lck = localStorage.getItem('ocg_cookie') || ''; } catch (_) {}
    if (lws || lck) {
      $('ocgWsInput').value = lws;
      $('ocgCookieInput').value = lck;
      ocgWs = lws;
      ocgCookie = lck;
    } else {
      ocgWs = $('ocgWsInput').value.trim();
      ocgCookie = $('ocgCookieInput').value.trim();
    }
  }
  updateOcgCredsState();
}

async function toggleOcgCookieReveal() {
  if (ocgRevealed) {
    ocgCookie = '';
    $('ocgCookieInput').value = '';
    ocgRevealed = false;
    ocgDirty = false;
  } else {
    var creds = await invoke('get_ocg_credentials', { reveal: true });
    ocgCookie = creds.auth_cookie || '';
    $('ocgCookieInput').value = ocgCookie;
    ocgRevealed = true;
    ocgDirty = false;
  }
  updateOcgCredsState();
}

async function saveOcgCreds() {
  var ws = $('ocgWsInput').value.trim();
  var cookie = $('ocgCookieInput').value.trim();
  // Tauri v2 converts Rust param names to camelCase for IPC args, so the JS
  // must send `workspaceId`/`authCookie` to match `workspace_id`/`auth_cookie`.
  await invoke('set_ocg_credentials', { workspaceId: ws, authCookie: cookie });
  // Backup so the fields survive a webview reload even if the keychain write is
  // temporarily unavailable (keychain remains the primary store).
  try {
    localStorage.setItem('ocg_ws', ws);
    localStorage.setItem('ocg_cookie', cookie);
  } catch (_) {}
  ocgWs = ws;
  ocgCookie = cookie;
  ocgDirty = false;
  ocgRevealed = false;
  ocgHasCredentials = !!(ws && cookie);
  // Keep the typed values visible — do NOT re-fetch and wipe the inputs.
  updateOcgCredsState();
}

// Persist whatever is currently in the OCg fields (if anything), independent of
// the dirty flag, so an explicit refresh/save always stores entered creds even
// if input/blur events were missed.
async function ocgSaveIfPresent() {
  var ws = $('ocgWsInput').value.trim();
  var ck = $('ocgCookieInput').value.trim();
  if (ws || ck) await saveOcgCreds();
}

// Debounced save while typing + immediate save when the field loses focus.
function scheduleOcgSave() {
  if (ocgSaveTimer) clearTimeout(ocgSaveTimer);
  ocgSaveTimer = setTimeout(function() {
    ocgSaveTimer = null;
    if (ocgDirty) saveOcgCreds();
  }, 600);
}

async function clearOcgCreds() {
  $('ocgWsInput').value = '';
  $('ocgCookieInput').value = '';
  ocgWs = '';
  ocgCookie = '';
  ocgDirty = true;
  await saveOcgCreds();
  $('ocgWsInput').focus();
}

function updateOcgCredsState() {
  var hasCookie = !!$('ocgCookieInput').value.trim() || !!ocgCookie;
  var hasStored = ocgHasCredentials;
  $('ocgShieldBtn').style.display = hasStored ? 'flex' : 'none';
  $('ocgEyeBtn').style.display = hasStored ? 'flex' : 'none';
  $('ocgClearBtn').style.display = hasStored ? 'flex' : 'none';
  $('ocgCookieInput').style.paddingLeft = hasStored ? '40px' : '10px';
  $('ocgCookieInput').style.paddingRight = (hasStored || hasCookie) ? '40px' : '10px';
}

async function toggleKeyReveal() {
  if (keyRevealed) {
    apiKey = '';
    $('apiKeyInput').value = maskedKey();
    $('apiKeyInput').type = 'password';
    keyRevealed = false;
    keyInputDirty = false;
  } else {
    apiKey = await invoke('get_api_key', { endpoint: endpoint, reveal: true });
    $('apiKeyInput').value = apiKey;
    // Switch to text so the revealed key is actually visible (it was previously
    // placed into a type=password field, so reveal was also invisible).
    $('apiKeyInput').type = 'text';
    keyRevealed = true;
    keyInputDirty = false;
  }
  updateKeyState();
}

async function switchEndpoint(ep) {
  if (ep === endpoint) return;
  // SEC-6-6: only save the pending key if the user actually edited or
  // explicitly revealed + accepted it. Otherwise leave the previous key
  // untouched when switching endpoints.
  if (keyInputDirty || keyRevealed) {
    var pendingKey = $('apiKeyInput').value.trim();
    if (!validateApiKeyPrefix(pendingKey)) return;
    await invoke('set_api_key', { key: pendingKey, endpoint: endpoint });
  }
  endpoint = ep;
  localStorage.setItem('mm_endpoint', endpoint);
  applyEndpointChrome();
  // Sync the selected endpoint to the backend so the background timer / tray
  // title use the same data source (otherwise the menubar keeps showing the
  // previous endpoint's state, e.g. AUTH! from an invalid Minimax key).
  await invoke('set_endpoint', { ep: endpoint });
  await refreshKeyDisplay();
  $('content').textContent = '';
  restartTimer();
  fetchUsage();
}

async function applySettings() {
  var newKey = null;
  if (keyInputDirty) {
    newKey = $('apiKeyInput').value.trim();
    if (!validateApiKeyPrefix(newKey)) return;
  } else if (keyRevealed) {
    newKey = apiKey;
    if (!validateApiKeyPrefix(newKey)) return;
  }
  // else: no change to key — skip set_api_key entirely.

  var newRefresh = parseInt($('refreshSlider').value) || 300;
  var newLang = document.querySelector('#segLang .seg-btn.active').dataset.val;
  var newTheme = document.querySelector('#segThemeHeader .seg-btn.active').dataset.val;

  var langChanged = lang !== newLang;
  var themeChanged = theme !== newTheme;
  var keyChanged = newKey !== null && newKey !== redactedKey && newKey !== apiKey;
  var refreshChanged = newRefresh !== refreshSec;

  if (newKey !== null) {
    apiKey = newKey;
    redactedKey = newKey === ''
      ? ''
      : (newKey.length > 8 ? (newKey.slice(0, 4) + '...' + newKey.slice(-4)) : '[REDACTED]');
    $('apiKeyInput').placeholder = redactedKey || 'sk-cp-...';
    await invoke('set_api_key', { key: apiKey, endpoint: endpoint });
    $('apiKeyInput').type = 'password';
    $('apiKeyInput').value = redactedKey ? maskedKey() : '';
    keyInputDirty = false;
    keyRevealed = false;
  }

  // OpenCode Go credentials live in their own keychain items; persist whatever
  // is in the fields when applying settings.
  if (endpoint === 'ocg') {
    await ocgSaveIfPresent();
  }
  refreshSec = newRefresh;
  theme = newTheme;

  await invoke('set_refresh_interval', { interval: refreshSec });
  localStorage.setItem('mm_endpoint', endpoint);
  localStorage.setItem('mm_refresh', refreshSec);
  localStorage.setItem('mm_lang', newLang);
  localStorage.setItem('mm_theme', theme);
  lang = newLang;
  applyLang();
  renderOcgHint();
  applyTheme();

  if (langChanged || themeChanged) $('content').textContent = '';
  if (keyChanged || refreshChanged) restartTimer();
  fetchUsage();
}

async function clearKey() {
  $('apiKeyInput').value = '';
  keyInputDirty = true; // explicit clear intent: applySettings should write empty
  updateKeyState();
  await applySettings();
  $('apiKeyInput').focus();
}

$('moreBtn').onclick = toggleSettings;
$('settingsClose').onclick = function() { if (settingsOpen) toggleSettings(); };
$('powerBtn').onclick = function() { invoke('quit_app'); };
$('keyClearBtn').onclick = clearKey;
$('keyEyeBtn').onclick = toggleKeyReveal;
$('keyCheckBtn').onclick = function() { applySettings(); };
$('apiKeyInput').oninput = function() {
  keyInputDirty = true;
  keyRevealed = false;
  updateKeyState();
};
$('apiKeyInput').onchange = function() { if (keyInputDirty) applySettings(); };
// Selecting the masked dots on focus lets the first keystroke replace them with
// a fresh key instead of inserting characters between the dots.
$('apiKeyInput').addEventListener('focus', function() {
  if (!keyRevealed && $('apiKeyInput').value === maskedKey()) {
    $('apiKeyInput').select();
  }
});
$('refreshSlider').oninput = function() { $('rangeVal').textContent = this.value; };
$('refreshSlider').onchange = function() { applySettings(); };
$('refreshIcon').onclick = function() { fetchUsage(); };

$('ocgEyeBtn').onclick = toggleOcgCookieReveal;
$('ocgClearBtn').onclick = clearOcgCreds;
// Auto-save on every keystroke (debounced) and immediately when the field
// loses focus, so credentials persist without an explicit save button.
function onOcgInput() { ocgDirty = true; updateOcgCredsState(); scheduleOcgSave(); }
// Save unconditionally on blur/change (not gated on the dirty flag) so a save
// is guaranteed whenever the user leaves the field, mirroring minimax.
function onOcgBlur() { ocgSaveIfPresent(); }
function onOcgChange() { ocgSaveIfPresent(); }
$('ocgWsInput').addEventListener('input', onOcgInput);
$('ocgWsInput').addEventListener('blur', onOcgBlur);
$('ocgWsInput').addEventListener('change', onOcgChange);
$('ocgCookieInput').addEventListener('input', onOcgInput);
$('ocgCookieInput').addEventListener('blur', onOcgBlur);
$('ocgCookieInput').addEventListener('change', onOcgChange);

// Reveal the minimal scrollbar only while actively scrolling (or hovered, via
// CSS), then fade it back out.
var settingsPanelEl = $('settingsPanel');
var ocgScrollTimer = null;
settingsPanelEl.addEventListener('scroll', function() {
  settingsPanelEl.classList.add('scrolling');
  if (ocgScrollTimer) clearTimeout(ocgScrollTimer);
  ocgScrollTimer = setTimeout(function() {
    settingsPanelEl.classList.remove('scrolling');
  }, 700);
});

function updateKeyState() {
  // SEC-6-6: visual indicators depend on whether the plaintext key is in
  // memory (revealed) and whether the input has been edited.
  var hasPlaintextInMemory = !!apiKey;
  var hasStoredKey = !!redactedKey;
  var hasText = $('apiKeyInput').value.length > 0;
  $('keyShieldBtn').style.display = hasStoredKey ? 'flex' : 'none';
  $('keyEyeBtn').style.display = hasStoredKey ? 'flex' : 'none';
  $('keyEyeBtn').title = keyRevealed ? 'Hide key' : 'Show key';
  $('keyCheckBtn').style.display = (!hasStoredKey && hasText) ? 'flex' : 'none';
  $('keyClearBtn').style.display = hasStoredKey ? 'flex' : 'none';
  $('apiKeyInput').style.paddingLeft = hasStoredKey ? '40px' : '10px';
  $('apiKeyInput').style.paddingRight = (hasStoredKey || hasText) ? '40px' : '10px';
}

document.querySelectorAll('#segThemeHeader .seg-btn, #segLang .seg-btn').forEach(function(b) {
  b.onclick = function() {
    var parent = this.parentElement;
    parent.querySelectorAll('.seg-btn').forEach(function(x) { x.classList.remove('active'); });
    this.classList.add('active');
    applySettings();
  };
});

document.querySelectorAll('#segEndpoint .seg-btn').forEach(function(b) {
  b.onclick = function() {
    document.querySelectorAll('#segEndpoint .seg-btn').forEach(function(x) { x.classList.remove('active'); });
    this.classList.add('active');
    switchEndpoint(this.dataset.val);
  };
});

document.addEventListener('click', function(e) {
  if (settingsOpen && !$('settingsPanel').contains(e.target) && e.target !== $('moreBtn') && !$('moreBtn').contains(e.target) && e.target !== $('settingsClose') && !$('settingsClose').contains(e.target)) toggleSettings();
});

// JS timer disabled: hidden WebView throttling makes it unreliable.
// All periodic fetching is handled by the Rust background timer.
function restartTimer() {}

async function fetchUsage() {
  // OCG endpoint reads OpenCode Go usage from the `agent-limits` CLI and has
  // no API key — bypass the key check and render 3 bars (5h / week / month).
  if (endpoint === 'ocg') {
    // Persist whatever is in the fields (if anything) BEFORE fetching so the
    // backend's AppState has the current credentials; the fetch command reads
    // them from there.
    await ocgSaveIfPresent();
    return fetchEndpointUsage('fetch_ocg_quota', buildOcgBars);
  }

  var c = $('content');

  // SEC-6-6: any key set on the backend (redacted or plaintext) counts as
  // "has key" — the actual API call goes through Rust which has the plaintext.
  if (!redactedKey) {
    showErrorMsg(c, t('errKey'));
    updateKeyState();
    $('refreshIcon').style.display = 'none';
    $('refreshTime').textContent = '';
    return;
  }

  return fetchEndpointUsage('fetch_quota', buildMinimaxBars);
}

async function fetchEndpointUsage(cmd, builder, args) {
  var c = $('content');
  var hasPills = c.querySelector('.pill') !== null;
  if (!hasPills) {
    c.textContent = '';
    var sp = document.createElement('div');
    sp.className = 'spinner';
    c.appendChild(sp);
  }
  $('refreshIcon').style.display = 'none';
  $('refreshSpinner').style.display = 'inline-block';

  try {
    var data = await invoke(cmd, args || {});
    var bars = builder(data);
    renderBars(c, bars, hasPills);

    $('refreshSpinner').style.display = 'none';
    $('refreshIcon').style.display = 'inline';
    updateKeyState();
    var now = new Date();
    $('refreshTime').textContent = now.toLocaleTimeString([], {hour:'2-digit',minute:'2-digit',second:'2-digit',hour12:false});
  } catch(e) {
    $('refreshSpinner').style.display = 'none';
    $('refreshIcon').style.display = 'inline';
    $('refreshTime').textContent = '';
    if (!hasPills) {
      var errDiv = document.createElement('div');
      errDiv.className = 'error';
      errDiv.textContent = (e && e.message) ? e.message : String(e);
      c.textContent = '';
      c.appendChild(errDiv);
    }
  }
}

function buildMinimaxBars(data) {
  if (data.base_resp && data.base_resp.status_code !== 0) {
    throw new Error(data.base_resp.status_msg || 'API error');
  }
  var m = (data.model_remains || []).find(function(x) { return x.model_name === 'general'; });
  if (!m) throw new Error('No general model data');

  var intervalPct = Math.round(100 - (m.current_interval_remaining_percent || 100));
  var weeklyPct = Math.round(100 - (m.current_weekly_remaining_percent || 100));
  var startH = timestampLabel(m.start_time);
  var endH = timestampLabel(m.end_time);

  return [
    { id:'5h', labelKey:'pill5h', sublabel: startH + '~' + endH, pct: intervalPct, ticks: getTicks('5h') },
    { id:'week', labelKey:'pillWeek', sublabel: '', pct: weeklyPct, ticks: getTicks('week') }
  ];
}

function labelKeyForOcg(id) {
  if (id === '5h') return 'pill5h';
  if (id === 'week') return 'pillWeek';
  return 'pillMonth';
}

function buildOcgBars(data) {
  var bars = data.bars || [];
  return bars.map(function(b) {
    return {
      id: b.id,
      labelKey: labelKeyForOcg(b.id),
      sublabel: b.reset_at ? timestampLabel(Date.parse(b.reset_at)) : '',
      pct: Math.round(b.used_percent || 0),
      ticks: getTicks(b.id)
    };
  });
}

function renderBars(c, bars, hasPills) {
  if (hasPills) {
    var pills = c.querySelectorAll('.pill');
    for (var i = 0; i < pills.length; i++) {
      var bar = bars[i];
      if (!bar) continue;
      var fill = pills[i].querySelector('.pill-fill');
      if (fill) fill.style.width = bar.pct + '%';
      var val = pills[i].querySelector('.pill-value');
      if (val && val.childNodes[0]) val.childNodes[0].textContent = bar.pct;
      var sub = pills[i].querySelector('.pill-sublabel');
      if (sub) sub.textContent = bar.sublabel;
    }
  } else {
    c.textContent = '';
    bars.forEach(function(b) {
      c.appendChild(createPill(b.id, t(b.labelKey), b.sublabel, b.pct, b.ticks));
    });
    c.querySelectorAll('.pill').forEach(function(pill) {
      pill.addEventListener('click', function() {
        togglePillMarker(pill.dataset.pill);
        fetchUsage();
      });
    });
  }
}

function applyEndpointChrome() {
  var w = $('widget');
  w.classList.toggle('endpoint-ocg', endpoint === 'ocg');
  var keyRow = $('apiKeyInput').closest('.setting-row');
  if (keyRow) keyRow.style.display = endpoint === 'ocg' ? 'none' : '';
  $('ocgCredsRow').style.display = endpoint === 'ocg' ? 'flex' : 'none';
  if ($('ocgHintRow')) $('ocgHintRow').style.display = endpoint === 'ocg' ? 'flex' : 'none';
  if (endpoint === 'ocg') renderOcgHint();
}

// Render the ocg hint. The `opencode.ai` token is an anchor to the dashboard
// (no underline until hovered) so users know where to obtain the cookie.
function renderOcgHint() {
  if (endpoint !== 'ocg') return;
  var row = $('ocgHintRow');
  if (!row) return;
  var span = row.querySelector('.ocg-hint-text');
  if (!span) return;
  var msg = t('ocgHint');
  var url = 'https://opencode.ai';
  span.innerHTML = msg.split('opencode.ai').join(
    '<a href="' + url + '" class="cli-link" target="_blank" rel="noreferrer">opencode.ai</a>'
  );
}

// The CLI link must open in the system browser (the webview won't navigate to
// a foreign URL on its own). Intercept clicks and hand the URL to Rust.
var ocgHintRow = $('ocgHintRow');
if (ocgHintRow) ocgHintRow.addEventListener('click', function(e) {
  var a = e.target.closest('.cli-link');
  if (!a) return;
  e.preventDefault();
  var url = a.getAttribute('href');
  invoke('open_external', { url: url }).catch(function() {});
});

async function init() {
  // SEC-6-6: fetch redacted form only; plaintext never enters the webview
  // until the user clicks the eye to reveal it.
  redactedKey = await invoke('get_api_key', { endpoint: endpoint, reveal: false });
  apiKey = '';
  keyRevealed = false;
  keyInputDirty = false;
  $('apiKeyInput').placeholder = redactedKey || 'sk-cp-...';
  $('apiKeyInput').type = 'password';
  $('apiKeyInput').value = redactedKey ? maskedKey() : '';
  applyLang();
  applyTheme();
  applyEndpointChrome();
  if (endpoint === 'ocg') await refreshOcgCredsDisplay();
  // Sync the persisted endpoint to the backend on startup too.
  try { await invoke('set_endpoint', { ep: endpoint }); } catch(_) {}
  var tip = $('keyShieldTip');
  if (tip) tip.textContent = t('aes');
  fetchUsage();

  // Fetch immediately when window becomes visible (tray click, focus, etc.)
  document.addEventListener('visibilitychange', function() {
    if (!document.hidden) fetchUsage();
  });
  window.addEventListener('focus', function() {
    fetchUsage();
  });

  try { $('appVersion').textContent = 'v' + await invoke('get_app_version'); } catch(_) {}
}
init().catch(function(e) { console.error('init error:', e); });
