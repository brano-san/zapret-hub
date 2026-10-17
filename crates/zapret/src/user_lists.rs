//! The three lists `service.bat :load_user_lists` creates, with entry checks.

use crate::folder::Folder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum UserList {
    General,
    Exclude,
    IpsetExclude,
}

impl UserList {
    pub const ALL: [UserList; 3] = [Self::General, Self::Exclude, Self::IpsetExclude];

    pub fn file_name(self) -> &'static str {
        match self {
            Self::General => "list-general-user.txt",
            Self::Exclude => "list-exclude-user.txt",
            Self::IpsetExclude => "ipset-exclude-user.txt",
        }
    }

    /// What `:load_user_lists` writes into a missing file.
    fn default_content(self) -> &'static str {
        match self {
            Self::General => "# Never leave this file empty\r\ndomain.example.abc\r\n",
            Self::Exclude => "domain.example.abc\r\n",
            Self::IpsetExclude => "203.0.113.113/32\r\n",
        }
    }

    /// An empty list matches everything in winws, so the last entry is replaced by this.
    fn placeholder(self) -> &'static str {
        match self {
            Self::General | Self::Exclude => "domain.example.abc",
            Self::IpsetExclude => "203.0.113.113/32",
        }
    }
}

/// Domains the hub keeps in `list-general-user.txt`: missing ones are added back on every start.
pub const HUB_GENERAL: &str = include_str!("default-general-user.txt");

pub fn ensure_defaults(folder: &Folder) -> anyhow::Result<()> {
    for list in UserList::ALL {
        let path = folder.lists().join(list.file_name());
        if !path.exists() {
            std::fs::write(&path, list.default_content())?;
        }
    }
    let mut lines = read(folder, UserList::General)?;
    let missing: Vec<String> = HUB_GENERAL
        .lines()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .filter(|d| !lines.iter().any(|l| l.trim().eq_ignore_ascii_case(d)))
        .map(str::to_owned)
        .collect();
    if !missing.is_empty() {
        lines.extend(missing);
        save(folder, UserList::General, &lines)?;
    }
    Ok(())
}

/// Every non-blank line, comments included.
pub fn read(folder: &Folder, list: UserList) -> std::io::Result<Vec<String>> {
    let path = folder.lists().join(list.file_name());
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(crate::read_text(&path)?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_owned)
        .collect())
}

pub fn add(folder: &Folder, list: UserList, entry: &str) -> anyhow::Result<()> {
    let entry = normalize(list, entry)?;
    let mut lines = read(folder, list)?;
    if lines
        .iter()
        .any(|line| line.trim().eq_ignore_ascii_case(&entry))
    {
        return Ok(());
    }
    lines.push(entry);
    save(folder, list, &lines)
}

pub fn remove(folder: &Folder, list: UserList, entry: &str) -> anyhow::Result<()> {
    let mut lines = read(folder, list)?;
    lines.retain(|line| line.trim() != entry.trim());
    if !lines.iter().any(|line| !line.trim_start().starts_with('#')) {
        lines.push(list.placeholder().to_owned());
    }
    save(folder, list, &lines)
}

fn save(folder: &Folder, list: UserList, lines: &[String]) -> anyhow::Result<()> {
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    std::fs::write(folder.lists().join(list.file_name()), text)?;
    log::info!("{}: {} lines", list.file_name(), lines.len());
    Ok(())
}

/// A lower-cased domain for the host lists; an IP or CIDR for the ipset list.
pub fn normalize(list: UserList, entry: &str) -> anyhow::Result<String> {
    let entry = entry.trim();
    match list {
        UserList::IpsetExclude => {
            anyhow::ensure!(is_ip_or_cidr(entry), "not an IP address or CIDR: {entry}");
            Ok(entry.to_owned())
        }
        UserList::General | UserList::Exclude => {
            let domain = entry.to_ascii_lowercase();
            anyhow::ensure!(is_domain(&domain), "not a domain name: {entry}");
            Ok(domain)
        }
    }
}

fn is_domain(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    domain.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn is_ip_or_cidr(entry: &str) -> bool {
    let (ip, prefix) = match entry.split_once('/') {
        Some((ip, prefix)) => (ip, Some(prefix)),
        None => (entry, None),
    };
    let Ok(ip) = ip.parse::<std::net::IpAddr>() else {
        return false;
    };
    let max = if ip.is_ipv4() { 32 } else { 128 };
    prefix.is_none_or(|p| p.parse::<u8>().is_ok_and(|p| p <= max))
}
