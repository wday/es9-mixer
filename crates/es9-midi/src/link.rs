//! Opening and driving a MIDI connection to the ES-9.

use std::sync::mpsc::{Receiver, Sender, channel};

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

use crate::assembler::SysexAssembler;
use es9_device::monitor::{Direction, Monitor};

const CLIENT_NAME: &str = "es9-mixer";

/// Anything that can go wrong talking to a MIDI port.
#[derive(Debug)]
pub enum MidiError {
    /// The MIDI subsystem could not be initialised.
    Init(String),
    /// The requested port index does not exist.
    NoSuchPort {
        /// The index that was asked for.
        index: usize,
    },
    /// The port could not be opened.
    Connect(String),
    /// A message could not be sent.
    Send(String),
}

impl core::fmt::Display for MidiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Init(e) => write!(f, "could not initialise MIDI: {e}"),
            Self::NoSuchPort { index } => write!(f, "no MIDI port at index {index}"),
            Self::Connect(e) => write!(f, "could not open MIDI port: {e}"),
            Self::Send(e) => write!(f, "could not send MIDI: {e}"),
        }
    }
}

impl core::error::Error for MidiError {}

/// A MIDI port that can be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// Index to pass to [`MidiLink::open`].
    pub index: usize,
    /// Human-readable port name.
    pub name: String,
}

/// Lists the available input and output ports.
pub fn list_ports() -> Result<(Vec<PortInfo>, Vec<PortInfo>), MidiError> {
    let input = MidiInput::new(CLIENT_NAME).map_err(|e| MidiError::Init(e.to_string()))?;
    let output = MidiOutput::new(CLIENT_NAME).map_err(|e| MidiError::Init(e.to_string()))?;

    let ins = input
        .ports()
        .iter()
        .enumerate()
        .map(|(index, p)| PortInfo {
            index,
            name: input.port_name(p).unwrap_or_else(|_| "?".to_string()),
        })
        .collect();
    let outs = output
        .ports()
        .iter()
        .enumerate()
        .map(|(index, p)| PortInfo {
            index,
            name: output.port_name(p).unwrap_or_else(|_| "?".to_string()),
        })
        .collect();
    Ok((ins, outs))
}

/// Picks the port most likely to be an ES-9.
///
/// Port names vary by platform and driver, so this matches loosely and the caller should
/// still let the user override it.
pub fn find_es9(ports: &[PortInfo]) -> Option<&PortInfo> {
    ports.iter().find(|p| {
        let n = p.name.to_ascii_lowercase().replace(['-', ' ', '_'], "");
        n.contains("es9")
    })
}

/// An open connection to the module.
///
/// Every message in both directions passes through [`MidiLink::send`] and
/// [`MidiLink::poll`], so the monitor sees all traffic by construction.
pub struct MidiLink {
    // Held to keep the callback alive; dropping it closes the port.
    _input: MidiInputConnection<Sender<Vec<u8>>>,
    output: MidiOutputConnection,
    incoming: Receiver<Vec<u8>>,
    monitor: Monitor,
    started: std::time::Instant,
}

impl MidiLink {
    /// Opens an input and an output port by index.
    pub fn open(input_index: usize, output_index: usize) -> Result<Self, MidiError> {
        let mut input = MidiInput::new(CLIENT_NAME).map_err(|e| MidiError::Init(e.to_string()))?;
        // Without this, SysEx is filtered out and nothing works.
        input.ignore(Ignore::None);
        let output = MidiOutput::new(CLIENT_NAME).map_err(|e| MidiError::Init(e.to_string()))?;

        let in_ports = input.ports();
        let in_port = in_ports
            .get(input_index)
            .ok_or(MidiError::NoSuchPort { index: input_index })?
            .clone();
        let out_ports = output.ports();
        let out_port = out_ports
            .get(output_index)
            .ok_or(MidiError::NoSuchPort {
                index: output_index,
            })?
            .clone();

        let (tx, incoming) = channel();
        let mut assembler = SysexAssembler::new();

        let connection = input
            .connect(
                &in_port,
                "es9-in",
                move |_timestamp, bytes, sender: &mut Sender<Vec<u8>>| {
                    for message in assembler.push(bytes) {
                        // A closed receiver means the link is being torn down.
                        let _ = sender.send(message);
                    }
                },
                tx,
            )
            .map_err(|e| MidiError::Connect(e.to_string()))?;

        let output = output
            .connect(&out_port, "es9-out")
            .map_err(|e| MidiError::Connect(e.to_string()))?;

        Ok(Self {
            _input: connection,
            output,
            incoming,
            monitor: Monitor::default(),
            started: std::time::Instant::now(),
        })
    }

    /// Sends one message, logging it.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), MidiError> {
        let at = self.elapsed_ms();
        self.monitor.record(Direction::Tx, bytes, at);
        self.output
            .send(bytes)
            .map_err(|e| MidiError::Send(e.to_string()))
    }

    /// Sends several messages in order, stopping at the first failure.
    pub fn send_all(&mut self, messages: &[Vec<u8>]) -> Result<(), MidiError> {
        for m in messages {
            self.send(m)?;
        }
        Ok(())
    }

    /// Takes any complete messages that have arrived, logging them.
    ///
    /// Non-blocking: returns an empty vector when nothing is waiting.
    pub fn poll(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(message) = self.incoming.try_recv() {
            let at = self.elapsed_ms();
            self.monitor.record(Direction::Rx, &message, at);
            out.push(message);
        }
        out
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// The traffic log.
    pub fn monitor(&self) -> &Monitor {
        &self.monitor
    }

    /// The traffic log, mutably, for clearing.
    pub fn monitor_mut(&mut self) -> &mut Monitor {
        &mut self.monitor
    }
}
