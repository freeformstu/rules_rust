"""Dependencies for Rust Prost rules"""

load("//proto/prost/private/3rdparty/crates:crates.bzl", "crate_repositories")

def rust_prost_dependencies():
    crate_repositories()

# buildifier: disable=unnamed-macro
def rust_prost_register_toolchains(register_toolchains = True):
    """_summary_

    Args:
        register_toolchains (bool, optional): _description_. Defaults to True.
    """
    if register_toolchains:
        native.register_toolchains(str(Label("//proto/prost:default_prost_toolchain")))
