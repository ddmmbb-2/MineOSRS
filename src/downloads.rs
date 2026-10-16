//! Official-source downloads for Windows x64 Temurin JDK and Minecraft server cores.
//! Downloads are never trusted simply because an HTTP request succeeded: hashes,
//! archive layout, size limits and final atomic moves are checked.
use std::{
    collections::HashMap,
    fs,
    io::Write,
    time::Duration,
    path::{Path, PathBuf},
};

use axum::{extract::{Path as AxPath, Query, State}, http::StatusCode, Json};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use tokio::{fs as async_fs, io::AsyncWriteExt, process::Command};
use uuid::Uuid;
use crate::{AppState, ApiResult, bad_request, internal_error, get_instance};
use rusqlite::params;

const MAX_JAVA_DOWNLOAD: u64 = 512 * 1024 * 1024;
const MAX_CORE_DOWNLOAD: u64 = 512 * 1024 * 1024;
const MAX_META_BYTES: usize = 8 * 1024 * 1024;
const MAX_JAVA_UNPACK: u64 = 2 * 1024 * 1024 * 1024;
const JAVA_MAJORS: [u32; 5] = [8, 11, 17, 21, 25];

#[derive(Clone, Serialize)]
pub(crate) struct Job {
    pub state: String,
    pub detail: String,
    pub percent: Option<u8>,
}
impl Job {
    fn working(detail: &str) -> Self { Self { state: "working".into(), detail: detail.into(), percent: None } }
}

#[derive(Serialize)]
pub(crate) struct JavaEntry { major: u32, installed: bool, path: Option<String> }

#[derive(Deserialize)]
pub(crate) struct InstallJava { major: u32 }

#[derive(Deserialize)]
pub(crate) struct InstallCore { core: String, version: String }

#[derive(Deserialize)]
pub(crate) struct VersionQuery { core: String }

#[derive(Serialize)]
pub(crate) struct VersionCatalog { versions: Vec<String>, latest: Option<String> }

pub(crate) async fn list_java(State(state): State<AppState>) -> Json<Vec<JavaEntry>> {
    Json(JAVA_MAJORS.iter().map(|major| {
        let path = java_path(&state.data_dir, *major);
        JavaEntry { major: *major, installed: path.is_some(), path: path.map(|p| p.to_string_lossy().to_string()) }
    }).collect())
}

pub(crate) async fn list_jobs(State(state): State<AppState>) -> Json<HashMap<String, Job>> {
    Json(state.jobs.read().await.clone())
}

pub(crate) async fn install_java(State(state): State<AppState>, Json(req): Json<InstallJava>) -> ApiResult<Json<Value>> {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        return Err(bad_request("此安裝器提供 Windows x64 Java，請在 Windows x64 上執行"));
    }
    if !JAVA_MAJORS.contains(&req.major) { return Err(bad_request("Java 版本僅支援 8、11、17、21、25")); }
    if java_path(&state.data_dir, req.major).is_some() { return Err((StatusCode::CONFLICT, "此 Java 已安裝".into())); }
    let key = format!("java:{}", req.major);
    mark_started(&state, &key, "正在查詢 Eclipse Adoptium…").await?;
    let job_key = key.clone();
    tokio::spawn(async move {
        let result = do_install_java(&state, req.major, &job_key).await;
        finish(&state, &job_key, result).await;
    });
    Ok(Json(serde_json::json!({"ok":true,"job":key})))
}

pub(crate) fn java_path(root: &Path, major: u32) -> Option<PathBuf> {
    let dir = root.join("runtimes").join(format!("java{major}"));
    let direct = dir.join("bin").join("java.exe");
    if direct.is_file() { return Some(direct); }
    let children = fs::read_dir(dir).ok()?;
    for entry in children.flatten() {
        let exe = entry.path().join("bin").join("java.exe");
        if exe.is_file() { return Some(exe); }
    }
    None
}

pub(crate) fn resolve_java(root: &Path, command: &str) -> ApiResult<String> {
    if let Some(text) = command.strip_prefix("managed:") {
        let major: u32 = text.parse().map_err(|_| bad_request("Java 選擇格式錯誤"))?;
        if !JAVA_MAJORS.contains(&major) { return Err(bad_request("不支援此 Java 版本")); }
        let exe = java_path(root, major).ok_or_else(|| bad_request(format!("請先至 Java 管理器下載 Java {major}")))?;
        Ok(exe.to_string_lossy().to_string())
    } else {
        Ok(command.into())
    }
}

async fn do_install_java(state: &AppState, major: u32, key: &str) -> Result<String, String> {
    let url = format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&heap_size=normal&image_type=jdk&jvm_impl=hotspot&os=windows&vendor=eclipse");
    let assets: Value = get_json(state, &url).await?;
    let asset = assets.as_array().and_then(|xs| xs.iter().find(|x| x["binary"]["package"]["link"].is_string()))
        .ok_or("Adoptium 沒有提供符合條件的 Windows x64 JDK")?;
    let binary = &asset["binary"]["package"];
    let link = str_field(binary, "link")?;
    let checksum = str_field(binary, "checksum")?;
    if !link.starts_with("https://") || checksum.len() != 64 || !checksum.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Adoptium 發行資訊不完整或格式錯誤".into());
    }
    let dir = state.data_dir.join("runtimes");
    async_fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
    let tempzip = dir.join(format!("._jdk_{major}_{}.zip", Uuid::new_v4()));
    let stage = dir.join(format!("._jdk_{major}_{}", Uuid::new_v4()));
    let result = async {
        set_job(state, key, "正在下載 Java ZIP…", None).await;
        download(state, link, &tempzip, Some(("sha256", checksum.to_string())), MAX_JAVA_DOWNLOAD, key).await?;
        set_job(state, key, "正在檢查並解壓縮 Java…", None).await;
        let stage2 = stage.clone();
        tokio::task::spawn_blocking(move || unpack_java(&tempzip, &stage2))
            .await.map_err(|e| format!("解壓縮工作失敗：{e}"))??;
        let found = find_java_in_dir(&stage).ok_or("下載的 JDK 中找不到 bin/java.exe")?;
        let out = tokio::process::Command::new(&found).arg("-version")
            .output().await.map_err(|e| format!("Java 執行檢查失敗：{e}"))?;
        if !out.status.success() { return Err("下載的 Java 無法正常執行".into()); }
        let finaldir = dir.join(format!("java{major}"));
        if finaldir.exists() { return Err("Java 目標目錄已存在，請先手動檢查".into()); }
        async_fs::rename(&stage, &finaldir).await.map_err(|e| e.to_string())?;
        Ok(format!("Java {major} 已安裝完成；不需要設定 PATH"))
    }.await;
    // Cleanup pending artifacts even on error. tempzip was consumed by spawn_blocking,
    // so cleanup by matching a unique name is done in unpack_java as well.
    let _ = async_fs::remove_dir_all(&stage).await;
    result
}

fn find_java_in_dir(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join("bin").join("java.exe");
    if direct.is_file() { return Some(direct); }
    for child in fs::read_dir(dir).ok()?.flatten() {
        let candidate = child.path().join("bin").join("java.exe");
        if candidate.is_file() { return Some(candidate); }
    }
    None
}

fn unpack_java(zipfile: &Path, stage: &Path) -> Result<(), String> {
    let result = (|| {
        let file = fs::File::open(zipfile).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        if archive.len() > 20_000 { return Err("ZIP 項目數超過安全限制".into()); }
        fs::create_dir_all(stage).map_err(|e| e.to_string())?;
        let mut total = 0u64;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let relative = entry.enclosed_name().ok_or("ZIP 含非法路徑")?;
            if entry.unix_mode().map(|m| (m & 0o170000) == 0o120000).unwrap_or(false) {
                return Err("不允許 ZIP 中的符號連結".into());
            }
            total = total.checked_add(entry.size()).ok_or("ZIP 解壓縮大小溢位")?;
            if total > MAX_JAVA_UNPACK { return Err("ZIP 展開大小超過安全限制".into()); }
            let target = stage.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            } else {
                if let Some(parent) = target.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
                let mut f = fs::File::create(target).map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
                f.flush().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })();
    let _ = fs::remove_file(zipfile);
    result
}

// Forge / NeoForge modern launchers require argfiles generated by their official installer.
// Never try to run them with `java -jar server.jar`.
#[derive(Serialize, Deserialize)]
struct LaunchSpec {
    kind: String,
    file: String,
}

pub(crate) fn core_is_ready(dir: &Path, core: &str) -> bool {
    if ["forge", "neoforge"].contains(&core) {
        return read_launch_spec(dir).map(|spec| launch_file(dir, &spec).is_file()).unwrap_or(false);
    }
    dir.join("server.jar").is_file()
}

fn read_launch_spec(dir: &Path) -> Result<LaunchSpec, String> {
    let text = fs::read_to_string(dir.join(".mineos-launch.json")).map_err(|e| format!("讀取啟動資訊失敗：{e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("啟動資訊損壞：{e}"))
}

fn launch_file(dir: &Path, spec: &LaunchSpec) -> PathBuf {
    dir.join(&spec.file)
}

pub(crate) fn configure_launch(command: &mut Command, dir: &Path, core: &str) -> ApiResult<()> {
    if ["forge", "neoforge"].contains(&core) {
        let spec = read_launch_spec(dir).map_err(bad_request)?;
        if spec.file.contains('\\') || !spec.file.bytes().all(|b| b.is_ascii_alphanumeric() || b"./_+-".contains(&b))
            || spec.file.split('/').any(|x| x == ".." || x.is_empty()) || !launch_file(dir, &spec).is_file() {
            return Err(bad_request("啟動檔案設定無效，請重新安裝核心"));
        }
        match spec.kind.as_str() {
            "args" => { command.arg(format!("@{}", spec.file)); command.arg("nogui"); }
            "jar" => { command.arg("-jar").arg(&spec.file).arg("nogui"); }
            _ => return Err(bad_request("未知的核心啟動模式")),
        }
    } else {
        command.arg("-jar").arg("server.jar").arg("nogui");
    }
    Ok(())
}

fn select_fabric_loader(items: &Value) -> Result<String, String> {
    items.as_array().and_then(|xs| xs.iter().find(|x| x["loader"]["stable"] == true)
        .or_else(|| xs.first())).and_then(|x| x["loader"]["version"].as_str())
        .map(str::to_owned).ok_or("此 Minecraft 版本沒有 Fabric Loader".to_string())
}

// Parse Maven metadata that comes from the fixed trusted endpoint, not user input.
fn metadata_versions(xml: &str) -> Vec<String> {
    xml.split("<version>").skip(1)
        .filter_map(|x| x.split_once("</version>").map(|(v,_)| v))
        .filter(|v| v.len() <= 64 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b)))
        .map(str::to_owned).collect()
}

fn neo_prefix(mc: &str) -> Option<String> {
    if let Some(s) = mc.strip_prefix("1.") {
        let mut parts = s.split('.');
        let major = parts.next()?;
        let minor = parts.next()?;
        if major.parse::<u32>().is_err() || minor.parse::<u32>().is_err() { return None; }
        Some(format!("{major}.{minor}."))
    } else if mc.starts_with("26.") {
        Some(format!("{mc}."))
    } else { None }
}

fn neo_mc_version(version: &str) -> Option<String> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    if major >= 26 { Some(format!("{major}.{minor}")) }
    else if major >= 20 { Some(format!("1.{major}.{minor}")) }
    else { None }
}

async fn get_text(state: &AppState, url: &str) -> Result<String, String> {
    let resp = state.http.get(url).send().await.map_err(|e| format!("官方資料讀取失敗：{e}"))?
        .error_for_status().map_err(|e| format!("官方網站回應錯誤：{e}"))?;
    if resp.content_length().map(|n| n > MAX_META_BYTES as u64).unwrap_or(false) { return Err("官方版本清單過大".into()); }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_META_BYTES { return Err("官方版本清單過大".into()); }
    String::from_utf8(bytes.to_vec()).map_err(|e| format!("官方資料不是 UTF-8：{e}"))
}

async fn get_checksum(state: &AppState, url: &str, length: usize) -> Result<String, String> {
    let text = get_text(state, url).await?;
    let checksum = text.split_whitespace().next().unwrap_or("");
    if checksum.len() != length || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("官方 checksum 格式不正確".into());
    }
    Ok(checksum.to_owned())
}

pub(crate) async fn versions(State(state): State<AppState>, Query(req): Query<VersionQuery>) -> ApiResult<Json<VersionCatalog>> {
    let catalog = match req.core.as_str() {
        "vanilla" => {
            let meta = get_json(&state, "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").await
                .map_err(internal_error)?;
            let versions: Vec<String> = meta["versions"].as_array().ok_or_else(|| internal_error("Mojang 版本清單無效"))?
                .iter().filter(|x| x["type"] == "release").filter_map(|x| x["id"].as_str().map(str::to_string)).take(80).collect();
            VersionCatalog { latest: meta["latest"]["release"].as_str().map(str::to_string), versions }
        }
        "paper" => {
            let data = get_json(&state, "https://fill.papermc.io/v3/projects/paper").await.map_err(internal_error)?;
            let mut versions: Vec<String> = data["versions"].as_object().ok_or_else(|| internal_error("Paper 版本清單無效"))?
                .values().filter_map(Value::as_array).flatten().filter_map(Value::as_str).map(str::to_string).collect();
            versions.sort_by(|a, b| version_key(b).cmp(&version_key(a)));
            versions.dedup();
            let latest = versions.first().cloned();
            VersionCatalog { latest, versions: versions.into_iter().take(80).collect() }
        }
        "fabric" => {
            let entries = get_json(&state, "https://meta.fabricmc.net/v2/versions/game").await.map_err(internal_error)?;
            let versions: Vec<String> = entries.as_array().ok_or_else(|| internal_error("Fabric 版本清單異常"))?
                .iter().filter(|x| x["stable"] == true)
                .filter_map(|x| x["version"].as_str().map(str::to_owned))
                .take(80).collect();
            VersionCatalog { latest: versions.first().cloned(), versions }
        }
        "forge" => {
            let promos = get_json(&state, "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json").await.map_err(internal_error)?;
            let mut versions: Vec<String> = promos["promos"].as_object().ok_or_else(|| internal_error("Forge 官方版本清單無效"))?
                .keys().filter_map(|k| k.strip_suffix("-latest").map(str::to_owned)).collect();
            versions.sort_by(|a,b| version_key(b).cmp(&version_key(a)));
            versions.dedup();
            VersionCatalog { latest: versions.first().cloned(), versions: versions.into_iter().take(80).collect() }
        }
        "neoforge" => {
            let xml = get_text(&state, "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml").await.map_err(internal_error)?;
            let mut versions: Vec<String> = metadata_versions(&xml).iter().filter_map(|x| neo_mc_version(x)).collect();
            versions.sort_by(|a,b| version_key(b).cmp(&version_key(a)));
            versions.dedup();
            VersionCatalog { latest: versions.first().cloned(), versions: versions.into_iter().take(80).collect() }
        }
        _ => return Err(bad_request("未知核心類型")),
    };
    Ok(Json(catalog))
}

fn version_key(s: &str) -> Vec<u32> {
    s.split('.').map(|x| x.parse().unwrap_or(0)).collect()
}

pub(crate) async fn install_core(
    State(state): State<AppState>, AxPath(id): AxPath<String>, Json(req): Json<InstallCore>
) -> ApiResult<Json<Value>> {
    let inst = get_instance(&state, &id).await?;
    if inst.config.read().await.core_type != req.core {
        return Err(bad_request("不可修改伺服器核心種類"));
    }
    begin_core_install(&state, &id, &req.core, &req.version).await?;
    {
        let db = state.db.lock().map_err(|_| internal_error("資料庫鎖定失敗"))?;
        db.execute("UPDATE servers SET mc_version=? WHERE id=?", params![req.version, id])
            .map_err(|e| internal_error(format!("更新 Minecraft 版本失敗：{e}")))?;
    }
    inst.config.write().await.mc_version = req.version;
    Ok(Json(serde_json::json!({"ok":true,"job":format!("core:{id}")})))
}

pub(crate) async fn begin_core_install(state: &AppState, id: &str, core: &str, version: &str) -> ApiResult<()> {
    if !["vanilla", "paper", "fabric", "forge", "neoforge"].contains(&core) { return Err(bad_request("不支援這個核心的線上安裝")); }
    if version.is_empty() || version.len() > 36 || !version.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'-') {
        return Err(bad_request("Minecraft 版本格式錯誤"));
    }
    let inst = get_instance(state, id).await?;
    if inst.maintenance.load(std::sync::atomic::Ordering::SeqCst) { return Err((StatusCode::CONFLICT, "伺服器正在備份／還原".into())); }
    if inst.process.lock().await.child.is_some() { return Err((StatusCode::CONFLICT, "請先停止伺服器".into())); }
    let target = state.data_dir.join("servers").join(id).join("server.jar");
    if target.exists() || state.data_dir.join("servers").join(id).join(".mineos-launch.json").exists() { return Err((StatusCode::CONFLICT, "此伺服器已有核心，不會自動覆寫".into())); }
    let key = format!("core:{id}");
    mark_started(state, &key, "正在查詢伺服器核心…").await?;
    let state = state.clone(); let core = core.to_owned(); let version = version.to_owned(); let id = id.to_owned();
    tokio::spawn(async move {
        let result = do_install_core(&state, &id, &core, &version, &key).await;
        finish(&state, &key, result).await;
    });
    Ok(())
}

async fn do_install_core(state: &AppState, id: &str, core: &str, version: &str, key: &str) -> Result<String, String> {
    if core == "fabric" { return install_fabric(state, id, version, key).await; }
    if core == "forge" || core == "neoforge" { return install_modded(state, id, core, version, key).await; }
    let (link, hash): (String, (&str, String)) = match core {
        "vanilla" => {
            let manifest = get_json(state, "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").await?;
            let meta_url = manifest["versions"].as_array().and_then(|xs| xs.iter().find(|x| x["id"].as_str() == Some(version) && x["type"] == "release"))
                .and_then(|x| x["url"].as_str()).ok_or("找不到指定 Minecraft 正式版本")?;
            let metadata = get_json(state, meta_url).await?;
            let item = &metadata["downloads"]["server"];
            (str_field(item, "url")?.to_string(), ("sha1", str_field(item, "sha1")?.to_string()))
        }
        "paper" => {
            let builds = get_json(state, &format!("https://fill.papermc.io/v3/projects/paper/versions/{version}/builds")).await?;
            let builds = builds.as_array().ok_or("Paper Builds 資料格式錯誤")?;
            let entry = builds.iter().find(|x| x["channel"] == "STABLE" && x["downloads"]["server:default"]["url"].is_string())
                .ok_or("這個 Paper 版本沒有穩定版，請選其他版本")?;
            let file = &entry["downloads"]["server:default"];
            (str_field(file, "url")?.to_string(), ("sha256", str_field(&file["checksums"], "sha256")?.to_string()))
        }
        _ => return Err("未知核心".into()),
    };
    if !link.starts_with("https://") { return Err("伺服器核心下載網址不是 HTTPS".into()); }
    let dir = state.data_dir.join("servers").join(id);
    let stage = dir.join(format!(".server-{}.download", Uuid::new_v4()));
    set_job(state, key, &format!("正在下載 {core} {version}…"), None).await;
    let res = download(state, &link, &stage, Some(hash), MAX_CORE_DOWNLOAD, key).await;
    if let Err(e) = res { let _ = async_fs::remove_file(&stage).await; return Err(e); }
    let dest = dir.join("server.jar");
    if dest.exists() { let _ = async_fs::remove_file(&stage).await; return Err("同時偵測到已有 server.jar".into()); }
    async_fs::rename(&stage, &dest).await.map_err(|e| e.to_string())?;
    Ok(format!("{core} {version} 已下載，請閱讀 EULA 後啟動"))
}

async fn install_fabric(state: &AppState, id: &str, version: &str, key: &str) -> Result<String, String> {
    let loaders = get_json(state, &format!("https://meta.fabricmc.net/v2/versions/loader/{version}")).await?;
    let loader = select_fabric_loader(&loaders)?;
    let installers = get_json(state, "https://meta.fabricmc.net/v2/versions/installer").await?;
    let installer = installers.as_array().and_then(|xs| xs.iter().find(|x| x["stable"] == true)
        .or_else(|| xs.first())).and_then(|x| x["version"].as_str()).ok_or("查不到 Fabric Installer 版本")?;
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{version}/{loader}/{installer}/server/jar");
    let dir = state.data_dir.join("servers").join(id);
    let temp = dir.join(format!(".fabric-{}.download", Uuid::new_v4()));
    set_job(state, key, &format!("正在下載 Fabric {version} / Loader {loader}…"), None).await;
    // Fabric's Meta API currently does not publish a SHA in this endpoint.
    // Validate the JAR container and use only the official HTTPS host.
    download(state, &url, &temp, None, MAX_CORE_DOWNLOAD, key).await?;
    let result = (|| {
        let mut f = fs::File::open(&temp).map_err(|e| e.to_string())?;
        let archive = zip::ZipArchive::new(&mut f).map_err(|e| format!("Fabric 下載內容不是有效的 JAR：{e}"))?;
        if archive.len() == 0 { return Err("Fabric JAR 沒有內容".into()); }
        Ok(())
    })();
    if let Err(e) = result { let _ = async_fs::remove_file(&temp).await; return Err(e); }
    if dir.join("server.jar").exists() { let _ = async_fs::remove_file(&temp).await; return Err("server.jar 已存在".into()); }
    async_fs::rename(&temp, dir.join("server.jar")).await.map_err(|e| e.to_string())?;
    Ok(format!("Fabric {version} / Loader {loader} 安裝完成；首次啟動會取得需要的依賴"))
}

async fn install_modded(state: &AppState, id: &str, core: &str, mc: &str, key: &str) -> Result<String, String> {
    let (url, algorithm, checksum_url) = if core == "forge" {
        let promos = get_json(state, "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json").await?;
        let ver = promos["promos"].get(format!("{mc}-recommended").as_str()).and_then(Value::as_str)
            .or_else(|| promos["promos"].get(format!("{mc}-latest").as_str()).and_then(Value::as_str))
            .ok_or("Forge 官方沒有這個 Minecraft 版本的安裝器")?;
        if !valid_version(ver) { return Err("Forge 發行版號無效".into()); }
        let base = format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{mc}-{ver}/forge-{mc}-{ver}-installer.jar");
        (base.clone(), "sha1", format!("{base}.sha1"))
    } else {
        let prefix = neo_prefix(mc).ok_or("這個 Minecraft 版本沒有對應的 NeoForge Maven 版本")?;
        let xml = get_text(state, "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml").await?;
        let mut available: Vec<String> = metadata_versions(&xml).into_iter().filter(|v| v.starts_with(&prefix)).collect();
        available.sort_by(|a,b| version_key(b).cmp(&version_key(a)));
        let ver = available.iter().find(|v| !v.contains('-')).or_else(|| available.first()).ok_or("NeoForge 尚無此 Minecraft 版本的發行版")?;
        let base = format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{ver}/neoforge-{ver}-installer.jar");
        (base.clone(), "sha256", format!("{base}.sha256"))
    };
    set_job(state, key, &format!("正在取得 {core} 官方 Installer 的檢查碼…"), None).await;
    let digest = get_checksum(state, &checksum_url, if algorithm == "sha1" { 40 } else { 64 }).await?;
    let root = state.data_dir.join("servers").join(id);
    let stage = root.join(format!(".mineos-install-{}", Uuid::new_v4()));
    async_fs::create_dir_all(&stage).await.map_err(|e| e.to_string())?;
    let installer = stage.join("installer.jar");
    let result = async {
        set_job(state, key, &format!("正在下載 {core} 官方 Installer…"), None).await;
        download(state, &url, &installer, Some((algorithm,digest)), MAX_CORE_DOWNLOAD, key).await?;
        // Install into a separate staging directory; world files and settings are untouched.
        let instance = get_instance(state, id).await.map_err(|(_,e)| e)?;
        let cfg = instance.config.read().await.clone();
        let java = resolve_java(&state.data_dir, &cfg.java_command).map_err(|(_,e)| e)?;
        set_job(state, key, &format!("正在執行 {core} 官方安裝器（可能需要幾分鐘）…"), None).await;
        let output = tokio::time::timeout(Duration::from_secs(900),
            Command::new(java).arg("-jar").arg("installer.jar").arg("--installServer")
                .current_dir(&stage).kill_on_drop(true).output()).await
            .map_err(|_| "官方安裝器執行超過 15 分鐘，已中止".to_owned())?
            .map_err(|e| format!("無法啟動 Installer：{e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!("Installer 失敗（{}）：{} {}", output.status,
                stdout.chars().rev().take(1300).collect::<String>().chars().rev().collect::<String>(),
                stderr.chars().rev().take(1300).collect::<String>().chars().rev().collect::<String>()));
        }
        let spec = discover_launch(&stage, core)?;
        set_job(state, key, "Installer 執行完成，正在整理伺服器檔案…", None).await;
        // Stage is in the same NTFS volume. Do not replace user's existing configs or worlds.
        for entry in fs::read_dir(&stage).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            if matches!(name.to_string_lossy().as_ref(), "installer.jar" | "server.properties" | "eula.txt") || name.to_string_lossy().ends_with(".log") { continue; }
            let target = root.join(&name);
            if target.exists() { return Err(format!("目標已存在，停止安裝避免覆寫：{}", target.display())); }
            fs::rename(entry.path(), &target).map_err(|e| format!("搬移安裝內容失敗：{e}"))?;
        }
        let launch = serde_json::to_vec_pretty(&spec).map_err(|e| e.to_string())?;
        fs::write(root.join(".mineos-launch.json"), launch).map_err(|e| e.to_string())?;
        Ok(format!("{core} {mc} 安裝完成，已設定 Windows 專用啟動參數"))
    }.await;
    let _ = async_fs::remove_dir_all(&stage).await;
    result
}

fn discover_launch(stage: &Path, core: &str) -> Result<LaunchSpec, String> {
    let base = stage.join("libraries").join("net");
    let family = if core == "forge" { "minecraftforge/forge" } else { "neoforged/neoforge" };
    let root = base.join(family);
    if root.is_dir() {
        for version in fs::read_dir(&root).map_err(|e| e.to_string())? {
            let version = version.map_err(|e| e.to_string())?;
            let args = version.path().join("win_args.txt");
            if args.is_file() {
                let relative = args.strip_prefix(stage).map_err(|e| e.to_string())?
                    .to_string_lossy().replace('\\', "/");
                return Ok(LaunchSpec { kind: "args".into(), file: relative });
            }
        }
    }
    // Legacy Forge installers generate a runnable root-level Forge JAR.
    if core == "forge" {
        for file in fs::read_dir(stage).map_err(|e| e.to_string())? {
            let file = file.map_err(|e| e.to_string())?;
            let n = file.file_name().to_string_lossy().to_string();
            if n.starts_with("forge-") && n.ends_with(".jar") && !n.contains("installer") {
                return Ok(LaunchSpec { kind: "jar".into(), file: n });
            }
        }
    }
    Err(format!("{core} 安裝器執行完成，但找不到 Windows 的 win_args.txt 或可啟動 JAR"))
}

fn valid_version(s: &str) -> bool {
    !s.is_empty() && s.len() <= 36 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
}

async fn get_json(state: &AppState, url: &str) -> Result<Value, String> {
    let resp = state.http.get(url).send().await.map_err(|e| format!("下載版本資訊失敗：{e}"))?
        .error_for_status().map_err(|e| format!("官方 API 回應失敗：{e}"))?;
    resp.json::<Value>().await.map_err(|e| format!("官方 JSON 解析失敗：{e}"))
}

fn str_field<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key].as_str().ok_or_else(|| format!("官方資料缺少 {key}"))
}

async fn download(state: &AppState, url: &str, target: &Path, checksum: Option<(&str, String)>, limit: u64, key: &str) -> Result<(), String> {
    let result = async {
        let resp = state.http.get(url).send().await.map_err(|e| e.to_string())?
            .error_for_status().map_err(|e| format!("HTTP 下載錯誤：{e}"))?;
        let expected = resp.content_length();
        if expected.map(|n| n > limit).unwrap_or(false) { return Err("下載檔超過大小上限".into()); }
        let mut file = async_fs::File::create(target).await.map_err(|e| e.to_string())?;
        let mut stream = resp.bytes_stream();
        let mut bytes = 0u64;
        let mut sha256 = Sha256::new();
        let mut sha1 = Sha1::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            bytes = bytes.checked_add(chunk.len() as u64).ok_or("下載大小溢位")?;
            if bytes > limit { return Err("下載檔超過大小上限".into()); }
            sha256.update(&chunk); sha1.update(&chunk);
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
            if let Some(size) = expected {
                if size > 0 { set_job(state, key, &format!("下載中：{} / {} MB", bytes / 1_048_576, size / 1_048_576), Some(((bytes * 100 / size).min(100)) as u8)).await; }
            }
        }
        file.flush().await.map_err(|e| e.to_string())?;
        drop(file);
        if bytes == 0 { return Err("下載檔為空".into()); }
        if let Some((algorithm, expected)) = checksum {
            let actual = match algorithm { "sha256" => hex::encode(sha256.finalize()), "sha1" => hex::encode(sha1.finalize()), _ => return Err("未知的校驗演算法".into()) };
            if !actual.eq_ignore_ascii_case(&expected) { return Err(format!("{algorithm} 雜湊值不符，檔案已丟棄")); }
        }
        Ok(())
    }.await;
    if result.is_err() { let _ = async_fs::remove_file(target).await; }
    result
}

async fn mark_started(state: &AppState, key: &str, message: &str) -> ApiResult<()> {
    let mut jobs = state.jobs.write().await;
    if jobs.get(key).map(|j| j.state == "working").unwrap_or(false) {
        return Err((StatusCode::CONFLICT, "相同的下載工作正在執行".into()));
    }
    jobs.insert(key.into(), Job::working(message));
    Ok(())
}
async fn set_job(state: &AppState, key: &str, detail: &str, percent: Option<u8>) {
    state.jobs.write().await.insert(key.into(), Job { state: "working".into(), detail: detail.into(), percent });
}
async fn finish(state: &AppState, key: &str, result: Result<String, String>) {
    let (status, message) = match result { Ok(message) => ("done", message), Err(message) => ("error", message) };
    state.jobs.write().await.insert(key.into(), Job { state: status.into(), detail: message, percent: if status == "done" {Some(100)} else {None} });
}
