use crate::input::Chord;
use serde::Deserialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub broad_override: bool,
    pub focus_follows_cursor: bool,
    pub remove_caption_buttons: bool,
    /// Accepted for compatibility with old configs. Minimized windows now stay
    /// minimized until explicitly restored; automatic restoration is retired.
    pub restore_minimized: bool,
    pub native_workspaces: bool,
    pub bindings: Vec<Binding>,
    pub ignore_classes: Vec<String>,
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub executable: Option<String>,
    pub class: Option<String>,
    pub title_contains: Option<String>,
    pub mode: RuleMode,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleMode {
    Ignore,
    Floating,
    Tile,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub chord: String,
    pub action: Action,
}
impl<'de> Deserialize<'de> for Binding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            chord: String,
            #[serde(flatten)]
            fields: toml::map::Map<String, toml::Value>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let kind = raw
            .fields
            .get("action")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| serde::de::Error::custom("binding needs an action string"))?;
        let allowed: &[&str] = match kind {
            "workspace" | "move_workspace" => &["action", "slot"],
            "focus" | "move" | "resize" => &["action", "direction"],
            "launch" => &["action", "executable", "args"],
            _ => &["action"],
        };
        if let Some(key) = raw
            .fields
            .keys()
            .find(|key| !allowed.contains(&key.as_str()))
        {
            return Err(serde::de::Error::custom(format!(
                "unknown binding field: {key}"
            )));
        }
        let action = toml::Value::Table(raw.fields)
            .try_into()
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            chord: raw.chord,
            action,
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Launcher,
    Close,
    Minimize,
    RestoreMinimized,
    ToggleFloating,
    Fullscreen,
    Pause,
    Reload,
    Exit,
    Workspace {
        slot: u32,
    },
    MoveWorkspace {
        slot: u32,
    },
    Focus {
        direction: Direction,
    },
    Move {
        direction: Direction,
    },
    Resize {
        direction: Direction,
    },
    Launch {
        executable: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Default for Config {
    fn default() -> Self {
        let mut bindings = vec![];
        for (key, action) in [
            ("Space", Action::Launcher),
            ("X", Action::Close),
            ("V", Action::ToggleFloating),
            ("M", Action::Pause),
            ("I", Action::Minimize),
            ("Shift+I", Action::RestoreMinimized),
            ("Enter", Action::Fullscreen),
            ("Ctrl+R", Action::Reload),
        ] {
            bindings.push(Binding {
                chord: format!("SUPER+{key}"),
                action,
            });
        }
        for slot in 1..=10 {
            let digit = slot % 10;
            bindings.push(Binding {
                chord: format!("SUPER+{digit}"),
                action: Action::Workspace { slot },
            });
            bindings.push(Binding {
                chord: format!("SUPER+Shift+{digit}"),
                action: Action::MoveWorkspace { slot },
            });
        }
        for (key, direction) in [
            ("Left", Direction::Left),
            ("Right", Direction::Right),
            ("Up", Direction::Up),
            ("Down", Direction::Down),
        ] {
            bindings.push(Binding {
                chord: format!("SUPER+{key}"),
                action: Action::Focus { direction },
            });
            bindings.push(Binding {
                chord: format!("SUPER+Shift+{key}"),
                action: Action::Move { direction },
            });
            bindings.push(Binding {
                chord: format!("SUPER+Ctrl+{key}"),
                action: Action::Resize { direction },
            });
        }
        Self {
            broad_override: false,
            focus_follows_cursor: true,
            remove_caption_buttons: true,
            restore_minimized: false,
            native_workspaces: true,
            bindings,
            ignore_classes: vec![],
            rules: vec![],
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        for rule in &self.rules {
            let fields = [&rule.executable, &rule.class, &rule.title_contains];
            if fields.iter().all(|f| f.is_none())
                || fields
                    .iter()
                    .any(|f| f.as_ref().is_some_and(|v| v.trim().is_empty()))
            {
                return Err("each rule needs at least one nonempty executable, class, or title_contains matcher".into());
            }
        }
        let mut seen = HashSet::new();
        for binding in &self.bindings {
            let chord = Chord::parse(&binding.chord)?;
            if chord == Chord::parse("SUPER+M")? && binding.action != Action::Pause {
                return Err("SUPER+M is reserved for pausing tiling".into());
            }
            if !seen.insert(chord) {
                return Err(format!("duplicate shortcut: {}", binding.chord));
            }
            match &binding.action {
                Action::Workspace { slot } | Action::MoveWorkspace { slot }
                    if !(1..=10).contains(slot) =>
                {
                    return Err("workspace slot must be 1..10".into());
                }
                Action::Launch { executable, .. } if executable.trim().is_empty() => {
                    return Err("launch executable cannot be empty".into());
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub fn default_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Tauiler/config.toml")
}
pub fn load(path: &Path) -> Result<Config, Box<dyn std::error::Error>> {
    let config = match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => return Err(e.into()),
    };
    config
        .validate()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_valid() {
        Config::default().validate().unwrap();
    }
    #[test]
    fn pause_and_reserved_toggle_are_validated() {
        let pause: Config =
            toml::from_str("[[bindings]]\nchord='SUPER+P'\naction='pause'").unwrap();
        pause.validate().unwrap();
        let c: Config = toml::from_str("[[bindings]]\nchord='SUPER+M'\naction='close'").unwrap();
        assert!(c.validate().is_err());
    }
    #[test]
    fn invalid_config_rejected() {
        let mut c = Config::default();
        c.bindings.push(c.bindings[0].clone());
        assert!(c.validate().is_err());
        let mut c = Config::default();
        c.bindings[0].action = Action::Workspace { slot: 11 };
        assert!(c.validate().is_err());
        assert!(toml::from_str::<Config>("typo = true").is_err());
    }
    #[test]
    fn structured_launch_args() {
        let c:Config=toml::from_str("[[bindings]]\nchord='SUPER+E'\naction='launch'\nexecutable='explorer.exe'\nargs=['C:\\Program Files']").unwrap();
        c.validate().unwrap();
        assert!(matches!(&c.bindings[0].action,Action::Launch{args,..} if args.len()==1));
    }
    #[test]
    fn rules_need_matchers_and_reject_unknown_binding_fields() {
        let c: Config = toml::from_str("[[rules]]\nmode='floating'").unwrap();
        assert!(c.validate().is_err());
        let c: Config =
            toml::from_str("[[rules]]\nexecutable='notepad.exe'\nmode='floating'").unwrap();
        c.validate().unwrap();
        assert!(
            toml::from_str::<Config>("[[bindings]]\nchord='SUPER+X'\naction='close'\ntypo=true")
                .is_err()
        );
    }
}
