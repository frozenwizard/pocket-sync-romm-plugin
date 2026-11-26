//! Pairing the Pocket with RomM as its own device: RomM's device sign-in, approved by the user on
//! RomM's web page while the plugin polls. See Architecture.md, sections 5.1 and 8.1.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{BaseUrl, RequestError, SignIn, parse, send};
use crate::card::PocketId;
use crate::pocket_sync::{PocketSync, Request};

/// How long to wait between polls while the user approves. RomM asks for 5 seconds and answers
/// `slow_down` to faster polls, but hands over the token on the first poll after approval all the
/// same, so a shorter wait gets the run going sooner.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Added to the wait between polls each time RomM answers `429 Too Many Requests`, which it does
/// past 60 polls a minute from one address.
pub const BACKOFF: Duration = Duration::from_secs(5);

/// The name RomM gives the Pocket's device. The approver can change it on RomM's page.
const DEVICE_NAME: &str = "Analogue Pocket";

/// How RomM's device list describes the device: the app that paired it, and what it is. `platform`
/// matches the `emulator` label the Pocket's saves are uploaded with.
const CLIENT: &str = "romm-sync";
const PLATFORM: &str = "analogue-pocket";

/// A RomM permission the plugin asks for when pairing (Architecture §8). A RomM user with the
/// Viewer role can only grant the `.read` ones.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Permission {
    PlatformsRead,
    RomsRead,
    CollectionsRead,
    CollectionsWrite,
    DevicesRead,
    DevicesWrite,
    AssetsRead,
    AssetsWrite,
    FirmwareRead,
}

impl Permission {
    /// Every permission asked for, covering every planned milestone so nobody has to pair again.
    pub const ALL: [Permission; 9] = [
        Permission::PlatformsRead,
        Permission::RomsRead,
        Permission::CollectionsRead,
        Permission::CollectionsWrite,
        Permission::DevicesRead,
        Permission::DevicesWrite,
        Permission::AssetsRead,
        Permission::AssetsWrite,
        Permission::FirmwareRead,
    ];

    /// Its name in RomM's API.
    pub fn name(self) -> &'static str {
        match self {
            Permission::PlatformsRead => "platforms.read",
            Permission::RomsRead => "roms.read",
            Permission::CollectionsRead => "collections.read",
            Permission::CollectionsWrite => "collections.write",
            Permission::DevicesRead => "devices.read",
            Permission::DevicesWrite => "devices.write",
            Permission::AssetsRead => "assets.read",
            Permission::AssetsWrite => "assets.write",
            Permission::FirmwareRead => "firmware.read",
        }
    }

    /// The permission RomM's API calls `name`, if it's one the plugin asks for.
    fn named(name: &str) -> Option<Permission> {
        Permission::ALL
            .into_iter()
            .find(|permission| permission.name() == name)
    }
}

/// The permissions asked for that `granted` leaves out.
pub fn missing(granted: &[Permission]) -> Vec<Permission> {
    Permission::ALL
        .into_iter()
        .filter(|permission| !granted.contains(permission))
        .collect()
}

/// A pairing waiting for the user's approval.
#[derive(PartialEq)]
pub struct Pairing {
    /// The secret the plugin polls with. Never logged.
    device_code: String,
    /// The code RomM's approval page shows, for the user to check or type in.
    pub user_code: String,
    /// RomM's approval page for this pairing.
    pub approval_url: String,
    /// How long RomM waits for approval.
    pub expires_in: Duration,
}

// Leaves out the code the plugin polls with, which gets the sign-in once the pairing is approved,
// so a debug print can't put it in a log (Architecture §13, rule 9).
impl fmt::Debug for Pairing {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Pairing")
            .field("user_code", &self.user_code)
            .field("approval_url", &self.approval_url)
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

/// Why pairing couldn't start.
#[derive(Debug, PartialEq)]
pub enum StartError {
    /// RomM has had too many pairing attempts from this computer in the last minute.
    Busy,
    Request(RequestError),
}

impl From<RequestError> for StartError {
    fn from(error: RequestError) -> Self {
        StartError::Request(error)
    }
}

/// RomM's answer to one poll.
#[derive(Debug, PartialEq)]
pub enum Poll {
    /// Not approved yet.
    Waiting,
    /// Polled too often: wait longer between polls.
    TooFast,
    Approved(Paired),
    Denied,
    /// The code expired before it was approved.
    Expired,
}

/// What pairing gives the Pocket.
#[derive(Debug, PartialEq)]
pub struct Paired {
    pub sign_in: SignIn,
    /// The permissions asked for that the approver granted, which can be fewer than all of them.
    pub permissions: Vec<Permission>,
}

/// `POST /api/auth/device/init`.
#[derive(Serialize)]
struct InitRequest {
    client_device_identifier: String,
    name: &'static str,
    client: &'static str,
    platform: &'static str,
    client_version: &'static str,
    requested_scopes: [&'static str; Permission::ALL.len()],
}

#[derive(Deserialize)]
struct InitResponse {
    device_code: String,
    user_code: String,
    verification_path_complete: String,
    expires_in: u64,
}

/// `POST /api/auth/device/token`.
#[derive(Serialize)]
struct TokenRequest<'a> {
    device_code: &'a str,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    device_id: String,
    #[serde(default)]
    scopes: Vec<String>,
}

/// RomM's error body, `{"detail": "…"}`.
#[derive(Deserialize)]
struct ErrorResponse {
    detail: String,
}

/// Starts pairing the Pocket with ID `pocket_id`. RomM gives the same device back each time the
/// same Pocket pairs.
pub fn start(
    pocket_sync: &mut impl PocketSync,
    base: &BaseUrl,
    pocket_id: PocketId,
) -> Result<Pairing, StartError> {
    let body = InitRequest {
        client_device_identifier: pocket_id.to_string(),
        name: DEVICE_NAME,
        client: CLIENT,
        platform: PLATFORM,
        client_version: env!("CARGO_PKG_VERSION"),
        requested_scopes: Permission::ALL.map(Permission::name),
    };
    let request = Request::post_json(base.api(&["auth", "device", "init"]), &body);
    let response = send(pocket_sync, &request)?;
    match response.status {
        200..=299 => {}
        429 => return Err(StartError::Busy),
        status => return Err(RequestError::Unexpected(status).into()),
    }
    let init: InitResponse = parse(&response)?;
    let approval_url = (base.page(&init.verification_path_complete))
        .ok_or(RequestError::Unexpected(response.status))?;
    Ok(Pairing {
        device_code: init.device_code,
        user_code: init.user_code,
        approval_url,
        expires_in: Duration::from_secs(init.expires_in),
    })
}

/// Asks RomM once whether `pairing` has been approved.
pub fn poll(
    pocket_sync: &mut impl PocketSync,
    base: &BaseUrl,
    pairing: &Pairing,
) -> Result<Poll, RequestError> {
    let body = TokenRequest {
        device_code: &pairing.device_code,
    };
    let request = Request::post_json(base.api(&["auth", "device", "token"]), &body);
    let response = send(pocket_sync, &request)?;
    match response.status {
        200 => {
            let token: TokenResponse = parse(&response)?;
            let sign_in = SignIn::new(token.access_token, token.device_id)
                .ok_or(RequestError::Unexpected(response.status))?;
            Ok(Poll::Approved(Paired {
                sign_in,
                permissions: (token.scopes.iter())
                    .filter_map(|name| Permission::named(name))
                    .collect(),
            }))
        }
        400 => {
            let error: ErrorResponse = parse(&response)?;
            match error.detail.as_str() {
                // RomM answers `slow_down` to every poll inside 5 seconds of the last one while it
                // waits, and still hands over the token on the first poll after approval.
                "authorization_pending" | "slow_down" => Ok(Poll::Waiting),
                "access_denied" => Ok(Poll::Denied),
                "expired_token" => Ok(Poll::Expired),
                _ => Err(RequestError::Unexpected(response.status)),
            }
        }
        429 => Ok(Poll::TooFast),
        status => Err(RequestError::Unexpected(status)),
    }
}

/// RomM's answer to starting a pairing, with the code `FDF64KC5`.
#[cfg(test)]
pub fn test_init_response() -> String {
    extism_pdk::json::json!({
        "device_code": "d".repeat(64),
        "user_code": "FDF64KC5",
        "verification_path": "/pair/device",
        "verification_path_complete": "/pair/device?user_code=FDF64KC5",
        "expires_in": 600,
        "interval": 5,
    })
    .to_string()
}

/// RomM's answer to a poll once the pairing is approved with `permissions`, giving the token
/// `rmm_paired` for device `c1a2`.
#[cfg(test)]
pub fn test_token_response(permissions: &[Permission]) -> String {
    let scopes: Vec<_> = permissions
        .iter()
        .map(|permission| permission.name())
        .collect();
    extism_pdk::json::json!({
        "id": 12,
        "access_token": "rmm_paired",
        "device_id": "c1a2",
        "scopes": scopes,
        "expires_at": null,
    })
    .to_string()
}

/// What a RomM user with the Viewer role can grant of what the plugin asks for.
#[cfg(test)]
pub fn viewer_permissions() -> Vec<Permission> {
    (Permission::ALL.into_iter())
        .filter(|permission| permission.name().ends_with(".read"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pocket_sync::FakePocketSync;
    use crate::romm::{base_url, test_error_response, test_sign_in};
    use extism_pdk::json::json;

    const INIT: &str = "https://example.com/romm/api/auth/device/init";
    const TOKEN: &str = "https://example.com/romm/api/auth/device/token";

    fn base() -> BaseUrl {
        base_url("https://example.com/romm").unwrap()
    }

    fn pocket_id() -> PocketId {
        PocketId::parse("6f1c5d0e-2b7a-4c1e-9a3d-0e5f8b2c4d71").unwrap()
    }

    fn pairing() -> Pairing {
        Pairing {
            device_code: "d".repeat(64),
            user_code: "FDF64KC5".into(),
            approval_url: "https://example.com/romm/pair/device?user_code=FDF64KC5".into(),
            expires_in: Duration::from_secs(600),
        }
    }

    #[test]
    fn pairing_starts_with_the_pocket_id_and_the_permissions() {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond("POST", INIT, 201, &test_init_response());
        let started = start(&mut pocket_sync, &base(), pocket_id());
        assert_eq!(started, Ok(pairing()));

        let sent = &pocket_sync.sent[0];
        assert_eq!(
            sent.headers,
            [("Content-Type", "application/json".to_string())]
        );
        assert_eq!(
            sent.json(),
            json!({
                "client_device_identifier": "6f1c5d0e-2b7a-4c1e-9a3d-0e5f8b2c4d71",
                "name": "Analogue Pocket",
                "client": "romm-sync",
                "platform": "analogue-pocket",
                "client_version": env!("CARGO_PKG_VERSION"),
                "requested_scopes": [
                    "platforms.read",
                    "roms.read",
                    "collections.read",
                    "collections.write",
                    "devices.read",
                    "devices.write",
                    "assets.read",
                    "assets.write",
                    "firmware.read",
                ],
            })
        );
    }

    #[test]
    fn pairing_may_not_start() {
        for (status, body, expected) in [
            (429, r#"{"detail": "Too many"}"#, StartError::Busy),
            (
                404,
                "Not Found",
                StartError::Request(RequestError::Unexpected(404)),
            ),
            (
                201,
                "{}",
                StartError::Request(RequestError::Unexpected(201)),
            ),
        ] {
            let mut pocket_sync = FakePocketSync::default();
            pocket_sync.respond("POST", INIT, status, body);
            assert_eq!(
                start(&mut pocket_sync, &base(), pocket_id()),
                Err(expected),
                "{status}"
            );
        }
    }

    #[test]
    fn a_debug_print_of_a_pairing_leaves_out_the_code_it_polls_with() {
        let printed = format!("{:?}", pairing());
        assert!(!printed.contains("dddd"), "{printed}");
        assert!(printed.contains("FDF64KC5"), "{printed}");
    }

    fn poll_answer(status: u16, body: &str) -> Result<Poll, RequestError> {
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond("POST", TOKEN, status, body);
        poll(&mut pocket_sync, &base(), &pairing())
    }

    #[test]
    fn polling_sends_the_device_code() {
        let mut pocket_sync = FakePocketSync::default();
        let pending = test_error_response("authorization_pending");
        pocket_sync.respond("POST", TOKEN, 400, &pending);
        poll(&mut pocket_sync, &base(), &pairing()).unwrap();
        let sent = pocket_sync.sent[0].json();
        assert_eq!(sent, json!({"device_code": "d".repeat(64)}));
    }

    #[test]
    fn polling_gets_the_sign_in_once_approved() {
        let paired = Paired {
            sign_in: test_sign_in("rmm_paired"),
            permissions: vec![Permission::RomsRead],
        };
        let body = test_token_response(&[Permission::RomsRead]);
        assert_eq!(poll_answer(200, &body), Ok(Poll::Approved(paired)));
    }

    #[test]
    fn permissions_the_plugin_didnt_ask_for_are_left_out() {
        let body = json!({
            "access_token": "rmm_paired",
            "device_id": "c1a2",
            "scopes": ["me.read", "roms.read"],
        });
        let Ok(Poll::Approved(paired)) = poll_answer(200, &body.to_string()) else {
            panic!("not approved");
        };
        assert_eq!(paired.permissions, [Permission::RomsRead]);
    }

    #[test]
    fn a_token_extism_cant_send_isnt_taken() {
        let body = json!({"access_token": "rmm_a b", "device_id": "c1a2", "scopes": []});
        let polled = poll_answer(200, &body.to_string());
        assert_eq!(polled, Err(RequestError::Unexpected(200)));
    }

    #[test]
    fn polling_says_how_pairing_is_going() {
        for (status, detail, expected) in [
            (400, "authorization_pending", Ok(Poll::Waiting)),
            (400, "slow_down", Ok(Poll::Waiting)),
            (400, "access_denied", Ok(Poll::Denied)),
            (400, "expired_token", Ok(Poll::Expired)),
            (400, "something new", Err(RequestError::Unexpected(400))),
            (429, "Too many polling attempts", Ok(Poll::TooFast)),
            (500, "oops", Err(RequestError::Unexpected(500))),
        ] {
            let body = test_error_response(detail);
            assert_eq!(poll_answer(status, &body), expected, "{detail}");
        }
        assert_eq!(poll_answer(200, "{}"), Err(RequestError::Unexpected(200)));
    }

    #[test]
    fn permissions_go_by_their_names_in_romm() {
        for permission in Permission::ALL {
            assert_eq!(Permission::named(permission.name()), Some(permission));
        }
        assert_eq!(Permission::named("me.read"), None);
    }

    #[test]
    fn every_permission_asked_for_is_granted_or_missing() {
        assert_eq!(missing(&Permission::ALL), []);
        assert_eq!(
            missing(&viewer_permissions()),
            [
                Permission::CollectionsWrite,
                Permission::DevicesWrite,
                Permission::AssetsWrite
            ]
        );
    }
}
