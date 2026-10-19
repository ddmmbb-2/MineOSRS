# MineOSRS

**Minecraft Java & Bedrock Server Manager for Windows — Powered by Rust**

MineOSRS 是一套使用 **Rust** 開發、以 **Windows** 為主要運行平台的 Minecraft 伺服器管理工具。透過內建 WebUI，就能在同一台電腦上建立、啟動、停止及管理多台伺服器，不必為每一台伺服器手動編寫批次檔。

> **目前版本：v0.6（開發／測試版）**  
> 已進行 Windows 本機測試。正式使用或升級既有世界前，請先備份完整資料目錄。

## v0.6 更新重點

- **新增 Bedrock Dedicated Server（基岩版）支援**：建立 Bedrock 伺服器後，可匯入 Minecraft 官方提供的 Windows BDS ZIP；由 MineOSRS 管理啟動、停止、Console 與備份。
- **改良各核心的版本選擇**：Vanilla、Paper、Fabric、Forge、NeoForge 可從官方來源查詢 Minecraft 版本；不再只顯示前 80 筆版本。
- **可選指定的核心 Build／Loader**：Paper Build、Fabric Loader、Forge Build 及 NeoForge 發行版本可依官方提供的清單選擇，也可留空交由程式自動選擇。
- **改良核心重新安裝流程**：Java 伺服器可重新選擇版本；適用的 JAR 核心更換時保留舊檔。Forge／NeoForge 不提供原地覆蓋，請另建伺服器。
- **保留原有管理功能**：多伺服器、Java 管理、插件／模組上傳、Console、自動重啟、備份與還原等功能沿用。
- **相容既有設定**：新增核心 Build 資料欄位，延續既有 SQLite 資料管理方式；仍建議升級前完整備份。

## 主要功能

| 類別 | 功能 |
| --- | --- |
| 多伺服器管理 | 各伺服器使用獨立的資料夾、世界與連接埠，可同時運行 |
| 伺服器操作 | 啟動、停止、重新啟動、自動啟動及異常重啟 |
| 核心管理 | 官方核心安裝、歷史版本查詢、Build／Loader 選擇 |
| Java 環境 | 下載與管理 Java 8／11／17／21／25，不必自行修改系統 PATH |
| Web Console | 查看執行日誌及傳送伺服器指令 |
| 插件／模組 | 上傳 JAR 至對應的 `plugins/` 或 `mods/` 目錄 |
| 備份與還原 | 手動備份、定時備份、ZIP 下載、保留份數及還原 |
| 管理登入 | 首次登入 Token、管理密碼與登入 Session |
| Bedrock 管理 | 官方 Windows BDS ZIP 匯入、原生程序啟停、UDP Port 設定 |

## 支援的 Minecraft 核心

| 核心 | 版本與安裝方式 | 說明 |
| --- | --- | --- |
| **Vanilla** | Mojang 官方 Minecraft 版本 | 原版 Java 伺服器 |
| **Paper** | Minecraft 版本 + Paper Build | 適用 Paper 插件 |
| **Fabric** | Minecraft 版本 + Fabric Loader | 適用 Fabric 模組 |
| **Forge** | Minecraft 版本 + Forge Build | 透過官方 Installer 安裝 |
| **NeoForge** | Minecraft 版本 + NeoForge 發行版本 | 透過官方 Installer 安裝 |
| **Bedrock** | 手動匯入官方 Windows BDS ZIP | 不需要 Java，使用原生執行檔 |
| **Manual** | 手動上傳 `server.jar` | 適用其他可直接由 Java 啟動的 JAR |

**版本選擇注意事項**

- 可選的 Minecraft 版本及 Build 以各核心**官方實際提供**的清單為準，不代表任何版本都一定有相容的核心或安裝檔。
- Bedrock 在 v0.6 **尚未提供歷史版本自動下載或 Build 清單**。請由 [Minecraft 官方 Bedrock Server 頁面](https://www.minecraft.net/en-us/download/server/bedrock)取得 Windows 專用 ZIP，再從 WebUI 匯入。
- Java 版與 Bedrock 版是不同的伺服器類型；本工具**不會自動提供兩者跨版本連線或跨平台互通**。
- Forge／NeoForge 的安裝與啟動方式不同於一般 `server.jar`，請勿直接替換 JAR 來升級。

## 系統需求

- **作業系統**：Windows 10／11，64 位元（Windows Server 尚未完整驗證）。
- **編譯工具**：[Rust 與 Cargo](https://rustup.rs/)；Windows 編譯可能需要 Visual Studio C++ Build Tools。
- **網路**：下載 Java、Java 版核心及其依賴時，需要連線至官方來源。
- **記憶體**：視 Minecraft 版本、模組數量與同時運行的伺服器數量決定。

> 目前以 Windows 為主要目標；不保證在 Linux／Wine 上可正常執行。

## 快速開始

### 1. 取得原始碼

可在 GitHub 點選 **Code → Download ZIP**，或使用：

```powershell
git clone https://github.com/ddmmbb-2/MineOSRS.git
cd MineOSRS
```

### 2. 編譯與啟動

在原始碼資料夾執行：

```powershell
cargo run --release
```

也可以直接編譯 EXE：

```powershell
cargo build --release
```

完成後的執行檔預設位於：

```text
target\release\mineos-win.exe
```

> GitHub 專案名稱是 `MineOSRS`，目前 Rust 套件與 EXE 檔名仍沿用 `mineos-win`。WebUI 靜態檔案會編入執行檔。

### 3. 登入 WebUI

在執行管理器的 Windows 電腦上開啟：

**http://127.0.0.1:3000**

第一次啟動後，可使用以下檔案內的管理 Token 登入：

```text
%LOCALAPPDATA%\MineOS-Win\admin-token.txt
```

登入後可在 WebUI 設定自己的管理密碼。

### 4. 建立 Java Edition 伺服器

1. 在 Java 管理頁面下載需要的 Java 版本。
2. 點選新增伺服器，選擇 Vanilla、Paper、Fabric、Forge 或 NeoForge。
3. 選擇 Minecraft 版本，視核心類型選擇 Build／Loader（也可採用自動選擇）。
4. 設定名稱、Port、Java、`Xms` 與 `Xmx`。
5. 建立伺服器，等待核心安裝完成，確認 Minecraft EULA 後啟動。

不同版本的插件與模組並不保證互相相容；安裝前請確認其對應 Minecraft 版本及核心。

### 5. 建立 Bedrock Edition 伺服器

1. 新增伺服器時選擇 **Bedrock 基岩版**。
2. 設定名稱與 UDP Port（常見預設值為 `19132`）。Bedrock **不需要選擇 Java 或 JVM 記憶體**。
3. 建立伺服器後，前往 [Minecraft 官方 Bedrock Server 下載頁](https://www.minecraft.net/en-us/download/server/bedrock)，下載 **Windows 專用** BDS ZIP。
4. 在該伺服器的管理頁面選擇 ZIP，點選匯入。
5. 確認相關授權條款及伺服器設定後啟動，在 Console 檢查啟動資訊。

**Bedrock 注意事項：**

- 主要使用 **UDP**，請確認 Windows 防火牆、路由器及外部連線設定。
- MineOSRS 會管理 `server-port`、`server-portv6`，並關閉 LAN 自動探索以降低多伺服器連接埠衝突。
- ZIP 匯入有檔案大小與安全檢查，不會直接覆蓋已存在的 Bedrock 執行檔或世界資料。
- Bedrock 的 Add-ons、行為包／資源包與 Java 插件／模組是不同機制，不可混用。

## 多伺服器與資料目錄

預設資料位置：

```text
%LOCALAPPDATA%\MineOS-Win\
├── admin-token.txt           # 首次登入 Token
├── mineos.db                 # 管理資料庫
├── runtimes\                 # Java 執行環境
├── servers\                  # 各伺服器資料夾
├── backups\                  # 備份 ZIP
└── restore-safety\           # 還原前安全副本
```

可在啟動 MineOSRS 前指定自己的資料路徑：

```powershell
$env:MINEOS_DATA_DIR = "D:\MinecraftServers"
.\target\release\mineos-win.exe
```

這項設定**不會自動搬移舊資料**。切換路徑、移機或升級前，請停止所有伺服器並備份完整資料夾（包含 `mineos.db`）。

## 備份與還原

- 可以執行單台伺服器的完整 ZIP 備份，也可設定自動備份間隔與保留份數。
- 執行中的伺服器進行備份時，管理器會先停止該伺服器，完成後再啟動；期間玩家會斷線。
- 還原操作須在伺服器停止後進行；還原前會建立安全副本。
- **單台伺服器的備份不等於整套管理器備份**：`mineos.db`、登入設定與共用 Java 環境須另外備份。
- 切換 Minecraft 版本或核心前，建議先建立備份；尤其降版可能導致世界資料或模組不相容。

## 安全提醒與已知限制

- WebUI 預設僅供本機使用。不要把沒有 HTTPS 防護的管理介面直接暴露到公網。
- 僅安裝可信任的核心、JAR、插件、模組及 Bedrock ZIP。
- Bedrock v0.6 採官方 ZIP **手動匯入**，不提供歷史版一鍵下載或自動升級。
- 尚未提供正式的 Windows Service 安裝、多使用者細部權限、內建 HTTPS、完整的線上模組商店及自動模組依賴管理。
- MineOSRS 仍在開發與測試階段；正式伺服器建議定期建立異地備份。

## 意見回報

如果發現問題，歡迎前往 [GitHub Issues](https://github.com/ddmmbb-2/MineOSRS/issues) 回報。請盡量附上 MineOSRS 版本、Minecraft 版本、核心及 Build、操作步驟與相關錯誤訊息；貼出紀錄前請移除 Token、密碼與其他敏感資訊。

## 授權與聲明

本專案採用 **MIT License**，請參閱 [LICENSE](LICENSE)。

MineOSRS 是獨立開發的第三方管理工具，並非 Mojang、Microsoft、PaperMC、Fabric、Forge 或 NeoForge 的官方產品。Minecraft、相關標誌及第三方軟體商標皆屬各自權利人；使用各核心及發行版時請遵守其授權條款。

---

**MineOSRS — Manage your Minecraft Java & Bedrock servers from one Windows dashboard.**
