use eframe::Storage;
use egui::Ui;

pub mod mining_notifier;
pub mod util;
pub mod notification;
pub mod combat_notifier;
pub mod friendly_activation;
pub mod log_printer;
pub mod mentioned_notifier;
pub mod decloak_notifier;

pub trait LogScript: Send {
    fn new_game_logs(&mut self, _logs: &[String], _character: &str) {}
    fn new_fleet_logs(&mut self, _logs: &[String], _character: &str) {}
    fn on_enabled(&mut self) {}
    fn on_disabled(&mut self) {}
    fn settings_ui(&mut self, ui: &mut Ui, _character_list: &[String]) {
        ui.label("Settings pannel not implemented for this script");
    }
    fn save(&mut self, storage: &mut dyn Storage);
}