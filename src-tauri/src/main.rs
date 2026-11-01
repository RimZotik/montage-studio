// Без окна консоли в выпуске.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    montage_studio_lib::run()
}
