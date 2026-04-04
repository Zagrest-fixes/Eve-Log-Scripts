use egui::{ComboBox, Slider};
use regex::Regex;
use rodio::Player;
use strum::{AsRefStr, EnumIter, IntoEnumIterator};

use crate::scripts::{LogScript, notification::{DEFAULT_NOTIFICATION, try_play_notification}, util::to_raw_text};

pub const GNEISS_SIZE: u64 = 500;
pub const COMPRESSED_GNIESS_SIZE: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, AsRefStr)]
pub enum Ship {
    Covetor
}

impl Ship {
    fn cargo_cap(self) -> u64 {
        match self {
            Ship::Covetor => 900000
        }
    }
}

pub struct MiningNotifier {
    ship: Ship,
    compressed_amount: u64,
    raw_ore_amount: u64,
    player: Option<Player>,
    base_cycle_amount: u64,
    num_miners: u64,
    num_cycle_warning: u64,
    notification: String,
}
//" (notify) Your Modulated Strip Miner II has completed operations. Ship's cargo hold is full."
impl MiningNotifier {
    pub fn new() -> Result<Self, rodio::DeviceSinkError> {
        Ok(Self {
            ship: Ship::Covetor,
            compressed_amount: 34490,
            raw_ore_amount: 0,
            player: None,
            base_cycle_amount: 0,
            num_miners: 2,
            num_cycle_warning: 2,
            notification: DEFAULT_NOTIFICATION.into(),
        })
    }

    pub fn estimated_cargo(&self) -> u64 {
        self.compressed_amount * COMPRESSED_GNIESS_SIZE + self.raw_ore_amount * GNEISS_SIZE
    }

    pub fn cycle_potential(&self) -> u64 {
        // const CRIT_MULTIPLIER: u64 = 3;
        // self.base_cycle_amount * self.num_miners * GNEISS_SIZE * CRIT_MULTIPLIER
        self.base_cycle_amount * self.num_miners * GNEISS_SIZE * 2
    }

    pub fn check_cargo(&mut self) {
        if self.player.is_some() {
            return;
        }

        let current_cargo_size = self.estimated_cargo();
        let future_cargo_need = current_cargo_size + self.cycle_potential();
        println!("{} {} > {}", current_cargo_size / 100, future_cargo_need / 100, self.ship.cargo_cap() / 100);
        if future_cargo_need > self.ship.cargo_cap() {
            self.notify();
        }
    }

    fn notify(&mut self) {
        try_play_notification(DEFAULT_NOTIFICATION);
    }

    fn mined(&mut self, amount: u64, ore: &str, mined_type: MinedType) {
        if mined_type == MinedType::Normal {
            self.base_cycle_amount = self.base_cycle_amount.max(amount);
        }
        self.raw_ore_amount += amount;
        self.check_cargo();
        println!("{} {} {}", ore, amount, self.base_cycle_amount);
    }

    fn compressed(&mut self) {
        self.compressed_amount += self.raw_ore_amount;
        self.raw_ore_amount = 0;
        self.player = None;
        println!("COMPRESSED");
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum MinedType {
    Normal,
    Crit,
}

impl LogScript for MiningNotifier {
    fn new_game_logs(&mut self, logs: &[String], _character: &str) {
        let cleaned = to_raw_text(logs);
        println!("---{cleaned:?}");
        let mined_re = Regex::new(r"\(mining\) ((You mined )|(?P<crit>Critical mining success! You mined an additional ))(?P<amount>\d+) units of (?P<ore>.*)").unwrap();
        let compressed_re = Regex::new(r"\(notify\) Successfully compressed ").unwrap();
        for log in cleaned {
            if let Some(m) = mined_re.captures(&log) {
                let ore_amount = m.name("amount").unwrap();
                let ore_type = m.name("ore").unwrap();
                let ore_count: u64 = ore_amount.as_str().parse().unwrap();
                let m_type = if m.name("crit").is_some() {
                    MinedType::Crit
                } else {
                    MinedType::Normal
                };
                self.mined(ore_count, ore_type.as_str(), m_type);
            } else if let Some(_m) = compressed_re.find(&log) {
                self.compressed();
            }
        }
    }
    
    fn settings_ui(&mut self, ui: &mut egui::Ui, _character_list: &[String]) {
        ui.add(Slider::new(&mut self.num_miners, 1..=7).text("Number of mining lasers"));
        ComboBox::from_label("Ship")
            .selected_text(self.ship.as_ref())
            .show_ui(ui, |ui| {
                for ship in Ship::iter() {
                    ui.selectable_value(&mut self.ship, ship, ship.as_ref());
                }
            });
    }
    
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
    }
}
