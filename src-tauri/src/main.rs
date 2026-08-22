// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    xterm_lib::install_early_panic_hook();
    xterm_lib::run()
}
