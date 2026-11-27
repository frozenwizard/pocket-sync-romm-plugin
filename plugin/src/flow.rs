//! The state machine: what each answer leads to, and which question comes next.
//! See Architecture.md, section 12.

use std::time::Duration;

use crate::card::{Card, PocketId, PocketIdError, ReadError};
use crate::pocket_sync::PocketSync;
use crate::romm::pairing::{self, Paired, Permission, Poll, StartError};
use crate::romm::{
    self, BaseUrl, ConnectError, DeleteDeviceError, RequestError, SignIn, SignInCheck,
};
use crate::ui::{
    self, BadServerChoice, ConnectChoice, HostMessage, Key, MenuChoice, NewDeviceChoice,
    OptionsChoice, PluginMessage, Strings, UnpairChoice,
};

/// What the plugin can't do without some permissions: the log line saying it's off, and the
/// permissions it needs (Architecture §8).
const FEATURES: [(Key, &[Permission]); 3] = [
    (
        Key::GamesOff,
        &[
            Permission::PlatformsRead,
            Permission::RomsRead,
            Permission::CollectionsRead,
        ],
    ),
    (Key::CollectionOff, &[Permission::CollectionsWrite]),
    (
        Key::SavesOff,
        &[
            Permission::AssetsRead,
            Permission::AssetsWrite,
            Permission::DevicesRead,
            Permission::DevicesWrite,
        ],
    ),
];

/// What a run remembers between calls. Later phases add the plan, pending conflicts and claimed
/// requests.
pub struct Run {
    strings: Strings,
    card: Card,
    /// The Pocket's ID, read when the run started.
    pocket_id: PocketId,
    /// The question waiting for its answer, asked again when an answer doesn't fit it.
    asked: PluginMessage,
    /// The pairing the new-device questions are about, kept while the new-device question, or the
    /// Unpair question asked from it, waits.
    new_device: Option<NewDevice>,
}

/// A pairing that gave the Pocket a new device, not yet saved (Architecture §13, rule 8).
struct NewDevice {
    /// The server that paired it, the only one its sign-in is sent to.
    host: String,
    paired: Paired,
}

/// Checks the card is a Pocket's and gives the Pocket an ID if it has none, then returns the run,
/// shown in `locale`, and its first question. There's no run when the first question is `Exit`.
pub fn start(
    locale: &str,
    card: Card,
    pocket_sync: &mut impl PocketSync,
) -> (Option<Run>, PluginMessage) {
    let strings = Strings::new(locale);
    let Some(pocket_id) = check_card(&strings, &card, pocket_sync) else {
        return (None, PluginMessage::Exit);
    };
    let mut run = Run {
        strings,
        card,
        pocket_id,
        asked: PluginMessage::Exit,
        new_device: None,
    };
    let question = server_question(&run, pocket_sync);
    run.asked = question.clone();
    let run = (question != PluginMessage::Exit).then_some(run);
    (run, question)
}

/// Acts on Pocket Sync's reply and returns the next question.
pub fn handle(
    message: HostMessage,
    run: &mut Run,
    pocket_sync: &mut impl PocketSync,
) -> PluginMessage {
    let question = match message {
        HostMessage::Kill => PluginMessage::Exit,
        HostMessage::Answer { name, value } if run.asked.name() == Some(name.as_str()) => {
            answer(run, &name, &value, pocket_sync).unwrap_or_else(|| run.asked.clone())
        }
        // An answer to a question that isn't waiting is ignored.
        HostMessage::Answer { .. } => run.asked.clone(),
    };
    // Leaving the new-device questions drops the pairing they're about.
    if !matches!(
        question.name(),
        Some(ui::NEW_DEVICE | ui::UNPAIR_NEW_DEVICE)
    ) {
        run.new_device = None;
    }
    run.asked = question.clone();
    question
}

/// Checks `card` is a Pocket's, removes leftover `.part` files, and returns the Pocket's ID, giving
/// it one if it has none. `None`, once the log says why, when the card can't be used.
fn check_card(
    strings: &Strings,
    card: &Card,
    pocket_sync: &mut impl PocketSync,
) -> Option<PocketId> {
    if !card.is_pocket_card() {
        pocket_sync.log(strings.get(Key::NotAPocketCard));
        return None;
    }
    card.remove_leftover_parts();
    match card.pocket_id() {
        Ok(pocket_id) => Some(pocket_id),
        Err(error) => {
            pocket_sync.log(strings.get(match &error {
                PocketIdError::Unreadable(_) => Key::PocketIdUnreadable,
                PocketIdError::Mismatch(_) => Key::PocketIdMismatch,
                PocketIdError::NotSaved(_) => Key::PocketIdNotSaved,
            }));
            pocket_sync.log(&error.to_string());
            None
        }
    }
}

/// The next question after `value` answered the question named `name`, or `None` when `value` isn't
/// one of its choices.
fn answer(
    run: &mut Run,
    name: &str,
    value: &str,
    pocket_sync: &mut impl PocketSync,
) -> Option<PluginMessage> {
    let strings = &run.strings;
    let question = match name {
        ui::SERVER => use_host(run, value, pocket_sync),
        ui::BAD_SERVER => match ui::chosen(strings, value)? {
            BadServerChoice::TryAgain => match saved_host(run, pocket_sync) {
                Ok(host) => ui::server(strings, host.as_deref()),
                Err(end) => end,
            },
            // Back to Options when an address is being changed; with none yet, there's nothing to
            // go back to.
            BadServerChoice::Cancel => match saved_host(run, pocket_sync) {
                Ok(Some(_)) => ui::options(strings),
                Ok(None) => PluginMessage::Exit,
                Err(end) => end,
            },
        },
        ui::CONNECT => match ui::chosen(strings, value)? {
            ConnectChoice::Connect => match saved_host(run, pocket_sync) {
                Ok(Some(host)) => connect(run, &host, pocket_sync),
                Ok(None) => ui::server(strings, None),
                Err(end) => end,
            },
            ConnectChoice::Options => ui::options(strings),
            ConnectChoice::Exit => PluginMessage::Exit,
        },
        ui::OPTIONS => match ui::chosen(strings, value)? {
            OptionsChoice::ChangeServer => match saved_host(run, pocket_sync) {
                Ok(host) => ui::server(strings, host.as_deref()),
                Err(end) => end,
            },
            OptionsChoice::Unpair => match saved_sign_in(run, pocket_sync) {
                Ok(Some(_)) => ui::unpair(strings),
                Ok(None) => not_paired(strings, pocket_sync),
                Err(end) => end,
            },
            OptionsChoice::Back => server_question(run, pocket_sync),
        },
        ui::UNPAIR => match ui::chosen(strings, value)? {
            UnpairChoice::Unpair => unpair(run, pocket_sync),
            UnpairChoice::Cancel => ui::options(strings),
        },
        ui::NEW_DEVICE => match ui::chosen(strings, value)? {
            NewDeviceChoice::PairAsNewDevice => {
                save_pairing(run, &held_pairing(run).paired, pocket_sync)
            }
            NewDeviceChoice::Unpair => ui::unpair_new_device(strings),
            NewDeviceChoice::Cancel => {
                pocket_sync.log(strings.get(Key::NewDeviceCancelled));
                server_question(run, pocket_sync)
            }
        },
        ui::UNPAIR_NEW_DEVICE => {
            let choice = ui::chosen(strings, value)?;
            let held = held_pairing(run);
            match choice {
                UnpairChoice::Unpair => {
                    unpair_with(run, &held.host, &held.paired.sign_in, pocket_sync)
                }
                UnpairChoice::Cancel => ui::new_device(strings, &held.host),
            }
        }
        ui::MENU => match ui::chosen(strings, value)? {
            MenuChoice::SyncNow => {
                pocket_sync.log(strings.get(Key::SyncNotReady));
                ui::menu(strings)
            }
            MenuChoice::Settings => {
                pocket_sync.log(strings.get(Key::SettingsNotReady));
                ui::menu(strings)
            }
            MenuChoice::Exit => PluginMessage::Exit,
        },
        _ => return None,
    };
    Some(question)
}

/// Offers to connect to the saved server, or asks for its address when there isn't one yet.
fn server_question(run: &Run, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    match saved_host(run, pocket_sync) {
        Ok(Some(host)) => ui::connect(&run.strings, &host),
        Ok(None) => ui::server(&run.strings, None),
        Err(end) => end,
    }
}

/// The server's address in `romm-sync/server.json`, read each time so edits made while the plugin
/// waits are used. `None` when there's no file or no address in it. A file that can't be read is
/// logged and ends the run (the error is `Exit`): asking for the address instead would lead to
/// replacing a file that may hold the sign-in.
fn saved_host(
    run: &Run,
    pocket_sync: &mut impl PocketSync,
) -> Result<Option<String>, PluginMessage> {
    run.card
        .server_host()
        .map_err(|error| server_file_unreadable(&run.strings, error, pocket_sync))
}

/// The sign-in in `romm-sync/server.json`, read each time like the address. A file that can't be
/// read is logged and ends the run (the error is `Exit`).
fn saved_sign_in(
    run: &Run,
    pocket_sync: &mut impl PocketSync,
) -> Result<Option<SignIn>, PluginMessage> {
    run.card
        .sign_in()
        .map_err(|error| server_file_unreadable(&run.strings, error, pocket_sync))
}

/// RomM's ID for the Pocket's device in `romm-sync/server.json`, read each time like the address,
/// with or without a sign-in. A file that can't be read is logged and ends the run (the error is
/// `Exit`).
fn saved_device_id(
    run: &Run,
    pocket_sync: &mut impl PocketSync,
) -> Result<Option<String>, PluginMessage> {
    run.card
        .romm_device_id()
        .map_err(|error| server_file_unreadable(&run.strings, error, pocket_sync))
}

/// Says `server.json` can't be read and why, and ends the run.
fn server_file_unreadable(
    strings: &Strings,
    error: ReadError,
    pocket_sync: &mut impl PocketSync,
) -> PluginMessage {
    end_run(
        strings,
        Key::ServerFileUnreadable,
        &error.to_string(),
        pocket_sync,
    )
}

/// Logs `key`'s line, then `reason`, the error behind it, and ends the run.
fn end_run(
    strings: &Strings,
    key: Key,
    reason: &str,
    pocket_sync: &mut impl PocketSync,
) -> PluginMessage {
    pocket_sync.log(strings.get(key));
    pocket_sync.log(reason);
    PluginMessage::Exit
}

/// Saves the address the user typed and connects to it. An answer that isn't an address, an empty
/// one included, offers Try again and Cancel.
fn use_host(run: &mut Run, typed: &str, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    let strings = &run.strings;
    let host = typed.trim();
    if romm::base_url(host).is_none() {
        return ui::bad_server(strings);
    }
    if let Err(error) = run.card.save_server_host(host) {
        return end_run(strings, Key::ServerFileNotSaved, &error, pocket_sync);
    }
    connect(run, host, pocket_sync)
}

/// Checks the server at `host` is RomM 5.4 or newer and signs in, pairing the Pocket if needed,
/// then shows the menu, or asks first when pairing gave the Pocket a new device. Otherwise says why
/// and offers to connect again.
fn connect(run: &mut Run, host: &str, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    match check_server(&run.strings, host, pocket_sync) {
        Some(base) => sign_in(run, host, &base, pocket_sync),
        None => ui::connect(&run.strings, host),
    }
}

/// Checks the server at `host` is RomM 5.4 or newer, and returns the URL its paths join onto.
/// `None` once the log says why it can't be used.
fn check_server(
    strings: &Strings,
    host: &str,
    pocket_sync: &mut impl PocketSync,
) -> Option<BaseUrl> {
    // Logged first: when the server can't be reached, Extism ends the call before anything else is.
    pocket_sync.log(&strings.fill(Key::Connecting, &[("host", host)]));
    let Some(base) = romm::base_url(host) else {
        pocket_sync.log(&strings.fill(Key::BadSavedHost, &[("host", host)]));
        return None;
    };
    match romm::connect(pocket_sync, &base) {
        Ok(version) => {
            pocket_sync.log(&strings.fill(Key::Connected, &[("version", &version)]));
            Some(base)
        }
        Err(error) => {
            pocket_sync.log(&match error {
                ConnectError::Request(error) => request_error(strings, host, error),
                ConnectError::NotRomm(status) => strings.fill(
                    Key::NotRomm,
                    &[("host", host), ("status", &status.to_string())],
                ),
                ConnectError::TooOld(version) => {
                    strings.fill(Key::RommTooOld, &[("host", host), ("version", &version)])
                }
            });
            None
        }
    }
}

/// Checks RomM still accepts the sign-in in `server.json`, pairing the Pocket when there's none or
/// RomM rejects it, then shows the menu, or asks first when pairing gave the Pocket a new device. The sign-in is kept when the address changes, so a server
/// that moved needs no pairing, and a different server rejects it.
fn sign_in(
    run: &mut Run,
    host: &str,
    base: &BaseUrl,
    pocket_sync: &mut impl PocketSync,
) -> PluginMessage {
    let strings = &run.strings;
    let saved = match saved_sign_in(run, pocket_sync) {
        Ok(saved) => saved,
        Err(end) => return end,
    };
    if let Some(saved) = saved {
        match romm::check_sign_in(pocket_sync, base, &saved) {
            Ok(SignInCheck::Accepted) => {
                pocket_sync.log(strings.get(Key::SignedIn));
                return ui::menu(strings);
            }
            Ok(SignInCheck::Rejected) => pocket_sync.log(strings.get(Key::SignInRejected)),
            Err(error) => return request_failed(strings, host, error, pocket_sync),
        }
    }
    pair(run, host, base, pocket_sync)
}

/// Pairs the Pocket with RomM: opens RomM's approval page and polls until the user approves or
/// denies it, or the code expires. Pocket Sync's Stop is the only way out while it polls. Saves
/// the sign-in and shows the menu, unless pairing gave the Pocket a different device from the one
/// in `server.json`: then it asks first, and saves nothing until the answer.
fn pair(
    run: &mut Run,
    host: &str,
    base: &BaseUrl,
    pocket_sync: &mut impl PocketSync,
) -> PluginMessage {
    let strings = &run.strings;
    let pending = match pairing::start(pocket_sync, base, run.pocket_id) {
        Ok(pending) => pending,
        Err(StartError::Busy) => {
            pocket_sync.log(strings.get(Key::PairingBusy));
            return ui::connect(strings, host);
        }
        Err(StartError::Request(error)) => {
            return request_failed(strings, host, error, pocket_sync);
        }
    };
    let code = ("code", pending.user_code.as_str());
    // Logged before the page is opened: when the browser can't open it, Pocket Sync ends the call.
    pocket_sync.log(&strings.fill(Key::Pairing, &[code]));
    let url = ("url", pending.approval_url.as_str());
    pocket_sync.log(&strings.fill(Key::PairingLink, &[url]));
    pocket_sync.open_url(&pending.approval_url);

    let mut interval = pairing::POLL_INTERVAL;
    let mut waited = Duration::ZERO;
    let expired = || strings.fill(Key::PairingExpired, &[code]);
    // Ends with the sign-in, or with the log line saying why there's none.
    let outcome = loop {
        pocket_sync.wait(interval);
        waited += interval;
        match pairing::poll(pocket_sync, base, &pending) {
            Ok(Poll::Approved(paired)) => break Ok(paired),
            Ok(Poll::Waiting) => {}
            Ok(Poll::TooFast) => interval += pairing::BACKOFF,
            Ok(Poll::Denied) => break Err(strings.get(Key::PairingDenied).into()),
            Ok(Poll::Expired) => break Err(expired()),
            Err(error) => break Err(request_error(strings, host, error)),
        }
        // RomM stops waiting then too, so this only matters if it never says so.
        if waited >= pending.expires_in {
            break Err(expired());
        }
    };
    let paired = match outcome {
        Ok(paired) => paired,
        Err(reason) => {
            pocket_sync.log(&reason);
            return ui::connect(strings, host);
        }
    };

    // Read before saving, which replaces it.
    let saved = match saved_device_id(run, pocket_sync) {
        Ok(saved) => saved,
        Err(end) => return end,
    };
    if saved.is_some_and(|device_id| device_id != paired.sign_in.device_id()) {
        run.new_device = Some(NewDevice {
            host: host.into(),
            paired,
        });
        return ui::new_device(strings, host);
    }
    save_pairing(run, &paired, pocket_sync)
}

/// The pairing the new-device questions are about, which `handle` keeps while they wait.
fn held_pairing(run: &Run) -> &NewDevice {
    run.new_device
        .as_ref()
        .expect("held while the new-device questions wait")
}

/// Saves the sign-in pairing gave, says which features the permissions granted leave off, and
/// shows the menu.
fn save_pairing(run: &Run, paired: &Paired, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    let strings = &run.strings;
    if let Err(error) = run.card.save_sign_in(&paired.sign_in) {
        return end_run(strings, Key::SignInNotSaved, &error, pocket_sync);
    }
    pocket_sync.log(strings.get(Key::Paired));
    log_missing_permissions(strings, &paired.permissions, pocket_sync);
    ui::menu(strings)
}

/// Unpairs the Pocket with the address and sign-in in `server.json`, read again since the file may
/// have changed while the plugin waited for the answer.
fn unpair(run: &Run, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    let host = match saved_host(run, pocket_sync) {
        Ok(host) => host,
        Err(end) => return end,
    };
    let sign_in = match saved_sign_in(run, pocket_sync) {
        Ok(sign_in) => sign_in,
        Err(end) => return end,
    };
    match (host, sign_in) {
        (Some(host), Some(sign_in)) => unpair_with(run, &host, &sign_in, pocket_sync),
        _ => not_paired(&run.strings, pocket_sync),
    }
}

/// Unpairs the Pocket with `sign_in`: checks the server at `host`, deletes the Pocket's device in
/// RomM, then the RomM address and sign-in on the Pocket, and asks for a server's address. The
/// Pocket keeps its ID. When RomM doesn't delete the device, says why, changes nothing on the
/// Pocket, and goes back to Options. Takes the sign-in rather than reading it, so one pairing just
/// returned, not yet saved, works too.
fn unpair_with(
    run: &Run,
    host: &str,
    sign_in: &SignIn,
    pocket_sync: &mut impl PocketSync,
) -> PluginMessage {
    let strings = &run.strings;
    let Some(base) = check_server(strings, host, pocket_sync) else {
        return ui::options(strings);
    };
    pocket_sync.log(strings.get(Key::Unpairing));
    if let Err(error) = romm::delete_device(pocket_sync, &base, sign_in) {
        let needs = ("permissions", Permission::DevicesWrite.name());
        pocket_sync.log(&match error {
            DeleteDeviceError::Rejected => strings.get(Key::UnpairRejected).into(),
            DeleteDeviceError::NotAllowed => strings.fill(Key::UnpairNotAllowed, &[needs]),
            DeleteDeviceError::NoDevice => strings.get(Key::UnpairNoDevice).into(),
            DeleteDeviceError::Request(error) => request_error(strings, host, error),
        });
        return ui::options(strings);
    }
    if let Err(error) = run.card.delete_server_file() {
        return end_run(strings, Key::ServerFileNotDeleted, &error, pocket_sync);
    }
    pocket_sync.log(strings.get(Key::Unpaired));
    ui::server(strings, None)
}

/// Says the Pocket isn't paired, so there's nothing to unpair, and goes back to Options.
fn not_paired(strings: &Strings, pocket_sync: &mut impl PocketSync) -> PluginMessage {
    pocket_sync.log(strings.get(Key::NotPaired));
    ui::options(strings)
}

/// Says which permissions pairing didn't grant, and which features are off without them.
fn log_missing_permissions(
    strings: &Strings,
    granted: &[Permission],
    pocket_sync: &mut impl PocketSync,
) {
    let missing = pairing::missing(granted);
    if missing.is_empty() {
        return;
    }
    let listed = names(&missing);
    pocket_sync.log(&strings.fill(Key::PermissionsMissing, &[("permissions", &listed)]));
    for (off, needs) in FEATURES {
        if needs.iter().any(|needed| missing.contains(needed)) {
            pocket_sync.log(&strings.fill(off, &[("permissions", &names(needs))]));
        }
    }
}

/// The names of `permissions` in RomM's API, as a list.
fn names(permissions: &[Permission]) -> String {
    let names: Vec<_> = permissions
        .iter()
        .map(|permission| permission.name())
        .collect();
    names.join(", ")
}

/// Says why a request to RomM at `host` failed, and offers to connect again.
fn request_failed(
    strings: &Strings,
    host: &str,
    error: RequestError,
    pocket_sync: &mut impl PocketSync,
) -> PluginMessage {
    pocket_sync.log(&request_error(strings, host, error));
    ui::connect(strings, host)
}

/// The log line saying why a request to RomM at `host` failed.
fn request_error(strings: &Strings, host: &str, error: RequestError) -> String {
    match error {
        RequestError::NoResponse(error) => {
            strings.fill(Key::NoResponse, &[("host", host), ("error", &error)])
        }
        RequestError::Unexpected(status) => strings.fill(
            Key::UnexpectedAnswer,
            &[("host", host), ("status", &status.to_string())],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card;
    use crate::pocket_sync::FakePocketSync;
    use crate::romm::pairing::{
        BACKOFF, POLL_INTERVAL, test_init_response, test_token_response, viewer_permissions,
    };
    use crate::romm::{test_error_response, test_sign_in};
    use std::fs;
    use tempfile::TempDir;

    const HOST: &str = "https://romm.example.com";
    const HEARTBEAT: &str = "https://romm.example.com/api/heartbeat";
    const DEVICE: &str = "https://romm.example.com/api/devices/c1a2";
    const OLD_DEVICE: &str = "https://romm.example.com/api/devices/b7e9";
    const INIT: &str = "https://romm.example.com/api/auth/device/init";
    const TOKEN: &str = "https://romm.example.com/api/auth/device/token";
    const APPROVAL: &str = "https://romm.example.com/pair/device?user_code=FDF64KC5";
    const CODE: (&str, &str) = ("code", "FDF64KC5");

    fn english() -> Strings {
        Strings::new("en-US")
    }

    fn heartbeat(version: &str) -> String {
        format!(r#"{{"SYSTEM": {{"VERSION": "{version}"}}}}"#)
    }

    /// A Pocket card whose `server.json` holds `HOST`.
    fn card_with_server() -> TempDir {
        let root = card::test_card();
        Card::new(root.path()).save_server_host(HOST).unwrap();
        root
    }

    /// The sign-in `signed_in_card` saves.
    fn card_sign_in() -> SignIn {
        test_sign_in("rmm_saved")
    }

    /// A Pocket card whose `server.json` holds `HOST` and a sign-in.
    fn signed_in_card() -> TempDir {
        let root = card_with_server();
        Card::new(root.path())
            .save_sign_in(&card_sign_in())
            .unwrap();
        root
    }

    /// RomM 5.4.0 at `host`. The test adds its other answers.
    fn romm(host: &str) -> FakePocketSync {
        let mut pocket_sync = FakePocketSync::default();
        let heartbeat_url = format!("{host}/api/heartbeat");
        pocket_sync.respond("GET", &heartbeat_url, 200, &heartbeat("5.4.0"));
        pocket_sync
    }

    /// RomM 5.4.0 at `host`, accepting the saved sign-in.
    fn romm_at(host: &str) -> FakePocketSync {
        let mut pocket_sync = romm(host);
        let device = format!("{host}/api/devices/c1a2");
        pocket_sync.respond("GET", &device, 200, r#"{"id": "c1a2"}"#);
        pocket_sync
    }

    /// RomM 5.4.0 at `HOST`, ready to start pairing. The test queues its answers to polls.
    fn romm_to_pair() -> FakePocketSync {
        let mut pocket_sync = romm(HOST);
        pocket_sync.respond("POST", INIT, 201, &test_init_response());
        pocket_sync
    }

    /// Queues RomM's answers to the next two polls: still waiting, then approved with
    /// `permissions`.
    fn approve(pocket_sync: &mut FakePocketSync, permissions: &[Permission]) {
        let pending = test_error_response("authorization_pending");
        pocket_sync.respond("POST", TOKEN, 400, &pending);
        pocket_sync.respond("POST", TOKEN, 200, &test_token_response(permissions));
    }

    /// The sign-in `test_token_response` hands over.
    fn paired_sign_in() -> SignIn {
        test_sign_in("rmm_paired")
    }

    /// A sign-in for a device RomM no longer has.
    fn old_sign_in() -> SignIn {
        SignIn::new("rmm_old".into(), "b7e9".into()).unwrap()
    }

    /// A Pocket card whose `server.json` holds `HOST` and `old_sign_in`.
    fn card_with_a_deleted_device() -> TempDir {
        let root = card_with_server();
        Card::new(root.path()).save_sign_in(&old_sign_in()).unwrap();
        root
    }

    /// RomM 5.4.0 at `HOST`, rejecting `old_sign_in` and pairing the Pocket as a new device.
    fn romm_with_a_new_device() -> FakePocketSync {
        let mut pocket_sync = romm_to_pair();
        let rejected = test_error_response("Not authenticated");
        pocket_sync.respond("GET", OLD_DEVICE, 401, &rejected);
        approve(&mut pocket_sync, &Permission::ALL);
        pocket_sync
    }

    /// Checks no line holds a token or the code pairing polls with.
    fn assert_no_secrets(lines: &[String]) {
        for line in lines {
            assert!(!line.contains("rmm_"), "{line}");
            assert!(!line.contains("dddd"), "{line}");
        }
    }

    /// Starts a run on the card at `root`, then gives each answer in turn to the question waiting.
    /// Returns the last question.
    fn play(root: &TempDir, pocket_sync: &mut FakePocketSync, answers: &[&str]) -> PluginMessage {
        let (mut run, mut question) = start("en-US", Card::new(root.path()), pocket_sync);
        for value in answers {
            let run = run.as_mut().expect("a run is going");
            let name = question.name().expect("a question is waiting").to_string();
            let answer = HostMessage::Answer {
                name,
                value: value.to_string(),
            };
            question = handle(answer, run, pocket_sync);
        }
        question
    }

    /// A run started on the card at `root`, waiting for its first answer.
    fn started(root: &TempDir, pocket_sync: &mut FakePocketSync) -> Run {
        let (run, _) = start("en-US", Card::new(root.path()), pocket_sync);
        run.expect("a run is going")
    }

    #[test]
    fn a_new_pocket_gets_an_id_and_is_asked_for_the_server() {
        let root = card::test_card();
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&root, &mut pocket_sync, &[]);
        assert_eq!(question, ui::server(&english(), None));
        // Checked before anything that would write it.
        assert!(root.path().join("romm-sync/device.json").is_file());
        assert_eq!(pocket_sync.lines, Vec::<String>::new());
    }

    #[test]
    fn a_run_starts_by_removing_leftover_part_files() {
        let root = card_with_server();
        let part = root.path().join("romm-sync/server.json.part");
        fs::write(&part, "{").unwrap();
        play(&root, &mut FakePocketSync::default(), &[]);
        assert!(!part.exists());
    }

    #[test]
    fn a_pocket_with_a_server_is_offered_connect_and_options() {
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&card_with_server(), &mut pocket_sync, &[]);
        assert_eq!(question, ui::connect(&english(), HOST));
    }

    #[test]
    fn a_card_that_isnt_a_pockets_ends_the_run() {
        let root = tempfile::tempdir().unwrap();
        let mut pocket_sync = FakePocketSync::default();
        assert_eq!(play(&root, &mut pocket_sync, &[]), PluginMessage::Exit);
        assert_eq!(pocket_sync.lines, [english().get(Key::NotAPocketCard)]);
        assert!(!root.path().join("romm-sync").exists());
    }

    #[test]
    fn an_unreadable_pocket_id_ends_the_run() {
        let root = card::test_card();
        fs::create_dir(root.path().join("romm-sync")).unwrap();
        fs::write(root.path().join("romm-sync/device.json"), "{").unwrap();
        let mut pocket_sync = FakePocketSync::default();
        assert_eq!(play(&root, &mut pocket_sync, &[]), PluginMessage::Exit);
        assert_eq!(pocket_sync.lines[0], english().get(Key::PocketIdMismatch));
        assert_eq!(pocket_sync.lines.len(), 2, "{:?}", pocket_sync.lines);
    }

    #[test]
    fn an_unreadable_server_json_ends_the_run_and_is_kept() {
        let root = card::test_card();
        fs::create_dir(root.path().join("romm-sync")).unwrap();
        let server_file = root.path().join("romm-sync/server.json");
        fs::write(&server_file, "{").unwrap();
        let mut pocket_sync = FakePocketSync::default();
        assert_eq!(play(&root, &mut pocket_sync, &[]), PluginMessage::Exit);
        assert_eq!(
            pocket_sync.lines,
            [
                english().get(Key::ServerFileUnreadable),
                "the file ends early, at line 1, column 1"
            ]
        );
        assert_eq!(fs::read_to_string(&server_file).unwrap(), "{");
    }

    #[test]
    fn server_json_broken_while_the_plugin_waits_ends_the_run() {
        let root = card_with_server();
        let mut pocket_sync = FakePocketSync::default();
        let mut run = started(&root, &mut pocket_sync);
        fs::write(root.path().join("romm-sync/server.json"), "{").unwrap();
        let connect = HostMessage::Answer {
            name: ui::CONNECT.into(),
            value: "Connect".into(),
        };
        assert_eq!(
            handle(connect, &mut run, &mut pocket_sync),
            PluginMessage::Exit
        );
        assert_eq!(
            pocket_sync.lines[0],
            english().get(Key::ServerFileUnreadable)
        );
    }

    #[test]
    fn a_saved_address_that_cant_be_used_is_logged() {
        let root = card::test_card();
        fs::create_dir(root.path().join("romm-sync")).unwrap();
        let server_file = root.path().join("romm-sync/server.json");
        fs::write(&server_file, r#"{"host": "romm.example.com"}"#).unwrap();
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&root, &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::connect(&english(), "romm.example.com"));
        let bad = english().fill(Key::BadSavedHost, &[("host", "romm.example.com")]);
        assert_eq!(pocket_sync.lines[1], bad);
    }

    #[test]
    fn a_typed_address_is_saved_and_connected_to() {
        let root = card::test_card();
        let mut pocket_sync = romm_to_pair();
        approve(&mut pocket_sync, &Permission::ALL);
        let question = play(&root, &mut pocket_sync, &[" https://romm.example.com "]);
        assert_eq!(question, ui::menu(&english()));
        assert_eq!(Card::new(root.path()).server_host(), Ok(Some(HOST.into())));
        assert_eq!(
            pocket_sync.lines[..2],
            [
                "Connecting to https://romm.example.com…",
                "Connected to RomM 5.4.0."
            ]
        );
    }

    #[test]
    fn an_answer_that_isnt_an_address_offers_try_again() {
        let root = card::test_card();
        let mut pocket_sync = FakePocketSync::default();
        for typed in ["romm.example.com", "", "https://:8080"] {
            let question = play(&root, &mut pocket_sync, &[typed]);
            assert_eq!(question, ui::bad_server(&english()), "{typed:?}");
        }
        assert_eq!(Card::new(root.path()).server_host(), Ok(None));
        let question = play(&root, &mut pocket_sync, &["romm.example.com", "Try again"]);
        assert_eq!(question, ui::server(&english(), None));
    }

    #[test]
    fn cancel_on_first_setup_ends_the_run() {
        let root = card::test_card();
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&root, &mut pocket_sync, &["", "Cancel"]);
        assert_eq!(question, PluginMessage::Exit);
    }

    #[test]
    fn cancel_keeps_the_address_when_changing_it() {
        let root = card_with_server();
        let mut pocket_sync = FakePocketSync::default();
        let change = ["Options", "Change server address", "", "Cancel"];
        assert_eq!(
            play(&root, &mut pocket_sync, &change),
            ui::options(&english())
        );
        assert_eq!(Card::new(root.path()).server_host(), Ok(Some(HOST.into())));
        let again = ["Options", "Change server address", "", "Try again"];
        assert_eq!(
            play(&root, &mut pocket_sync, &again),
            ui::server(&english(), Some(HOST))
        );
    }

    #[test]
    fn connect_checks_the_server_and_the_saved_sign_in() {
        let mut pocket_sync = romm_at(HOST);
        let question = play(&signed_in_card(), &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::menu(&english()));
        assert_eq!(
            pocket_sync.lines,
            [
                "Connecting to https://romm.example.com…",
                "Connected to RomM 5.4.0.",
                "Signed in."
            ]
        );
        assert_eq!(
            pocket_sync.sent[1].headers,
            [("Authorization", "Bearer rmm_saved".to_string())]
        );
    }

    #[test]
    fn the_sign_in_check_accepts_a_403_and_fails_on_other_answers() {
        let unexpected =
            english().fill(Key::UnexpectedAnswer, &[("host", HOST), ("status", "500")]);
        for (status, question, line) in [
            (
                403,
                ui::menu(&english()),
                english().get(Key::SignedIn).to_string(),
            ),
            (500, ui::connect(&english(), HOST), unexpected),
        ] {
            let mut pocket_sync = romm(HOST);
            pocket_sync.respond("GET", DEVICE, status, &test_error_response("Forbidden"));
            assert_eq!(
                play(&signed_in_card(), &mut pocket_sync, &["Connect"]),
                question
            );
            assert_eq!(pocket_sync.lines.last(), Some(&line));
        }
    }

    #[test]
    fn a_pocket_without_a_sign_in_pairs_on_connect() {
        let root = card_with_server();
        let mut pocket_sync = romm_to_pair();
        approve(&mut pocket_sync, &Permission::ALL);
        let question = play(&root, &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::menu(&english()));

        let card = Card::new(root.path());
        let init = pocket_sync.sent[1].json();
        let pocket_id = card.pocket_id().unwrap().to_string();
        assert_eq!(init["client_device_identifier"], pocket_id.as_str());
        assert_eq!(pocket_sync.opened, [APPROVAL]);
        assert_eq!(pocket_sync.waited, POLL_INTERVAL * 2);
        assert_eq!(card.sign_in(), Ok(Some(paired_sign_in())));
        assert_eq!(card.server_host(), Ok(Some(HOST.into())));
        assert_eq!(
            pocket_sync.lines[2..],
            [
                english().fill(Key::Pairing, &[CODE]),
                english().fill(Key::PairingLink, &[("url", APPROVAL)]),
                english().get(Key::Paired).to_string()
            ]
        );
        assert_no_secrets(&pocket_sync.lines);
    }

    #[test]
    fn a_sign_in_romm_rejects_is_replaced_by_pairing_again() {
        let root = signed_in_card();
        let mut pocket_sync = romm_to_pair();
        pocket_sync.respond(
            "GET",
            DEVICE,
            401,
            &test_error_response("Not authenticated"),
        );
        approve(&mut pocket_sync, &Permission::ALL);
        // RomM gives back the same device, so there's nothing to ask.
        let question = play(&root, &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::menu(&english()));
        assert_eq!(pocket_sync.lines[2], english().get(Key::SignInRejected));
        assert_eq!(Card::new(root.path()).sign_in(), Ok(Some(paired_sign_in())));
        assert_no_secrets(&pocket_sync.lines);
    }

    #[test]
    fn a_new_device_is_asked_about_before_anything_is_saved() {
        let root = card_with_a_deleted_device();
        let mut pocket_sync = romm_with_a_new_device();
        let question = play(&root, &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::new_device(&english(), HOST));
        assert_eq!(Card::new(root.path()).sign_in(), Ok(Some(old_sign_in())));
        assert_eq!(
            pocket_sync.lines[2..],
            [
                english().get(Key::SignInRejected).to_string(),
                english().fill(Key::Pairing, &[CODE]),
                english().fill(Key::PairingLink, &[("url", APPROVAL)]),
            ]
        );
        assert_no_secrets(&pocket_sync.lines);
    }

    #[test]
    fn a_device_id_without_a_sign_in_is_compared_too() {
        let root = card_with_server();
        let server_file = root.path().join("romm-sync/server.json");
        let file = format!(r#"{{"host": "{HOST}", "romm_device_id": "b7e9"}}"#);
        fs::write(&server_file, file).unwrap();
        let mut pocket_sync = romm_to_pair();
        approve(&mut pocket_sync, &Permission::ALL);
        let question = play(&root, &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::new_device(&english(), HOST));
        let card = Card::new(root.path());
        assert_eq!(card.romm_device_id(), Ok(Some("b7e9".into())));
    }

    #[test]
    fn pair_as_new_device_saves_the_new_sign_in() {
        let root = card_with_a_deleted_device();
        let mut pocket_sync = romm_with_a_new_device();
        let mut run = started(&root, &mut pocket_sync);
        let connect = HostMessage::Answer {
            name: ui::CONNECT.into(),
            value: "Connect".into(),
        };
        assert_eq!(
            handle(connect, &mut run, &mut pocket_sync),
            ui::new_device(&english(), HOST)
        );
        let pair = HostMessage::Answer {
            name: ui::NEW_DEVICE.into(),
            value: "Pair as new device".into(),
        };
        assert_eq!(
            handle(pair, &mut run, &mut pocket_sync),
            ui::menu(&english())
        );
        assert_eq!(Card::new(root.path()).sign_in(), Ok(Some(paired_sign_in())));
        let paired = english().get(Key::Paired).to_string();
        assert_eq!(pocket_sync.lines.last(), Some(&paired));
    }

    #[test]
    fn unpair_from_the_new_device_question_deletes_the_new_device() {
        let root = card_with_a_deleted_device();
        let mut pocket_sync = romm_with_a_new_device();
        pocket_sync.respond("DELETE", DEVICE, 204, "");
        let question = play(&root, &mut pocket_sync, &["Connect", "Unpair", "Unpair"]);
        assert_eq!(question, ui::server(&english(), None));
        assert!(!root.path().join("romm-sync/server.json").exists());
        let deleted = pocket_sync.sent.last().unwrap();
        assert_eq!((deleted.method, deleted.url.as_str()), ("DELETE", DEVICE));
        assert_eq!(
            deleted.headers,
            [("Authorization", "Bearer rmm_paired".to_string())]
        );
        let unpaired = english().get(Key::Unpaired).to_string();
        assert_eq!(pocket_sync.lines.last(), Some(&unpaired));
        assert_no_secrets(&pocket_sync.lines);
    }

    #[test]
    fn unpair_from_the_new_device_question_asks_first() {
        let root = card_with_a_deleted_device();
        let mut pocket_sync = romm_with_a_new_device();
        let asked = play(&root, &mut pocket_sync, &["Connect", "Unpair"]);
        assert_eq!(asked, ui::unpair_new_device(&english()));
        let mut pocket_sync = romm_with_a_new_device();
        let question = play(&root, &mut pocket_sync, &["Connect", "Unpair", "Cancel"]);
        assert_eq!(question, ui::new_device(&english(), HOST));
        assert!(pocket_sync.sent.iter().all(|sent| sent.method != "DELETE"));
    }

    #[test]
    fn cancel_on_the_new_device_question_saves_nothing() {
        let root = card_with_a_deleted_device();
        let mut pocket_sync = romm_with_a_new_device();
        let question = play(&root, &mut pocket_sync, &["Connect", "Cancel"]);
        assert_eq!(question, ui::connect(&english(), HOST));
        let cancelled = english().get(Key::NewDeviceCancelled).to_string();
        assert_eq!(pocket_sync.lines.last(), Some(&cancelled));
        assert_eq!(Card::new(root.path()).sign_in(), Ok(Some(old_sign_in())));
    }

    #[test]
    fn when_romm_doesnt_delete_the_new_device_nothing_changes_on_the_pocket() {
        let root = card_with_a_deleted_device();
        let mut pocket_sync = romm_with_a_new_device();
        let forbidden = test_error_response("Forbidden");
        pocket_sync.respond("DELETE", DEVICE, 403, &forbidden);
        let question = play(&root, &mut pocket_sync, &["Connect", "Unpair", "Unpair"]);
        assert_eq!(question, ui::options(&english()));
        let needs = ("permissions", "devices.write");
        let not_allowed = english().fill(Key::UnpairNotAllowed, &[needs]);
        assert_eq!(pocket_sync.lines.last(), Some(&not_allowed));
        let card = Card::new(root.path());
        assert_eq!(card.sign_in(), Ok(Some(old_sign_in())));
        assert_eq!(card.server_host(), Ok(Some(HOST.into())));
    }

    #[test]
    fn the_new_devices_sign_in_is_dropped_once_its_questions_are_left() {
        let root = card_with_a_deleted_device();
        let mut pocket_sync = romm_with_a_new_device();
        let mut run = started(&root, &mut pocket_sync);
        for (name, value, held) in [
            (ui::CONNECT, "Connect", true),
            (ui::NEW_DEVICE, "Unpair", true),
            (ui::UNPAIR_NEW_DEVICE, "Cancel", true),
            (ui::NEW_DEVICE, "Cancel", false),
        ] {
            let answer = HostMessage::Answer {
                name: name.into(),
                value: value.into(),
            };
            handle(answer, &mut run, &mut pocket_sync);
            assert_eq!(run.new_device.is_some(), held, "{value} to {name}");
        }
    }

    #[test]
    fn pairing_that_isnt_approved_offers_connect_again() {
        for (answer, reason) in [
            (
                "access_denied",
                english().get(Key::PairingDenied).to_string(),
            ),
            (
                "expired_token",
                english().fill(Key::PairingExpired, &[CODE]),
            ),
        ] {
            let root = card_with_server();
            let mut pocket_sync = romm_to_pair();
            pocket_sync.respond("POST", TOKEN, 400, &test_error_response(answer));
            let question = play(&root, &mut pocket_sync, &["Connect"]);
            assert_eq!(question, ui::connect(&english(), HOST));
            assert_eq!(pocket_sync.lines.last(), Some(&reason));
            assert_eq!(Card::new(root.path()).sign_in(), Ok(None));
        }
    }

    #[test]
    fn pairing_stops_waiting_when_the_code_expires() {
        let mut pocket_sync = romm_to_pair();
        pocket_sync.respond("POST", TOKEN, 400, &test_error_response("slow_down"));
        let question = play(&card_with_server(), &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::connect(&english(), HOST));

        let expires_in = Duration::from_secs(600);
        let waited = pocket_sync.waited;
        assert!(
            waited >= expires_in && waited < expires_in + POLL_INTERVAL,
            "{waited:?}"
        );
        let expired = english().fill(Key::PairingExpired, &[CODE]);
        assert_eq!(pocket_sync.lines.last(), Some(&expired));
    }

    #[test]
    fn pairing_waits_longer_when_romm_has_had_too_many_polls() {
        let mut pocket_sync = romm_to_pair();
        let too_many = test_error_response("Too many polling attempts.");
        pocket_sync.respond("POST", TOKEN, 429, &too_many);
        approve(&mut pocket_sync, &Permission::ALL);
        let question = play(&card_with_server(), &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::menu(&english()));
        // The 429, then two polls each waiting longer.
        assert_eq!(pocket_sync.waited, POLL_INTERVAL * 3 + BACKOFF * 2);
    }

    #[test]
    fn missing_permissions_are_logged_with_the_features_they_turn_off() {
        let mut pocket_sync = romm_to_pair();
        approve(&mut pocket_sync, &viewer_permissions());
        let question = play(&card_with_server(), &mut pocket_sync, &["Connect"]);
        assert_eq!(question, ui::menu(&english()));
        assert_eq!(
            pocket_sync.lines[5..],
            [
                "RomM didn't allow everything RomM Sync asked for. Missing: collections.write, devices.write, assets.write.",
                "Creating the Analogue Pocket collection is off: it needs collections.write.",
                "Syncing saves is off: it needs assets.read, assets.write, devices.read, devices.write.",
            ]
        );
    }

    #[test]
    fn pairing_that_cant_start_offers_connect_again() {
        let unexpected =
            english().fill(Key::UnexpectedAnswer, &[("host", HOST), ("status", "500")]);
        for (status, reason) in [
            (429, english().get(Key::PairingBusy).to_string()),
            (500, unexpected),
        ] {
            let mut pocket_sync = romm(HOST);
            let too_many = test_error_response("Too many authorize attempts.");
            pocket_sync.respond("POST", INIT, status, &too_many);
            let question = play(&card_with_server(), &mut pocket_sync, &["Connect"]);
            assert_eq!(question, ui::connect(&english(), HOST));
            assert_eq!(pocket_sync.lines.last(), Some(&reason));
            assert!(pocket_sync.opened.is_empty());
        }
    }

    #[test]
    fn a_sign_in_that_cant_be_saved_ends_the_run() {
        let root = card_with_server();
        // A folder where the file being written goes.
        fs::create_dir(root.path().join("romm-sync/server.json.part")).unwrap();
        let mut pocket_sync = romm_to_pair();
        approve(&mut pocket_sync, &Permission::ALL);
        let question = play(&root, &mut pocket_sync, &["Connect"]);
        assert_eq!(question, PluginMessage::Exit);
        let not_saved = english().get(Key::SignInNotSaved).to_string();
        assert!(pocket_sync.lines.contains(&not_saved));
        assert_no_secrets(&pocket_sync.lines);
    }

    #[test]
    fn a_server_that_cant_be_used_is_offered_again() {
        let too_old = english().fill(Key::RommTooOld, &[("host", HOST), ("version", "5.3.2")]);
        let not_romm = english().fill(Key::NotRomm, &[("host", HOST), ("status", "404")]);
        for (status, body, reason) in [
            (200, heartbeat("5.3.2"), too_old),
            (404, "Not Found".to_string(), not_romm),
        ] {
            let mut pocket_sync = FakePocketSync::default();
            pocket_sync.respond("GET", HEARTBEAT, status, &body);
            let question = play(&card_with_server(), &mut pocket_sync, &["Connect"]);
            assert_eq!(question, ui::connect(&english(), HOST));
            assert_eq!(pocket_sync.lines[1], reason);
        }
    }

    #[test]
    fn edits_to_server_json_are_used_on_connect() {
        let root = signed_in_card();
        let other = "http://192.168.1.5:8080";
        let mut pocket_sync = romm_at(other);
        let mut run = started(&root, &mut pocket_sync);
        Card::new(root.path()).save_server_host(other).unwrap();
        let connect = HostMessage::Answer {
            name: ui::CONNECT.into(),
            value: "Connect".into(),
        };
        assert_eq!(
            handle(connect, &mut run, &mut pocket_sync),
            ui::menu(&english())
        );
    }

    #[test]
    fn options_change_the_server_address_and_keep_the_sign_in() {
        let root = signed_in_card();
        let other = "http://192.168.1.5:8080";
        let mut pocket_sync = romm_at(other);
        let asked = play(
            &root,
            &mut pocket_sync,
            &["Options", "Change server address"],
        );
        assert_eq!(asked, ui::server(&english(), Some(HOST)));

        let answers = ["Options", "Change server address", other];
        assert_eq!(
            play(&root, &mut pocket_sync, &answers),
            ui::menu(&english())
        );
        let card = Card::new(root.path());
        assert_eq!(card.server_host(), Ok(Some(other.into())));
        // A server that moved accepts it, so there's no pairing again.
        assert_eq!(card.sign_in(), Ok(Some(card_sign_in())));
        assert_eq!(pocket_sync.opened, Vec::<String>::new());
    }

    #[test]
    fn unpair_deletes_the_device_in_romm_and_server_json_on_the_card() {
        let root = signed_in_card();
        let pocket_id = Card::new(root.path()).pocket_id().unwrap();
        let mut pocket_sync = romm(HOST);
        pocket_sync.respond("DELETE", DEVICE, 204, "");
        let question = play(&root, &mut pocket_sync, &["Options", "Unpair", "Unpair"]);
        assert_eq!(question, ui::server(&english(), None));

        let card = Card::new(root.path());
        assert!(!root.path().join("romm-sync/server.json").exists());
        assert_eq!(card.pocket_id().unwrap(), pocket_id);
        let deleted = pocket_sync.sent.last().unwrap();
        assert_eq!((deleted.method, deleted.url.as_str()), ("DELETE", DEVICE));
        assert_eq!(
            deleted.headers,
            [("Authorization", "Bearer rmm_saved".to_string())]
        );
        assert_eq!(
            pocket_sync.lines[2..],
            [english().get(Key::Unpairing), english().get(Key::Unpaired)]
        );
        assert_no_secrets(&pocket_sync.lines);
    }

    #[test]
    fn unpair_asks_first_and_cancel_changes_nothing() {
        let root = signed_in_card();
        let mut pocket_sync = FakePocketSync::default();
        let asked = play(&root, &mut pocket_sync, &["Options", "Unpair"]);
        assert_eq!(asked, ui::unpair(&english()));
        let question = play(&root, &mut pocket_sync, &["Options", "Unpair", "Cancel"]);
        assert_eq!(question, ui::options(&english()));
        assert!(pocket_sync.sent.is_empty());
        assert_eq!(Card::new(root.path()).sign_in(), Ok(Some(card_sign_in())));
    }

    #[test]
    fn a_pocket_without_a_sign_in_isnt_paired() {
        let mut pocket_sync = FakePocketSync::default();
        let question = play(
            &card_with_server(),
            &mut pocket_sync,
            &["Options", "Unpair"],
        );
        assert_eq!(question, ui::options(&english()));
        assert_eq!(pocket_sync.lines, [english().get(Key::NotPaired)]);
    }

    #[test]
    fn when_romm_doesnt_delete_the_device_nothing_changes_on_the_pocket() {
        let unexpected =
            english().fill(Key::UnexpectedAnswer, &[("host", HOST), ("status", "500")]);
        for (status, reason) in [
            (401, english().get(Key::UnpairRejected).to_string()),
            (
                403,
                english().fill(Key::UnpairNotAllowed, &[("permissions", "devices.write")]),
            ),
            (404, english().get(Key::UnpairNoDevice).to_string()),
            (500, unexpected),
        ] {
            let root = signed_in_card();
            let mut pocket_sync = romm(HOST);
            pocket_sync.respond("DELETE", DEVICE, status, &test_error_response("x"));
            let question = play(&root, &mut pocket_sync, &["Options", "Unpair", "Unpair"]);
            assert_eq!(question, ui::options(&english()), "{status}");
            assert_eq!(pocket_sync.lines.last(), Some(&reason));
            let card = Card::new(root.path());
            assert_eq!(card.sign_in(), Ok(Some(card_sign_in())));
            assert_eq!(card.server_host(), Ok(Some(HOST.into())));
        }
    }

    #[test]
    fn unpair_needs_the_server_to_answer_as_romm() {
        let root = signed_in_card();
        let mut pocket_sync = FakePocketSync::default();
        pocket_sync.respond("GET", HEARTBEAT, 404, "Not Found");
        let question = play(&root, &mut pocket_sync, &["Options", "Unpair", "Unpair"]);
        assert_eq!(question, ui::options(&english()));
        let not_romm = english().fill(Key::NotRomm, &[("host", HOST), ("status", "404")]);
        assert_eq!(pocket_sync.lines.last(), Some(&not_romm));
        assert_eq!(Card::new(root.path()).sign_in(), Ok(Some(card_sign_in())));
    }

    #[test]
    fn back_leaves_options() {
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&card_with_server(), &mut pocket_sync, &["Options", "Back"]);
        assert_eq!(question, ui::connect(&english(), HOST));
    }

    #[test]
    fn exit_ends_the_run() {
        let mut pocket_sync = FakePocketSync::default();
        let question = play(&card_with_server(), &mut pocket_sync, &["Exit"]);
        assert_eq!(question, PluginMessage::Exit);
    }

    #[test]
    fn kill_ends_the_run() {
        let root = card_with_server();
        let mut pocket_sync = FakePocketSync::default();
        let mut run = started(&root, &mut pocket_sync);
        assert_eq!(
            handle(HostMessage::Kill, &mut run, &mut pocket_sync),
            PluginMessage::Exit
        );
    }

    /// Gives `value` as the answer to question `name` while the menu is waiting. Returns the next
    /// question and the log.
    fn at_menu(name: &str, value: &str) -> (PluginMessage, Vec<String>) {
        let strings = english();
        let mut run = Run {
            asked: ui::menu(&strings),
            strings,
            card: Card::new("unused"),
            pocket_id: PocketId::parse("6f1c5d0e-2b7a-4c1e-9a3d-0e5f8b2c4d71").unwrap(),
            new_device: None,
        };
        let mut pocket_sync = FakePocketSync::default();
        let answer = HostMessage::Answer {
            name: name.into(),
            value: value.into(),
        };
        let question = handle(answer, &mut run, &mut pocket_sync);
        (question, pocket_sync.lines)
    }

    #[test]
    fn sync_now_is_not_available_yet() {
        let english = english();
        let not_ready = vec![english.get(Key::SyncNotReady).to_string()];
        assert_eq!(
            at_menu(ui::MENU, english.get(Key::SyncNow)),
            (ui::menu(&english), not_ready)
        );
    }

    #[test]
    fn settings_are_not_available_yet() {
        let english = english();
        let not_ready = vec![english.get(Key::SettingsNotReady).to_string()];
        assert_eq!(
            at_menu(ui::MENU, english.get(Key::Settings)),
            (ui::menu(&english), not_ready)
        );
    }

    #[test]
    fn exit_on_the_menu_ends_the_run() {
        let exit = english().get(Key::Exit).to_string();
        assert_eq!(at_menu(ui::MENU, &exit), (PluginMessage::Exit, vec![]));
    }

    #[test]
    fn unexpected_answers_ask_the_question_again() {
        assert_eq!(at_menu(ui::MENU, "Nope"), (ui::menu(&english()), vec![]));
        // A menu label sent to a question that isn't waiting is ignored too.
        let exit = english().get(Key::Exit).to_string();
        assert_eq!(at_menu(ui::CONNECT, &exit), (ui::menu(&english()), vec![]));
    }
}
