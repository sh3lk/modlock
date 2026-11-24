//! modlock writes Modlock mods in Rust. A mod is one value that implements
//! [`Mod`]; modlock-host compiles the module to a sandbox inside the game
//! server and calls the mod's events on the server's frame thread, one at a
//! time.
//!
//! ```ignore
//! use modlock::prelude::*;
//!
//! #[derive(Default)]
//! struct Hello;
//!
//! impl Mod for Hello {
//!     fn start(&mut self, _event: &StartEvent) {
//!         command("hello", Self::hello);
//!     }
//! }
//!
//! impl Hello {
//!     fn hello(&mut self, player: Player) {
//!         player.chat("Hello from Rust!");
//!     }
//! }
//!
//! modlock::run!(Hello::default());
//! ```
//!
//! The calls into the game, their types and the events are generated from the
//! schema in host.gen.rs.

// The macros name the library as a mod does, which the library itself can
// then do too.
extern crate self as modlock;

pub mod camera;
pub mod console;
pub mod curve;
pub mod dropper;
pub mod entity;
pub mod format;
pub mod frame_clock;
pub mod host;
pub mod hud;
pub mod persist;
pub mod racegate;
pub mod rotation;
pub mod theme;
pub mod ui;
pub mod wire;

mod bits;
mod bystander;
mod cast;
mod ground;
mod heroes;
mod hook;
mod items;
mod pawns;
mod player;
mod probe_run;
mod puppet;
mod replayed_damage;
mod restore;
mod runtime;
mod scores;
mod seating;
mod services;
mod spot;
mod strays;
mod vector;

#[allow(clippy::all)]
#[rustfmt::skip]
#[path = "host.gen.rs"]
#[macro_use]
mod generated;

#[cfg(test)]
mod helper_tests;
#[cfg(test)]
mod tests;

pub use bits::{Buttons, Layers};
pub use bystander::{
    hold_bystander, spectate_from, Spectating, BYSTANDER_STATES, PARKED_STATES, PARK_HEIGHT,
};
pub use camera::{
    camera_pulse, camera_shake, entity_camera, play_camera, set_orbit_camera, CameraMove,
    EntityCamera, EntityCameraOptions, ENTITY_CAMERA_SETTLE_SECONDS,
};
pub use cast::{Bots, Cast, HeroPick, Want};
pub use generated::*;
pub use ground::{ground, Ground};
pub use heroes::{find_hero, hero_named, Hero, HEROES};
pub use hook::{Hook, HookRead, HOOK_ABILITY, HOOK_UPGRADES};
pub use items::{item, item_id, Item, Slot, ITEMS};
pub use pawns::set_pawn_cache;
pub use player::Player;
pub use probe_run::{Check, ProbeRun};
pub use puppet::{Pose, Puppet, PuppetOptions, PuppetState, Track, GHOST_STATES, TICKS_PER_SECOND};
pub use replayed_damage::{replay_damage, ReplayedHit};
pub use restore::{restore_hero, HeroSnapshot};
pub use runtime::{
    after, command, connection_of_steam_id, every, modifier_event, players, sequence,
    CommandHandler, Timer,
};
pub use scores::{bind_score_player, read_board, report_match, report_score, ScoreRefusal};
pub use seating::{set_respawn_delay, Seating};
pub use services::{call_service_later, serve, ServiceError};
pub use spot::{
    decode_spot, encode_spot, load_spot, LoadedSpot, Placed, Spot, SpotObject, TRIGGERS,
};
pub use strays::{Stragglers, Strays, STRAGGLER_CONNECT_SECONDS, STRAY_GRACE_SECONDS};
pub use ui::{chat_shown, show_chat_lines, ChatLines, ChatSpeaker, SpeakerLook};

/// prelude holds what a mod names: the calls, their types and the events.
pub mod prelude {
    pub use crate::bits::{Buttons, Layers};
    pub use crate::bystander::{hold_bystander, spectate_from, Spectating};
    pub use crate::camera::{
        camera_pulse, camera_shake, entity_camera, play_camera, set_orbit_camera, CameraMove,
        EntityCamera, EntityCameraOptions,
    };
    pub use crate::cast::{Bots, Cast, HeroPick, Want};
    pub use crate::generated::*;
    pub use crate::ground::{ground, Ground};
    pub use crate::heroes::{find_hero, hero_named, Hero, HEROES};
    pub use crate::hook::{Hook, HookRead};
    pub use crate::items::{item, item_id, Item, Slot, ITEMS};
    pub use crate::log;
    pub use crate::pawns::set_pawn_cache;
    pub use crate::persist::{
        self, PersistChange, PersistOptions, PersistRead, PersistRecord, PersistValue,
    };
    pub use crate::player::Player;
    pub use crate::probe_run::{Check, ProbeRun};
    pub use crate::puppet::{Puppet, PuppetOptions, PuppetState, Track};
    pub use crate::replayed_damage::{replay_damage, ReplayedHit};
    pub use crate::restore::{restore_hero, HeroSnapshot};
    pub use crate::runtime::{
        after, command, connection_of_steam_id, every, modifier_event, players, sequence, Timer,
    };
    pub use crate::scores::{
        bind_score_player, read_board, report_match, report_score, ScoreRefusal,
    };
    pub use crate::seating::{set_respawn_delay, Seating};
    pub use crate::services::{call_service_later, serve, ServiceError};
    pub use crate::spot::{
        decode_spot, encode_spot, load_spot, LoadedSpot, Placed, Spot, SpotObject,
    };
    pub use crate::strays::{Stragglers, Strays};
}

/// log! writes one line to the server log under the mod's name, formatted as
/// format! formats it.
#[macro_export]
macro_rules! log {
    ($($argument:tt)*) => {
        {
            $crate::log_line(::std::format!($($argument)*));
        }
    };
}

/// run! makes the value of its expression the module's mod: it exports
/// modlock_event, through which the game delivers each event, and evaluates
/// the expression on the first one. It asks the game for each event the mod
/// has a trait for, such as OnKill, and for no other.
#[macro_export]
macro_rules! run {
    ($make:expr) => {
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn modlock_event(size: u32) -> u64 {
            let make = || $make;
            let events = $crate::events!(make);
            $crate::host::event(size, make, &events)
        }
    };
}
