use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Braian",
        version = "0.0.0",
        description = "Write and read definitions and objects."
    ),
    modifiers(&SecurityAddon, &PlatformPaths),
    tags(
        (name = "meta", description = "Process and caller"),
        (name = "definitions", description = "Versioned model. One request is one revision."),
        (name = "objects", description = "Object values. Addresses accept a key or an id; responses use keys."),
        (name = "platform", description = "Auth, search, workspaces, proposals, jobs and the live stream")
    ),
    paths(
        crate::health,
        crate::me,
        crate::openapi_json,
        crate::definitions::get_definitions,
        crate::definitions::put_definitions,
        crate::definitions::post_attribute,
        crate::definitions::get_attribute,
        crate::definitions::put_attribute,
        crate::definitions::delete_attribute_route,
        crate::definitions::post_type,
        crate::definitions::get_type,
        crate::definitions::put_type,
        crate::definitions::delete_type_route,
        crate::definitions::post_structure,
        crate::definitions::get_structure,
        crate::definitions::put_structure,
        crate::definitions::delete_structure_route,
        crate::definitions::post_unit_family,
        crate::definitions::get_unit_family,
        crate::definitions::put_unit_family,
        crate::definitions::delete_unit_family_route,
        crate::definitions::put_languages,
        crate::objects::create_object,
        crate::objects::batch_get,
        crate::objects::get_object,
        crate::objects::patch_object,
        crate::objects::delete_object,
        crate::objects::object_changes,
    ),
    components(schemas(
        crate::problem::Problem,
        crate::problem::ConflictBody,
        crate::objects::CreateObject,
        crate::objects::BatchGetRequest,
        crate::objects::BatchGetResponse,
        crate::objects::PatchObject,
        crate::objects::ObjectResponse,
        crate::objects::DeletedResponse,
        crate::objects::ChangeResponse,
        crate::definitions::DefinitionWrite,
        crate::Me,
        braian_model::DefinitionSet,
        braian_model::Attribute,
        braian_model::ObjectType,
        braian_model::Structure,
        braian_model::UnitFamily,
        braian_model::ValidationError,
    ))
)]
struct ApiDoc;

pub fn spec() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

struct PlatformPaths;

impl utoipa::Modify for PlatformPaths {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::path::{HttpMethod, OperationBuilder};
        let routes = [
            (HttpMethod::Post, "/v1/login", "Start a cookie session"),
            (HttpMethod::Post, "/v1/logout", "Clear the cookie session"),
            (
                HttpMethod::Post,
                "/v1/tokens",
                "Create an API token for a person",
            ),
            (
                HttpMethod::Get,
                "/v1/operations",
                "List operations the caller may run",
            ),
            (
                HttpMethod::Post,
                "/v1/operations/{name}",
                "Run one operation. Agents receive a proposal.",
            ),
            (
                HttpMethod::Post,
                "/mcp",
                "MCP endpoint over the operation registry",
            ),
            (
                HttpMethod::Post,
                "/v1/agent",
                "Proxy a chat completion when a model key is set",
            ),
            (
                HttpMethod::Get,
                "/v1/stream",
                "Server-sent events for committed changes",
            ),
            (
                HttpMethod::Get,
                "/v1/search",
                "Search the index. Supports filter, sort, group and page",
            ),
            (
                HttpMethod::Get,
                "/v1/objects",
                "List object heads. type and q run in SQL before limit; cursor pages the rest",
            ),
            (
                HttpMethod::Post,
                "/v1/objects/names",
                "Display names for a batch of objects",
            ),
            (
                HttpMethod::Post,
                "/v1/export",
                "Export the current view as an xlsx file. Large exports run as a job.",
            ),
            (
                HttpMethod::Get,
                "/v1/exports/{id}",
                "Download a finished xlsx export",
            ),
            (HttpMethod::Get, "/v1/stats", "Object counts by type"),
            (HttpMethod::Get, "/v1/workspaces", "List workspaces"),
            (
                HttpMethod::Post,
                "/v1/workspaces",
                "Create or update a workspace layout",
            ),
            (
                HttpMethod::Get,
                "/v1/workspaces/{id}",
                "Workspace with views and layout",
            ),
            (
                HttpMethod::Post,
                "/v1/workspaces/{id}/assign",
                "Assign a workspace to a person",
            ),
            (
                HttpMethod::Post,
                "/v1/share/{token}",
                "Open a workspace by share token",
            ),
            (
                HttpMethod::Post,
                "/v1/views",
                "Create or update a view, including table state",
            ),
            (HttpMethod::Get, "/v1/view-types", "List view types"),
            (HttpMethod::Post, "/v1/view-types", "Register a view type"),
            (HttpMethod::Get, "/v1/assortments", "List saved searches"),
            (HttpMethod::Post, "/v1/assortments", "Save an assortment"),
            (HttpMethod::Get, "/v1/job-types", "List job types"),
            (HttpMethod::Post, "/v1/job-types", "Register a job type"),
            (
                HttpMethod::Get,
                "/v1/job-types/{key}/runs",
                "Runs for one job type",
            ),
            (HttpMethod::Get, "/v1/jobs/{id}", "One job run"),
            (
                HttpMethod::Post,
                "/v1/bulk",
                "Write many objects as one job",
            ),
            (
                HttpMethod::Post,
                "/v1/imports",
                "Upload a spreadsheet import",
            ),
            (
                HttpMethod::Get,
                "/v1/imports/{id}",
                "Read an import session",
            ),
            (
                HttpMethod::Post,
                "/v1/imports/{id}/sheet",
                "Switch the imported sheet",
            ),
            (
                HttpMethod::Post,
                "/v1/imports/{id}/remap",
                "Re-run column mapping for a type",
            ),
            (
                HttpMethod::Put,
                "/v1/imports/{id}/mapping",
                "Save the column mapping",
            ),
            (
                HttpMethod::Post,
                "/v1/imports/{id}/approve",
                "Build the import preview",
            ),
            (
                HttpMethod::Get,
                "/v1/imports/{id}/rows",
                "Read preview rows",
            ),
            (HttpMethod::Post, "/v1/imports/{id}/save", "Save the import"),
            (
                HttpMethod::Get,
                "/v1/imports/{id}/failed.xlsx",
                "Download failing rows",
            ),
            (HttpMethod::Delete, "/v1/imports/{id}", "Discard an import"),
            (HttpMethod::Get, "/v1/proposals", "List proposals"),
            (HttpMethod::Post, "/v1/proposals", "Create a proposal"),
            (HttpMethod::Get, "/v1/proposals/{id}", "Read a proposal"),
            (
                HttpMethod::Post,
                "/v1/proposals/{id}/accept",
                "Accept a proposal in full or by row",
            ),
            (
                HttpMethod::Post,
                "/v1/proposals/{id}/discard",
                "Discard a proposal",
            ),
            (
                HttpMethod::Get,
                "/v1/history",
                "Recent changes across objects",
            ),
            (
                HttpMethod::Get,
                "/v1/objects/{id}/as-of",
                "Object values as of a sequence",
            ),
            (
                HttpMethod::Post,
                "/v1/objects/{id}/rollback",
                "Propose a rollback to a sequence",
            ),
            (
                HttpMethod::Get,
                "/v1/objects/{id}/graph",
                "Relations and where each attribute comes from",
            ),
            (
                HttpMethod::Post,
                "/v1/objects/{id}/relations",
                "Set a relation",
            ),
            (
                HttpMethod::Post,
                "/v1/impact",
                "Affected objects and jobs for a breaking definition change",
            ),
            (HttpMethod::Get, "/v1/webhooks", "List webhooks"),
            (HttpMethod::Post, "/v1/webhooks", "Create a webhook"),
            (
                HttpMethod::Get,
                "/v1/frecency",
                "Command frecency for the signed-in person",
            ),
            (
                HttpMethod::Post,
                "/v1/frecency",
                "Record one use of a command for the signed-in person",
            ),
            (
                HttpMethod::Get,
                "/v1/workspaces/{id}/state",
                "Personal workspace layout for the signed-in person and environment",
            ),
            (
                HttpMethod::Put,
                "/v1/workspaces/{id}/state",
                "Save the personal workspace layout",
            ),
            (
                HttpMethod::Delete,
                "/v1/workspaces/{id}/state",
                "Delete the personal workspace layout",
            ),
        ];
        for (method, path, summary) in routes {
            let operation = OperationBuilder::new()
                .summary(Some(summary))
                .tags(Some(["platform"]))
                .build();
            openapi
                .paths
                .add_path_operation(path, vec![method], operation);
        }
    }
}

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let Some(components) = openapi.components.as_mut() else {
            return;
        };
        components.add_security_scheme(
            "bearer",
            utoipa::openapi::security::SecurityScheme::Http(
                utoipa::openapi::security::Http::builder()
                    .scheme(utoipa::openapi::security::HttpAuthScheme::Bearer)
                    .bearer_format("token")
                    .description(Some(
                        "Dev token from BRAIAN_DEV_TOKENS. An agent token acts for a person."
                            .to_owned(),
                    ))
                    .build(),
            ),
        );
    }
}
