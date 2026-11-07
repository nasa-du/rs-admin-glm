//! M1 schema acceptance (issue #14): the two migrations must land a structure
//! that matches docs/spec.md §2 column-for-column, and deleting a user must
//! cascade their refresh-token families away (§2.2 semantics).
//!
//! `#[sqlx::test]` gives each test a fresh temp database with the migrations
//! applied — which is itself the "migrations apply cleanly" check on every run.

use sqlx::PgPool;

/// (name, data_type, udt_name, is_nullable) as information_schema reports them.
/// Order matters: this is the spec §2.1 column list top to bottom.
const USERS_COLUMNS: &[(&str, &str, &str, &str)] = &[
    ("id", "uuid", "uuid", "NO"),
    ("email", "text", "text", "NO"),
    ("display_name", "text", "text", "NO"),
    ("password_hash", "text", "text", "NO"),
    ("role", "USER-DEFINED", "user_role", "NO"),
    ("is_enabled", "boolean", "bool", "NO"),
    ("must_change_password", "boolean", "bool", "NO"),
    (
        "created_at",
        "timestamp with time zone",
        "timestamptz",
        "NO",
    ),
    (
        "updated_at",
        "timestamp with time zone",
        "timestamptz",
        "NO",
    ),
];

const REFRESH_TOKENS_COLUMNS: &[(&str, &str, &str, &str)] = &[
    ("id", "uuid", "uuid", "NO"),
    ("family_id", "uuid", "uuid", "NO"),
    ("user_id", "uuid", "uuid", "NO"),
    ("token_hash", "character", "bpchar", "NO"),
    (
        "expires_at",
        "timestamp with time zone",
        "timestamptz",
        "NO",
    ),
    (
        "consumed_at",
        "timestamp with time zone",
        "timestamptz",
        "YES",
    ),
    (
        "created_at",
        "timestamp with time zone",
        "timestamptz",
        "NO",
    ),
];

async fn columns(pool: &PgPool, table: &str) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT column_name, data_type, udt_name, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema = 'public' AND table_name = $1 \
         ORDER BY ordinal_position",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// `&str` table rows → owned, for comparison against the query result.
fn owned(rows: &[(&str, &str, &str, &str)]) -> Vec<(String, String, String, String)> {
    rows.iter()
        .map(|(a, b, c, d)| ((*a).into(), (*b).into(), (*c).into(), (*d).into()))
        .collect()
}

#[sqlx::test]
async fn users_table_matches_spec_section_2_1(pool: PgPool) {
    let got = columns(&pool, "users").await;
    assert_eq!(
        got,
        owned(USERS_COLUMNS),
        "users columns must match spec §2.1 exactly"
    );
}

#[sqlx::test]
async fn refresh_tokens_table_matches_spec_section_2_2(pool: PgPool) {
    let got = columns(&pool, "refresh_tokens").await;
    assert_eq!(
        got,
        owned(REFRESH_TOKENS_COLUMNS),
        "refresh_tokens columns must match spec §2.2 exactly"
    );

    // token_hash is CHAR(64)
    let char_len: i32 = sqlx::query_scalar(
        "SELECT character_maximum_length FROM information_schema.columns \
         WHERE table_name = 'refresh_tokens' AND column_name = 'token_hash'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(char_len, 64, "token_hash is the SHA-256 hex width");
}

#[sqlx::test]
async fn user_role_enum_holds_exactly_the_three_ranks(pool: PgPool) {
    let values: Vec<String> = sqlx::query_scalar(
        "SELECT e.enumlabel FROM pg_enum e \
         JOIN pg_type t ON t.oid = e.enumtypid WHERE t.typname = 'user_role' \
         ORDER BY e.enumsortorder",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(values, vec!["superadmin", "admin", "user"]);
}

#[sqlx::test]
async fn key_constraints_match_spec(pool: PgPool) {
    // email UNIQUE under the name the seed CLI's unique-violation probe expects.
    let email_unique: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint \
         WHERE conrelid = 'users'::regclass AND contype = 'u' \
         AND conname = 'users_email_key' AND conkey = ARRAY[(SELECT attnum \
             FROM pg_attribute WHERE attrelid = 'users'::regclass \
             AND attname = 'email')]::smallint[]",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(email_unique, 1, "users_email_key unique on email");

    // token_hash UNIQUE
    let hash_unique: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint \
         WHERE conrelid = 'refresh_tokens'::regclass AND contype = 'u' \
         AND conname = 'refresh_tokens_token_hash_key'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hash_unique, 1, "token_hash unique");

    // user_id FK → users ON DELETE CASCADE (§2.2: 删用户级联撤销);
    // pg_constraint confdeltype 'c' = CASCADE
    let cascade: String = sqlx::query_scalar(
        "SELECT confdeltype::text FROM pg_constraint \
         WHERE conrelid = 'refresh_tokens'::regclass AND contype = 'f' \
         AND conname = 'refresh_tokens_user_id_fkey'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cascade, "c", "refresh_tokens.user_id ON DELETE CASCADE");

    // the two §2.2 indexes exist by name
    let indexes: Vec<String> = sqlx::query_scalar(
        "SELECT indexname FROM pg_indexes \
         WHERE tablename = 'refresh_tokens' ORDER BY indexname",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(indexes.contains(&"idx_refresh_tokens_family".to_string()));
    assert!(indexes.contains(&"idx_refresh_tokens_user".to_string()));
}

#[sqlx::test]
async fn deleting_a_user_cascades_their_token_families_away(pool: PgPool) {
    let user_id: String = sqlx::query_scalar(
        "INSERT INTO users (email, display_name, password_hash) \
             VALUES ('gone@example.com', 'Gone', 'x') RETURNING id::text",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    for hash_prefix in ["aa", "bb"] {
        sqlx::query(
            "INSERT INTO refresh_tokens (family_id, user_id, token_hash, expires_at) \
                     VALUES (gen_random_uuid(), $1::uuid, $2, now() + interval '7 days')",
        )
        .bind(&user_id)
        .bind(format!(
            "{hash_prefix}{}",
            "0".repeat(64 - hash_prefix.len())
        ))
        .execute(&pool)
        .await
        .unwrap();
    }
    let tokens: i64 = sqlx::query_scalar("SELECT count(*) FROM refresh_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tokens, 2, "two families seeded");

    sqlx::query("DELETE FROM users WHERE id = $1::uuid")
        .bind(&user_id)
        .execute(&pool)
        .await
        .unwrap();

    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM refresh_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0, "deleting the user must revoke every family (§2.2)");
}

#[sqlx::test]
async fn migrations_replay_is_a_no_op(pool: PgPool) {
    // #[sqlx::test] already applied them once; a second run must succeed
    // without touching anything (幂等可重放, acceptance criterion 1).
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("second migrate run is a no-op");
}
