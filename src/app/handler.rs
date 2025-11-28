use {
    crate::app::auto_trade::pair_key_addr,
    crate::app::cfg_bindings::cfg_bindings,
    crate::app::results::{results, results_interactions, ResultsAreas},
    crate::app::auto_trade::{ensure_sell_allowance, manual_sell_all},
    crate::libs::bsc::{
        client::BscClient,
        spells::{format_bnb, get_balance},
    },
    crate::libs::cache::{
        load_autotrade_cache, load_settings_cache, save_autotrade_cache, save_settings_cache,
        SettingsCache,
    },
    crate::libs::config::{load_env, Config},
    crate::libs::lookup::save_log_to_file,
    crate::log,
    crate::libs::writing::cc,
    crate::libs::sim::{DexType, SimEngine, SimPosition},
    crate::libs::tui::{
        centered_rect, draw_box, draw_config_main, draw_main_window, draw_modal, draw_modal_lines,
        draw_modal_pairs, draw_tab_strip, draw_title_bar, new_store_with_defaults, BoxProps,
        ConfigAreas, ConfigStore,
    },
    crate::libs::ws::pairs::{fourmeme_stream, pancakev2_stream, pancakev3_stream, PairInfo},
    alloy::primitives::{utils::parse_units, Address, U256},
    alloy::providers::ProviderBuilder,
    alloy::providers::WalletProvider,
    alloy::signers::local::PrivateKeySigner,
    alloy::signers::Signer,
    anyhow::Result,
    crossterm::{
        event::{self, Event, EventStream, KeyCode, KeyModifiers, MouseButton, MouseEventKind},
        execute,
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    },
    pancakes::pancake::pancake_swap::addresses::*,
    pancakes::pancake::pancake_swap_v2::router::format_token as format_token_v2,
    ratatui::prelude::*,
        std::{
            io::{BufRead, BufReader},
            path::PathBuf,
            str::FromStr,
            time::{Duration, Instant, SystemTime, UNIX_EPOCH},
        },
        url::Url,
    };

alloy::sol! {
    #[sol(rpc)]
    interface IERC20Lite {
        function balanceOf(address owner) view returns (uint256);
    }
}
#[allow(async_fn_in_trait)]
pub trait CalculateFee {
    async fn calculate_fee_usd<P: alloy::providers::Provider + Clone + 'static>(
        &self,
        provider: &P,
    ) -> Result<f64>;
    async fn calculate_fee_str<P: alloy::providers::Provider + Clone + 'static>(
        &self,
        provider: &P,
    ) -> Result<String>;
}

impl CalculateFee for BscClient {
    async fn calculate_fee_usd<P: alloy::providers::Provider + Clone + 'static>(
        &self,
        provider: &P,
    ) -> Result<f64> {
        // gas price in wei
        let gas_price_wei: U256 = match self.raw_call("eth_gasPrice", vec![]).await {
            Ok(val) => {
                if let Some(hex) = val.as_str() {
                    let s = hex.trim_start_matches("0x");
                    let bytes: Vec<u8> = match hex::decode(if s.len() % 2 == 1 {
                        format!("0{s}")
                    } else {
                        s.to_string()
                    }) {
                        Ok(b) => b,
                        Err(_) => Vec::new(),
                    };
                    U256::from_be_slice(&bytes)
                } else {
                    U256::ZERO
                }
            }
            Err(_) => U256::ZERO,
        };
        let gas_cost_wei = gas_price_wei.saturating_mul(U256::from(21_000u64));

        // 1 WBNB -> USDT quote (amount_out in USDT base units)
        match pancakes::plug::price::get_price_v2(provider.clone(), WBNB, USDT).await {
            Ok(q) => {
                let scale_1e18 = U256::from(10u64).pow(U256::from(18u64));
                let fee_usdt_units =
                    gas_cost_wei.saturating_mul(q.amount_out_base_units) / scale_1e18;
                // Convert base units (U256) -> f64 amount
                let fee_units_str = fee_usdt_units.to_string();
                let fee_units_f64 = fee_units_str.parse::<f64>().unwrap_or(0.0);
                let denom = 10f64.powi(q.decimals_out as i32);
                Ok(fee_units_f64 / denom)
            }
            Err(_) => Ok(0.0),
        }
    }

    async fn calculate_fee_str<P: alloy::providers::Provider + Clone + 'static>(
        &self,
        provider: &P,
    ) -> Result<String> {
        // gas price in wei
        let gas_price_wei: U256 = match self.raw_call("eth_gasPrice", vec![]).await {
            Ok(val) => {
                if let Some(hex) = val.as_str() {
                    let s = hex.trim_start_matches("0x");
                    let bytes: Vec<u8> = match hex::decode(if s.len() % 2 == 1 {
                        format!("0{s}")
                    } else {
                        s.to_string()
                    }) {
                        Ok(b) => b,
                        Err(_) => Vec::new(),
                    };
                    U256::from_be_slice(&bytes)
                } else {
                    U256::ZERO
                }
            }
            Err(_) => U256::ZERO,
        };
        let gas_cost_wei = gas_price_wei.saturating_mul(U256::from(21_000u64));

        // 1 WBNB -> USDT quote (amount_out in USDT base units)
        let fee_str = match pancakes::plug::price::get_price_v2(provider.clone(), WBNB, USDT).await
        {
            Ok(q) => {
                let scale_1e18 = U256::from(10u64).pow(U256::from(18u64));
                let fee_usdt_units =
                    gas_cost_wei.saturating_mul(q.amount_out_base_units) / scale_1e18;
                format!("${}", format_token_v2(fee_usdt_units, q.decimals_out))
            }
            Err(_) => "$…".to_string(),
        };
        Ok(fee_str)
    }
}

pub async fn init() -> Result<()> {
    load_env();
    let cfg = Config::new();
    let cli = BscClient::new(cfg.bsc_rpc.clone(), cfg.private_key.clone()).await?;
    let bal = get_balance(&cli, cli.address).await?;
    let chain_id = cli.chain_id().await?;
    let address_display = format!("{}", cli.address);
    let balance_bnb = format_bnb(format!("0x{:x}", bal))?;

    // start ws feeds for v2/v3/fm pairs
    use crate::libs::bsc::client::BscWsClient;
    let ws = BscWsClient::new(cfg.bsc_wss.clone(), cfg.private_key.clone()).await?;
    let provider = {
        let url = Url::parse(&cfg.bsc_rpc)?;
        let signer = PrivateKeySigner::from_str(&cfg.private_key)?.with_chain_id(Some(56));
        ProviderBuilder::new()
            .with_chain_id(56)
            .wallet(signer)
            .connect_http(url)
    };
    let config_store = new_store_with_defaults();
    log!(cc::LIGHT_GREEN, "Selling all FourMeme tokens...");
    startup_liquidate_fm_tokens(provider.clone(), &config_store).await;
    log!(cc::LIGHT_GREEN, "Finished.");

    // Shared channel for Hermes streams
    let (pair_tx, pair_rx) = tokio::sync::mpsc::channel::<(String, String, PairInfo)>(4096);

    let mut app = JimmyTUI::new(
        chain_id,
        &address_display,
        &balance_bnb,
        pair_tx,
        pair_rx,
        ws,
        provider,
        cli.clone(),
        config_store,
    );
    app.run_tui().await?;
    Ok(())
}

pub struct JimmyTUI<P> {
    chain_id: u64,
    address: String,
    balance_bnb: String,
    pair_tx: tokio::sync::mpsc::Sender<(String, String, PairInfo)>,
    pairs_rx: Option<tokio::sync::mpsc::Receiver<(String, String, PairInfo)>>,
    ws: crate::libs::bsc::client::BscWsClient,
    provider: P,
    cli: crate::libs::bsc::client::BscClient,
    config_store: ConfigStore,
    session_started_at: Instant,
}

impl<P> JimmyTUI<P>
where
    P: alloy::providers::Provider + WalletProvider + Clone + Send + Sync + 'static,
{
    pub fn new(
        chain_id: u64,
        address: &str,
        balance_bnb: &str,
        pair_tx: tokio::sync::mpsc::Sender<(String, String, PairInfo)>,
        pairs_rx: tokio::sync::mpsc::Receiver<(String, String, PairInfo)>,
        ws: crate::libs::bsc::client::BscWsClient,
        provider: P,
        cli: crate::libs::bsc::client::BscClient,
        config_store: ConfigStore,
    ) -> Self {
        Self {
            chain_id,
            address: address.to_string(),
            balance_bnb: balance_bnb.to_string(),
            pair_tx,
            pairs_rx: Some(pairs_rx),
            ws,
            provider,
            cli,
            config_store,
            session_started_at: Instant::now(),
        }
    }

    fn draw_wallet_block(&self, f: &mut Frame, left_col: Rect, avg_fee_usd: Option<&str>) -> u16 {
        let addr_short = format!(
            "{}…{}",
            &self.address[..6],
            &self.address[self.address.len().saturating_sub(4)..]
        );
        let wallet_lines = vec![
            format!("Wallet"),
            format!("Address: {}", addr_short),
            format!("Balance: {}", balance_short(&self.balance_bnb)),
            format!("Avg Fee/tx: {}", avg_fee_usd.unwrap_or("…")),
        ];
        let wallet_h = (wallet_lines.len() as u16).saturating_add(2);
        let props = BoxProps {
            offset: (0, 0),
            size: (left_col.width, 0),
            border_color: Color::LightBlue,
            title: "Wallet".into(),
        };
        let area = Rect {
            x: left_col.x,
            y: left_col.y,
            width: left_col.width,
            height: wallet_h,
        };
        draw_box(f, area, wallet_lines, &props);
        wallet_h
    }

    fn draw_runtime_block(
        &self,
        f: &mut Frame,
        left_col: Rect,
        y_offset: u16,
        v2c: usize,
        v3c: usize,
        fmc: usize,
    ) -> u16 {
        let rt_lines = vec![
            "WS: connected".to_string(),
            "Hermes: streaming".to_string(),
            "Pairs cached:".to_string(),
            format!("v2:{} v3:{} fm:{}", v2c, v3c, fmc),
        ];
        let rt_h = (rt_lines.len() as u16).saturating_add(2);
        let rt_props = BoxProps {
            offset: (0, 0),
            size: (left_col.width, 0),
            border_color: Color::DarkGray,
            title: "Runtime".into(),
        };
        let rt_area = Rect {
            x: left_col.x,
            y: left_col.y.saturating_add(y_offset),
            width: left_col.width,
            height: rt_h,
        };
        draw_box(f, rt_area, rt_lines, &rt_props);
        rt_h
    }

    async fn run_tui(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::event::EnableMouseCapture
        )?;
        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let mut terminal = ratatui::Terminal::new(backend)?;

        use futures_util::StreamExt;
        use std::collections::HashSet;
        use std::collections::{HashMap, VecDeque};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use tokio::sync::{Mutex, RwLock};
        let pair_keys: Arc<RwLock<VecDeque<String>>> = Arc::new(RwLock::new(VecDeque::new()));
        let pairs_map: Arc<RwLock<HashMap<String, crate::app::pair_state::PairState>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let sold_pairs: Arc<RwLock<HashSet<String>>> = Arc::new(RwLock::new(HashSet::new()));
        const MAX_PAIRS: usize = 120;

        let mut events = EventStream::new();
        let mut ticker = tokio::time::interval(Duration::from_millis(100));
        let mut fee_ticker = tokio::time::interval(Duration::from_secs(15));
        let mut balance_ticker = tokio::time::interval(Duration::from_secs(5));
        let mut dex_watch = tokio::time::interval(Duration::from_secs(1));

        // Hermes scroll
        let mut pairs_scroll: usize = 0;
        let mut pairs_scroll_state = ratatui::widgets::ScrollbarState::default();
        let mut last_viewport_len: usize = 0;

        // Results scroll
        let mut results_scroll: usize = 0;
        let mut results_scroll_state = ratatui::widgets::ScrollbarState::default();
        let mut results_areas: ResultsAreas = ResultsAreas::default();

        // Logs state
        let logs_dir = PathBuf::from("logs");
        let mut logs_lines: Vec<String> = load_logs_from_dir(&logs_dir);
        let mut logs_scroll: usize = 0;
        let mut logs_scroll_state = ratatui::widgets::ScrollbarState::default();
        let mut logs_refresh_ticker = tokio::time::interval(Duration::from_secs(1));

        // Auto Trade scroll
        let mut config_scroll: usize = 0;
        let mut config_scroll_state = ratatui::widgets::ScrollbarState::default();

        // Average fee in USD (refreshed periodically)
        let mut avg_fee_usd: Option<String> = None;
        let mut config_areas: ConfigAreas = ConfigAreas::default();

        let tab_labels: [&str; 5] = ["Home", "Auto Trade", "Results", "Logs", "Settings"];
        let mut active_tab: usize = 0;
        let mut hovered_tab: Option<usize> = None;
        let mut tab_areas: Vec<Rect> = Vec::with_capacity(tab_labels.len());

        // Hermes stream handles (per dex)
        #[derive(Default)]
        struct StreamHandles {
            v2: Option<tokio::task::JoinHandle<()>>,
            v3: Option<tokio::task::JoinHandle<()>>,
            fm: Option<tokio::task::JoinHandle<()>>,
            hb: Option<tokio::task::JoinHandle<()>>,
        }
        let mut stream_handles: StreamHandles = StreamHandles::default();
        let mut last_dexes_csv = self
            .config_store
            .get("dexes")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "v2,v3,fm".to_string());

        // --- SETTINGS STATE ---
        let settings = load_settings_cache().unwrap_or_default();
        let mut hide_wallet: bool = settings.hide_wallet;
        let mut hide_runtime: bool = settings.hide_runtime;
        // Always start with Simulation Mode ON to prevent accidental real trading
        let mut sim_mode: bool = true;
        // Persist this startup default so the UI reflects it immediately
        let _ = save_settings_cache(&SettingsCache {
            hide_wallet,
            hide_runtime,
            sim_mode,
        });
        let mut settings_wallet_toggle_area: Option<Rect> = None;
        let mut settings_runtime_toggle_area: Option<Rect> = None;
        let mut settings_sim_mode_toggle_area: Option<Rect> = None;

        // --- INPUT STATE ---
        let mut focused_field: Option<String> = None;
        let mut input_buffer: String = String::new();

        // --- SIMULATION ENGINE ---
        let max_pos = self
            .config_store
            .get("max_positions")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(3);
        let sim_engine = Arc::new(Mutex::new(SimEngine::new(max_pos)));
        // Initialize max hold secs from config (0 = disabled)
        if let Some(mh) = self
            .config_store
            .get("max_hold_secs")
            .and_then(|v| v.parse::<u64>().ok())
        {
            let mut se = sim_engine.lock().await;
            se.set_max_hold_secs(mh);
        }
        // Initialize max hold pnl toggle
        let mh_pnl = self
            .config_store
            .get("max_hold_pnl")
            .map(|v| v.as_str() == "true")
            .unwrap_or(true);
        {
            let mut se = sim_engine.lock().await;
            se.set_max_hold_pnl_enabled(mh_pnl);
        }
        // Shared toggle for background sim usage
        let sim_mode_flag = Arc::new(AtomicBool::new(sim_mode));

        let pairs_rx_local = self.pairs_rx.take().unwrap_or_else(|| {
            let (_tx, rx) = tokio::sync::mpsc::channel::<(String, String, PairInfo)>(1);
            rx
        });

        let dexes_enabled = |csv: &str| -> (bool, bool, bool) {
            let lower = csv.to_ascii_lowercase();
            (
                lower.contains("v2"),
                lower.contains("v3"),
                lower.contains("fm"),
            )
        };

        let sync_streams = |want_v2: bool,
                            want_v3: bool,
                            want_fm: bool,
                            stream_handles: &mut StreamHandles,
                            pair_tx: tokio::sync::mpsc::Sender<(String, String, PairInfo)>,
                            ws: crate::libs::bsc::client::BscWsClient,
                            provider: P| {
            if want_v2 && stream_handles.v2.is_none() {
                let tx = pair_tx.clone();
                let ws_c = ws.clone();
                let prov = provider.clone();
                stream_handles.v2 = Some(tokio::spawn(async move {
                    pancakev2_stream(tx, ws_c, prov).await;
                }));
            } else if !want_v2 {
                if let Some(h) = stream_handles.v2.take() {
                    h.abort();
                }
            }

            if want_v3 && stream_handles.v3.is_none() {
                let tx = pair_tx.clone();
                let ws_c = ws.clone();
                let prov = provider.clone();
                stream_handles.v3 = Some(tokio::spawn(async move {
                    pancakev3_stream(tx, ws_c, prov).await;
                }));
            } else if !want_v3 {
                if let Some(h) = stream_handles.v3.take() {
                    h.abort();
                }
            }

            if want_fm && stream_handles.fm.is_none() {
                let tx = pair_tx.clone();
                let ws_c = ws.clone();
                let prov = provider.clone();
                stream_handles.fm = Some(tokio::spawn(async move {
                    fourmeme_stream(tx, ws_c, prov).await;
                }));
            } else if !want_fm {
                if let Some(h) = stream_handles.fm.take() {
                    h.abort();
                }
            }
        };

        let initial_dexes = last_dexes_csv.clone();
        let (want_v2, want_v3, want_fm) = dexes_enabled(&initial_dexes);
        sync_streams(
            want_v2,
            want_v3,
            want_fm,
            &mut stream_handles,
            self.pair_tx.clone(),
            self.ws.clone(),
            self.provider.clone(),
        );
        if stream_handles.hb.is_none() {
            let tx_hb = self.pair_tx.clone();
            stream_handles.hb = Some(tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
                loop {
                    ticker.tick().await;
                    let line1 = "hb | Hermes heartbeat | Price: ?".to_string();
                    let link = String::new();
                    let pair_info = PairInfo {
                        addr1: Address::ZERO,
                        addr2: Address::ZERO,
                        pair: Address::ZERO,
                        fee: None,
                        tick_spacing: None,
                        symbol_base: "HB".to_string(),
                        symbol_quote: "HB".to_string(),
                        liquidity_usd: None,
                        buy_count: 0,
                        sell_count: 0,
                        unique_buyers: 0,
                    };
                    let _ = tx_hb.try_send((line1, link, pair_info));
                }
            }));
        }

        // Spawn background ingestion loop to drain pairs_rx continuously
        {
            let mut rx_bg = pairs_rx_local;
            let pairs_map_c = pairs_map.clone();
            let pair_keys_c = pair_keys.clone();
            let sold_pairs_c = sold_pairs.clone();
            let sim_engine_c = sim_engine.clone();
            let sim_mode_flag_c = sim_mode_flag.clone();
            let config_store_c = self.config_store.clone();
            let provider_c = self.provider.clone();
            tokio::spawn(async move {
                while let Some((l1, l2, pair_info)) = rx_bg.recv().await {
                    let pk = pair_key_addr(pair_info.pair);
                    let dexes_csv = config_store_c
                        .get("dexes")
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "v2,v3,fm".to_string());
                    let (en_v2, en_v3, en_fm) = {
                        let lower = dexes_csv.to_ascii_lowercase();
                        (lower.contains("v2"), lower.contains("v3"), lower.contains("fm"))
                    };
                    let src = crate::app::pair_state::detect_source(&l1);
                    let allowed = match src {
                        crate::app::pair_state::PairSource::V2 => en_v2,
                        crate::app::pair_state::PairSource::V3 => en_v3,
                        crate::app::pair_state::PairSource::FourMeme => en_fm,
                        crate::app::pair_state::PairSource::Unknown => false,
                    };
                    if !allowed {
                        continue;
                    }
                    // 1) update Hermes state
                    {
                        let mut map = pairs_map_c.write().await;
                        let mut keys = pair_keys_c.write().await;
                        let mut sold = sold_pairs_c.write().await;
                        crate::app::pair_streams::update_pairs_state(
                            l1.clone(),
                            l2,
                            pair_info.clone(),
                            &mut *map,
                            &mut *keys,
                            &mut *sold,
                            MAX_PAIRS,
                        );
                    }
                    // 2) Keep PnL state updated for both sim and real paths
                    if let Some(current_price) = crate::app::pair_state::extract_price_f64(&l1) {
                        let sim_on = sim_mode_flag_c.load(std::sync::atomic::Ordering::Relaxed);
                        let mut se = sim_engine_c.lock().await;
                        let maybe_msg = se.update_or_execute(
                            &pk,
                            current_price,
                            pair_info.liquidity_usd,
                            sim_on,
                        );
                        drop(se);
                        if sim_on {
                            if let Some(msg) = maybe_msg {
                                save_log_to_file(&format!("[sim] {}", msg));
                            }
                        }
                    }

                    if sim_mode_flag_c.load(std::sync::atomic::Ordering::Relaxed) {
                        // 3) Simulation: consider new buy submission based on config and counters
                        let buy_count = {
                            let map = pairs_map_c.read().await;
                            map.get(&pk)
                                .map(|e| e.buy_count)
                                .unwrap_or(pair_info.buy_count)
                        };
                        let mut se = sim_engine_c.lock().await;
                        let _ = crate::app::auto_trade::auto_trade(
                            l1.clone(),
                            pair_info.clone(),
                            buy_count,
                            true,
                            &mut *se,
                            &config_store_c,
                        )
                        .await;
                    } else {
                        // 3) Real trading path when simulation is OFF
                        let buy_count = {
                            let map = pairs_map_c.read().await;
                            map.get(&pk)
                                .map(|e| e.buy_count)
                                .unwrap_or(pair_info.buy_count)
                        };
                        let _ = crate::app::auto_trade::auto_trade_real(
                            l1.clone(),
                            pair_info.clone(),
                            buy_count,
                            provider_c.clone(),
                            &config_store_c,
                            Some(&sim_engine_c),
                        )
                        .await;
                    }
                }
            });
        }

        let mut should_quit = false;
        while !should_quit {
            tokio::select! {
            // prioritize key/mouse handling responsiveness
            maybe_ev = events.next() => {
                if let Some(Ok(ev)) = maybe_ev {
                    match ev {
                        Event::Key(key) => {
                    // Handle input field editing
                    if let Some(field) = focused_field.as_ref() {
                        match key.code {
                            KeyCode::Char(c) if c.is_ascii_digit() || c == '.' => {
                                input_buffer.push(c);
                            }
                            KeyCode::Backspace => {
                                input_buffer.pop();
                            }
                            KeyCode::Enter => {
                                // Save the value
                                if !input_buffer.is_empty() {
                                    self.config_store.insert(field.clone(), input_buffer.clone());
                                    let _ = save_autotrade_cache(&self.config_store);
                                    // Update sim engine if max_positions changed
                                    if field == "max_positions" {
                                        if let Ok(new_max) = input_buffer.parse::<usize>() {
                                            let mut se = sim_engine.lock().await;
                                            se.update_max_positions(new_max);
                                        }
                                    }
                                    // Update sim engine if max_hold_secs changed
                                    if field == "max_hold_secs" {
                                        if let Ok(new_hold) = input_buffer.parse::<u64>() {
                                            let mut se = sim_engine.lock().await;
                                            se.set_max_hold_secs(new_hold);
                                        }
                                    }
                                }
                                focused_field = None;
                                input_buffer.clear();
                            }
                            KeyCode::Esc => {
                                focused_field = None;
                                input_buffer.clear();
                            }
                            _ => {}
                        }
                        continue; // Don't process other keys when editing
                    }

                    if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                        should_quit = true;
                    }
                    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        should_quit = true;
                    }
                    // Tab navigation and scrolling (context-aware)
                    match key.code {
                        KeyCode::Left => { active_tab = active_tab.saturating_sub(1); }
                        KeyCode::Right => { active_tab = (active_tab + 1) % tab_labels.len(); }
                        KeyCode::Down | KeyCode::Char('j') => {
                            match active_tab {
                                0 => { pairs_scroll = pairs_scroll.saturating_add(1); }
                                1 => { config_scroll = config_scroll.saturating_add(1); }
                                2 => { results_scroll = results_scroll.saturating_add(1); }
                                3 => { logs_scroll = logs_scroll.saturating_add(1); }
                                _ => {}
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            match active_tab {
                                0 => { pairs_scroll = pairs_scroll.saturating_sub(1); }
                                1 => { config_scroll = config_scroll.saturating_sub(1); }
                                2 => { results_scroll = results_scroll.saturating_sub(1); }
                                3 => { logs_scroll = logs_scroll.saturating_sub(1); }
                                _ => {}
                            }
                        }
                        KeyCode::PageDown => {
                            match active_tab {
                                0 => { pairs_scroll = pairs_scroll.saturating_add(last_viewport_len.saturating_sub(1)); }
                                1 => { config_scroll = config_scroll.saturating_add(10); }
                                2 => { results_scroll = results_scroll.saturating_add(10); }
                                3 => { logs_scroll = logs_scroll.saturating_add(100); }
                                _ => {}
                            }
                        }
                        KeyCode::PageUp => {
                            match active_tab {
                                0 => { pairs_scroll = pairs_scroll.saturating_sub(last_viewport_len.saturating_sub(1)); }
                                1 => { config_scroll = config_scroll.saturating_sub(10); }
                                2 => { results_scroll = results_scroll.saturating_sub(10); }
                                3 => { logs_scroll = logs_scroll.saturating_sub(100); }
                                _ => {}
                            }
                        }
                        KeyCode::Home => {
                            match active_tab {
                                0 => { pairs_scroll = 0; }
                                1 => { config_scroll = 0; }
                                2 => { results_scroll = 0; }
                                3 => { logs_scroll = 0; }
                                _ => {}
                            }
                        }
                        KeyCode::End => {
                            match active_tab {
                                0 => { pairs_scroll = usize::MAX; }
                                1 => { config_scroll = usize::MAX; }
                                2 => { results_scroll = usize::MAX; }
                                3 => { logs_scroll = usize::MAX; }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                        }
                        Event::Mouse(me) => {
                            // Hover updates
                            if let MouseEventKind::Moved = me.kind {
                                let mx = me.column;
                                let my = me.row;
                                hovered_tab = None;
                                for (i, r) in tab_areas.iter().enumerate() {
                                    if mx >= r.x && mx < r.x + r.width && my >= r.y && my < r.y + r.height {
                                        hovered_tab = Some(i);
                                        break;
                                    }
                                }
                            }
                            // Click to select tab
                            if let MouseEventKind::Down(MouseButton::Left) = me.kind {
                                let mx = me.column;
                                let my = me.row;
                                for (i, r) in tab_areas.iter().enumerate() {
                                    if mx >= r.x && mx < r.x + r.width && my >= r.y && my < r.y + r.height {
                                        active_tab = i;
                                    }
                                }
                                // Interactions INSIDE Results view (tab index 2)
                                if active_tab == 2 {
                                    if sim_mode {
                                        let mut se = sim_engine.lock().await;
                                        let mut sold = sold_pairs.write().await;
                                        results_interactions(&results_areas, &mut *se, &mut *sold, mx, my);
                                    } else {
                                        // Real mode: spawn async sell handler to avoid blocking UI
                                        let areas = results_areas.clone();
                                        let se = sim_engine.clone();
                                        let sold = sold_pairs.clone();
                                        let provider = self.provider.clone();
                                        let config_store = self.config_store.clone();
                                        tokio::spawn(async move {
                                            crate::app::handler::results_interactions_real(
                                                areas, se, sold, provider, mx, my, &config_store
                                            ).await;
                                        });
                                    }
                                }
                                // Interactions INSIDE Auto Trade view (tab index 1)
                                if active_tab == 1 {
                                    let mut se = sim_engine.lock().await;
                                    cfg_bindings(&config_areas, &self.config_store, &mut *se, mx, my, &mut focused_field, &mut input_buffer);

                                }
                                // Interactions INSIDE Settings view (tab index 4)
                                if active_tab == 4 {
                                    let contains = |r: Option<Rect>| -> bool {
                                        if let Some(rr) = r {
                                            mx >= rr.x && mx < rr.x + rr.width && my >= rr.y && my < rr.y + rr.height
                                        } else { false }
                                    };
                                    if contains(settings_wallet_toggle_area) {
                                        hide_wallet = !hide_wallet;
                                        let _ = save_settings_cache(&SettingsCache { hide_wallet, hide_runtime, sim_mode });
                                    }
                                    if contains(settings_runtime_toggle_area) {
                                        hide_runtime = !hide_runtime;
                                        let _ = save_settings_cache(&SettingsCache { hide_wallet, hide_runtime, sim_mode });
                                    }
                                    if contains(settings_sim_mode_toggle_area) {
                                        sim_mode = !sim_mode;
                                        // Update background view on toggle
                                        sim_mode_flag.store(sim_mode, Ordering::Relaxed);
                                        if !sim_mode {
                                            // Reset simulation when turning off
                                            let mut se = sim_engine.lock().await;
                                            se.reset();
                                        }
                                        let _ = save_settings_cache(&SettingsCache { hide_wallet, hide_runtime, sim_mode });
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            _ = fee_ticker.tick() => {
                // Refresh average BSC fee ($) using eth_gasPrice and WBNB->USDT spot via v2
                let fee_str = self.cli.calculate_fee_str(&self.provider).await?;
                avg_fee_usd = Some(fee_str);
            }
            _ = dex_watch.tick() => {
                let now_csv = self
                    .config_store
                    .get("dexes")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "v2,v3,fm".to_string());
                if now_csv != last_dexes_csv {
                    let (want_v2, want_v3, want_fm) = dexes_enabled(&now_csv);
                    sync_streams(
                        want_v2,
                        want_v3,
                        want_fm,
                        &mut stream_handles,
                        self.pair_tx.clone(),
                        self.ws.clone(),
                        self.provider.clone(),
                    );
                    if !want_v2 || !want_v3 || !want_fm {
                        let mut map = pairs_map.write().await;
                        let mut keys = pair_keys.write().await;
                        keys.retain(|k| {
                            map.get(k).map(|v| {
                                (want_v2 || v.source != crate::app::pair_state::PairSource::V2) &&
                                (want_v3 || v.source != crate::app::pair_state::PairSource::V3) &&
                                (want_fm || v.source != crate::app::pair_state::PairSource::FourMeme)
                            }).unwrap_or(true)
                        });
                        map.retain(|_, v| {
                            (want_v2 || v.source != crate::app::pair_state::PairSource::V2) &&
                            (want_v3 || v.source != crate::app::pair_state::PairSource::V3) &&
                            (want_fm || v.source != crate::app::pair_state::PairSource::FourMeme)
                        });
                    }
                    last_dexes_csv = now_csv;
                }
            }
            _ = logs_refresh_ticker.tick() => {
                let dir = logs_dir.clone();
                match tokio::task::spawn_blocking(move || load_logs_from_dir(&dir)).await {
                    Ok(lines) => {
                        logs_lines = lines;
                        if logs_lines.is_empty() {
                            logs_scroll = 0;
                        } else if logs_scroll >= logs_lines.len() {
                            logs_scroll = logs_lines.len().saturating_sub(1);
                        }
                    }
                    Err(_) => {
                        // keep previous lines on failure
                    }
                }
            }
            _ = balance_ticker.tick() => {
                // Refresh on-chain balance for wallet panel + title
                if let Ok(bal) = get_balance(&self.cli, self.cli.address).await {
                    if let Ok(b) = format_bnb(format!("0x{:x}", bal)) {
                        self.balance_bnb = b;
                    }
                }
            }
            _ = ticker.tick() => {
                // 1) Prune stale items on UI tick
                let now = Instant::now();
                let to_remove: Vec<String> = {
                    let map = pairs_map.read().await;
                    let keys = pair_keys.read().await;
                    let mut rm = Vec::new();
                    for k in keys.iter() {
                        if let Some(v) = map.get(k) {
                            let low_for_30s = v.below_thresh_since.map(|s| now.duration_since(s) >= Duration::from_secs(30)).unwrap_or(false);
                            let no_change_5m = now.duration_since(v.last_pnl_change_at) >= Duration::from_secs(5 * 60);
                            if low_for_30s || no_change_5m {
                                rm.push(k.clone());
                            }
                        }
                    }
                    rm
                };
                if !to_remove.is_empty() {
                    let rm: std::collections::HashSet<String> = to_remove.into_iter().collect();
                    let mut map = pairs_map.write().await;
                    let mut keys = pair_keys.write().await;
                    let mut sold = sold_pairs.write().await;
                    for k in rm.iter() { let _ = map.remove(k); sold.insert(k.clone()); }
                    keys.retain(|k| !rm.contains(k));
                }

                // lock sim_engine for results view
                let se_guard = sim_engine.lock().await;
                // 2) Snapshot state for drawing
                let (v2c, v3c, fmc, all_pairs): (usize, usize, usize, Vec<(String, String, String)>) = {
                    let map = pairs_map.read().await;
                    let keys = pair_keys.read().await;
                    let avoid_cn = self
                        .config_store
                        .get("avoid_chinese")
                        .map(|v| v.as_str() == "true")
                        .unwrap_or(false);
                    let open_set: std::collections::HashSet<String> = {
                        let mut set = std::collections::HashSet::new();
                        for p in se_guard.open_positions() {
                            set.insert(p.pair_address.clone());
                        }
                        set
                    };
                    let mut v2c = 0usize;
                    let mut v3c = 0usize;
                    let mut fmc = 0usize;
                    let mut pairs_list: Vec<(String, String, String)> = Vec::with_capacity(keys.len());
                    for k in keys.iter() {
                        if let Some(v) = map.get(k) {
                            if avoid_cn && !open_set.contains(k) && contains_cjk(&v.upair_address) {
                                continue;
                            }
                            match v.source {
                                crate::app::pair_state::PairSource::V2 => v2c += 1,
                                crate::app::pair_state::PairSource::V3 => v3c += 1,
                                crate::app::pair_state::PairSource::FourMeme => fmc += 1,
                                _ => {}
                            }
                            let (l1, l2, l3) = v.to_three_lines();
                            pairs_list.push((l1, l2, l3));
                        }
                    }
                    (v2c, v3c, fmc, pairs_list)
                };

                // draw
                terminal.draw(|f| {
                    let size = f.area();
                    draw_main_window(f, size);
                    if size.height < *crate::shared::MIN_TERMINAL_HEIGHT {
                        let area = centered_rect(70, 30, size);
                        let lines = [
                            "Terminal too small to render UI.",
                            "Minimum height required: 70 rows.",
                            "Please resize your terminal window.",
                        ];
                        draw_modal(f, area, "Resize Needed", &lines);
                    } else {
                            // live counters for mid-title
                            let short_addr = short_addr(&self.address);
                            let mid = format!(
                                "BSC-{}  •  {}  •  {}  •  v2:{} v3:{} fm:{}",
                                self.chain_id,
                                short_addr,
                                balance_short(&self.balance_bnb),
                                v2c,
                                v3c,
                                fmc,
                            );
                            let right = "q/esc: quit  ↑/↓: scroll  PgUp/PgDn: fast";
                            draw_title_bar(f, size, "JimmyBSC", &mid, right);

                            // body below title
                            let body = Rect { x: size.x + 1, y: size.y + 3, width: size.width.saturating_sub(2), height: size.height.saturating_sub(4) };

                            // Dynamic layout based on settings
                            let show_left_column = !hide_wallet || !hide_runtime;
                            let (left_col_opt, right_area) = if show_left_column {
                                let cols = Layout::default()
                                    .direction(Direction::Horizontal)
                                    .constraints([
                                        Constraint::Length(36),
                                        Constraint::Min(10),
                                    ])
                                    .split(body);
                                (Some(cols[0]), cols[1])
                            } else {
                                // Full width for right side when left is hidden
                                (None, body)
                            };

                            {
                                // Left column blocks (conditionally rendered)
                                if let Some(left_col) = left_col_opt {
                                    let mut y_offset = 0u16;
                                    if !hide_wallet {
                                        let wallet_h = self.draw_wallet_block(f, left_col, avg_fee_usd.as_deref());
                                        y_offset = wallet_h;
                                    }
                                    if !hide_runtime {
                                        let _rt_h = self.draw_runtime_block(f, left_col, y_offset, v2c, v3c, fmc);
                                    }
                                }

                                // RIGHT COLUMN: tab strip + stacked content (always shown)
                                // top 1 row → tabs, rest → content (browser-like)
                                let tab_strip_h: u16 = 1;
                                let right_rows = Layout::default()
                                    .direction(Direction::Vertical)
                                    .constraints([Constraint::Length(tab_strip_h), Constraint::Min(10)])
                                    .split(right_area);

                                // draw tab strip (fills tab_areas for hit testing)
                                draw_tab_strip(f, right_rows[0], &tab_labels, hovered_tab, active_tab, &mut tab_areas);

                                // stacked content under the strip
                                let content_area = right_rows[1];
                                match active_tab {
                                    0 => {
                                        // Home → Hermes list
                                        let content_len = if all_pairs.is_empty() { 1 } else { all_pairs.len() * 3 };
                                        // match modal.rs: visible text rows = height - 2 (borders)
                                        let viewport_len = content_area.height.saturating_sub(2) as usize;
                                        let max_pos = content_len.saturating_sub(viewport_len);
                                        if pairs_scroll > max_pos { pairs_scroll = max_pos; }
                                        last_viewport_len = viewport_len;
                                        draw_modal_pairs(f, content_area, "Hermes", &all_pairs, pairs_scroll, &mut pairs_scroll_state);
                                    }
                                    1 => {
                                        // Auto Trade → no modal; bordered panel for the config form
                                        let block = ratatui::widgets::Block::default()
                                            .borders(ratatui::widgets::Borders::ALL)
                                            .border_type(ratatui::widgets::BorderType::Rounded)
                                            .title(Span::styled("Auto Trade", Style::default().fg(Color::White)));
                                        f.render_widget(block, content_area);
                                        let inner = content_area.inner(ratatui::layout::Margin::new(1, 1));

                                        // If editing a field, show input_buffer instead of stored value
                                        let focused_ref = focused_field.as_deref();
                                        let rows_used = if let Some(field) = focused_ref {
                                            // Temporarily update store with buffer for display
                                            let original = self.config_store.get(field).map(|v| v.to_string());
                                            self.config_store.insert(field.to_string(), input_buffer.clone());
                                            let r = draw_config_main(f, inner, &self.config_store, &mut config_areas, Some(field), config_scroll);
                                            // Restore original if we're still editing
                                            if let Some(orig) = original {
                                                self.config_store.insert(field.to_string(), orig);
                                            }
                                            r
                                        } else {
                                            draw_config_main(f, inner, &self.config_store, &mut config_areas, None, config_scroll)
                                        };
                                        let rows_total = rows_used as usize;
                                        let viewport = inner.height as usize;
                                        let max_scroll = rows_total.saturating_sub(viewport);
                                        if config_scroll > max_scroll {
                                            config_scroll = max_scroll;
                                        }
                                        config_scroll_state = config_scroll_state
                                            .content_length(rows_total)
                                            .viewport_content_length(viewport)
                                            .position(config_scroll);
                                        f.render_stateful_widget(
                                            ratatui::widgets::Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)
                                                .begin_symbol(Some("↑"))
                                                .end_symbol(Some("↓")),
                                            inner,
                                            &mut config_scroll_state,
                                        );
                                    }
                                    2 => {
                                        // Results → simulation results
                                        let title_text = if sim_mode { "Simulation Results" } else { "Trading Results" };
                                        let block = ratatui::widgets::Block::default()
                                            .borders(ratatui::widgets::Borders::ALL)
                                            .border_type(ratatui::widgets::BorderType::Rounded)
                                            .title(Span::styled(title_text, Style::default().fg(Color::White)));
                                        f.render_widget(block, content_area);
                                        let inner = content_area.inner(ratatui::layout::Margin::new(2, 1));
                                    results(
                                            f,
                                            inner,
                                            &mut results_areas,
                                            sim_mode,
                                            &*se_guard,
                                            results_scroll,
                                            &mut results_scroll_state,
                                            &self.config_store,
                                            self.session_started_at.elapsed().as_secs(),
                                        );

                                    }
                                    3 => {
                                        let total = logs_lines.len();
                                        if total == 0 {
                                            logs_scroll = 0;
                                        } else {
                                            logs_scroll = logs_scroll.min(total.saturating_sub(1));
                                        }
                                        let page = if total == 0 { 1 } else { (logs_scroll / 100) + 1 };
                                        let pages = std::cmp::max(1, (total + 99) / 100);
                                        let title = format!("Logs (newest first) {}/{} ({} lines)", page, pages, total);
                                        draw_modal_lines(
                                            f,
                                            content_area,
                                            title.as_str(),
                                            &logs_lines,
                                            logs_scroll,
                                            &mut logs_scroll_state,
                                        );
                                    }
                                    _ => {
                                        // Settings → toggles
                                        let block = ratatui::widgets::Block::default()
                                            .borders(ratatui::widgets::Borders::ALL)
                                            .border_type(ratatui::widgets::BorderType::Rounded)
                                            .title(Span::styled("Settings", Style::default().fg(Color::White)));
                                        f.render_widget(block, content_area);
                                        let inner = content_area.inner(ratatui::layout::Margin::new(2, 2));

                                        // Three toggle rows
                                        let rows = Layout::default()
                                            .direction(Direction::Vertical)
                                            .constraints([
                                                Constraint::Length(3),
                                                Constraint::Length(3),
                                                Constraint::Length(3),
                                                Constraint::Min(0),
                                            ])
                                            .split(inner);

                                        use crate::libs::tui::toggle::draw_toggle;
                                        draw_toggle(f, rows[0], "Hide Wallet Panel", hide_wallet, false);
                                        settings_wallet_toggle_area = Some(rows[0]);

                                        draw_toggle(f, rows[1], "Hide Runtime Panel", hide_runtime, false);
                                        settings_runtime_toggle_area = Some(rows[1]);

                                        draw_toggle(f, rows[2], "Simulation Mode", sim_mode, false);
                                        settings_sim_mode_toggle_area = Some(rows[2]);
                                    }
                                }
                            }
                        }
                    })?;
                }
            }
        }

        // Save Auto Trade config before exit
        let _ = save_autotrade_cache(&self.config_store);

        let mut stdout = std::io::stdout();
        execute!(
            stdout,
            crossterm::event::DisableMouseCapture,
            LeaveAlternateScreen
        )?;
        disable_raw_mode()?;
        Ok(())
    }
}

fn short_addr(addr: &str) -> String {
    if addr.len() > 12 {
        let (a, b) = addr.split_at(6);
        let tail = &b[b.len().saturating_sub(4)..];
        format!("{}…{}", a, tail)
    } else {
        addr.to_string()
    }
}

fn balance_short(bal: &str) -> String {
    if let Some(space) = bal.find(' ') {
        let (num, cur) = bal.split_at(space);
        let trimmed = if num.len() > 6 { &num[..6] } else { num };
        format!("{}{}", trimmed, cur)
    } else {
        bal.to_string()
    }
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x4E00..=0x9FFF).contains(&u)
            || (0x3400..=0x4DBF).contains(&u)
            || (0x20000..=0x2A6DF).contains(&u)
            || (0x2A700..=0x2B73F).contains(&u)
            || (0x2B740..=0x2B81F).contains(&u)
            || (0x2B820..=0x2CEAF).contains(&u)
    })
}

fn load_logs_from_dir(dir: &PathBuf) -> Vec<String> {
    let mut merged: Vec<(i64, i64, String)> = Vec::new();
    if !dir.exists() {
        return Vec::new();
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("log");
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);
        for (idx, line) in reader.lines().enumerate() {
            if let Ok(l) = line {
                let trimmed = l.trim_end();
                let display = format!("{} | {}", file_name, trimmed);
                merged.push((modified, idx as i64, display));
            }
        }
    }

    merged.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    merged.into_iter().map(|(_, _, s)| s).collect()
}

fn gas_price_wei_from_cfg(config_store: &ConfigStore) -> u128 {
    config_store
        .get("max_gwei")
        .and_then(|v| parse_units(v.as_str(), 9).ok())
        .and_then(|wei| wei.try_into().ok())
        .filter(|wei| *wei > 0)
        .unwrap_or(1_000_000_000)
}

async fn startup_liquidate_fm_tokens<P>(provider: P, config_store: &ConfigStore)
where
    P: alloy::providers::Provider + WalletProvider + Clone + Send + Sync + 'static,
{
    // Collect candidate token addresses from log files (addresses that end with 4444)
    let mut addrs: std::collections::HashSet<Address> = std::collections::HashSet::new();
    let logs_dir = PathBuf::from("logs");
    if logs_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&logs_dir) {
            for entry in entries.flatten() {
                if let Ok(data) = std::fs::read_to_string(entry.path()) {
                    for word in data.split_whitespace() {
                        if word.len() == 42 && word.starts_with("0x") {
                            if let Ok(addr) = word.parse::<Address>() {
                                let s = format!("{addr:?}").to_ascii_lowercase();
                                if s.ends_with("4444") {
                                    addrs.insert(addr);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if addrs.is_empty() {
        return;
    }

    let from = provider.default_signer_address();
    let erc20 = |token: Address| IERC20Lite::new(token, provider.clone());
    let router = crate::router::FmRouter::new(provider.clone());
    let gas_price = U256::from(gas_price_wei_from_cfg(config_store));

    for token in addrs {
        let bal = erc20(token).balanceOf(from).call().await.unwrap_or(U256::ZERO);
        if bal.is_zero() {
            continue;
        }
        let pct = 100u32;
        let gas_price_wei = gas_price.as_limbs()[0] as u128;
        if let Err(e) = ensure_sell_allowance(
            provider.clone(),
            DexType::FourMeme,
            token,
            bal,
            gas_price_wei,
        )
        .await
        {
            save_log_to_file(&format!(
                "[startup] allowance failed {} err={}",
                format!("{token:?}"),
                e
            ));
            continue;
        }
        match router.sell_percent_pct(from, token, pct, Some(gas_price)).await {
            Ok((_est, tx)) => save_log_to_file(&format!(
                "[startup] sell {} pct={} tx={:#x}",
                format!("{token:?}"),
                pct,
                tx
            )),
            Err(e) => save_log_to_file(&format!(
                "[startup] sell failed {} err={}",
                format!("{token:?}"),
                e
            )),
        }
    }
}

// triggers actual sells instead of simulation-only actions.
pub async fn results_interactions_real<P>(
    results_areas: crate::app::results::ResultsAreas,
    sim_engine: std::sync::Arc<tokio::sync::Mutex<crate::libs::sim::SimEngine>>,
    sold_pairs: std::sync::Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>,
    provider: P,
    mx: u16,
    my: u16,
    config_store: &ConfigStore,
) where
    P: alloy::providers::Provider
        + alloy::providers::WalletProvider
        + Clone
        + Send
        + Sync
        + 'static,
{
    let contains_rect = |r: ratatui::layout::Rect| -> bool {
        mx >= r.x && mx < r.x + r.width && my >= r.y && my < r.y + r.height
    };

    // If UI shows "Ok" overlays, clicks should ACK (same as sim path) instead of attempting on-chain actions.
    let pending_liq_ack = {
        let se = sim_engine.lock().await;
        se.has_pending_liq_alert()
    };

    // Take All / Ok
    if let Some(r) = results_areas.take_all_btn {
        if contains_rect(r) {
            if pending_liq_ack {
                return;
            }

            save_log_to_file("[trade] manual TAKE ALL requested");
            match crate::app::auto_trade::manual_sell_all(
                provider.clone(),
                &sim_engine,
                Some(&sold_pairs),
                &config_store,
            )
            .await
            {
                Ok(_) => save_log_to_file("[trade] manual TAKE ALL finished"),
                Err(e) => save_log_to_file(&format!("[trade] manual TAKE ALL failed: {}", e)),
            }
            return;
        }
    }

    // Partial sell buttons (10/25/50 or Ok)
    for (r, pair_addr, fraction) in results_areas.partial_btns.iter() {
        if contains_rect(*r) {
            let needs_ack = {
                let se = sim_engine.lock().await;
                se.position_needs_liq_ack(pair_addr)
            };
            if needs_ack {
                return;
            }

            let pct_points = ((*fraction * 100.0).round() as u32).clamp(1, 100);
            save_log_to_file(&format!(
                "[trade] manual PARTIAL TAKE requested: {} {}%",
                pair_addr, pct_points
            ));
            match crate::app::auto_trade::manual_sell(
                pair_addr,
                pct_points,
                provider.clone(),
                &sim_engine,
                Some(&sold_pairs),
                &config_store,
            )
            .await
            {
                Ok(true) => save_log_to_file(&format!(
                    "[trade] manual PARTIAL TAKE sent: {} {}%",
                    pair_addr, pct_points
                )),
                Ok(false) => save_log_to_file(&format!(
                    "[trade] manual PARTIAL TAKE ignored: {} (no real position?)",
                    pair_addr
                )),
                Err(e) => save_log_to_file(&format!(
                    "[trade] manual PARTIAL TAKE failed: {} {}% err={}",
                    pair_addr, pct_points, e
                )),
            }
            return;
        }
    }

    // Remove buttons (stop managing a position)
    for (r, pair_addr) in results_areas.remove_btns.iter() {
        if contains_rect(*r) {
            save_log_to_file(&format!("[trade] manual REMOVE requested: {}", pair_addr));
            match crate::app::auto_trade::manual_remove_position(&pair_addr, &sim_engine).await {
                Ok(true) => save_log_to_file(&format!(
                    "[trade] manual REMOVE applied: {}",
                    pair_addr
                )),
                Ok(false) => save_log_to_file(&format!(
                    "[trade] manual REMOVE ignored: {} (no position)",
                    pair_addr
                )),
                Err(e) => save_log_to_file(&format!(
                    "[trade] manual REMOVE failed: {} err={}",
                    pair_addr, e
                )),
            }
            return;
        }
    }

    // Individual Take buttons (full close) / Ok
    for (r, pair_addr) in results_areas.take_btns.iter() {
        if contains_rect(*r) {
            let needs_ack = {
                let se = sim_engine.lock().await;
                se.position_needs_liq_ack(pair_addr)
            };
            if needs_ack {
                return;
            }

            save_log_to_file(&format!("[trade] manual TAKE requested: {}", pair_addr));
            match crate::app::auto_trade::manual_sell(
                pair_addr,
                100,
                provider.clone(),
                &sim_engine,
                Some(&sold_pairs),
                &config_store,
            )
            .await
            {
                Ok(true) => save_log_to_file(&format!("[trade] manual TAKE sent: {}", pair_addr)),
                Ok(false) => save_log_to_file(&format!(
                    "[trade] manual TAKE ignored: {} (no real position?)",
                    pair_addr
                )),
                Err(e) => save_log_to_file(&format!(
                    "[trade] manual TAKE failed: {} err={}",
                    pair_addr, e
                )),
            }
            return;
        }
    }

    // Freeze/unfreeze / Ok (mirrors sim behavior)
    for (r, pair_addr) in results_areas.freeze_btns.iter() {
        if contains_rect(*r) {
            let needs_ack = {
                let se = sim_engine.lock().await;
                se.position_needs_liq_ack(pair_addr)
            };
            if needs_ack {
                return;
            }

            let mut se = sim_engine.lock().await;
            let _ = se.toggle_freeze(pair_addr);
            return;
        }
    }
}
