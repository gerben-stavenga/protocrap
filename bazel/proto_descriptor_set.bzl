"""Rule to generate a merged descriptor set with all transitive imports."""

def _proto_descriptor_set_impl(ctx):
    # Collect all transitive sources
    transitive_sources = depset(transitive = [
        dep[ProtoInfo].transitive_sources for dep in ctx.attr.deps
    ])

    # Collect only the canonical import paths (transitive_proto_path has the right roots)
    # Use a depset to properly dedupe
    import_paths = depset(transitive = [
        dep[ProtoInfo].transitive_proto_path for dep in ctx.attr.deps
    ])

    output = ctx.actions.declare_file(ctx.label.name + ".bin")

    # Build protoc command with --include_source_info
    protoc = ctx.executable._protoc

    args = ctx.actions.args()
    args.add("--include_source_info")
    args.add("--include_imports")
    args.add("--descriptor_set_out", output)

    # Add import paths (each prefixed with --proto_path)
    args.add_all(import_paths, format_each = "--proto_path=%s")

    # Add only direct source files from each dep (not transitive)
    # This avoids duplicate file issues while still having import paths for resolution
    direct_sources = []
    for dep in ctx.attr.deps:
        direct_sources.extend(dep[ProtoInfo].direct_sources)
    args.add_all(direct_sources)

    ctx.actions.run(
        executable = protoc,
        arguments = [args],
        inputs = transitive_sources,
        outputs = [output],
        mnemonic = "ProtoDescriptorSet",
        progress_message = "Generating descriptor set with source info",
    )

    return [DefaultInfo(files = depset([output]))]

proto_descriptor_set = rule(
    implementation = _proto_descriptor_set_impl,
    attrs = {
        "deps": attr.label_list(
            providers = [ProtoInfo],
            doc = "proto_library targets to include",
        ),
        "_protoc": attr.label(
            default = "@protobuf//:protoc",
            executable = True,
            cfg = "exec",
        ),
    },
    doc = "Generates a merged descriptor set with source info and all transitive imports.",
)
