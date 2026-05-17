//! JS 引擎 —— 基于 obscura-js (deno_core/V8)
//!
//! 提供 JavaScript 执行能力。

use log::{info, warn};
use obscura_dom::tree::DomTree;

/// JS 运行时封装
pub struct JsEngine {
    runtime: Option<obscura_js::runtime::ObscuraJsRuntime>,
}

impl JsEngine {
    pub fn new() -> Self {
        info!("初始化 JS 引擎 (obscura-js + deno_core)");
        Self { runtime: None }
    }

    /// 初始化 V8 运行时
    pub fn initialize(&mut self, url: &str) -> Result<(), String> {
        if self.runtime.is_some() {
            return Ok(());
        }
        info!("启动 V8 运行时, URL: {}", url);
        let rt = obscura_js::runtime::ObscuraJsRuntime::with_base_url(url);
        self.runtime = Some(rt);
        info!("JS 引擎已就绪");
        Ok(())
    }

    /// 设置 DOM 树
    pub fn set_dom(&self, dom: DomTree) {
        if let Some(ref rt) = self.runtime {
            rt.set_dom(dom);
        }
    }

    /// 设置当前 URL
    pub fn set_url(&self, url: &str) {
        if let Some(ref rt) = self.runtime {
            rt.set_url(url);
        }
    }

    /// 执行 JavaScript
    pub fn evaluate(&mut self, code: &str) -> Result<String, String> {
        match self.runtime.as_mut() {
            Some(rt) => {
                let result = rt.evaluate(code).map_err(|e| format!("{:?}", e))?;
                Ok(result.to_string())
            }
            None => Err("JS 引擎未初始化".to_string()),
        }
    }

    pub fn is_ready(&self) -> bool {
        self.runtime.is_some()
    }
}

impl Default for JsEngine {
    fn default() -> Self {
        Self::new()
    }
}
