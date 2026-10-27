# Phase 3: definitions, objects, validation, merge, conflict, warnings, delete.
# `file,` paths are relative to the repository root (`hurl --file-root`).

GET {{host}}/healthz
HTTP 204

GET {{host}}/v1/me
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.handle" == "ada"
jsonpath "$.kind" == "human"

PUT {{host}}/v1/definitions
Authorization: Bearer {{token}}
Content-Type: application/json
file,testdata/rules/definitions.json;
HTTP 200
[Asserts]
jsonpath "$.model_revision" == 1
header "ETag" == "\"1\""

POST {{host}}/v1/attributes
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "key": "extra",
  "labels": { "sv": "Extra" },
  "language_dependent": false,
  "multiple": false,
  "kind": { "type": "text", "max_length": 16 },
  "index": { "searchable": false, "filterable": false, "sortable": false }
}
HTTP 200
[Asserts]
jsonpath "$.key" == "extra"

GET {{host}}/v1/definitions
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.model_revision" == 2

POST {{host}}/v1/objects
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "type": "item",
  "groups": { "catalog": "root", "channel": "flat" },
  "values": { "sku": "SKU-1", "name": { "sv": "Stol" }, "active": true }
}
HTTP 200
[Captures]
object: jsonpath "$.id"
[Asserts]
jsonpath "$.type" == "item"
jsonpath "$.version" == 1
jsonpath "$.values.sku" == "SKU-1"
jsonpath "$.groups.catalog" == "root"
header "ETag" == "\"1\""

GET {{host}}/v1/objects/{{object}}
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.values.name.sv" == "Stol"
header "ETag" == "\"1\""

PATCH {{host}}/v1/objects/{{object}}
Authorization: Bearer {{token}}
Content-Type: application/json
{ "values": { "active": false } }
HTTP 428
[Asserts]
jsonpath "$.code" == "if_match_required"

POST {{host}}/v1/objects
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "type": "item",
  "groups": { "catalog": "root" },
  "values": {
    "sku": "SKU-BAD",
    "name": { "sv": "12345678901" },
    "color": "green"
  }
}
HTTP 422
[Asserts]
jsonpath "$.code" == "invalid_object"
jsonpath "$.errors[*].code" contains "text_too_long"
jsonpath "$.errors[*].code" contains "value_not_in_list"
jsonpath "$.errors[*].language" contains "sv"

POST {{host}}/v1/objects
Authorization: Bearer {{token}}
Content-Type: application/json
{ "type": "item", "values": { "sku": "SKU-MERGE", "name": { "sv": "Ett" } } }
HTTP 200
[Captures]
merge: jsonpath "$.id"

PATCH {{host}}/v1/objects/{{merge}}
Authorization: Bearer {{token}}
If-Match: "1"
Content-Type: application/json
{ "values": { "name": { "sv": "Två" } } }
HTTP 200

PATCH {{host}}/v1/objects/{{merge}}
Authorization: Bearer {{token}}
If-Match: "1"
Content-Type: application/json
{ "values": { "name": { "en-GB": "Two" } } }
HTTP 200
[Asserts]
jsonpath "$.version" == 3
jsonpath "$.values.name.sv" == "Två"
jsonpath "$.values.name['en-GB']" == "Two"

POST {{host}}/v1/objects
Authorization: Bearer {{token}}
Content-Type: application/json
{ "type": "item", "values": { "sku": "SKU-CONFLICT", "name": { "sv": "Ett" } } }
HTTP 200
[Captures]
clash: jsonpath "$.id"

PATCH {{host}}/v1/objects/{{clash}}
Authorization: Bearer {{token}}
If-Match: "1"
Content-Type: application/json
{ "values": { "name": { "sv": "A" } } }
HTTP 200

PATCH {{host}}/v1/objects/{{clash}}
Authorization: Bearer {{token}}
If-Match: "1"
Content-Type: application/json
{ "values": { "name": { "sv": "B" } } }
HTTP 409
[Asserts]
jsonpath "$.code" == "conflict"
jsonpath "$.conflicts[0].attribute" == "name"
jsonpath "$.conflicts[0].language" == "sv"
jsonpath "$.conflicts[0].actor" == "ada"

POST {{host}}/v1/objects
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "type": "item",
  "groups": { "catalog": "root" },
  "values": { "sku": "SKU-WARN", "name": { "sv": "Stol" }, "active": true }
}
HTTP 200
[Captures]
warn: jsonpath "$.id"

PUT {{host}}/v1/attributes/name
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "id": "018f0000-0000-7000-8000-000000000101",
  "key": "name",
  "labels": { "sv": "Namn", "en-GB": "Name" },
  "language_dependent": true,
  "multiple": false,
  "kind": { "type": "text", "max_length": 3 },
  "index": { "searchable": true, "filterable": false, "sortable": false }
}
HTTP 200

GET {{host}}/v1/objects/{{warn}}
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.warnings[*].code" contains "text_too_long"

PATCH {{host}}/v1/objects/{{warn}}
Authorization: Bearer {{token}}
If-Match: "1"
Content-Type: application/json
{ "values": { "color": "red" } }
HTTP 200
[Asserts]
jsonpath "$.values.color" == "red"
jsonpath "$.warnings[*].code" contains "text_too_long"

PATCH {{host}}/v1/objects/{{warn}}
Authorization: Bearer {{token}}
If-Match: "2"
Content-Type: application/json
{ "values": { "name": { "sv": "1234" } } }
HTTP 422
[Asserts]
jsonpath "$.errors[*].code" contains "text_too_long"

PUT {{host}}/v1/types/item
Authorization: Bearer {{token}}
Content-Type: application/json
{
  "id": "018f0000-0000-7000-8000-000000000200",
  "key": "item",
  "labels": { "sv": "Objekt" },
  "key_attribute": "018f0000-0000-7000-8000-000000000100",
  "attributes": [
    "018f0000-0000-7000-8000-000000000100",
    "018f0000-0000-7000-8000-000000000101",
    "018f0000-0000-7000-8000-000000000102",
    "018f0000-0000-7000-8000-000000000103",
    "018f0000-0000-7000-8000-000000000106",
    "018f0000-0000-7000-8000-000000000107",
    "018f0000-0000-7000-8000-000000000108",
    "018f0000-0000-7000-8000-000000000109"
  ]
}
HTTP 200

GET {{host}}/v1/objects/{{warn}}
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
body not contains "\"active\""

GET {{host}}/v1/objects/{{warn}}?include=inapplicable
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.values.active" == true

POST {{host}}/v1/objects
Authorization: Bearer {{agent}}
Content-Type: application/json
{ "type": "item", "values": { "sku": "SKU-AGENT" } }
HTTP 202
[Captures]
proposal: jsonpath "$.proposal_id"
[Asserts]
jsonpath "$.needs_approval" == true
jsonpath "$.source" == "agent"

GET {{host}}/v1/objects?q=SKU-AGENT
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$.objects" count == 0

POST {{host}}/v1/proposals/{{proposal}}/accept
Authorization: Bearer {{agent}}
Content-Type: application/json
{}
HTTP 403
[Asserts]
jsonpath "$.code" == "agent_cannot_accept"

POST {{host}}/v1/proposals/{{proposal}}/accept
Authorization: Bearer {{token}}
Content-Type: application/json
{}
HTTP 200
[Asserts]
jsonpath "$.failed" count == 0

GET {{host}}/v1/objects?q=SKU-AGENT
Authorization: Bearer {{token}}
HTTP 200
[Captures]
agent_object: jsonpath "$.objects[0].id"
[Asserts]
jsonpath "$.objects[0].key" == "SKU-AGENT"

GET {{host}}/v1/objects/{{agent_object}}/changes
Authorization: Bearer {{token}}
HTTP 200
[Asserts]
jsonpath "$[0].op" == "create"
jsonpath "$[0].actor" == "ada"

DELETE {{host}}/v1/objects/{{agent_object}}
Authorization: Bearer {{agent}}
If-Match: "1"
HTTP 403
[Asserts]
jsonpath "$.code" == "agent_cannot_delete"

DELETE {{host}}/v1/objects/{{agent_object}}
Authorization: Bearer {{token}}
If-Match: "1"
HTTP 200

GET {{host}}/v1/objects/{{agent_object}}
Authorization: Bearer {{token}}
HTTP 410
[Asserts]
jsonpath "$.code" == "deleted"
