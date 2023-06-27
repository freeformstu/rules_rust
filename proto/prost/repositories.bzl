"""Dependencies for Rust Prost rules"""

load("//proto/prost/private/3rdparty/crates:crates.bzl", "crate_repositories")

def rust_prost_dependencies():
    """Prost repository dependencies."""
    crate_repositories()

# buildifier: disable=unnamed-macro
def rust_prost_register_toolchains(register_toolchains = True):
    """Register toolchains for Rust Prost rules."""

    if register_toolchains:
        native.register_toolchains("@rules_rust//proto/prost/...")
