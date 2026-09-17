//! Audio capture for the ES-9, over ASIO.
//!
//! This crate is the backend edge only: opening the module's stream and keeping meters
//! fed from it. The ballistics themselves are in `es9-meter`, which links nothing and is
//! MIT — the split is what keeps a host that cannot carry GPLv3 code able to meter.

#![forbid(unsafe_code)]

pub mod capture;
