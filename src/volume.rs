use serde::{Deserialize, Serialize};
use sysinfo::{Disk, Disks};

use crate::fmt::home_string;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub name: String,
    pub path: String,
    pub uuid: String,
    pub total: u64,
    pub free: u64,
}

impl VolumeInfo {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }
}

pub fn volume_info() -> VolumeInfo {
    let disks = Disks::new_with_refreshed_list();
    let home = home_string().to_lowercase();
    let mut best: Option<&Disk> = None;
    for d in &disks {
        if home.starts_with(&d.mount_point().to_string_lossy().to_lowercase()) {
            if best
                .map(|b| d.mount_point().as_os_str().len() > b.mount_point().as_os_str().len())
                .unwrap_or(true)
            {
                best = Some(d);
            }
        }
    }
    let d = best.or_else(|| disks.iter().next());
    match d {
        Some(d) => VolumeInfo {
            name: d.name().to_string_lossy().to_string(),
            path: d.mount_point().to_string_lossy().to_string(),
            uuid: d.mount_point().to_string_lossy().to_string(),
            total: d.total_space(),
            free: d.available_space(),
        },
        None => VolumeInfo {
            name: "Disk".into(),
            path: "/".into(),
            uuid: "/".into(),
            total: 0,
            free: 0,
        },
    }
}

pub fn free_bytes(mount: &str) -> u64 {
    let disks = Disks::new_with_refreshed_list();
    let m = mount.to_lowercase();
    let mut best: Option<&Disk> = None;
    for d in &disks {
        let dp = d.mount_point().to_string_lossy().to_lowercase();
        if dp == m || dp.starts_with(&m) {
            if best
                .map(|b| dp.len() > b.mount_point().as_os_str().len())
                .unwrap_or(true)
            {
                best = Some(d);
            }
        }
    }
    best.map(|d| d.available_space()).unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct ExternalVolume {
    pub name: String,
    pub path: String,
    pub uuid: String,
    pub total: u64,
    pub free: u64,
}

pub fn external_volumes() -> Vec<ExternalVolume> {
    let disks = Disks::new_with_refreshed_list();
    let home = home_string().to_lowercase();
    let mut out = vec![];
    for d in &disks {
        let mp = d.mount_point().to_string_lossy().to_string();
        if home.starts_with(&mp.to_lowercase()) {
            continue;
        }
        if d.total_space() == 0 {
            continue;
        }
        out.push(ExternalVolume {
            name: d.name().to_string_lossy().to_string(),
            path: mp.clone(),
            uuid: mp,
            total: d.total_space(),
            free: d.available_space(),
        });
    }
    out
}
