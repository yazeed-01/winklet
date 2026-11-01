//! Which GPU this is and how much video memory it has (for the Game tab).

/// Name and dedicated memory (GB) of the main GPU: `nvidia-smi` for NVIDIA, the DRM
/// driver's sysfs files for the rest.
pub fn main_adapter() -> (String, f64) {
    let smi = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    if let Some((name, mib)) = smi.as_deref().and_then(|s| s.lines().next()).and_then(|l| l.rsplit_once(',')) {
        return (name.trim().to_string(), mib.trim().parse::<f64>().unwrap_or(0.0) / 1024.0);
    }
    for i in 0..4 {
        let d = format!("/sys/class/drm/card{i}/device");
        let Ok(vendor) = std::fs::read_to_string(format!("{d}/vendor")) else { continue };
        let vram = std::fs::read_to_string(format!("{d}/mem_info_vram_total")).ok().and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0);
        let name = std::fs::read_to_string(format!("{d}/product_name")).ok().map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
        let fallback = match vendor.trim() {
            "0x1002" => "AMD Radeon",
            "0x8086" => "Intel Graphics",
            "0x10de" => "NVIDIA",
            _ => continue,
        };
        return (name.unwrap_or_else(|| fallback.into()), vram / 1024f64.powi(3));
    }
    (String::new(), 0.0)
}
