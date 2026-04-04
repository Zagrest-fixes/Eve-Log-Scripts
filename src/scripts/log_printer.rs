use eframe::Storage;
use serde::{Deserialize, Serialize};

use crate::scripts::{LogScript, util::to_raw_text};

const STORAGE_KEY: &str = "LogPrinterSettings";

#[derive(Debug, Serialize, Deserialize)]
pub struct LogPrinter {
    fleet_logs: bool,
    game_logs: bool,
    clean_game_logs: bool,
}

impl LogPrinter {
    pub fn new(storage: Option<&dyn Storage>) -> Self {
        if let Some(storage) = storage {
            if let Some(stored) = storage.get_string(STORAGE_KEY) {
                if let Ok(settings) = serde_json::from_str(&stored) {
                    return settings;
                }
            }
        }
        Self { fleet_logs: true, game_logs: true, clean_game_logs: false }
    }
}

impl LogScript for LogPrinter {

    fn new_fleet_logs(&mut self, logs: &[String], character: &str) {
        if self.fleet_logs {
            println!("{character}--{:?}", logs);
        }
    }

    fn new_game_logs(&mut self, logs: &[String], character: &str) {
        if self.game_logs {
            let text = if self.clean_game_logs {
                &to_raw_text(logs)
            } else {
                logs
            };
            println!("{character}--{:?}", to_raw_text(logs));
        }
    }
    
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(STORAGE_KEY, serde_json::to_string(self).unwrap());
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, _character_list: &[String]) {
        ui.label("Script for testing/development. Will print to the terminal logs.");
        ui.checkbox(&mut self.fleet_logs, "Print fleet logs");
        ui.checkbox(&mut self.game_logs, "Print game logs");
        ui.checkbox(&mut self.clean_game_logs, "Clean game logs");
    }
}