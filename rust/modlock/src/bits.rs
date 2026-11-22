//! bits names the game's input and collision bits.

/// Buttons names input bits for block_input, watch_input and Player::press.
/// A mod joins them with |, such as Buttons::ATTACK | Buttons::JUMP.
pub struct Buttons;

impl Buttons {
    /// ATTACK is the primary fire button.
    pub const ATTACK: u64 = 1;
    /// JUMP is the jump key, Space by default (IN_MANTLE). Blocking it stops
    /// the native jump and mantle; the jumping client still predicts the jump
    /// and is corrected.
    pub const JUMP: u64 = 1 << 48;
    /// CROUCH is the crouch key, Ctrl by default.
    pub const CROUCH: u64 = 1 << 2;
    /// DASH is the dash key, Shift by default.
    pub const DASH: u64 = 1 << 16;
    /// USE is the interact key.
    pub const USE: u64 = 1 << 5;
    /// ALT_FIRE is the zoom and alternate fire button, right mouse by default.
    pub const ALT_FIRE: u64 = 1 << 11;
    /// RELOAD is the reload key.
    pub const RELOAD: u64 = 1 << 13;
    /// MELEE is the light melee key, Q by default.
    pub const MELEE: u64 = 1 << 32;
    /// ABILITY1 to ABILITY3 are the hero's first three ability keys.
    pub const ABILITY1: u64 = 1 << 33;
    pub const ABILITY2: u64 = 1 << 34;
    pub const ABILITY3: u64 = 1 << 35;
    /// ULTIMATE is the hero's ultimate ability key, the fourth.
    pub const ULTIMATE: u64 = 1 << 36;
    /// PARRY is the melee parry button.
    pub const PARRY: u64 = 1 << 42;
    /// SPEC_NEXT to SPEC_TOGGLE_TEAM are the buttons a player's client sends
    /// in place of the others while they spectate, with no hero of their own
    /// in play: SPEC_NEXT and SPEC_PREV cycle the watched player, SPEC_MODE
    /// changes the camera mode, and SPEC_TOGGLE_TEAM is the jump key, Space
    /// by default.
    pub const SPEC_NEXT: u64 = 1 << 49;
    pub const SPEC_PREV: u64 = 1 << 50;
    pub const SPEC_MODE: u64 = 1 << 51;
    pub const SPEC_TOGGLE_TEAM: u64 = 1 << 52;
}

/// Layers names collision layers for a trace.
pub struct Layers;

impl Layers {
    /// SOLID is the world and solid objects.
    pub const SOLID: u64 = 1 << 0;
    /// HITBOX is the hit boxes of heroes and units.
    pub const HITBOX: u64 = 1 << 1;
    /// TRIGGER is invisible volumes that react to touch.
    pub const TRIGGER: u64 = 1 << 2;
    /// PLAYER_CLIP is walls that only stop heroes.
    pub const PLAYER_CLIP: u64 = 1 << 4;
    /// PLAYER is player-controlled heroes.
    pub const PLAYER: u64 = 1 << 18;
    /// NPC is units that are not players.
    pub const NPC: u64 = 1 << 19;
    /// STATIC_LEVEL is the map's built geometry, foliage included.
    pub const STATIC_LEVEL: u64 = 1 << 30;
    /// HERO, TROOPER and BUILDING are those units' bodies.
    pub const HERO: u64 = 1 << 37;
    pub const TROOPER: u64 = 1 << 38;
    pub const BUILDING: u64 = 1 << 40;
}
