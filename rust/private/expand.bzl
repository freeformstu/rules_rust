# Copyright 2020 The Bazel Authors. All rights reserved.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#    http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""A module defining Rust expansion rules"""

load("//rust/private:common.bzl", "rust_common")
load(
    "//rust/private:rustc.bzl",
    "collect_deps",
    "collect_inputs",
    "construct_arguments",
)
load(
    "//rust/private:utils.bzl",
    "determine_output_hash",
    "find_cc_toolchain",
    "find_toolchain",
)

def _get_expand_ready_crate_info(target, aspect_ctx):
    """Check that a target is suitable for expansion and extract the `CrateInfo` provider from it.

    Args:
        target (Target): The target the aspect is running on.
        aspect_ctx (ctx, optional): The aspect's context object.

    Returns:
        CrateInfo, optional: A `CrateInfo` provider if rust expand should be run or `None`.
    """

    # Ignore external targets
    if target.label.workspace_root.startswith("external"):
        return None

    # Targets with specific tags will not be formatted
    if aspect_ctx:
        ignore_tags = [
            "noexpand",
            "no-expand",
        ]

        for tag in ignore_tags:
            if tag in aspect_ctx.rule.attr.tags:
                return None

    # Obviously ignore any targets that don't contain `CrateInfo`
    if rust_common.crate_info not in target:
        return None

    return target[rust_common.crate_info]

def _expand_aspect_impl(target, ctx):
    crate_info = _get_expand_ready_crate_info(target, ctx)
    if not crate_info:
        return []

    toolchain = find_toolchain(ctx)
    cc_toolchain, feature_configuration = find_cc_toolchain(ctx)

    dep_info, build_info, linkstamps = collect_deps(
        deps = crate_info.deps,
        proc_macro_deps = crate_info.proc_macro_deps,
        aliases = crate_info.aliases,
        # Rust expand doesn't need to invoke transitive linking, therefore doesn't need linkstamps.
        are_linkstamps_supported = False,
    )

    compile_inputs, out_dir, build_env_files, build_flags_files, linkstamp_outs, ambiguous_libs = collect_inputs(
        ctx,
        ctx.rule.file,
        ctx.rule.files,
        linkstamps,
        toolchain,
        cc_toolchain,
        feature_configuration,
        crate_info,
        dep_info,
        build_info,
    )

    args, env = construct_arguments(
        ctx = ctx,
        attr = ctx.rule.attr,
        file = ctx.file,
        toolchain = toolchain,
        tool_path = toolchain.rustc.path,
        cc_toolchain = cc_toolchain,
        feature_configuration = feature_configuration,
        crate_info = crate_info,
        dep_info = dep_info,
        linkstamp_outs = linkstamp_outs,
        ambiguous_libs = ambiguous_libs,
        output_hash = determine_output_hash(crate_info.root, ctx.label),
        rust_flags = [],
        out_dir = out_dir,
        build_env_files = build_env_files,
        build_flags_files = build_flags_files,
        emit = ["dep-info", "metadata"],
    )

    if crate_info.is_test:
        args.rustc_flags.add("--test")

    expand_out = ctx.actions.declare_file(ctx.label.name + ".expand.rs", sibling = crate_info.output)
    args.process_wrapper_flags.add("--stdout-file", expand_out.path)

    # Expand all macros and dump the source to stdout.
    args.rustc_flags.add("-Zunpretty=expanded")

    ctx.actions.run(
        executable = ctx.executable._process_wrapper,
        inputs = compile_inputs,
        outputs = [expand_out],
        env = env,
        arguments = args.all,
        mnemonic = "RustExpand",
    )

    return [
        OutputGroupInfo(expanded = depset([expand_out])),
    ]

# Example: Expand all rust targets in the codebase.
#   bazel build --aspects=@rules_rust//rust:defs.bzl%rust_expand_aspect \
#               --output_groups=expanded \
#               //...
rust_expand_aspect = aspect(
    fragments = ["cpp"],
    host_fragments = ["cpp"],
    attrs = {
        "_cc_toolchain": attr.label(
            doc = (
                "Required attribute to access the cc_toolchain. See [Accessing the C++ toolchain]" +
                "(https://docs.bazel.build/versions/master/integrating-with-rules-cc.html#accessing-the-c-toolchain)"
            ),
            default = Label("@bazel_tools//tools/cpp:current_cc_toolchain"),
        ),
        "_extra_rustc_flag": attr.label(default = "//:extra_rustc_flag"),
        "_extra_rustc_flags": attr.label(default = "//:extra_rustc_flags"),
        "_process_wrapper": attr.label(
            doc = "A process wrapper for running clippy on all platforms",
            default = Label("//util/process_wrapper"),
            executable = True,
            cfg = "exec",
        ),
    },
    toolchains = [
        str(Label("//rust:toolchain_type")),
        "@bazel_tools//tools/cpp:toolchain_type",
    ],
    incompatible_use_toolchain_transition = True,
    implementation = _expand_aspect_impl,
    doc = """\
Executes Rust expand on specified targets.

This aspect applies to existing rust_library, rust_test, and rust_binary rules.

As an example, if the following is defined in `examples/hello_lib/BUILD.bazel`:

```python
load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")

rust_library(
    name = "hello_lib",
    srcs = ["src/lib.rs"],
)

rust_test(
    name = "greeting_test",
    srcs = ["tests/greeting.rs"],
    deps = [":hello_lib"],
)
```

Then the targets can be expanded with the following command:

```output
$ bazel build --aspects=@rules_rust//rust:defs.bzl%rust_expand_aspect \
              --output_groups=expanded //hello_lib:all
```
""",
)

def _rust_expand_rule_impl(ctx):
    expand_ready_targets = [dep for dep in ctx.attr.deps if "expanded" in dir(dep[OutputGroupInfo])]
    files = depset([], transitive = [dep[OutputGroupInfo].expanded for dep in expand_ready_targets])
    return [DefaultInfo(files = files)]

rust_expand = rule(
    implementation = _rust_expand_rule_impl,
    attrs = {
        "deps": attr.label_list(
            doc = "Rust targets to run expand on.",
            providers = [rust_common.crate_info],
            aspects = [rust_expand_aspect],
        ),
    },
    doc = """\
Executes rust expand on a specific target.

Similar to `rust_expand_aspect`, but allows specifying a list of dependencies \
within the build system.

For example, given the following example targets:

```python
load("@rules_rust//rust:defs.bzl", "rust_library", "rust_test")

rust_library(
    name = "hello_lib",
    srcs = ["src/lib.rs"],
)

rust_test(
    name = "greeting_test",
    srcs = ["tests/greeting.rs"],
    deps = [":hello_lib"],
)
```

Rust expand can be set as a build target with the following:

```python
load("@rules_rust//rust:defs.bzl", "rust_expand")

rust_expand(
    name = "hello_library_expand",
    testonly = True,
    deps = [
        ":hello_lib",
        ":greeting_test",
    ],
)
```
""",
)
