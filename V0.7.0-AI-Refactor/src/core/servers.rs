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
    validate_server_resources(name, payload.port, payload.xms_mb, payload.xmx_mb)?;
    if !["manual", "vanilla", "paper", "fabric", "forge", "neoforge", "bedrock"].contains(&payload.core_type.as_str()) {
        return Err(bad_request("不支援此伺服器核心類型"));
    }
    if payload.core_type != "manual" && (payload.mc_version.is_empty() || payload.mc_version.len() > 36 ||
        !payload.mc_version.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-')) {
        return Err(bad_request("請填寫有效的 Minecraft 版本"));
    }
    if !downloads::valid_build(&payload.core_build) { return Err(bad_request("核心 Build 格式錯誤")); }
    let java = payload.java_command.trim();
    validate_java_command(java)?;
    if state.instances.read().await.values().any(|i| {
        i.config.try_read().map(|cfg| managed_port_conflict(&cfg.core_type, cfg.port, &payload.core_type, payload.port)).unwrap_or(false)
    }) {
        return Err((StatusCode::CONFLICT, "此 Port 已由另一個 Server 設定使用".into()));
    }
    // A successful bind doesn't guarantee it will still be free when Minecraft starts.
    if !port_available(&payload.core_type, payload.port) {
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
        core_build: payload.core_build.clone(),
    };
    let dir = state.data_dir.join("servers").join(&config.id);
    fs::create_dir_all(&dir).map_err(internal_io)?;
    fs::write(dir.join("server.properties"), format!("server-port={}\n", config.port)).map_err(internal_io)?;
    if config.core_type == "bedrock" { set_bedrock_ports(&dir.join("server.properties"), config.port).map_err(internal_io)?; }
    {
        let db = state.db.lock().map_err(|_| internal_error("資料庫鎖定失敗"))?;
        db.execute("INSERT INTO servers (id,name,port,xms_mb,xmx_mb,java_command,auto_start,auto_restart,core_type,mc_version,core_build) VALUES (?,?,?,?,?,?,?,?,?,?,?)",
            params![config.id, config.name, config.port, config.xms_mb, config.xmx_mb, config.java_command, config.auto_start, config.auto_restart, config.core_type, config.mc_version, config.core_build])
            .map_err(|e| internal_error(format!("建立設定失敗：{e}")))?;
    }
    state.instances.write().await.insert(config.id.clone(), Arc::new(Instance::new(config.clone())));
    if config.core_type != "manual" && config.core_type != "bedrock" {
        // The instance is created immediately; large downloads continue as an observable job.
        downloads::begin_core_install(&state, &config.id, &config.core_type, &config.mc_version, &config.core_build).await?;
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
    validate_server_resources(name, payload.port, payload.xms_mb, payload.xmx_mb)?;
    let java = payload.java_command.trim();
    validate_java_command(java)?;

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
            if other_id != &id && { let other_cfg = other.config.read().await; managed_port_conflict(&other_cfg.core_type, other_cfg.port, &original.core_type, payload.port) } {
                return Err((StatusCode::CONFLICT, "Port 已由另一個 Server 使用".into()));
            }
        }
        if !port_available(&original.core_type, payload.port) {
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
    if original.core_type == "bedrock" { set_bedrock_ports(&properties, new_config.port) } else { update_server_port(&properties, new_config.port) }.map_err(internal_io)?;
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
