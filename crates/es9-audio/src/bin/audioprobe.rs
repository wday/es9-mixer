//! Spike S0: can we open a multichannel capture stream on the ES-9, and how many
//! channels does the host actually expose?
//!
//! This decides the metering design. WASAPI needs no SDK; if it exposes all 16 capture
//! channels the ASIO build dependency disappears entirely, which is the best outcome in
//! `features/mixer-ui/plan.md` §0.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample};

use es9_meter::Meter;

fn main() {
    // `--hold <secs>` keeps the capture stream open for longer, so that another process
    // can ask the module over SysEx what sample rate it is running at *while* the host
    // holds the stream. That is how the re-clocking question gets settled.
    let args: Vec<String> = std::env::args().collect();
    let hold_secs: u64 = args
        .iter()
        .position(|a| a == "--hold")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);
    // ASIO lets the host choose the rate; WASAPI shared mode does not. Defaults to
    // 48 kHz so the difference is visible.
    let want_rate: u32 = args
        .iter()
        .position(|a| a == "--rate")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(48_000);

    println!("== hosts ==");
    for id in cpal::available_hosts() {
        println!("  {id:?}");
    }

    let mut es9_input = None;

    for id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(id) else {
            println!("\n-- host {id:?}: unavailable");
            continue;
        };
        println!("\n== host {id:?} : input devices ==");
        let devices = match host.input_devices() {
            Ok(d) => d,
            Err(e) => {
                println!("  could not enumerate: {e}");
                continue;
            }
        };
        for device in devices {
            let name = device.name().unwrap_or_else(|_| "?".into());
            println!("  device: {name}");
            match device.supported_input_configs() {
                Ok(configs) => {
                    let mut best = 0u16;
                    for c in configs {
                        println!(
                            "    channels {:>2}, {} - {} Hz, {:?}",
                            c.channels(),
                            c.min_sample_rate().0,
                            c.max_sample_rate().0,
                            c.sample_format()
                        );
                        best = best.max(c.channels());
                    }
                    // The WASAPI endpoint is called "ES-9"; the ASIO driver is called
                    // "Expert Sleepers USB Audio". Both are the same module.
                    if is_es9(&name) {
                        // Compared by name because `HostId::Asio` only exists on the
                        // Windows build with the asio feature enabled.
                        let prefer = es9_input
                            .as_ref()
                            .is_none_or(|(prev, _, _)| !is_asio(*prev) && is_asio(id));
                        println!("    ^ ES-9 capture, max {best} channels");
                        if prefer {
                            es9_input = Some((id, device, best));
                        }
                    }
                }
                Err(e) => println!("    no configs: {e}"),
            }
        }
    }

    let Some((host_id, device, max_channels)) = es9_input else {
        println!("\nNo ES-9 input device found on any host.");
        println!("S0: cannot open a capture stream at all through the available hosts.");
        return;
    };

    println!(
        "\n== S0: opening a capture stream on {host_id:?} / {} ==",
        device.name().unwrap_or_else(|_| "?".into())
    );

    // Prefer the most channels at the requested rate; fall back to the device default.
    let config = pick_config(&device, max_channels, want_rate).or_else(|| {
        println!("  no config at {want_rate} Hz; falling back to the device default");
        device.default_input_config().ok()
    });
    let Some(config) = config else {
        println!("  no usable input config");
        return;
    };
    println!(
        "  chosen config: {} channels @ {} Hz, {:?}",
        config.channels(),
        config.sample_rate().0,
        config.sample_format()
    );
    println!("  best advertised channel count: {max_channels}");

    let channels = usize::from(config.channels());
    let sample_rate = config.sample_rate().0;
    let meters = Arc::new(Mutex::new(vec![Meter::new(sample_rate, 512); channels]));
    let callbacks = Arc::new(AtomicU32::new(0));

    // The sample format is the driver's choice, so all three widths are handled and
    // converted to f32 for metering.
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let stream = match format {
        cpal::SampleFormat::F32 => {
            build::<f32>(&device, &stream_config, channels, &meters, &callbacks)
        }
        cpal::SampleFormat::I32 => {
            build::<i32>(&device, &stream_config, channels, &meters, &callbacks)
        }
        cpal::SampleFormat::I16 => {
            build::<i16>(&device, &stream_config, channels, &meters, &callbacks)
        }
        other => {
            println!("  unsupported sample format {other:?}");
            return;
        }
    };

    let stream = match stream {
        Ok(s) => s,
        Err(e) => {
            println!("  BUILD FAILED: {e}");
            println!("  S0: a capture stream could not be built.");
            return;
        }
    };
    if let Err(e) = stream.play() {
        println!("  PLAY FAILED: {e}");
        return;
    }

    println!("  stream running — holding for {hold_secs}s, make some noise...");
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(hold_secs) {
        std::thread::sleep(Duration::from_millis(200));
    }
    drop(stream);

    let n = callbacks.load(Ordering::Relaxed);
    println!("  {n} audio callbacks in {hold_secs}s");
    if let Ok(m) = meters.lock() {
        println!("  per-channel peak (dBFS):");
        for (i, meter) in m.iter().enumerate() {
            let db = linear_db_label(meter.hold);
            println!("    ch {:>2}: {db}", i + 1);
        }
    }

    println!("\n  S0 verdict:");
    if n == 0 {
        println!("    stream opened but delivered no audio — treat as a failure.");
    } else if channels >= 16 {
        println!("    {channels} channels @ {sample_rate} Hz via {host_id:?}.");
    } else {
        println!(
            "    only {channels} channels via {host_id:?} (device advertises {max_channels})."
        );
        println!("    Full 16-channel metering needs the ASIO backend.");
    }
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
        // Prefer a float format when the driver offers one; it needs no conversion.
        .max_by_key(|c| u8::from(c.sample_format() == cpal::SampleFormat::F32))
        .map(|c| c.with_sample_rate(rate))
}

/// Builds an input stream of one sample type, metering every channel.
fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    meters: &Arc<Mutex<Vec<Meter>>>,
    callbacks: &Arc<AtomicU32>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let meters = Arc::clone(meters);
    let callbacks = Arc::clone(callbacks);
    device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            callbacks.fetch_add(1, Ordering::Relaxed);
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
        |e| eprintln!("  stream error: {e}"),
        None,
    )
}

/// Whether a host is ASIO, without needing the ASIO-only enum variant to exist.
fn is_asio(id: cpal::HostId) -> bool {
    id.name().eq_ignore_ascii_case("asio")
}

/// Whether a device name denotes the ES-9, under either host's naming.
fn is_es9(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("es-9") || n.contains("es9") || n.contains("expert sleepers")
}

fn linear_db_label(v: f32) -> String {
    if v <= 0.0 {
        "  -inf".into()
    } else {
        format!("{:>6.1}", 20.0 * v.log10())
    }
}
