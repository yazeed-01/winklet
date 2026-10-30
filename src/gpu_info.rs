//! Which GPU this is and how much video memory it has (for the Game tab).

use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

/// Name and dedicated memory (GB) of the GPU with the most video memory.
pub fn main_adapter() -> (String, f64) {
    const SOFTWARE: u32 = 2;
    let mut best = (String::new(), 0.0);
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else { return best };
        let mut i = 0;
        while let Ok(adapter) = factory.EnumAdapters1(i) {
            i += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            let gb = desc.DedicatedVideoMemory as f64 / 1024f64.powi(3);
            if desc.Flags & SOFTWARE == 0 && gb > best.1 {
                let n = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
                best = (String::from_utf16_lossy(&desc.Description[..n]).trim().to_string(), gb);
            }
        }
    }
    best
}
