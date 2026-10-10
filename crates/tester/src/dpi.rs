//! The hyperion-cs DPI 16-20 suite (Apache-2.0), fetched as `test zapret.ps1` fetches it; the last
//! good copy stays in `data/cache` (spec 7.2).

use std::path::Path;

pub const SUITE_URL: &str = "https://hyperion-cs.github.io/dpi-checkers/ru/tcp-16-20/suite.v2.json";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DpiTarget {
    pub id: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub country: String,
    pub host: String,
}

pub fn parse(json: &str) -> anyhow::Result<Vec<DpiTarget>> {
    Ok(serde_json::from_str(json)?)
}

pub fn load(cache_dir: &Path, socks5: Option<&str>) -> anyhow::Result<Vec<DpiTarget>> {
    from_fetch(cache_dir, probe::curl::fetch_text(SUITE_URL, socks5))
}

/// `fetched` when it parses (it becomes the cached copy), the cached copy otherwise.
pub fn from_fetch(
    cache_dir: &Path,
    fetched: anyhow::Result<String>,
) -> anyhow::Result<Vec<DpiTarget>> {
    let cache = cache_dir.join("dpi-suite.json");
    let fresh = fetched.and_then(|text| {
        let targets = parse(&text)?;
        anyhow::ensure!(!targets.is_empty(), "the DPI suite is empty");
        Ok((text, targets))
    });
    match fresh {
        Ok((text, targets)) => {
            std::fs::create_dir_all(cache_dir)?;
            std::fs::write(&cache, text)?;
            Ok(targets)
        }
        Err(err) => {
            log::warn!("DPI suite not downloaded, the cached copy is used: {err:#}");
            let text = std::fs::read_to_string(&cache).map_err(|_| {
                anyhow::anyhow!(
                    "the DPI suite could not be downloaded and there is no cached copy: {err:#}"
                )
            })?;
            parse(&text)
        }
    }
}
