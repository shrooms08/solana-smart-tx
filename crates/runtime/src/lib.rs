//! Shared process-wide runtime setup for the smart-transaction stack.
//!
//! Two concerns live here:
//!
//! * [`init_crypto`] installs a rustls `CryptoProvider` as the process default.
//!   rustls 0.23 refuses to auto-select a provider when both `ring` and
//!   `aws-lc-rs` are present in the dependency graph (they are, transitively, via
//!   tonic / reqwest / tokio-tungstenite), so any TLS handshake panics with:
//!
//!   > Could not automatically determine the process-level CryptoProvider from
//!   > Rustls crate features
//!
//!   Calling [`init_crypto`] first thing in a binary's `main` fixes it for every
//!   TLS client in the process.
//!
//! * [`redact_url`] / [`mask_secret`] keep credentials out of logs. RPC/provider
//!   URLs frequently embed secrets (`?api_key=…`, a token path segment, or
//!   `user:pass@host`), and those URLs surface verbatim inside transport error
//!   strings. Route any log/error value that may contain a URL through
//!   [`redact_url`], and any bare token/key through [`mask_secret`].

use std::sync::Once;

static INIT: Once = Once::new();

/// Install the `ring`-backed rustls [`CryptoProvider`] as the process default,
/// exactly once.
///
/// Safe to call from anywhere, any number of times, from multiple crates: the
/// install runs under a [`Once`], and an `AlreadyInstalled` error (if some other
/// component beat us to it) is ignored. Call it at the top of `main` — before any
/// TLS client connects.
///
/// [`CryptoProvider`]: rustls::crypto::CryptoProvider
pub fn init_crypto() {
    INIT.call_once(|| {
        // Ignore the result: an `Err` only means a provider is already installed,
        // which is exactly the state we want.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

// ---------------------------------------------------------------------------
// Credential redaction
// ---------------------------------------------------------------------------

/// Mask a bare secret (token / api key) for logging: the first 4 characters
/// followed by `…`. Secrets of 4 chars or fewer are masked entirely so nothing
/// useful leaks. Empty input stays empty.
pub fn mask_secret(secret: &str) -> String {
    let len = secret.chars().count();
    if len == 0 {
        String::new()
    } else if len <= 4 {
        "…".to_string()
    } else {
        let prefix: String = secret.chars().take(4).collect();
        format!("{prefix}…")
    }
}

/// Redact credentials from any string that may contain URLs.
///
/// Works on arbitrary text (e.g. a transport error like
/// `error sending request for url (https://x.com/?api-key=SECRET): ...`): every
/// `scheme://…` run is located and, within it, the userinfo (`user:pass@`), any
/// token-like path segment, and **all** query-parameter values are masked. Text
/// outside URLs and the host/port are left untouched.
pub fn redact_url(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(rel) = input[cursor..].find("://") {
        let sep = cursor + rel;
        // Walk back over the scheme characters to find where the URL starts.
        let scheme_start = input[..sep]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'))
            .map(|i| i + 1)
            .unwrap_or(0)
            .max(cursor);

        // Emit untouched text before the URL.
        out.push_str(&input[cursor..scheme_start]);

        // The URL runs until the first whitespace or wrapping delimiter.
        let region = &input[scheme_start..];
        let end = region
            .find(|c: char| c.is_whitespace() || "\"'<>()[]{}|\\^`".contains(c))
            .unwrap_or(region.len());
        let raw = &region[..end];
        // Don't swallow trailing sentence punctuation that isn't part of the URL.
        let url = raw.trim_end_matches(['.', ',']);
        let trailer = &raw[url.len()..];

        out.push_str(&redact_single_url(url));
        out.push_str(trailer);
        cursor = scheme_start + end;
    }
    out.push_str(&input[cursor..]);
    out
}

fn redact_single_url(url: &str) -> String {
    match url.split_once('?') {
        Some((base, query)) => {
            format!("{}?{}", redact_authority_and_path(base), redact_query(query))
        }
        None => redact_authority_and_path(url),
    }
}

fn redact_authority_and_path(s: &str) -> String {
    let Some(idx) = s.find("://") else {
        return s.to_string();
    };
    let scheme = &s[..idx + 3];
    let rest = &s[idx + 3..];
    let (authority, path) = match rest.find('/') {
        Some(p) => (&rest[..p], &rest[p..]),
        None => (rest, ""),
    };
    format!("{scheme}{}{}", redact_userinfo(authority), redact_path(path))
}

fn redact_userinfo(authority: &str) -> String {
    match authority.rsplit_once('@') {
        Some((userinfo, host)) => {
            let masked = match userinfo.split_once(':') {
                // user:password — keep the user, hide the password.
                Some((user, _password)) => format!("{user}:****"),
                None => mask_secret(userinfo),
            };
            format!("{masked}@{host}")
        }
        None => authority.to_string(),
    }
}

fn redact_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    path.split('/')
        .map(|seg| {
            if looks_like_secret(seg) {
                mask_secret(seg)
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn redact_query(query: &str) -> String {
    query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) if !value.is_empty() => format!("{key}={}", mask_secret(value)),
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// A path segment long enough and opaque enough to plausibly be a token
/// (e.g. a QuickNode/Helius key embedded in the path). Normal segments like
/// `api`, `v1`, `bundles`, `mainnet-beta` are short and pass through.
fn looks_like_secret(segment: &str) -> bool {
    segment.len() >= 16
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_secret_reveals_prefix_only() {
        assert_eq!(mask_secret(""), "");
        assert_eq!(mask_secret("abcd"), "…"); // too short to reveal anything
        assert_eq!(mask_secret("abcde"), "abcd…");
        assert_eq!(mask_secret("supersecrettoken1234"), "supe…");
    }

    #[test]
    fn redact_query_api_key() {
        assert_eq!(
            redact_url("https://mainnet.helius-rpc.com/?api-key=abcdef0123456789"),
            "https://mainnet.helius-rpc.com/?api-key=abcd…"
        );
    }

    #[test]
    fn redact_multiple_query_params() {
        assert_eq!(
            redact_url("https://rpc.example.com/v1?token=topsecretvalue&commitment=finalized"),
            "https://rpc.example.com/v1?token=tops…&commitment=fina…"
        );
    }

    #[test]
    fn redact_token_path_segment() {
        // A long opaque path segment (QuickNode-style) gets masked; short
        // segments like `api`/`v1` pass through.
        assert_eq!(
            redact_url("https://name.solana-mainnet.quiknode.pro/abcdef0123456789abcd/api"),
            "https://name.solana-mainnet.quiknode.pro/abcd…/api"
        );
    }

    #[test]
    fn redact_userinfo_password() {
        assert_eq!(
            redact_url("https://user:hunter2pass@rpc.example.com/path"),
            "https://user:****@rpc.example.com/path"
        );
    }

    #[test]
    fn redact_inside_error_prose() {
        let err = "error sending request for url (https://x.com/?api_key=SECRET1234): connection refused";
        assert_eq!(
            redact_url(err),
            "error sending request for url (https://x.com/?api_key=SECR…): connection refused"
        );
    }

    #[test]
    fn redact_leaves_clean_urls_untouched() {
        // No secrets: query-less, short path -> unchanged.
        let clean = "https://api.mainnet-beta.solana.com";
        assert_eq!(redact_url(clean), clean);
        // No scheme at all -> unchanged.
        assert_eq!(redact_url("ams.grpc.solinfra.dev:443"), "ams.grpc.solinfra.dev:443");
    }

    #[test]
    fn redact_trailing_punctuation_preserved() {
        assert_eq!(
            redact_url("see https://x.com/?k=secretval123."),
            "see https://x.com/?k=secr…."
        );
    }
}
