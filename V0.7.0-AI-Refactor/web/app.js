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
let catalogEpoch = 0;
let buildEpoch = 0;


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
  mineStopHost();
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
    mineStartHost();
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
  $('serverInfo').textContent = srv.core_type === 'bedrock' ? `Bedrock  |  UDP ${srv.port}  |  ${srv.message || '狀態正常'}` : `Port: ${srv.port}  |  Java: ${srv.java_command}  |  RAM: ${srv.xms_mb}–${srv.xmx_mb} MB  |  ${srv.message || '狀態正常'}`;
  const job = jobs[`core:${srv.id}`];
  const core = $('coreStatus');
  core.textContent = job ? `核心下載：${job.detail}${job.percent == null ? '' : ` (${job.percent}%)`}` : (srv.core_type !== 'manual' ? `核心：${srv.core_type} ${srv.mc_version} ${srv.core_build || ""}` : '手動核心：上傳 server.jar');
  core.className = job ? `jobText ${job.state}` : 'hint';
  const files = await request(`/servers/${encodeURIComponent(srv.id)}/files`);
  const hasCore = !!srv.core_ready;
  $('retryCoreBtn').hidden = !(['vanilla','paper','fabric','forge','neoforge'].includes(srv.core_type) && srv.status === 'stopped' && (!job || job.state !== 'working'));
   $('bedrockUploadPanel').hidden = srv.core_type !== 'bedrock';
   $('bedrockUploadBtn').disabled = srv.status !== 'stopped' || hasCore || (job && job.state === 'working'); 
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
    core_build: String(data.get('core_build') || ''),
    auto_start: data.has('auto_start'),
    auto_restart: data.has('auto_restart'),
  };
  try {
    const s = await send('/servers',body);
    selectedId = s.id;
    backupCache = null;
    alertUser(body.core_type === 'manual' ? 'Server 已建立，請手動上傳核心 JAR 並確認 EULA' : body.core_type === 'bedrock' ? 'Bedrock 已建立，請匯入官方 ZIP 並確認 EULA' : 'Server 已建立，核心開始背景下載，請確認 Java 已安裝及 EULA');
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
  const isBedrock = core === 'bedrock';
  $('versionLabel').hidden = isManual || isBedrock;
  $('mcVersion').required = !isManual && !isBedrock;
  $('buildLabel').hidden = isManual || isBedrock || core === 'vanilla';
  for (const key of ['xms_mb','xmx_mb','java_command']) {
    const field = $('createForm').elements.namedItem(key);
    if (field) field.closest('label').hidden = isBedrock;
  }
  $('createAdvice').textContent = isBedrock ? '建立後上傳官方 Bedrock Dedicated Server ZIP；不需要 Java，使用 UDP Port。' : isManual ? '建立後上傳可直接啟動的 server.jar。' :
    ['forge','neoforge'].includes(core) ? 'Forge／NeoForge 使用官方 Installer；切換核心請另建伺服器。' : '從官方來源取得核心，可選擇歷史版本與 Build。';
  $('createForm').querySelector('button[type=submit]').textContent = isBedrock ? '建立 Bedrock Server' : isManual ? '建立 Server（手動上傳）' : '建立 Server 並下載核心';
  $('minecraftVersions').replaceChildren();
  $('mcVersion').value = isBedrock ? 'manual' : '';
  const port = $('createForm').elements.namedItem('port');
  if (port.value === '25565' && isBedrock) port.value = '19132';
  if (port.value === '19132' && !isBedrock) port.value = '25565';
  await loadBuilds();
  if (isManual || isBedrock) { $('versionHint').textContent = ''; return; }
  const epoch = ++catalogEpoch;
  $('versionHint').textContent = '正在載入完整歷史版本清單…';
  try {
    const result = await request(`/versions?core=${encodeURIComponent(core)}`);
    if (epoch !== catalogEpoch || $('coreType').value !== core) return;
    $('mcVersion').value = result.latest || '';
    for (const v of result.versions) { const opt = make('option'); opt.value = v; $('minecraftVersions').append(opt); }
    $('versionHint').textContent = `已載入 ${result.versions.length} 個官方版本，可輸入關鍵字搜尋。`;
    suggestJava();
    await loadBuilds();
  } catch(e) { $('versionHint').textContent = '官方清單讀取失敗：' + e.message; }
}
async function loadBuilds() {
  const core = $('coreType').value, version = $('mcVersion').value.trim(), epoch = ++buildEpoch;
  const select = $('coreBuild'); select.replaceChildren();
  const first = make('option', '', '自動選擇最新版可用 Build'); first.value = ''; select.append(first); select.value = '';
  if (!version || ['manual','bedrock','vanilla'].includes(core)) { $('buildHint').textContent = ''; return; }
  $('buildHint').textContent = '查詢官方 Build／Loader…';
  try {
    const data = await request(`/builds?core=${encodeURIComponent(core)}&version=${encodeURIComponent(version)}`);
    if (epoch !== buildEpoch || $('coreType').value !== core || $('mcVersion').value.trim() !== version) return;
    for (const item of data.builds) { const opt = make('option','',item.label); opt.value = item.id; select.append(opt); }
    $('buildHint').textContent = `共有 ${data.builds.length} 個可用 Build；不選則由程式自動挑選。`;
  } catch(e) { $('buildHint').textContent = 'Build 清單讀取失敗：' + e.message; }
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
  if (!s || ['manual','bedrock'].includes(s.core_type)) return;
  if (s.status !== 'stopped') return alertUser('請先停止伺服器',true);
  if (['forge','neoforge'].includes(s.core_type) && s.core_ready) return alertUser('Forge／NeoForge 不允許原地覆蓋，請新建伺服器',true);
  const catalog = await request(`/versions?core=${encodeURIComponent(s.core_type)}`);
  const version = window.prompt(`請輸入 Minecraft 版本（官方版本數：${catalog.versions.length}）`, s.mc_version || catalog.latest);
  if (version == null) return;
  if (!catalog.versions.includes(version)) return alertUser('版本不存在於官方清單',true);
  const builds = await request(`/builds?core=${encodeURIComponent(s.core_type)}&version=${encodeURIComponent(version)}`);
  const build = window.prompt(`選擇 Build／Loader（輸入版本字串，留空自動）：\n${builds.builds.slice(0,20).map(x=>x.label).join('\n')}${builds.builds.length>20?'\n...更多請至建立伺服器列表查看':''}`,s.core_build || builds.latest || '');
  if (build == null) return;
  if (build && !builds.builds.some(x => x.id === build)) return alertUser('無效的 Build／Loader',true);
  if (!window.confirm(s.core_ready ? '舊核心將保留為 server.jar.previous-*，世界不會清除。仍建議先執行完整備份。確定切換？' : '確認開始下載核心？')) return;
  try { await send(`/servers/${encodeURIComponent(s.id)}/install-core`,{core:s.core_type,version,build}); alertUser('已開始下載核心'); await refresh(); }
  catch(e) { alertUser(e.message,true); }
}
async function uploadBedrockZip() {
  const srv = serverList.find(x => x.id === selectedId);
  if (!srv || srv.core_type !== 'bedrock' || srv.status !== 'stopped') return alertUser('請先停止 Bedrock 伺服器',true);
  const file = $('bedrockZip').files[0];
  if (!file || !file.name.toLowerCase().endsWith('.zip')) return alertUser('請選擇官方 Windows BDS ZIP 檔',true);
  if (!window.confirm('請確認 ZIP 來自 Minecraft 官方下載頁。將安裝至這台伺服器的資料夾，已有的檔案不會被覆蓋。')) return;
  const form = new FormData(); form.append('file',file);
  try { await request(`/servers/${encodeURIComponent(srv.id)}/bedrock-zip`,{method:'POST',body:form}); $('bedrockZip').value=''; alertUser('Bedrock ZIP 已匯入'); await refresh(); }
  catch(e) { alertUser(e.message,true); }
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
$('mcVersion').onchange = () => { suggestJava(); loadBuilds(); };
$('mcVersion').addEventListener('input', () => { clearTimeout(window.mineosBuildDelay); window.mineosBuildDelay = setTimeout(loadBuilds,450); });
$('bedrockUploadBtn').onclick = uploadBedrockZip;
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

// MineOSRS V0.6.1 - host hardware monitor. Loaded as part of the existing app.js.
let mineHostTimer = null;
let mineHostBusy = false;
function mineBytes(bytes) {
  const n = Number(bytes);
  if (!Number.isFinite(n) || n < 0) return '—';
  return (n / (1024 ** 3)).toFixed(1) + ' GB';
}
function minePct(n) {
  n = Number(n);
  return Number.isFinite(n) ? Math.max(0, Math.min(100, n)) : 0;
}
function mineGauge(id, pct) {
  const bar = $(id);
  if (bar) bar.style.width = minePct(pct).toFixed(1) + '%';
}
async function mineRefreshHost() {
  if (!authToken || mineHostBusy || $('workspace').hidden) return;
  mineHostBusy = true;
  try {
    const h = await request('/hardware');
    if (h.error) throw new Error(h.error);
    $('mineHostStatus').textContent = '主機即時資訊 · 5 秒更新';
    $('mineHostCpuModel').textContent = h.cpu_model || '未知';
    $('mineHostCpuCores').textContent = `${h.physical_cores ?? '—'} 核心 / ${h.logical_threads ?? '—'} 執行緒`;
    $('mineHostCpuLoad').textContent = minePct(h.cpu_usage_percent).toFixed(1) + '%';
    mineGauge('mineHostCpuBar',h.cpu_usage_percent);
    const total = Number(h.total_memory_bytes) || 0;
    const used = Number(h.used_memory_bytes) || 0;
    const pct = total > 0 ? 100 * used / total : 0;
    $('mineHostRamAmount').textContent = `${mineBytes(used)} / ${mineBytes(total)}`;
    $('mineHostRamLoad').textContent = minePct(pct).toFixed(1) + '%';
    mineGauge('mineHostRamBar',pct);
    $('mineHostOs').textContent = h.os || '未知';
    const gpu = Array.isArray(h.gpus) && h.gpus.length ? h.gpus.join(' · ') : '無法辨識（請確認 WMI/CIM 服務）';
    $('mineHostGpu').textContent = gpu;
    const nvidia = Array.isArray(h.nvidia_metrics) ? h.nvidia_metrics : [];
    $('mineHostGpuStats').textContent = nvidia.length
      ? nvidia.map(g => `${g.name}: ${minePct(g.usage_percent).toFixed(0)}% · VRAM ${(Number(g.vram_used_mib)/1024).toFixed(1)} / ${(Number(g.vram_total_mib)/1024).toFixed(1)} GB`).join(' / ')
      : 'GPU 使用率／VRAM：此裝置未提供 NVIDIA-SMI 資料';
    const host = $('mineHostDisks'); host.replaceChildren();
    if (!Array.isArray(h.disks) || !h.disks.length) {
      host.append(make('div','mine-host-disk','沒有可用的磁碟資料'));
    } else {
      for (const d of h.disks) {
        const wrap = make('div','mine-host-disk');
        const usedBytes = Math.max(0, Number(d.total_bytes) - Number(d.free_bytes));
        const percentage = Number(d.total_bytes) > 0 ? 100 * usedBytes / Number(d.total_bytes) : 0;
        wrap.append(make('span','',`${d.mount || d.name || '磁碟'}：${mineBytes(usedBytes)} / ${mineBytes(d.total_bytes)}（剩餘 ${mineBytes(d.free_bytes)}）`));
        const track = make('div','mine-host-track');
        const bar = make('span','mine-host-bar'); bar.style.width = minePct(percentage).toFixed(1) + '%';
        track.append(bar); wrap.append(track); host.append(wrap);
      }
    }
  } catch (e) {
    if ($('mineHostStatus')) $('mineHostStatus').textContent = '主機資訊暫時無法取得：' + e.message;
  } finally { mineHostBusy = false; }
}
function mineStartHost() {
  if (!mineHostTimer) mineHostTimer = setInterval(mineRefreshHost, 5000);
  mineRefreshHost();
}
function mineStopHost() {
  if (mineHostTimer) clearInterval(mineHostTimer);
  mineHostTimer = null;
}

/* MineOSRS V0.6.1 — Unified Server create/edit form.
 * Uses the existing settingsForm submission handler and API. No database changes.
 * Loaded after app.js's original logic; registered in capture phase to prevent
 * accidental new-server creation while editing.
 */
(() => {
  'use strict';
  const get = (id) => document.getElementById(id);
  const form = get('createForm');
  const original = get('settingsForm');
  const editButton = get('editSettingsBtn');
  const cancelButton = get('miCancelEditBtn');
  const banner = get('miEditNotice');
  const title = get('miFormTitle');
  const editCard = form && form.closest('article');
  const submitButton = form && form.querySelector('button[type="submit"]');
  let editing = null;
  let beforeEdit = null;

  if (!form || !original || !editButton || !cancelButton || !banner || !title || !submitButton || !editCard) {
    console.warn('[MineOSRS] 統一編輯表單：原始介面不完整，已停用。');
    return;
  }

  const field = (name, root = form) => root.elements.namedItem(name);
  const allNames = ['name', 'port', 'xms_mb', 'xmx_mb', 'java_command', 'auto_start', 'auto_restart'];
  const currentServer = () => {
    try { return (serverList || []).find(s => s.id === selectedId); }
    catch (_) { return null; }
  };
  const notify = (message, isError = false) => {
    if (typeof alertUser === 'function') alertUser(message, isError);
    else window.alert(message);
  };

  function snapshot() {
    return {
      heading: title.textContent,
      submit: submitButton.textContent,
      values: [...form.elements].filter(el => el.name).map(el => ({
        el, value: el.value, checked: el.checked,
        disabled: el.disabled, readOnly: el.readOnly, required: el.required,
        labelHidden: el.closest('label') ? el.closest('label').hidden : null,
      })),
      versionLabelHidden: !!get('versionLabel')?.hidden,
      buildLabelHidden: !!get('buildLabel')?.hidden,
    };
  }

  function setEditVisibility(core) {
    const isBedrock = core === 'bedrock';
    const isManual = core === 'manual';
    if (get('versionLabel')) get('versionLabel').hidden = isManual || isBedrock;
    if (get('buildLabel')) get('buildLabel').hidden = isManual || isBedrock || core === 'vanilla';
    for (const name of ['xms_mb', 'xmx_mb', 'java_command']) {
      const el = field(name);
      const label = el && el.closest('label');
      if (label) label.hidden = isBedrock;
    }
  }

  function restoreCreateMode() {
    if (!editing) return;
    editing = null;
    editCard.classList.remove('mi-editing-card');
    banner.hidden = true;
    banner.textContent = '';
    cancelButton.hidden = true;
    title.textContent = beforeEdit?.heading || '新增伺服器';
    submitButton.textContent = beforeEdit?.submit || '建立 Server 並下載核心';
    for (const item of beforeEdit?.values || []) {
      item.el.disabled = item.disabled;
      item.el.readOnly = item.readOnly;
      item.el.required = item.required;
      item.el.value = item.value;
      if ('checked' in item.el) item.el.checked = item.checked;
      item.el.removeAttribute('data-mi-locked');
      const label = item.el.closest('label');
      if (label && item.labelHidden !== null) label.hidden = item.labelHidden;
    }
    if (get('versionLabel')) get('versionLabel').hidden = beforeEdit?.versionLabelHidden || false;
    if (get('buildLabel')) get('buildLabel').hidden = beforeEdit?.buildLabelHidden || false;
    beforeEdit = null;
    // Existing form remains in its pre-edit state (including user-entered drafts).
  }

  function openEditMode() {
    const srv = currentServer();
    if (!srv) return notify('請先選擇要修改的伺服器', true);
    if (srv.status !== 'stopped') return notify('修改伺服器設定前，請先停止該伺服器', true);
    if (editing) restoreCreateMode();
    beforeEdit = snapshot();
    editing = { id: srv.id, name: srv.name };

    // Invalidate any outstanding version/build requests from original create UI.
    try { if (typeof catalogEpoch === 'number') ++catalogEpoch; } catch (_) {}
    try { if (typeof buildEpoch === 'number') ++buildEpoch; } catch (_) {}

    for (const name of allNames) {
      const el = field(name);
      if (!el) { restoreCreateMode(); return notify(`新增表單缺少欄位：${name}`, true); }
      if (el.type === 'checkbox') el.checked = !!srv[name];
      else el.value = srv[name] ?? '';
    }
    const core = field('core_type');
    if (core) { core.value = srv.core_type || 'manual'; core.disabled = true; core.dataset.miLocked = 'true'; }
    const version = field('mc_version');
    if (version) { version.value = srv.mc_version || ''; version.readOnly = true; version.required = false; version.dataset.miLocked = 'true'; }
    const build = field('core_build');
    if (build) {
      const selected = srv.core_build || '';
      if (selected && ![...build.options].some(opt => opt.value === selected)) {
        build.add(new Option(`目前 Build：${selected}`, selected));
      }
      build.value = selected;
      build.disabled = true;
      build.dataset.miLocked = 'true';
    }
    setEditVisibility(srv.core_type || 'manual');
    original.hidden = true;
    original.style.display = 'none';
    title.textContent = `修改伺服器：${srv.name}`;
    submitButton.textContent = '儲存修改';
    banner.textContent = `目前正在修改「${srv.name}」的設定。橘色邊框代表修改模式；只會更新這台 Server，不會新增第二台。核心與 Minecraft 版本需使用專用的核心管理功能更換。`;
    banner.hidden = false;
    cancelButton.hidden = false;
    editCard.classList.add('mi-editing-card');
    editCard.scrollIntoView({ behavior: 'smooth', block: 'start' });
    field('name')?.focus({ preventScroll: true });
  }

  function saveEditMode() {
    if (!editing) return;
    const srv = currentServer();
    if (!srv || srv.id !== editing.id) return notify('目前選取的伺服器已變更。請取消修改後重新選取。', true);
    if (srv.status !== 'stopped') return notify('請先停止伺服器才能儲存修改', true);
    if (!form.reportValidity()) return;
    if (!window.confirm(`確定儲存「${editing.name}」的設定修改？\n此操作不會下載核心、重建伺服器或刪除世界。`)) return;

    for (const name of allNames) {
      const from = field(name);
      const to = field(name, original);
      if (!from || !to) return notify(`原本設定表單缺少欄位：${name}`, true);
      if (to.type === 'checkbox') to.checked = from.checked;
      else to.value = from.value;
    }
    if (!original.checkValidity()) return notify('設定值格式不正確，請檢查 Port 或記憶體範圍', true);
    // Reuse V0.6.1's existing settingsForm submit handler, including validation,
    // endpoint method, error notices and refresh behavior.
    try {
      original.requestSubmit(get('saveSettingsBtn') || undefined);
      banner.textContent = `已送出「${editing.name}」的修改要求；請以系統顯示的儲存結果為準。完成後按「取消修改／返回新增」即可退出修改模式。`;
    } catch (error) {
      notify(`送出修改失敗：${error.message}`, true);
    }
  }

  // Capture first, before the original onclick/onsubmit callbacks run.
  editButton.addEventListener('click', event => {
    event.preventDefault(); event.stopImmediatePropagation();
    openEditMode();
  }, true);
  form.addEventListener('submit', event => {
    if (!editing) return; // Original "create" submission works normally.
    event.preventDefault(); event.stopImmediatePropagation();
    saveEditMode();
  }, true);
  cancelButton.addEventListener('click', event => {
    event.preventDefault(); restoreCreateMode();
  });

  // Changing the selected server must not silently re-target the edit form.
  const servers = get('servers');
  if (servers) servers.addEventListener('click', () => {
    if (!editing) return;
    setTimeout(() => {
      if (editing && currentServer()?.id !== editing.id) restoreCreateMode();
    }, 0);
  });
})();

