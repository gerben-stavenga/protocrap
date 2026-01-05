use protocrap;

include!("generated.pc.rs");

pub const DESCRIPTOR_BYTES: &[u8] = include_bytes!(env!("TEST_PROTOS_DESCRIPTOR_SET"));

// Test message builders
pub fn make_small() -> Test::ProtoType {
    let mut msg = Test::ProtoType::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg
}

pub fn make_medium(arena: &mut protocrap::arena::Arena) -> Test::ProtoType {
    let mut msg = Test::ProtoType::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg.set_z(
        "Hello World! This is a test string with some content.",
        arena,
    );
    let child1 = msg.child1_mut(arena);
    child1.set_x(123);
    child1.set_y(456);
    msg
}

pub fn make_large(arena: &mut protocrap::arena::Arena) -> Test::ProtoType {
    use protocrap::containers::Bytes;
    let mut msg = Test::ProtoType::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg.set_z("Hello World!", arena);
    for i in 0..100 {
        let nested_msg = msg.add_nested_message(arena);
        nested_msg.set_x(i);
    }
    for i in 0..5 {
        msg.rep_bytes_mut().push(
            Bytes::from_slice(format!("byte array number {}", i).as_bytes(), arena).unwrap(),
            arena,
        );
    }
    msg
}
