use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng;
use std::path::Path;

/// Generate a random 256-bit token, base64url-encoded.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Load the auth token from disk, or generate and save a new one.
pub fn load_or_create_token(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    if path.exists() {
        let token = std::fs::read_to_string(path)?.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }

    let token = generate_token();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &token)?;

    // Restrict permissions: owner-only read
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    tracing::info!("generated new auth token at {}", path.display());
    Ok(token)
}
