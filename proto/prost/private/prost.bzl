"""Rules for building protos in Rust with Prost and Tonic."""

load("@rules_proto//proto:defs.bzl", "ProtoInfo", "proto_common")
load("//rust:defs.bzl", "rust_common")

# buildifier: disable=bzl-visibility
load("//rust/private:providers.bzl", "DepVariantInfo")

# buildifier: disable=bzl-visibility
load("//rust/private:rustc.bzl", "rustc_compile_action")
load(":providers.bzl", "ProstProtoInfo", "TonicProtoInfo")

PROST_EXTENSION = ".rs"
TONIC_EXTENSION = ".tonic.rs"

RUST_EDITION = "2021"

def _compile_proto(ctx, crate_name, proto_info, deps, prost_toolchain, is_tonic, rustfmt_toolchain = None):
    kind = "tonic" if is_tonic else "prost"
    extension = TONIC_EXTENSION if is_tonic else PROST_EXTENSION
    provider = TonicProtoInfo if is_tonic else ProstProtoInfo

    deps_info_file = ctx.actions.declare_file(ctx.label.name + ".{}_deps_info".format(kind))
    dep_package_infos = [dep[provider].package_info for dep in deps]
    ctx.actions.write(
        output = deps_info_file,
        content = "\n".join([file.path for file in dep_package_infos]),
    )

    package_info_file = ctx.actions.declare_file(ctx.label.name + ".{}_package_info".format(kind))
    lib_rs = ctx.actions.declare_file("{}.lib{}".format(ctx.label.name, extension))

    proto_compiler = prost_toolchain.proto_compiler[DefaultInfo].files_to_run
    tools = depset([proto_compiler.executable])

    additional_args = ctx.actions.args()

    # Prost process wrapper specific args
    additional_args.add("--protoc={}".format(proto_compiler.executable.path))
    additional_args.add("--out_librs={}".format(lib_rs.path))
    additional_args.add("--package_info_output={}".format("{}={}".format(crate_name, package_info_file.path)))
    additional_args.add("--deps_info={}".format(deps_info_file.path))
    additional_args.add("--prost_opt=compile_well_known_types")
    additional_args.add_all(prost_toolchain.prost_opts, format_each = "--prost_opt=%s")

    if prost_toolchain.tonic_plugin:
        tonic_plugin = prost_toolchain.tonic_plugin[DefaultInfo].files_to_run
        additional_args.add(prost_toolchain.tonic_plugin_flag % tonic_plugin.executable.path)
        additional_args.add("--tonic_opt=no_include")
        additional_args.add_all(prost_toolchain.tonic_opts, format_each = "--tonic_opt=%s")
        tools = depset([tonic_plugin.executable], transitive = [tools])

    if rustfmt_toolchain:
        additional_args.add("--rustfmt={}".format(rustfmt_toolchain.rustfmt.path))
        tools = depset(transitive = [tools, rustfmt_toolchain.all_files])

    additional_inputs = depset([deps_info_file] + [dep[provider].package_info for dep in deps])

    proto_common.compile(
        actions = ctx.actions,
        proto_info = proto_info,
        additional_tools = tools.to_list(),
        additional_inputs = additional_inputs,
        additional_args = additional_args,
        generated_files = [lib_rs, package_info_file],
        proto_lang_toolchain_info = prost_toolchain.proto_lang_toolchain,
        plugin_output = ctx.bin_dir.path,
    )

    return lib_rs, package_info_file

def _get_crate_info(providers):
    """Finds the CrateInfo provider in the list of providers."""
    for provider in providers:
        if hasattr(provider, "name"):
            return provider
    fail("Couldn't find a CrateInfo in the list of providers")

def _get_dep_info(providers):
    """Finds the DepInfo provider in the list of providers."""
    for provider in providers:
        if hasattr(provider, "direct_crates"):
            return provider
    fail("Couldn't find a DepInfo in the list of providers")

def _get_cc_info(providers):
    """Finds the CcInfo provider in the list of providers."""
    for provider in providers:
        if hasattr(provider, "linking_context"):
            return provider
    fail("Couldn't find a CcInfo in the list of providers")

def _compile_rust(ctx, attr, crate_name, src, deps, edition):
    """Compiles a Rust source file.

    Args:
      ctx (RuleContext): The rule context.
      attr (Attrs): The current rule's attributes (`ctx.attr` for rules, `ctx.rule.attr` for aspects)
      crate_name (str): The crate module name to use.
      src (File): The crate root source file to be compiled.
      deps (List of DepVariantInfo): A list of dependencies needed.
      edition (str): The Rust edition to use.

    Returns:
      A DepVariantInfo provider.
    """
    toolchain = ctx.toolchains["@rules_rust//rust:toolchain_type"]
    output_hash = repr(hash(src.path))

    lib_name = "{prefix}{name}-{lib_hash}{extension}".format(
        prefix = "lib",
        name = crate_name,
        lib_hash = output_hash,
        extension = ".rlib",
    )

    rmeta_name = "{prefix}{name}-{lib_hash}{extension}".format(
        prefix = "lib",
        name = crate_name,
        lib_hash = output_hash,
        extension = ".rmeta",
    )

    lib = ctx.actions.declare_file(lib_name)
    rmeta = ctx.actions.declare_file(rmeta_name)

    providers = rustc_compile_action(
        ctx = ctx,
        attr = attr,
        toolchain = toolchain,
        crate_info = rust_common.create_crate_info(
            name = crate_name,
            type = "rlib",
            root = src,
            srcs = depset([src]),
            deps = depset(deps),
            proc_macro_deps = depset([]),
            aliases = {},
            output = lib,
            metadata = rmeta,
            edition = edition,
            is_test = False,
            rustc_env = {},
            compile_data = depset([]),
            compile_data_targets = depset([]),
            owner = ctx.label,
        ),
        output_hash = output_hash,
    )

    crate_info = _get_crate_info(providers)
    dep_info = _get_dep_info(providers)
    cc_info = _get_cc_info(providers)

    return DepVariantInfo(
        crate_info = crate_info,
        dep_info = dep_info,
        cc_info = cc_info,
        build_info = None,
    )

def _rust_prost_aspect_impl(target, ctx):
    proto_info_provider = TonicProtoInfo if ctx.attr._is_tonic else ProstProtoInfo
    if proto_info_provider in target:
        return []

    runtime_deps = []

    rustfmt_toolchain = ctx.toolchains["@rules_rust//rust/rustfmt:toolchain_type"]
    prost_toolchain = ctx.toolchains["@rules_rust//proto/prost:toolchain_type"]
    for prost_runtime in [prost_toolchain.prost_runtime, prost_toolchain.tonic_runtime]:
        if not prost_runtime:
            continue
        if rust_common.crate_group_info in prost_runtime:
            crate_group_info = prost_runtime[rust_common.crate_group_info]
            runtime_deps.extend(crate_group_info.dep_variant_infos.to_list())
        else:
            runtime_deps.append(DepVariantInfo(
                crate_info = prost_runtime[rust_common.crate_info] if rust_common.crate_info in prost_runtime else None,
                dep_info = prost_runtime[rust_common.dep_info] if rust_common.dep_info in prost_runtime else None,
                cc_info = prost_runtime[CcInfo] if CcInfo in prost_runtime else None,
                build_info = None,
            ))

    proto_deps = getattr(ctx.rule.attr, "deps", [])

    direct_deps = []
    transitive_deps = []
    for proto_dep in proto_deps:
        proto_info = proto_dep[proto_info_provider]

        direct_deps.append(proto_info.dep_variant_info)
        transitive_deps.append(depset(
            [proto_info.dep_variant_info],
            transitive = [proto_info.transitive_dep_infos],
        ))

    deps = runtime_deps + direct_deps

    crate_name = ctx.label.name.replace("-", "_").replace("/", "_")

    lib_rs, package_info_file = _compile_proto(
        ctx = ctx,
        crate_name = crate_name,
        proto_info = target[ProtoInfo],
        deps = proto_deps,
        prost_toolchain = prost_toolchain,
        is_tonic = ctx.attr._is_tonic,
        rustfmt_toolchain = rustfmt_toolchain,
    )

    dep_variant_info = _compile_rust(
        ctx = ctx,
        attr = ctx.rule.attr,
        crate_name = crate_name,
        src = lib_rs,
        deps = deps,
        edition = RUST_EDITION,
    )

    return [
        proto_info_provider(
            dep_variant_info = dep_variant_info,
            transitive_dep_infos = depset(transitive = transitive_deps),
            package_info = package_info_file,
        ),
    ]

def _make_rust_prost_aspect(doc, is_tonic):
    return aspect(
        doc = doc,
        implementation = _rust_prost_aspect_impl,
        attr_aspects = ["deps"],
        attrs = {
            "_cc_toolchain": attr.label(
                doc = (
                    "In order to use find_cc_toolchain, your rule has to depend " +
                    "on C++ toolchain. See `@rules_cc//cc:find_cc_toolchain.bzl` " +
                    "docs for details."
                ),
                default = Label("@bazel_tools//tools/cpp:current_cc_toolchain"),
            ),
            "_collect_cc_coverage": attr.label(
                default = Label("//util:collect_coverage"),
                executable = True,
                cfg = "exec",
            ),
            "_error_format": attr.label(
                default = Label("//:error_format"),
            ),
            "_extra_exec_rustc_flag": attr.label(
                default = Label("//:extra_exec_rustc_flag"),
            ),
            "_extra_exec_rustc_flags": attr.label(
                default = Label("//:extra_exec_rustc_flags"),
            ),
            "_extra_rustc_flag": attr.label(
                default = Label("//:extra_rustc_flag"),
            ),
            "_extra_rustc_flags": attr.label(
                default = Label("//:extra_rustc_flags"),
            ),
            "_grep_includes": attr.label(
                allow_single_file = True,
                default = Label("@bazel_tools//tools/cpp:grep-includes"),
                cfg = "exec",
            ),
            "_is_tonic": attr.bool(
                doc = "Indicates whether or not Tonic behavior should be enabled.",
                default = is_tonic,
            ),
            "_process_wrapper": attr.label(
                doc = "A process wrapper for running rustc on all platforms.",
                default = Label("//util/process_wrapper"),
                executable = True,
                allow_single_file = True,
                cfg = "exec",
            ),
        },
        fragments = ["cpp"],
        host_fragments = ["cpp"],
        toolchains = [
            "@bazel_tools//tools/cpp:toolchain_type",
            "@rules_rust//proto/prost:toolchain_type",
            "@rules_rust//rust:toolchain_type",
            "@rules_rust//rust/rustfmt:toolchain_type",
        ],
        incompatible_use_toolchain_transition = True,
    )

_rust_prost_aspect = _make_rust_prost_aspect(
    doc = "An aspect used to generate and compile proto files with Prost.",
    is_tonic = False,
)

_rust_tonic_aspect = _make_rust_prost_aspect(
    doc = "An aspect used to generate and compile proto files with Prost and Tonic.",
    is_tonic = True,
)

def _rust_prost_library_impl(ctx):
    proto_info_provider = TonicProtoInfo if ctx.attr._is_tonic else ProstProtoInfo

    proto_dep = ctx.attr.proto
    rust_proto_info = proto_dep[proto_info_provider]
    dep_variant_info = rust_proto_info.dep_variant_info

    return [
        DefaultInfo(files = depset([dep_variant_info.crate_info.output])),
        rust_common.crate_group_info(
            dep_variant_infos = depset(
                [dep_variant_info],
                transitive = [rust_proto_info.transitive_dep_infos],
            ),
        ),
    ]

def _make_rust_prost_library_rule(doc, is_tonic):
    return rule(
        doc = doc,
        implementation = _rust_prost_library_impl,
        attrs = {
            "proto": attr.label(
                doc = "A `proto_library` target for which to generate Rust gencode.",
                providers = [ProtoInfo],
                aspects = [_rust_tonic_aspect if is_tonic else _rust_prost_aspect],
                mandatory = True,
            ),
            "_collect_cc_coverage": attr.label(
                default = Label("@rules_rust//util:collect_coverage"),
                executable = True,
                cfg = "exec",
            ),
            "_is_tonic": attr.bool(
                doc = "Indicates whether or not Tonic behavior should be enabled.",
                default = is_tonic,
            ),
        },
    )

rust_tonic_library = _make_rust_prost_library_rule(
    doc = "A rule for generating a Rust library using Prost and Tonic.",
    is_tonic = True,
)

rust_prost_library = _make_rust_prost_library_rule(
    doc = "A rule for generating a Rust library using Prost.",
    is_tonic = False,
)

def _rust_prost_toolchain_impl(ctx):
    is_tonic = ctx.attr.tonic_runtime != None

    proto_lang_toolchain = proto_common.ProtoLangToolchainInfo(
        out_replacement_format_flag = "--tonic_out=%s" if is_tonic else "--prost_out=%s",
        plugin_format_flag = ctx.attr.prost_plugin_flag,
        plugin = ctx.attr.prost_plugin[DefaultInfo].files_to_run,
        runtime = ctx.attr.prost_runtime,
        provided_proto_sources = depset(),
        proto_compiler = ctx.attr._prost_process_wrapper[DefaultInfo].files_to_run,
        protoc_opts = ctx.fragments.proto.experimental_protoc_opts,
        progress_message = "",
        mnemonic = "ToncGenProto" if is_tonic else "ProstGenProto",
    )

    tonic_attrs = [ctx.attr.tonic_plugin_flag, ctx.attr.tonic_plugin, ctx.attr.tonic_runtime]
    if any(tonic_attrs) and not all(tonic_attrs):
        fail("When one tonic attribute is added, all must be added")

    return [platform_common.ToolchainInfo(
        prost_opts = ctx.attr.prost_opts,
        prost_plugin = ctx.attr.prost_plugin,
        prost_plugin_flag = ctx.attr.prost_plugin_flag,
        prost_runtime = ctx.attr.prost_runtime,
        proto_compiler = ctx.attr.proto_compiler,
        proto_lang_toolchain = proto_lang_toolchain,
        protoc_opts = ctx.fragments.proto.experimental_protoc_opts,
        tonic_opts = ctx.attr.tonic_opts,
        tonic_plugin = ctx.attr.tonic_plugin,
        tonic_plugin_flag = ctx.attr.tonic_plugin_flag,
        tonic_runtime = ctx.attr.tonic_runtime,
    )]

rust_prost_toolchain = rule(
    implementation = _rust_prost_toolchain_impl,
    doc = "Rust Prost toolchain rule.",
    fragments = ["proto"],
    attrs = {
        "prost_opts": attr.string_list(
            doc = "Additional options to add to Prost.",
        ),
        "prost_plugin": attr.label(
            doc = "Additional plugins to add to Prost.",
            cfg = "exec",
            executable = True,
            mandatory = True,
        ),
        "prost_plugin_flag": attr.string(
            doc = "Prost plugin flag format. (e.g. `--plugin=protoc-gen-prost=%s`)",
            mandatory = True,
        ),
        "prost_runtime": attr.label(
            doc = "The Prost runtime crates to use.",
            providers = [[rust_common.crate_info], [rust_common.crate_group_info]],
            mandatory = True,
        ),
        "proto_compiler": attr.label(
            doc = "The protoc compiler to use.",
            cfg = "exec",
            executable = True,
            mandatory = True,
        ),
        "tonic_opts": attr.string_list(
            doc = "Additional options to add to Tonic.",
        ),
        "tonic_plugin": attr.label(
            doc = "Additional plugins to add to Tonic.",
            cfg = "exec",
            executable = True,
        ),
        "tonic_plugin_flag": attr.string(
            doc = "Tonic plugin flag format. (e.g. `--plugin=protoc-gen-tonic=%s`))",
        ),
        "tonic_runtime": attr.label(
            doc = "The Tonic runtime crates to use.",
            providers = [[rust_common.crate_info], [rust_common.crate_group_info]],
        ),
        "_prost_process_wrapper": attr.label(
            doc = "The wrapper script for the Prost protoc plugin.",
            cfg = "exec",
            executable = True,
            default = Label("//proto/prost/private:protoc_wrapper"),
        ),
    },
)
