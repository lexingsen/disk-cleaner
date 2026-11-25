use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::fmt::{fmt_bytes, is_zh, t};

pub type Bytes = u64;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThingKind {
    Project,
    Media,
    DevCache,
    Model,
    App,
    AppData,
    System,
    LargeFile,
    Browser,
}

impl ThingKind {
    pub fn title(self) -> String {
        match self {
            ThingKind::Project => t("项目", "Projects"),
            ThingKind::Media => t("作品", "Media"),
            ThingKind::DevCache => t("开发缓存", "Dev caches"),
            ThingKind::Model => t("模型", "Models"),
            ThingKind::App => t("应用", "Apps"),
            ThingKind::AppData => t("应用数据", "App data"),
            ThingKind::System => t("系统", "System"),
            ThingKind::LargeFile => t("大文件", "Large files"),
            ThingKind::Browser => t("浏览器", "Browsers"),
        }
    }

    pub fn color(self) -> u32 {
        match self {
            ThingKind::Project => 0x0a84ff,
            ThingKind::Media => 0xff2d55,
            ThingKind::DevCache => 0xff9f0a,
            ThingKind::Model => 0xbf5af2,
            ThingKind::App => 0x5e5ce6,
            ThingKind::AppData => 0x30b0c7,
            ThingKind::System => 0x8e8e93,
            ThingKind::LargeFile => 0xac8e68,
            ThingKind::Browser => 0x64d2ff,
        }
    }

    pub fn section(self) -> Section {
        match self {
            ThingKind::Project => Section::Project,
            ThingKind::Media => Section::Media,
            ThingKind::DevCache => Section::DevCache,
            ThingKind::Model => Section::Model,
            ThingKind::App => Section::Apps,
            ThingKind::AppData => Section::AppData,
            ThingKind::LargeFile => Section::LargeFiles,
            ThingKind::Browser => Section::Browsers,
            ThingKind::System => Section::Overview,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Section {
    Overview,
    Project,
    Media,
    DevCache,
    Model,
    LargeFiles,
    Apps,
    AppData,
    Browsers,
    Disk,
    Timeline,
}

impl Section {
    pub const ALL: [Section; 11] = [
        Section::Overview,
        Section::Project,
        Section::Media,
        Section::DevCache,
        Section::Model,
        Section::LargeFiles,
        Section::Apps,
        Section::AppData,
        Section::Browsers,
        Section::Disk,
        Section::Timeline,
    ];

    pub fn title(self) -> String {
        match self {
            Section::Overview => t("概览", "Overview"),
            Section::Disk => t("全盘", "Whole Disk"),
            Section::Timeline => t("时间线", "Timeline"),
            _ => self.kind().map(|k| k.title()).unwrap_or_default(),
        }
    }

    pub fn kind(self) -> Option<ThingKind> {
        match self {
            Section::Project => Some(ThingKind::Project),
            Section::Media => Some(ThingKind::Media),
            Section::DevCache => Some(ThingKind::DevCache),
            Section::Model => Some(ThingKind::Model),
            Section::Apps => Some(ThingKind::App),
            Section::AppData => Some(ThingKind::AppData),
            Section::LargeFiles => Some(ThingKind::LargeFile),
            Section::Browsers => Some(ThingKind::Browser),
            _ => None,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Section::Overview => "◉",
            Section::Project => "⚒",
            Section::Media => "▤",
            Section::DevCache => "▣",
            Section::Model => "◈",
            Section::LargeFiles => "◍",
            Section::Apps => "▢",
            Section::AppData => "▥",
            Section::Browsers => "◎",
            Section::Disk => "▦",
            Section::Timeline => "◔",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    Lossless,
    Regenerable,
    Yours,
    Guide,
    Erase,
}

impl Tier {
    pub fn rank(self) -> i32 {
        match self {
            Tier::Lossless => 0,
            Tier::Regenerable => 1,
            Tier::Yours => 2,
            Tier::Erase => 3,
            Tier::Guide => 4,
        }
    }
}

impl PartialOrd for Tier {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tier {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl Tier {
    pub fn title(self) -> String {
        match self {
            Tier::Lossless => t("无损", "lossless"),
            Tier::Regenerable => t("可再生", "regenerable"),
            Tier::Yours => t("你的文件", "yours"),
            Tier::Erase => t("永久清除", "erased"),
            Tier::Guide => t("仅指引", "guide only"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReclaimAction {
    Delete { path: String },
    DeleteAll { paths: Vec<String> },
    Trash { path: String },
    Command { argv: Vec<String>, cwd: Option<String> },
    Erase { db: String, sql: Vec<String> },
    Compound { actions: Vec<ReclaimAction> },
    None,
}

impl ReclaimAction {
    pub fn paths(&self) -> Vec<String> {
        match self {
            ReclaimAction::Delete { path } | ReclaimAction::Trash { path } => vec![path.clone()],
            ReclaimAction::DeleteAll { paths } => paths.clone(),
            ReclaimAction::Compound { actions } => {
                actions.iter().flat_map(|a| a.paths()).collect()
            }
            _ => vec![],
        }
    }

    pub fn databases(&self) -> Vec<String> {
        match self {
            ReclaimAction::Erase { db, .. } => vec![db.clone()],
            ReclaimAction::Compound { actions } => {
                actions.iter().flat_map(|a| a.databases()).collect()
            }
            _ => vec![],
        }
    }

    pub fn commands(&self) -> Vec<Vec<String>> {
        match self {
            ReclaimAction::Command { argv, .. } => vec![argv.clone()],
            ReclaimAction::Compound { actions } => {
                actions.iter().flat_map(|a| a.commands()).collect()
            }
            _ => vec![],
        }
    }

    pub fn is_trash_only(&self) -> bool {
        match self {
            ReclaimAction::Trash { .. } => true,
            ReclaimAction::Compound { actions } => {
                !actions.is_empty() && actions.iter().all(|a| a.is_trash_only())
            }
            _ => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.paths().is_empty() && self.commands().is_empty() && self.databases().is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReclaimItem {
    pub id: String,
    pub label: String,
    pub path: Option<String>,
    pub bytes: Bytes,
    pub estimate: Bytes,
    #[serde(default)]
    pub estimate_is_lower_bound: bool,
    pub tier: Tier,
    pub action: ReclaimAction,
    pub restore: Option<String>,
    pub reason: String,
    pub preselected: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub covers: Vec<String>,
    /// The installed app (its folder path) this item belongs to.
    #[serde(default)]
    pub app: Option<String>,
    /// Records erased, when known (browser history, cookies).
    #[serde(default)]
    pub records: Option<i64>,
}

impl ReclaimItem {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        path: Option<String>,
        bytes: Bytes,
        tier: Tier,
        action: ReclaimAction,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            path,
            bytes,
            estimate: bytes,
            estimate_is_lower_bound: false,
            tier,
            action,
            restore: None,
            reason: reason.into(),
            preselected: false,
            warnings: vec![],
            covers: vec![],
            app: None,
            records: None,
        }
    }

    pub fn with_estimate(mut self, estimate: Bytes) -> Self {
        self.estimate = estimate;
        self
    }

    pub fn preselected(mut self, on: bool) -> Self {
        self.preselected = on;
        self
    }

    pub fn with_restore(mut self, restore: impl Into<String>) -> Self {
        self.restore = Some(restore.into());
        self
    }

    pub fn with_warning(mut self, w: impl Into<String>) -> Self {
        self.warnings.push(w.into());
        self
    }

    pub fn with_app(mut self, app: impl Into<String>) -> Self {
        self.app = Some(app.into());
        self
    }

    pub fn with_records(mut self, n: i64) -> Self {
        self.records = Some(n);
        self
    }

    pub fn with_covers(mut self, covers: Vec<String>) -> Self {
        self.covers = covers;
        self
    }

    /// "1.2 GB", "3 records", or empty for guide items.
    pub fn amount(&self) -> String {
        if let Some(n) = self.records {
            if is_zh() {
                format!("{} 条", crate::fmt::fmt_count(n))
            } else if n == 1 {
                "1 record".to_string()
            } else {
                format!("{} records", crate::fmt::fmt_count(n))
            }
        } else if self.tier == Tier::Guide || (self.tier == Tier::Erase && self.estimate == 0) {
            String::new()
        } else {
            format!(
                "{}{}",
                fmt_bytes(self.estimate),
                if self.estimate_is_lower_bound { "+" } else { "" }
            )
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Thing {
    pub id: String,
    pub kind: ThingKind,
    pub title: String,
    pub path: Option<String>,
    pub bytes: Bytes,
    pub last_active: Option<i64>,
    pub activity_source: Option<String>,
    pub detail: Option<String>,
    pub items: Vec<ReclaimItem>,
}

impl Thing {
    pub fn new(
        id: impl Into<String>,
        kind: ThingKind,
        title: impl Into<String>,
        path: Option<String>,
        bytes: Bytes,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            title: title.into(),
            path,
            bytes,
            last_active: None,
            activity_source: None,
            detail: None,
            items: vec![],
        }
    }

    pub fn reclaimable(&self) -> Bytes {
        let covered: HashSet<&str> = self.items.iter().flat_map(|i| i.covers.iter().map(|s| s.as_str())).collect();
        self.items
            .iter()
            .filter(|i| i.tier < Tier::Guide && !covered.contains(i.id.as_str()))
            .map(|i| i.estimate)
            .sum()
    }

    pub fn preselected_bytes(&self) -> Bytes {
        self.items.iter().filter(|i| i.preselected).map(|i| i.estimate).sum()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub created_at: i64,
    pub volume_total: Bytes,
    pub volume_free: Bytes,
    pub scanned: Bytes,
    pub things: Vec<Thing>,
    pub language: Option<String>,
}

impl Report {
    /// Items that act on exactly this path.
    pub fn items_at<'a>(&'a self, path: &str) -> Vec<(&'a Thing, &'a ReclaimItem)> {
        let mut out = vec![];
        for t in &self.things {
            for i in &t.items {
                if i.tier < Tier::Guide
                    && (i.path.as_deref() == Some(path) || i.action.paths().iter().any(|p| p == path))
                {
                    out.push((t, i));
                }
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub items: Vec<ReclaimItem>,
    pub freed: Bytes,
}

pub fn plan(things: &[Thing], selected: Option<&HashSet<String>>) -> Plan {
    let all: Vec<&ReclaimItem> = things
        .iter()
        .flat_map(|t| t.items.iter())
        .filter(|i| i.tier < Tier::Guide)
        .collect();
    let mut chosen: Vec<ReclaimItem> = all
        .into_iter()
        .filter(|i| match selected {
            Some(sel) => sel.contains(&i.id),
            None => i.preselected && i.tier != Tier::Erase,
        })
        .cloned()
        .collect();
    let covered: HashSet<String> = chosen.iter().flat_map(|i| i.covers.clone()).collect();
    chosen.retain(|i| !covered.contains(&i.id));
    let freed = chosen.iter().map(|i| i.estimate).sum();
    Plan { items: chosen, freed }
}
