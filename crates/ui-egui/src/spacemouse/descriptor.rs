#![cfg_attr(not(windows), allow(dead_code))]
//! Where a HID device's input reports carry its six motion axes (X, Y, Z, Rx, Ry, Rz on the Generic Desktop page) and over
//! what range, read from its report descriptor's items as the HID 1.11 specification (6.2.2) lays them out. A 3D mouse
//! declares them under a Multi-axis Controller; only the axes are looked for, so the order and width a device chose don't
//! matter.

/// Fields a single item may declare at most, so a hostile descriptor can't make the reader loop for long.
const MAX_COUNT: u32 = 1024;
/// The axes' usages on the Generic Desktop page: X, Y, Z, Rx, Ry, Rz.
const GENERIC_DESKTOP: u32 = 0x01;
const FIRST_AXIS: u32 = 0x30;

/// One axis in an input report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    pub report_id: u8,
    /// Bits from the start of the report's data, after its id.
    pub bit_offset: u32,
    pub bits: u32,
    pub logical_min: i64,
    pub logical_max: i64,
}

impl Field {
    /// Its value in `data` (a report after its id) as a share of its range, -1 to 1 about the range's middle.
    pub fn read(&self, data: &[u8]) -> Option<f64> {
        if self.bits == 0 || self.bits > 32 {
            return None;
        }
        let mut raw = 0_u64;
        for k in 0..self.bits {
            let bit = self.bit_offset.checked_add(k)?;
            let byte = *data.get(usize::try_from(bit / 8).ok()?)?;
            raw |= u64::from((byte >> (bit % 8)) & 1) << k;
        }
        let sign = 1_u64 << (self.bits - 1);
        let value = if self.logical_min < 0 && raw & sign != 0 { raw as i64 - (1_i64 << self.bits) } else { raw as i64 };
        let (min, max) = (self.logical_min as f64, self.logical_max as f64);
        let half = (max - min) / 2.0;
        (half > 0.0).then(|| ((value as f64 - (min + max) / 2.0) / half).clamp(-1.0, 1.0))
    }
}

/// Where the six axes are, as far as the descriptor declares them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Layout {
    pub axes: [Option<Field>; 6],
    /// Whether the reports begin with their id (the descriptor declares report ids).
    pub numbered: bool,
}

impl Layout {
    /// 3Dconnexion's own, for one of its devices whose descriptor can't be read: report 1 holds X, Y and Z, report 2 Rx, Ry and
    /// Rz, each 16 bits of ±350.
    pub fn three_d_connexion() -> Layout {
        let field = |report_id, index: u32| Field { report_id, bit_offset: index * 16, bits: 16, logical_min: -350, logical_max: 350 };
        Layout { axes: [field(1, 0), field(1, 1), field(1, 2), field(2, 0), field(2, 1), field(2, 2)].map(Some), numbered: true }
    }

    pub fn is_empty(&self) -> bool {
        self.axes.iter().all(Option::is_none)
    }

    /// The axes `report` carries (its id first when the reports are numbered), each as a share -1 to 1; None for the others.
    pub fn decode(&self, report: &[u8]) -> [Option<f64>; 6] {
        let (id, data) = match (self.numbered, report.split_first()) {
            (true, Some((id, data))) => (*id, data),
            (true, None) => return [None; 6],
            (false, _) => (0, report),
        };
        self.axes.map(|f| f.filter(|f| f.report_id == id).and_then(|f| f.read(data)))
    }

    /// The axes of input reports a report descriptor declares. What it doesn't declare (or what can't be read) is None.
    pub fn parse(descriptor: &[u8]) -> Layout {
        #[derive(Clone, Copy, Default)]
        struct Globals {
            page: u32,
            min: i64,
            max: i64,
            max_unsigned: i64,
            size: u32,
            count: u32,
            id: u8,
        }
        let mut g = Globals::default();
        let mut pushed: Vec<Globals> = Vec::new();
        // Local items: usages (with their page), or a range of them.
        let mut usages: Vec<u32> = Vec::new();
        let (mut from, mut to): (Option<u32>, Option<u32>) = (None, None);
        // Bits of input data each report id has so far.
        let mut offsets: Vec<(u8, u32)> = Vec::new();
        let mut layout = Layout::default();
        let mut i = 0_usize;
        while let Some(&prefix) = descriptor.get(i) {
            // A long item: its data's size is in the next byte, and nothing here is one.
            if prefix == 0xFE {
                i = i.saturating_add(3 + descriptor.get(i + 1).map_or(0, |s| usize::from(*s)));
                continue;
            }
            let size = match prefix & 0x03 {
                3 => 4,
                s => usize::from(s),
            };
            let Some(data) = descriptor.get(i + 1..i + 1 + size) else { break };
            let unsigned = data.iter().rev().fold(0_u32, |v, b| (v << 8) | u32::from(*b));
            let signed = match *data {
                [a] => i64::from(i8::from_le_bytes([a])),
                [a, b] => i64::from(i16::from_le_bytes([a, b])),
                [a, b, c, d] => i64::from(i32::from_le_bytes([a, b, c, d])),
                _ => 0,
            };
            // A usage of four bytes names its page itself; a shorter one is on the page in effect.
            let usage = if size == 4 { unsigned } else { (g.page << 16) | unsigned };
            match prefix & 0xFC {
                // Input: its fields take the usages in turn, the last repeating, or the range's.
                0x80 => {
                    let count = g.count.min(MAX_COUNT);
                    let at = match offsets.iter().position(|(id, _)| *id == g.id) {
                        Some(k) => k,
                        None => {
                            offsets.push((g.id, 0));
                            offsets.len() - 1
                        }
                    };
                    let start = offsets.get(at).map_or(0, |(_, bits)| *bits);
                    // Constant padding carries nothing, and an array's fields hold usages, not values.
                    let values = unsigned & 0x01 == 0 && unsigned & 0x02 != 0;
                    // A logical maximum below the minimum is one that was meant unsigned (0xFF after a minimum of 0).
                    let max = if g.max < g.min { g.max_unsigned } else { g.max };
                    for k in 0..count {
                        let usage = match (from, to) {
                            (Some(a), Some(b)) => Some(a.saturating_add(k).min(b)),
                            _ => usages.get(k as usize).or(usages.last()).copied(),
                        };
                        let Some(usage) = usage.filter(|_| values) else { continue };
                        let (page, id) = (usage >> 16, usage & 0xFFFF);
                        let axis = id.checked_sub(FIRST_AXIS).filter(|a| page == GENERIC_DESKTOP && *a < 6);
                        if let Some(slot) = axis.and_then(|a| layout.axes.get_mut(a as usize))
                            && slot.is_none()
                        {
                            let bit_offset = start.saturating_add(k.saturating_mul(g.size));
                            *slot = Some(Field { report_id: g.id, bit_offset, bits: g.size, logical_min: g.min, logical_max: max });
                        }
                    }
                    if let Some((_, bits)) = offsets.get_mut(at) {
                        *bits = start.saturating_add(count.saturating_mul(g.size));
                    }
                    usages.clear();
                    (from, to) = (None, None);
                }
                // Output, Feature, Collection: their usages are spent.
                0x90 | 0xA0 | 0xB0 => {
                    usages.clear();
                    (from, to) = (None, None);
                }
                0x04 => g.page = unsigned,
                0x14 => g.min = signed,
                0x24 => (g.max, g.max_unsigned) = (signed, i64::from(unsigned)),
                0x74 => g.size = unsigned,
                0x84 => {
                    g.id = u8::try_from(unsigned).unwrap_or(0);
                    layout.numbered = true;
                }
                0x94 => g.count = unsigned,
                0xA4 => pushed.push(g),
                0xB4 => g = pushed.pop().unwrap_or(g),
                0x08 if usages.len() < MAX_COUNT as usize => usages.push(usage),
                0x18 => from = Some(usage),
                0x28 => to = Some(usage),
                _ => {}
            }
            i += 1 + size;
        }
        layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A SpacePilot Pro's (046d:c629) report descriptor, as Windows gives it.
    const SPACE_PILOT_PRO: &[u8] = &[
        0x05, 0x01, 0x09, 0x08, 0xa1, 0x01, 0x09, 0x00, 0xa1, 0x00, 0x85, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x32, 0x16, 0x0c, 0xfe, 0x26,
        0xf4, 0x01, 0x36, 0x00, 0x80, 0x46, 0xff, 0x7f, 0x75, 0x10, 0x95, 0x03, 0x81, 0x02, 0xc0, 0x09, 0x00, 0xa1, 0x00, 0x85, 0x02, 0x09,
        0x33, 0x09, 0x34, 0x09, 0x35, 0x16, 0x0c, 0xfe, 0x26, 0xf4, 0x01, 0x75, 0x10, 0x95, 0x03, 0x81, 0x02, 0xc0, 0x09, 0x00, 0xa1, 0x02,
        0x85, 0x03, 0x05, 0x09, 0x19, 0x01, 0x29, 0x1f, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x1f, 0x35, 0x00, 0x45, 0x00, 0x81, 0x02,
        0x75, 0x01, 0x95, 0x01, 0x81, 0x03, 0xc0, 0x09, 0x00, 0xa1, 0x02, 0x85, 0x04, 0x05, 0x08, 0x09, 0x4b, 0x15, 0x00, 0x25, 0x01, 0x75,
        0x01, 0x95, 0x01, 0x91, 0x02, 0x75, 0x07, 0x95, 0x01, 0x91, 0x03, 0xc0, 0x06, 0x00, 0xff, 0x09, 0x01, 0xa1, 0x02, 0x09, 0x3a, 0xa1,
        0x02, 0x85, 0x05, 0x09, 0x20, 0x15, 0x80, 0x25, 0x7f, 0x35, 0x00, 0x45, 0x01, 0x75, 0x08, 0x95, 0x01, 0xb1, 0x02, 0xc0, 0x09, 0x00,
        0xa1, 0x02, 0x85, 0x06, 0x09, 0x21, 0x15, 0x80, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x01, 0xb1, 0x02, 0xc0, 0x09, 0x00, 0xa1, 0x02, 0x85,
        0x07, 0x09, 0x22, 0x15, 0x80, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x01, 0xb1, 0x02, 0xc0, 0x09, 0x00, 0xa1, 0x02, 0x85, 0x08, 0x09, 0x23,
        0x15, 0x80, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x07, 0xb1, 0x02, 0xc0, 0x09, 0x00, 0xa1, 0x02, 0x85, 0x09, 0x09, 0x24, 0x15, 0x80, 0x25,
        0x7f, 0x75, 0x08, 0x95, 0x07, 0xb1, 0x02, 0xc0, 0x09, 0x00, 0xa1, 0x02, 0x85, 0x0a, 0x09, 0x25, 0x15, 0x80, 0x25, 0x7f, 0x75, 0x08,
        0x95, 0x07, 0xb1, 0x02, 0xc0, 0x09, 0x00, 0xa1, 0x02, 0x85, 0x0b, 0x09, 0x26, 0x15, 0x80, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x01, 0xb1,
        0x02, 0xc0, 0x09, 0x00, 0xa1, 0x02, 0x85, 0x13, 0x09, 0x2e, 0x15, 0x80, 0x25, 0x7f, 0x75, 0x08, 0x95, 0x01, 0xb1, 0x02, 0xc0, 0x09,
        0x00, 0xa1, 0x02, 0x85, 0x14, 0x09, 0x2f, 0x15, 0x80, 0x25, 0x7f, 0x75, 0x10, 0x95, 0x0e, 0xb1, 0x02, 0xc0, 0xc0, 0xc0,
    ];
    #[test]
    fn a_space_pilot_pro_s_axes_are_16_bits_of_500_in_reports_1_and_2() {
        let layout = Layout::parse(SPACE_PILOT_PRO);
        assert!(layout.numbered);
        let field = |report_id, bit_offset| Some(Field { report_id, bit_offset, bits: 16, logical_min: -500, logical_max: 500 });
        assert_eq!(layout.axes, [field(1, 0), field(1, 16), field(1, 32), field(2, 0), field(2, 16), field(2, 32)]);
        // X at 250, Z at -500; report 2 holds the turns.
        assert_eq!(layout.decode(&[1, 250, 0, 0, 0, 0x0C, 0xFE]), [Some(0.5), Some(0.0), Some(-1.0), None, None, None]);
        assert_eq!(layout.decode(&[2, 0, 0, 0xF4, 0x01, 0, 0]), [None, None, None, Some(0.0), Some(1.0), Some(0.0)]);
        // The buttons' report, and one too short for its fields, hold no axes.
        assert_eq!(layout.decode(&[3, 1, 0, 0, 0]), [None; 6]);
        assert_eq!(layout.decode(&[1, 250]), [None; 6]);
    }

    #[test]
    fn a_fallback_and_hostile_descriptors() {
        let fallback = Layout::three_d_connexion();
        assert_eq!(fallback.decode(&[1, 0x5E, 0x01, 0, 0, 0xA2, 0xFE]), [Some(1.0), Some(0.0), Some(-1.0), None, None, None]);
        // Nothing, truncated items, a long item, an absurd count and size, and numbered reports with no id give no axes or panic.
        for hostile in [&[][..], &[0x26], &[0xFE, 0xFF], &[0x95, 0xFF, 0xFF, 0xFF, 0xFF, 0x75, 0xFF, 0x09, 0x30, 0x81, 0x02], &[0xB4, 0xB4]]
        {
            let layout = Layout::parse(hostile);
            let _ = layout.decode(&[1, 2, 3]);
        }
        assert!(Layout::parse(&[0x05, 0x01, 0x09, 0x30, 0x75, 0x08, 0x95, 0x01, 0x81, 0x00]).is_empty(), "an array holds no values");
        assert_eq!(Layout::parse(SPACE_PILOT_PRO).decode(&[]), [None; 6]);
    }
}
