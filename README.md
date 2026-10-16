# MineOSRS

**適用於 Windows 的 Minecraft Java 伺服器管理平台，以 Rust 開發並內建 WebUI。**

*MineOSRS is an independent, Windows-first Minecraft Java server manager built with Rust, featuring a browser-based dashboard for running multiple servers.*

MineOSRS 讓你在一台 Windows 電腦上管理多個 Minecraft Java 伺服器，不需要為每個伺服器手動撰寫啟動批次檔。透過瀏覽器就能下載 Java、建立伺服器、調整記憶體、查看 Console、上傳插件或模組，以及備份與還原世界。

> **專案狀態：V0.5，開發／測試中。** 已於 Windows 測試多伺服器與 Vanilla、Paper、Fabric、Forge、NeoForge 的建立及啟動；新版本仍可能有問題，請先備份重要世界。

## 主要功能

### 多伺服器管理

- 在同一台 Windows 主機建立多個 Minecraft Server，使用不同 Port。
- 每台伺服器有獨立資料夾、Minecraft 核心、世界、插件／模組及記憶體設定。
- 從 WebUI **啟動、正常停止、重新啟動**伺服器。
- 設定管理器啟動時自動啟動指定伺服器，以及異常結束後自動重啟。
- 建立後仍可修改名稱、Port、`Xms`、`Xmx` 與 Java 執行環境。

### Java 與 Minecraft 核心

- 透過 Eclipse Adoptium 下載 Windows x64 版 Temurin Java（8／11／17／21／25），不必更改系統 `PATH`。
- 不同伺服器可選用不同 Java 版本。
- 支援自動下載／安裝以下伺服器核心：

| 核心 | 建立方式 |
| --- | --- |
| Vanilla | Mojang 官方版本檔 |
| Paper | Paper 官方下載來源 |
| Fabric | Fabric 官方 Loader／Installer 來源 |
| Forge | 官方 Installer |
| NeoForge | 官方 Installer |

- 也提供手動上傳 JAR 的基本選項；**需要專用安裝程序的核心不可直接以 `server.jar` 取代安裝流程**。
- 首次啟動前，可在啟動按鈕旁閱讀並同意 Minecraft EULA；同意後不再重複顯示。

### WebUI 與管理工具

- 管理密碼登入；首次啟動使用自動產生的管理 Token，之後可變更密碼。
- 即時 Console 日誌、輸入 Minecraft 指令。
- 上傳插件 JAR 至 `plugins/`、模組 JAR 至 `mods/`。
- 顯示伺服器資料夾路徑、複製路徑，以及在 Windows 主機開啟資料夾。
- 顯示核心下載與 Java 下載作業狀態。

### 備份與還原（V0.5）

- 一鍵**完整備份**伺服器資料夾，包括世界、玩家資料、模組、插件、核心與設定檔。
- 每台 Server 可設定自動備份間隔（1～720 小時，`0` 為關閉）。
- 設定自動備份保留數量（1～50 份）；不會自動刪除手動備份。
- 在 WebUI 檢視、下載、刪除備份 ZIP，或選擇備份進行還原。
- **一致性優先**：執行中的 Server 會正常停止，備份完成後再啟動；備份期間玩家會暫時斷線。
- 還原前必須停止 Server；還原前原始資料會另存到 `restore-safety/`，成功還原後由管理員手動啟動。

> 單一 Server 的 ZIP 備份**不包含** MineOSRS 的 `mineos.db`、Java runtimes 或管理帳號資訊。請另外保存完整資料目錄，且定期將備份複製到不同硬碟或 NAS。

## 系統需求

- **作業系統**：Windows 10／11 64 位元（Windows Server 尚未完整驗證）。
- **從原始碼編譯**：[Rust / Cargo](https://rustup.rs/)（Windows 平台可能需 Visual Studio C++ Build Tools）。
- **網路**：首次安裝 Java／Minecraft 核心及相關依賴時，需要連線至官方下載來源。
- **記憶體**：依 Minecraft 版本、模組與同時執行的 Server 數量自行配置。

## 快速開始

### 1. 下載原始碼並啟動

在已安裝 Rust 的 Windows PowerShell 中，進入專案目錄：

```powershell
cd D:\path\to\MineOSRS
cargo run --release
```

也可以使用專案內的腳本：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\run-dev.ps1
```

首次編譯需要下載 Rust 套件依賴。

### 2. 開啟 WebUI

在同一台電腦的瀏覽器開啟：

**http://127.0.0.1:3000**

第一次啟動時，程式會建立管理 Token。預設可在下列位置找到：

```text
%LOCALAPPDATA%\MineOS-Win\admin-token.txt
```

登入後，建議立即使用 **「變更管理密碼」** 設定自己的密碼。設定後原本的 Token 不再可用於登入。請勿公開 Token、登入密碼或資料庫。

### 3. 建立 Minecraft Server

1. 在「Java 版本管理」下載需要的 Java。
2. 在「新增伺服器」輸入名稱、Port、Minecraft 版本與核心類型。
3. 設定最小／最大記憶體（`Xms`／`Xmx`），並選擇 Java。
4. 建立伺服器，等待核心安裝完成。
5. 閱讀並確認接受 [Minecraft EULA](https://www.minecraft.net/eula)。
6. 點選「啟動」，透過 Console 確認伺服器載入完成。

**注意：** Java 的 `Xmx` 限制的是 JVM 堆積記憶體，不等於整個 Java 程序的全部 RAM 使用量。多台伺服器同時啟動時，請預留 Windows 系統與非堆積記憶體的空間。

## 建置 EXE

```powershell
cargo build --release
```

產生的執行檔：

```text
target\release\mineos-win.exe
```

目前 EXE 檔名仍沿用內部 Cargo 專案名稱 `mineos-win`；GitHub 專案名稱使用 **MineOSRS**。

## 資料儲存位置

預設資料目錄：

```text
%LOCALAPPDATA%\MineOS-Win\
├── admin-token.txt
├── mineos.db
├── runtimes\
├── servers\
│   └── <Server UUID>\
├── backups\
│   └── <Server UUID>\
└── restore-safety\
    └── <Server UUID>\
```

可以在啟動 MineOSRS **之前**，以環境變數自訂位置：

```powershell
$env:MINEOS_DATA_DIR = "D:\MinecraftServers"
cargo run --release
```

**切換資料路徑不會自動搬移原世界或資料庫。** 升級／搬移前請先停止所有 Minecraft Server、關閉管理器，並備份完整資料目錄。

## 安全注意事項

- 預設 WebUI 只監聽 `127.0.0.1:3000`，供本機使用。
- **請勿直接將未加密的 HTTP 管理介面公開到網際網路。** 若需要遠端管理，應配置 HTTPS 反向代理、身分驗證與存取限制。
- JAR 插件／模組可能執行任意程式碼，僅安裝可信任的檔案。
- 正式世界的還原操作請先在測試伺服器演練；注意剩餘硬碟空間。
- MineOSRS V0.5 **尚未安裝成正式 Windows Service**。排程備份與管理器啟動時自動啟服，需要管理器程序保持運作。

## 目前尚未包含

Modrinth 線上模組商店、模組依賴自動管理、進階玩家圖形化管理、多使用者權限、HTTPS 直接服務，以及正式 Windows Service 安裝等功能，均不屬於目前 V0.5 的已完成功能。

## 授權與聲明

專案在 `Cargo.toml` 宣告採用 **MIT License**。公開原始碼時，請一併加入 MIT 授權全文的 `LICENSE` 檔案。

MineOSRS 是**獨立開發**的 Minecraft 伺服器管理工具，靈感來自 MineOS，但**不是 MineOS 官方版本，也不隸屬於 Mojang、Microsoft、PaperMC、Fabric、Forge 或 NeoForge**。Minecraft 及相關名稱、商標屬於各自權利人；各第三方伺服器核心與 Java 發行版適用各自的授權條款。

---

**MineOSRS — Manage your Minecraft servers from one Windows dashboard.**
