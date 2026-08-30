//! ES-9 device model: state, actions, undo, and an offline mock.
//!
//! Pure logic with no I/O, so the whole application can be developed and tested with no
//! hardware attached. Transport lives in a separate crate.

#![forbid(unsafe_code)]

pub mod action;
pub mod mixer_defaults;
pub mod mock;
pub mod monitor;
pub mod state;
pub mod throttle;
pub mod view;

pub use action::{Action, Applied, Session};
pub use mock::MockEs9;
pub use monitor::{Direction, Entry, Monitor};
pub use state::{DeviceState, EditTarget};
