//! Tests the annotations protos.

use annotations_proto::google::api::http_rule::Pattern;
use annotations_proto::google::api::HttpRule;

#[test]
fn test_annotations() {
    let http_rule = HttpRule {
        pattern: Some(Pattern::Get("/v1/{name=shelves/*}/books".to_string())),
        ..HttpRule::default()
    };

    assert_eq!(
        http_rule.pattern,
        Some(Pattern::Get("/v1/{name=shelves/*}/books".to_string()))
    );
}
