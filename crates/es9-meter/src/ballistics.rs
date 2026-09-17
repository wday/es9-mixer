//! Peak and RMS metering with ballistics.
//!
//! Pure arithmetic over blocks of samples: no backend, no allocation, no I/O.

/// Per-channel meter state.
///
/// Attack is instantaneous and decay is exponential, which is the standard behaviour for
/// a mixing-console meter: transients must be visible, and the eye needs the fall to be
/// slow enough to read.
#[derive(Debug, Clone, Copy)]
pub struct Meter {
    /// Current displayed peak, linear.
    pub peak: f32,
    /// Current RMS, linear.
    pub rms: f32,
    /// Held peak, linear.
    pub hold: f32,
    /// Whether the channel has clipped since the hold was last reset.
    pub clipped: bool,
    /// Smoothed mean sample value: the channel's DC offset, in the same linear units.
    ///
    /// Peak and RMS both discard sign, so neither shows a constant offset. On a DC-coupled
    /// module where an idle output can sit at a non-zero voltage, that is exactly the
    /// thing worth seeing.
    pub dc: f32,
    decay: f32,
    rms_coeff: f32,
}

impl Meter {
    /// Creates a meter for a given sample rate and block size.
    pub fn new(sample_rate: u32, block: usize) -> Self {
        // Roughly 20 dB per second of fall, evaluated once per block.
        let blocks_per_second = sample_rate as f32 / block.max(1) as f32;
        let decay = (-2.3 / blocks_per_second.max(1.0)).exp();
        Self {
            peak: 0.0,
            rms: 0.0,
            hold: 0.0,
            clipped: false,
            dc: 0.0,
            decay,
            rms_coeff: 0.2,
        }
    }

    /// Folds one block of samples for a single channel into the meter.
    pub fn push(&mut self, samples: impl IntoIterator<Item = f32>) {
        let mut block_peak = 0.0f32;
        let mut sum_squares = 0.0f64;
        let mut sum = 0.0f64;
        let mut count = 0u32;
        for s in samples {
            let a = s.abs();
            if a > block_peak {
                block_peak = a;
            }
            sum_squares += f64::from(s) * f64::from(s);
            sum += f64::from(s);
            count += 1;
        }
        if count == 0 {
            return;
        }
        if block_peak >= 1.0 {
            self.clipped = true;
        }
        // Attack is immediate, decay is smoothed. The comparison has to be `>=` or a
        // steady tone droops: each block would decay off its own equal peak.
        self.peak = if block_peak >= self.peak {
            block_peak
        } else {
            self.peak * self.decay
        };
        if block_peak > self.hold {
            self.hold = block_peak;
        }
        let block_rms = (sum_squares / f64::from(count)).sqrt() as f32;
        self.rms += (block_rms - self.rms) * self.rms_coeff;
        // The mean is smoothed harder than the RMS: a DC reading is only useful if it is
        // steady enough to trim against, and audio content averages towards zero slowly.
        let block_mean = (sum / f64::from(count)) as f32;
        self.dc += (block_mean - self.dc) * 0.05;
    }

    /// Clears the held peak and the clip flag.
    pub fn reset_hold(&mut self) {
        self.hold = self.peak;
        self.clipped = false;
    }

    /// Peak in dBFS, or [`f32::NEG_INFINITY`] at silence.
    pub fn peak_db(&self) -> f32 {
        linear_to_db(self.peak)
    }

    /// RMS in dBFS, or [`f32::NEG_INFINITY`] at silence.
    pub fn rms_db(&self) -> f32 {
        linear_to_db(self.rms)
    }
}

/// Converts a linear amplitude to dBFS.
pub fn linear_to_db(v: f32) -> f32 {
    if v <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * v.log10()
    }
}
