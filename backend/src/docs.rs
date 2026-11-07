//! The OpenAPI document (spec §0.1: utoipa 6; Swagger UI is development-only,
//! issue #15 erects the skeleton; endpoint modules register per ticket).

use utoipa::OpenApi;

#[derive(OpenApi)]
// Paths are collected by OpenApiRouter::routes!/nest — do not also declare
// them here or they appear twice in the document.
#[openapi(info(
    title = "rs-admin API",
    description = "Login-system API of the rs-admin management console \
                       (docs/spec.md §5; camelCase JSON; errors are `{code, title}`)."
))]
pub struct ApiDoc;
