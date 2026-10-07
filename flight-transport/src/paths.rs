// SPDX-License-Identifier: MIT

//! gRPC method paths. They must match `service Flight` in `flight-proto/proto/flight.proto`
//! (checked by `tests/paths.rs`).

pub(crate) const SERVICE_NAME: &str = "flight.v1.Flight";
pub(crate) const NODE_CONNECT: &str = "/flight.v1.Flight/NodeConnect";
pub(crate) const UI_CONNECT: &str = "/flight.v1.Flight/UiConnect";
pub(crate) const ENROLL: &str = "/flight.v1.Flight/Enroll";
