//! Linux: systemd-resolved's DNS over TLS, from its configuration files.

/// True when `/etc/systemd/resolved.conf` or one of its drop-ins turns DNS over TLS on.
// ponytail: the last file wins in resolved; any file saying so counts here
pub fn system_doh() -> bool {
    let mut files = vec![std::path::PathBuf::from("/etc/systemd/resolved.conf")];
    if let Ok(entries) = std::fs::read_dir("/etc/systemd/resolved.conf.d") {
        files.extend(entries.flatten().map(|entry| entry.path()));
    }
    files
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .any(|text| dns_over_tls(&text))
}

/// `DNSOverTLS=yes` or `=opportunistic` in a `resolved.conf`: either way names go encrypted
/// whenever the server takes it.
fn dns_over_tls(conf: &str) -> bool {
    conf.lines().any(|line| {
        line.split_once('=').is_some_and(|(key, value)| {
            key.trim() == "DNSOverTLS" && matches!(value.trim(), "yes" | "opportunistic")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_over_tls_on_is_yes_or_opportunistic() {
        assert!(dns_over_tls("[Resolve]\nDNS=1.1.1.1\nDNSOverTLS=yes\n"));
        assert!(dns_over_tls("[Resolve]\nDNSOverTLS = opportunistic\n"));
        assert!(!dns_over_tls("[Resolve]\nDNSOverTLS=no\n"));
        assert!(!dns_over_tls("[Resolve]\n#DNSOverTLS=yes\n"));
        assert!(!dns_over_tls("[Resolve]\n;DNSOverTLS=yes\n"));
        assert!(!dns_over_tls(""));
    }

    #[test]
    fn reads_this_machine_without_failing() {
        let _ = system_doh();
    }
}
