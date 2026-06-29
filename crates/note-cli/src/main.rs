//! The `note` binary entry point. clap CLI front-end over `note-store`.
//! M6 TODO: launch the ratatui-tea TUI when run bare.

// This is a binary crate: items shared across its own modules are written `pub`
// for readability and are never part of a public library API.
#![allow(unreachable_pub)]

mod cli;
mod commands;
mod config;
mod editor;
mod picker;
mod render;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    match commands::dispatch(cli::Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
