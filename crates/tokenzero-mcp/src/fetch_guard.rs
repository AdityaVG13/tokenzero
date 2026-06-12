//! SSRF guard for `tz_fetch`: URL/host validation, allow/deny lists, and
//! post-DNS IP checks with connection pinning. Without this, any MCP-capable
//! agent could fetch loopback, RFC1918, link-local, and cloud-metadata
//! endpoints and cache the bodies behind refs.

use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

/// A validated fetch destination for one redirect hop.
#[derive(Debug)]
pub(crate) struct FetchTarget {
    pub(crate) host: String,
    pub(crate) port: u16,
    /// IP to pin curl's connection to via `--resolve`, closing the
    /// resolve-then-connect (DNS rebinding) window. None when the host is
    /// allowlisted (explicitly trusted as configured) or is an IP literal
    /// that already passed the checks.
    pub(crate) pinned_ip: Option<IpAddr>,
}

#[derive(Debug)]
pub(crate) struct FetchBlocked {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) repair: Option<String>,
}

impl FetchBlocked {
    fn new(code: &'static str, message: String) -> Self {
        Self {
            code,
            message,
            repair: None,
        }
    }

    fn with_repair(code: &'static str, message: String, repair: &str) -> Self {
        Self {
            code,
            message,
            repair: Some(repair.to_string()),
        }
    }
}

/// Validate one fetch URL against the configured policy. Allowlisted hosts
/// skip resolution and IP checks: the allowlist is the explicit escape hatch
/// for intentionally reachable private hosts.
pub(crate) fn validate_fetch_target(
    url: &str,
    allow_hosts: &[String],
    deny_hosts: &[String],
) -> Result<FetchTarget, FetchBlocked> {
    let (host, port) = parse_host_port(url)?;
    if deny_hosts
        .iter()
        .any(|pattern| host_matches(&host, pattern))
    {
        return Err(FetchBlocked::new(
            "fetch_blocked",
            format!("host {host} is denied by TOKENZERO_FETCH_DENY"),
        ));
    }
    if allow_hosts
        .iter()
        .any(|pattern| host_matches(&host, pattern))
    {
        return Ok(FetchTarget {
            host,
            port,
            pinned_ip: None,
        });
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        if let Some(reason) = blocked_ip_reason(ip) {
            return Err(blocked_ip_error(&host, ip, reason));
        }
        return Ok(FetchTarget {
            host,
            port,
            pinned_ip: None,
        });
    }
    let addrs: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|err| {
            FetchBlocked::new(
                "fetch_failed",
                format!("could not resolve host {host}: {err}"),
            )
        })?
        .collect();
    if addrs.is_empty() {
        return Err(FetchBlocked::new(
            "fetch_failed",
            format!("could not resolve host {host}"),
        ));
    }
    // Reject if ANY resolved address is blocked: a mixed answer is exactly
    // what a rebinding attack looks like.
    for addr in &addrs {
        if let Some(reason) = blocked_ip_reason(addr.ip()) {
            return Err(blocked_ip_error(&host, addr.ip(), reason));
        }
    }
    Ok(FetchTarget {
        host,
        port,
        pinned_ip: Some(addrs[0].ip()),
    })
}

fn blocked_ip_error(host: &str, ip: IpAddr, reason: &str) -> FetchBlocked {
    FetchBlocked::with_repair(
        "fetch_blocked",
        format!("host {host} resolves to {ip} ({reason}); refusing to fetch"),
        "add the host to TOKENZERO_FETCH_ALLOW to explicitly trust it",
    )
}

/// Extract (host, port) from an http(s) URL. Rejects userinfo to rule out
/// `http://trusted.com@evil.com/` confusion.
fn parse_host_port(url: &str) -> Result<(String, u16), FetchBlocked> {
    let (rest, default_port) = if let Some(rest) = url.strip_prefix("https://") {
        (rest, 443u16)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (rest, 80u16)
    } else {
        return Err(FetchBlocked::new(
            "invalid_url",
            format!("fetch requires an http(s) URL, got {url}"),
        ));
    };
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    if authority.is_empty() {
        return Err(FetchBlocked::new(
            "invalid_url",
            format!("URL has no host: {url}"),
        ));
    }
    if authority.contains('@') {
        return Err(FetchBlocked::new(
            "invalid_url",
            "userinfo in fetch URLs is not supported".to_string(),
        ));
    }
    if let Some(rest) = authority.strip_prefix('[') {
        // bracketed IPv6 literal, optionally followed by :port
        let Some((host, after)) = rest.split_once(']') else {
            return Err(FetchBlocked::new(
                "invalid_url",
                format!("unterminated IPv6 literal in {url}"),
            ));
        };
        let port = match after.strip_prefix(':') {
            None if after.is_empty() => default_port,
            Some(port_text) => parse_port(port_text, url)?,
            None => {
                return Err(FetchBlocked::new(
                    "invalid_url",
                    format!("malformed authority in {url}"),
                ));
            }
        };
        return Ok((format!("[{}]", host.to_ascii_lowercase()), port));
    }
    match authority.split_once(':') {
        Some((host, port_text)) if !port_text.contains(':') => {
            Ok((host.to_ascii_lowercase(), parse_port(port_text, url)?))
        }
        Some(_) => Err(FetchBlocked::new(
            "invalid_url",
            format!("IPv6 hosts must be bracketed in {url}"),
        )),
        None => Ok((authority.to_ascii_lowercase(), default_port)),
    }
}

fn parse_port(text: &str, url: &str) -> Result<u16, FetchBlocked> {
    text.parse::<u16>()
        .map_err(|_| FetchBlocked::new("invalid_url", format!("invalid port in {url}")))
}

/// Suffix match: `example.com` matches `example.com` and `api.example.com`.
fn host_matches(host: &str, pattern: &str) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    if pattern.is_empty() {
        return false;
    }
    host == pattern || host.ends_with(&format!(".{pattern}"))
}

/// Why an IP must not be fetched, or None when it is publicly routable.
fn blocked_ip_reason(ip: IpAddr) -> Option<&'static str> {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            if v4.is_unspecified() || octets[0] == 0 {
                Some("unspecified")
            } else if v4.is_loopback() {
                Some("loopback")
            } else if v4.is_private() {
                Some("private (RFC1918)")
            } else if v4.is_link_local() {
                // includes the 169.254.169.254 cloud metadata endpoint
                Some("link-local")
            } else if v4.is_broadcast() {
                Some("broadcast")
            } else if octets[0] == 100 && (octets[1] & 0xC0) == 64 {
                Some("carrier-grade NAT (100.64/10)")
            } else if octets[0] == 192 && octets[1] == 0 && octets[2] == 0 {
                Some("IETF protocol assignments (192.0.0/24)")
            } else {
                None
            }
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return blocked_ip_reason(IpAddr::V4(mapped));
            }
            let segments = v6.segments();
            if v6.is_unspecified() {
                Some("unspecified")
            } else if v6.is_loopback() {
                Some("loopback")
            } else if (segments[0] & 0xffc0) == 0xfe80 {
                Some("link-local")
            } else if (segments[0] & 0xfe00) == 0xfc00 {
                Some("unique-local (fc00::/7)")
            } else {
                None
            }
        }
    }
}

/// Marker appended to curl stdout via `-w` so one invocation yields body,
/// status code, and redirect target without following redirects itself.
pub(crate) const FETCH_META_MARKER: &str = "__TOKENZERO_FETCH_META__";

/// Split curl output into (body, http_code, redirect_url). Output from a
/// curl that ignored `-w` (or a truncated capture) parses as a plain body.
pub(crate) fn split_fetch_meta(stdout: &str) -> (String, Option<u16>, Option<String>) {
    let marker = format!("\n{FETCH_META_MARKER} ");
    let Some((body, meta)) = stdout.rsplit_once(&marker) else {
        return (stdout.to_string(), None, None);
    };
    let meta = meta.trim();
    let (code_text, redirect) = meta.split_once(' ').unwrap_or((meta, ""));
    let redirect = redirect.trim();
    (
        body.to_string(),
        code_text.parse().ok(),
        (!redirect.is_empty()).then(|| redirect.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(url: &str) -> Result<FetchTarget, FetchBlocked> {
        validate_fetch_target(url, &[], &[])
    }

    #[test]
    fn blocks_loopback_private_linklocal_and_metadata_literals() {
        for url in [
            "http://127.0.0.1/",
            "http://10.1.2.3/x",
            "http://172.16.9.1:8080/",
            "http://192.168.0.10/",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.1.1/",
            "http://0.0.0.0/",
            "http://[::1]/",
            "http://[fe80::1]/",
            "http://[fd00::2]/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            let blocked = validate(url).err().unwrap_or_else(|| {
                panic!("{url} must be blocked");
            });
            assert_eq!(blocked.code, "fetch_blocked", "{url}");
        }
    }

    #[test]
    fn resolution_based_loopback_is_blocked() {
        // `localhost` resolves locally — hermetic in any environment.
        let blocked = validate("http://localhost:9999/admin").err().unwrap();
        assert_eq!(blocked.code, "fetch_blocked");
        assert!(blocked.message.contains("loopback"), "{}", blocked.message);
    }

    #[test]
    fn allowlist_is_an_explicit_escape_hatch_and_deny_wins() {
        let allow = vec!["localhost".to_string()];
        let target = validate_fetch_target("http://localhost:3000/", &allow, &[]).unwrap();
        assert_eq!(target.host, "localhost");
        assert_eq!(target.port, 3000);
        assert!(target.pinned_ip.is_none());

        let deny = vec!["localhost".to_string()];
        let blocked = validate_fetch_target("http://localhost:3000/", &allow, &deny)
            .err()
            .unwrap();
        assert!(blocked.message.contains("denied"));
    }

    #[test]
    fn host_matching_is_suffix_based_and_case_insensitive() {
        assert!(host_matches("api.example.com", "example.com"));
        assert!(host_matches("example.com", "Example.COM"));
        assert!(!host_matches("notexample.com", "example.com"));
        assert!(!host_matches("example.com.evil.net", "example.com"));
    }

    #[test]
    fn userinfo_and_malformed_urls_are_rejected() {
        for url in [
            "http://trusted.com@127.0.0.1/",
            "ftp://example.com/",
            "http:///path",
            "http://[::1/",
        ] {
            assert!(validate(url).is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn split_fetch_meta_separates_body_code_and_redirect() {
        let stdout = format!("hello body\n{FETCH_META_MARKER} 302 https://next.example.com/x\n");
        let (body, code, redirect) = split_fetch_meta(&stdout);
        assert_eq!(body, "hello body");
        assert_eq!(code, Some(302));
        assert_eq!(redirect.as_deref(), Some("https://next.example.com/x"));

        let plain = "no marker here\n";
        let (body, code, redirect) = split_fetch_meta(plain);
        assert_eq!(body, plain);
        assert_eq!(code, None);
        assert_eq!(redirect, None);

        let final_hop = format!("body\n{FETCH_META_MARKER} 200 ");
        let (body, code, redirect) = split_fetch_meta(&final_hop);
        assert_eq!(body, "body");
        assert_eq!(code, Some(200));
        assert_eq!(redirect, None);
    }
}
