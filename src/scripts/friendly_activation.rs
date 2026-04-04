use std::{mem::take, time::{Duration, Instant}};

use eframe::Storage;
use egui::Grid;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::scripts::{
    LogScript,
    notification::{DEFAULT_NOTIFICATION, notification_selection_ui, try_play_notification},
};

const STORAGE_KEY: &str = "FA SETTINGS";

#[derive(Serialize, Deserialize)]
struct TrackedHoles {
    name: String,
    notification: String,
}

#[derive(Serialize, Deserialize)]
struct FriendlyActivationSettings {
    tracked_holes: Vec<TrackedHoles>,
    hole_text: String,
}

const MIN_TIME_BETWEEN_NOTIFICATIONS: Duration = Duration::from_millis(600);

pub struct FriendlyActivation {
    settings: FriendlyActivationSettings,
    last_call: Option<Instant>,
}

impl FriendlyActivation {
    pub fn new(storage: Option<&dyn Storage>) -> Self {
        if let Some(storage) = storage {
            if let Some(stored) = storage.get_string(STORAGE_KEY) {
                if let Ok(settings) = serde_json::from_str(&stored) {
                    return FriendlyActivation { settings, last_call: None };
                }
            }
        }
        Self {
            
            settings: FriendlyActivationSettings { 
                tracked_holes: Vec::new(),
                hole_text: "".to_string(), 
            },
            last_call: None,
        }
    }
}

impl LogScript for FriendlyActivation {
    fn new_fleet_logs(&mut self, logs: &[String], _character: &str) {
        let activation_re = Regex::new(r"^\[.+\] .+ > fa ?(?P<hole>[a-z]( x\d+)?\d*)\s*$").unwrap();
        for log in logs {
            let lower = log.to_ascii_lowercase();
            let Some(m) = activation_re.captures(&lower) else {
                continue;
            };
            let called_hole = m.name("hole").unwrap().as_str();
            for hole in &self.settings.tracked_holes {
                if called_hole == hole.name {
                    let should_play = if let Some(last_ping) = self.last_call {
                        last_ping + MIN_TIME_BETWEEN_NOTIFICATIONS <= Instant::now()
                    } else {
                        true
                    };
                    if should_play {
                        try_play_notification(&hole.notification);
                        self.last_call = Some(Instant::now());
                    }
                };
            }
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, _character_list: &[String]) {
        if ui.button("Clear all tracked holes").clicked() {
            self.settings.tracked_holes.clear();
        }

        if ui.text_edit_singleline(&mut self.settings.hole_text).lost_focus() {
            let hole = TrackedHoles {
                name: take(&mut self.settings.hole_text).to_ascii_lowercase(),
                notification: DEFAULT_NOTIFICATION.to_string(),
            };
            self.settings.tracked_holes.push(hole);
        };

        let mut remove = None;
        Grid::new("FA grid").show(ui, |ui| {
            for (i, hole) in self.settings.tracked_holes.iter_mut().enumerate() {
                if ui.button(&hole.name).clicked() {
                    remove = Some(i);
                }
                notification_selection_ui(ui, &mut hole.notification, "", format!("fa{i}"));
                ui.end_row();
            }
        });
        if let Some(i) = remove {
            self.settings.tracked_holes.remove(i);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(
            STORAGE_KEY,
            serde_json::to_string(&self.settings).expect("Failed to serialize FA APP"),
        );
    }
}
