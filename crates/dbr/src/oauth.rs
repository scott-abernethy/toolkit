//! Native OAuth U2M (PKCE) flow for Databricks authentication.
use common::{Result, ToolkitError};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix timestamp when the access token expires.
    pub expires_at: u64,
}

/// Generate a PKCE (code_verifier, code_challenge) pair.
/// Returns (verifier, challenge) as base64url strings without padding.
pub fn generate_pkce() -> Result<(String, String)> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand::RngCore;

    let mut bytes = [0u8; 64];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let verifier = URL_SAFE_NO_PAD.encode(bytes);

    let digest = <sha2::Sha256 as sha2::Digest>::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(digest);

    Ok((verifier, challenge))
}

/// Generate a random state string for CSRF protection.
pub fn generate_state() -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use rand::RngCore;

    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Path to the OAuth token file for a connection.
/// Returns `$HOME/.config/toolkit/dbr-oauth/<conn>.json`.
pub fn token_file_path(conn: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .map_err(|_| ToolkitError::config("HOME environment variable not set"))?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("toolkit")
        .join("dbr-oauth")
        .join(format!("{}.json", conn)))
}

/// Read a token pair from disk.
pub fn read_token_file(path: &Path) -> Result<TokenPair> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ToolkitError::other(format!("Failed to read token file: {}", e)))?;
    serde_json::from_str(&content)
        .map_err(|e| ToolkitError::other(format!("Failed to parse token file: {}", e)))
}

/// Write a token pair to disk with 0600 permissions.
/// Parent directory created with 0700. Atomic write (temp → rename).
pub fn write_token_file(path: &Path, tokens: &TokenPair) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    if let Some(parent) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|e| ToolkitError::other(format!("Failed to create token dir: {}", e)))?;
    }

    let tmp_path = path.with_extension("tmp");

    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp_path)
            .map_err(|e| {
                ToolkitError::other(format!("Failed to open token file for writing: {}", e))
            })?;

        let content = serde_json::to_string(tokens)
            .map_err(|e| ToolkitError::other(format!("Failed to serialize tokens: {}", e)))?;

        file.write_all(content.as_bytes())
            .map_err(|e| ToolkitError::other(format!("Failed to write token file: {}", e)))?;
    }

    std::fs::rename(&tmp_path, path)
        .map_err(|e| ToolkitError::other(format!("Failed to rename token file: {}", e)))?;

    Ok(())
}

/// Percent-encode a string for use in OAuth URLs (query parameter values).
pub fn url_encode(s: &str) -> String {
    let mut encoded = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

/// Form-encode a string for use in HTTP request bodies.
fn form_encode(s: &str) -> String {
    let mut encoded = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    encoded
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Exchange an authorization code for a token pair.
pub fn exchange_code(
    host: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenPair> {
    let url = format!("{}/oidc/v1/token", host.trim_end_matches('/'));
    let body = format!(
        "client_id=databricks-cli&grant_type=authorization_code&code={}&code_verifier={}&redirect_uri={}&scope=all-apis+offline_access",
        form_encode(code),
        form_encode(verifier),
        form_encode(redirect_uri),
    );

    let response = ureq::post(&url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
        .map_err(|e| ToolkitError::connection(format!("Token exchange request failed: {}", e)))?;

    let json_str = response
        .into_string()
        .map_err(|e| ToolkitError::connection(format!("Failed to read token response: {}", e)))?;

    parse_token_response(&json_str, None)
}

/// Refresh an access token using a stored refresh token.
/// Preserves old refresh_token if the server doesn't return a new one.
pub fn refresh_tokens(host: &str, refresh_token: &str) -> Result<TokenPair> {
    let url = format!("{}/oidc/v1/token", host.trim_end_matches('/'));
    let body = format!(
        "client_id=databricks-cli&grant_type=refresh_token&refresh_token={}",
        form_encode(refresh_token),
    );

    let response = ureq::post(&url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&body)
        .map_err(|e| ToolkitError::connection(format!("Token refresh request failed: {}", e)))?;

    let json_str = response
        .into_string()
        .map_err(|e| ToolkitError::connection(format!("Failed to read refresh response: {}", e)))?;

    parse_token_response(&json_str, Some(refresh_token))
}

/// Check whether the access token will expire within 5 minutes.
pub fn is_near_expiry(expires_at: u64) -> bool {
    unix_now() + 300 >= expires_at
}

/// Parse the JSON token endpoint response.
/// If response has `error`, returns Err.
/// If response lacks `refresh_token`, preserves `old_refresh_token`.
/// Computes `expires_at = unix_now() + expires_in` (default 3600).
fn parse_token_response(json: &str, old_refresh_token: Option<&str>) -> Result<TokenPair> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| ToolkitError::other(format!("Failed to parse token response: {}", e)))?;

    if let Some(error) = v.get("error").and_then(|e| e.as_str()) {
        let desc = v
            .get("error_description")
            .and_then(|d| d.as_str())
            .unwrap_or(error);
        return Err(ToolkitError::auth(format!("OAuth error: {}", desc)));
    }

    let access_token = v
        .get("access_token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| ToolkitError::other("Token response missing access_token"))?
        .to_string();

    let refresh_token = v
        .get("refresh_token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .or_else(|| old_refresh_token.map(|s| s.to_string()));

    let expires_in = v.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(3600);
    let expires_at = unix_now() + expires_in;

    Ok(TokenPair {
        access_token,
        refresh_token,
        expires_at,
    })
}

/// Try to bind a TCP listener for the OAuth callback on localhost.
/// Tries IPv6 first (macOS prefers ::1), then IPv4.
pub fn bind_callback_listener(port: u16) -> Result<TcpListener> {
    TcpListener::bind(format!("[::1]:{}", port))
        .or_else(|_| TcpListener::bind(format!("127.0.0.1:{}", port)))
        .map_err(|e| ToolkitError::other(format!("Failed to bind port {}: {}", port, e)))
}

/// Wait for the OAuth redirect callback. Returns the authorization code.
/// Handles OAuth error redirects and CSRF validation.
/// Sends an HTML success/error page back to the browser.
pub fn wait_for_callback(
    listener: TcpListener,
    expected_state: &str,
    timeout: Duration,
) -> Result<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let expected_state = expected_state.to_string();

    std::thread::spawn(move || match listener.accept() {
        Ok((mut stream, _)) => {
            let result = handle_callback_request(&mut stream, &expected_state);
            let _ = tx.send(result);
        }
        Err(e) => {
            let _ = tx.send(Err(ToolkitError::other(format!(
                "Failed to accept connection: {}",
                e
            ))));
        }
    });

    rx.recv_timeout(timeout)
        .map_err(|_| ToolkitError::other("Authentication timed out waiting for browser callback"))?
}

fn handle_callback_request(
    stream: &mut std::net::TcpStream,
    expected_state: &str,
) -> Result<String> {
    let mut buf = [0u8; 4096];
    let n = stream
        .read(&mut buf)
        .map_err(|e| ToolkitError::other(format!("Failed to read callback request: {}", e)))?;

    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or("");

    let result = parse_callback_line(first_line, expected_state);

    let (status_line, body) = match &result {
        Ok(_) => (
            "200 OK",
            "<html><body><h2>Authentication successful!</h2><p>You can close this window.</p></body></html>",
        ),
        Err(_) => (
            "400 Bad Request",
            "<html><body><h2>Authentication failed</h2><p>An error occurred. Check the terminal for details.</p></body></html>",
        ),
    };

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status_line,
        body.len(),
        body,
    );
    let _ = stream.write_all(response.as_bytes());

    result
}

fn parse_callback_line(line: &str, expected_state: &str) -> Result<String> {
    // Parse: "GET /?code=XXX&state=YYY HTTP/1.1"
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(ToolkitError::other("Invalid OAuth callback request"));
    }

    let path = parts[1];
    let query = path.split('?').nth(1).unwrap_or("");

    let mut code = None;
    let mut state = None;
    let mut error: Option<String> = None;
    let mut error_description: Option<String> = None;

    for param in query.split('&') {
        let mut kv = param.splitn(2, '=');
        let key = kv.next().unwrap_or("");
        let value = percent_decode(kv.next().unwrap_or(""));

        match key {
            "code" => code = Some(value),
            "state" => state = Some(value),
            "error" => error = Some(value),
            "error_description" => error_description = Some(value),
            _ => {}
        }
    }

    if let Some(err) = error {
        let desc = error_description.unwrap_or(err);
        return Err(ToolkitError::auth(format!("OAuth error: {}", desc)));
    }

    let state = state.ok_or_else(|| ToolkitError::other("Missing state in OAuth callback"))?;
    if state != expected_state {
        return Err(ToolkitError::other(
            "CSRF validation failed: state mismatch",
        ));
    }

    code.ok_or_else(|| ToolkitError::other("Missing code in OAuth callback"))
}

fn percent_decode(s: &str) -> String {
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(b) = u8::from_str_radix(hex_str, 16) {
                    result.push(b as char);
                    i += 3;
                    continue;
                }
            }
        } else if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_pkce_lengths() {
        let (verifier, challenge) = generate_pkce().unwrap();
        // Verifier is base64url of 64 bytes = 86 chars (no padding)
        assert_eq!(verifier.len(), 86);
        // Challenge is base64url of SHA-256 (32 bytes) = 43 chars (no padding)
        assert_eq!(challenge.len(), 43);
        // Neither should contain padding characters
        assert!(!verifier.contains('='));
        assert!(!challenge.contains('='));
    }

    #[test]
    fn test_generate_pkce_unique() {
        let (v1, _) = generate_pkce().unwrap();
        let (v2, _) = generate_pkce().unwrap();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_url_encode_passthrough() {
        assert_eq!(url_encode("abc123-_.~"), "abc123-_.~");
    }

    #[test]
    fn test_url_encode_special() {
        assert_eq!(
            url_encode("http://localhost:8020"),
            "http%3A%2F%2Flocalhost%3A8020"
        );
    }

    #[test]
    fn test_percent_decode() {
        assert_eq!(percent_decode("hello+world"), "hello world");
        assert_eq!(percent_decode("foo%20bar"), "foo bar");
        assert_eq!(percent_decode("abc%2Fdef"), "abc/def");
    }

    #[test]
    fn test_is_near_expiry_past() {
        assert!(is_near_expiry(0));
    }

    #[test]
    fn test_is_near_expiry_future() {
        let far_future = unix_now() + 3600;
        assert!(!is_near_expiry(far_future));
    }

    #[test]
    fn test_parse_callback_line_valid() {
        let line = "GET /?code=mycode&state=mystate HTTP/1.1";
        let result = parse_callback_line(line, "mystate");
        assert_eq!(result.unwrap(), "mycode");
    }

    #[test]
    fn test_parse_callback_line_state_mismatch() {
        let line = "GET /?code=mycode&state=wrongstate HTTP/1.1";
        let result = parse_callback_line(line, "mystate");
        assert!(result.is_err());
        assert!(result.unwrap_err().message().contains("CSRF"));
    }

    #[test]
    fn test_parse_callback_line_oauth_error() {
        let line =
            "GET /?error=access_denied&error_description=User+denied+access&state=mystate HTTP/1.1";
        let result = parse_callback_line(line, "mystate");
        assert!(result.is_err());
        assert!(result.unwrap_err().message().contains("User denied access"));
    }

    #[test]
    fn test_parse_token_response_success() {
        let json = r#"{"access_token":"tok123","refresh_token":"ref456","expires_in":3600}"#;
        let pair = parse_token_response(json, None).unwrap();
        assert_eq!(pair.access_token, "tok123");
        assert_eq!(pair.refresh_token.as_deref(), Some("ref456"));
        assert!(pair.expires_at > unix_now());
    }

    #[test]
    fn test_parse_token_response_preserves_old_refresh() {
        let json = r#"{"access_token":"tok123","expires_in":3600}"#;
        let pair = parse_token_response(json, Some("old_refresh")).unwrap();
        assert_eq!(pair.refresh_token.as_deref(), Some("old_refresh"));
    }

    #[test]
    fn test_parse_token_response_error() {
        let json = r#"{"error":"invalid_grant","error_description":"Token expired"}"#;
        let result = parse_token_response(json, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().message().contains("Token expired"));
    }

    #[test]
    fn test_token_file_roundtrip() {
        let tokens = TokenPair {
            access_token: "access123".to_string(),
            refresh_token: Some("refresh456".to_string()),
            expires_at: 9999999999,
        };
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test-conn.json");
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        write_token_file(&path, &tokens).unwrap();
        let read_back = read_token_file(&path).unwrap();
        assert_eq!(read_back.access_token, "access123");
        assert_eq!(read_back.refresh_token.as_deref(), Some("refresh456"));
        assert_eq!(read_back.expires_at, 9999999999);
        std::fs::remove_file(&path).unwrap();
    }
}
