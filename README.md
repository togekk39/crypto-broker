# Crypto Broker Portfolio (Rust + Dioxus Web)

這是一個可在本機執行的單頁加密貨幣投資組合追蹤網站，使用 **Rust + Dioxus (Web)** 開發，介面模擬券商 App 的資訊架構。

## 功能

- 單頁 Web App（非桌面版）
- 首頁包含：標題、簡介、CTA 按鈕、3 張功能卡
- 顯示投資組合總覽（總市值、總盈虧、區間盈虧）
- 顯示各幣種持倉、總盈虧與區間盈虧
- 可切換時間區間：`7D / 30D / 90D / ALL`
- 依鏈別（Chain）設定外部報價來源（REST / WebSocket）
- 手動觸發「從外部來源更新報價」，自動寫入最新價格與歷史點位
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

## 外部報價來源設定方法

本專案支援「每條鏈一組報價來源設定」，可用於主流 EVM、Bitcoin 與 Cosmos 生態鏈。

### 1) 進入設定區塊

在頁面中的 **「外部報價來源設定（依鏈分類）」** 區塊，逐條鏈設定下列欄位：

- **來源類型（Source Kind）**
  - `RESTful API`
  - `WebSocket`
- **Endpoint Template**
  - 使用 `{symbol}` 作為幣種占位符，例如：`BTC`、`ETH`
- **Price JSON Path**
  - 指向回應中的價格欄位，支援巢狀路徑（用 `.` 分隔），例如：`price`、`data.last`
- **WS Subscribe Template**
  - 若為 WebSocket，可填入訂閱訊息 JSON，並使用 `{symbol}` 占位符

### 2) REST 設定範例

- 來源類型：`RESTful API`
- Endpoint Template：`https://api.binance.com/api/v3/ticker/price?symbol={symbol}USDT`
- Price JSON Path：`price`

若 `symbol = BTC`，實際請求 URL 會是：

`https://api.binance.com/api/v3/ticker/price?symbol=BTCUSDT`

### 3) WebSocket 設定範例

- 來源類型：`WebSocket`
- Endpoint Template：`wss://stream.binance.com:9443/ws`
- WS Subscribe Template：`{"method":"SUBSCRIBE","params":["{symbol}usdt@trade"],"id":1}`
- Price JSON Path：`p`（實際路徑依供應商訊息格式調整）

### 4) 更新報價

按下 **「從外部來源更新報價」** 後，系統會：

1. 依每個資產的 `chain` 對應設定來源抓取最新價格。
2. 更新 `current_price`。
3. 在 `history` 新增一筆時間點（用於 7D/30D/90D/ALL 區間盈虧計算）。

### 5) 資料保存與相容性說明

- 所有設定與資產資料都儲存在瀏覽器 IndexedDB。
- 新增欄位已提供反序列化預設值，舊版已保存資料可持續載入。
