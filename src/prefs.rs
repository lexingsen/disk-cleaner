use std::fs;

use serde::{Deserialize, Serialize};

use crate::fmt;
use crate::history::data_dir;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance::System
    }
}

impl Appearance {
    pub fn title(self) -> String {
        match self {
            Appearance::System => fmt::t("跟随系统", "System"),
            Appearance::Light => fmt::t("浅色", "Light"),
            Appearance::Dark => fmt::t("深色", "Dark"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LanguagePref {
    System,
    Zh,
    En,
}

impl Default for LanguagePref {
    fn default() -> Self {
        LanguagePref::System
    }
}

impl LanguagePref {
    pub fn title(self) -> &'static str {
        match self {
            LanguagePref::System => "跟随系统 / System",
            LanguagePref::Zh => "简体中文",
            LanguagePref::En => "English",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Default)]
pub struct Prefs {
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub language: LanguagePref,
}

fn path() -> std::path::PathBuf {
    data_dir().join("settings.json")
}

pub fn load() -> Prefs {
    fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(p: &Prefs) {
    if let Ok(s) = serde_json::to_string_pretty(p) {
        let _ = fs::write(path(), s);
    }
}

pub fn apply_language(p: LanguagePref) {
    match p {
        LanguagePref::System => fmt::detect_language(),
        LanguagePref::Zh => fmt::set_language(true),
        LanguagePref::En => fmt::set_language(false),
    }
}

pub fn system_is_dark() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize") {
            if let Ok(v) = key.get_value::<u32, _>("AppsUseLightTheme") {
                return v == 0;
            }
        }
        false
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn is_dark(p: Appearance) -> bool {
    match p {
        Appearance::Dark => true,
        Appearance::Light => false,
        Appearance::System => system_is_dark(),
    }
}
