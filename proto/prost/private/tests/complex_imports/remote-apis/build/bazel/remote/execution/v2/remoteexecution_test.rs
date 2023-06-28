//! Tests the remote execution protos.

use remoteexecution_proto::build::bazel::remote::execution::v2::Digest;

#[test]
fn test_remote_execution() {
    let digest = Digest {
        hash: "hash".to_string(),
        size_bytes: 50,
    };
    assert_eq!(digest.hash, "hash");
}
