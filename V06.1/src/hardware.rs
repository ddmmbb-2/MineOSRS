//! MineOSRS V0.6.1 Windows host hardware / resource monitor.
//! Route this only inside the existing authenticated /api router.
//! GPU names come from Windows CIM (not GPU usage telemetry).
use axum::Json;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use sysinfo::{Disks, System};

static HOST_SYSTEM: OnceLock<Mutex<System>> = OnceLock::new();
static GPU_NAMES: OnceLock<Vec<String>> = OnceLock::new();

#[cfg(windows)]
fn query_gpu_names() -> Vec<String> {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = cmd
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg("[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name")
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let mut names = Vec::new();
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let name = line.trim().trim_start_matches('\u{feff}');
                if !name.is_empty() && !names.iter().any(|x: &String| x == name) {
                    names.push(name.to_owned());
                }
            }
            names
        }
        _ => Vec::new(),
    }
}

#[cfg(not(windows))]
fn query_gpu_names() -> Vec<String> { Vec::new() }

/// Optional NVIDIA-only telemetry, if nvidia-smi is available on PATH.
/// Other GPU vendors remain name-only; never infer VRAM from 32-bit WMI AdapterRAM.
fn nvidia_metrics() -> Vec<Value> {
    let mut cmd = std::process::Command::new("nvidia-smi");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let output = cmd.args([
        "--query-gpu=name,memory.total,memory.used,utilization.gpu",
        "--format=csv,noheader,nounits",
    ]).output();
    let mut result = Vec::new();
    if let Ok(output) = output {
        if !output.status.success() { return result; }
        for line in String::from_utf8_lossy(&output.stdout).lines().take(8) {
            let parts: Vec<_> = line.split(',').map(str::trim).collect();
            if parts.len() != 4 { continue; }
            let total = parts[1].parse::<u64>().ok();
            let used = parts[2].parse::<u64>().ok();
            let usage = parts[3].parse::<f32>().ok();
            if let (Some(total), Some(used), Some(usage)) = (total,used,usage) {
                result.push(json!({
                    "name": parts[0], "vram_total_mib": total,
                    "vram_used_mib": used, "usage_percent": usage.clamp(0.0,100.0),
                }));
            }
        }
    }
    result
}

fn snapshot_inner() -> Value {
    let sys_lock = HOST_SYSTEM.get_or_init(|| Mutex::new(System::new_all()));
    let mut sys = match sys_lock.lock() {
        Ok(s) => s,
        Err(_) => return json!({"error":"主機監控資料暫時無法存取"}),
    };
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let total_mem = sys.total_memory();
    let used_mem = sys.used_memory().min(total_mem);
    let logical = sys.cpus().len();
    let brand = sys.cpus().first().map(|c| c.brand().to_owned()).unwrap_or_else(|| "未知 CPU".into());
    let cpu_pct = f64::from(sys.global_cpu_usage()).clamp(0.0, 100.0);
    // Limit disk enumeration to local/visible volumes, skipping zero-capacity entries.
    let disks: Vec<Value> = Disks::new_with_refreshed_list().iter()
        .filter(|d| d.total_space() > 0)
        .take(12)
        .map(|d| json!({
            "name": d.name().to_string_lossy(),
            "mount": d.mount_point().display().to_string(),
            "total_bytes": d.total_space(),
            "free_bytes": d.available_space(),
        }))
        .collect();
    // Cached: GPU enumeration via Windows CIM should not run on each 5s poll.
    let gpus = GPU_NAMES.get_or_init(query_gpu_names);
    let nvidia = nvidia_metrics();
    json!({
        "cpu_model": brand,
        "cpu_usage_percent": cpu_pct,
        "physical_cores": System::new().physical_core_count(),
        "logical_threads": logical,
        "total_memory_bytes": total_mem,
        "used_memory_bytes": used_mem,
        "available_memory_bytes": sys.available_memory(),
        "gpus": gpus,
        "nvidia_metrics": nvidia,
        "disks": disks,
        "os": System::long_os_version().or_else(System::name).unwrap_or_else(|| "未知".into()),
        "uptime_seconds": System::uptime(),
    })
}

/// No inputs or arbitrary process execution: authenticated host-only GET endpoint.
pub(crate) async fn snapshot() -> Json<Value> {
    match tokio::task::spawn_blocking(snapshot_inner).await {
        Ok(data) => Json(data),
        Err(_) => Json(json!({"error":"主機資訊取得失敗"})),
    }
}
