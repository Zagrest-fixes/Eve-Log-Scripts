#![feature(str_from_utf16_endian)]

mod engine;
mod scripts;
mod ui;

use crate::{
    ui::run_ui,
};


fn main() {
    run_ui().unwrap();

    // println!("Hello, world!");
    // let log = find_log().unwrap().expect("Failed to get log");
    // println!("{:?}", log);

    // let mut not = MiningNotifier::new(COVETOR_CARGO_SIZE).unwrap();
    // loop {
    //     let logs = engine.wait_for_log().unwrap();
    //     println!("{logs:?}");
    //     not.new_logs(logs);
    // }
}


