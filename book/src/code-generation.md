# Code Generation Reference

<a id="shared-generation-options"></a>
<a id="code-generation"></a>
<a id="table-of-contents"></a>

Choose a reference topic below for the generation flags, schema rules, or HTTP behavior
you need. For a worked example, start with [Client Generation](./client-generation.md) ,
[Server Generation](./server-generation.md) , or [Builder Pattern](./builders.md) .

## Generation Options

- <a id="generation-modes"></a><a id="types"></a><a id="client"></a><a id="client-mod"></a><a id="server-mod"></a> [Generation Modes](./generation-options.md#generation-modes)
- <a id="workspace-crate-output"></a><a id="crate-name"></a><a id="generated-manifest"></a><a id="dependencies"></a><a id="using-the-crate"></a> [Workspace Crate Output](./generation-options.md#workspace-crate-output)
- <a id="operation-filtering"></a><a id="--only"></a><a id="--exclude"></a><a id="schema-dependency-resolution"></a> [Operation Filtering](./generation-options.md#operation-filtering)
- <a id="schema-filtering"></a><a id="default-reachability-filtering"></a><a id="with---all-schemas"></a><a id="combining-with-operation-filtering"></a> [Schema Filtering](./generation-options.md#schema-filtering)
- <a id="api-name-override"></a> [API Name Override](./generation-options.md#api-name-override)
- <a id="function-name-overrides"></a> [Function Name Overrides](./generation-options.md#function-name-overrides)
- <a id="visibility"></a><a id="example---visibility-public"></a><a id="example---visibility-crate"></a><a id="example---visibility-file"></a> [Visibility](./generation-options.md#visibility)
- <a id="header-emission"></a><a id="default-operation-referenced-headers-only"></a><a id="with---all-headers"></a> [Header Emission](./generation-options.md#header-emission)
- <a id="builder-generation"></a><a id="default-builders-disabled"></a><a id="with---enable-builders"></a> [Builder Generation](./generation-options.md#builder-generation)
- <a id="documentation-formatting"></a><a id="default-formatting-disabled"></a><a id="with---doc-format"></a> [Documentation Formatting](./generation-options.md#documentation-formatting)
- <a id="known-limitations"></a> [Known Limitations](./generation-options.md#known-limitations)
- <a id="flag-summary"></a> [Flag Summary](./generation-options.md#flag-summary)

## Schemas and Types

- <a id="generated-names"></a> [Generated Names](./schema-types.md#generated-names)
- <a id="ordering-and-collections"></a> [Ordering and Collections](./schema-types.md#ordering-and-collections)
- <a id="field-defaults"></a> [Field Defaults](./schema-types.md#field-defaults)
- <a id="constant-properties"></a> [Constant Properties](./schema-types.md#constant-properties)
- <a id="field-validation"></a><a id="multiples"></a><a id="when-validation-runs"></a> [Field Validation](./schema-types.md#field-validation)
- <a id="odata-support"></a><a id="input-schema-2"></a><a id="default-odata-support-disabled"></a><a id="with---odata-support"></a> [OData Support](./schema-types.md#odata-support)
- <a id="type-customization"></a><a id="default-no-customization"></a><a id="with-a-date-time-adapter"></a><a id="handling-optional-and-array-fields"></a> [Type Customization](./schema-types.md#type-customization)

## Enums and Unions

- <a id="enum-mode"></a><a id="input-schema"></a><a id="example---enum-mode-merge"></a><a id="example---enum-mode-preserve"></a><a id="example---enum-mode-relaxed"></a> [Enum Mode](./enum-types.md#enum-mode)
- <a id="enum-layout"></a><a id="input-schema-1"></a><a id="example---enum-layout-spec"></a><a id="example---enum-layout-sorted"></a> [Enum Layout](./enum-types.md#enum-layout)
- <a id="numeric-backed-enums"></a><a id="floating-point-values"></a> [Numeric-Backed Enums](./enum-types.md#numeric-backed-enums)
- <a id="boolean-and-mixed-type-enums"></a> [Boolean and Mixed-Type Enums](./enum-types.md#boolean-and-mixed-type-enums)
- <a id="discriminated-unions"></a><a id="inferred-discriminators"></a><a id="nullable-arrays-of-unions"></a> [Discriminated Unions](./enum-types.md#discriminated-unions)
- <a id="constant-unions"></a> [Constant Unions](./enum-types.md#constant-unions)
- <a id="non-exhaustive-enums"></a> [Non-Exhaustive Enums](./enum-types.md#non-exhaustive-enums)
- <a id="helper-methods"></a><a id="default-helpers-enabled"></a><a id="with---no-helpers"></a> [Helper Methods](./enum-types.md#helper-methods)

## HTTP Requests and Responses

- <a id="operations-and-parameters"></a><a id="path-item-references"></a><a id="query-parameters"></a><a id="cookie-parameters"></a><a id="deprecated-operations"></a> [Operations and Parameters](./http.md#operations-and-parameters)
- <a id="security-schemes"></a><a id="requirements"></a><a id="keeping-keys-out-of-logs"></a> [Security Schemes](./http.md#security-schemes)
- <a id="responses"></a><a id="response-headers"></a><a id="response-media-types"></a> [Responses](./http.md#responses)
- <a id="multipart-form-bodies"></a><a id="choosing-the-part-for-each-property"></a><a id="encoding-objects"></a><a id="generation-warnings"></a> [Multipart Form Bodies](./http.md#multipart-form-bodies)
