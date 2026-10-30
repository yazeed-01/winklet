//! Core Audio: master volume, per-app volume (audio sessions), and the microphone.
//! All calls are made from the UI thread (STA) and only when the Audio tab needs them.

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, S_OK};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AudioSessionStateActive, AudioSessionStateExpired, IAudioCaptureClient, IAudioClient, IAudioSessionControl2,
    IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, eCapture, eConsole,
    eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::{GUID, Interface, PWSTR, Result};

#[derive(Clone, PartialEq)]
pub struct AppVolume {
    pub name: String,
    pub volume: f32,
    pub muted: bool,
}

pub struct Audio {
    devices: IMMDeviceEnumerator,
}

impl Audio {
    pub fn new() -> Option<Self> {
        let devices = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok()?;
        Some(Self { devices })
    }

    fn speakers(&self) -> Result<IMMDevice> {
        unsafe { self.devices.GetDefaultAudioEndpoint(eRender, eConsole) }
    }

    fn mic(&self) -> Result<IMMDevice> {
        unsafe { self.devices.GetDefaultAudioEndpoint(eCapture, eConsole) }
    }

    fn endpoint(dev: &IMMDevice) -> Result<IAudioEndpointVolume> {
        unsafe { dev.Activate(CLSCTX_ALL, None) }
    }

    // ---------------------------------------------------------------- master
    pub fn master(&self) -> (f32, bool) {
        let get = || -> Result<(f32, bool)> {
            let ep = Self::endpoint(&self.speakers()?)?;
            unsafe { Ok((ep.GetMasterVolumeLevelScalar()?, ep.GetMute()?.as_bool())) }
        };
        get().unwrap_or((0.0, false))
    }

    pub fn set_master(&self, v: f32) {
        if let Ok(ep) = self.speakers().and_then(|d| Self::endpoint(&d)) {
            unsafe {
                let _ = ep.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), &GUID::zeroed());
                if v > 0.0 {
                    let _ = ep.SetMute(false, &GUID::zeroed());
                }
            }
        }
    }

    pub fn toggle_master_mute(&self) {
        if let Ok(ep) = self.speakers().and_then(|d| Self::endpoint(&d)) {
            unsafe {
                if let Ok(m) = ep.GetMute() {
                    let _ = ep.SetMute(!m.as_bool(), &GUID::zeroed());
                }
            }
        }
    }

    // ---------------------------------------------------------------- per app
    fn sessions(&self) -> Result<Vec<(String, ISimpleAudioVolume)>> {
        let mgr: IAudioSessionManager2 = unsafe { self.speakers()?.Activate(CLSCTX_ALL, None)? };
        let list = unsafe { mgr.GetSessionEnumerator()? };
        let mut out = Vec::new();
        for i in 0..unsafe { list.GetCount()? } {
            let Ok(ctl) = (unsafe { list.GetSession(i) }) else { continue };
            unsafe {
                if ctl.GetState().map(|s| s == AudioSessionStateExpired).unwrap_or(true) {
                    continue;
                }
            }
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            let name = if unsafe { ctl2.IsSystemSoundsSession() } == S_OK {
                "System sounds".to_string()
            } else {
                match unsafe { ctl2.GetProcessId() } {
                    Ok(pid) if pid != 0 => process_name(pid).unwrap_or_else(|| format!("PID {pid}")),
                    _ => continue,
                }
            };
            if let Ok(vol) = ctl.cast::<ISimpleAudioVolume>() {
                out.push((name, vol));
            }
        }
        Ok(out)
    }

    /// One row per app (Chrome's many sessions collapse into one), sorted by name.
    pub fn apps(&self) -> Vec<AppVolume> {
        let mut apps: Vec<AppVolume> = Vec::new();
        for (name, vol) in self.sessions().unwrap_or_default() {
            if apps.iter().any(|a| a.name == name) {
                continue;
            }
            unsafe {
                apps.push(AppVolume {
                    name,
                    volume: vol.GetMasterVolume().unwrap_or(1.0),
                    muted: vol.GetMute().map(|m| m.as_bool()).unwrap_or(false),
                });
            }
        }
        apps.sort_by(|a, b| (a.name == "System sounds").cmp(&(b.name == "System sounds")).then(a.name.cmp(&b.name)));
        apps
    }

    pub fn set_app_volume(&self, name: &str, v: f32) {
        for (n, vol) in self.sessions().unwrap_or_default() {
            if n == name {
                unsafe {
                    let _ = vol.SetMasterVolume(v.clamp(0.0, 1.0), &GUID::zeroed());
                    if v > 0.0 {
                        let _ = vol.SetMute(false, &GUID::zeroed());
                    }
                }
            }
        }
    }

    pub fn toggle_app_mute(&self, name: &str) {
        let sessions = self.sessions().unwrap_or_default();
        let target = sessions
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| unsafe { !v.GetMute().map(|m| m.as_bool()).unwrap_or(false) });
        if let Some(mute) = target {
            for (n, vol) in &sessions {
                if n == name {
                    unsafe {
                        let _ = vol.SetMute(mute, &GUID::zeroed());
                    }
                }
            }
        }
    }

    // ---------------------------------------------------------------- microphone
    /// None = no microphone
    pub fn mic_muted(&self) -> Option<bool> {
        let ep = self.mic().and_then(|d| Self::endpoint(&d)).ok()?;
        unsafe { ep.GetMute().ok().map(|m| m.as_bool()) }
    }

    pub fn set_mic_muted(&self, mute: bool) {
        if let Ok(ep) = self.mic().and_then(|d| Self::endpoint(&d)) {
            unsafe {
                let _ = ep.SetMute(mute, &GUID::zeroed());
            }
        }
    }

    pub fn mic_name(&self) -> String {
        let get = || -> Result<String> {
            let store = unsafe { self.mic()?.OpenPropertyStore(STGM_READ)? };
            let v = unsafe { store.GetValue(&PKEY_Device_FriendlyName)? };
            Ok(v.to_string())
        };
        get().unwrap_or_else(|_| "No microphone".into())
    }

    /// True while another app is capturing from the mic (a call, a recording...).
    pub fn mic_in_use(&self) -> bool {
        let check = || -> Result<bool> {
            let mgr: IAudioSessionManager2 = unsafe { self.mic()?.Activate(CLSCTX_ALL, None)? };
            let list = unsafe { mgr.GetSessionEnumerator()? };
            let me = std::process::id();
            for i in 0..unsafe { list.GetCount()? } {
                let ctl = unsafe { list.GetSession(i)? };
                if unsafe { ctl.GetState()? } == AudioSessionStateActive {
                    let pid = ctl.cast::<IAudioSessionControl2>().and_then(|c| unsafe { c.GetProcessId() }).unwrap_or(0);
                    if pid != me {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        };
        check().unwrap_or(false)
    }

    pub fn start_mic_test(&self) -> Option<MicTest> {
        MicTest::start(&self.mic().ok()?).ok()
    }
}

/// Opens a capture stream and measures the input level from the actual samples
/// (the endpoint meter alone can read 0 on some drivers).
pub struct MicTest {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    meter: IAudioMeterInformation,
    channels: usize,
    bits: u16,
    /// true once any non-silent sample arrived
    pub heard_anything: bool,
}

impl MicTest {
    fn start(dev: &IMMDevice) -> Result<Self> {
        unsafe {
            let client: IAudioClient = dev.Activate(CLSCTX_ALL, None)?;
            let fmt = client.GetMixFormat()?;
            let (channels, bits) = ((*fmt).nChannels as usize, (*fmt).wBitsPerSample);
            let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, 2_000_000, 0, fmt, None);
            CoTaskMemFree(Some(fmt as *const _));
            init?;
            let capture: IAudioCaptureClient = client.GetService()?;
            let meter: IAudioMeterInformation = dev.Activate(CLSCTX_ALL, None)?;
            client.Start()?;
            Ok(Self { client, capture, meter, channels: channels.max(1), bits, heard_anything: false })
        }
    }

    /// Peak level 0..1 since the last call.
    pub fn level(&mut self) -> f32 {
        let (meter, samples) = self.levels();
        meter.max(samples)
    }

    /// (endpoint meter, peak of the captured samples). Drains the buffer so it never fills.
    pub fn levels(&mut self) -> (f32, f32) {
        let mut peak = 0f32;
        unsafe {
            while let Ok(n) = self.capture.GetNextPacketSize() {
                if n == 0 {
                    break;
                }
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                if self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).is_err() {
                    break;
                }
                const SILENT: u32 = 0x2; // AUDCLNT_BUFFERFLAGS_SILENT
                if flags & SILENT == 0 && !data.is_null() {
                    let count = frames as usize * self.channels;
                    match self.bits {
                        32 => {
                            for &v in std::slice::from_raw_parts(data as *const f32, count) {
                                peak = peak.max(v.abs());
                            }
                        }
                        16 => {
                            for &v in std::slice::from_raw_parts(data as *const i16, count) {
                                peak = peak.max((v as f32 / 32768.0).abs());
                            }
                        }
                        _ => {}
                    }
                }
                let _ = self.capture.ReleaseBuffer(frames);
            }
            let meter = self.meter.GetPeakValue().unwrap_or(0.0);
            if peak > 0.0005 || meter > 0.0005 {
                self.heard_anything = true;
            }
            (meter, peak.min(1.0))
        }
    }
}
impl Drop for MicTest {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let stem = std::path::Path::new(&path).file_stem()?.to_string_lossy().to_string();
        Some(pretty(&stem))
    }
}

fn pretty(stem: &str) -> String {
    match stem.to_lowercase().as_str() {
        "msedge" | "msedgewebview2" => "Microsoft Edge".into(),
        "chrome" => "Google Chrome".into(),
        "firefox" => "Firefox".into(),
        "spotify" => "Spotify".into(),
        "discord" => "Discord".into(),
        "code" => "VS Code".into(),
        "ms-teams" | "teams" => "Teams".into(),
        "vlc" => "VLC".into(),
        "zoom" => "Zoom".into(),
        "whatsapp" => "WhatsApp".into(),
        "telegram" => "Telegram".into(),
        _ => {
            let mut c = stem.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "uses this machine's microphone"]
    fn mic_levels() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let a = Audio::new().expect("audio");
        println!("mic: {} muted={:?}", a.mic_name(), a.mic_muted());
        let mut t = a.start_mic_test().expect("start");
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let (meter, samples) = t.levels();
            println!("meter={meter:.4} samples={samples:.4}");
        }
    }
}