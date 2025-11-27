#![deny(unused_imports)]
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct PairState {
    pub upair_address: String,
    pub link_line: String,
    pub first_price: Option<f64>,
    pub last_price: Option<f64>,
    pub source: PairSource,
    pub last_nonzero_seen: Instant, // last time the pair had a non-zero price
    pub last_pnl_change_at: Instant,
    pub last_pnl: Option<i32>,
    pub below_thresh_since: Option<Instant>,
    pub liquidity_usd: Option<f64>,
    pub buy_count: u32,  // Real buy transactions from swap events
    pub sell_count: u32, // Real sell transactions from swap events
}

impl PairState {
    pub fn to_three_lines(&self) -> (String, String, String) {
        // Row 1: header without price
        let mut header = self.upair_address.clone();
        if let Some(idx) = header.find(" | Price:") {
            header.truncate(idx);
        }

        // PnL segment
        let pnl_seg: Option<String> =
            if let (Some(fp), Some(lp)) = (self.first_price, self.last_price) {
                if fp > 0.0 {
                    let pct = (lp / fp - 1.0) * 100.0;
                    let sign = if pct >= 0.0 { "+" } else { "" };
                    Some(format!("{}{:.2}%", sign, pct))
                } else {
                    None
                }
            } else {
                None
            };

        // Price text taken from original line if present; fallback to numeric
        let price_text: String = if let Some(idx) = self.upair_address.find("| Price:") {
            let after = &self.upair_address[idx + "| Price:".len()..];
            let trimmed = after.trim();
            let end = trimmed.find('|').unwrap_or(trimmed.len());
            trimmed[..end].trim().to_string()
        } else if let Some(p) = self.last_price {
            format!("{:.8}", p)
        } else {
            "?".to_string()
        };

        // Row 2: PnL, Price, Buys/Sells
        let mut row2_parts = Vec::new();

        if let Some(pnl) = pnl_seg {
            row2_parts.push(format!("PnL: {}", pnl));
        }
        row2_parts.push(format!("Price: {}", price_text));
        row2_parts.push(format!("B:{} S:{}", self.buy_count, self.sell_count));

        let row2 = format!("| {}", row2_parts.join(" | "));

        // Row 3: link
        let row3 = self.link_line.clone();

        (header, row2, row3)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PairSource {
    #[default]
    Unknown,
    V2,
    V3,
    FourMeme,
}

pub fn extract_price_f64(line: &str) -> Option<f64> {
    let needle = "| Price:";
    let idx = line.find(needle)?;
    let after = &line[idx + needle.len()..];
    let trimmed = after.trim();
    let mut end = trimmed.len();
    for (i, ch) in trimmed.char_indices() {
        if ch.is_whitespace() || ch == '|' {
            end = i;
            break;
        }
    }
    trimmed[..end].trim().parse::<f64>().ok()
}

pub fn detect_source(line: &str) -> PairSource {
    if line.starts_with("v2 ") || line.starts_with("v2 |") {
        PairSource::V2
    } else if line.starts_with("v3 ") || line.starts_with("v3 |") {
        PairSource::V3
    } else if line.starts_with("fm ") || line.starts_with("fm |") {
        PairSource::FourMeme
    } else {
        PairSource::Unknown
    }
}
