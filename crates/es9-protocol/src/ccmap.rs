//! Resolving which MIDI CC number controls what.
//!
//! The ES-9 can be driven directly by MIDI CC with no computer in the path, which is
//! what makes a DAW-free live rig possible. The CC numbers are **fixed** — they cannot
//! be reassigned — and the base scheme is simple:
//!
//! ```text
//! CC = mix_index * 8 + channel_index
//! ```
//!
//! Mixes 1-8 occupy CC 0-63; with Mixer 2 enabled, mixes 9-16 occupy CC 64-127.
//!
//! Stereo links then rewrite it, per the user manual:
//!
//! - When a mixer input or a mix is stereo, the **lower-numbered** CC of the two that
//!   would apply is used, and the other is left unassigned.
//! - On a stereo mix, **pan** takes the CC that the right-channel mix would have used.
//!
//! The practical consequence is that toggling one stereo link silently changes the
//! meaning of up to half the CC numbers. A controller mapping is only valid for the link
//! configuration it was built against, and nothing on the module warns about this — hence
//! [`CcMap::diff`].

use crate::links::Links;
use crate::tables::{Family, Source};

/// A mix, or a stereo pair of mixes treated as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixRef {
    /// Zero-based index of the mix, or of the left mix of a stereo pair.
    pub first: u8,
    /// Whether this is a stereo pair occupying `first` and `first + 1`.
    pub stereo: bool,
}

/// A mixer input channel, or a stereo pair of channels treated as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelRef {
    /// Zero-based mixer channel, or the left channel of a stereo pair.
    pub first: u8,
    /// Whether this is a stereo pair occupying `first` and `first + 1`.
    pub stereo: bool,
}

/// Which control a CC drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// Fader level.
    Level,
    /// Pan position. Only exists on a stereo mix.
    Pan,
}

/// What a single CC number does under a given configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CcAssignment {
    /// The mix, or stereo mix pair, this CC belongs to.
    pub mix: MixRef,
    /// The mixer channel, or stereo channel pair.
    pub channel: ChannelRef,
    /// Whether the CC sets level or pan.
    pub control: Control,
}

/// The resolved meaning of all 128 CC numbers.
///
/// An entry is `None` when the CC is unassigned in this configuration — which happens
/// when a stereo link collapses two controls onto the lower-numbered CC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CcMap {
    entries: [Option<CcAssignment>; 128],
}

impl Default for CcMap {
    fn default() -> Self {
        Self {
            entries: [None; 128],
        }
    }
}

/// One CC whose meaning changed between two configurations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CcChange {
    /// The CC number affected.
    pub cc: u8,
    /// What it did before.
    pub before: Option<CcAssignment>,
    /// What it does now.
    pub after: Option<CcAssignment>,
}

/// One mixer strip: a level, and a pan when the mix is stereo.
///
/// This is the shape a console UI actually needs, and deriving it from the resolved CC
/// map rather than from the raw configuration means the CC numbers shown beside a control
/// are the ones the hardware will honour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripRef {
    /// The mix, or stereo mix pair.
    pub mix: MixRef,
    /// The mixer channel, or stereo channel pair.
    pub channel: ChannelRef,
    /// CC controlling this strip's level.
    pub level_cc: u8,
    /// CC controlling pan, present only on a stereo mix.
    pub pan_cc: Option<u8>,
}

impl CcMap {
    /// Resolves the CC map for a configuration.
    ///
    /// `capture` holds the eight sources feeding each mixer block: index 0 is block 2
    /// (mixes 1-8) and index 1 is block 3 (mixes 9-16).
    pub fn resolve(links: Links, mixer2_enabled: bool, capture: &[[Source; 8]; 2]) -> Self {
        let mut map = Self::default();
        let mix_count: u8 = if mixer2_enabled { 16 } else { 8 };

        for base in (0..mix_count).step_by(2) {
            let block = usize::from(base) / 8;
            let stereo_mix = links.is_linked(mix_family(base), mix_family_index(base));

            let mut ch: u8 = 0;
            while ch < 8 {
                // A stereo *source* collapses two adjacent mixer channels into one
                // strip, and only the lower channel's CC is used.
                let stereo_in = ch.is_multiple_of(2)
                    && capture[block][usize::from(ch)]
                        .family_index()
                        .is_some_and(|(f, i)| links.in_stereo_pair(f, i));

                let mix = MixRef {
                    first: base,
                    stereo: stereo_mix,
                };
                let channel = ChannelRef {
                    first: ch,
                    stereo: stereo_in,
                };

                map.put(base * 8 + ch, mix, channel, Control::Level);

                if stereo_mix {
                    // Pan takes the CC the right-hand mix of the pair would have used.
                    map.put((base + 1) * 8 + ch, mix, channel, Control::Pan);
                } else {
                    // Two independent mono mixes; the odd one keeps its own level CC.
                    let odd = MixRef {
                        first: base + 1,
                        stereo: false,
                    };
                    map.put((base + 1) * 8 + ch, odd, channel, Control::Level);
                }

                ch += if stereo_in { 2 } else { 1 };
            }
        }
        map
    }

    fn put(&mut self, cc: u8, mix: MixRef, channel: ChannelRef, control: Control) {
        if let Some(slot) = self.entries.get_mut(usize::from(cc)) {
            *slot = Some(CcAssignment {
                mix,
                channel,
                control,
            });
        }
    }

    /// What CC `cc` controls, if anything.
    pub fn get(&self, cc: u8) -> Option<&CcAssignment> {
        self.entries.get(usize::from(cc))?.as_ref()
    }

    /// Every assigned CC, in ascending order.
    pub fn assigned(&self) -> impl Iterator<Item = (u8, &CcAssignment)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| e.as_ref().map(|a| (i as u8, a)))
    }

    /// Finds the CC that drives a given control, if one does.
    pub fn cc_for(&self, mix: u8, channel: u8, control: Control) -> Option<u8> {
        self.assigned()
            .find(|(_, a)| {
                a.control == control
                    && covers(a.mix.first, a.mix.stereo, mix)
                    && covers(a.channel.first, a.channel.stereo, channel)
            })
            .map(|(cc, _)| cc)
    }

    /// The mixer strips this map describes, in mix then channel order.
    ///
    /// Levels and pans are paired up, so a caller does not have to reimplement the
    /// stereo rules to work out which pan belongs to which fader.
    pub fn strips(&self) -> Vec<StripRef> {
        let mut strips: Vec<StripRef> = Vec::new();
        for (cc, a) in self.assigned() {
            match a.control {
                Control::Level => strips.push(StripRef {
                    mix: a.mix,
                    channel: a.channel,
                    level_cc: cc,
                    pan_cc: None,
                }),
                Control::Pan => {
                    if let Some(s) = strips
                        .iter_mut()
                        .find(|s| s.mix.first == a.mix.first && s.channel.first == a.channel.first)
                    {
                        s.pan_cc = Some(cc);
                    }
                }
            }
        }
        strips
    }

    /// The level CC whose aux byte holds the pan written to `pan_cc`.
    ///
    /// Pan is written to one CC and stored against another: the module keeps it in the
    /// aux byte of the corresponding level cell (checklist H3). Anything reading pan back
    /// needs this indirection, so it lives here rather than being rediscovered.
    pub fn level_cc_for_pan_cc(&self, pan_cc: u8) -> Option<u8> {
        self.strips()
            .into_iter()
            .find(|s| s.pan_cc == Some(pan_cc))
            .map(|s| s.level_cc)
    }

    /// Every CC whose meaning differs between `self` and `other`.
    ///
    /// This is what makes a stereo-link toggle legible: the module rewrites the CC map
    /// silently, so the app shows what just moved.
    pub fn diff(&self, other: &Self) -> Vec<CcChange> {
        (0u8..128)
            .filter_map(|cc| {
                let before = self.entries[usize::from(cc)];
                let after = other.entries[usize::from(cc)];
                (before != after).then_some(CcChange { cc, before, after })
            })
            .collect()
    }
}

fn covers(first: u8, stereo: bool, target: u8) -> bool {
    target == first || (stereo && target == first + 1)
}

/// The link family that carries a mix's stereo flag.
///
/// Mixes 1-8 live in the MIX family. Mixes 9-16 reuse the S/PDIF family's link bits,
/// since block 3 is either S/PDIF or the second mixer but never both.
const fn mix_family(mix: u8) -> Family {
    if mix < 8 {
        Family::Mix
    } else {
        Family::SpdifOrMix2
    }
}

const fn mix_family_index(mix: u8) -> u8 {
    if mix < 8 { mix } else { mix - 8 }
}
