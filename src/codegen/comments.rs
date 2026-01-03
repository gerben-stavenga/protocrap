// Comment extraction from SourceCodeInfo
//
// Builds a map from dotted name paths to comment strings by walking
// the numeric paths in SourceCodeInfo.Location using reflection.

use std::collections::HashMap;

use super::protocrap;
use protocrap::ProtobufRef;
use protocrap::google::protobuf::FieldDescriptorProto::Label;
use protocrap::google::protobuf::FileDescriptorProto::ProtoType as FileDescriptorProto;
use protocrap::reflection::{DynamicMessageRef, Value};

/// Extract comments from a FileDescriptorProto's source_code_info.
/// Returns a map from dotted name path (e.g., "MyMessage.my_field") to comment string.
pub fn extract_comments(file: &FileDescriptorProto) -> HashMap<String, String> {
    let mut comments = HashMap::new();

    let Some(source_code_info) = file.source_code_info() else {
        return comments;
    };

    for location in source_code_info.location() {
        // Get comment: prefer leading, fall back to trailing
        let comment = location
            .get_leading_comments()
            .or_else(|| location.get_trailing_comments());

        let Some(comment) = comment else {
            continue;
        };

        let path = location.path();
        if path.is_empty() {
            continue;
        }

        // Walk the path to resolve the name
        if let Some(name_path) = walk_path(file.as_dyn(), path) {
            let trimmed = trim_comment(comment);
            if !trimmed.is_empty() {
                comments.insert(name_path, trimmed);
            }
        }
    }

    comments
}

/// Walk a SourceCodeInfo path through the descriptor tree using reflection.
/// Returns the dotted name path (e.g., "MyMessage.nested_field").
fn walk_path(start: DynamicMessageRef, path: &[i32]) -> Option<String> {
    let mut current = start;
    let mut name_parts: Vec<String> = Vec::new();
    let mut i = 0;

    while i < path.len() {
        let field_num = path[i];
        i += 1;

        let field_desc = current.find_field_descriptor_by_number(field_num)?;
        let is_repeated = field_desc.label() == Some(Label::LABEL_REPEATED);

        let value = current.get_field(field_desc);

        if is_repeated {
            // Next element should be an index
            if i >= path.len() {
                break;
            }
            let index = path[i] as usize;
            i += 1;

            if let Some(Value::RepeatedMessage(array)) = value {
                if index >= array.len() {
                    return None;
                }
                current = array.get(index);

                // Try to extract "name" field from this message
                if let Some(name) = get_name_field(&current) {
                    name_parts.push(name);
                }
            } else {
                // Repeated non-message field, we're done
                break;
            }
        } else {
            // Singular field
            if let Some(Value::Message(msg)) = value {
                current = msg;
            } else {
                // Scalar field or unset message, path ends here
                break;
            }
        }
    }

    if name_parts.is_empty() {
        None
    } else {
        Some(name_parts.join("."))
    }
}

/// Extract the "name" field from a message if it has one.
fn get_name_field(msg: &DynamicMessageRef) -> Option<String> {
    let field_desc = msg.find_field_descriptor("name")?;
    if let Some(Value::String(s)) = msg.get_field(field_desc) {
        Some(s.to_string())
    } else {
        None
    }
}

/// Trim and clean up a comment string.
fn trim_comment(comment: &str) -> String {
    // Remove leading/trailing whitespace from each line and rejoin
    comment
        .lines()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_comments_from_descriptor() {
        // Use the descriptor.proto file descriptor which now has source_code_info
        let file_desc = protocrap::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();

        assert!(file_desc.source_code_info().is_some(), "descriptor should have source_code_info");

        let comments = extract_comments(file_desc);

        // descriptor.proto has lots of comments
        assert!(comments.len() > 100, "Should have extracted many comments, got {}", comments.len());

        // Check for some known types
        assert!(comments.contains_key("FileDescriptorProto"), "Should have FileDescriptorProto");
        assert!(comments.contains_key("DescriptorProto"), "Should have DescriptorProto");
        assert!(comments.contains_key("FieldDescriptorProto"), "Should have FieldDescriptorProto");

        // Check for nested types
        assert!(comments.contains_key("FieldDescriptorProto.Type.TYPE_DOUBLE"), "Should have enum value comment");

        // Check for fields
        assert!(comments.contains_key("FileDescriptorProto.name"), "Should have field comment");

        // Verify comment content
        let desc_comment = comments.get("DescriptorProto").unwrap();
        assert!(desc_comment.contains("message type"), "DescriptorProto comment should mention 'message type'");
    }

    #[test]
    fn dump_comments_to_file() {
        let file_desc = protocrap::google::protobuf::FileDescriptorProto::ProtoType::file_descriptor();
        let comments = extract_comments(file_desc);

        let mut output = String::new();
        let mut keys: Vec<_> = comments.keys().collect();
        keys.sort();

        for key in keys {
            let comment = comments.get(key).unwrap();
            output.push_str(&format!("=== {} ===\n{}\n\n", key, comment));
        }

        std::fs::write("comments_map.txt", &output).unwrap();
        println!("Wrote {} comments to comments_map.txt", comments.len());
    }
}
