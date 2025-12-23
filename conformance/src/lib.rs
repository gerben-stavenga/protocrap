use protocrap;

// Include all generated code from conformance_all.proto
// This includes conformance.proto, test_messages_proto2.proto, and test_messages_proto3.proto
include!(concat!(env!("OUT_DIR"), "/conformance_all.pc.rs"));
