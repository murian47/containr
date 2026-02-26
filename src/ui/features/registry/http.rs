use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context as _;
use reqwest::header::WWW_AUTHENTICATE;
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;

use crate::config;

use crate::ui::RegistryAuthResolved;

fn is_local_registry_host(host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    host == "localhost"
        || host.starts_with("localhost:")
        || host == "127.0.0.1"
        || host.starts_with("127.0.0.1:")
        || host == "::1"
        || host.starts_with("[::1]")
}

fn registry_api_base_url(host: &str) -> anyhow::Result<String> {
    let host = host.trim();
    anyhow::ensure!(!host.is_empty(), "registry host is empty");
    if host.starts_with("http://") || host.starts_with("https://") {
        let url = Url::parse(host).context("invalid registry url")?;
        let scheme = url.scheme();
        let host_str = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("registry url missing host"))?;
        let host_str = if host_str.eq_ignore_ascii_case("docker.io")
            || host_str.eq_ignore_ascii_case("index.docker.io")
        {
            "registry-1.docker.io"
        } else {
            host_str
        };
        let mut base = format!("{scheme}://{host_str}");
        if let Some(port) = url.port() {
            base.push_str(&format!(":{port}"));
        }
        return Ok(base);
    }
    let host_norm = if host.eq_ignore_ascii_case("docker.io")
        || host.eq_ignore_ascii_case("index.docker.io")
    {
        "registry-1.docker.io".to_string()
    } else {
        host.to_string()
    };
    let scheme = if is_local_registry_host(host) {
        "http"
    } else {
        "https"
    };
    Ok(format!("{scheme}://{host_norm}"))
}

fn parse_www_authenticate_params(value: &str, scheme: &str) -> Option<HashMap<String, String>> {
    let value_trim = value.trim();
    let scheme_lc = scheme.to_ascii_lowercase();
    let value_lc = value_trim.to_ascii_lowercase();
    let prefix = format!("{scheme_lc} ");
    let pos = value_lc.find(&prefix)?;
    let params_str = &value_trim[pos + prefix.len()..];
    let mut params: HashMap<String, String> = HashMap::new();
    for part in params_str.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut it = part.splitn(2, '=');
        let key = it.next()?.trim();
        let val = it.next().unwrap_or("").trim().trim_matches('"');
        if !key.is_empty() {
            params.insert(key.to_string(), val.to_string());
        }
    }
    if params.is_empty() {
        None
    } else {
        Some(params)
    }
}

async fn registry_fetch_token(
    client: &Client,
    realm: &str,
    service: Option<&str>,
    scope: Option<&str>,
    basic: Option<(&str, &str)>,
) -> anyhow::Result<String> {
    let mut url = Url::parse(realm).context("invalid token realm url")?;
    {
        let mut pairs = url.query_pairs_mut();
        if let Some(service) = service {
            if !service.trim().is_empty() {
                pairs.append_pair("service", service);
            }
        }
        if let Some(scope) = scope {
            if !scope.trim().is_empty() {
                pairs.append_pair("scope", scope);
            }
        }
    }
    let mut req = client.get(url);
    if let Some((user, pass)) = basic {
        req = req.basic_auth(user, Some(pass));
    }
    let resp = req.send().await.context("token request failed")?;
    if !resp.status().is_success() {
        anyhow::bail!("token request failed: http {}", resp.status());
    }
    let body = resp.text().await.context("invalid token response")?;
    let value: Value = serde_json::from_str(&body).context("invalid token response")?;
    if let Some(token) = value
        .get("token")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("access_token").and_then(|v| v.as_str()))
    {
        return Ok(token.to_string());
    }
    anyhow::bail!("token response missing token");
}

fn token_context(realm: &str, service: Option<&str>, scope: Option<&str>) -> String {
    let service = service.unwrap_or("-");
    let scope = scope.unwrap_or("-");
    format!("realm={realm} service={service} scope={scope}")
}

fn normalize_test_repo(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches('/');
    let raw = raw.split('@').next().unwrap_or(raw);
    let raw = raw.split(':').next().unwrap_or(raw);
    raw.to_string()
}

pub(in crate::ui) async fn registry_test(
    host: &str,
    auth: &RegistryAuthResolved,
    test_repo: Option<&str>,
) -> anyhow::Result<String> {
    let base = registry_api_base_url(host)?;
    let repo = test_repo
        .map(normalize_test_repo)
        .filter(|v| !v.is_empty());
    let url = format!("{base}/v2/");
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to build http client")?;
    let host_lc = host.trim().to_ascii_lowercase();
    if host_lc == "ghcr.io" && matches!(auth.auth, config::RegistryAuth::Anonymous) && repo.is_none()
    {
        anyhow::bail!("ghcr.io anonymous test requires test-repo");
    }

    let mut request = client.get(&url);
    if let config::RegistryAuth::BearerToken = auth.auth {
        if let Some(token) = auth.secret.as_deref() {
            request = request.bearer_auth(token);
        }
    }
    let resp = request.send().await.context("registry request failed")?;
    if resp.status().is_success() {
        return Ok(format!("ok ({})", resp.status()));
    }
    if resp.status() != StatusCode::UNAUTHORIZED {
        anyhow::bail!("registry request failed: http {}", resp.status());
    }

    let auth_header = resp
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if parse_www_authenticate_params(&auth_header, "basic").is_some() {
        let (user, pass) = match auth.auth {
            config::RegistryAuth::Basic | config::RegistryAuth::GithubPat => {
                let user = auth.username.as_deref().unwrap_or("");
                let pass = auth.secret.as_deref().unwrap_or("");
                if user.is_empty() || pass.is_empty() {
                    anyhow::bail!("registry credentials missing for {host}");
                }
                (user, pass)
            }
            _ => anyhow::bail!("registry requires basic auth"),
        };
        let resp = client
            .get(&url)
            .basic_auth(user, Some(pass))
            .send()
            .await
            .context("registry basic auth request failed")?;
        if resp.status().is_success() {
            return Ok(format!("ok ({})", resp.status()));
        }
        anyhow::bail!("registry basic auth failed: http {}", resp.status());
    }

    let params = parse_www_authenticate_params(&auth_header, "bearer")
        .ok_or_else(|| anyhow::anyhow!("registry auth challenge missing bearer details"))?;
    let realm = params
        .get("realm")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("registry auth challenge missing realm"))?;
    let service = params.get("service").cloned();
    let mut scope = params.get("scope").cloned();
    if let Some(repo) = repo.as_deref() {
        scope = Some(format!("repository:{repo}:pull"));
    }
    let service = service.as_deref();
    let scope = scope.as_deref();
    let basic = match auth.auth {
        config::RegistryAuth::Anonymous => None,
        config::RegistryAuth::Basic | config::RegistryAuth::GithubPat => {
            let user = auth.username.as_deref().unwrap_or("");
            let pass = auth.secret.as_deref().unwrap_or("");
            if user.is_empty() || pass.is_empty() {
                anyhow::bail!("registry credentials missing for {host}");
            }
            Some((user, pass))
        }
        config::RegistryAuth::BearerToken => None,
    };
    let mut used_ghcr = false;
    let token = match auth.auth {
        config::RegistryAuth::BearerToken => auth
            .secret
            .clone()
            .ok_or_else(|| anyhow::anyhow!("registry token missing for {host}"))?,
        _ => match registry_fetch_token(&client, &realm, service, scope, basic).await {
            Ok(token) => token,
            Err(e) => {
                if host_lc == "lscr.io" {
                    let repo = repo
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("lscr.io test requires test-repo"))?;
                    let ghcr_realm = "https://ghcr.io/token";
                    let ghcr_scope = format!("repository:{repo}:pull");
                    let ghcr_service = "ghcr.io";
                    match registry_fetch_token(
                        &client,
                        ghcr_realm,
                        Some(ghcr_service),
                        Some(&ghcr_scope),
                        basic,
                    )
                    .await
                    {
                        Ok(token) => {
                            used_ghcr = true;
                            token
                        }
                        Err(e2) => {
                            let ctx =
                                token_context(ghcr_realm, Some(ghcr_service), Some(&ghcr_scope));
                            anyhow::bail!("token request failed: {:#} ({ctx})", e2);
                        }
                    }
                } else {
                    let ctx = token_context(&realm, service, scope);
                    anyhow::bail!("token request failed: {:#} ({ctx})", e);
                }
            }
        },
    };
    let test_base = if used_ghcr {
        "https://ghcr.io".to_string()
    } else {
        base
    };
    let test_url = if let Some(repo) = repo.as_deref() {
        format!("{test_base}/v2/{repo}/tags/list")
    } else {
        url.clone()
    };
    let resp = client
        .get(&test_url)
        .bearer_auth(token)
        .send()
        .await
        .context("registry bearer auth request failed")?;
    if resp.status().is_success() {
        let hint = if used_ghcr { " via ghcr.io" } else { "" };
        return Ok(format!("ok ({}){hint}", resp.status()));
    }
    if resp.status() == StatusCode::NOT_FOUND && repo.is_some() {
        anyhow::bail!("registry repository not found (check test-repo)");
    }
    anyhow::bail!("registry bearer auth failed: http {}", resp.status());
}
