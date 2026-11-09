//! The 3D mouse: a 3Dconnexion SpaceMouse, SpaceNavigator, SpacePilot and the like. While its cap is held off rest it pans,
//! zooms and orbits the active 3D view, as 3DxWare's object mode moves a model: push the cap right and the model goes right,
//! press it down and the model goes down, pull it toward you and the model comes closer, twist it and the model spins, tilt it
//! back and the model tips its top toward you. Its roll (tilting the cap left or right) does nothing: the view's camera keeps
//! upright. Like the hand, it moves only the active view (`orbit.rs`): plans, sections, sheets and schedules it leaves alone,
//! and a 3D view drawn beside the active one too.
//!
//! The device is read on a thread of its own, from its HID Multi-axis Controller: without 3DxWare, and with it while its
//! profile for BIMCraft (`%APPDATA%\3Dconnexion\3DxWare\Cfg\bimcraft.xml`) passes the axes on as `HIDMultiAxis_*`. The axes
//! are found in the report descriptor (`descriptor.rs`), so their order and range don't matter, and come in the device's own
//! order and signs: X right, Y toward the user, Z down, and the turns about them. The speeds, the dead zone, the staleness and
//! the object-mode mapping follow SolidCraft's 3D mouse (tangency-labs/solidcraft, MIT OR Apache-2.0).
//!
//! The desktop app attaches the reader (`BimApp::space_mouse`): a window starts without one, so a test never opens a device.
//! It reads on Windows only for now; elsewhere [`SpaceMouse::start`] gives none. With `BIMCRAFT_SPACEMOUSE_LOG` set to a
//! file, the reader writes there what it found and every report it read.

mod descriptor;

use std::io::Write as _;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bimcraft_core::view3d::Camera;

use descriptor::Layout;

/// The share of full deflection about rest that reads as rest, so a cap resting slightly off zero doesn't creep the view.
const DEAD_ZONE: f64 = 20.0 / 350.0;
/// The overall sensitivity: every speed below is multiplied by it.
const SENSITIVITY: f64 = 1.0;
/// At full deflection, a second pans the view by this share of its height,
const PAN_PER_SECOND: f64 = 0.6;
/// zooms by e to this power,
const ZOOM_PER_SECOND: f64 = 1.5;
/// and turns it by this many degrees.
const ORBIT_DEGREES_PER_SECOND: f64 = 100.0;
/// The longest a frame moves for: the first frame after a pause moves as far as a 50 ms one.
const MAX_STEP: f64 = 0.05;
/// How long a reading holds without a new report: the device sends its reports for as long as the cap is off rest.
const STALE_AFTER: Duration = Duration::from_millis(150);

/// The cap as of now, each axis -1 to 1 past the dead zone: X, Y and Z, and the turns about them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub translation: [f64; 3],
    pub rotation: [f64; 3],
}

impl Motion {
    fn at_rest(&self) -> bool {
        self.translation.iter().chain(&self.rotation).all(|v| *v == 0.0)
    }
}

/// The device's last reading: each axis as a share of full deflection, -1 to 1, and when it came.
#[derive(Clone, Copy, Debug, Default)]
struct Reading {
    axes: [f64; 6],
    at: [Option<Instant>; 6],
}

impl Reading {
    /// Takes one input report as `layout` lays it out. Whether it held any axis.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn take(&mut self, layout: &Layout, report: &[u8], now: Instant) -> bool {
        let mut took = false;
        for ((value, at), new) in self.axes.iter_mut().zip(&mut self.at).zip(layout.decode(report)) {
            if let Some(new) = new {
                (*value, *at) = (new, Some(now));
                took = true;
            }
        }
        took
    }

    /// The cap at `now`: an axis not refreshed within [`STALE_AFTER`] reads as rest.
    fn motion(&self, now: Instant) -> Motion {
        let axis = |k: usize| match (self.axes.get(k), self.at.get(k)) {
            (Some(v), Some(Some(at))) if now.saturating_duration_since(*at) < STALE_AFTER => normalized(*v),
            _ => 0.0,
        };
        Motion { translation: [axis(0), axis(1), axis(2)], rotation: [axis(3), axis(4), axis(5)] }
    }
}

/// A share of full deflection as -1 to 1: rest within the dead zone, linear past it.
fn normalized(share: f64) -> f64 {
    if !share.is_finite() || share.abs() <= DEAD_ZONE {
        return 0.0;
    }
    share.signum() * ((share.abs() - DEAD_ZONE) / (1.0 - DEAD_ZONE)).min(1.0)
}

/// `cam` moved by `seconds` of `motion`, about the middle of the picture: X pans across, Z up and down and Y zooms, the twist
/// (Rz) turns it about the vertical and the tilt (Rx) raises or lowers it, as dragging the model would.
pub(crate) fn navigate(cam: Camera, motion: Motion, seconds: f64) -> Camera {
    let s = if seconds.is_finite() { seconds.clamp(0.0, MAX_STEP) * SENSITIVITY } else { 0.0 };
    let ([x, y, z], [rx, _, rz]) = (motion.translation, motion.rotation);
    let middle = (f64::from(cam.width) / 2.0, f64::from(cam.height) / 2.0);
    let pan = f64::from(cam.height) * PAN_PER_SECOND * s;
    let cam = if x != 0.0 || z != 0.0 { cam.pan(middle, (middle.0 + x * pan, middle.1 + z * pan)) } else { cam };
    let cam = if y != 0.0 { cam.zoom((y * ZOOM_PER_SECOND * s).exp(), middle) } else { cam };
    cam.orbit(rz * ORBIT_DEGREES_PER_SECOND * s, rx * ORBIT_DEGREES_PER_SECOND * s)
}

/// The debug log, when `BIMCRAFT_SPACEMOUSE_LOG` names a file: what the reader found and every report it read.
struct Log {
    file: std::fs::File,
    start: Instant,
    lines: u32,
}

static LOG: OnceLock<Option<Mutex<Log>>> = OnceLock::new();
/// The most lines the debug log takes in a run.
const MAX_LOG_LINES: u32 = 200_000;

/// A line in the debug log, if there is one.
#[cfg_attr(not(windows), allow(dead_code))]
fn note(line: impl FnOnce() -> String) {
    let log = LOG.get_or_init(|| {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(std::env::var_os("BIMCRAFT_SPACEMOUSE_LOG")?).ok()?;
        Some(Mutex::new(Log { file, start: Instant::now(), lines: 0 }))
    });
    let Some(Ok(mut log)) = log.as_ref().map(Mutex::lock) else { return };
    if log.lines < MAX_LOG_LINES {
        log.lines += 1;
        let t = log.start.elapsed().as_secs_f64();
        let _ = writeln!(log.file, "{t:9.3} {}", line());
    }
}

/// Six axes as the debug log shows them.
#[cfg_attr(not(windows), allow(dead_code))]
fn shown(axes: [f64; 6]) -> String {
    axes.map(|v| format!("{v:+.2}")).join(" ")
}

/// The 3D mouse's reader: its thread, and the reading it leaves.
pub struct SpaceMouse {
    reading: Arc<Mutex<Reading>>,
}

impl SpaceMouse {
    /// Starts reading the 3D mouse on a thread of its own, which calls `wake` as the cap moves, so the window draws a frame.
    /// Whether a device is plugged in or not: it looks for one every few seconds. None where it can't be read.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Option<SpaceMouse> {
        #[cfg(windows)]
        {
            let reading = Arc::new(Mutex::new(Reading::default()));
            let reader = Arc::clone(&reading);
            std::thread::Builder::new().name("bimcraft-3d-mouse".into()).spawn(move || hid::look(&reader, &wake)).ok()?;
            Some(SpaceMouse { reading })
        }
        #[cfg(not(windows))]
        {
            drop(wake);
            None
        }
    }

    /// The cap as of now, or None at rest or without a device.
    pub(crate) fn motion(&self) -> Option<Motion> {
        let motion = self.reading.lock().ok()?.motion(Instant::now());
        (!motion.at_rest()).then_some(motion)
    }

    /// A cap held at `shares` of full deflection (X, Y, Z, Rx, Ry, Rz) for the next hour, for a test.
    #[cfg(test)]
    pub(crate) fn held(shares: [f64; 6]) -> SpaceMouse {
        let at = Instant::now().checked_add(Duration::from_secs(3600));
        SpaceMouse { reading: Arc::new(Mutex::new(Reading { axes: shares, at: [at; 6] })) }
    }
}

/// Windows: the device through hidapi's own Windows backend (pure Rust).
#[cfg(windows)]
mod hid {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use hidapi::{HidApi, HidDevice};

    use super::{Layout, Reading, normalized, note, shown};

    const THREEDCONNEXION: u16 = 0x256f;
    const LOGITECH: u16 = 0x046d;
    /// 3Dconnexion's devices from before it had a vendor id of its own, under Logitech's: SpaceMouse Plus XT, CadMan,
    /// SpaceMouse Classic, SpaceBall 5000, SpaceTraveler, SpacePilot, SpaceNavigator, SpaceExplorer, SpaceNavigator for
    /// Notebooks, SpacePilot Pro and SpaceMouse Pro. Asked for one by one, so no Logitech keyboard or mouse is asked anything.
    const LOGITECH_3D_MICE: [u16; 11] = [0xc603, 0xc605, 0xc606, 0xc621, 0xc623, 0xc625, 0xc626, 0xc627, 0xc628, 0xc629, 0xc62b];
    /// The top-level usage (page, usage) a 3D mouse's axes come under: Generic Desktop, Multi-axis Controller.
    const MULTI_AXIS: (u16, u16) = (0x01, 0x08);
    /// How often to look for a device while none is open.
    const LOOK_EVERY: Duration = Duration::from_secs(2);

    /// Looks for a device every few seconds, and reads the one found until it goes away.
    pub(super) fn look(reading: &Mutex<Reading>, wake: &(dyn Fn() + Send + Sync)) {
        // Making the context lists every HID device once (hidapi 2.6 does, whatever its discovery setting); after that the
        // scans open each one without access and read only its ids, and ask the strings of the wanted ones alone.
        let Ok(mut api) = HidApi::new() else {
            note(|| "3D mouse: HID could not be started".into());
            return;
        };
        note(|| "3D mouse: reader started".into());
        loop {
            if let Some((device, layout, name)) = find(&mut api) {
                note(|| format!("3D mouse: opened {name}; {layout:?}"));
                follow(&device, &layout, reading, wake);
                // Gone: its cap is at rest.
                if let Ok(mut reading) = reading.lock() {
                    *reading = Reading::default();
                }
                wake();
            }
            std::thread::sleep(LOOK_EVERY);
        }
    }

    /// The first 3D mouse that opens: the device, its layout (from its descriptor, else 3Dconnexion's) and a name.
    fn find(api: &mut HidApi) -> Option<(HidDevice, Layout, String)> {
        api.reset_devices().ok()?;
        let _ = api.add_devices(THREEDCONNEXION, 0);
        for pid in LOGITECH_3D_MICE {
            let _ = api.add_devices(LOGITECH, pid);
        }
        let found: Vec<_> = api.device_list().filter(|d| (d.usage_page(), d.usage()) == MULTI_AXIS).cloned().collect();
        found.iter().find_map(|info| {
            let device = api.open_path(info.path()).ok()?;
            let mut descriptor = [0_u8; hidapi::MAX_REPORT_DESCRIPTOR_SIZE];
            let read = device.get_report_descriptor(&mut descriptor).ok().and_then(|n| descriptor.get(..n)).map(Layout::parse);
            // A descriptor that can't be read, or lays out no axis, is taken for 3Dconnexion's.
            let layout = read.filter(|layout| !layout.is_empty()).unwrap_or_else(Layout::three_d_connexion);
            let name = format!("{:04x}:{:04x} {}", info.vendor_id(), info.product_id(), info.path().to_string_lossy());
            Some((device, layout, name))
        })
    }

    /// Reads the device's reports until it goes away.
    fn follow(device: &HidDevice, layout: &Layout, reading: &Mutex<Reading>, wake: &(dyn Fn() + Send + Sync)) {
        let mut buffer = [0_u8; 64];
        let mut count = 0_u64;
        loop {
            match device.read_timeout(&mut buffer, 1000) {
                Ok(0) => {}
                Ok(n) => {
                    let report = buffer.get(..n).unwrap_or_default();
                    let taken = reading.lock().ok().and_then(|mut r| r.take(layout, report, Instant::now()).then_some(r.axes));
                    count += 1;
                    note(|| match taken {
                        Some(axes) => format!("report #{count} {report:02x?} shares {} axes {}", shown(axes), shown(axes.map(normalized))),
                        None => format!("report #{count} {report:02x?} (no axes)"),
                    });
                    if taken.is_some() {
                        wake();
                    }
                }
                Err(error) => {
                    note(|| format!("3D mouse: read failed ({error}): closed"));
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> Camera {
        Camera { azimuth: -90.0, elevation: 30.0, scale: 0.1, target: [0.0; 3], width: 800, height: 600, lens: None }
    }

    /// A 3Dconnexion report: its id, then little-endian i16s.
    fn report(id: u8, values: &[i16]) -> Vec<u8> {
        std::iter::once(id).chain(values.iter().flat_map(|v| v.to_le_bytes())).collect()
    }

    #[test]
    fn a_device_s_reports_come_in_as_shares_of_full_deflection() {
        let now = Instant::now();
        let device = Layout::three_d_connexion();
        let mut r = Reading::default();
        assert!(r.take(&device, &report(1, &[350, -175, 0]), now));
        assert!(r.take(&device, &report(2, &[0, 0, -350]), now));
        let m = r.motion(now);
        assert_eq!((m.translation[0], m.translation[2], m.rotation), (1.0, 0.0, [0.0, 0.0, -1.0]));
        assert!((m.translation[1] + (0.5 - DEAD_ZONE) / (1.0 - DEAD_ZONE)).abs() < 1e-12, "{m:?}");
        // Buttons, short reports and nothing at all are no axes.
        assert!(!r.take(&device, &report(3, &[1]), now) && !r.take(&device, &[1, 2], now) && !r.take(&device, &[], now));
    }

    #[test]
    fn the_dead_zone_and_a_stale_reading_are_rest() {
        let now = Instant::now();
        let device = Layout::three_d_connexion();
        let mut r = Reading::default();
        r.take(&device, &report(1, &[19, -19, 0]), now);
        assert!(r.motion(now).at_rest());
        r.take(&device, &report(1, &[200, 0, 0]), now);
        assert!(!r.motion(now).at_rest());
        assert!(r.motion(now + STALE_AFTER).at_rest());
        assert!(normalized(0.7) < 1.0 && normalized(1.0) == 1.0 && normalized(-1.8) == -1.0 && normalized(f64::NAN) == 0.0);
        // At rest the reader has no motion to give.
        assert_eq!(SpaceMouse { reading: Arc::new(Mutex::new(Reading::default())) }.motion(), None);
        assert!(SpaceMouse::held([0.5, 0.0, 0.0, 0.0, 0.0, 0.0]).motion().is_some());
    }

    #[test]
    fn the_cap_moves_the_model_as_object_mode_does() {
        let (cam, second) = (camera(), MAX_STEP);
        let middle = (400.0, 300.0);
        let push = |t: [f64; 3], r: [f64; 3]| navigate(cam, Motion { translation: t, rotation: r }, second);
        // Right and down: the target goes right and down on screen.
        let (x, y, _) = push([1.0, 0.0, 0.0], [0.0; 3]).project(cam.target);
        assert!(x > middle.0 + 1.0 && (y - middle.1).abs() < 1e-6, "{x} {y}");
        let (x, y, _) = push([0.0, 0.0, 1.0], [0.0; 3]).project(cam.target);
        assert!(y > middle.1 + 1.0 && (x - middle.0).abs() < 1e-6, "{x} {y}");
        // Toward the user: closer. A twist spins it, a tilt back shows more of its top.
        assert!(push([0.0, 1.0, 0.0], [0.0; 3]).scale > cam.scale && push([0.0, -1.0, 0.0], [0.0; 3]).scale < cam.scale);
        assert!(push([0.0; 3], [0.0, 0.0, 1.0]).azimuth > cam.azimuth);
        assert!(push([0.0; 3], [1.0, 0.0, 0.0]).elevation > cam.elevation);
        // Rest, no time, a hostile time and the roll leave it be.
        assert_eq!(push([0.0; 3], [0.0; 3]), cam);
        assert_eq!(push([0.0; 3], [0.0, 1.0, 0.0]), cam);
        assert_eq!(navigate(cam, Motion { translation: [1.0; 3], rotation: [1.0; 3] }, f64::NAN), cam);
        assert_eq!(navigate(cam, Motion { translation: [1.0; 3], rotation: [1.0; 3] }, 0.0), cam);
        // A long gap moves no further than a 50 ms frame.
        assert_eq!(navigate(cam, Motion { translation: [0.0, 1.0, 0.0], rotation: [0.0; 3] }, 10.0), push([0.0, 1.0, 0.0], [0.0; 3]));
    }
}
