//! `seed-superadmin` — the single trusted bootstrap entry (spec §2.3, #14).
//!
//! A cold-started system has exactly one way in: its first superadmin is born
//! via this CLI. The generated password is printed once and never stored in
//! the clear; the account lands `is_enabled = true` and
//! `must_change_password = true` (initial-password semantics, CONTEXT.md).

use anyhow::{Context, Result, bail, ensure};
use sqlx::PgPool;

use crate::password;

/// CLI options of `seed-superadmin`.
#[derive(Debug)]
pub struct SeedOptions {
    pub email: String,
    pub password: Option<String>,
}

impl SeedOptions {
    /// Parse `--email <email> [--password <password>]`; accepts both
    /// `--flag value` and `--flag=value`. Strict: unknown flags, duplicates
    /// and missing values are errors, not silently ignored.
    pub fn parse(args: &[String]) -> Result<Self> {
        const USAGE: &str =
            "usage: cargo run -- seed-superadmin --email <email> [--password <password>]";

        let mut email: Option<String> = None;
        let mut password: Option<String> = None;
        let mut args = args.iter();

        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) => (flag, Some(value.to_owned())),
                None => (arg.as_str(), None),
            };
            let slot = match flag {
                "--email" => &mut email,
                "--password" => &mut password,
                other => bail!("unexpected argument `{other}`\n{USAGE}"),
            };
            if slot.is_some() {
                bail!("`{flag}` given more than once\n{USAGE}");
            }
            let value = match inline {
                Some(value) => value,
                None => {
                    let next = args.next().context(format!("missing value for `{flag}`"));
                    match next {
                        // A following `--flag` reads as a missing value, not as one.
                        Ok(v) if v.starts_with("--") => {
                            bail!(
                                "missing value for `{flag}` (next token is another flag)\n{USAGE}"
                            )
                        }
                        Ok(v) => v.clone(),
                        Err(err) => return Err(err),
                    }
                }
            };
            *slot = Some(value);
        }

        let email = email.with_context(|| format!("missing required `--email`\n{USAGE}"))?;
        Ok(Self { email, password })
    }
}

/// Failures of [`create_superadmin`].
#[derive(Debug)]
pub enum SeedError {
    /// Email already present — seed never updates existing users (spec §2.3).
    EmailTaken,
    /// Anything else; reported via anyhow.
    Internal(anyhow::Error),
}

impl From<sqlx::Error> for SeedError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal(err.into())
    }
}

/// Insert the superadmin row. Idempotence is by email: an existing email is
/// `EmailTaken`, never an update. The unique constraint on `users.email` is
/// the race-safe backstop behind the friendly check-then-insert.
pub async fn create_superadmin(
    pool: &PgPool,
    email: &str,
    password: &str,
) -> std::result::Result<String, SeedError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE email = $1)")
        .bind(email)
        .fetch_one(pool)
        .await?;
    if exists {
        return Err(SeedError::EmailTaken);
    }

    let password_hash = password::hash_password(password).map_err(SeedError::Internal)?;

    let id: String = sqlx::query_scalar(
        r#"INSERT INTO users (email, display_name, password_hash, role, is_enabled, must_change_password)
           VALUES ($1, 'Superadmin', $2, 'superadmin', TRUE, TRUE)
           RETURNING id::text"#,
    )
    .bind(email)
    .bind(&password_hash)
    .fetch_one(pool)
    .await
    .map_err(|err| {
        if is_users_email_unique_violation(&err) {
            SeedError::EmailTaken
        } else {
            SeedError::Internal(err.into())
        }
    })?;
    Ok(id)
}

fn is_users_email_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(|db| db.is_unique_violation() && db.constraint() == Some("users_email_key"))
}

/// CLI entrypoint: parse, validate, migrate, seed, print the password once.
pub async fn run(args: &[String]) -> Result<()> {
    let options = SeedOptions::parse(args)?;
    validate_email(&options.email)?;
    if let Some(explicit) = &options.password {
        validate_explicit_password(explicit)?;
    }

    let pool = crate::bootstrap().await?;

    let plaintext = options
        .password
        .clone()
        .unwrap_or_else(password::generate_initial_password);
    let id = create_superadmin(&pool, &options.email, &plaintext)
        .await
        .map_err(|err| match err {
            SeedError::EmailTaken => anyhow::anyhow!(
                "a user with email `{}` already exists — seed-superadmin never updates \
                 existing users (spec §2.3)",
                options.email
            ),
            SeedError::Internal(err) => err,
        })?;

    println!("superadmin created");
    println!("  id:       {id}");
    println!("  email:    {}", options.email);
    match &options.password {
        // Print-once applies to the generated initial password (spec §2.3);
        // an explicit --password is already known to its operator.
        Some(_) => println!("  password: (as provided via --password)"),
        None => {
            println!("  password: {plaintext}");
            println!();
            println!(
                "This password is shown only once and cannot be recovered. The account is\n\
                 enabled and must change this password at first sign-in."
            );
        }
    }
    Ok(())
}

/// Light CLI sanity check only — the real format rules (EMAIL_INVALID etc.,
/// spec §5.2) belong to the API layer's validation, not to the seeder.
fn validate_email(email: &str) -> Result<()> {
    let (local, domain) = email
        .split_once('@')
        .context("email must contain `@` (e.g. admin@example.com)")?;
    ensure!(!local.is_empty(), "email local part is empty");
    ensure!(!domain.is_empty(), "email domain part is empty");
    ensure!(
        !email.chars().any(char::is_whitespace),
        "email must not contain whitespace"
    );
    Ok(())
}

/// Explicit `--password` must respect the 8–128 length policy (spec §2.1):
/// no composition rules, upper bound caps argon2 work against overlong input.
fn validate_explicit_password(password: &str) -> Result<()> {
    let len = password.chars().count();
    ensure!(
        (8..=128).contains(&len),
        "password must be 8–128 characters (got {len})"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_both_flag_forms() {
        let spaced =
            SeedOptions::parse(&args(&["--email", "a@b.c", "--password", "hunter2!"])).unwrap();
        assert_eq!(spaced.email, "a@b.c");
        assert_eq!(spaced.password.as_deref(), Some("hunter2!"));

        let inline = SeedOptions::parse(&args(&["--email=a@b.c"])).unwrap();
        assert_eq!(inline.email, "a@b.c");
        assert_eq!(inline.password, None);
    }

    #[test]
    fn rejects_missing_or_malformed_arguments() {
        for bad in [
            vec![],                                          // no email
            args(&["--email"]),                              // value missing at end
            args(&["--email", "--password", "x"]),           // value swallowed a flag
            args(&["--email", "a@b.c", "--email", "d@e.f"]), // duplicate
            args(&["--email", "a@b.c", "--wat"]),            // unknown flag
        ] {
            assert!(SeedOptions::parse(&bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn email_sanity_check() {
        assert!(validate_email("admin@example.com").is_ok());
        assert!(validate_email("admin@example").is_ok()); // CLI does not judge domains
        for bad in ["no-at-sign", "@example.com", "admin@", "a b@example.com"] {
            assert!(validate_email(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn explicit_password_length_bounds() {
        assert!(validate_explicit_password("12345678").is_ok());
        assert!(validate_explicit_password(&"x".repeat(128)).is_ok());
        assert!(validate_explicit_password("1234567").is_err());
        assert!(validate_explicit_password(&"x".repeat(129)).is_err());
    }

    #[sqlx::test]
    async fn create_superadmin_inserts_enabled_superadmin_with_verifiable_hash(pool: sqlx::PgPool) {
        let id = create_superadmin(&pool, "seed@example.com", "one-time-password")
            .await
            .unwrap();
        assert!(!id.is_empty(), "returns the new user id");

        let (role, hash, is_enabled, must_change): (String, String, bool, bool) = sqlx::query_as(
            "SELECT role::text, password_hash, is_enabled, must_change_password \
                 FROM users WHERE email = $1",
        )
        .bind("seed@example.com")
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(role, "superadmin");
        assert!(is_enabled);
        assert!(
            must_change,
            "initial password must be changed at first login"
        );
        assert!(password::verify_password("one-time-password", &hash).unwrap());
        assert!(
            hash.starts_with("$argon2id$"),
            "stored form is a PHC string"
        );
    }

    #[sqlx::test]
    async fn create_superadmin_on_existing_email_errors_without_updating(pool: sqlx::PgPool) {
        create_superadmin(&pool, "seed@example.com", "password-one")
            .await
            .unwrap();

        let err = create_superadmin(&pool, "seed@example.com", "password-two")
            .await
            .unwrap_err();
        assert!(matches!(err, SeedError::EmailTaken));

        let (count, hash): (i64, String) =
            sqlx::query_as("SELECT count(*), max(password_hash) FROM users")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "no second row was inserted");
        assert!(
            password::verify_password("password-one", &hash).unwrap(),
            "original row must be untouched"
        );
        assert!(!password::verify_password("password-two", &hash).unwrap());
    }
}
