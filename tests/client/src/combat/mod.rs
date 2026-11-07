//! How a load-test bot fights once a match is on: the map's walk grid, line of sight, and the
//! decisions that turn a snapshot into input commands.

mod brain;
mod nav;
mod sight;

pub use brain::{Arena, Body, Brain, Me};
