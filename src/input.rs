use crate::config::{Action, Config};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Chord {
    pub key: u32,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}
impl Chord {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut c = Self {
            key: 0,
            shift: false,
            ctrl: false,
            alt: false,
        };
        let mut super_key = false;
        for part in text.split('+').map(|p| p.trim().to_ascii_uppercase()) {
            match part.as_str() {
                "SUPER" | "WIN" if !super_key => super_key = true,
                "SHIFT" if !c.shift => c.shift = true,
                "CTRL" if !c.ctrl => c.ctrl = true,
                "ALT" if !c.alt => c.alt = true,
                key if c.key == 0 => {
                    c.key = match key {
                        "SPACE" => 0x20,
                        "ENTER" => 0x0d,
                        "LEFT" => 0x25,
                        "UP" => 0x26,
                        "RIGHT" => 0x27,
                        "DOWN" => 0x28,
                        "TAB" => 9,
                        "ESCAPE" => 0x1b,
                        s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() => {
                            s.as_bytes()[0] as u32
                        }
                        s if s.starts_with('F') => s[1..]
                            .parse::<u32>()
                            .ok()
                            .filter(|n| (1..=24).contains(n))
                            .map(|n| 0x6f + n)
                            .ok_or_else(|| format!("unknown key: {s}"))?,
                        _ => return Err(format!("unknown/duplicate key or modifier in {text}")),
                    }
                }
                _ => return Err(format!("invalid shortcut: {text}")),
            }
        }
        if !super_key || c.key == 0 {
            return Err(format!("shortcut must include SUPER and one key: {text}"));
        }
        if c.key == b'L' as u32 || (c.ctrl && c.key == 0x73) {
            return Err(format!("protected Windows shortcut: {text}"));
        }
        Ok(c)
    }
}

pub struct Input {
    down: [bool; 256],
    consumed: [bool; 256],
    handled_win: bool,
    broad: bool,
    bindings: HashMap<Chord, Action>,
}
#[derive(Default)]
pub struct Decision {
    pub consume: bool,
    pub mask_start: bool,
    pub action: Option<Action>,
}
impl Input {
    pub fn new(config: &Config) -> Self {
        let mut bindings: HashMap<Chord, Action> = config
            .bindings
            .iter()
            .map(|b| {
                (
                    Chord::parse(&b.chord).expect("validated configuration"),
                    b.action.clone(),
                )
            })
            .collect();
        bindings.insert(Chord::parse("SUPER+M").unwrap(), Action::Pause);
        Self {
            down: [false; 256],
            consumed: [false; 256],
            handled_win: false,
            broad: config.broad_override,
            bindings,
        }
    }
    pub fn super_down(&self) -> bool {
        self.down[0x5b] || self.down[0x5c]
    }
    pub fn shift_down(&self) -> bool {
        self.down[0x10] || self.down[0xa0] || self.down[0xa1]
    }
    pub fn mark_super_handled(&mut self) {
        self.handled_win = true;
    }
    pub fn update_config(&mut self, config: &Config) {
        let replacement = Self::new(config);
        self.broad = replacement.broad;
        self.bindings = replacement.bindings;
    }
    pub fn event(&mut self, key: u32, pressed: bool) -> Decision {
        if key >= 256 {
            return Decision::default();
        }
        let index = key as usize;
        let repeat = self.down[index];
        let win = matches!(key, 0x5b | 0x5c);
        self.down[index] = pressed;
        if !pressed {
            let consume = std::mem::take(&mut self.consumed[index]);
            let mask_start = win && !self.super_down() && std::mem::take(&mut self.handled_win);
            return Decision {
                consume,
                mask_start,
                action: None,
            };
        }
        if repeat {
            return Decision {
                consume: self.consumed[index],
                action: if self.consumed[index] && self.super_down() {
                    let chord = Chord {
                        key,
                        shift: self.shift_down(),
                        ctrl: self.down[0x11] || self.down[0xa2] || self.down[0xa3],
                        alt: self.down[0x12] || self.down[0xa4] || self.down[0xa5],
                    };
                    self.bindings
                        .get(&chord)
                        .filter(|a| matches!(a, Action::Resize { .. }))
                        .cloned()
                } else {
                    None
                },
                ..Default::default()
            };
        }
        if win {
            self.consumed[index] = self.broad;
            return Decision {
                consume: self.broad,
                ..Default::default()
            };
        }
        if self.super_down() {
            let chord = Chord {
                key,
                shift: self.shift_down(),
                ctrl: self.down[0x11] || self.down[0xa2] || self.down[0xa3],
                alt: self.down[0x12] || self.down[0xa4] || self.down[0xa5],
            };
            let action = self.bindings.get(&chord).cloned();
            let consume = action.is_some() || (self.broad && key != b'L' as u32);
            self.consumed[index] = consume;
            if consume {
                self.handled_win = true;
            }
            return Decision {
                consume,
                action,
                ..Default::default()
            };
        }
        Decision::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chord_consumes_both_edges_and_masks_start_without_repeating() {
        let mut i = Input::new(&Config::default());
        assert!(!i.event(0x5b, true).consume);
        assert!(i.event(b'X' as u32, true).action.is_some());
        assert!(i.event(b'X' as u32, true).action.is_none());
        assert!(i.event(b'X' as u32, false).consume);
        assert!(i.event(0x5b, false).mask_start);
        assert!(!i.event(b'X' as u32, true).consume);
    }
    #[test]
    fn broad_mode_passes_normal_typing_and_reload_releases_held_chords() {
        let c = Config {
            broad_override: true,
            ..Config::default()
        };
        let mut i = Input::new(&c);
        assert!(i.event(0x5b, true).consume);
        assert!(i.event(b'Z' as u32, true).consume);
        i.update_config(&Config::default());
        assert!(i.event(b'Z' as u32, false).consume);
        assert!(i.event(0x5b, false).consume);
        assert!(!i.event(b'Z' as u32, true).consume);
    }
    #[test]
    fn reload_keeps_release_tracking() {
        let mut i = Input::new(&Config::default());
        i.event(0x5b, true);
        i.event(b'X' as u32, true);
        i.update_config(&Config {
            bindings: vec![],
            ..Config::default()
        });
        assert!(i.event(b'X' as u32, false).consume);
    }
    #[test]
    fn requested_shortcuts_and_pause_are_available_in_custom_maps() {
        let mut i = Input::new(&Config::default());
        i.event(0x5b, true);
        assert_eq!(
            i.event(b'V' as u32, true).action,
            Some(Action::ToggleFloating)
        );
        i.event(b'V' as u32, false);
        assert!(i.event(b'F' as u32, true).action.is_none());
        i.event(b'F' as u32, false);
        i.update_config(&Config {
            bindings: vec![],
            ..Config::default()
        });
        assert_eq!(i.event(b'M' as u32, true).action, Some(Action::Pause));
    }
    #[test]
    fn resize_repeats_but_one_shot_commands_do_not() {
        let mut i = Input::new(&Config::default());
        i.event(0x5b, true);
        i.event(0xa2, true);
        let first = i.event(0x27, true);
        assert!(matches!(first.action, Some(Action::Resize { .. })));
        assert_eq!(i.event(0x27, true).action, first.action);
        i.event(0x27, false);
        i.event(0xa2, false);
        i.event(b'M' as u32, true);
        assert!(i.event(b'M' as u32, true).action.is_none());
    }
    #[test]
    fn minimize_and_monitor_restore_use_requested_chords() {
        let mut input = Input::new(&Config::default());
        input.event(0x5b, true);
        assert_eq!(
            input.event(b'I' as u32, true).action,
            Some(Action::Minimize)
        );
        assert!(input.event(b'I' as u32, true).action.is_none());
        input.event(b'I' as u32, false);
        input.event(0xa0, true);
        assert_eq!(
            input.event(b'I' as u32, true).action,
            Some(Action::RestoreMinimized)
        );
    }
    #[test]
    fn super_mouse_masks_start_and_win_release_before_key_release_is_safe() {
        let mut i = Input::new(&Config::default());
        i.event(0x5b, true);
        i.mark_super_handled();
        assert!(i.event(0x5b, false).mask_start);
        i.event(0x5b, true);
        i.event(b'X' as u32, true);
        assert!(i.event(0x5b, false).mask_start);
        assert!(i.event(b'X' as u32, false).consume);
    }
}
