use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fmt::*;
use crate::model::*;
use crate::scan::*;
use crate::volume::VolumeInfo;

// ---------------------------------------------------------------------------
// Activity signals
// ---------------------------------------------------------------------------

pub struct ActivityIndex {
    agent_dirs: Vec<(String, i64, String)>,
    path_hits: Vec<(String, i64, String)>,
    home: String,
}

impl ActivityIndex {
    pub fn new() -> Self {
        let home = home_string();
        let mut a = ActivityIndex {
            agent_dirs: vec![],
            path_hits: vec![],
            home: home.clone(),
        };
        a.load_agent_dir(&format!("{}\\.claude\\projects", home), "Claude Code");
        a.load_agent_dir(&format!("{}\\.pi\\agent\\sessions", home), "pi");
        a.load_codex(&format!("{}\\.codex\\sessions", home));
        a.path_hits.sort_by(|x, y| x.0.cmp(&y.0));
        a
    }

    pub fn encode(path: &str) -> String {
        path.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    }

    fn trim_dashes(s: &str) -> &str {
        s.trim_matches('-')
    }

    fn load_agent_dir(&mut self, dir: &str, source: &str) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if let Some(d) = newest_file_date(&p) {
                let name = e.file_name().to_string_lossy().to_string();
                self.agent_dirs.push((Self::trim_dashes(&name).to_lowercase(), d, source.into()));
            }
        }
    }

    fn load_codex(&mut self, dir: &str) {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>, budget: &mut usize) {
            if *budget == 0 {
                return;
            }
            let Ok(rd) = fs::read_dir(dir) else { return };
            for e in rd.filter_map(|e| e.ok()) {
                if *budget == 0 {
                    return;
                }
                let p = e.path();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    walk(&p, out, budget);
                } else if p.extension().map(|x| x == "jsonl" || x == "json").unwrap_or(false) {
                    out.push(p);
                    *budget = budget.saturating_sub(1);
                }
            }
        }
        let mut files = vec![];
        let mut budget = 2000usize;
        walk(Path::new(dir), &mut files, &mut budget);
        let cutoff = now_ts() - 400 * 86400;
        for f in files {
            let Ok(meta) = fs::metadata(&f) else { continue };
            let Some(m) = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) else {
                continue;
            };
            let ts = m.as_secs() as i64;
            if ts < cutoff {
                continue;
            }
            let Ok(bytes) = fs::read(&f) else { continue };
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(16384)]);
            if let Some(pos) = head.find("\"cwd\":\"") {
                let rest = &head[pos + 7..];
                if let Some(end) = rest.find('"') {
                    self.path_hits.push((rest[..end].to_string(), ts, "Codex".into()));
                }
            }
        }
    }

    pub fn last_for(&self, project: &str) -> Option<(i64, String)> {
        let mut best: Option<(i64, String)> = None;
        let enc = Self::trim_dashes(&Self::encode(project)).to_lowercase();
        for (k, d, s) in &self.agent_dirs {
            if *k == enc || k.starts_with(&(enc.clone() + "-")) {
                if *k != enc && !k.starts_with(&enc) {
                    continue;
                }
                if best.as_ref().map(|b| *d > b.0).unwrap_or(true) {
                    best = Some((*d, s.clone()));
                }
            }
        }
        let prefix = format!("{}{}", project.to_lowercase(), std::path::MAIN_SEPARATOR);
        let pl = project.to_lowercase();
        for (p, d, s) in &self.path_hits {
            let lower = p.to_lowercase();
            if lower == pl || lower.starts_with(&prefix) {
                if best.as_ref().map(|b| *d > b.0).unwrap_or(true) {
                    best = Some((*d, s.clone()));
                }
            }
        }
        best
    }
}

fn newest_file_date(dir: &Path) -> Option<i64> {
    let mut newest = None;
    let rd = fs::read_dir(dir).ok()?;
    for e in rd.filter_map(|e| e.ok()) {
        if let Ok(meta) = e.metadata() {
            if let Ok(m) = meta.modified() {
                if let Ok(d) = m.duration_since(std::time::UNIX_EPOCH) {
                    let ts = d.as_secs() as i64;
                    if newest.map(|n| ts > n).unwrap_or(true) {
                        newest = Some(ts);
                    }
                }
            }
        }
    }
    newest
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn which(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        vec!["exe".into(), "cmd".into(), "bat".into()]
    } else {
        vec![String::new()]
    };
    let key = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&key) {
        for ext in &exts {
            let cand = if ext.is_empty() {
                dir.join(name)
            } else {
                dir.join(format!("{}.{}", name, ext))
            };
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn command_ok(argv: &[&str]) -> bool {
    Command::new(argv[0])
        .args(&argv[1..])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_under(path: &str, base: &str) -> bool {
    let p = path_key(path);
    let b = path_key(base);
    p == b || p.starts_with(&(b + &std::path::MAIN_SEPARATOR.to_string()))
}

fn user_area(path: &str, home: &str) -> bool {
    if !is_under(path, home) || path_key(path) == path_key(home) {
        return false;
    }
    let rel = &path[home.len().min(path.len())..];
    let rel = rel.trim_start_matches(['\\', '/']);
    if rel.is_empty() {
        return false;
    }
    let first = rel.split(['\\', '/']).next().unwrap_or("");
    if first.starts_with('.') {
        return false;
    }
    if first.eq_ignore_ascii_case("AppData") {
        return false;
    }
    true
}

fn user_area_flat(path: &str, home: &str) -> bool {
    if !is_under(path, home) || path_key(path) == path_key(home) {
        return false;
    }
    let rel = &path[home.len().min(path.len())..];
    let rel = rel.trim_start_matches(['\\', '/']);
    let first = rel.split(['\\', '/']).next().unwrap_or("");
    !first.starts_with('.') && !first.eq_ignore_ascii_case("AppData")
}

fn days_between(ts: i64, now: i64) -> i64 {
    (now - ts) / 86400
}

// ---------------------------------------------------------------------------
// Recognizers
// ---------------------------------------------------------------------------

pub struct Ctx<'a> {
    pub tree: &'a ScanTree,
    pub home: String,
    pub now: i64,
    pub sleep_after_days: i64,
    pub allow_any_path: bool,
    pub off_limits: Vec<String>,
    pub activity: ActivityIndex,
    pub volume: VolumeInfo,
    /// Restricts recognition to these mount points; empty means every scanned
    /// volume is fair game. Used to run a per-volume pass over a tree that
    /// contains all the drives without cross-contaminating the results.
    pub scope_roots: Vec<String>,
}

impl<'a> Ctx<'a> {
    pub fn size(&self, path: &str) -> u64 {
        self.tree.find(path).map(|i| self.tree.size(i)).unwrap_or(0)
    }

    pub fn off_limits(&self, path: &str) -> bool {
        self.off_limits.iter().any(|p| is_under(path, p))
    }

    fn in_scope(&self, path: &str) -> bool {
        self.scope_roots.is_empty() || self.scope_roots.iter().any(|r| is_under(path, r))
    }

    fn usable(&self, path: &str) -> bool {
        if !self.in_scope(path) {
            return false;
        }
        if self.off_limits(path) {
            return false;
        }
        if self.allow_any_path {
            true
        } else {
            user_area(path, &self.home)
        }
    }
}

pub fn recognize_all(tree: &ScanTree, volume: &VolumeInfo, log: impl Fn(String)) -> Report {
    let home = home_string();
    // The scan covers the whole volume now; never offer anything inside these.
    let mut off_limits = vec![];
    for name in [
        "Windows",
        "ProgramData",
        "$Recycle.Bin",
        "System Volume Information",
        "Recovery",
    ] {
        let p = std::path::Path::new(&volume.path).join(name);
        off_limits.push(p.to_string_lossy().to_string());
    }
    let mut ctx = Ctx {
        tree,
        home: home.clone(),
        now: now_ts(),
        sleep_after_days: 30,
        allow_any_path: false,
        off_limits: vec![],
        activity: ActivityIndex::new(),
        volume: volume.clone(),
        scope_roots: vec![],
    };
    ctx.off_limits = off_limits;
    log(t("识别项目…", "Finding projects…"));
    let (projects, venvs) = recognize_projects(&ctx);
    let preselected_venvs: HashSet<String> = projects
        .iter()
        .flat_map(|t| t.items.iter())
        .filter(|i| i.preselected && i.label == "venv")
        .map(|i| i.id.clone())
        .collect();
    log(t("识别渲染帧…", "Finding render frames…"));
    let mut media = recognize_media(&ctx);
    log(t("识别开发缓存…", "Finding dev caches…"));
    let caches = recognize_caches(&ctx, &venvs, &preselected_venvs, &log);
    let models = recognize_models(&ctx);
    let app_data = recognize_app_data(&ctx);
    let system = recognize_system(&ctx);
    log(t("识别已安装的应用…", "Finding installed apps…"));
    let installed = crate::apps::recognize_apps(&ctx);
    let mut all = projects;
    all.extend(media.drain(..));
    all.extend(caches);
    all.extend(models);
    all.extend(app_data);
    all.extend(system);
    all.extend(installed);
    log(t("查找其他大文件…", "Finding other large files…"));
    let covered: Vec<String> = all
        .iter()
        .flat_map(|t| t.items.iter())
        .flat_map(|i| {
            let mut v = vec![];
            if let Some(p) = &i.path {
                v.push(p.clone());
            }
            v.extend(i.action.paths());
            v
        })
        .collect();
    let mut large = recognize_large_files(&ctx, &covered);
    all.append(&mut large);
    log(t("查看浏览器…", "Looking at browsers…"));
    let mut browsers = crate::apps::recognize_browsers(&ctx);
    all.append(&mut browsers);
    Report {
        created_at: now_ts(),
        volume_total: volume.total,
        volume_free: volume.free,
        scanned: tree.unique_total,
        things: all,
        language: Some(if is_zh() { "zh".into() } else { "en".into() }),
    }
}

pub fn recognize_external(tree: &ScanTree, volume: &VolumeInfo, root: &str, log: impl Fn(String)) -> Report {
    let mut off = vec![];
    if let Some(n) = tree.find(root) {
        for c in tree.children(n) {
            let name = tree.name(*c);
            if name.starts_with('.') {
                off.push(tree.path(*c));
            }
        }
    }
    for name in [
        "Windows",
        "ProgramData",
        "Program Files",
        "Program Files (x86)",
        "$Recycle.Bin",
        "System Volume Information",
        "Recovery",
    ] {
        let p = std::path::Path::new(root).join(name);
        off.push(p.to_string_lossy().to_string());
    }
    let ctx = Ctx {
        tree,
        home: home_string(),
        now: now_ts(),
        sleep_after_days: 30,
        allow_any_path: true,
        off_limits: off,
        activity: ActivityIndex::new(),
        volume: volume.clone(),
        scope_roots: vec![root.to_string()],
    };
    log(t("识别项目…", "Finding projects…"));
    let (projects, _) = recognize_projects(&ctx);
    log(t("识别渲染帧…", "Finding render frames…"));
    let media = recognize_media(&ctx);
    let mut all = projects;
    all.extend(media);
    Report {
        created_at: now_ts(),
        volume_total: volume.total,
        volume_free: volume.free,
        scanned: tree.unique_total,
        things: all,
        language: Some(if is_zh() { "zh".into() } else { "en".into() }),
    }
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct VenvRef {
    pub path: String,
    pub project: String,
    pub sleeping: bool,
    pub item_id: String,
}

struct Artifact {
    kind: String,
    restore: Option<String>,
    strong: bool,
    warnings: Vec<String>,
    needs_git_check: bool,
}

const SMALL_CACHES: [&str; 4] = ["__pycache__", ".mypy_cache", ".pytest_cache", ".ruff_cache"];
const JS_CACHES: [&str; 6] = [".next", ".nuxt", ".turbo", ".svelte-kit", ".parcel-cache", ".angular"];

fn locks(tree: &ScanTree, from: usize, root: usize) -> u32 {
    let mut m = 0;
    let mut cur = Some(from);
    while let Some(c) = cur {
        m |= tree.markers(c);
        if c == root {
            break;
        }
        cur = tree.parent(c);
    }
    m
}

fn artifact(tree: &ScanTree, i: usize, name: &str, owner: usize) -> Option<Artifact> {
    let Some(parent) = tree.parent(i) else { return None };
    let pm = tree.markers(parent);
    if small_cache(name) {
        return Some(Artifact {
            kind: name.into(),
            restore: None,
            strong: true,
            warnings: vec![],
            needs_git_check: false,
        });
    }
    if name == "node_modules" && pm & F_PACKAGE_JSON != 0 {
        let m = locks(tree, parent, owner);
        let restore = if m & F_LOCK_PNPM != 0 {
            "pnpm install"
        } else if m & F_LOCK_BUN != 0 {
            "bun install"
        } else if m & F_LOCK_YARN != 0 {
            "yarn install"
        } else if m & F_LOCK_NPM != 0 {
            "npm ci"
        } else {
            "npm install"
        };
        let locked = m & (F_LOCK_PNPM | F_LOCK_BUN | F_LOCK_YARN | F_LOCK_NPM) != 0;
        let mut a = Artifact {
            kind: "node_modules".into(),
            restore: Some(restore.into()),
            strong: locked,
            warnings: vec![],
            needs_git_check: false,
        };
        if !locked {
            a.warnings.push(t(
                "没有锁文件，重装后的版本可能不同",
                "no lockfile: reinstall may resolve different versions",
            ));
        }
        return Some(a);
    }
    if tree.markers(i) & F_PYVENV_CFG != 0 || ((name == ".venv" || name == "venv") && Path::new(&tree.path(i)).join("pyvenv.cfg").exists()) {
        let m = locks(tree, parent, owner);
        let restore = if m & F_LOCK_UV != 0 {
            "uv sync".to_string()
        } else if m & F_LOCK_POETRY != 0 {
            "poetry install".to_string()
        } else if m & F_PYPROJECT != 0 {
            "uv sync".to_string()
        } else if m & F_REQUIREMENTS != 0 {
            format!("python -m venv {} && {}/Scripts/pip install -r requirements.txt", name, name)
        } else {
            format!("python -m venv {}", name)
        };
        let locked = m & (F_LOCK_UV | F_LOCK_POETRY) != 0;
        let mut a = Artifact {
            kind: "venv".into(),
            restore: Some(restore),
            strong: locked,
            warnings: vec![],
            needs_git_check: false,
        };
        if !locked {
            a.warnings.push(t(
                "没有 uv.lock / poetry.lock，重建的依赖版本可能不同",
                "no uv.lock / poetry.lock: rebuilt versions may differ",
            ));
        }
        return Some(a);
    }
    if name == "target" && (pm & (F_CARGO_TOML | F_POM_XML) != 0) {
        return Some(Artifact {
            kind: "target".into(),
            restore: Some(if pm & F_POM_XML != 0 { "mvn package".into() } else { "cargo build".into() }),
            strong: pm & F_CARGO_TOML != 0 || pm & F_POM_XML != 0,
            warnings: vec![],
            needs_git_check: false,
        });
    }
    if JS_CACHES.contains(&name) && pm & F_PACKAGE_JSON != 0 {
        return Some(Artifact {
            kind: name.into(),
            restore: Some("npm run build".into()),
            strong: true,
            warnings: vec![],
            needs_git_check: false,
        });
    }
    if name == ".build" && pm & F_PACKAGE_SWIFT != 0 {
        return Some(Artifact {
            kind: ".build".into(),
            restore: Some("swift build".into()),
            strong: true,
            warnings: vec![],
            needs_git_check: false,
        });
    }
    if name == ".gradle" && pm & F_GRADLE != 0 {
        return Some(Artifact {
            kind: ".gradle".into(),
            restore: Some("./gradlew build".into()),
            strong: true,
            warnings: vec![],
            needs_git_check: false,
        });
    }
    if name == ".dart_tool" && pm & F_PUBSPEC != 0 {
        return Some(Artifact {
            kind: ".dart_tool".into(),
            restore: Some("flutter pub get".into()),
            strong: true,
            warnings: vec![],
            needs_git_check: false,
        });
    }
    if ["build", "dist", "out"].contains(&name)
        && pm & (F_PACKAGE_JSON | F_CMAKE | F_SETUP_PY | F_PYPROJECT | F_GRADLE) != 0
        && tree.markers(owner) & F_GIT != 0
    {
        if subtree_has_media(tree, i) {
            return None;
        }
        return Some(Artifact {
            kind: name.into(),
            restore: if pm & F_PACKAGE_JSON != 0 { Some("npm run build".into()) } else { None },
            strong: false,
            warnings: vec![],
            needs_git_check: true,
        });
    }
    None
}

fn small_cache(name: &str) -> bool {
    SMALL_CACHES.contains(&name)
}

fn subtree_has_media(tree: &ScanTree, i: usize) -> bool {
    let mut found = false;
    tree.walk(i, &mut |j| {
        let name = tree.name(j);
        let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        if ["mp4", "mov", "mkv", "avi", "wav", "mp3", "m4a", "flac"].contains(&ext.as_str())
            && tree.size(j) > 10_000_000
        {
            found = true;
        }
        !found
    });
    found
}

fn git_status(project: &str, path: &str) -> Option<(bool, bool)> {
    if which("git").is_none() {
        return None;
    }
    let ignored = Command::new("git")
        .args(["-C", project, "check-ignore", "-q", path])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let tracked = Command::new("git")
        .args(["-C", project, "ls-files", "--", path])
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    Some((ignored, tracked))
}

fn reason_for(kind: &str) -> String {
    match kind {
        "node_modules" => t("npm 依赖，可以按锁文件重新安装", "npm dependencies, reinstallable from the lockfile"),
        "venv" => t("Python 虚拟环境，可以重建", "Python virtualenv, can be recreated"),
        "target" => t("Rust/Java 编译产物，重新编译即可", "Rust/Java build output, rebuilt by compiling"),
        ".build" => t("SwiftPM 编译产物", "SwiftPM build output"),
        ".gradle" => t("Gradle 缓存", "Gradle cache"),
        ".dart_tool" => t("Dart/Flutter 工具缓存", "Dart/Flutter tool cache"),
        _ if JS_CACHES.contains(&kind) => t("前端构建缓存", "front-end build cache"),
        _ => t("构建输出，被 git 忽略", "build output ignored by git"),
    }
}

fn recognize_projects(ctx: &Ctx) -> (Vec<Thing>, Vec<VenvRef>) {
    let tree = ctx.tree;
    let home = ctx.home.clone();
    let mut project_roots: Vec<usize> = vec![];
    for i in 0..tree.nodes.len() {
        let n = &tree.nodes[i];
        if !n.dir || i == tree.root {
            continue;
        }
        let path = &n.path;
        if path_key(path) == path_key(&home) {
            continue;
        }
        if tree.parent(i) == Some(tree.root) {
            continue;
        }
        if !ctx.usable(path) {
            continue;
        }
        let m = n.markers;
        if m & F_GIT != 0 || m & MANIFEST_MASK != 0 {
            // Manifest folders that are only a subfolder of a project still count as nested projects.
            project_roots.push(i);
        }
    }
    let root_set: HashSet<usize> = project_roots.iter().copied().collect();

    let mut things = vec![];
    let mut venv_refs = vec![];
    for root in &project_roots {
        let root_path = tree.path(*root);
        let mut artifacts: Vec<(usize, Artifact)> = vec![];
        let mut small: Vec<usize> = vec![];
        let mut stack = tree.children(*root).to_vec();
        while let Some(i) = stack.pop() {
            if root_set.contains(&i) {
                continue;
            }
            let n = &tree.nodes[i];
            let name = n.name.as_str();
            if !n.dir {
                continue;
            }
            if let Some(a) = artifact(tree, i, name, *root) {
                if small_cache(name) {
                    small.push(i);
                } else {
                    artifacts.push((i, a));
                }
                continue;
            }
            for c in tree.children(i) {
                stack.push(*c);
            }
        }
        if artifacts.is_empty() && small.is_empty() {
            continue;
        }
        let mut activity = ctx.activity.last_for(&root_path);
        let source_newest = tree.source_newest(*root);
        if source_newest > 0 {
            if activity.as_ref().map(|a| source_newest > a.0).unwrap_or(true) {
                activity = Some((source_newest, t("文件改动", "file changes")));
            }
        }
        let days = activity
            .as_ref()
            .map(|a| days_between(a.0, ctx.now))
            .unwrap_or(i64::MAX);
        let sleeping = days >= ctx.sleep_after_days;
        let mut items: Vec<ReclaimItem> = vec![];
        for (i, mut a) in artifacts {
            let size = tree.size(i);
            if size < 1_000_000 {
                continue;
            }
            let path = tree.path(i);
            let mtime = tree.nodes[i].mtime;
            let recent = mtime > 0 && ctx.now - mtime < 7 * 86400;
            if a.needs_git_check {
                match git_status(&root_path, &path) {
                    Some((ignored, tracked)) => {
                        if ignored && !tracked {
                            a.strong = true;
                        } else {
                            a.strong = false;
                            a.warnings.push(if tracked {
                                t("里面有 git 跟踪的文件，不会删", "contains git-tracked files")
                            } else {
                                t("没有被 git 忽略，可能不是构建产物", "not ignored by git: may not be a build output")
                            });
                        }
                    }
                    None => {
                        a.strong = false;
                        a.warnings.push(t(
                            "没有找到 git，无法确认这是构建产物",
                            "git isn't available, so this can't be confirmed as build output",
                        ));
                    }
                }
            }
            let mut item = ReclaimItem::new(
                format!("art:{}", path),
                a.kind.clone(),
                Some(path.clone()),
                size,
                Tier::Regenerable,
                ReclaimAction::Delete { path: path.clone() },
                reason_for(&a.kind),
            )
            .preselected(sleeping && a.strong && !recent);
            item.restore = a.restore.clone();
            item.warnings.append(&mut a.warnings);
            if recent {
                item.warnings.push(t(
                    &format!("{} 还有改动，所以没预选", fmt_stamp(mtime)),
                    &format!("changed {}, so not preselected", fmt_stamp(mtime)),
                ));
            }
            if a.kind == "venv" {
                venv_refs.push(VenvRef {
                    path: path.clone(),
                    project: root_path.clone(),
                    sleeping,
                    item_id: item.id.clone(),
                });
            }
            items.push(item);
        }
        let small_bytes: u64 = small.iter().map(|i| tree.size(*i)).sum();
        if small_bytes >= 1 << 20 {
            let paths: Vec<String> = small.iter().map(|i| tree.path(*i)).collect();
            items.push(
                ReclaimItem::new(
                    format!("pycache:{}", root_path),
                    t(&format!("Python 缓存 ×{}", paths.len()), &format!("Python caches ×{}", paths.len())),
                    None,
                    small_bytes,
                    Tier::Regenerable,
                    ReclaimAction::DeleteAll { paths },
                    t("运行时自动重建", "rebuilt automatically when code runs"),
                )
                .preselected(sleeping),
            );
        }
        let reclaim: u64 = items.iter().map(|i| i.bytes).sum();
        if reclaim < 20_000_000 {
            continue;
        }
        items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
        let mut th = Thing::new(
            format!("project:{}", root_path),
            ThingKind::Project,
            tree.name(*root).to_string(),
            Some(root_path.clone()),
            tree.size(*root),
        );
        if let Some((ts, src)) = activity {
            th.last_active = Some(ts);
            th.activity_source = Some(src);
            if sleeping {
                th.detail = Some(if days == i64::MAX {
                    t("沉睡 很久", "asleep for a long time")
                } else {
                    t(&format!("沉睡 {} 天", days), &format!("asleep for {} days", days))
                });
            }
        } else if sleeping {
            th.detail = Some(t("沉睡 很久", "asleep for a long time"));
        }
        th.items = items;
        things.push(th);
    }
    things.sort_by_key(|t| std::cmp::Reverse(t.reclaimable()));
    (things, venv_refs)
}

// ---------------------------------------------------------------------------
// Render frames
// ---------------------------------------------------------------------------

struct Sequence {
    count: i64,
    newest: i64,
}

fn inspect_sequence(dir: &str) -> Option<Sequence> {
    let rd = fs::read_dir(dir).ok()?;
    let images: HashSet<&str> = ["jpg", "jpeg", "png", "webp", "tif", "tiff", "exr", "dpx", "bmp", "tga"].into_iter().collect();
    let mut groups: HashMap<(String, usize, String), Vec<i64>> = HashMap::new();
    let mut images_total = 0i64;
    for e in rd.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(ext) = name.rsplit('.').next() else { continue };
        let ext = ext.to_lowercase();
        if !images.contains(ext.as_str()) {
            continue;
        }
        images_total += 1;
        let stem = name[..name.len().saturating_sub(ext.len() + 1)].to_string();
        let digits: String = stem
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        if digits.len() < 3 {
            continue;
        }
        let Ok(v) = digits.parse::<i64>() else { continue };
        let prefix = stem[..stem.len() - digits.len()].to_string();
        groups.entry((prefix, digits.len(), ext)).or_default().push(v);
    }
    let (_, nums) = groups.into_iter().max_by_key(|(_, v)| v.len())?;
    if nums.len() < 300 || (nums.len() as f64) < 0.9 * images_total as f64 {
        return None;
    }
    let lo = *nums.iter().min()?;
    let hi = *nums.iter().max()?;
    let gaps = (hi - lo + 1) - nums.len() as i64;
    if gaps as f64 > 0.05 * nums.len() as f64 {
        return None;
    }
    let newest = fs::metadata(dir)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Some(Sequence {
        count: nums.len() as i64,
        newest,
    })
}

fn ffprobe_frames(path: &str) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=nb_frames,r_frame_rate,duration",
            "-of",
            "json",
            path,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let st = v.get("streams")?.get(0)?;
    if let Some(n) = st.get("nb_frames").and_then(|x| x.as_str()).and_then(|s| s.parse::<f64>().ok()) {
        if n > 0.0 {
            return Some(n);
        }
    }
    let duration = st.get("duration").and_then(|x| x.as_str()).and_then(|s| s.parse::<f64>().ok())?;
    let rate = st.get("r_frame_rate").and_then(|x| x.as_str())?;
    let fps = if let Some((a, b)) = rate.split_once('/') {
        let a: f64 = a.parse().ok()?;
        let b: f64 = b.parse().ok()?;
        if b == 0.0 {
            return None;
        }
        a / b
    } else {
        rate.parse().ok()?
    };
    if duration > 0.0 && fps > 0.0 {
        Some(duration * fps)
    } else {
        None
    }
}

fn videos_in(dir: &str) -> Vec<String> {
    let exts: HashSet<&str> = ["mp4", "mov", "m4v", "mkv", "webm"].into_iter().collect();
    let Ok(rd) = fs::read_dir(dir) else { return vec![] };
    rd.filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let ext = name.rsplit('.').next()?.to_lowercase();
            if exts.contains(ext.as_str()) {
                Some(e.path().to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect()
}

fn recognize_media(ctx: &Ctx) -> Vec<Thing> {
    let tree = ctx.tree;
    let mut candidates = vec![];
    for i in 0..tree.nodes.len() {
        let n = &tree.nodes[i];
        if !n.dir || n.collapsed || n.img_count < 300 || n.size < 50_000_000 {
            continue;
        }
        if !ctx.usable(&n.path) {
            continue;
        }
        candidates.push(i);
    }
    let mut groups: HashMap<String, Vec<ReclaimItem>> = HashMap::new();
    let mut group_bytes: HashMap<String, u64> = HashMap::new();
    let has_ffprobe = which("ffprobe").is_some();
    for i in candidates {
        let path = tree.path(i);
        let Some(seq) = inspect_sequence(&path) else { continue };
        // Scope: nearest ancestor with a manifest, else two levels up.
        let mut scope = i;
        let mut hops = 0;
        while let Some(p) = tree.parent(scope) {
            if p == tree.root {
                break;
            }
            scope = p;
            hops += 1;
            if tree.markers(scope) & MANIFEST_MASK != 0 {
                break;
            }
            if hops >= 6 {
                break;
            }
        }
        // Find a final video among up to 6 ancestors.
        let mut matched: Option<(String, bool)> = None;
        let mut cur = i;
        for _ in 0..6 {
            let Some(p) = tree.parent(cur) else { break };
            if p == tree.root {
                break;
            }
            cur = p;
            for v in videos_in(&tree.path(p)) {
                let vm = fs::metadata(&v)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                if seq.newest > 0 && vm < seq.newest - 300 {
                    continue;
                }
                if has_ffprobe {
                    if let Some(expected) = ffprobe_frames(&v) {
                        let err = (seq.count as f64 - expected.round()).abs();
                        if err <= (3.0f64).max(0.005 * expected) {
                            matched = Some((v, true));
                            break;
                        }
                    }
                } else {
                    matched = Some((v, false));
                    break;
                }
            }
            if matched.is_some() {
                break;
            }
        }
        let size = tree.size(i);
        let recent = seq.newest > 0 && ctx.now - seq.newest < 7 * 86400;
        let item = if let Some((video, verified)) = matched {
            let mut it = ReclaimItem::new(
                format!("frames:{}", path),
                t(&format!("渲染帧 {} 张", seq.count), &format!("{} rendered frames", seq.count)),
                Some(path.clone()),
                size,
                Tier::Regenerable,
                ReclaimAction::Delete { path: path.clone() },
                t(
                    &format!("成片已存在：{}（帧数和成片一致）", fmt_home(&video)),
                    &format!("final video exists: {} (same number of frames)", fmt_home(&video)),
                ),
            )
            .with_restore(t("用原工程重新渲染", "re-render from the source project"))
            .preselected(!recent);
            if !verified {
                it.warnings.push(t(
                    "没有 ffprobe，只确认了成片存在，未核对帧数",
                    "ffprobe isn't available, so only the video's presence was checked, not its frame count",
                ));
            }
            if recent {
                it.warnings.push(t(
                    &format!("{} 刚渲染过，所以没预选", fmt_stamp(seq.newest)),
                    &format!("rendered {}, so not preselected", fmt_stamp(seq.newest)),
                ));
            }
            it
        } else {
            let mut it = ReclaimItem::new(
                format!("frames:{}", path),
                t(&format!("图片序列 {} 张", seq.count), &format!("{}-image sequence", seq.count)),
                Some(path.clone()),
                size,
                Tier::Yours,
                ReclaimAction::Trash { path: path.clone() },
                t(
                    "像渲染帧，但没找到对应的成片，按你的文件处理",
                    "looks like frames, but no matching final video was found",
                ),
            )
            .preselected(false);
            if recent {
                it.warnings.push(t(
                    &format!("{} 还有改动，所以没预选", fmt_stamp(seq.newest)),
                    &format!("changed {}, so not preselected", fmt_stamp(seq.newest)),
                ));
            }
            it
        };
        let scope_path = tree.path(scope);
        groups.entry(scope_path.clone()).or_default().push(item);
        *group_bytes.entry(scope_path).or_insert(0) += size;
    }
    let mut things: Vec<Thing> = groups
        .into_iter()
        .map(|(scope, mut items)| {
            let name = scope.rsplit(['\\', '/']).next().unwrap_or(&scope).to_string();
            let mut th = Thing::new(
                format!("media:{}", scope),
                ThingKind::Media,
                t(&format!("渲染中间帧 · {}", name), &format!("Render frames · {}", name)),
                Some(scope.clone()),
                *group_bytes.get(&scope).unwrap_or(&0),
            );
            th.detail = Some(t(
                &format!("{} 组帧序列", items.len()),
                &format!("{} frame folders", items.len()),
            ));
            items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
            th.items = items;
            th
        })
        .collect();
    things.sort_by_key(|t| std::cmp::Reverse(t.reclaimable()));
    things
}

// ---------------------------------------------------------------------------
// Dev caches
// ---------------------------------------------------------------------------

fn recognize_caches(ctx: &Ctx, venvs: &[VenvRef], preselected_venvs: &HashSet<String>, log: &impl Fn(String)) -> Vec<Thing> {
    let h = ctx.home.clone();
    let mut things = vec![];
    let mut uv_dir = format!("{}\\.cache\\uv", h);
    if which("uv").is_some() {
        if let Ok(o) = Command::new("uv").args(["cache", "dir"]).output() {
            let d = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if o.status.success() && !d.is_empty() {
                uv_dir = d;
            }
        }
    }
    if let Some(node) = ctx.tree.find(&uv_dir) {
        let bytes = ctx.tree.size(node);
        if bytes >= 200_000_000 {
            let mut buckets: HashMap<String, Vec<(i64, usize)>> = HashMap::new();
            for c in ctx.tree.children(node) {
                let name = ctx.tree.name(*c);
                if let Some(pos) = name.rfind("-v") {
                    if let Ok(v) = name[pos + 2..].parse::<i64>() {
                        buckets.entry(name[..pos].to_string()).or_default().push((v, *c));
                    }
                }
            }
            let mut stale = 0u64;
            for (_, vs) in &buckets {
                if vs.len() > 1 {
                    let maxv = vs.iter().map(|(v, _)| *v).max().unwrap_or(0);
                    for (v, c) in vs {
                        if *v < maxv {
                            stale += ctx.tree.size(*c);
                        }
                    }
                }
            }
            let mut prune = ReclaimItem::new(
                "uv:prune",
                "uv cache prune",
                Some(uv_dir.clone()),
                bytes,
                Tier::Regenerable,
                ReclaimAction::Command { argv: vec!["uv".into(), "cache".into(), "prune".into()], cwd: None },
                t("删除旧版本格式的缓存和没人引用的条目", "drops old-format buckets and unreferenced entries"),
            )
            .with_estimate(stale)
            .preselected(true);
            prune.estimate_is_lower_bound = true;
            log(t(
                &format!("正在计算 uv 缓存和 {} 个 .venv 的共享数据…", venvs.len()),
                &format!("Measuring data shared between the uv cache and {} venvs…", venvs.len()),
            ));
            let mut clean = ReclaimItem::new(
                "uv:clean",
                "uv cache clean",
                Some(uv_dir.clone()),
                bytes,
                Tier::Regenerable,
                ReclaimAction::Command { argv: vec!["uv".into(), "cache".into(), "clean".into()], cwd: None },
                t("清空整个 uv 缓存，以后装包会重新下载", "empties the whole uv cache; packages are re-downloaded later"),
            )
            .preselected(false)
            .with_covers(vec!["uv:prune".into()]);
            clean.estimate = bytes;
            let mut items = vec![prune, clean];
            let sleeping: Vec<&VenvRef> = venvs.iter().filter(|v| preselected_venvs.contains(&v.item_id)).collect();
            if !sleeping.is_empty() {
                let mut actions = vec![ReclaimAction::Command {
                    argv: vec!["uv".into(), "cache".into(), "clean".into()],
                    cwd: None,
                }];
                let mut combo_bytes = bytes;
                for v in &sleeping {
                    actions.push(ReclaimAction::Delete { path: v.path.clone() });
                    combo_bytes += ctx.size(&v.path);
                }
                let mut covers = vec!["uv:prune".to_string(), "uv:clean".to_string()];
                covers.extend(sleeping.iter().map(|v| v.item_id.clone()));
                let combo = ReclaimItem::new(
                    "uv:combo",
                    t(
                        &format!("uv cache clean + {} 个沉睡项目的 .venv", sleeping.len()),
                        &format!("uv cache clean + {} sleeping-project venvs", sleeping.len()),
                    ),
                    Some(uv_dir.clone()),
                    combo_bytes,
                    Tier::Regenerable,
                    ReclaimAction::Compound { actions },
                    t(
                        "缓存和沉睡项目的环境一起清，克隆共享的数据才会真正释放",
                        "clearing the cache together with sleeping venvs frees the cloned data too",
                    ),
                )
                .with_restore(t("在各项目里运行 uv sync", "run uv sync in each project"))
                .preselected(false)
                .with_covers(covers);
                items[0].preselected = false;
                items.push(combo);
            }
            let mut th = Thing::new("cache:uv", ThingKind::DevCache, "uv cache", Some(uv_dir.clone()), bytes);
            th.items = items;
            things.push(th);
        }
    }

    let mut simple = |id: &str,
                      title: &str,
                      path: String,
                      tool: Option<&str>,
                      argv: Vec<&str>,
                      reason: String,
                      preselect: bool,
                      restore: Option<String>| {
        let Some(node) = ctx.tree.find(&path) else { return };
        let bytes = ctx.tree.size(node);
        if bytes < 50_000_000 {
            return;
        }
        let action = match (tool, which(tool.unwrap_or(""))) {
            (Some(_), Some(_)) => ReclaimAction::Command {
                argv: argv.iter().map(|s| s.to_string()).collect(),
                cwd: None,
            },
            _ => ReclaimAction::Delete { path: path.clone() },
        };
        let mut item = ReclaimItem::new(format!("cache:{}", id), title, Some(path.clone()), bytes, Tier::Regenerable, action, reason)
            .preselected(preselect);
        item.restore = restore;
        let mut th = Thing::new(format!("cache:{}", id), ThingKind::DevCache, title, Some(path), bytes);
        th.items = vec![item];
        things.push(th);
    };

    simple(
        "npm",
        "npm cache",
        format!("{}\\AppData\\Local\\npm-cache\\_cacache", h),
        Some("npm"),
        vec!["npm", "cache", "clean", "--force"],
        t("npm 下载缓存，需要时会重新下载", "npm download cache, re-downloaded on demand"),
        true,
        None,
    );
    simple(
        "bun",
        "bun cache",
        format!("{}\\.bun\\install\\cache", h),
        Some("bun"),
        vec!["bun", "pm", "cache", "rm"],
        t("bun 下载缓存", "bun download cache"),
        true,
        None,
    );
    simple(
        "yarn",
        "Yarn cache",
        format!("{}\\AppData\\Local\\Yarn\\Cache", h),
        Some("yarn"),
        vec!["yarn", "cache", "clean"],
        t("Yarn 下载缓存", "Yarn download cache"),
        true,
        None,
    );
    simple(
        "pip",
        "pip cache",
        format!("{}\\AppData\\Local\\pip\\Cache", h),
        None,
        vec![],
        t("pip 下载缓存", "pip download cache"),
        true,
        None,
    );
    simple(
        "cargo-registry",
        "Cargo registry",
        format!("{}\\.cargo\\registry", h),
        None,
        vec![],
        t("crate 源码和压缩包，编译时会重新下载", "crate sources, re-downloaded when building"),
        false,
        None,
    );
    simple(
        "puppeteer",
        "Puppeteer browsers",
        format!("{}\\.cache\\puppeteer", h),
        None,
        vec![],
        t("Puppeteer 下载的浏览器，需要时会重新下载", "browsers downloaded by Puppeteer"),
        false,
        None,
    );

    // pnpm store
    if which("pnpm").is_some() {
        if let Ok(o) = Command::new("pnpm").args(["store", "path"]).output() {
            let store = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !store.is_empty() {
                if let Some(node) = ctx.tree.find(&store) {
                    let bytes = ctx.tree.size(node);
                    if bytes >= 100_000_000 {
                        let mut it = ReclaimItem::new(
                            "cache:pnpm",
                            "pnpm store prune",
                            Some(store.clone()),
                            bytes,
                            Tier::Regenerable,
                            ReclaimAction::Command { argv: vec!["pnpm".into(), "store".into(), "prune".into()], cwd: None },
                            t("只删除没有项目在用的包", "removes only packages no project references"),
                        )
                        .with_estimate(0)
                        .preselected(true);
                        it.estimate_is_lower_bound = true;
                        let mut th = Thing::new("cache:pnpm", ThingKind::DevCache, "pnpm store", Some(store), bytes);
                        th.items = vec![it];
                        things.push(th);
                    }
                }
            }
        }
    }

    // npx cache
    let npx_base = format!("{}\\AppData\\Local\\npm-cache\\_npx", h);
    if let Some(node) = ctx.tree.find(&npx_base) {
        let total = ctx.tree.size(node);
        if total >= 50_000_000 {
            let mut packages: HashMap<String, (Vec<String>, u64, i64)> = HashMap::new();
            let mut small: Vec<(String, u64)> = vec![];
            for c in ctx.tree.children(node) {
                let n = &ctx.tree.nodes[*c];
                if !n.dir {
                    continue;
                }
                if ctx.now - n.mtime < 7 * 86400 {
                    continue;
                }
                let pkg_json = Path::new(&n.path).join("package.json");
                let deps: Vec<String> = fs::read_to_string(&pkg_json)
                    .ok()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                    .and_then(|v| v.get("dependencies").and_then(|d| d.as_object()).map(|o| o.keys().cloned().collect()))
                    .unwrap_or_default();
                let name = if deps.is_empty() {
                    t("（空）", "(empty)")
                } else {
                    deps[0].clone()
                };
                let e = packages.entry(name).or_insert((vec![], 0, 0));
                e.0.push(n.path.clone());
                e.1 += n.size;
                e.2 += 1;
                if n.size < 10_000_000 {
                    small.push((n.path.clone(), n.size));
                }
            }
            let mut items: Vec<ReclaimItem> = packages
                .into_iter()
                .map(|(name, (paths, bytes, count))| {
                    ReclaimItem::new(
                        format!("npx:{}", name),
                        name,
                        Some(paths[0].clone()),
                        bytes,
                        Tier::Regenerable,
                        ReclaimAction::DeleteAll { paths },
                        t(
                            &format!("npx 装过 {} 份，用 npx 运行时会重新下载", count),
                            &format!("installed {} times by npx, downloaded again when it runs", count),
                        ),
                    )
                    .preselected(true)
                })
                .collect();
            if items.len() <= 3 && small.len() > 1 {
                let paths: Vec<String> = small.iter().map(|(p, _)| p.clone()).collect();
                let bytes: u64 = small.iter().map(|(_, b)| *b).sum();
                items.push(
                    ReclaimItem::new(
                        "npx:small",
                        t(&format!("其他 {} 个小包", small.len()), &format!("{} small packages", small.len())),
                        Some(npx_base.clone()),
                        bytes,
                        Tier::Regenerable,
                        ReclaimAction::DeleteAll { paths },
                        t("npx 装来运行的小包", "small packages installed by npx"),
                    )
                    .preselected(true),
                );
            }
            if !items.is_empty() {
                let mut th = Thing::new(
                    "cache:npx",
                    ThingKind::DevCache,
                    t("npx 缓存", "npx cache"),
                    Some(npx_base),
                    total,
                );
                th.detail = Some(t(
                    &format!("{} 个包，用 npx 运行时会重新下载", items.len()),
                    &format!("{} packages, downloaded again when npx runs them", items.len()),
                ));
                th.items = items;
                things.push(th);
            }
        }
    }

    // Docker / OrbStack
    if which("docker").is_some() {
        if let Ok(o) = Command::new("docker")
            .args(["system", "df", "--format", "{{json .}}"])
            .output()
        {
            if o.status.success() {
                let mut build_cache = 0u64;
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        if v.get("Type").and_then(|t| t.as_str()) == Some("Build Cache") {
                            build_cache = parse_docker_size(v.get("Reclaimable").and_then(|x| x.as_str()).unwrap_or(""));
                        }
                    }
                }
                let dangling = Command::new("docker")
                    .args(["images", "-f", "dangling=true", "--format", "{{.Size}}"])
                    .output()
                    .map(|o| {
                        String::from_utf8_lossy(&o.stdout)
                            .lines()
                            .map(parse_docker_size)
                            .sum::<u64>()
                    })
                    .unwrap_or(0);
                let note = t(
                    "宿主机上的磁盘镜像会稍后才缩小，执行后实测",
                    "the VM disk image shrinks later; measured after running",
                );
                let mut items = vec![];
                if build_cache > 50_000_000 {
                    items.push(
                        ReclaimItem::new(
                            "docker:builder",
                            "docker builder prune",
                            None,
                            build_cache,
                            Tier::Regenerable,
                            ReclaimAction::Command {
                                argv: vec!["docker".into(), "builder".into(), "prune".into(), "-f".into()],
                                cwd: None,
                            },
                            t("镜像构建缓存，下次构建会重新生成", "image build cache, rebuilt on the next build"),
                        )
                        .preselected(true)
                        .with_warning(note.clone()),
                    );
                }
                if dangling > 50_000_000 {
                    items.push(
                        ReclaimItem::new(
                            "docker:dangling",
                            t("悬空镜像", "dangling images"),
                            None,
                            dangling,
                            Tier::Regenerable,
                            ReclaimAction::Command {
                                argv: vec!["docker".into(), "image".into(), "prune".into(), "-f".into()],
                                cwd: None,
                            },
                            t("没有标签、也没有容器在用的旧镜像层", "untagged layers no container uses"),
                        )
                        .preselected(true)
                        .with_warning(note),
                    );
                }
                if !items.is_empty() {
                    let bytes: u64 = items.iter().map(|i| i.bytes).sum();
                    let mut th = Thing::new("cache:docker", ThingKind::DevCache, "Docker / OrbStack", None, bytes);
                    th.detail = Some(t(
                        "不会运行 docker system prune：它会删掉停止的容器和本地构建的镜像",
                        "never runs docker system prune: it deletes stopped containers and local images",
                    ));
                    th.items = items;
                    things.push(th);
                }
            }
        }
    }
    things
}

fn parse_docker_size(s: &str) -> u64 {
    let t = s.split_whitespace().next().unwrap_or(s);
    for (u, m) in [("TB", 1e12), ("GB", 1e9), ("MB", 1e6), ("kB", 1e3), ("KB", 1e3), ("B", 1.0)] {
        if let Some(num) = t.strip_suffix(u) {
            return (num.parse::<f64>().unwrap_or(0.0) * m) as u64;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

fn recognize_models(ctx: &Ctx) -> Vec<Thing> {
    let h = ctx.home.clone();
    let mut things = vec![];
    // Ollama
    let ollama = format!("{}\\.ollama\\models", h);
    let manifests = Path::new(&ollama).join("manifests");
    if manifests.exists() && which("ollama").is_some() {
        let mut seen: HashSet<String> = HashSet::new();
        let mut items = vec![];
        fn walk(dir: &Path, base: &Path, seen: &mut HashSet<String>, items: &mut Vec<ReclaimItem>) {
            let Ok(rd) = fs::read_dir(dir) else { return };
            for e in rd.filter_map(|e| e.ok()) {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, base, seen, items);
                    continue;
                }
                let Ok(data) = fs::read(&p) else { continue };
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) else { continue };
                let Some(layers) = v.get("layers").and_then(|l| l.as_array()) else { continue };
                let mut size = 0u64;
                for l in layers {
                    let Some(dg) = l.get("digest").and_then(|d| d.as_str()) else { continue };
                    let s = l.get("size").and_then(|s| s.as_u64()).unwrap_or(0);
                    if seen.insert(dg.to_string()) {
                        size += s;
                    }
                }
                let rel = p.strip_prefix(base).ok().map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
                let parts: Vec<&str> = rel.split(['\\', '/']).collect();
                if parts.len() < 2 {
                    continue;
                }
                let name = if parts.len() >= 3 && parts[parts.len() - 3] != "library" {
                    format!("{}/{}:{}", parts[parts.len() - 3], parts[parts.len() - 2], parts[parts.len() - 1])
                } else {
                    format!("{}:{}", parts[parts.len() - 2], parts[parts.len() - 1])
                };
                let pulled = fs::metadata(&p)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64);
                items.push(
                    ReclaimItem::new(
                        format!("ollama:{}", name),
                        name.clone(),
                        None,
                        size,
                        Tier::Regenerable,
                        ReclaimAction::Command {
                            argv: vec!["ollama".into(), "rm".into(), name.clone()],
                            cwd: None,
                        },
                        t(
                            &format!("下载于 {}，可以重新 pull", fmt_ago(pulled)),
                            &format!("pulled {}, can be pulled again", fmt_ago(pulled)),
                        ),
                    )
                    .with_restore(format!("ollama pull {}", name))
                    .preselected(false),
                );
            }
        }
        walk(&manifests, &manifests, &mut seen, &mut items);
        if !items.is_empty() {
            items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
            let mut th = Thing::new("model:ollama", ThingKind::Model, "Ollama", Some(ollama.clone()), ctx.size(&ollama));
            th.items = items;
            things.push(th);
        }
    }
    // Hugging Face / ModelScope
    for (id, title, base) in [
        ("hf", "Hugging Face", format!("{}\\.cache\\huggingface\\hub", h)),
        ("modelscope", "ModelScope", format!("{}\\.cache\\modelscope\\hub", h)),
    ] {
        let Some(node) = ctx.tree.find(&base) else { continue };
        let mut items = vec![];
        ctx.tree.walk(node, &mut |j| {
            if j == node {
                return true;
            }
            let name = ctx.tree.name(j);
            if ctx.tree.size(j) >= 100_000_000
                && (name.starts_with("models--") || name.starts_with("datasets--") || id == "modelscope")
            {
                let label = name
                    .trim_start_matches("models--")
                    .trim_start_matches("datasets--")
                    .replace("--", "/");
                let path = ctx.tree.path(j);
                let action = if id == "hf" && which("hf").is_some() {
                    ReclaimAction::Command {
                        argv: vec!["hf".into(), "cache".into(), "rm".into(), label.clone(), "-y".into()],
                        cwd: None,
                    }
                } else {
                    ReclaimAction::Delete { path: path.clone() }
                };
                items.push(
                    ReclaimItem::new(
                        format!("{}:{}", id, label),
                        label,
                        Some(path),
                        ctx.tree.size(j),
                        Tier::Regenerable,
                        action,
                        t(
                            &format!("最后改动 {}", fmt_ago(ctx.tree.newest(j))),
                            &format!("last changed {}", fmt_ago(ctx.tree.newest(j))),
                        ),
                    )
                    .with_restore(t("用到时会重新下载", "re-downloaded when used"))
                    .preselected(false),
                );
                return false;
            }
            true
        });
        if !items.is_empty() {
            items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
            let mut th = Thing::new(format!("model:{}", id), ThingKind::Model, title, Some(base), ctx.tree.size(node));
            th.items = items;
            things.push(th);
        }
    }
    // LM Studio
    let lms = format!("{}\\.lmstudio\\models", h);
    if let Some(node) = ctx.tree.find(&lms) {
        if ctx.tree.size(node) >= 100_000_000 {
            let mut items = vec![];
            for publisher in ctx.tree.children(node) {
                for repo in ctx.tree.children(*publisher) {
                    if ctx.tree.size(*repo) >= 100_000_000 {
                        let path = ctx.tree.path(*repo);
                        let mut it = ReclaimItem::new(
                            format!("lms:{}", path),
                            format!("{}/{}", ctx.tree.name(*publisher), ctx.tree.name(*repo)),
                            Some(path.clone()),
                            ctx.tree.size(*repo),
                            Tier::Yours,
                            ReclaimAction::Trash { path },
                            t("LM Studio 模型", "LM Studio model"),
                        )
                        .preselected(false);
                        if let Some(m) = ctx.tree.newest(*repo) {
                            if ctx.now - m < 7 * 86400 {
                                it.warnings.push(t(
                                    &format!("{} 还有改动，所以没预选", fmt_stamp(m)),
                                    &format!("changed {}, so not preselected", fmt_stamp(m)),
                                ));
                            }
                        }
                        items.push(it);
                    }
                }
            }
            if !items.is_empty() {
                items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
                let mut th = Thing::new("model:lmstudio", ThingKind::Model, "LM Studio", Some(lms), ctx.tree.size(node));
                th.items = items;
                things.push(th);
            }
        }
    }
    things
}

// ---------------------------------------------------------------------------
// App data (guide only)
// ---------------------------------------------------------------------------

fn recognize_app_data(ctx: &Ctx) -> Vec<Thing> {
    let h = ctx.home.clone();
    let local = format!("{}\\AppData\\Local", h);
    let roaming = format!("{}\\AppData\\Roaming", h);
    let known: Vec<(String, String, String)> = vec![
        (format!("{}\\Documents\\WeChat Files", h), t("微信", "WeChat"), t("在微信里打开 设置 → 通用 → 存储空间 清理聊天文件", "In WeChat: Settings → General → Storage")),
        (format!("{}\\Documents\\xwechat_files", h), t("微信", "WeChat"), t("在微信里打开 设置 → 通用 → 存储空间 清理聊天文件", "In WeChat: Settings → General → Storage")),
        (format!("{}\\Apple Computer\\MobileSync\\Backup", roaming), t("iPhone 备份", "iPhone backups"), t("在 访达 → 你的 iPhone → 管理备份 里删除旧备份", "Finder → your iPhone → Manage Backups")),
        (format!("{}\\Claude\\vm_bundles", roaming), t("Claude 虚拟机镜像", "Claude VM bundles"), t("Claude 桌面版运行代码用的虚拟机，删了会重新下载", "VM images used by the Claude app; re-downloaded if removed")),
        (format!("{}\\Google\\Chrome\\User Data", local), "Chrome".into(), t("浏览记录、Cookie 和缓存可以在「浏览器」里清（Chrome 要先退出），或者用 Chrome 的「清除浏览数据」；不要直接删这个文件夹", "Clear history, cookies and cache under Browsers (quit Chrome first) or with Chrome's Clear browsing data; don't delete the folder")),
        (format!("{}\\LarkShell", roaming), t("飞书", "Lark"), t("在飞书 设置 → 通用 → 清理缓存", "In Lark: Settings → General → Clear cache")),
        (format!("{}\\Notion", roaming), "Notion".into(), t("在 Notion 设置里清缓存，或退出后删 Partitions 下的 Cache", "Clear cache in Notion settings")),
    ];
    let mut things = vec![];
    let mut seen = HashSet::new();
    for (path, title, advice) in known {
        if !seen.insert(path_key(&path)) {
            continue;
        }
        let bytes = ctx.size(&path);
        if bytes < 1_000_000_000 {
            continue;
        }
        let mut th = Thing::new(format!("app:{}", path), ThingKind::AppData, title.clone(), Some(path.clone()), bytes);
        th.detail = Some(advice.clone());
        th.items = vec![ReclaimItem::new(
            format!("app:{}", path),
            title,
            Some(path),
            bytes,
            Tier::Guide,
            ReclaimAction::None,
            advice,
        )
        .preselected(false)];
        things.push(th);
    }
    things.sort_by_key(|t| std::cmp::Reverse(t.bytes));
    things
}

// ---------------------------------------------------------------------------
// System
// ---------------------------------------------------------------------------

fn recognize_system(ctx: &Ctx) -> Vec<Thing> {
    let mut things = vec![];
    let scanned = ctx.tree.unique_total;
    let used = ctx.volume.used();
    let hidden = used.saturating_sub(scanned);
    let mut th = Thing::new("system:hidden", ThingKind::System, t("扫描不到的部分", "Not visible to the scan"), None, hidden);
    th.detail = Some(t(
        &format!(
            "磁盘已用 {} − 扫描到 {}：还原点占用、没有权限读的目录（{} 个）等。",
            fmt_bytes(used),
            fmt_bytes(scanned),
            ctx.tree.denied
        ),
        &format!(
            "used {} − scanned {}: restore points, unreadable folders ({}), etc.",
            fmt_bytes(used),
            fmt_bytes(scanned),
            ctx.tree.denied
        ),
    ));
    things.push(th);
    #[cfg(windows)]
    {
        let mut t2 = Thing::new("system:reserved", ThingKind::System, t("系统保留空间", "Reserved by the system"), None, 0);
        t2.detail = Some(t(
            "Windows 会为更新、虚拟内存和系统还原预留一部分空间，由系统管理。",
            "Windows reserves space for updates, virtual memory and restore points; the system manages it.",
        ));
        things.push(t2);
    }
    things
}

// ---------------------------------------------------------------------------
// Large files
// ---------------------------------------------------------------------------

fn large_group(name: &str, is_dir: bool) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    let groups: [(&str, &[&str]); 7] = [
        ("installer", &["dmg", "iso", "img", "pkg", "mpkg", "xip", "ipsw", "msi", "exe", "msix"]),
        ("vm", &["vmdk", "vdi", "qcow2", "vhd", "vhdx", "ova", "hdd"]),
        ("video", &["mp4", "mov", "m4v", "mkv", "avi", "webm", "flv", "wmv", "mts", "m2ts", "mpg", "mpeg", "3gp", "mxf"]),
        ("audio", &["wav", "aif", "aiff", "flac", "mp3", "m4a", "aac", "ogg", "caf", "ape"]),
        ("archive", &["zip", "tar", "gz", "tgz", "bz2", "tbz", "tbz2", "xz", "txz", "zst", "7z", "rar", "lz4", "lzma"]),
        ("model", &["gguf", "safetensors", "ckpt", "pt", "pth", "onnx", "mlmodel", "npz", "h5"]),
        ("app", &["app", "exe"]),
    ];
    if is_dir {
        let lext = name.rsplit('.').next().unwrap_or("").to_lowercase();
        if lext == "app" || lext == "pvm" || lext == "vmwarevm" || lext == "utm" || lext == "vhd" {
            return match lext.as_str() {
                "app" => "app",
                _ => "vm",
            };
        }
        return "other";
    }
    for (g, exts) in groups {
        if exts.contains(&ext.as_str()) {
            return g;
        }
    }
    "other"
}

fn group_title(g: &str) -> String {
    match g {
        "installer" => t("安装包和磁盘映像", "Installers and disk images"),
        "vm" => t("虚拟机", "Virtual machines"),
        "video" => t("视频", "Videos"),
        "audio" => t("音频", "Audio"),
        "archive" => t("压缩包", "Archives"),
        "model" => t("模型权重", "Model weights"),
        "app" => t("「应用程序」以外的应用", "Apps outside Applications"),
        _ => t("其他大文件", "Other large files"),
    }
}

fn recognize_large_files(ctx: &Ctx, covered: &[String]) -> Vec<Thing> {
    let tree = ctx.tree;
    let home = ctx.home.clone();
    let mut roots = vec![];
    if ctx.allow_any_path {
        roots = tree.children(tree.root).to_vec();
    } else if let Some(n) = tree.find(&home) {
        roots.push(n);
    }
    let covered_keys: Vec<String> = covered.iter().map(|p| path_key(p)).collect();
    let mut hits: Vec<(usize, &'static str)> = vec![];
    for r in roots {
        tree.walk(r, &mut |j| {
            if j == r {
                return true;
            }
            let n = &tree.nodes[j];
            let size = n.size;
            if size < 20_000_000 {
                return false;
            }
            let key = path_key(&n.path);
            if covered_keys.iter().any(|c| {
                key.as_str() == c.as_str()
                    || key.starts_with(&(c.clone() + &std::path::MAIN_SEPARATOR.to_string()))
            }) {
                return false;
            }
            let name = n.name.as_str();
            if name.starts_with('.') || ctx.off_limits(&n.path) {
                return false;
            }
            if !user_area_flat(&n.path, &home) && !ctx.allow_any_path {
                return false;
            }
            if !n.dir {
                let g = large_group(name, false);
                let min = if g == "installer" { 20_000_000 } else { 100_000_000 };
                if size >= min {
                    hits.push((j, g));
                }
                return false;
            }
            if ["node_modules", "target", ".venv", "venv", "build", "dist", "out"].contains(&name) {
                return false;
            }
            true
        });
    }
    let mut things = vec![];
    for g in ["vm", "installer", "video", "audio", "archive", "model", "app", "other"] {
        let mut list: Vec<usize> = hits.iter().filter(|(_, gg)| *gg == g).map(|(i, _)| *i).collect();
        if list.is_empty() {
            continue;
        }
        list.sort_by_key(|i| std::cmp::Reverse(tree.size(*i)));
        let total_count = list.len();
        let shown: Vec<usize> = list.into_iter().take(50).collect();
        let items: Vec<ReclaimItem> = shown
            .iter()
            .map(|i| {
                let path = tree.path(*i);
                let name = tree.name(*i).to_string();
                let m = tree.nodes[*i].mtime;
                let mut it = ReclaimItem::new(
                    format!("large:{}", path),
                    name,
                    Some(path.clone()),
                    tree.size(*i),
                    Tier::Yours,
                    ReclaimAction::Trash { path },
                    t(
                        &format!("修改于 {}", fmt_ago(Some(m))),
                        &format!("modified {}", fmt_ago(Some(m))),
                    ),
                )
                .with_restore(t("从废纸篓放回", "put back from the Trash"))
                .preselected(false);
                if m > 0 && ctx.now - m < 7 * 86400 {
                    it.warnings.push(t(
                        &format!("{} 还有改动，所以没预选", fmt_stamp(m)),
                        &format!("changed {}, so not preselected", fmt_stamp(m)),
                    ));
                }
                if g == "vm" {
                    it.warnings.push(t(
                        "虚拟机的整块磁盘：确认里面没有要留的东西",
                        "A whole virtual machine disk: make sure nothing in it is still needed",
                    ));
                }
                it
            })
            .collect();
        let bytes: u64 = items.iter().map(|i| i.bytes).sum();
        let mut th = Thing::new(
            format!("large:{}", g),
            ThingKind::LargeFile,
            group_title(g),
            None,
            bytes,
        );
        let mut detail = t(
            &format!("{} 个", total_count),
            &format!("{} items", total_count),
        );
        if g == "other" {
            detail.push_str(&t(
                "，类型认不出来，可能是别的程序在用的数据",
                "; types not recognized, so they may be another program's data",
            ));
        }
        th.detail = Some(detail);
        th.items = items;
        things.push(th);
    }
    things.sort_by_key(|t| std::cmp::Reverse(t.bytes));
    things
}
