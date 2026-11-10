//! Password hashing and initial-password generation.
//!
//! Hashing is argon2id with crate defaults (`Argon2::default()` = the OWASP
//! baseline m=19456 KiB / t=2 / p=1, spec §0.1; argon2 0.6 auto-salts).
//! The generator produces the one-time initial password of spec §2.4:
//! 16 characters drawn from an alphabet with confusable glyphs
//! (`0 O o 1 l L I i`) removed, seeded from OS entropy.

use anyhow::{Context, Result};
use argon2::password_hash::phc::PasswordHash;
use argon2::{Argon2, PasswordHasher, PasswordVerifier};

/// Length of a generated initial password (spec §2.4).
pub const INITIAL_PASSWORD_LEN: usize = 16;

/// Password policy (spec §2.1): 8–128 characters, no composition rules; the
/// cap protects argon2 from overlong-input DoS. The one home of the bounds —
/// the change-password endpoint and the seed CLI's explicit-password check
/// both read them here.
pub const PASSWORD_MIN_CHARS: usize = 8;
pub const PASSWORD_MAX_CHARS: usize = 128;

/// 54 glyphs: base-62 minus the eight confusables `0 O o 1 l L I i`.
const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// First byte value that would bias the modulo mapping (see `password_from_bytes`).
const UNBIASED_LIMIT: usize = (u8::MAX as usize + 1) / ALPHABET.len() * ALPHABET.len();

/// Map random bytes onto the alphabet, purely: a byte is accepted only below
/// `UNBIASED_LIMIT` so `byte % ALPHABET.len()` is uniform (no modulo bias);
/// accepted bytes yield exactly `INITIAL_PASSWORD_LEN` characters.
fn password_from_bytes(bytes: impl Iterator<Item = u8>) -> String {
    bytes
        .filter(|&b| usize::from(b) < UNBIASED_LIMIT)
        .map(|b| ALPHABET[usize::from(b) % ALPHABET.len()] as char)
        .take(INITIAL_PASSWORD_LEN)
        .collect()
}

/// Generate a one-time initial password from OS entropy.
pub fn generate_initial_password() -> String {
    let mut out = String::with_capacity(INITIAL_PASSWORD_LEN);
    while out.len() < INITIAL_PASSWORD_LEN {
        let mut block = [0u8; 32];
        getrandom::fill(&mut block).expect("OS entropy source unavailable");
        out.push_str(&password_from_bytes(block.into_iter()));
    }
    out.truncate(INITIAL_PASSWORD_LEN);
    out
}

/// Hash a plaintext into an argon2id PHC string (auto-salted, crate defaults).
pub fn hash_password(password: &str) -> Result<String> {
    Argon2::default()
        .hash_password(password.as_bytes())
        .context("failed to hash password")
        .map(|hash| hash.to_string())
}

/// Verify a plaintext against a stored PHC string. `Ok(false)` = mismatch;
/// `Err` = the stored string is not a valid PHC hash (data corruption).
#[allow(
    dead_code,
    reason = "first consumed by /auth/login (M2); exercised here by seed tests"
)]
pub fn verify_password(password: &str, phc: &str) -> Result<bool> {
    let parsed = PasswordHash::new(phc).context("stored password hash is not valid PHC")?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passwords_have_spec_length_and_alphabet() {
        for _ in 0..64 {
            let password = generate_initial_password();
            assert_eq!(password.len(), INITIAL_PASSWORD_LEN);
            assert!(
                password.bytes().all(|b| ALPHABET.contains(&b)),
                "glyph outside the confusable-free alphabet: {password}"
            );
        }
    }

    #[test]
    fn generated_passwords_do_not_repeat_in_practice() {
        let seen: std::collections::HashSet<_> =
            (0..256).map(|_| generate_initial_password()).collect();
        // 54^16 ≈ 8e27 space; 256 draws colliding would mean a broken entropy path.
        assert_eq!(seen.len(), 256, "generator produced duplicates");
    }

    #[test]
    fn byte_mapping_is_uniform_domain_and_deterministic() {
        assert_eq!(ALPHABET.len(), 54);
        assert_eq!(
            UNBIASED_LIMIT, 216,
            "4 * 54: bytes 216..=255 must be dropped"
        );
        // Every byte 0..=255: below-limit bytes map by modulo, the rest drop.
        // Pin the exact mapping so the bias guard can't silently regress.
        let expected: String = (0u16..256)
            .filter(|b| usize::from(*b) < UNBIASED_LIMIT)
            .map(|b| ALPHABET[usize::from(b as u8) % ALPHABET.len()] as char)
            .take(INITIAL_PASSWORD_LEN)
            .collect();
        assert_eq!(password_from_bytes((0..=255u8).chain(0..=255u8)), expected);
    }

    #[test]
    fn argon2_round_trip() {
        let phc = hash_password("correct horse battery staple").unwrap();
        assert!(phc.starts_with("$argon2id$"), "stored form is a PHC string");
        assert!(
            phc.contains("m=19456") && phc.contains("t=2") && phc.contains("p=1"),
            "crate defaults must remain the OWASP baseline: {phc}"
        );
        assert!(verify_password("correct horse battery staple", &phc).unwrap());
        assert!(!verify_password("Tr0ub4dor&3", &phc).unwrap());
    }

    #[test]
    fn verify_rejects_corrupt_hash() {
        assert!(verify_password("x", "not-a-phc-string").is_err());
    }
}
