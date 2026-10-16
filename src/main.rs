mod downloads;
mod backups;

use std::{
    collections::{HashMap, VecDeque},
    env, fs,
    net::{SocketAddr, TcpListener as StdTcpListener},
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex, atomic::{AtomicBool, Ordering}},
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path as AxPath, Query, State},
    http::{header, HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, Connection};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::SaltString;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, RwLock},
    time,
};
use tracing::{info, warn};
use uuid::Uuid;

const MAX_LOG_LINES: usize = 1200;
const MAX_UPLOAD_BYTES: usize = 512 * 1024 * 1024;

type ApiResult<T> = Result<T, (StatusCode, String)>;

#[derive(Clone)]
struct AppState {
    db: Arc<StdMutex<Connection>>,
    instances: Arc<RwLock<HashMap<String, Arc<Instance>>>>,
    token: Arc<String>,
    password_set: Arc<AtomicBool>,
    sessions: Arc<StdMutex<HashMap<String, Instant>>>,
    login_attempts: Arc<StdMutex<(u32, Instant)>>,
    data_dir: Arc<PathBuf>,
    jobs: Arc<RwLock<HashMap<String, downloads::Job>>>,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServerConfig {
    id: String,
    name: String,
    port: u16,
    xms_mb: u32,
    xmx_mb: u32,
    java_command: String,
    auto_start: bool,
    auto_restart: bool,
    #[serde(default = "default_core")]
    core_type: String,
    #[serde(default)]
    mc_version: String,
}

// Edits to an existing Server never change the core or its Minecraft version.
#[derive(Debug, Deserialize)]
struct UpdateServer {
    name: String,
    port: u16,
    xms_mb: u32,
    xmx_mb: u32,
    java_command: String,
    auto_start: bool,
    auto_restart: bool,
}

#[derive(Debug, Deserialize)]
struct NewServer {
    name: String,
    port: u16,
    xms_mb: u32,
    xmx_mb: u32,
    #[serde(default = "default_java")]
    java_command: String,
    #[serde(default)]
    auto_start: bool,
    #[serde(default)]
    auto_restart: bool,
    #[serde(default = "default_core")]
    core_type: String,
    #[serde(default)]
    mc_version: String,
}

fn default_core() -> String { "manual".into() }

fn default_java() -> String {
    "java".into()
}

#[derive(Serialize)]
struct ServerView {
    #[serde(flatten)]
    config: ServerConfig,
    status: &'static str,
    message: Option<String>,
    eula_accepted: bool,
    core_ready: bool,
    folder_path: String,
}

struct Instance {
    config: RwLock<ServerConfig>,
    process: Mutex<ProcessState>,
    logs: Mutex<VecDeque<String>>,
    maintenance: AtomicBool,
}

struct ProcessState {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    status: &'static str,
    requested_stop: bool,
    pending_restart: bool,
    starts_in_window: VecDeque<Instant>,
    message: Option<String>,
}

impl Instance {
    fn new(config: ServerConfig) -> Self {
        Self {
            config: RwLock::new(config),
            process: Mutex::new(ProcessState {
                child: None,
                stdin: None,
                status: "stopped",
                requested_stop: false,
                pending_restart: false,
                starts_in_window: VecDeque::new(),
                message: None,
            }),
            logs: Mutex::new(VecDeque::new()),
            maintenance: AtomicBool::new(false),
        }
    }

    async fn log(&self, line: impl Into<String>) {
        let mut logs = self.logs.lock().await;
        logs.push_back(line.into());
        while logs.len() > MAX_LOG_LINES {
            logs.pop_front();
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    // One shared directory for Debug and Release under the same Windows user.
    // A future Windows Service will require an explicitly configured data directory.
    // NEVER silently move an existing world's data: see README migration steps.
    let home = env::var_os("MINEOS_DATA_DIR").filter(|v| !v.is_empty())
        .map(PathBuf::from)
        // Per-user directory avoids making the admin token readable through
        // inherited ProgramData ACLs in this non-Service prototype.
        .or_else(|| env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())
            .map(|v| PathBuf::from(v).join("MineOS-Win")))
        .unwrap_or_else(|| {
            env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from(".")).join("data")
        });
    fs::create_dir_all(home.join("servers"))?;
    let token_file = home.join("admin-token.txt");
    let token = match env::var("MINEOS_ADMIN_TOKEN") {
        Ok(token) if token.trim().len() >= 24 => token.trim().to_owned(),
        Ok(_) => return Err("MINEOS_ADMIN_TOKEN 必須至少 24 個字元".into()),
        Err(_) => {
            if token_file.exists() {
                fs::read_to_string(&token_file)?.trim().to_owned()
            } else {
                let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
                fs::write(&token_file, format!("{token}\n"))?;
                info!("首次啟動：管理 Token 已建立於 {}", token_file.display());
                token
            }
        }
    };
    if token.len() < 24 {
        return Err("管理 Token 長度不足；請修正 data/admin-token.txt".into());
    }

    let conn = Connection::open(home.join("mineos.db"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS servers (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            port INTEGER NOT NULL UNIQUE,
            xms_mb INTEGER NOT NULL,
            xmx_mb INTEGER NOT NULL,
            java_command TEXT NOT NULL,
            auto_start INTEGER NOT NULL DEFAULT 0,
            auto_restart INTEGER NOT NULL DEFAULT 0
        );",
    )?;
    // In-place schema migration: preserves all V0.1 server definitions and worlds.
    for (name, declaration) in [
        ("core_type", "TEXT NOT NULL DEFAULT 'manual'"),
        ("mc_version", "TEXT NOT NULL DEFAULT ''"),
    ] {
        let mut stmt = conn.prepare("PRAGMA table_info(servers)")?;
        let cols: Vec<String> = stmt.query_map([], |r| r.get(1))?.collect::<Result<_, _>>()?;
        if !cols.iter().any(|c| c == name) {
            conn.execute(&format!("ALTER TABLE servers ADD COLUMN {name} {declaration}"), [])?;
        }
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS backup_schedules (server_id TEXT PRIMARY KEY, interval_hours INTEGER NOT NULL DEFAULT 0, keep_count INTEGER NOT NULL DEFAULT 5, next_due INTEGER NOT NULL DEFAULT 0);")?;
    let password_set: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM app_settings WHERE key='admin_password')", [], |r| r.get(0))?;
    let configs = {
        let mut stmt = conn.prepare(
            "SELECT id,name,port,xms_mb,xmx_mb,java_command,auto_start,auto_restart,core_type,mc_version FROM servers ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ServerConfig {
                id: r.get(0)?,
                name: r.get(1)?,
                port: r.get(2)?,
                xms_mb: r.get(3)?,
                xmx_mb: r.get(4)?,
                java_command: r.get(5)?,
                auto_start: r.get(6)?,
                auto_restart: r.get(7)?,
                core_type: r.get(8)?,
                mc_version: r.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut instances = HashMap::new();
    let mut autostart = Vec::new();
    for cfg in configs {
        if cfg.auto_start {
            autostart.push(cfg.id.clone());
        }
        instances.insert(cfg.id.clone(), Arc::new(Instance::new(cfg)));
    }

    let state = AppState {
        db: Arc::new(StdMutex::new(conn)),
        instances: Arc::new(RwLock::new(instances)),
        token: Arc::new(token),
        password_set: Arc::new(AtomicBool::new(password_set)),
        sessions: Arc::new(StdMutex::new(HashMap::new())),
        login_attempts: Arc::new(StdMutex::new((0, Instant::now()))),
        data_dir: Arc::new(home),
        jobs: Arc::new(RwLock::new(HashMap::new())),
        http: reqwest::Client::builder()
            .user_agent("MineOS-Win/0.2 (https://github.com/ddmmbb-2)")
            .connect_timeout(Duration::from_secs(20))
            .build()?,
    };

    let api = Router::new()
        .route("/auth/logout", post(logout_admin))
        .route("/servers", get(list_servers).post(create_server))
        .route("/java", get(downloads::list_java))
        .route("/java/install", post(downloads::install_java))
        .route("/jobs", get(downloads::list_jobs))
        .route("/versions", get(downloads::versions))
        .route("/servers/{id}/install-core", post(downloads::install_core))
        .route("/servers/{id}/remove", post(remove_server))
        .route("/servers/{id}/settings", post(update_server_settings))
        .route("/servers/{id}/open-folder", post(open_server_folder))
        .route("/system", get(system_info))
        .route("/servers/{id}/start", post(start_server))
        .route("/servers/{id}/stop", post(stop_server))
        .route("/servers/{id}/restart", post(restart_server))
        .route("/servers/{id}/console", post(console_command))
        .route("/servers/{id}/logs", get(server_logs))
        .route("/servers/{id}/upload", post(upload_file))
        .route("/servers/{id}/files", get(list_files))
        .route("/servers/{id}/eula", post(accept_eula))
        .route("/servers/{id}/backups", get(backups::list_backups).post(backups::make_backup))
        .route("/servers/{id}/backups/settings", post(backups::save_schedule))
        .route("/servers/{id}/backups/{name}/restore", post(backups::restore_backup))
        .route("/servers/{id}/backups/{name}/delete", post(backups::delete_backup))
        .route("/servers/{id}/backups/{name}/download", get(backups::download_backup))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES + 4096))
        .route_layer(middleware::from_fn_with_state(state.clone(), check_token));

    // Login is public, password changes require authenticated bearer sessions.
    let app = Router::new()
        .route("/api/auth/login", post(login_admin))
        .route("/api/auth/change-password", post(change_password).route_layer(middleware::from_fn_with_state(state.clone(), check_token)))
        .route("/", get(index_html))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .nest("/api", api)
        .with_state(state.clone());

    tokio::spawn(watch_processes(state.clone()));
    tokio::spawn(backups::run_scheduler(state.clone()));
    for id in autostart {
        let state = state.clone();
        tokio::spawn(async move {
            if let Err((_, message)) = start_by_id(&state, &id).await {
                warn!(server = %id, error = %message, "自動啟動失敗");
            }
        });
    }

    let bind = env::var("MINEOS_BIND").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let address: SocketAddr = bind.parse()?;
    if !address.ip().is_loopback() {
        warn!("WebUI 尚未內建 HTTPS。若要遠端使用，務必設定 HTTPS 反向代理及網路存取限制。");
    }
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!("MineOS-Win WebUI: http://{}", address);
    info!("首次登入 Token 存放於：{}（設定密碼後 Token 不再可登入）", token_file.display());
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Deserialize)]
struct LoginRequest { password: String }
#[derive(Deserialize)]
struct ChangePasswordRequest { current_password: String, new_password: String }

fn read_admin_hash(state: &AppState) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|_| "資料庫鎖定失敗".to_owned())?;
    match db.query_row("SELECT value FROM app_settings WHERE key='admin_password'", [], |r| r.get(0)) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(format!("密碼設定讀取失敗：{err}")),
    }
}

fn credential_matches(state: &AppState, password: &str) -> bool {
    if password.len() > 512 { return false; }
    match read_admin_hash(state) {
        Ok(Some(hash)) => PasswordHash::new(&hash)
            .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
            .unwrap_or(false),
        Ok(None) => constant_time_eq(password.as_bytes(), state.token.as_bytes()),
        Err(_) => false,
    }
}

fn issue_session(state: &AppState) -> Result<String, (StatusCode, String)> {
    let session = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let mut sessions = state.sessions.lock().map_err(|_| internal_error("登入狀態鎖定失敗"))?;
    sessions.retain(|_, created| created.elapsed() < Duration::from_secs(12*60*60));
    sessions.insert(session.clone(), Instant::now());
    Ok(session)
}

async fn login_admin(State(state): State<AppState>, Json(req): Json<LoginRequest>) -> ApiResult<Json<serde_json::Value>> {
    // Global limit to make LAN password guessing costly; resets after one minute.
    {
        let mut attempts = state.login_attempts.lock().map_err(|_| internal_error("登入鎖定失敗"))?;
        if attempts.1.elapsed() >= Duration::from_secs(60) { *attempts = (0, Instant::now()); }
        if attempts.0 >= 8 { return Err((StatusCode::TOO_MANY_REQUESTS, "登入嘗試過多，請稍後再試".into())); }
        attempts.0 += 1;
    }
    if !credential_matches(&state, &req.password) {
        return Err((StatusCode::UNAUTHORIZED, "密碼不正確".into()));
    }
    let session = issue_session(&state)?;
    if let Ok(mut attempts) = state.login_attempts.lock() { *attempts = (0, Instant::now()); }
    Ok(Json(serde_json::json!({"ok":true,"session":session,"password_set":state.password_set.load(Ordering::SeqCst)})))
}

async fn logout_admin(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<serde_json::Value>> {
    if let Some(token) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")) {
        if let Ok(mut sessions) = state.sessions.lock() { sessions.remove(token); }
    }
    Ok(Json(serde_json::json!({"ok":true})))
}

async fn change_password(
    State(state): State<AppState>, Json(req): Json<ChangePasswordRequest>
) -> ApiResult<Json<serde_json::Value>> {
    if !credential_matches(&state, &req.current_password) {
        return Err((StatusCode::FORBIDDEN, "目前密碼不正確".into()));
    }
    if req.new_password.chars().count() < 12 || req.new_password.len() > 256 || req.new_password.contains('\0') {
        return Err(bad_request("新密碼至少 12 字元，最多 256 Bytes"));
    }
    if req.current_password == req.new_password { return Err(bad_request("新密碼不可與目前密碼相同")); }
    let hash = tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default().hash_password(req.new_password.as_bytes(), &salt).map(|v| v.to_string())
            .map_err(|e| format!("密碼雜湊失敗：{e}"))
    }).await.map_err(|e| internal_error(format!("密碼工作失敗：{e}")))?
        .map_err(internal_error)?;
    {
        let db = state.db.lock().map_err(|_| internal_error("資料庫鎖定失敗"))?;
        db.execute("INSERT INTO app_settings(key,value) VALUES('admin_password',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![hash])
            .map_err(|e| internal_error(format!("儲存密碼失敗：{e}")))?;
    }
    state.password_set.store(true, Ordering::SeqCst);
    state.sessions.lock().map_err(|_| internal_error("登入狀態鎖定失敗"))?.clear();
    let session = issue_session(&state)?;
    Ok(Json(serde_json::json!({"ok":true,"session":session})))
}

async fn check_token(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let value = request.headers().get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let authorized = value.map(|value| {
        if !state.password_set.load(Ordering::SeqCst) && constant_time_eq(value.as_bytes(), state.token.as_bytes()) {
            return true; // Supports upgrading a V0.3 browser session before the initial password change.
        }
        match state.sessions.lock() {
            Ok(mut sessions) => {
                sessions.retain(|_, created| created.elapsed() < Duration::from_secs(12*60*60));
                sessions.contains_key(value)
            }
            Err(_) => false,
        }
    }).unwrap_or(false);
    if authorized { next.run(request).await }
    else { (StatusCode::UNAUTHORIZED, "登入已失效，請重新登入").into_response() }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for (i, byte) in b.iter().enumerate() {
        diff |= (a.get(i).copied().unwrap_or(0) ^ byte) as usize;
    }
    diff == 0
}

async fn index_html() -> Html<&'static str> { Html(include_str!("../web/index.html")) }
async fn app_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], include_str!("../web/app.js"))
}
async fn style_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], include_str!("../web/style.css"))
}

async fn get_instance(state: &AppState, id: &str) -> ApiResult<Arc<Instance>> {
    state.instances.read().await.get(id).cloned()
        .ok_or((StatusCode::NOT_FOUND, "找不到 Server".into()))
}

async fn system_info(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"data_dir":state.data_dir.to_string_lossy()}))
}

fn eula_is_accepted(dir: &Path) -> bool {
    fs::read_to_string(dir.join("eula.txt")).unwrap_or_default()
        .lines().any(|line| line.trim().eq_ignore_ascii_case("eula=true"))
}

async fn list_servers(State(state): State<AppState>) -> Json<Vec<ServerView>> {
    let entries: Vec<_> = state.instances.read().await.values().cloned().collect();
    let mut items = Vec::with_capacity(entries.len());
    for inst in entries {
        let cfg = inst.config.read().await.clone();
        let proc = inst.process.lock().await;
        let folder = state.data_dir.join("servers").join(&cfg.id);
        items.push(ServerView {
            config: cfg.clone(),
            status: proc.status,
            message: proc.message.clone(),
            eula_accepted: eula_is_accepted(&folder),
            core_ready: downloads::core_is_ready(&folder, &cfg.core_type),
            folder_path: folder.to_string_lossy().into_owned(),
        });
    }
    items.sort_by(|a, b| a.config.name.cmp(&b.config.name));
    Json(items)
}

async fn create_server(
    State(state): State<AppState>,
    Json(payload): Json<NewServer>,
) -> ApiResult<(StatusCode, Json<ServerConfig>)> {
    let name = payload.name.trim();
    if name.is_empty() || name.chars().count() > 60 || payload.port == 0 {
        return Err(bad_request("請輸入有效的 Server 名稱與 Port"));
    }
    if payload.xms_mb < 256 || payload.xmx_mb < payload.xms_mb || payload.xmx_mb > 262_144 {
        return Err(bad_request("記憶體必須滿足 256MB <= Xms <= Xmx <= 262144MB"));
    }
    if !["manual", "vanilla", "paper", "fabric", "forge", "neoforge"].contains(&payload.core_type.as_str()) {
        return Err(bad_request("不支援此伺服器核心類型"));
    }
    if payload.core_type != "manual" && (payload.mc_version.is_empty() || payload.mc_version.len() > 36 ||
        !payload.mc_version.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')) {
        return Err(bad_request("請填寫有效的 Minecraft 版本"));
    }
    let java = payload.java_command.trim();
    if java.is_empty() || java.contains('\0') {
        return Err(bad_request("Java 執行路徑不正確"));
    }
    if state.instances.read().await.values().any(|i| {
        i.config.try_read().map(|cfg| cfg.port == payload.port).unwrap_or(false)
    }) {
        return Err((StatusCode::CONFLICT, "此 Port 已由另一個 Server 設定使用".into()));
    }
    // A successful bind doesn't guarantee it will still be free when Minecraft starts.
    if StdTcpListener::bind(("0.0.0.0", payload.port)).is_err() {
        return Err((StatusCode::CONFLICT, "此 Port 目前被其他程序使用".into()));
    }
    let config = ServerConfig {
        id: Uuid::new_v4().to_string(),
        name: name.to_owned(),
        port: payload.port,
        xms_mb: payload.xms_mb,
        xmx_mb: payload.xmx_mb,
        java_command: java.to_owned(),
        auto_start: payload.auto_start,
        auto_restart: payload.auto_restart,
        core_type: payload.core_type.clone(),
        mc_version: payload.mc_version.clone(),
    };
    let dir = state.data_dir.join("servers").join(&config.id);
    fs::create_dir_all(&dir).map_err(internal_io)?;
    fs::write(dir.join("server.properties"), format!("server-port={}\n", config.port)).map_err(internal_io)?;
    {
        let db = state.db.lock().map_err(|_| internal_error("資料庫鎖定失敗"))?;
        db.execute("INSERT INTO servers (id,name,port,xms_mb,xmx_mb,java_command,auto_start,auto_restart,core_type,mc_version) VALUES (?,?,?,?,?,?,?,?,?,?)",
            params![config.id, config.name, config.port, config.xms_mb, config.xmx_mb, config.java_command, config.auto_start, config.auto_restart, config.core_type, config.mc_version])
            .map_err(|e| internal_error(format!("建立設定失敗：{e}")))?;
    }
    state.instances.write().await.insert(config.id.clone(), Arc::new(Instance::new(config.clone())));
    if config.core_type != "manual" {
        // The instance is created immediately; large downloads continue as an observable job.
        downloads::begin_core_install(&state, &config.id, &config.core_type, &config.mc_version).await?;
    }
    Ok((StatusCode::CREATED, Json(config)))
}

// Removing management never deletes the world's files: data/servers/<UUID> remains.
async fn remove_server(State(state): State<AppState>, AxPath(id): AxPath<String>) -> ApiResult<Json<serde_json::Value>> {
    let inst = get_instance(&state, &id).await?;
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into())); }
    if inst.process.lock().await.child.is_some() {
        return Err((StatusCode::CONFLICT, "請先正常停止 Server，再移除管理紀錄".into()));
    }
    if state.jobs.read().await.get(&format!("core:{id}")).map(|j| j.state == "working").unwrap_or(false) {
        return Err((StatusCode::CONFLICT, "核心正在下載，不能移除".into()));
    }
    {
        let db = state.db.lock().map_err(|_| internal_error("資料庫鎖定失敗"))?;
        db.execute("DELETE FROM servers WHERE id=?", params![id])
            .map_err(|e| internal_error(format!("移除管理紀錄失敗：{e}")))?;
        // Keep old backup ZIP files, but stop scheduling work for a removed Server.
        db.execute("DELETE FROM backup_schedules WHERE server_id=?", params![id])
            .map_err(|e| internal_error(format!("移除備份排程失敗：{e}")))?;
    }
    state.instances.write().await.remove(&id);
    Ok(Json(serde_json::json!({"ok":true,"retained_directory":state.data_dir.join("servers").join(&id).to_string_lossy()})))
}

// Edit initial launch settings without overwriting the Minecraft world or changing core files.
// This endpoint requires the Java process to be stopped: Xms/Xmx only take effect on next launch.
async fn update_server_settings(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
    Json(payload): Json<UpdateServer>,
) -> ApiResult<Json<ServerConfig>> {
    let name = payload.name.trim();
    if name.is_empty() || name.chars().count() > 60 || payload.port == 0 {
        return Err(bad_request("請輸入有效的 Server 名稱與 Port"));
    }
    if payload.xms_mb < 256 || payload.xmx_mb < payload.xms_mb || payload.xmx_mb > 262_144 {
        return Err(bad_request("記憶體必須滿足 256MB <= Xms <= Xmx <= 262144MB"));
    }
    let java = payload.java_command.trim();
    if java.is_empty() || java.contains('\0') {
        return Err(bad_request("Java 執行路徑不正確"));
    }

    let inst = get_instance(&state, &id).await?;
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into())); }
    let process = inst.process.lock().await;
    if process.child.is_some() || process.status != "stopped" {
        return Err((StatusCode::CONFLICT, "必須先停止 Server 才能修改啟動設定".into()));
    }
    let original = inst.config.read().await.clone();
    if payload.port != original.port {
        // Check conflicts with other managed servers and unrelated programs.
        let servers = state.instances.read().await;
        for (other_id, other) in servers.iter() {
            if other_id != &id && other.config.read().await.port == payload.port {
                return Err((StatusCode::CONFLICT, "Port 已由另一個 Server 使用".into()));
            }
        }
        if StdTcpListener::bind(("0.0.0.0", payload.port)).is_err() {
            return Err((StatusCode::CONFLICT, "Port 目前已被其他程序占用".into()));
        }
    }

    let new_config = ServerConfig {
        name: name.into(),
        port: payload.port,
        xms_mb: payload.xms_mb,
        xmx_mb: payload.xmx_mb,
        java_command: java.into(),
        auto_start: payload.auto_start,
        auto_restart: payload.auto_restart,
        ..original.clone()
    };
    let properties = state.data_dir.join("servers").join(&id).join("server.properties");
    // Keep a copy so a database error won't silently leave a different port in the file.
    let original_properties = fs::read(&properties).ok();
    update_server_port(&properties, new_config.port).map_err(internal_io)?;
    let db_result = {
        let db = state.db.lock().map_err(|_| internal_error("資料庫鎖定失敗"))?;
        db.execute(
            "UPDATE servers SET name=?,port=?,xms_mb=?,xmx_mb=?,java_command=?,auto_start=?,auto_restart=? WHERE id=?",
            params![new_config.name, new_config.port, new_config.xms_mb,
                new_config.xmx_mb, new_config.java_command, new_config.auto_start,
                new_config.auto_restart, new_config.id],
        )
    };
    if let Err(e) = db_result {
        if let Some(bytes) = original_properties {
            let _ = fs::write(&properties, bytes);
        } else {
            let _ = fs::remove_file(&properties);
        }
        return Err(internal_error(format!("儲存設定失敗：{e}")));
    }
    *inst.config.write().await = new_config.clone();
    drop(process);
    inst.log(format!(
        "[MineOS] 啟動設定已更新：Port {}，Java {}，RAM {}–{} MB（下次啟動生效）",
        new_config.port, new_config.java_command, new_config.xms_mb, new_config.xmx_mb
    )).await;
    Ok(Json(new_config))
}

// Open on the host PC, not in the remote browser. For a future Windows Service
// running in Session 0 this is unavailable visually; the copy-path action still works.
async fn open_server_folder(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
) -> ApiResult<Json<serde_json::Value>> {
    get_instance(&state, &id).await?;
    let dir = state.data_dir.join("servers").join(id);
    if !dir.is_dir() {
        return Err((StatusCode::NOT_FOUND, "伺服器資料夾不存在".into()));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe").arg(&dir).spawn()
            .map_err(internal_io)?;
        Ok(Json(serde_json::json!({"ok":true,"path":dir.to_string_lossy()})))
    }
    #[cfg(not(windows))]
    {
        Err(bad_request("只有 Windows 支援直接開啟檔案總管"))
    }
}

async fn start_by_id(state: &AppState, id: &str) -> ApiResult<()> {
    start_by_id_internal(state, id, false).await
}

async fn start_by_id_internal(state: &AppState, id: &str, allowed_during_maintenance: bool) -> ApiResult<()> {
    let instance = get_instance(state, id).await?;
    if !allowed_during_maintenance && instance.maintenance.load(Ordering::SeqCst) {
        return Err((StatusCode::CONFLICT, "伺服器正在備份／還原，暫時無法啟動".into()));
    }
    let cfg = instance.config.read().await.clone();
    let dir = state.data_dir.join("servers").join(id);
    if !downloads::core_is_ready(&dir, &cfg.core_type) {
        return Err(bad_request("伺服器核心尚未安裝完成，或缺少啟動檔案"));
    }
    let java_exec = downloads::resolve_java(&state.data_dir, &cfg.java_command)?;
    if !eula_is_accepted(&dir) {
        return Err(bad_request("請先閱讀並同意 Minecraft EULA"));
    }
    update_server_port(&dir.join("server.properties"), cfg.port).map_err(internal_io)?;
    let mut proc = instance.process.lock().await;
    if !allowed_during_maintenance && instance.maintenance.load(Ordering::SeqCst) {
        return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into()));
    }
    if proc.child.is_some() { return Err((StatusCode::CONFLICT, "Server 已在執行中".into())); }
    let now = Instant::now();
    while proc.starts_in_window.front().map(|t| now.duration_since(*t) > Duration::from_secs(60)).unwrap_or(false) {
        proc.starts_in_window.pop_front();
    }
    if proc.starts_in_window.len() >= 4 {
        return Err((StatusCode::TOO_MANY_REQUESTS, "60 秒內啟動過於頻繁，請稍後重試".into()));
    }
    proc.starts_in_window.push_back(now);
    proc.status = "starting";
    proc.message = None;
    let mut command = Command::new(java_exec);
    command.arg(format!("-Xms{}M", cfg.xms_mb))
        .arg(format!("-Xmx{}M", cfg.xmx_mb));
    downloads::configure_launch(&mut command, &dir, &cfg.core_type)?;
    command.current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    match command.spawn() {
        Ok(mut child) => {
            proc.stdin = child.stdin.take();
            if let Some(stdout) = child.stdout.take() {
                spawn_log_reader(instance.clone(), stdout, "");
            }
            if let Some(stderr) = child.stderr.take() {
                spawn_log_reader(instance.clone(), stderr, "[stderr] ");
            }
            proc.child = Some(child);
            proc.status = "running"; // Process is alive; game readiness is not asserted.
            proc.requested_stop = false;
            proc.pending_restart = false;
            drop(proc);
            instance.log(format!("[MineOS] 已啟動 Java 程序；Port {}，堆積記憶體 {}–{} MB", cfg.port, cfg.xms_mb, cfg.xmx_mb)).await;
            Ok(())
        }
        Err(e) => {
            proc.status = "stopped";
            proc.message = Some(format!("啟動失敗：{e}"));
            Err(internal_error(format!("無法啟動 Java：{e}")))
        }
    }
}

fn spawn_log_reader<R>(instance: Arc<Instance>, reader: R, prefix: &'static str)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            instance.log(format!("{prefix}{line}")).await;
        }
    });
}

fn update_server_port(file: &Path, port: u16) -> std::io::Result<()> {
    let old = fs::read_to_string(file).unwrap_or_default();
    let mut changed = false;
    let mut lines = vec![];
    for line in old.lines() {
        if line.trim_start().starts_with("server-port=") {
            if !changed {
                lines.push(format!("server-port={port}"));
                changed = true;
            }
        } else {
            lines.push(line.to_owned());
        }
    }
    if !changed { lines.push(format!("server-port={port}")); }
    fs::write(file, format!("{}\n", lines.join("\n")))
}

async fn start_server(State(state): State<AppState>, AxPath(id): AxPath<String>) -> ApiResult<Json<serde_json::Value>> {
    start_by_id(&state, &id).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn stop_server(State(state): State<AppState>, AxPath(id): AxPath<String>) -> ApiResult<Json<serde_json::Value>> {
    let inst = get_instance(&state, &id).await?;
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into())); }
    request_stop(&inst, false).await?;
    Ok(Json(serde_json::json!({"ok": true, "note": "已發送正常停止指令"})))
}

async fn restart_server(State(state): State<AppState>, AxPath(id): AxPath<String>) -> ApiResult<Json<serde_json::Value>> {
    let inst = get_instance(&state, &id).await?;
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into())); }
    if inst.process.lock().await.child.is_some() {
        request_stop(&inst, true).await?;
    } else {
        start_by_id(&state, &id).await?;
    }
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn request_stop(inst: &Arc<Instance>, restart: bool) -> ApiResult<()> {
    let mut proc = inst.process.lock().await;
    if proc.child.is_none() { return Err((StatusCode::CONFLICT, "Server 未執行".into())); }
    if proc.status == "stopping" {
        return Err((StatusCode::CONFLICT, "Server 正在停止中".into()));
    }
    if let Some(stdin) = proc.stdin.as_mut() {
        stdin.write_all(b"stop\n").await.map_err(|e| internal_error(format!("無法送出 stop：{e}")))?;
        stdin.flush().await.map_err(internal_io)?;
    } else { return Err(internal_error("沒有 Console 輸入管線")); }
    proc.requested_stop = true;
    proc.pending_restart = restart;
    proc.status = "stopping";
    drop(proc);
    inst.log(if restart { "[MineOS] 已發送 stop，等待停止後重啟" } else { "[MineOS] 已發送 stop，等待正常停止" }).await;
    Ok(())
}

#[derive(Deserialize)]
struct CommandRequest { command: String }

async fn console_command(State(state): State<AppState>, AxPath(id): AxPath<String>, Json(data): Json<CommandRequest>) -> ApiResult<Json<serde_json::Value>> {
    let command = data.command.trim();
    if command.is_empty() || command.len() > 2048 || command.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
        return Err(bad_request("指令格式不正確"));
    }
    let inst = get_instance(&state, &id).await?;
    let mut proc = inst.process.lock().await;
    if proc.status != "running" { return Err((StatusCode::CONFLICT, "Server 尚未執行".into())); }
    let stdin = proc.stdin.as_mut().ok_or_else(|| internal_error("Console 不可用"))?;
    stdin.write_all(format!("{command}\n").as_bytes()).await.map_err(internal_io)?;
    stdin.flush().await.map_err(internal_io)?;
    drop(proc);
    inst.log(format!("[Admin] > {command}")).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn server_logs(State(state): State<AppState>, AxPath(id): AxPath<String>) -> ApiResult<Json<Vec<String>>> {
    let inst = get_instance(&state, &id).await?;
    let logs = inst.logs.lock().await;
    Ok(Json(logs.iter().cloned().collect()))
}

async fn watch_processes(state: AppState) {
    let mut ticker = time::interval(Duration::from_secs(1));
    loop {
        ticker.tick().await;
        let items: Vec<_> = state.instances.read().await.iter().map(|(id, i)| (id.clone(), i.clone())).collect();
        for (id, inst) in items {
            let exit = {
                let mut proc = inst.process.lock().await;
                let status = match proc.child.as_mut() {
                    Some(child) => child.try_wait(),
                    None => continue,
                };
                match status {
                    Ok(Some(code)) => {
                        let restart = !inst.maintenance.load(Ordering::SeqCst) && (proc.pending_restart || (!proc.requested_stop && inst.config.read().await.auto_restart));
                        proc.stdin = None;
                        proc.child = None;
                        proc.status = "stopped";
                        proc.message = Some(format!("Java 已退出：{code}"));
                        proc.requested_stop = false;
                        proc.pending_restart = false;
                        Some((code.to_string(), restart))
                    }
                    Ok(None) => None,
                    Err(e) => {
                        proc.message = Some(format!("程序監控失敗：{e}"));
                        None
                    }
                }
            };
            if let Some((code, restart)) = exit {
                inst.log(format!("[MineOS] Java 程序結束：{code}")).await;
                if restart {
                    time::sleep(Duration::from_secs(2)).await;
                    if let Err((_, msg)) = start_by_id(&state, &id).await {
                        inst.log(format!("[MineOS] 自動重新啟動失敗：{msg}")).await;
                    }
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct UploadTarget { folder: String }

async fn upload_file(
    State(state): State<AppState>,
    AxPath(id): AxPath<String>,
    Query(target): Query<UploadTarget>,
    mut multipart: Multipart,
) -> ApiResult<Json<serde_json::Value>> {
    let inst = get_instance(&state, &id).await?;
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into())); }
    if inst.process.lock().await.child.is_some() {
        return Err((StatusCode::CONFLICT, "為避免檔案不一致，請先停止 Server 再上傳".into()));
    }
    if state.jobs.read().await.get(&format!("core:{id}")).map(|j| j.state == "working").unwrap_or(false) && target.folder == "core" {
        return Err((StatusCode::CONFLICT, "核心正在下載，請先等待下載完成".into()));
    }
    let dir = state.data_dir.join("servers").join(&id);
    if target.folder == "core" && ["forge", "neoforge"].contains(&inst.config.read().await.core_type.as_str()) {
        return Err(bad_request("Forge／NeoForge 請使用官方 Installer 自動安裝，不支援覆蓋 server.jar"));
    }
    let dest_dir = match target.folder.as_str() {
        "core" => dir,
        "plugins" => dir.join("plugins"),
        "mods" => dir.join("mods"),
        _ => return Err(bad_request("只能上傳到 core、plugins 或 mods")),
    };
    tokio::fs::create_dir_all(&dest_dir).await.map_err(internal_io)?;
    let mut field = multipart.next_field().await.map_err(|e| bad_request(e.to_string()))?
        .ok_or_else(|| bad_request("沒有上傳檔案"))?;
    let raw_name = field.file_name().ok_or_else(|| bad_request("缺少檔名"))?.to_owned();
    if !valid_jar_name(&raw_name) { return Err(bad_request("僅允許安全檔名的 .jar 檔")); }
    let file_name = if target.folder == "core" { "server.jar".to_owned() } else { raw_name };
    let dest = dest_dir.join(&file_name);
    if dest.exists() {
        return Err((StatusCode::CONFLICT, "同名檔案已存在；請先備份並移除舊檔".into()));
    }
    let temp = dest_dir.join(format!(".upload-{}.tmp", Uuid::new_v4()));
    let result = async {
        let mut file = tokio::fs::File::create(&temp).await.map_err(internal_io)?;
        let mut size = 0usize;
        while let Some(chunk) = field.chunk().await.map_err(|e| bad_request(e.to_string()))? {
            size += chunk.len();
            if size > MAX_UPLOAD_BYTES {
                return Err(bad_request("檔案超過 512 MB 限制"));
            }
            file.write_all(&chunk).await.map_err(internal_io)?;
        }
        file.flush().await.map_err(internal_io)?;
        drop(file);
        if size == 0 { return Err(bad_request("不可上傳空檔案")); }
        // Basic JAR/ZIP magic verification; does not establish trust or compatibility.
        let mut probe = tokio::fs::File::open(&temp).await.map_err(internal_io)?;
        let mut bytes = [0u8; 4];
        probe.read_exact(&mut bytes).await.map_err(internal_io)?;
        if &bytes[0..2] != b"PK" {
            return Err(bad_request("檔案不是有效的 ZIP/JAR 開頭"));
        }
        drop(probe);
        if dest.exists() { return Err((StatusCode::CONFLICT, "上傳過程中出現同名檔案".into())); }
        tokio::fs::rename(&temp, &dest).await.map_err(internal_io)?;
        Ok::<usize, (StatusCode, String)>(size)
    }.await;
    if result.is_err() { let _ = tokio::fs::remove_file(&temp).await; }
    let size = result?;
    inst.log(format!("[MineOS] 上傳檔案：{}/{} ({} bytes)", target.folder, file_name, size)).await;
    Ok(Json(serde_json::json!({"ok": true, "file": file_name, "bytes": size})))
}

fn valid_jar_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 180
        && !s.starts_with('.')
        && s.to_ascii_lowercase().ends_with(".jar")
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-()[] ".contains(c))
}

#[derive(Serialize)]
struct FileInfo { folder: String, name: String, bytes: u64 }

async fn list_files(State(state): State<AppState>, AxPath(id): AxPath<String>) -> ApiResult<Json<Vec<FileInfo>>> {
    get_instance(&state, &id).await?;
    let root = state.data_dir.join("servers").join(id);
    let mut entries = Vec::new();
    for (folder, path) in [("core", root.clone()), ("plugins", root.join("plugins")), ("mods", root.join("mods"))] {
        let read = fs::read_dir(path);
        if let Ok(read) = read {
            for item in read.flatten() {
                if let Ok(meta) = item.metadata() {
                    if meta.is_file() && item.file_name().to_string_lossy().to_ascii_lowercase().ends_with(".jar") {
                        entries.push(FileInfo { folder: folder.into(), name: item.file_name().to_string_lossy().into_owned(), bytes: meta.len() });
                    }
                }
            }
        }
    }
    entries.sort_by(|a, b| a.folder.cmp(&b.folder).then(a.name.cmp(&b.name)));
    Ok(Json(entries))
}

#[derive(Deserialize)]
struct EulaRequest { accepted: bool }

async fn accept_eula(State(state): State<AppState>, AxPath(id): AxPath<String>, Json(req): Json<EulaRequest>) -> ApiResult<Json<serde_json::Value>> {
    let inst = get_instance(&state, &id).await?;
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into())); }
    if !req.accepted { return Err(bad_request("必須由管理員自行確認同意 EULA")); }
    if inst.process.lock().await.child.is_some() {
        return Err((StatusCode::CONFLICT, "請先停止 Server".into()));
    }
    let path = state.data_dir.join("servers").join(&id).join("eula.txt");
    fs::write(&path, "# The administrator confirmed acceptance of the Minecraft EULA.\neula=true\n").map_err(internal_io)?;
    inst.log("[MineOS] 管理員已確認接受 Minecraft EULA").await;
    Ok(Json(serde_json::json!({"ok": true})))
}

fn bad_request(msg: impl Into<String>) -> (StatusCode, String) { (StatusCode::BAD_REQUEST, msg.into()) }
fn internal_error(msg: impl Into<String>) -> (StatusCode, String) { (StatusCode::INTERNAL_SERVER_ERROR, msg.into()) }
fn internal_io(e: std::io::Error) -> (StatusCode, String) { internal_error(format!("檔案操作失敗：{e}")) }
