
/* MineOSRS V0.7 — DOM-only UI shell; existing backend endpoints and handlers preserved. */
(() => {
  'use strict';
  const ID = id => document.getElementById(id);
  const THEME_KEY = 'mineosrs-v07-theme';
  const pages = {
    host: ['本機資訊', 'Windows 主機硬體與資源使用概況'],
    servers: ['伺服器清單', '選取要管理的 Minecraft 伺服器'],
    create: ['新增／修改伺服器', '建立伺服器，或使用相同表單修改現有設定'],
    java: ['Java 環境', '下載並管理伺服器使用的 Java 版本'],
    detail: ['伺服器資訊', '啟動、停止、變更設定與查看狀態'],
    console: ['Console', '查看伺服器日誌並傳送指令'],
    backups: ['備份與還原', '建立、排程、下載與還原伺服器備份'],
    files: ['核心／插件／模組', '上傳 JAR 與查看伺服器檔案'],
    settings: ['管理器設定', '登入密碼與資料儲存位置']
  };
  let current = 'host';
  let serverDetail = null;
  let shell = null;
  let changedByUser = false;

  function element(tag, cls, content) {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (content !== undefined) node.textContent = content;
    return node;
  }
  function getTheme() {
    try { return localStorage.getItem(THEME_KEY) === 'dark' ? 'dark' : 'classic'; }
    catch (_) { return 'classic'; }
  }
  function setTheme(theme) {
    const selected = theme === 'dark' ? 'dark' : 'classic';
    document.documentElement.setAttribute('data-mi07-theme', selected);
    try { localStorage.setItem(THEME_KEY, selected); } catch (_) {}
    const button = ID('mi07Theme');
    if (button) {
      button.textContent = selected === 'dark' ? '☀ 經典配色' : '☾ 黑色配色';
      button.setAttribute('aria-pressed', String(selected === 'dark'));
      button.title = selected === 'dark' ? '切換回 MineOS 經典配色' : '切換成黑色配色';
    }
  }
  function attachCard(container, node) {
    if (node && container && node !== container && !container.contains(node)) container.appendChild(node);
  }
  function topLevelDetailSections(detail) {
    if (!detail) return;
    const sectionFrom = (name, nodes) => {
      const valid = nodes.filter(Boolean).filter(n => n.parentElement === detail);
      if (!valid.length) return null;
      const group = element('section', 'mi07-detail-group');
      group.setAttribute('data-mi07-only', name);
      detail.insertBefore(group, valid[0]);
      valid.forEach(n => group.appendChild(n));
      return group;
    };
    const backup = ID('backupSection');
    if (backup && backup.parentElement === detail) backup.setAttribute('data-mi07-only', 'backups');
    const consoleEl = ID('console');
    const commandForm = ID('commandForm');
    if (consoleEl && consoleEl.parentElement === detail) {
      const previous = consoleEl.previousElementSibling;
      sectionFrom('console', [previous && /^H[2-4]$/.test(previous.tagName) ? previous : null, consoleEl, commandForm]);
    }
    const uploadForm = ID('uploadForm');
    if (uploadForm && uploadForm.parentElement === detail) {
      let begin = uploadForm.previousElementSibling;
      if (!begin || !/^H[2-4]$/.test(begin.tagName)) begin = uploadForm;
      const all = [];
      for (let item = begin; item; item = item.nextElementSibling) all.push(item);
      sectionFrom('files', all);
    }
    for (const child of Array.from(detail.children)) {
      if (!child.hasAttribute('data-mi07-only')) child.setAttribute('data-mi07-only', 'detail');
    }
  }
  function updateDetailView(view) {
    const detail = serverDetail;
    if (!detail) return;
    for (const item of Array.from(detail.children)) {
      const only = item.getAttribute('data-mi07-only');
      item.classList.toggle('mi07-view-hidden', !!only && only !== view);
    }
  }
  function serversAvailable() {
    const selected = ID('selectedName');
    return selected && selected.textContent.trim() !== '—' && selected.textContent.trim() !== '';
  }
  function updateStatus() {
    const name = ID('selectedName')?.textContent.trim() || '—';
    const srv = ID('mi07ServerName');
    if (srv) srv.textContent = name && name !== '—' ? name : '尚未選取';
    const noSrv = ID('mi07NoServer');
    if (noSrv) noSrv.hidden = !!serversAvailable();
    const stats = ID('mi07ServerCount');
    const running = ID('mi07ServerRunning');
    try {
      // Global lexical serverList is maintained by MineOSRS's existing app.js.
      if (typeof serverList !== 'undefined' && Array.isArray(serverList)) {
        if (stats) stats.textContent = String(serverList.length);
        if (running) running.textContent = String(serverList.filter(s => s.status === 'running' || s.running === true).length);
      }
    } catch (_) { /* Keep the last verified data; never guess. */ }
    document.body.classList.toggle('mi07-logged-in', !!ID('workspace') && !ID('workspace').hidden);
  }
  function show(page, userInitiated = true) {
    if (!pages[page]) page = 'host';
    current = page;
    if (userInitiated) changedByUser = true;
    for (const pane of shell.querySelectorAll('.mi07-page')) pane.hidden = pane.dataset.mi07Page !== (['detail','console','backups','files'].includes(page) ? 'detail' : page);
    const tab = ['detail','console','backups','files'].includes(page) ? page : 'detail';
    updateDetailView(tab);
    shell.querySelectorAll('.mi07-nav button[data-mi07-target]').forEach(btn => {
      const on = btn.dataset.mi07Target === page;
      btn.classList.toggle('mi07-active', on);
      if (on) btn.setAttribute('aria-current', 'page'); else btn.removeAttribute('aria-current');
    });
    ID('mi07Title').textContent = pages[page][0];
    ID('mi07Breadcrumb').textContent = (['detail','console','backups','files'].includes(page) ? '伺服器管理 / ' : '主機管理 / ') + pages[page][0];
    shell.classList.remove('mi07-menu-open');
    updateStatus();
    if (userInitiated) window.scrollTo({top:0,behavior:'instant'});
  }
  function navBtn(id, text, ico) {
    const btn = element('button');
    btn.type = 'button'; btn.dataset.mi07Target = id;
    btn.innerHTML = '<span class="mi07-ico" aria-hidden="true"></span><span></span>';
    btn.children[0].textContent = ico;
    btn.children[1].textContent = text;
    btn.addEventListener('click', () => show(id));
    return btn;
  }
  function sectionTitle(title, description) {
    const head = element('div','mi07-page-title');
    const info = element('div');
    info.append(element('h2','',title),element('p','',description));
    head.append(info);
    return head;
  }
  function makePage(parent,id,title,description) {
    const section = element('section','mi07-page'); section.dataset.mi07Page = id;
    section.append(sectionTitle(title,description)); parent.appendChild(section);
    return section;
  }
  function makeInfoCard(value,label) {
    const card=element('div','mi07-stat');
    card.append(element('div','mi07-stat-label',label));
    const amount=element('div','mi07-stat-value','—'); amount.id=value;
    card.append(amount); return card;
  }
  function initialize() {
    const workspace=ID('workspace');
    const form=ID('createForm'), list=ID('servers'), detail=ID('detail');
    if (!workspace || !form || !list || !detail) {
      console.warn('[MineOSRS V0.7] 找不到舊介面關鍵元素，已保留 V0.6.1 介面。');
      return;
    }
    if (ID('mi07Shell')) return;
    const originalHeadline = Array.from(workspace.children).find(x=>x.classList?.contains('headline'));
    const hardware = ID('mineHostPanel');
    const createCard = form.closest('article');
    const listCard = list.closest('article');
    const detailCard = detail.closest('article');
    const javaCard = ID('javaList')?.closest('article') || workspace.querySelector('.javaManager');
    if (!createCard || !listCard || !detailCard) {
      console.warn('[MineOSRS V0.7] 沒有找到舊介面完整卡片，已保留原本配置。');
      return;
    }
    topLevelDetailSections(detail);
    serverDetail = detail;
    shell = element('div'); shell.id='mi07Shell';
    const side=element('aside','mi07-side'); side.setAttribute('aria-label','主功能選單');
    const brand=element('div','mi07-logo');
    brand.innerHTML='<span class="mi07-logo-icon" aria-hidden="true">◆</span><span>MineOSRS <span class="mi07-version">V0.7</span></span>';
    side.append(brand);
    const appendGroup=(title,entries)=>{
      side.append(element('div','mi07-group',title));
      const nav=element('nav','mi07-nav');
      entries.forEach(e=>nav.appendChild(navBtn(...e)));
      side.append(nav);
    };
    appendGroup('主機管理', [['host','本機資訊','▣'],['servers','伺服器清單','▤'],['create','新增／修改伺服器','＋'],['java','Java 環境','☕']]);
    appendGroup('選取的伺服器', [['detail','伺服器資訊','◎'],['console','Console','⌘'],['backups','備份與還原','▥'],['files','核心／插件／模組','⬡']]);
    appendGroup('系統', [['settings','管理器設定','⚙']]);
    const sideInfo=element('div','mi07-side-info');
    sideInfo.append('目前伺服器：');
    const name=element('strong'); name.id='mi07ServerName'; name.textContent='尚未選取'; sideInfo.append(name);
    side.append(sideInfo);
    const shade=element('button','mi07-mobile-shade'); shade.type='button';
    shade.setAttribute('aria-label','關閉功能選單'); shade.addEventListener('click',()=>shell.classList.remove('mi07-menu-open'));

    const main=element('div','mi07-main');
    const top=element('div','mi07-topbar');
    const left=element('div');
    const menu=element('button','mi07-menu-btn','☰'); menu.type='button'; menu.setAttribute('aria-label','展開功能選單');
    menu.addEventListener('click',()=>shell.classList.toggle('mi07-menu-open'));
    left.append(menu);
    const bread=element('div','mi07-crumb'); bread.id='mi07Breadcrumb'; left.append(bread);
    const title=element('h1'); title.id='mi07Title'; left.append(title);
    const tools=element('div','mi07-toptools');
    const theme=element('button','mi07-theme-btn'); theme.type='button'; theme.id='mi07Theme';
    theme.addEventListener('click',()=>setTheme(document.documentElement.getAttribute('data-mi07-theme')==='dark'?'classic':'dark'));
    tools.append(theme);
    if (originalHeadline) { originalHeadline.classList.add('mi07-original-headline'); tools.append(originalHeadline); }
    top.append(left,tools); main.append(top);
    const content=element('div','mi07-pages'); main.append(content);
    const hostPage=makePage(content,'host',...pages.host);
    const stats=element('div','mi07-statbar');
    stats.append(makeInfoCard('mi07ServerCount','已建立的伺服器'),makeInfoCard('mi07ServerRunning','執行中的伺服器'));
    hostPage.append(stats);
    if (hardware) attachCard(hostPage,hardware);
    else hostPage.append(element('div','mi07-blank','本機硬體資訊功能未找到，請確認來源是已安裝硬體監控的 V0.6.1。'));
    const hostLink=element('button','mi07-quicklink','查看所有伺服器 →'); hostLink.type='button'; hostLink.addEventListener('click',()=>show('servers')); hostPage.append(hostLink);
    const serverPage=makePage(content,'servers',...pages.servers); attachCard(serverPage,listCard);
    const createPage=makePage(content,'create',...pages.create); attachCard(createPage,createCard);
    const javaPage=makePage(content,'java',...pages.java);
    if (javaCard) attachCard(javaPage,javaCard);
    else javaPage.append(element('div','mi07-blank','Java 管理功能位於你目前版本的其他位置，請檢查來源。'));
    const detailPage=makePage(content,'detail',...pages.detail);
    const missing=element('div','mi07-blank','請先從「伺服器清單」選取一台伺服器。'); missing.id='mi07NoServer'; detailPage.append(missing);
    const wrap=element('div','mi07-detail-wrapper'); wrap.append(detailCard); detailPage.append(wrap);
    const settingsPage=makePage(content,'settings',...pages.settings);
    const passwordForm=ID('passwordForm');
    if (passwordForm) attachCard(settingsPage,passwordForm);
    const dataLocation=workspace.querySelector('.dataLocation');
    if (dataLocation) attachCard(settingsPage,dataLocation);
    // Preserve any unknown panels rather than silently dropping them.
    const legacyLayout=Array.from(workspace.children).find(x=>x.classList?.contains('layout'));
    if (legacyLayout && legacyLayout.querySelector('article, form, section:not(.stack)')) {
      const other=element('div','card');
      other.append(element('h3','', '其他功能'));
      attachCard(other,legacyLayout); attachCard(settingsPage,other);
    } else if (legacyLayout) legacyLayout.remove();
    // Extra top-level panels introduced by user plugins remain available in Settings.
    for (const node of Array.from(workspace.children)) {
      if (node === originalHeadline || node === shell) continue;
      if (node.id === 'mi07Shell' || node.id === 'mineHostPanel') continue;
      if (node === hardware || node === javaCard || node === passwordForm || node === dataLocation) continue;
      if (node.matches('script,style')) continue;
      if (node.nodeType === 1) settingsPage.appendChild(node);
    }
    shell.append(side,shade,main); workspace.prepend(shell);

    // Go to the actual server selected via the original Server list.
    list.addEventListener('click',()=>{
      let before = null;
      try { if (typeof selectedId !== 'undefined') before=selectedId; } catch (_) {}
      setTimeout(()=>{
        let after=null;
        try { if (typeof selectedId !== 'undefined') after=selectedId; } catch (_) {}
        if (after != null && (after !== before || current === 'servers')) show('detail');
      },30);
    });
    // The unified V0.6.1 editor performs its own safe update handler.
    // Unified Editor stops event propagation at the target during capture.
    // Listen on document capture (which runs first) and navigate only if edit mode really opened.
    document.addEventListener('click',(event)=>{
      if (!event.target.closest?.('#editSettingsBtn')) return;
      setTimeout(()=>{
        const banner=ID('miEditNotice');
        if (banner && !banner.hidden) show('create');
      },0);
    },true);
    ID('changePasswordBtn')?.addEventListener('click',()=>setTimeout(()=>show('settings'),0));
    const selected=ID('selectedName');
    if (selected) new MutationObserver(updateStatus).observe(selected,{childList:true,subtree:true,characterData:true});
    // The application already polls server status. Refreshing the summary does not trigger API calls.
    setInterval(updateStatus,5000);
    const authObserver=new MutationObserver(updateStatus);
    authObserver.observe(workspace,{attributes:true,attributeFilter:['hidden']});
    setTheme(getTheme());
    show('host',false);
  }
  if (document.readyState==='loading') document.addEventListener('DOMContentLoaded',initialize,{once:true});
  else initialize();
})();

