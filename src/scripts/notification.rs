use std::{
    collections::BTreeMap,
    fs::File,
    io::Cursor,
    mem::take,
    path::PathBuf,
    sync::{
        RwLock,
        mpsc::{self, Receiver, Sender},
    },
    thread,
};

use egui::{ComboBox, Grid, Slider, Ui};
use lazy_static::lazy_static;
use rodio::{Decoder, DeviceSinkError, MixerDeviceSink, Source};
use serde::{Deserialize, Serialize};

lazy_static! {
    static ref REQUESTS: AudioRequester = {
        let notifications = RwLock::new(BTreeMap::new());
        let audio = Audio::new().expect("Failed to init audio");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            take_audio_requests(rx, audio);
        });
        AudioRequester {
            notifications,
            requests: tx,
            default: RwLock::new(String::new()),
            volume: RwLock::new(1.0),
        }
    };
}

pub const DEFAULT_NOTIFICATION: &str = "Default";

const STORAGE_KEY: &str = "Notification settings";

const BUILD_IN_NOTIFICATIONS: &'static [(&'static str, &'static [u8])] = &[
    (
        "Cake",
        include_bytes!("../built_in_notifications/piece-of-cake-611.mp3"),
    ),
    (
        "Bell",
        include_bytes!("../built_in_notifications/undertakers-bell_2UwFCIe.mp3"),
    ),
    (
        "Ding",
        include_bytes!("../built_in_notifications/ding-sound-effect_1_CVUaI0C.mp3"),
    ),
    (
        "Error",
        include_bytes!("../built_in_notifications/error_CDOxCYm.mp3"),
    ),
    (
        "Nice Shot",
        include_bytes!("../built_in_notifications/nice-shot-wii-sports_DJJ0VOz.mp3"),
    ),
    (
        "Question",
        include_bytes!("../built_in_notifications/question-mark.mp3"),
    ),
    (
        "Not good",
        include_bytes!("../built_in_notifications/wcgertcz074.mp3"),
    ),
    (
        "W10 Error",
        include_bytes!("../built_in_notifications/windows-10-error-sound.mp3"),
    ),
];

#[derive(Debug, Clone)]
struct NotWithVolume {
    volume: f32,
    notification: Notification,
}

struct AudioRequester {
    notifications: RwLock<BTreeMap<String, NotWithVolume>>,
    requests: Sender<NotifcationRequest>,
    default: RwLock<String>,
    volume: RwLock<f32>,
}

enum NotifcationRequest {
    GobalVolume(f32),
    PlayNotification(NotWithVolume),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Notification {
    BuiltIn(&'static [u8]),
    Custom(PathBuf),
}

fn take_audio_requests(rx: Receiver<NotifcationRequest>, mut audio: Audio) {
    for request in rx {
        match request {
            NotifcationRequest::GobalVolume(v) => audio.set_volume(v),
            NotifcationRequest::PlayNotification(n) => audio.play_notification(n),
        }
    }
}

pub struct Audio {
    mixer: MixerDeviceSink,
    volume: f32,
}

impl Audio {
    fn new() -> Result<Self, DeviceSinkError> {
        let sink_handle = rodio::DeviceSinkBuilder::open_default_sink()?;
        Ok(Self {
            mixer: sink_handle,
            volume: 1.0,
        })
    }

    fn play_notification(&self, notification: NotWithVolume) {
        let volume = notification.volume * self.volume;
        match notification.notification {
            Notification::BuiltIn(bytes) => self.mixer.mixer().add(
                Decoder::try_from(Cursor::new(bytes))
                    .unwrap()
                    .amplify(volume),
            ),
            Notification::Custom(file_path) => {
                let file = match File::open(file_path) {
                    Ok(f) => f,
                    Err(e) => {
                        println!("Failed to open notification file {e:?}");
                        return;
                    }
                };
                let decoder = match Decoder::try_from(file) {
                    Ok(dec) => dec,
                    Err(e) => {
                        println!("Failed to decode notification file {e:?}");
                        return;
                    }
                };

                self.mixer.mixer().add(decoder.amplify(volume));
            }
        }
    }

    fn set_volume(&mut self, v: f32) {
        self.volume = dbg!(v);
    }
}
#[derive(Debug, Clone, Copy)]
pub enum NotificationError {
    UnknownSound,
    InternalError,
    DefaultInvalid,
}

pub fn try_play_notification(sound: &str) {
    if let Err(e) = play_notification(sound) {
        println!("Failed to play notification {e:?}");
    }
}

pub fn play_notification(sound: &str) -> Result<(), NotificationError> {
    let requests = REQUESTS.notifications.read().unwrap();
    let default = REQUESTS.default.read().unwrap();
    let to_find;
    if sound == DEFAULT_NOTIFICATION {
        to_find = default.as_str();
        if to_find == "" {
            return Err(NotificationError::DefaultInvalid);
        }
    } else {
        to_find = sound;
    }
    let notification = match requests.get(to_find) {
        Some(n) => n.clone(),
        None => return Err(NotificationError::UnknownSound),
    };
    let request = NotifcationRequest::PlayNotification(notification.clone());
    if REQUESTS.requests.send(request).is_err() {
        return Err(NotificationError::InternalError);
    } else {
        return Ok(());
    }
}

pub fn notification_selection_ui(
    ui: &mut Ui,
    notification: &mut String,
    label: &str,
    salt: impl std::hash::Hash,
) {
    ComboBox::new(salt, label)
        .selected_text(notification.as_str())
        .show_ui(ui, |ui| {
            let mut current = notification.as_str();
            let notifications = REQUESTS.notifications.read().unwrap();
            for notification in [DEFAULT_NOTIFICATION]
                .into_iter()
                .chain(notifications.iter().map(|(not, _)| not.as_str()))
            {
                ui.selectable_value(&mut current, notification, notification);
            }
            if current != notification {
                *notification = current.to_string();
            }
        });
}

pub fn set_default(default: String) {
    *REQUESTS.default.write().unwrap() = default;
}

pub fn register_notification(name: String, notification: Notification) {
    let update_default = REQUESTS.default.read().unwrap().as_str() == "";
    if update_default {
        set_default(name.clone());
    }
    REQUESTS.notifications.write().unwrap().insert(
        name,
        NotWithVolume {
            volume: 1.0,
            notification,
        },
    );
}

pub struct NotificationUi {
    path_entry: Option<PathBuf>,
    name: String,
    error_msg: String,
}

impl NotificationUi {
    pub fn new() -> Self {
        Self {
            path_entry: None,
            name: String::new(),
            error_msg: String::new(),
        }
    }
}

pub fn notification_settings(ui: &mut Ui, nui: &mut NotificationUi) {
    let default = REQUESTS.default.read().unwrap();
    let mut current = default.as_str();
    let before = current;

    if ui.button("Add custom notification").clicked() {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            nui.path_entry = Some(path);
        }
    };

    if let Some(path) = &nui.path_entry {
        ui.label(path.to_string_lossy());
    }
    if nui.path_entry.is_some() {
        ui.label("Name:");
        ui.text_edit_singleline(&mut nui.name);
        if ui.button("Add notification").clicked() {
            if nui.name == "" {
                nui.error_msg = "Enter a name".into()
            } else {
                let notifications = REQUESTS.notifications.read().unwrap();
                if notifications.keys().find(|not| *not == &nui.name).is_some() {
                    nui.error_msg = format!("{} already exists", nui.name);
                } else {
                    drop(notifications);
                    nui.error_msg.clear();
                    register_notification(
                        take(&mut nui.name),
                        Notification::Custom(take(&mut nui.path_entry).unwrap()),
                    );
                }
            }
        }
    }

    let notifications = REQUESTS.notifications.read().unwrap();
    ComboBox::from_label("Default Notification")
        .selected_text(before)
        .show_ui(ui, |ui| {
            for notification in notifications.iter().map(|(not, _)| not.as_str()) {
                ui.selectable_value(&mut current, notification, notification);
            }
        });

    if before != current {
        let new = current.to_string();
        drop(default);
        *REQUESTS.default.write().unwrap() = new;
    }

    let mut volume = REQUESTS.volume.write().unwrap();
    let before = *volume;
    ui.label("Volume");
    ui.add(Slider::new(&mut *volume, 0.0..=2.0));
    if *volume != before {
        REQUESTS
            .requests
            .send(NotifcationRequest::GobalVolume(*volume))
            .unwrap();
    }
    drop(notifications);
    let mut notifications = REQUESTS.notifications.write().unwrap();

    let mut to_play = None;
    let mut to_remove = None;
    Grid::new("Not Test grid").show(ui, |ui| {
        for (notification, volume) in notifications.iter_mut() {
            if ui.button(format!("play {notification}")).clicked() {
                to_play = Some(notification.clone());
            }
            ui.add(Slider::new(&mut volume.volume, 0.0..=2.0));
            if let Notification::Custom(_) = volume.notification {
                if ui.button("Remove").clicked() {
                    to_remove = Some(notification.clone());
                }
            }
            ui.end_row();
        }
    });
    if let Some(remove) = &to_remove {
        notifications.remove(remove);
    }
    drop(notifications);
    if let Some(play) = to_play
        && to_remove.is_none()
    {
        play_notification(&play).unwrap();
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct SavedSettings {
    saved_notifications: Vec<SavedNotification>,
    default: String,
    volume: f32,
}

#[derive(Debug, Deserialize, Serialize)]
enum SavedNotification {
    BuiltIn(String, f32),
    Custom(String, PathBuf, f32),
}

pub fn save(storage: &mut dyn eframe::Storage) {
    let notifications = REQUESTS.notifications.read().unwrap();
    let mut saved_notifications = vec![];
    for (name, notification) in notifications.iter() {
        let converted = match &notification.notification {
            Notification::BuiltIn(_) => {
                SavedNotification::BuiltIn(name.clone(), notification.volume)
            }
            Notification::Custom(path) => {
                SavedNotification::Custom(name.clone(), path.clone(), notification.volume)
            }
        };
        saved_notifications.push(converted);
    }

    let to_store = SavedSettings {
        saved_notifications,
        default: REQUESTS.default.read().unwrap().clone(),
        volume: *REQUESTS.volume.read().unwrap(),
    };
    storage.set_string(STORAGE_KEY, serde_json::to_string(&to_store).unwrap());
}

pub fn load(storage: Option<&dyn eframe::Storage>) {
    for (name, data) in BUILD_IN_NOTIFICATIONS {
        register_notification(name.to_string(), Notification::BuiltIn(data));
    }
    if let Some(storage) = storage {
        if let Some(saved) = storage.get_string(STORAGE_KEY) {
            if let Ok(settings) = serde_json::from_str::<SavedSettings>(&saved) {
                REQUESTS
                    .requests
                    .send(NotifcationRequest::GobalVolume(settings.volume))
                    .unwrap();
                *REQUESTS.volume.write().unwrap() = settings.volume;
                *REQUESTS.default.write().unwrap() = settings.default;
                let mut notifications = REQUESTS.notifications.write().unwrap();
                for notification in settings.saved_notifications {
                    match notification {
                        SavedNotification::BuiltIn(name, volume) => {
                            notifications.get_mut(&name).unwrap().volume = volume
                        }
                        SavedNotification::Custom(name, path, volume) => {
                            notifications.insert(
                                name,
                                NotWithVolume {
                                    volume,
                                    notification: Notification::Custom(path),
                                },
                            );
                        }
                    }
                }
            }
        }
    }
}
