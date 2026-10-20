//! The cryptography both sides of remote access share: long-term P-256 keys and their ids, the
//! `code` method's SPAKE2 exchange with key confirmation over a transcript, the `qr` method's
//! proof, and the challenge a paired device signs. Test vectors for each are in
//! `tests/fixtures/pairing_vectors.json`.
use anyhow::{Context, Result, bail, ensure};
use hmac::{Hmac, KeyInit, Mac};
use p256::ecdsa::{
    Signature, SigningKey, VerifyingKey,
    signature::{Signer, Verifier},
};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity as SpakeIdentity, Password, Spake2};
use std::io::Read;

/// The SPAKE2 identities of the `code` method.
pub const DEVICE_IDENTITY: &[u8] = b"wiffletree-device";
pub const HOST_IDENTITY: &[u8] = b"wiffletree-host";

/// A long-term P-256 key pair: a host's or a device's.
#[derive(Clone)]
pub struct KeyPair(SigningKey);
impl KeyPair {
    pub fn generate() -> Result<Self> {
        loop {
            let mut bytes = [0u8; 32];
            std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
            // A value outside the curve's order, about one in 2^32, is drawn again.
            if let Ok(pair) = Self::from_secret(&bytes) {
                return Ok(pair);
            }
        }
    }
    /// From the 32-byte secret scalar `secret` returns.
    pub fn from_secret(secret: &[u8]) -> Result<Self> {
        Ok(Self(
            SigningKey::from_slice(secret).context("Not a P-256 secret key")?,
        ))
    }
    pub fn secret(&self) -> Vec<u8> {
        self.0.to_bytes().to_vec()
    }
    /// The public key, SEC1 uncompressed: 65 bytes.
    pub fn public_key(&self) -> Vec<u8> {
        self.0.verifying_key().to_sec1_bytes().into_vec()
    }
    pub fn id(&self) -> String {
        key_id(&self.public_key())
    }
    /// An ECDSA P-256 SHA-256 signature, raw `r ‖ s`: 64 bytes.
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let signature: Signature = self.0.sign(message);
        signature.to_bytes().to_vec()
    }
    /// The secret key as PKCS#8 DER, for a TLS certificate.
    pub fn pkcs8_der(&self) -> Result<Vec<u8>> {
        use p256::pkcs8::EncodePrivateKey;
        Ok(self.0.to_pkcs8_der()?.as_bytes().to_vec())
    }
}

/// A key's id: the SHA-256 of its SEC1 uncompressed public key, in lowercase hex.
pub fn key_id(public_key: &[u8]) -> String {
    hex(&Sha256::digest(public_key))
}
/// The first 8 bytes of an id, as Bonjour and logs show it.
pub fn short_id(id: &str) -> &str {
    &id[..id.len().min(16)]
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex(text: &str) -> Result<Vec<u8>> {
    ensure!(
        text.len().is_multiple_of(2) && text.bytes().all(|b| b.is_ascii_hexdigit()),
        "Not hex"
    );
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).context("Not hex"))
        .collect()
}

/// Whether `key` is a P-256 public key, SEC1 uncompressed.
pub fn is_public_key(key: &[u8]) -> bool {
    key.len() == 65 && VerifyingKey::from_sec1_bytes(key).is_ok()
}

/// Whether `signature`, raw `r ‖ s`, signs `message` under the SEC1 `public_key`.
pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    let (Ok(key), Ok(signature)) = (
        VerifyingKey::from_sec1_bytes(public_key),
        Signature::from_slice(signature),
    ) else {
        return false;
    };
    key.verify(message, &signature).is_ok()
}

/// What a device signs on `/v1`: a label, the host's challenge, and the host's and the
/// device's ids as raw bytes.
pub fn challenge_message(challenge: &[u8], host_id: &str, device_id: &str) -> Result<Vec<u8>> {
    let mut message = b"wiffletree-challenge-v1".to_vec();
    message.extend(challenge);
    message.extend(unhex(host_id)?);
    message.extend(unhex(device_id)?);
    Ok(message)
}

/// What the confirmations cover: the host's public key as the device saw it in TLS (the host's
/// own on the host), the device's public key, the device's name and the host's name, each behind
/// its length as 4 bytes big-endian.
pub fn transcript(
    host_key: &[u8],
    device_key: &[u8],
    device_name: &str,
    host_name: &str,
) -> Vec<u8> {
    let mut transcript = b"wiffletree-pair-v1".to_vec();
    for field in [
        host_key,
        device_key,
        device_name.as_bytes(),
        host_name.as_bytes(),
    ] {
        transcript.extend((field.len() as u32).to_be_bytes());
        transcript.extend(field);
    }
    transcript
}

/// Which side computes a confirmation or proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Device,
    Host,
}
impl Side {
    fn label(self) -> &'static [u8] {
        match self {
            Self::Device => b"device",
            Self::Host => b"host",
        }
    }
}

/// HMAC-SHA256 under `key` over the side's label and the transcript. The `code` method keys it
/// with the SPAKE2 key; the `qr` method with the QR code's one-time secret.
pub fn confirmation(key: &[u8], side: Side, transcript: &[u8]) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key");
    mac.update(side.label());
    mac.update(transcript);
    mac.finalize().into_bytes().to_vec()
}
/// Checks a confirmation in time that does not depend on where it differs.
pub fn confirms(key: &[u8], side: Side, transcript: &[u8], given: &[u8]) -> bool {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key");
    mac.update(side.label());
    mac.update(transcript);
    mac.verify_slice(given).is_ok()
}

/// One side of the `code` method's SPAKE2 exchange (the Ed25519 group), the device as A.
pub struct CodeExchange(Spake2<Ed25519Group>);
impl CodeExchange {
    /// Starts the exchange for `side` with the 4-digit code; returns the message to send.
    pub fn start(side: Side, code: &str) -> Result<(Self, Vec<u8>)> {
        ensure!(is_code(code), "The code is 4 digits");
        let (password, a, b) = Self::inputs(code);
        let (state, message) = match side {
            Side::Device => Spake2::<Ed25519Group>::start_a(&password, &a, &b),
            Side::Host => Spake2::<Ed25519Group>::start_b(&password, &a, &b),
        };
        Ok((Self(state), message))
    }
    /// `start` with a chosen random source, for test vectors.
    #[cfg(test)]
    fn start_with_rng(
        side: Side,
        code: &str,
        rng: impl rand_core::CryptoRng + rand_core::RngCore,
    ) -> (Self, Vec<u8>) {
        let (password, a, b) = Self::inputs(code);
        let (state, message) = match side {
            Side::Device => Spake2::<Ed25519Group>::start_a_with_rng(&password, &a, &b, rng),
            Side::Host => Spake2::<Ed25519Group>::start_b_with_rng(&password, &a, &b, rng),
        };
        (Self(state), message)
    }
    fn inputs(code: &str) -> (Password, SpakeIdentity, SpakeIdentity) {
        (
            Password::new(code.as_bytes()),
            SpakeIdentity::new(DEVICE_IDENTITY),
            SpakeIdentity::new(HOST_IDENTITY),
        )
    }
    /// The shared key, from the other side's message. A wrong code still gives a key, which
    /// then fails confirmation.
    pub fn finish(self, theirs: &[u8]) -> Result<Vec<u8>> {
        match self.0.finish(theirs) {
            Ok(key) => Ok(key),
            Err(error) => bail!("The pairing message is malformed: {error:?}"),
        }
    }
}

/// Whether `code` is a 4-digit code.
pub fn is_code(code: &str) -> bool {
    code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_verifies_only_under_its_key_and_message() {
        let (device, other) = (KeyPair::generate().unwrap(), KeyPair::generate().unwrap());
        assert_eq!(device.public_key().len(), 65);
        let message = challenge_message(&[7; 32], &other.id(), &device.id()).unwrap();
        let signature = device.sign(&message);
        assert!(verify(&device.public_key(), &message, &signature));
        assert!(!verify(&other.public_key(), &message, &signature));
        assert!(!verify(&device.public_key(), b"another", &signature));
        assert!(challenge_message(&[7; 32], &other.id(), "aéa").is_err());
        assert!(unhex("zz").is_err());
        let again = KeyPair::from_secret(&device.secret()).unwrap();
        assert_eq!(again.id(), device.id());
    }

    #[test]
    fn the_same_code_confirms_and_a_wrong_one_does_not() {
        let transcript = transcript(b"host", b"device", "MacBook", "Studio");
        let exchange = |device_code: &str| {
            let (device, to_host) = CodeExchange::start(Side::Device, device_code).unwrap();
            let (host, to_device) = CodeExchange::start(Side::Host, "4817").unwrap();
            let device_key = device.finish(&to_device).unwrap();
            let host_key = host.finish(&to_host).unwrap();
            let proof = confirmation(&device_key, Side::Device, &transcript);
            confirms(&host_key, Side::Device, &transcript, &proof)
        };
        assert!(exchange("4817"));
        assert!(!exchange("4818"));
    }

    /// Bytes for a random source, the SHA-256 of a label and a counter, block after block, so
    /// another implementation can draw the same ones.
    struct Drawn {
        label: &'static [u8],
        bytes: Vec<u8>,
        drawn: Vec<u8>,
    }
    impl Drawn {
        fn new(label: &'static [u8]) -> Self {
            Self {
                label,
                bytes: vec![],
                drawn: vec![],
            }
        }
    }
    impl rand_core::RngCore for Drawn {
        fn next_u32(&mut self) -> u32 {
            rand_core::impls::next_u32_via_fill(self)
        }
        fn next_u64(&mut self) -> u64 {
            rand_core::impls::next_u64_via_fill(self)
        }
        fn fill_bytes(&mut self, dest: &mut [u8]) {
            while self.bytes.len() < dest.len() {
                let mut block = Sha256::new();
                block.update(self.label);
                block.update((self.drawn.len() as u32 / 32).to_be_bytes());
                let block = block.finalize();
                self.bytes.extend(block);
                self.drawn.extend(block);
            }
            let rest = self.bytes.split_off(dest.len());
            dest.copy_from_slice(&self.bytes);
            self.bytes = rest;
        }
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }
    impl rand_core::CryptoRng for Drawn {}

    /// The vectors `docs/remote-protocol.md` documents for both pairing methods and the
    /// challenge. `UPDATE_PAIRING_VECTORS=1` rewrites the fixture.
    #[test]
    fn the_pairing_test_vectors_hold() {
        let device = KeyPair::from_secret(&Sha256::digest(b"wiffletree device key")).unwrap();
        let host = KeyPair::from_secret(&Sha256::digest(b"wiffletree host key")).unwrap();
        let (device_name, host_name, code) = ("Michael's MacBook Air", "Studio Mac", "4817");
        let transcript = transcript(
            &host.public_key(),
            &device.public_key(),
            device_name,
            host_name,
        );
        let (mut device_random, mut host_random) = (Drawn::new(b"device"), Drawn::new(b"host"));
        let (device_spake, to_host) =
            CodeExchange::start_with_rng(Side::Device, code, &mut device_random);
        let (host_spake, to_device) =
            CodeExchange::start_with_rng(Side::Host, code, &mut host_random);
        let key = device_spake.finish(&to_device).unwrap();
        assert_eq!(host_spake.finish(&to_host).unwrap(), key);
        let qr_secret = Sha256::digest(b"wiffletree qr secret")[..16].to_vec();
        let challenge = Sha256::digest(b"wiffletree challenge").to_vec();
        let signed = challenge_message(&challenge, &host.id(), &device.id()).unwrap();
        let signature = device.sign(&signed);
        assert!(verify(&device.public_key(), &signed, &signature));
        let vectors = serde_json::json!({
            "keys": {
                "device_secret": hex(&device.secret()),
                "device_public": hex(&device.public_key()),
                "device_id": device.id(),
                "host_secret": hex(&host.secret()),
                "host_public": hex(&host.public_key()),
                "host_id": host.id(),
                "host_short_id": short_id(&host.id()),
            },
            "transcript": {
                "device_name": device_name,
                "host_name": host_name,
                "bytes": hex(&transcript),
            },
            "code": {
                "code": code,
                "device_identity": String::from_utf8_lossy(DEVICE_IDENTITY),
                "host_identity": String::from_utf8_lossy(HOST_IDENTITY),
                "device_random": hex(&device_random.drawn),
                "host_random": hex(&host_random.drawn),
                "device_message": hex(&to_host),
                "host_message": hex(&to_device),
                "key": hex(&key),
                "device_confirmation": hex(&confirmation(&key, Side::Device, &transcript)),
                "host_confirmation": hex(&confirmation(&key, Side::Host, &transcript)),
            },
            "qr": {
                "secret": hex(&qr_secret),
                "device_proof": hex(&confirmation(&qr_secret, Side::Device, &transcript)),
                "host_proof": hex(&confirmation(&qr_secret, Side::Host, &transcript)),
            },
            "challenge": {
                "challenge": hex(&challenge),
                "signed": hex(&signed),
                "signature": hex(&signature),
            },
        });
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/pairing_vectors.json"
        );
        if std::env::var_os("UPDATE_PAIRING_VECTORS").is_some() {
            std::fs::write(path, serde_json::to_string_pretty(&vectors).unwrap() + "\n").unwrap();
        }
        let recorded: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(recorded, vectors);
    }
}
