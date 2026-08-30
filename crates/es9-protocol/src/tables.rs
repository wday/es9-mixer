//! Routing source and destination values, and the signal families they belong to.

/// A family of related signals. Stereo links are stored per family, not per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// USB channels 1-8.
    UsbLow,
    /// USB channels 9-16.
    UsbHigh,
    /// Mixer block 2 outputs, "MIX 1-8".
    Mix,
    /// S/PDIF ports, or mixer block 3 outputs when Mixer 2 is enabled.
    SpdifOrMix2,
    /// The 16 internal summing buses.
    Bus,
    /// The physical analogue inputs.
    Input,
}

impl Family {
    /// The high nibble the module uses for this family in routing bytes.
    pub const fn nibble(self) -> u8 {
        match self {
            Self::UsbLow => 0x0,
            Self::UsbHigh => 0x1,
            Self::Mix => 0x2,
            Self::SpdifOrMix2 => 0x3,
            Self::Bus => 0x6,
            Self::Input => 0x7,
        }
    }

    /// Reads a family from the high nibble of a routing byte.
    pub const fn from_nibble(n: u8) -> Option<Self> {
        match n {
            0x0 => Some(Self::UsbLow),
            0x1 => Some(Self::UsbHigh),
            0x2 => Some(Self::Mix),
            0x3 => Some(Self::SpdifOrMix2),
            0x6 => Some(Self::Bus),
            0x7 => Some(Self::Input),
            _ => None,
        }
    }

    /// Bit position in the link bitmap for the first pair of this family.
    ///
    /// Derived from the config dump layout: inputs occupy the low bits, then buses,
    /// then the four small families.
    pub const fn link_bit_base(self) -> u32 {
        match self {
            Self::Input => 0,
            Self::Bus => 8,
            Self::UsbLow => 16,
            Self::UsbHigh => 20,
            Self::Mix => 24,
            Self::SpdifOrMix2 => 28,
        }
    }

    /// How many stereo pairs this family can form.
    pub const fn pair_count(self) -> u32 {
        match self {
            // 14 inputs are offered, giving 7 pairs.
            Self::Input => 7,
            Self::Bus => 8,
            Self::UsbLow | Self::UsbHigh | Self::Mix | Self::SpdifOrMix2 => 4,
        }
    }
}

/// Wire values for the physical inputs, which are not in natural order.
///
/// Index is `input number - 1`; the value is the byte the module expects. The config
/// tool hides this permutation from the user and so does this crate.
pub const INPUT_CAPTURE_LOOKUP: [u8; 14] = [
    0x78, 0x79, 0x76, 0x75, 0x74, 0x7B, 0x7A, 0x77, 0x73, 0x72, 0x7D, 0x7C, 0x7E, 0x7F,
];

/// Converts a natural input number (1-based) to its wire byte.
pub fn input_to_wire(input: u8) -> Option<u8> {
    INPUT_CAPTURE_LOOKUP
        .get(usize::from(input.checked_sub(1)?))
        .copied()
}

/// Converts a wire byte in the input family back to a natural input number (1-based).
pub fn wire_to_input(wire: u8) -> Option<u8> {
    INPUT_CAPTURE_LOOKUP
        .iter()
        .position(|&w| w == wire)
        .map(|i| i as u8 + 1)
}

/// Something a mixer or USB capture channel can listen to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// USB playback channel, 1-16.
    Usb(u8),
    /// Mixer output, 1-8.
    Mix(u8),
    /// S/PDIF input; `false` is left, `true` is right.
    Spdif(bool),
    /// Internal summing bus, 1-16.
    Bus(u8),
    /// Physical analogue input, 1-14, in natural order.
    Input(u8),
}

impl Source {
    /// Encodes to the module's wire byte, applying the input permutation.
    pub fn to_wire(self) -> Option<u8> {
        Some(match self {
            Self::Usb(n @ 1..=8) => n - 1,
            Self::Usb(n @ 9..=16) => 0x10 + (n - 9),
            Self::Mix(n @ 1..=8) => 0x20 + (n - 1),
            Self::Spdif(right) => 0x30 + u8::from(right),
            Self::Bus(n @ 1..=16) => 0x60 + (n - 1),
            Self::Input(n) => input_to_wire(n)?,
            _ => return None,
        })
    }

    /// Decodes a wire byte, reversing the input permutation.
    pub fn from_wire(w: u8) -> Option<Self> {
        match Family::from_nibble(w >> 4)? {
            Family::UsbLow => Some(Self::Usb((w & 0x0F) + 1)),
            Family::UsbHigh => Some(Self::Usb((w & 0x0F) + 9)),
            Family::Mix => Some(Self::Mix((w & 0x0F) + 1)),
            Family::SpdifOrMix2 => match w & 0x0F {
                0 => Some(Self::Spdif(false)),
                1 => Some(Self::Spdif(true)),
                _ => None,
            },
            Family::Bus => Some(Self::Bus((w & 0x0F) + 1)),
            Family::Input => wire_to_input(w).map(Self::Input),
        }
    }

    /// Reconstructs a source from its family and zero-based index within that family.
    ///
    /// Used when a stereo link forces a channel's partner to the adjacent source.
    pub const fn from_family_index(family: Family, index: u8) -> Option<Self> {
        Some(match family {
            Family::UsbLow if index < 8 => Self::Usb(index + 1),
            Family::UsbHigh if index < 8 => Self::Usb(index + 9),
            Family::Mix if index < 8 => Self::Mix(index + 1),
            Family::SpdifOrMix2 if index < 2 => Self::Spdif(index == 1),
            Family::Bus if index < 16 => Self::Bus(index + 1),
            Family::Input if index < 14 => Self::Input(index + 1),
            _ => return None,
        })
    }

    /// The family this source belongs to, and its zero-based index within it.
    ///
    /// Used to look up stereo links, which are stored per family.
    pub fn family_index(self) -> Option<(Family, u8)> {
        Some(match self {
            Self::Usb(n @ 1..=8) => (Family::UsbLow, n - 1),
            Self::Usb(n @ 9..=16) => (Family::UsbHigh, n - 9),
            Self::Mix(n @ 1..=8) => (Family::Mix, n - 1),
            Self::Spdif(right) => (Family::SpdifOrMix2, u8::from(right)),
            Self::Bus(n @ 1..=16) => (Family::Bus, n - 1),
            Self::Input(n @ 1..=14) => (Family::Input, n - 1),
            _ => return None,
        })
    }
}

impl core::fmt::Display for Source {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Usb(n) => write!(f, "USB {n}"),
            Self::Mix(n) => write!(f, "MIX {n}"),
            Self::Spdif(right) => write!(f, "S/PDIF {}", if *right { "R" } else { "L" }),
            Self::Bus(n) => write!(f, "Bus {n}"),
            Self::Input(n) => write!(f, "Input {n}"),
        }
    }
}

/// Where a USB playback or mixer output channel is sent.
///
/// Physical output numbering is permuted on the wire, and outputs 7 and 8 are
/// additionally swapped relative to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    /// Main output, `false` left and `true` right.
    MainOut(bool),
    /// Headphone output.
    Phones(bool),
    /// ES-5 expansion output.
    Es5(bool),
    /// Physical 3.5mm output, 1-8.
    Output(u8),
    /// Internal summing bus, 1-16.
    Bus(u8),
}

impl core::fmt::Display for Destination {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let side = |right: &bool| if *right { "R" } else { "L" };
        match self {
            Self::MainOut(r) => write!(f, "Main Out {}", side(r)),
            Self::Phones(r) => write!(f, "Phones {}", side(r)),
            Self::Es5(r) => write!(f, "ES-5 {}", side(r)),
            Self::Output(n) => write!(f, "Output {n}"),
            Self::Bus(n) => write!(f, "Bus {n}"),
        }
    }
}

const OUTPUT_WIRE: [u8; 8] = [8, 9, 4, 5, 2, 3, 11, 10];

impl Destination {
    /// Encodes to the module's wire byte.
    pub fn to_wire(self) -> Option<u8> {
        Some(match self {
            Self::MainOut(right) => 6 + u8::from(right),
            Self::Phones(right) => 12 + u8::from(right),
            Self::Es5(right) => 14 + u8::from(right),
            Self::Output(n @ 1..=8) => OUTPUT_WIRE[usize::from(n - 1)],
            Self::Bus(n @ 1..=16) => 0x10 + (n - 1),
            _ => return None,
        })
    }

    /// Decodes a wire byte.
    pub fn from_wire(w: u8) -> Option<Self> {
        if (0x10..=0x1F).contains(&w) {
            return Some(Self::Bus((w & 0x0F) + 1));
        }
        if let Some(i) = OUTPUT_WIRE.iter().position(|&x| x == w) {
            return Some(Self::Output(i as u8 + 1));
        }
        match w {
            6 | 7 => Some(Self::MainOut(w == 7)),
            12 | 13 => Some(Self::Phones(w == 13)),
            14 | 15 => Some(Self::Es5(w == 15)),
            _ => None,
        }
    }
}
