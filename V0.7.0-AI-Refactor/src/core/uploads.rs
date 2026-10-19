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
                    if meta.is_file() && (item.file_name().to_string_lossy().to_ascii_lowercase().ends_with(".jar") || item.file_name().to_string_lossy().eq_ignore_ascii_case("bedrock_server.exe")) {
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
