//! Tests the semver protos.

use semver_proto::build::bazel::semver::SemVer;

#[test]
fn test_semver() {
    let semver = SemVer {
        major: 1,
        minor: 2,
        patch: 3,
        prerelease: "prerelease".to_string(),
    };

    assert_eq!(semver.major, 1);
}
