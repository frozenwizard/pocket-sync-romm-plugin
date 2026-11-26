//! The Pocket's SD card: checking it is one, and the plugin's files on it. See Architecture.md,
//! sections 3.1 and 7.1.

use std::fmt;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use extism_pdk::json::error::Category;
use extism_pdk::json::{self, Map, Value};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::romm::SignIn;

/// Folders every Pocket card has.
const POCKET_FOLDERS: [&str; 3] = ["Cores", "Assets", "Platforms"];

/// The plugin's folder on the card.
const FOLDER: &str = "romm-sync";

/// The Pocket's identity and when it last synced. Every computer that syncs the Pocket reads the
/// same one.
const DEVICE_FILE: &str = "device.json";

/// The RomM server the Pocket syncs with, and what it syncs. People may edit it by hand.
const SERVER_FILE: &str = "server.json";

/// The version of the plugin's file formats, written in each file so a later plugin can tell an
/// older file.
const SCHEMA: u32 = 1;

/// The Pocket's ID: a random UUID, kept in `romm-sync/device.json`. RomM knows the Pocket by it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PocketId(Uuid);

impl fmt::Display for PocketId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
impl PocketId {
    pub fn parse(text: &str) -> Option<Self> {
        Uuid::parse_str(text).ok().map(PocketId)
    }
}

/// `romm-sync/device.json`.
#[derive(Serialize, Deserialize)]
struct DeviceFile {
    #[serde(default = "schema")]
    schema: u32,
    // A UUID, so a card edited by hand can't put a path in the file names it's used for.
    pocket_id: Uuid,
    /// When the Pocket last finished a sync, as RFC 3339. Sync now sets it.
    #[serde(default)]
    last_synced_at: Option<String>,
    /// Fields from a newer plugin, kept when this one writes the file.
    #[serde(flatten)]
    rest: Map<String, Value>,
}

fn schema() -> u32 {
    SCHEMA
}

/// `romm-sync/server.json` as the plugin writes it: `schema` and `host` first, then the rest of the
/// file. It's read as plain JSON, so a file written by hand, or with a field this plugin can't
/// read, keeps its other fields when the plugin changes one.
#[derive(Serialize)]
struct ServerFile {
    schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<Value>,
    #[serde(flatten)]
    rest: Map<String, Value>,
}

/// Why one of the plugin's files can't be read. Each says where the problem is and never quotes the
/// file, so a token in it can't reach the log.
#[derive(Debug, PartialEq)]
pub enum ReadError {
    /// Reading the file failed.
    Io(String),
    /// The file doesn't match its format.
    Format(String),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ReadError::Io(reason) | ReadError::Format(reason) => reason.fmt(f),
        }
    }
}

/// Why the Pocket has no ID. A new ID would make RomM see a different Pocket, so a `device.json`
/// that's there is always left alone.
#[derive(Debug)]
pub enum PocketIdError {
    /// `device.json` is there but can't be read.
    Unreadable(String),
    /// `device.json` doesn't match its format. Nobody should edit it, so it's damaged.
    Mismatch(String),
    /// There was no `device.json`, and writing one failed.
    NotSaved(io::Error),
}

impl fmt::Display for PocketIdError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PocketIdError::Unreadable(reason) | PocketIdError::Mismatch(reason) => reason.fmt(f),
            PocketIdError::NotSaved(error) => error.fmt(f),
        }
    }
}

/// The Pocket's SD card, at the folder Pocket Sync gives the plugin.
pub struct Card {
    root: PathBuf,
}

impl Card {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Card { root: root.into() }
    }

    /// Whether the card looks like a Pocket's: it has the folders every Pocket card has.
    pub fn is_pocket_card(&self) -> bool {
        POCKET_FOLDERS
            .iter()
            .all(|folder| self.root.join(folder).is_dir())
    }

    /// The Pocket's ID. A card with no `device.json` gets one with a new ID.
    pub fn pocket_id(&self) -> Result<PocketId, PocketIdError> {
        match self.read::<DeviceFile>(DEVICE_FILE) {
            Ok(Some(file)) => Ok(PocketId(file.pocket_id)),
            Ok(None) => {
                let file = DeviceFile {
                    schema: SCHEMA,
                    pocket_id: Uuid::new_v4(),
                    last_synced_at: None,
                    rest: Map::new(),
                };
                self.write(DEVICE_FILE, &file)
                    .map_err(PocketIdError::NotSaved)?;
                Ok(PocketId(file.pocket_id))
            }
            Err(ReadError::Io(reason)) => Err(PocketIdError::Unreadable(reason)),
            Err(ReadError::Format(reason)) => Err(PocketIdError::Mismatch(reason)),
        }
    }

    /// The server's address, from `host` in `romm-sync/server.json`. `None` when there's no such
    /// file or it has no address in `host`. The error says why the file can't be read.
    pub fn server_host(&self) -> Result<Option<String>, ReadError> {
        let file = self.read::<Map<String, Value>>(SERVER_FILE)?;
        Ok(text_field(file.as_ref(), "host"))
    }

    /// The sign-in in `romm-sync/server.json`, from `api_key` and `romm_device_id`. `None` unless
    /// both are there and make a sign-in that can be used (`SignIn::new`).
    pub fn sign_in(&self) -> Result<Option<SignIn>, ReadError> {
        let file = self.read::<Map<String, Value>>(SERVER_FILE)?;
        let api_key = text_field(file.as_ref(), "api_key");
        let device_id = text_field(file.as_ref(), "romm_device_id");
        Ok(api_key
            .zip(device_id)
            .and_then(|(api_key, device_id)| SignIn::new(api_key, device_id)))
    }

    /// Sets `host` in `romm-sync/server.json`, keeping the rest of the file, sign-in included. A
    /// missing file is created. One that can't be read is left alone, since it may hold the
    /// sign-in.
    pub fn save_server_host(&self, host: &str) -> Result<(), String> {
        self.change_server_file(|file| {
            file.insert("host".into(), host.into());
        })
    }

    /// Sets `api_key` and `romm_device_id` in `romm-sync/server.json` to `sign_in`, keeping the
    /// rest of the file. One that can't be read is left alone.
    pub fn save_sign_in(&self, sign_in: &SignIn) -> Result<(), String> {
        self.change_server_file(|file| {
            file.insert("api_key".into(), sign_in.api_key().into());
            file.insert("romm_device_id".into(), sign_in.device_id().into());
        })
    }

    /// Deletes `romm-sync/server.json`, the RomM address and sign-in, leaving the Pocket's ID. A
    /// missing file counts as deleted. The error says why it can't be deleted.
    pub fn delete_server_file(&self) -> Result<(), String> {
        match fs::remove_file(self.path(SERVER_FILE)) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        }
    }

    /// Reads `romm-sync/server.json` as plain JSON, applies `change`, and writes it back. A missing
    /// file starts empty. The error says why the file can't be read or written.
    fn change_server_file(
        &self,
        change: impl FnOnce(&mut Map<String, Value>),
    ) -> Result<(), String> {
        let mut rest = self
            .read::<Map<String, Value>>(SERVER_FILE)
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        change(&mut rest);
        let file = ServerFile {
            schema: rest.remove("schema").unwrap_or(SCHEMA.into()),
            host: rest.remove("host"),
            rest,
        };
        self.write(SERVER_FILE, &file)
            .map_err(|error| error.to_string())
    }

    /// The JSON file `name` in the plugin's folder, or `None` if there's no such file.
    fn read<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>, ReadError> {
        let bytes = match fs::read(self.path(name)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(ReadError::Io(error.to_string())),
        };
        let mut file = json::Deserializer::from_slice(&bytes);
        let value = serde_path_to_error::deserialize(&mut file)
            .map_err(|error| describe(&error.path().to_string(), error.into_inner()))?;
        // Nothing may follow the JSON value, as `json::from_slice` checks.
        file.end().map_err(|error| describe(".", error))?;
        Ok(Some(value))
    }

    /// Writes `file` as the JSON file `name` in the plugin's folder.
    fn write(&self, name: &str, file: &impl Serialize) -> io::Result<()> {
        let bytes = json::to_vec_pretty(file).expect("the plugin's files serialize");
        write_whole(&self.path(name), &bytes)
    }

    /// Deletes the `.part` files a cut-off write left in the plugin's folder (Architecture §13,
    /// rule 2). Best effort: one that can't be deleted stays, and the next write of its file
    /// replaces it anyway.
    pub fn remove_leftover_parts(&self) {
        let Ok(entries) = fs::read_dir(self.root.join(FOLDER)) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "part")
                && path.is_file()
            {
                let _ = fs::remove_file(path);
            }
        }
    }

    /// The path of `name` in the plugin's folder.
    fn path(&self, name: &str) -> PathBuf {
        self.root.join(FOLDER).join(name)
    }
}

/// The text in field `name` of `file`, trimmed. `None` when it's missing, empty or not text.
fn text_field(file: Option<&Map<String, Value>>, name: &str) -> Option<String> {
    let text = file?.get(name)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.into())
}

/// Where `error` is in a file, and what kind of problem it is. `path` is the field being read, `.`
/// for the whole file. The JSON parser's own message can quote a value from the file, which could
/// be a token, so it's kept only for a missing field, where it names the field.
fn describe(path: &str, error: json::Error) -> ReadError {
    let place = format!("line {}, column {}", error.line(), error.column());
    ReadError::Format(match error.classify() {
        Category::Syntax | Category::Io => format!("not valid JSON at {place}"),
        Category::Eof => format!("the file ends early, at {place}"),
        Category::Data => {
            let message = error.to_string();
            let missing = message
                .strip_prefix("missing field `")
                .and_then(|rest| rest.split_once('`'));
            match missing {
                Some((field, _)) if path == "." => format!("`{field}` is missing, at {place}"),
                Some((field, _)) => format!("`{path}.{field}` is missing, at {place}"),
                None if path == "." => format!("the file isn't a JSON object, at {place}"),
                None => format!("`{path}` has a value RomM Sync can't use, at {place}"),
            }
        }
    })
}

/// Writes `bytes` to `path`, creating its folder. The bytes go to `<path>.part` first, which is then
/// renamed, so a card pulled out mid-write never holds half a file under the real name.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let written = write_and_sync(Path::new(&part), bytes).and_then(|()| fs::rename(&part, path));
    if written.is_err() {
        // Best effort. One left behind is deleted when the next run starts.
        let _ = fs::remove_file(&part);
    }
    written
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// An empty folder set up like a Pocket's card, deleted when dropped.
#[cfg(test)]
pub fn test_card() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for folder in POCKET_FOLDERS {
        fs::create_dir(root.path().join(folder)).unwrap();
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romm::test_sign_in;
    use extism_pdk::json::json;
    use tempfile::TempDir;

    fn device_file(root: &TempDir) -> PathBuf {
        root.path().join(FOLDER).join(DEVICE_FILE)
    }

    fn server_file(root: &TempDir) -> PathBuf {
        root.path().join(FOLDER).join(SERVER_FILE)
    }

    fn read_json(path: PathBuf) -> Value {
        json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn a_pocket_card_has_cores_assets_and_platforms() {
        let root = test_card();
        assert!(Card::new(root.path()).is_pocket_card());
        fs::remove_dir(root.path().join("Platforms")).unwrap();
        assert!(!Card::new(root.path()).is_pocket_card());
    }

    #[test]
    fn a_new_pocket_gets_an_id_that_stays() {
        let root = test_card();
        let card = Card::new(root.path());
        let id = card.pocket_id().unwrap();
        assert_eq!(card.pocket_id().unwrap(), id);

        assert_eq!(
            read_json(device_file(&root)),
            json!({"schema": 1, "pocket_id": id.to_string(), "last_synced_at": null})
        );
        assert!(!root.path().join(FOLDER).join("device.json.part").exists());
    }

    #[test]
    fn an_existing_id_is_read() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        fs::write(
            device_file(&root),
            r#"{
              "schema": 1,
              "pocket_id": "6f1c5d0e-2b7a-4c1e-9a3d-0e5f8b2c4d71",
              "last_synced_at": "2026-11-18T09:00:00Z"
            }"#,
        )
        .unwrap();
        let id = Card::new(root.path()).pocket_id().unwrap();
        assert_eq!(id.to_string(), "6f1c5d0e-2b7a-4c1e-9a3d-0e5f8b2c4d71");
    }

    #[test]
    fn a_device_file_needs_only_the_id() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        let id = "6f1c5d0e-2b7a-4c1e-9a3d-0e5f8b2c4d71";
        fs::write(device_file(&root), json!({"pocket_id": id}).to_string()).unwrap();
        assert_eq!(Card::new(root.path()).pocket_id().unwrap().to_string(), id);
    }

    #[test]
    fn a_broken_device_file_is_left_alone() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        for broken in [r#"{"schema": 1, "pocket_id": "../../x"}"#, "{"] {
            fs::write(device_file(&root), broken).unwrap();
            let result = Card::new(root.path()).pocket_id();
            assert!(
                matches!(result, Err(PocketIdError::Mismatch(_))),
                "{broken}"
            );
            assert_eq!(fs::read_to_string(device_file(&root)).unwrap(), broken);
        }
    }

    #[test]
    fn a_failed_write_is_reported() {
        let root = test_card();
        // A folder where the file being written should be.
        fs::create_dir_all(root.path().join(FOLDER).join("device.json.part")).unwrap();
        let result = Card::new(root.path()).pocket_id();
        assert!(matches!(result, Err(PocketIdError::NotSaved(_))));
    }

    #[test]
    fn a_failed_write_leaves_no_part_file() {
        let root = test_card();
        // A folder with something in it can't be replaced by a file, so the rename fails.
        let target = root.path().join(FOLDER).join("server.json");
        fs::create_dir_all(target.join("in-the-way")).unwrap();
        assert!(write_whole(&target, b"{}").is_err());
        assert!(!root.path().join(FOLDER).join("server.json.part").exists());
    }

    #[test]
    fn leftover_part_files_are_removed() {
        let root = test_card();
        let folder = root.path().join(FOLDER);
        fs::create_dir_all(folder.join("folder.part")).unwrap();
        for name in ["device.json.part", "server.json.part", "notes.txt"] {
            fs::write(folder.join(name), "").unwrap();
        }
        Card::new(root.path()).remove_leftover_parts();
        let mut left: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["folder.part", "notes.txt"]);
    }

    #[test]
    fn a_card_without_server_json_has_no_host() {
        let root = test_card();
        assert_eq!(Card::new(root.path()).server_host(), Ok(None));
    }

    #[test]
    fn a_saved_host_is_read_back() {
        let root = test_card();
        let card = Card::new(root.path());
        card.save_server_host("https://romm.example.com").unwrap();
        assert_eq!(
            card.server_host(),
            Ok(Some("https://romm.example.com".into()))
        );
        assert_eq!(
            read_json(server_file(&root)),
            json!({"schema": 1, "host": "https://romm.example.com"})
        );
    }

    #[test]
    fn a_hand_written_server_json_needs_only_host() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        fs::write(
            server_file(&root),
            r#"{"host": " http://192.168.1.5:8080 "}"#,
        )
        .unwrap();
        let host = Card::new(root.path()).server_host();
        assert_eq!(host, Ok(Some("http://192.168.1.5:8080".into())));
    }

    #[test]
    fn a_server_json_without_an_address_has_no_host() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        for file in [
            json!({"host": ""}),
            json!({"host": "  "}),
            json!({"host": null}),
            json!({"host": 5}),
            json!({"api_key": "k"}),
        ] {
            fs::write(server_file(&root), file.to_string()).unwrap();
            assert_eq!(Card::new(root.path()).server_host(), Ok(None), "{file}");
        }
    }

    #[test]
    fn changing_the_host_keeps_the_rest_of_server_json() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        let before = json!({"host": "https://old.example.com", "api_key": "k", "collection": 12});
        fs::write(server_file(&root), before.to_string()).unwrap();
        Card::new(root.path())
            .save_server_host("https://new.example.com")
            .unwrap();
        assert_eq!(
            read_json(server_file(&root)),
            json!({"schema": 1, "host": "https://new.example.com", "api_key": "k", "collection": 12})
        );
    }

    #[test]
    fn a_new_host_keeps_the_rest_of_a_server_json_without_one() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        let before = json!({"schema": "1", "host": null, "api_key": "k", "romm_device_id": "d"});
        fs::write(server_file(&root), before.to_string()).unwrap();
        Card::new(root.path())
            .save_server_host("https://romm.example.com")
            .unwrap();
        assert_eq!(
            read_json(server_file(&root)),
            json!({"schema": "1", "host": "https://romm.example.com", "api_key": "k", "romm_device_id": "d"})
        );
    }

    #[test]
    fn a_broken_server_json_is_reported_and_kept() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        let broken = r#"{"host": "https://romm.example.com", "api_key": "k",}"#;
        fs::write(server_file(&root), broken).unwrap();
        let card = Card::new(root.path());
        assert!(card.server_host().is_err());
        assert!(card.save_server_host("https://other.example.com").is_err());
        assert!(card.sign_in().is_err());
        assert!(card.save_sign_in(&test_sign_in("rmm_new")).is_err());
        assert_eq!(fs::read_to_string(server_file(&root)).unwrap(), broken);
    }

    #[test]
    fn a_saved_sign_in_is_read_back_and_keeps_the_rest_of_server_json() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        let before =
            json!({"host": "https://romm.example.com", "api_key": "rmm_old", "collection": 12});
        fs::write(server_file(&root), before.to_string()).unwrap();
        let card = Card::new(root.path());
        card.save_sign_in(&test_sign_in("rmm_new")).unwrap();
        assert_eq!(card.sign_in(), Ok(Some(test_sign_in("rmm_new"))));
        assert_eq!(
            read_json(server_file(&root)),
            json!({
                "schema": 1,
                "host": "https://romm.example.com",
                "api_key": "rmm_new",
                "romm_device_id": "c1a2",
                "collection": 12,
            })
        );
    }

    #[test]
    fn a_sign_in_needs_a_usable_token_and_a_device_id() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        let card = Card::new(root.path());
        assert_eq!(card.sign_in(), Ok(None));
        for file in [
            json!({"api_key": "rmm_k"}),
            json!({"romm_device_id": "d"}),
            json!({"api_key": "", "romm_device_id": "d"}),
            json!({"api_key": "rmm_k", "romm_device_id": 5}),
            json!({"api_key": "rmm k", "romm_device_id": "d"}),
            json!({"api_key": "rmm_k\nX-Other: 1", "romm_device_id": "d"}),
        ] {
            fs::write(server_file(&root), file.to_string()).unwrap();
            assert_eq!(card.sign_in(), Ok(None), "{file}");
        }
    }

    #[test]
    fn deleting_server_json_keeps_the_pocket_id() {
        let root = test_card();
        let card = Card::new(root.path());
        let id = card.pocket_id().unwrap();
        card.save_sign_in(&test_sign_in("rmm_k")).unwrap();
        card.delete_server_file().unwrap();
        assert!(!server_file(&root).exists());
        assert_eq!(card.pocket_id().unwrap(), id);
        // Already gone.
        assert_eq!(card.delete_server_file(), Ok(()));
    }

    #[test]
    fn a_server_json_that_cant_be_deleted_is_reported() {
        let root = test_card();
        // A folder can't be deleted as a file.
        fs::create_dir_all(server_file(&root)).unwrap();
        assert!(Card::new(root.path()).delete_server_file().is_err());
    }

    // A value in the file could be a token, and errors go to the log.
    #[test]
    fn errors_say_where_the_problem_is_not_what_the_file_holds() {
        let root = test_card();
        fs::create_dir(root.path().join(FOLDER)).unwrap();
        let card = Card::new(root.path());
        let id = "6f1c5d0e-2b7a-4c1e-9a3d-0e5f8b2c4d71";
        for (device_file_text, expected) in [
            (
                format!(r#"{{"schema": "x", "pocket_id": "{id}"}}"#),
                "`schema` has a value RomM Sync can't use, at line 1, column 14",
            ),
            (
                r#"{"schema": 1}"#.to_string(),
                "`pocket_id` is missing, at line 1, column 13",
            ),
            ("{".to_string(), "the file ends early, at line 1, column 1"),
        ] {
            fs::write(device_file(&root), &device_file_text).unwrap();
            let error = card.pocket_id().unwrap_err().to_string();
            assert_eq!(error, expected, "{device_file_text}");
        }

        fs::write(server_file(&root), r#""rmm_secret123""#).unwrap();
        let error = card.server_host().unwrap_err().to_string();
        assert_eq!(error, "the file isn't a JSON object, at line 1, column 15");
        fs::write(server_file(&root), r#"{"api_key": "rmm_secret123",}"#).unwrap();
        let error = card.server_host().unwrap_err().to_string();
        assert_eq!(error, "not valid JSON at line 1, column 29");
    }
}
