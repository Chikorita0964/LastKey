//! Agent inspection for the settings window: the two-step gate and its
//! loopback default.
//!
//! Inspection is an out-of-band debug channel into the running UI: an external
//! client speaks the `egui_inspection` request/response protocol over TCP to
//! read the AccessKit tree (`GetTree`), inject input (`HandleEvents`), capture
//! a screenshot, or resize the window (`egui_inspection` 0.36.2 -- the wire
//! protocol plus the `InspectionPlugin` eframe attaches).
//!
//! # Two-step gate
//!
//! 1. **Build time**: the `agent-inspection` Cargo feature enables
//!    `eframe/inspection`. eframe then attaches `egui_inspection`'s plugin in
//!    its wgpu integration (`eframe::maybe_attach_inspection_plugin`), so no
//!    app code participates. A default `cargo build` does not compile the
//!    plugin in at all.
//! 2. **Run time**: `EGUI_INSPECTION` must be set to a truthy value or a bind
//!    address. `1` (or any other truthy value) binds the default
//!    `127.0.0.1:5719`; an address-shaped value binds that address instead
//!    (`EGUI_INSPECTION=0.0.0.0:5719` exposes the app across the network);
//!    unset, `0`, or `false` keeps inspection completely off
//!    (production-safe).
//!
//! Neither step alone opens the port. CI and the release gate run without the
//! feature, which is what keeps `egui_inspection` out of shipped binaries.
//!
//! # Loopback default
//!
//! The default bind is loopback only. Binding a non-loopback address gives
//! anyone who can reach the port full control of the app and its screenshots
//! with **no authentication** (`egui_inspection` logs a warning when that
//! happens); prefer loopback plus an SSH tunnel for remote debugging, and
//! never enable `agent-inspection` for a release build.
//!
//! # Reading the tree
//!
//! `egui_mcp` 0.2.0 is the MCP server for this protocol: it attaches to the
//! inspection port and exposes `query_tree` / `get_node`, input injection,
//! screenshots, resize, and `wait_for`. Tree reads and input injection work
//! while the window is in the background; screenshots need a rendered frame
//! and time out on a fully occluded window.

/// True when this build enabled `agent-inspection`.
///
/// The runtime gate is `EGUI_INSPECTION`; both steps must be satisfied before
/// the inspection port opens. See the module docs for the exact contract.
pub const BUILD_ENABLED: bool = cfg!(feature = "agent-inspection");
