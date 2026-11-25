//! One file per screen. Each adds its drawing (and any work only it starts) to `App` as an `impl App` block, and
//! returns what the player pressed as an `Action` for `App::run`.

pub mod library;
pub mod log;
pub mod play;
pub mod settings;
pub mod transfer;
