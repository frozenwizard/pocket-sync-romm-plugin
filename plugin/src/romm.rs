//! Talking to the RomM server. See Architecture.md, section 8, and docs/research/romm-api.md.

pub mod pairing;

use std::fmt;

use extism_pdk::json;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::Url;

use crate::pocket_sync::{PocketSync, Request, Response};

/// The URL RomM's paths join onto, made by `base_url`. It ends in `/`, so a server under a sub-path
/// such as `https://example.com/romm` works.
#[derive(Debug, PartialEq)]
pub struct BaseUrl(Url);

impl BaseUrl {
    /// The URL of the API endpoint at `segments` under `api/`, each escaped as one path segment, so
    /// a value from the card can't change which endpoint is called.
    fn api(&self, segments: &[&str]) -> String {
        self.api_with_query(segments, &[])
    }

    /// `api`'s URL with `query` added, each name and value escaped.
    fn api_with_query(&self, segments: &[&str], query: &[(&str, &str)]) -> String {
        let mut url = self.0.clone();
        url.path_segments_mut()
            .expect("an http URL has a path")
            .pop_if_empty()
            .push("api")
            .extend(segments);
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        url.into()
    }

    /// The URL of the web page at `path`, as RomM gives it (`/pair/device?…`), under the server's
    /// sub-path. `None` if the result isn't a URL. It always starts with the base URL, so the page
    /// is on the user's server, whatever `path` holds.
    fn page(&self, path: &str) -> Option<String> {
        let page = format!("{}{}", self.0.as_str(), path.trim_start_matches('/'));
        let url = Url::parse(&page).ok()?;
        Some(url.into())
    }
}

/// The sign-in RomM gave the Pocket when it paired: a token for the API, bound to the Pocket's
/// RomM device, and that device's ID.
#[derive(PartialEq)]
pub struct SignIn {
    api_key: String,
    device_id: String,
}

impl SignIn {
    /// The sign-in with token `api_key` for device `device_id`. `None` unless both are there, and
    /// the token is one Extism can send: a token with a space or a line break in it would end the
    /// call.
    pub fn new(api_key: String, device_id: String) -> Option<SignIn> {
        let sendable = !api_key.is_empty() && api_key.chars().all(|c| c.is_ascii_graphic());
        (sendable && !device_id.is_empty()).then_some(SignIn { api_key, device_id })
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }
}

// Leaves the token out, so a debug print can't put it in a log (Architecture §13, rule 9).
impl fmt::Debug for SignIn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("SignIn")
            .field("device_id", &self.device_id)
            .finish_non_exhaustive()
    }
}

/// Why a request to RomM didn't get an answer the plugin can use.
#[derive(Debug, PartialEq)]
pub enum RequestError {
    /// The request failed without a response. A refused connection or a bad certificate ends the
    /// call instead, so this is rare.
    NoResponse(String),
    /// RomM answered with a status or a body the plugin didn't expect. Holds the HTTP status.
    Unexpected(u16),
}

/// Why connecting to RomM failed.
#[derive(Debug, PartialEq)]
pub enum ConnectError {
    Request(RequestError),
    /// Something answered, but not as RomM does. Holds the HTTP status.
    NotRomm(u16),
    /// RomM is older than 5.4. Holds its version.
    TooOld(String),
}

/// What RomM made of the sign-in saved on the card.
#[derive(Debug, PartialEq)]
pub enum SignInCheck {
    Accepted,
    /// RomM doesn't accept the token (expired, revoked, its device deleted, or from another
    /// server), or the device ID isn't the token's.
    Rejected,
}

/// Why RomM didn't delete the Pocket's device.
#[derive(Debug, PartialEq)]
pub enum DeleteDeviceError {
    /// RomM doesn't accept the token (revoked, expired, from another server, or its device already
    /// deleted), so it can't say whether the device is there.
    Rejected,
    /// The token works, but pairing didn't grant `devices.write`.
    NotAllowed,
    /// The token works, but the device ID isn't its device.
    NoDevice,
    Request(RequestError),
}

impl From<RequestError> for DeleteDeviceError {
    fn from(error: RequestError) -> Self {
        DeleteDeviceError::Request(error)
    }
}

/// Why RomM didn't list the collections or games asked for.
#[derive(Debug, PartialEq)]
pub enum ListError {
    /// RomM doesn't accept the token (expired, revoked, its device deleted, or from another
    /// server).
    Rejected,
    /// The token works, but pairing didn't grant the permission the list needs.
    NotAllowed,
    Request(RequestError),
}

impl From<RequestError> for ListError {
    fn from(error: RequestError) -> Self {
        ListError::Request(error)
    }
}

/// A regular collection in RomM: the user's own, or another user's public one. Keeps only what the
/// plugin reads.
#[derive(Debug, Deserialize)]
pub struct Collection {
    pub id: u64,
    pub name: String,
}

/// A game in a collection, with its files. Keeps only the fields syncing needs.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "only logged until Sync now plans from it")]
pub struct Game {
    pub id: u64,
    /// `None` when RomM hasn't matched the game.
    pub name: Option<String>,
    pub platform_slug: String,
    pub files: Vec<GameFile>,
}

/// One of a game's files.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "only logged until Sync now plans from it")]
pub struct GameFile {
    pub id: u64,
    pub file_name: String,
    pub file_size_bytes: u64,
    pub md5_hash: Option<String>,
    /// `game`, `dlc`, `hack`, `manual`…, or `None` when uncategorised.
    pub category: Option<String>,
    /// The files inside, when it's an archive.
    pub archive_members: Option<Vec<ArchiveMember>>,
}

impl GameFile {
    /// Whether it may be the game itself rather than a manual, DLC, patch or other extra: the first
    /// filter of Architecture §9.2, before the extensions the core accepts. RomM categorises a file
    /// by the top folder it's in within a game's folder, so a game that's a single file, a file at
    /// the top of a game's folder, or one in a folder such as `Extras/`, has no category.
    pub fn is_game(&self) -> bool {
        matches!(self.category.as_deref(), None | Some("game"))
    }
}

/// A file inside an archive.
#[derive(Debug, Deserialize)]
#[expect(dead_code, reason = "only logged until Sync now plans from it")]
pub struct ArchiveMember {
    pub name: String,
    pub size: u64,
    pub md5_hash: Option<String>,
}

/// How many games to ask for at once. Fewer than RomM allows, so a page with every game's files
/// stays well under the 50 MB Extism allows a response.
const GAMES_PER_PAGE: usize = 200;

/// One page of `GET /api/roms`.
#[derive(Deserialize)]
struct GamesPage {
    items: Vec<Game>,
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

/// The URL RomM's paths join onto. `None` unless `host` is an `http://` or `https://` address with
/// a server and nothing after its path. It's checked with the URL parser Extism uses, because
/// Extism ends the call on an address it can't parse (and can't be asked to try `https://` then
/// `http://` either).
pub fn base_url(host: &str) -> Option<BaseUrl> {
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
    let mut base = url;
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    Some(BaseUrl(base))
}

/// Sends `request`, failing with `NoResponse` when no response arrives.
fn send(pocket_sync: &mut impl PocketSync, request: &Request) -> Result<Response, RequestError> {
    pocket_sync.send(request).map_err(RequestError::NoResponse)
}

/// Reads `response`'s JSON body, failing with `Unexpected` when it isn't the shape expected.
fn parse<T: DeserializeOwned>(response: &Response) -> Result<T, RequestError> {
    json::from_slice(&response.body).map_err(|_| RequestError::Unexpected(response.status))
}

/// Checks the server at `base` is RomM 5.4 or newer, and returns its version.
pub fn connect(pocket_sync: &mut impl PocketSync, base: &BaseUrl) -> Result<String, ConnectError> {
    let request = Request::get(base.api(&["heartbeat"]));
    let response = send(pocket_sync, &request).map_err(ConnectError::Request)?;
    if response.status != 200 {
        return Err(ConnectError::NotRomm(response.status));
    }
    let heartbeat: Heartbeat =
        parse(&response).map_err(|_| ConnectError::NotRomm(response.status))?;
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

/// Asks RomM whether it still accepts `sign_in`, by reading the Pocket's device with it.
pub fn check_sign_in(
    pocket_sync: &mut impl PocketSync,
    base: &BaseUrl,
    sign_in: &SignIn,
) -> Result<SignInCheck, RequestError> {
    let request =
        Request::get(base.api(&["devices", sign_in.device_id()])).with_bearer(sign_in.api_key());
    let response = send(pocket_sync, &request)?;
    match response.status {
        // A 403 means the token works, but pairing didn't grant `devices.read`.
        200 | 403 => Ok(SignInCheck::Accepted),
        // RomM hides another device's ID from a token bound to one device as a 404.
        401 | 404 => Ok(SignInCheck::Rejected),
        status => Err(RequestError::Unexpected(status)),
    }
}

/// Deletes the Pocket's device in RomM with `sign_in`. RomM deletes the device's tokens with it, so
/// `sign_in` stops working too.
pub fn delete_device(
    pocket_sync: &mut impl PocketSync,
    base: &BaseUrl,
    sign_in: &SignIn,
) -> Result<(), DeleteDeviceError> {
    let request =
        Request::delete(base.api(&["devices", sign_in.device_id()])).with_bearer(sign_in.api_key());
    let response = send(pocket_sync, &request)?;
    match response.status {
        // RomM answers 204.
        200..=299 => Ok(()),
        401 => Err(DeleteDeviceError::Rejected),
        403 => Err(DeleteDeviceError::NotAllowed),
        404 => Err(DeleteDeviceError::NoDevice),
        status => Err(RequestError::Unexpected(status).into()),
    }
}

/// The regular collections `sign_in`'s user can see: their own, and other users' public ones.
pub fn collections(
    pocket_sync: &mut impl PocketSync,
    base: &BaseUrl,
    sign_in: &SignIn,
) -> Result<Vec<Collection>, ListError> {
    let request = Request::get(base.api(&["collections"])).with_bearer(sign_in.api_key());
    get_list(pocket_sync, &request)
}

/// Every game in collection `collection_id`, with its files, a page at a time.
pub fn collection_games(
    pocket_sync: &mut impl PocketSync,
    base: &BaseUrl,
    sign_in: &SignIn,
    collection_id: u64,
) -> Result<Vec<Game>, ListError> {
    let collection_id = collection_id.to_string();
    let limit = GAMES_PER_PAGE.to_string();
    let mut games = Vec::new();
    loop {
        let offset = games.len().to_string();
        let query = [
            ("collection_id", collection_id.as_str()),
            ("with_files", "true"),
            // RomM adds these unless told not to, and the ID index scans every game on each page.
            ("with_provider_metadata", "false"),
            ("with_char_index", "false"),
            ("with_filter_values", "false"),
            ("with_rom_id_index", "false"),
            // In a fixed order, so no game moves between pages.
            ("order_by", "id"),
            ("order_dir", "asc"),
            ("limit", limit.as_str()),
            ("offset", offset.as_str()),
        ];
        let request =
            Request::get(base.api_with_query(&["roms"], &query)).with_bearer(sign_in.api_key());
        let page: GamesPage = get_list(pocket_sync, &request)?;
        let last_page = page.items.len() < GAMES_PER_PAGE;
        games.extend(page.items);
        if last_page {
            return Ok(games);
        }
    }
}

/// Sends `request` for a list and reads its JSON body.
fn get_list<T: DeserializeOwned>(
    pocket_sync: &mut impl PocketSync,
    request: &Request,
) -> Result<T, ListError> {
    let response = send(pocket_sync, request)?;
    match response.status {
        200 => Ok(parse(&response)?),
        401 => Err(ListError::Rejected),
        403 => Err(ListError::NotAllowed),
        status => Err(RequestError::Unexpected(status).into()),
    }
}

/// The sign-in with token `api_key` for device `c1a2`.
#[cfg(test)]
pub fn test_sign_in(api_key: &str) -> SignIn {
    SignIn::new(api_key.into(), "c1a2".into()).unwrap()
}

/// RomM's error body, `{"detail": …}`.
#[cfg(test)]
pub fn test_error_response(detail: &str) -> String {
    json::json!({ "detail": detail }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pocket_sync::FakePocketSync;

    fn base(host: &str) -> BaseUrl {
        base_url(host).unwrap()
    }

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
        for (host, expected) in cases {
            assert_eq!(
                base_url(host).map(|base| String::from(base.0)).as_deref(),
                Some(expected),
                "{host}"
            );
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
    fn api_paths_join_onto_the_servers_path() {
        let base = base("https://example.com/romm");
        assert_eq!(
            base.api(&["auth", "device", "init"]),
            "https://example.com/romm/api/auth/device/init"
        );
        assert_eq!(
            base.api(&["devices", "a/b?c#d"]),
            "https://example.com/romm/api/devices/a%2Fb%3Fc%23d"
        );
    }

    #[test]
    fn pages_stay_on_the_server() {
        let base = base("https://example.com/romm");
        for (path, expected) in [
            (
                "/pair/device?user_code=FDF64KC5",
                "https://example.com/romm/pair/device?user_code=FDF64KC5",
            ),
            (
                "//evil.example/x",
                "https://example.com/romm/evil.example/x",
            ),
            (
                "javascript:alert(1)",
                "https://example.com/romm/javascript:alert(1)",
            ),
            ("/a b", "https://example.com/romm/a%20b"),
        ] {
            assert_eq!(base.page(path).as_deref(), Some(expected), "{path}");
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
        pocket_sync.respond("GET", HEARTBEAT, status, body);
        connect(&mut pocket_sync, &base("https://example.com/romm"))
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
    fn connecting_fails_without_a_response() {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.fail("GET", HEARTBEAT, "out of memory");
        assert_eq!(
            connect(&mut pocket_sync, &base("https://example.com/romm")),
            Err(ConnectError::Request(RequestError::NoResponse(
                "out of memory".into()
            )))
        );
    }

    const DEVICE: &str = "https://romm.example.com/api/devices/c1a2";

    fn check(status: u16, body: &str) -> Result<SignInCheck, RequestError> {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond("GET", DEVICE, status, body);
        check_sign_in(
            &mut pocket_sync,
            &base("https://romm.example.com"),
            &test_sign_in("rmm_secret"),
        )
    }

    #[test]
    fn the_sign_in_is_checked_by_reading_the_pockets_device() {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond("GET", DEVICE, 200, r#"{"id": "c1a2"}"#);
        let sign_in = test_sign_in("rmm_secret");
        let checked = check_sign_in(
            &mut pocket_sync,
            &base("https://romm.example.com"),
            &sign_in,
        );
        assert_eq!(checked, Ok(SignInCheck::Accepted));
        assert_eq!(
            pocket_sync.sent[0].headers,
            [("Authorization", "Bearer rmm_secret".to_string())]
        );
    }

    #[test]
    fn romm_accepts_or_rejects_the_sign_in() {
        let forbidden = test_error_response("Forbidden");
        assert_eq!(check(403, &forbidden), Ok(SignInCheck::Accepted));
        assert_eq!(check(401, "{}"), Ok(SignInCheck::Rejected));
        assert_eq!(check(404, "{}"), Ok(SignInCheck::Rejected));
        assert_eq!(check(500, "oops"), Err(RequestError::Unexpected(500)));
    }

    fn delete(status: u16) -> (Result<(), DeleteDeviceError>, FakePocketSync) {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond("DELETE", DEVICE, status, &test_error_response("x"));
        let deleted = delete_device(
            &mut pocket_sync,
            &base("https://romm.example.com"),
            &test_sign_in("rmm_secret"),
        );
        (deleted, pocket_sync)
    }

    #[test]
    fn the_pockets_device_is_deleted_with_its_sign_in() {
        let (deleted, pocket_sync) = delete(204);
        assert_eq!(deleted, Ok(()));
        assert_eq!(pocket_sync.sent[0].method, "DELETE");
        assert_eq!(
            pocket_sync.sent[0].headers,
            [("Authorization", "Bearer rmm_secret".to_string())]
        );
    }

    #[test]
    fn romm_may_refuse_to_delete_the_device() {
        for (status, error) in [
            (401, DeleteDeviceError::Rejected),
            (403, DeleteDeviceError::NotAllowed),
            (404, DeleteDeviceError::NoDevice),
            (
                500,
                DeleteDeviceError::Request(RequestError::Unexpected(500)),
            ),
        ] {
            assert_eq!(delete(status).0, Err(error), "{status}");
        }
    }

    const COLLECTIONS: &str = "https://romm.example.com/api/collections";

    #[test]
    fn collections_are_read_with_the_sign_in() {
        let mut pocket_sync = FakePocketSync::default();
        let body = r#"[
            {"id": 3, "name": "Favourites", "is_public": false, "rom_ids": [1, 2]},
            {"id": 7, "name": "Analogue Pocket", "is_public": false, "rom_ids": []}
        ]"#;
        pocket_sync.respond("GET", COLLECTIONS, 200, body);
        let collections = collections(
            &mut pocket_sync,
            &base("https://romm.example.com"),
            &test_sign_in("rmm_secret"),
        )
        .unwrap();
        let read: Vec<_> = collections.iter().map(|c| (c.id, &*c.name)).collect();
        assert_eq!(read, [(3, "Favourites"), (7, "Analogue Pocket")]);
        assert_eq!(
            pocket_sync.sent[0].headers,
            [("Authorization", "Bearer rmm_secret".to_string())]
        );
    }

    #[test]
    fn romm_may_refuse_to_list() {
        for (status, error) in [
            (401, ListError::Rejected),
            (403, ListError::NotAllowed),
            (500, ListError::Request(RequestError::Unexpected(500))),
        ] {
            let mut pocket_sync = FakePocketSync::default();
            pocket_sync.respond("GET", COLLECTIONS, status, &test_error_response("x"));
            let listed = collections(
                &mut pocket_sync,
                &base("https://romm.example.com"),
                &test_sign_in("rmm_secret"),
            );
            assert_eq!(listed.unwrap_err(), error, "{status}");
        }
    }

    /// The URL of the page of collection 7's games starting at `offset`.
    fn games_page(offset: usize) -> String {
        format!(
            "https://romm.example.com/api/roms?collection_id=7&with_files=true\
             &with_provider_metadata=false&with_char_index=false&with_filter_values=false\
             &with_rom_id_index=false&order_by=id&order_dir=asc&limit=200&offset={offset}"
        )
    }

    /// A page of games with IDs `ids`, each with one file, as RomM sends it.
    fn games_body(ids: impl Iterator<Item = u64>) -> String {
        let items: Vec<_> = ids
            .map(|id| {
                json::json!({
                    "id": id,
                    "name": format!("Game {id}"),
                    "platform_slug": "gb",
                    "fs_name": format!("Game {id}.gb"),
                    "files": [{
                        "id": id * 10,
                        "file_name": format!("Game {id}.gb"),
                        "file_size_bytes": 32768,
                        "md5_hash": "8e2ab8b3ef6f2d1e8e2ab8b3ef6f2d1e",
                        "category": null,
                        "archive_members": null
                    }]
                })
            })
            .collect();
        json::json!({ "items": items, "total": null, "limit": 200, "offset": 0 }).to_string()
    }

    #[test]
    fn a_collections_games_are_read_a_page_at_a_time() {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond("GET", &games_page(0), 200, &games_body(1..=200));
        pocket_sync.respond("GET", &games_page(200), 200, &games_body(201..=201));
        let games = collection_games(
            &mut pocket_sync,
            &base("https://romm.example.com"),
            &test_sign_in("rmm_secret"),
            7,
        )
        .unwrap();
        assert_eq!(games.len(), 201);
        assert_eq!(pocket_sync.sent.len(), 2);
        let last = &games[200];
        assert_eq!((last.id, last.name.as_deref()), (201, Some("Game 201")));
        assert_eq!(last.files[0].file_name, "Game 201.gb");
        assert_eq!(last.files[0].file_size_bytes, 32768);
    }

    #[test]
    fn a_games_archive_members_are_read() {
        let mut pocket_sync = FakePocketSync::default();
        let body = r#"{"items": [{
            "id": 4, "name": null, "platform_slug": "gba",
            "files": [{
                "id": 40, "file_name": "Game.zip", "file_size_bytes": 1000,
                "md5_hash": null, "category": "game",
                "archive_members": [{"name": "Game.gba", "size": 4194304, "crc_hash": "0a1b2c3d",
                                     "md5_hash": "1f2e3d4c5b6a79881f2e3d4c5b6a7988", "sha1_hash": null}]
            }]
        }]}"#;
        pocket_sync.respond("GET", &games_page(0), 200, body);
        let games = collection_games(
            &mut pocket_sync,
            &base("https://romm.example.com"),
            &test_sign_in("rmm_secret"),
            7,
        )
        .unwrap();
        let file = &games[0].files[0];
        assert_eq!(games[0].name, None);
        assert_eq!(file.category.as_deref(), Some("game"));
        let members = file.archive_members.as_ref().unwrap();
        assert_eq!((&*members[0].name, members[0].size), ("Game.gba", 4194304));
    }

    #[test]
    fn only_game_and_uncategorised_files_are_the_game() {
        let file = |category: Option<&str>| GameFile {
            id: 1,
            file_name: "Game.gb".into(),
            file_size_bytes: 32768,
            md5_hash: None,
            category: category.map(Into::into),
            archive_members: None,
        };
        for category in [None, Some("game")] {
            assert!(file(category).is_game(), "{category:?}");
        }
        for category in ["manual", "dlc", "hack", "patch", "update", "translation"] {
            assert!(!file(Some(category)).is_game(), "{category}");
        }
    }

    #[test]
    fn a_debug_print_of_a_sign_in_leaves_out_the_token() {
        let printed = format!("{:?}", test_sign_in("rmm_secret"));
        assert!(!printed.contains("rmm_secret"), "{printed}");
        assert!(printed.contains("c1a2"), "{printed}");
    }
}
