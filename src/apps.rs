use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::fmt::*;
use crate::model::*;
use crate::recognize::Ctx;
use crate::scan::*;

// ---------------------------------------------------------------------------
// Installed apps
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct InstalledApp {
    pub name: String,
    pub path: Option<String>,
}

#[cfg(windows)]
fn registry_apps() -> Vec<InstalledApp> {
    use winreg::enums::*;
    use winreg::RegKey;

    fn read(root: &RegKey, path: &str, wow: u32, out: &mut Vec<InstalledApp>) {
        let Ok(key) = root.open_subkey_with_flags(path, KEY_READ | wow) else {
            return;
        };
        for name in key.enum_keys().filter_map(|k| k.ok()) {
            let Ok(sub) = key.open_subkey_with_flags(&name, KEY_READ) else {
                continue;
            };
            let display: String = sub.get_value("DisplayName").unwrap_or_default();
            if display.trim().is_empty() {
                continue;
            }
            let system: u32 = sub.get_value("SystemComponent").unwrap_or(0);
            if system == 1 {
                continue;
            }
            let release: String = sub.get_value("ReleaseType").unwrap_or_default();
            if release.to_lowercase().contains("update") {
                continue;
            }
            let loc: String = sub.get_value("InstallLocation").unwrap_or_default();
            let icon: String = sub.get_value("DisplayIcon").unwrap_or_default();
            let mut path: Option<String> = None;
            if !loc.trim().is_empty() {
                for cand in loc.split(';') {
                    let c = cand.trim().trim_matches('"');
                    if !c.is_empty() && Path::new(c).exists() {
                        path = Some(c.to_string());
                        break;
                    }
                }
            }
            if path.is_none() && !icon.trim().is_empty() {
                let c = icon.trim().trim_matches('"');
                let c = match c.rfind(',') {
                    Some(p) if c[p + 1..].trim().chars().all(|x| x.is_ascii_digit()) => &c[..p],
                    _ => c,
                };
                let p = Path::new(c);
                if p.exists() {
                    if p.is_file() {
                        path = p.parent().map(|d| d.to_string_lossy().to_string());
                    } else {
                        path = Some(c.to_string());
                    }
                }
            }
            out.push(InstalledApp {
                name: display.trim().to_string(),
                path,
            });
        }
    }

    let mut out = vec![];
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    const UN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    read(&hklm, UN, KEY_WOW64_64KEY, &mut out);
    read(&hklm, UN, KEY_WOW64_32KEY, &mut out);
    read(&hkcu, UN, KEY_WOW64_64KEY, &mut out);
    read(&hkcu, UN, KEY_WOW64_32KEY, &mut out);
    let mut seen = HashSet::new();
    out.retain(|a| seen.insert(a.name.to_lowercase()));
    out
}

#[cfg(not(windows))]
fn registry_apps() -> Vec<InstalledApp> {
    let mut out = vec![];
    if let Ok(rd) = fs::read_dir("/Applications") {
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.extension().map(|x| x == "app").unwrap_or(false) {
                out.push(InstalledApp {
                    name: p
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    path: Some(p.to_string_lossy().to_string()),
                });
            }
        }
    }
    out
}

fn normalize_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

const GENERIC_DATA_DIRS: [&str; 18] = [
    "microsoft",
    "google",
    "mozilla",
    "apple",
    "nvidia",
    "intel",
    "commonfiles",
    "packages",
    "temp",
    "programs",
    "virtualstore",
    "crashdumps",
    "d3dscache",
    "connecteddevicesplatform",
    "comms",
    "publishers",
    "adobe",
    "oracle",
];

fn app_data_dirs(ctx: &Ctx, name: &str) -> Vec<String> {
    let norm = normalize_name(name);
    if norm.len() < 4 || norm.len() > 40 {
        return vec![];
    }
    let h = &ctx.home;
    let mut out = vec![];
    let mut seen = HashSet::new();
    for base in [
        format!("{}\\AppData\\Roaming", h),
        format!("{}\\AppData\\Local", h),
        format!("{}\\AppData\\LocalLow", h),
    ] {
        let Some(node) = ctx.tree.find(&base) else {
            continue;
        };
        for c in ctx.tree.children(node) {
            let n = &ctx.tree.nodes[*c];
            if !n.dir {
                continue;
            }
            let cn = normalize_name(&n.name);
            if cn.len() < 4 || GENERIC_DATA_DIRS.contains(&cn.as_str()) {
                continue;
            }
            let hit = cn == norm
                || cn.starts_with(&norm)
                || (norm.starts_with(&cn) && cn.len() >= 6);
            if hit && seen.insert(path_key(&n.path)) {
                out.push(n.path.clone());
            }
        }
    }
    if let Some(progs) = ctx.tree.find(&format!("{}\\AppData\\Local\\Programs", h)) {
        for c in ctx.tree.children(progs) {
            let n = &ctx.tree.nodes[*c];
            let cn = normalize_name(&n.name);
            if (cn == norm || cn.starts_with(&norm)) && seen.insert(path_key(&n.path)) {
                out.push(n.path.clone());
            }
        }
    }
    out
}

pub fn process_running(image: &str) -> bool {
    if image.is_empty() {
        return false;
    }
    Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {}", image), "/NH"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains(&image.to_lowercase())
        })
        .unwrap_or(false)
}

pub fn recognize_apps(ctx: &Ctx) -> Vec<Thing> {
    let mut things = vec![];
    let mut apps: Vec<(InstalledApp, u64)> = vec![];
    for a in registry_apps() {
        let Some(p) = &a.path else { continue };
        if ctx.off_limits(p) {
            continue;
        }
        let bytes = ctx.size(p);
        if bytes < 1_000_000 {
            continue;
        }
        apps.push((a, bytes));
    }
    apps.sort_by_key(|(_, b)| std::cmp::Reverse(*b));
    apps.truncate(200);
    for (a, app_bytes) in apps {
        let app_path = a.path.clone().unwrap_or_default();
        if app_path.is_empty() {
            continue;
        }
        let data = app_data_dirs(ctx, &a.name);
        let mut data_bytes = 0u64;
        let mut cache_dirs: Vec<String> = vec![];
        let mut cache_bytes = 0u64;
        for d in &data {
            data_bytes += ctx.size(d);
            if let Some(node) = ctx.tree.find(d) {
                for c in ctx.tree.children(node) {
                    let n = &ctx.tree.nodes[*c];
                    if !n.dir {
                        continue;
                    }
                    if n.name.to_lowercase().contains("cache") && n.size >= 1_000_000 {
                        cache_dirs.push(n.path.clone());
                        cache_bytes += n.size;
                    }
                }
            }
        }
        let total = app_bytes + data_bytes;
        let protected = app_path.to_lowercase().contains("\\program files");
        let image = Path::new(&app_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut action_parts = vec![ReclaimAction::Trash {
            path: app_path.clone(),
        }];
        for d in &data {
            action_parts.push(ReclaimAction::Trash { path: d.clone() });
        }
        let mut item = ReclaimItem::new(
            format!("app:{}", app_path),
            a.name.clone(),
            Some(app_path.clone()),
            total,
            Tier::Yours,
            ReclaimAction::Compound {
                actions: action_parts,
            },
            t(
                "卸载：把应用和它的用户数据移到废纸篓，以后还能放回",
                "Uninstall: moves the app and its user data to the Trash, recoverable later",
            ),
        )
        .with_restore(t("从废纸篓放回", "put back from the Trash"));
        if protected {
            item.warnings.push(t(
                "在 Program Files 里，删除需要管理员权限",
                "lives in Program Files: deleting needs administrator rights",
            ));
        }
        if process_running(&image) {
            item.warnings.push(t(
                "应用正在运行，先退出再删",
                "the app is running; quit it before deleting",
            ));
        }
        let mut th = Thing::new(
            format!("installed:{}", app_path),
            ThingKind::App,
            a.name.clone(),
            Some(app_path.clone()),
            total,
        );
        if let Some(node) = ctx.tree.find(&app_path) {
            if let Some(m) = ctx.tree.newest(node) {
                th.last_active = Some(m);
                if now_ts() - m >= 120 * 86400 {
                    th.detail = Some(t(
                        &format!("{} 没动过", fmt_ago(Some(m))),
                        &format!("untouched for {}", fmt_ago(Some(m))),
                    ));
                }
            }
        }
        let mut items = vec![item];
        if cache_bytes >= 10_000_000 {
            items.push(
                ReclaimItem::new(
                    format!("appcache:{}", app_path),
                    t("缓存", "Caches"),
                    Some(cache_dirs[0].clone()),
                    cache_bytes,
                    Tier::Regenerable,
                    ReclaimAction::DeleteAll { paths: cache_dirs },
                    t(
                        "应用的缓存文件夹，需要时会重建",
                        "the app's cache folders, rebuilt when needed",
                    ),
                )
                .preselected(false)
                .with_app(app_path.clone()),
            );
        }
        th.items = items;
        things.push(th);
    }
    things
}

// ---------------------------------------------------------------------------
// Browsers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BrowserKind {
    Chromium,
    Firefox,
}

struct BrowserProfile {
    browser: &'static str,
    title: String,
    dir: String,
    kind: BrowserKind,
    executable: String,
}

fn chromium_profile(
    vendor: &str,
    product: &str,
    exe: &str,
    title: &str,
    out: &mut Vec<BrowserProfile>,
) {
    let local = format!("{}\\AppData\\Local", home_string());
    let dir = format!("{}\\{}\\{}\\User Data\\Default", local, vendor, product);
    if !Path::new(&dir).exists() {
        return;
    }
    out.push(BrowserProfile {
        browser: "chromium",
        title: title.into(),
        dir,
        kind: BrowserKind::Chromium,
        executable: exe.to_string(),
    });
}

fn browser_profiles() -> Vec<BrowserProfile> {
    let mut out = vec![];
    let local = format!("{}\\AppData\\Local", home_string());
    let _ = local;
    let roaming = format!("{}\\AppData\\Roaming", home_string());
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let pf86 =
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| "C:\\Program Files (x86)".into());
    chromium_profile(
        "Google",
        "Chrome",
        &format!("{}\\Google\\Chrome\\Application\\chrome.exe", pf),
        "Chrome",
        &mut out,
    );
    chromium_profile(
        "Microsoft",
        "Edge",
        &format!("{}\\Microsoft\\Edge\\Application\\msedge.exe", pf86),
        "Edge",
        &mut out,
    );
    chromium_profile(
        "BraveSoftware",
        "Brave-Browser",
        &format!("{}\\BraveSoftware\\Brave-Browser\\Application\\brave.exe", pf),
        "Brave",
        &mut out,
    );
    chromium_profile(
        "Vivaldi",
        "Vivaldi",
        &format!("{}\\Vivaldi\\Application\\vivaldi.exe", pf),
        "Vivaldi",
        &mut out,
    );
    let profiles_dir = format!("{}\\Mozilla\\Firefox\\Profiles", roaming);
    if let Ok(rd) = fs::read_dir(&profiles_dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let dir = e.path().to_string_lossy().to_string();
            if !Path::new(&dir).join("places.sqlite").exists() {
                continue;
            }
            out.push(BrowserProfile {
                browser: "firefox",
                title: "Firefox".into(),
                dir,
                kind: BrowserKind::Firefox,
                executable: format!("{}\\Mozilla Firefox\\firefox.exe", pf),
            });
        }
    }
    out
}

fn sqlite_count(db: &str, sql: &str) -> Option<i64> {
    if !Path::new(db).exists() {
        return None;
    }
    let c = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    c.query_row(sql, [], |r| r.get::<_, i64>(0)).ok()
}

fn dir_size_sum(paths: &[String]) -> u64 {
    paths.iter().map(|p| crate::scan::measure_dir(p)).sum()
}

pub fn recognize_browsers(_ctx: &Ctx) -> Vec<Thing> {
    let mut things = vec![];
    for p in browser_profiles() {
        let title = p.title.clone();
        let exe_name = Path::new(&p.executable)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let running = process_running(&exe_name);
        let installed = Path::new(&p.executable).exists();
        let mut items: Vec<ReclaimItem> = vec![];
        let mut bytes = 0u64;
        match p.kind {
            BrowserKind::Chromium => {
                let history_db = format!("{}\\History", p.dir);
                let cookie_db = {
                    let a = format!("{}\\Network\\Cookies", p.dir);
                    if Path::new(&a).exists() {
                        a
                    } else {
                        format!("{}\\Cookies", p.dir)
                    }
                };
                let forms_db = format!("{}\\Web Data", p.dir);
                let cache_dirs: Vec<String> = [
                    format!("{}\\Cache", p.dir),
                    format!("{}\\Code Cache", p.dir),
                    format!("{}\\GPUCache", p.dir),
                    format!("{}\\Service Worker\\CacheStorage", p.dir),
                ]
                .into_iter()
                .filter(|d| Path::new(d).exists())
                .collect();
                let cache = dir_size_sum(&cache_dirs);
                let hist = sqlite_count(&history_db, "SELECT COUNT(*) FROM urls").unwrap_or(0);
                let downloads =
                    sqlite_count(&history_db, "SELECT COUNT(*) FROM downloads").unwrap_or(0);
                let cookies = sqlite_count(&cookie_db, "SELECT COUNT(*) FROM cookies").unwrap_or(0);
                let forms = sqlite_count(&forms_db, "SELECT COUNT(*) FROM autofill").unwrap_or(0);
                if hist > 0 {
                    items.push(
                        ReclaimItem::new(
                            format!("browser:{}:history", p.browser),
                            t("浏览记录", "Browsing history"),
                            Some(history_db.clone()),
                            0,
                            Tier::Erase,
                            ReclaimAction::Erase {
                                db: history_db.clone(),
                                sql: vec![
                                    "DELETE FROM urls".into(),
                                    "DELETE FROM visits".into(),
                                    "DELETE FROM downloads".into(),
                                    "DELETE FROM downloads_url_chains".into(),
                                    "DELETE FROM keyword_search_terms".into(),
                                ],
                            },
                            t(
                                &format!(
                                    "删除 {} 条浏览记录和 {} 条下载记录，书签保留",
                                    fmt_count(hist),
                                    fmt_count(downloads)
                                ),
                                &format!(
                                    "Deletes {} history rows and {} download rows; bookmarks stay",
                                    fmt_count(hist),
                                    fmt_count(downloads)
                                ),
                            ),
                        )
                        .with_records(hist + downloads)
                        .preselected(false),
                    );
                }
                if cookies > 0 {
                    items.push(
                        ReclaimItem::new(
                            format!("browser:{}:cookies", p.browser),
                            t("Cookie", "Cookies"),
                            Some(cookie_db.clone()),
                            0,
                            Tier::Erase,
                            ReclaimAction::Erase {
                                db: cookie_db.clone(),
                                sql: vec!["DELETE FROM cookies".into()],
                            },
                            t(
                                &format!("删除 {} 条 Cookie，所有网站都要重新登录", fmt_count(cookies)),
                                &format!(
                                    "Deletes {} cookies; you'll be signed out everywhere",
                                    fmt_count(cookies)
                                ),
                            ),
                        )
                        .with_records(cookies)
                        .preselected(false),
                    );
                }
                if forms > 0 {
                    items.push(
                        ReclaimItem::new(
                            format!("browser:{}:forms", p.browser),
                            t("自动填充记录", "Autofill entries"),
                            Some(forms_db.clone()),
                            0,
                            Tier::Erase,
                            ReclaimAction::Erase {
                                db: forms_db.clone(),
                                sql: vec!["DELETE FROM autofill".into()],
                            },
                            t(
                                &format!("删除 {} 条保存的表单内容", fmt_count(forms)),
                                &format!("Deletes {} saved form entries", fmt_count(forms)),
                            ),
                        )
                        .with_records(forms)
                        .preselected(false),
                    );
                }
                if cache > 0 {
                    bytes += cache;
                    items.push(
                        ReclaimItem::new(
                            format!("browser:{}:cache", p.browser),
                            t("缓存和 Service Worker", "Cache and service workers"),
                            Some(p.dir.clone()),
                            cache,
                            Tier::Regenerable,
                            ReclaimAction::DeleteAll {
                                paths: cache_dirs,
                            },
                            t(
                                "网页的离线缓存，会重新下载",
                                "offline web cache, downloaded again",
                            ),
                        )
                        .preselected(true),
                    );
                }
                let sessions = format!("{}\\Sessions", p.dir);
                if Path::new(&sessions).exists() {
                    items.push(
                        ReclaimItem::new(
                            format!("browser:{}:sessions", p.browser),
                            t("打开的标签页快照", "Open-tab snapshots"),
                            Some(sessions.clone()),
                            0,
                            Tier::Regenerable,
                            ReclaimAction::Delete { path: sessions },
                            t(
                                "恢复上次会话用的临时文件",
                                "temporary files used to restore the last session",
                            ),
                        )
                        .preselected(false),
                    );
                }
                if hist + cookies + forms + downloads > 0 || cache > 0 {
                    bytes += 300_000_000;
                }
                for it in items.iter_mut() {
                    it.app = Some(p.executable.clone());
                }
            }
            BrowserKind::Firefox => {
                let places = format!("{}\\places.sqlite", p.dir);
                let cookies_db = format!("{}\\cookies.sqlite", p.dir);
                let forms_db = format!("{}\\formhistory.sqlite", p.dir);
                let cache_dir = format!("{}\\cache2", p.dir);
                let cache = crate::scan::measure_dir(&cache_dir);
                let hist = sqlite_count(&places, "SELECT COUNT(*) FROM moz_places").unwrap_or(0);
                let cookies = sqlite_count(&cookies_db, "SELECT COUNT(*) FROM moz_cookies").unwrap_or(0);
                let forms =
                    sqlite_count(&forms_db, "SELECT COUNT(*) FROM moz_formhistory").unwrap_or(0);
                if hist > 0 {
                    items.push(
                        ReclaimItem::new(
                            "browser:firefox:history",
                            t("浏览记录", "Browsing history"),
                            Some(places.clone()),
                            0,
                            Tier::Erase,
                            ReclaimAction::Erase {
                                db: places.clone(),
                                sql: vec![
                                    "DELETE FROM moz_historyvisits".into(),
                                    "DELETE FROM moz_inputhistory".into(),
                                ],
                            },
                            t(
                                &format!("删除 {} 条浏览记录，书签保留", fmt_count(hist)),
                                &format!("Deletes {} history rows; bookmarks stay", fmt_count(hist)),
                            ),
                        )
                        .with_records(hist)
                        .preselected(false),
                    );
                }
                if cookies > 0 {
                    items.push(
                        ReclaimItem::new(
                            "browser:firefox:cookies",
                            t("Cookie", "Cookies"),
                            Some(cookies_db.clone()),
                            0,
                            Tier::Erase,
                            ReclaimAction::Erase {
                                db: cookies_db.clone(),
                                sql: vec!["DELETE FROM moz_cookies".into()],
                            },
                            t(
                                &format!("删除 {} 条 Cookie，所有网站都要重新登录", fmt_count(cookies)),
                                &format!(
                                    "Deletes {} cookies; you'll be signed out everywhere",
                                    fmt_count(cookies)
                                ),
                            ),
                        )
                        .with_records(cookies)
                        .preselected(false),
                    );
                }
                if forms > 0 {
                    items.push(
                        ReclaimItem::new(
                            "browser:firefox:forms",
                            t("自动填充记录", "Autofill entries"),
                            Some(forms_db.clone()),
                            0,
                            Tier::Erase,
                            ReclaimAction::Erase {
                                db: forms_db.clone(),
                                sql: vec!["DELETE FROM moz_formhistory".into()],
                            },
                            t(
                                &format!("删除 {} 条保存的表单内容", fmt_count(forms)),
                                &format!("Deletes {} saved form entries", fmt_count(forms)),
                            ),
                        )
                        .with_records(forms)
                        .preselected(false),
                    );
                }
                if cache > 0 {
                    bytes += cache;
                    items.push(
                        ReclaimItem::new(
                            "browser:firefox:cache",
                            t("缓存", "Cache"),
                            Some(cache_dir.clone()),
                            cache,
                            Tier::Regenerable,
                            ReclaimAction::Delete { path: cache_dir },
                            t(
                                "网页的离线缓存，会重新下载",
                                "offline web cache, downloaded again",
                            ),
                        )
                        .preselected(true),
                    );
                }
                for it in items.iter_mut() {
                    it.app = Some(p.executable.clone());
                }
            }
        }
        if items.is_empty() {
            continue;
        }
        if running {
            for it in items.iter_mut() {
                it.warnings.push(t(
                    &format!("{} 正在运行，先退出再清理", title),
                    &format!("{} is running; quit it first", title),
                ));
            }
        }
        items.sort_by_key(|i| (i.tier, std::cmp::Reverse(i.estimate)));
        let mut th = Thing::new(
            format!("browser:{}:{}", p.browser, p.title),
            ThingKind::Browser,
            p.title.clone(),
            Some(p.dir.clone()),
            bytes,
        );
        if !installed {
            th.detail = Some(t(
                &format!("{} 看起来已经卸载，数据还在", title),
                &format!("{} seems gone, but its data is still here", title),
            ));
        }
        th.items = items;
        things.push(th);
    }
    things
}
