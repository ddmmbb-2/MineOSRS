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
