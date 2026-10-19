# MineOSRS V0.7.0 — AI-oriented Refactor

原始版本：V0.7.0。這份是**相容性優先的程式碼重構**，不是新功能版。

## 相容性承諾

- **外觀及操作流程不變**：保留 WebUI 所有畫面元素、表單 ID、文字、CSS 順序與既有 JavaScript；只將 V0.7 內嵌的 CSS／JS 外部化為獨立檔案。
- **REST API 不變**：原本 32 個路由保留，包括登入／伺服器管理／Java／下載／Bedrock／日誌／備份；只額外提供兩個靜態資源端點。
- **資料不搬遷**：保留 SQLite 檔案、原有 schema migration、伺服器 UUID 資料夾、備份資料夾；既有世界檔案不主動修改。
- **啟動行為不變**：保留 Windows Java、Minecraft 版本及核心安裝、EULA、Auto Start、Auto Restart、備份排程及舊 Paper 相容修復。
- **設定參數規則不變**：建立／更新伺服器共用 Port、記憶體、Java 路徑驗證，原本合法／非法的條件未改動。

## AI 修改導航

`src/main.rs` 只作為共用依賴宣告與組裝入口，`src/core/*.rs` 是**編譯期切片**（Rust `include!`，不新增跨模組 adapter），讓 AI 可針對一個功能讀寫單一檔案，同時減低一開始就更動所有 handler 的風險。

| 任務 | 檔案 |
|---|---|
| AppState、ServerConfig、執行個體模型 | `src/core/state.rs` |
| 程式啟動、DB 初始化、路由與背景工作 | `src/core/bootstrap.rs` |
| Token、密碼、Session 與認證 | `src/core/auth.rs` |
| 網頁靜態資源與回應 | `src/core/assets.rs` |
| 建立、更新、清單與管理伺服器 | `src/core/servers.rs` |
| 欄位驗證與單元測試 | `src/core/validation.rs` |
| Java/Bedrock 子程序、Port、Console 與監控 | `src/core/process.rs` |
| 上傳 JAR、EULA、檔案清單 | `src/core/uploads.rs` |
| HTTP 錯誤對應 | `src/core/errors.rs` |
| Java 與 Minecraft 核心下載 | `src/downloads.rs` |
| Bedrock ZIP 安裝 | `src/bedrock.rs` |
| 備份、還原、排程 | `src/backups.rs` |
| 舊 Paper 核心快取自動修復 | `src/legacy_paper.rs`、`src/legacy_paper_cache.ps1` |
| CPU、RAM、GPU、硬碟 | `src/hardware.rs` |
| 原本 WebUI 邏輯（未改變） | `web/app.js` |
| V0.7 左側導覽外觀及互動 | `web/ui-v07.css`、`web/ui-v07.js` |
| HTML 結構與表單 | `web/index.html` |

另可參考 `refactor-map.json` 作為機器可讀的路由與模組索引。

## Windows 編譯與啟動

在已安裝 **Rust stable / Cargo / Windows C++ 建置工具** 的 PowerShell 中：

```powershell
cd <解壓後的 V0.7.0 資料夾>
cargo test
cargo run --release
# 或直接執行：
.\scripts\build-release.ps1
```

預設瀏覽 `http://127.0.0.1:3000`，需要更改監聽位置時設定 `MINEOS_BIND`。請勿直接將 HTTP 管理介面暴露於網際網路。

資料位置**不是** EXE 旁邊（Windows 已設定 `LOCALAPPDATA` 的一般使用者）。程式預設將資料寫在 `%LOCALAPPDATA%\MineOS-Win`；如有設定 `MINEOS_DATA_DIR` 則以該位置為準。請先備份資料夾內的 `mineos.db`、`servers`、`backups`、`admin-token.txt` 及其他設定，再做版本替換。若以不同的 Windows 身分執行，使用者目錄可能不同。

## 相容性回歸檢查

```powershell
python scripts/verify-refactor.py "V0.7.0(1).zip"
node --check web/app.js
node --check web/ui-v07.js
cargo test
cargo build --release
```

第一個指令比對原版 ZIP 的可見 DOM、V0.7 CSS / JS、其他 Rust 功能檔及 API route；不用連線網路。`cargo test` / `cargo build` 請務必在 Windows Rust 環境實際執行。對執行中的 Minecraft 伺服器，建議先備份再手動測試：啟動、停止、Console、核心安裝、備份還原及自動啟動。

**檢驗狀態**：已執行原始 ZIP 靜態相容性檢查及 JavaScript 語法檢查；產生此版本的環境未提供 Rust / Cargo，因此未實際完成 Rust 編譯，也未聲稱通過 Windows 上的 Minecraft 實機回歸測試。
