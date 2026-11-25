use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0); // 0 unknown, 1 zh, 2 en

pub fn set_language(zh: bool) {
    LANG.store(if zh { 1 } else { 2 }, Ordering::Relaxed);
}

pub fn detect_language() {
    if LANG.load(Ordering::Relaxed) != 0 {
        return;
    }
    if let Ok(v) = std::env::var("SMD_LANG") {
        set_language(v.to_lowercase().starts_with("zh"));
        return;
    }
    let zh = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .map(|v| v.to_lowercase().starts_with("zh"))
        .unwrap_or_else(|_| {
            // Windows: check the UI language through the registry locale name.
            #[cfg(windows)]
            {
                use std::process::Command;
                if let Ok(out) = Command::new("powershell")
                    .args(["-NoProfile", "-Command", "(Get-Culture).Name"])
                    .output()
                {
                    let s = String::from_utf8_lossy(&out.stdout).to_lowercase();
                    return s.starts_with("zh");
                }
            }
            false
        });
    set_language(zh);
}

pub fn is_zh() -> bool {
    match LANG.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            detect_language();
            LANG.load(Ordering::Relaxed) == 1
        }
    }
}

pub fn t(zh: &str, en: &str) -> String {
    if is_zh() {
        zh.to_string()
    } else {
        en.to_string()
    }
}

pub fn fmt_bytes(v: u64) -> String {
    let d = v as f64;
    if d >= 1e12 {
        format!("{:.2} TB", d / 1e12)
    } else if d >= 1e9 {
        if d >= 1e11 {
            format!("{:.0} GB", d / 1e9)
        } else {
            format!("{:.1} GB", d / 1e9)
        }
    } else if d >= 1e6 {
        format!("{:.0} MB", d / 1e6)
    } else if d >= 1e3 {
        format!("{:.0} KB", d / 1e3)
    } else {
        format!("{} B", v)
    }
}

/// Splits "22.9 GB" into ("22.9", "GB") for the big numbers.
pub fn split_bytes(v: u64) -> (String, String) {
    let s = fmt_bytes(v);
    match s.rsplit_once(' ') {
        Some((n, u)) => (n.to_string(), u.to_string()),
        None => (s, String::new()),
    }
}

pub fn fmt_count(n: i64) -> String {
    if is_zh() && n >= 10_000 {
        let s = format!("{:.1}", n as f64 / 1e4);
        let s = s.strip_suffix(".0").unwrap_or(&s).to_string();
        format!("{} 万", s)
    } else {
        let s = n.abs().to_string();
        let mut out = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        if n < 0 {
            format!("-{}", out)
        } else {
            out
        }
    }
}

pub fn now_ts() -> i64 {
    chrono::Local::now().timestamp()
}

pub fn fmt_ago(ts: Option<i64>) -> String {
    let Some(ts) = ts else {
        return t("未知", "unknown");
    };
    let days = (now_ts() - ts) / 86400;
    if days <= 0 {
        t("今天", "today")
    } else if days < 60 {
        if is_zh() {
            format!("{} 天前", days)
        } else {
            format!("{}d ago", days)
        }
    } else if days < 730 {
        if is_zh() {
            format!("{} 个月前", days / 30)
        } else {
            format!("{}mo ago", days / 30)
        }
    } else if is_zh() {
        format!("{} 年前", days / 365)
    } else {
        format!("{}y ago", days / 365)
    }
}

pub fn fmt_stamp(ts: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let d = Local.timestamp_opt(ts, 0).single().unwrap_or_else(Local::now);
    let now = Local::now();
    let year = d.year() != now.year();
    if is_zh() {
        if year {
            d.format("%Y-%m-%d %H:%M").to_string()
        } else {
            d.format("%m-%d %H:%M").to_string()
        }
    } else if year {
        d.format("%b %-d %Y, %H:%M").to_string()
    } else {
        d.format("%b %-d, %H:%M").to_string()
    }
}

pub fn fmt_relative(ts: i64) -> String {
    let secs = now_ts() - ts;
    if secs < 60 {
        t("刚刚", "just now")
    } else if secs < 3600 {
        if is_zh() {
            format!("{} 分钟前", secs / 60)
        } else {
            format!("{} min ago", secs / 60)
        }
    } else if secs < 86400 {
        if is_zh() {
            format!("{} 小时前", secs / 3600)
        } else {
            format!("{} h ago", secs / 3600)
        }
    } else {
        fmt_ago(Some(ts))
    }
}

pub fn home_dir() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))
}

pub fn home_string() -> String {
    home_dir().to_string_lossy().to_string()
}

pub fn fmt_home(path: &str) -> String {
    let h = home_string();
    if path == h {
        return "~".to_string();
    }
    let trimmed = h.trim_end_matches(['\\', '/']);
    if let Some(rest) = path.strip_prefix(trimmed) {
        if rest.starts_with('\\') || rest.starts_with('/') {
            return format!("~{}", &rest[1..]);
        }
    }
    path.to_string()
}

pub fn today_key(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    Local.timestamp_opt(ts, 0)
        .single()
        .unwrap_or_else(Local::now)
        .format("%Y-%m-%d")
        .to_string()
}

pub fn day_key_to_label(day: &str) -> String {
    day.to_string()
}

/// Joins a path fragment below home, using the platform separator.
pub fn home_join(rel: &str) -> String {
    let mut p = home_dir();
    for part in rel.split(['/', '\\']).filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p.to_string_lossy().to_string()
}
