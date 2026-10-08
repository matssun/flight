// SPDX-License-Identifier: MIT

//! The hand-bound gRPC paths must be the ones `service Flight` declares.

#[test]
fn the_service_in_the_proto_declares_the_paths_the_transport_serves() {
    let proto = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../flight-proto/proto/flight.proto"
    ))
    .expect("proto");
    assert!(proto.contains("package flight.v1;"));
    assert!(proto.contains("service Flight {"));
    for rpc in [
        "rpc NodeConnect(stream NodeFrame) returns (stream OrchestratorFrame);",
        "rpc UiConnect(stream UiRequest) returns (stream UiEvent);",
        "rpc TerminalNode(stream TerminalFrame) returns (stream TerminalFrame);",
        "rpc TerminalUi(stream TerminalFrame) returns (stream TerminalFrame);",
        "rpc Enroll(EnrollRequest) returns (EnrollResponse);",
    ] {
        assert!(proto.contains(rpc), "{rpc}");
    }
}
