//! Consistent full-server backups and guarded restoration.
//! Minecraft is gracefully stopped before copying files, then restarted if it
//! was previously running. Restore always requires the server to be stopped.
//! Backup archives live OUTSIDE servers/<UUID>/ to prevent recursive copies.

use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{atomic::Ordering, Arc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use axum::{
    body::Body,
    extract::{Path as AxPath, State},
    http::{header, HeaderValue, StatusCode},
    response::Response,
    Json,
};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{fs as async_fs, time};
use tokio_util::io::ReaderStream;
use uuid::Uuid;
use zip::{write::FileOptions, CompressionMethod, ZipArchive, ZipWriter};

use crate::{
    bad_request, get_instance, internal_error, internal_io, request_stop,
    start_by_id_internal, AppState, ApiResult, Instance,
};
use crate::downloads::Job;

const MAX_ARCHIVE_FILES: usize = 250_000;
const MAX_RESTORE_BYTES: u64 = 500 * 1024 * 1024 * 1024; // 500 GiB
const GRACEFUL_STOP_WAIT: Duration = Duration::from_secs(180);

#[derive(Clone, Serialize)]
pub(crate) struct Schedule {
    interval_hours: u32,
    keep_count: u32,
    next_due: i64,
}
impl Default for Schedule {
    fn default() -> Self { Self { interval_hours: 0, keep_count: 5, next_due: 0 } }
}

#[derive(Deserialize)]
pub(crate) struct ScheduleUpdate { interval_hours: u32, keep_count: u32 }

#[derive(Serialize)]
pub(crate) struct BackupEntry {
    name: String,
    kind: String,
    created_at: i64,
    bytes: u64,
}

#[derive(Serialize)]
pub(crate) struct BackupList {
    schedule: Schedule,
    backups: Vec<BackupEntry>,
}

struct MaintenanceGuard(Arc<Instance>);
impl Drop for MaintenanceGuard {
    fn drop(&mut self) { self.0.maintenance.store(false, Ordering::SeqCst); }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

fn backup_dir(state: &AppState, id: &str) -> PathBuf {
    state.data_dir.join("backups").join(id)
}

fn valid_backup_name(name: &str) -> bool {
    name.len() <= 100 && (name.starts_with("manual-") || name.starts_with("auto-"))
        && name.ends_with(".zip")
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        && !name.contains("..")
}

fn archive_path(state: &AppState, id: &str, name: &str) -> ApiResult<PathBuf> {
    if !valid_backup_name(name) { return Err(bad_request("備份檔案名稱不合法")); }
    Ok(backup_dir(state, id).join(name))
}

fn read_schedule(state: &AppState, id: &str) -> ApiResult<Schedule> {
    let db = state.db.lock().map_err(|_| internal_error("資料庫鎖定失敗"))?;
    db.query_row(
        "SELECT interval_hours,keep_count,next_due FROM backup_schedules WHERE server_id=?",
        params![id], |r| Ok(Schedule {
            interval_hours: r.get(0)?, keep_count: r.get(1)?, next_due: r.get(2)?,
        }),
    ).optional().map(|v| v.unwrap_or_default())
        .map_err(|e| internal_error(format!("讀取備份排程失敗：{e}")))
}

fn list_entries(dir: &Path) -> Result<Vec<BackupEntry>, String> {
    let mut entries = Vec::new();
    if !dir.exists() { return Ok(entries); }
    for item in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let name = item.file_name().to_string_lossy().to_string();
        if !valid_backup_name(&name) { continue; }
        let metadata = item.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || item.file_type().map_err(|e| e.to_string())?.is_symlink() { continue; }
        let created_at = metadata.modified().unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        entries.push(BackupEntry {
            kind: if name.starts_with("auto-") { "auto" } else { "manual" }.into(),
            name, created_at, bytes: metadata.len(),
        });
    }
    entries.sort_by(|a,b| b.name.cmp(&a.name));
    Ok(entries)
}

pub(crate) async fn list_backups(
    State(state): State<AppState>, AxPath(id): AxPath<String>
) -> ApiResult<Json<BackupList>> {
    get_instance(&state, &id).await?;
    let schedule = read_schedule(&state, &id)?;
    let path = backup_dir(&state, &id);
    let backups = tokio::task::spawn_blocking(move || list_entries(&path))
        .await.map_err(|e| internal_error(format!("掃描備份失敗：{e}")))?
        .map_err(internal_error)?;
    Ok(Json(BackupList { schedule, backups }))
}

pub(crate) async fn save_schedule(
    State(state): State<AppState>, AxPath(id): AxPath<String>, Json(req): Json<ScheduleUpdate>
) -> ApiResult<Json<Schedule>> {
    let inst = get_instance(&state, &id).await?;
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "備份／還原進行中，請稍後再修改排程".into())); }
    if req.interval_hours > 720 || req.keep_count == 0 || req.keep_count > 50 {
        return Err(bad_request("備份間隔可設為 0（關閉）或 1–720 小時；自動備份保留數量為 1–50"));
    }
    let next_due = if req.interval_hours == 0 { 0 } else { now() + i64::from(req.interval_hours) * 3600 };
    {
        let db = state.db.lock().map_err(|_| internal_error("資料庫鎖定失敗"))?;
        db.execute("INSERT INTO backup_schedules (server_id,interval_hours,keep_count,next_due) VALUES(?,?,?,?) \
            ON CONFLICT(server_id) DO UPDATE SET interval_hours=excluded.interval_hours,keep_count=excluded.keep_count,next_due=excluded.next_due",
            params![id, req.interval_hours, req.keep_count, next_due])
            .map_err(|e| internal_error(format!("儲存排程失敗：{e}")))?;
    }
    Ok(Json(Schedule { interval_hours: req.interval_hours, keep_count: req.keep_count, next_due }))
}

pub(crate) async fn make_backup(
    State(state): State<AppState>, AxPath(id): AxPath<String>
) -> ApiResult<Json<serde_json::Value>> {
    begin_backup(state, id, false).await?;
    Ok(Json(json!({"ok":true,"job": "backup"})))
}

async fn set_job(state: &AppState, id: &str, detail: &str) {
    if let Some(job) = state.jobs.write().await.get_mut(&format!("backup:{id}")) {
        job.detail = detail.into();
    }
}

async fn finish_job(state: &AppState, id: &str, result: Result<String,String>) {
    let (status, message) = match result {
        Ok(msg) => ("done", msg),
        Err(msg) => ("error", msg),
    };
    state.jobs.write().await.insert(format!("backup:{id}"), Job { state: status.into(), detail: message, percent: None });
}

async fn begin_backup(state: AppState, id: String, automatic: bool) -> ApiResult<()> {
    let inst = get_instance(&state, &id).await?;
    if inst.maintenance.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Err((StatusCode::CONFLICT, "已有備份或還原工作正在執行".into()));
    }
    // Before a backup, the core installer must be completely idle.
    if state.jobs.read().await.get(&format!("core:{id}")).is_some_and(|j| j.state == "working") {
        inst.maintenance.store(false, Ordering::SeqCst);
        return Err((StatusCode::CONFLICT, "核心正在安裝，請稍後再備份".into()));
    }
    if inst.process.lock().await.status == "stopping" {
        inst.maintenance.store(false, Ordering::SeqCst);
        return Err((StatusCode::CONFLICT, "伺服器正在停止或重新啟動，請稍後再備份".into()));
    }
    state.jobs.write().await.insert(format!("backup:{id}"), Job {
        state: "working".into(), detail: "準備進行完整備份…".into(), percent: None,
    });
    tokio::spawn(async move {
        let _guard = MaintenanceGuard(inst.clone());
        let result = backup_work(&state, &id, automatic, inst.clone()).await;
        inst.log(format!("[MineOS] 備份結果：{}", match &result { Ok(msg) => msg, Err(msg) => msg })).await;
        finish_job(&state, &id, result).await;
    });
    Ok(())
}

async fn backup_work(state: &AppState, id: &str, automatic: bool, inst: Arc<Instance>) -> Result<String,String> {
    let was_running = inst.process.lock().await.child.is_some();
    let result = async {
        if was_running {
            set_job(state, id, "正在正常停止 Minecraft，確保世界存檔完整…").await;
            request_stop(&inst, false).await.map_err(|(_, e)| e)?;
            time::timeout(GRACEFUL_STOP_WAIT, async {
                loop {
                    let proc = inst.process.lock().await;
                    let stopped = proc.child.is_none() && proc.status == "stopped";
                    drop(proc);
                    if stopped { break; }
                    time::sleep(Duration::from_millis(300)).await;
                }
            }).await.map_err(|_| "Minecraft 無法在 180 秒內正常停止，備份已取消；沒有強制終止程序".to_string())?;
        }
        let source = state.data_dir.join("servers").join(id);
        let target_dir = backup_dir(state, id);
        let cfg = inst.config.read().await.clone();
        set_job(state, id, "正在建立完整 ZIP 快照（世界、模組、核心、設定檔）…").await;
        let kind = if automatic { "auto" } else { "manual" };
        let kind = kind.to_owned();
        let path = tokio::task::spawn_blocking(move || make_zip(&source, &target_dir, &cfg, &kind))
            .await.map_err(|e| format!("備份工作異常：{e}"))??;
        // Automatic retention only removes earlier automatic archives, never manual ones.
        if automatic {
            let schedule = read_schedule(state, id).map_err(|(_,e)| e)?;
            let target_dir = backup_dir(state,id);
            tokio::task::spawn_blocking(move || prune_auto(&target_dir, schedule.keep_count))
                .await.map_err(|e| format!("備份清理工作異常：{e}"))??;
        }
        Ok::<String, String>(format!("備份完成：{}", path.file_name().unwrap_or_default().to_string_lossy()))
    }.await;

    // A running Minecraft server is restarted even if backup failed, unless it
    // never stopped normally (in that case start would conflict and is not forced).
    let restart_message = if was_running {
        set_job(state, id, "備份已結束，正在恢復伺服器運行…").await;
        match start_by_id_internal(state, id, true).await {
            Ok(()) => None,
            Err((_, msg)) => {
                if inst.process.lock().await.child.is_some() { None }
                else { Some(format!("；注意：伺服器未能自動重新啟動：{msg}")) }
            },
        }
    } else { None };
    if automatic {
        let next = now() + if result.is_ok() {
            read_schedule(state,id).map(|s| i64::from(s.interval_hours.max(1)) * 3600).unwrap_or(3600)
        } else { 15 * 60 };
        if let Ok(db) = state.db.lock() {
            let _ = db.execute("UPDATE backup_schedules SET next_due=? WHERE server_id=? AND interval_hours>0", params![next,id]);
        }
    }
    match (result, restart_message) {
        (Ok(msg), Some(warning)) => Err(format!("{msg}{warning}")),
        (Err(msg), Some(warning)) => Err(format!("{msg}{warning}")),
        (val, None) => val,
    }
}

fn is_reparse_or_symlink(path: &Path) -> Result<bool,String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("檢查檔案屬性失敗：{e}"))?;
    if meta.file_type().is_symlink() { return Ok(true); }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 { return Ok(true); }
    }
    Ok(false)
}

fn walk_archive(dir: &Path, root: &Path, zip: &mut ZipWriter<File>, count: &mut usize) -> Result<(),String> {
    if is_reparse_or_symlink(dir)? { return Err(format!("備份中止：目錄含連結／接合點：{}", dir.display())); }
    for item in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let path = item.path();
        if is_reparse_or_symlink(&path)? { return Err(format!("備份中止：資料夾有連結／接合點：{}", path.display())); }
        *count += 1;
        if *count > MAX_ARCHIVE_FILES { return Err("檔案數量超過安全限制".into()); }
        let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
        let mut name = String::from("server/");
        for component in rel.components() {
            let p = component.as_os_str().to_str().ok_or("有無法轉換成 UTF-8 的檔案名稱")?;
            name.push_str(p); name.push('/');
        }
        let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            zip.add_directory(name.clone(), FileOptions::default().compression_method(CompressionMethod::Stored))
                .map_err(|e| e.to_string())?;
            walk_archive(&path, root, zip, count)?;
        } else if meta.is_file() {
            name.pop();
            let options = FileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .compression_level(Some(3))
                .large_file(meta.len() >= u32::MAX as u64);
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            let mut input = File::open(&path).map_err(|e| e.to_string())?;
            std::io::copy(&mut input, zip).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn make_zip(source: &Path, dir: &Path, cfg: &crate::ServerConfig, kind: &str) -> Result<PathBuf,String> {
    if !source.is_dir() { return Err("伺服器資料夾不存在".into()); }
    if is_reparse_or_symlink(source)? { return Err("伺服器資料夾不可為連結／接合點".into()); }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let file_name = format!("{}-{}-{}.zip", kind, now(), Uuid::new_v4().simple());
    let out = dir.join(&file_name);
    let partial = dir.join(format!(".{file_name}.part"));
    let operation = (|| -> Result<(),String> {
        let file = File::create(&partial).map_err(|e| e.to_string())?;
        let mut writer = ZipWriter::new(file);
        writer.start_file("manifest.json", FileOptions::default().compression_method(CompressionMethod::Deflated))
            .map_err(|e| e.to_string())?;
        writer.write_all(json!({"format":1,"server_id":cfg.id,"created_at":now(),"kind":kind,"settings":cfg}).to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        let mut files = 0usize;
        walk_archive(source, source, &mut writer, &mut files)?;
        let finished = writer.finish().map_err(|e| e.to_string())?;
        finished.sync_all().map_err(|e| e.to_string())?;
        fs::rename(&partial, &out).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if operation.is_err() { let _ = fs::remove_file(&partial); }
    operation.map(|_| out)
}

fn prune_auto(dir: &Path, keep: u32) -> Result<(),String> {
    let auto: Vec<_> = list_entries(dir)?.into_iter().filter(|e| e.kind == "auto").collect();
    for entry in auto.iter().skip(keep as usize) {
        fs::remove_file(dir.join(&entry.name)).map_err(|e| format!("清理舊備份失敗：{e}"))?;
    }
    Ok(())
}

pub(crate) async fn restore_backup(
    State(state): State<AppState>, AxPath((id,name)): AxPath<(String,String)>
) -> ApiResult<Json<serde_json::Value>> {
    let inst = get_instance(&state,&id).await?;
    let archive = archive_path(&state,&id,&name)?;
    if !archive.is_file() { return Err((StatusCode::NOT_FOUND, "找不到備份檔".into())); }
    if inst.maintenance.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).is_err() {
        return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into()));
    }
    if inst.process.lock().await.child.is_some() {
        inst.maintenance.store(false,Ordering::SeqCst);
        return Err((StatusCode::CONFLICT, "還原前必須先正常停止 Server".into()));
    }
    if state.jobs.read().await.get(&format!("core:{id}")).is_some_and(|j| j.state == "working") {
        inst.maintenance.store(false,Ordering::SeqCst);
        return Err((StatusCode::CONFLICT, "核心安裝中，不能還原".into()));
    }
    state.jobs.write().await.insert(format!("backup:{id}"), Job {
        state:"working".into(), detail:"正在驗證備份 ZIP 與解壓到暫存目錄…".into(), percent:None,
    });
    // Keep the response job name outside the async move block.
    let response_job = format!("backup:{id}");
    tokio::spawn(async move {
        let _guard = MaintenanceGuard(inst.clone());
        let data_dir = state.data_dir.as_ref().clone();
        let backup_id = id.clone();
        let result = tokio::task::spawn_blocking(move || restore_zip(&archive,&data_dir,&backup_id))
            .await.map_err(|e| format!("還原工作中斷：{e}"))
            .and_then(|v| v);
        let status = match result {
            Ok(path) => Ok(format!("還原完成（保持停止）；還原前資料保存在 {}", path.display())),
            Err(e) => Err(e),
        };
        inst.log(format!("[MineOS] 還原結果：{}", status.as_ref().map(|s| s.as_str()).unwrap_or("失敗，請檢查備份工作狀態"))).await;
        finish_job(&state,&id,status).await;
    });
    Ok(Json(json!({"ok":true,"job":response_job})))
}

fn restore_zip(archive: &Path, data_root: &Path, id: &str) -> Result<PathBuf,String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| format!("ZIP 損壞：{e}"))?;
    if zip.len() > MAX_ARCHIVE_FILES { return Err("ZIP 項目太多".into()); }
    {
        let mut manifest = zip.by_name("manifest.json").map_err(|_| "備份缺少 manifest.json")?;
        if manifest.size() > 64 * 1024 { return Err("備份資訊異常".into()); }
        let mut value = String::new();
        manifest.read_to_string(&mut value).map_err(|e| e.to_string())?;
        let parsed: serde_json::Value = serde_json::from_str(&value).map_err(|e| format!("備份資訊損壞：{e}"))?;
        if parsed["format"] != 1 || parsed["server_id"].as_str() != Some(id) {
            return Err("此備份不屬於目前的伺服器，或備份格式不相容".into());
        }
    }
    let server_root = data_root.join("servers");
    let original = server_root.join(id);
    let stage = server_root.join(format!(".restore-staging-{}",Uuid::new_v4()));
    let safety_root = data_root.join("restore-safety").join(id);
    let safety = safety_root.join(format!("{}-{}",now(),Uuid::new_v4().simple()));
    if !original.is_dir() || is_reparse_or_symlink(&original)? { return Err("原伺服器資料夾不存在或為連結".into()); }
    fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let extraction = (|| -> Result<(),String> {
        let mut total = 0u64;
        let mut seen = HashSet::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
            let raw = entry.name().to_string();
            if raw == "manifest.json" { continue; }
            if !raw.starts_with("server/") { return Err("ZIP 含不屬於伺服器的路徑".into()); }
            if entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) {
                return Err("ZIP 包含符號連結，已拒絕".into());
            }
            let enclosed = entry.enclosed_name().ok_or("備份內存在不安全路徑")?.to_owned();
            let rel = enclosed.strip_prefix("server").map_err(|_| "ZIP 路徑異常")?;
            if rel.as_os_str().is_empty() { continue; }
            if rel.components().any(|c| match c {
                Component::Normal(part) => part.to_string_lossy().contains(':') || part.to_string_lossy().contains('\\'),
                _ => true,
            }) { return Err("ZIP 路徑不安全".into()); }
            if !seen.insert(rel.to_owned()) { return Err("ZIP 包含重複的檔案名稱".into()); }
            total = total.checked_add(entry.size()).ok_or("備份解壓大小溢位")?;
            if total > MAX_RESTORE_BYTES { return Err("備份解壓容量超過安全上限".into()); }
            let dest = stage.join(rel);
            if entry.is_dir() {
                fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            } else {
                if let Some(parent) = dest.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
                let mut output = File::create(&dest).map_err(|e| e.to_string())?;
                let bytes = std::io::copy(&mut entry,&mut output).map_err(|e| e.to_string())?;
                if bytes != entry.size() { return Err("ZIP 檔案長度驗證失敗".into()); }
            }
        }
        fs::create_dir_all(&safety_root).map_err(|e| e.to_string())?;
        fs::rename(&original,&safety).map_err(|e| format!("無法保存還原前資料：{e}"))?;
        if let Err(err) = fs::rename(&stage,&original) {
            let rollback = fs::rename(&safety,&original);
            return Err(format!("無法啟用還原檔案：{err}；回復原資料：{}",
                if rollback.is_ok() {"成功".into()} else {format!("失敗，原資料仍在 {}",safety.display())}));
        }
        Ok(())
    })();
    if extraction.is_err() { let _ = fs::remove_dir_all(&stage); }
    extraction.map(|_| safety)
}

pub(crate) async fn delete_backup(
    State(state): State<AppState>, AxPath((id,name)): AxPath<(String,String)>
) -> ApiResult<Json<serde_json::Value>> {
    let inst = get_instance(&state,&id).await?;
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "備份／還原進行中，無法刪除".into())); }
    let path = archive_path(&state,&id,&name)?;
    async_fs::remove_file(path).await.map_err(internal_io)?;
    Ok(Json(json!({"ok":true})))
}

pub(crate) async fn download_backup(
    State(state): State<AppState>, AxPath((id,name)): AxPath<(String,String)>
) -> ApiResult<Response> {
    get_instance(&state,&id).await?;
    let path = archive_path(&state,&id,&name)?;
    let file = async_fs::File::open(path).await.map_err(internal_io)?;
    let len = file.metadata().await.map_err(internal_io)?.len();
    let stream = ReaderStream::new(file);
    let mut resp = Response::new(Body::from_stream(stream));
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    resp.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from_str(&len.to_string()).map_err(|e| internal_error(e.to_string()))?);
    resp.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
        .map_err(|e| internal_error(e.to_string()))?);
    resp.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(resp)
}

pub(crate) async fn run_scheduler(state: AppState) {
    let mut ticker = time::interval(Duration::from_secs(60));
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let due: Vec<String> = {
            match state.db.lock() {
                Ok(db) => (|| -> Result<Vec<String>,rusqlite::Error> {
                    let mut stmt = db.prepare("SELECT server_id FROM backup_schedules WHERE interval_hours>0 AND next_due>0 AND next_due<=?")?;
                    let rows = stmt.query_map(params![now()], |r| r.get::<_, String>(0))?;
                    let server_ids: Vec<String> = rows.collect::<Result<Vec<_>, _>>()?;
                    Ok(server_ids)
                })().unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        };
        for id in due {
            let _ = begin_backup(state.clone(), id, true).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerConfig;

    #[test]
    fn full_backup_restores_world_and_preserves_current_files() {
        let base = std::env::temp_dir().join(format!("mineos-backup-test-{}",Uuid::new_v4()));
        let id = Uuid::new_v4().to_string();
        let src = base.join("servers").join(&id);
        fs::create_dir_all(src.join("world")).unwrap();
        fs::create_dir_all(src.join("mods")).unwrap();
        fs::write(src.join("world").join("level.dat"),b"original-world").unwrap();
        fs::write(src.join("mods").join("example.jar"),b"testmod").unwrap();
        let cfg = ServerConfig {
            id:id.clone(),name:"test".into(),port:25565,xms_mb:1024,xmx_mb:2048,
            java_command:"java".into(),auto_start:false,auto_restart:false,
            core_type:"fabric".into(),mc_version:"1.21.1".into(),
        };
        let archive = make_zip(&src,&base.join("backups").join(&id),&cfg,"manual").unwrap();
        fs::write(src.join("world").join("level.dat"),b"changed-world").unwrap();
        let safety = restore_zip(&archive,&base,&id).unwrap();
        assert_eq!(fs::read(src.join("world").join("level.dat")).unwrap(),b"original-world");
        assert_eq!(fs::read(src.join("mods").join("example.jar")).unwrap(),b"testmod");
        assert_eq!(fs::read(safety.join("world").join("level.dat")).unwrap(),b"changed-world");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn invalid_zip_will_not_replace_world() {
        let base = std::env::temp_dir().join(format!("mineos-restore-test-{}",Uuid::new_v4()));
        let id = Uuid::new_v4().to_string();
        let src = base.join("servers").join(&id);
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("world.dat"),b"keep-this").unwrap();
        let zip_path = base.join("broken.zip");
        fs::write(&zip_path,b"not-a-zip").unwrap();
        assert!(restore_zip(&zip_path,&base,&id).is_err());
        assert_eq!(fs::read(src.join("world.dat")).unwrap(),b"keep-this");
        let _ = fs::remove_dir_all(base);
    }
}
