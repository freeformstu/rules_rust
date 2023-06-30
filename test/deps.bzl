"""A module defining dependencies of the `rules_rust` tests"""

load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")
load("@bazel_tools//tools/build_defs/repo:utils.bzl", "maybe")
load("//test/load_arbitrary_tool:load_arbitrary_tool_test.bzl", "load_arbitrary_tool_test")
load("//test/unit/toolchain:toolchain_test_utils.bzl", "rules_rust_toolchain_test_target_json_repository")

_LIBC_BUILD_FILE_CONTENT = """\
load("@rules_rust//rust:defs.bzl", "rust_library")

rust_library(
    name = "libc",
    srcs = glob(["src/**/*.rs"]),
    edition = "2015",
    rustc_flags = [
        # In most cases, warnings in 3rd party crates are not interesting as
        # they're out of the control of consumers. The flag here silences
        # warnings. For more details see:
        # https://doc.rust-lang.org/rustc/lints/levels.html
        "--cap-lints=allow",
    ],
    visibility = ["//visibility:public"],
)
"""

def rules_rust_test_deps():
    """Load dependencies for rules_rust tests"""

    load_arbitrary_tool_test()

    maybe(
        http_archive,
        name = "libc",
        build_file_content = _LIBC_BUILD_FILE_CONTENT,
        sha256 = "1ac4c2ac6ed5a8fb9020c166bc63316205f1dc78d4b964ad31f4f21eb73f0c6d",
        strip_prefix = "libc-0.2.20",
        urls = [
            "https://mirror.bazel.build/github.com/rust-lang/libc/archive/0.2.20.zip",
            "https://github.com/rust-lang/libc/archive/0.2.20.zip",
        ],
    )

    maybe(
        rules_rust_toolchain_test_target_json_repository,
        name = "rules_rust_toolchain_test_target_json",
        target_json = Label("//test/unit/toolchain:toolchain-test-triple.json"),
    )

    maybe(
        http_archive,
        name = "bazel_remote_apis",
        urls = ["https://github.com/bazelbuild/remote-apis/archive/068363a3625e166056c155f6441cfb35ca8dfbf2.zip"],
        sha256 = "34263013f19e6161195747dc0d3293f8bf65c8fed1f60ba40d4246c4f8008de1",
        strip_prefix = "remote-apis-068363a3625e166056c155f6441cfb35ca8dfbf2",
        workspace_file_content = "exports_files(['external/BUILD.googleapis'])",
    )

    maybe(
        http_archive,
        name = "googleapis",
        build_file = "@bazel_remote_apis//:external/BUILD.googleapis",
        sha256 = "7b6ea252f0b8fb5cd722f45feb83e115b689909bbb6a393a873b6cbad4ceae1d",
        strip_prefix = "googleapis-143084a2624b6591ee1f9d23e7f5241856642f4d",
        urls = ["https://github.com/googleapis/googleapis/archive/143084a2624b6591ee1f9d23e7f5241856642f4d.zip"],
    )

    maybe(
        http_archive,
        name = "com_github_grpc_grpc",
        sha256 = "b391a327429279f6f29b9ae7e5317cd80d5e9d49cc100e6d682221af73d984a6",
        strip_prefix = "grpc-93e8830070e9afcbaa992c75817009ee3f4b63a0",  # v1.24.3 with fixes
        urls = ["https://github.com/grpc/grpc/archive/93e8830070e9afcbaa992c75817009ee3f4b63a0.zip"],
    )
