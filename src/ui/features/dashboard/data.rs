//! Dashboard command + parser helpers.

use std::collections::HashMap;

use crate::config::DockerCmd;
use crate::ui::{DashboardSnapshot, DiskEntry, NicEntry, now_local};

pub(in crate::ui) fn dashboard_command(docker_cmd: &DockerCmd) -> String {
    if docker_cmd.is_empty() {
        return String::new();
    }
    // Single round-trip via SSH/Local runner to collect basic host metrics.
    // Keep dependencies minimal: rely on /proc and coreutils if present.
    const OS: &str = "__CONTAINR_DASH_OS__";
    const KERNEL: &str = "__CONTAINR_DASH_KERNEL__";
    const ARCH: &str = "__CONTAINR_DASH_ARCH__";
    const UPTIME: &str = "__CONTAINR_DASH_UPTIME__";
    const CORES: &str = "__CONTAINR_DASH_CORES__";
    const LOAD: &str = "__CONTAINR_DASH_LOAD__";
    const MEM: &str = "__CONTAINR_DASH_MEM__";
    const DISK: &str = "__CONTAINR_DASH_DISK__";
    const NICS: &str = "__CONTAINR_DASH_NICS__";
    const ENGINE: &str = "__CONTAINR_DASH_ENGINE__";
    const CONTAINERS: &str = "__CONTAINR_DASH_CONTAINERS__";

    let docker_fmt = "{{{{.Server.Version}}}}|{{{{.Server.Os}}}}|{{{{.Server.Arch}}}}|{{{{.Server.ApiVersion}}}}";
    let dc = docker_cmd.to_shell();
    format!(
        "uname_s=$(uname -s 2>/dev/null || echo unknown); \
         echo {OS}; \
         if [ -r /etc/os-release ]; then . /etc/os-release && echo \"$PRETTY_NAME\"; \
         elif [ \"$uname_s\" = Darwin ]; then sw_vers -productName 2>/dev/null | tr -d '\\n'; echo \" $(sw_vers -productVersion 2>/dev/null)\"; \
         else uname -s 2>/dev/null; fi; \
         echo {KERNEL}; uname -r 2>/dev/null || true; \
         echo {ARCH}; uname -m 2>/dev/null || true; \
         echo {UPTIME}; ( uptime -p 2>/dev/null || uptime 2>/dev/null || cat /proc/uptime 2>/dev/null || true ); \
         echo {CORES}; ( nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || grep -c '^processor' /proc/cpuinfo 2>/dev/null || echo 1 ); \
         echo {LOAD}; ( cat /proc/loadavg 2>/dev/null || sysctl -n vm.loadavg 2>/dev/null | tr -d '{{}}' || uptime 2>/dev/null || true ); \
         echo {MEM}; ( \
           if [ -r /proc/meminfo ]; then cat /proc/meminfo 2>/dev/null; \
           elif [ \"$uname_s\" = Darwin ]; then \
             total=$(sysctl -n hw.memsize 2>/dev/null || echo 0); \
             pagesize=$(sysctl -n hw.pagesize 2>/dev/null || echo 4096); \
             vm=$(vm_stat 2>/dev/null); \
             free=$(echo \"$vm\" | awk '/Pages free/ {{print $3}}' | tr -d '.'); \
             inactive=$(echo \"$vm\" | awk '/Pages inactive/ {{print $3}}' | tr -d '.'); \
             speculative=$(echo \"$vm\" | awk '/Pages speculative/ {{print $3}}' | tr -d '.'); \
             avail_pages=$((free+inactive+speculative)); \
             avail=$((avail_pages*pagesize)); \
             used=$((total-avail)); \
             echo \"MEM_TOTAL=$total MEM_AVAIL=$avail MEM_USED=$used\"; \
           fi ); \
         echo {DISK}; ( df -B1 -P -T 2>/dev/null || df -k -P 2>/dev/null || true ); \
         echo {NICS}; ( \
           if [ -d /sys/class/net ]; then \
             for i in /sys/class/net/*; do \
               iface=$(basename \"$i\"); \
               [ -e \"$i/device\" ] || continue; \
               case \"$iface\" in \
                 lo|br*|bond*|team*|vlan*|veth*|docker*|virbr*|cni*|flannel*|kube*|tap*|tun*) continue ;; \
               esac; \
               ip -o -4 addr show dev \"$iface\" 2>/dev/null | awk '{{print $2, $4}}'; \
             done; \
           elif [ \"$uname_s\" = Darwin ]; then \
             networksetup -listallhardwareports 2>/dev/null | awk '/Device:/ {{print $2}}' | while read -r dev; do \
               ip=$(ipconfig getifaddr \"$dev\" 2>/dev/null || true); \
               if [ -n \"$ip\" ]; then echo \"$dev $ip\"; fi; \
             done; \
           fi ); \
         echo {ENGINE}; ( {dc} version --format '{docker_fmt}' 2>/dev/null || {dc} --version 2>/dev/null || true ); \
         echo {CONTAINERS}; ( {dc} ps -q 2>/dev/null | wc -l | tr -d ' ' ); ( {dc} ps -a -q 2>/dev/null | wc -l | tr -d ' ' )",
        OS = OS,
        KERNEL = KERNEL,
        ARCH = ARCH,
        UPTIME = UPTIME,
        CORES = CORES,
        LOAD = LOAD,
        MEM = MEM,
        DISK = DISK,
        NICS = NICS,
        ENGINE = ENGINE,
        CONTAINERS = CONTAINERS,
        dc = dc,
        docker_fmt = docker_fmt,
    )
}

fn format_uptime_from_proc(raw: &str) -> Option<String> {
    // /proc/uptime: "<seconds> <idle_seconds>"
    let secs = raw.split_whitespace().next()?.parse::<f64>().ok()?;
    let mut secs = secs.max(0.0).round() as u64;
    let days = secs / 86_400;
    secs %= 86_400;
    let hours = secs / 3600;
    secs %= 3600;
    let minutes = secs / 60;
    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    parts.push(format!("{minutes}m"));
    Some(parts.join(" "))
}

fn normalize_uptime_line(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return "-".to_string();
    }

    // BSD/macOS uptime often looks like:
    // "14:03  up 5 days,  3:02, 3 users, load averages: 1.11 1.08 1.05"
    // Keep only the actual uptime segment.
    if !s.starts_with("up ") {
        if let Some((_, rest)) = s.split_once(" up ") {
            s = format!("up {}", rest.trim());
        }
    }

    if let Some((left, _)) = s.split_once(", load average") {
        s = left.trim().to_string();
    }
    if let Some((left, _)) = s.split_once(", load averages") {
        s = left.trim().to_string();
    }

    // Remove trailing user count (", 3 users" / ", 1 user").
    if let Some(idx) = s.rfind(",") {
        let tail = s[idx + 1..].trim();
        let mut it = tail.split_whitespace();
        if let (Some(n), Some(u)) = (it.next(), it.next()) {
            if n.chars().all(|c| c.is_ascii_digit()) && (u == "user" || u == "users") {
                s = s[..idx].trim().to_string();
            }
        }
    }

    // Normalize shorthand clock-style uptime to Linux-like wording:
    // "up 6:15" -> "up 6 hours, 15 minutes"
    // "up 5 days, 6:15" -> "up 5 days, 6 hours, 15 minutes"
    let mut out_parts: Vec<String> = Vec::new();
    let core = s.strip_prefix("up ").unwrap_or(&s).trim();
    for part in core.split(',').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        if let Some((h, m)) = part.split_once(':') {
            let h_ok = h.chars().all(|c| c.is_ascii_digit());
            let m_ok = m.chars().all(|c| c.is_ascii_digit());
            if h_ok && m_ok {
                let hours = h.parse::<u32>().unwrap_or(0);
                let mins = m.parse::<u32>().unwrap_or(0);
                if hours > 0 {
                    let unit = if hours == 1 { "hour" } else { "hours" };
                    out_parts.push(format!("{hours} {unit}"));
                }
                let unit = if mins == 1 { "minute" } else { "minutes" };
                out_parts.push(format!("{mins} {unit}"));
                continue;
            }
        }
        out_parts.push(part.to_string());
    }

    if out_parts.is_empty() {
        s
    } else {
        format!("up {}", out_parts.join(", "))
    }
}

pub(in crate::ui) fn parse_dashboard_output(out: &str) -> anyhow::Result<DashboardSnapshot> {
    const OS: &str = "__CONTAINR_DASH_OS__";
    const KERNEL: &str = "__CONTAINR_DASH_KERNEL__";
    const ARCH: &str = "__CONTAINR_DASH_ARCH__";
    const UPTIME: &str = "__CONTAINR_DASH_UPTIME__";
    const CORES: &str = "__CONTAINR_DASH_CORES__";
    const LOAD: &str = "__CONTAINR_DASH_LOAD__";
    const MEM: &str = "__CONTAINR_DASH_MEM__";
    const DISK: &str = "__CONTAINR_DASH_DISK__";
    const NICS: &str = "__CONTAINR_DASH_NICS__";
    const ENGINE: &str = "__CONTAINR_DASH_ENGINE__";
    const CONTAINERS: &str = "__CONTAINR_DASH_CONTAINERS__";

    let mut cur: Option<&'static str> = None;
    let mut sec: HashMap<&'static str, Vec<String>> = HashMap::new();
    for line in out.lines() {
        let t = line.trim_end_matches('\r');
        cur = match t.trim() {
            OS => Some(OS),
            KERNEL => Some(KERNEL),
            ARCH => Some(ARCH),
            UPTIME => Some(UPTIME),
            CORES => Some(CORES),
            LOAD => Some(LOAD),
            MEM => Some(MEM),
            DISK => Some(DISK),
            NICS => Some(NICS),
            ENGINE => Some(ENGINE),
            CONTAINERS => Some(CONTAINERS),
            _ => cur,
        };
        if matches!(
            t.trim(),
            OS | KERNEL | ARCH | UPTIME | CORES | LOAD | MEM | DISK | NICS | ENGINE | CONTAINERS
        ) {
            if let Some(k) = cur {
                sec.entry(k).or_default();
            }
            continue;
        }
        if let Some(k) = cur {
            sec.entry(k).or_default().push(t.to_string());
        }
    }

    let first = |k: &'static str| -> String {
        sec.get(k)
            .and_then(|xs| xs.iter().find(|s| !s.trim().is_empty()).cloned())
            .unwrap_or_else(|| "-".to_string())
    };

    let os = first(OS);
    let kernel = first(KERNEL);
    let arch = first(ARCH);

    let uptime_raw = first(UPTIME);
    let uptime = if uptime_raw.contains("up ") || uptime_raw.starts_with("up ") {
        normalize_uptime_line(&uptime_raw)
    } else if let Some(u) = format_uptime_from_proc(&uptime_raw) {
        u
    } else {
        normalize_uptime_line(&uptime_raw)
    };

    let cpu_cores = first(CORES).trim().parse::<u32>().unwrap_or(1).max(1);

    let load_raw = first(LOAD);
    let mut load1 = 0.0f32;
    let mut load5 = 0.0f32;
    let mut load15 = 0.0f32;
    if let Some(line) = sec
        .get(LOAD)
        .and_then(|xs| xs.iter().find(|s| !s.trim().is_empty()))
    {
        let cleaned = line.replace('{', "").replace('}', "");
        let toks: Vec<&str> = cleaned.split_whitespace().collect();
        if toks.len() >= 3 {
            load1 = toks[0].parse::<f32>().unwrap_or(0.0);
            load5 = toks[1].parse::<f32>().unwrap_or(0.0);
            load15 = toks[2].parse::<f32>().unwrap_or(0.0);
        }
    } else if let Some(line) = load_raw.split("load average:").nth(1) {
        let toks: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if toks.len() >= 3 {
            load1 = toks[0].parse::<f32>().unwrap_or(0.0);
            load5 = toks[1].parse::<f32>().unwrap_or(0.0);
            load15 = toks[2].parse::<f32>().unwrap_or(0.0);
        }
    }

    let mut mem_total_kb: Option<u64> = None;
    let mut mem_avail_kb: Option<u64> = None;
    let mut mem_total_bytes: Option<u64> = None;
    let mut mem_avail_bytes: Option<u64> = None;
    let mut mem_used_bytes: Option<u64> = None;
    if let Some(lines) = sec.get(MEM) {
        for l in lines {
            let l = l.trim();
            if l.contains("MEM_TOTAL=") {
                for part in l.split_whitespace() {
                    if let Some(rest) = part.strip_prefix("MEM_TOTAL=") {
                        mem_total_bytes = rest.parse::<u64>().ok();
                    }
                    if let Some(rest) = part.strip_prefix("MEM_AVAIL=") {
                        mem_avail_bytes = rest.parse::<u64>().ok();
                    }
                    if let Some(rest) = part.strip_prefix("MEM_USED=") {
                        mem_used_bytes = rest.parse::<u64>().ok();
                    }
                }
            }
            if let Some(rest) = l.strip_prefix("MemTotal:") {
                mem_total_kb = rest.split_whitespace().next().and_then(|x| x.parse().ok());
            }
            if let Some(rest) = l.strip_prefix("MemAvailable:") {
                mem_avail_kb = rest.split_whitespace().next().and_then(|x| x.parse().ok());
            }
            if (mem_total_kb.is_some() && mem_avail_kb.is_some())
                || (mem_total_bytes.is_some() && (mem_avail_bytes.is_some() || mem_used_bytes.is_some()))
            {
                break;
            }
        }
    }
    let mem_total_bytes = mem_total_bytes.unwrap_or_else(|| mem_total_kb.unwrap_or(0).saturating_mul(1024));
    let mem_avail_bytes = mem_avail_bytes.unwrap_or_else(|| mem_avail_kb.unwrap_or(0).saturating_mul(1024));
    let mem_used_bytes = mem_used_bytes.unwrap_or_else(|| mem_total_bytes.saturating_sub(mem_avail_bytes));

    let mut disk_entries: Vec<DiskEntry> = Vec::new();
    if let Some(lines) = sec.get(DISK) {
        for line in lines {
            let line = line.trim();
            if line.is_empty() || line.starts_with("Filesystem") {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 6 {
                continue;
            }
            let (source, fs_type, total_bytes, used_bytes, mount) = if parts.len() >= 7 {
                (
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].parse::<u64>().unwrap_or(0),
                    parts[3].parse::<u64>().unwrap_or(0),
                    parts[6].to_string(),
                )
            } else {
                // `df -k -P` (e.g. macOS fallback) reports 1K blocks, not bytes.
                (
                    parts[0].to_string(),
                    String::new(),
                    parts[1].parse::<u64>().unwrap_or(0).saturating_mul(1024),
                    parts[2].parse::<u64>().unwrap_or(0).saturating_mul(1024),
                    parts[5].to_string(),
                )
            };
            disk_entries.push(DiskEntry {
                source,
                fs_type,
                mount,
                used_bytes,
                total_bytes,
            });
        }
    }

    let disk_entries = collapse_disks(filter_disk_entries(disk_entries));
    let mut disk_used_bytes = 0u64;
    let mut disk_total_bytes = 0u64;
    let is_macos = os.to_ascii_lowercase().contains("mac");
    if is_macos {
        if let Some(data) = disk_entries
            .iter()
            .find(|d| d.mount == "/System/Volumes/Data")
        {
            disk_used_bytes = data.used_bytes;
            disk_total_bytes = data.total_bytes;
        } else if let Some(root) = disk_entries.iter().find(|d| d.mount == "/") {
            disk_used_bytes = root.used_bytes;
            disk_total_bytes = root.total_bytes;
        } else if let Some(first) = disk_entries.first() {
            disk_used_bytes = first.used_bytes;
            disk_total_bytes = first.total_bytes;
        }
    } else if let Some(root) = disk_entries.iter().find(|d| d.mount == "/") {
        disk_used_bytes = root.used_bytes;
        disk_total_bytes = root.total_bytes;
    } else if let Some(first) = disk_entries.first() {
        disk_used_bytes = first.used_bytes;
        disk_total_bytes = first.total_bytes;
    }

    let engine_raw = first(ENGINE);
    let engine = engine_raw.trim().to_string();

    let containers_raw = first(CONTAINERS);
    let mut containers_running = 0u32;
    let mut containers_total = 0u32;
    if let Some(lines) = sec.get(CONTAINERS) {
        let mut nums: Vec<u32> = Vec::new();
        for l in lines {
            let t = l.trim();
            if t.is_empty() {
                continue;
            }
            if let Ok(v) = t.parse::<u32>() {
                nums.push(v);
            } else if t.contains('/') {
                let parts: Vec<&str> = t.split('/').collect();
                if parts.len() >= 2 {
                    containers_running = parts[0].trim().parse::<u32>().unwrap_or(0);
                    containers_total = parts[1].trim().parse::<u32>().unwrap_or(0);
                    nums.clear();
                    break;
                }
            }
        }
        if nums.len() >= 2 {
            containers_running = nums[0];
            containers_total = nums[1];
        }
    } else if containers_raw.contains('/') {
        let parts: Vec<&str> = containers_raw.split('/').collect();
        if parts.len() >= 2 {
            containers_running = parts[0].trim().parse::<u32>().unwrap_or(0);
            containers_total = parts[1].trim().parse::<u32>().unwrap_or(0);
        }
    }

    let mut nics: Vec<NicEntry> = Vec::new();
    if let Some(lines) = sec.get(NICS) {
        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let Some(name) = parts.next() else {
                continue;
            };
            let Some(addr) = parts.next() else {
                continue;
            };
            let addr = addr.split('/').next().unwrap_or(addr).to_string();
            nics.push(NicEntry {
                name: name.to_string(),
                addr,
            });
        }
    }
    nics.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(DashboardSnapshot {
        os,
        kernel,
        arch,
        uptime,
        engine,
        containers_running,
        containers_total,
        cpu_cores,
        load1,
        load5,
        load15,
        mem_used_bytes,
        mem_total_bytes,
        disk_used_bytes,
        disk_total_bytes,
        disks: disk_entries,
        nics,
        collected_at: now_local(),
    })
}

fn filter_disk_entries(mut entries: Vec<DiskEntry>) -> Vec<DiskEntry> {
    if entries.is_empty() {
        return entries;
    }

    let excluded_types = [
        "tmpfs",
        "devtmpfs",
        "udev",
        "overlay",
        "proc",
        "sysfs",
        "cgroup",
        "cgroup2",
        "squashfs",
        "autofs",
        "fusectl",
        // Network filesystems/shares should not be treated as local host disks.
        "nfs",
        "nfs4",
        "cifs",
        "smbfs",
        "sshfs",
        "fuse.sshfs",
        "fuse.glusterfs",
        "ceph",
        "ceph-fuse",
        "9p",
    ];

    entries.retain(|e| {
        let ty = e.fs_type.to_ascii_lowercase();
        let mount = e.mount.as_str();
        if excluded_types.iter().any(|t| ty == *t) {
            return false;
        }
        if mount.starts_with("/run") || mount.starts_with("/dev") || mount.starts_with("/sys") {
            return false;
        }
        if mount.starts_with("/proc") || mount.starts_with("/snap") {
            return false;
        }
        if mount.starts_with("/var/lib/docker/overlay2") {
            return false;
        }
        // Ignore /boot mounts per user request.
        if mount.starts_with("/boot") {
            return false;
        }
        true
    });

    entries.sort_by(|a, b| {
        let rank = |m: &str| -> u8 {
            if m == "/" {
                0
            } else if m.starts_with("/var/lib/docker") {
                1
            } else if m.starts_with("/data") {
                2
            } else if m.starts_with("/mnt") {
                3
            } else if m.starts_with("/srv") {
                4
            } else {
                5
            }
        };
        let ra = rank(&a.mount);
        let rb = rank(&b.mount);
        ra.cmp(&rb).then_with(|| a.mount.cmp(&b.mount))
    });

    entries
}

fn collapse_disks(mut entries: Vec<DiskEntry>) -> Vec<DiskEntry> {
    let has_zfs = entries.iter().any(|e| e.fs_type == "zfs");
    let has_btrfs = entries.iter().any(|e| e.fs_type == "btrfs");
    if !has_zfs && !has_btrfs {
        let mut selected: Vec<DiskEntry> = Vec::new();
        for e in &entries {
            let m = e.mount.as_str();
            if m == "/"
                || m == "/System/Volumes/Data"
                || m == "/var/lib/docker"
                || m.starts_with("/mnt/")
                || m.starts_with("/data/")
                || m.starts_with("/srv/")
            {
                selected.push(e.clone());
            }
        }
        if selected.is_empty() {
            selected = entries;
        }
        selected.truncate(5);
        return selected;
    }
    if entries.is_empty() {
        return entries;
    }

    let mut out: Vec<DiskEntry> = Vec::new();
    let mut other: Vec<DiskEntry> = Vec::new();
    let mut zfs: Vec<DiskEntry> = Vec::new();
    let mut btrfs: Vec<DiskEntry> = Vec::new();

    for e in entries.drain(..) {
        if e.fs_type == "zfs" {
            zfs.push(e);
        } else if e.fs_type == "btrfs" {
            btrfs.push(e);
        } else {
            other.push(e);
        }
    }

    if let Some(root) = other.iter().find(|e| e.mount == "/") {
        out.push(root.clone());
    }

    if !zfs.is_empty() {
        let mut pools: HashMap<String, (u64, u64)> = HashMap::new();
        for e in &zfs {
            let pool = e.source.split('/').next().unwrap_or(&e.source).to_string();
            let entry = pools.entry(pool).or_insert((0, 0));
            entry.0 = entry.0.max(e.total_bytes);
            entry.1 = entry.1.saturating_add(e.used_bytes);
        }
        let mut pool_rows: Vec<DiskEntry> = pools
            .into_iter()
            .map(|(pool, (total, used))| DiskEntry {
                source: pool,
                fs_type: "zfs".to_string(),
                mount: String::new(),
                used_bytes: used,
                total_bytes: total,
            })
            .collect();
        pool_rows.sort_by_key(|e| std::cmp::Reverse(e.total_bytes));
        out.extend(pool_rows);
    }

    if !btrfs.is_empty() {
        let mut max_total = 0u64;
        let mut max_used = 0u64;
        for e in &btrfs {
            max_total = max_total.max(e.total_bytes);
            max_used = max_used.max(e.used_bytes);
        }
        out.push(DiskEntry {
            source: "btrfs".to_string(),
            fs_type: "btrfs".to_string(),
            mount: String::new(),
            used_bytes: max_used,
            total_bytes: max_total,
        });
    }

    for e in other {
        let m = e.mount.as_str();
        if m == "/"
            || m == "/var/lib/docker"
            || m.starts_with("/mnt/")
            || m.starts_with("/data/")
            || m.starts_with("/srv/")
        {
            if !out
                .iter()
                .any(|x| x.mount == e.mount && !x.mount.is_empty())
            {
                out.push(e);
            }
        }
    }

    out.truncate(5);
    out
}
