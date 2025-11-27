<div align="center">

  <img alt="jimmy (1)" src="https://github.com/user-attachments/assets/fd9461f6-8333-4b90-9c7d-bcd8935b871b" />

</div>

# JimmyBSC

JimmyB is a TUI app for watching new BSC pairs and auto–trading them using simple rules.

> [!NOTE]
> Provided software is **free**. One can distribute further under standard MIT license.

**Tested for both Linux and Windows platforms.**

It connects to:
- PancakeSwap V2 & V3
- [FourMeme](https://four.meme)

to simulate or execute buys and sells based on streaming on–chain data. 

You get:

- A live “Hermes” feed of new pairs and prices
- A configurable auto–trade engine (per–pair buy amount, TP/SL, max positions, max hold, etc.)
- A results view with PnL and manual “Take / Partial / Remove” actions
- A small settings panel to hide UI blocks and flip between simulation and real mode

The whole thing runs in a single terminal window and logs to disk so you can replay sessions later.

---

## Screenshots

### Home (Hermes feed)

`Home` shows the live stream of pairs plus some high–level stats.

![Home](media/home.png)

### Auto Trade config

`Auto Trade` is where you tune the engine: which DEXes to use, how much BNB to spend per trade, TP/SL, limits, liquidity guards, etc.

![Auto Trade](media/auto_trade.png)

### Results (positions & PnL)

`Results` shows the simulated or real positions, including per–pair PnL and quick buttons for Take, Partial, Freeze and Remove.

![Results](media/results.png)

### Settings

`Settings` is a small panel for cosmetic and safety preferences (wallet block, runtime block, sim mode).

![Settings](media/settings.png)

---

## How it works (high level)

At a very high level:

- WebSocket clients subscribe to:
  - Pancake V2 `PairCreated`
  - Pancake V3 `PoolCreated`
  - FourMeme token creation events
- A `SimEngine` tracks positions in memory:
  - Entry price, current price, PnL, remaining size, duration, liquidity and “frozen / out–of–liq” flags.
- The Home tab (“Hermes”) continuously updates pairs with live prices and metadata.
- The Auto Trade logic decides when to open a new position based on:
  - Buy count / popularity
  - Minimum liquidity
  - Config thresholds (TP, SL, max positions, max hold, etc.)
- Depending on the mode:
  - **Simulation ON**: trades only touch the `SimEngine` and logs – nothing on–chain.
  - **Simulation OFF**: trades call Pancake / FourMeme routers using your configured wallet.
- The Results tab renders a summary of open / closed positions and gives you clickable actions:
  - Take (full close), 10/25/50% partials, Freeze / Unfreeze, Remove.

All of this is wired through the TUI in `src/app/handler.rs`, backed by various helpers in `src/libs`.

---

## Features

- Live BSC pair feed with aggregated stats
  - Pancake V2 / V3 and FourMeme token manager streams.
  - Basic runtime info (WS status, cached pairs, etc.).
- Auto–trading engine
  - Per–pair buy size (in BNB/WBNB).
  - Max simultaneous positions.
  - Take–profit / Stop–loss configuration.
  - Max hold duration with optional PnL gating.
  - Liquidity guard and “out of liq” alerts.
  - One–shot approvals for routers and FourMeme token managers.
- Simulation and real mode
  - Starts in **Simulation mode ON** to prevent accidents.
  - Real mode routes to:
    - Pancake V2 / V3 for swaps.
    - FourMeme Helper/TokenManager for buys/sells.
- Results / position management
  - PnL per position (open + realized for partials).
  - Take all, Take per position, partial sells (10/25/50%).
  - Freeze/unfreeze positions so TP/SL won’t auto–close them.
  - Remove positions from management without closing on–chain.
- Persistent config & settings
  - Auto trade config cached in `.cache/autotrade.json`.
  - UI preferences cached in `.cache/settings.json`.
  - Env–based connection config (`BSC_RPC`, `BSC_WSS`, `PRIVATE_KEY`).

---

## Requirements

- Rust
- A BSC RPC endpoint (HTTPS)
- A BSC WebSocket endpoint
- A funded BSC wallet private key (for **real trading**)

You can still run in simulation mode with a dummy key, but a real key is required if you flip sim mode off.

---

## Configuration

Configuration is split between:

- Environment variables (connection & global values)
- Auto trade config (stored via the TUI in `.cache/autotrade.json`)
- Settings cache (cosmetic and safety toggles in `.cache/settings.json`)

### Env vars

`src/libs/config.rs` expects:

```bash
BSC_WSS=wss://your-bsc-websocket
PRIVATE_KEY=0xYourPrivateKeyHere
BSC_RPC=https://bsc-rpc.example.com    # optional; defaults to public RPC
```

These can go in a `.env` file in the project root; they are loaded via `dotenv`.

Additional tests and helpers in `src/main.rs` also look for some optional values when you run them directly:

```bash
FOUR_TOKEN=0xYourFourMemeTokenAddress
FM_BNB_AMOUNT=0.0001          # test amount for FM buys
FM_SLIPPAGE_BPS=100           # FM slippage (basis points)
FM_SELL_PCT=100               # FM sell percent (0–100)
BNB_AMOUNT=0.0003             # generic BNB spend in examples
BTCB_AMOUNT=0.00001           # generic BTCB spend in examples
```

If you don’t intend to run the integration tests, you can ignore most of these.

### Auto trade config (TUI)

Auto trade configuration is edited directly inside the `Auto Trade` tab; it’s backed by the `ConfigStore` type and persisted via `src/libs/cache.rs` into `.cache/autotrade.json`.

A few key parameters you’ll see in the UI:

- `max_positions` – max number of concurrently managed positions.
- `tp_enabled` / `tp_pct` – enable and set take–profit percentage.
- `sl_enabled` / `sl_pct` – enable and set stop–loss percentage.
- `max_hold_secs` – after this many seconds, the engine may close positions.
- `max_hold_pnl` – whether max–hold auto–close is gated by PnL (e.g. only close if under a threshold).
- Per–pair buy size (BNB/WBNB) and minimum liquidity thresholds.

You don’t have to edit JSON by hand; just change values through the TUI and they will persist.

### Settings cache

The `Settings` tab config controls:

- `hide_wallet` – hides the wallet block (address and BNB balance).
- `hide_runtime` – hides the runtime block (WS / pair counts).
- `sim_mode` – simulation vs real–mode trading.

These are stored in `.cache/settings.json`. On startup:

- The cache is loaded (if present).
- `sim_mode` is **forced to true at startup**, and then persisted, so you always boot in simulation mode even if you last shut down in real mode.

---

## Building and running

Clone the repo and build:

```bash
git clone <this-repo-url>
cd JimmyBSC
cargo build --release
```

Set up your `.env`:

```bash
cp .env.example .env   # if you have one, otherwise create it
```

Fill in at least:

```bash
BSC_WSS=wss://your-bsc-websocket
BSC_RPC=https://your-bsc-rpc
PRIVATE_KEY=0xYourPrivateKey
```

Then run:

```bash
cargo run --release
```

If everything is wired correctly, you should see:

- The TUI switch to the alternate screen.
- Wallet / runtime info at the top left.
- Hermes feed populating on the Home tab as new pairs appear.

Logs will be written to `logs/` (e.g. `logs/logs_00-27-11-2025.txt` and `pancakes.log`).

---

## TUI layout and navigation

JimmyB runs inside a single terminal window using `ratatui` + `crossterm`.

### Global navigation

- Tabs: `Home`, `Auto Trade`, `Results`, `Settings`.
- Keyboard:
  - `←` / `→` – switch tabs.
  - `q` or `Esc` – quit.
  - `Ctrl+C` – quit.
- Mouse:
  - Move the cursor over a tab; click to switch.
  - Click the underlined actions in the Results / Settings / Auto Trade sections.

### Home tab

You’ll see:

- A title bar with chain ID, session runtime, and short wallet address.
- Wallet box:
  - Short address (e.g. `0x1234…ABCD`).
  - BNB balance (shortened).
  - Average gas fee estimate (USD) computed via `CalculateFee` on `BscClient`.
- Runtime box:
  - WebSocket status.
  - Pairs cached counts per DEX (V2, V3, FourMeme).
- Main “Hermes” list:
  - New pairs with labels, liquidity, price, buy counts, and simple flags.
  - Scrollable via `j/k`, `↑/↓`, `PageUp/PageDown`, `Home/End`.

The Hermes list is where the Auto Trade logic listens for opportunities; what you see here is essentially what the engine sees.

### Auto Trade tab

This tab is a thin wrapper around `draw_config_main` from `src/libs/tui/config_modals/main.rs`, which combines:

- Enable/disable toggle for auto trading.
- DEX selection (V2 / V3 / FourMeme).
- Buy amount configuration.
- Advanced section with:
  - Take profit / stop loss.
  - Max positions.
  - Max hold and PnL gating.
  - Liquidity / volume filters.

You can:

- Click on fields to focus them.
- Type numeric values.
- Press `Enter` to save or `Esc` to cancel.

Values are immediately stored in the `ConfigStore` and later written to `.cache/autotrade.json`.

### Results tab

The Results tab uses `src/app/results.rs` to:

- Render a list of open and closed positions with:
  - Pair address.
  - DEX type (V2, V3, FourMeme).
  - Base/quote symbols.
  - Entry vs current price.
  - PnL (percentage and WBNB).
  - Liquidity flag / out–of–liq alerts.
  - Frozen status.
- Attach clickable regions for:
  - `Take All` (or `Ok` when there is a pending liquidity ack).
  - Per–position:
    - `Take` – full close (sim or real).
    - `Freeze` / `Unfreeze` – prevent TP/SL auto closes.
    - `Remove` – stop managing a position without on–chain action.
    - `10%`, `25%`, `50%` partial sells (sim or real).

In simulation:

- Actions call into `SimEngine` (`take_all`, `partial_take`, `toggle_freeze`, `take_position`) and log messages like:
  - `[sim] ✓ MANUAL TAKE closed …`.
  - `[sim] ◐ PARTIAL TAKE …`.

In real mode:

- Actions use the auto–trade module:
  - `manual_sell_all` / `manual_sell` / `manual_remove_position`.
- On–chain activity uses:
  - Pancake routers for Pancake pairs.
  - FourMeme router / helpers for FourMeme tokens.
- All operations are logged as `[trade] …` events.

The scrollbar on the right lets you navigate long sessions.

### Settings tab

Settings is intentionally small and opinionated:

- `Hide Wallet` – toggle wallet display.
- `Hide Runtime` – toggle runtime info box.
- `Simulation Mode` – global safety toggle:
  - When ON (default): all trading is simulated.
  - When OFF: Hermes / Auto Trade may perform real trades.

Every toggle is persisted in `.cache/settings.json`. On startup:

- Current settings are loaded.
- `sim_mode` is set to `true` and saved again to enforce a safe default.

---

## Logs

Logs live under `logs/`:

- `logs/logs_00-27-11-2025.txt` – main TUI / trade log (the date stamp will change per run).
- `logs/pancakes.log` – lower–level logs from Pancake helpers and tests.

Typical messages include:

- WebSocket connectivity:
  - `[ws/v3] subscribed to PancakeV3 PoolCreated`
  - `[ws] connected`
- Simulation engine events:
  - `[sim] WAITING <token>: 0 buys < min 3`
  - `[sim] ✓ SUBMITTED <token> @ <price> (buys:3 liq:$20041)`
  - `[sim] EXECUTED buy …`
- Real trades and approvals:
  - `[allow] ✓ approved <token> for v2`
  - `[trade] ✓ V2 BUY <token> via WBNB (0.00001 BNB) tx=…`
  - `[trade] ✗ V2 BUY <token> … but token balance did not increase`

You can tail these while running the TUI to understand how the engine is behaving.

---

## Safety notes

If you flip Simulation mode OFF, you are giving the program permission to trade with your wallet on BSC. A few practical suggestions:

- Start with tiny BNB amounts (e.g. `0.00001`) until you’re comfortable.
- Never point this at a wallet that holds funds you can’t afford to lose.
- Expect failures: slippage, liquidity, and token contract quirks can all break trades.
- Read through the code path in:
  - `src/app/auto_trade.rs`
  - `src/router.rs`
  - `fourmeme/src/trade.rs`

so you understand what’s getting called on–chain.

---

## Development notes

- Main entrypoint: `src/main.rs` (`handler::init()`).
- TUI controller: `src/app/handler.rs`.
- Auto trade logic: `src/app/auto_trade.rs`.
- Simulation engine: `src/libs/sim.rs`.
- TUI widgets / layout: `src/libs/tui/*`.
- FourMeme helpers: `fourmeme/src/*`.
- Pancake helpers: `pancakes/*`.

The repo includes a handful of `#[tokio::test]` integration tests in `src/main.rs` that exercise:

- FourMeme buy/sell routes.
- Pancake V2/V3 price quoting.
- Simple swap flows via `routy_v2` / `routy_v3`.
