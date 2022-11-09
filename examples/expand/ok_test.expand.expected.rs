#![feature(prelude_import)]
#[prelude_import]
use std::prelude::rust_2021::*;
#[macro_use]
extern crate std;
pub fn greeting() -> String { "Hello World".to_owned() }

// too_many_args/clippy.toml will require no more than 2 args.
pub fn with_args(_: u32, _: u32, _: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate test;
    #[cfg(test)]
    #[rustc_test_marker = "tests::it_works"]
    pub const it_works: test::TestDescAndFn =
        test::TestDescAndFn {
            desc: test::TestDesc {
                name: test::StaticTestName("tests::it_works"),
                ignore: false,
                ignore_message: ::core::option::Option::None,
                compile_fail: false,
                no_run: false,
                should_panic: test::ShouldPanic::No,
                test_type: test::TestType::UnitTest,
            },
            testfn: test::StaticTestFn(||
                    test::assert_test_result(it_works())),
        };
    fn it_works() {
        match (&greeting(), &"Hello World".to_owned()) {
            (left_val, right_val) => {
                if !(*left_val == *right_val) {
                        let kind = ::core::panicking::AssertKind::Eq;
                        ::core::panicking::assert_failed(kind, &*left_val,
                            &*right_val, ::core::option::Option::None);
                    }
            }
        };
    }
}
#[rustc_main]
pub fn main() -> () {
    extern crate test;
    test::test_main_static(&[&it_works])
}
