//! Legacy Paperclip Mojang JAR cache compatibility for MineOSRS on Windows.
//!
//! Only targets older Paper builds (Minecraft Java 1.8.x-1.12.2).
//! Avoids touching Vanilla/Fabric/Forge/NeoForge/Bedrock.
//! Reuses the already user-tested official-Mojang PowerShell recovery method.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn is_legacy_paper_version(version: &str) -> bool {
    let pieces: Vec<_> = version.split('.').collect();
    if pieces.len() < 2 || pieces.len() > 3 {
        return false;
    }
    let numbers: Option<Vec<u32>> = pieces.iter().map(|s| s.parse::<u32>().ok()).collect();
    let Some(v) = numbers else { return false; };
    if v[0] != 1 { return false; }
    (8..=11).contains(&v[1]) || (v[1] == 12 && v.get(2).copied().unwrap_or(0) <= 2)
}

fn ps_literal(input: &str) -> String {
    // Single quotes are escaped rather than interpolated as PowerShell expressions.
    input.replace('\'', "''")
}

fn describe_output(output: &Output) -> String {
    let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !err.is_empty() && !out.is_empty() {
        format!("{out}\n{err}")
    } else if !err.is_empty() { err }
    else if !out.is_empty() { out }
    else { format!("PowerShell exit code: {}", output.status) }
}

fn check_and_repair_cache(server_dir: &Path, version: &str) -> Result<(), String> {
    let script = include_str!("legacy_paper_cache.ps1");
    // In-memory script: no writable .ps1 artifact left in the server or temp directory.
    let invocation = format!(
        "$ErrorActionPreference = 'Stop'; [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false);\n& {{\n{}\n}} -ServerDir '{}' -Version '{}'",
        script,
        ps_literal(&server_dir.to_string_lossy()),
        ps_literal(version),
    );
    let mut cmd = Command::new("powershell.exe");
    cmd.arg("-NoLogo").arg("-NoProfile").arg("-NonInteractive")
        .arg("-ExecutionPolicy").arg("Bypass")
        .arg("-Command").arg(invocation);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd.output().map_err(|e| format!("無法啟動 Windows PowerShell：{e}"))?;
    let message = describe_output(&output);
    if output.status.success() {
        println!("[MineOS][Paper] {message}");
        Ok(())
    } else {
        Err(format!("自動補齊 Minecraft {version} 原版 JAR 失敗：{message}"))
    }
}

/// Call after the EULA / core checks and before spawning the Java process.
/// Blocking PowerShell/network work runs outside the Tokio worker thread.
/// On a previously-verified cache, the bundled PowerShell script can work offline.
pub async fn prepare_if_needed(server_dir: &Path, version: &str) -> Result<(), String> {
    if !is_legacy_paper_version(version) {
        return Ok(());
    }
    if !server_dir.join("server.jar").is_file() {
        return Err(format!("舊版 Paper {version} 的 server.jar 不存在，請先安裝 Paper 核心"));
    }
    println!("[MineOS][Paper] Minecraft {version}：檢查 Mojang 原版 JAR 快取…");
    let path: PathBuf = server_dir.to_path_buf();
    let ver = version.to_owned();
    tokio::task::spawn_blocking(move || check_and_repair_cache(&path, &ver))
        .await
        .map_err(|e| format!("Paper 快取檢查工作中斷：{e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_legacy_versions() {
        for v in ["1.8", "1.8.8", "1.9.4", "1.10.2", "1.11.2", "1.12", "1.12.1", "1.12.2"] {
            assert!(is_legacy_paper_version(v), "{v}");
        }
        for v in ["", "1", "1.12.3", "1.13", "1.16.5", "1.20.1", "26.1", "snapshot", "1.12.1.jar", "../1.12.1"] {
            assert!(!is_legacy_paper_version(v), "{v}");
        }
    }
    #[test]
    fn literal_quotes() {
        assert_eq!(ps_literal("D:\\My's Server"), "D:\\My''s Server");
    }
}
