//! Editable, persistable keyboard shortcuts.
//!
//! Each [`Action`] has a [`Hotkey`]. Hotkeys are stored as modifier flags plus
//! an egui key *name* (e.g. "G"), so they serialize cleanly and reconstruct into
//! an [`egui::KeyboardShortcut`] for matching.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};

/// A user-triggerable action that can carry a keyboard shortcut.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    DetectGame,
    Attach,
    Save,
    Load,
    ToggleMemory,
    FirstScan,
    NextScan,
    ResetScan,
}

impl Action {
    pub const ALL: [Action; 8] = [
        Action::DetectGame,
        Action::Attach,
        Action::Save,
        Action::Load,
        Action::ToggleMemory,
        Action::FirstScan,
        Action::NextScan,
        Action::ResetScan,
    ];
}

/// A serializable keyboard shortcut: modifiers + an egui key name.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// egui [`Key`] name, e.g. "G", "F1", "Enter".
    pub key: String,
}

impl Hotkey {
    fn ctrl(key: &str) -> Self {
        Hotkey {
            ctrl: true,
            shift: false,
            alt: false,
            key: key.to_owned(),
        }
    }

    /// Build from a live modifier state and pressed key.
    pub fn from_parts(mods: Modifiers, key: Key) -> Self {
        Hotkey {
            // `command` mirrors ctrl on Windows/Linux and is Cmd on macOS.
            ctrl: mods.ctrl || mods.command,
            shift: mods.shift,
            alt: mods.alt,
            key: key.name().to_owned(),
        }
    }

    /// Reconstruct an egui shortcut, or `None` if the key name is unknown.
    pub fn to_shortcut(&self) -> Option<KeyboardShortcut> {
        let key = Key::from_name(&self.key)?;
        let mut mods = Modifiers::NONE;
        if self.ctrl {
            mods = mods | Modifiers::CTRL;
        }
        if self.shift {
            mods = mods | Modifiers::SHIFT;
        }
        if self.alt {
            mods = mods | Modifiers::ALT;
        }
        Some(KeyboardShortcut::new(mods, key))
    }

    /// Human-readable form, e.g. "Ctrl+G".
    pub fn label(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push_str(&self.key);
        s
    }
}

/// The full set of bindings. All default to Ctrl + a letter.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBindings {
    pub detect_game: Hotkey,
    pub attach: Hotkey,
    pub save: Hotkey,
    pub load: Hotkey,
    pub toggle_memory: Hotkey,
    pub first_scan: Hotkey,
    pub next_scan: Hotkey,
    pub reset_scan: Hotkey,
}

impl Default for KeyBindings {
    fn default() -> Self {
        KeyBindings {
            detect_game: Hotkey::ctrl("G"),
            attach: Hotkey::ctrl("B"),
            save: Hotkey::ctrl("S"),
            load: Hotkey::ctrl("O"),
            toggle_memory: Hotkey::ctrl("M"),
            first_scan: Hotkey::ctrl("J"),
            next_scan: Hotkey::ctrl("N"),
            reset_scan: Hotkey::ctrl("R"),
        }
    }
}

impl KeyBindings {
    pub fn get(&self, a: Action) -> &Hotkey {
        match a {
            Action::DetectGame => &self.detect_game,
            Action::Attach => &self.attach,
            Action::Save => &self.save,
            Action::Load => &self.load,
            Action::ToggleMemory => &self.toggle_memory,
            Action::FirstScan => &self.first_scan,
            Action::NextScan => &self.next_scan,
            Action::ResetScan => &self.reset_scan,
        }
    }

    pub fn set(&mut self, a: Action, hk: Hotkey) {
        let slot = match a {
            Action::DetectGame => &mut self.detect_game,
            Action::Attach => &mut self.attach,
            Action::Save => &mut self.save,
            Action::Load => &mut self.load,
            Action::ToggleMemory => &mut self.toggle_memory,
            Action::FirstScan => &mut self.first_scan,
            Action::NextScan => &mut self.next_scan,
            Action::ResetScan => &mut self.reset_scan,
        };
        *slot = hk;
    }
}
