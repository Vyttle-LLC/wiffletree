//! This device's long-term P-256 key (design 2 of `remote-hosts`), with which it pairs and
//! answers a paired host's challenge. It is created the first time it is needed and its secret
//! kept in a login Keychain item, whose access list then names only the program that created it,
//! the signed app, so an agent on this Mac cannot sign as the device.
use crate::pairing::{KeyPair, hex, unhex};
use security_framework::os::macos::{keychain::SecKeychain, passwords::find_generic_password};
use std::io;

const SERVICE: &str = "com.vyttle.wiffletree.device-key";
const ACCOUNT: &str = "device";

/// The device key from the login Keychain, created there if there is none.
pub fn device_key() -> io::Result<KeyPair> {
    if let Ok((secret, _)) = find_generic_password(None, SERVICE, ACCOUNT) {
        let secret = String::from_utf8_lossy(&secret);
        return unhex(&secret)
            .and_then(|bytes| KeyPair::from_secret(&bytes))
            .map_err(|e| io::Error::other(format!("The device key in the Keychain: {e:#}")));
    }
    let key = KeyPair::generate().map_err(io::Error::other)?;
    let keychain = SecKeychain::default().map_err(io::Error::other)?;
    (keychain.add_generic_password(SERVICE, ACCOUNT, hex(&key.secret()).as_bytes()))
        .map_err(io::Error::other)?;
    Ok(key)
}
