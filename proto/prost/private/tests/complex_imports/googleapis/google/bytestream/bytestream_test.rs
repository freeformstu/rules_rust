//! Tests the bytestream protos.

use bytestream_proto::google::bytestream::WriteRequest;

#[test]
fn test_bytestream() {
    let write_request = WriteRequest {
        resource_name: "resource_name".to_string(),
        write_offset: 0,
        finish_write: false,
        data: vec![0, 1, 2, 3],
    };

    assert_eq!(write_request.resource_name, "resource_name")
}
