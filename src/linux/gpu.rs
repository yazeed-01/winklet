//! GPU load and video memory in use: AMD through the amdgpu driver's sysfs files, NVIDIA
//! through `nvidia-smi`. (0, 0) when unavailable (Intel, or no driver tools).

use std::path::PathBuf;

/// The first DRM card that reports its load (amdgpu).
fn amd_card() -> Option<PathBuf> {
    (0..4).map(|i| PathBuf::from(format!("/sys/class/drm/card{i}/device"))).find(|d| d.join("gpu_busy_percent").exists())
}

fn num(p: PathBuf) -> Option<f64> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

/// (GPU load 0..1, dedicated video memory in use in GB). (0, 0) when unavailable.
pub fn usage() -> (f32, f64) {
    if let Some(d) = amd_card() {
        let load = num(d.join("gpu_busy_percent")).unwrap_or(0.0) / 100.0;
        let vram = num(d.join("mem_info_vram_used")).unwrap_or(0.0) / 1024f64.powi(3);
        return (load.clamp(0.0, 1.0) as f32, vram);
    }
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=utilization.gpu,memory.used", "--format=csv,noheader,nounits"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let f: Vec<f64> = out.lines().next().unwrap_or("").split(',').filter_map(|v| v.trim().parse().ok()).collect();
    match f[..] {
        [load, mib, ..] => ((load / 100.0).clamp(0.0, 1.0) as f32, mib / 1024.0),
        _ => (0.0, 0.0),
    }
}
