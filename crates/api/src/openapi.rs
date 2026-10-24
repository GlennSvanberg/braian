use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Braian",
        version = "0.0.0",
        description = "Write and read definitions and objects."
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "meta", description = "Process and caller"),
        (name = "definitions", description = "Versioned model. One request is one revision."),
        (name = "objects", description = "Object values. Addresses accept a key or an id; responses use keys.")
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
        crate::objects::get_object,
        crate::objects::patch_object,
        crate::objects::delete_object,
        crate::objects::object_changes,
    ),
    components(schemas(
        crate::problem::Problem,
        crate::problem::ConflictBody,
        crate::objects::CreateObject,
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
