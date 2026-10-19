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
        ("core_build", "TEXT NOT NULL DEFAULT ''"),
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
            "SELECT id,name,port,xms_mb,xmx_mb,java_command,auto_start,auto_restart,core_type,mc_version,core_build FROM servers ORDER BY name",
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
                core_build: r.get(10)?,
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
        .route("/hardware", get(hardware::snapshot))
        .route("/builds", get(downloads::builds))
        .route("/servers/{id}/install-core", post(downloads::install_core))
        .route("/servers/{id}/bedrock-zip", post(bedrock::upload_bedrock_zip))
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
        .route("/ui-v07.js", get(ui_v07_js))
        .route("/ui-v07.css", get(ui_v07_css))
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
