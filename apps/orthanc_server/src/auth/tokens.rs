use rand::RngExt;
use sha2::{Digest, Sha256};

pub fn generate_refresh_token() -> String {
    let bytes: Vec<u8> = (0..64).map(|_| rand::rng().random::<u8>()).collect();
    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
