//! Logitech HID++ 1.0 / 2.0 protocol.
//!
//! Wire format: `[report_id, device_index, feature_index, function<<4 | sw_id, params...]`.
//! Short reports (0x10) are 7 bytes, long (0x11) 20 bytes, very long (0x12) 64 bytes.
//! On Windows every HID top-level collection is a separate handle, so a [`Channel`]
//! opens all vendor collections of a device and learns which one accepts which report.

pub mod features;

use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

pub const REPORT_SHORT: u8 = 0x10;
pub const REPORT_LONG: u8 = 0x11;
pub const REPORT_VERY_LONG: u8 = 0x12;
pub const SHORT_LEN: usize = 7;
pub const LONG_LEN: usize = 20;
pub const VERY_LONG_LEN: usize = 64;

/// Software id we stamp on requests; notifications from the device always use 0.
pub const SW_ID: u8 = 0x0E;
/// Device index for devices plugged in directly (not behind a receiver).
pub const DEVICE_DIRECT: u8 = 0xFF;

const DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HID: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("timed out waiting for device response")]
    Timeout,
    #[error("HID++ 2.0 error {0:#04x} ({})", hidpp20_error_name(*.0))]
    Hidpp20(u8),
    #[error("HID++ 1.0 error {0:#04x}")]
    Hidpp10(u8),
    #[error("feature {0:#06x} not supported by device")]
    Unsupported(u16),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn hidpp20_error_name(code: u8) -> &'static str {
    match code {
        0 => "no error",
        1 => "unknown",
        2 => "invalid argument",
        3 => "out of range",
        4 => "hardware error",
        5 => "logitech internal",
        6 => "invalid feature index",
        7 => "invalid function",
        8 => "busy",
        9 => "unsupported",
        _ => "?",
    }
}

/// One HID++ report as received from or sent to the device.
#[derive(Clone, Copy)]
pub struct Report {
    buf: [u8; VERY_LONG_LEN],
    len: usize,
}

impl Report {
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let expected = match *bytes.first()? {
            REPORT_SHORT => SHORT_LEN,
            REPORT_LONG => LONG_LEN,
            REPORT_VERY_LONG => VERY_LONG_LEN,
            _ => return None,
        };
        if bytes.len() < 4 {
            return None;
        }
        let mut buf = [0u8; VERY_LONG_LEN];
        let len = bytes.len().min(expected);
        buf[..len].copy_from_slice(&bytes[..len]);
        Some(Self { buf, len: expected })
    }

    pub fn report_id(&self) -> u8 {
        self.buf[0]
    }
    pub fn device_index(&self) -> u8 {
        self.buf[1]
    }
    pub fn feature_index(&self) -> u8 {
        self.buf[2]
    }
    pub fn function(&self) -> u8 {
        self.buf[3] >> 4
    }
    pub fn sw_id(&self) -> u8 {
        self.buf[3] & 0x0F
    }
    pub fn params(&self) -> &[u8] {
        &self.buf[4..self.len]
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

impl std::fmt::Debug for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.as_bytes() {
            write!(f, "{b:02x} ")?;
        }
        Ok(())
    }
}

/// All vendor-defined HID collections of one physical device.
pub struct Channel {
    handles: Vec<HidDevice>,
    /// Which handle accepts short / long / very long writes, learned on first use.
    route: [Option<usize>; 3],
}

impl Channel {
    /// Opens every vendor-defined collection (usage page >= 0xFF00) in `infos`.
    pub fn open(api: &HidApi, infos: &[&DeviceInfo]) -> Result<Self> {
        let mut handles = Vec::new();
        for info in infos.iter().filter(|i| i.usage_page() >= 0xFF00) {
            match info.open_device(api) {
                Ok(dev) => handles.push(dev),
                Err(e) => log::debug!(
                    "cannot open {:?} (usage {:04x}:{:04x}): {e}",
                    info.path(),
                    info.usage_page(),
                    info.usage()
                ),
            }
        }
        if handles.is_empty() {
            return Err(Error::Other("no vendor HID collection could be opened".into()));
        }
        Ok(Self { handles, route: [None; 3] })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        let slot = (data[0] - REPORT_SHORT) as usize;
        if let Some(i) = self.route[slot] {
            self.handles[i].write(data)?;
            return Ok(());
        }
        let mut last_err = None;
        for (i, h) in self.handles.iter().enumerate() {
            match h.write(data) {
                Ok(_) => {
                    self.route[slot] = Some(i);
                    return Ok(());
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.map(Error::from).unwrap_or(Error::Timeout))
    }

    pub fn supports(&mut self, report_id: u8) -> bool {
        self.route[(report_id - REPORT_SHORT) as usize].is_some()
    }

    /// Reads the next HID++ report from any collection, or `None` on timeout.
    pub fn read(&self, timeout: Duration) -> Result<Option<Report>> {
        let deadline = Instant::now() + timeout;
        let primary = self.route[1].or(self.route[0]).unwrap_or(0);
        let mut buf = [0u8; VERY_LONG_LEN];
        loop {
            for (i, h) in self.handles.iter().enumerate() {
                if i == primary {
                    continue;
                }
                let n = h.read_timeout(&mut buf, 0)?;
                if let Some(r) = (n > 0).then(|| Report::from_bytes(&buf[..n])).flatten() {
                    return Ok(Some(r));
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            // Block on the main collection; wake regularly only if others need polling.
            let mut wait = deadline - now;
            if self.handles.len() > 1 {
                wait = wait.min(Duration::from_millis(4));
            }
            let n = self.handles[primary].read_timeout(&mut buf, wait.as_millis().max(1) as i32)?;
            if let Some(r) = (n > 0).then(|| Report::from_bytes(&buf[..n])).flatten() {
                return Ok(Some(r));
            }
        }
    }
}

/// Information about one feature in a HID++ 2.0 device's feature table.
#[derive(Debug, Clone, Copy)]
pub struct FeatureInfo {
    pub id: u16,
    pub index: u8,
    pub flags: u8,
    pub version: u8,
}

/// A HID++ device (or the receiver itself) reachable through a [`Channel`].
pub struct Device {
    chan: Channel,
    pub index: u8,
    features: HashMap<u16, u8>,
    events: VecDeque<Report>,
}

impl Device {
    pub fn new(chan: Channel, index: u8) -> Self {
        Self { chan, index, features: HashMap::new(), events: VecDeque::new() }
    }

    /// Returns the HID++ protocol version `(major, minor)`. 1.0 devices answer with an error.
    pub fn protocol_version(&mut self) -> Result<(u8, u8)> {
        match self.call_index(0x00, 0x1, &[0, 0, 0x5A]) {
            Ok(r) => Ok((r.params()[0], r.params()[1])),
            Err(Error::Hidpp10(0x01)) => Ok((1, 0)),
            Err(e) => Err(e),
        }
    }

    /// Looks up (and caches) the feature-table index for a feature id.
    pub fn feature_index(&mut self, id: u16) -> Result<u8> {
        if let Some(&i) = self.features.get(&id) {
            return Ok(i);
        }
        let r = self.call_index(0x00, 0x0, &id.to_be_bytes())?;
        let index = r.params()[0];
        if index == 0 && id != features::ROOT {
            return Err(Error::Unsupported(id));
        }
        self.features.insert(id, index);
        Ok(index)
    }

    pub fn has_feature(&mut self, id: u16) -> bool {
        self.feature_index(id).is_ok()
    }

    /// Calls `function` of the feature `id`, resolving its index first.
    pub fn call(&mut self, id: u16, function: u8, params: &[u8]) -> Result<Report> {
        let index = self.feature_index(id)?;
        self.call_index(index, function, params)
    }

    /// Sends a request to a raw feature index and waits for the matching response.
    /// Unrelated reports (button events, etc.) are queued for [`Device::next_event`].
    pub fn call_index(&mut self, index: u8, function: u8, params: &[u8]) -> Result<Report> {
        let fn_sw = (function << 4) | SW_ID;
        let (report_id, len) = if params.len() > 16 {
            (REPORT_VERY_LONG, VERY_LONG_LEN)
        } else if params.len() <= 3 && !self.chan.supports(REPORT_LONG) && self.chan.supports(REPORT_SHORT) {
            (REPORT_SHORT, SHORT_LEN)
        } else {
            (REPORT_LONG, LONG_LEN)
        };
        let mut out = [0u8; VERY_LONG_LEN];
        out[0] = report_id;
        out[1] = self.index;
        out[2] = index;
        out[3] = fn_sw;
        out[4..4 + params.len()].copy_from_slice(params);

        if let Err(e) = self.chan.write(&out[..len]) {
            // Some receivers only expose the short collection.
            if report_id == REPORT_LONG && params.len() <= 3 {
                out[0] = REPORT_SHORT;
                self.chan.write(&out[..SHORT_LEN])?;
            } else {
                return Err(e);
            }
        }

        let deadline = Instant::now() + DEFAULT_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(r) = self.chan.read(left)? else {
                return Err(Error::Timeout);
            };
            let b = r.as_bytes();
            if b[1] != self.index {
                self.events.push_back(r);
                continue;
            }
            if b[2] == index && b[3] == fn_sw {
                return Ok(r);
            }
            if b[2] == 0xFF && b[3] == index && b[4] == fn_sw {
                return Err(Error::Hidpp20(b[5]));
            }
            if b[2] == 0x8F && b[3] == index && b[4] == fn_sw {
                return Err(Error::Hidpp10(b[5]));
            }
            self.events.push_back(r);
        }
    }

    /// Next unsolicited report (notification / diverted button event) from this channel.
    pub fn next_event(&mut self, timeout: Duration) -> Result<Option<Report>> {
        if let Some(r) = self.events.pop_front() {
            return Ok(Some(r));
        }
        self.chan.read(timeout)
    }

    /// Enumerates the whole feature table via IFeatureSet (0x0001).
    pub fn list_features(&mut self) -> Result<Vec<FeatureInfo>> {
        let set = self.feature_index(features::FEATURE_SET)?;
        let count = self.call_index(set, 0x0, &[])?.params()[0];
        let mut out = vec![FeatureInfo { id: features::ROOT, index: 0, flags: 0, version: 0 }];
        for i in 1..=count {
            let p = self.call_index(set, 0x1, &[i])?;
            let p = p.params();
            let id = u16::from_be_bytes([p[0], p[1]]);
            self.features.insert(id, i);
            out.push(FeatureInfo { id, index: i, flags: p[2], version: p[3] });
        }
        Ok(out)
    }

    /// Marketing name via DeviceName (0x0005).
    pub fn name(&mut self) -> Result<String> {
        let len = self.call(features::DEVICE_NAME, 0x0, &[])?.params()[0] as usize;
        let mut name = Vec::with_capacity(len);
        while name.len() < len {
            let r = self.call(features::DEVICE_NAME, 0x1, &[name.len() as u8])?;
            let chunk = r.params();
            let take = (len - name.len()).min(chunk.len());
            if take == 0 {
                break;
            }
            name.extend_from_slice(&chunk[..take]);
        }
        Ok(String::from_utf8_lossy(&name).trim_end_matches('\0').to_string())
    }
}
