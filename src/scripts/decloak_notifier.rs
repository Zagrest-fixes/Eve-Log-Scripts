use std::collections::BTreeMap;
use eframe::Storage;
use egui::Grid;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::scripts::{
    LogScript,
    notification::{DEFAULT_NOTIFICATION, notification_selection_ui, try_play_notification},
};

const STORAGE_KEY: &str = "Decloaked notifier";

#[derive(Debug, Serialize, Deserialize)]
struct CharacterSettings {
    notification: String,
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DecloakedNotifier {
    ui_settings: BTreeMap<String, CharacterSettings>,
}

impl DecloakedNotifier {
    pub fn new(storage: Option<&dyn Storage>) -> Self {
        if let Some(storage) = storage {
            if let Some(stored) = storage.get_string(STORAGE_KEY) {
                if let Ok(settings) = serde_json::from_str(&stored) {
                    return settings;
                }
            }
        }
        Self {
            ui_settings: BTreeMap::new(),
        }
    }
}

impl LogScript for DecloakedNotifier {
    fn new_game_logs(&mut self, logs: &[String], character: &str) {
        let re = Regex::new(r" \(notify\) Your cloak deactivates due to ").unwrap();
        for log in logs {
            if re.find(log).is_some() {
                let settings = self.ui_settings.get(character);
                let notification = if let Some(char) = settings {
                    if char.enabled {
                        Some(char.notification.as_str())
                    } else {
                        None
                    }
                } 
                else {
                    Some(DEFAULT_NOTIFICATION)
                };

                if let Some(not) = notification {
                    try_play_notification(not);
                }
            }
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            STORAGE_KEY,
            serde_json::to_string(self).expect("Failed to serialize FA APP"),
        );
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, character_list: &[String]) {
        Grid::new("Decloak grid").min_col_width(0.).show(ui, |ui| {
            for char in character_list {
                let char_settings = match self.ui_settings.get_mut(char) {
                    Some(entry) => entry,
                    None => {
                        self.ui_settings.insert(
                            char.clone(),
                            CharacterSettings {
                                notification: DEFAULT_NOTIFICATION.to_string(),
                                enabled: true
                            },
                        );
                        self.ui_settings.get_mut(char).unwrap()
                    }
                };
                ui.checkbox(&mut char_settings.enabled, "");
                ui.label(char);
                notification_selection_ui(ui, &mut char_settings.notification, "", format!("{char}Decloak"));
                ui.end_row();
            }
        });
    }
}
