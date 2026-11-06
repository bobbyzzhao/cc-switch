use axum::{
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("上游响应体超过大小上限: {0} 字节")]
    ResponseBodyTooLarge(usize),

    #[error("服务器已在运行")]
    AlreadyRunning,

    #[error("服务器未运行")]
    NotRunning,

    #[error("地址绑定失败: {0}")]
    BindFailed(String),

    #[error("停止超时")]
    StopTimeout,

    #[error("停止失败: {0}")]
    StopFailed(String),

    #[error("请求转发失败: {0}")]
    ForwardFailed(String),

    #[error("无可用的Provider")]
    NoAvailableProvider,

    #[error("所有供应商已熔断，无可用渠道")]
    AllProvidersCircuitOpen,

    #[error("未配置供应商")]
    NoProvidersConfigured,

    #[allow(dead_code)]
    #[error("Provider不健康: {0}")]
    ProviderUnhealthy(String),

    #[error("上游错误 (状态码 {status}): {body:?}")]
    UpstreamError { status: u16, body: Option<String> },

    #[error("上游限流 (429): {body:?}")]
    UpstreamRateLimited {
        body: Option<String>,
        retry_after: Option<String>,
        retry_after_ms: Option<String>,
    },

    #[error("超过最大重试次数")]
    MaxRetriesExceeded,

    #[error("数据库错误: {0}")]
    DatabaseError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[allow(dead_code)]
    #[error("格式转换错误: {0}")]
    TransformError(String),

    #[allow(dead_code)]
    #[error("无效的请求: {0}")]
    InvalidRequest(String),

    #[error("超时: {0}")]
    Timeout(String),

    /// 流式响应空闲超时
    #[allow(dead_code)]
    #[error("流式响应空闲超时: {0}秒无数据")]
    StreamIdleTimeout(u64),

    /// 认证错误
    #[error("认证失败: {0}")]
    AuthError(String),

    /// Local pre-exhaustion policy, never an upstream usage-limit response.
    #[error("Codex 官方额度保护 (quota protection): {0}")]
    CodexQuotaProtection(String),

    #[allow(dead_code)]
    #[error("内部错误: {0}")]
    Internal(String),
}

impl ProxyError {
    pub fn upstream_response(&self) -> Option<(u16, Option<&str>)> {
        match self {
            Self::UpstreamError { status, body } => Some((*status, body.as_deref())),
            Self::UpstreamRateLimited { body, .. } => Some((429, body.as_deref())),
            _ => None,
        }
    }

    /// Codex's 429 recovery requires a server wait hint. Preserve a real hint;
    /// only a known transient rate_limit_exceeded gets the proxy's 5s fallback.
    /// Billing/quota failures and other HTTP errors must not acquire retries.
    pub fn retry_response_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Self::UpstreamRateLimited {
            retry_after,
            retry_after_ms,
            ..
        } = self
        {
            for (name, value) in [
                ("retry-after", retry_after),
                ("retry-after-ms", retry_after_ms),
            ] {
                if let Some(value) = value.as_ref().and_then(|v| HeaderValue::from_str(v).ok()) {
                    headers.insert(name, value);
                }
            }
            if !headers.contains_key("retry-after") {
                if let Some(ms) = retry_after_ms.as_ref().and_then(|v| v.parse::<u64>().ok()) {
                    let seconds = ms.div_ceil(1000).max(1);
                    if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                        headers.insert("retry-after", value);
                    }
                }
            }
        }
        if !headers.contains_key("retry-after") {
            if let Some((429, Some(body))) = self.upstream_response() {
                let transient = serde_json::from_str::<serde_json::Value>(body)
                    .ok()
                    .is_some_and(|v| {
                        v.pointer("/error/code").and_then(|c| c.as_str())
                            == Some("rate_limit_exceeded")
                    });
                if transient {
                    headers.insert("retry-after", HeaderValue::from_static("5"));
                }
            }
        }
        headers
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, body) = match self.upstream_response() {
            Some((upstream_status, upstream_body)) => {
                let http_status =
                    StatusCode::from_u16(upstream_status).unwrap_or(StatusCode::BAD_GATEWAY);

                // 尝试解析上游响应体为 JSON，如果失败则包装为字符串
                let error_body = if let Some(body_str) = upstream_body {
                    if let Ok(json_body) = serde_json::from_str::<serde_json::Value>(body_str) {
                        // 上游返回的是 JSON，直接透传
                        json_body
                    } else {
                        // 上游返回的不是 JSON，包装为错误消息
                        json!({
                            "error": {
                                "message": body_str,
                                "type": "upstream_error",
                            }
                        })
                    }
                } else {
                    json!({
                        "error": {
                            "message": format!("Upstream error (status {})", upstream_status),
                            "type": "upstream_error",
                        }
                    })
                };

                (http_status, error_body)
            }
            _ => {
                let (http_status, message) = match &self {
                    ProxyError::AlreadyRunning => (StatusCode::CONFLICT, self.to_string()),
                    ProxyError::NotRunning => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
                    ProxyError::BindFailed(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::StopTimeout => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::StopFailed(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::ForwardFailed(_) => (StatusCode::BAD_GATEWAY, self.to_string()),
                    ProxyError::NoAvailableProvider => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::AllProvidersCircuitOpen => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::NoProvidersConfigured => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::ProviderUnhealthy(_) => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::MaxRetriesExceeded => {
                        (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
                    }
                    ProxyError::DatabaseError(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::ConfigError(_) => (StatusCode::BAD_REQUEST, self.to_string()),
                    ProxyError::TransformError(_) => {
                        (StatusCode::UNPROCESSABLE_ENTITY, self.to_string())
                    }
                    ProxyError::InvalidRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
                    ProxyError::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, self.to_string()),
                    ProxyError::StreamIdleTimeout(_) => {
                        (StatusCode::GATEWAY_TIMEOUT, self.to_string())
                    }
                    ProxyError::AuthError(_) => (StatusCode::UNAUTHORIZED, self.to_string()),
                    ProxyError::CodexQuotaProtection(_) => {
                        (StatusCode::FORBIDDEN, self.to_string())
                    }
                    ProxyError::Internal(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                    }
                    ProxyError::ResponseBodyTooLarge(_) => {
                        (StatusCode::BAD_GATEWAY, self.to_string())
                    }
                    ProxyError::UpstreamError { .. } | ProxyError::UpstreamRateLimited { .. } => {
                        unreachable!()
                    }
                };

                let error_body = json!({
                    "error": {
                        "message": message,
                        "type": "proxy_error",
                    }
                });

                (http_status, error_body)
            }
        };

        let mut response = (status, Json(body)).into_response();
        response.headers_mut().extend(self.retry_response_headers());
        response
    }
}

/// 错误分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// 可重试错误（网络问题、5xx）
    Retryable, // 网络超时、5xx 错误
    /// 不可重试错误（4xx、认证失败）
    NonRetryable, // 认证失败、参数错误、4xx 错误
    #[allow(dead_code)]
    ClientAbort, // 客户端主动中断
}

/// 把错误连同 `source()` 原因链拼成一行。
///
/// reqwest / hyper 的外层文本只有「error sending request」「error decoding response body」
/// 这类描述，真正能区分 DNS、TCP、TLS、连接被重置的信息都在原因链里。
pub fn describe_error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        // 有的错误会把原因也写进自己的文本，重复的就不再追加
        if !cause_text.is_empty() && !text.ends_with(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

/// reqwest 错误的文本带完整 URL（查询参数里可能有 Key），先去掉 URL 再展开原因链。
pub fn describe_reqwest_error(error: reqwest::Error) -> String {
    describe_error_chain(&error.without_url())
}

/// 判断错误是否可重试
#[allow(dead_code)]
pub fn categorize_error(error: &reqwest::Error) -> ErrorCategory {
    if error.is_timeout() || error.is_connect() {
        return ErrorCategory::Retryable;
    }

    if let Some(status) = error.status() {
        if status.is_server_error() {
            ErrorCategory::Retryable
        } else if status.is_client_error() {
            ErrorCategory::NonRetryable
        } else {
            ErrorCategory::Retryable
        }
    } else {
        ErrorCategory::Retryable
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_retry_headers_preserve_upstream_wait_hints() {
        for (retry_after, retry_after_ms, expected) in [
            (Some("17"), None, Some("17")),
            (Some("0"), Some("1500"), Some("0")),
            (
                Some("Wed, 21 Oct 2026 07:28:00 GMT"),
                None,
                Some("Wed, 21 Oct 2026 07:28:00 GMT"),
            ),
            (None, Some("1501"), Some("2")),
            (None, Some("0"), Some("1")),
            (Some("invalid\r\nheader"), None, None),
            (None, Some("invalid"), None),
        ] {
            let error = ProxyError::UpstreamRateLimited {
                body: Some(r#"{"error":{"code":"unknown_limit"}}"#.into()),
                retry_after: retry_after.map(str::to_owned),
                retry_after_ms: retry_after_ms.map(str::to_owned),
            };
            let headers = error.retry_response_headers();
            assert_eq!(
                headers.get("retry-after").and_then(|v| v.to_str().ok()),
                expected
            );
            assert_eq!(
                headers.get("retry-after-ms").and_then(|v| v.to_str().ok()),
                retry_after_ms
            );
        }
    }

    #[test]
    fn rate_limit_retry_fallback_excludes_quota_and_other_errors() {
        for (status, body, expected) in [
            (
                429,
                Some(r#"{"error":{"code":"rate_limit_exceeded"}}"#),
                Some("5"),
            ),
            (
                429,
                Some(r#"{"error":{"code":"insufficient_quota"}}"#),
                None,
            ),
            (429, Some(r#"{"error":{"type":"rate_limit_error"}}"#), None),
            (429, Some("rate_limit_exceeded"), None),
            (429, None, None),
            (
                400,
                Some(r#"{"error":{"code":"rate_limit_exceeded"}}"#),
                None,
            ),
            (
                503,
                Some(r#"{"error":{"code":"rate_limit_exceeded"}}"#),
                None,
            ),
        ] {
            let error = ProxyError::UpstreamError {
                status,
                body: body.map(str::to_owned),
            };
            assert_eq!(
                error
                    .retry_response_headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()),
                expected
            );
        }
    }

    #[tokio::test]
    async fn rate_limit_response_keeps_status_and_upstream_error_body() {
        let body = json!({"error": {"code": "rate_limit_exceeded", "message": "Please wait", "type": "rate_limit_error"}});
        let response = ProxyError::UpstreamRateLimited {
            body: Some(body.to_string()),
            retry_after: Some("23".into()),
            retry_after_ms: None,
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["retry-after"], "23");
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            body
        );
    }

    #[derive(Debug)]
    struct Outer(std::io::Error);

    impl std::fmt::Display for Outer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("error sending request")
        }
    }

    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn describe_error_chain_appends_every_cause() {
        let error = Outer(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "tcp connect error",
        ));
        assert_eq!(
            describe_error_chain(&error),
            "error sending request: tcp connect error"
        );
    }

    #[tokio::test]
    async fn describe_reqwest_error_keeps_cause_and_drops_url() {
        // 先占一个端口再释放，保证连接被拒绝
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let error = reqwest::Client::new()
            .get(format!(
                "http://127.0.0.1:{port}/v1/responses?key=secret-token"
            ))
            .send()
            .await
            .expect_err("connection should be refused");
        assert!(error.is_connect());

        let text = describe_reqwest_error(error);
        assert!(text.starts_with("error sending request: "), "{text}");
        assert!(!text.contains("secret-token"), "{text}");
    }
}
