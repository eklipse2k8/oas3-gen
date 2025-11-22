pub(crate) mod doc_attrs {
  pub(crate) const HIDDEN: &str = "#[doc(hidden)]";
}

pub(crate) const REQUEST_SUFFIX: &str = "Request";
pub(crate) const REQUEST_BODY_SUFFIX: &str = "RequestBody";
pub(crate) const RESPONSE_SUFFIX: &str = "Response";
pub(crate) const BODY_FIELD_NAME: &str = "body";
pub(crate) const SUCCESS_RESPONSE_PREFIX: char = '2';

pub(crate) const REQUEST_PARAMS_SUFFIX: &str = "Params";
pub(crate) const RESPONSE_ENUM_SUFFIX: &str = "Enum";
pub(crate) const DISCRIMINATED_BASE_SUFFIX: &str = "Base";
pub(crate) const MERGED_SCHEMA_CACHE_SUFFIX: &str = "_merged";
pub(crate) const RESPONSE_PREFIX: &str = "Response";

pub(crate) const DEFAULT_RESPONSE_VARIANT: &str = "Unknown";
pub(crate) const DEFAULT_RESPONSE_DESCRIPTION: &str = "Unknown response";

pub(crate) const STATUS_OK: &str = "Ok";
pub(crate) const STATUS_CREATED: &str = "Created";
pub(crate) const STATUS_ACCEPTED: &str = "Accepted";
pub(crate) const STATUS_NO_CONTENT: &str = "NoContent";
pub(crate) const STATUS_MOVED_PERMANENTLY: &str = "MovedPermanently";
pub(crate) const STATUS_FOUND: &str = "Found";
pub(crate) const STATUS_NOT_MODIFIED: &str = "NotModified";
pub(crate) const STATUS_BAD_REQUEST: &str = "BadRequest";
pub(crate) const STATUS_UNAUTHORIZED: &str = "Unauthorized";
pub(crate) const STATUS_FORBIDDEN: &str = "Forbidden";
pub(crate) const STATUS_NOT_FOUND: &str = "NotFound";
pub(crate) const STATUS_METHOD_NOT_ALLOWED: &str = "MethodNotAllowed";
pub(crate) const STATUS_NOT_ACCEPTABLE: &str = "NotAcceptable";
pub(crate) const STATUS_REQUEST_TIMEOUT: &str = "RequestTimeout";
pub(crate) const STATUS_CONFLICT: &str = "Conflict";
pub(crate) const STATUS_GONE: &str = "Gone";
pub(crate) const STATUS_UNPROCESSABLE_ENTITY: &str = "UnprocessableEntity";
pub(crate) const STATUS_TOO_MANY_REQUESTS: &str = "TooManyRequests";
pub(crate) const STATUS_INTERNAL_SERVER_ERROR: &str = "InternalServerError";
pub(crate) const STATUS_NOT_IMPLEMENTED: &str = "NotImplemented";
pub(crate) const STATUS_BAD_GATEWAY: &str = "BadGateway";
pub(crate) const STATUS_SERVICE_UNAVAILABLE: &str = "ServiceUnavailable";
pub(crate) const STATUS_GATEWAY_TIMEOUT: &str = "GatewayTimeout";

pub(crate) const STATUS_INFORMATIONAL: &str = "Informational";
pub(crate) const STATUS_SUCCESS: &str = "Success";
pub(crate) const STATUS_REDIRECTION: &str = "Redirection";
pub(crate) const STATUS_CLIENT_ERROR: &str = "ClientError";
pub(crate) const STATUS_SERVER_ERROR: &str = "ServerError";
pub(crate) const STATUS_PREFIX: &str = "Status";
