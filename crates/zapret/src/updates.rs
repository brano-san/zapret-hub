//! Flowseal's update endpoints: the version file, the ipset list, the hosts file (5.6).

use crate::folder::Folder;
use crate::hosts::HostsPlan;

const RAW: &str = "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube";
/// The release page of a version: `RELEASE_URL` + version.
pub const RELEASE_URL: &str = "https://github.com/Flowseal/zapret-discord-youtube/releases/tag/";

pub fn version_url() -> String {
    format!("{RAW}/main/.service/version.txt")
}

pub fn ipset_url() -> String {
    format!("{RAW}/refs/heads/main/.service/ipset-service.txt")
}

pub fn hosts_url() -> String {
    format!("{RAW}/refs/heads/main/.service/hosts")
}

/// The latest released version, as `:service_check_updates` reads it.
pub fn latest_version(socks5: Option<&str>) -> anyhow::Result<String> {
    Ok(probe::curl::fetch_text(&version_url(), socks5)?
        .trim()
        .to_owned())
}

/// `Some(latest)` when it differs from the folder's version: `service.bat` compares the strings.
pub fn newer_version(folder: &Folder, socks5: Option<&str>) -> anyhow::Result<Option<String>> {
    let latest = latest_version(socks5)?;
    Ok((folder.version().as_deref() != Some(latest.as_str())).then_some(latest))
}

/// Downloads `ipset-service.txt` next to the list, then swaps it in.
pub fn update_ipset(folder: &Folder, socks5: Option<&str>) -> anyhow::Result<()> {
    let download = folder.lists().join("ipset-all.txt.download");
    let result = probe::curl::download(&ipset_url(), &download, socks5)
        .and_then(|()| crate::ipset::install(folder, &download));
    let _ = std::fs::remove_file(&download);
    result
}

fn flowseal_hosts(socks5: Option<&str>) -> anyhow::Result<String> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    probe::curl::fetch_text(&format!("{}?t={nonce}", hosts_url()), socks5)
}

/// What applying Flowseal's hosts would change.
pub fn hosts_plan(socks5: Option<&str>) -> anyhow::Result<HostsPlan> {
    let flowseal = flowseal_hosts(socks5)?;
    Ok(crate::hosts::plan(&crate::hosts::read()?, &flowseal))
}

/// Downloads Flowseal's hosts again and writes it as the zapret-hub block.
pub fn apply_hosts(socks5: Option<&str>) -> anyhow::Result<()> {
    crate::hosts::apply(&flowseal_hosts(socks5)?)
}
