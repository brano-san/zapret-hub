//! Linux: systemd-resolved's DNS over TLS, from its configuration files.

/// Where systemd-resolved reads drop-ins, as `systemd.unit` lists them.
const DROP_INS: [&str; 4] = [
    "/etc/systemd/resolved.conf.d",
    "/run/systemd/resolved.conf.d",
    "/usr/local/lib/systemd/resolved.conf.d",
    "/usr/lib/systemd/resolved.conf.d",
];

/// True when `/etc/systemd/resolved.conf` or one of its drop-ins turns DNS over TLS on.
// ponytail: resolved lets the last file win; any file saying so counts here. Per-link DoT
// (NetworkManager) and a stopped resolved are not looked at.
pub fn system_doh() -> bool {
    let mut files = vec![std::path::PathBuf::from("/etc/systemd/resolved.conf")];
    for dir in DROP_INS {
        if let Ok(entries) = std::fs::read_dir(dir) {
            files.extend(entries.flatten().map(|entry| entry.path()));
        }
    }
    files
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .any(|text| dns_over_tls(&text))
}

/// `DNSOverTLS=` true (any systemd spelling) or `opportunistic` in a `resolved.conf`: either
/// way names go encrypted whenever the server takes it.
fn dns_over_tls(conf: &str) -> bool {
    conf.lines().any(|line| {
        line.split_once('=').is_some_and(|(key, value)| {
            key.trim() == "DNSOverTLS"
                && matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "yes" | "true" | "on" | "1" | "y" | "t" | "opportunistic"
                )
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
        assert!(dns_over_tls("[Resolve]\nDNSOverTLS=true\n"));
        assert!(dns_over_tls("[Resolve]\nDNSOverTLS=1\n"));
        assert!(dns_over_tls("[Resolve]\nDNSOverTLS=On\n"));
        assert!(!dns_over_tls("[Resolve]\nDNSOverTLS=no\n"));
        assert!(!dns_over_tls("[Resolve]\nDNSOverTLS=false\n"));
        assert!(!dns_over_tls("[Resolve]\n#DNSOverTLS=yes\n"));
        assert!(!dns_over_tls("[Resolve]\n;DNSOverTLS=yes\n"));
        assert!(!dns_over_tls(""));
    }

    #[test]
    fn reads_this_machine_without_failing() {
        let _ = system_doh();
    }
}
