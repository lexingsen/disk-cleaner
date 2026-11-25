use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::fmt::*;
use crate::model::*;

#[derive(Clone, Debug)]
pub enum ItemState {
    Pending,
    Running,
    Done,
    Failed(String),
    Skipped(String),
}

#[derive(Clone, Debug)]
pub struct ItemProgress {
    pub id: String,
    pub state: ItemState,
}

#[derive(Clone, Debug)]
pub struct CleanResult {
    pub reclaimed: u64,
    pub measured_freed: u64,
    pub failures: Vec<(String, String)>,
    pub cancelled: bool,
    pub states: Vec<ItemProgress>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Planning,
    Running,
    Done,
}

pub struct Cleaner<'a> {
    pub cancel: &'a AtomicBool,
    pub progress: Box<dyn FnMut(Phase, Option<&ItemProgress>) + 'a>,
}

fn dangerous_path(p: &str) -> Option<String> {
    let lower = p.to_lowercase().replace('/', "\\");
    let trimmed = lower.trim_end_matches('\\');
    let home = home_string().to_lowercase().replace('/', "\\");
    let home = home.trim_end_matches('\\').to_string();
    let home_parent = Path::new(&home)
        .parent()
        .map(|x| x.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if trimmed.is_empty() || trimmed.len() <= 2 {
        return Some(t("拒绝删除磁盘根目录", "refusing to delete a drive root"));
    }
    if trimmed == home || trimmed == home_parent {
        return Some(t("拒绝删除用户主目录", "refusing to delete the home directory"));
    }
    if trimmed == r"c:\windows" || trimmed.starts_with(r"c:\windows\system32") {
        return Some(t("拒绝删除 Windows 系统目录", "refusing to delete the Windows system folder"));
    }
    if trimmed == r"c:\program files" || trimmed == r"c:\program files (x86)" {
        return Some(t("拒绝删除整个 Program Files", "refusing to delete all of Program Files"));
    }
    if trimmed == r"c:\users" || trimmed == r"c:\programdata" {
        return Some(t("拒绝删除系统级共享目录", "refusing to delete a shared system folder"));
    }
    None
}

fn force_remove_dir(dir: &Path) -> std::io::Result<()> {
    fn clear_readonly(p: &Path) {
        if let Ok(m) = fs::symlink_metadata(p) {
            let mut perm = m.permissions();
            if perm.readonly() {
                perm.set_readonly(false);
                let _ = fs::set_permissions(p, perm);
            }
        }
    }
    fn walk(p: &Path) {
        clear_readonly(p);
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.filter_map(|e| e.ok()) {
                let cp = e.path();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    walk(&cp);
                } else {
                    clear_readonly(&cp);
                }
            }
        }
    }
    if let Err(first) = fs::remove_dir_all(dir) {
        if dir.exists() {
            walk(dir);
            fs::remove_dir_all(dir).map_err(|_| first)?;
        }
    }
    Ok(())
}

fn remove_path(p: &str) -> Result<(), String> {
    if let Some(why) = dangerous_path(p) {
        return Err(why);
    }
    let path = Path::new(p);
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(()); // already gone
    };
    if meta.file_type().is_symlink() {
        return fs::remove_file(path).map_err(|e| e.to_string());
    }
    if meta.is_dir() {
        force_remove_dir(path).map_err(|e| e.to_string())
    } else {
        if meta.permissions().readonly() {
            let mut perm = meta.permissions();
            perm.set_readonly(false);
            let _ = fs::set_permissions(path, perm);
        }
        fs::remove_file(path).map_err(|e| e.to_string())
    }
}

fn trash_path(p: &str) -> Result<(), String> {
    if let Some(why) = dangerous_path(p) {
        return Err(why);
    }
    if !Path::new(p).exists() {
        return Ok(());
    }
    trash::delete(p).map_err(|e| e.to_string())
}

fn erase_db(db: &str, statements: &[String]) -> Result<(), String> {
    if let Some(why) = dangerous_path(db) {
        return Err(why);
    }
    if !Path::new(db).exists() {
        return Ok(());
    }
    let conn = rusqlite::Connection::open(db).map_err(|e| {
        t(
            &format!("打不开数据库（浏览器可能还在运行）：{}", e),
            &format!("can't open the database (the browser may still be running): {}", e),
        )
    })?;
    let _ = conn.busy_timeout(Duration::from_secs(5));
    let mut any_ok = false;
    let mut last_err = None;
    for sql in statements {
        match conn.execute_batch(sql) {
            Ok(_) => any_ok = true,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("no such table") {
                    any_ok = true;
                } else {
                    last_err = Some(msg);
                }
            }
        }
    }
    if any_ok {
        let _ = conn.execute_batch("VACUUM");
        Ok(())
    } else {
        Err(last_err.unwrap_or_else(|| "erase failed".into()))
    }
}

fn run_command(argv: &[String], cwd: Option<&str>) -> Result<(), String> {
    if argv.is_empty() {
        return Err("empty command".into());
    }
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let out = cmd.output().map_err(|e| {
        t(
            &format!("找不到 {}：{}", argv[0], e),
            &format!("{} not found: {}", argv[0], e),
        )
    })?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let out_s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let msg = if !err.is_empty() { err } else { out_s };
        let last = msg.lines().last().unwrap_or("").to_string();
        Err(if last.is_empty() {
            t(
                &format!("{} 没有成功（退出码 {:?}）", argv[0], out.status.code()),
                &format!("{} failed (exit {:?})", argv[0], out.status.code()),
            )
        } else {
            last
        })
    }
}

fn run_action(action: &ReclaimAction) -> Result<(), String> {
    match action {
        ReclaimAction::Delete { path } => remove_path(path),
        ReclaimAction::DeleteAll { paths } => {
            let mut first_err = None;
            for p in paths {
                if let Err(e) = remove_path(p) {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
            match first_err {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
        ReclaimAction::Trash { path } => trash_path(path),
        ReclaimAction::Command { argv, cwd } => run_command(argv, cwd.as_deref()),
        ReclaimAction::Erase { db, sql } => erase_db(db, sql),
        ReclaimAction::Compound { actions } => {
            let mut errors = vec![];
            for a in actions {
                if let Err(e) = run_action(a) {
                    errors.push(e);
                }
            }
            if errors.is_empty() {
                Ok(())
            } else if errors.len() == 1 {
                Err(errors.pop().unwrap())
            } else {
                Err(errors.join("; "))
            }
        }
        ReclaimAction::None => Ok(()),
    }
}

pub fn execute(plan: &Plan, cleaner: &mut Cleaner) -> CleanResult {
    let mount = home_string();
    let before = crate::volume::free_bytes(&mount);
    let mut states: Vec<ItemProgress> = plan
        .items
        .iter()
        .map(|i| ItemProgress {
            id: i.id.clone(),
            state: ItemState::Pending,
        })
        .collect();
    let mut failures = vec![];
    let mut reclaimed = 0u64;
    let total = plan.items.len();
    for (n, item) in plan.items.iter().enumerate() {
        if cleaner.cancel.load(Ordering::Relaxed) {
            return CleanResult {
                reclaimed,
                measured_freed: crate::volume::free_bytes(&mount).saturating_sub(before),
                failures,
                cancelled: true,
                states,
            };
        }
        // Apps that still run (browsers) will lock their databases; skip early.
        if let Some(app) = &item.app {
            let image = Path::new(app)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if crate::apps::process_running(&image) {
                let msg = t(
                    &format!("{} 还在运行，先退出它", image),
                    &format!("{} is running; quit it first", image),
                );
                states[n].state = ItemState::Skipped(msg.clone());
                failures.push((item.label.clone(), msg));
                let upd = states[n].clone();
                (cleaner.progress)(Phase::Running, Some(&upd));
                continue;
            }
        }
        states[n].state = ItemState::Running;
        (cleaner.progress)(Phase::Running, Some(&states[n].clone()));
        let _ = total;
        match run_action(&item.action) {
            Ok(()) => {
                states[n].state = ItemState::Done;
                reclaimed += item.estimate;
                let upd = states[n].clone();
                (cleaner.progress)(Phase::Running, Some(&upd));
            }
            Err(e) => {
                states[n].state = ItemState::Failed(e.clone());
                failures.push((item.label.clone(), e));
                let upd = states[n].clone();
                (cleaner.progress)(Phase::Running, Some(&upd));
            }
        }
    }
    (cleaner.progress)(Phase::Done, None);
    let after = crate::volume::free_bytes(&mount);
    CleanResult {
        reclaimed,
        measured_freed: after.saturating_sub(before),
        failures,
        cancelled: false,
        states,
    }
}
