//! lava-serverless — typed `(deflava-serverless-function …)` for serverless
//! functions across AWS Lambda, GCP Functions, and Cloudflare Workers.
//!
//! One typed `ServerlessFunction` shape; the typed [`Provider`] enum
//! dispatches to provider-specific terraform.json emission. The
//! magma renderer applies the resulting JSON; lava-test asserts
//! against it.
//!
//! ## Form
//!
//! ```lisp
//! (deflava-serverless-function api-handler
//!   :provider aws
//!   :runtime nodejs20
//!   :handler "index.handler"
//!   :code-uri "./dist/handler.zip"
//!   :memory 512
//!   :timeout 30
//!   :env (:NODE_ENV "production" :LOG_LEVEL "info"))
//! ```

#![allow(clippy::module_name_repetitions)]

use indexmap::IndexMap;
use lava_eval::{parse_all, Sx};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

/// Provider-specific dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Aws,
    Gcp,
    CloudflareWorker,
}

impl Provider {
    /// Stable lowercase token for serialization + CLI surfaces.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Gcp => "gcp",
            Self::CloudflareWorker => "cloudflare-worker",
        }
    }
}

/// One serverless-function declaration. Provider-agnostic by shape;
/// `provider` selects the terraform.json emission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerlessFunction {
    pub name: String,
    pub provider: Provider,
    pub runtime: String,
    pub handler: String,
    pub code_uri: String,
    #[serde(default = "default_memory")]
    pub memory: u32,
    #[serde(default = "default_timeout")]
    pub timeout: u32,
    #[serde(default)]
    pub env: IndexMap<String, String>,
    #[serde(default)]
    pub doc: Option<String>,
}

const fn default_memory() -> u32 {
    128
}
const fn default_timeout() -> u32 {
    3
}

#[derive(Debug, Error)]
pub enum ServerlessParseError {
    #[error("parse: {0}")]
    Parse(#[from] lava_eval::ParseError),
    #[error("missing :{0} clause")]
    MissingClause(&'static str),
    #[error("malformed deflava-serverless-function form: {0}")]
    Malformed(String),
    #[error("unknown provider `{0}` (expected aws|gcp|cloudflare-worker)")]
    UnknownProvider(String),
    #[error("bad numeric value for `{0}`: {1}")]
    BadNumber(&'static str, String),
}

/// Scan a source string for every `(deflava-serverless-function …)` form.
///
/// # Errors
/// Surfaces parse errors and per-form shape errors as typed variants.
pub fn functions_in_source(src: &str) -> Result<Vec<ServerlessFunction>, ServerlessParseError> {
    let forms = parse_all(src)?;
    let mut out = Vec::new();
    for form in forms {
        let Some(xs) = form.as_list() else { continue };
        if xs.first().and_then(Sx::as_sym) == Some("deflava-serverless-function") {
            out.push(function_from_form(xs)?);
        }
    }
    Ok(out)
}

fn function_from_form(xs: &[Sx]) -> Result<ServerlessFunction, ServerlessParseError> {
    let name = xs
        .get(1)
        .and_then(Sx::as_sym)
        .or_else(|| xs.get(1).and_then(Sx::as_str))
        .ok_or_else(|| ServerlessParseError::Malformed("missing function name".into()))?
        .to_string();
    let mut provider: Option<Provider> = None;
    let mut runtime: Option<String> = None;
    let mut handler: Option<String> = None;
    let mut code_uri: Option<String> = None;
    let mut memory = default_memory();
    let mut timeout = default_timeout();
    let mut env: IndexMap<String, String> = IndexMap::new();
    let mut doc: Option<String> = None;
    let mut i = 2;
    while i + 1 < xs.len() {
        match xs[i].as_kw() {
            Some("provider") => {
                let p = xs[i + 1]
                    .as_sym()
                    .or_else(|| xs[i + 1].as_str())
                    .ok_or_else(|| ServerlessParseError::Malformed(":provider not a sym".into()))?;
                provider = Some(match p {
                    "aws" => Provider::Aws,
                    "gcp" => Provider::Gcp,
                    "cloudflare-worker" | "cloudflare" => Provider::CloudflareWorker,
                    other => return Err(ServerlessParseError::UnknownProvider(other.to_string())),
                });
            }
            Some("runtime") => {
                runtime = xs[i + 1]
                    .as_sym()
                    .or_else(|| xs[i + 1].as_str())
                    .map(std::string::ToString::to_string);
            }
            Some("handler") => {
                handler = xs[i + 1].as_str().map(std::string::ToString::to_string);
            }
            Some("code-uri") => {
                code_uri = xs[i + 1].as_str().map(std::string::ToString::to_string);
            }
            Some("memory") => {
                memory = xs[i + 1]
                    .as_int()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| ServerlessParseError::BadNumber("memory", format!("{:?}", xs[i + 1])))?;
            }
            Some("timeout") => {
                timeout = xs[i + 1]
                    .as_int()
                    .and_then(|v| u32::try_from(v).ok())
                    .ok_or_else(|| ServerlessParseError::BadNumber("timeout", format!("{:?}", xs[i + 1])))?;
            }
            Some("env") => {
                if let Some(pairs) = xs[i + 1].as_list() {
                    let mut j = 0;
                    while j + 1 < pairs.len() {
                        if let (Some(k), Some(v)) = (pairs[j].as_kw(), pairs[j + 1].as_str()) {
                            env.insert(k.to_string(), v.to_string());
                        }
                        j += 2;
                    }
                }
            }
            Some("doc") => {
                doc = xs[i + 1].as_str().map(std::string::ToString::to_string);
            }
            _ => {}
        }
        i += 2;
    }
    Ok(ServerlessFunction {
        name,
        provider: provider.ok_or(ServerlessParseError::MissingClause("provider"))?,
        runtime: runtime.ok_or(ServerlessParseError::MissingClause("runtime"))?,
        handler: handler.ok_or(ServerlessParseError::MissingClause("handler"))?,
        code_uri: code_uri.ok_or(ServerlessParseError::MissingClause("code-uri"))?,
        memory,
        timeout,
        env,
        doc,
    })
}

/// Emit the terraform.json `resource` entries for this function under
/// the provider-appropriate type. Returns `(type_id, name, body)`
/// triples the architecture renderer can splice into its top-level
/// `resource` map.
#[must_use]
pub fn render_terraform_resources(f: &ServerlessFunction) -> Vec<(String, String, Value)> {
    match f.provider {
        Provider::Aws => render_aws_lambda(f),
        Provider::Gcp => render_gcp_function(f),
        Provider::CloudflareWorker => render_cf_worker(f),
    }
}

fn render_aws_lambda(f: &ServerlessFunction) -> Vec<(String, String, Value)> {
    let mut env_vars = serde_json::Map::new();
    for (k, v) in &f.env {
        env_vars.insert(k.clone(), Value::String(v.clone()));
    }
    let body = json!({
        "function_name": f.name,
        "runtime": f.runtime,
        "handler": f.handler,
        "filename": f.code_uri,
        "memory_size": f.memory,
        "timeout": f.timeout,
        "environment": { "variables": env_vars },
    });
    vec![("aws_lambda_function".to_string(), f.name.clone(), body)]
}

fn render_gcp_function(f: &ServerlessFunction) -> Vec<(String, String, Value)> {
    let mut env_vars = serde_json::Map::new();
    for (k, v) in &f.env {
        env_vars.insert(k.clone(), Value::String(v.clone()));
    }
    let body = json!({
        "name": f.name,
        "runtime": f.runtime,
        "entry_point": f.handler,
        "source_archive_object": f.code_uri,
        "available_memory_mb": f.memory,
        "timeout": f.timeout,
        "environment_variables": env_vars,
    });
    vec![("google_cloudfunctions_function".to_string(), f.name.clone(), body)]
}

fn render_cf_worker(f: &ServerlessFunction) -> Vec<(String, String, Value)> {
    let mut plain_text_bindings = Vec::new();
    for (k, v) in &f.env {
        plain_text_bindings.push(json!({ "name": k, "text": v }));
    }
    let body = json!({
        "name": f.name,
        "content": format!("file://{}", f.code_uri),
        "plain_text_binding": plain_text_bindings,
    });
    vec![(
        "cloudflare_workers_script".to_string(),
        f.name.clone(),
        body,
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_from_form_extracts_all_typed_fields() {
        let src = r#"
            (deflava-serverless-function api-handler
              :provider aws
              :runtime nodejs20
              :handler "index.handler"
              :code-uri "./dist/handler.zip"
              :memory 512
              :timeout 30
              :env (:NODE_ENV "production" :LOG_LEVEL "info"))
        "#;
        let fns = functions_in_source(src).unwrap();
        assert_eq!(fns.len(), 1);
        let f = &fns[0];
        assert_eq!(f.name, "api-handler");
        assert_eq!(f.provider, Provider::Aws);
        assert_eq!(f.runtime, "nodejs20");
        assert_eq!(f.handler, "index.handler");
        assert_eq!(f.code_uri, "./dist/handler.zip");
        assert_eq!(f.memory, 512);
        assert_eq!(f.timeout, 30);
        assert_eq!(f.env["NODE_ENV"], "production");
    }

    #[test]
    fn missing_runtime_surfaces_typed_error() {
        let src = r#"
            (deflava-serverless-function x
              :provider aws
              :handler "h"
              :code-uri "u")
        "#;
        let err = functions_in_source(src).unwrap_err();
        matches!(err, ServerlessParseError::MissingClause("runtime"));
    }

    #[test]
    fn unknown_provider_surfaces_typed_error() {
        let src = r#"
            (deflava-serverless-function x
              :provider mars
              :runtime nodejs20
              :handler "h"
              :code-uri "u")
        "#;
        let err = functions_in_source(src).unwrap_err();
        matches!(err, ServerlessParseError::UnknownProvider(_));
    }

    #[test]
    fn render_aws_lambda_emits_aws_lambda_function() {
        let f = ServerlessFunction {
            name: "h".into(),
            provider: Provider::Aws,
            runtime: "nodejs20".into(),
            handler: "index.handler".into(),
            code_uri: "./h.zip".into(),
            memory: 256,
            timeout: 10,
            env: {
                let mut m = IndexMap::new();
                m.insert("FOO".into(), "bar".into());
                m
            },
            doc: None,
        };
        let r = render_terraform_resources(&f);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, "aws_lambda_function");
        let body = &r[0].2;
        assert_eq!(body["function_name"], "h");
        assert_eq!(body["runtime"], "nodejs20");
        assert_eq!(body["memory_size"], 256);
        assert_eq!(body["environment"]["variables"]["FOO"], "bar");
    }

    #[test]
    fn render_gcp_function_emits_google_cloudfunctions_function() {
        let f = ServerlessFunction {
            name: "g".into(),
            provider: Provider::Gcp,
            runtime: "python311".into(),
            handler: "main".into(),
            code_uri: "src.zip".into(),
            memory: 128,
            timeout: 60,
            env: IndexMap::new(),
            doc: None,
        };
        let r = render_terraform_resources(&f);
        assert_eq!(r[0].0, "google_cloudfunctions_function");
        assert_eq!(r[0].2["entry_point"], "main");
    }

    #[test]
    fn render_cloudflare_worker_emits_cloudflare_workers_script() {
        let f = ServerlessFunction {
            name: "edge".into(),
            provider: Provider::CloudflareWorker,
            runtime: "javascript".into(),
            handler: "default".into(),
            code_uri: "worker.js".into(),
            memory: 128,
            timeout: 3,
            env: IndexMap::new(),
            doc: None,
        };
        let r = render_terraform_resources(&f);
        assert_eq!(r[0].0, "cloudflare_workers_script");
        assert_eq!(r[0].2["name"], "edge");
    }

    #[test]
    fn function_round_trips_through_serde() {
        let f = ServerlessFunction {
            name: "x".into(),
            provider: Provider::Aws,
            runtime: "nodejs20".into(),
            handler: "i.h".into(),
            code_uri: "z.zip".into(),
            memory: 128,
            timeout: 3,
            env: IndexMap::new(),
            doc: None,
        };
        let s = serde_json::to_string(&f).unwrap();
        let parsed: ServerlessFunction = serde_json::from_str(&s).unwrap();
        assert_eq!(f, parsed);
    }
}
