# MineOSRS

**Minecraft Java & Bedrock Server Manager for Windows — Built with Rust**

MineOSRS 是一套以 **Rust** 開發、透過 **WebUI** 管理 Minecraft 伺服器的 Windows 工具。可以在同一台電腦建立及管理多台 Java Edition／Bedrock Edition 伺服器，集中處理核心、Java 環境、Console、插件／模組及備份，不需要為每台伺服器各自編寫啟動批次檔。

> **目前主要開發版本：V0.7.0 — AI Refactor**  
> 狀態：開發／測試版。V0.7.0 的介面已改為左側導覽式管理中心；`AI-Refactor` 版本則以**保留功能與外觀**為前提重新整理程式結構，**不是新增功能的另一個版本**。  
> **重要：**已完成原始碼的靜態相容性與 JavaScript 語法檢查，但 AI 重構版尚未完成 Windows Rust 編譯與 Minecraft 實機回歸驗證。請先在測試環境確認，再使用既有世界資料。

**[查看 V0.7.0 AI 重構版原始碼](https://github.com/ddmmbb-2/MineOSRS/tree/main/V0.7.0-AI-Refactor)** · **[回報問題](https://github.com/ddmmbb-2/MineOSRS/issues)** · **[MIT License](LICENSE)**

---

## V0.7.0 更新重點

### 全新的 WebUI 操作方式

- **左側固定功能導覽列**：不再需要在同一個長頁面中尋找功能。
- **右側單一內容區**：選擇功能後，在主要工作區切換對應畫面。
- **預設首頁「本機資訊」**：登入後先查看管理主機的 CPU、記憶體、顯示卡、磁碟及伺服器數量。
- **新增／修改共用表單**：建立伺服器與修改既有伺服器時，使用一致的操作方式。
- **經典／深色配色切換**：可依使用偏好切換介面顏色。
- **分頁式管理功能**：伺服器清單、Java 環境、Console、備份等各自有明確入口。

### V0.7.0 AI Refactor

這個版本的核心目標是：**使用者看到的介面及操作方式不變，內部原始碼改成更容易讓 AI 定位、修改與檢查的形式**。

- 將大型 `main.rs` 的主要職責拆到 `src/core/`，減少修改單一功能時需要閱讀的範圍。
- 保留原有 API 路由、登入機制、SQLite 資料格式及伺服器資料路徑。
- 將 V0.7 原先內嵌於 HTML 的 UI CSS／JavaScript 分離成獨立檔案。
- 將新增及修改伺服器的共用驗證集中處理。
- 新增 `refactor-map.json`，提供 AI／自動化工具查找模組、功能及路由的索引。
- 新增靜態相容性檢查腳本 `scripts/verify-refactor.py`。

> `AI Refactor` 是**偏重相容性的結構重整**，並非已經全面重寫所有下載、備份與程序管理演算法。`src/core/` 目前透過 Rust `include!` 方式組合，仍共享同一個編譯作用域；這是降低此次重構風險的設計，而不是完全獨立的 Rust 模組架構。

## 主要功能

| 功能 | 說明 |
| --- | --- |
| 多伺服器管理 | 建立多台伺服器，各自使用獨立資料夾、Port 與啟動設定 |
| 主機資訊 | 顯示 CPU、RAM、作業系統及磁碟資訊；GPU 顯示依系統支援程度而定 |
| 伺服器控制 | 啟動、停止、重新啟動、自動啟動與異常結束後重啟 |
| 核心安裝 | Vanilla、Paper、Fabric、Forge、NeoForge 的版本與 Build／Loader 選擇 |
| Java 管理 | 下載及選用 Java 8／11／17／21／25，無須修改系統 PATH |
| Web Console | 觀看伺服器輸出、查詢日誌、傳送指令 |
| 插件／模組 | 將 JAR 上傳到對應的 `plugins/`、`mods/` 或核心檔案位置 |
| 備份與還原 | 手動備份、排程備份、保留數量管理、ZIP 下載與還原 |
| 登入管理 | 首次 Token 登入、管理密碼及登入 Session |
| Bedrock 支援 | 匯入 Minecraft 官方 Windows BDS ZIP，管理執行與備份 |
| 舊版 Paper 相容 | 針對部分舊版 Paper 啟動所需的 Mojang 原版 JAR 快取進行檢查／修復 |

**硬體監控說明：**主機資訊顯示的是**整台 Windows 主機**的資源狀態，不是每台 Minecraft 伺服器各自的用量。GPU 名稱可由 Windows 取得；若安裝並可呼叫 `nvidia-smi`，則可額外顯示支援的 NVIDIA 使用率與顯示記憶體數據。其他 GPU 不保證有即時使用率。

## 支援的伺服器類型

| 類型 | 安裝方式 | 注意事項 |
| --- | --- | --- |
| Vanilla | 官方 Minecraft Java 版本 | 原版 Java Server |
| Paper | Minecraft 版本與 Paper Build | 插件需配合 Paper／Minecraft 版本 |
| Fabric | Minecraft 版本與 Fabric Loader | 使用 Fabric 模組 |
| Forge | Minecraft 版本與 Forge Build | 透過 Installer 安裝 |
| NeoForge | Minecraft 版本與 NeoForge 發行版 | 透過 Installer 安裝 |
| Bedrock | 手動匯入官方 Windows BDS ZIP | 不需要 Java；使用 UDP Port |
| Manual | 手動上傳 `server.jar` | 適用可由 Java 直接啟動的其他 JAR 核心 |

核心版本與 Build 能否安裝，取決於各官方來源實際提供的檔案。**Bedrock 不會自動下載歷史版本**；Java 與 Bedrock 也不會因為由同一管理器控制就自動互通。

## 系統需求

- **作業系統：**Windows 10／11 64 位元為主要目標。Windows Server、Linux 及 Wine 尚未完整驗證。
- **編譯環境：**[Rust / Cargo](https://rustup.rs/)；Windows 上通常需要 Visual Studio C++ Build Tools（MSVC 工具鏈）。
- **網路連線：**首次下載 Java、Minecraft 核心或必要依賴時需連線至對應下載來源。
- **電腦資源：**CPU、RAM 與硬碟需求依 Minecraft 版本、玩家數、模組及同時運行的伺服器數量而不同。

## 下載、編譯與啟動

### 1. 取得原始碼

在 Windows PowerShell 執行：

```powershell
git clone https://github.com/ddmmbb-2/MineOSRS.git
cd .\MineOSRS\V0.7.0-AI-Refactor
```

也可以在 GitHub 使用 **Code → Download ZIP**，解壓後進入 `V0.7.0-AI-Refactor` 資料夾。請確認當前目錄包含 `Cargo.toml`。

### 2. 編譯或直接執行

```powershell
# 先執行 Rust 測試
cargo test

# 編譯 Release EXE
cargo build --release

# 或直接編譯並啟動
cargo run --release
```

專案也提供 Windows 腳本：

```powershell
.\scripts\build-release.ps1
# 或
.\scripts\run-dev.ps1
```

編譯完成後的執行檔：

```text
target\release\mineos-win.exe
```

> GitHub 專案名稱為 **MineOSRS**，目前 Cargo 套件名稱與 EXE 檔名仍保留 `mineos-win`。WebUI 的 HTML、CSS、JavaScript 在編譯時會打包進執行檔；使用 EXE 時一般不必另外放置 `web` 資料夾。

### 3. 開啟 WebUI

啟動程式後，在**執行 MineOSRS 的電腦**開啟：

```text
http://127.0.0.1:3000
```

首次啟動會產生管理 Token，Windows 一般使用者的預設位置為：

```text
%LOCALAPPDATA%\MineOS-Win\admin-token.txt
```

使用 Token 登入後，可在管理器設定中建立管理密碼（至少 12 個字元）；設定後以密碼登入。

### 4. 建立第一台伺服器

**Java Edition：**

1. 開啟左側的「Java 環境」，下載符合 Minecraft 版本需求的 Java。
2. 進入「新增／修改伺服器」，填寫名稱、Port、核心及 Minecraft 版本。
3. 依核心選擇 Build／Loader；也可在支援時使用自動選擇。
4. 設定 Java 版本及 `Xms`／`Xmx`，送出並等待核心安裝。
5. 閱讀並同意 [Minecraft EULA](https://www.minecraft.net/eula) 後，再啟動伺服器。

**Bedrock Edition：**

1. 新增伺服器時選擇「Bedrock 基岩版」，設定 UDP Port（常見預設為 `19132`）。
2. 從 [Minecraft 官方 Bedrock Server 下載頁](https://www.minecraft.net/en-us/download/server/bedrock) 取得 **Windows 版** BDS ZIP。
3. 選取該伺服器，在管理頁面匯入 ZIP，確認設定並啟動。

請檢查 Windows 防火牆及路由器的 Port 設定。Java Edition 常用 TCP；Bedrock Edition 主要使用 UDP。

## 資料儲存、升級與備份

Windows 一般使用者預設資料目錄：

```text
%LOCALAPPDATA%\MineOS-Win\
├── admin-token.txt         # 首次登入 Token
├── mineos.db               # SQLite 管理資料庫
├── runtimes\               # 下載的 Java 環境
├── servers\                # 各 Minecraft 伺服器的資料夾
├── backups\                # 伺服器備份 ZIP
└── restore-safety\         # 還原前的安全副本
```

部分資料夾會在對應功能首次使用時才建立。

如需指定其他資料位置，請在啟動前設定環境變數：

```powershell
$env:MINEOS_DATA_DIR = 'D:\MinecraftServers'
.\target\release\mineos-win.exe
```

`MINEOS_DATA_DIR` **只變更讀寫位置，不會替你移動舊資料**。更換版本、搬移設備或切換資料目錄前，請停止伺服器並完整備份原資料夾，尤其是 `mineos.db`、`servers`、`backups` 與認證設定。

備份功能提供手動備份、依小時間隔的排程備份、保留數量及還原。**若備份時伺服器正在執行，管理器會先停止伺服器，備份完成後再重新啟動；玩家將暫時斷線。** 還原前亦須先停止伺服器。單台 Minecraft 伺服器的備份**不等於**整套 MineOSRS 管理器資料的備份。

## AI 重構版的原始碼架構

此版本將功能定位與相容性放在優先順位，方便開發者使用 AI 分段修改，不強制採用供人類逐行閱讀的層層抽象架構。

```text
V0.7.0-AI-Refactor/
├── Cargo.toml
├── Cargo.lock
├── refactor-map.json       # AI／工具使用的功能與路由索引
├── src/
│   ├── main.rs             # 共用相依項與 include! 組裝入口
│   ├── core/
│   │   ├── bootstrap.rs    # 啟動、資料庫、API 路由、背景工作
│   │   ├── state.rs        # 資料模型與共享狀態
│   │   ├── auth.rs         # 登入、密碼與 Session
│   │   ├── servers.rs      # 新增、修改、清單與管理
│   │   ├── validation.rs   # 共用參數驗證與測試
│   │   ├── process.rs      # 執行、停止、Console 與程序監控
│   │   ├── uploads.rs      # JAR、EULA 與檔案管理
│   │   ├── assets.rs       # 內建 WebUI 資源
│   │   └── errors.rs       # API 錯誤處理
│   ├── downloads.rs        # Java 與核心下載
│   ├── bedrock.rs          # Bedrock ZIP 匯入
│   ├── backups.rs          # 備份、還原與排程
│   ├── hardware.rs         # 主機硬體資訊
│   └── legacy_paper.rs     # 舊 Paper 核心相容處理
├── web/
│   ├── index.html
│   ├── app.js               # 原有操作邏輯
│   ├── style.css
│   ├── ui-v07.js            # V0.7 左側導覽與頁面切換
│   └── ui-v07.css           # V0.7 介面樣式
└── scripts/
    ├── build-release.ps1
    ├── run-dev.ps1
    ├── migrate-data.ps1
    └── verify-refactor.py
```

### AI 協作建議

需要修改功能時，建議先將 `refactor-map.json` 與相關的 `src/core/*.rs`／`web/*.js` 提供給 AI，而不是每次直接重寫整個專案。對外部介面或資料格式的修改，請優先檢查 API 路由、DOM ID、資料庫遷移、檔案路徑及舊版本相容性。

## 測試與相容性驗證

```powershell
# Rust 單元測試及編譯
cargo test
cargo build --release

# 選用：需要已安裝 Node.js
node --check web/app.js
node --check web/ui-v07.js

# 選用：需要 Python 3 與「重構前」V0.7.0 原始 ZIP
python scripts/verify-refactor.py 'D:\Downloads\V0.7.0(1).zip'
```

`verify-refactor.py` 比對原始 V0.7.0 的可見 DOM、原有 UI CSS／JavaScript、部分 Rust 功能檔與 API 路由，**不是**可替代實機測試的完整測試套件。

截至此版公開時，已有靜態相容性與 JavaScript 語法檢查紀錄；**不能據此宣稱 Rust 已在 Windows 成功編譯，或實際 Minecraft 啟停、下載與還原流程都已通過測試**。若測試後發現異常，歡迎至 [Issues](https://github.com/ddmmbb-2/MineOSRS/issues) 回報。

## 安全提醒與目前限制

- WebUI 預設監聽 `127.0.0.1:3000`，僅限管理主機本機存取。可用 `MINEOS_BIND` 修改，但**不建議直接將沒有 HTTPS 的管理介面暴露到公網**。
- 如須遠端存取，請自行部署具備 HTTPS 與存取限制的反向代理或安全通道。
- 請勿分享 `admin-token.txt`、登入密碼、Session 或可能包含敏感資訊的伺服器記錄。
- 僅匯入可信任來源的 JAR、模組、插件與 Bedrock ZIP。
- 尚未提供正式 Windows Service 安裝、細緻的多使用者權限管理、內建 HTTPS 或完整的模組依賴管理。
- 執行 Minecraft 世界升版、降版或更換核心前，應額外備份世界及設定檔。

## 授權與聲明

本專案採用 **MIT License**，詳見 [LICENSE](LICENSE)。

MineOSRS 為獨立開發的第三方管理工具，並非 Mojang、Microsoft、PaperMC、Fabric、Forge 或 NeoForge 的官方產品。Minecraft 與其他商標屬各自權利人；使用各核心、模組或伺服器發行檔時，請遵守其授權條款。

---

**MineOSRS — One Windows dashboard for your Minecraft servers.**
