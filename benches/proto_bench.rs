use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use prost::Message;

// Your crate
use protocrap::{ProtobufExt, test::Test};

// Prost generated (you'll need to set this up)
pub mod prost_gen {
    include!(concat!(env!("OUT_DIR"), "/_.rs"));
}

// Small message: just scalars
fn make_small_protocrap() -> Test {
    let mut msg = Test::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg
}

fn make_small_prost() -> prost_gen::Test {
    prost_gen::Test {
        x: Some(42),
        y: Some(0xDEADBEEF),
        z: None,
        child1: None,
        child2: None,
        nested_message: vec![],
    }
}

// Medium message: scalars + bytes + one child
fn make_medium_protocrap() -> Test {
    let mut msg = Test::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg.set_z(b"Hello World! This is a test string with some content.");
    let child = msg.child1_mut();
    child.set_x(123);
    child.set_y(456);
    msg
}

fn make_medium_prost() -> prost_gen::Test {
    prost_gen::Test {
        x: Some(42),
        y: Some(0xDEADBEEF),
        z: Some(b"Hello World! This is a test string with some content.".to_vec()),
        child1: Some(Box::new(prost_gen::Test {
            x: Some(123),
            y: Some(456),
            z: None,
            child1: None,
            child2: None,
            nested_message: vec![],
        })),
        child2: None,
        nested_message: vec![],
    }
}

// Large message: repeated nested messages
fn make_large_protocrap() -> Test {
    let mut msg = Test::default();
    msg.set_x(42);
    msg.set_y(0xDEADBEEF);
    msg.set_z(b"Hello World!");
    
    for _ in 0..100 {
        let nested = msg.nested_message_mut();
        // You'll need to adjust this based on your actual API
        // nested.push(...) or similar
    }
    msg
}

fn make_large_prost() -> prost_gen::Test {
    prost_gen::Test {
        x: Some(42),
        y: Some(0xDEADBEEF),
        z: Some(b"Hello World!".to_vec()),
        child1: None,
        child2: None,
        nested_message: (0..100).map(|i| {
            prost_gen::test::NestedMessage {
                x: Some(i),
                recursive: None,
            }
        }).collect(),
    }
}

fn encode_prost(msg: &prost_gen::Test) -> Vec<u8> {
    let mut buf = Vec::with_capacity(msg.encoded_len());
    msg.encode(&mut buf).unwrap();
    buf
}

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    
    // Small message
    let small_data = encode_prost(&make_small_prost());
    group.throughput(Throughput::Bytes(small_data.len() as u64));
    
    group.bench_function("small/protocrap", |b| {
        b.iter(|| {
            let mut msg = Test::default();
            msg.parse_flat::<32>(black_box(&small_data));
            std::mem::forget(msg); // Don't measure drop
            black_box(())
        })
    });
    
    group.bench_function("small/prost", |b| {
        b.iter(|| {
            let msg = prost_gen::Test::decode(black_box(small_data.as_slice())).unwrap();
            black_box(msg)
        })
    });
    
    // Medium message
    let medium_data = encode_prost(&make_medium_prost());
    group.throughput(Throughput::Bytes(medium_data.len() as u64));
    
    group.bench_function("medium/protocrap", |b| {
        b.iter(|| {
            let mut msg = Test::default();
            msg.parse_flat::<32>(black_box(&medium_data));
            std::mem::forget(msg);
            black_box(())
        })
    });
    
    group.bench_function("medium/prost", |b| {
        b.iter(|| {
            let msg = prost_gen::Test::decode(black_box(medium_data.as_slice())).unwrap();
            black_box(msg)
        })
    });
    
    // Large message
    let large_data = encode_prost(&make_large_prost());
    group.throughput(Throughput::Bytes(large_data.len() as u64));
    
    group.bench_function("large/protocrap", |b| {
        b.iter(|| {
            let mut msg = Test::default();
            msg.parse_flat::<32>(black_box(&large_data));
            std::mem::forget(msg);
            black_box(())
        })
    });
    
    group.bench_function("large/prost", |b| {
        b.iter(|| {
            let msg = prost_gen::Test::decode(black_box(large_data.as_slice())).unwrap();
            black_box(msg)
        })
    });
    
    group.finish();
}

fn bench_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("encode");
    
    // Small
    let mut small_protocrap = make_small_protocrap();
    let small_prost = make_small_prost();
    
    group.bench_function("small/protocrap", |b| {
        let mut buf = vec![0u8; 256];
        b.iter(|| {
            let result = small_protocrap.encode_flat::<32>(black_box(&mut buf)).unwrap();
            black_box(result.len())
        })
    });
    
    group.bench_function("small/prost", |b| {
        let mut buf = Vec::with_capacity(256);
        b.iter(|| {
            buf.clear();
            small_prost.encode(black_box(&mut buf)).unwrap();
            black_box(buf.len())
        })
    });
    
    // Medium
    let mut medium_protocrap = make_medium_protocrap();
    let medium_prost = make_medium_prost();
    
    group.bench_function("medium/protocrap", |b| {
        let mut buf = vec![0u8; 256];
        b.iter(|| {
            let result = medium_protocrap.encode_flat::<32>(black_box(&mut buf)).unwrap();
            black_box(result.len())
        })
    });
    
    group.bench_function("medium/prost", |b| {
        let mut buf = Vec::with_capacity(256);
        b.iter(|| {
            buf.clear();
            medium_prost.encode(black_box(&mut buf)).unwrap();
            black_box(buf.len())
        })
    });
    
    group.finish();
}

criterion_group!(benches, bench_parse, bench_encode);
criterion_main!(benches);