use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, Key, KeyInit, Nonce,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

use crate::relay::{
    canonicalize_json, derive_channel_id, RelayEnvelope, RelayWireDerivationError,
    SecureChannelAccept, SecureChannelHello,
};

pub const RELAY_A2D_KEY_INFO: &[u8] = b"agentchat-relay a2d v1";
pub const RELAY_D2A_KEY_INFO: &[u8] = b"agentchat-relay d2a v1";
const CHACHA20_POLY1305_KEY_LEN: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelaySessionKeys {
    pub shared_secret: [u8; 32],
    pub transcript_hash: [u8; 32],
    pub channel_id: String,
    pub key_app_to_daemon: [u8; CHACHA20_POLY1305_KEY_LEN],
    pub key_daemon_to_app: [u8; CHACHA20_POLY1305_KEY_LEN],
}

impl RelaySessionKeys {
    pub fn shared_secret_base64url(&self) -> String {
        encode_base64url(&self.shared_secret)
    }

    pub fn transcript_hash_base64url(&self) -> String {
        encode_base64url(&self.transcript_hash)
    }

    pub fn key_app_to_daemon_base64url(&self) -> String {
        encode_base64url(&self.key_app_to_daemon)
    }

    pub fn key_daemon_to_app_base64url(&self) -> String {
        encode_base64url(&self.key_daemon_to_app)
    }
}

impl SecureChannelHello {
    pub fn sign_with_identity_seed(
        &mut self,
        identity_seed: &[u8; 32],
    ) -> Result<(), RelayWireDerivationError> {
        let message = self.signature_input_canonical_json()?;
        self.signature = encode_base64url(&ed25519_sign(identity_seed, message.as_bytes()));
        self.validate().map_err(RelayWireDerivationError::from)
    }

    pub fn verify_signature_with_public_key(
        &self,
        public_key: &[u8; 32],
    ) -> Result<(), RelayWireDerivationError> {
        self.validate().map_err(RelayWireDerivationError::from)?;
        let message = self.signature_input_canonical_json()?;
        let signature = decode_base64url_exact::<64>("signature", &self.signature)?;
        ed25519_verify(public_key, message.as_bytes(), &signature)
    }

    pub fn ephemeral_public_key_bytes(&self) -> Result<[u8; 32], RelayWireDerivationError> {
        decode_base64url_exact("ephemeral_public_key", &self.ephemeral_public_key)
    }
}

impl SecureChannelAccept {
    pub fn sign_with_identity_seed(
        &mut self,
        identity_seed: &[u8; 32],
    ) -> Result<(), RelayWireDerivationError> {
        let message = self.signature_input_canonical_json()?;
        self.signature = encode_base64url(&ed25519_sign(identity_seed, message.as_bytes()));
        self.validate().map_err(RelayWireDerivationError::from)
    }

    pub fn verify_signature_with_public_key(
        &self,
        public_key: &[u8; 32],
    ) -> Result<(), RelayWireDerivationError> {
        self.validate().map_err(RelayWireDerivationError::from)?;
        let message = self.signature_input_canonical_json()?;
        let signature = decode_base64url_exact::<64>("signature", &self.signature)?;
        ed25519_verify(public_key, message.as_bytes(), &signature)
    }

    pub fn ephemeral_public_key_bytes(&self) -> Result<[u8; 32], RelayWireDerivationError> {
        decode_base64url_exact("ephemeral_public_key", &self.ephemeral_public_key)
    }
}

impl RelayEnvelope {
    pub fn aad_value(&self) -> Value {
        relay_envelope_aad_value(&self.from, &self.to, &self.channel_id, self.seq)
    }

    pub fn aad_canonical_json(&self) -> Result<String, RelayWireDerivationError> {
        relay_envelope_aad_canonical_json(&self.from, &self.to, &self.channel_id, self.seq)
    }
}

pub fn generate_random_secret_bytes() -> Result<[u8; 32], RelayWireDerivationError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|err| RelayWireDerivationError {
        message: err.to_string(),
    })?;
    Ok(bytes)
}

pub fn seed_from_label(label: &str) -> [u8; 32] {
    Sha256::digest(label.as_bytes()).into()
}

pub fn encode_base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn decode_base64url_exact<const N: usize>(
    field: &'static str,
    value: &str,
) -> Result<[u8; N], RelayWireDerivationError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|err| RelayWireDerivationError {
            message: format!("failed to decode {field} as base64url: {err}"),
        })?;
    bytes.try_into().map_err(|_| RelayWireDerivationError {
        message: format!("decoded {field} must be exactly {N} bytes"),
    })
}

pub fn ed25519_public_key(identity_seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(identity_seed)
        .verifying_key()
        .to_bytes()
}

pub fn ed25519_public_key_base64url(identity_seed: &[u8; 32]) -> String {
    encode_base64url(&ed25519_public_key(identity_seed))
}

pub fn ed25519_sign(identity_seed: &[u8; 32], message: &[u8]) -> [u8; 64] {
    SigningKey::from_bytes(identity_seed)
        .sign(message)
        .to_bytes()
}

pub fn ed25519_verify(
    public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<(), RelayWireDerivationError> {
    let signature = Signature::from_bytes(signature);
    let verifying_key =
        VerifyingKey::from_bytes(public_key).map_err(|err| RelayWireDerivationError {
            message: format!("invalid Ed25519 public key: {err}"),
        })?;
    verifying_key
        .verify(message, &signature)
        .map_err(|err| RelayWireDerivationError {
            message: format!("Ed25519 signature verification failed: {err}"),
        })
}

pub fn x25519_public_key(secret_key: &[u8; 32]) -> [u8; 32] {
    X25519PublicKey::from(&StaticSecret::from(*secret_key)).to_bytes()
}

pub fn x25519_public_key_base64url(secret_key: &[u8; 32]) -> String {
    encode_base64url(&x25519_public_key(secret_key))
}

pub fn x25519_shared_secret(local_secret_key: &[u8; 32], remote_public_key: &[u8; 32]) -> [u8; 32] {
    let local_secret = StaticSecret::from(*local_secret_key);
    let remote_public = X25519PublicKey::from(*remote_public_key);
    local_secret.diffie_hellman(&remote_public).to_bytes()
}

pub fn derive_transcript_hash(
    hello: &SecureChannelHello,
    accept: &SecureChannelAccept,
) -> Result<[u8; 32], RelayWireDerivationError> {
    hello.validate().map_err(RelayWireDerivationError::from)?;
    accept.validate().map_err(RelayWireDerivationError::from)?;

    let hello_json = hello.signature_input_canonical_json()?;
    let accept_json = accept.signature_input_canonical_json()?;

    Ok(Sha256::digest([hello_json.as_bytes(), accept_json.as_bytes()].concat()).into())
}

pub fn derive_session_keys(
    local_ephemeral_secret: &[u8; 32],
    remote_ephemeral_public_key: &[u8; 32],
    hello: &SecureChannelHello,
    accept: &SecureChannelAccept,
) -> Result<RelaySessionKeys, RelayWireDerivationError> {
    let shared_secret = x25519_shared_secret(local_ephemeral_secret, remote_ephemeral_public_key);
    let transcript_hash = derive_transcript_hash(hello, accept)?;
    let channel_id = derive_channel_id(hello, accept)?;

    let hkdf = Hkdf::<Sha256>::new(Some(&transcript_hash), &shared_secret);
    let mut key_app_to_daemon = [0u8; CHACHA20_POLY1305_KEY_LEN];
    let mut key_daemon_to_app = [0u8; CHACHA20_POLY1305_KEY_LEN];
    hkdf.expand(RELAY_A2D_KEY_INFO, &mut key_app_to_daemon)
        .map_err(|err| RelayWireDerivationError {
            message: format!("failed to derive app->daemon session key: {err}"),
        })?;
    hkdf.expand(RELAY_D2A_KEY_INFO, &mut key_daemon_to_app)
        .map_err(|err| RelayWireDerivationError {
            message: format!("failed to derive daemon->app session key: {err}"),
        })?;

    Ok(RelaySessionKeys {
        shared_secret,
        transcript_hash,
        channel_id,
        key_app_to_daemon,
        key_daemon_to_app,
    })
}

pub fn relay_envelope_aad_value(from: &str, to: &str, channel_id: &str, seq: u64) -> Value {
    json!({
        "type": "relay_envelope",
        "from": from,
        "to": to,
        "channel_id": channel_id,
        "seq": seq,
    })
}

pub fn relay_envelope_aad_canonical_json(
    from: &str,
    to: &str,
    channel_id: &str,
    seq: u64,
) -> Result<String, RelayWireDerivationError> {
    canonicalize_json(&relay_envelope_aad_value(from, to, channel_id, seq))
}

pub fn relay_envelope_nonce(seq: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&seq.to_be_bytes());
    nonce
}

pub fn encrypt_relay_plaintext(
    session_key: &[u8; CHACHA20_POLY1305_KEY_LEN],
    from: &str,
    to: &str,
    channel_id: &str,
    seq: u64,
    plaintext: &[u8],
) -> Result<String, RelayWireDerivationError> {
    let aad = relay_envelope_aad_canonical_json(from, to, channel_id, seq)?;
    let nonce = Nonce::from(relay_envelope_nonce(seq));
    let key = Key::from(*session_key);
    let cipher = ChaCha20Poly1305::new(&key);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|err| RelayWireDerivationError {
            message: format!("ChaCha20-Poly1305 encryption failed: {err}"),
        })?;
    Ok(encode_base64url(&ciphertext))
}

pub fn decrypt_relay_ciphertext(
    session_key: &[u8; CHACHA20_POLY1305_KEY_LEN],
    from: &str,
    to: &str,
    channel_id: &str,
    seq: u64,
    ciphertext_base64url: &str,
) -> Result<Vec<u8>, RelayWireDerivationError> {
    let aad = relay_envelope_aad_canonical_json(from, to, channel_id, seq)?;
    let nonce = Nonce::from(relay_envelope_nonce(seq));
    let ciphertext = URL_SAFE_NO_PAD
        .decode(ciphertext_base64url)
        .map_err(|err| RelayWireDerivationError {
            message: format!("failed to decode ciphertext as base64url: {err}"),
        })?;
    let key = Key::from(*session_key);
    let cipher = ChaCha20Poly1305::new(&key);
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: ciphertext.as_ref(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|err| RelayWireDerivationError {
            message: format!("ChaCha20-Poly1305 decryption failed: {err}"),
        })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::relay::{SecureChannelAccept, SecureChannelHello};

    #[derive(Debug, Deserialize)]
    struct RelayCryptoFixture {
        app_identity_seed: String,
        app_identity_public_key: String,
        daemon_identity_seed: String,
        daemon_identity_public_key: String,
        app_ephemeral_secret: String,
        app_ephemeral_public_key: String,
        daemon_ephemeral_secret: String,
        daemon_ephemeral_public_key: String,
        hello: SecureChannelHello,
        accept: SecureChannelAccept,
        transcript_hash: String,
        shared_secret: String,
        channel_id: String,
        key_app_to_daemon: String,
        key_daemon_to_app: String,
        aad_canonical_json: String,
        plaintext_json: String,
        ciphertext_app_to_daemon_seq_1: String,
    }

    #[test]
    fn relay_crypto_fixture_signatures_and_keys_round_trip() {
        let fixture = load_fixture();

        let app_identity_seed =
            decode_base64url_exact::<32>("app_identity_seed", &fixture.app_identity_seed).unwrap();
        let app_identity_public_key = decode_base64url_exact::<32>(
            "app_identity_public_key",
            &fixture.app_identity_public_key,
        )
        .unwrap();
        let daemon_identity_seed =
            decode_base64url_exact::<32>("daemon_identity_seed", &fixture.daemon_identity_seed)
                .unwrap();
        let daemon_identity_public_key = decode_base64url_exact::<32>(
            "daemon_identity_public_key",
            &fixture.daemon_identity_public_key,
        )
        .unwrap();
        let app_ephemeral_secret =
            decode_base64url_exact::<32>("app_ephemeral_secret", &fixture.app_ephemeral_secret)
                .unwrap();
        let app_ephemeral_public_key = decode_base64url_exact::<32>(
            "app_ephemeral_public_key",
            &fixture.app_ephemeral_public_key,
        )
        .unwrap();
        let daemon_ephemeral_secret = decode_base64url_exact::<32>(
            "daemon_ephemeral_secret",
            &fixture.daemon_ephemeral_secret,
        )
        .unwrap();
        let daemon_ephemeral_public_key = decode_base64url_exact::<32>(
            "daemon_ephemeral_public_key",
            &fixture.daemon_ephemeral_public_key,
        )
        .unwrap();

        assert_eq!(
            ed25519_public_key(&app_identity_seed),
            app_identity_public_key
        );
        assert_eq!(
            ed25519_public_key(&daemon_identity_seed),
            daemon_identity_public_key
        );
        assert_eq!(
            x25519_public_key(&app_ephemeral_secret),
            app_ephemeral_public_key
        );
        assert_eq!(
            x25519_public_key(&daemon_ephemeral_secret),
            daemon_ephemeral_public_key
        );

        fixture
            .hello
            .verify_signature_with_public_key(&app_identity_public_key)
            .unwrap();
        fixture
            .accept
            .verify_signature_with_public_key(&daemon_identity_public_key)
            .unwrap();

        let transcript_hash = derive_transcript_hash(&fixture.hello, &fixture.accept).unwrap();
        assert_eq!(encode_base64url(&transcript_hash), fixture.transcript_hash);

        let app_session_keys = derive_session_keys(
            &app_ephemeral_secret,
            &daemon_ephemeral_public_key,
            &fixture.hello,
            &fixture.accept,
        )
        .unwrap();
        let daemon_session_keys = derive_session_keys(
            &daemon_ephemeral_secret,
            &app_ephemeral_public_key,
            &fixture.hello,
            &fixture.accept,
        )
        .unwrap();

        assert_eq!(app_session_keys, daemon_session_keys);
        assert_eq!(app_session_keys.channel_id, fixture.channel_id);
        assert_eq!(
            app_session_keys.shared_secret_base64url(),
            fixture.shared_secret
        );
        assert_eq!(
            app_session_keys.key_app_to_daemon_base64url(),
            fixture.key_app_to_daemon
        );
        assert_eq!(
            app_session_keys.key_daemon_to_app_base64url(),
            fixture.key_daemon_to_app
        );
    }

    #[test]
    fn relay_crypto_fixture_envelope_encrypts_and_decrypts() {
        let fixture = load_fixture();
        let app_ephemeral_secret =
            decode_base64url_exact::<32>("app_ephemeral_secret", &fixture.app_ephemeral_secret)
                .unwrap();
        let daemon_ephemeral_public_key = decode_base64url_exact::<32>(
            "daemon_ephemeral_public_key",
            &fixture.daemon_ephemeral_public_key,
        )
        .unwrap();

        let session_keys = derive_session_keys(
            &app_ephemeral_secret,
            &daemon_ephemeral_public_key,
            &fixture.hello,
            &fixture.accept,
        )
        .unwrap();

        assert_eq!(
            relay_envelope_aad_canonical_json(
                &fixture.hello.from,
                &fixture.hello.to,
                &fixture.channel_id,
                1,
            )
            .unwrap(),
            fixture.aad_canonical_json
        );

        let ciphertext = encrypt_relay_plaintext(
            &session_keys.key_app_to_daemon,
            &fixture.hello.from,
            &fixture.hello.to,
            &fixture.channel_id,
            1,
            fixture.plaintext_json.as_bytes(),
        )
        .unwrap();
        assert_eq!(ciphertext, fixture.ciphertext_app_to_daemon_seq_1);

        let plaintext = decrypt_relay_ciphertext(
            &session_keys.key_app_to_daemon,
            &fixture.hello.from,
            &fixture.hello.to,
            &fixture.channel_id,
            1,
            &fixture.ciphertext_app_to_daemon_seq_1,
        )
        .unwrap();
        assert_eq!(plaintext, fixture.plaintext_json.as_bytes());
    }

    fn load_fixture() -> RelayCryptoFixture {
        let raw = include_str!("../fixtures/relay/crypto/handshake_v1.json");
        serde_json::from_str(raw).unwrap()
    }
}
