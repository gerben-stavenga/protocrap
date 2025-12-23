#![feature(allocator_api)]

use std::io::{self, Read, Write};
use anyhow::{Context, Result, bail};
use protocrap::ProtobufExt;
use protocrap_conformance::conformance::{ConformanceRequest, ConformanceResponse, WireFormat};
use protocrap_conformance::protobuf_test_messages::proto2::TestAllTypesProto2;
use protocrap_conformance::protobuf_test_messages::proto3::TestAllTypesProto3;

fn read_varint<R: Read>(reader: &mut R) -> Result<u64> {
    let mut result = 0u64;
    let mut shift = 0;

    loop {
        let mut byte = 0u8;
        reader.read_exact(std::slice::from_mut(&mut byte)).context("Failed to read varint byte")?;

        result |= ((byte & 0x7f) as u64) << shift;

        if byte & 0x80 == 0 {
            break;
        }

        shift += 7;
        if shift >= 64 {
            anyhow::bail!("Varint too long");
        }
    }

    Ok(result)
}

fn write_varint<W: Write>(writer: &mut W, mut value: u64) -> Result<()> {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;

        if value != 0 {
            byte |= 0x80;
        }

        writer.write_all(&[byte])?;

        if value == 0 {
            break;
        }
    }

    Ok(())
}

fn do_test(request: &ConformanceRequest::ProtoType, arena: &mut protocrap::arena::Arena) -> ConformanceResponse::ProtoType {
    let mut response = ConformanceResponse::ProtoType::default();

    // Get the input payload
    let input_bytes = if request.has_protobuf_payload() {
        request.protobuf_payload()
    } else if request.has_json_payload() {
        response.set_skipped("JSON input not supported", arena);
        return response;
    } else if request.has_text_payload() {
        response.set_skipped("Text format input not supported", arena);
        return response;
    } else {
        response.set_runtime_error("No input payload specified", arena);
        return response;
    };

    // Determine message type
    let message_type = request.message_type();
    let is_proto3 = message_type.contains("Proto3") || message_type.is_empty();

    // Get requested output format
    let output_format = request.requested_output_format()
        .unwrap_or(WireFormat::PROTOBUF);

    eprintln!("Test message type: {}, output format: {:?}", message_type, output_format);

    match output_format {
        WireFormat::PROTOBUF => {
            // Parse and re-serialize protobuf
            if is_proto3 {
                let mut msg = TestAllTypesProto3::ProtoType::default();
                if !msg.decode_flat::<32>(arena, input_bytes) {
                    response.set_parse_error("Failed to parse TestAllTypesProto3", arena);
                    return response;
                }
                match msg.encode_vec::<32>() {
                    Ok(bytes) => response.set_protobuf_payload(&bytes, arena),
                    Err(e) => response.set_serialize_error(&format!("Encode error: {:?}", e), arena),
                }
            } else {
                let mut msg = TestAllTypesProto2::ProtoType::default();
                if !msg.decode_flat::<32>(arena, input_bytes) {
                    response.set_parse_error("Failed to parse TestAllTypesProto2", arena);
                    return response;
                }
                match msg.encode_vec::<32>() {
                    Ok(bytes) => response.set_protobuf_payload(&bytes, arena),
                    Err(e) => response.set_serialize_error(&format!("Encode error: {:?}", e), arena),
                }
            }
        }
        WireFormat::JSON => {
            response.set_skipped("JSON output not supported", arena);
        }
        WireFormat::TEXT_FORMAT => {
            response.set_skipped("Text format output not supported", arena);
        }
        _ => {
            response.set_skipped(&format!("Output format {:?} not supported", output_format), arena);
        }
    }

    response
}

fn main() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();

    eprintln!("Protocrap conformance test runner starting...");

    let mut test_count = 0;

    loop {
        let mut arena = protocrap::arena::Arena::new(&std::alloc::Global);
        // Read message length
        use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
        let Ok(len) = stdin.read_u32::<byteorder::LittleEndian>().context("Failed to read message length") else {
            break;
        };
        // eprintln!("Processing test #{} ({} bytes)", test_count + 1, len);

        // Read request message
        let mut request_bytes = vec![0u8; len as usize];
        stdin.read_exact(&mut request_bytes)
            .context("Failed to read request")?;

        // Parse ConformanceRequest
        let mut request = ConformanceRequest::ProtoType::default();
        if !request.decode_flat::<32>(&mut arena, &request_bytes) {
            bail!("Failed to decode ConformanceRequest");
        }
        let response = do_test(&request, &mut arena);

        // Serialize ConformanceResponse
        let response_bytes = response.encode_vec::<32>()
            .context("Failed to encode response")?;

        // eprintln!("Response {:?} finished test #{}", response, test_count + 1);

        // Write response length and message
        stdout.write_u32::<LittleEndian>(response_bytes.len() as u32)?;
        stdout.write_all(&response_bytes)?;
        stdout.flush()?;

        test_count += 1;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint() {
        let mut buf = Vec::new();
        write_varint(&mut buf, 0).unwrap();
        assert_eq!(buf, vec![0]);

        buf.clear();
        write_varint(&mut buf, 127).unwrap();
        assert_eq!(buf, vec![127]);

        buf.clear();
        write_varint(&mut buf, 128).unwrap();
        assert_eq!(buf, vec![0x80, 0x01]);

        buf.clear();
        write_varint(&mut buf, 300).unwrap();
        assert_eq!(buf, vec![0xac, 0x02]);
    }

    #[test]
    fn test_varint_roundtrip() {
        let test_values = [0u64, 1, 127, 128, 255, 256, 16384, 1000000];

        for &value in &test_values {
            let mut buf = Vec::new();
            write_varint(&mut buf, value).unwrap();
            let mut cursor = &buf[..];
            let decoded = read_varint(&mut cursor).unwrap();
            assert_eq!(value, decoded, "Roundtrip failed for {}", value);
        }
    }
}
