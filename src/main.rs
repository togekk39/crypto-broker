//! 單頁 Dioxus Web 應用：加密貨幣投資組合追蹤器。
//! 所有資料都存在瀏覽器 IndexedDB，不需要後端或資料庫服務。

use dioxus::prelude::*;
use futures_channel::oneshot;
use futures_util::StreamExt;
use gloo_net::http::Request;
use js_sys::{Date, Promise};
use serde::{Deserialize, Serialize};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{ErrorEvent, Event, IdbDatabase, IdbOpenDbRequest, IdbRequest, IdbTransactionMode, MessageEvent, WebSocket};

const DB_NAME: &str = "crypto_broker_db";
const STORE_NAME: &str = "portfolio";
const STATE_KEY: &str = "state";

/// 可切換的區間。用來顯示近 7/30/90 天或全部的盈虧資訊。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum TimeRange {
    D7,
    D30,
    D90,
    All,
}

impl TimeRange {
    fn label(self) -> &'static str {
        match self {
            Self::D7 => "7D",
            Self::D30 => "30D",
            Self::D90 => "90D",
            Self::All => "ALL",
        }
    }

    fn days(self) -> Option<f64> {
        match self {
            Self::D7 => Some(7.0),
            Self::D30 => Some(30.0),
            Self::D90 => Some(90.0),
            Self::All => None,
        }
    }
}

/// 單一持倉資料。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct Asset {
    id: String,
    symbol: String,
    chain: Chain,
    quantity: f64,
    avg_cost: f64,
    current_price: f64,
    history: Vec<PricePoint>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Chain {
    Bitcoin,
    Ethereum,
    Bnb,
    Solana,
    Polygon,
    Avalanche,
    Arbitrum,
    Optimism,
    Base,
    CosmosHub,
    Osmosis,
    Juno,
    Injective,
}

impl Chain {
    const ALL: [Self; 13] = [
        Self::Bitcoin,
        Self::Ethereum,
        Self::Bnb,
        Self::Solana,
        Self::Polygon,
        Self::Avalanche,
        Self::Arbitrum,
        Self::Optimism,
        Self::Base,
        Self::CosmosHub,
        Self::Osmosis,
        Self::Juno,
        Self::Injective,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Bitcoin => "Bitcoin",
            Self::Ethereum => "Ethereum",
            Self::Bnb => "BNB Chain",
            Self::Solana => "Solana",
            Self::Polygon => "Polygon",
            Self::Avalanche => "Avalanche",
            Self::Arbitrum => "Arbitrum",
            Self::Optimism => "Optimism",
            Self::Base => "Base",
            Self::CosmosHub => "Cosmos Hub",
            Self::Osmosis => "Osmosis",
            Self::Juno => "Juno",
            Self::Injective => "Injective",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum QuoteSourceKind {
    Rest,
    WebSocket,
}

impl QuoteSourceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Rest => "RESTful API",
            Self::WebSocket => "WebSocket",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct ChainQuoteConfig {
    chain: Chain,
    kind: QuoteSourceKind,
    endpoint_template: String,
    price_path: String,
    ws_subscribe_template: String,
}

/// 價格時間點，用於區間盈虧計算。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct PricePoint {
    timestamp_ms: f64,
    price: f64,
}

/// 存到 IndexedDB 的完整應用狀態。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct PortfolioState {
    assets: Vec<Asset>,
    quote_sources: Vec<ChainQuoteConfig>,
}

impl Default for PortfolioState {
    fn default() -> Self {
        Self {
            assets: vec![
                Asset::new("BTC", Chain::Bitcoin, 0.25, 52000.0, 68000.0),
                Asset::new("ETH", Chain::Ethereum, 1.5, 2600.0, 3400.0),
            ],
            quote_sources: Chain::ALL
                .iter()
                .map(|chain| ChainQuoteConfig {
                    chain: *chain,
                    kind: QuoteSourceKind::Rest,
                    endpoint_template: "https://api.binance.com/api/v3/ticker/price?symbol={symbol}USDT".to_string(),
                    price_path: "price".to_string(),
                    ws_subscribe_template: r#"{"method":"SUBSCRIBE","params":["{symbol}usdt@trade"],"id":1}"#.to_string(),
                })
                .collect(),
        }
    }
}

impl Asset {
    /// 建立新資產，會自動補上示範歷史資料（方便第一次開啟就看得到區間切換效果）。
    fn new(symbol: &str, chain: Chain, quantity: f64, avg_cost: f64, current_price: f64) -> Self {
        let id = format!("{}-{}", symbol, Date::now());
        let mut history = Vec::with_capacity(4);
        let now = Date::now();
        let day_ms = 86_400_000.0;

        // 模擬不同時間點的價格，讓 7/30/90 天切換有可見變化。
        history.push(PricePoint {
            timestamp_ms: now - 90.0 * day_ms,
            price: current_price * 0.72,
        });
        history.push(PricePoint {
            timestamp_ms: now - 30.0 * day_ms,
            price: current_price * 0.88,
        });
        history.push(PricePoint {
            timestamp_ms: now - 7.0 * day_ms,
            price: current_price * 0.95,
        });
        history.push(PricePoint {
            timestamp_ms: now,
            price: current_price,
        });

        Self {
            id,
            symbol: symbol.to_uppercase(),
            chain,
            quantity,
            avg_cost,
            current_price,
            history,
        }
    }

    fn market_value(&self) -> f64 {
        self.quantity * self.current_price
    }

    fn cost_basis(&self) -> f64 {
        self.quantity * self.avg_cost
    }

    fn total_pnl(&self) -> f64 {
        self.market_value() - self.cost_basis()
    }

    fn range_pnl(&self, range: TimeRange) -> f64 {
        let Some(start_days) = range.days() else {
            return self.total_pnl();
        };

        let now = Date::now();
        let threshold = now - start_days * 86_400_000.0;
        let mut base_price = self.avg_cost;

        for point in &self.history {
            if point.timestamp_ms >= threshold {
                base_price = point.price;
                break;
            }
            base_price = point.price;
        }

        self.quantity * (self.current_price - base_price)
    }
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    // 主要應用狀態：持倉清單。
    let mut state = use_signal(PortfolioState::default);
    // UI 狀態：目前選取的時間範圍。
    let mut selected_range = use_signal(|| TimeRange::D30);
    let mut loading = use_signal(|| true);

    // 儲存工作序列化：所有寫入都經過同一個協程，避免快點擊造成舊快照覆蓋新狀態。
    let save_queue = use_coroutine(|mut rx: UnboundedReceiver<PortfolioState>| async move {
        while let Some(next) = rx.next().await {
            if let Err(err) = save_state(&next).await {
                web_sys::console::error_1(
                    &JsValue::from_str(&format!("Failed to save state to IndexedDB: {:?}", err)),
                );
            }
        }
    });

    // 表單欄位採獨立 signal，避免整體重繪成本。
    let mut symbol = use_signal(|| String::new());
    let mut chain = use_signal(|| Chain::Bitcoin);
    let mut quantity = use_signal(|| "0.0".to_string());
    let mut avg_cost = use_signal(|| "0.0".to_string());
    let mut current_price = use_signal(|| "0.0".to_string());

    // 初次載入時，嘗試從 IndexedDB 還原資料。
    use_effect(move || {
        spawn(async move {
            match load_state().await {
                Ok(Some(saved)) => state.set(saved),
                Ok(None) => {
                    let _ = save_state(&state()).await;
                }
                Err(err) => {
                    web_sys::console::error_1(
                        &JsValue::from_str(&format!("Failed to load state from IndexedDB: {:?}", err)),
                    );
                }
            }
            loading.set(false);
        });
    });

    // 聚合資料在渲染前計算，減少 JSX 內重複計算。
    let total_market_value: f64 = state.read().assets.iter().map(Asset::market_value).sum();
    let total_cost: f64 = state.read().assets.iter().map(Asset::cost_basis).sum();
    let total_pnl = total_market_value - total_cost;
    let range_total_pnl: f64 = state
        .read()
        .assets
        .iter()
        .map(|a| a.range_pnl(selected_range()))
        .sum();

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }

        main { class: "min-h-screen bg-slate-950 text-slate-100",
            section { class: "mx-auto max-w-6xl px-4 py-10 md:px-6 lg:px-8",
                header { class: "rounded-2xl border border-slate-800 bg-slate-900/70 p-6 shadow-xl md:p-10",
                    h1 { class: "text-3xl font-extrabold tracking-tight md:text-5xl", "Crypto Broker Portfolio" }
                    p { class: "mt-3 max-w-2xl text-slate-300 md:text-lg", "像券商 App 一樣追蹤個別幣種與整體投資組合，支援區間切換與即時盈虧檢視。" }
                    button {
                        class: "mt-6 inline-flex rounded-xl bg-emerald-500 px-4 py-2 font-semibold text-slate-950 hover:bg-emerald-400",
                        onclick: move |_| {
                            let _ = web_sys::window()
                                .and_then(|w| w.alert_with_message("已準備好開始管理你的投資組合！").ok());
                        },
                        "開始管理"
                    }
                }

                div { class: "mt-6 grid gap-4 md:grid-cols-3",
                    FeatureCard { title: "即時持倉總覽", desc: "快速查看總資產、總成本、總盈虧。" }
                    FeatureCard { title: "幣種盈虧分析", desc: "每個資產都可查看漲跌與持倉績效。" }
                    FeatureCard { title: "時間區間切換", desc: "支援 7D/30D/90D/ALL 的變化追蹤。" }
                }

                if loading() {
                    p { class: "mt-8 text-slate-300", "正在從 IndexedDB 載入資料..." }
                } else {
                    section { class: "mt-8 rounded-2xl border border-slate-800 bg-slate-900/60 p-5",
                        h2 { class: "text-xl font-bold", "投資組合總覽" }
                        div { class: "mt-4 grid gap-3 sm:grid-cols-3",
                            StatItem { label: "總市值", value: format_currency(total_market_value), highlight: false }
                            StatItem { label: "總盈虧", value: format_currency(total_pnl), highlight: true }
                            StatItem { label: format!("{} 區間盈虧", selected_range().label()), value: format_currency(range_total_pnl), highlight: true }
                        }

                        div { class: "mt-5 flex flex-wrap gap-2",
                            for range in [TimeRange::D7, TimeRange::D30, TimeRange::D90, TimeRange::All] {
                                button {
                                    class: if selected_range() == range { "rounded-lg bg-emerald-500 px-3 py-1 text-sm font-semibold text-slate-950" } else { "rounded-lg bg-slate-800 px-3 py-1 text-sm text-slate-200 hover:bg-slate-700" },
                                    onclick: move |_| selected_range.set(range),
                                    "{range.label()}"
                                }
                            }
                        }
                    }

                    section { class: "mt-6 rounded-2xl border border-slate-800 bg-slate-900/60 p-5",
                        h2 { class: "text-xl font-bold", "新增 / 管理持倉" }
                        div { class: "mt-4 grid gap-3 md:grid-cols-5",
                            InputBox { label: "幣種", placeholder: "BTC", value: symbol(), oninput: move |v| symbol.set(v) }
                            SelectBox {
                                label: "鏈別",
                                value: chain().label().to_string(),
                                options: Chain::ALL.iter().map(|c| c.label().to_string()).collect::<Vec<_>>(),
                                oninput: move |v| {
                                    if let Some(found) = Chain::ALL.iter().find(|c| c.label() == v) {
                                        chain.set(*found);
                                    }
                                }
                            }
                            InputBox { label: "數量", placeholder: "0.5", value: quantity(), oninput: move |v| quantity.set(v) }
                            InputBox { label: "均價", placeholder: "52000", value: avg_cost(), oninput: move |v| avg_cost.set(v) }
                            InputBox { label: "現價", placeholder: "68000", value: current_price(), oninput: move |v| current_price.set(v) }
                        }
                        button {
                            class: "mt-4 rounded-xl bg-blue-500 px-4 py-2 font-semibold text-white hover:bg-blue-400",
                            onclick: move |_| {
                                let parsed_qty = quantity().parse::<f64>().unwrap_or(0.0);
                                let parsed_avg = avg_cost().parse::<f64>().unwrap_or(0.0);
                                let parsed_cur = current_price().parse::<f64>().unwrap_or(0.0);

                                if symbol().trim().is_empty() || parsed_qty <= 0.0 || parsed_avg <= 0.0 || parsed_cur <= 0.0 {
                                    let _ = web_sys::window().and_then(|w| w.alert_with_message("請輸入有效資料").ok());
                                    return;
                                }

                                let mut next = state();
                                next.assets.push(Asset::new(&symbol(), chain(), parsed_qty, parsed_avg, parsed_cur));
                                state.set(next.clone());
                                symbol.set(String::new());
                                quantity.set("0.0".to_string());
                                avg_cost.set("0.0".to_string());
                                current_price.set("0.0".to_string());

                                save_queue.send(next);
                            },
                            "加入持倉"
                        }
                    }

                    section { class: "mt-6 rounded-2xl border border-slate-800 bg-slate-900/60 p-5",
                        h2 { class: "text-xl font-bold", "外部報價來源設定（依鏈分類）" }
                        p { class: "mt-1 text-sm text-slate-300", "每條鏈可選 RESTful API 或 WebSocket。URL 請用 token placeholder（symbol）當幣種占位符，price path 例如 price 或 data.last。" }
                        button {
                            class: "mt-4 rounded-xl bg-emerald-500 px-4 py-2 font-semibold text-slate-950 hover:bg-emerald-400",
                            onclick: move |_| {
                                let snapshot = state();
                                spawn(async move {
                                    match refresh_quotes(snapshot).await {
                                        Ok(next) => {
                                            state.set(next.clone());
                                            save_queue.send(next);
                                        }
                                        Err(err) => {
                                            let _ = web_sys::window().and_then(|w| w.alert_with_message(&format!("更新報價失敗: {err}")).ok());
                                        }
                                    }
                                });
                            },
                            "從外部來源更新報價"
                        }

                        div { class: "mt-4 space-y-3",
                            for cfg in state.read().quote_sources.iter() {
                                div { class: "rounded-xl border border-slate-800 p-3",
                                    p { class: "font-semibold", "{cfg.chain.label()}" }
                                    div { class: "mt-2 grid gap-2 md:grid-cols-2",
                                        SelectBox {
                                            label: "來源類型",
                                            value: cfg.kind.label().to_string(),
                                            options: vec![QuoteSourceKind::Rest.label().to_string(), QuoteSourceKind::WebSocket.label().to_string()],
                                            oninput: {
                                                let chain = cfg.chain;
                                                move |v| {
                                                    let mut next = state();
                                                    if let Some(target) = next.quote_sources.iter_mut().find(|x| x.chain == chain) {
                                                        target.kind = if v == QuoteSourceKind::WebSocket.label() { QuoteSourceKind::WebSocket } else { QuoteSourceKind::Rest };
                                                    }
                                                    state.set(next.clone());
                                                    save_queue.send(next);
                                                }
                                            }
                                        }
                                        InputBox {
                                            label: "Endpoint Template",
                                            placeholder: "https://.../SYMBOL",
                                            value: cfg.endpoint_template.clone(),
                                            oninput: {
                                                let chain = cfg.chain;
                                                move |v| {
                                                    let mut next = state();
                                                    if let Some(target) = next.quote_sources.iter_mut().find(|x| x.chain == chain) {
                                                        target.endpoint_template = v;
                                                    }
                                                    state.set(next.clone());
                                                    save_queue.send(next);
                                                }
                                            }
                                        }
                                        InputBox {
                                            label: "Price JSON Path",
                                            placeholder: "price",
                                            value: cfg.price_path.clone(),
                                            oninput: {
                                                let chain = cfg.chain;
                                                move |v| {
                                                    let mut next = state();
                                                    if let Some(target) = next.quote_sources.iter_mut().find(|x| x.chain == chain) {
                                                        target.price_path = v;
                                                    }
                                                    state.set(next.clone());
                                                    save_queue.send(next);
                                                }
                                            }
                                        }
                                        InputBox {
                                            label: "WS Subscribe Template",
                                            placeholder: "JSON 訂閱訊息模板",
                                            value: cfg.ws_subscribe_template.clone(),
                                            oninput: {
                                                let chain = cfg.chain;
                                                move |v| {
                                                    let mut next = state();
                                                    if let Some(target) = next.quote_sources.iter_mut().find(|x| x.chain == chain) {
                                                        target.ws_subscribe_template = v;
                                                    }
                                                    state.set(next.clone());
                                                    save_queue.send(next);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    section { class: "mt-6 overflow-hidden rounded-2xl border border-slate-800 bg-slate-900/60",
                        h2 { class: "border-b border-slate-800 px-5 py-4 text-xl font-bold", "持倉列表" }
                        div { class: "overflow-x-auto",
                            table { class: "min-w-full text-sm",
                                thead { class: "bg-slate-800/80 text-slate-300",
                                    tr {
                                        th { class: "px-4 py-3 text-left", "幣種" }
                                        th { class: "px-4 py-3 text-left", "鏈別" }
                                        th { class: "px-4 py-3 text-right", "數量" }
                                        th { class: "px-4 py-3 text-right", "均價" }
                                        th { class: "px-4 py-3 text-right", "現價" }
                                        th { class: "px-4 py-3 text-right", "總盈虧" }
                                        th { class: "px-4 py-3 text-right", "區間盈虧" }
                                    }
                                }
                                tbody {
                                    for asset in state.read().assets.iter() {
                                        tr { class: "border-t border-slate-800 hover:bg-slate-800/40",
                                            td { class: "px-4 py-3 font-semibold", "{asset.symbol}" }
                                            td { class: "px-4 py-3", "{asset.chain.label()}" }
                                            td { class: "px-4 py-3 text-right", "{format_float(asset.quantity)}" }
                                            td { class: "px-4 py-3 text-right", "{format_currency(asset.avg_cost)}" }
                                            td { class: "px-4 py-3 text-right", "{format_currency(asset.current_price)}" }
                                            td { class: pnl_class(asset.total_pnl()), "{format_currency(asset.total_pnl())}" }
                                            td { class: pnl_class(asset.range_pnl(selected_range())), "{format_currency(asset.range_pnl(selected_range()))}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn FeatureCard(title: &'static str, desc: &'static str) -> Element {
    rsx! {
        article { class: "rounded-2xl border border-slate-800 bg-slate-900/50 p-5",
            h3 { class: "text-lg font-semibold", "{title}" }
            p { class: "mt-2 text-slate-300", "{desc}" }
        }
    }
}

#[component]
fn StatItem(label: String, value: String, highlight: bool) -> Element {
    let cls = if highlight {
        if value.starts_with('-') {
            "rounded-xl bg-red-500/15 p-3 text-red-300"
        } else {
            "rounded-xl bg-emerald-500/15 p-3 text-emerald-300"
        }
    } else {
        "rounded-xl bg-slate-800/80 p-3 text-slate-100"
    };

    rsx! {
        div { class: cls,
            p { class: "text-xs uppercase tracking-wide text-slate-400", "{label}" }
            p { class: "mt-1 text-lg font-bold", "{value}" }
        }
    }
}

#[component]
fn InputBox(
    label: &'static str,
    placeholder: &'static str,
    value: String,
    oninput: EventHandler<String>,
) -> Element {
    rsx! {
        label { class: "block",
            span { class: "text-sm text-slate-300", "{label}" }
            input {
                class: "mt-1 w-full rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 outline-none ring-emerald-500 focus:ring-2",
                r#type: "text",
                placeholder: "{placeholder}",
                value: "{value}",
                oninput: move |evt| oninput.call(evt.value()),
            }
        }
    }
}

#[component]
fn SelectBox(
    label: &'static str,
    value: String,
    options: Vec<String>,
    oninput: EventHandler<String>,
) -> Element {
    rsx! {
        label { class: "block",
            span { class: "text-sm text-slate-300", "{label}" }
            select {
                class: "mt-1 w-full rounded-lg border border-slate-700 bg-slate-950 px-3 py-2 outline-none ring-emerald-500 focus:ring-2",
                value: "{value}",
                onchange: move |evt| oninput.call(evt.value()),
                for option in options {
                    option { value: "{option}", "{option}" }
                }
            }
        }
    }
}

fn pnl_class(v: f64) -> &'static str {
    if v >= 0.0 {
        "px-4 py-3 text-right text-emerald-300"
    } else {
        "px-4 py-3 text-right text-red-300"
    }
}

fn format_currency(value: f64) -> String {
    format!("{value:+.2} USD")
}

fn format_float(v: f64) -> String {
    format!("{v:.6}")
}

async fn refresh_quotes(mut state: PortfolioState) -> Result<PortfolioState, String> {
    for asset in &mut state.assets {
        let Some(cfg) = state.quote_sources.iter().find(|x| x.chain == asset.chain) else {
            continue;
        };
        let latest = fetch_external_quote(cfg, &asset.symbol).await?;
        asset.current_price = latest;
        asset.history.push(PricePoint {
            timestamp_ms: Date::now(),
            price: latest,
        });
    }
    Ok(state)
}

async fn fetch_external_quote(config: &ChainQuoteConfig, symbol: &str) -> Result<f64, String> {
    let url = config.endpoint_template.replace("{symbol}", symbol);
    match config.kind {
        QuoteSourceKind::Rest => {
            let text = Request::get(&url)
                .send()
                .await
                .map_err(|e| format!("REST request failed for {symbol}: {e}"))?
                .text()
                .await
                .map_err(|e| format!("REST response parse failed for {symbol}: {e}"))?;
            extract_price(&text, &config.price_path)
        }
        QuoteSourceKind::WebSocket => fetch_quote_from_ws(config, &url, symbol).await,
    }
}

async fn fetch_quote_from_ws(config: &ChainQuoteConfig, url: &str, symbol: &str) -> Result<f64, String> {
    let ws = WebSocket::new(url).map_err(|_| format!("WebSocket open failed for {symbol}"))?;
    let (tx, rx) = oneshot::channel::<Result<String, String>>();
    let sender = std::rc::Rc::new(std::cell::RefCell::new(Some(tx)));

    let onmessage_sender = sender.clone();
    let onmessage = Closure::<dyn FnMut(_)>::new(move |evt: MessageEvent| {
        let payload = evt.data().as_string().unwrap_or_default();
        if let Some(tx) = onmessage_sender.borrow_mut().take() {
            let _ = tx.send(Ok(payload));
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    let onerror_sender = sender.clone();
    let onerror = Closure::<dyn FnMut(_)>::new(move |_evt: ErrorEvent| {
        if let Some(tx) = onerror_sender.borrow_mut().take() {
            let _ = tx.send(Err("WebSocket message error".to_string()));
        }
    });
    ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    if !config.ws_subscribe_template.trim().is_empty() {
        let msg = config.ws_subscribe_template.replace("{symbol}", symbol);
        let subscribe_msg = msg.clone();
        let ws_cloned = ws.clone();
        let onopen = Closure::<dyn FnMut(_)>::new(move |_evt: Event| {
            let _ = ws_cloned.send_with_str(&subscribe_msg);
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();
    }

    let raw = rx.await.map_err(|_| "WebSocket channel canceled".to_string())??;
    ws.close().ok();
    extract_price(&raw, &config.price_path)
}

fn extract_price(text: &str, path: &str) -> Result<f64, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("JSON decode failed: {e}"))?;
    let mut cursor = &value;
    for key in path.split('.') {
        cursor = cursor
            .get(key)
            .ok_or_else(|| format!("price path not found: {path}"))?;
    }
    if let Some(n) = cursor.as_f64() {
        return Ok(n);
    }
    if let Some(s) = cursor.as_str() {
        return s
            .parse::<f64>()
            .map_err(|e| format!("price parse failed: {e}"));
    }
    Err(format!("price path is not number/string: {path}"))
}

/// 開啟 IndexedDB 並確保 object store 存在。
async fn open_db() -> Result<IdbDatabase, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let idb = window
        .indexed_db()?
        .ok_or_else(|| JsValue::from_str("IndexedDB unavailable"))?;

    let open_req: IdbOpenDbRequest = idb.open_with_u32(DB_NAME, 1)?;

    {
        let on_upgrade = Closure::<dyn FnMut(_)>::new(move |evt: web_sys::IdbVersionChangeEvent| {
            if let Some(target) = evt.target() {
                if let Ok(req) = target.dyn_into::<IdbOpenDbRequest>() {
                    if let Ok(result) = req.result() {
                        if let Ok(db) = result.dyn_into::<IdbDatabase>() {
                            let _ = db.create_object_store(STORE_NAME);
                        }
                    }
                }
            }
        });
        open_req.set_onupgradeneeded(Some(on_upgrade.as_ref().unchecked_ref()));
        on_upgrade.forget();
    }

    let promise = request_to_promise(open_req.unchecked_into::<IdbRequest>());
    let result = JsFuture::from(promise).await?;
    result.dyn_into::<IdbDatabase>()
}

/// 儲存整份狀態為 JSON 字串到 IndexedDB。
async fn save_state(state: &PortfolioState) -> Result<(), JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_NAME, IdbTransactionMode::Readwrite)?;
    let store = tx.object_store(STORE_NAME)?;

    let payload = serde_json::to_string(state).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let req = store.put_with_key(&JsValue::from_str(&payload), &JsValue::from_str(STATE_KEY))?;
    let _ = JsFuture::from(request_to_promise(req)).await?;
    drop(tx);
    Ok(())
}

/// 從 IndexedDB 讀取狀態，若無資料則回傳 None。
async fn load_state() -> Result<Option<PortfolioState>, JsValue> {
    let db = open_db().await?;
    let tx = db.transaction_with_str_and_mode(STORE_NAME, IdbTransactionMode::Readonly)?;
    let store = tx.object_store(STORE_NAME)?;
    let req = store.get(&JsValue::from_str(STATE_KEY))?;

    let value = JsFuture::from(request_to_promise(req)).await?;
    if value.is_undefined() || value.is_null() {
        return Ok(None);
    }

    let json = value
        .as_string()
        .ok_or_else(|| JsValue::from_str("stored state is not a string"))?;
    let parsed = serde_json::from_str::<PortfolioState>(&json)
        .map_err(|e| JsValue::from_str(&format!("state parse failed: {e}")))?;
    Ok(Some(parsed))
}

/// 把 IDB request 轉為 Promise，便於用 async/await。
fn request_to_promise(request: IdbRequest) -> Promise {
    Promise::new(&mut |resolve, reject| {
        let success_req = request.clone();
        let on_success = Closure::<dyn FnMut()>::new(move || {
            let result = success_req.result().unwrap_or(JsValue::UNDEFINED);
            let _ = resolve.call1(&JsValue::NULL, &result);
        });

        let error_req = request.clone();
        let on_error = Closure::<dyn FnMut()>::new(move || {
            let _ = error_req;
            let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("IDB request failed"));
        });

        request.set_onsuccess(Some(on_success.as_ref().unchecked_ref()));
        request.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        on_success.forget();
        on_error.forget();
    })
}
