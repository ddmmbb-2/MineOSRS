//! V0.6: conservative import of official Windows Bedrock Dedicated Server ZIP.
//! Never overwrite an existing world or executable. Do not treat an arbitrary ZIP
//! as trusted: administrator should download it from minecraft.net.
use std::{fs, io::{Read, Write}, path::{Path as FsPath, PathBuf}, sync::atomic::Ordering};
use axum::{extract::{Multipart, Path as AxPath, State}, http::StatusCode, Json};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use crate::{AppState, ApiResult, get_instance, bad_request, internal_error, internal_io, set_bedrock_ports};

const MAX_BDS_ZIP: u64 = 512 * 1024 * 1024;
const MAX_EXPANDED: u64 = 2 * 1024 * 1024 * 1024;

pub(crate) async fn upload_bedrock_zip(
    State(state): State<AppState>, AxPath(id): AxPath<String>, mut multipart: Multipart
) -> ApiResult<Json<Value>> {
    let inst = get_instance(&state, &id).await?;
    let cfg = inst.config.read().await.clone();
    if cfg.core_type != "bedrock" { return Err(bad_request("只有 Bedrock 伺服器可匯入 BDS ZIP")); }
    if inst.maintenance.load(Ordering::SeqCst) { return Err((StatusCode::CONFLICT,"備份或還原期間不可匯入".into())); }
    if inst.process.lock().await.child.is_some() { return Err((StatusCode::CONFLICT,"請先停止伺服器".into())); }
    let root = state.data_dir.join("servers").join(&id);
    if root.join("bedrock_server.exe").exists() { return Err((StatusCode::CONFLICT,"已有 Bedrock 執行檔，不會覆蓋；請先另行備份".into())); }
    let mut field = multipart.next_field().await.map_err(|e| bad_request(e.to_string()))?
        .ok_or_else(|| bad_request("沒有 ZIP 檔案"))?;
    let filename = field.file_name().unwrap_or("").to_lowercase();
    if !filename.ends_with(".zip") { return Err(bad_request("只允許官方 Bedrock Windows ZIP")); }
    let stage = root.join(format!(".mineos-bds-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&stage).await.map_err(internal_io)?;
    let source = stage.join("bedrock.zip");
    let outcome: ApiResult<()> = async {
        let mut target = tokio::fs::File::create(&source).await.map_err(internal_io)?;
        let mut received = 0u64;
        while let Some(chunk) = field.chunk().await.map_err(|e| bad_request(e.to_string()))? {
            received = received.checked_add(chunk.len() as u64).ok_or_else(|| bad_request("ZIP 過大"))?;
            if received > MAX_BDS_ZIP { return Err(bad_request("BDS ZIP 不可超過 512 MB")); }
            target.write_all(&chunk).await.map_err(internal_io)?;
        }
        target.flush().await.map_err(internal_io)?;
        drop(target);
        if received < 8 { return Err(bad_request("ZIP 過小")); }
        let source_clone = source.clone();
        let root_clone = root.clone();
        let extract = stage.join("unpacked");
        tokio::task::spawn_blocking(move || safe_import(&source_clone, &extract, &root_clone))
            .await.map_err(|e| internal_error(format!("ZIP 處理失敗：{e}")))?
            .map_err(bad_request)?;
        set_bedrock_ports(&root.join("server.properties"), cfg.port).map_err(internal_io)?;
        Ok(())
    }.await;
    let _ = tokio::fs::remove_dir_all(&stage).await;
    outcome?;
    inst.log("[MineOS] Bedrock 官方 Windows ZIP 已匯入；請確認 EULA 再啟動").await;
    Ok(Json(serde_json::json!({"ok":true,"file":"bedrock_server.exe"})))
}

fn safe_import(source: &FsPath, extract: &FsPath, root: &FsPath) -> Result<(), String> {
    let f=fs::File::open(source).map_err(|e|e.to_string())?;
    let mut zip=zip::ZipArchive::new(f).map_err(|e|format!("ZIP 無法讀取：{e}"))?;
    if zip.len()>10_000 {return Err("ZIP 檔案數量超過安全限制".into());}
    fs::create_dir_all(extract).map_err(|e|e.to_string())?;
    let mut expanded=0u64;
    for i in 0..zip.len() {
        let mut entry=zip.by_index(i).map_err(|e|e.to_string())?;
        if entry.unix_mode().map(|mode| mode & 0o170000 == 0o120000).unwrap_or(false) {
            return Err("ZIP 包含符號連結，拒絕匯入".into());
        }
        let path=entry.enclosed_name().ok_or("ZIP 含非法路徑")?.to_path_buf();
        // BDS official Windows ZIP uses a flat top-level executable.
        let output=extract.join(&path);
        if entry.is_dir() { fs::create_dir_all(&output).map_err(|e|e.to_string())?; continue; }
        expanded=expanded.checked_add(entry.size()).ok_or("解壓縮大小超限")?;
        if expanded>MAX_EXPANDED{return Err("解壓縮後總量超過 2 GB".into());}
        if let Some(parent)=output.parent(){ fs::create_dir_all(parent).map_err(|e|e.to_string())?; }
        let mut target=fs::File::create(&output).map_err(|e|e.to_string())?;
        let copied=std::io::copy(&mut entry,&mut target).map_err(|e|e.to_string())?;
        if copied != entry.size() { return Err("ZIP 內容長度異常".into()); }
        target.flush().map_err(|e|e.to_string())?;
    }
    let exe=extract.join("bedrock_server.exe");
    if !exe.is_file() {return Err("找不到 bedrock_server.exe，請使用 Windows 專用官方 ZIP".into());}
    let mut magic=[0u8;2];
    fs::File::open(&exe).and_then(|mut f| f.read_exact(&mut magic)).map_err(|e|e.to_string())?;
    if &magic!=b"MZ" {return Err("Windows 執行檔格式異常".into());}
    // Never replace existing configs/worlds; official template files may safely be skipped.
    let preserved=["server.properties","permissions.json","allowlist.json","eula.txt"];
    let mut movable=Vec::<(PathBuf,PathBuf)>::new();
    for item in fs::read_dir(extract).map_err(|e|e.to_string())? {
        let item=item.map_err(|e|e.to_string())?;
        let name=item.file_name().to_string_lossy().to_string();
        let dest=root.join(&name);
        if dest.exists() {
            if preserved.iter().any(|n| name.eq_ignore_ascii_case(n)) { continue; }
            return Err(format!("目標檔案已存在，拒絕覆蓋：{name}"));
        }
        movable.push((item.path(),dest));
    }
    // Safety: staging and target share a filesystem, no replacement of existing files.
    for (from,to) in &movable {
        fs::rename(from,to).map_err(|e|format!("搬移 BDS 檔案失敗：{e}；請檢查伺服器資料夾"))?;
    }
    Ok(())
}
