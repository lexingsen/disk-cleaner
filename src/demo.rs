use std::collections::{HashMap, HashSet};

use crate::fmt::{fmt_ago, fmt_bytes, fmt_stamp, is_zh, t};
use crate::history::{DayPoint, ThingPoint};
use crate::model::*;
use crate::scan::ScanTree;
use crate::volume::{ExternalVolume, VolumeInfo};

pub struct DemoData {
    pub report: Report,
    pub tree: ScanTree,
    pub history: Vec<DayPoint>,
    pub thing_history: Vec<ThingPoint>,
    pub volume: VolumeInfo,
    pub user_files: u64,
    pub applications: u64,
    pub external: Option<DemoExternal>,
}

pub struct DemoExternal {
    pub volume: ExternalVolume,
    pub report: Report,
    pub tree: ScanTree,
    pub things: Vec<Thing>,
}

fn gb<T: Into<f64>>(x: T) -> u64 {
    (x.into() * 1e9) as u64
}

pub fn make() -> DemoData {
    let now = chrono::Local::now().timestamp();
    let mut b = Builder {
        now,
        home: crate::fmt::home_string(),
        things: vec![],
        app_folders: HashMap::new(),
    };
    b.projects();
    b.media();
    b.dev_caches();
    b.models();
    b.apps();
    b.app_data();
    b.large_files();
    b.browsers();

    let total = 994_662_584_320u64;
    let free = gb(61.4);
    let used = total - free;
    let hidden = gb(17.6);
    let other_volumes = gb(30.9);
    b.system(used, hidden, other_volumes);

    let report = Report {
        created_at: now - 4 * 60,
        volume_total: total,
        volume_free: free,
        scanned: used.saturating_sub(hidden),
        things: b.things.clone(),
        language: Some(if is_zh() { "zh".into() } else { "en".into() }),
    };
    let (history, thing_history) = b.history(free);
    DemoData {
        report,
        tree: b.tree(),
        history,
        thing_history,
        volume: VolumeInfo {
            name: if is_zh() { "启动盘".into() } else { "Startup Disk".into() },
            path: crate::fmt::home_string(),
            uuid: "demo".into(),
            total,
            free,
        },
        user_files: gb(262),
        applications: gb(62),
        external: Some(external_disk(now)),
    }
}

fn external_disk(now: i64) -> DemoExternal {
    let root = "D:\\Studio SSD".to_string();
    let volume = ExternalVolume {
        name: "Studio SSD".into(),
        path: root.clone(),
        uuid: "demo-studio-ssd".into(),
        total: 2_000_189_177_856,
        free: gb(612),
    };
    let ago = |days: f64| now - (days * 86400.0) as i64;
    let art = |path: String, kind: &str, size: f64, restore: &str, pre: bool, reason: String| {
        ReclaimItem::new(
            format!("art:{}", path),
            kind,
            Some(path.clone()),
            gb(size),
            Tier::Regenerable,
            ReclaimAction::Delete { path },
            reason,
        )
        .with_restore(restore)
        .preselected(pre)
    };
    let npm = t("npm 依赖，可以按锁文件重新安装", "npm dependencies, reinstallable from the lockfile");
    let rust = t("Rust/Java 编译产物，重新编译即可", "Rust/Java build output, rebuilt by compiling");
    let py = t("Python 虚拟环境，可以重建", "Python virtualenv, can be recreated");
    let p = format!("{}\\Projects\\", root);
    let film = format!("{}\\Films\\harbor-documentary", root);
    let mut things = vec![
        {
            let path = format!("{}client-portal-2024", p);
            let mut th = Thing::new(
                format!("project:{}", path),
                ThingKind::Project,
                "client-portal-2024",
                Some(path.clone()),
                gb(6.8),
            );
            th.last_active = Some(ago(412.0));
            th.activity_source = Some(t("文件改动", "file changes"));
            th.detail = Some(t("沉睡 412 天", "asleep for 412 days"));
            th.items = vec![
                art(format!("{}\\node_modules", path), "node_modules", 4.1, "npm ci", true, npm.clone()),
                art(
                    format!("{}\\dist", path),
                    "dist",
                    0.9,
                    "npm run build",
                    true,
                    t("构建输出，被 git 忽略", "build output ignored by git"),
                ),
            ];
            th
        },
        {
            let path = format!("{}tile-engine", p);
            let mut th = Thing::new(
                format!("project:{}", path),
                ThingKind::Project,
                "tile-engine",
                Some(path.clone()),
                gb(14.2),
            );
            th.last_active = Some(ago(198.0));
            th.activity_source = Some("Claude Code".into());
            th.detail = Some(t("沉睡 198 天", "asleep for 198 days"));
            th.items = vec![art(format!("{}\\target", path), "target", 12.6, "cargo build", true, rust.clone())];
            th
        },
        {
            let path = format!("{}speech-dataset-tools", p);
            let mut th = Thing::new(
                format!("project:{}", path),
                ThingKind::Project,
                "speech-dataset-tools",
                Some(path.clone()),
                gb(5.3),
            );
            th.last_active = Some(ago(12.0));
            th.activity_source = Some("Codex".into());
            th.items = vec![art(format!("{}\\.venv", path), "venv", 4.4, "uv sync", false, py.clone())];
            th
        },
    ];
    let mut media = Thing::new(
        format!("media:{}", film),
        ThingKind::Media,
        t("渲染中间帧 · harbor-documentary", "Render frames · harbor-documentary"),
        Some(film.clone()),
        gb(48.6),
    );
    media.detail = Some(t("1 组帧序列", "1 frame folders"));
    media.items = vec![ReclaimItem::new(
        format!("frames:{}\\render\\frames", film),
        t("渲染帧 14400 张", "14,400 rendered frames"),
        Some(format!("{}\\render\\frames", film)),
        gb(48.6),
        Tier::Regenerable,
        ReclaimAction::Delete {
            path: format!("{}\\render\\frames", film),
        },
        t("成片已存在：harbor-final.mov（帧数与时长 × 帧率吻合）", "final video exists: harbor-final.mov (frame count matches duration × fps)"),
    )
    .with_restore(t("用原工程重新渲染", "re-render from the source project"))
    .preselected(true)];
    things.push(media);

    let mut folders: HashMap<String, u64> = HashMap::new();
    folders.insert(format!("{}\\Footage", root), gb(742.0));
    folders.insert(format!("{}\\Photos", root), gb(318.0));
    folders.insert(format!("{}\\Backups", root), gb(166.0));
    folders.insert(format!("{}\\Films", root), gb(61.0));
    folders.insert(format!("{}\\Projects", root), gb(3.0));
    let mut folded = HashSet::new();
    for th in &things {
        let mut rest = th.bytes;
        for i in &th.items {
            if let Some(ip) = &i.path {
                *folders.entry(ip.clone()).or_insert(0) += i.bytes;
                rest = rest.saturating_sub(i.bytes);
                let last = ip.rsplit('\\').next().unwrap_or("");
                if ["node_modules", "venv", "target"].contains(&last) {
                    folded.insert(ip.clone());
                }
            }
        }
        if let Some(tp) = &th.path {
            *folders.entry(tp.clone()).or_insert(0) += rest;
        }
    }
    let tree = ScanTree::synthetic(&root, &folders.into_iter().collect::<Vec<_>>(), &folded, ago(3.0));
    let report = Report {
        created_at: now - 20 * 60,
        volume_total: volume.total,
        volume_free: volume.free,
        scanned: volume.total - volume.free,
        things: things.clone(),
        language: Some(if is_zh() { "zh".into() } else { "en".into() }),
    };
    DemoExternal {
        volume,
        report,
        tree,
        things,
    }
}

struct Builder {
    now: i64,
    home: String,
    things: Vec<Thing>,
    app_folders: HashMap<String, u64>,
}

impl Builder {
    fn ago(&self, days: f64) -> i64 {
        self.now - (days * 86400.0) as i64
    }

    fn artifact(
        &self,
        path: String,
        kind: &str,
        size: f64,
        estimate: Option<f64>,
        restore: &str,
        pre: bool,
        warnings: Vec<String>,
    ) -> ReclaimItem {
        let reason = match kind {
            "node_modules" => t("npm 依赖，可以按锁文件重新安装", "npm dependencies, reinstallable from the lockfile"),
            "venv" => t("Python 虚拟环境，可以重建", "Python virtualenv, can be recreated"),
            "target" => t("Rust/Java 编译产物，重新编译即可", "Rust/Java build output, rebuilt by compiling"),
            ".build" => t("SwiftPM 编译产物", "SwiftPM build output"),
            ".next" => t("前端构建缓存", "front-end build cache"),
            _ => t("构建输出，被 git 忽略", "build output ignored by git"),
        };
        let mut item = ReclaimItem::new(
            format!("art:{}", path),
            kind,
            Some(path.clone()),
            gb(size),
            Tier::Regenerable,
            ReclaimAction::Delete { path },
            reason,
        )
        .with_restore(restore)
        .preselected(pre);
        if let Some(e) = estimate {
            item.estimate = gb(e);
        }
        item.warnings = warnings;
        item
    }

    fn shared(&self, size: f64) -> String {
        if is_zh() {
            format!(
                "其中 {} 和别处共享（克隆或硬链接），单删这一项腾不出来",
                fmt_bytes(gb(size))
            )
        } else {
            format!(
                "{} is shared with other places and stays allocated",
                fmt_bytes(gb(size))
            )
        }
    }

    fn recent(&self) -> String {
        let d = fmt_stamp(self.now - 2 * 86400 - 3 * 3600);
        t(
            &format!("{} 还有改动，所以没预选", d),
            &format!("changed {}, so not preselected", d),
        )
    }

    fn project(&mut self, name: &str, size: f64, days: i64, source: &str, items: Vec<ReclaimItem>) {
        let path = format!("{}\\workspace\\{}", self.home, name);
        let mut th = Thing::new(
            format!("project:{}", path),
            ThingKind::Project,
            name,
            Some(path),
            gb(size),
        );
        th.last_active = Some(self.ago(days as f64 + 0.3));
        th.activity_source = Some(source.to_string());
        if days >= 30 {
            th.detail = Some(t(&format!("沉睡 {} 天", days), &format!("asleep for {} days", days)));
        }
        th.items = items;
        self.things.push(th);
    }

    fn projects(&mut self) {
        let w = format!("{}\\workspace\\", self.home);
        let files = t("文件改动", "file changes");
        self.project(
            "pixel-forge",
            22.9,
            74,
            "Claude Code",
            vec![
                self.artifact(format!("{}pixel-forge\\target", w), "target", 18.4, None, "cargo build", true, vec![]),
                self.artifact(format!("{}pixel-forge\\web\\node_modules", w), "node_modules", 1.3, None, "pnpm install", true, vec![]),
                self.artifact(format!("{}pixel-forge\\web\\.next", w), ".next", 0.62, None, "npm run build", true, vec![]),
            ],
        );
        self.project(
            "voice-clone-lab",
            8.1,
            46,
            "Codex",
            vec![
                self.artifact(
                    format!("{}voice-clone-lab\\.venv", w),
                    "venv",
                    7.2,
                    Some(1.9),
                    "uv sync",
                    true,
                    vec![self.shared(5.3)],
                ),
                {
                    let mut it = ReclaimItem::new(
                        format!("pycache:{}voice-clone-lab", w),
                        t("Python 缓存 ×23", "Python caches ×23"),
                        None,
                        gb(0.21),
                        Tier::Regenerable,
                        ReclaimAction::DeleteAll { paths: vec![] },
                        t("运行时自动重建", "rebuilt automatically when code runs"),
                    )
                    .preselected(true);
                    it.path = None;
                    it
                },
            ],
        );
        self.project(
            "rag-notes",
            3.6,
            58,
            "Claude Code",
            vec![self.artifact(
                format!("{}rag-notes\\.venv", w),
                "venv",
                3.1,
                Some(0.4),
                "uv sync",
                true,
                vec![self.shared(2.7)],
            )],
        );
        let recent = self.recent();
        self.project(
            "storyboard-ai",
            4.6,
            2,
            "Claude Code",
            vec![
                self.artifact(
                    format!("{}storyboard-ai\\node_modules", w),
                    "node_modules",
                    2.3,
                    None,
                    "pnpm install",
                    false,
                    vec![recent.clone()],
                ),
                self.artifact(
                    format!("{}storyboard-ai\\.next", w),
                    ".next",
                    1.1,
                    None,
                    "npm run build",
                    false,
                    vec![recent.clone()],
                ),
            ],
        );
        self.project(
            "comfy-workflows",
            5.1,
            9,
            "Claude Code",
            vec![self.artifact(format!("{}comfy-workflows\\.venv", w), "venv", 4.6, None, "uv sync", false, vec![])],
        );
        self.project(
            "habit-tracker-ios",
            4.0,
            133,
            &files,
            vec![self.artifact(format!("{}habit-tracker-ios\\.build", w), ".build", 3.4, None, "swift build", true, vec![])],
        );
        self.project(
            "agent-playground",
            2.0,
            0,
            "Claude Code",
            vec![self.artifact(
                format!("{}agent-playground\\node_modules", w),
                "node_modules",
                1.6,
                None,
                "bun install",
                false,
                vec![recent.clone()],
            )],
        );
        self.project(
            "landing-page-2025",
            1.3,
            210,
            "shell",
            vec![
                self.artifact(
                    format!("{}landing-page-2025\\node_modules", w),
                    "node_modules",
                    0.98,
                    None,
                    "npm install",
                    false,
                    vec![t(
                        "没有锁文件，重装后的版本可能不同",
                        "no lockfile: reinstall may resolve different versions",
                    )],
                ),
                self.artifact(
                    format!("{}landing-page-2025\\dist", w),
                    "dist",
                    0.12,
                    None,
                    "npm run build",
                    true,
                    vec![],
                ),
            ],
        );
    }

    fn media(&mut self) {
        let frames = |path: String, count: i64, size: f64, video: String, pre: bool, warnings: Vec<String>| {
            let mut it = ReclaimItem::new(
                format!("frames:{}", path),
                t(&format!("渲染帧 {} 张", count), &format!("{} rendered frames", count)),
                Some(path.clone()),
                gb(size),
                Tier::Regenerable,
                ReclaimAction::Delete { path },
                t(
                    &format!("成片已存在：{}（帧数与时长 × 帧率吻合）", crate::fmt::fmt_home(&video)),
                    &format!("final video exists: {} (frame count matches duration × fps)", crate::fmt::fmt_home(&video)),
                ),
            )
            .with_restore(t("用原工程重新渲染", "re-render from the source project"))
            .preselected(pre);
            it.warnings = warnings;
            it
        };
        let promo = format!("{}\\Movies\\promo-video", self.home);
        let stills = format!("{}\\stills", promo);
        let mut t1 = Thing::new(
            format!("media:{}", promo),
            ThingKind::Media,
            t("渲染中间帧 · promo-video", "Render frames · promo-video"),
            Some(promo.clone()),
            gb(24.7),
        );
        t1.detail = Some(t("2 组帧序列", "2 frame folders"));
        t1.items = vec![
            frames(format!("{}\\frames", promo), 7304, 21.6, format!("{}\\promo-final.mp4", promo), true, vec![]),
            ReclaimItem::new(
                format!("frames:{}", stills),
                t("图片序列 1800 张", "1800-image sequence"),
                Some(stills.clone()),
                gb(3.1),
                Tier::Yours,
                ReclaimAction::Trash { path: stills },
                t("像渲染帧，但没找到对应的成片，按你的文件处理", "looks like frames, but no matching final video was found"),
            )
            .preselected(false),
        ];
        self.things.push(t1);

        let ep = format!("{}\\workspace\\explainer-ep3", self.home);
        let mut t2 = Thing::new(
            format!("media:{}", ep),
            ThingKind::Media,
            t("渲染中间帧 · explainer-ep3", "Render frames · explainer-ep3"),
            Some(ep.clone()),
            gb(8.2),
        );
        t2.detail = Some(t("1 组帧序列", "1 frame folders"));
        let d = fmt_stamp(self.now - 86400 - 5 * 3600);
        t2.items = vec![frames(
            format!("{}\\out\\frames", ep),
            2910,
            8.2,
            format!("{}\\out\\explainer-ep3.mp4", ep),
            false,
            vec![t(
                &format!("{} 刚渲染过，所以没预选", d),
                &format!("rendered {}, so not preselected", d),
            )],
        )];
        self.things.push(t2);
    }

    fn cache(&mut self, id: &str, title: &str, path: String, size: f64, action: ReclaimAction, reason: String, pre: bool) {
        let mut th = Thing::new(format!("cache:{}", id), ThingKind::DevCache, title, Some(path.clone()), gb(size));
        let mut item = ReclaimItem::new(
            format!("cache:{}", id),
            title,
            Some(path),
            gb(size),
            Tier::Regenerable,
            action,
            reason,
        )
        .preselected(pre);
        item.bytes = gb(size);
        th.items = vec![item];
        self.things.push(th);
    }

    fn dev_caches(&mut self) {
        let h = self.home.clone();
        let uv = format!("{}\\.cache\\uv", h);
        let venvs = vec![
            format!("{}\\workspace\\voice-clone-lab\\.venv", h),
            format!("{}\\workspace\\rag-notes\\.venv", h),
        ];
        let mut prune = ReclaimItem::new(
            "uv:prune",
            "uv cache prune",
            Some(uv.clone()),
            gb(31.5),
            Tier::Regenerable,
            ReclaimAction::Command { argv: vec!["uv".into(), "cache".into(), "prune".into()], cwd: None },
            t("删除旧版本格式的缓存和没人引用的条目", "drops old-format buckets and unreferenced entries"),
        )
        .with_estimate(gb(4.8))
        .preselected(false);
        prune.estimate_is_lower_bound = true;
        let mut clean = ReclaimItem::new(
            "uv:clean",
            "uv cache clean",
            Some(uv.clone()),
            gb(31.5),
            Tier::Regenerable,
            ReclaimAction::Command { argv: vec!["uv".into(), "cache".into(), "clean".into()], cwd: None },
            t("清空整个 uv 缓存，以后装包会重新下载", "empties the whole uv cache; packages are re-downloaded later"),
        )
        .with_estimate(gb(9.6))
        .preselected(false)
        .with_warning(t(
            "其中 14.2 GB 被现有 .venv 克隆引用，单清缓存腾不出来",
            "14.2 GB is cloned into existing venvs and stays until they go too",
        ));
        clean.covers = vec!["uv:prune".into()];
        let mut combo_actions = vec![ReclaimAction::Command {
            argv: vec!["uv".into(), "cache".into(), "clean".into()],
            cwd: None,
        }];
        combo_actions.extend(venvs.iter().map(|v| ReclaimAction::Delete { path: v.clone() }));
        let mut combo = ReclaimItem::new(
            "uv:combo",
            t("uv cache clean + 2 个沉睡项目的 .venv", "uv cache clean + 2 sleeping-project venvs"),
            Some(uv.clone()),
            gb(41.8),
            Tier::Regenerable,
            ReclaimAction::Compound { actions: combo_actions },
            t(
                "缓存和沉睡项目的环境一起清，克隆共享的数据才会真正释放",
                "clearing the cache together with sleeping venvs frees the cloned data too",
            ),
        )
        .with_estimate(gb(26.4))
        .with_restore(t("在各项目里运行 uv sync", "run uv sync in each project"))
        .preselected(true)
        .with_warning(t("仍有 3.1 GB 被活跃项目的 .venv 引用", "3.1 GB stays referenced by active venvs"));
        let mut covers = vec!["uv:prune".to_string(), "uv:clean".to_string()];
        covers.extend(venvs.iter().map(|v| format!("art:{}", v)));
        combo.covers = covers;
        let mut uv_thing = Thing::new("cache:uv", ThingKind::DevCache, "uv cache", Some(uv.clone()), gb(31.5));
        uv_thing.detail = Some(t(
            "单清缓存实际腾出 9.6 GB；14.2 GB 与 .venv 共享",
            "clearing alone frees 9.6 GB; 14.2 GB is shared with venvs",
        ));
        uv_thing.items = vec![prune, clean, combo];
        self.things.push(uv_thing);

        let pnpm_path = format!("{}\\AppData\\Local\\pnpm\\store", h);
        let mut pnpm = ReclaimItem::new(
            "cache:pnpm",
            "pnpm store prune",
            Some(pnpm_path.clone()),
            gb(12.3),
            Tier::Regenerable,
            ReclaimAction::Command { argv: vec!["pnpm".into(), "store".into(), "prune".into()], cwd: None },
            t("只删除没有项目在用的包", "removes only packages no project references"),
        )
        .with_estimate(gb(3.2))
        .preselected(true);
        pnpm.estimate_is_lower_bound = true;
        let mut pnpm_thing = Thing::new("cache:pnpm", ThingKind::DevCache, "pnpm store", Some(pnpm_path), gb(12.3));
        pnpm_thing.items = vec![pnpm];
        self.things.push(pnpm_thing);

        self.cache(
            "npm",
            "npm cache",
            format!("{}\\AppData\\Local\\npm-cache\\_cacache", h),
            4.1,
            ReclaimAction::Command {
                argv: vec!["npm".into(), "cache".into(), "clean".into(), "--force".into()],
                cwd: None,
            },
            t("npm 下载缓存，需要时会重新下载", "npm download cache, re-downloaded on demand"),
            true,
        );
        self.cache(
            "cargo-registry",
            "Cargo registry",
            format!("{}\\.cargo\\registry", h),
            1.9,
            ReclaimAction::Delete {
                path: format!("{}\\.cargo\\registry", h),
            },
            t("crate 源码和压缩包，编译时会重新下载", "crate sources, re-downloaded when building"),
            false,
        );
        self.cache(
            "puppeteer",
            "Puppeteer browsers",
            format!("{}\\.cache\\puppeteer", h),
            1.1,
            ReclaimAction::Delete {
                path: format!("{}\\.cache\\puppeteer", h),
            },
            t("Puppeteer 下载的浏览器，需要时会重新下载", "browsers downloaded by Puppeteer"),
            false,
        );

        let note = t(
            "宿主机上的磁盘镜像会稍后才缩小，执行后实测",
            "the VM disk image shrinks later; measured after running",
        );
        let mut docker = Thing::new("cache:docker", ThingKind::DevCache, "Docker / OrbStack", None, gb(9.6));
        docker.detail = Some(t(
            "不会运行 docker system prune：它会删掉停止的容器和本地构建的镜像",
            "never runs docker system prune: it deletes stopped containers and local images",
        ));
        docker.items = vec![
            ReclaimItem::new(
                "docker:builder",
                "docker builder prune",
                None,
                gb(7.4),
                Tier::Regenerable,
                ReclaimAction::Command {
                    argv: vec!["docker".into(), "builder".into(), "prune".into(), "-f".into()],
                    cwd: None,
                },
                t("镜像构建缓存，下次构建会重新生成", "image build cache, rebuilt on the next build"),
            )
            .preselected(true)
            .with_warning(note.clone()),
            ReclaimItem::new(
                "docker:dangling",
                t("悬空镜像", "dangling images"),
                None,
                gb(2.2),
                Tier::Regenerable,
                ReclaimAction::Command {
                    argv: vec!["docker".into(), "image".into(), "prune".into(), "-f".into()],
                    cwd: None,
                },
                t("没有标签、也没有容器在用的旧镜像层", "untagged layers no container uses"),
            )
            .preselected(true)
            .with_warning(note),
        ];
        self.things.push(docker);
    }

    fn models(&mut self) {
        let h = self.home.clone();
        let ollama = |name: &str, size: f64, days: f64| {
            ReclaimItem::new(
                format!("ollama:{}", name),
                name,
                None,
                gb(size),
                Tier::Regenerable,
                ReclaimAction::Command {
                    argv: vec!["ollama".into(), "rm".into(), name.into()],
                    cwd: None,
                },
                t(
                    &format!("下载于 {}，可以重新 pull", fmt_ago(Some(chrono::Local::now().timestamp() - (days * 86400.0) as i64))),
                    &format!("pulled {}, can be pulled again", fmt_ago(Some(chrono::Local::now().timestamp() - (days * 86400.0) as i64))),
                ),
            )
            .with_restore(format!("ollama pull {}", name))
            .preselected(false)
        };
        let mut ol = Thing::new("model:ollama", ThingKind::Model, "Ollama", Some(format!("{}\\.ollama\\models", h)), gb(37.5));
        ol.items = vec![
            ollama("qwen3:32b", 20.2, 95.0),
            ollama("gemma3:27b", 17.0, 150.0),
            ollama("nomic-embed-text:latest", 0.27, 240.0),
        ];
        self.things.push(ol);

        let hub = format!("{}\\.cache\\huggingface\\hub", h);
        let hf = |repo: &str, size: f64, days: f64| {
            let path = format!("{}\\models--{}", hub, repo.replace('/', "--"));
            ReclaimItem::new(
                format!("hf:{}", repo),
                repo,
                Some(path),
                gb(size),
                Tier::Regenerable,
                ReclaimAction::Command {
                    argv: vec!["hf".into(), "cache".into(), "rm".into(), repo.into(), "-y".into()],
                    cwd: None,
                },
                t(
                    &format!("最后改动 {}", fmt_ago(Some(chrono::Local::now().timestamp() - (days * 86400.0) as i64))),
                    &format!("last changed {}", fmt_ago(Some(chrono::Local::now().timestamp() - (days * 86400.0) as i64))),
                ),
            )
            .with_restore(t("用到时会重新下载", "re-downloaded when used"))
            .preselected(false)
        };
        let mut hfthing = Thing::new("model:hf", ThingKind::Model, "Hugging Face", Some(hub.clone()), gb(27.0));
        hfthing.items = vec![hf("black-forest-labs/FLUX.1-dev", 23.8, 120.0), hf("openai/whisper-large-v3", 3.1, 64.0)];
        self.things.push(hfthing);

        let lms = format!("{}\\.lmstudio\\models", h);
        let qwen = format!("{}\\lmstudio-community\\Qwen3-14B-GGUF", lms);
        let mut lms_thing = Thing::new("model:lmstudio", ThingKind::Model, "LM Studio", Some(lms), gb(9.0));
        lms_thing.items = vec![ReclaimItem::new(
            format!("lms:{}", qwen),
            "lmstudio-community/Qwen3-14B-GGUF",
            Some(qwen.clone()),
            gb(9.0),
            Tier::Yours,
            ReclaimAction::Trash { path: qwen },
            t("LM Studio 模型", "LM Studio model"),
        )
        .preselected(false)];
        self.things.push(lms_thing);
    }

    fn app(
        &mut self,
        name: &str,
        file: &str,
        size: f64,
        days: Option<f64>,
        data: Vec<(&str, f64)>,
        caches: Vec<(&str, f64)>,
        app_store: bool,
        warnings: Vec<String>,
    ) {
        let path = format!("C:\\Program Files\\{}", file);
        let lib = format!("{}\\AppData\\", self.home);
        let data_bytes: f64 = data.iter().map(|(_, g)| *g).sum();
        for (rel, g) in &data {
            *self.app_folders.entry(format!("{}{}", lib, rel)).or_insert(0) += gb(*g);
        }
        for (rel, g) in &caches {
            if data.iter().any(|(d, _)| d == rel) {
                continue;
            }
            if let Some(outer) = data.iter().find(|(d, _)| rel.starts_with(&format!("{}/", d))) {
                let key = format!("{}{}", lib, outer.0);
                let cur = *self.app_folders.get(&key).unwrap_or(&0);
                *self.app_folders.entry(key).or_insert(0) = cur.saturating_sub(gb(*g));
            }
            *self.app_folders.entry(format!("{}{}", lib, rel)).or_insert(0) += gb(*g);
        }
        let mut actions = vec![ReclaimAction::Trash { path: path.clone() }];
        actions.extend(data.iter().map(|(rel, _)| ReclaimAction::Trash { path: format!("{}{}", lib, rel) }));
        let mut uninstall = ReclaimItem::new(
            format!("apps:uninstall:{}", path),
            t(&format!("卸载 {}", name), &format!("Uninstall {}", name)),
            Some(path.clone()),
            gb(size + data_bytes),
            Tier::Yours,
            ReclaimAction::Compound { actions },
            t(
                &format!(
                    "应用 {}，连同资源库里 {} 处数据 {}，一起移到废纸篓",
                    fmt_bytes(gb(size)),
                    data.len(),
                    fmt_bytes(gb(data_bytes))
                ),
                &format!(
                    "the app ({}) and its data in {} places in your Library ({}) go to the Trash",
                    fmt_bytes(gb(size)),
                    data.len(),
                    fmt_bytes(gb(data_bytes))
                ),
            ),
        )
        .with_restore(if app_store {
            t(
                "从应用商店重新安装；数据可以从废纸篓放回",
                "reinstall from the store; put the data back from the Trash",
            )
        } else {
            t("从废纸篓放回", "put back from the Trash")
        })
        .preselected(false)
        .with_app(path.clone());
        uninstall.warnings = warnings;
        let mut items = vec![uninstall];
        if !caches.is_empty() {
            let cache_bytes: f64 = caches.iter().map(|(_, g)| *g).sum();
            let mut c = ReclaimItem::new(
                format!("apps:caches:{}", path),
                t(&format!("{} 的缓存", name), &format!("{} caches", name)),
                None,
                gb(cache_bytes),
                Tier::Regenerable,
                ReclaimAction::DeleteAll {
                    paths: caches.iter().map(|(rel, _)| format!("{}{}", lib, rel)).collect(),
                },
                t(
                    "缓存、日志和窗口状态，删掉后应用会自己重建；应用和你的数据都留着",
                    "caches, logs and saved window state, which the app rebuilds; the app and your data stay",
                ),
            )
            .with_restore(t("应用下次打开时会重建", "rebuilt when the app next runs"))
            .preselected(false)
            .with_app(path.clone());
            c.path = None;
            let cid = c.id.clone();
            items[0].covers = vec![cid];
            items.push(c);
        }
        let mut th = Thing::new(
            format!("apps:{}", path),
            ThingKind::App,
            name,
            Some(path),
            gb(size + data_bytes),
        );
        th.last_active = days.map(|d| self.ago(d + 0.2));
        if let Some(d) = days {
            if d >= 90.0 {
                th.detail = Some(t(&format!("{} 天没打开", d as i64), &format!("unused for {} days", d as i64)));
            }
        }
        th.items = items;
        self.things.push(th);
    }

    fn apps(&mut self) {
        self.app(
            "Docker",
            "Docker\\Docker.exe",
            2.1,
            Some(41.0),
            vec![
                ("Roaming\\Docker", 38.4),
                ("Local\\Docker", 0.31),
                ("Roaming\\Docker\\Preferences", 0.0001),
            ],
            vec![("Local\\Docker", 0.31)],
            false,
            vec![t(
                "它在系统里还装了组件（Docker Desktop Service），这里卸载不会动它们；有自带的卸载程序的话最好用它",
                "It installed system components that uninstalling here leaves in place; use its own uninstaller if it has one",
            )],
        );
        self.app(
            "Steam",
            "Steam\\Steam.exe",
            0.03,
            Some(263.0),
            vec![("Roaming\\Steam", 27.3), ("Local\\Steam\\Logs", 0.04)],
            vec![("Local\\Steam\\Logs", 0.04)],
            false,
            vec![],
        );
        self.app(
            "Final Cut Pro",
            "Final Cut Pro\\Final Cut Pro.exe",
            5.9,
            Some(158.0),
            vec![("Roaming\\Apple\\FinalCut", 1.6), ("Local\\Apple\\FinalCut", 0.92)],
            vec![("Local\\Apple\\FinalCut", 0.92)],
            true,
            vec![t("移到废纸篓时可能需要管理员权限", "moving it to the Trash may need administrator rights")],
        );
        self.app(
            "Slack",
            "Slack\\slack.exe",
            0.52,
            Some(0.0),
            vec![("Roaming\\Slack", 2.4), ("Local\\Slack\\ShipIt", 0.25)],
            vec![("Roaming\\Slack\\Cache", 0.96), ("Roaming\\Slack\\Code Cache", 0.31), ("Local\\Slack\\ShipIt", 0.25)],
            false,
            vec![],
        );
        self.app(
            "Visual Studio Code",
            "Microsoft VS Code\\Code.exe",
            0.62,
            Some(1.0),
            vec![("Roaming\\Code", 1.8), ("Local\\Microsoft\\VSCode\\ShipIt", 0.21)],
            vec![("Roaming\\Code\\CachedData", 0.64), ("Roaming\\Code\\Cache", 0.18), ("Local\\Microsoft\\VSCode\\ShipIt", 0.21)],
            false,
            vec![t(
                "用包管理器装的：卸载后再运行一次它的卸载命令，包管理器才会忘掉它",
                "Installed with a package manager: run its uninstall command afterwards so the package manager forgets it",
            )],
        );
        self.app(
            "Blender",
            "Blender Foundation\\Blender\\blender.exe",
            1.9,
            Some(133.0),
            vec![("Roaming\\Blender Foundation\\Blender", 0.42), ("Local\\Blender Foundation\\Blender", 0.06)],
            vec![("Local\\Blender Foundation\\Blender", 0.06)],
            false,
            vec![],
        );
        self.app(
            "Zoom",
            "Zoom\\bin\\Zoom.exe",
            0.61,
            Some(214.0),
            vec![("Roaming\\Zoom", 0.88), ("Local\\Zoom\\logs", 0.12)],
            vec![("Local\\Zoom\\logs", 0.12)],
            false,
            vec![],
        );
        self.app(
            "Figma",
            "Figma\\Figma.exe",
            0.45,
            Some(6.0),
            vec![("Roaming\\Figma", 1.1)],
            vec![("Roaming\\Figma\\Cache", 0.52), ("Roaming\\Figma\\GPUCache", 0.03)],
            false,
            vec![],
        );
        self.app(
            "Unity Hub",
            "Unity Hub\\Unity Hub.exe",
            0.41,
            Some(301.0),
            vec![("Roaming\\UnityHub", 0.23), ("Local\\Unity\\logs", 0.02)],
            vec![("Local\\Unity\\logs", 0.02)],
            false,
            vec![],
        );
    }

    fn large_files(&mut self) {
        let h = self.home.clone();
        let mut make_file = |rel: &str, size: f64, reason: String, days: f64, mut warnings: Vec<String>| {
            let path = format!("{}\\{}", h, rel.replace('/', "\\"));
            if days < 7.0 {
                warnings.push(t(
                    &format!("{} 还有改动，所以没预选", fmt_stamp(self.now - (days * 86400.0) as i64)),
                    &format!("changed {}, so not preselected", fmt_stamp(self.now - (days * 86400.0) as i64)),
                ));
            }
            let name = path.rsplit('\\').next().unwrap_or(&path).to_string();
            let mut it = ReclaimItem::new(
                format!("large:{}", path),
                name,
                Some(path.clone()),
                gb(size),
                Tier::Yours,
                ReclaimAction::Trash { path },
                format!(
                    "{} · {}",
                    reason,
                    t(
                        &format!(
                            "修改于 {}",
                            fmt_ago(Some(self.now - (days * 86400.0) as i64))
                        ),
                        &format!(
                            "modified {}",
                            fmt_ago(Some(self.now - (days * 86400.0) as i64))
                        )
                    )
                ),
            )
            .with_restore(t("从废纸篓放回", "put back from the Trash"))
            .preselected(false);
            it.warnings = warnings;
            it
        };
        let opened = |days: f64| {
            t(
                &format!("上次打开 {}", fmt_ago(Some(self.now - (days * 86400.0) as i64))),
                &format!("last opened {}", fmt_ago(Some(self.now - (days * 86400.0) as i64))),
            )
        };
        let mut group = |id: &str, title: &str, items: Vec<ReclaimItem>| {
            let bytes: u64 = items.iter().map(|i| i.bytes).sum();
            let mut th = Thing::new(format!("large:{}", id), ThingKind::LargeFile, title, None, bytes);
            let n = items.len();
            th.detail = Some(t(&format!("{} 个", n), &format!("{} items", n)));
            let mut items = items;
            items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
            th.items = items;
            self.things.push(th);
        };
        group(
            "vm",
            t("虚拟机", "Virtual machines").as_str(),
            vec![make_file(
                "Documents\\Virtual Machines\\Windows 11.vhdx",
                38.4,
                opened(212.0),
                212.0,
                vec![t(
                    "虚拟机的整块磁盘：确认里面没有要留的东西",
                    "A whole virtual machine disk: make sure nothing in it is still needed",
                )],
            )],
        );
        group(
            "installer",
            t("安装包和磁盘映像", "Installers and disk images").as_str(),
            vec![
                make_file("Downloads\\Xcode_26.1.xip", 11.9, format!("{} · developer.apple.com", t("下载自 developer.apple.com", "downloaded from developer.apple.com")), 96.0, vec![]),
                make_file("Downloads\\ubuntu-24.04.3-live-server-arm64.iso", 2.9, format!("{} · {}", t("下载自 ubuntu.com", "downloaded from ubuntu.com"), opened(140.0)), 141.0, vec![]),
                make_file("Downloads\\Docker.dmg", 0.62, format!("{} · {}", t("已经装了 Docker", "Docker is already installed"), t("下载自 desktop.docker.com", "downloaded from desktop.docker.com")), 2.0, vec![]),
                make_file("Downloads\\Blender-4.5.3.dmg", 0.41, format!("{} · {}", t("已经装了 Blender", "Blender is already installed"), t("下载自 download.blender.org", "downloaded from download.blender.org")), 133.0, vec![]),
                make_file("Downloads\\Figma.dmg", 0.24, format!("{} · {}", t("已经装了 Figma", "Figma is already installed"), t("下载自 figma.com", "downloaded from figma.com")), 61.0, vec![]),
            ],
        );
        group(
            "video",
            t("视频", "Videos").as_str(),
            vec![
                make_file("Videos\\Screen Recording 2026-03-14.mov", 7.4, opened(238.0), 240.0, vec![]),
                make_file("Desktop\\conference-talk-raw.mp4", 4.8, opened(51.0), 52.0, vec![]),
                make_file("Downloads\\drone-footage-4k.mp4", 3.2, t("下载自 drive.google.com", "downloaded from drive.google.com"), 174.0, vec![]),
                make_file("Videos\\Captures\\gameplay-2026-07-02.mkv", 2.1, String::new(), 130.0, vec![]),
            ],
        );
        group(
            "archive",
            t("压缩包", "Archives").as_str(),
            vec![
                make_file("Documents\\Photos Backup 2022.zip", 9.1, opened(402.0), 410.0, vec![]),
                make_file("Downloads\\dataset-v2.tar.gz", 5.6, t("下载自 huggingface.co", "downloaded from huggingface.co"), 88.0, vec![]),
            ],
        );
        group(
            "model",
            t("模型权重", "Model weights").as_str(),
            vec![
                make_file("models\\sdxl-base-1.0.safetensors", 6.9, String::new(), 160.0, vec![]),
                make_file("Downloads\\whisper-large-v3.gguf", 3.1, t("下载自 huggingface.co", "downloaded from huggingface.co"), 120.0, vec![]),
            ],
        );
        group(
            "other",
            t("其他大文件", "Other large files").as_str(),
            vec![make_file("workspace\\analytics\\events-2025.parquet", 2.4, String::new(), 75.0, vec![])],
        );
    }

    fn browsers(&mut self) {
        let h = self.home.clone();
        let sup = format!("{}\\AppData\\Roaming\\", h);
        let mut profile = |title: &str, dir: &str, app: Option<String>, days: f64, cache: f64, history: i64, downloads: i64, cookies: i64, forms: i64, sessions: bool, firefox: bool, gone: bool| {
            let p = format!("{}{}", sup, dir.replace('/', "\\"));
            let sync = if firefox {
                t(
                    "只清这台电脑上的；开了同步的话，其他设备上的不受影响",
                    "only on this Mac; with sync on, your other devices keep theirs",
                )
            } else {
                t(
                    "只清这台电脑上的；开了同步的话，账号里的记录要在网上另外删",
                    "only on this Mac; with sync on, delete your account's copy online too",
                )
            };
            let mut items: Vec<ReclaimItem> = vec![];
            let erase = |kind: &str, label: String, n: i64, reason: String, warnings: Vec<String>, app: Option<String>| {
                let mut it = ReclaimItem::new(
                    format!("browser:{}:{}", kind, p),
                    label,
                    None,
                    0,
                    Tier::Erase,
                    ReclaimAction::Erase { db: format!("{}\\History", p), sql: vec![] },
                    reason,
                )
                .preselected(false)
                .with_records(n);
                it.warnings = warnings;
                it.app = app;
                it
            };
            if history > 0 {
                items.push(erase(
                    "history",
                    if firefox {
                        t("浏览和下载记录", "Browsing and download history")
                    } else {
                        t("浏览记录", "Browsing history")
                    },
                    history,
                    if firefox {
                        t(
                            &format!("{} 个网址的访问记录，包括下载列表；书签不动", crate::fmt::fmt_count(history)),
                            &format!("visits to {} addresses, the downloads list included; bookmarks stay", crate::fmt::fmt_count(history)),
                        )
                    } else {
                        t(
                            &format!("{} 个网址的访问记录，还有搜索词、常去的网站和地址栏建议", crate::fmt::fmt_count(history)),
                            &format!("visits to {} addresses, plus search terms, top sites and address-bar suggestions", crate::fmt::fmt_count(history)),
                        )
                    },
                    vec![sync.clone()],
                    app.clone(),
                ));
            }
            if downloads > 0 {
                items.push(erase(
                    "downloads",
                    t("下载记录", "Download history"),
                    downloads,
                    t(
                        &format!("下载列表里的 {} 条记录；下载下来的文件不动", crate::fmt::fmt_count(downloads)),
                        &format!("{} entries in the downloads list; the files themselves stay", crate::fmt::fmt_count(downloads)),
                    ),
                    vec![],
                    app.clone(),
                ));
            }
            if cookies > 0 {
                items.push(erase(
                    "cookies",
                    "Cookie".to_string(),
                    cookies,
                    t(
                        &format!("网站存在浏览器里的 {} 个 Cookie，用来记住登录和跟踪访问", crate::fmt::fmt_count(cookies)),
                        &format!("{} cookies websites keep to remember logins and follow visits", crate::fmt::fmt_count(cookies)),
                    ),
                    vec![t("清掉后所有网站都要重新登录", "signs you out of every website")],
                    app.clone(),
                ));
            }
            if forms > 0 {
                items.push(erase(
                    "forms",
                    t("表单自动填充记录", "Autofill form entries"),
                    forms,
                    t(
                        &format!("在网页表单里输过的 {} 条内容；保存的密码、地址和银行卡不动", crate::fmt::fmt_count(forms)),
                        &format!("{} things typed into web forms; saved passwords, addresses and cards stay", crate::fmt::fmt_count(forms)),
                    ),
                    vec![],
                    app.clone(),
                ));
            }
            if sessions {
                let mut it = ReclaimItem::new(
                    format!("browser:sessions:{}", p),
                    t("上次打开的标签页", "Last session's tabs"),
                    None,
                    2_400_000,
                    Tier::Erase,
                    ReclaimAction::DeleteAll { paths: vec![format!("{}\\Sessions", p)] },
                    t(
                        "浏览器用来恢复上次的窗口、标签页和最近关闭的标签页",
                        "what the browser uses to reopen your last windows, tabs and recently closed tabs",
                    ),
                )
                .preselected(false)
                .with_warning(t(
                    "下次打开浏览器不会再恢复上次的标签页",
                    "the browser won't reopen your last tabs",
                ));
                it.app = app.clone();
                items.push(it);
            }
            if cache > 0.0 {
                let mut it = ReclaimItem::new(
                    format!("browser:cache:{}", p),
                    t("缓存", "Cache"),
                    None,
                    gb(cache),
                    Tier::Regenerable,
                    ReclaimAction::DeleteAll { paths: vec![format!("{}\\Cache", p)] },
                    t(
                        "网页的图片、脚本和离线缓存；登录状态和设置都不受影响",
                        "images, scripts and offline caches of web pages; logins and settings stay",
                    ),
                )
                .with_restore(t("浏览网页时会重新下载", "downloaded again as you browse"))
                .preselected(false);
                it.app = app.clone();
                items.push(it);
            }
            let mut th = Thing::new(format!("browser:{}", p), ThingKind::Browser, title, Some(p), gb(cache) + gb(0.3));
            th.last_active = Some(self.now - (days * 86400.0) as i64);
            if gone {
                th.detail = Some(t("应用已经删掉，这些是它留下的", "the app is gone; this is what it left"));
            }
            th.items = items;
            self.things.push(th);
        };
        profile(
            "Google Chrome",
            "Google/Chrome/Default",
            Some("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe".into()),
            0.0,
            6.8,
            24_318,
            412,
            3_166,
            287,
            true,
            false,
            false,
        );
        profile(
            "Firefox",
            "Mozilla/Firefox/Profiles/x7k2p9qa.default-release",
            Some("C:\\Program Files\\Mozilla Firefox\\firefox.exe".into()),
            23.0,
            0.92,
            6_844,
            0,
            1_207,
            96,
            true,
            true,
            false,
        );
        profile(
            "Brave",
            "BraveSoftware/Brave-Browser/Default",
            None,
            290.0,
            0.18,
            1_204,
            37,
            388,
            0,
            false,
            false,
            true,
        );
        let safari = format!("{}\\AppData\\Roaming\\Safari", h);
        let mut th = Thing::new(format!("browser:{}", safari), ThingKind::Browser, "Safari", Some(safari.clone()), gb(0.64));
        th.last_active = Some(self.now - 4 * 86400);
        th.items = vec![
            ReclaimItem::new(
                format!("browser:cache:{}", safari),
                t("缓存", "Cache"),
                None,
                gb(0.31),
                Tier::Regenerable,
                ReclaimAction::DeleteAll { paths: vec![format!("{}\\SafariCache", h)] },
                t(
                    "网页的图片、脚本和离线缓存；登录状态和设置都不受影响",
                    "images, scripts and offline caches of web pages; logins and settings stay",
                ),
            )
            .with_restore(t("浏览网页时会重新下载", "downloaded again as you browse"))
            .preselected(false),
            ReclaimItem::new(
                format!("browser:history:{}", safari),
                t("浏览记录", "Browsing history"),
                None,
                0,
                Tier::Guide,
                ReclaimAction::None,
                t(
                    "在 Safari 里清：「历史记录」→「清除历史记录…」。开了 iCloud 里的 Safari 同步，你其他设备上的也会一起清掉",
                    "In Safari: History → Clear History…. With Safari in iCloud on, it clears your other devices too",
                ),
            )
            .preselected(false)
            .with_records(5_902),
            ReclaimItem::new(
                format!("browser:cookies:{}", safari),
                t("Cookie 和网站数据", "Cookies and website data"),
                None,
                0,
                Tier::Guide,
                ReclaimAction::None,
                t(
                    "在 Safari 里清：「设置」→「隐私」→「管理网站数据…」→「全部移除」",
                    "In Safari: Settings → Privacy → Manage Website Data… → Remove All",
                ),
            )
            .preselected(false),
        ];
        self.things.push(th);
    }

    fn app_data(&mut self) {
        let h = self.home.clone();
        let apps: Vec<(String, String, f64, String)> = vec![
            (format!("{}\\Documents\\WeChat Files", h), t("微信", "WeChat"), 48.2, t("在微信里打开 设置 → 通用 → 存储空间 清理聊天文件", "In WeChat: Settings → General → Storage").into()),
            (format!("{}\\AppData\\Roaming\\Apple Computer\\MobileSync\\Backup", h), t("iPhone 备份", "iPhone backups"), 31.7, t("在 访达 → 你的 iPhone → 管理备份 里删除旧备份", "Finder → your iPhone → Manage Backups").into()),
            (format!("{}\\AppData\\Roaming\\Claude\\vm_bundles", h), t("Claude 虚拟机镜像", "Claude VM bundles"), 9.8, t("Claude 桌面版运行代码用的虚拟机，删了会重新下载", "VM images used by the Claude app; re-downloaded if removed").into()),
            (format!("{}\\AppData\\Local\\Google\\Chrome\\User Data", h), "Chrome".into(), 6.4, t("在 Chrome 设置 → 隐私 → 清除浏览数据 里清缓存；不要直接删这个文件夹", "Use Chrome's Clear browsing data; don't delete the folder").into()),
            (format!("{}\\AppData\\Roaming\\LarkShell", h), t("飞书", "Lark"), 4.3, t("在飞书 设置 → 通用 → 清理缓存", "In Lark: Settings → General → Clear cache").into()),
        ];
        for (path, title, size, advice) in apps {
            let mut th = Thing::new(format!("app:{}", path), ThingKind::AppData, title.clone(), Some(path.clone()), gb(size));
            th.detail = Some(advice.clone());
            th.items = vec![ReclaimItem::new(
                format!("app:{}", path),
                title,
                Some(path),
                gb(size),
                Tier::Guide,
                ReclaimAction::None,
                advice,
            )
            .preselected(false)];
            self.things.push(th);
        }
    }

    fn system(&mut self, used: u64, hidden: u64, other_volumes: u64) {
        let trash = format!("{}\\$Recycle.Bin", self.home);
        let mut t1 = Thing::new("system:trash", ThingKind::System, t("废纸篓", "Trash"), Some(trash.clone()), gb(3.2));
        t1.detail = Some(t(
            "放进废纸篓的东西还占着空间，清空后才会腾出",
            "items in the Trash still use space until it is emptied",
        ));
        t1.items = vec![ReclaimItem::new(
            "system:trash",
            t("清空废纸篓", "Empty Trash"),
            Some(trash),
            gb(3.2),
            Tier::Yours,
            ReclaimAction::Command { argv: vec!["true".into()], cwd: None },
            t("这些是你之前删掉的文件", "files you deleted earlier"),
        )
        .preselected(false)];
        self.things.push(t1);

        let mut t2 = Thing::new("system:snapshots", ThingKind::System, t("还原点", "Restore points"), None, 0);
        t2.detail = Some(t(
            "2 个系统还原点。还原点占用的空间扫描不到；有还原点时，删掉文件也不会立刻腾出空间。",
            "2 system restore points. Their space can't be measured by scanning, and deleted files stay allocated while one holds them.",
        ));
        self.things.push(t2);

        let scanned = used.saturating_sub(hidden);
        let mut t3 = Thing::new("system:hidden", ThingKind::System, t("扫描不到的部分", "Not visible to the scan"), None, hidden);
        t3.detail = Some(t(
            &format!(
                "磁盘已用 {} − 扫描到 {}：还原点占用、没有权限读的目录（12 个）等。",
                fmt_bytes(used),
                fmt_bytes(scanned)
            ),
            &format!(
                "used {} − scanned {}: restore points, unreadable folders (12), etc.",
                fmt_bytes(used),
                fmt_bytes(scanned)
            ),
        ));
        self.things.push(t3);

        let mut t4 = Thing::new("system:volumes", ThingKind::System, t("系统和其他卷", "System and other volumes"), None, other_volumes);
        t4.detail = Some(t(
            "系统卷、交换文件、恢复分区等，由系统管理",
            "the system volume, swap, Recovery…, managed by the system",
        ));
        self.things.push(t4);
    }

    fn tree(&self) -> ScanTree {
        let h = self.home.clone();
        let mut folders: Vec<(String, u64)> = vec![
            (format!("{}\\Pictures\\Photos Library", h), gb(118.0)),
            (format!("{}\\Downloads", h), gb(57.0)),
            (format!("{}\\Documents", h), gb(41.0)),
            (format!("{}\\Videos", h), gb(24.0)),
            (format!("{}\\Desktop", h), gb(14.0)),
            (format!("{}\\Music", h), gb(8.0)),
            (format!("{}\\AppData\\Roaming", h), gb(38.0)),
            (format!("{}\\AppData\\Local", h), gb(9.0)),
            (format!("{}\\workspace", h), gb(6.0)),
        ];
        let mut folded: HashSet<String> = HashSet::new();
        folded.insert(format!("{}\\Pictures\\Photos Library", h));
        for (p, b) in &self.app_folders {
            folders.push((p.clone(), *b));
        }
        for th in &self.things {
            if th.kind == ThingKind::LargeFile {
                for i in &th.items {
                    if let Some(p) = &i.path {
                        folders.push((p.clone(), i.bytes));
                    }
                }
            }
        }
        for th in &self.things {
            if th.kind == ThingKind::App || th.kind == ThingKind::Browser {
                continue;
            }
            let inside: Vec<&ReclaimItem> = th
                .items
                .iter()
                .filter(|i| match (&i.path, &th.path) {
                    (Some(p), Some(tp)) => p.starts_with(&format!("{}\\", tp)),
                    _ => false,
                })
                .collect();
            for i in &inside {
                let p = i.path.clone().unwrap();
                folders.push((p.clone(), i.bytes));
                let last = p.rsplit('\\').next().unwrap_or("");
                if [".venv", "node_modules", ".next", ".build"].contains(&last) || i.id.starts_with("dd:") {
                    folded.insert(p);
                }
            }
            if let Some(tp) = &th.path {
                let inside_bytes: u64 = inside.iter().map(|i| i.bytes).sum();
                let rest = th.bytes.saturating_sub(inside_bytes);
                folders.push((tp.clone(), rest));
            }
        }
        ScanTree::synthetic(&h, &folders, &folded, self.ago(1.0))
    }

    fn history(&self, free: u64) -> (Vec<DayPoint>, Vec<ThingPoint>) {
        let kinds = [ThingKind::Project, ThingKind::Media, ThingKind::DevCache, ThingKind::Model, ThingKind::AppData];
        let mut shown: Vec<Thing> = self
            .things
            .iter()
            .filter(|t| kinds.contains(&t.kind))
            .cloned()
            .collect();
        shown.push(Thing::new(
            "cache:yarn",
            ThingKind::DevCache,
            "Yarn cache",
            Some(format!("{}\\AppData\\Local\\Yarn\\Cache", self.home)),
            gb(3.8),
        ));
        let mut out: Vec<DayPoint> = vec![];
        let mut rows: Vec<ThingPoint> = vec![];
        let days = 14;
        for d in 0..days {
            let back = days - 1 - d;
            let day = crate::fmt::today_key(self.now - (back * 86400) as i64);
            for (i, th) in shown.iter().enumerate() {
                let Some(s) = self.share(th, i, back) else { continue };
                rows.push(ThingPoint {
                    day: day.clone(),
                    thing_id: th.id.clone(),
                    kind: crate::history::kind_key(th.kind).to_string(),
                    title: th.title.clone(),
                    bytes: (th.bytes as f64 * s) as u64,
                });
            }
            for k in kinds {
                let total: u64 = rows
                    .iter()
                    .filter(|r| r.day == day && r.kind == crate::history::kind_key(k))
                    .map(|r| r.bytes)
                    .sum();
                if total > 0 {
                    out.push(DayPoint {
                        day: day.clone(),
                        kind: crate::history::kind_key(k).to_string(),
                        bytes: total,
                    });
                }
            }
            let f = d as f64 / (days - 1) as f64;
            let free_gb = free as f64 / 1e9 + 78.0 * (1.0 - f) + 4.0 * (d as f64).sin();
            out.push(DayPoint {
                day,
                kind: "free".into(),
                bytes: (free_gb * 1e9) as u64,
            });
        }
        (out, rows)
    }

    fn share(&self, th: &Thing, i: usize, back: i64) -> Option<f64> {
        let w = format!("project:{}\\workspace\\", self.home);
        match th.id.as_str() {
            "model:ollama" => Some(if back == 0 {
                1.0
            } else if back < 9 {
                0.875
            } else {
                0.76
            }),
            "model:hf" => Some(if back < 6 { 1.0 } else { 0.7 }),
            "cache:yarn" => {
                if back == 0 {
                    None
                } else {
                    Some(1.0)
                }
            }
            _ if th.id == format!("{}agent-playground", w) => {
                if back < 3 {
                    Some([1.0, 0.55, 0.3][back as usize])
                } else {
                    None
                }
            }
            _ if th.id == format!("{}storyboard-ai", w) => Some(if back < 2 { 1.0 } else { 0.62 }),
            _ if th.id == format!("{}comfy-workflows", w) => Some(if back < 9 { 1.0 } else { 0.8 }),
            _ if th.id == format!("media:{}\\Movies\\promo-video", self.home) => {
                if back <= 8 {
                    Some(1.0)
                } else {
                    None
                }
            }
            _ if th.id == format!("media:{}\\workspace\\explainer-ep3", self.home) => {
                if back <= 1 {
                    Some(1.0)
                } else {
                    None
                }
            }
            _ => {
                if th.kind == ThingKind::Project {
                    Some(1.0)
                } else {
                    Some(1.0 - [0.04, 0.18, 0.3, 0.1, 0.4, 0.22][i % 6] * back as f64 / 13.0)
                }
            }
        }
    }
}
