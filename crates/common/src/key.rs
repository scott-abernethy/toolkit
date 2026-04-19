use secrecy::{ExposeSecret, SecretString};
use std::path::PathBuf;

const SERVICE: &str = "toolkit";
const ACCOUNT: &str = "age-identity";

fn key_file_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home)
        .join(".config")
        .join("sops")
        .join("age")
        .join("keys.txt")
}

fn secret(s: String) -> SecretString {
    // SecretString = SecretBox<str>; Box<str> via into_boxed_str()
    SecretString::new(s.into_boxed_str())
}

/// Generate a new age keypair. Returns (private_key, public_key).
pub fn generate_keypair() -> (SecretString, String) {
    let identity = age::x25519::Identity::generate();
    // Identity::to_string() returns SecretString in age 0.11
    let private_key = identity.to_string();
    let public_key = identity.to_public().to_string();
    (private_key, public_key)
}

/// Derive the age public key (recipient) from a stored private key string.
pub fn public_key_from_private(private_key: &SecretString) -> Result<String, String> {
    let identity: age::x25519::Identity = private_key
        .expose_secret()
        .parse()
        .map_err(|e| format!("Invalid age private key: {}", e))?;
    Ok(identity.to_public().to_string())
}

/// Retrieve the age private key. Tries the OS keychain first, then falls back
/// to the standard sops key file at `~/.config/sops/age/keys.txt`.
pub fn get_private_key() -> Result<SecretString, String> {
    match keyring::Entry::new(SERVICE, ACCOUNT) {
        Ok(entry) => match entry.get_password() {
            Ok(key) => return Ok(secret(key)),
            Err(keyring::Error::NoEntry) => {}
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("cancel") || msg.contains("denied") || msg.contains("not allowed") {
                    return Err("Keychain access was denied".to_string());
                }
                eprintln!("Warning: keychain lookup failed ({}), trying key file", e);
            }
        },
        Err(e) => {
            eprintln!("Warning: keychain unavailable ({}), trying key file", e);
        }
    }

    read_key_file()
}

/// Read the age private key from the standard sops key file.
pub fn read_key_file() -> Result<SecretString, String> {
    let path = key_file_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("No key in keychain and cannot read {}: {}", path.display(), e))?;

    let key = content
        .lines()
        .find(|line| line.starts_with("AGE-SECRET-KEY-"))
        .ok_or_else(|| format!("No AGE-SECRET-KEY found in {}", path.display()))?;

    Ok(secret(key.to_owned()))
}

/// Store the age private key in the OS keychain.
pub fn store_private_key(key: &SecretString) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("Failed to access keychain: {}", e))?;
    entry
        .set_password(key.expose_secret())
        .map_err(|e| format!("Failed to store key in keychain: {}", e))
}

/// Write the age private key to the standard sops key file with mode 0600.
pub fn write_key_file(key: &SecretString) -> Result<PathBuf, String> {
    let path = key_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }

    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("Failed to create key file: {}", e))?;

    writeln!(file, "# created by toolkit init")
        .and_then(|_| writeln!(file, "{}", key.expose_secret()))
        .map_err(|e| format!("Failed to write key file: {}", e))?;

    Ok(path)
}
