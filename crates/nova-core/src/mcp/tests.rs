#[cfg(test)]
use crate::mcp::types::{JsonRpcRequest, JsonRpcResponse, McpContent};
#[cfg(test)]
use serde_json::json;

#[test]
fn jsonrpc_request_serialize() {
    let req = JsonRpcRequest {
        jsonrpc: "2.0",
        id: Some(42),
        method: "initialize".to_string(),
        params: Some(json!({"foo": "bar"})),
    };
    let s = serde_json::to_string(&req).unwrap();
    let expected = r#"{"jsonrpc":"2.0","id":42,"method":"initialize","params":{"foo":"bar"}}"#;
    assert_eq!(s, expected);
}

#[test]
fn jsonrpc_response_deserialize() {
    let txt = r#"{"jsonrpc":"2.0","id":42,"result":{"status":"ok"}}"#;
    let resp: JsonRpcResponse = serde_json::from_str(txt).unwrap();
    assert_eq!(resp.jsonrpc, "2.0");
    assert_eq!(resp.id, Some(42));
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["status"], "ok");
}

#[test]
fn mcp_content_variant() {
    let txt = McpContent::Text {
        text: "hello".to_string(),
    };
    match txt {
        McpContent::Text { text } => assert_eq!(text, "hello"),
        _ => panic!("unexpected variant"),
    }
}
