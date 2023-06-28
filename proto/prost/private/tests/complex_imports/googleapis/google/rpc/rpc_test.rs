//! Tests the rpc protos.

use code_proto::google::rpc::Code;
use duration_proto::google::protobuf::Duration;
use error_details_proto::google::rpc::RetryInfo;
use status_proto::google::rpc::Status;

#[test]
fn test_rpc() {
    let retry_info = RetryInfo {
        retry_delay: Some(Duration {
            seconds: 1,
            nanos: 2,
        }),
    };
    assert_eq!(
        retry_info.retry_delay,
        Some(Duration {
            seconds: 1,
            nanos: 2,
        })
    );

    let status = Status {
        code: Code::Ok.into(),
        message: "message".to_string(),
        details: vec![],
    };

    assert_eq!(status.code, Code::Ok.into());
}
