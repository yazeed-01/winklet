//! Listening TCP ports and the processes that own them (the "port killer").

use std::collections::HashMap;
use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, QueryFullProcessImageNameW,
    TerminateProcess,
};
use windows::core::PWSTR;

#[derive(Clone, Debug)]
pub struct Listener {
    pub port: u16,
    pub pid: u32,
    /// "localhost", "all interfaces" or a specific address
    pub addr: String,
    pub name: String,
    /// Windows services (session 0): hidden by default, never what you're looking for
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

pub fn listeners() -> Vec<Listener> {
    let mut rows: Vec<(u16, u32, String)> = Vec::new();
    unsafe {
        if let Some(buf) = table(AF_INET.0 as u32) {
            let n = u32::from_ne_bytes(buf[..4].try_into().unwrap()) as usize;
            let first = buf.as_ptr().add(4) as *const MIB_TCPROW_OWNER_PID;
            for i in 0..n {
                let r = &*first.add(i);
                let port = u16::from_be(r.dwLocalPort as u16);
                let ip = std::net::Ipv4Addr::from(u32::from_be(r.dwLocalAddr));
                rows.push((port, r.dwOwningPid, describe_v4(ip)));
            }
        }
        if let Some(buf) = table(AF_INET6.0 as u32) {
            let n = u32::from_ne_bytes(buf[..4].try_into().unwrap()) as usize;
            let first = buf.as_ptr().add(4) as *const MIB_TCP6ROW_OWNER_PID;
            for i in 0..n {
                let r = &*first.add(i);
                let port = u16::from_be(r.dwLocalPort as u16);
                let ip = std::net::Ipv6Addr::from(r.ucLocalAddr);
                rows.push((port, r.dwOwningPid, describe_v6(ip)));
            }
        }
    }

    let me = std::process::id();
    let mut names: HashMap<u32, Option<String>> = HashMap::new();
    let mut out: Vec<Listener> = Vec::new();
    for (port, pid, addr) in rows {
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
        let path = names.entry(pid).or_insert_with(|| process_path(pid)).clone();
        // services run in session 0; anything you started (terminals, node, python, IDEs) is in your session
        let system = pid <= 4 || session_of(pid).is_none_or(|s| s == 0);
        let name = path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| if pid == 4 { "System".into() } else { format!("PID {pid}") });
        out.push(Listener { port, pid, addr, name, system });
    }
    // dev ports first, then by number
    out.sort_by_key(|l| (l.system, label(l.port).is_empty(), l.port));
    out
}

/// Ends the process. Errors are short, user-facing strings.
pub fn kill(pid: u32) -> Result<(), String> {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, false, pid).map_err(|_| "Needs admin rights".to_string())?;
        let r = TerminateProcess(h, 1);
        let _ = CloseHandle(h);
        r.map_err(|_| "Windows refused to stop it".to_string())
    }
}

unsafe fn table(family: u32) -> Option<Vec<u8>> {
    let mut size = 0u32;
    let first = unsafe { GetExtendedTcpTable(None, &mut size, false, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
    if first != ERROR_INSUFFICIENT_BUFFER.0 && first != NO_ERROR.0 {
        return None;
    }
    // the table can grow between the two calls; retry a few times
    for _ in 0..3 {
        let mut buf = vec![0u8; size as usize + 1024];
        size = buf.len() as u32;
        let r = unsafe {
            GetExtendedTcpTable(Some(buf.as_mut_ptr() as *mut _), &mut size, false, family, TCP_TABLE_OWNER_PID_LISTENER, 0)
        };
        if r == NO_ERROR.0 {
            return Some(buf);
        }
        if r != ERROR_INSUFFICIENT_BUFFER.0 {
            return None;
        }
    }
    None
}

fn describe_v4(ip: std::net::Ipv4Addr) -> String {
    if ip.is_unspecified() {
        "all interfaces".into()
    } else if ip.is_loopback() {
        "localhost".into()
    } else {
        ip.to_string()
    }
}

fn describe_v6(ip: std::net::Ipv6Addr) -> String {
    if ip.is_unspecified() {
        "all interfaces".into()
    } else if ip.is_loopback() {
        "localhost".into()
    } else {
        ip.to_string()
    }
}

fn session_of(pid: u32) -> Option<u32> {
    let mut session = 0u32;
    unsafe { windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(pid, &mut session).ok().map(|_| session) }
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads this machine's ports"]
    fn list_ports() {
        for l in listeners() {
            println!("{:>5} {:<22} {:<28} pid {:<6} {} {}", l.port, label(l.port), l.name, l.pid, l.addr, if l.system { "(system)" } else { "" });
        }
    }

    #[test]
    #[ignore = "kills the process listening on port 8081"]
    fn kill_8081() {
        let l = listeners().into_iter().find(|l| l.port == 8081).expect("something on 8081");
        kill(l.pid).expect("kill");
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(listeners().iter().all(|l| l.port != 8081), "port still in use");
        println!("killed {} (pid {}), 8081 is free", l.name, l.pid);
    }

    #[test]
    fn labels_metro() {
        assert_eq!(label(8081), "Metro (React Native)");
        assert_eq!(label(1), "");
    }
}
