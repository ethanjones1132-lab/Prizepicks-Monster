// Prevents additional console window on Windows release
// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    prizepicks_monster_lib::run();
}
