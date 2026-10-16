# MineOSRS

**Minecraft Java Server Manager for Windows · Powered by Rust**

MineOSRS 是一套專為 Windows 打造的 Minecraft Java 伺服器管理工具。透過內建 **WebUI**，可以在同一台電腦上建立與管理多個 Minecraft Server，不必為每台伺服器手動編寫啟動批次檔。

> **目前版本：v0.5（開發／測試版）**  
> 已在 Windows 上測試多伺服器管理，以及 Vanilla、Paper、Fabric、Forge、NeoForge 的建立與啟動。正式使用前，請先備份重要世界資料。

## 主要功能

| 分類 | 功能 |
| --- | --- |
| 多伺服器 | 多台 Server 同時運行，各自使用獨立 Port、資料夾與世界 |
| 伺服器控制 | 啟動、停止、重新啟動；自動啟動與異常重啟 |
| Java 管理 | 下載與管理 Java 8／11／17／21／25，不必修改 Windows PATH |
| 核心安裝 | Vanilla、Paper、Fabric、Forge、NeoForge |
| 資源設定 | 每台 Server 獨立設定 `Xms`、`Xmx`、Java 版本與 Port |
| Web Console | 查看伺服器日誌、輸入指令 |
| 插件／模組 | 從網頁上傳 JAR 到 `plugins/` 或 `mods/` |
| 備份與還原 | 完整備份、排程、自動保留份數、ZIP 下載與還原 |
| 管理安全 | 首次登入 Token、管理密碼修改及登入 Session |

### 支援的伺服器核心

| 核心 | 安裝方式 |
| --- | --- |
| **Vanilla** | 從 Mojang 官方來源下載 |
| **Paper** | 從 Paper 官方來源下載 |
| **Fabric** | 使用 Fabric 官方 Loader／Server Launcher |
| **Forge** | 執行官方 Installer |
| **NeoForge** | 執行官方 Installer |

也支援手動上傳其他伺服器 JAR。請注意：Forge、NeoForge 等核心具有專用的安裝與啟動流程，不能直接以更換 `server.jar` 的方式取代安裝。

### 多伺服器獨立管理

每台伺服器都可以設定不同的：

- 名稱、Minecraft 版本、核心類型與連接埠（Port）
- Java 執行環境及最小／最大記憶體（`Xms`／`Xmx`）
- 世界、插件、模組與其他伺服器檔案
- 管理器啟動時自動啟服、異常結束後自動重啟

建立後可再次修改名稱、Port、記憶體及 Java 設定；修改前請先停止該伺服器。每台 Server 也有自己的 Console、EULA 狀態與資料夾位置。

### 備份與還原（v0.5）

- **立即備份**：完整備份單一 Server 的世界、玩家資料、核心、插件、模組與設定檔。
- **自動備份**：各 Server 可獨立設定間隔（1～720 小時；`0` 代表關閉）。
- **保留數量**：可設定保留 1～50 份自動備份；手動備份不會被自動輪替刪除。
- **備份管理**：可從 WebUI 查看、下載及刪除 ZIP 備份。
- **一鍵還原**：選擇備份還原；還原前的資料會另外保存在 `restore-safety/`。

**備份安全規則：** 若 Server 正在執行，管理器會先正常停止它、完成備份後再啟動，因此玩家會暫時斷線。還原操作則需要 Server 處於停止狀態，完成後由管理員自行啟動。

> **重要：** 單台 Server 的 ZIP 備份不包含 MineOSRS 的 `mineos.db`、管理帳號資訊及共用 Java runtimes。建議定期另外備份完整的 MineOSRS 資料目錄，並將重要備份複製到另一個儲存裝置。

## 系統需求

- **作業系統**：Windows 10／11（64 位元）；Windows Server 尚未完整驗證。
- **編譯環境**：[Rust 與 Cargo](https://rustup.rs/)；Windows 上可能需要 Visual Studio C++ Build Tools。
- **網路連線**：首次下載 Java、Minecraft 核心與相關依賴時必須連線至官方來源。
- **記憶體**：取決於 Minecraft 版本、模組數量及同時執行的伺服器數量。

## 快速開始

### 1. 取得原始碼

在專案頁面按 **Code → Download ZIP**，解壓縮後以 PowerShell 進入專案目錄；也可以使用 Git：

```powershell
git clone https://github.com/ddmmbb-2/MineOSRS.git
cd MineOSRS
```

### 2. 編譯並啟動管理器

請先安裝 Rust，然後在專案根目錄執行：

```powershell
cargo run --release
```

或者使用專案附帶的啟動腳本：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\run-dev.ps1
```

首次編譯會下載 Rust 套件，所需時間依電腦與網路環境而定。

### 3. 登入 WebUI

在管理器執行的同一台 Windows 電腦開啟：

**http://127.0.0.1:3000**

首次啟動會產生管理 Token，預設檔案位置：

```text
%LOCALAPPDATA%\MineOS-Win\admin-token.txt
```

使用 Token 登入後，可以在 WebUI 的 **「變更管理密碼」** 設定新密碼（至少 12 個字元）。設定完成後，舊 Token 不再作為登入憑證。

### 4. 建立 Minecraft Server

1. 在 **Java 版本管理** 下載需要的 Java。
2. 在 **新增伺服器** 選擇核心、Minecraft 版本，輸入名稱與 Port。
3. 設定 `Xms`、`Xmx`，並選擇對應的 Java。
4. 建立 Server，等待核心與依賴安裝完成。
5. 閱讀並同意 [Minecraft EULA](https://www.minecraft.net/eula)。
6. 點選 **啟動**，在 Console 查看啟動紀錄。

插件與模組可透過所選 Server 的 JAR 上傳區加入。**插件與模組並非跨核心通用**，上傳前請確認與該 Minecraft 版本及核心相容；安裝或更換後建議重新啟動 Server。

> **記憶體提醒：** `Xmx` 是 JVM 堆積記憶體上限，不代表整個 Java 程序的總記憶體使用量；多台 Server 同時運行時，請替 Windows 系統與其他程序保留足夠 RAM。

## 編譯 EXE

在專案根目錄執行：

```powershell
cargo build --release
```

產生的執行檔預設位於：

```text
target\release\mineos-win.exe
```

目前 GitHub 專案名稱為 **MineOSRS**，但 Rust 套件名稱與 EXE 檔名仍沿用 `mineos-win`。WebUI 的 HTML、CSS 與 JavaScript 會在編譯時嵌入執行檔。

## 資料儲存位置

預設使用 Windows 使用者自己的 AppData 目錄（Debug 與 Release 共用）：

```text
%LOCALAPPDATA%\MineOS-Win\
├── admin-token.txt         # 首次登入 Token
├── mineos.db               # 伺服器設定與管理資料
├── runtimes\               # 已下載的 Java
├── servers\                # 各 Server 的實際資料
│   └── <Server UUID>\
├── backups\                # 備份 ZIP
│   └── <Server UUID>\
└── restore-safety\         # 還原前的安全副本
    └── <Server UUID>\
```

若要指定其他資料位置，請在**啟動程式前**設定環境變數：

```powershell
$env:MINEOS_DATA_DIR = "D:\MinecraftServers"
cargo run --release
```

**變更路徑不會自動搬移原有資料。** 搬移前請先停止所有 Minecraft Server、關閉管理器，並備份整個資料目錄（包含 SQLite 資料庫）。

## 安全與使用注意事項

- WebUI 預設監聽 `127.0.0.1:3000`，僅供本機使用。
- **不要直接把未加密的 HTTP 管理介面公開到網際網路。** 若需要遠端存取，應另外配置 HTTPS、身分驗證與網路存取限制。
- 插件與模組 JAR 可能執行任意程式碼，請只安裝可信任的檔案。
- 正式世界的還原操作請先在測試伺服器演練，並確保硬碟有足夠空間。
- **v0.5 尚未實作正式的 Windows Service 安裝**：管理器程序必須持續運作，才能維持其排程與伺服器控制功能。

## 開發狀態

MineOSRS 目前仍是測試版本。下列項目**尚未列為 v0.5 已完成功能**：Modrinth 線上模組商店、模組依賴自動管理、進階玩家圖形化管理、多使用者權限、內建 HTTPS，以及正式 Windows Service 安裝。

如遇問題，歡迎透過 [GitHub Issues](https://github.com/ddmmbb-2/MineOSRS/issues) 提供重現步驟、Minecraft 版本、伺服器核心及錯誤紀錄。貼上紀錄前，請先移除帳號憑證和其他私密資訊。

## 授權與聲明

MineOSRS 採用 **MIT License**，授權內容請參閱專案根目錄的 `LICENSE` 檔案。

本專案是受 MineOS 概念啟發的**獨立開發工具**，不是 MineOS 官方版本，也不隸屬於 Mojang、Microsoft、PaperMC、Fabric、Forge 或 NeoForge。Minecraft 及相關名稱與商標屬於各自權利人；第三方核心與 Java 發行版依各自授權條款提供。

---

**MineOSRS — Manage your Minecraft servers from one Windows dashboard.**
