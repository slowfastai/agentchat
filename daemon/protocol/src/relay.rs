use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const RELAY_WIRE_PROTOCOL_VERSION: &str = "relay-wire/0.1";
pub const RELAY_WIRE_CRYPTO_SUITE: &str = "X25519_Ed25519_ChaCha20Poly1305_HKDFSHA256_v1";
pub const RELAY_PEER_DAEMON: &str = "daemon";

const CHANNEL_ID_CONTEXT: &[u8] = b"agentchat-channel";

pub const SECURE_CHANNEL_HELLO_SCHEMA: &str =
    include_str!("../schemas/relay/secure_channel_hello/v0.1.json");
pub const SECURE_CHANNEL_ACCEPT_SCHEMA: &str =
    include_str!("../schemas/relay/secure_channel_accept/v0.1.json");
pub const RELAY_ENVELOPE_SCHEMA: &str = include_str!("../schemas/relay/relay_envelope/v0.1.json");

/// Relay-generated readiness message sent immediately after authentication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayReady {
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub timestamp: u64,
    pub protocol_version: String,
    pub device_id: String,
    pub role: String,
    pub peer_id: String,
    pub connection_id: String,
}

impl RelayReady {
    /// Validates the field-level constraints documented for `relay_ready`.
    pub fn validate(&self) -> Result<(), RelayWireValidationError> {
        ensure_message_type("type", &self.message_type, "relay_ready")?;
        ensure_uuid("id", &self.id)?;
        ensure_protocol_version(&self.protocol_version)?;
        ensure_non_empty("device_id", &self.device_id)?;
        ensure_role("role", &self.role)?;
        ensure_peer_id("peer_id", &self.peer_id)?;
        ensure_connection_id("connection_id", &self.connection_id)?;
        Ok(())
    }
}

/// Relay-generated protocol error frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayError {
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub timestamp: u64,
    pub code: String,
    pub message: String,
    pub ref_id: Option<String>,
}

/// Field-level wire contract for `secure_channel_hello`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecureChannelHello {
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub timestamp: u64,
    pub protocol_version: String,
    pub from: String,
    pub to: String,
    pub connection_id: String,
    pub crypto_suite: String,
    pub ephemeral_public_key: String,
    pub expires_at: u64,
    pub signature: String,
}

impl SecureChannelHello {
    /// Validates the field-level constraints defined by the v0.1 JSON Schema.
    /// Stateful, time-based, and cryptographic checks are intentionally out of scope.
    pub fn validate(&self) -> Result<(), RelayWireValidationError> {
        ensure_message_type("type", &self.message_type, "secure_channel_hello")?;
        ensure_uuid("id", &self.id)?;
        ensure_protocol_version(&self.protocol_version)?;
        ensure_app_peer_id("from", &self.from)?;
        ensure_message_type("to", &self.to, RELAY_PEER_DAEMON)?;
        ensure_connection_id("connection_id", &self.connection_id)?;
        ensure_crypto_suite(&self.crypto_suite)?;
        ensure_base64url_exact("ephemeral_public_key", &self.ephemeral_public_key, 43)?;
        ensure_base64url_exact("signature", &self.signature, 86)?;
        Ok(())
    }

    /// Returns the canonical signature input object defined by the wire protocol.
    pub fn signature_input_value(&self) -> Value {
        json!({
            "type": "secure_channel_hello",
            "protocol_version": self.protocol_version,
            "from": self.from,
            "to": self.to,
            "connection_id": self.connection_id,
            "crypto_suite": self.crypto_suite,
            "ephemeral_public_key": self.ephemeral_public_key,
            "expires_at": self.expires_at,
        })
    }

    /// Returns the RFC 8785-compatible canonical JSON string used by this codebase.
    pub fn signature_input_canonical_json(&self) -> Result<String, RelayWireDerivationError> {
        canonicalize_json(&self.signature_input_value())
    }
}

/// Field-level wire contract for `secure_channel_accept`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecureChannelAccept {
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub timestamp: u64,
    pub protocol_version: String,
    pub from: String,
    pub to: String,
    pub hello_id: String,
    pub connection_id: String,
    pub crypto_suite: String,
    pub ephemeral_public_key: String,
    pub expires_at: u64,
    pub signature: String,
}

impl SecureChannelAccept {
    /// Validates the field-level constraints defined by the v0.1 JSON Schema.
    /// Stateful, time-based, and cryptographic checks are intentionally out of scope.
    pub fn validate(&self) -> Result<(), RelayWireValidationError> {
        ensure_message_type("type", &self.message_type, "secure_channel_accept")?;
        ensure_uuid("id", &self.id)?;
        ensure_protocol_version(&self.protocol_version)?;
        ensure_message_type("from", &self.from, RELAY_PEER_DAEMON)?;
        ensure_app_peer_id("to", &self.to)?;
        ensure_uuid("hello_id", &self.hello_id)?;
        ensure_connection_id("connection_id", &self.connection_id)?;
        ensure_crypto_suite(&self.crypto_suite)?;
        ensure_base64url_exact("ephemeral_public_key", &self.ephemeral_public_key, 43)?;
        ensure_base64url_exact("signature", &self.signature, 86)?;
        Ok(())
    }

    /// Returns the canonical signature input object defined by the wire protocol.
    pub fn signature_input_value(&self) -> Value {
        json!({
            "type": "secure_channel_accept",
            "protocol_version": self.protocol_version,
            "from": self.from,
            "to": self.to,
            "hello_id": self.hello_id,
            "connection_id": self.connection_id,
            "crypto_suite": self.crypto_suite,
            "ephemeral_public_key": self.ephemeral_public_key,
            "expires_at": self.expires_at,
        })
    }

    /// Returns the RFC 8785-compatible canonical JSON string used by this codebase.
    pub fn signature_input_canonical_json(&self) -> Result<String, RelayWireDerivationError> {
        canonicalize_json(&self.signature_input_value())
    }
}

/// Field-level wire contract for `relay_envelope`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayEnvelope {
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub timestamp: u64,
    pub from: String,
    pub to: String,
    pub channel_id: String,
    pub seq: u64,
    pub ciphertext: String,
}

impl RelayEnvelope {
    /// Validates the field-level constraints defined by the v0.1 JSON Schema.
    /// Channel state, replay checks, and AEAD checks are intentionally out of scope.
    pub fn validate(&self) -> Result<(), RelayWireValidationError> {
        ensure_message_type("type", &self.message_type, "relay_envelope")?;
        ensure_uuid("id", &self.id)?;
        ensure_peer_id("from", &self.from)?;
        ensure_peer_id("to", &self.to)?;
        ensure_base64url_exact("channel_id", &self.channel_id, 22)?;
        if self.seq == 0 {
            return Err(RelayWireValidationError::new(
                "seq",
                "must be an integer >= 1",
            ));
        }
        ensure_base64url_min("ciphertext", &self.ciphertext, 16)?;
        Ok(())
    }
}

/// Computes the deterministic `channel_id` from the hello/accept transcript.
pub fn derive_channel_id(
    hello: &SecureChannelHello,
    accept: &SecureChannelAccept,
) -> Result<String, RelayWireDerivationError> {
    hello.validate().map_err(RelayWireDerivationError::from)?;
    accept.validate().map_err(RelayWireDerivationError::from)?;

    let hello_json = hello.signature_input_canonical_json()?;
    let accept_json = accept.signature_input_canonical_json()?;

    let mut transcript_hasher = Sha256::new();
    transcript_hasher.update(hello_json.as_bytes());
    transcript_hasher.update(accept_json.as_bytes());
    let transcript_hash = transcript_hasher.finalize();

    let mut channel_hasher = Sha256::new();
    channel_hasher.update(CHANNEL_ID_CONTEXT);
    channel_hasher.update(transcript_hash);
    let channel_digest = channel_hasher.finalize();

    Ok(URL_SAFE_NO_PAD.encode(&channel_digest[..16]))
}

/// Canonicalizes JSON objects by sorting keys lexicographically and serializing strings
/// with serde_json's escaping rules. This is sufficient for the integer/string-only
/// signature inputs used by relay-wire v0.1.
pub fn canonicalize_json(value: &Value) -> Result<String, RelayWireDerivationError> {
    let mut output = String::new();
    canonicalize_value(value, &mut output)?;
    Ok(output)
}

fn canonicalize_value(value: &Value, output: &mut String) -> Result<(), RelayWireDerivationError> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&value.to_string()),
        Value::String(value) => output.push_str(
            &serde_json::to_string(value)
                .map_err(|err| RelayWireDerivationError::new(err.to_string()))?,
        ),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                canonicalize_value(value, output)?;
            }
            output.push(']');
        }
        Value::Object(object) => {
            output.push('{');
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(
                    &serde_json::to_string(key)
                        .map_err(|err| RelayWireDerivationError::new(err.to_string()))?,
                );
                output.push(':');
                canonicalize_value(value, output)?;
            }
            output.push('}');
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayWireValidationError {
    pub field: &'static str,
    pub message: String,
}

impl RelayWireValidationError {
    fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }
}

impl fmt::Display for RelayWireValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid relay wire field `{}`: {}",
            self.field, self.message
        )
    }
}

impl std::error::Error for RelayWireValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayWireDerivationError {
    pub message: String,
}

impl RelayWireDerivationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<RelayWireValidationError> for RelayWireDerivationError {
    fn from(value: RelayWireValidationError) -> Self {
        Self::new(value.to_string())
    }
}

impl fmt::Display for RelayWireDerivationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "relay wire derivation failed: {}", self.message)
    }
}

impl std::error::Error for RelayWireDerivationError {}

fn ensure_message_type(
    field: &'static str,
    value: &str,
    expected: &str,
) -> Result<(), RelayWireValidationError> {
    if value == expected {
        Ok(())
    } else {
        Err(RelayWireValidationError::new(
            field,
            format!("must equal {expected:?}"),
        ))
    }
}

fn ensure_protocol_version(value: &str) -> Result<(), RelayWireValidationError> {
    ensure_message_type("protocol_version", value, RELAY_WIRE_PROTOCOL_VERSION)
}

fn ensure_crypto_suite(value: &str) -> Result<(), RelayWireValidationError> {
    ensure_message_type("crypto_suite", value, RELAY_WIRE_CRYPTO_SUITE)
}

fn ensure_role(field: &'static str, value: &str) -> Result<(), RelayWireValidationError> {
    if matches!(value, "daemon" | "app") {
        Ok(())
    } else {
        Err(RelayWireValidationError::new(
            field,
            "must equal \"daemon\" or \"app\"",
        ))
    }
}

fn ensure_non_empty(field: &'static str, value: &str) -> Result<(), RelayWireValidationError> {
    if value.trim().is_empty() {
        Err(RelayWireValidationError::new(field, "must not be empty"))
    } else {
        Ok(())
    }
}

fn ensure_uuid(field: &'static str, value: &str) -> Result<(), RelayWireValidationError> {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return Err(RelayWireValidationError::new(
            field,
            "must be a lowercase UUID string",
        ));
    }

    for (index, byte) in bytes.iter().copied().enumerate() {
        match index {
            8 | 13 | 18 | 23 if byte == b'-' => {}
            8 | 13 | 18 | 23 => {
                return Err(RelayWireValidationError::new(
                    field,
                    "must be a lowercase UUID string",
                ));
            }
            _ if matches!(byte, b'0'..=b'9' | b'a'..=b'f') => {}
            _ => {
                return Err(RelayWireValidationError::new(
                    field,
                    "must be a lowercase UUID string",
                ));
            }
        }
    }

    if !matches!(bytes[14], b'1'..=b'8') {
        return Err(RelayWireValidationError::new(
            field,
            "must use a UUID version between 1 and 8",
        ));
    }

    if !matches!(bytes[19], b'8' | b'9' | b'a' | b'b') {
        return Err(RelayWireValidationError::new(
            field,
            "must use the RFC 4122 variant",
        ));
    }

    Uuid::parse_str(value).map_err(|_| {
        RelayWireValidationError::new(field, "must be a syntactically valid UUID string")
    })?;

    Ok(())
}

fn ensure_app_peer_id(field: &'static str, value: &str) -> Result<(), RelayWireValidationError> {
    let Some(suffix) = value.strip_prefix("app:") else {
        return Err(RelayWireValidationError::new(
            field,
            "must match ^app:[A-Za-z0-9._-]{1,128}$",
        ));
    };

    ensure_ascii_component(
        field,
        suffix,
        1,
        128,
        is_peer_component_char,
        "must match ^app:[A-Za-z0-9._-]{1,128}$",
    )
}

fn ensure_peer_id(field: &'static str, value: &str) -> Result<(), RelayWireValidationError> {
    if value == RELAY_PEER_DAEMON {
        Ok(())
    } else {
        ensure_app_peer_id(field, value)
    }
}

fn ensure_connection_id(field: &'static str, value: &str) -> Result<(), RelayWireValidationError> {
    let Some(suffix) = value.strip_prefix("rc_") else {
        return Err(RelayWireValidationError::new(
            field,
            "must match ^rc_[A-Za-z0-9_-]{12,64}$",
        ));
    };

    ensure_ascii_component(
        field,
        suffix,
        12,
        64,
        is_base64url_char,
        "must match ^rc_[A-Za-z0-9_-]{12,64}$",
    )
}

fn ensure_base64url_exact(
    field: &'static str,
    value: &str,
    exact_len: usize,
) -> Result<(), RelayWireValidationError> {
    if value.len() != exact_len {
        return Err(RelayWireValidationError::new(
            field,
            format!("must be base64url without padding and exactly {exact_len} characters long"),
        ));
    }

    if !value.chars().all(is_base64url_char) {
        return Err(RelayWireValidationError::new(
            field,
            "must be base64url without padding",
        ));
    }

    Ok(())
}

fn ensure_base64url_min(
    field: &'static str,
    value: &str,
    min_len: usize,
) -> Result<(), RelayWireValidationError> {
    if value.len() < min_len {
        return Err(RelayWireValidationError::new(
            field,
            format!("must be base64url without padding and at least {min_len} characters long"),
        ));
    }

    if !value.chars().all(is_base64url_char) {
        return Err(RelayWireValidationError::new(
            field,
            "must be base64url without padding",
        ));
    }

    Ok(())
}

fn ensure_ascii_component(
    field: &'static str,
    value: &str,
    min_len: usize,
    max_len: usize,
    allowed: fn(char) -> bool,
    expectation: &'static str,
) -> Result<(), RelayWireValidationError> {
    if value.len() < min_len || value.len() > max_len {
        return Err(RelayWireValidationError::new(field, expectation));
    }

    if !value.chars().all(|ch| ch.is_ascii() && allowed(ch)) {
        return Err(RelayWireValidationError::new(field, expectation));
    }

    Ok(())
}

fn is_base64url_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')
}

fn is_peer_component_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use serde::de::DeserializeOwned;
    use serde_json::Value;

    use super::*;

    const EXPECTED_CHANNEL_ID_FROM_BASIC_FIXTURES: &str = "r7ub2PzCHYp9bGMJx8R9gA";

    trait FixtureMessage: DeserializeOwned {
        const KIND: &'static str;

        fn validate_fixture(&self) -> Result<(), RelayWireValidationError>;
    }

    impl FixtureMessage for SecureChannelHello {
        const KIND: &'static str = "secure_channel_hello";

        fn validate_fixture(&self) -> Result<(), RelayWireValidationError> {
            self.validate()
        }
    }

    impl FixtureMessage for SecureChannelAccept {
        const KIND: &'static str = "secure_channel_accept";

        fn validate_fixture(&self) -> Result<(), RelayWireValidationError> {
            self.validate()
        }
    }

    impl FixtureMessage for RelayEnvelope {
        const KIND: &'static str = "relay_envelope";

        fn validate_fixture(&self) -> Result<(), RelayWireValidationError> {
            self.validate()
        }
    }

    #[test]
    fn secure_channel_hello_schema_and_fixtures_are_well_formed() {
        assert_schema_metadata(
            SECURE_CHANNEL_HELLO_SCHEMA,
            "https://agentchat.dev/schemas/relay/secure_channel_hello/v0.1.json",
            "secure_channel_hello",
        );
        assert_valid_fixtures::<SecureChannelHello>();
        assert_invalid_fixtures::<SecureChannelHello>();
    }

    #[test]
    fn secure_channel_accept_schema_and_fixtures_are_well_formed() {
        assert_schema_metadata(
            SECURE_CHANNEL_ACCEPT_SCHEMA,
            "https://agentchat.dev/schemas/relay/secure_channel_accept/v0.1.json",
            "secure_channel_accept",
        );
        assert_valid_fixtures::<SecureChannelAccept>();
        assert_invalid_fixtures::<SecureChannelAccept>();
    }

    #[test]
    fn relay_envelope_schema_and_fixtures_are_well_formed() {
        assert_schema_metadata(
            RELAY_ENVELOPE_SCHEMA,
            "https://agentchat.dev/schemas/relay/relay_envelope/v0.1.json",
            "relay_envelope",
        );
        assert_valid_fixtures::<RelayEnvelope>();
        assert_invalid_fixtures::<RelayEnvelope>();
    }

    #[test]
    fn hello_signature_input_is_canonicalized_in_sorted_key_order() {
        let hello =
            load_fixture::<SecureChannelHello>("secure_channel_hello", "valid", "basic.json");
        let canonical = hello.signature_input_canonical_json().unwrap();

        assert_eq!(
            canonical,
            "{\"connection_id\":\"rc_8PrnFvN3vM2NgWQY\",\"crypto_suite\":\"X25519_Ed25519_ChaCha20Poly1305_HKDFSHA256_v1\",\"ephemeral_public_key\":\"M4rG8QwH5dL9uN7wF6sI9r4XbL6R2Q1G0tS4o5y7z8A\",\"expires_at\":1774257630100,\"from\":\"app:7d44a5b8-b448-4c7a-9c4d-b1d496f0c3af\",\"protocol_version\":\"relay-wire/0.1\",\"to\":\"daemon\",\"type\":\"secure_channel_hello\"}"
        );
    }

    #[test]
    fn derived_channel_id_matches_the_fixture_pair() {
        let hello =
            load_fixture::<SecureChannelHello>("secure_channel_hello", "valid", "basic.json");
        let accept =
            load_fixture::<SecureChannelAccept>("secure_channel_accept", "valid", "basic.json");

        assert_eq!(
            derive_channel_id(&hello, &accept).unwrap(),
            EXPECTED_CHANNEL_ID_FROM_BASIC_FIXTURES
        );
    }

    fn assert_schema_metadata(schema: &str, expected_id: &str, expected_title: &str) {
        let value: Value = serde_json::from_str(schema).expect("schema JSON must parse");
        assert_eq!(
            value["$schema"].as_str(),
            Some("https://json-schema.org/draft/2020-12/schema")
        );
        assert_eq!(value["$id"].as_str(), Some(expected_id));
        assert_eq!(value["title"].as_str(), Some(expected_title));
    }

    fn assert_valid_fixtures<T: FixtureMessage>() {
        let files = fixture_files(T::KIND, "valid");
        assert!(
            !files.is_empty(),
            "expected at least one valid fixture for {}",
            T::KIND
        );

        for path in files {
            let raw = fs::read_to_string(&path).unwrap_or_else(|err| {
                panic!("failed to read valid fixture {}: {err}", path.display())
            });
            let message: T = serde_json::from_str(&raw).unwrap_or_else(|err| {
                panic!(
                    "expected valid {} fixture at {} to deserialize: {err}",
                    T::KIND,
                    path.display()
                )
            });
            message.validate_fixture().unwrap_or_else(|err| {
                panic!(
                    "expected valid {} fixture at {} to validate: {err}",
                    T::KIND,
                    path.display()
                )
            });
        }
    }

    fn assert_invalid_fixtures<T: FixtureMessage>() {
        let files = fixture_files(T::KIND, "invalid");
        assert!(
            !files.is_empty(),
            "expected at least one invalid fixture for {}",
            T::KIND
        );

        for path in files {
            let raw = fs::read_to_string(&path).unwrap_or_else(|err| {
                panic!("failed to read invalid fixture {}: {err}", path.display())
            });

            if let Ok(message) = serde_json::from_str::<T>(&raw) {
                assert!(
                    message.validate_fixture().is_err(),
                    "expected invalid {} fixture at {} to fail validation",
                    T::KIND,
                    path.display()
                );
            }
        }
    }

    fn load_fixture<T: DeserializeOwned>(kind: &str, validity: &str, file_name: &str) -> T {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("relay")
            .join(kind)
            .join(validity)
            .join(file_name);
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()));
        serde_json::from_str(&raw)
            .unwrap_or_else(|err| panic!("failed to parse fixture {}: {err}", path.display()))
    }

    fn fixture_files(kind: &str, validity: &str) -> Vec<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join("relay")
            .join(kind)
            .join(validity);

        let mut files: Vec<_> = fs::read_dir(&dir)
            .unwrap_or_else(|err| {
                panic!("failed to read fixture directory {}: {err}", dir.display())
            })
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        files.sort();
        files
    }
}
