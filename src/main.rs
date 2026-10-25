//! ainitShot: a small program for one person that replaces Lightshot. It waits in the
//! background for the hotkey, freezes the screen, lets the user select a part of it and draw on
//! it, and then copies it, saves it or asks Google's AI about it. What the program
//! should do is described in docs/en/requirements.md.

// Release builds run without a console window; debug builds keep one for messages.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod annotation;
mod card;
mod color;
mod draw;
mod geometry;
mod icons;
mod image;
mod magic;
mod mask;
mod privacy;
mod query;
mod selection;
mod settings;
mod text;
mod toolbar;
mod update;
mod win;

fn main() {
    if let Err(error) = win::app::run() {
        win::app::report(&error.to_string());
        std::process::exit(1);
    }
}
