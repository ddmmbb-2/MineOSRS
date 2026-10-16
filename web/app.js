'use strict';
let authToken = sessionStorage.getItem('mineos_token') || '';
let serverList = [];
let selectedId = null;
let logInterval = null;
let noticeTimer = null;
let javaVersions = [];
let jobs = {};
let editingServerId = null;
let activeDataPath = '';
let backupCache = null;
let backupCacheFor = null;
let backupLastFetch = 0;
let backupScheduleDirty = false;


const $ = id => document.getElementById(id);
const make = (tag, className, value) => {
  const e = document.createElement(tag);
  if (className) e.className = className;
  if (value !== undefined) e.textContent = String(value);
  return e;
};
const friendlyState = {running:'執行中', stopped:'已停止', stopping:'停止中', starting:'啟動中'};
function alertUser(message, isError = false) {
  clearTimeout(noticeTimer);
  const el = $('notice');
  el.textContent = message;
  el.classList.toggle('error', isError);
  el.hidden = false;
  noticeTimer = setTimeout(() => { el.hidden = true; }, 6500);
}

async function request(path, options = {}) {
  const resp = await fetch('/api' + path, {
    ...options,
    headers: {Authorization: `Bearer ${authToken}`, ...(options.headers || {})},
  });
  if (resp.status === 401) { logout(); throw new Error('登入已失效，請重新登入'); }
  if (!resp.ok) throw new Error(await resp.text());
  return resp.json();
}
async function send(path, obj) {
  return request(path, {method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify(obj || {})});
}
function showWorkspace(show) {
  $('loginPanel').hidden = show;
  $('workspace').hidden = !show;
}
function logout() {
  authToken = '';
  sessionStorage.removeItem('mineos_token');
  showWorkspace(false);
  clearInterval(logInterval);
  logInterval = null;
  if ($('passwordForm')) $('passwordForm').hidden = true;
}
async function login() {
  const password = $('token').value;
  if (!password) return alertUser('請輸入管理密碼或首次登入 Token', true);
  try {
    const resp = await fetch('/api/auth/login', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({password})});
    if (!resp.ok) throw new Error(await resp.text());
    const result = await resp.json();
    authToken = result.session;
    sessionStorage.setItem('mineos_token', authToken);
    $('token').value = '';
    if (!result.password_set) alertUser('首次登入成功，建議立即點選「變更管理密碼」');
    showWorkspace(true);
    await refresh();
    await loadSystemInfo();
    await updateCoreType();
    if (!logInterval) logInterval = setInterval(refreshQuiet, 1500);
  } catch (e) { alertUser(e.message, true); }
}

async function refreshQuiet() {
  try { await refresh(); } catch(e) { console.warn(e); }
}
async function refresh() {
  const [list, javas, currentJobs] = await Promise.all([request('/servers'), request('/java'), request('/jobs')]);
  serverList = list;
  javaVersions = javas;
  jobs = currentJobs;
  renderJava();
  if (!selectedId && serverList.length) selectedId = serverList[0].id;
  if (selectedId && !serverList.some(s => s.id === selectedId)) selectedId = null;
  renderServers();
  await renderDetails();
}
function renderServers() {
  const host = $('servers'); host.replaceChildren();
  if (!serverList.length) host.append(make('div','hint','尚未建立伺服器'));
  for (const s of serverList) {
    const btn = make('button','serverItem' + (s.id === selectedId ? ' active' : ''));
    const desc = make('span');
    desc.append(make('span','name',s.name),make('div','meta',`Port ${s.port} · ${s.xms_mb}–${s.xmx_mb} MB · ${s.core_type === 'manual' ? '手動核心' : s.core_type + ' ' + s.mc_version}`));
    const status = make('span','state '+s.status,friendlyState[s.status] || s.status);
    btn.append(desc,status);
    btn.onclick = () => {
      if (selectedId !== s.id) { closeSettings(); backupCache = null; backupCacheFor = null; backupScheduleDirty = false; }
      selectedId = s.id;
      refresh().catch(e=>alertUser(e.message,true));
    };
    host.append(btn);
  }
}
async function renderDetails() {
  const srv = serverList.find(s => s.id === selectedId);
  $('detail').hidden = !srv;
  $('selectedHint').hidden = !!srv;
  $('selectedName').textContent = srv ? srv.name : '—';
  if (!srv) { closeSettings(); backupCache = null; return; }
  $('eulaPrompt').hidden = !!srv.eula_accepted;
  $('serverPath').textContent = `檔案位置：${srv.folder_path}`;
  $('editSettingsBtn').disabled = srv.status !== 'stopped';
  if (editingServerId && editingServerId !== srv.id) closeSettings();
  if (editingServerId === srv.id) $('saveSettingsBtn').disabled = srv.status !== 'stopped';
  $('serverInfo').textContent = `Port: ${srv.port}  |  Java: ${srv.java_command}  |  RAM: ${srv.xms_mb}–${srv.xmx_mb} MB  |  ${srv.message || '狀態正常'}`;
  const job = jobs[`core:${srv.id}`];
  const core = $('coreStatus');
  core.textContent = job ? `核心下載：${job.detail}${job.percent == null ? '' : ` (${job.percent}%)`}` : (srv.core_type !== 'manual' ? `核心：${srv.core_type} ${srv.mc_version}` : '手動核心：上傳 server.jar');
  core.className = job ? `jobText ${job.state}` : 'hint';
  const files = await request(`/servers/${encodeURIComponent(srv.id)}/files`);
  const hasCore = !!srv.core_ready;
  $('retryCoreBtn').hidden = !(srv.core_type !== 'manual' && !hasCore && (!job || job.state === 'error')); 
  $('startBtn').disabled = srv.status !== 'stopped' || !srv.eula_accepted || !hasCore || (job && job.state === 'working');
  $('stopBtn').disabled = srv.status !== 'running';
  $('removeBtn').disabled = srv.status !== 'stopped' || (job && job.state === 'working');
  $('restartBtn').disabled = srv.status === 'stopping';
  const log = await request(`/servers/${encodeURIComponent(srv.id)}/logs`);
  const consoleBox = $('console');
  const follow = consoleBox.scrollTop + consoleBox.clientHeight >= consoleBox.scrollHeight - 40;
  const content = log.join('\n');
  if (consoleBox.textContent !== content) consoleBox.textContent = content;
  if (follow) consoleBox.scrollTop = consoleBox.scrollHeight;
  const host = $('files'); host.replaceChildren();
  for (const f of files) host.append(make('div','fileRow',`${f.folder}/ ${f.name}  (${(f.bytes / 1024 / 1024).toFixed(2)} MB)`));
  if (!files.length) host.append(make('div','hint','尚未上傳任何 JAR'));
  await loadBackups(false);
}

async function performAction(action) {
  if (!selectedId) return;
  try {
    await send(`/servers/${encodeURIComponent(selectedId)}/${action}`);
    alertUser(({start:'已啟動 Java 程序',stop:'已送出正常停止指令',restart:'已要求重新啟動'})[action]);
    await refresh();
  } catch(e) { alertUser(e.message,true); }
}
async function createServer(event) {
  event.preventDefault();
  const data = new FormData(event.target);
  const body = {
    name: String(data.get('name') || '').trim(),
    port: Number(data.get('port')),
    xms_mb: Number(data.get('xms_mb')),
    xmx_mb: Number(data.get('xmx_mb')),
    java_command: String(data.get('java_command') || '').trim(),
    core_type: String(data.get('core_type') || 'manual'),
    mc_version: String(data.get('mc_version') || '').trim(),
    auto_start: data.has('auto_start'),
    auto_restart: data.has('auto_restart'),
  };
  try {
    const s = await send('/servers',body);
    selectedId = s.id;
    backupCache = null;
    alertUser(body.core_type === 'manual' ? 'Server 已建立，請手動上傳核心 JAR 並確認 EULA' : 'Server 已建立，核心開始背景下載，請確認 Java 已安裝及 EULA');
    await refresh();
  } catch(e) { alertUser(e.message,true); }
}
async function sendConsole(event) {
  event.preventDefault();
  if (!selectedId) return;
  const input = $('command');
  try {
    await send(`/servers/${encodeURIComponent(selectedId)}/console`, {command:input.value});
    input.value = '';
    await refresh();
  } catch(e) { alertUser(e.message,true); }
}
async function uploadJar(event) {
  event.preventDefault();
  if (!selectedId) return;
  const file = $('jarfile').files[0];
  if (!file) return;
  const data = new FormData(); data.append('file',file);
  const folder = $('folder').value;
  try {
    const s = await request(`/servers/${encodeURIComponent(selectedId)}/upload?folder=${encodeURIComponent(folder)}`, {method:'POST',body:data});
    alertUser(`已上傳 ${s.file}`);
    $('jarfile').value = '';
    await refresh();
  } catch(e) { alertUser(e.message,true); }
}
async function acceptEula() {
  if (!selectedId) return;
  if (!window.confirm('請確認您已閱讀 Minecraft EULA 並同意其條款。要為此伺服器寫入 eula=true 嗎？')) return;
  try {
    await send(`/servers/${encodeURIComponent(selectedId)}/eula`, {accepted:true});
    alertUser('EULA 已確認');
    await refresh();
  } catch(e) { alertUser(e.message,true); }
}


function renderJava() {
  const host = $('javaList'); host.replaceChildren();
  for (const j of javaVersions) {
    const tile = make('div','runtimeRow');
    tile.append(make('h3','',`Java ${j.major}`));
    tile.append(make('div','meta',j.installed ? '已安裝（可用於新伺服器）' : '未安裝'));
    const job = jobs[`java:${j.major}`];
    if (job) tile.append(make('div',`jobText ${job.state}`,job.detail + (job.percent == null ? '' : ` (${job.percent}%)`)));
    if (!j.installed) {
      const btn = make('button','','下載並安裝');
      btn.disabled = !!job && job.state === 'working';
      btn.onclick = async () => {
        try { await send('/java/install',{major:j.major}); alertUser(`Java ${j.major} 已開始背景下載`); await refresh(); }
        catch(e) {alertUser(e.message,true);}
      };
      const buttons = make('div','buttons'); buttons.append(btn); tile.append(buttons);
    }
    host.append(tile);
  }
  for (const select of [$('javaChoice'), $('settingsJavaChoice')]) {
    for (const opt of select.options) {
      const num = Number(opt.value.replace('managed:',''));
      if (Number.isInteger(num) && num > 0) {
        opt.textContent = `內建 Java ${num}（${javaVersions.some(x => x.major === num && x.installed) ? '已安裝' : '尚未下載'}）`;
      }
    }
  }
}
async function updateCoreType() {
  const core = $('coreType').value;
  const isManual = core === 'manual';
  $('versionLabel').hidden = isManual;
  $('mcVersion').required = !isManual;
  $('createAdvice').textContent = isManual ? '建立後請上傳可直接啟動的 server.jar。' :
    (['forge','neoforge'].includes(core) ? 'Forge／NeoForge 會先下載官方 Installer 再安裝所需 libraries；Java 必須先下載。安裝過程可能較長。' : '將由官方自動下載核心；Java 請先從上方下載。');
  $('createForm').querySelector('button[type=submit]').textContent = isManual ? '建立 Server（手動上傳）' : '建立 Server 並下載核心';
  $('minecraftVersions').replaceChildren();
  $('mcVersion').value = '';
  if (isManual) return;
  $('versionHint').textContent = '正在查詢官方版本資訊…';
  try {
    const result = await request(`/versions?core=${encodeURIComponent(core)}`);
    if ($('coreType').value !== core) return;
    $('mcVersion').value = result.latest || '';
    for (const v of result.versions) {
      const item = make('option'); item.value = v;
      $('minecraftVersions').append(item);
    }
    $('versionHint').textContent = '可自行輸入版本；列表僅顯示最新部分正式版本';
    suggestJava();
  } catch(e) {
    if ($('coreType').value === core) $('versionHint').textContent = '版本清單讀取失敗，仍可手動輸入版本';
    alertUser(e.message,true);
  }
}
function suggestJava() {
  const v = $('mcVersion').value.trim();
  const core = $('coreType').value;
  if (core === 'manual' || !v) return;
  const parts = v.split('.').map(x => Number(x));
  let major = 21;
  if (parts[0] >= 26) major = 25;
  else if (parts[0] === 1) {
    if (parts[1] >= 21 || (parts[1] === 20 && parts[2] >= 5)) major = 21;
    else if (parts[1] >= 18) major = 17;
    else if (parts[1] === 17) major = 17;
    else major = 8;
  }
  const value = `managed:${major}`;
  if ([...$('javaChoice').options].some(x => x.value === value)) $('javaChoice').value = value;
}
async function retryCore() {
  const s = serverList.find(x => x.id === selectedId);
  if (!s || s.core_type === 'manual') return;
  const version = window.prompt('重新下載的 Minecraft 版本',s.mc_version);
  if (version === null) return;
  if (!/^[a-zA-Z0-9.-]{1,36}$/.test(version)) return alertUser('版本格式錯誤',true);
  try {
    await send(`/servers/${encodeURIComponent(s.id)}/install-core`,{core:s.core_type,version});
    alertUser('重新開始下載核心'); await refresh();
  } catch(e) { alertUser(e.message,true); }
}
async function removeSelected() {
  const s = serverList.find(x => x.id === selectedId);
  if (!s) return;
  if (!window.confirm(`確定移除「${s.name}」的管理設定？Port 將釋出，原本資料夾與世界存檔會保留，不會刪除。`)) return;
  try {
    await send(`/servers/${encodeURIComponent(s.id)}/remove`);
    selectedId = null;
    closeSettings();
    alertUser('已移除管理設定，伺服器資料夾仍保留在硬碟');
    await refresh();
  } catch(e) { alertUser(e.message,true); }
}

async function loadSystemInfo() {
  const info = await request('/system');
  activeDataPath = info.data_dir || '';
  $('dataLocation').textContent = activeDataPath;
}
function closeSettings() {
  editingServerId = null;
  $('settingsForm').hidden = true;
}
function editSettings() {
  const srv = serverList.find(x => x.id === selectedId);
  if (!srv || srv.status !== 'stopped') return alertUser('請先停止 Server 才能修改設定',true);
  const form = $('settingsForm');
  if (editingServerId === srv.id) { closeSettings(); return; }
  editingServerId = srv.id;
  form.elements.namedItem('name').value = srv.name;
  form.elements.namedItem('port').value = srv.port;
  form.elements.namedItem('xms_mb').value = srv.xms_mb;
  form.elements.namedItem('xmx_mb').value = srv.xmx_mb;
  const javaSelect = $('settingsJavaChoice');
  [...javaSelect.options].filter(o => o.dataset.custom === 'yes').forEach(o => o.remove());
  if (![...javaSelect.options].some(o => o.value === srv.java_command)) {
    const custom = make('option', '', `自訂 Java：${srv.java_command}`);
    custom.value = srv.java_command;
    custom.dataset.custom = 'yes';
    javaSelect.append(custom);
  }
  javaSelect.value = srv.java_command;
  form.elements.namedItem('auto_start').checked = srv.auto_start;
  form.elements.namedItem('auto_restart').checked = srv.auto_restart;
  $('saveSettingsBtn').disabled = false;
  form.hidden = false;
}
async function saveSettings(event) {
  event.preventDefault();
  const srv = serverList.find(x => x.id === editingServerId);
  if (!srv || srv.id !== selectedId) return alertUser('請重新選擇 Server',true);
  const values = new FormData(event.target);
  const settings = {
    name: String(values.get('name') || '').trim(),
    port: Number(values.get('port')),
    xms_mb: Number(values.get('xms_mb')),
    xmx_mb: Number(values.get('xmx_mb')),
    java_command: String(values.get('java_command') || '').trim(),
    auto_start: values.has('auto_start'),
    auto_restart: values.has('auto_restart'),
  };
  if (!settings.name || !Number.isInteger(settings.port) || settings.port < 1 || settings.port > 65535 ||
      !Number.isInteger(settings.xms_mb) || !Number.isInteger(settings.xmx_mb) ||
      settings.xms_mb < 256 || settings.xmx_mb < settings.xms_mb) {
    return alertUser('請檢查名稱、Port 與記憶體設定（Xms 不可大於 Xmx）',true);
  }
  $('saveSettingsBtn').disabled = true;
  try {
    await send(`/servers/${encodeURIComponent(srv.id)}/settings`,settings);
    closeSettings();
    alertUser('啟動設定已儲存；下次啟動 Server 時生效');
    await refresh();
  } catch(e) {
    $('saveSettingsBtn').disabled = false;
    alertUser(e.message,true);
  }
}
async function copyPath(path) {
  if (!path) return;
  try {
    await navigator.clipboard.writeText(path);
    alertUser('已複製資料夾路徑');
  } catch(e) {
    window.prompt('請複製這個資料夾路徑：', path);
  }
}
async function openFolder() {
  if (!selectedId) return;
  try {
    await send(`/servers/${encodeURIComponent(selectedId)}/open-folder`);
    alertUser('已在 Windows 主機呼叫檔案總管（遠端瀏覽器不會開啟本機資料夾）');
  } catch(e) { alertUser(e.message,true); }
}

function togglePasswordForm() {
  $('passwordForm').hidden = !$('passwordForm').hidden;
  if (!$('passwordForm').hidden) $('passwordForm').elements.namedItem('current_password').focus();
}
async function savePassword(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const data = new FormData(form);
  const current_password = String(data.get('current_password') || '');
  const new_password = String(data.get('new_password') || '');
  const confirm_password = String(data.get('confirm_password') || '');
  if (new_password !== confirm_password) return alertUser('兩次輸入的新密碼不一致', true);
  if (new_password.length < 12) return alertUser('新密碼至少需要 12 字元', true);
  const btn = form.querySelector('button[type=submit]');
  btn.disabled = true;
  try {
    const result = await send('/auth/change-password',{current_password,new_password});
    authToken = result.session;
    sessionStorage.setItem('mineos_token', authToken);
    form.reset();
    form.hidden = true;
    alertUser('管理密碼已更新，舊 Token 與其他登入 Session 已失效');
  } catch (e) { alertUser(e.message,true); }
  finally { btn.disabled = false; }
}



function fmtBytes(bytes) {
  if (bytes >= 1024 ** 3) return (bytes / 1024 ** 3).toFixed(2) + ' GB';
  if (bytes >= 1024 ** 2) return (bytes / 1024 ** 2).toFixed(1) + ' MB';
  return Math.round(bytes / 1024) + ' KB';
}
async function loadBackups(force = false) {
  if (!selectedId) return;
  const id = selectedId;
  if (force || backupCacheFor !== id || Date.now() - backupLastFetch > 6000) {
    const current = await request(`/servers/${encodeURIComponent(id)}/backups`);
    if (selectedId !== id) return;
    backupCache = current;
    backupCacheFor = id;
    backupLastFetch = Date.now();
  }
  renderBackups();
}
function renderBackups() {
  const srv = serverList.find(x => x.id === selectedId);
  if (!srv || !backupCache || backupCacheFor !== srv.id) return;
  const currentJob = jobs[`backup:${srv.id}`];
  const busy = !!currentJob && currentJob.state === 'working';
  const jobLabel = $('backupJob');
  jobLabel.textContent = currentJob ? `${currentJob.state === 'working' ? '處理中：' : currentJob.state === 'error' ? '失敗：' : '完成：'}${currentJob.detail}` : '';
  jobLabel.className = 'jobText ' + (currentJob?.state || '');
  $('backupNowBtn').disabled = busy;
  if (busy) { $('startBtn').disabled = true; $('stopBtn').disabled = true; $('restartBtn').disabled = true; $('editSettingsBtn').disabled = true; }
  $('backupScheduleForm').querySelector('button[type="submit"]').disabled = busy;
  $('backupPath').textContent = activeDataPath ? `${activeDataPath}\\backups\\${srv.id}` : `backups/${srv.id}`;
  if (!backupScheduleDirty) {
    $('backupInterval').value = backupCache.schedule.interval_hours;
    $('backupKeep').value = backupCache.schedule.keep_count;
  }
  const due = backupCache.schedule.next_due;
  $('backupNextDue').textContent = backupCache.schedule.interval_hours === 0 ? '自動備份未啟用' : `下次預計：${new Date(due * 1000).toLocaleString('zh-TW')}`;
  const host = $('backupList'); host.replaceChildren();
  if (!backupCache.backups.length) {
    host.append(make('div','hint','尚未建立備份。'));
    return;
  }
  for (const entry of backupCache.backups) {
    const item = make('div','backupItem');
    const info = make('div');
    info.append(make('div','backupName',`${entry.kind === 'auto' ? '排程備份' : '手動備份'} · ${new Date(entry.created_at * 1000).toLocaleString('zh-TW')}`));
    info.append(make('div','meta',`${fmtBytes(entry.bytes)} · ${entry.name}`));
    const controls = make('div','buttons');
    const download = make('button','','下載');
    download.onclick = () => downloadBackup(entry);
    const restore = make('button','','還原');
    restore.disabled = busy || srv.status !== 'stopped';
    restore.onclick = () => restoreBackup(entry);
    const del = make('button','danger','刪除');
    del.disabled = busy;
    del.onclick = () => deleteBackup(entry);
    controls.append(download,restore,del);
    item.append(info,controls);
    host.append(item);
  }
}
async function makeBackup() {
  if (!selectedId) return;
  const srv = serverList.find(x=>x.id===selectedId);
  if (!srv) return;
  if (!window.confirm(`要備份「${srv.name}」嗎？若正在執行，MineOS 會正常停止 Minecraft、備份後再啟動。備份期間玩家會斷線。`)) return;
  try {
    await send(`/servers/${encodeURIComponent(selectedId)}/backups`);
    alertUser('完整備份已開始；可在此查看進度');
    backupLastFetch = 0;
    await refresh();
  } catch(e) {alertUser(e.message,true);}
}
async function saveBackupSchedule(event) {
  event.preventDefault();
  if (!selectedId) return;
  const interval_hours = Number($('backupInterval').value);
  const keep_count = Number($('backupKeep').value);
  if (!Number.isInteger(interval_hours) || interval_hours < 0 || interval_hours > 720 ||
      !Number.isInteger(keep_count) || keep_count < 1 || keep_count > 50) return alertUser('間隔為 0–720 小時（0 關閉）；保留數量為 1–50',true);
  try {
    await send(`/servers/${encodeURIComponent(selectedId)}/backups/settings`,{interval_hours,keep_count});
    alertUser(interval_hours ? '自動備份排程已儲存' : '已關閉自動備份');
    backupScheduleDirty = false;
    await loadBackups(true);
  } catch(e) {alertUser(e.message,true);}
}
async function restoreBackup(entry) {
  const srv = serverList.find(x=>x.id===selectedId);
  if (!srv || srv.status !== 'stopped') return alertUser('請先停止 Server 才能還原',true);
  if (!window.confirm(`確定將「${srv.name}」還原到 ${new Date(entry.created_at * 1000).toLocaleString('zh-TW')} 的備份嗎？\n\n目前的世界與模組等檔案會被替換；原本資料會保存在 restore-safety 資料夾。還原後需手動啟動 Server。`)) return;
  try {
    await send(`/servers/${encodeURIComponent(srv.id)}/backups/${encodeURIComponent(entry.name)}/restore`);
    alertUser('還原工作已開始；完成後請檢查檔案再手動啟動');
    await refresh();
  } catch(e) {alertUser(e.message,true);}
}
async function deleteBackup(entry) {
  if (!selectedId || !window.confirm(`確定永久刪除 ${entry.name}？此操作不能復原。`)) return;
  try {
    await send(`/servers/${encodeURIComponent(selectedId)}/backups/${encodeURIComponent(entry.name)}/delete`);
    alertUser('備份檔已刪除');
    await loadBackups(true);
  } catch(e) {alertUser(e.message,true);}
}
async function downloadBackup(entry) {
  if (!selectedId) return;
  // Stream directly to a chosen file on supporting Chromium browsers, avoiding
  // buffering multi-GB Minecraft worlds in the browser's RAM.
  try {
    const supportsPicker = typeof window.showSaveFilePicker === 'function';
    let writable = null;
    if (supportsPicker) {
      const handle = await window.showSaveFilePicker({suggestedName:entry.name,types:[{description:'ZIP Archive',accept:{'application/zip':['.zip']}}]});
      writable = await handle.createWritable();
    } else if (entry.bytes > 512 * 1024 * 1024) {
      throw new Error('此瀏覽器不支援大型檔案串流儲存。請直接從主機的 backups 資料夾複製。');
    }
    const url = `/api/servers/${encodeURIComponent(selectedId)}/backups/${encodeURIComponent(entry.name)}/download`;
    const resp = await fetch(url,{headers:{Authorization:`Bearer ${authToken}`}});
    if (!resp.ok) { if (writable) await writable.abort(); throw new Error(await resp.text()); }
    if (writable) {
      await resp.body.pipeTo(writable);
    } else {
      const blob = await resp.blob();
      const objectUrl = URL.createObjectURL(blob);
      const link = document.createElement('a'); link.href = objectUrl; link.download = entry.name;
      document.body.append(link); link.click(); link.remove();
      setTimeout(()=>URL.revokeObjectURL(objectUrl),10000);
    }
    alertUser('備份檔已完成下載／儲存');
  } catch(e) {
    if (e.name !== 'AbortError') alertUser(e.message,true);
  }
}

$('loginBtn').onclick = login;
$('token').addEventListener('keydown', e => { if (e.key === 'Enter') login(); });
$('logoutBtn').onclick = async () => {
  const token = authToken;
  logout();
  if (token) { try { await fetch('/api/auth/logout',{method:'POST',headers:{Authorization:`Bearer ${token}`}}); } catch (_) {} }
};
$('changePasswordBtn').onclick = togglePasswordForm;
$('passwordForm').onsubmit = savePassword;
$('cancelPasswordBtn').onclick = () => { $('passwordForm').reset(); $('passwordForm').hidden = true; };
$('refreshBtn').onclick = () => refresh().catch(e=>alertUser(e.message,true));
$('createForm').onsubmit = createServer;
$('coreType').onchange = updateCoreType;
$('mcVersion').onchange = suggestJava;
$('javaRefreshBtn').onclick = () => refresh().catch(e=>alertUser(e.message,true));
$('retryCoreBtn').onclick = retryCore;
$('removeBtn').onclick = removeSelected;
$('editSettingsBtn').onclick = editSettings;
$('settingsForm').onsubmit = saveSettings;
$('cancelSettingsBtn').onclick = closeSettings;
$('openFolderBtn').onclick = openFolder;
$('copyFolderBtn').onclick = () => copyPath(serverList.find(x => x.id === selectedId)?.folder_path);
$('copyDataBtn').onclick = () => copyPath(activeDataPath);
$('backupNowBtn').onclick = makeBackup;
$('backupRefreshBtn').onclick = () => loadBackups(true).catch(e=>alertUser(e.message,true));
$('backupScheduleForm').onsubmit = saveBackupSchedule;
$('backupInterval').oninput = () => backupScheduleDirty = true;
$('backupKeep').oninput = () => backupScheduleDirty = true;
$('startBtn').onclick = () => performAction('start');
$('stopBtn').onclick = () => performAction('stop');
$('restartBtn').onclick = () => performAction('restart');
$('commandForm').onsubmit = sendConsole;
$('uploadForm').onsubmit = uploadJar;
$('eulaBtn').onclick = acceptEula;
if (authToken) {
  request('/servers').then(() => {
    showWorkspace(true);
    refresh().catch(e=>alertUser(e.message,true));
    loadSystemInfo().catch(e=>alertUser(e.message,true));
    updateCoreType();
    logInterval = setInterval(refreshQuiet,1500);
  }).catch(e => alertUser(e.message,true));
}
