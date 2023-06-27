//! A process wrapper for running a Protobuf compiler configured for Prost or Tonic output in a Bazel rule.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::env;
use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process;

/// Locate prost outputs in the protoc output directory.
fn find_generated_rust_files(out_dir: &Path) -> BTreeSet<PathBuf> {
    let mut all_rs_files: BTreeSet<PathBuf> = BTreeSet::new();
    for entry in fs::read_dir(out_dir).expect("Failed to read directory") {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();
        if path.is_dir() {
            for f in find_generated_rust_files(&path) {
                all_rs_files.insert(f);
            }
        } else if let Some(ext) = path.extension() {
            if ext == "rs" {
                all_rs_files.insert(path);
            }
        } else if let Some(name) = path.file_name() {
            if name == "_" {
                let rs_name = path.parent().expect("Failed to get parent").join("_.rs");
                fs::rename(&path, &rs_name).expect("Failed to rename file");
                all_rs_files.insert(rs_name);
            }
        }
    }

    all_rs_files
}

/// Rust module definition.
#[derive(Debug, Default)]
struct Module {
    /// The name of the module.
    name: String,

    /// The contents of the module.
    contents: String,

    /// The names of any other modules which are submodules of this module.
    submodules: BTreeSet<String>,
}

/// Generate a lib.rs file with all prost/tonic outputs embeeded in modules which
/// mirror the proto packages. For the example proto file we would expect to see
/// the Rust output that follows it.
///
/// ```proto
/// syntax = "proto3";
/// package examples.prost.helloworld;
///
/// message HelloRequest {
///     // Request message contains the name to be greeted
///     string name = 1;
/// }
//
/// message HelloReply {
///     // Reply contains the greeting message
///     string message = 1;
/// }
/// ```
///
/// This is expected to render out to something like the following. Note that
/// formatting is not applied so indentation may be missing in the actual output.
///
/// ```ignore
/// pub mod examples {
///     pub mod prost {
///         pub mod helloworld {
///             // @generated
///             #[allow(clippy::derive_partial_eq_without_eq)]
///             #[derive(Clone, PartialEq, ::prost::Message)]
///             pub struct HelloRequest {
///                 /// Request message contains the name to be greeted
///                 #[prost(string, tag = "1")]
///                 pub name: ::prost::alloc::string::String,
///             }
///             #[allow(clippy::derive_partial_eq_without_eq)]
///             #[derive(Clone, PartialEq, ::prost::Message)]
///             pub struct HelloReply {
///                 /// Reply contains the greeting message
///                 #[prost(string, tag = "1")]
///                 pub message: ::prost::alloc::string::String,
///             }
///             // @protoc_insertion_point(module)
///         }
///     }
/// }
/// ```
fn generate_lib_rs(prost_outputs: &BTreeSet<PathBuf>, is_tonic: bool) -> String {
    let mut module_info = BTreeMap::new();

    for path in prost_outputs.iter() {
        let mut package = path
            .file_stem()
            .expect("Failed to get file stem")
            .to_str()
            .expect("Failed to convert to str")
            .to_string();

        if is_tonic {
            package = package
                .strip_suffix(".tonic")
                .expect("Failed to strip suffix")
                .to_string()
        };

        let module_name = package.to_lowercase().to_string();

        if module_name.is_empty() {
            continue;
        }

        let mut name = module_name.clone();
        if module_name.contains('.') {
            name = module_name
                .rsplit_once('.')
                .expect("Failed to split on '.'")
                .1
                .to_string();
        }

        module_info.insert(
            module_name.clone(),
            Module {
                name,
                contents: fs::read_to_string(path).expect("Failed to read file"),
                submodules: BTreeSet::new(),
            },
        );

        let module_parts = module_name.split('.').collect::<Vec<&str>>();
        for parent_module_index in 0..module_parts.len() {
            let child_module_index = parent_module_index + 1;
            if child_module_index >= module_parts.len() {
                break;
            }
            let full_parent_module_name = module_parts[0..parent_module_index + 1].join(".");
            let parent_module_name = module_parts[parent_module_index];
            let child_module_name = module_parts[child_module_index];

            module_info
                .entry(full_parent_module_name.clone())
                .and_modify(|parent_module| {
                    parent_module
                        .submodules
                        .insert(child_module_name.to_string());
                })
                .or_insert(Module {
                    name: parent_module_name.to_string(),
                    contents: "".to_string(),
                    submodules: [child_module_name.to_string()].iter().cloned().collect(),
                });
        }
    }

    let mut content = "// @generated\n\n".to_string();
    write_module(&mut content, &module_info, "", 0);
    content
}

/// Write out a rust module and all of its submodules.
fn write_module(
    content: &mut String,
    module_info: &BTreeMap<String, Module>,
    module_name: &str,
    depth: usize,
) {
    if module_name.is_empty() {
        for submodule_name in module_info.keys() {
            write_module(content, module_info, submodule_name, depth + 1);
        }
        return;
    }
    let module = module_info.get(module_name).expect("Failed to get module");
    let indent = "  ".repeat(depth);
    let is_rust_module = module.name != "_";

    if is_rust_module {
        content
            .write_str(&format!("{}pub mod {} {{\n", indent, module.name))
            .expect("Failed to write string");
    }

    content
        .write_str(&module.contents)
        .expect("Failed to write string");

    for submodule_name in module.submodules.iter() {
        write_module(
            content,
            module_info,
            [module_name, submodule_name].join(".").as_str(),
            depth + 1,
        );
    }

    if is_rust_module {
        content
            .write_str(&format!("{}}}\n", indent))
            .expect("Failed to write string");
    }
}

/// Create a map of proto files to their free field number strings.
///
/// We use the free field numbers api as a convenient way to get a list of all message types in a
/// proto file.
fn create_free_field_numbers_map(
    proto_files: BTreeSet<PathBuf>,
    protoc: &Path,
    includes: &[String],
    proto_paths: &[String],
) -> BTreeMap<PathBuf, String> {
    proto_files
        .into_iter()
        .map(|proto_file| {
            let output = process::Command::new(protoc)
                .args(includes.iter().map(|include| format!("-I{}", include)))
                .arg("--print_free_field_numbers")
                .args(
                    proto_paths
                        .iter()
                        .map(|proto_path| format!("--proto_path={}", proto_path)),
                )
                .arg(&proto_file)
                .stdout(process::Stdio::piped())
                .spawn()
                .expect("Failed to spawn protoc")
                .wait_with_output()
                .expect("Failed to wait on protoc");

            // check success
            if !output.status.success() {
                panic!(
                    "Failed to run protoc: {}",
                    std::str::from_utf8(&output.stderr).expect("Failed to parse stderr")
                );
            }

            let stdout = std::str::from_utf8(&output.stdout).expect("Failed to parse stdout");
            (proto_file, stdout.to_owned())
        })
        .collect()
}

/// Compute the `--extern_path` flags for a list of proto files. This is
/// expected to convert proto files into a list of
/// `.example.prost.helloworld=crate_name::example::prost::helloworld`
fn compute_proto_package_info(
    proto_free_field_numbers: &BTreeMap<PathBuf, String>,
    crate_name: &str,
) -> Result<BTreeSet<String>, String> {
    let mut extern_paths = BTreeSet::new();
    for stdout in proto_free_field_numbers.values() {
        for line in stdout.lines() {
            let text = line.trim();
            if text.is_empty() {
                continue;
            }

            let (absolute, _) = text
                .split_once(' ')
                .expect("Failed to split free field number line");

            let mut package = "";
            let mut symbol_name = absolute;
            if let Some((package_, symbol_name_)) = absolute.rsplit_once('.') {
                package = package_;
                symbol_name = symbol_name_;
            }
            let symbol = format!("{}::{}", package.replace('.', "::"), symbol_name);
            let extern_path = format!(".{}={}::{}", absolute, crate_name, symbol.trim_matches(':'));
            if !extern_paths.insert(extern_path.clone()) {
                return Err(format!("Duplicate extern: {}", extern_path));
            }
        }
    }

    Ok(extern_paths)
}

/// The parsed command-line arguments.
struct Args {
    /// The path to the protoc binary.
    protoc: PathBuf,

    /// The path to the output directory.
    out_dir: PathBuf,

    /// The name of the crate.
    crate_name: String,

    /// The path to the package info file.
    package_info_file: PathBuf,

    /// The proto files to compile.
    proto_files: Vec<PathBuf>,

    /// The include directories.
    includes: Vec<String>,

    /// The path to the generated lib.rs file.
    out_librs: PathBuf,

    /// The proto include paths.
    proto_paths: Vec<String>,

    /// The path to the rustfmt binary.
    rustfmt: Option<PathBuf>,

    /// Whether to generate tonic code.
    is_tonic: bool,

    /// Extra arguments to pass to protoc.
    extra_args: Vec<String>,
}

impl Args {
    /// Parse the command-line arguments.
    fn parse() -> Result<Args, String> {
        let mut protoc: Option<PathBuf> = None;
        let mut out_dir: Option<PathBuf> = None;
        let mut crate_name: Option<String> = None;
        let mut package_info_file: Option<PathBuf> = None;
        let mut proto_files: Vec<PathBuf> = Vec::new();
        let mut includes = Vec::new();
        let mut out_librs: Option<PathBuf> = None;
        let mut rustfmt: Option<PathBuf> = None;
        let mut proto_paths = Vec::new();
        let mut is_tonic = false;

        let mut extra_args = Vec::new();

        // Iterate over the given command line arguments parsing out arguments
        // for the process runner and arguments for protoc and potentially spawn
        // additional arguments needed by prost.
        for arg in env::args().skip(1) {
            if !arg.starts_with('-') {
                proto_files.push(PathBuf::from(arg));
                continue;
            }

            if arg.starts_with("-I") {
                includes.push(
                    arg.strip_prefix("-I")
                        .expect("Failed to strip -I")
                        .to_string(),
                );
                continue;
            }

            if arg == "--is_tonic" {
                is_tonic = true;
                continue;
            }

            if !arg.contains('=') {
                extra_args.push(arg);
                continue;
            }

            let part = arg.split_once('=').expect("Failed to split argument on =");
            match part {
                ("--protoc", value) => {
                    protoc = Some(PathBuf::from(value));
                }
                ("--prost_out", value) => {
                    out_dir = Some(PathBuf::from(value));
                }
                ("--tonic_out", value) => {
                    out_dir = Some(PathBuf::from(value));
                }
                ("--crate_name", value) => {
                    crate_name = Some(value.to_string());
                }
                ("--package_info_output", value) => {
                    let (key, value) = value
                        .split_once('=')
                        .map(|(a, b)| (a.to_string(), PathBuf::from(b)))
                        .expect("Failed to parse package info output");
                    crate_name = Some(key);
                    package_info_file = Some(value);
                }
                ("--deps_info", value) => {
                    for line in fs::read_to_string(value)
                        .expect("Failed to read file")
                        .lines()
                    {
                        let path = PathBuf::from(line.trim());
                        for flag in fs::read_to_string(path)
                            .expect("Failed to read file")
                            .lines()
                        {
                            extra_args.push(format!("--prost_opt=extern_path={}", flag.trim()));
                        }
                    }
                }
                ("--out_librs", value) => {
                    out_librs = Some(PathBuf::from(value));
                }
                ("--rustfmt", value) => {
                    rustfmt = Some(PathBuf::from(value));
                }
                ("--proto_path", value) => {
                    // if value.ends_with("import_proto") {
                    //     continue;
                    // }
                    proto_paths.push(value.to_string());
                }
                (arg, value) => {
                    extra_args.push(format!("{}={}", arg, value));
                }
            }
        }

        if protoc.is_none() {
            return Err(
                "No `--protoc` value was found. Unable to parse path to proto compiler."
                    .to_string(),
            );
        }
        if out_dir.is_none() {
            return Err(
                "No `--prost_out` value was found. Unable to parse output directory.".to_string(),
            );
        }
        if crate_name.is_none() {
            return Err(
                "No `--package_info_output` value was found. Unable to parse target crate name."
                    .to_string(),
            );
        }
        if package_info_file.is_none() {
            return Err("No `--package_info_output` value was found. Unable to parse package info output file.".to_string());
        }
        if out_librs.is_none() {
            return Err("No `--out_librs` value was found. Unable to parse the output location for all combined prost outputs.".to_string());
        }

        Ok(Args {
            protoc: protoc.unwrap(),
            out_dir: out_dir.unwrap(),
            crate_name: crate_name.unwrap(),
            package_info_file: package_info_file.unwrap(),
            proto_files,
            includes,
            out_librs: out_librs.unwrap(),
            rustfmt,
            proto_paths,
            is_tonic,
            extra_args,
        })
    }
}

fn main() {
    let Args {
        protoc,
        out_dir,
        crate_name,
        package_info_file,
        proto_files,
        includes,
        out_librs,
        rustfmt,
        proto_paths,
        is_tonic,
        extra_args,
    } = Args::parse().expect("Failed to parse args");

    let mut cmd = process::Command::new(&protoc);
    cmd.arg(format!("--prost_out={}", out_dir.display()));
    if is_tonic {
        cmd.arg(format!("--tonic_out={}", out_dir.display()));
    }
    cmd.args(extra_args);
    cmd.args(
        proto_paths
            .iter()
            .map(|proto_path| format!("--proto_path={}", proto_path)),
    );
    cmd.args(includes.iter().map(|include| format!("-I{}", include)));
    cmd.args(&proto_files);

    let status = cmd.status().expect("Failed to spawn protoc process");
    if !status.success() {
        panic!(
            "protoc failed with status: {}",
            status.code().expect("failed to get exit code")
        );
    }

    // Not all proto files will consistently produce `.rs` or `.tonic.rs` files. This is
    // caused by the proto file being transpiled not having an RPC service or other protos
    // defined (a natural and expected situation). To guarantee consistent outputs, all
    // `.rs` files are either renamed to `.tonic.rs` if there is no `.tonic.rs` or prepended
    // to the existing `.tonic.rs`.
    if is_tonic {
        let tonic_files: BTreeSet<PathBuf> = find_generated_rust_files(&out_dir);

        for tonic_file in tonic_files.iter() {
            if !tonic_file
                .file_name()
                .map(|os_name| {
                    os_name
                        .to_str()
                        .map(|name| name.ends_with(".tonic.rs"))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
            {
                let real_tonic_file = PathBuf::from(format!(
                    "{}.tonic.rs",
                    tonic_file
                        .to_str()
                        .expect("Failed to convert to str")
                        .strip_suffix(".rs")
                        .expect("Failed to strip suffix.")
                ));
                if real_tonic_file.exists() {
                    continue;
                }
                fs::rename(tonic_file, real_tonic_file).expect("Failed to rename file.");
            } else {
                let rs_file = PathBuf::from(format!(
                    "{}.rs",
                    tonic_file
                        .to_str()
                        .expect("Failed to convert to str")
                        .strip_suffix(".tonic.rs")
                        .expect("Failed to strip suffix.")
                ));

                if rs_file.exists() {
                    let rs_content = fs::read_to_string(&rs_file).expect("Failed to read file.");
                    let tonic_content =
                        fs::read_to_string(tonic_file).expect("Failed to read file.");
                    fs::write(tonic_file, format!("{}\n{}", rs_content, tonic_content))
                        .expect("Failed to write file.");
                    fs::remove_file(&rs_file).expect("Failed to remove file.");
                }
            }
        }
    }

    // Locate all prost-generated outputs.
    let rust_files: BTreeSet<PathBuf> = find_generated_rust_files(&out_dir);
    if rust_files.is_empty() {
        panic!("No .rs files were generated by prost.");
    }

    let free_field_numbers = create_free_field_numbers_map(
        proto_files.into_iter().collect::<BTreeSet<_>>(),
        &protoc,
        &includes,
        &proto_paths,
    );

    let package_info: BTreeSet<String> =
        compute_proto_package_info(&free_field_numbers, &crate_name)
            .expect("Failed to compute proto package info");

    // Write outputs
    fs::write(&out_librs, generate_lib_rs(&rust_files, is_tonic)).expect("Failed to write file.");
    fs::write(
        package_info_file,
        package_info.into_iter().collect::<Vec<_>>().join("\n"),
    )
    .expect("Failed to write file.");

    // Finally run rustfmt on the output lib.rs file
    if let Some(rustfmt) = rustfmt {
        let fmt_status = process::Command::new(rustfmt)
            .arg("--edition")
            .arg("2021")
            .arg("--quiet")
            .arg(&out_librs)
            .status()
            .expect("Failed to spawn rustfmt process");
        if !fmt_status.success() {
            panic!(
                "rustfmt failed with exit code: {}",
                fmt_status.code().expect("Failed to get exit code")
            );
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;

    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn compute_proto_package_info_test() {
        // Example output from running `protoc --print_free_field_numbers` on
        // https://github.com/protocolbuffers/protobuf/blob/v23.3/src/google/protobuf/descriptor.proto
        let free_field_numbers_output = r"
google.protobuf.FileDescriptorSet   free: 2-INF
google.protobuf.FileDescriptorProto free: 13-INF
google.protobuf.DescriptorProto.ExtensionRange free: 4-INF
google.protobuf.DescriptorProto.ReservedRange free: 3-INF
google.protobuf.DescriptorProto     free: 11-INF
"
        .to_owned();
        let package_infos = compute_proto_package_info(
            &BTreeMap::from([(
                PathBuf::from("/tmp/google/protobuf/descriptor.proto"),
                free_field_numbers_output,
            )]),
            "crate_name",
        )
        .unwrap();

        assert_eq!(package_infos, [
            ".google.protobuf.DescriptorProto.ExtensionRange=crate_name::google::protobuf::DescriptorProto::ExtensionRange",
            ".google.protobuf.DescriptorProto.ReservedRange=crate_name::google::protobuf::DescriptorProto::ReservedRange",
            ".google.protobuf.DescriptorProto=crate_name::google::protobuf::DescriptorProto",
            ".google.protobuf.FileDescriptorProto=crate_name::google::protobuf::FileDescriptorProto",
            ".google.protobuf.FileDescriptorSet=crate_name::google::protobuf::FileDescriptorSet"
        ].into_iter().map(String::from).collect::<BTreeSet<String>>()
    );
    }
}
