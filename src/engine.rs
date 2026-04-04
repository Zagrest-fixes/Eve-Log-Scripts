use std::{
    collections::{BTreeMap, BTreeSet}, env, fs::{File, metadata, read_dir}, io::{self, BufRead, BufReader, ErrorKind, Read, Seek}, mem::replace, path::{Path, PathBuf}, slice, sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    }, thread, time::{Duration, Instant}, vec
};

use chrono::NaiveDateTime;
use notify::{Event, EventKind, RecursiveMode, Watcher, event::CreateKind};

use crate::scripts::LogScript;

const GAME_LOGS_EXTENTION: &str = "Gamelogs/";
const CHAT_LOGS_EXTENTION: &str = "Chatlogs/";

const NEW_LOG_WAIT_PERIOD: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum EngineError {
    Io(io::Error),
    Notify(notify::Error),
    WatcherGaveUp,
    FailedToFindLog(String),
}

impl From<io::Error> for EngineError {
    fn from(value: io::Error) -> Self {
        EngineError::Io(value)
    }
}

impl From<notify::Error> for EngineError {
    fn from(value: notify::Error) -> Self {
        EngineError::Notify(value)
    }
}

pub struct EngineScript {
    pub enabled: AtomicBool,
    pub script: Mutex<Box<dyn LogScript>>,
}

#[derive(Debug, Clone, Copy)]
enum LogType {
    Game,
    Fleet,
}

impl LogType {
    pub fn encoding(self) -> Encoding {
        match self {
            LogType::Fleet => Encoding::UTF16,
            LogType::Game => Encoding::UTF8,
        }
    }
}

enum Encoding {
    UTF8,
    UTF16,
}

struct Log {
    seek_len: u64,
    log_type: LogType,
    account: String,
}

fn listen_for_scripts(
    scripts: Arc<Mutex<Vec<Arc<EngineScript>>>>,
    script_registration: Receiver<Arc<EngineScript>>,
) {
    for script in script_registration {
        scripts.lock().unwrap().push(script);
    }
}

pub fn run_scripts(
    script_registration: Receiver<Arc<EngineScript>>,
    characters: Arc<RwLock<Vec<String>>>,
    log_location: PathBuf,
) -> EngineError {
    let game_logs = match find_logs(log_location.clone(), LogType::Game) {
        Ok(log) => log,
        Err(e) => return e.into(),
    };
    let fleet_logs = match find_logs(log_location.clone(), LogType::Fleet) {
        Ok(log) => log,
        Err(e) => return e.into(),
    };

    let (tx, updates) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => return e.into(),
    };

    let mut logs = BTreeMap::new();
    let game_iter = game_logs
        .into_iter()
        .map(|log| (log.path, LogType::Game, log.name));
    let fleet_iter = fleet_logs
        .into_iter()
        .map(|log| (log.path, LogType::Fleet, log.name));

    if let Err(e) = watcher.watch(
        &get_game_logs(log_location.clone()),
        RecursiveMode::NonRecursive,
    ) {
        return e.into();
    };
    if let Err(e) = watcher.watch(
        &get_chat_logs(log_location.clone()),
        RecursiveMode::NonRecursive,
    ) {
        return e.into();
    };

    let mut characters_set = BTreeSet::new();
    for (log, log_type, character) in game_iter.chain(fleet_iter) {
        let starting_len = match metadata(&log) {
            Ok(metadata) => metadata.len(),
            Err(e) => return e.into(),
        };
        characters_set.insert(character.clone());
        logs.insert(
            log,
            Log {
                seek_len: starting_len,
                log_type,
                account: character,
            },
        );
    }

    let mut scripts = Arc::new(Mutex::new(vec![]));
    let scripts2 = Arc::clone(&scripts);
    thread::spawn(move || {
        listen_for_scripts(scripts2, script_registration);
    });

    let mut chars = characters.write().unwrap();
    *chars = characters_set.into_iter().collect();
    chars.sort();
    drop(chars);
    let mut new_logs_to_add: Vec<ToAddLog> = vec![];
    loop {
        for res in &updates {
            let mut i = 0;
            while new_logs_to_add.len() > i {
                let new_log = &new_logs_to_add[i];
                if Instant::now() >= new_log.wait_until {
                    try_add_new_log(&mut logs, &log_location, new_log);
                    new_logs_to_add.remove(i);
                } else {
                    i += 1;
                }
            }


            match res {
                Ok(event) => {
                    handle_event(&mut logs, event, &mut scripts, &log_location, &mut new_logs_to_add);
                }
                Err(e) => {
                    println!("watch error: {:?}", e);
                    return EngineError::Notify(e);
                }
            }
        }
    }
}

struct ToAddLog {
    path: PathBuf,
    wait_until: Instant,
}

fn handle_event(
    logs: &mut BTreeMap<PathBuf, Log>,
    event: Event,
    scripts: &mut Arc<Mutex<Vec<Arc<EngineScript>>>>,
    log_location: &PathBuf,
    new_logs_to_add: &mut Vec<ToAddLog>,
) {
    for log_path in &event.paths {
        // Existing log
        if let Some(log) = logs.get_mut(log_path) {
            match &event.kind {
                EventKind::Modify(_) => match parse_new_logs(&log_path, log) {
                    Ok(new_logs) => {
                        let log_type = log.log_type;
                        send_new_logs(new_logs, log_type, &log.account, scripts);
                    }
                    Err(e) => {
                        println!("Failed to get new logs from {log_path:?} for {e:?}");
                    }
                },
                _ => {
                    // println!("Ignored event {event:?}");
                }
            }
        } else {
            // New or non tracked log
            match &event.kind {
                EventKind::Create(e) => {
                    if &CreateKind::File == e {
                        println!("new file at {:?}", log_path);
                        new_logs_to_add.push(ToAddLog {
                            path: log_path.clone(),
                            wait_until: Instant::now() + NEW_LOG_WAIT_PERIOD,
                        });
                    }
                }
                _ => (),
            }
        }
    }
}

fn try_add_new_log(logs: &mut BTreeMap<PathBuf, Log>, log_location: &PathBuf, new_log: &ToAddLog) {
    if let Some(parent) = new_log.path.parent() {
        let (new_logs, log_type) = if parent == get_chat_logs(log_location.clone()) {
            (parse_fresh_fleet_logs(new_log.path.clone()), LogType::Fleet)
        } else if parent == get_game_logs(log_location.clone()) {
            (parse_fresh_game_logs(new_log.path.clone()), LogType::Game)
        } else {
            println!("Uknown created file {:?}", new_log.path);
            return;
        };
        match new_logs {
            Ok(log) => {
                let seek_amount = match get_new_log_seek_amount(&new_log.path, log_type) {
                    Err(e) => {
                        println!("New log seek amount error: {e:?}");
                        return;
                    },
                    Ok(None) => {
                        println!("Failed to find seek len of new log");
                        return;
                    },
                    Ok(Some(seek)) => {
                        seek
                    }
                };
                logs.insert(
                    new_log.path.clone(),
                    Log {
                        seek_len: seek_amount,
                        log_type: log_type,
                        account: log.name,
                    },
                );
            }
            Err(FreshLogParseError::UnexpectedData) => return,
            Err(FreshLogParseError::Io(e)) => {
                println!("Error reading new log {e:?}");
                return;
            }
        }
    }
}

fn send_new_logs(
    new_logs: Vec<String>,
    log_type: LogType,
    character: &str,
    scripts: &mut Arc<Mutex<Vec<Arc<EngineScript>>>>,
) {
    for script in scripts.lock().unwrap().iter_mut() {
        if !script.enabled.load(Ordering::Acquire) {
            continue;
        }
        let mut owned_script = script.script.lock().unwrap();
        match log_type {
            LogType::Game => owned_script.new_game_logs(&new_logs, character),
            LogType::Fleet => owned_script.new_fleet_logs(&new_logs, character),
        }
    }
}

fn parse_new_logs(path: &PathBuf, log: &mut Log) -> Result<Vec<String>, EngineError> {
    let mut log_file = BufReader::new(File::open(path)?);
    log_file.seek(io::SeekFrom::Start(log.seek_len))?;
    let mut buf = vec![];
    log_file.read_to_end(&mut buf)?;
    log.seek_len += buf.len() as u64;

    let new_logs = match log.log_type.encoding() {
        Encoding::UTF8 => String::from_utf8(buf).expect("logs not valid utf-8"),
        Encoding::UTF16 => {
            let mut string = String::from_utf16le_lossy(&buf);
            // REMOVE 0xFEFF from start
            string.remove(0);
            string
        }
    };

    let mut logs_out: Vec<String> = new_logs.split("\n").map(|s| s.trim().to_string()).collect();

    // println!("{new_logs}");

    //Remove extra line
    logs_out.pop();
    Ok(logs_out)
}

fn get_game_logs(mut log_location: PathBuf) -> PathBuf {
    log_location.push(GAME_LOGS_EXTENTION);
    log_location
}

fn get_chat_logs(mut log_location: PathBuf) -> PathBuf {
    log_location.push(CHAT_LOGS_EXTENTION);
    log_location
}

enum FreshLogParseError {
    Io(io::Error),
    UnexpectedData,
}

impl From<io::Error> for FreshLogParseError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug)]
struct PotentialLog {
    path: PathBuf,
    date: NaiveDateTime,
    name: String,
}

fn parse_fresh_game_logs(path: PathBuf) -> Result<PotentialLog, FreshLogParseError> {
    let mut file = BufReader::new(File::open(&path)?);

    const DASH_HEADER: &str =
        "------------------------------------------------------------\r\n  Gamelog\r\n  Listener: ";
    if !read_expected(&mut file, DASH_HEADER)? {
        return Err(FreshLogParseError::UnexpectedData);
    }

    let mut buf = vec![];
    file.read_until(b'\r', &mut buf)?;
    let name = str::from_utf8(&buf).unwrap().trim_end();

    const INBETWEEN: &str = "\n  Session Started: ";
    if !read_expected(&mut file, INBETWEEN)? {
        return Err(FreshLogParseError::UnexpectedData);
    }

    let mut buf = vec![];
    file.read_until(b'\r', &mut buf)?;
    let mut date_raw = String::from_utf8(buf).unwrap();
    //Pop \r
    date_raw.pop();
    let date = NaiveDateTime::parse_from_str(&date_raw, "%Y.%m.%d %H:%M:%S").unwrap();

    Ok(PotentialLog {
        path: path,
        date,
        name: name.to_string(),
    })
}

fn find_logs(mut log_location: PathBuf, log_type: LogType) -> io::Result<Vec<PotentialLog>> {
    let mut logs = vec![];
    log_location = match log_type {
        LogType::Game => get_game_logs(log_location),
        LogType::Fleet => get_chat_logs(log_location),
    };
    for log in read_dir(log_location)? {
        let log = log?;
        if !log.file_type()?.is_file() {
            continue;
        }
        let parsed_logs = match log_type {
            LogType::Fleet => parse_fresh_fleet_logs(log.path()),
            LogType::Game => parse_fresh_game_logs(log.path()),
        };
        let new_log = match parsed_logs {
            Err(FreshLogParseError::Io(e)) => return Err(e),
            Err(FreshLogParseError::UnexpectedData) => continue,
            Ok(new) => new,
        };
        logs.push(new_log);
    }

    Ok(logs)
}

fn parse_fresh_fleet_logs(path: PathBuf) -> Result<PotentialLog, FreshLogParseError> {
    let mut file = BufReader::new(File::open(&path)?);

    file.read_exact(&mut [0, 0])?;
    const DASH_HEADER: &str = "\r\n\r\n\n\n        ---------------------------------------------------------------\n\n          Channel ID:      fleet_";
    if !read_expected_utf_16(&mut file, DASH_HEADER)? {
        return Err(FreshLogParseError::UnexpectedData);
    }

    skip_until_utf_16(&mut file, '\n')?;

    const INBETWEEN_CHANNEL_AND_NAME: &str =
        "          Channel Name:    Fleet\n          Listener:        ";
    if !read_expected_utf_16(&mut file, INBETWEEN_CHANNEL_AND_NAME)? {
        return Err(FreshLogParseError::UnexpectedData);
    }

    let mut buf = vec![];
    read_until_utf_16(&mut file, '\n', &mut buf)?;
    let name = String::from_utf16le_lossy(&buf);
    // println!("{name}");

    const INBETWEEN_NAME_AND_SESSION: &str = "          Session started: ";
    if !read_expected_utf_16(&mut file, INBETWEEN_NAME_AND_SESSION)? {
        return Err(FreshLogParseError::UnexpectedData);
    }

    let mut buf = vec![];
    read_until_utf_16(&mut file, '\n', &mut buf)?;
    let date_raw = String::from_utf16le_lossy(&buf);
    let date = NaiveDateTime::parse_from_str(&date_raw, "%Y.%m.%d %H:%M:%S").unwrap();

    Ok(PotentialLog {
        path: path,
        date,
        name,
    })
}

fn read_expected(file: &mut BufReader<File>, expected: &str) -> io::Result<bool> {
    let mut buf = 0;
    for c in expected.bytes() {
        file.read(slice::from_mut(&mut buf))?;
        if c != buf {
            return Ok(false);
        }
    }

    Ok(true)
}

fn read_expected_utf_16(file: &mut BufReader<File>, expected: &str) -> io::Result<bool> {
    let mut buf = [0, 0];
    for c in expected.encode_utf16() {
        file.read_exact(&mut buf)?;
        let utf_16_c = u16::from_le_bytes(buf);
        // println!("{c:X} {utf_16_c:X}");
        if c != utf_16_c {
            return Ok(false);
        }
    }

    Ok(true)
}

// Returns true if sucessful or false if file ends before finding it. Also returns how many reads until found.
fn skip_until_utf_16(file: &mut BufReader<File>, char: char) -> io::Result<(bool, u64)> {
    let mut char_utf_16 = 0;
    // If this panics then you probably gave a utf-16 char that takes multiple u16 which is not supported by this function yet.
    char.encode_utf16(slice::from_mut(&mut char_utf_16));
    let mut buf = [0; 2];
    let mut num_read = 0;
    loop {
        if let Err(e) = file.read_exact(&mut buf) {
            if e.kind() == ErrorKind::UnexpectedEof {
                return Ok((false, num_read));
            }
            return Err(e);
        };
        num_read += 2;
        let utf_16_c = u16::from_le_bytes(buf);
        if char_utf_16 == utf_16_c {
            return Ok((true, num_read));
        }
    }
}

//Does not include until character. Returns true if sucessful or false if file ends before finding it.
fn read_until_utf_16(
    file: &mut BufReader<File>,
    char: char,
    buf: &mut Vec<u8>,
) -> io::Result<bool> {
    let mut char_utf_16 = 0;
    // If this panics then you probably gave a utf-16 char that takes multiple u16 which is not supported by this function yet.
    char.encode_utf16(slice::from_mut(&mut char_utf_16));
    let mut c_buf = [0; 2];
    loop {
        if let Err(e) = file.read_exact(&mut c_buf) {
            if e.kind() == ErrorKind::UnexpectedEof {
                return Ok(false);
            }
            return Err(e);
        };
        let utf_16_c = u16::from_le_bytes(c_buf);
        if char_utf_16 == utf_16_c {
            return Ok(true);
        }
        buf.extend_from_slice(&c_buf);
    }
}

pub fn get_eve_log_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;

    match env::consts::OS {
        "windows" => {
            // Standard Windows path
            let mut path = dirs::document_dir()?;
            path.push("EVE/logs");
            if path.exists() {
                return Some(path);
            }
        }
        "macos" => {
            // macOS EVE Client path
            let mut path = home.clone();
            path.push("Library/Application Support/EVE Online/p_drive/User/Documents/EVE/logs");
            if path.exists() {
                return Some(path);
            }

            // Fallback for older installations using "My Documents"
            let mut fallback = home;
            fallback
                .push("Library/Application Support/EVE Online/p_drive/User/My Documents/EVE/logs");
            if fallback.exists() {
                return Some(fallback);
            }
        }
        "linux" => {
            // Common Steam install locations on Linux
            let steam_roots = vec![
                home.join(".local/share/Steam"),
                home.join(".steam/steam"),
                home.join(".var/app/com.valvesoftware.Steam/data/Steam"), // Flatpak
            ];

            let suffix_variants = vec![
                "steamapps/compatdata/8500/pfx/drive_c/users/steamuser/Documents/EVE/logs",
                "steamapps/compatdata/8500/pfx/drive_c/users/steamuser/My Documents/EVE/logs",
            ];

            for root in steam_roots {
                for suffix in &suffix_variants {
                    let full_path = root.join(suffix);
                    if full_path.exists() {
                        return Some(full_path);
                    }
                }
            }
        }
        _ => return None,
    }

    None
}


fn get_new_log_seek_amount(log: &Path, log_type: LogType) -> io::Result<Option<u64>> {
    let mut file = BufReader::new(File::open(log)?);
    match log_type {
        LogType::Fleet => {
            match skip_until_utf_16(&mut file, '[')? {
                (false, _) => Ok(None),
                // Sub 2 due to including the first [ and 2 bytes for utf-16
                (true, n) => Ok(n.checked_sub(2)),
            }
        },
        LogType::Game => {
            let n = file.skip_until("[".as_bytes()[0])?;
            Ok(n.checked_sub(1).map(|x| x.try_into().unwrap()))
        }
    }

}

