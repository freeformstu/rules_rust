//! Tests the longrunning protos.

use longrunning_proto::google::longrunning::GetOperationRequest;

#[test]
fn test_longrunning() {
    let get_operation_request = GetOperationRequest {
        name: "name".to_string(),
    };

    assert_eq!(get_operation_request.name, "name")
}
