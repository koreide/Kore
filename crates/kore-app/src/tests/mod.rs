//! Layer 1: the real UI in headless GPUI test windows against fake clusters.
//! See `crate::testing` for the driver. One module per area; each test names the
//! harness scenario (or spec section) whose behavior it pins.

mod cluster_admin;
mod cluster_views;
mod details;
mod shell_nav;
mod shell_table;
mod smoke;
