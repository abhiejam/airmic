// Keeps the Windows release build from opening a console window (PRD §10).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    airmic_app_lib::run()
}
