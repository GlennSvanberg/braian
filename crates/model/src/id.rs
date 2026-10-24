use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifiers are UUIDv7. The newtypes keep attribute, type, structure, group
/// and class ids from being mixed up. Languages are not ids: they are BCP 47 tags.
macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
        #[cfg_attr(feature = "openapi", schema(value_type = String, format = Uuid))]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::fmt::Display::fmt(&self.0, f)
            }
        }

        impl From<Uuid> for $name {
            fn from(id: Uuid) -> Self {
                Self(id)
            }
        }
    };
}

id_type!(AttributeId, "Id of an attribute in the global pool.");
id_type!(TypeId, "Id of an object type.");
id_type!(StructureId, "Id of a structure.");
id_type!(GroupId, "Id of a group inside one structure.");
id_type!(
    ClassId,
    "Id of a class. A class belongs to exactly one group."
);
id_type!(
    UnitFamilyId,
    "Id of a family of units that share a base unit."
);
id_type!(ObjectId, "Id of a stored object.");
