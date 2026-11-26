//! One file per page. Each adds its drawing (and any work only it starts) to `App` as an `impl App` block, and
//! returns what the player pressed as an `Action` for `App::run`.

pub mod feeds;
pub mod guild;
pub mod library;
pub mod log;
pub mod settings;
pub mod shares;
pub mod transfer;
pub mod welcome;
