use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub const F_GIT: u32 = 1 << 0;
pub const F_PACKAGE_JSON: u32 = 1 << 1;
pub const F_PYPROJECT: u32 = 1 << 2;
pub const F_CARGO_TOML: u32 = 1 << 3;
pub const F_GO_MOD: u32 = 1 << 4;
pub const F_PACKAGE_SWIFT: u32 = 1 << 5;
pub const F_XCODEPROJ: u32 = 1 << 6;
pub const F_POM_XML: u32 = 1 << 7;
pub const F_GRADLE: u32 = 1 << 8;
pub const F_SETUP_PY: u32 = 1 << 9;
pub const F_CMAKE: u32 = 1 << 10;
pub const F_REQUIREMENTS: u32 = 1 << 11;
pub const F_PUBSPEC: u32 = 1 << 12;
pub const F_LOCK_NPM: u32 = 1 << 13;
pub const F_LOCK_PNPM: u32 = 1 << 14;
pub const F_LOCK_YARN: u32 = 1 << 15;
pub const F_LOCK_BUN: u32 = 1 << 16;
pub const F_LOCK_UV: u32 = 1 << 17;
pub const F_LOCK_POETRY: u32 = 1 << 18;
pub const F_PYVENV_CFG: u32 = 1 << 19;
pub const F_MODEL_CONFIG: u32 = 1 << 20;
pub const F_MODEL_INDEX: u32 = 1 << 21;

pub const MANIFEST_MASK: u32 = F_GIT
    | F_PACKAGE_JSON
    | F_PYPROJECT
    | F_CARGO_TOML
    | F_GO_MOD
    | F_PACKAGE_SWIFT
    | F_XCODEPROJ
    | F_POM_XML
    | F_GRADLE
    | F_SETUP_PY
    | F_CMAKE
    | F_PUBSPEC;

const FILE_NODE_THRESHOLD: u64 = 20_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub own: u64,
    pub dir: bool,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub mtime: i64,
    pub collapsed: bool,
    pub markers: u32,
    pub img_count: u32,
}

impl ScanNode {
    pub fn is_dir(&self) -> bool {
        self.dir
    }
}

#[derive(Clone, Debug)]
pub struct ScanTree {
    pub nodes: Vec<ScanNode>,
    index: HashMap<String, usize>,
    pub root: usize,
    pub unique_total: u64,
    pub denied: u64,
    pub updated_at: i64,
}

pub fn path_key(p: &str) -> String {
    #[cfg(windows)]
    {
        let mut s = p.replace('/', "\\").to_lowercase();
        // Also trim drive roots ("c:\" -> "c:") so prefix checks such as
        // is_under("c:\code", "c:\") keep working.
        while s.len() > 1 && s.ends_with('\\') {
            s.pop();
        }
        s
    }
    #[cfg(not(windows))]
    {
        let mut s = p.replace('\\', "/");
        while s.len() > 1 && s.ends_with('/') {
            s.pop();
        }
        s
    }
}

fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else if base.ends_with('\\') || base.ends_with('/') {
        format!("{}{}", base, name)
    } else {
        format!("{}{}{}", base, std::path::MAIN_SEPARATOR, name)
    }
}

fn base_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn is_image_name(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "webp" | "tif" | "tiff" | "exr" | "dpx" | "bmp" | "tga"
    )
}

fn marker_for(name: &str) -> u32 {
    let lower = name.to_lowercase();
    match lower.as_str() {
        ".git" => F_GIT,
        "package.json" => F_PACKAGE_JSON,
        "pyproject.toml" => F_PYPROJECT,
        "cargo.toml" => F_CARGO_TOML,
        "go.mod" => F_GO_MOD,
        "package.swift" => F_PACKAGE_SWIFT,
        "pom.xml" => F_POM_XML,
        "build.gradle" | "build.gradle.kts" => F_GRADLE,
        "setup.py" => F_SETUP_PY,
        "cmakelists.txt" => F_CMAKE,
        "requirements.txt" => F_REQUIREMENTS,
        "pubspec.yaml" => F_PUBSPEC,
        "package-lock.json" => F_LOCK_NPM,
        "pnpm-lock.yaml" => F_LOCK_PNPM,
        "yarn.lock" => F_LOCK_YARN,
        "bun.lockb" | "bun.lock" => F_LOCK_BUN,
        "uv.lock" => F_LOCK_UV,
        "poetry.lock" => F_LOCK_POETRY,
        "pyvenv.cfg" => F_PYVENV_CFG,
        "model_index.json" => F_MODEL_INDEX,
        "config.json" => F_MODEL_CONFIG,
        _ => {
            if lower.ends_with(".xcodeproj") {
                F_XCODEPROJ
            } else {
                0
            }
        }
    }
}

pub fn folded_name(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "node_modules"
            | ".venv"
            | "venv"
            | "target"
            | ".next"
            | ".nuxt"
            | ".turbo"
            | ".svelte-kit"
            | ".parcel-cache"
            | ".angular"
            | ".build"
            | ".gradle"
            | ".dart_tool"
            | "__pycache__"
            | ".mypy_cache"
            | ".pytest_cache"
            | ".ruff_cache"
            | "DerivedData"
            | "dist"
            | "build"
            | "out"
            | "$RECYCLE.BIN"
            | "System Volume Information"
    ) || name.eq_ignore_ascii_case("$RECYCLE.BIN")
}

#[derive(Default)]
pub struct ScanProgress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub denied: AtomicU64,
}

impl ScanProgress {
    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.files.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }
}

struct Scanner<'a> {
    nodes: Vec<ScanNode>,
    index: HashMap<String, usize>,
    seen: HashSet<(u64, u64)>,
    folded: HashSet<String>,
    skip: Vec<String>,
    denied: u64,
    progress: Option<&'a ScanProgress>,
    cancel: Option<&'a AtomicBool>,
    skipped: bool,
}

fn file_id(meta: &fs::Metadata) -> Option<(u64, u64)> {
    #[cfg(windows)]
    {
        // File index / volume serial live behind unstable std APIs, so hardlink
        // de-duplication is skipped on Windows.
        let _ = meta;
        None
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            return Some((meta.dev(), meta.ino()));
        }
        None
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = meta;
        None
    }
}

fn mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl<'a> Scanner<'a> {
    fn should_skip(&self, path: &str) -> bool {
        self.skip.iter().any(|s| {
            let k = path_key(path);
            let sk = path_key(s);
            k == sk || k.starts_with(&(sk.clone() + &std::path::MAIN_SEPARATOR.to_string()))
        })
    }

    /// Size of a folder treated as one block (no nodes inside).
    fn dir_block(&mut self, path: &str) -> (u64, i64) {
        let mut size = 0u64;
        let mut newest = 0i64;
        let Ok(rd) = fs::read_dir(path) else {
            self.denied += 1;
            return (0, 0);
        };
        let entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        for entry in entries {
            if self.skipped {
                break;
            }
            let ft = match entry.file_type() {
                Ok(f) => f,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                continue;
            }
            let p = entry.path();
            let pstr = p.to_string_lossy().to_string();
            if self.should_skip(&pstr) {
                self.denied += 1;
                continue;
            }
            if ft.is_dir() {
                let (s, m) = self.dir_block(&pstr);
                size += s;
                newest = newest.max(m);
            } else if ft.is_file() {
                if let Ok(meta) = entry.metadata() {
                    let s = meta.len();
                    if let Some(key) = file_id(&meta) {
                        if !self.seen.insert(key) {
                            continue;
                        }
                    }
                    size += s;
                    newest = newest.max(mtime_of(&meta));
                    if let Some(p) = self.progress {
                        p.files.fetch_add(1, Ordering::Relaxed);
                        p.bytes.fetch_add(s, Ordering::Relaxed);
                    }
                }
            }
        }
        (size, newest)
    }

    fn visit_dir(&mut self, path: &str, name: &str, parent: Option<usize>) -> usize {
        let id = self.nodes.len();
        self.nodes.push(ScanNode {
            name: name.to_string(),
            path: path.to_string(),
            size: 0,
            own: 0,
            dir: true,
            parent,
            children: vec![],
            mtime: 0,
            collapsed: false,
            markers: 0,
            img_count: 0,
        });
        self.index.insert(path_key(path), id);
        if let Some(p) = self.progress {
            p.dirs.fetch_add(1, Ordering::Relaxed);
        }

        let mut own = 0u64;
        let mut size = 0u64;
        let mut newest = 0i64;
        let mut markers = 0u32;
        let mut img_count = 0u32;

        let Ok(rd) = fs::read_dir(path) else {
            self.denied += 1;
            if let Some(p) = self.progress {
                p.denied.fetch_add(1, Ordering::Relaxed);
            }
            let node = &mut self.nodes[id];
            node.mtime = now();
            return id;
        };

        let entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        for entry in entries {
            if self.skipped {
                break;
            }
            if let Some(c) = self.cancel {
                if c.load(Ordering::Relaxed) {
                    self.skipped = true;
                    break;
                }
            }
            let ft = match entry.file_type() {
                Ok(f) => f,
                Err(_) => continue,
            };
            if ft.is_symlink() {
                continue;
            }
            let cname = entry.file_name().to_string_lossy().to_string();
            let cpath = entry.path();
            let cpstr = cpath.to_string_lossy().to_string();
            if self.should_skip(&cpstr) {
                self.denied += 1;
                if let Some(p) = self.progress {
                    p.denied.fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
            if cname.starts_with('$') && cname.eq_ignore_ascii_case("$RECYCLE.BIN") {
                continue;
            }
            if cname == "System Volume Information" || cname == "pagefile.sys" || cname == "hiberfil.sys" || cname == "swapfile.sys" {
                continue;
            }
            markers |= marker_for(&cname);
            if ft.is_dir() {
                if folded_name(&cname) {
                    let (sz, m) = self.dir_block(&cpstr);
                    let cid = self.nodes.len();
                    let mut fm = 0u32;
                    if (cname == ".venv" || cname == "venv") && Path::new(&cpstr).join("pyvenv.cfg").exists() {
                        fm |= F_PYVENV_CFG;
                    }
                    self.nodes.push(ScanNode {
                        name: cname,
                        path: cpstr,
                        size: sz,
                        own: sz,
                        dir: true,
                        parent: Some(id),
                        children: vec![],
                        mtime: m,
                        collapsed: true,
                        markers: fm,
                        img_count: 0,
                    });
                    self.nodes[id].children.push(cid);
                    size += sz;
                    // Build output's mtime is not "when the project was worked on".
                } else {
                    let cid = self.visit_dir(&cpstr, &cname, Some(id));
                    let cnode = self.nodes[cid].clone();
                    self.nodes[id].children.push(cid);
                    size += cnode.size;
                    newest = newest.max(cnode.mtime);
                }
            } else if ft.is_file() {
                let Ok(meta) = entry.metadata() else { continue };
                let mut fsz = meta.len();
                if let Some(key) = file_id(&meta) {
                    if !self.seen.insert(key) {
                        fsz = 0;
                    }
                }
                own += fsz;
                size += fsz;
                let mt = mtime_of(&meta);
                newest = newest.max(mt);
                if is_image_name(&cname) {
                    img_count += 1;
                }
                if let Some(p) = self.progress {
                    p.files.fetch_add(1, Ordering::Relaxed);
                    p.bytes.fetch_add(fsz, Ordering::Relaxed);
                }
                if fsz >= FILE_NODE_THRESHOLD {
                    let cid = self.nodes.len();
                    self.nodes.push(ScanNode {
                        name: cname,
                        path: cpstr,
                        size: fsz,
                        own: fsz,
                        dir: false,
                        parent: Some(id),
                        children: vec![],
                        mtime: mt,
                        collapsed: false,
                        markers: 0,
                        img_count: 0,
                    });
                    self.nodes[id].children.push(cid);
                }
            }
        }

        let node = &mut self.nodes[id];
        node.own = own;
        node.size = size;
        node.mtime = if newest == 0 { now() } else { newest };
        node.markers = markers;
        node.img_count = img_count;
        id
    }
}

fn now() -> i64 {
    chrono::Local::now().timestamp()
}

pub struct ScanOptions {
    pub roots: Vec<String>,
    pub folded: HashSet<String>,
    pub skip: Vec<String>,
    pub progress: Option<Arc<ScanProgress>>,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            roots: vec![],
            folded: HashSet::new(),
            skip: vec![],
            progress: None,
            cancel: None,
        }
    }
}

pub fn scan(opts: ScanOptions) -> ScanTree {
    let mut nodes = vec![ScanNode {
        name: String::new(),
        path: String::new(),
        size: 0,
        own: 0,
        dir: true,
        parent: None,
        children: vec![],
        mtime: now(),
        collapsed: false,
        markers: 0,
        img_count: 0,
    }];
    let mut index = HashMap::new();
    index.insert(String::new(), 0);
    let mut s = Scanner {
        nodes,
        index,
        seen: HashSet::new(),
        folded: opts.folded,
        skip: opts.skip,
        denied: 0,
        progress: opts.progress.as_deref(),
        cancel: opts.cancel.as_deref(),
        skipped: false,
    };
    for root in &opts.roots {
        if !Path::new(root).exists() {
            continue;
        }
        let name = base_name(root);
        let id = s.visit_dir(root, &name, Some(0));
        s.nodes[0].children.push(id);
        s.nodes[0].size += s.nodes[id].size;
        s.nodes[0].mtime = s.nodes[0].mtime.max(s.nodes[id].mtime);
    }
    let nodes = std::mem::take(&mut s.nodes);
    let unique_total = nodes[0].size;
    ScanTree {
        nodes,
        index: std::mem::take(&mut s.index),
        root: 0,
        unique_total,
        denied: s.denied,
        updated_at: now(),
    }
}

impl ScanTree {
    pub fn size(&self, i: usize) -> u64 {
        self.nodes.get(i).map(|n| n.size).unwrap_or(0)
    }

    pub fn name(&self, i: usize) -> &str {
        self.nodes.get(i).map(|n| n.name.as_str()).unwrap_or("")
    }

    pub fn path(&self, i: usize) -> String {
        self.nodes.get(i).map(|n| n.path.clone()).unwrap_or_default()
    }

    pub fn parent(&self, i: usize) -> Option<usize> {
        self.nodes.get(i).and_then(|n| n.parent)
    }

    pub fn children(&self, i: usize) -> &[usize] {
        self.nodes
            .get(i)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }

    pub fn sorted_children(&self, i: usize) -> Vec<usize> {
        let mut c: Vec<usize> = self.children(i).to_vec();
        c.sort_by_key(|n| std::cmp::Reverse(self.size(*n)));
        c
    }

    pub fn is_dir(&self, i: usize) -> bool {
        self.nodes.get(i).map(|n| n.dir).unwrap_or(false)
    }

    pub fn markers(&self, i: usize) -> u32 {
        self.nodes.get(i).map(|n| n.markers).unwrap_or(0)
    }

    pub fn newest(&self, i: usize) -> Option<i64> {
        self.nodes
            .get(i)
            .and_then(|n| if n.mtime > 0 { Some(n.mtime) } else { None })
    }

    pub fn find(&self, path: &str) -> Option<usize> {
        self.index.get(&path_key(path)).copied()
    }

    pub fn nearest(&self, path: &str) -> Option<usize> {
        let mut k = path_key(path);
        loop {
            if let Some(i) = self.index.get(&k) {
                return Some(*i);
            }
            let Some(pos) = k.rfind(std::path::MAIN_SEPARATOR).or_else(|| k.rfind('/')) else {
                break;
            };
            if pos < 3 && cfg!(windows) {
                break;
            }
            k.truncate(pos);
        }
        None
    }

    pub fn walk(&self, from: usize, visit: &mut impl FnMut(usize) -> bool) {
        let mut stack = vec![from];
        while let Some(i) = stack.pop() {
            if !visit(i) {
                continue;
            }
            for c in self.children(i) {
                stack.push(*c);
            }
        }
    }

    /// The newest mtime of content outside collapsed (build/cache) folders.
    pub fn source_newest(&self, i: usize) -> i64 {
        let mut newest = self.nodes[i].mtime;
        for c in self.children(i) {
            if self.nodes[*c].collapsed {
                continue;
            }
            let n = self.source_newest(*c);
            newest = newest.max(n);
        }
        newest
    }

    pub fn depth(&self, mut i: usize) -> usize {
        let mut d = 0;
        while let Some(p) = self.parent(i) {
            if p == self.root {
                break;
            }
            d += 1;
            i = p;
        }
        d
    }

    pub fn ancestors(&self, i: usize) -> Vec<usize> {
        let mut out = vec![];
        let mut cur = Some(i);
        while let Some(c) = cur {
            out.push(c);
            cur = self.parent(c);
        }
        out
    }

    /// A tree made from folder sizes, for demo mode. `folders` maps a path to bytes held
    /// directly in that folder; missing parents are created.
    pub fn synthetic(root: &str, folders: &[(String, u64)], folded: &HashSet<String>, modified: i64) -> ScanTree {
        let root = root.trim_end_matches(['\\', '/']).to_string();
        let mut nodes = vec![ScanNode {
            name: base_name(&root),
            path: root.clone(),
            size: 0,
            own: 0,
            dir: true,
            parent: None,
            children: vec![],
            mtime: modified,
            collapsed: false,
            markers: 0,
            img_count: 0,
        }];
        let mut index: HashMap<String, usize> = HashMap::new();
        index.insert(path_key(&root), 0);

        fn ensure(
            path: &str,
            root: &str,
            modified: i64,
            folded: &HashSet<String>,
            nodes: &mut Vec<ScanNode>,
            index: &mut HashMap<String, usize>,
        ) -> usize {
            let key = path_key(path);
            if let Some(i) = index.get(&key) {
                return *i;
            }
            let parent_path = Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| root.to_string());
            let pid = ensure(&parent_path, root, modified, folded, nodes, index);
            let id = nodes.len();
            let name = base_name(path);
            let collapsed = folded.contains(path);
            nodes.push(ScanNode {
                name,
                path: path.to_string(),
                size: 0,
                own: 0,
                dir: true,
                parent: Some(pid),
                children: vec![],
                mtime: modified,
                collapsed,
                markers: 0,
                img_count: 0,
            });
            nodes[pid].children.push(id);
            index.insert(key, id);
            id
        }

        for (path, bytes) in folders {
            let p = path.trim_end_matches(['\\', '/']).to_string();
            if !path_key(&p).starts_with(&path_key(&root)) || path_key(&p) == path_key(&root) {
                continue;
            }
            let id = ensure(&p, &root, modified, folded, &mut nodes, &mut index);
            nodes[id].own += *bytes;
        }
        // Sizes, bottom-up (children always have larger indices than their parents).
        for i in (0..nodes.len()).rev() {
            let own = nodes[i].own;
            let mut size = own;
            let children = nodes[i].children.clone();
            for c in children {
                size += nodes[c].size;
            }
            nodes[i].size = size;
        }
        let unique_total = nodes[0].size;
        ScanTree {
            nodes,
            index,
            root: 0,
            unique_total,
            denied: 0,
            updated_at: modified,
        }
    }
}

/// Recursively measures a folder, for apps outside the scanned tree.
pub fn measure_dir(path: &str) -> u64 {
    fn rec(path: &Path, depth: usize) -> u64 {
        if depth > 64 {
            return 0;
        }
        let Ok(rd) = fs::read_dir(path) else {
            return 0;
        };
        let mut total = 0;
        for entry in rd.filter_map(|e| e.ok()) {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                total += rec(&entry.path(), depth + 1);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
        total
    }
    rec(Path::new(path), 0)
}

/// Quick check that a directory has at least one of the given file names.
pub fn dir_has(dir: &str, name: &str) -> bool {
    PathBuf::from(dir).join(name).exists()
}
