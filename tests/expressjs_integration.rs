// Integration test: validates the adapter works with a real Express.js app.
//
// Prerequisites: the Express.js example app must be running on port 8080:
//   cd examples/expressjs/app && npm install && PORT=8080 node index.js
//
// Run this test:
//   cargo test --test expressjs_integration -- --nocapture

use azure_functions_web_adapter::config::{AdapterConfig, ReadinessProtocol, StatusRange};
use azure_functions_web_adapter::http_forwarder::HttpForwarder;
use azure_functions_web_adapter::proto::{
    typed_data, InvocationRequest, ParameterBinding, RpcHttp, TypedData,
};
use azure_functions_web_adapter::readiness;
use std::collections::HashMap;
use std::time::Duration;

/// Helper to create an InvocationRequest from an RpcHttp trigger
fn make_invocation(method: &str, url: &str, body: Option<&str>) -> InvocationRequest {
    let rpc_body = body.map(|b| {
        Box::new(TypedData {
            data: Some(typed_data::Data::StringValue(b.to_string())),
        })
    });

    let mut headers = HashMap::new();
    if body.is_some() {
        headers.insert("content-type".to_string(), "application/json".to_string());
    }

    let rpc_http = RpcHttp {
        method: method.to_string(),
        url: url.to_string(),
        headers,
        body: rpc_body,
        params: HashMap::new(),
        status_code: String::new(),
        nullable_headers: HashMap::new(),
        nullable_params: HashMap::new(),
        query: HashMap::new(),
        enable_content_negotiation: false,
        raw_body: None,
    };

    let input_binding = ParameterBinding {
        name: "req".to_string(),
        data: Some(TypedData {
            data: Some(typed_data::Data::Http(Box::new(rpc_http))),
        }),
    };

    InvocationRequest {
        invocation_id: "test-invocation-1".to_string(),
        function_id: "test-function-1".to_string(),
        input_data: vec![input_binding],
        trigger_metadata: HashMap::new(),
        trace_context: None,
        retry_context: None,
    }
}

fn test_config() -> AdapterConfig {
    AdapterConfig {
        host: "127.0.0.1".to_string(),
        port: 8080,
        readiness_check_port: 8080,
        readiness_check_path: "/".to_string(),
        readiness_check_protocol: ReadinessProtocol::Http,
        readiness_healthy_status: StatusRange::parse("100-499"),
        readiness_check_interval: Duration::from_millis(10),
        readiness_check_timeout: Duration::from_secs(5),
        startup_command: None,
        remove_base_path: None,
        enable_compression: false,
    }
}

#[tokio::test]
async fn test_readiness_check_http() {
    let config = test_config();
    let result = readiness::wait_until_ready(&config).await;
    assert!(result.is_ok(), "Express.js app should be ready on port 8080");
    let elapsed = result.unwrap();
    println!("Readiness check passed in {:?}", elapsed);
}

#[tokio::test]
async fn test_readiness_check_tcp() {
    let config = AdapterConfig {
        readiness_check_protocol: ReadinessProtocol::Tcp,
        ..test_config()
    };
    let result = readiness::wait_until_ready(&config).await;
    assert!(result.is_ok(), "TCP readiness check should pass");
}

#[tokio::test]
async fn test_forward_get_root() {
    let config = test_config();
    let forwarder = HttpForwarder::new(&config);

    let request = make_invocation("GET", "http://localhost:8080/", None);
    let response = forwarder.forward(&request).await;

    assert_eq!(response.invocation_id, "test-invocation-1");
    assert!(response.result.is_some());
    let status = response.result.as_ref().unwrap().status;
    assert_eq!(status, 1, "Should be Success (1)"); // Success = 1

    // Check the HTTP response output
    assert!(!response.output_data.is_empty());
    let output = &response.output_data[0];
    assert_eq!(output.name, "$return");

    if let Some(TypedData {
        data: Some(typed_data::Data::Http(ref http)),
    }) = output.data
    {
        assert_eq!(http.status_code, "200");
        // Body should contain our Express.js response
        if let Some(ref body) = http.body {
            if let Some(typed_data::Data::StringValue(ref s)) = body.data {
                assert!(s.contains("Hello from Express.js on Azure Functions!"));
                assert!(s.contains("Express.js"));
                println!("GET / response body: {}", s);
            } else {
                panic!("Expected string body");
            }
        } else {
            panic!("Expected body in response");
        }
    } else {
        panic!("Expected HTTP response in output_data");
    }
}

#[tokio::test]
async fn test_forward_get_hello_with_query() {
    let config = test_config();
    let forwarder = HttpForwarder::new(&config);

    let request = make_invocation("GET", "http://localhost:8080/api/hello?name=AzureAdapter", None);
    let response = forwarder.forward(&request).await;

    let output = &response.output_data[0];
    if let Some(TypedData {
        data: Some(typed_data::Data::Http(ref http)),
    }) = output.data
    {
        assert_eq!(http.status_code, "200");
        if let Some(ref body) = http.body {
            if let Some(typed_data::Data::StringValue(ref s)) = body.data {
                assert!(s.contains("Hello, AzureAdapter!"));
                println!("GET /api/hello?name=AzureAdapter response: {}", s);
            }
        }
    }
}

#[tokio::test]
async fn test_forward_post_echo() {
    let config = test_config();
    let forwarder = HttpForwarder::new(&config);

    let body = r#"{"test": "data", "number": 42}"#;
    let request = make_invocation("POST", "http://localhost:8080/api/echo", Some(body));
    let response = forwarder.forward(&request).await;

    let output = &response.output_data[0];
    if let Some(TypedData {
        data: Some(typed_data::Data::Http(ref http)),
    }) = output.data
    {
        assert_eq!(http.status_code, "200");
        if let Some(ref body) = http.body {
            if let Some(typed_data::Data::StringValue(ref s)) = body.data {
                assert!(s.contains("\"test\""));
                assert!(s.contains("42"));
                println!("POST /api/echo response: {}", s);
            }
        }
    }
}

#[tokio::test]
async fn test_forward_health_check() {
    let config = test_config();
    let forwarder = HttpForwarder::new(&config);

    let request = make_invocation("GET", "http://localhost:8080/api/health", None);
    let response = forwarder.forward(&request).await;

    let output = &response.output_data[0];
    if let Some(TypedData {
        data: Some(typed_data::Data::Http(ref http)),
    }) = output.data
    {
        assert_eq!(http.status_code, "200");
        if let Some(ref body) = http.body {
            if let Some(typed_data::Data::StringValue(ref s)) = body.data {
                assert!(s.contains("healthy"));
                println!("GET /api/health response: {}", s);
            }
        }
    }
}

#[tokio::test]
async fn test_forward_404_route() {
    let config = test_config();
    let forwarder = HttpForwarder::new(&config);

    let request = make_invocation("GET", "http://localhost:8080/nonexistent", None);
    let response = forwarder.forward(&request).await;

    // Should still succeed at the adapter level (Express returns 404)
    assert!(response.result.is_some());
    let status = response.result.as_ref().unwrap().status;
    assert_eq!(status, 1, "Adapter should succeed even for 404");

    let output = &response.output_data[0];
    if let Some(TypedData {
        data: Some(typed_data::Data::Http(ref http)),
    }) = output.data
    {
        assert_eq!(http.status_code, "404");
        println!("GET /nonexistent → status {}", http.status_code);
    }
}
