use std::{
    collections::BTreeMap,
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    thread,
};

use eframe::{App, CreationContext};
use egui::{CentralPanel, Ui};

use crate::{
    engine::{EngineScript, get_eve_log_dir, run_scripts},
    scripts::{
        LogScript,
        combat_notifier::CombatNotifier,
        decloak_notifier::DecloakedNotifier,
        friendly_activation::FriendlyActivation,
        log_printer::LogPrinter,
        mentioned_notifier::MentionedNotifier,
        mining_notifier::MiningNotifier,
        notification::{self, NotificationUi, notification_settings},
    },
};

pub fn run_ui() -> eframe::Result<()> {
    let mut native_options = eframe::NativeOptions::default();
    native_options.persist_window = true;

    eframe::run_native(
        "EVE Log Scripts",
        native_options,
        Box::new(|cc| Ok(Box::new(EveLogScriptApp::new(cc).unwrap()))),
    )
}

const STORAGE_KEY_APP: &str = "MainApp";
const STORAGE_KEY_LOG_FINDER: &str = "LogFinderApp";

struct Script {
    name: String,
    script: Arc<EngineScript>,
}

enum Selected {
    Script(usize),
    Notification,
}

struct EveLogScriptApp {
    scripts: Vec<Script>,
    selected_script: Option<Selected>,
    script_sender: Sender<Arc<EngineScript>>,
    characters: Arc<RwLock<Vec<String>>>,
    notification_ui: NotificationUi,
    file_path_to_save: Option<String>,
}

impl EveLogScriptApp {
    fn new(cc: &CreationContext) -> io::Result<Self> {
        let mut path = None;
        if let Some(storage) = cc.storage {
            if let Some(logs) = storage.get_string(STORAGE_KEY_LOG_FINDER) {
                path = Some(PathBuf::from(logs));
            }
        }

        if path.is_none() {
            path = get_eve_log_dir();
        }

        let mut file_path_to_save = None;
        if path.is_none() {
            let dialog = rfd::FileDialog::new()
                .set_title("Log location not found. Select the log location.");
            path = dialog.pick_folder();
            if let Some(user_path) = &path {
                if let Some(path_str) = user_path.to_str() {
                    file_path_to_save = Some(path_str.to_string());
                }
            }
        }

        let Some(log_path) = path else {
            panic!("Failed to find log location and user faild to provide one.");
        };

        println!("Using log location {log_path:?}");

        notification::load(cc.storage);
        let (tx, rx) = mpsc::channel();
        let characters = Arc::new(RwLock::new(vec![]));
        let characters2 = Arc::clone(&characters);
        thread::spawn(move || {
            println!(
                "Failed to run scripts {:?}",
                run_scripts(rx, characters2, log_path)
            );
        });

        let mut out = Self {
            scripts: vec![],
            selected_script: None,
            script_sender: tx,
            characters,
            notification_ui: NotificationUi::new(),
            file_path_to_save,
        };

        #[cfg(debug_assertions)]
        out.add_script(
            "Mining notifier".into(),
            Box::new(MiningNotifier::new().unwrap()),
        );
        out.add_script(
            "Combat notifier".into(),
            Box::new(CombatNotifier::new(cc.storage)),
        );
        out.add_script(
            "Friendly activation".into(),
            Box::new(FriendlyActivation::new(cc.storage)),
        );
        #[cfg(debug_assertions)]
        out.add_script("Log printer".into(), Box::new(LogPrinter::new(cc.storage)));
        out.add_script(
            "Mention Notifier".into(),
            Box::new(MentionedNotifier::new(cc.storage)),
        );
        out.add_script(
            "Decloak notifier".into(),
            Box::new(DecloakedNotifier::new(cc.storage)),
        );

        if let Some(storage) = cc.storage {
            if let Some(json) = storage.get_string(STORAGE_KEY_APP) {
                if let Ok(enabled) = serde_json::from_str::<BTreeMap<String, bool>>(&json) {
                    for script in out.scripts.iter_mut() {
                        if let Some(is_enabled) = enabled.get(&script.name) {
                            script.script.enabled.store(*is_enabled, Ordering::Release);
                        }
                    }
                }
            }
        }

        Ok(out)
    }

    fn add_script(&mut self, name: String, script: Box<dyn LogScript>) {
        let engine_script = Arc::new(EngineScript {
            enabled: AtomicBool::new(false),
            script: Mutex::new(script),
        });
        self.script_sender.send(Arc::clone(&engine_script)).unwrap();
        self.scripts.push(Script {
            name,
            script: engine_script,
        });
    }

    fn script_selector(&mut self, ui: &mut Ui) {
        egui::Grid::new("Script grid")
            .min_col_width(0.)
            .show(ui, |ui| {
                // Empty label for moving button
                ui.label("");
                if ui.button("Notification Settings").clicked() {
                    self.selected_script = Some(Selected::Notification);
                }
                ui.end_row();

                for (i, script) in self.scripts.iter_mut().enumerate() {
                    let before = script.script.enabled.load(Ordering::Acquire);
                    let mut enabled = before;
                    ui.checkbox(&mut enabled, "");
                    if !before && enabled {
                        // Just got enabled
                        script.script.enabled.store(true, Ordering::Release);
                        script.script.script.lock().unwrap().on_enabled();
                    } else if before && !enabled {
                        // Just got disabled
                        script.script.enabled.store(false, Ordering::Release);
                        script.script.script.lock().unwrap().on_disabled();
                    }
                    if ui.button(&script.name).clicked() {
                        self.selected_script = Some(Selected::Script(i));
                    }
                    ui.end_row();
                }
            });
    }
}

impl App for EveLogScriptApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |_ui| {
            egui::SidePanel::left("ScriptSidePanel").show(ctx, |ui| self.script_selector(ui));
            egui::CentralPanel::default().show(ctx, |ui| {
                let Some(selected) = &self.selected_script else {
                    ui.label("No script selected");
                    return;
                };
                match selected {
                    Selected::Script(i) => {
                        let settings = &self.scripts[*i];
                        ui.label(format!("{} settings", settings.name));
                        settings
                            .script
                            .script
                            .lock()
                            .unwrap()
                            .settings_ui(ui, &self.characters.read().unwrap());
                    }
                    Selected::Notification => {
                        notification_settings(ui, &mut self.notification_ui);
                    }
                }
            });
        });
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let mut enabled: BTreeMap<&String, bool> = BTreeMap::new();
        for script in &self.scripts {
            enabled.insert(&script.name, script.script.enabled.load(Ordering::Acquire));
        }
        storage.set_string(STORAGE_KEY_APP, serde_json::to_string(&enabled).unwrap());
        notification::save(storage);
        for script in &self.scripts {
            script.script.script.lock().unwrap().save(storage);
        }
        if let Some(path) = self.file_path_to_save.take() {
            storage.set_string(STORAGE_KEY_LOG_FINDER, path);
        }
    }
}
