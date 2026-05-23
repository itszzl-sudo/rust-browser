//! CORS — 跨域资源共享检查
//!
//! 实现 Fetch 规范中的 CORS 检查算法：
//! - 简单请求 vs 预检请求
//! - Access-Control-Allow-Origin 校验
//! - Access-Control-Allow-Methods/Credentials/Headers

use log::warn;

/// CORS 检查结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsResult {
    /// 允许跨域
    Allowed,
    /// 被 CORS 策略阻止
    Blocked(String),
}

/// CORS 检查器
pub struct CorsChecker {
    /// 当前页面 origin
    page_origin: String,
}

impl CorsChecker {
    pub fn new(page_url: &str) -> Self {
        Self {
            page_origin: extract_origin(page_url),
        }
    }

    /// 更新页面 URL（导航时调用）
    pub fn set_page_url(&mut self, url: &str) {
        self.page_origin = extract_origin(url);
    }

    /// 判断请求是否为跨域
    pub fn is_cross_origin(&self, request_url: &str) -> bool {
        let req_origin = extract_origin(request_url);
        self.page_origin != req_origin
    }

    /// 检查 CORS 简单请求
    ///
    /// 对简单请求（GET/HEAD/POST，无自定义头，content-type 为简单类型），
    /// 只需检查 Access-Control-Allow-Origin。
    pub fn check_simple_request(
        &self,
        request_url: &str,
        response_headers: &[(String, String)],
    ) -> CorsResult {
        if !self.is_cross_origin(request_url) {
            return CorsResult::Allowed; // 同源请求不需要 CORS
        }

        let allow_origin = response_headers.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin"))
            .map(|(_, v)| v.as_str());

        match allow_origin {
            Some("*") => CorsResult::Allowed,
            Some(origin) if origin == self.page_origin => CorsResult::Allowed,
            Some(origin) => {
                let msg = format!("CORS: Origin '{}' not allowed by Access-Control-Allow-Origin '{}'",
                    self.page_origin, origin);
                warn!("{}", msg);
                CorsResult::Blocked(msg)
            },
            None => {
                let msg = format!("CORS: No Access-Control-Allow-Origin header for cross-origin request to '{}'", request_url);
                warn!("{}", msg);
                CorsResult::Blocked(msg)
            }
        }
    }

    /// 检查 CORS 凭据请求
    ///
    /// 带凭据（cookies）的请求：
    /// - Access-Control-Allow-Origin 不能为 *
    /// - 必须有 Access-Control-Allow-Credentials: true
    pub fn check_credentials(
        &self,
        request_url: &str,
        response_headers: &[(String, String)],
    ) -> CorsResult {
        if !self.is_cross_origin(request_url) {
            return CorsResult::Allowed;
        }

        // Origin 不能为 *
        let origin_check = self.check_simple_request(request_url, response_headers);
        if let CorsResult::Blocked(_) = origin_check {
            return origin_check;
        }

        let allow_origin = response_headers.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin"))
            .map(|(_, v)| v.as_str());

        if allow_origin == Some("*") {
            return CorsResult::Blocked(
                "CORS: Access-Control-Allow-Origin cannot be '*' with credentials".to_string()
            );
        }

        let allow_credentials = response_headers.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-credentials"))
            .map(|(_, v)| v.as_str());

        match allow_credentials {
            Some("true") => CorsResult::Allowed,
            _ => CorsResult::Blocked(
                "CORS: No Access-Control-Allow-Credentials: true header".to_string()
            ),
        }
    }
}

fn extract_origin(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        parsed.origin().ascii_serialization()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_origin_allowed() {
        let checker = CorsChecker::new("https://example.com/page");
        let headers = vec![]; // 同源不需要 CORS 头
        assert_eq!(
            checker.check_simple_request("https://example.com/api", &headers),
            CorsResult::Allowed
        );
    }

    #[test]
    fn test_cross_origin_with_allow() {
        let checker = CorsChecker::new("https://myapp.com/page");
        let headers = vec![
            ("access-control-allow-origin".to_string(), "https://myapp.com".to_string()),
        ];
        assert_eq!(
            checker.check_simple_request("https://api.other.com/data", &headers),
            CorsResult::Allowed
        );
    }

    #[test]
    fn test_cross_origin_wildcard() {
        let checker = CorsChecker::new("https://myapp.com/page");
        let headers = vec![
            ("access-control-allow-origin".to_string(), "*".to_string()),
        ];
        assert_eq!(
            checker.check_simple_request("https://api.other.com/data", &headers),
            CorsResult::Allowed
        );
    }

    #[test]
    fn test_cross_origin_blocked() {
        let checker = CorsChecker::new("https://myapp.com/page");
        let headers = vec![
            ("access-control-allow-origin".to_string(), "https://notmyapp.com".to_string()),
        ];
        assert!(matches!(
            checker.check_simple_request("https://api.other.com/data", &headers),
            CorsResult::Blocked(_)
        ));
    }

    #[test]
    fn test_no_cors_header() {
        let checker = CorsChecker::new("https://myapp.com/page");
        let headers: Vec<(String, String)> = vec![];
        assert!(matches!(
            checker.check_simple_request("https://api.other.com/data", &headers),
            CorsResult::Blocked(_)
        ));
    }

    #[test]
    fn test_credentials_no_wildcard() {
        let checker = CorsChecker::new("https://myapp.com/page");
        let headers = vec![
            ("access-control-allow-origin".to_string(), "*".to_string()),
            ("access-control-allow-credentials".to_string(), "true".to_string()),
        ];
        assert!(matches!(
            checker.check_credentials("https://api.other.com/data", &headers),
            CorsResult::Blocked(_)
        ));
    }

    #[test]
    fn test_is_cross_origin() {
        let checker = CorsChecker::new("https://myapp.com/page");
        assert!(checker.is_cross_origin("https://other.com/api"));
        assert!(!checker.is_cross_origin("https://myapp.com/other"));
        assert!(!checker.is_cross_origin("https://myapp.com:443/page")); // 默认端口
    }
}
