use base32;

use super::*;

pub fn serialize<S>(hash: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let encoded = base32::encode(crate::BASE32, hash);
    serializer.serialize_str(&encoded)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    base32::decode(crate::BASE32, &s)
        .ok_or_else(|| serde::de::Error::custom("Invalid Base32 string"))
        .and_then(|bytes| {
            bytes
                .try_into()
                .map_err(|_| serde::de::Error::custom("Expected 32 bytes for BLAKE3 hash"))
        })
}
