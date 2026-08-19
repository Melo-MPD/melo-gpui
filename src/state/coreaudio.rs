//! Minimal CoreAudio queries (no bindgen): output devices with name,
//! transport, *current* nominal sample rate and hog owner. Cheap enough to
//! poll while playing, which is what makes the Signal Path verdict honest.

#[derive(Clone, Debug, PartialEq)]
pub struct CaDevice {
    pub id: u32,
    pub name: String,
    /// "USB", "Bluetooth", "Built-in", "DisplayPort", "HDMI", "AirPlay", "Thunderbolt", "Virtual", "Aggregate", ""
    pub transport: String,
    pub sample_rate: Option<u32>,
    /// Highest sample rate any physical format of the first output stream supports.
    pub max_sample_rate: Option<u32>,
    /// pid holding the device in hog (exclusive) mode; None when free.
    pub hog_pid: Option<i32>,
    pub is_default: bool,
}

impl CaDevice {
    pub fn is_lossy_transport(&self) -> bool {
        matches!(self.transport.as_str(), "Bluetooth" | "AirPlay")
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use super::CaDevice;
    use std::ffi::c_void;

    #[repr(C)]
    struct PropAddr {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyDataSize(
            id: u32,
            addr: *const PropAddr,
            qual_size: u32,
            qual: *const c_void,
            out_size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            id: u32,
            addr: *const PropAddr,
            qual_size: u32,
            qual: *const c_void,
            io_size: *mut u32,
            out: *mut c_void,
        ) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringGetCString(s: *const c_void, buf: *mut u8, len: isize, enc: u32) -> u8;
        fn CFRelease(cf: *const c_void);
    }

    const fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }
    const SYSTEM_OBJECT: u32 = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const SCOPE_OUTPUT: u32 = fourcc(b"outp");
    const ELEMENT_MAIN: u32 = 0;
    const PROP_DEVICES: u32 = fourcc(b"dev#");
    const PROP_DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
    const PROP_NAME: u32 = fourcc(b"lnam");
    const PROP_NOMINAL_RATE: u32 = fourcc(b"nsrt");
    const PROP_TRANSPORT: u32 = fourcc(b"tran");
    const PROP_HOG: u32 = fourcc(b"oink");
    const PROP_STREAMS: u32 = fourcc(b"stm#");
    const PROP_AVAILABLE_PHYSICAL_FORMATS: u32 = fourcc(b"pfta");
    const CF_UTF8: u32 = 0x0800_0100;

    fn addr(selector: u32, scope: u32) -> PropAddr {
        PropAddr {
            selector,
            scope,
            element: ELEMENT_MAIN,
        }
    }

    unsafe fn get<T: Copy>(id: u32, a: &PropAddr) -> Option<T> {
        let mut size = std::mem::size_of::<T>() as u32;
        let mut out = std::mem::MaybeUninit::<T>::uninit();
        let st = unsafe {
            AudioObjectGetPropertyData(
                id,
                a,
                0,
                std::ptr::null(),
                &mut size,
                out.as_mut_ptr() as *mut c_void,
            )
        };
        if st == 0 && size as usize >= std::mem::size_of::<T>() {
            Some(unsafe { out.assume_init() })
        } else {
            None
        }
    }

    unsafe fn data_size(id: u32, a: &PropAddr) -> Option<u32> {
        let mut size = 0u32;
        let st = unsafe { AudioObjectGetPropertyDataSize(id, a, 0, std::ptr::null(), &mut size) };
        (st == 0).then_some(size)
    }

    unsafe fn cf_string(id: u32, a: &PropAddr) -> Option<String> {
        let s: *const c_void = unsafe { get(id, a)? };
        if s.is_null() {
            return None;
        }
        let mut buf = vec![0u8; 512];
        let ok = unsafe { CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, CF_UTF8) };
        unsafe { CFRelease(s) };
        if ok == 0 {
            return None;
        }
        let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
        String::from_utf8(buf[..end].to_vec()).ok()
    }

    /// AudioStreamRangedDescription: AudioStreamBasicDescription (40 bytes) + AudioValueRange (16 bytes).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct RangedDesc {
        sample_rate: f64,
        format_id: u32,
        format_flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels_per_frame: u32,
        bits_per_channel: u32,
        reserved: u32,
        rate_min: f64,
        rate_max: f64,
    }

    unsafe fn max_physical_rate(device: u32) -> Option<u32> {
        let streams_addr = addr(PROP_STREAMS, SCOPE_OUTPUT);
        let size = unsafe { data_size(device, &streams_addr)? };
        let count = (size / 4) as usize;
        if count == 0 {
            return None;
        }
        let mut ids = vec![0u32; count];
        let mut io = size;
        let st = unsafe {
            AudioObjectGetPropertyData(
                device,
                &streams_addr,
                0,
                std::ptr::null(),
                &mut io,
                ids.as_mut_ptr() as *mut c_void,
            )
        };
        if st != 0 {
            return None;
        }
        let stream = ids[0];
        let fmt_addr = addr(PROP_AVAILABLE_PHYSICAL_FORMATS, SCOPE_GLOBAL);
        let size = unsafe { data_size(stream, &fmt_addr)? };
        let n = size as usize / std::mem::size_of::<RangedDesc>();
        if n == 0 {
            return None;
        }
        let mut buf: Vec<RangedDesc> = vec![unsafe { std::mem::zeroed() }; n];
        let mut io = size;
        let st = unsafe {
            AudioObjectGetPropertyData(
                stream,
                &fmt_addr,
                0,
                std::ptr::null(),
                &mut io,
                buf.as_mut_ptr() as *mut c_void,
            )
        };
        if st != 0 {
            return None;
        }
        buf.iter()
            .map(|d| d.rate_max.max(d.sample_rate))
            .filter(|r| r.is_finite() && *r > 0.0)
            .fold(None, |acc: Option<f64>, r| {
                Some(acc.map_or(r, |a| a.max(r)))
            })
            .map(|r| r.round() as u32)
    }

    fn transport_label(t: u32) -> &'static str {
        match &t.to_be_bytes() {
            b"usb " => "USB",
            b"blue" | b"blea" => "Bluetooth",
            b"bltn" => "Built-in",
            b"dprt" => "DisplayPort",
            b"hdmi" => "HDMI",
            b"airp" => "AirPlay",
            b"thun" => "Thunderbolt",
            b"virt" => "Virtual",
            b"grup" => "Aggregate",
            b"pci " => "PCI",
            b"1394" => "FireWire",
            b"eavb" => "AVB",
            _ => "",
        }
    }

    pub fn output_devices() -> Vec<CaDevice> {
        let mut out = Vec::new();
        unsafe {
            let devs_addr = addr(PROP_DEVICES, SCOPE_GLOBAL);
            let Some(size) = data_size(SYSTEM_OBJECT, &devs_addr) else {
                return out;
            };
            let count = (size / 4) as usize;
            let mut ids = vec![0u32; count];
            let mut io_size = size;
            let st = AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &devs_addr,
                0,
                std::ptr::null(),
                &mut io_size,
                ids.as_mut_ptr() as *mut c_void,
            );
            if st != 0 {
                return out;
            }
            let default_id: u32 =
                get(SYSTEM_OBJECT, &addr(PROP_DEFAULT_OUTPUT, SCOPE_GLOBAL)).unwrap_or(0);
            for id in ids {
                // Output-capable = has at least one output stream.
                let streams = data_size(id, &addr(PROP_STREAMS, SCOPE_OUTPUT)).unwrap_or(0);
                if streams == 0 {
                    continue;
                }
                let name = cf_string(id, &addr(PROP_NAME, SCOPE_GLOBAL)).unwrap_or_default();
                if name.is_empty() {
                    continue;
                }
                let transport: u32 = get(id, &addr(PROP_TRANSPORT, SCOPE_GLOBAL)).unwrap_or(0);
                let rate: Option<f64> = get(id, &addr(PROP_NOMINAL_RATE, SCOPE_GLOBAL));
                let hog: Option<i32> = get(id, &addr(PROP_HOG, SCOPE_GLOBAL));
                let max_rate = max_physical_rate(id);
                out.push(CaDevice {
                    id,
                    name,
                    transport: transport_label(transport).to_owned(),
                    sample_rate: rate.map(|r| r.round() as u32),
                    max_sample_rate: max_rate,
                    hog_pid: hog.filter(|p| *p >= 0),
                    is_default: id == default_id,
                });
            }
        }
        out
    }
}

#[cfg(target_os = "macos")]
pub fn output_devices() -> Vec<CaDevice> {
    sys::output_devices()
}

#[cfg(not(target_os = "macos"))]
pub fn output_devices() -> Vec<CaDevice> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn enumerates_output_devices() {
        let d = output_devices();
        // Every Mac has at least built-in speakers (or a virtual output in CI).
        assert!(!d.is_empty());
        assert!(d.iter().all(|x| !x.name.is_empty()));
        assert!(d.iter().any(|x| x.is_default));
    }
}

#[cfg(test)]
mod dump {
    #[test]
    #[ignore]
    fn dump_devices() {
        for d in super::output_devices() {
            println!("{d:?}");
        }
    }
}
