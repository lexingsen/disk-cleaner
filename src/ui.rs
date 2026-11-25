use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;

use crate::clean::{CleanResult, Cleaner, ItemProgress, ItemState, Phase};
use crate::demo::{self, DemoExternal};
use crate::fmt::t as tr;
use crate::fmt::*;
use crate::history::{self, DayPoint, DayDetail, ThingPoint};
use crate::model::*;
use crate::prefs::{self, Appearance, LanguagePref};
use crate::recognize::{recognize_all, recognize_external};
use crate::scan::{self, ScanOptions, ScanProgress, ScanTree};
use crate::theme::{alpha, hue_color, Theme};
use crate::treemap::squarify_in;
use crate::volume::{self, ExternalVolume, VolumeInfo};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub(crate) struct ScanUi {
    progress: Arc<ScanProgress>,
    cancel: Arc<AtomicBool>,
    log: Arc<Mutex<String>>,
}

pub(crate) struct External {
    pub id: String,
    pub name: String,
    pub path: String,
    pub total: u64,
    pub free: u64,
    pub tree: Option<ScanTree>,
    pub report: Option<Report>,
    pub busy: bool,
    pub scan: Option<ScanUi>,
    pub map: bool,
    pub map_stack: Vec<String>,
    pub expanded: HashSet<String>,
    pub selection: HashSet<String>,
    pub real: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CleanTarget {
    Startup,
    External(usize),
}

enum CleanMsg {
    Item(ItemProgress),
    Done(CleanResult),
}

struct LiveClean {
    plan: Plan,
    states: HashMap<String, ItemState>,
    current: Option<String>,
    freed: u64,
    estimate: u64,
    result: Option<CleanResult>,
    applied: bool,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<CleanMsg>,
    started: i64,
    target: CleanTarget,
    disk_name: Option<String>,
}

pub struct Root {
    pub(crate) demo: bool,
    pub(crate) report: Option<Report>,
    pub(crate) tree: Option<ScanTree>,
    pub(crate) volume: VolumeInfo,
    pub(crate) history: Vec<DayPoint>,
    pub(crate) thing_history: Vec<ThingPoint>,
    pub(crate) externals: Vec<External>,
    pub(crate) section: Section,
    pub(crate) active_external: Option<usize>,
    pub(crate) selection: HashSet<String>,
    pub(crate) expanded: HashSet<String>,
    pub(crate) expanded_section: Option<Section>,
    pub(crate) search: String,
    search_marked: usize,
    search_focus: FocusHandle,
    pub(crate) show_map: bool,
    pub(crate) map_stack: Vec<String>,
    map_hovered: Option<String>,
    map_picked: Option<String>,
    map_menu: Option<String>,
    viewport: Size<Pixels>,
    pub(crate) timeline_day: Option<String>,
    pub(crate) sheet_open: bool,
    pub(crate) settings_open: bool,
    pub(crate) show_picked: bool,
    pub(crate) show_sel_list: bool,
    pub(crate) scan: Option<ScanUi>,
    pub(crate) scan_phase: Option<String>,
    clean: Option<LiveClean>,
    pub(crate) appearance: Appearance,
    pub(crate) language: LanguagePref,
    pub(crate) language_start: LanguagePref,
    auto_scan: bool,
}

fn default_selection(things: &[Thing]) -> HashSet<String> {
    things
        .iter()
        .flat_map(|t| t.items.iter())
        .filter(|i| i.preselected && i.tier != Tier::Erase)
        .map(|i| i.id.clone())
        .collect()
}

fn occupied(things: &[Thing], kind: ThingKind) -> u64 {
    things.iter().filter(|t| t.kind == kind).map(|t| t.bytes).sum()
}

fn reclaimable(things: &[Thing], kind: ThingKind) -> u64 {
    things
        .iter()
        .filter(|t| t.kind == kind)
        .map(|t| t.reclaimable())
        .sum()
}

fn selected_bytes(things: &[Thing], sel: &HashSet<String>, kind: ThingKind) -> u64 {
    things
        .iter()
        .filter(|t| t.kind == kind)
        .flat_map(|t| t.items.iter())
        .filter(|i| sel.contains(&i.id))
        .map(|i| i.estimate)
        .sum()
}

impl Root {
    pub fn new(
        demo_mode: bool,
        appearance: Appearance,
        language: LanguagePref,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut root = Root {
            demo: demo_mode,
            report: None,
            tree: None,
            volume: volume::volume_info(),
            history: vec![],
            thing_history: vec![],
            externals: vec![],
            section: Section::Overview,
            active_external: None,
            selection: HashSet::new(),
            expanded: HashSet::new(),
            expanded_section: None,
            search: String::new(),
            search_marked: 0,
            search_focus: cx.focus_handle(),
            show_map: false,
            map_stack: vec![],
            map_hovered: None,
            map_picked: None,
            map_menu: None,
            viewport: size(px(1180.), px(760.)),
            timeline_day: None,
            sheet_open: false,
            settings_open: false,
            show_picked: false,
            show_sel_list: false,
            scan: None,
            scan_phase: None,
            clean: None,
            appearance,
            language,
            language_start: language,
            auto_scan: !demo_mode,
        };
        if demo_mode {
            root.load_demo();
        } else {
            let (days, points) = history::load_history();
            root.history = days;
            root.thing_history = points;
            if let Some(report) = history::load_report() {
                root.volume = VolumeInfo {
                    name: root.volume.name.clone(),
                    path: root.volume.path.clone(),
                    uuid: root.volume.uuid.clone(),
                    total: report.volume_total,
                    free: report.volume_free,
                };
                root.selection = default_selection(&report.things);
                root.report = Some(report);
            }
            root.externals = volume::external_volumes()
                .into_iter()
                .map(|v| External {
                    id: v.uuid.clone(),
                    name: v.name.clone(),
                    path: v.path.clone(),
                    total: v.total,
                    free: v.free,
                    tree: None,
                    report: None,
                    busy: false,
                    scan: None,
                    map: false,
                    map_stack: vec![],
                    expanded: HashSet::new(),
                    selection: HashSet::new(),
                    real: true,
                })
                .collect();
            root.start_scan(cx);
        }
        root.ensure_expanded();
        root
    }

    fn load_demo(&mut self) {
        let d = demo::make();
        self.volume = d.volume.clone();
        self.history = d.history;
        self.thing_history = d.thing_history;
        self.selection = default_selection(&d.report.things);
        self.report = Some(d.report);
        self.tree = Some(d.tree);
        self.externals.clear();
        if let Some(e) = d.external {
            let DemoExternal {
                volume,
                report,
                tree,
                things,
            } = e;
            let mut report = report;
            report.things = things;
            self.externals.push(External {
                id: volume.uuid.clone(),
                name: volume.name.clone(),
                path: volume.path.clone(),
                total: volume.total,
                free: volume.free,
                tree: Some(tree),
                report: Some(report),
                busy: false,
                scan: None,
                map: false,
                map_stack: vec![],
                expanded: HashSet::new(),
                selection: HashSet::new(),
                real: false,
            });
        }
        let _ = &self.externals;
        self.timeline_day = None;
    }

    pub(crate) fn theme(&self) -> Theme {
        if prefs::is_dark(self.appearance) {
            Theme::dark()
        } else {
            Theme::light()
        }
    }

    pub(crate) fn things(&self) -> &[Thing] {
        if let Some(i) = self.active_external {
            if let Some(e) = self.externals.get(i) {
                if let Some(r) = &e.report {
                    return &r.things;
                }
            }
            return &[];
        }
        self.report.as_ref().map(|r| r.things.as_slice()).unwrap_or(&[])
    }

    fn ext_volume(&self) -> Option<&External> {
        self.active_external.and_then(|i| self.externals.get(i))
    }

    fn selection_ref(&self) -> &HashSet<String> {
        if let Some(i) = self.active_external {
            if let Some(e) = self.externals.get(i) {
                return &e.selection;
            }
        }
        &self.selection
    }

    fn selection_mut(&mut self) -> &mut HashSet<String> {
        if let Some(i) = self.active_external {
            if let Some(e) = self.externals.get_mut(i) {
                return &mut e.selection;
            }
        }
        &mut self.selection
    }

    fn plan(&self) -> Plan {
        let sel = self.selection_ref();
        crate::model::plan(self.things(), Some(sel))
    }

    fn total_reclaimable(&self) -> u64 {
        self.things().iter().map(|t| t.reclaimable()).sum()
    }

    fn toggle_item(&mut self, id: &str, cx: &mut Context<Self>) {
        let sel = self.selection_mut();
        if !sel.remove(id) {
            sel.insert(id.to_string());
        }
        cx.notify();
    }

    fn set_items(&mut self, ids: Vec<String>, on: bool, cx: &mut Context<Self>) {
        let sel = self.selection_mut();
        for id in ids {
            if on {
                sel.insert(id);
            } else {
                sel.remove(&id);
            }
        }
        cx.notify();
    }

    fn toggle_thing(&mut self, thing: &Thing, cx: &mut Context<Self>) {
        let ids: Vec<String> = thing
            .items
            .iter()
            .filter(|i| i.tier < Tier::Guide)
            .map(|i| i.id.clone())
            .collect();
        if ids.is_empty() {
            return;
        }
        let sel = self.selection_ref();
        let n = ids.iter().filter(|i| sel.contains(*i)).count();
        self.set_items(ids, n != thing.items.iter().filter(|i| i.tier < Tier::Guide).count(), cx);
    }

    fn ensure_expanded(&mut self) {
        if self.expanded_section == Some(self.section) {
            return;
        }
        self.expanded_section = Some(self.section);
        self.expanded.clear();
        let Some(kind) = self.section.kind() else {
            return;
        };
        let n = match kind {
            ThingKind::Project => 3,
            ThingKind::LargeFile => 1,
            _ => 2,
        };
        let ids: Vec<String> = self
            .things()
            .iter()
            .filter(|t| t.kind == kind)
            .take(n)
            .map(|t| t.id.clone())
            .collect();
        self.expanded.extend(ids);
    }

    fn open_section(&mut self, s: Section, cx: &mut Context<Self>) {
        self.active_external = None;
        self.section = s;
        self.search.clear();
        self.show_sel_list = false;
        self.show_map = false;
        self.map_hovered = None;
        self.map_picked = None;
        self.map_menu = None;
        self.ensure_expanded();
        cx.notify();
    }

    fn open_external(&mut self, i: usize, cx: &mut Context<Self>) {
        self.active_external = Some(i);
        self.sheet_open = false;
        self.show_sel_list = false;
        self.map_hovered = None;
        self.map_picked = None;
        self.map_menu = None;
        if let Some(e) = self.externals.get(i) {
            if e.expanded.is_empty() {
                let ids: Vec<String> = e
                    .report
                    .as_ref()
                    .map(|r| r.things.iter().take(3).map(|t| t.id.clone()).collect())
                    .unwrap_or_default();
                if let Some(e) = self.externals.get_mut(i) {
                    e.expanded.extend(ids);
                }
            }
        }
        cx.notify();
    }

    // -- scanning ----------------------------------------------------------

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        if self.scan.is_some() {
            return;
        }
        let progress = Arc::new(ScanProgress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let log = Arc::new(Mutex::new(tr("正在扫描…", "Scanning…")));
        self.scan_phase = Some(tr("正在扫描…", "Scanning…"));
        self.scan = Some(ScanUi {
            progress: progress.clone(),
            cancel: cancel.clone(),
            log: log.clone(),
        });

        let home = home_string();
        // The first scan covers every mounted volume, not just the startup disk.
        let startup_path = if self.volume.path.trim().is_empty() {
            home.clone()
        } else {
            self.volume.path.clone()
        };
        let startup_name = self.volume.name.clone();
        let startup_uuid = self.volume.uuid.clone();
        let skip: Vec<String> = vec![];
        let progress2 = progress.clone();
        let cancel2 = cancel.clone();
        let log2 = log.clone();
        cx.spawn(async move |this, cx| {
            let (tree, report, volume) = cx.background_executor().spawn(async move {
                let live = volume::volume_info();
                let mut roots = vec![startup_path.clone()];
                let mut total = live.total;
                let mut free = live.free;
                for e in volume::external_volumes() {
                    let key = crate::scan::path_key(&e.path);
                    if roots.iter().any(|r| crate::scan::path_key(r) == key) {
                        continue;
                    }
                    total = total.saturating_add(e.total);
                    free = free.saturating_add(e.free);
                    roots.push(e.path);
                }
                let vol = VolumeInfo {
                    name: startup_name,
                    path: startup_path,
                    uuid: startup_uuid,
                    total,
                    free,
                };
                let extra_roots: Vec<String> = roots.iter().skip(1).cloned().collect();
                let tree = scan::scan(ScanOptions {
                    roots,
                    skip,
                    progress: Some(progress2),
                    cancel: Some(cancel2),
                    ..Default::default()
                });
                *log2.lock().unwrap() = tr("正在识别项目、缓存和浏览器…", "Recognizing projects, caches and browsers…");
                let mut report = recognize_all(&tree, &vol, |m| {
                    *log2.lock().unwrap() = m;
                });
                // The other drives were scanned into the same tree, so run the
                // per-volume project/media pass on it for each of them.
                for r in &extra_roots {
                    let extra = recognize_external(&tree, &vol, r, |m| {
                        *log2.lock().unwrap() = m;
                    });
                    for th in extra.things {
                        if !report.things.iter().any(|x| x.id == th.id) {
                            report.things.push(th);
                        }
                    }
                }
                (tree, report, vol)
            }).await;
            let _ = this.update(cx, |this, cx| {
                this.apply_scan(tree, report, volume);
                cx.notify();
            });
        })
        .detach();

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(160)).await;
                let keep = this
                    .update(cx, |this, cx| {
                        let Some(scan) = &this.scan else { return false };
                        let (files, bytes) = scan.progress.snapshot();
                        let log = scan.log.lock().unwrap().clone();
                        this.scan_phase = Some(if files == 0 {
                            log
                        } else if log.starts_with("正在识别") || log.starts_with("Recogniz") {
                            log
                        } else {
                            format!(
                                "{} · {} · {}",
                                tr("正在扫描", "Scanning"),
                                fmt_count(files as i64),
                                fmt_bytes(bytes)
                            )
                        });
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    fn apply_scan(&mut self, tree: ScanTree, report: Report, volume: VolumeInfo) {
        self.volume = volume.clone();
        self.tree = Some(tree);
        self.selection = default_selection(&report.things);
        self.report = Some(report);
        self.scan = None;
        self.scan_phase = None;
        if !self.demo {
            if let Some(r) = &self.report {
                history::save_report(r);
                history::record_history(r, self.volume.free);
                let (days, points) = history::load_history();
                self.history = days;
                self.thing_history = points;
            }
        }
        self.expanded_section = None;
        self.ensure_expanded();
    }

    fn scan_external(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(e) = self.externals.get_mut(i) else { return };
        if e.busy {
            return;
        }
        e.busy = true;
        let path = e.path.clone();
        let busy_log = Arc::new(Mutex::new(tr("正在扫描…", "Scanning…")));
        let progress = Arc::new(ScanProgress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let log2 = busy_log.clone();
        let progress2 = progress.clone();
        let cancel2 = cancel.clone();
        if let Some(e) = self.externals.get_mut(i) {
            e.scan = Some(ScanUi {
                progress: progress.clone(),
                cancel: cancel.clone(),
                log: busy_log,
            });
        }
        cx.spawn(async move |this, cx| {
            let path2 = path.clone();
            let (tree, report, free, total) = cx.background_executor().spawn(async move {
                let tree = scan::scan(ScanOptions {
                    roots: vec![path2.clone()],
                    skip: vec![],
                    progress: Some(progress2),
                    cancel: Some(cancel2),
                    ..Default::default()
                });
                *log2.lock().unwrap() = tr("正在识别项目…", "Finding projects…");
                let vols = volume::external_volumes();
                let vol = vols
                    .iter()
                    .find(|v| v.path == path2)
                    .cloned()
                    .unwrap_or(ExternalVolume {
                        name: Path::new(&path2)
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| path2.clone()),
                        path: path2.clone(),
                        uuid: path2.clone(),
                        total: 0,
                        free: 0,
                    });
                let info = VolumeInfo {
                    name: vol.name.clone(),
                    path: vol.path.clone(),
                    uuid: vol.uuid.clone(),
                    total: vol.total,
                    free: vol.free,
                };
                let report = recognize_external(&tree, &info, &path2, |m| {
                    *log2.lock().unwrap() = m;
                });
                (tree, report, info.free, info.total)
            }).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(e) = this.externals.get_mut(i) {
                    e.busy = false;
                    e.scan = None;
                    e.selection = default_selection(&report.things);
                    e.expanded = report.things.iter().take(3).map(|t| t.id.clone()).collect();
                    e.tree = Some(tree);
                    e.report = Some(report);
                    e.free = free;
                    e.total = total;
                }
                cx.notify();
            });
        })
        .detach();
    }

    // -- cleaning ----------------------------------------------------------

    fn open_sheet(&mut self, cx: &mut Context<Self>) {
        if self.plan().items.is_empty() {
            return;
        }
        self.sheet_open = true;
        self.show_sel_list = false;
        cx.notify();
    }

    fn begin_clean(&mut self, cx: &mut Context<Self>) {
        let plan = self.plan();
        if plan.items.is_empty() {
            return;
        }
        let target = match self.active_external {
            Some(i) => CleanTarget::External(i),
            None => CleanTarget::Startup,
        };
        let disk_name = self.ext_volume().map(|e| e.name.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel2 = cancel.clone();
        let (tx, rx) = mpsc::channel::<CleanMsg>();
        let plan2 = plan.clone();
        let tx_ev = tx.clone();
        std::thread::spawn(move || {
            let mut cleaner = Cleaner {
                cancel: &cancel2,
                progress: Box::new(move |_phase: Phase, item: Option<&ItemProgress>| {
                    if let Some(p) = item {
                        let _ = tx_ev.send(CleanMsg::Item(p.clone()));
                    }
                }),
            };
            let res = crate::clean::execute(&plan2, &mut cleaner);
            let _ = tx.send(CleanMsg::Done(res));
        });
        self.sheet_open = false;
        self.show_sel_list = false;
        self.clean = Some(LiveClean {
            states: plan
                .items
                .iter()
                .map(|i| (i.id.clone(), ItemState::Pending))
                .collect(),
            estimate: plan.freed,
            plan,
            current: None,
            freed: 0,
            result: None,
            applied: false,
            cancel,
            rx,
            started: now_ts(),
            target,
            disk_name,
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(80)).await;
                let keep = this
                    .update(cx, |this, cx| this.pump_clean(cx))
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
    }

    /// Returns whether there may be more messages.
    fn pump_clean(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(live) = &mut self.clean else {
            return false;
        };
        let mut applied_result: Option<(CleanResult, CleanTarget)> = None;
        loop {
            match live.rx.try_recv() {
                Ok(CleanMsg::Item(p)) => {
                    if let ItemState::Done = p.state {
                        if let Some(it) = live.plan.items.iter().find(|i| i.id == p.id) {
                            live.freed += it.estimate;
                        }
                    }
                    if let ItemState::Running = p.state {
                        live.current = Some(p.id.clone());
                    }
                    live.states.insert(p.id, p.state);
                    cx.notify();
                }
                Ok(CleanMsg::Done(res)) => {
                    if !live.applied {
                        live.applied = true;
                        applied_result = Some((res.clone(), live.target));
                    }
                    live.current = None;
                    live.result = Some(res);
                    cx.notify();
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        let finished = live.result.is_some();
        if let Some((res, target)) = applied_result {
            match target {
                CleanTarget::Startup => {
                    if self.demo {
                        self.volume.free = (self.volume.free + res.reclaimed).min(self.volume.total);
                    } else {
                        let v = volume::volume_info();
                        let mut free = v.free;
                        for e in volume::external_volumes() {
                            free = free.saturating_add(e.free);
                        }
                        self.volume.free = free.min(self.volume.total);
                    }
                }
                CleanTarget::External(i) => {
                    if let Some(e) = self.externals.get_mut(i) {
                        e.free = (e.free + res.reclaimed).min(e.total);
                    }
                }
            }
        }
        cx.notify();
        !finished
    }

    fn close_clean(&mut self, cx: &mut Context<Self>) {
        let target = self.clean.as_ref().map(|c| c.target);
        self.clean = None;
        match target {
            Some(CleanTarget::External(i)) => {
                self.scan_external(i, cx);
            }
            _ => {
                if self.demo {
                    let section = self.section;
                    let ext = self.active_external;
                    self.load_demo();
                    self.section = section;
                    self.active_external = ext.filter(|i| *i < self.externals.len());
                    self.ensure_expanded();
                } else {
                    self.start_scan(cx);
                }
            }
        }
        cx.notify();
    }

    fn stop_clean(&mut self, cx: &mut Context<Self>) {
        if let Some(live) = &self.clean {
            live.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    // -- search ------------------------------------------------------------

    fn on_search_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Printable characters (including IME composition) go through the
        // window input handler; only editing keys are handled here.
        let key = ev.keystroke.key.as_str();
        match key {
            "backspace" => {
                // Drop any in-progress composition first.
                if self.search_marked > 0 {
                    self.search_marked = 0;
                } else {
                    self.search.pop();
                }
                cx.stop_propagation();
            }
            "escape" => {
                self.search.clear();
                self.search_marked = 0;
                cx.stop_propagation();
            }
            _ => return,
        }
        let _ = window;
        cx.notify();
    }

    // -- helpers for rendering data ---------------------------------------

    pub(crate) fn filtered_things(&self, kind: ThingKind) -> Vec<Thing> {
        let q = self.search.to_lowercase();
        self.things()
            .iter()
            .filter(|t| t.kind == kind)
            .filter(|t| {
                q.is_empty()
                    || t.title.to_lowercase().contains(&q)
                    || t.path
                        .as_ref()
                        .map(|p| p.to_lowercase().contains(&q))
                        .unwrap_or(false)
                    || t.items.iter().any(|i| {
                        i.label.to_lowercase().contains(&q)
                            || i.path
                                .as_ref()
                                .map(|p| p.to_lowercase().contains(&q))
                                .unwrap_or(false)
                            || i.reason.to_lowercase().contains(&q)
                    })
            })
            .cloned()
            .collect()
    }

    pub(crate) fn all_kind_counts(&self) -> HashMap<ThingKind, (usize, u64)> {
        let mut m: HashMap<ThingKind, (usize, u64)> = HashMap::new();
        for t in self.things() {
            let e = m.entry(t.kind).or_insert((0, 0));
            e.0 += 1;
            e.1 += if t.kind == ThingKind::App {
                t.bytes
            } else {
                t.reclaimable()
            };
        }
        m
    }
}

// ---------------------------------------------------------------------------
// Search field text input (ASCII + IME composition)
// ---------------------------------------------------------------------------

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn utf16_to_byte(s: &str, index: usize) -> usize {
    let mut u = 0;
    for (b, ch) in s.char_indices() {
        if u >= index {
            return b;
        }
        u += ch.len_utf16();
    }
    s.len()
}

fn replace_utf16(s: &mut String, range: Range<usize>, text: &str) {
    let len = utf16_len(s);
    let start = utf16_to_byte(s, range.start.min(len));
    let end = utf16_to_byte(s, range.end.min(len));
    s.replace_range(start..end, text);
}

impl EntityInputHandler for Root {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let len = utf16_len(&self.search);
        let r = range.start.min(len)..range.end.min(len);
        *adjusted_range = Some(r.clone());
        let start = utf16_to_byte(&self.search, r.start);
        let end = utf16_to_byte(&self.search, r.end);
        Some(self.search[start..end].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let len = utf16_len(&self.search);
        Some(UTF16Selection {
            range: len..len,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let len = utf16_len(&self.search);
        let marked = self.search_marked.min(len);
        if marked == 0 {
            None
        } else {
            Some(len - marked..len)
        }
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.search_marked = 0;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = utf16_len(&self.search);
        let marked = self.search_marked.min(len);
        let r = range.unwrap_or_else(|| {
            if marked > 0 {
                len - marked..len
            } else {
                len..len
            }
        });
        replace_utf16(&mut self.search, r, text);
        self.search_marked = 0;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = utf16_len(&self.search);
        let marked = self.search_marked.min(len);
        let r = range.unwrap_or(len - marked..len);
        replace_utf16(&mut self.search, r, new_text);
        self.search_marked = utf16_len(new_text);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(element_bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(utf16_len(&self.search))
    }
}

// ---------------------------------------------------------------------------
// Small shared pieces
// ---------------------------------------------------------------------------

fn font_name() -> &'static str {
    if cfg!(windows) {
        "Segoe UI"
    } else {
        "Helvetica Neue"
    }
}

fn text(s: impl Into<SharedString>, size: f32, color: Rgba) -> Div {
    div().text_size(px(size)).text_color(color).child(s.into())
}

fn big_number(bytes: u64, size: f32, t: &Theme) -> Div {
    let (n, u) = split_bytes(bytes);
    div()
        .flex()
        .flex_row()
        .items_end()
        .gap(px(6.))
        .child(
            div()
                .text_size(px(size))
                .text_color(t.text())
                .font_weight(FontWeight::SEMIBOLD)
                .child(n),
        )
        .child(
            div()
                .text_size(px(size * 0.42))
                .text_color(t.secondary())
                .font_weight(FontWeight::MEDIUM)
                .pb(px(size * 0.09))
                .child(u),
        )
}

fn card(t: &Theme) -> Div {
    div()
        .bg(t.card())
        .border_1()
        .border_color(t.border())
        .rounded_2xl()
        .p(px(20.))
}

fn fade(color: Rgba, a: f32) -> Rgba {
    let mut c = color;
    c.a *= a;
    c
}

fn primary_button(id: &'static str, label: impl Into<SharedString>, enabled: bool, t: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(16.))
        .py(px(9.))
        .rounded_full()
        .bg(if enabled { t.accent() } else { alpha(t.accent, 0x66) })
        .text_color(rgb(0xffffff))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .when(enabled, |el| el.cursor_pointer().hover(|s| s.opacity(0.9)))
        .child(label.into())
}

fn ghost_button(id: &'static str, label: impl Into<SharedString>, t: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(12.))
        .py(px(7.))
        .rounded(px(8.))
        .bg(t.card_alt())
        .border_1()
        .border_color(t.border())
        .text_color(t.text())
        .text_size(px(13.))
        .hover(|s| s.bg(t.hover()))
        .cursor_pointer()
        .child(label.into())
}

fn link_button(id: &'static str, label: impl Into<SharedString>, t: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .text_size(px(12.))
        .text_color(t.accent())
        .cursor_pointer()
        .hover(|s| s.opacity(0.75))
        .child(label.into())
}

fn spinner(t: &Theme) -> Div {
    div()
        .size(px(14.))
        .rounded_full()
        .border_2()
        .border_color(alpha(t.accent, 0x88))
}

fn tier_badge(tier: Tier) -> Div {
    let (label, color) = match tier {
        Tier::Lossless => (tr("无损", "lossless"), rgb(0x34c759)),
        Tier::Regenerable => (tr("可再生", "rebuildable"), rgb(0x0a84ff)),
        Tier::Yours => (tr("进废纸篓", "to Trash"), rgb(0xff9f0a)),
        Tier::Erase => (tr("永久清除", "erased for good"), rgb(0xff453a)),
        Tier::Guide => (tr("指引", "guide"), rgb(0x30b0c7)),
    };
    div()
        .px(px(6.))
        .py(px(1.))
        .rounded_full()
        .bg(fade(color, 0.14))
        .text_color(color)
        .text_size(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

fn app_tile(name: &str, side: f32) -> Div {
    let color = hue_color(name);
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(6.))
        .bg(color)
        .text_color(rgb(0xffffff))
        .text_size(px(side * 0.52))
        .font_weight(FontWeight::SEMIBOLD)
        .child(
            name.chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "?".into()),
        )
}

fn symbol_for_large(id: &str) -> &'static str {
    match id.strip_prefix("large:").unwrap_or("") {
        "installer" => "◉",
        "vm" => "▣",
        "video" => "▶",
        "audio" => "♪",
        "archive" => "▤",
        "model" => "◈",
        "app" => "▢",
        _ => "•",
    }
}

fn dot(color: Rgba, side: f32) -> Div {
    div().size(px(side)).rounded_full().bg(color)
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

impl Root {
    fn render_root(
        &mut self,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut root = div()
            .id("app-root")
            .flex()
            .flex_row()
            .size_full()
            .bg(t.bg())
            .text_color(t.text())
            .font_family(font_name())
            .text_size(px(13.))
            .relative();
        root = root.child(self.render_sidebar(t, cx));
        root = root.child(self.render_main(t, window, cx));
        if self.sheet_open {
            root = root.child(self.render_clean_sheet(t, cx));
        }
        if self.settings_open {
            root = root.child(self.render_settings(t, cx));
        }
        root
    }

    fn nav_row(
        &self,
        t: &Theme,
        icon: &str,
        icon_color: Rgba,
        title: &str,
        value: Option<String>,
        selected: bool,
        section: Section,
        external: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut row = div()
            .id(SharedString::from(format!(
                "nav-{:?}-{}",
                section,
                external.map(|i| i.to_string()).unwrap_or_default()
            )))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(9.))
            .h(px(28.))
            .px(px(9.))
            .mx(px(8.))
            .rounded(px(7.))
            .cursor_pointer()
            .when(selected, |el| el.bg(alpha(t.accent, 0x26)))
            .when(!selected, |el| el.hover(|s| s.bg(t.hover())))
            .child(
                div()
                    .w(px(16.))
                    .flex()
                    .justify_center()
                    .text_color(icon_color)
                    .text_size(px(12.))
                    .child(icon.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(13.))
                    .text_color(if selected { t.text() } else { t.text() })
                    .child(title.to_string()),
            );
        if let Some(v) = value {
            row = row.child(
                div()
                    .text_size(px(11.))
                    .text_color(t.secondary())
                    .child(v),
            );
        }
        if let Some(i) = external {
            row = row.on_click(cx.listener(move |this, _, _, cx| this.open_external(i, cx)));
        } else {
            row = row.on_click(cx.listener(move |this, _, _, cx| this.open_section(section, cx)));
        }
        row
    }

    fn render_sidebar(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let counts = self.all_kind_counts();
        let active = self.active_external.is_none();
        let mut col = div()
            .flex()
            .flex_col()
            .w(px(234.))
            .h_full()
            .flex_shrink_0()
            .bg(t.sidebar())
            .border_r_1()
            .border_color(t.border())
            .pt(px(16.))
            .pb(px(10.));

        col = col.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .px(px(18.))
                .pb(px(12.))
                .child(
                    div()
                        .size(px(22.))
                        .rounded(px(6.))
                        .bg(t.accent())
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .relative()
                                .w(px(13.))
                                .h(px(8.))
                                .rounded(px(2.))
                                .bg(rgb(0xffffff))
                                .child(
                                    div()
                                        .absolute()
                                        .left(px(2.))
                                        .top(px(2.))
                                        .w(px(6.5))
                                        .h(px(1.8))
                                        .rounded_full()
                                        .bg(t.accent()),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .right(px(1.8))
                                        .bottom(px(1.6))
                                        .size(px(2.))
                                        .rounded_full()
                                        .bg(t.accent()),
                                ),
                        ),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("disk-cleaner"),
                )
                .when(self.demo, |el| {
                    el.child(
                        div()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded_full()
                            .bg(alpha(0xbf5af2, 0x22))
                            .text_color(rgb(0xbf5af2))
                            .text_size(px(9.))
                            .child(tr("演示", "demo")),
                    )
                }),
        );

        let val = |k: ThingKind| counts.get(&k).map(|(_, b)| *b).unwrap_or(0);
        col = col.child(self.nav_row(
            t,
            Section::Overview.symbol(),
            t.accent(),
            &Section::Overview.title(),
            None,
            self.section == Section::Overview && active,
            Section::Overview,
            None,
            cx,
        ));
        col = col.child(sidebar_label(t, tr("可以清理", "Reclaim")));
        for s in [
            Section::Project,
            Section::Media,
            Section::DevCache,
            Section::Model,
            Section::LargeFiles,
            Section::Apps,
            Section::AppData,
        ] {
            let k = s.kind().unwrap();
            let color = rgb(k.color());
            let value = if k == ThingKind::App {
                let b = val(k);
                if b > 0 { Some(fmt_bytes(b)) } else { None }
            } else {
                let b = val(k);
                if b > 0 { Some(fmt_bytes(b)) } else { None }
            };
            col = col.child(self.nav_row(
                t,
                s.symbol(),
                color,
                &s.title(),
                value,
                active && self.section == s,
                s,
                None,
                cx,
            ));
        }
        col = col.child(sidebar_label(t, tr("隐私", "Privacy")));
        let browsers = counts.get(&ThingKind::Browser).map(|(n, _)| *n).unwrap_or(0);
        col = col.child(self.nav_row(
            t,
            Section::Browsers.symbol(),
            rgb(ThingKind::Browser.color()),
            &Section::Browsers.title(),
            if browsers > 0 { Some(browsers.to_string()) } else { None },
            active && self.section == Section::Browsers,
            Section::Browsers,
            None,
            cx,
        ));
        col = col.child(sidebar_label(t, tr("透视", "Explore")));
        for s in [Section::Disk, Section::Timeline] {
            col = col.child(self.nav_row(
                t,
                s.symbol(),
                t.secondary(),
                &s.title(),
                None,
                active && self.section == s,
                s,
                None,
                cx,
            ));
        }
        if !self.externals.is_empty() {
            col = col.child(sidebar_label(t, tr("外接磁盘", "External Disks")));
            for (i, e) in self.externals.iter().enumerate() {
                let value = if e.busy {
                    Some(tr("扫描中…", "scanning…"))
                } else if let Some(r) = &e.report {
                    let b: u64 = r.things.iter().map(|t| t.reclaimable()).sum();
                    if b > 0 { Some(fmt_bytes(b)) } else { None }
                } else {
                    None
                };
                col = col.child(self.nav_row(
                    t,
                    "▣",
                    t.accent(),
                    &e.name,
                    value,
                    self.active_external == Some(i),
                    Section::Disk,
                    Some(i),
                    cx,
                ));
            }
        }

        col = col.child(div().flex_1());
        col = col.child(
            div()
                .id("open-settings")
                .flex()
                .flex_row()
                .items_center()
                .gap(px(9.))
                .h(px(30.))
                .px(px(9.))
                .mx(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .hover(|s| s.bg(t.hover()))
                .child(
                    div()
                        .w(px(16.))
                        .flex()
                        .justify_center()
                        .text_color(t.secondary())
                        .child("⚙"),
                )
                .child(text(tr("设置", "Settings"), 13., t.text()))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings_open = true;
                    cx.notify();
                })),
        );
        col = col.child(
            div()
                .px(px(18.))
                .pt(px(4.))
                .text_size(px(10.))
                .text_color(t.tertiary())
                .child(format!(
                    "{} · {}",
                    fmt_bytes(self.volume.free),
                    tr("可用", "free")
                )),
        );
        col
    }

    fn render_toolbar(
        &mut self,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let title = match self.active_external {
            Some(i) => self
                .externals
                .get(i)
                .map(|e| e.name.clone())
                .unwrap_or_default(),
            None => self.section.title(),
        };
        let show_search = self.active_external.is_none() && self.section.kind().is_some();
        let mut bar = div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(46.))
            .px(px(20.))
            .gap(px(12.))
            .border_b_1()
            .border_color(t.border())
            .bg(t.bg())
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(div().flex_1());
        if show_search {
            bar = bar.child(self.render_search(t, window, cx));
        }
        let busy = self.scan.is_some()
            || self.clean.is_some()
            || self.active_external.and_then(|i| self.externals.get(i)).map(|e| e.busy).unwrap_or(false);
        let id = if self.active_external.is_some() {
            "rescan-ext"
        } else {
            "rescan"
        };
        bar = bar.child(
            ghost_button(id, format!("⟳  {}", tr("重新扫描", "Rescan")), t)
                .when(!busy, |el| {
                    el.on_click(cx.listener(|this, _, _, cx| {
                        if let Some(i) = this.active_external {
                            this.scan_external(i, cx);
                        } else {
                            this.start_scan(cx);
                        }
                    }))
                })
                .when(busy, |el| el.opacity(0.5)),
        );
        bar
    }

    fn render_search(
        &mut self,
        t: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let focused = self.search_focus.is_focused(window);
        let entity = cx.entity();
        let input_focus = self.search_focus.clone();
        // Registers the platform input handler while the field is focused, which is
        // what lets IME composition (Chinese, Japanese, …) reach `Root`.
        let input_layer = canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                window.handle_input(
                    &input_focus,
                    ElementInputHandler::new(bounds, entity.clone()),
                    cx,
                );
            },
        )
        .size_full();
        let display = if self.search.is_empty() {
            tr("搜索", "Search").to_string()
        } else {
            self.search.clone()
        };
        let caret = |leading: bool| {
            let c = div().w(px(1.5)).h(px(13.)).flex_none().bg(t.text());
            let c = if leading { c.mr(px(1.)) } else { c.ml(px(1.)) };
            c.with_animation(
                "search-caret",
                Animation::new(Duration::from_millis(1060)).repeat(),
                |el, t| el.opacity(if t < 0.5 { 1.0 } else { 0.0 }),
            )
        };
        let mut field = div()
            .id("search-field")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .w(px(190.))
            .h(px(28.))
            .px(px(9.))
            .rounded(px(8.))
            .bg(t.field())
            .border_1()
            .border_color(if focused {
                t.accent()
            } else if self.search.is_empty() {
                t.border()
            } else {
                alpha(t.accent, 0x99)
            })
            .cursor(CursorStyle::IBeam)
            .track_focus(&self.search_focus)
            .on_key_down(cx.listener(Self::on_search_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.focus(&this.search_focus);
                    cx.notify();
                }),
            )
            .child(div().absolute().inset_0().child(input_layer))
            .child(
                div()
                    .text_color(if focused { t.accent() } else { t.tertiary() })
                    .text_size(px(12.))
                    .child("⌕"),
            )
            .child({
                let mut row = div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_center()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.))
                    .text_color(if self.search.is_empty() {
                        t.tertiary()
                    } else {
                        t.text()
                    });
                if focused && self.search.is_empty() {
                    row = row.child(caret(true));
                }
                row = row.child(display);
                if focused && !self.search.is_empty() {
                    row = row.child(caret(false));
                }
                row
            });
        if !self.search.is_empty() {
            field = field.child(
                div()
                    .id("search-clear")
                    .cursor_pointer()
                    .text_color(t.tertiary())
                    .text_size(px(12.))
                    .child("×")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.search.clear();
                        this.search_marked = 0;
                        cx.notify();
                    })),
            );
        }
        field
    }

    fn render_main(&mut self, t: &Theme, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let mut main = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .relative()
            .overflow_hidden();
        main = main.child(self.render_toolbar(t, window, cx));
        main = main.child(
            div()
                .flex_1()
                .overflow_hidden()
                .relative()
                .child(self.render_content(t, cx)),
        );
        if let Some(phase) = self.scan_phase.clone() {
            main = main.child(
                div()
                    .absolute()
                    .bottom(px(18.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(9.))
                            .px(px(16.))
                            .py(px(9.))
                            .rounded_full()
                            .bg(t.card())
                            .border_1()
                            .border_color(t.border())
                            .shadow_lg()
                            .child(spinner(t))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(t.secondary())
                                    .child(phase),
                            ),
                    ),
            );
        }
        if self.show_selection_bar() {
            main = main.child(self.render_selection_bar(t, cx));
        }
        main
    }

    fn show_selection_bar(&self) -> bool {
        if self.clean.is_some() || self.sheet_open {
            return false;
        }
        match self.active_external {
            Some(i) => self
                .externals
                .get(i)
                .and_then(|e| e.report.as_ref())
                .map(|r| !r.things.is_empty())
                .unwrap_or(false),
            None => match self.section {
                Section::Overview | Section::Disk | Section::Timeline => false,
                Section::AppData => false,
                _ => self.report.is_some(),
            },
        }
    }

    fn render_selection_bar(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let plan = self.plan();
        let mut bar = div()
            .absolute()
            .bottom(px(16.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(16.))
                    .px(px(16.))
                    .py(px(10.))
                    .rounded_2xl()
                    .bg(t.card())
                    .border_1()
                    .border_color(t.border())
                    .shadow_lg()
                    // Own a hitbox so clicks on the bar's padding never fall
                    // through to the list underneath.
                    .occlude()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .child(
                        div()
                            .id("sel-count")
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_sel_list = !this.show_sel_list;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(tr(
                                        &format!("已选 {} 项", plan.items.len()),
                                        &format!("{} selected", plan.items.len()),
                                    ))
                                    .child(
                                        div()
                                            .text_color(t.secondary())
                                            .text_size(px(9.))
                                            .child(if self.show_sel_list { "▴" } else { "▾" }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.secondary())
                                    .child(tr(
                                        &format!("预计腾出 {}", fmt_bytes(plan.freed)),
                                        &format!("frees about {}", fmt_bytes(plan.freed)),
                                    )),
                            ),
                    )
                    .child(
                        primary_button("clean-up", tr("清理…", "Clean Up…"), !plan.items.is_empty(), t)
                            .on_click(cx.listener(|this, _, _, cx| this.open_sheet(cx))),
                    ),
            );
        if self.show_sel_list && !plan.items.is_empty() {
            bar = bar.child(self.render_selected_list(t, 380., cx));
        }
        bar
    }

    fn render_selected_list(&mut self, t: &Theme, max_h: f32, cx: &mut Context<Self>) -> Div {
        let sel = self.selection_ref().clone();
        let mut groups: Vec<(String, Vec<ReclaimItem>)> = vec![];
        for th in self.things() {
            let items: Vec<ReclaimItem> = th
                .items
                .iter()
                .filter(|i| sel.contains(&i.id))
                .cloned()
                .collect();
            if !items.is_empty() {
                groups.push((th.title.clone(), items));
            }
        }
        // Build the inner list first, then put it *inside* the card — appending
        // it to the outer wrapper made it a sibling of the card, which showed
        // up as an empty white panel with the rows floating beside it.
        let mut inner = div().flex().flex_col().gap(px(10.));
        for (title, items) in groups {
            let mut g = div().flex().flex_col().gap(px(2.)).child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.secondary())
                    .child(title),
            );
            for it in items {
                let id = it.id.clone();
                g = g.child(
                    div()
                        .id(SharedString::from(format!("sel-{}", it.id)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .px(px(4.))
                        .py(px(2.))
                        .rounded(px(5.))
                        .hover(|s| s.bg(t.hover()))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_item(&id, cx);
                        }))
                        .child(
                            div()
                                .text_color(t.accent())
                                .text_size(px(12.))
                                .child("☑"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(12.))
                                .child(it.label.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(t.secondary())
                                .child(it.amount()),
                        ),
                );
            }
            inner = inner.child(g);
        }
        div()
            .absolute()
            .bottom(px(72.))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("sel-pop")
                    .w(px(460.))
                    .max_h(px(max_h))
                    .overflow_y_scroll()
                    .rounded_2xl()
                    .bg(t.card())
                    .border_1()
                    .border_color(t.border())
                    .shadow_lg()
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    // Opaque overlay: consume clicks and wheel events so the
                    // page rows underneath don't react to them.
                    .occlude()
                    .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .child(inner),
            )
    }

    fn render_content(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        if let Some(i) = self.active_external {
            return self.render_external_page(t, i, cx).into_any_element();
        }
        match self.section {
            Section::Overview => self.render_overview(t, cx).into_any_element(),
            Section::Disk => self.render_disk_page(t, cx).into_any_element(),
            Section::Timeline => self.render_timeline(t, cx).into_any_element(),
            s => match s.kind() {
                Some(k) => self.render_category(t, k, cx).into_any_element(),
                None => div().into_any_element(),
            },
        }
    }

    fn render_overview(&mut self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let things = self.things().to_vec();
        let plan = self.plan();
        let used = self.volume.used();
        let free = self.volume.free;
        let multi_volume = self
            .tree
            .as_ref()
            .map(|tr| tr.children(tr.root).len() > 1)
            .unwrap_or(false);
        let mut segs: Vec<(String, u64, Rgba)> = vec![];
        let mut sum = 0u64;
        for k in [
            ThingKind::Project,
            ThingKind::Media,
            ThingKind::DevCache,
            ThingKind::Model,
            ThingKind::AppData,
        ] {
            let b = occupied(&things, k);
            if b > 0 {
                segs.push((k.title(), b, rgb(k.color())));
                sum += b;
            }
        }
        let other = used.saturating_sub(sum);
        if other > 0 {
            segs.push((tr("其他", "Other"), other, rgb(0x8e8e93)));
        }
        let undecided = self.total_reclaimable().saturating_sub(plan.freed);
        let sys: Vec<Thing> = things
            .iter()
            .filter(|x| x.kind == ThingKind::System)
            .cloned()
            .collect();

        let mut scroll = div()
            .id("overview-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(26.))
                    .p(px(30.))
                    .max_w(px(1100.))
                    .pb(px(90.))
                    // header
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_end()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(text(tr("可用空间", "Free space"), 16., t.secondary()))
                                    .child(big_number(free, 62., t)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_end()
                                    .gap(px(3.))
                                    .text_size(px(12.))
                                    .text_color(t.secondary())
                                    .child(format!(
                                        "{} {}",
                                        if multi_volume {
                                            tr("所有磁盘", "All disks")
                                        } else {
                                            tr("磁盘", "Disk")
                                        },
                                        fmt_bytes(self.volume.total)
                                    ))
                                    .child(format!(
                                        "{} {}",
                                        tr("已用", "Used"),
                                        fmt_bytes(used)
                                    ))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(5.))
                                            .child(dot(rgb(0x34c759), 6.))
                                            .child(tr("实时", "Live")),
                                    ),
                            ),
                    )
                    // capacity bar + legend
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(capacity_bar(&segs, self.volume.total, 18., t))
                            .child(legend(&segs, free, t)),
                    ),
            );

        // hero
        {
            let selected_groups = self.plan_groups();
            let mut hero = card(t)
                .flex()
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(24.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .gap(px(4.))
                                .child(text(tr("还能放心腾出", "You can safely free"), 16., t.secondary()))
                                .child(big_number(plan.freed, 44., t))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(t.secondary())
                                        .child(tr(
                                            &format!("已经替你勾好 {} 项：都是能重新生成的东西，或者工具自己会清理的缓存。", plan.items.len()),
                                            &format!("{} items are preselected: all rebuildable, or caches the tools clean themselves.", plan.items.len()),
                                        )),
                                )
                                .when(!plan.items.is_empty(), |el| {
                                    el.child(
                                        div()
                                            .pt(px(2.))
                                            .child(
                                                link_button(
                                                    "toggle-picked",
                                                    tr(
                                                        &format!("{} 这 {} 项", if self.show_picked { "收起" } else { "查看" }, plan.items.len()),
                                                        &format!("{} the {} items", if self.show_picked { "Hide" } else { "See" }, plan.items.len()),
                                                    ),
                                                    t,
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.show_picked = !this.show_picked;
                                                    cx.notify();
                                                })),
                                            ),
                                    )
                                })
                                .when(undecided > 1_000_000_000, |el| {
                                    el.child(
                                        div().pt(px(2.)).child(
                                            link_button(
                                                "undecided-link",
                                                tr(
                                                    &format!("另有 {} 等你决定：大文件、还在用的项目…", fmt_bytes(undecided)),
                                                    &format!("{} more awaits your call: large files, active projects…", fmt_bytes(undecided)),
                                                ),
                                                t,
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.open_section(Section::LargeFiles, cx);
                                            })),
                                        ),
                                    )
                                }),
                        )
                        .child(
                            primary_button(
                                "hero-clean",
                                format!("✦  {}", tr("一键清理", "Clean Up")),
                                !plan.items.is_empty(),
                                t,
                            )
                            .when(!plan.items.is_empty(), |el| {
                                el.on_click(cx.listener(|this, _, _, cx| this.open_sheet(cx)))
                            }),
                        ),
                );
            if self.show_picked && !plan.items.is_empty() {
                let mut body = div().flex().flex_col().gap(px(8.));
                for (title, items) in selected_groups {
                    let mut g = div().flex().flex_col().gap(px(2.)).child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.secondary())
                            .child(title),
                    );
                    for it in items {
                        g = g.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex_1()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .text_size(px(12.))
                                        .child(it.label.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(t.secondary())
                                        .child(it.amount()),
                                ),
                        );
                    }
                    body = body.child(g);
                }
                hero = hero.child(
                    div()
                        .id("overview-picked")
                        .border_t_1()
                        .border_color(t.border())
                        .pt(px(10.))
                        .max_h(px(360.))
                        .overflow_y_scroll()
                        .child(body),
                );
            }
            scroll = scroll.child(hero);
        }

        // cards
        let mut grid = div().flex().flex_wrap().gap(px(14.));
        for k in [
            ThingKind::Project,
            ThingKind::Media,
            ThingKind::DevCache,
            ThingKind::Model,
            ThingKind::LargeFile,
            ThingKind::App,
            ThingKind::AppData,
            ThingKind::Browser,
        ] {
            let n = things.iter().filter(|x| x.kind == k).count();
            let reclaim = if k == ThingKind::App {
                occupied(&things, k)
            } else {
                reclaimable(&things, k)
            };
            let selb = selected_bytes(&things, self.selection_ref(), k);
            let subtitle = if k == ThingKind::Browser {
                tr(
                    &format!("{} 个浏览器 · {} 条记录待清", n, things.iter().filter(|x| x.kind == k).flat_map(|x| x.items.iter()).filter_map(|i| i.records).sum::<i64>()),
                    &format!("{} browsers · {} records to erase", n, things.iter().filter(|x| x.kind == k).flat_map(|x| x.items.iter()).filter_map(|i| i.records).sum::<i64>()),
                )
            } else if n == 0 {
                tr("没有发现", "Nothing found")
            } else if selb > 0 {
                tr(
                    &format!("{} 项 · 已选 {}", n, fmt_bytes(selb)),
                    &format!("{} items · {} selected", n, fmt_bytes(selb)),
                )
            } else {
                tr(&format!("{} 项", n), &format!("{} items", n))
            };
            let section = Section::ALL
                .iter()
                .find(|s| s.kind() == Some(k))
                .copied()
                .unwrap_or(Section::Overview);
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("card-{:?}", k)))
                    .w(px(252.))
                    .h(px(122.))
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .p(px(16.))
                    .rounded_2xl()
                    .bg(t.card_alt())
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_section(section, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(7.))
                            .child(
                                div()
                                    .text_color(rgb(k.color()))
                                    .text_size(px(12.))
                                    .child(section.symbol().to_string()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(k.title()),
                            )
                            .child(div().text_color(t.tertiary()).text_size(px(11.)).child("›")),
                    )
                    .child(
                        div()
                            .text_size(px(26.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if k == ThingKind::App {
                                fmt_bytes(occupied(&things, k))
                            } else {
                                fmt_bytes(reclaim)
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .line_height(px(15.))
                            .child(subtitle),
                    ),
            );
        }
        scroll = scroll.child(grid);

        // honesty
        let mut honesty = div()
            .flex()
            .flex_col()
            .gap(px(9.))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr("账算清楚", "Honest numbers")),
            );
        for s in sys {
            let mut row = div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(10.))
                .child(div().text_color(t.secondary()).text_size(px(12.)).child("ⓘ"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(8.))
                                .child(text(s.title.clone(), 13., t.text()))
                                .when(s.bytes > 0, |el| {
                                    el.child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(t.secondary())
                                            .child(fmt_bytes(s.bytes)),
                                    )
                                }),
                        )
                        .when_some(s.detail.clone(), |el, d| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.secondary())
                                    .line_height(px(16.))
                                    .child(d),
                            )
                        }),
                );
            row = row.w_full();
            honesty = honesty.child(row);
        }
        scroll = scroll.child(honesty);
        scroll
    }

    fn plan_groups(&self) -> Vec<(String, Vec<ReclaimItem>)> {
        let sel = self.selection_ref();
        let mut groups = vec![];
        for th in self.things() {
            let items: Vec<ReclaimItem> = th
                .items
                .iter()
                .filter(|i| sel.contains(&i.id))
                .cloned()
                .collect();
            if !items.is_empty() {
                groups.push((th.title.clone(), items));
            }
        }
        groups
    }
}

fn sidebar_label(t: &Theme, s: impl Into<SharedString>) -> Div {
    div()
        .px(px(18.))
        .pt(px(14.))
        .pb(px(4.))
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.tertiary())
        .child(s.into())
}

fn capacity_bar(segs: &[(String, u64, Rgba)], total: u64, height: f32, t: &Theme) -> Div {
    let mut bar = div()
        .flex()
        .flex_row()
        .gap(px(2.))
        .w_full()
        .h(px(height))
        .rounded(px(height / 2.4))
        .bg(t.track())
        .overflow_hidden();
    let total = total.max(1);
    for (_, bytes, color) in segs {
        let frac = (*bytes as f64 / total as f64) as f32;
        if frac <= 0.0 {
            continue;
        }
        bar = bar.child(
            div()
                .h_full()
                .min_w(px(2.))
                .w(relative(frac))
                .bg(*color),
        );
    }
    bar
}

fn legend(segs: &[(String, u64, Rgba)], free: u64, t: &Theme) -> Div {
    let mut row = div().flex().flex_row().flex_wrap().gap_x(px(18.)).gap_y(px(7.));
    for (title, bytes, color) in segs {
        row = row.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(dot(*color, 8.))
                .child(text(title.clone(), 12., t.secondary()))
                .child(text(fmt_bytes(*bytes), 12., t.text())),
        );
    }
    row.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(dot(alpha(t.text, 0x22), 8.))
            .child(text(tr("可用", "Free"), 12., t.secondary()))
            .child(text(fmt_bytes(free), 12., t.text())),
    )
}

// ---------------------------------------------------------------------------
// Category list
// ---------------------------------------------------------------------------

fn kind_blurb(kind: ThingKind) -> String {
    match kind {
        ThingKind::Project => tr(
            "很久没动的项目，它们的依赖和构建产物可以先删掉，源码和 git 不碰；要接着做时运行一条命令就能装回来。",
            "Projects you haven't touched in a while: their dependencies and build output can go. Source and git stay; one command brings them back.",
        ),
        ThingKind::Media => tr(
            "渲染视频时生成的中间帧。只有找到了对应成片（帧数和成片时长 × 帧率吻合）的才会勾选。",
            "Frames rendered on the way to a video. Only folders whose final video was found (frame count matches duration × fps) are selected.",
        ),
        ThingKind::DevCache => tr(
            "各种工具的下载缓存。能用工具自己的清理命令就用它，它最清楚哪些还在用。",
            "Download caches of developer tools. When a tool has its own cleanup command we use it: it knows what is still in use.",
        ),
        ThingKind::Model => tr(
            "本地模型。删掉以后可以重新下载，默认都不勾。",
            "Local models. They can be downloaded again; nothing is preselected.",
        ),
        ThingKind::App => tr(
            "装好的应用，连同它们放在用户目录里的数据。可以整个卸载（应用和数据一起进废纸篓，能放回），也可以保留应用、只删它会自己重建的缓存。打开着的应用不会动，默认都不勾。",
            "Installed apps, with the data they keep in your home folder. Uninstall one (the app and its data go to the Trash, where you can put them back), or keep it and delete only the caches it rebuilds. Apps that are open are left alone; nothing is preselected.",
        ),
        ThingKind::AppData => tr(
            "这些数据属于各个应用，直接删会出问题。这里告诉你在应用里怎么清。",
            "This data belongs to apps and must be cleaned from inside them. Here's how.",
        ),
        ThingKind::LargeFile => tr(
            "个人文件夹里别处没列出的大文件：安装包、视频、磁盘映像、压缩包、模型权重…（AppData 和以点开头的文件夹属于应用和工具，不在这里）。它们是你的文件：都不勾，选中的移到废纸篓，可以放回。",
            "Big files in your home folder that nothing else lists: installers, videos, disk images, archives, model weights… (AppData and dot folders belong to apps and tools, so they're left out). They're your files: nothing is ticked, and what you pick goes to the Trash, where you can put it back.",
        ),
        ThingKind::Browser => tr(
            "浏览器记下的浏览记录、下载记录、Cookie、表单内容和上次的标签页。清掉的记录找不回来，所以都不勾；浏览器开着时不动它，先退出再清。保存的密码、书签、地址、银行卡和扩展一律不碰。",
            "What browsers remember: history, downloads, cookies, form entries and your last tabs. Erased records can't be brought back, so nothing is ticked, and a browser that's open is left alone: quit it first. Saved passwords, bookmarks, addresses, cards and extensions are never touched.",
        ),
        ThingKind::System => String::new(),
    }
}

impl Root {
    fn render_category(&mut self, t: &Theme, kind: ThingKind, cx: &mut Context<Self>) -> Stateful<Div> {
        let things = self.filtered_things(kind);
        let sel = self.selection_ref().clone();
        let expanded = self.expanded.clone();
        let big = if kind == ThingKind::App || kind == ThingKind::AppData {
            occupied(&things, kind)
        } else if kind == ThingKind::Browser {
            things.iter().map(|t| t.reclaimable()).sum()
        } else {
            reclaimable(&things, kind)
        };
        let label = match kind {
            ThingKind::AppData => tr("被应用数据占用", "used by app data"),
            ThingKind::App => tr("被应用和它们的数据占用", "used by apps and their data"),
            ThingKind::Browser => tr("缓存可以腾出", "of cache can go"),
            ThingKind::LargeFile => tr("移到废纸篓、清倒后可以腾出", "freed once moved to the Trash and emptied"),
            _ => tr("可以腾出", "reclaimable"),
        };

        let mut list = div().flex().flex_col();
        for th in &things {
            list = list.child(self.render_thing_row(t, th, &sel, expanded.contains(&th.id), None, cx));
            list = list.child(div().h(px(1.)).bg(alpha(t.border, 0x88)));
        }
        if things.is_empty() {
            list = list.child(
                div()
                    .py(px(40.))
                    .text_size(px(13.))
                    .text_color(t.secondary())
                    .child(if self.search.is_empty() {
                        tr("没有发现可清理的内容。", "Nothing found here.")
                    } else {
                        tr("没有匹配的内容。", "Nothing matches your search.")
                    }),
            );
        }

        div()
            .id("category-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(28.))
                    .pt(px(18.))
                    .pb(px(96.))
                    .max_w(px(1100.))
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_end()
                            .gap(px(10.))
                            .child(big_number(big, 40., t))
                            .child(
                                div()
                                    .pb(px(5.))
                                    .text_size(px(13.))
                                    .text_color(t.secondary())
                                    .child(label),
                            ),
                    )
                    .child(
                        div()
                            .pb(px(8.))
                            .text_size(px(12.))
                            .text_color(t.secondary())
                            .line_height(px(18.))
                            .child(kind_blurb(kind)),
                    )
                    .child(list),
            )
    }

    fn render_thing_row(
        &mut self,
        t: &Theme,
        thing: &Thing,
        sel: &HashSet<String>,
        expanded: bool,
        external: Option<usize>,
        cx: &mut Context<Self>,
    ) -> Div {
        let selectable: Vec<String> = thing
            .items
            .iter()
            .filter(|i| i.tier < Tier::Guide)
            .map(|i| i.id.clone())
            .collect();
        let n = selectable.iter().filter(|i| sel.contains(*i)).count();
        let color = rgb(thing.kind.color());
        let th = thing.clone();

        let mut head = div()
            .id(SharedString::from(format!("grp-{}", thing.id)))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .py(px(8.))
            .cursor_pointer()
            .rounded(px(8.))
            .hover(|s| s.bg(alpha(t.hover, 0xcc)))
            .on_click(cx.listener(move |this, _, _, cx| {
                match external {
                    Some(i) => {
                        if let Some(e) = this.externals.get_mut(i) {
                            if e.expanded.contains(&th.id) {
                                e.expanded.remove(&th.id);
                            } else {
                                e.expanded.insert(th.id.clone());
                            }
                        }
                    }
                    None => {
                        if this.expanded.contains(&th.id) {
                            this.expanded.remove(&th.id);
                        } else {
                            this.expanded.insert(th.id.clone());
                        }
                    }
                }
                cx.notify();
            }))
            .child(
                div()
                    .w(px(12.))
                    .text_color(t.tertiary())
                    .text_size(px(10.))
                    .child(if expanded { "▾" } else { "▸" }),
            );

        if selectable.is_empty() {
            head = head.child(
                div()
                    .w(px(18.))
                    .text_color(rgb(0x30b0c7))
                    .text_size(px(13.))
                    .child("☀"),
            );
        } else {
            let ids: Vec<String> = selectable.clone();
            let all = selectable.len();
            head = head.child(
                div()
                    .id(SharedString::from(format!("chk-{}", thing.id)))
                    .w(px(18.))
                    .text_size(px(16.))
                    .text_color(if n == 0 { t.secondary() } else { color })
                    .cursor_pointer()
                    .child(if n == 0 {
                        "○"
                    } else if n == all {
                        "●"
                    } else {
                        "◐"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let sel = this.selection_ref();
                        let all = ids.iter().filter(|i| sel.contains(*i)).count() == ids.len();
                        this.set_items(ids.clone(), !all, cx);
                    })),
            );
        }

        if thing.kind == ThingKind::App {
            head = head.child(app_tile(&thing.title, 24.));
        } else if thing.kind == ThingKind::Browser {
            head = head.child(app_tile(&thing.title, 24.));
        } else if thing.kind == ThingKind::LargeFile {
            head = head.child(
                div()
                    .w(px(26.))
                    .text_color(color)
                    .text_size(px(15.))
                    .child(symbol_for_large(&thing.id)),
            );
        }

        let mut subtitle_parts: Vec<String> = vec![];
        if let Some(last) = thing.last_active {
            subtitle_parts.push(if thing.kind == ThingKind::App {
                tr(
                    &format!("打开于 {}", fmt_ago(Some(last))),
                    &format!("opened {}", fmt_ago(Some(last))),
                )
            } else {
                let base = tr(
                    &format!("活跃于 {}", fmt_ago(Some(last))),
                    &format!("active {}", fmt_ago(Some(last))),
                );
                match &thing.activity_source {
                    Some(s) => format!("{} · {}", base, s),
                    None => base,
                }
            });
        }
        if let Some(p) = &thing.path {
            subtitle_parts.push(fmt_home(p));
        }
        if thing.kind == ThingKind::LargeFile {
            if let Some(d) = &thing.detail {
                subtitle_parts.push(d.clone());
            }
        }
        let subtitle = subtitle_parts.join("  ·  ");
        let amount = if thing.kind == ThingKind::AppData {
            thing.bytes
        } else {
            thing.reclaimable()
        };

        head = head
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(7.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(thing.title.clone()),
                            )
                            .when(
                                matches!(thing.kind, ThingKind::Project | ThingKind::App | ThingKind::Browser),
                                |el| {
                                    el.when_some(thing.detail.clone(), |el, d| {
                                        el.child(
                                            div()
                                                .px(px(6.))
                                                .py(px(1.))
                                                .rounded_full()
                                                .bg(alpha(0x5e5ce6, 0x26))
                                                .text_color(rgb(0x5e5ce6))
                                                .text_size(px(9.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(d),
                                        )
                                    })
                                },
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(subtitle),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(fmt_bytes(amount)),
                    )
                    .when(
                        thing.kind != ThingKind::AppData
                            && thing.kind != ThingKind::LargeFile
                            && thing.bytes != thing.reclaimable(),
                        |el| {
                            el.child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(t.tertiary())
                                    .child(tr(
                                        &format!("占用 {}", fmt_bytes(thing.bytes)),
                                        &format!("uses {}", fmt_bytes(thing.bytes)),
                                    )),
                            )
                        },
                    ),
            );

        let mut col = div().flex().flex_col().py(px(2.)).child(head);
        if expanded {
            let mut body = div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pl(px(56.))
                .pb(px(8.));
            if matches!(thing.kind, ThingKind::AppData | ThingKind::DevCache) {
                if let Some(d) = &thing.detail {
                    body = body.child(
                        div()
                            .py(px(2.))
                            .text_size(px(12.))
                            .text_color(t.secondary())
                            .line_height(px(17.))
                            .child(d.clone()),
                    );
                }
            }
            if thing.kind != ThingKind::AppData {
                for item in &thing.items {
                    body = body.child(self.render_item_row(t, item, thing.path.as_deref(), cx));
                }
            }
            col = col.child(body);
        }
        col
    }

    fn render_item_row(
        &mut self,
        t: &Theme,
        item: &ReclaimItem,
        parent_path: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let on = self.selection_ref().contains(&item.id);
        let id = item.id.clone();
        let amount = item.amount();
        let mut left = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(2.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(item.label.clone()),
                    )
                    .when(item.tier != Tier::Guide, |el| el.child(tier_badge(item.tier))),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.secondary())
                    .line_height(px(16.))
                    .child(match &item.restore {
                        Some(r) => format!("{} · {}{}", item.reason, tr("恢复：", "restore: "), r),
                        None => item.reason.clone(),
                    }),
            );
        if let Some(p) = &item.path {
            if parent_path != Some(p.as_str()) && item.app.is_none() {
                left = left.child(
                    div()
                        .text_size(px(10.))
                        .text_color(t.tertiary())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(fmt_home(p)),
                );
            }
        }
        if item.app.is_some() && item.action.is_trash_only() {
            let mut places = div().flex().flex_col().gap(px(1.));
            for p in item.action.paths().iter().skip(1).take(6) {
                places = places.child(
                    div()
                        .text_size(px(10.))
                        .text_color(t.tertiary())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(fmt_home(p)),
                );
            }
            left = left.child(places);
        }
        for w in &item.warnings {
            left = left.child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(0xff9f0a))
                    .line_height(px(15.))
                    .child(format!("⚠  {}", w)),
            );
        }

        let mut row = div()
            .id(SharedString::from(format!("itm-{}", item.id)))
            .flex()
            .flex_row()
            .items_start()
            .gap(px(10.))
            .py(px(5.))
            .cursor_pointer()
            .rounded(px(6.))
            .hover(|s| s.bg(alpha(t.hover, 0x99)))
            .child(
                div()
                    .w(px(15.))
                    .text_size(px(13.))
                    .text_color(if on { t.accent() } else { t.secondary() })
                    .child(if item.tier == Tier::Guide {
                        ""
                    } else if on {
                        "☑"
                    } else {
                        "☐"
                    }),
            )
            .child(left)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.secondary())
                    .child(amount),
            );
        if item.tier != Tier::Guide {
            row = row.on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_item(&id, cx);
            }));
        }
        row
    }

    // ---------------------------------------------------------------
    // Clean Up confirmation sheet
    // ---------------------------------------------------------------

    fn render_clean_sheet(&mut self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let plan = self.plan();
        let group_titles: HashMap<String, String> = self
            .things()
            .iter()
            .flat_map(|th| {
                th.items
                    .iter()
                    .map(move |i| (i.id.clone(), th.title.clone()))
            })
            .collect();

        let uninstall: Vec<&ReclaimItem> = plan
            .items
            .iter()
            .filter(|i| i.app.is_some() && i.action.is_trash_only())
            .collect();
        let erase: Vec<&ReclaimItem> = plan
            .items
            .iter()
            .filter(|i| i.tier == Tier::Erase)
            .collect();
        let rest: Vec<&ReclaimItem> = plan
            .items
            .iter()
            .filter(|i| !(i.app.is_some() && i.action.is_trash_only()) && i.tier != Tier::Erase)
            .collect();
        let deletes: Vec<&ReclaimItem> = rest
            .iter()
            .copied()
            .filter(|i| {
                matches!(
                    &i.action,
                    ReclaimAction::Delete { .. } | ReclaimAction::DeleteAll { .. }
                ) || (matches!(&i.action, ReclaimAction::Compound { .. })
                    && !i.action.is_trash_only())
            })
            .collect();
        let commands: Vec<&ReclaimItem> = rest
            .iter()
            .copied()
            .filter(|i| matches!(&i.action, ReclaimAction::Command { .. }))
            .collect();
        let trashes: Vec<&ReclaimItem> = rest
            .iter()
            .copied()
            .filter(|i| i.action.is_trash_only())
            .collect();

        let mut groups: Vec<(String, String, Vec<&ReclaimItem>)> = vec![];
        if !erase.is_empty() {
            groups.push((
                tr("永久清除浏览器记录", "Erase browser records"),
                tr("清掉就找不回来；浏览器开着的会跳过", "gone for good; skipped while the browser is open"),
                erase,
            ));
        }
        if !uninstall.is_empty() {
            groups.push((
                tr("卸载应用", "Uninstall apps"),
                tr("应用和它的数据移到废纸篓，可以放回", "each app and its data go to the Trash; you can put them back"),
                uninstall,
            ));
        }
        if !deletes.is_empty() {
            groups.push((
                tr("直接删除", "Delete"),
                tr("能重新生成，恢复方法记在日志里", "rebuildable; restore steps are logged"),
                deletes,
            ));
        }
        if !commands.is_empty() {
            groups.push((
                tr("运行工具自带的清理命令", "Run the tool's own cleanup"),
                tr("工具自己知道哪些还在用", "the tool knows what is still in use"),
                commands,
            ));
        }
        if !trashes.is_empty() {
            groups.push((
                tr("移到废纸篓", "Move to Trash"),
                tr("你的文件，可以从废纸篓找回", "your files; recoverable from the Trash"),
                trashes,
            ));
        }

        let mut body = div().flex().flex_col().gap(px(12.));
        for (title, desc, items) in groups {
            let mut g = div().flex().flex_col().gap(px(4.)).child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(t.secondary())
                            .child(desc),
                    ),
            );
            for it in items {
                let where_ = it
                    .path
                    .as_ref()
                    .map(|p| fmt_home(p))
                    .or_else(|| group_titles.get(&it.id).cloned())
                    .unwrap_or_default();
                g = g.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(it.label.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(t.tertiary())
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(where_),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(t.secondary())
                                .child(it.amount()),
                        ),
                );
            }
            body = body.child(g);
        }

        let count = plan.items.len();
        let freed = plan.freed;
        div()
            .id("sheet-dim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000066))
            .occlude()
            .on_click(cx.listener(|this, _, _, cx| {
                this.sheet_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .id("sheet-card")
                    .w(px(560.))
                    .max_h(px(620.))
                    .bg(t.card())
                    .border_1()
                    .border_color(t.border())
                    .rounded_2xl()
                    .shadow_lg()
                    .p(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_end()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(tr(
                                        &format!("清理 {} 项", count),
                                        &format!("Clean up {} items", count),
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .text_color(t.secondary())
                                    .child(tr(
                                        &format!("约腾出 {}", fmt_bytes(freed)),
                                        &format!("≈ {}", fmt_bytes(freed)),
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("sheet-scroll")
                            .max_h(px(380.))
                            .overflow_y_scroll()
                            .child(body),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(t.secondary())
                            .line_height(px(15.))
                            .child(tr(
                                "受保护的文件不会被删。遇到程序占用时，这一项会跳过并告诉你原因。",
                                "Protected files are never deleted. If a program is using an item, it is skipped and the reason is shown.",
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                ghost_button("sheet-cancel", tr("取消", "Cancel"), t)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sheet_open = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                primary_button("sheet-go", tr("清理", "Clean Up"), true, t)
                                    .on_click(cx.listener(|this, _, _, cx| this.begin_clean(cx))),
                            ),
                    ),
            )
    }

    // ---------------------------------------------------------------
    // Full-window clean run
    // ---------------------------------------------------------------

    fn render_clean(&mut self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(live) = &self.clean else {
            return div().into_any_element();
        };
        let items = live.plan.items.clone();
        let states = live.states.clone();
        let current = live.current.clone();
        let freed = live.freed;
        let estimate = live.estimate;
        let result = live.result.clone();
        let started = live.started;
        let disk_name = live.disk_name.clone();
        let cancelled = live.cancel.load(Ordering::Relaxed);

        let colors: HashMap<String, Rgba> = self
            .things()
            .iter()
            .flat_map(|th| {
                let c = rgb(th.kind.color());
                th.items.iter().map(move |i| (i.id.clone(), c))
            })
            .collect();
        let accent = t.accent();
        let gray = rgb(0x8e8e93);

        let done = states.values().filter(|s| matches!(s, ItemState::Done)).count();
        let failed = states
            .values()
            .filter(|s| matches!(s, ItemState::Failed(_)))
            .count();
        let skipped = states
            .values()
            .filter(|s| matches!(s, ItemState::Skipped(_)))
            .count();
        let total = items.len();
        let current_item = current
            .as_ref()
            .and_then(|id| items.iter().find(|i| &i.id == id));
        let finished = result.is_some();
        let title = if let Some(_res) = &result {
            if done == 0 {
                tr("什么都没清理", "Nothing was cleaned")
            } else if failed > 0 {
                tr(
                    &format!("清理完成，有 {} 项没清掉", failed),
                    &format!("Done, but {} failed", failed),
                )
            } else if cancelled && done < total {
                tr("已停止", "Stopped")
            } else {
                tr("清理完成", "All clean")
            }
        } else {
            current_item
                .map(|i| {
                    tr(
                        &format!("正在清理「{}」", i.label),
                        &format!("Cleaning “{}”", i.label),
                    )
                })
                .unwrap_or_else(|| tr("正在准备…", "Getting ready…"))
        };
        let subtitle = if let Some(res) = &result {
            let secs = (now_ts() - started).max(1);
            let mut parts: Vec<String> = vec![tr(
                &format!("完成 {} 项", done),
                &format!("{} done", done),
            )];
            if failed > 0 {
                parts.push(tr(
                    &format!("{} 项没清掉", failed),
                    &format!("{} failed", failed),
                ));
            }
            if skipped > 0 {
                parts.push(tr(
                    &format!("{} 项跳过", skipped),
                    &format!("{} skipped", skipped),
                ));
            }
            let _ = res;
            parts.push(if secs < 60 {
                tr(&format!("用时 {} 秒", secs), &format!("took {} s", secs))
            } else {
                tr(
                    &format!("用时 {} 分 {} 秒", secs / 60, secs % 60),
                    &format!("took {} min {} s", secs / 60, secs % 60),
                )
            });
            if let Some(d) = &disk_name {
                parts.push(format!("「{}」", d));
            }
            parts.join(if is_zh() { "，" } else { ", " })
        } else {
            let n = done + failed + skipped + 1;
            let mut s = tr(
                &format!("第 {} 项，共 {} 项", n.min(total), total),
                &format!("Item {} of {}", n.min(total), total),
            );
            if let Some(d) = &disk_name {
                s.push_str(&format!(" · 「{}」", d));
            }
            if cancelled {
                s.push_str(&tr(" · 正在停下", " · stopping"));
            }
            s
        };

        // Trash-only bytes among what was done.
        let mut trashed = 0u64;
        if finished {
            for it in &items {
                if it.action.is_trash_only()
                    && matches!(states.get(&it.id), Some(ItemState::Done))
                {
                    trashed += it.estimate;
                }
            }
        }

        // Ring geometry in fractions.
        let floor = (items.iter().map(|i| i.estimate).max().unwrap_or(1) as f64 / 4.0).max(1.0);
        let weights: Vec<f64> = items
            .iter()
            .map(|i| (i.estimate as f64).max(floor))
            .collect();
        let sumw: f64 = weights.iter().sum::<f64>().max(1.0);
        let gap = if items.len() > 1 {
            (0.004f64).min(0.3 * (weights.iter().cloned().fold(f64::INFINITY, f64::min) / sumw))
        } else {
            0.0
        };
        let mut arcs: Vec<(f32, f32, Rgba, bool, bool)> = vec![];
        let mut acc = 0.0f64;
        for (n, it) in items.iter().enumerate() {
            let len = weights[n] / sumw;
            let a0 = acc + gap / 2.0;
            let a1 = (acc + len - gap / 2.0).max(a0);
            acc += len;
            let state = states.get(&it.id).cloned().unwrap_or(ItemState::Pending);
            let (color, lit, running) = match &state {
                ItemState::Done => (*colors.get(&it.id).unwrap_or(&accent), true, false),
                ItemState::Running => (accent, false, true),
                _ => (fade(gray, 0.4), false, false),
            };
            arcs.push((
                (a0 * std::f64::consts::TAU) as f32,
                (a1 * std::f64::consts::TAU) as f32,
                color,
                lit,
                running,
            ));
        }
        let track_color = alpha(t.text, 0x12);
        let ring = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let cx0 = f32::from(bounds.center().x);
                let cy0 = f32::from(bounds.center().y);
                let ro = f32::from(bounds.size.width) / 2.0;
                let ri = ro * 0.86;
                let track = arc_path(cx0, cy0, ro, ri, 0.0, std::f32::consts::TAU);
                if let Ok(p) = track {
                    window.paint_path(p, track_color);
                }
                for (a0, a1, color, lit, running) in &arcs {
                    let end = if *lit || *running { *a1 } else { *a0 };
                    if end - *a0 <= 0.0001 {
                        continue;
                    }
                    if let Ok(p) = arc_path(cx0, cy0, ro, ri, *a0, end) {
                        window.paint_path(p, *color);
                    }
                }
            },
        )
        .size(px(230.));

        let mut list_rows = div().flex().flex_col();
        for it in &items {
            let state = states.get(&it.id).cloned().unwrap_or(ItemState::Pending);
            let (icon, icon_color) = match &state {
                ItemState::Done => ("✓", *colors.get(&it.id).unwrap_or(&accent)),
                ItemState::Running => ("◌", accent),
                ItemState::Failed(_) => ("!", rgb(0xff9f0a)),
                ItemState::Skipped(_) => ("–", t.secondary()),
                _ => ("○", t.tertiary()),
            };
            let msg = match &state {
                ItemState::Failed(m) => Some((m.clone(), rgb(0xff9f0a))),
                ItemState::Skipped(m) => Some((m.clone(), t.secondary())),
                _ => None,
            };
            let where_ = it.path.as_ref().map(|p| fmt_home(p)).unwrap_or_default();
            list_rows = list_rows.child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(10.))
                    .px(px(16.))
                    .py(px(8.))
                    .when(
                        !matches!(
                            &state,
                            ItemState::Done
                                | ItemState::Running
                                | ItemState::Failed(_)
                                | ItemState::Skipped(_)
                        ),
                        |el| el.opacity(0.55),
                    )
                    .child(
                        div()
                            .w(px(16.))
                            .text_size(px(13.))
                            .text_color(icon_color)
                            .child(icon),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(it.label.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(t.tertiary())
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(where_),
                                    ),
                            )
                            .when_some(msg, |el, (m, c)| {
                                el.child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(c)
                                        .line_height(px(14.))
                                        .child(m),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .child(it.amount()),
                    ),
            );
        }

        let mut page = div()
            .id("clean-page")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(0.))
            .bg(t.bg())
            .px(px(40.))
            .child(
                div()
                    .relative()
                    .size(px(230.))
                    .child(ring)
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(2.))
                            .child(if finished {
                                div()
                                    .text_size(px(22.))
                                    .text_color(if failed > 0 { rgb(0xff9f0a) } else { rgb(0x34c759) })
                                    .child(if failed > 0 { "⚠" } else { "✓" })
                            } else {
                                div()
                            })
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.secondary())
                                    .child(if finished {
                                        tr("腾出了", "Freed")
                                    } else {
                                        tr("已腾出", "Freed so far")
                                    }),
                            )
                            .child(big_number(freed, 36., t))
                            .child(
                                div()
                                    .text_size(px(9.))
                                    .text_color(t.tertiary())
                                    .child(tr(
                                        &format!("预计约 {}", fmt_bytes(estimate)),
                                        &format!("about {} expected", fmt_bytes(estimate)),
                                    )),
                            ),
                    ),
            )
            .child(
                div()
                    .pt(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.secondary())
                            .child(subtitle),
                    )
                    .when(finished && trashed > 0, |el| {
                        el.child(
                            div()
                                .pt(px(3.))
                                .text_size(px(11.))
                                .text_color(t.secondary())
                                .child(tr(
                                    &format!("移到废纸篓的约 {} 要清空废纸篓后才会腾出来", fmt_bytes(trashed)),
                                    &format!("About {} moved to the Trash comes back when you empty it", fmt_bytes(trashed)),
                                )),
                        )
                    }),
            )
            .child(
                div()
                    .pt(px(18.))
                    .w(px(580.))
                    .max_h(px(320.))
                    .rounded_2xl()
                    .bg(t.card_alt())
                    .border_1()
                    .border_color(t.border())
                    .overflow_hidden()
                    .child(
                        div()
                            .id("clean-list")
                            .size_full()
                            .max_h(px(320.))
                            .overflow_y_scroll()
                            .py(px(5.))
                            .child(list_rows),
                    ),
            )
            .child(
                div().pt(px(18.)).child(if finished {
                    ghost_button("clean-done", tr("完成", "Done"), t)
                        .on_click(cx.listener(|this, _, _, cx| this.close_clean(cx)))
                } else {
                    ghost_button(
                        "clean-stop",
                        if cancelled {
                            tr("正在停止…", "Stopping…")
                        } else {
                            tr("停止", "Stop")
                        },
                        t,
                    )
                    .when(!cancelled, |el| {
                        el.on_click(cx.listener(|this, _, _, cx| this.stop_clean(cx)))
                    })
                }),
            );
        page = page.child(
            div()
                .text_size(px(10.))
                .text_color(t.tertiary())
                .py(px(14.))
                .child(tr(
                    "正在清理时窗口不会关闭；每清完一项，上面的环就多亮一段。",
                    "The window stays open while cleaning; each finished item lights another arc.",
                )),
        );
        page.into_any_element()
    }
}

fn arc_path(cx: f32, cy: f32, ro: f32, ri: f32, a0: f32, a1: f32) -> Result<gpui::Path<Pixels>, ()> {
    let steps = (((a1 - a0).abs() * ro / 3.0) as usize).clamp(4, 240);
    let mut pts = vec![];
    for k in 0..=steps {
        let a = a0 + (a1 - a0) * k as f32 / steps as f32;
        pts.push(point(px(cx + ro * a.cos()), px(cy + ro * a.sin())));
    }
    for k in (0..=steps).rev() {
        let a = a0 + (a1 - a0) * k as f32 / steps as f32;
        pts.push(point(px(cx + ri * a.cos()), px(cy + ri * a.sin())));
    }
    let mut builder = PathBuilder::fill();
    builder.add_polygon(&pts, true);
    builder.build().map_err(|_| ())
}

// ---------------------------------------------------------------------------
// Treemap (Whole Disk)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum MapTarget {
    Startup,
    External(usize),
}

impl Root {
    fn render_disk_page(&mut self, t: &Theme, cx: &mut Context<Self>) -> Div {
        let Some(tree) = &self.tree else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr("第一次需要扫描整块磁盘", "The first run scans the whole disk")),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.secondary())
                        .child(tr(
                            "还没扫描完。扫描结束后这里会显示磁盘的方块图。",
                            "The scan hasn't finished yet. The disk map appears here when it's done.",
                        )),
                )
                .when_some(self.scan_phase.clone(), |el, p| {
                    el.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(9.))
                            .child(spinner(t))
                            .child(text(p, 12., t.secondary())),
                    )
                });
        };
        // With several scanned volumes, open the map at the container root so
        // every drive is visible side by side; a single volume still opens at
        // the home folder like the reference app.
        let multi = tree.children(tree.root).len() > 1;
        let start = if multi {
            tree.path(tree.root)
        } else {
            home_string()
        };
        let label = if self.volume.name.trim().is_empty() {
            tr("磁盘", "Disk").to_string()
        } else {
            self.volume.name.clone()
        };
        let things = self.things().to_vec();
        self.render_map(t, tree, &start, &label, MapTarget::Startup, &things, cx)
    }

    fn set_map_stack(&mut self, target: MapTarget, paths: &[String]) {
        match target {
            MapTarget::Startup => self.map_stack = paths.to_vec(),
            MapTarget::External(i) => {
                if let Some(e) = self.externals.get_mut(i) {
                    e.map_stack = paths.to_vec();
                }
            }
        }
    }

    fn push_map_stack(&mut self, target: MapTarget, path: &str) {
        match target {
            MapTarget::Startup => self.map_stack.push(path.to_string()),
            MapTarget::External(i) => {
                if let Some(e) = self.externals.get_mut(i) {
                    e.map_stack.push(path.to_string());
                }
            }
        }
    }

    fn map_items_at(&self, path: &str) -> Vec<(String, String, String)> {
        let mut out = vec![];
        for th in self.things() {
            for i in &th.items {
                if i.tier < Tier::Guide
                    && (i.path.as_deref() == Some(path)
                        || i.action.paths().iter().any(|p| p == path))
                {
                    out.push((i.id.clone(), i.label.clone(), i.reason.clone()));
                }
            }
        }
        out
    }

    fn render_map(
        &self,
        t: &Theme,
        tree: &ScanTree,
        start: &str,
        disk_label: &str,
        target: MapTarget,
        things: &[Thing],
        cx: &mut Context<Self>,
    ) -> Div {
        let stack: Vec<String> = match target {
            MapTarget::Startup => self.map_stack.clone(),
            MapTarget::External(i) => self
                .externals
                .get(i)
                .map(|e| e.map_stack.clone())
                .unwrap_or_default(),
        };
        let base = tree.find(start).unwrap_or(tree.root);
        let current = stack
            .last()
            .and_then(|p| tree.find(p))
            .filter(|n| tree.size(*n) > 0 || !tree.children(*n).is_empty())
            .unwrap_or(base);

        // The crumb trail always runs from the volume root down to the current
        // folder, whether we opened the page or drilled into it.
        // A synthetic root (real scans) is a container, not a folder; a demo
        // tree's root is the home folder itself and belongs in the trail.
        // When several volumes were scanned the container root is part of the
        // trail, so it can be reached from a drive.
        let multi_root = tree.children(tree.root).len() > 1;
        let mut chain = vec![current];
        let mut node = current;
        while let Some(p) = tree.parent(node) {
            if p == tree.root {
                if !tree.path(p).is_empty() || multi_root {
                    chain.push(p);
                }
                break;
            }
            chain.push(p);
            node = p;
        }
        chain.reverse();

        // kind lookup
        let mut kind_at: HashMap<String, ThingKind> = HashMap::new();
        for th in things {
            if th.kind == ThingKind::Browser {
                continue;
            }
            if let Some(p) = &th.path {
                kind_at.insert(crate::scan::path_key(p), th.kind);
            }
            for it in &th.items {
                if let Some(p) = &it.path {
                    kind_at.insert(crate::scan::path_key(p), th.kind);
                }
            }
        }

        // breadcrumb
        let mut crumbs = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .px(px(20.))
            .py(px(10.))
            .text_size(px(12.))
            .flex_shrink_0();
        for (i, n) in chain.iter().enumerate() {
            if i > 0 {
                crumbs = crumbs.child(div().text_color(t.tertiary()).child("›"));
            }
            let label = if *n == tree.root && tree.path(*n).is_empty() {
                tr("所有磁盘", "All Disks").to_string()
            } else if i == 0 {
                disk_label.to_string()
            } else if multi_root && tree.parent(*n) == Some(tree.root) {
                tree.name(*n)
                    .trim_end_matches(['\\', '/'])
                    .to_string()
            } else {
                tree.name(*n).to_string()
            };
            let is_current = i + 1 == chain.len();
            let paths: Vec<String> = chain.iter().take(i + 1).map(|n| tree.path(*n)).collect();
            crumbs = crumbs.child(
                div()
                    .id(SharedString::from(format!("crumb-{}", i)))
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover(|s| s.bg(t.hover()))
                    .font_weight(if is_current {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if is_current { t.text() } else { t.secondary() })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_map_stack(target, &paths);
                        this.map_picked = None;
                        this.map_menu = None;
                        cx.notify();
                    })),
            );
        }
        crumbs = crumbs
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(t.secondary())
                    .child(fmt_bytes(tree.size(current))),
            );

        // Lay the blocks out in estimated pixel space so nested children keep
        // the real aspect ratio of the map area; percentages stretch on resize.
        let vw = f64::from(self.viewport.width);
        let vh = f64::from(self.viewport.height);
        let mw = (vw - 234.0 - 32.0).max(360.0);
        let mh = (vh - 46.0 - 40.0 - 40.0).max(240.0);
        let mut blocks: Vec<MapBlock> = vec![];
        layout_map(tree, current, (0.0, 0.0, mw, mh), 0, &kind_at, &mut blocks);
        let blocks = std::rc::Rc::new(blocks);

        // The canvas captures the map's window-relative bounds so mouse
        // positions can be turned into block hit tests (topmost block wins).
        let bounds_cell: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>> =
            std::rc::Rc::new(std::cell::Cell::new(None));
        let bounds_paint = bounds_cell.clone();
        let bounds_layer = canvas(
            |_, _, _| (),
            move |bounds, _, _, _| bounds_paint.set(Some(bounds)),
        )
        .absolute()
        .inset_0();

        let mut layer = div().absolute().inset_0().child(bounds_layer);
        for b in blocks.iter() {
            let hovered = self.map_hovered.as_deref() == Some(b.path.as_str());
            let picked = self.map_picked.as_deref() == Some(b.path.as_str());
            let fx = (b.x / mw) as f32;
            let fy = (b.y / mh) as f32;
            let fw = (b.w / mw) as f32;
            let fh = (b.h / mh) as f32;
            let fill = if hovered {
                if b.depth == 0 {
                    0.30
                } else {
                    0.85
                }
            } else if b.depth == 0 {
                0.18
            } else {
                0.55
            };
            layer = layer.child(
                div()
                    .absolute()
                    .left(relative(fx))
                    .top(relative(fy))
                    .w(relative(fw))
                    .h(relative(fh))
                    .rounded(px(if b.depth == 0 { 7. } else { 4. }))
                    .bg(fade(b.color, fill))
                    .overflow_hidden()
                    .p(px(if b.depth == 0 { 5. } else { 4. }))
                    .when(picked, |el| el.border_1().border_color(t.text()))
                    .when(b.w > 54.0 && b.h > 18.0, |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(4.))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(
                                    div()
                                        .text_size(px(if b.depth == 0 { 11. } else { 10.5 }))
                                        .font_weight(if b.depth == 0 {
                                            FontWeight::SEMIBOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(t.text())
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(b.name.clone()),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(10.))
                                        .text_color(t.secondary())
                                        .child(fmt_bytes(b.size)),
                                ),
                        )
                    }),
            );
        }

        let mut area = div()
            .id("map-area")
            .flex_1()
            .min_h(px(240.))
            .mx(px(16.))
            .mb(px(4.))
            .relative()
            .rounded(px(12.))
            .overflow_hidden()
            .bg(t.card())
            .child(layer)
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.map_hovered.is_some() {
                    this.map_hovered = None;
                    cx.notify();
                }
            }));

        {
            let blocks = blocks.clone();
            let bounds = bounds_cell.clone();
            area = area.on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                let hit = match bounds.get() {
                    Some(b) => map_hit(&blocks, ev.position, b, mw, mh),
                    None => None,
                };
                let path = hit.map(|i| blocks[i].path.clone());
                if this.map_hovered != path {
                    this.map_hovered = path;
                    cx.notify();
                }
            }));
        }
        {
            let blocks = blocks.clone();
            let bounds = bounds_cell.clone();
            area = area.on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                if this.map_menu.is_some() {
                    this.map_menu = None;
                    cx.notify();
                    return;
                }
                let hit = match bounds.get() {
                    Some(b) => map_hit(&blocks, ev.position(), b, mw, mh),
                    None => None,
                };
                let Some(i) = hit else {
                    this.map_picked = None;
                    cx.notify();
                    return;
                };
                let b = &blocks[i];
                if b.drillable {
                    let path = b.path.clone();
                    this.push_map_stack(target, &path);
                    this.map_picked = None;
                    this.map_hovered = None;
                } else if this.map_picked.as_deref() == Some(b.path.as_str()) {
                    this.map_picked = None;
                } else {
                    this.map_picked = Some(b.path.clone());
                }
                cx.notify();
            }));
        }
        {
            let blocks = blocks.clone();
            let bounds = bounds_cell.clone();
            area = area.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    let hit = match bounds.get() {
                        Some(b) => map_hit(&blocks, ev.position, b, mw, mh),
                        None => None,
                    };
                    this.map_menu = hit.map(|i| blocks[i].path.clone());
                    cx.notify();
                }),
            );
        }

        // Right-click menu
        if let Some(menu_path) = self.map_menu.clone() {
            if let Some(b) = blocks.iter().find(|b| b.path == menu_path) {
                let mx = (b.x / mw).min(0.52) as f32;
                let my = (b.y / mh).min(0.58) as f32;
                let found = self.map_items_at(&menu_path);
                let shown = tree
                    .name(tree.find(&menu_path).unwrap_or(tree.root))
                    .to_string();
                let mut menu = div()
                    .id("map-menu")
                    .absolute()
                    .left(relative(mx))
                    .top(relative(my))
                    .min_w(px(200.))
                    .bg(t.card())
                    .border_1()
                    .border_color(t.border())
                    .rounded(px(9.))
                    .py(px(4.))
                    .flex()
                    .flex_col()
                    .occlude()
                    .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    );
                menu = menu.child(
                    div()
                        .px(px(10.))
                        .py(px(5.))
                        .text_size(px(11.))
                        .text_color(t.tertiary())
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(shown.clone()),
                );
                let reveal_target = menu_path.clone();
                menu = menu.child(
                    menu_item(
                        "map-menu-reveal",
                        &tr("在资源管理器中显示", "Show in Explorer"),
                        t,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.map_menu = None;
                        reveal_path(&reveal_target);
                        cx.notify();
                    })),
                );
                if found.is_empty() {
                    menu = menu.child(
                        div()
                            .px(px(10.))
                            .py(px(5.))
                            .text_size(px(11.))
                            .text_color(t.tertiary())
                            .child(tr(
                                "这一项没有列出，可以在资源管理器中处理",
                                "Not listed here; handle it in Explorer",
                            )),
                    );
                }
                for (id, label, _) in &found {
                    let id = id.clone();
                    let label_text = if label == &shown {
                        tr("加入清理…", "Add to Clean Up…").to_string()
                    } else {
                        format!("{}：{}…", tr("加入清理", "Add to Clean Up"), label)
                    };
                    menu = menu.child(
                        menu_item(&format!("map-menu-{}", id), &label_text, t).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.map_menu = None;
                                this.map_picked = None;
                                this.set_items(vec![id.clone()], true, cx);
                                this.open_sheet(cx);
                            }),
                        ),
                    );
                }
                area = area.child(menu);
            }
        }

        // Preview card for a file (or a folder the map doesn't descend into)
        if let Some(picked_path) = self.map_picked.clone() {
            if let Some(b) = blocks.iter().find(|b| b.path == picked_path) {
                let found = self.map_items_at(&picked_path);
                let mut card = div()
                    .id("map-preview")
                    .absolute()
                    .left(px(10.))
                    .right(px(10.))
                    .bottom(px(10.))
                    .flex()
                    .flex_row()
                    .justify_center()
                    .occlude()
                    .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|_, _, _, cx| cx.stop_propagation()),
                    );
                let mut inner = div()
                    .w(px(520.))
                    .max_w(px(560.))
                    .bg(t.card())
                    .border_1()
                    .border_color(t.border())
                    .rounded(px(14.))
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .size(px(30.))
                                    .rounded(px(7.))
                                    .bg(fade(b.color, 0.22))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(fade(b.color, 1.0))
                                    .text_size(px(14.))
                                    .child(if b.drillable { "▣" } else { "▤" }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(b.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.secondary())
                                    .child(fmt_bytes(b.size)),
                            )
                            .child(
                                div()
                                    .id("map-prev-close")
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .text_color(t.tertiary())
                                    .hover(|s| s.text_color(t.text()))
                                    .child("✕")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.map_picked = None;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(b.path.clone()),
                    );
                if found.is_empty() {
                    inner = inner.child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.tertiary())
                            .child(tr(
                                "这一项没有在这几类里列出，可以在资源管理器中处理",
                                "Not in any list here; you can handle it in Explorer",
                            )),
                    );
                } else if found.len() == 1 {
                    let (id, _, reason) = found[0].clone();
                    inner = inner.child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .child(reason),
                    );
                    inner = inner.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .pt(px(2.))
                            .child(
                                primary_button(
                                    "map-prev-add",
                                    tr("加入清理…", "Add to Clean Up…"),
                                    true,
                                    t,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.map_picked = None;
                                    this.set_items(vec![id.clone()], true, cx);
                                    this.open_sheet(cx);
                                })),
                            )
                            .child({
                                let p = picked_path.clone();
                                ghost_button(
                                    "map-prev-reveal",
                                    tr("在资源管理器中显示", "Show in Explorer"),
                                    t,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this;
                                    reveal_path(&p);
                                    cx.notify();
                                }))
                            }),
                    );
                } else {
                    for (id, label, _) in found.iter().take(3) {
                        let id = id.clone();
                        inner = inner.child(
                            menu_item(&format!("map-prev-{}", id), label, t).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.map_picked = None;
                                    this.set_items(vec![id.clone()], true, cx);
                                    this.open_sheet(cx);
                                }),
                            ),
                        );
                    }
                    let p = picked_path.clone();
                    inner = inner.child(
                        ghost_button(
                            "map-prev-reveal",
                            tr("在资源管理器中显示", "Show in Explorer"),
                            t,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let _ = this;
                            reveal_path(&p);
                            cx.notify();
                        })),
                    );
                }
                card = card.child(inner);
                area = area.child(card);
            }
        }

        // info bar: hovered block details, otherwise the hint
        let info = if let Some(hp) = &self.map_hovered {
            if let Some(b) = blocks.iter().find(|b| &b.path == hp) {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(10.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(b.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(b.path.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .child(fmt_bytes(b.size)),
                    )
            } else {
                map_hint(t)
            }
        } else {
            map_hint(t)
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(crumbs)
            .child(area)
            .child(info)
    }
}

// ---------------------------------------------------------------------------
// Treemap layout: two levels, group frames with their children inside.
// ---------------------------------------------------------------------------

struct MapBlock {
    path: String,
    name: String,
    size: u64,
    depth: usize,
    drillable: bool,
    color: Rgba,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Topmost block under the pointer: children are pushed after their group, so
/// searching the list back to front matches the paint order.
fn map_hit(
    blocks: &[MapBlock],
    pos: Point<Pixels>,
    bounds: Bounds<Pixels>,
    mw: f64,
    mh: f64,
) -> Option<usize> {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let fx = ((f32::from(pos.x - bounds.origin.x) / w) as f64) * mw;
    let fy = ((f32::from(pos.y - bounds.origin.y) / h) as f64) * mh;
    blocks
        .iter()
        .rposition(|b| fx >= b.x && fx <= b.x + b.w && fy >= b.y && fy <= b.y + b.h)
}

fn layout_map(
    tree: &ScanTree,
    node: usize,
    rect: (f64, f64, f64, f64),
    depth: usize,
    kind_at: &HashMap<String, ThingKind>,
    out: &mut Vec<MapBlock>,
) {
    let mut kids: Vec<usize> = tree
        .sorted_children(node)
        .into_iter()
        .filter(|i| tree.size(*i) > 0)
        .collect();
    let total: f64 = kids.iter().map(|k| tree.size(*k) as f64).sum();
    if total <= 0.0 {
        return;
    }
    // Keep what is visible; the long tail is not drawn.
    let mut acc = 0.0;
    let mut keep = 0;
    for k in &kids {
        acc += tree.size(*k) as f64;
        keep += 1;
        if acc / total > 0.995 || keep >= 60 {
            break;
        }
    }
    kids.truncate(keep);
    let values: Vec<f64> = kids.iter().map(|k| tree.size(*k) as f64).collect();
    let frames = squarify_in(&values, rect.0, rect.1, rect.2, rect.3);
    for (k, f) in kids.iter().zip(frames.iter()) {
        if f.w < 2.0 || f.h < 2.0 {
            continue;
        }
        let x = f64::from(f.x) + 1.5;
        let y = f64::from(f.y) + 1.5;
        let w = f64::from(f.w) - 3.0;
        let h = f64::from(f.h) - 3.0;
        if w <= 1.0 || h <= 1.0 {
            continue;
        }
        let drillable = tree.is_dir(*k)
            && !tree.nodes[*k].collapsed
            && !tree.children(*k).is_empty();
        let mut color = rgb(0x8e8e93);
        let mut cur = Some(*k);
        while let Some(c) = cur {
            if let Some(kind) = kind_at.get(&crate::scan::path_key(&tree.path(c))) {
                color = rgb(kind.color());
                break;
            }
            cur = tree.parent(c);
        }
        let mut name = tree.name(*k).to_string();
        if tree.path(tree.root).is_empty() && tree.parent(*k) == Some(tree.root) {
            // Drive roots ("C:\") read better without the trailing separator.
            name = name.trim_end_matches(['\\', '/']).to_string();
        }
        out.push(MapBlock {
            path: tree.path(*k),
            name,
            size: tree.size(*k),
            depth,
            drillable,
            color,
            x,
            y,
            w,
            h,
        });
        // The reference map descends one level into large group frames.
        if depth == 0 && drillable && w > 70.0 && h > 50.0 {
            layout_map(
                tree,
                *k,
                (x + 3.0, y + 20.0, w - 6.0, h - 23.0),
                1,
                kind_at,
                out,
            );
        }
    }
}

fn menu_item(id: &str, label: &str, t: &Theme) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .px(px(10.))
        .py(px(5.))
        .text_size(px(12.))
        .cursor_pointer()
        .hover(|s| s.bg(t.hover()))
        .child(label.to_string())
}

fn map_hint(t: &Theme) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .px(px(20.))
        .py(px(10.))
        .child(
            div()
                .flex_1()
                .text_size(px(11.))
                .text_color(t.secondary())
                .child(tr(
                    "点文件夹钻进去，点文件看预览，右键可以在资源管理器中显示或加入清理；上方路径可以返回，颜色表示它属于哪一类。",
                    "Click a folder to go in and a file to preview it; right-click to show it in Explorer or add it to Clean Up. The path above goes back; colors show what kind of thing it is.",
                )),
        )
}

fn reveal_path(path: &str) {
    #[cfg(windows)]
    {
        let p = std::path::Path::new(path);
        let mut cmd = std::process::Command::new("explorer.exe");
        if p.is_file() {
            cmd.arg(format!("/select,\"{}\"", path));
        } else {
            cmd.arg(path);
        }
        let _ = cmd.spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
}

// ---------------------------------------------------------------------------
// External disk
// ---------------------------------------------------------------------------

impl Root {
    fn render_external_page(&mut self, t: &Theme, i: usize, cx: &mut Context<Self>) -> Div {
        let Some((name, total, free, busy, map, things, has_data, progress)) =
            self.externals.get(i).map(|e| {
                (
                    e.name.clone(),
                    e.total,
                    e.free,
                    e.busy,
                    e.map,
                    e.report
                        .as_ref()
                        .map(|r| r.things.clone())
                        .unwrap_or_default(),
                    e.tree.is_some() && e.report.is_some(),
                    e.scan
                        .as_ref()
                        .map(|s| s.progress.snapshot())
                        .unwrap_or((0, 0)),
                )
            })
        else {
            return div();
        };
        let reclaim: u64 = things.iter().map(|x| x.reclaimable()).sum();
        let project = occupied(&things, ThingKind::Project);
        let media = occupied(&things, ThingKind::Media);
        let used = total.saturating_sub(free);
        let other = used.saturating_sub(project + media);
        let mut segs = vec![];
        if project > 0 {
            segs.push((
                ThingKind::Project.title(),
                project,
                rgb(ThingKind::Project.color()),
            ));
        }
        if media > 0 {
            segs.push((
                ThingKind::Media.title(),
                media,
                rgb(ThingKind::Media.color()),
            ));
        }
        if other > 0 {
            segs.push((tr("其他", "Other"), other, rgb(0x8e8e93)));
        }

        let header = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(26.))
            .pt(px(16.))
            .pb(px(12.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(text(tr("可以腾出", "Reclaimable"), 15., t.secondary()))
                            .child(big_number(reclaim, 42., t)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap(px(2.))
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .child(format!(
                                "{} {}",
                                tr("磁盘", "Disk"),
                                fmt_bytes(total)
                            ))
                            .child(format!(
                                "{} {}",
                                tr("可用", "Free"),
                                fmt_bytes(free)
                            ))
                            .child(if busy {
                                tr("正在扫描…", "scanning…")
                            } else {
                                tr("已扫描", "scanned")
                            }),
                    ),
            )
            .child(capacity_bar(&segs, total, 14., t))
            .child(legend(&segs, free, t))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(3.))
                    .child(
                        ghost_button(
                            "ext-reclaim",
                            tr("可以清理", "Reclaim"),
                            t,
                        )
                        .when(map, |el| el.opacity(0.7))
                        .when(!map, |el| el.bg(t.accent()).text_color(rgb(0xffffff)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(e) = this.externals.get_mut(i) {
                                e.map = false;
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        ghost_button("ext-map", tr("方块图", "Map"), t)
                            .when(!map, |el| el.opacity(0.7))
                            .when(map, |el| el.bg(t.accent()).text_color(rgb(0xffffff)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(e) = this.externals.get_mut(i) {
                                    e.map = true;
                                }
                                cx.notify();
                            })),
                    ),
            );

        if !has_data {
            let inner = if busy {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(spinner(t))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(t.secondary())
                            .child(if progress.0 > 0 {
                                format!(
                                    "{} · {} · {}",
                                    tr("正在扫描", "Scanning"),
                                    fmt_count(progress.0 as i64),
                                    fmt_bytes(progress.1)
                                )
                            } else {
                                tr("正在扫描…", "Scanning…")
                            }),
                    )
            } else {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr("还没扫描这块盘", "This disk hasn't been scanned")),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.secondary())
                            .child(tr(
                                "扫描后可以找出盘上的项目和渲染帧。",
                                "Scan it to find projects and render frames on it.",
                            )),
                    )
                    .child(
                        primary_button("ext-scan", tr("扫描", "Scan"), true, t).on_click(
                            cx.listener(move |this, _, _, cx| this.scan_external(i, cx)),
                        ),
                    )
            };
            return div()
                .flex()
                .flex_col()
                .size_full()
                .child(header)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(inner),
                );
        }

        if map {
            let vol_path = self.externals[i].path.clone();
            let tree = self.externals[i].tree.as_ref().unwrap();
            let map_view = self.render_map(t, tree, &vol_path, &name, MapTarget::External(i), &things, cx);
            return div().flex().flex_col().size_full().child(header).child(map_view);
        }

        let expanded = self.externals[i].expanded.clone();
        let sel = self.externals[i].selection.clone();
        let mut list = div().flex().flex_col();
        for kind in [ThingKind::Project, ThingKind::Media] {
            let group: Vec<Thing> = things.iter().filter(|x| x.kind == kind).cloned().collect();
            if group.is_empty() {
                continue;
            }
            let mut g = div().flex().flex_col().py(px(6.)).child(
                div()
                    .pb(px(4.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(kind.title()),
            );
            for th in &group {
                g = g.child(self.render_thing_row(
                    t,
                    th,
                    &sel,
                    expanded.contains(&th.id),
                    Some(i),
                    cx,
                ));
                g = g.child(div().h(px(1.)).bg(alpha(t.border, 0x88)));
            }
            list = list.child(g);
        }
        if things.is_empty() {
            list = list.child(
                div()
                    .py(px(30.))
                    .text_size(px(12.))
                    .text_color(t.secondary())
                    .child(tr(
                        "这块盘上没有发现可清理的项目或渲染帧。",
                        "No reclaimable projects or render frames on this disk.",
                    )),
            );
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("ext-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .px(px(26.))
                            .pb(px(96.))
                            .max_w(px(1100.))
                            .child(list),
                    ),
            )
    }
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

impl Root {
    fn render_timeline(&mut self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let kinds = [
            ThingKind::Project,
            ThingKind::Media,
            ThingKind::DevCache,
            ThingKind::Model,
            ThingKind::AppData,
        ];
        let mut by_day: BTreeMap<String, HashMap<String, u64>> = BTreeMap::new();
        for p in &self.history {
            if p.kind == "free" || p.kind == "app" || p.kind == "largeFile" || p.kind == "browser" {
                continue;
            }
            by_day
                .entry(p.day.clone())
                .or_default()
                .insert(p.kind.clone(), p.bytes);
        }
        let days: Vec<String> = by_day.keys().cloned().collect();
        let totals: Vec<u64> = days
            .iter()
            .map(|d| by_day.get(d).map(|m| m.values().sum()).unwrap_or(0))
            .collect();
        let max_total = totals.iter().copied().max().unwrap_or(1).max(1);
        let chosen = self
            .timeline_day
            .clone()
            .filter(|d| by_day.contains_key(d))
            .or_else(|| days.last().cloned());
        let n_days = days.len();

        let mut bars = div()
            .flex()
            .flex_row()
            .items_end()
            .gap(px(3.))
            .h(px(230.))
            .w_full();
        for (idx, day) in days.iter().enumerate() {
            let m = by_day.get(day).cloned().unwrap_or_default();
            let total = totals[idx];
            let dim = chosen.as_ref().map(|c| c != day).unwrap_or(false);
            let mut col = div()
                .id(SharedString::from(format!("day-{}", idx)))
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .justify_end()
                .cursor_pointer()
                .when(dim, |el| el.opacity(0.5))
                .on_click(cx.listener({
                    let d = day.clone();
                    move |this, _, _, cx| {
                        this.timeline_day = Some(d.clone());
                        cx.notify();
                    }
                }));
            let mut stack = div()
                .w_full()
                .flex()
                .flex_col()
                .justify_end()
                .h(relative(total as f32 / max_total as f32))
                .overflow_hidden()
                .rounded(px(3.));
            if total == 0 {
                stack = stack.child(div().h(px(2.)).w_full().bg(t.track()));
            }
            for k in kinds {
                let b = m.get(history::kind_key(k)).copied().unwrap_or(0);
                if b == 0 {
                    continue;
                }
                stack = stack.child(
                    div()
                        .w_full()
                        .h(relative(b as f32 / total as f32))
                        .bg(rgb(k.color())),
                );
            }
            col = col.child(stack);
            bars = bars.child(col);
        }

        // day labels
        let mut labels = div()
            .flex()
            .flex_row()
            .gap(px(3.))
            .pt(px(5.))
            .w_full();
        for (idx, day) in days.iter().enumerate() {
            let show = idx == 0 || idx + 1 == n_days || idx % 3 == 0;
            labels = labels.child(
                div()
                    .flex_1()
                    .text_size(px(8.))
                    .text_color(t.tertiary())
                    .text_center()
                    .child(if show {
                        day.chars().skip(5).collect::<String>()
                    } else {
                        String::new()
                    }),
            );
        }

        // free space trend
        let free_points: Vec<(String, u64)> = self
            .history
            .iter()
            .filter(|p| p.kind == "free")
            .map(|p| (p.day.clone(), p.bytes))
            .collect();
        let fmax = free_points.iter().map(|(_, b)| *b).max().unwrap_or(1).max(1);
        let fmin = free_points.iter().map(|(_, b)| *b).min().unwrap_or(0);
        let span = fmax.saturating_sub(fmin).max(1);
        let mut free_chart = div()
            .flex()
            .flex_row()
            .items_end()
            .gap(px(3.))
            .h(px(120.));
        for (_, b) in &free_points {
            let frac = 0.15 + 0.85 * ((b.saturating_sub(fmin)) as f32 / span as f32);
            free_chart = free_chart.child(
                div()
                    .flex_1()
                    .h(relative(frac))
                    .bg(fade(rgb(0x34c759), 0.55))
                    .rounded(px(3.)),
            );
        }

        // detail
        let mut detail_box = div().flex().flex_col().gap(px(8.));
        if let Some(day) = &chosen {
            let detail: DayDetail = history::detail_for(day, &self.thing_history);
            let mut head = div()
                .flex()
                .flex_row()
                .items_end()
                .justify_between()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr(
                            &format!("{} 的明细", detail.day),
                            &format!("{}, thing by thing", detail.day),
                        )),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.secondary())
                        .child(match &detail.previous_day {
                            Some(prev) => tr(
                                &format!("和 {} 相比，变化最大的排在前面", prev),
                                &format!("Compared with {}, biggest changes first", prev),
                            ),
                            None => tr(
                                "这是第一天的记录，还没有可以比较的日子",
                                "The first day on record, so nothing to compare yet",
                            ),
                        }),
                );
            detail_box = detail_box.child(head);
            if detail.things.is_empty() {
                detail_box = detail_box.child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.secondary())
                        .child(tr("这一天没有明细记录。", "No breakdown recorded for this day.")),
                );
            }
            for k in kinds {
                let mut rows: Vec<&history::ThingChange> = detail
                    .things
                    .iter()
                    .filter(|c| c.kind == history::kind_key(k))
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                rows.sort_by_key(|c| std::cmp::Reverse(c.change.abs()));
                let mut g = div().flex().flex_col().gap(px(2.)).py(px(4.)).child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(7.))
                        .child(dot(rgb(k.color()), 8.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(k.title()),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(t.tertiary())
                                .child(rows.len().to_string()),
                        )
                        .child(div().flex_1()),
                );
                for c in rows.iter().take(6) {
                    let change = if c.change > 0 {
                        format!("+{}", fmt_bytes(c.change as u64))
                    } else if c.change < 0 {
                        format!("−{}", fmt_bytes((-c.change) as u64))
                    } else {
                        String::new()
                    };
                    let change_color = if c.change > 0 {
                        rgb(0xff9f0a)
                    } else {
                        rgb(0x34c759)
                    };
                    g = g.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .pl(px(15.))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(11.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(c.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(t.secondary())
                                    .child(match c.bytes {
                                        Some(b) => fmt_bytes(b),
                                        None => tr("已消失", "gone"),
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(change_color)
                                    .child(change),
                            ),
                    );
                }
                if rows.len() > 6 {
                    g = g.child(
                        div()
                            .pl(px(15.))
                            .text_size(px(10.))
                            .text_color(t.tertiary())
                            .child(tr(
                                &format!("还有 {} 项…", rows.len() - 6),
                                &format!("{} more…", rows.len() - 6),
                            )),
                    );
                }
                detail_box = detail_box.child(g);
            }
        }

        let mut legend = div().flex().flex_row().flex_wrap().gap_x(px(14.)).gap_y(px(6.));
        for k in kinds {
            legend = legend.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(5.))
                    .child(dot(rgb(k.color()), 8.))
                    .child(text(k.title(), 11., t.secondary())),
            );
        }

        div()
            .id("timeline-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .p(px(30.))
                    .max_w(px(1100.))
                    .pb(px(90.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(tr("谁在吃磁盘", "What is eating the disk")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(t.secondary())
                                    .child(if n_days < 3 {
                                        tr(
                                            &format!(
                                                "从 {} 开始记录。每次扫描都会存一笔，过几天就能看出是谁在长。",
                                                days.first().cloned().unwrap_or_else(|| "今天".into())
                                            ),
                                            &format!(
                                                "Recording since {}. Every scan adds a point; in a few days you'll see what grows.",
                                                days.first().cloned().unwrap_or_else(|| "today".into())
                                            ),
                                        )
                                    } else {
                                        tr(
                                            "每天各类东西占用的空间。点一根柱子看那一天的明细。",
                                            "Space used by each kind, per day. Click a bar for that day's breakdown.",
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(bars)
                            .child(labels)
                            .child(legend),
                    )
                    .when(!free_points.is_empty(), |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(tr("可用空间", "Free space")),
                                )
                                .child(free_chart)
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .justify_between()
                                        .text_size(px(10.))
                                        .text_color(t.tertiary())
                                        .child(format!(
                                            "{}  {}",
                                            free_points.first().map(|(d, _)| d.clone()).unwrap_or_default(),
                                            fmt_bytes(fmin)
                                        ))
                                        .child(format!(
                                            "{}  {}",
                                            free_points.last().map(|(d, _)| d.clone()).unwrap_or_default(),
                                            fmt_bytes(fmax)
                                        )),
                                ),
                        )
                    })
                    .child(detail_box),
            )
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

impl Root {
    fn render_settings(&mut self, t: &Theme, cx: &mut Context<Self>) -> Stateful<Div> {
        let appearance = self.appearance;
        let language = self.language;
        let mut gen = div().flex().flex_col().gap(px(10.));
        gen = gen.child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(tr("通用", "General")),
        );
        gen = gen.child(setting_row(
            t,
            tr("外观", "Appearance"),
            None,
            div()
                .flex()
                .flex_row()
                .gap(px(4.))
                .children(
                    [Appearance::System, Appearance::Light, Appearance::Dark]
                        .into_iter()
                        .map(|a| {
                            let active = appearance == a;
                            div()
                                .id(SharedString::from(format!("appr-{:?}", a)))
                                .px(px(9.))
                                .py(px(4.))
                                .rounded(px(6.))
                                .cursor_pointer()
                                .bg(if active { t.accent() } else { t.card_alt() })
                                .text_color(if active { rgb(0xffffff) } else { t.text() })
                                .text_size(px(11.))
                                .child(a.title())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.appearance = a;
                                    let mut p = prefs::load();
                                    p.appearance = a;
                                    prefs::save(&p);
                                    cx.notify();
                                }))
                        }),
                ),
        ));
        gen = gen.child(setting_row(
            t,
            tr("语言", "Language"),
            None,
            div()
                .flex()
                .flex_row()
                .gap(px(4.))
                .children(
                    [
                        LanguagePref::System,
                        LanguagePref::Zh,
                        LanguagePref::En,
                    ]
                    .into_iter()
                    .map(|l| {
                        let active = language == l;
                        div()
                            .id(SharedString::from(format!("lang-{:?}", l)))
                            .px(px(9.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .bg(if active { t.accent() } else { t.card_alt() })
                            .text_color(if active { rgb(0xffffff) } else { t.text() })
                            .text_size(px(11.))
                            .child(l.title())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.language = l;
                                let mut p = prefs::load();
                                p.language = l;
                                prefs::save(&p);
                                cx.notify();
                            }))
                    }),
                ),
        ));
        if language != self.language_at_start() {
            gen = gen.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .text_color(t.secondary())
                            .child(tr(
                                "重新打开后换成所选语言。",
                                "Switches language when disk-cleaner reopens.",
                            )),
                    )
                    .child(
                        ghost_button("relaunch", tr("立即重新打开", "Relaunch Now"), t).on_click(
                            |_, _, cx: &mut App| {
                                if let Ok(exe) = std::env::current_exe() {
                                    let _ = std::process::Command::new(exe)
                                        .args(std::env::args().skip(1))
                                        .spawn();
                                }
                                cx.quit();
                            },
                        ),
                    ),
            );
        }

        let mut scan_box = div().flex().flex_col().gap(px(4.0)).child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(tr("扫描", "Scanning")),
        );
        scan_box = scan_box.child(
            div()
                .text_size(px(11.))
                .text_color(t.secondary())
                .line_height(px(16.))
                .child(tr(
                    "扫描用户目录和 Program Files；AppData 和应用数据会被识别，但临时目录不扫描。清理前每一项都会再检查一次，受保护的文件不会被删。",
                    "Scans your home folder and Program Files. AppData is recognized as app data; temp folders are skipped. Every item is checked again before cleaning, and protected files are never deleted.",
                )),
        );
        scan_box = scan_box.child(
            div()
                .text_size(px(11.))
                .text_color(t.tertiary())
                .child(tr(
                    "设置保存在 disk-cleaner 的数据目录里。",
                    "Settings are stored in disk-cleaner's data folder.",
                )),
        );

        let version = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(tr("关于", "About")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .child(text(tr("当前版本", "Version"), 11., t.secondary()))
                    .child(text("0.10.0", 11., t.text())),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .child(text(tr("数据模式", "Data"), 11., t.secondary()))
                    .child(text(
                        if self.demo {
                            tr("演示数据", "Demo data")
                        } else {
                            tr("本机扫描", "Local scan")
                        },
                        11.,
                        t.text(),
                    )),
            );

        let demo_box = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .text_size(px(11.))
                    .text_color(t.secondary())
                    .line_height(px(16.))
                    .child(tr(
                        "用 --demo 启动可以查看完整的演示数据，不会读取或改动你的文件。",
                        "Launch with --demo to explore a complete demo dataset; your files are never read or changed.",
                    )),
            )
            .when(self.demo, |el| {
                el.child(ghost_button("reload-demo", tr("重新载入演示数据", "Reload demo"), t).on_click(
                    cx.listener(|this, _, _, cx| {
                        let section = this.section;
                        let ext = this.active_external;
                        this.load_demo();
                        this.section = section;
                        this.active_external = ext.filter(|i| *i < this.externals.len());
                        this.ensure_expanded();
                        cx.notify();
                    }),
                ))
            });

        div()
            .id("settings-dim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000055))
            .on_click(cx.listener(|this, _, _, cx| {
                this.settings_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .id("settings-card")
                    .w(px(540.))
                    .max_h(px(640.))
                    .overflow_y_scroll()
                    .rounded_2xl()
                    .bg(t.card())
                    .border_1()
                    .border_color(t.border())
                    .shadow_lg()
                    .p(px(22.))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr("设置", "Settings")),
                    )
                    .child(gen)
                    .child(div().h(px(1.)).bg(t.border()))
                    .child(scan_box)
                    .child(div().h(px(1.)).bg(t.border()))
                    .child(version)
                    .child(div().h(px(1.)).bg(t.border()))
                    .child(demo_box)
                    .child(
                        div().flex().flex_row().justify_end().child(
                            primary_button("settings-close", tr("完成", "Done"), true, t).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.settings_open = false;
                                    cx.notify();
                                }),
                            ),
                        ),
                    ),
            )
    }

    fn language_at_start(&self) -> LanguagePref {
        self.language_start
    }
}

fn setting_row(t: &Theme, title: String, help: Option<String>, control: Div) -> Div {
    let mut row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .child(div().flex_1().flex().flex_col().gap(px(1.)).child(
            div().text_size(px(12.)).child(title),
        ))
        .child(control);
    if let Some(h) = help {
        row = row.child(
            div()
                .text_size(px(10.))
                .text_color(t.secondary())
                .child(h),
        );
    }
    row
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme();
        self.viewport = window.viewport_size();
        if self.auto_scan {
            self.auto_scan = false;
        }
        if self.clean.is_some() {
            return self.render_clean(&t, cx).into_any_element();
        }
        self.render_root(&t, window, cx).into_any_element()
    }
}
