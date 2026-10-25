// Без лишнего окна консоли в релизной сборке под Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    scrobb_player_desktop::run();
}
