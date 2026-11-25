//! Talking to the RomM server. See Architecture.md, section 8, and docs/research/romm-api.md.

use extism_pdk::json;
use serde::Deserialize;
use url::Url;

use crate::pocket_sync::PocketSync;

/// Why connecting to RomM failed.
#[derive(Debug, PartialEq)]
pub enum ConnectError {
    /// The address doesn't start with `http://` or `https://`.
    BadHost,
    /// The request failed without a response. A refused connection or a bad certificate ends the
    /// call instead, so this is rare.
    NoResponse(String),
    /// Something answered, but not as RomM does. Holds the HTTP status.
    NotRomm(u16),
    /// RomM is older than 5.4. Holds its version.
    TooOld(String),
}

/// `GET /api/heartbeat`, keeping only what the plugin reads.
#[derive(Deserialize)]
struct Heartbeat {
    #[serde(rename = "SYSTEM")]
    system: System,
}

#[derive(Deserialize)]
struct System {
    #[serde(rename = "VERSION")]
    version: String,
}

/// The URL RomM's paths join onto, ending in `/` so a server under a sub-path such as
/// `https://example.com/romm` works. `None` unless `host` is an `http://` or `https://` address with
/// a server and nothing after its path. It's checked with the URL parser Extism uses, because
/// Extism ends the call on an address it can't parse (and can't be asked to try `https://` then
/// `http://` either).
pub fn base_url(host: &str) -> Option<String> {
    let url = Url::parse(host.trim()).ok()?;
    let usable = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|server| !server.is_empty())
        // A query or fragment would swallow the paths joined on, and a user name or password
        // would be sent to the server.
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none();
    if !usable {
        return None;
    }
    let mut base = String::from(url);
    if !base.ends_with('/') {
        base.push('/');
    }
    Some(base)
}

/// Checks the server at `host` is RomM 5.4 or newer, and returns its version.
pub fn connect(pocket_sync: &mut impl PocketSync, host: &str) -> Result<String, ConnectError> {
    let base = base_url(host).ok_or(ConnectError::BadHost)?;
    let response = pocket_sync
        .get(&format!("{base}api/heartbeat"))
        .map_err(ConnectError::NoResponse)?;
    if response.status != 200 {
        return Err(ConnectError::NotRomm(response.status));
    }
    let heartbeat: Heartbeat =
        json::from_slice(&response.body).map_err(|_| ConnectError::NotRomm(response.status))?;
    let version = heartbeat.system.version;
    if is_supported(&version) {
        Ok(version)
    } else {
        Err(ConnectError::TooOld(version))
    }
}

/// Whether RomM `version`, such as `5.4.0`, is 5.4 or newer. A development build counts as new.
fn is_supported(version: &str) -> bool {
    if version == "development" {
        return true;
    }
    let mut numbers = version.split('.').map(|part| part.parse::<u32>());
    match (numbers.next(), numbers.next()) {
        (Some(Ok(major)), Some(Ok(minor))) => (major, minor) >= (5, 4),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pocket_sync::FakePocketSync;

    #[test]
    fn a_host_gets_a_slash_for_paths_to_join_onto() {
        let cases = [
            ("https://romm.example.com", "https://romm.example.com/"),
            (" http://192.168.1.5:8080/ ", "http://192.168.1.5:8080/"),
            ("https://example.com/romm", "https://example.com/romm/"),
            ("http://192.168.1.5", "http://192.168.1.5/"),
            ("http://[::1]:8080", "http://[::1]:8080/"),
            (
                "https://romm.example.com:8443/romm",
                "https://romm.example.com:8443/romm/",
            ),
            // Written the way Extism will read it.
            ("HTTPS://Romm.Example.com", "https://romm.example.com/"),
            ("https://romm.example.com:443", "https://romm.example.com/"),
        ];
        for (host, base) in cases {
            assert_eq!(base_url(host).as_deref(), Some(base), "{host}");
        }
    }

    #[test]
    fn a_host_needs_http_or_https_and_a_server() {
        for host in [
            "romm.example.com",
            "ftp://x",
            "https://",
            "https://a b",
            "localhost:8080",
            "",
        ] {
            assert_eq!(base_url(host), None, "{host}");
        }
    }

    // Each would end the call (Extism can't parse it) or send the heartbeat somewhere else.
    #[test]
    fn a_host_must_be_an_address_extism_can_request() {
        for host in [
            "https://:8080",
            "https://romm.example.com:84a3",
            "https://romm.example.com:+80",
            "https://romm.example.com:70000",
            "https://[::1",
            "https://::1",
            "https://[nope]",
            "https://999.1.1.1",
            "https://user@romm.example.com",
            "https://romm.example.com?x",
            "https://romm.example.com/#x",
            "https://romm%20example.com",
            "https://romm.example.com\u{200E}",
            "https://xn--a.com",
            "https://foo.0x1",
            "https://romm.1.",
        ] {
            assert_eq!(base_url(host), None, "{host}");
        }
    }

    #[test]
    fn romm_5_4_or_newer_is_supported() {
        for version in ["5.4.0", "5.10.1", "6.0.0", "5.4.0-beta.1", "development"] {
            assert!(is_supported(version), "{version}");
        }
        for version in ["5.3.9", "4.9", "5", "unknown", ""] {
            assert!(!is_supported(version), "{version}");
        }
    }

    const HEARTBEAT: &str = "https://example.com/romm/api/heartbeat";

    fn connect_to(status: u16, body: &str) -> Result<String, ConnectError> {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond(HEARTBEAT, status, body);
        connect(&mut pocket_sync, "https://example.com/romm")
    }

    #[test]
    fn connecting_reads_the_version_from_the_heartbeat() {
        let body = r#"{"SYSTEM": {"VERSION": "5.4.0", "GIT_BRANCH": null}, "DEVICE_INSTALL": {}}"#;
        assert_eq!(connect_to(200, body), Ok("5.4.0".into()));
        let old = r#"{"SYSTEM": {"VERSION": "5.3.2"}}"#;
        assert_eq!(
            connect_to(200, old),
            Err(ConnectError::TooOld("5.3.2".into()))
        );
    }

    #[test]
    fn a_server_that_isnt_romm_is_refused() {
        assert_eq!(
            connect_to(404, "Not Found"),
            Err(ConnectError::NotRomm(404))
        );
        assert_eq!(connect_to(200, "<html>"), Err(ConnectError::NotRomm(200)));
        assert_eq!(connect_to(200, "{}"), Err(ConnectError::NotRomm(200)));
    }

    #[test]
    fn connecting_fails_without_a_response_or_a_usable_host() {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.fail(HEARTBEAT, "out of memory");
        assert_eq!(
            connect(&mut pocket_sync, "https://example.com/romm"),
            Err(ConnectError::NoResponse("out of memory".into()))
        );
        assert_eq!(
            connect(&mut pocket_sync, "example.com"),
            Err(ConnectError::BadHost)
        );
    }
}
