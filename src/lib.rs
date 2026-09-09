//! Native Contract Net tasks, calibration, strategy, and WebSocket client.
pub mod benchmark;
pub mod bidder;
pub mod client;
pub mod market;
pub mod protocol;
mod random;
pub mod strategy;
pub mod tasks;

pub use client::{ClientConfig, Contractor};
pub use protocol::{Bid, Rules, Settlement, Task};
pub use strategy::{BidContext, MyContractor, Strategy};
pub use tasks::run_task;
