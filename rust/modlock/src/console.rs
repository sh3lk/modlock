//! console sets the server's console variables and runs its console
//! commands, through functions modlock-entitygen generates from the game's
//! console:
//!
//! ```ignore
//! use modlock::console;
//!
//! console::sv_cheats().set(true);
//! console::changelevel().run(&["street_test"]);
//! ```
//!
//! Each function has the name of its variable or command, as the game has
//! it.
//!
//! Each runs one line at the server console through server_command, which
//! may set a development-only or cheat-protected variable. A variable of a
//! type other than a scalar or a string, such as a vector, has no function
//! here; set it with server_command.
#![allow(non_snake_case)]

use std::marker::PhantomData;

use crate::generated::server_command;

/// Value is a type of console variable.
pub trait Value {
    /// format returns the value as the console reads it: a bool as 1 or 0
    /// and a string in quotes.
    fn format(&self) -> String;
}

impl Value for bool {
    fn format(&self) -> String {
        if *self { "1" } else { "0" }.to_owned()
    }
}

impl Value for String {
    fn format(&self) -> String {
        format!("\"{self}\"")
    }
}

/// number implements Value for the numeric types.
macro_rules! number {
    ($($type:ty),*) => {$(
        impl Value for $type {
            fn format(&self) -> String {
                self.to_string()
            }
        }
    )*};
}

number!(i8, u8, i16, u16, i32, u32, i64, u64, f32, f64);

/// Variable is one console variable, whose value is a T.
pub struct Variable<T> {
    /// name is the variable's name, such as sv_cheats.
    pub name: &'static str,
    holds: PhantomData<fn(T)>,
}

impl<T: Value> Variable<T> {
    /// new returns the Variable of name.
    pub const fn new(name: &'static str) -> Self {
        Variable {
            name,
            holds: PhantomData,
        }
    }

    /// set sets the variable to value and returns whether the host took it.
    pub fn set(&self, value: impl Into<T>) -> bool {
        server_command(format!("{} {}", self.name, value.into().format()))
    }

    /// to returns the setting of the variable to value, for apply.
    pub fn to(&self, value: impl Into<T>) -> Setting {
        Setting {
            name: self.name,
            value: value.into().format(),
        }
    }
}

/// Command is one console command.
pub struct Command {
    /// name is the command's name, such as changelevel.
    pub name: &'static str,
}

impl Command {
    /// new returns the Command of name.
    pub const fn new(name: &'static str) -> Self {
        Command { name }
    }

    /// run runs the command with arguments, each one word of the line, and
    /// returns whether the host took it.
    pub fn run(&self, arguments: &[&str]) -> bool {
        let mut line = self.name.to_owned();
        for argument in arguments {
            line.push(' ');
            line.push_str(argument);
        }
        server_command(line)
    }
}

/// Setting is one console variable with the value apply sets it to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting {
    /// name is the variable's name, such as sv_cheats.
    pub name: &'static str,
    /// value is the value as the console reads it, such as 1 for true.
    pub value: String,
}

/// apply sets each variable in settings, in order, and returns the names the
/// host refused, so a mod can log them and play on. Each setting is one host
/// call: apply settings when the mod starts or a world loads, not every
/// frame.
pub fn apply(settings: &[Setting]) -> Vec<&'static str> {
    settings
        .iter()
        .filter(|setting| !server_command(format!("{} {}", setting.name, setting.value)))
        .map(|setting| setting.name)
        .collect()
}

/// arena returns the settings that empty a map for a mode of its own. The
/// server runs frames with no one connected, which it otherwise skips, and
/// no solo bot match, King of the Hill, troopers, jungle camps or practice
/// bots start, nor does a player start on a zipline. A mode applies them
/// before its own settings, which may override them:
///
/// ```ignore
/// let mut settings = console::arena();
/// settings.push(console::citadel_allow_duplicate_heroes().to(true));
/// console::apply(&settings);
/// ```
pub fn arena() -> Vec<Setting> {
    vec![
        sv_hibernate_when_empty().to(false),
        citadel_solo_bot_match().to(false),
        citadel_koth_enabled().to(false),
        citadel_trooper_spawn_enabled().to(false),
        citadel_neutral_spawn_enabled().to(false),
        citadel_spawn_practice_bots().to(false),
        citadel_start_players_on_zipline().to(false),
    ]
}

#[rustfmt::skip]
#[path = "console/variables.gen.rs"]
mod variables;
#[rustfmt::skip]
#[path = "console/commands.gen.rs"]
mod commands;

pub use commands::*;
pub use variables::*;
