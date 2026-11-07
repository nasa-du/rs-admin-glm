//! Parsing and validation of the `GET /users` query string (spec §5.4).
//!
//! Pagination, sort and filters are pure data here: the handler turns a
//! [`ValidatedList`] into SQL. Keeping the whitelist and bounds in a pure
//! module makes them unit-testable without a database, and keeps untrusted
//! strings out of the SQL text (sort columns are enum values, never raw
//! query input).

use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};

use crate::auth::Role;
use crate::error::{AppError, FieldErrors};

/// 1-based page number (spec §5.4).
pub const DEFAULT_PAGE: i64 = 1;
/// Default page size (spec §5.4).
pub const DEFAULT_PAGE_SIZE: i64 = 10;
/// Upper bound on `pageSize` (spec §5.4).
pub const MAX_PAGE_SIZE: i64 = 100;

/// The raw query string of `GET /users`.
///
/// `role` is repeatable (`role=admin&role=user`), which is why axum-extra's
/// `Query` (serde_html_form) is used instead of axum's single-value `Query`.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    /// Single column, optionally suffixed `.asc` / `.desc`.
    pub sort: Option<String>,
    /// Zero or more roles: `role=admin&role=user`.
    #[serde(default)]
    pub role: Vec<Role>,
    /// `enabled | disabled` (single-valued).
    pub status: Option<StatusFilter>,
    /// Fuzzy match over login identifier and display name.
    pub q: Option<String>,
}

/// `status` filter values (spec §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum StatusFilter {
    Enabled,
    Disabled,
}

impl StatusFilter {
    /// The `is_enabled` value this filter matches.
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

/// A whitelisted sort column (spec §5.4). The SQL identifier is selected
/// from this enum, never interpolated from the query string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Email,
    DisplayName,
    Role,
    IsEnabled,
    CreatedAt,
    UpdatedAt,
}

impl SortColumn {
    /// The `ORDER BY` expression for this column.
    pub fn sql(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::DisplayName => "display_name",
            Self::Role => "role",
            Self::IsEnabled => "is_enabled",
            Self::CreatedAt => "created_at",
            Self::UpdatedAt => "updated_at",
        }
    }

    /// Whitelist lookup, exact and case-sensitive (`displayName`).
    fn parse(field: &str) -> Option<Self> {
        match field {
            "email" => Some(Self::Email),
            "displayName" => Some(Self::DisplayName),
            "role" => Some(Self::Role),
            "isEnabled" => Some(Self::IsEnabled),
            "createdAt" => Some(Self::CreatedAt),
            "updatedAt" => Some(Self::UpdatedAt),
            _ => None,
        }
    }
}

/// A parsed `sort` parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: SortColumn,
    pub descending: bool,
}

impl Default for Sort {
    /// `createdAt.desc` (spec §5.4).
    fn default() -> Self {
        Self {
            column: SortColumn::CreatedAt,
            descending: true,
        }
    }
}

impl Sort {
    /// Parse `field` or `field.asc` / `field.desc`. Returns `None` for a
    /// column outside the whitelist or an unknown direction.
    fn parse(raw: &str) -> Option<Self> {
        let (field, direction) = match raw.rsplit_once('.') {
            Some((field, direction)) => (field, Some(direction)),
            None => (raw, None),
        };
        let column = SortColumn::parse(field)?;
        let descending = match direction {
            None | Some("asc") => false,
            Some("desc") => true,
            Some(_) => return None,
        };
        Some(Self { column, descending })
    }

    /// The `ASC` / `DESC` keyword.
    pub fn direction_sql(self) -> &'static str {
        if self.descending { "DESC" } else { "ASC" }
    }
}

/// A list query that passed validation; the handler can build SQL from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedList {
    pub page: i64,
    pub page_size: i64,
    pub sort: Sort,
    pub roles: Vec<Role>,
    pub status: Option<bool>,
    /// An ILIKE pattern (`%…%`) with `%`, `_` and `\` escaped.
    pub search_pattern: Option<String>,
}

impl ValidatedList {
    /// Rows to skip: `(page - 1) * pageSize`, saturating rather than
    /// overflowing on an absurd page number.
    pub fn offset(&self) -> i64 {
        self.page.saturating_sub(1).saturating_mul(self.page_size)
    }
}

impl ListQuery {
    /// Enforce the §5.4 bounds and whitelist. Invalid values are 422
    /// `VALIDATION_ERROR` with a field-level code.
    pub fn validate(self) -> Result<ValidatedList, AppError> {
        let mut errors = FieldErrors::new();

        let page = self.page.unwrap_or(DEFAULT_PAGE);
        if page < DEFAULT_PAGE {
            errors.insert("page".to_owned(), vec!["PAGE_INVALID".to_owned()]);
        }

        let page_size = self.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
        if !(1..=MAX_PAGE_SIZE).contains(&page_size) {
            errors.insert("pageSize".to_owned(), vec!["PAGE_SIZE_INVALID".to_owned()]);
        }

        let sort = match self.sort.as_deref() {
            None => Sort::default(),
            Some(raw) => Sort::parse(raw).ok_or_else(|| {
                let mut errors = FieldErrors::new();
                errors.insert("sort".to_owned(), vec!["SORT_INVALID".to_owned()]);
                AppError::ValidationError(errors)
            })?,
        };

        let search_pattern = self
            .q
            .map(|q| q.trim().to_owned())
            .filter(|q| !q.is_empty())
            .map(|q| format!("%{}%", escape_like(&q)));

        if !errors.is_empty() {
            return Err(AppError::ValidationError(errors));
        }

        Ok(ValidatedList {
            page,
            page_size,
            sort,
            roles: self.role,
            status: self.status.map(StatusFilter::is_enabled),
            search_pattern,
        })
    }
}

/// Escape the `LIKE` metacharacters so user input is matched literally:
/// `\` first, then `%` and `_` (spec §5.4).
pub fn escape_like(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> ListQuery {
        ListQuery {
            page: None,
            page_size: None,
            sort: None,
            role: Vec::new(),
            status: None,
            q: None,
        }
    }

    fn codes(err: AppError) -> FieldErrors {
        match err {
            AppError::ValidationError(errors) => errors,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn defaults_are_page_one_page_size_ten_created_at_desc() {
        let validated = query().validate().unwrap();
        assert_eq!(validated.page, 1);
        assert_eq!(validated.page_size, 10);
        assert_eq!(
            validated.sort,
            Sort {
                column: SortColumn::CreatedAt,
                descending: true
            }
        );
        assert_eq!(validated.offset(), 0);
        assert!(validated.roles.is_empty());
        assert_eq!(validated.status, None);
        assert_eq!(validated.search_pattern, None);
    }

    #[test]
    fn page_bounds_are_field_level_422s() {
        let errors = codes(
            ListQuery {
                page: Some(0),
                ..query()
            }
            .validate()
            .unwrap_err(),
        );
        assert_eq!(errors["page"], vec!["PAGE_INVALID"]);

        for size in [0, -1, MAX_PAGE_SIZE + 1] {
            let errors = codes(
                ListQuery {
                    page_size: Some(size),
                    ..query()
                }
                .validate()
                .unwrap_err(),
            );
            assert_eq!(errors["pageSize"], vec!["PAGE_SIZE_INVALID"], "size {size}");
        }
    }

    #[test]
    fn sort_whitelist_accepts_every_documented_column() {
        for (raw, column, descending) in [
            ("email", SortColumn::Email, false),
            ("email.asc", SortColumn::Email, false),
            ("email.desc", SortColumn::Email, true),
            ("displayName", SortColumn::DisplayName, false),
            ("role.asc", SortColumn::Role, false),
            ("isEnabled.desc", SortColumn::IsEnabled, true),
            ("createdAt.desc", SortColumn::CreatedAt, true),
            ("updatedAt", SortColumn::UpdatedAt, false),
        ] {
            assert_eq!(
                Sort::parse(raw),
                Some(Sort { column, descending }),
                "sort={raw}"
            );
        }
    }

    #[test]
    fn sort_rejects_columns_outside_the_whitelist_and_bad_directions() {
        for raw in [
            "password_hash.desc",
            "id.asc",
            "display_name.asc",
            "email.sideways",
            "email.DESC",
            "",
        ] {
            assert_eq!(Sort::parse(raw), None, "sort={raw}");
            let errors = codes(
                ListQuery {
                    sort: Some(raw.to_owned()),
                    ..query()
                }
                .validate()
                .unwrap_err(),
            );
            assert_eq!(errors["sort"], vec!["SORT_INVALID"], "sort={raw}");
        }
    }

    #[test]
    fn like_metacharacters_are_escaped() {
        assert_eq!(escape_like("plain"), "plain");
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("back\\slash"), "back\\\\slash");
        let validated = ListQuery {
            q: Some("  a_b  ".to_owned()),
            ..query()
        }
        .validate()
        .unwrap();
        assert_eq!(validated.search_pattern.as_deref(), Some("%a\\_b%"));
    }

    #[test]
    fn empty_q_means_no_search() {
        let validated = ListQuery {
            q: Some("   ".to_owned()),
            ..query()
        }
        .validate()
        .unwrap();
        assert_eq!(validated.search_pattern, None);
    }

    #[test]
    fn offset_uses_one_based_pages() {
        let validated = ListQuery {
            page: Some(3),
            page_size: Some(20),
            ..query()
        }
        .validate()
        .unwrap();
        assert_eq!(validated.offset(), 40);
    }
}
