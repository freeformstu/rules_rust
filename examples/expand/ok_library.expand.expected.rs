#![feature(prelude_import)]
#[prelude_import]
use std::prelude::rust_2021::*;
#[macro_use]
extern crate std;
pub fn greeting() -> String { "Hello World".to_owned() }

// too_many_args/clippy.toml will require no more than 2 args.
pub fn with_args(_: u32, _: u32, _: u32) {}

