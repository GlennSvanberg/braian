//! Schemas for handlers that still return `serde_json::Value`.
//!
//! Each operation's request and response match the JSON that handler builds.

use utoipa::IntoParams;
use utoipa::openapi::path::{
    HttpMethod, Operation, OperationBuilder, Parameter, ParameterBuilder, ParameterIn,
};
use utoipa::openapi::request_body::RequestBodyBuilder;
use utoipa::openapi::schema::{ObjectBuilder, Schema, SchemaFormat, Type};
use utoipa::openapi::security::SecurityRequirement;
use utoipa::openapi::{
    ContentBuilder, Ref, RefOr, Required, Response, ResponseBuilder, ResponsesBuilder,
};

struct Resp {
    status: &'static str,
    description: &'static str,
    schema: Option<&'static str>,
    content_type: &'static str,
}

fn json(status: &'static str, description: &'static str, schema: &'static str) -> Resp {
    Resp {
        status,
        description,
        schema: Some(schema),
        content_type: "application/json",
    }
}

fn empty(status: &'static str, description: &'static str) -> Resp {
    Resp {
        status,
        description,
        schema: None,
        content_type: "application/json",
    }
}

fn problem(status: &'static str, description: &'static str) -> Resp {
    Resp {
        status,
        description,
        schema: Some("Problem"),
        content_type: "application/problem+json",
    }
}

pub fn apply(openapi: &mut utoipa::openapi::OpenApi) {
    for (method, path, operation) in operations() {
        openapi
            .paths
            .add_path_operation(path, vec![method], operation);
    }
}

fn operations() -> Vec<(HttpMethod, &'static str, Operation)> {
    vec![
        call(
            HttpMethod::Post,
            "/v1/login",
            "login",
            "Start a cookie session",
            vec![],
            Some("LoginBody"),
            Some("SessionUser"),
            "200",
            "Cookie session started",
            false,
        ),
        call(
            HttpMethod::Post,
            "/v1/logout",
            "logout",
            "Clear the cookie session",
            vec![],
            None,
            None,
            "204",
            "Session cookie cleared",
            false,
        ),
        call(
            HttpMethod::Post,
            "/v1/tokens",
            "create_token",
            "Create an API token for a person",
            vec![],
            Some("TokenBody"),
            Some("CreatedToken"),
            "200",
            "Token created once",
            true,
        ),
        query::<crate::platform::SearchParams>(
            HttpMethod::Get,
            "/v1/search",
            "search",
            "Search the index. Supports filter, sort, group and page",
            vec![],
            None,
            Some("SearchResponse"),
            "200",
            "Search page",
        ),
        query::<crate::platform::ListParams>(
            HttpMethod::Get,
            "/v1/objects",
            "list_objects",
            "List object heads. type and q run in SQL before limit; cursor pages the rest",
            vec![],
            None,
            Some("ObjectList"),
            "200",
            "One page of object heads",
        ),
        call(
            HttpMethod::Post,
            "/v1/objects/names",
            "object_names",
            "Display names for a batch of objects",
            vec![],
            Some("NamesBody"),
            Some("NamedObjectList"),
            "200",
            "Names in request order, skipping missing ids",
            true,
        ),
        export_post(),
        export_get(),
        call(
            HttpMethod::Get,
            "/v1/stats",
            "stats",
            "Object counts by type",
            vec![],
            None,
            Some("StatsResponse"),
            "200",
            "Counts by type key",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/workspaces",
            "list_workspaces",
            "List workspaces",
            vec![],
            None,
            Some("WorkspaceList"),
            "200",
            "Workspaces the caller can open",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/workspaces",
            "save_workspace",
            "Create or update a workspace layout",
            vec![],
            Some("WorkspaceBody"),
            Some("IdBody"),
            "200",
            "Saved workspace id",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/workspaces/{id}",
            "get_workspace",
            "Workspace with views and layout",
            vec![path_uuid("id", "Workspace id")],
            None,
            Some("WorkspaceRow"),
            "200",
            "Workspace and its views",
            true,
        ),
        call(
            HttpMethod::Delete,
            "/v1/workspaces/{id}",
            "delete_workspace",
            "Delete a workspace and its views and layouts. Object data is not deleted. Locked or last workspaces are rejected.",
            vec![path_uuid("id", "Workspace id")],
            None,
            None,
            "204",
            "Workspace removed",
            true,
        ),
        query::<crate::platform::StateQuery>(
            HttpMethod::Get,
            "/v1/workspaces/{id}/state",
            "get_workspace_state",
            "Personal workspace layout for the signed-in person and environment",
            vec![path_uuid("id", "Workspace id")],
            None,
            Some("WorkspaceState"),
            "200",
            "Personal layout, or a null document when none is saved",
        ),
        call(
            HttpMethod::Put,
            "/v1/workspaces/{id}/state",
            "put_workspace_state",
            "Save the personal workspace layout",
            vec![path_uuid("id", "Workspace id")],
            Some("StateBody"),
            Some("WorkspaceStateSaved"),
            "200",
            "Saved layout",
            true,
        ),
        query::<crate::platform::StateQuery>(
            HttpMethod::Delete,
            "/v1/workspaces/{id}/state",
            "delete_workspace_state",
            "Delete the personal workspace layout",
            vec![path_uuid("id", "Workspace id")],
            None,
            None,
            "204",
            "Personal layout removed",
        ),
        call(
            HttpMethod::Post,
            "/v1/workspaces/{id}/assign",
            "assign_workspace",
            "Assign a workspace to a person",
            vec![path_uuid("id", "Workspace id")],
            Some("AssignBody"),
            None,
            "204",
            "Assignment saved",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/share/{token}",
            "open_share",
            "Open a workspace by share token",
            vec![path_string("token", "Share token")],
            None,
            Some("WorkspaceRow"),
            "200",
            "Shared workspace",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/views",
            "save_view",
            "Create or update a view, including table state",
            vec![],
            Some("ViewBody"),
            Some("IdBody"),
            "200",
            "Saved view id",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/view-types",
            "list_view_types",
            "List view types",
            vec![],
            None,
            Some("ViewTypeList"),
            "200",
            "Registered view types",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/view-types",
            "register_view_type",
            "Register a view type",
            vec![],
            Some("ViewTypeBody"),
            None,
            "204",
            "View type registered",
            true,
        ),
        query::<crate::platform::AssortmentTypeQuery>(
            HttpMethod::Get,
            "/v1/assortments",
            "list_assortments",
            "List selections. Optional type is an id or a key.",
            vec![],
            None,
            Some("AssortmentList"),
            "200",
            "Selections",
        ),
        call(
            HttpMethod::Post,
            "/v1/assortments",
            "save_assortment",
            "Save a selection. type is an id or a key and is not stored in query.",
            vec![],
            Some("AssortmentBody"),
            Some("AssortmentRow"),
            "200",
            "Saved selection",
            true,
        ),
        query::<crate::platform::AssortmentTypeQuery>(
            HttpMethod::Get,
            "/v1/assortments/{id}",
            "get_assortment",
            "Read one selection. type must be that selection's type.",
            vec![path_uuid("id", "Selection id")],
            None,
            Some("AssortmentRow"),
            "200",
            "One selection",
        ),
        call(
            HttpMethod::Get,
            "/v1/job-types",
            "list_job_types",
            "List job types",
            vec![],
            None,
            Some("JobTypeList"),
            "200",
            "Job types",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/job-types",
            "register_job_type",
            "Register a job type",
            vec![],
            Some("JobTypeBody"),
            None,
            "204",
            "Job type registered",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/job-types/{key}/runs",
            "job_runs",
            "Runs for one job type",
            vec![path_string("key", "Job type key")],
            None,
            Some("JobRunList"),
            "200",
            "Recent runs",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/jobs/{id}",
            "job_run",
            "One job run",
            vec![path_uuid("id", "Job id")],
            None,
            Some("JobRun"),
            "200",
            "Job run",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/bulk",
            "bulk_write",
            "Write many objects as one job",
            vec![],
            Some("BulkBody"),
            Some("RunningJob"),
            "202",
            "Bulk job accepted",
            true,
        ),
        import_upload(),
        call(
            HttpMethod::Get,
            "/v1/imports/{id}",
            "import_session",
            "Read an import session",
            vec![path_uuid("id", "Import id")],
            None,
            Some("ImportSessionView"),
            "200",
            "Import session",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/imports/{id}/sheet",
            "import_sheet",
            "Switch the imported sheet",
            vec![path_uuid("id", "Import id")],
            Some("SheetBody"),
            Some("ImportSessionView"),
            "200",
            "Session after the sheet change",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/imports/{id}/remap",
            "import_remap",
            "Re-run column mapping for a type",
            vec![path_uuid("id", "Import id")],
            Some("RemapBody"),
            Some("ImportSessionView"),
            "200",
            "Session after remapping",
            true,
        ),
        call(
            HttpMethod::Put,
            "/v1/imports/{id}/mapping",
            "import_mapping",
            "Save the column mapping",
            vec![path_uuid("id", "Import id")],
            Some("ImportMappingBody"),
            None,
            "204",
            "Mapping saved",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/imports/{id}/approve",
            "import_approve",
            "Build the import preview",
            vec![path_uuid("id", "Import id")],
            None,
            Some("ImportStatus"),
            "202",
            "Preview started",
            true,
        ),
        query::<crate::import::RowQuery>(
            HttpMethod::Get,
            "/v1/imports/{id}/rows",
            "import_rows",
            "Read preview rows",
            vec![path_uuid("id", "Import id")],
            None,
            Some("ImportPreviewPage"),
            "200",
            "Preview page",
        ),
        call(
            HttpMethod::Patch,
            "/v1/imports/{id}/rows/{row_no}",
            "import_row_edit",
            "Edit one cell of a draft row",
            vec![
                path_uuid("id", "Import id"),
                path_int("row_no", "Row number"),
            ],
            Some("ImportRowEdit"),
            Some("ImportRowEdited"),
            "200",
            "The row after it was checked again",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/import-mappings",
            "import_mappings_list",
            "List saved import mappings",
            vec![],
            None,
            Some("SavedMappingList"),
            "200",
            "Saved mappings",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/import-mappings",
            "import_mappings_create",
            "Save the mapping of an import for reuse",
            vec![],
            Some("SavedMappingBody"),
            Some("SavedMappingView"),
            "201",
            "Mapping saved",
            true,
        ),
        call(
            HttpMethod::Patch,
            "/v1/import-mappings/{id}",
            "import_mappings_rename",
            "Rename a saved mapping",
            vec![path_uuid("id", "Mapping id")],
            Some("SavedMappingRename"),
            None,
            "204",
            "Renamed",
            true,
        ),
        call(
            HttpMethod::Delete,
            "/v1/import-mappings/{id}",
            "import_mappings_delete",
            "Delete a saved mapping",
            vec![path_uuid("id", "Mapping id")],
            None,
            None,
            "204",
            "Deleted",
            true,
        ),
        import_failed(),
        call(
            HttpMethod::Delete,
            "/v1/imports/{id}",
            "import_discard",
            "Discard an import",
            vec![path_uuid("id", "Import id")],
            None,
            None,
            "204",
            "Import discarded",
            true,
        ),
        query::<crate::platform::ProposalListParams>(
            HttpMethod::Get,
            "/v1/proposals",
            "list_proposals",
            "List proposals",
            vec![],
            None,
            Some("ProposalList"),
            "200",
            "A page of proposals and the pending summary",
        ),
        query::<crate::platform::ProposalRowsParams>(
            HttpMethod::Get,
            "/v1/proposals/{id}/rows",
            "proposal_rows",
            "Read one page of a draft's rows",
            vec![path_uuid("id", "Proposal id")],
            None,
            Some("DraftRowsPage"),
            "200",
            "Rows of an import draft or an ordinary proposal",
        ),
        call(
            HttpMethod::Post,
            "/v1/proposals",
            "create_proposal",
            "Create a proposal",
            vec![],
            Some("ProposalBody"),
            Some("ProposalView"),
            "200",
            "Created proposal",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/proposals/{id}",
            "get_proposal",
            "Read a proposal",
            vec![path_uuid("id", "Proposal id")],
            None,
            Some("ProposalView"),
            "200",
            "Proposal and its rows",
            true,
        ),
        accept_proposal(),
        call(
            HttpMethod::Post,
            "/v1/proposals/{id}/discard",
            "discard_proposal",
            "Discard a proposal",
            vec![path_uuid("id", "Proposal id")],
            None,
            None,
            "204",
            "Proposal discarded",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/frecency",
            "get_frecency",
            "Command frecency for the signed-in person",
            vec![],
            None,
            Some("FrecencyList"),
            "200",
            "Use counts",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/frecency",
            "post_frecency",
            "Record one use of a command for the signed-in person",
            vec![],
            Some("FrecencyBody"),
            Some("FrecencyTouch"),
            "200",
            "Updated entry",
            true,
        ),
        query::<crate::platform::HistoryParams>(
            HttpMethod::Get,
            "/v1/history",
            "history",
            "Recent changes across objects",
            vec![],
            None,
            Some("HistoryList"),
            "200",
            "Change rows",
        ),
        query::<crate::platform::AsOfParams>(
            HttpMethod::Get,
            "/v1/objects/{id}/as-of",
            "object_as_of",
            "Object values as of a sequence",
            vec![path_uuid("id", "Object id")],
            None,
            Some("AsOfView"),
            "200",
            "Values replayed up to seq",
        ),
        call(
            HttpMethod::Post,
            "/v1/objects/{id}/rollback",
            "object_rollback",
            "Propose a rollback to a sequence",
            vec![path_uuid("id", "Object id")],
            Some("RollbackBody"),
            Some("ProposalView"),
            "200",
            "Rollback proposal",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/objects/{id}/graph",
            "object_graph",
            "Links and where each attribute comes from",
            vec![path_uuid("id", "Object id")],
            None,
            Some("ObjectGraph"),
            "200",
            "Graph for one object",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/impact",
            "impact",
            "Affected objects and jobs for a breaking definition change",
            vec![],
            Some("ImpactBody"),
            Some("ImpactReport"),
            "200",
            "Impact of the described change",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/webhooks",
            "list_webhooks",
            "List webhooks",
            vec![],
            None,
            Some("WebhookList"),
            "200",
            "Webhooks",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/webhooks",
            "create_webhook",
            "Create a webhook",
            vec![],
            Some("WebhookBody"),
            Some("CreatedWebhook"),
            "200",
            "Webhook id and secret",
            true,
        ),
        call(
            HttpMethod::Get,
            "/v1/operations",
            "list_operations",
            "List operations the caller may run",
            vec![],
            None,
            Some("OperationList"),
            "200",
            "Operations",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/operations/{name}",
            "call_operation",
            "Run one operation. Agents receive a proposal.",
            vec![path_string("name", "Operation name")],
            Some("FreeObject"),
            Some("JsonValue"),
            "200",
            "Operation result. The shape depends on the operation.",
            true,
        ),
        mcp(),
        agent(),
        call(
            HttpMethod::Post,
            "/v1/feedback",
            "submit_feedback",
            "Send in-app feedback. Admins only, and only when a webhook URL is configured.",
            vec![],
            Some("FeedbackBody"),
            None,
            "204",
            "Feedback delivered",
            true,
        ),
        stream(),
        call(
            HttpMethod::Get,
            "/v1/model/draft",
            "get_model_draft",
            "Read the signed-in person's model draft",
            vec![],
            None,
            Some("DraftView"),
            "200",
            "Draft, or an empty draft",
            true,
        ),
        call(
            HttpMethod::Delete,
            "/v1/model/draft",
            "discard_model_draft",
            "Discard the model draft",
            vec![],
            None,
            None,
            "204",
            "Draft discarded",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/model/draft/steps",
            "post_model_draft_steps",
            "Append, replace or remove model-draft steps",
            vec![],
            Some("DraftBody"),
            Some("DraftView"),
            "200",
            "Draft after the change",
            true,
        ),
        call(
            HttpMethod::Post,
            "/v1/model/draft/rebase",
            "rebase_model_draft",
            "Rebase the draft onto the current model",
            vec![],
            None,
            Some("DraftView"),
            "200",
            "Rebased draft",
            true,
        ),
        query::<crate::deletes::DryRun>(
            HttpMethod::Post,
            "/v1/model/draft/save",
            "save_model_draft",
            "Save the model draft as one revision",
            vec![],
            Some("SaveBody"),
            Some("ModelSaveResult"),
            "200",
            "Dry run or applied revision",
        ),
    ]
}

#[expect(
    clippy::too_many_arguments,
    reason = "one operation is one row of the route table"
)]
fn call(
    method: HttpMethod,
    path: &'static str,
    id: &'static str,
    summary: &'static str,
    params: Vec<Parameter>,
    body: Option<&'static str>,
    response: Option<&'static str>,
    status: &'static str,
    description: &'static str,
    secure: bool,
) -> (HttpMethod, &'static str, Operation) {
    let success = match response {
        Some(schema) if status != "204" => json(status, description, schema),
        _ => empty(status, description),
    };
    finish(
        method,
        path,
        id,
        summary,
        params,
        body,
        vec![success],
        secure,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "one operation is one row of the route table"
)]
fn query<T: IntoParams>(
    method: HttpMethod,
    path: &'static str,
    id: &'static str,
    summary: &'static str,
    mut params: Vec<Parameter>,
    body: Option<&'static str>,
    response: Option<&'static str>,
    status: &'static str,
    description: &'static str,
) -> (HttpMethod, &'static str, Operation) {
    params.extend(T::into_params(|| Some(ParameterIn::Query)));
    call(
        method,
        path,
        id,
        summary,
        params,
        body,
        response,
        status,
        description,
        true,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "one operation is one row of the route table"
)]
fn finish(
    method: HttpMethod,
    path: &'static str,
    id: &'static str,
    summary: &'static str,
    params: Vec<Parameter>,
    body: Option<&'static str>,
    mut responses: Vec<Resp>,
    secure: bool,
) -> (HttpMethod, &'static str, Operation) {
    if secure {
        responses.push(problem("401", "Missing or unknown token"));
    }
    let mut builder = OperationBuilder::new()
        .operation_id(Some(id))
        .summary(Some(summary))
        .tags(Some(["platform"]))
        .responses(response_set(&responses));
    if !params.is_empty() {
        builder = builder.parameters(Some(params));
    }
    if let Some(name) = body {
        builder = builder.request_body(Some(json_body(name)));
    }
    if secure {
        builder = builder.security(SecurityRequirement::new::<&str, [&str; 0], &str>(
            "bearer",
            [],
        ));
    }
    (method, path, builder.build())
}

fn response_set(responses: &[Resp]) -> utoipa::openapi::Responses {
    let mut set = ResponsesBuilder::new();
    for response in responses {
        set = set.response(response.status, response_body(response));
    }
    set.build()
}

fn response_body(response: &Resp) -> Response {
    let mut builder = ResponseBuilder::new().description(response.description);
    if let Some(name) = response.schema {
        builder = builder.content(
            response.content_type,
            ContentBuilder::new().schema(Some(schema_ref(name))).build(),
        );
    }
    builder.build()
}

fn json_body(name: &'static str) -> utoipa::openapi::request_body::RequestBody {
    RequestBodyBuilder::new()
        .required(Some(Required::True))
        .content(
            "application/json",
            ContentBuilder::new().schema(Some(schema_ref(name))).build(),
        )
        .build()
}

fn schema_ref(name: &str) -> RefOr<Schema> {
    RefOr::Ref(Ref::from_schema_name(name))
}

fn path_uuid(name: &'static str, description: &'static str) -> Parameter {
    path_param(name, description, uuid_schema())
}

fn path_int(name: &'static str, description: &'static str) -> Parameter {
    path_param(
        name,
        description,
        RefOr::T(Schema::Object(
            ObjectBuilder::new().schema_type(Type::Integer).build(),
        )),
    )
}

fn path_string(name: &'static str, description: &'static str) -> Parameter {
    path_param(name, description, string_schema())
}

fn path_param(name: &'static str, description: &'static str, schema: RefOr<Schema>) -> Parameter {
    ParameterBuilder::new()
        .name(name)
        .description(Some(description))
        .parameter_in(ParameterIn::Path)
        .required(Required::True)
        .schema(Some(schema))
        .build()
}

fn string_schema() -> RefOr<Schema> {
    RefOr::T(Schema::Object(
        ObjectBuilder::new().schema_type(Type::String).build(),
    ))
}

fn uuid_schema() -> RefOr<Schema> {
    RefOr::T(Schema::Object(
        ObjectBuilder::new()
            .schema_type(Type::String)
            .format(Some(SchemaFormat::KnownFormat(
                utoipa::openapi::schema::KnownFormat::Uuid,
            )))
            .build(),
    ))
}

fn binary_schema() -> RefOr<Schema> {
    RefOr::T(Schema::Object(
        ObjectBuilder::new()
            .schema_type(Type::String)
            .format(Some(SchemaFormat::KnownFormat(
                utoipa::openapi::schema::KnownFormat::Binary,
            )))
            .build(),
    ))
}

fn header_param(name: &'static str, description: &'static str) -> Parameter {
    ParameterBuilder::new()
        .name(name)
        .description(Some(description))
        .parameter_in(ParameterIn::Header)
        .required(Required::False)
        .schema(Some(string_schema()))
        .build()
}

fn import_upload() -> (HttpMethod, &'static str, Operation) {
    let body = RequestBodyBuilder::new()
        .required(Some(Required::True))
        .description(Some(
            "Raw spreadsheet bytes. The file name is the X-File-Name header.",
        ))
        .content(
            "application/octet-stream",
            ContentBuilder::new().schema(Some(binary_schema())).build(),
        )
        .build();
    let operation = OperationBuilder::new()
        .operation_id(Some("import_upload"))
        .summary(Some("Upload a spreadsheet import"))
        .tags(Some(["platform"]))
        .parameters(Some(vec![header_param(
            "X-File-Name",
            "Original file name, percent-encoded. Used to choose csv, xls or xlsx.",
        )]))
        .request_body(Some(body))
        .responses(response_set(&[
            json("201", "Import created", "ImportCreated"),
            problem("401", "Missing or unknown token"),
        ]))
        .security(SecurityRequirement::new::<&str, [&str; 0], &str>(
            "bearer",
            [],
        ))
        .build();
    (HttpMethod::Post, "/v1/imports", operation)
}

fn import_failed() -> (HttpMethod, &'static str, Operation) {
    let operation = OperationBuilder::new()
        .operation_id(Some("import_failed_xlsx"))
        .summary(Some("Download failing rows"))
        .tags(Some(["platform"]))
        .parameters(Some(vec![path_uuid("id", "Import id")]))
        .responses({
            let mut set = ResponsesBuilder::new();
            set = set.response(
                "200",
                ResponseBuilder::new()
                    .description("Spreadsheet of rows that failed the preview")
                    .content(
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                        ContentBuilder::new().schema(Some(binary_schema())).build(),
                    )
                    .build(),
            );
            set = set.response(
                "401",
                response_body(&problem("401", "Missing or unknown token")),
            );
            set.build()
        })
        .security(SecurityRequirement::new::<&str, [&str; 0], &str>(
            "bearer",
            [],
        ))
        .build();
    (HttpMethod::Get, "/v1/imports/{id}/failed.xlsx", operation)
}

fn export_post() -> (HttpMethod, &'static str, Operation) {
    let operation = OperationBuilder::new()
        .operation_id(Some("export_xlsx"))
        .summary(Some(
            "Export the current view as an xlsx file. Large exports run as a job.",
        ))
        .tags(Some(["platform"]))
        .request_body(Some(json_body("ExportBody")))
        .responses({
            let mut set = ResponsesBuilder::new();
            set = set.response(
                "200",
                ResponseBuilder::new()
                    .description("Inline workbook when the row count is small")
                    .content(
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                        ContentBuilder::new().schema(Some(binary_schema())).build(),
                    )
                    .build(),
            );
            set = set.response(
                "202",
                response_body(&json("202", "Export job", "ExportJob")),
            );
            set = set.response(
                "401",
                response_body(&problem("401", "Missing or unknown token")),
            );
            set.build()
        })
        .security(SecurityRequirement::new::<&str, [&str; 0], &str>(
            "bearer",
            [],
        ))
        .build();
    (HttpMethod::Post, "/v1/export", operation)
}

fn export_get() -> (HttpMethod, &'static str, Operation) {
    let operation = OperationBuilder::new()
        .operation_id(Some("download_export"))
        .summary(Some("Download a finished xlsx export"))
        .tags(Some(["platform"]))
        .parameters(Some(vec![path_uuid("id", "Export job id")]))
        .responses({
            let mut set = ResponsesBuilder::new();
            set = set.response(
                "200",
                ResponseBuilder::new()
                    .description("Finished workbook")
                    .content(
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                        ContentBuilder::new().schema(Some(binary_schema())).build(),
                    )
                    .build(),
            );
            set = set.response(
                "401",
                response_body(&problem("401", "Missing or unknown token")),
            );
            set.build()
        })
        .security(SecurityRequirement::new::<&str, [&str; 0], &str>(
            "bearer",
            [],
        ))
        .build();
    (HttpMethod::Get, "/v1/exports/{id}", operation)
}

fn accept_proposal() -> (HttpMethod, &'static str, Operation) {
    finish(
        HttpMethod::Post,
        "/v1/proposals/{id}/accept",
        "accept_proposal",
        "Accept a proposal in full or by row",
        vec![path_uuid("id", "Proposal id")],
        Some("AcceptBody"),
        vec![
            json("200", "Accepted and failed rows", "AcceptResult"),
            json("202", "Large accept running as a job", "RunningJob"),
        ],
        true,
    )
}

fn mcp() -> (HttpMethod, &'static str, Operation) {
    call(
        HttpMethod::Post,
        "/mcp",
        "mcp",
        "MCP endpoint over the operation registry",
        vec![],
        Some("FreeObject"),
        Some("JsonValue"),
        "200",
        "JSON-RPC response",
        true,
    )
}

fn agent() -> (HttpMethod, &'static str, Operation) {
    let operation = OperationBuilder::new()
        .operation_id(Some("agent"))
        .summary(Some("Proxy a chat completion when a model key is set"))
        .tags(Some(["platform"]))
        .request_body(Some(json_body("AgentBody")))
        .responses({
            let mut set = ResponsesBuilder::new();
            set = set.response(
                "200",
                ResponseBuilder::new()
                    .description("JSON reply, or a text event stream when stream is true")
                    .content(
                        "application/json",
                        ContentBuilder::new()
                            .schema(Some(schema_ref("AgentReply")))
                            .build(),
                    )
                    .content(
                        "text/event-stream",
                        ContentBuilder::new().schema(Some(string_schema())).build(),
                    )
                    .build(),
            );
            set = set.response(
                "401",
                response_body(&problem("401", "Missing or unknown token")),
            );
            set.build()
        })
        .security(SecurityRequirement::new::<&str, [&str; 0], &str>(
            "bearer",
            [],
        ))
        .build();
    (HttpMethod::Post, "/v1/agent", operation)
}

fn stream() -> (HttpMethod, &'static str, Operation) {
    let operation = OperationBuilder::new()
        .operation_id(Some("change_stream"))
        .summary(Some("Server-sent events for committed changes"))
        .tags(Some(["platform"]))
        .responses({
            let mut set = ResponsesBuilder::new();
            set = set.response(
                "200",
                ResponseBuilder::new()
                    .description("Event stream. Each event's data is a JSON object.")
                    .content(
                        "text/event-stream",
                        ContentBuilder::new().schema(Some(string_schema())).build(),
                    )
                    .build(),
            );
            set = set.response(
                "401",
                response_body(&problem("401", "Missing or unknown token")),
            );
            set.build()
        })
        .security(SecurityRequirement::new::<&str, [&str; 0], &str>(
            "bearer",
            [],
        ))
        .build();
    (HttpMethod::Get, "/v1/stream", operation)
}
