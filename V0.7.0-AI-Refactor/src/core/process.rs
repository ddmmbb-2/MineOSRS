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
    let java_exec = if cfg.core_type == "bedrock" { None } else { Some(downloads::resolve_java(&state.data_dir, &cfg.java_command)?) };
    if !eula_is_accepted(&dir) {
        return Err(bad_request("請先閱讀並同意 Minecraft EULA"));
    }
    if cfg.core_type == "bedrock" { set_bedrock_ports(&dir.join("server.properties"), cfg.port) } else { update_server_port(&dir.join("server.properties"), cfg.port) }.map_err(internal_io)?;
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
    // Legacy Paperclip versions may point to retired Mojang S3 URLs.
    // Validate the official Mojang JAR into cache before Java starts.
    if cfg.core_type == "paper" {
        legacy_paper::prepare_if_needed(&dir, &cfg.mc_version)
            .await
            .map_err(|e| bad_request(&format!("舊版 Paper 原版核心快取修復失敗：{e}")))?;
    }
    let mut command = if cfg.core_type == "bedrock" {
        Command::new(dir.join("bedrock_server.exe"))
    } else {
        let mut java = Command::new(java_exec.expect("Java required for Java cores"));
        java.arg(format!("-Xms{}M", cfg.xms_mb))
            .arg(format!("-Xmx{}M", cfg.xmx_mb));
        downloads::configure_launch(&mut java, &dir, &cfg.core_type)?;
        java
    };
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
            instance.log(format!("[MineOS] 已啟動伺服器程序；Port {}，設定記憶體 {}–{} MB", cfg.port, cfg.xms_mb, cfg.xmx_mb)).await;
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

fn bedrock_ipv6_port(port: u16) -> u16 { if port < 65535 { port + 1 } else { port - 1 } }
fn managed_port_conflict(a_core: &str, a_port: u16, b_core: &str, b_port: u16) -> bool {
    if a_core == "bedrock" && b_core == "bedrock" {
        [a_port,bedrock_ipv6_port(a_port)].iter().any(|a|
            [b_port,bedrock_ipv6_port(b_port)].contains(a))
    } else { a_port == b_port }
}
fn port_available(core: &str, port: u16) -> bool {
    if core == "bedrock" {
        let v6 = bedrock_ipv6_port(port);
        StdUdpSocket::bind(("0.0.0.0", port)).is_ok() &&
        StdUdpSocket::bind(("[::]", v6)).is_ok()
    } else { StdTcpListener::bind(("0.0.0.0", port)).is_ok() }
}
fn set_bedrock_ports(file: &Path, port: u16) -> std::io::Result<()> {
    // Do not allow multiple BDS instances to also bind the default LAN discovery ports.
    let text = fs::read_to_string(file).unwrap_or_default();
    let mut props: Vec<String> = text.lines().filter(|line| {
        !["server-port", "server-portv6", "enable-lan-visibility"].iter()
            .any(|k| line.trim_start().starts_with(&format!("{k}=")))
    }).map(str::to_owned).collect();
    props.push(format!("server-port={port}"));
    props.push(format!("server-portv6={}", bedrock_ipv6_port(port)));
    props.push("enable-lan-visibility=false".to_string());
    fs::write(file, format!("{}\n", props.join("\n")))
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
                        proc.message = Some(format!("Server 已退出：{code}"));
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
                inst.log(format!("[MineOS] Server 程序結束：{code}")).await;
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
