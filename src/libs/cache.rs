use {
    anyhow::{Context, Result},
    serde::{Deserialize, Serialize},
    std::{fs, path::PathBuf},
};

/// Settings cache (UI preferences)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsCache {
    pub hide_wallet: bool,
    pub hide_runtime: bool,
    pub sim_mode: bool,
}

impl Default for SettingsCache {
    fn default() -> Self {
        Self {
            hide_wallet: false,
            hide_runtime: false,
            sim_mode: false,
        }
    }
}

/// Get cache directory path (creates if doesn't exist)
fn cache_dir() -> Result<PathBuf> {
    let mut path = std::env::current_dir()?;
    path.push(".cache");
    if !path.exists() {
        fs::create_dir_all(&path)?;
    }
    Ok(path)
}

/// Auto Trade config cache path
fn autotrade_cache_path() -> Result<PathBuf> {
    let mut path = cache_dir()?;
    path.push("autotrade.json");
    Ok(path)
}

/// Settings cache path
fn settings_cache_path() -> Result<PathBuf> {
    let mut path = cache_dir()?;
    path.push("settings.json");
    Ok(path)
}

/// Load Auto Trade config from cache
pub fn load_autotrade_cache() -> Result<std::collections::HashMap<String, String>> {
    let path = autotrade_cache_path()?;
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let map: std::collections::HashMap<String, String> =
        serde_json::from_str(&contents).with_context(|| "Failed to parse autotrade cache")?;
    Ok(map)
}

/// Save Auto Trade config to cache
pub fn save_autotrade_cache(store: &crate::libs::tui::ConfigStore) -> Result<()> {
    let path = autotrade_cache_path()?;
    let mut map = std::collections::HashMap::new();
    for entry in store.iter() {
        map.insert(entry.key().clone(), entry.value().clone());
    }
    let json = serde_json::to_string_pretty(&map)?;
    fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Load Settings from cache
pub fn load_settings_cache() -> Result<SettingsCache> {
    let path = settings_cache_path()?;
    if !path.exists() {
        return Ok(SettingsCache::default());
    }
    let contents =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let settings: SettingsCache =
        serde_json::from_str(&contents).with_context(|| "Failed to parse settings cache")?;
    Ok(settings)
}

/// Save Settings to cache
pub fn save_settings_cache(settings: &SettingsCache) -> Result<()> {
    let path = settings_cache_path()?;
    let json = serde_json::to_string_pretty(settings)?;
    fs::write(&path, json).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}
