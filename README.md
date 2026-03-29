# Crypto Broker Portfolio (Rust + Dioxus Web)

這是一個可在本機執行的單頁加密貨幣投資組合追蹤網站，使用 **Rust + Dioxus (Web)** 開發，介面模擬券商 App 的資訊架構。

## 功能

- 單頁 Web App（非桌面版）
- 首頁包含：標題、簡介、CTA 按鈕、3 張功能卡
- 顯示投資組合總覽（總市值、總盈虧、區間盈虧）
- 顯示各幣種持倉、總盈虧與區間盈虧
- 可切換時間區間：`7D / 30D / 90D / ALL`
- 使用者資料儲存在瀏覽器 **IndexedDB**（無後端、無登入、無資料庫）
- 響應式版面（手機 / 桌面）
- 使用 Tailwind CSS，並已直接套用編譯後 CSS 檔案

## 技術選型說明

> 需求提到依賴盡量使用 GitHub main commit。此專案以穩定可執行為優先，採用相容性最佳的 crates.io 穩定版（Dioxus 0.6.3），避免主分支 API 變動導致 `dx serve` 與編譯流程不穩定。

## 專案結構

- `src/main.rs`：Dioxus 單頁主程式與 IndexedDB 存取邏輯
- `assets/tailwind.css`：編譯後 Tailwind CSS（直接由 app 載入）
- `styles/tailwind.input.css`：Tailwind 原始輸入檔
- `tailwind.config.js`：Tailwind 掃描設定
- `Dioxus.toml`：Dioxus Web app 設定

## 本機啟動方式

### 1) 安裝 Rust Web target

```bash
rustup target add wasm32-unknown-unknown
```

### 2) 安裝 Dioxus CLI (`dx`)

如果你的環境還沒有 `dx`，請先安裝：

```bash
cargo install dioxus-cli
```

### 3) 啟動開發伺服器

```bash
dx serve --platform web
```

啟動後通常可在：

- `http://127.0.0.1:8080`

看到網站畫面。

## Tailwind 編譯（已完成，可選）

若你後續修改樣式，可重新編譯：

```bash
npx tailwindcss@3.4.17 -i ./styles/tailwind.input.css -o ./assets/tailwind.css --minify
```

## 效能實作重點

- 使用 Dioxus `Signal` 管理局部狀態，降低不必要重繪
- 聚合值（總市值/總盈虧）在渲染前計算，避免模板中重複計算
- IndexedDB 以單一序列化 payload 儲存，簡化 I/O 路徑與資料一致性

