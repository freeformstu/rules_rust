### Expand Example

Rustc can be used to expand all macros so that you can inspect the generated source files easier.

This feature is enabled via `-Zunpretty=expanded`. The `-Z` flag is only available in the nightly
version of `rustc`.


### Expanding

Build and test your targets normally.

```
bazel build //:ok_binary   
INFO: Analyzed target //:ok_binary (0 packages loaded, 0 targets configured).
INFO: Found 1 target...
Target //:ok_binary up-to-date:
  bazel-bin/ok_binary
INFO: Elapsed time: 0.081s, Critical Path: 0.00s
INFO: 1 process: 1 internal.
INFO: Build completed successfully, 1 total action
```

Use the aspect to generate the expanded files in as a one-off build.

```
bazel build --config=expanded //:ok_binary
INFO: Analyzed target //:ok_binary (1 packages loaded, 2 targets configured).
INFO: Found 1 target...
Aspect @rules_rust//rust/private:expand.bzl%rust_expand_aspect of //:ok_binary up-to-date:
  bazel-bin/ok_binary.expand.rs
INFO: Elapsed time: 0.149s, Critical Path: 0.00s
INFO: 1 process: 1 internal.
INFO: Build completed successfully, 1 total action
```

Targeting tests is valid as well.

```
bazel build --config=expanded //:ok_test  
INFO: Analyzed target //:ok_test (0 packages loaded, 2 targets configured).
INFO: Found 1 target...
Aspect @rules_rust//rust/private:expand.bzl%rust_expand_aspect of //:ok_test up-to-date:
  bazel-bin/test-397521499/ok_test.expand.rs
INFO: Elapsed time: 0.113s, Critical Path: 0.00s
INFO: 1 process: 1 internal.
INFO: Build completed successfully, 1 total action
```

Finally, manually wire up a `rust_expand` target explicitly if you want a target to build.

```
load("@rules_rust//rust:defs.bzl", "rust_binary", "rust_expand")

rust_binary(
    name = "ok_binary",
    srcs = ["src/main.rs"],
    edition = "2021",
)

rust_expand(
    name = "ok_binary_expand",
    deps = [":ok_binary"],
)
```

```
bazel build //:ok_binary_expand
INFO: Analyzed target //:ok_binary_expand (0 packages loaded, 1 target configured).
INFO: Found 1 target...
Target //:ok_binary_expand up-to-date:
  bazel-bin/ok_binary.expand.rs
INFO: Elapsed time: 0.090s, Critical Path: 0.00s
INFO: 1 process: 1 internal.
INFO: Build completed successfully, 1 total action
```
