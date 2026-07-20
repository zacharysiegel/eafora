//! The map surface: the wgpu canvas host, the region-detail panel, and (a later phase) the legend and
//! controls chrome.

pub mod canvas;
pub mod detail_panel;
pub mod map;

pub use map::*;
