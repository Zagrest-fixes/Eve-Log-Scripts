use std::{collections::BTreeMap, mem::take};

use eframe::Storage;
use egui::Grid;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::scripts::{
    LogScript,
    notification::{DEFAULT_NOTIFICATION, notification_selection_ui, try_play_notification},
};

const STORAGE_KEY: &str = "Mentioned notifier";

#[derive(Debug, Serialize, Deserialize)]
struct CharacterSettings {
    alias_entry: String,
    notification: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MentionedNotifier {
    alias: BTreeMap<String, Vec<String>>,
    ui_settings: BTreeMap<String, CharacterSettings>,
}

impl MentionedNotifier {
    pub fn new(storage: Option<&dyn Storage>) -> Self {
        if let Some(storage) = storage {
            if let Some(stored) = storage.get_string(STORAGE_KEY) {
                if let Ok(settings) = serde_json::from_str(&stored) {
                    return settings;
                }
            }
        }
        Self {
            alias: BTreeMap::new(),
            ui_settings: BTreeMap::new(),
        }
    }
}

impl LogScript for MentionedNotifier {
    fn new_fleet_logs(&mut self, logs: &[String], character: &str) {
        for char in self.alias.get(character).iter().next().unwrap().iter().map(|s| s.as_str()).chain([character].into_iter()) {
            let re_litteral = regex::escape(char);
            let mentioned = Regex::new(&format!(r"\s{re_litteral}\b")).unwrap();
            for log in logs {
                // Includes a " " at the start
                let raw_message = log.split_once(">").unwrap().1;
                if mentioned.find(raw_message).is_some() {
                    let notification = if let Some(setting) = self.ui_settings.get(char) {
                        &setting.notification
                    } else {
                        DEFAULT_NOTIFICATION
                    };
                    try_play_notification(notification);
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
        ui.label("Enter any alternate names to watch for");
        Grid::new("Mentioned grid").show(ui, |ui| {
            for char in character_list {
                let char_settings = match self.ui_settings.get_mut(char) {
                    Some(entry) => entry,
                    None => {
                        self.ui_settings.insert(
                            char.clone(),
                            CharacterSettings {
                                alias_entry: String::new(),
                                notification: DEFAULT_NOTIFICATION.to_string(),
                            },
                        );
                        self.ui_settings.get_mut(char).unwrap()
                    }
                };
                if ui
                    .text_edit_singleline(&mut char_settings.alias_entry)
                    .lost_focus()
                    && char_settings.alias_entry != ""
                {
                    let alias_list = match self.alias.get_mut(char) {
                        Some(list) => list,
                        None => {
                            self.alias.insert(char.clone(), vec![]);
                            self.alias.get_mut(char).unwrap()
                        }
                    };
                    alias_list.push(take(&mut char_settings.alias_entry));
                }
                ui.label(char);
                notification_selection_ui(
                    ui,
                    &mut char_settings.notification,
                    "",
                    format!("{char}_note_mentioned"),
                );
                ui.end_row();
                if let Some(aliases) = self.alias.get_mut(char) {
                    let mut to_remove = None;
                    for (i, alias) in aliases.iter().enumerate() {
                        ui.label("");
                        if ui.button(alias).clicked() {
                            to_remove = Some(i);
                        }
                        ui.end_row();
                    }
                    if let Some(remove) = to_remove {
                        aliases.remove(remove);
                    }
                }
            }
        });
    }
}
