//! Listening TCP ports and the processes that own them (the "port killer"), from
//! /proc/net/tcp{,6} and the sockets each process holds open.

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Listener {
    pub port: u16,
    pub pid: u32,
    /// "localhost", "all interfaces" or a specific address
    pub addr: String,
    pub name: String,
    /// another user's or the system's (its owner can't be seen): hidden by default
    pub system: bool,
}

/// Well-known development ports, so the list reads "8081 · Metro" instead of just a number.
pub fn label(port: u16) -> &'static str {
    match port {
        8081 => "Metro (React Native)",
        19000 | 19001 | 19002 | 19006 => "Expo",
        3000 => "React / Next / Node",
        3001 => "Dev server",
        5173 | 5174 => "Vite",
        4200 => "Angular",
        8080 => "Web server",
        8000 => "Django / HTTP",
        5000 => "Flask / .NET",
        4000 => "Dev server",
        9229 => "Node debugger",
        5037 => "ADB server",
        5432 => "PostgreSQL",
        3306 => "MySQL",
        1433 => "SQL Server",
        6379 => "Redis",
        27017 => "MongoDB",
        9000 => "PHP-FPM / Minio",
        8888 => "Jupyter",
        5555 => "ADB (wifi)",
        _ => "",
    }
}

/// (port, socket inode, address) of every listening socket in a /proc/net/tcp file.
fn table(file: &str, v6: bool) -> Vec<(u16, u64, String)> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        // sl local_address rem_address st tx:rx tr:when retrnsmt uid timeout inode
        if f.len() < 10 || f[3] != "0A" {
            continue;
        }
        let Some((ip, port)) = f[1].split_once(':') else { continue };
        let Ok(port) = u16::from_str_radix(port, 16) else { continue };
        let addr = if v6 {
            match ip {
                "00000000000000000000000000000000" => "all interfaces".into(),
                "00000000000000000000000001000000" => "localhost".into(),
                _ => "IPv6".into(),
            }
        } else {
            let ip = std::net::Ipv4Addr::from(u32::from_str_radix(ip, 16).unwrap_or(0).swap_bytes());
            if ip.is_unspecified() {
                "all interfaces".into()
            } else if ip.is_loopback() {
                "localhost".into()
            } else {
                ip.to_string()
            }
        };
        out.push((port, f[9].parse().unwrap_or(0), addr));
    }
    out
}

/// socket inode → pid, for every process whose open files we may look at (our own).
fn socket_owners() -> HashMap<u64, u32> {
    let mut map = HashMap::new();
    let Ok(procs) = std::fs::read_dir("/proc") else { return map };
    for p in procs.flatten() {
        let Ok(pid) = p.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Ok(fds) = std::fs::read_dir(p.path().join("fd")) else { continue };
        for fd in fds.flatten() {
            let Ok(link) = std::fs::read_link(fd.path()) else { continue };
            let link = link.to_string_lossy();
            if let Some(inode) = link.strip_prefix("socket:[").and_then(|s| s.strip_suffix(']')).and_then(|s| s.parse().ok()) {
                map.insert(inode, pid);
            }
        }
    }
    map
}

pub fn listeners() -> Vec<Listener> {
    let mut rows = table("/proc/net/tcp", false);
    rows.extend(table("/proc/net/tcp6", true));
    let owners = socket_owners();
    let me = std::process::id();
    let mut out: Vec<Listener> = Vec::new();
    for (port, inode, addr) in rows {
        let pid = owners.get(&inode).copied().unwrap_or(0);
        if pid == me {
            continue;
        }
        // the same app usually listens on both IPv4 and IPv6: show it once
        if let Some(l) = out.iter_mut().find(|l| l.port == port && l.pid == pid) {
            if l.addr != addr && addr == "all interfaces" {
                l.addr = addr;
            }
            continue;
        }
        let name = if pid == 0 {
            "System".to_string()
        } else {
            std::fs::read_to_string(format!("/proc/{pid}/comm")).map(|c| c.trim().to_string()).unwrap_or_else(|_| format!("PID {pid}"))
        };
        out.push(Listener { port, pid, addr, name, system: pid == 0 });
    }
    // dev ports first, then by number
    out.sort_by_key(|l| (l.system, label(l.port).is_empty(), l.port));
    out
}

/// Ends the process. Errors are short, user-facing strings.
pub fn kill(pid: u32) -> Result<(), String> {
    if pid == 0 {
        return Err("Needs root".into());
    }
    let ok = std::process::Command::new("kill").arg(pid.to_string()).status().is_ok_and(|s| s.success());
    if ok { Ok(()) } else { Err("Couldn't stop it".into()) }
}
