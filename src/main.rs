// A window when started without arguments; command-line mode (see cli.rs) otherwise.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod addons;
mod art;
mod cli;
mod core;
mod find;
mod gui;
mod install;
mod ldb;
mod net;
mod pak;
mod recording;
mod settings;
mod signin;
mod transfer;
mod update;
mod vault;

fn main() {
    update::clean_up();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        gui::run();
    } else {
        // a windows-subsystem exe has no console: borrow the one it was started from
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS);
        }
        std::process::exit(cli::run(args));
    }
}
