use std::{collections::VecDeque, sync::mpsc::{self, Receiver, Sender, TryRecvError}, thread::{self, sleep}, time::{Duration, Instant}};

use eframe::Storage;
use egui::Slider;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::scripts::{LogScript, notification::{DEFAULT_NOTIFICATION, try_play_notification}, util::to_raw_text};

#[derive(Debug, PartialEq, Eq)]
enum NotifierCommand {
    WaitUntil(Instant),
    Disable,
    Enable,
}

#[derive(Serialize, Deserialize)]
struct AppSettings {
    num_cycles: u8,
    sample_num: usize,
    num_samples: usize,
    min_num_samples: usize,
}

pub struct CombatNotifier {
    last_delays: VecDeque<Duration>,
    last_shot_time: Instant,
    update_sender: Sender<NotifierCommand>,
    settings: AppSettings,
}

const STORAGE_KEY: &str = "Combat Notifier";

impl CombatNotifier {
    pub fn new(storage: Option<&dyn Storage>) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(|| {
            notifier_main(rx);
        });

        let mut app_settings = AppSettings {
            num_cycles: 3,
            sample_num: 4,
            num_samples: 10,
            min_num_samples: 6,
        };
        if let Some(storage) = storage {
            if let Some(stored) = storage.get_string(STORAGE_KEY) {
                if let Ok(settings) = serde_json::from_str(&stored) {
                    app_settings = settings;
                }
            }
        }
        Self {
            last_delays: VecDeque::new(),
            last_shot_time: Instant::now(),
            update_sender: tx,
            settings: app_settings,
        }
    }

    fn add_new_duration(&mut self, dur: Duration) {
        println!("adding {dur:?} len {}", self.last_delays.len());
        if self.last_delays.len() >= self.settings.num_samples {
            self.last_delays.pop_front();
        }
        self.last_delays.push_back(dur);
    }

    fn expected_cycle_duration(&self) -> Option<Duration> {
        if self.last_delays.len() < self.settings.min_num_samples {
            return None;
        }
        const EXAMPLE_NUM: usize = 3;
        let mut delays: Vec<&Duration> = self.last_delays.iter().collect();
        delays.sort();
        let example = delays[EXAMPLE_NUM];
        return Some(*example);
    }
}

impl LogScript for CombatNotifier {
    fn new_game_logs(&mut self, logs: &[String], _character: &str) {
        let cleaned = to_raw_text(logs);
        let damage_regex = Regex::new(r" \(combat\) \d+ to .* - ").unwrap();
        let now = Instant::now();
        for log in cleaned {
            if damage_regex.find(&log).is_none() {
                continue;
            }
            println!("{now:?} {:?}", self.last_shot_time);
            let time_since_last = now - self.last_shot_time;
            self.add_new_duration(time_since_last);
            self.last_shot_time = now;
            let Some(expected_dur) = self.expected_cycle_duration() else {
                continue;
            };
            let wait_until = expected_dur * self.settings.num_cycles as u32;
            self.update_sender.send(NotifierCommand::WaitUntil(now + wait_until)).unwrap();
        }
    }

    fn on_enabled(&mut self) {
        self.update_sender.send(NotifierCommand::Enable).unwrap();
    }

    fn on_disabled(&mut self) {
        self.update_sender.send(NotifierCommand::Disable).unwrap();
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui, _character_list: &[String]) {
        ui.add(Slider::new(&mut self.settings.num_cycles, 2..=30).text("Number of weapon cycles before notification"));
        ui.label("Advanced settings:");
        ui.add(Slider::new(&mut self.settings.num_samples, 1..=40).text("Number of saved samples"));
        ui.add(Slider::new(&mut self.settings.sample_num, 1..=self.settings.num_samples).text("Example sample"));
        ui.add(Slider::new(&mut self.settings.min_num_samples, self.settings.sample_num..=self.settings.num_samples).text("Min num samples"));
        while (self.settings.num_cycles as usize) < self.last_delays.len() {
            self.last_delays.pop_front();
        } 
    }
    
    fn save(&mut self, storage: &mut dyn Storage) {
        storage.set_string(STORAGE_KEY, serde_json::to_string(&self.settings).expect("Failed to serialize settings"));
    }
}

fn notifier_main(updates: Receiver<NotifierCommand>) {
    loop {
        while updates.recv().unwrap() != NotifierCommand::Enable {}
        println!("Enabled");
        let mut update = updates.recv().unwrap();
        loop {
            println!("update");
            match update {
                NotifierCommand::Disable => break,
                NotifierCommand::Enable => println!("Unexpected enable"),
                NotifierCommand::WaitUntil(instant) => {
                    let sleep_time = instant - Instant::now();
                    println!("Sleeping {sleep_time:?}");
                    sleep(sleep_time);
                }
            }
            match updates.try_recv() {
                Err(TryRecvError::Disconnected) => panic!("Combat notifier disconnected"),
                Err(TryRecvError::Empty) => {
                    try_play_notification(DEFAULT_NOTIFICATION);
                    update = updates.recv().unwrap();
                },
                Ok(new_update) => update = new_update,
            }
        }
    }
}

