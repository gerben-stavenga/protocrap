use anyhow::{Context, Result, bail};
use protocrap::proto_json::{ProtoJsonDeserializer, ProtoJsonSerializer};
use protocrap::descriptor_pool::DescriptorPool;
use protocrap::{ProtobufMut, ProtobufRef};
use protocrap_conformance::{GLOBAL_ALLOC, load_descriptor_pool};
use serde::Serialize;
use test_protos::conformance::{ConformanceRequest, ConformanceResponse, WireFormat};
use test_protos::protobuf_test_messages::proto2::TestAllTypesProto2;
use test_protos::protobuf_test_messages::proto3::TestAllTypesProto3;
use std::io::{self, Read, Write};

const TEST_JSON: bool = true;

fn roundtrip_proto<'pool, T: protocrap::ProtobufMut<'pool>>(
    msg: &mut T,
    arena: &mut protocrap::arena::Arena,
    request: &ConformanceRequest::ProtoType,
) -> ConformanceResponse::ProtoType {
    let mut response = ConformanceResponse::ProtoType::default();
    // Decode input
    if let Some(data) = request.get_protobuf_payload() {
        if !msg.decode_flat::<32>(arena, data) {
            eprintln!("Parse error");
            response.set_parse_error(
                &format!(
                    "Failed to decode message of type '{}'",
                    msg.descriptor().name()
                ),
                arena,
            ).unwrap();
            return response;
        }
    } else if let Some(data) = request.get_json_payload() {
        if !TEST_JSON {
            response.set_skipped("Json format input not supported", arena).unwrap();
            return response;
        }
        let mut inner = serde_json::Deserializer::from_str(data);
        if let Err(e) = msg.serde_deserialize(arena, ProtoJsonDeserializer::new(&mut inner))
        {
            response.set_parse_error(&format!("Failed to parse JSON message: {:?}", e), arena).unwrap();
            return response;
        }
    } else if request.has_text_payload() {
        response.set_skipped("Text format input not supported", arena).unwrap();
        return response;
    } else if request.has_jspb_payload() {
        response.set_skipped("JSPB input not supported", arena).unwrap();
        return response;
    } else {
        response.set_runtime_error("No input payload specified", arena).unwrap();
        return response;
    };
    // Encode output
    match request.requested_output_format() {
        Some(WireFormat::PROTOBUF) => match msg.encode_vec::<32>() {
            Ok(bytes) => {
                response.set_protobuf_payload(&bytes, arena).unwrap();
            }
            Err(e) => {
                response.set_serialize_error(&format!("Encode error: {:?}", e), arena).unwrap();
            }
        },
        Some(WireFormat::JSON) => {
            if !TEST_JSON {
                response.set_skipped("Json format output not supported", arena).unwrap();
                return response;
            }
            let mut inner = serde_json::Serializer::new(Vec::new());
            let dynamic_msg = msg.as_dyn();
            match dynamic_msg.serialize(ProtoJsonSerializer::new(&mut inner)) {
                Ok(()) => {
                    let vec = inner.into_inner();
                    let json_str = String::from_utf8(vec).unwrap_or_default();
                    response.set_json_payload(&json_str, arena).unwrap();
                }
                Err(e) => {
                    response.set_serialize_error(&format!("JSON serialize error: {:?}", e), arena).unwrap();
                }
            }
        }
        Some(WireFormat::TEXT_FORMAT) => {
            response.set_skipped("Text format output not supported", arena).unwrap();
        }
        Some(WireFormat::JSPB) => {
            response.set_skipped("JSPB output not supported", arena).unwrap();
        }
        None | Some(WireFormat::UNSPECIFIED) => {
            response.set_skipped("Output format unspecified", arena).unwrap();
        }
    }
    response
}

fn do_test_dynamic(
    pool: &DescriptorPool<'static>,
    request: &ConformanceRequest::ProtoType,
    arena: &mut protocrap::arena::Arena,
) -> ConformanceResponse::ProtoType {
    let mut response = ConformanceResponse::ProtoType::default();
    let message_type = request.message_type();

    // Decode input using per-test arena
    let mut msg = match pool.create_message(message_type, arena) {
        Ok(m) => m,
        Err(e) => {
            response.set_skipped(
                &format!(
                    "Message type '{}' not found in descriptor pool : {:?}",
                    message_type, e
                ),
                arena,
            ).unwrap();
            return response;
        }
    };
    roundtrip_proto(&mut msg, arena, request)
}

fn do_test(
    request: &ConformanceRequest::ProtoType,
    arena: &mut protocrap::arena::Arena,
) -> ConformanceResponse::ProtoType {
    let mut response = ConformanceResponse::ProtoType::default();

    let message_type = request.message_type();
    let is_proto3 = message_type.contains("Proto3") || message_type.is_empty();

    if is_proto3 {
        let mut msg = TestAllTypesProto3::ProtoType::default();
        roundtrip_proto::<TestAllTypesProto3::ProtoType>(&mut msg, arena, request)
    } else {
        if message_type != "protobuf_test_messages.proto2.TestAllTypesProto2" {
            response.set_skipped(
                &format!("Message type {} not supported", message_type),
                arena,
            ).unwrap();
            return response;
        }
        let mut msg = TestAllTypesProto2::ProtoType::default();
        roundtrip_proto::<TestAllTypesProto2::ProtoType>(&mut msg, arena, request)
    }
}

fn main() -> Result<()> {
    use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

    let args: Vec<String> = std::env::args().collect();
    let use_dynamic = args.contains(&"--dynamic".to_string());

    let pool = if use_dynamic {
        eprintln!("Protocrap conformance test runner starting (DYNAMIC MODE)...");
        Some(load_descriptor_pool()?)
    } else {
        eprintln!("Protocrap conformance test runner starting...");
        None
    };

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();

    let mut count = 0;

    loop {
        let mut arena = protocrap::arena::Arena::new(&GLOBAL_ALLOC);

        // Read message length
        let Ok(len) = stdin
            .read_u32::<LittleEndian>()
            .context("Failed to read message length")
        else {
            break;
        };
        count += 1;
        eprintln!("--- Test #{}: reading {} bytes ---", count, len);

        // Read request message
        let mut request_bytes = vec![0u8; len as usize];
        stdin
            .read_exact(&mut request_bytes)
            .context("Failed to read request")?;

        // Parse ConformanceRequest
        let mut request = ConformanceRequest::ProtoType::default();
        if !request.decode_flat::<32>(&mut arena, &request_bytes) {
            bail!("Failed to decode ConformanceRequest");
        }
        eprintln!("Request is {:?}", &request);

        // Process test
        let response = if let Some(ref pool) = pool {
            do_test_dynamic(pool, &request, &mut arena)
        } else {
            do_test(&request, &mut arena)
        };

        // Serialize ConformanceResponse
        let response_bytes = response
            .encode_vec::<32>()
            .context("Failed to encode response")?;

        // Write response length and message
        stdout.write_u32::<LittleEndian>(response_bytes.len() as u32)?;
        stdout.write_all(&response_bytes)?;
        stdout.flush()?;

        eprintln!("Request done");
    }

    Ok(())
}
