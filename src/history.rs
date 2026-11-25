use std::collections::{BTreeMap, HashMap};
use std::fs;

use serde::{Deserialize, Serialize};

use crate::fmt::{today_key, now_ts};
use crate::model::{Report, ThingKind};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DayPoint {
    pub day: String,
    pub kind: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThingPoint {
    pub day: String,
    pub thing_id: String,
    pub kind: String,
    pub title: String,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct ThingChange {
    pub title: String,
    pub kind: String,
    pub bytes: Option<u64>,
    pub change: i64,
    pub before: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct DayDetail {
    pub day: String,
    pub previous_day: Option<String>,
    pub things: Vec<ThingChange>,
}

#[derive(Serialize, Deserialize, Default)]
struct HistoryFile {
    days: Vec<DayPoint>,
    things: Vec<ThingPoint>,
}

pub fn data_dir() -> std::path::PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut d = base.clone();
    d.push("disk-cleaner");
    // Carry over the report/history/settings from the pre-rename folder once.
    if !d.exists() {
        let mut old = base;
        old.push("SuckMyDisk");
        if old.is_dir() {
            let _ = fs::create_dir_all(&d);
            for name in ["report.json", "history.json", "settings.json"] {
                let from = old.join(name);
                if from.exists() {
                    let _ = fs::copy(&from, d.join(name));
                }
            }
        }
    }
    let _ = fs::create_dir_all(&d);
    d
}

fn report_path() -> std::path::PathBuf {
    data_dir().join("report.json")
}

fn history_path() -> std::path::PathBuf {
    data_dir().join("history.json")
}

pub fn load_report() -> Option<Report> {
    let data = fs::read_to_string(report_path()).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_report(report: &Report) {
    if let Ok(s) = serde_json::to_string(report) {
        let _ = fs::write(report_path(), s);
    }
}

pub fn load_history() -> (Vec<DayPoint>, Vec<ThingPoint>) {
    let Ok(data) = fs::read_to_string(history_path()) else {
        return (vec![], vec![]);
    };
    let f: HistoryFile = serde_json::from_str(&data).unwrap_or_default();
    (f.days, f.things)
}

pub fn record_history(report: &Report, free: u64) {
    let (mut days, mut things) = load_history();
    let day = today_key(now_ts());
    days.retain(|p| p.day != day);
    things.retain(|p| p.day != day);
    let kinds = [
        ThingKind::Project,
        ThingKind::Media,
        ThingKind::DevCache,
        ThingKind::Model,
        ThingKind::AppData,
    ];
    for k in kinds {
        let total: u64 = report
            .things
            .iter()
            .filter(|t| t.kind == k)
            .map(|t| t.bytes)
            .sum();
        if total > 0 {
            days.push(DayPoint {
                day: day.clone(),
                kind: kind_key(k).to_string(),
                bytes: total,
            });
        }
        for t in report.things.iter().filter(|t| t.kind == k) {
            things.push(ThingPoint {
                day: day.clone(),
                thing_id: t.id.clone(),
                kind: kind_key(k).to_string(),
                title: t.title.clone(),
                bytes: t.bytes,
            });
        }
    }
    days.push(DayPoint {
        day: day.clone(),
        kind: "free".into(),
        bytes: free,
    });
    // Keep about half a year.
    let cutoff = today_key(now_ts() - 190 * 86400);
    days.retain(|p| p.day >= cutoff);
    things.retain(|p| p.day >= cutoff);
    let f = HistoryFile { days, things };
    if let Ok(s) = serde_json::to_string(&f) {
        let _ = fs::write(history_path(), s);
    }
}

pub fn kind_key(k: ThingKind) -> &'static str {
    match k {
        ThingKind::Project => "project",
        ThingKind::Media => "media",
        ThingKind::DevCache => "devCache",
        ThingKind::Model => "model",
        ThingKind::App => "app",
        ThingKind::AppData => "appData",
        ThingKind::System => "system",
        ThingKind::LargeFile => "largeFile",
        ThingKind::Browser => "browser",
    }
}

pub fn kind_from_key(s: &str) -> Option<ThingKind> {
    Some(match s {
        "project" => ThingKind::Project,
        "media" => ThingKind::Media,
        "devCache" => ThingKind::DevCache,
        "model" => ThingKind::Model,
        "app" => ThingKind::App,
        "appData" => ThingKind::AppData,
        "system" => ThingKind::System,
        "largeFile" => ThingKind::LargeFile,
        "browser" => ThingKind::Browser,
        _ => return None,
    })
}

pub fn detail_for(day: &str, points: &[ThingPoint]) -> DayDetail {
    let mut by_day: BTreeMap<String, HashMap<String, (String, String, u64)>> = BTreeMap::new();
    for p in points {
        by_day.entry(p.day.clone()).or_default().insert(
            p.thing_id.clone(),
            (p.kind.clone(), p.title.clone(), p.bytes),
        );
    }
    let Some(today) = by_day.get(day) else {
        return DayDetail {
            day: day.to_string(),
            ..Default::default()
        };
    };
    let previous = by_day
        .keys()
        .filter(|d| d.as_str() < day)
        .next_back()
        .cloned();
    let prev = previous.as_ref().and_then(|d| by_day.get(d));
    let mut changes = vec![];
    for (id, (kind, title, bytes)) in today {
        let before = prev.and_then(|m| m.get(id)).map(|(_, _, b)| *b);
        let change = match before {
            Some(b) => *bytes as i64 - b as i64,
            None => 0,
        };
        changes.push(ThingChange {
            title: title.clone(),
            kind: kind.clone(),
            bytes: Some(*bytes),
            change,
            before,
        });
    }
    if let Some(prev) = prev {
        for (id, (kind, title, bytes)) in prev {
            if !today.contains_key(id) {
                changes.push(ThingChange {
                    title: title.clone(),
                    kind: kind.clone(),
                    bytes: None,
                    change: -(*bytes as i64),
                    before: Some(*bytes),
                });
            }
        }
    }
    changes.sort_by_key(|c| std::cmp::Reverse(c.change.abs()));
    DayDetail {
        day: day.to_string(),
        previous_day: previous,
        things: changes,
    }
}
