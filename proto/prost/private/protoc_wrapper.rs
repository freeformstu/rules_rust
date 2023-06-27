//! A process wrapper for running a Protobuf compiler configured for Prost or Tonic output in a Bazel rule.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::env;
use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process;

type ProtocResult<T> = Result<T, String>;

/// Convert a `std::io::Error` to a `String`.
fn from_error<T: ToString>(e: T) -> String {
    format!("IO error: {}", e.to_string())
}

/// Locate prost outputs in the protoc output directory.
fn find_generated_rust_files(out_dir: &Path) -> ProtocResult<BTreeSet<PathBuf>> {
    let mut all_rs_files: BTreeSet<PathBuf> = BTreeSet::new();
    for entry in fs::read_dir(out_dir).map_err(from_error)? {
        let entry = entry.expect("Failed to read entry");
        let path = entry.path();
        if path.is_dir() {
            for f in find_generated_rust_files(&path)? {
                all_rs_files.insert(f);
            }
        } else if let Some(ext) = path.extension() {
            if ext == "rs" {
                all_rs_files.insert(path);
            }
        } else if let Some(name) = path.file_name() {
            if name == "_" {
                let rs_name = path.parent().expect("Failed to get parent").join("_.rs");
                fs::rename(&path, &rs_name).map_err(from_error)?;
                all_rs_files.insert(rs_name);
            }
        }
    }

    Ok(all_rs_files)
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
fn generate_lib_rs(prost_outputs: &BTreeSet<PathBuf>, is_tonic: bool) -> ProtocResult<String> {
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
                contents: fs::read_to_string(path).map_err(from_error)?,
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
    write_module(&mut content, &module_info, "", 0)?;
    Ok(content)
}

fn write_module(
    content: &mut String,
    module_info: &BTreeMap<String, Module>,
    module_name: &str,
    depth: usize,
) -> ProtocResult<()> {
    if module_name.is_empty() {
        for submodule_name in module_info.keys() {
            write_module(content, module_info, submodule_name, depth + 1)?;
        }
        return Ok(());
    }
    let module = module_info.get(module_name).expect("Failed to get module");
    let indent = "  ".repeat(depth);
    let is_rust_module = module.name != "_";

    if is_rust_module {
        content
            .write_str(&format!("{}pub mod {} {{\n", indent, module.name))
            .map_err(from_error)?;
    }

    content.write_str(&module.contents).map_err(from_error)?;

    for submodule_name in module.submodules.iter() {
        write_module(
            content,
            module_info,
            [module_name, submodule_name].join(".").as_str(),
            depth + 1,
        )?;
    }

    if is_rust_module {
        content
            .write_str(&format!("{}}}\n", indent))
            .map_err(from_error)?;
    }

    Ok(())
}

/// Compute the `--extern_path` flags for a list of proto files. This is
/// expected to convert proto files into a list of
/// `.example.prost.helloworld=crate_name::example::prost::helloworld`
fn compute_proto_package_info(
    proto_files: &BTreeSet<PathBuf>,
    crate_name: &str,
    protoc: &Path,
    includes: &BTreeSet<String>,
    proto_paths: &[String],
) -> ProtocResult<BTreeSet<String>> {
    let mut extern_paths = BTreeSet::new();
    for proto_file in proto_files.iter() {
        let output = process::Command::new(protoc)
            .args(includes.iter().map(|include| format!("-I{}", include)))
            .arg("--print_free_field_numbers")
            .args(
                proto_paths
                    .iter()
                    .map(|proto_path| format!("--proto_path={}", proto_path)),
            )
            .arg(proto_file)
            .stdout(process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn protoc")
            .wait_with_output()
            .expect("Failed to wait on protoc");

        // check success
        if !output.status.success() {
            return Err(format!(
                "Failed to run protoc: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = std::str::from_utf8(&output.stdout).expect("Failed to parse stdout");
        for line in stdout.lines() {
            let text = line.trim();
            if text.is_empty() {
                continue;
            }

            let (absolute, _) = text
                .split_once(' ')
                .ok_or_else(|| format!("Failed to split line: {}", text))?;

            let mut package = "";
            let mut symbol_name = absolute;
            if let Some((package_, symbol_name_)) = absolute.rsplit_once('.') {
                package = package_;
                symbol_name = symbol_name_;
            }
            let symbol = format!("{}::{}", package.replace('.', "::"), symbol_name);
            let extern_path = format!(".{}={}::{}", absolute, crate_name, symbol.trim_matches(':'));
            if !extern_paths.insert(extern_path.clone()) {
                panic!("Duplicate extern: {}", extern_path);
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
    proto_files: BTreeSet<PathBuf>,

    /// The include directories.
    includes: BTreeSet<String>,

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
    fn parse() -> ProtocResult<Args> {
        let mut protoc: Option<PathBuf> = None;
        let mut out_dir: Option<PathBuf> = None;
        let mut crate_name: Option<String> = None;
        let mut package_info_file: Option<PathBuf> = None;
        let mut proto_files: BTreeSet<PathBuf> = BTreeSet::new();
        let mut includes: BTreeSet<String> = BTreeSet::new();
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
                proto_files.insert(PathBuf::from(arg));
                continue;
            }

            if arg.starts_with("-I") {
                includes.insert(
                    arg.strip_prefix("-I")
                        .expect("Failed to strip -I")
                        .to_string(),
                );
                continue;
            }

            if arg == "--is_tonic" {
                is_tonic = true;
                println!("IS TONIC");
                continue;
            }

            if !arg.contains('=') {
                extra_args.push(arg);
                continue;
            }

            let part = arg
                .split_once('=')
                .ok_or_else(|| format!("Failed to parse argument `{arg}`"))?;
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
                    for line in fs::read_to_string(value).map_err(from_error)?.lines() {
                        let path = PathBuf::from(line.trim());
                        for flag in fs::read_to_string(path).map_err(from_error)?.lines() {
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

fn main() -> ProtocResult<()> {
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
    } = Args::parse()?;

    let mut cmd = process::Command::new(&protoc);
    cmd.arg(format!("--prost_out={}", out_dir.display()));
    if is_tonic {
        cmd.arg(format!("--tonic_out={}", out_dir.display()));
    }
    cmd.args(includes.iter().map(|include| format!("-I{}", include)));
    cmd.args(
        proto_paths
            .iter()
            .map(|proto_path| format!("--proto_path={}", proto_path)),
    );
    cmd.args(extra_args);
    cmd.args(&proto_files);

    let output = cmd.output().map_err(from_error)?;
    if !output.status.success() {
        return Err(format!(
            "protoc failed with status: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Not all proto files will consistently produce `.rs` or `.tonic.rs` files. This is
    // caused by the proto file being transpiled not having an RPC service or other protos
    // defined (a natural and expected situation). To guarantee consistent outputs, all
    // `.rs` files are either renamed to `.tonic.rs` if there is no `.tonic.rs` or prepended
    // to the existing `.tonic.rs`.
    if is_tonic {
        let tonic_files: BTreeSet<PathBuf> = find_generated_rust_files(&out_dir)?;

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
                fs::rename(tonic_file, real_tonic_file).map_err(from_error)?;
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
                    let rs_content = fs::read_to_string(&rs_file).map_err(from_error)?;
                    let tonic_content = fs::read_to_string(tonic_file).map_err(from_error)?;
                    fs::write(tonic_file, format!("{}\n{}", rs_content, tonic_content))
                        .map_err(from_error)?;
                    fs::remove_file(&rs_file).map_err(from_error)?;
                }
            }
        }
    }

    // Locate all prost-generated outputs.
    let rust_files: BTreeSet<PathBuf> = find_generated_rust_files(&out_dir)?;
    if rust_files.is_empty() {
        return Err("Failed to find any outputs".to_string());
    }

    let package_info: BTreeSet<String> =
        compute_proto_package_info(&proto_files, &crate_name, &protoc, &includes, &proto_paths)?;

    // Write outputs
    fs::write(&out_librs, generate_lib_rs(&rust_files, is_tonic)?).map_err(from_error)?;
    fs::write(
        package_info_file,
        package_info.into_iter().collect::<Vec<_>>().join("\n"),
    )
    .map_err(from_error)?;

    // Finally run rustfmt on the output lib.rs file
    if let Some(rustfmt) = rustfmt {
        let fmt_status = process::Command::new(rustfmt)
            .arg("--edition")
            .arg("2021")
            .arg("--quiet")
            .arg(&out_librs)
            .status()
            .map_err(from_error)?;
        if !fmt_status.success() {
            Err(format!(
                "rustfmt failed with exit code: {}",
                fmt_status.code().expect("Failed to get exit code")
            ))?;
        }
    }

    Ok(())
}
