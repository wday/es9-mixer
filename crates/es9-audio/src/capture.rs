//! Opening the ES-9's capture stream and keeping meters fed from it.
//!
//! **ASIO, not WASAPI.** WASAPI shared mode can only run at the rate the Windows endpoint
//! is configured for — it refuses any other — and opening it re-clocks the module to that
//! rate. A configuration tool that silently changes the rig's sample rate to draw its
//! meters is worse than one with no meters, so the ASIO host is strongly preferred and
//! WASAPI is a last resort that reports what it is doing.
//!
//! `cpal::Stream` is not `Send`, so the stream lives on its own thread and only the
//! shared meter array crosses back.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};

use crate::meter::Meter;

/// Why a capture stream could not be opened.
#[derive(Debug, Clone)]
pub enum CaptureError {
    /// No host offered a device that looks like an ES-9.
    NoDevice,
    /// A device was found but offered no usable configuration.
    NoConfig(String),
    /// The stream could not be built or started.
    Stream(String),
}

impl core::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoDevice => write!(
                f,
                "no ES-9 audio device found; is the Expert Sleepers driver installed?"
            ),
            Self::NoConfig(e) => write!(f, "no usable input configuration: {e}"),
            Self::Stream(e) => write!(f, "could not start capture: {e}"),
        }
    }
}

impl core::error::Error for CaptureError {}

/// One channel's levels, in dBFS.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Levels {
    /// Current peak.
    pub peak_db: f32,
    /// Current RMS.
    pub rms_db: f32,
    /// Held peak.
    pub hold_db: f32,
    /// Whether the channel has clipped since the hold was last reset.
    pub clipped: bool,
    /// Mean sample value — the channel's DC offset, linear, signed.
    pub dc: f32,
}

/// A snapshot of every metered channel, with the stream it came from.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterSnapshot {
    /// Per-channel levels, in capture-channel order.
    pub channels: Vec<Levels>,
    /// Stream sample rate.
    pub sample_rate: u32,
    /// Host name, e.g. `"ASIO"`.
    pub host: String,
}

/// A running capture stream.
pub struct Capture {
    meters: Arc<Mutex<Vec<Meter>>>,
    stop: Arc<AtomicBool>,
    /// Channels being captured.
    pub channels: usize,
    /// Stream sample rate.
    pub sample_rate: u32,
    /// Host the stream was opened on.
    pub host: String,
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Capture {
    /// Opens the ES-9's capture stream, preferring ASIO and the given sample rate.
    pub fn open(preferred_rate: u32) -> Result<Self, CaptureError> {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);

        // The stream is not `Send`, so it is created and dropped entirely on this thread.
        std::thread::spawn(move || {
            let opened = open_stream(preferred_rate);
            match opened {
                Ok((stream, info)) => {
                    if let Err(e) = stream.play() {
                        let _ = tx.send(Err(CaptureError::Stream(e.to_string())));
                        return;
                    }
                    if tx.send(Ok(info)).is_err() {
                        return;
                    }
                    while !stop_thread.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    drop(stream);
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });

        let info = rx
            .recv()
            .map_err(|_| CaptureError::Stream("capture thread died".into()))??;

        Ok(Self {
            meters: info.meters,
            stop,
            channels: info.channels,
            sample_rate: info.sample_rate,
            host: info.host,
        })
    }

    /// Current levels for every channel.
    pub fn snapshot(&self) -> MeterSnapshot {
        let channels = match self.meters.lock() {
            Ok(m) => m
                .iter()
                .map(|meter| Levels {
                    peak_db: meter.peak_db(),
                    rms_db: meter.rms_db(),
                    hold_db: crate::meter::linear_to_db(meter.hold),
                    clipped: meter.clipped,
                    dc: meter.dc,
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        MeterSnapshot {
            channels,
            sample_rate: self.sample_rate,
            host: self.host.clone(),
        }
    }

    /// Clears every held peak and clip indicator.
    pub fn reset_holds(&self) {
        if let Ok(mut m) = self.meters.lock() {
            for meter in m.iter_mut() {
                meter.reset_hold();
            }
        }
    }
}

struct StreamInfo {
    meters: Arc<Mutex<Vec<Meter>>>,
    channels: usize,
    sample_rate: u32,
    host: String,
}

/// Whether a device name denotes the ES-9, under either host's naming.
///
/// The WASAPI endpoint is called "ES-9"; the ASIO driver is "Expert Sleepers USB Audio".
pub fn is_es9(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("es-9") || n.contains("es9") || n.contains("expert sleepers")
}

fn is_asio(id: cpal::HostId) -> bool {
    id.name().eq_ignore_ascii_case("asio")
}

fn open_stream(preferred_rate: u32) -> Result<(cpal::Stream, StreamInfo), CaptureError> {
    let mut best: Option<(cpal::HostId, cpal::Device, u16)> = None;

    for id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(id) else {
            continue;
        };
        let Ok(devices) = host.input_devices() else {
            continue;
        };
        for device in devices {
            let name = device.name().unwrap_or_default();
            if !is_es9(&name) {
                continue;
            }
            let channels = device
                .supported_input_configs()
                .map(|cs| cs.map(|c| c.channels()).max().unwrap_or(0))
                .unwrap_or(0);
            // ASIO wins outright: it is the only host that will not re-clock the module.
            let better = match &best {
                None => true,
                Some((prev_id, _, prev_channels)) => {
                    (is_asio(id) && !is_asio(*prev_id)) || channels > *prev_channels
                }
            };
            if better {
                best = Some((id, device, channels));
            }
        }
    }

    let (host_id, device, max_channels) = best.ok_or(CaptureError::NoDevice)?;

    let config = pick_config(&device, max_channels, preferred_rate)
        .or_else(|| device.default_input_config().ok())
        .ok_or_else(|| CaptureError::NoConfig("device offered none".into()))?;

    let channels = usize::from(config.channels());
    let sample_rate = config.sample_rate().0;
    let meters = Arc::new(Mutex::new(vec![Meter::new(sample_rate, 512); channels]));
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();

    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &stream_config, channels, &meters),
        cpal::SampleFormat::I32 => build::<i32>(&device, &stream_config, channels, &meters),
        cpal::SampleFormat::I16 => build::<i16>(&device, &stream_config, channels, &meters),
        other => Err(CaptureError::NoConfig(format!(
            "unsupported sample format {other:?}"
        ))),
    }?;

    Ok((
        stream,
        StreamInfo {
            meters,
            channels,
            sample_rate,
            host: host_id.name().to_string(),
        },
    ))
}

/// Picks the widest config at `want_rate`, if the device offers one.
fn pick_config(
    device: &cpal::Device,
    max_channels: u16,
    want_rate: u32,
) -> Option<cpal::SupportedStreamConfig> {
    let rate = cpal::SampleRate(want_rate);
    device
        .supported_input_configs()
        .ok()?
        .filter(|c| {
            c.channels() == max_channels
                && c.min_sample_rate() <= rate
                && rate <= c.max_sample_rate()
        })
        .max_by_key(|c| u8::from(c.sample_format() == cpal::SampleFormat::F32))
        .map(|c| c.with_sample_rate(rate))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    meters: &Arc<Mutex<Vec<Meter>>>,
) -> Result<cpal::Stream, CaptureError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let meters = Arc::clone(meters);
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                if let Ok(mut m) = meters.lock() {
                    for (ch, meter) in m.iter_mut().enumerate() {
                        meter.push(
                            data.iter()
                                .skip(ch)
                                .step_by(channels)
                                .map(|s| f32::from_sample(*s)),
                        );
                    }
                }
            },
            |e| eprintln!("capture stream error: {e}"),
            None,
        )
        .map_err(|e| CaptureError::Stream(e.to_string()))
}
