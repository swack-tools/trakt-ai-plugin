use serde_json::{Value, json};
use worker::Response;
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub retry_after: Option<u64>,
}
impl ApiError {
    pub fn new(status: u16, code: &'static str) -> Self {
        Self {
            status,
            code,
            retry_after: None,
        }
    }
    pub fn value(&self) -> Value {
        json!({"error":self.code,"error_description":describe(self.code),"retry_after":self.retry_after})
    }
    pub fn response(&self) -> worker::Result<Response> {
        let mut r = Response::from_json(&self.value())?.with_status(self.status);
        if let Some(n) = self.retry_after {
            r.headers_mut().set("Retry-After", &n.to_string())?;
        }
        Ok(r)
    }
}
impl From<worker::Error> for ApiError {
    fn from(_: worker::Error) -> Self {
        Self::new(500, "storage_or_runtime_error")
    }
}
impl From<serde_json::Error> for ApiError {
    fn from(_: serde_json::Error) -> Self {
        Self::new(400, "invalid_json")
    }
}
impl From<worker::kv::KvError> for ApiError {
    fn from(_: worker::kv::KvError) -> Self {
        Self::new(500, "cache_error")
    }
}
pub type Result<T> = std::result::Result<T, ApiError>;

/// Human-readable explanation and next step for each error code. The code stays the
/// stable contract; descriptions never echo request or upstream data.
pub fn describe(code: &str) -> &'static str {
    match code {
        "access_denied" => {
            "The Trakt account owner denied this connection. Start a new connection only if the user asks."
        }
        "already_used" => {
            "This device code was already used. Request a new code if the user still wants to connect."
        }
        "authorization_pending" => {
            "The user has not approved the Trakt code yet. Wait for retry_after or the login interval, then confirm again."
        }
        "cache_error" => {
            "The service could not read or write its token cache. Retry a read later; inspect state before repeating a write."
        }
        "confirmation_required" => {
            "This change needs confirmed set to true, and only after the user requested this exact change."
        }
        "device_code_required" => {
            "Provide the device_code returned by trakt_request_login, not the user code."
        }
        "duplicate_parameter" => {
            "A query parameter appears more than once. Send each parameter once."
        }
        "expired_token" => {
            "The device code expired. Request a new code if the user still wants to connect."
        }
        "flow_already_completed" => {
            "This authorization already finished. Return to the client to continue."
        }
        "invalid_action" => "The browser step action must be start or poll.",
        "invalid_api_parameters" => {
            "The parameters do not match the operation schema. Read trakt_get_operation and correct path_params, query_params, or body once."
        }
        "invalid_authorization_request" => {
            "The authorization request needs response_type=code and a redirect_uri registered for this client."
        }
        "invalid_client" => {
            "The client ID or secret is unknown or does not match. Register the client again through the MCP client's connection flow."
        }
        "invalid_client_metadata" => {
            "Client registration metadata is invalid. Use token_endpoint_auth_method none or client_secret_post and a client_name of at most 100 characters."
        }
        "invalid_code_challenge" => {
            "PKCE needs code_challenge_method S256 and a 43-character base64url code_challenge."
        }
        "invalid_detail" => "detail must be compact or full.",
        "invalid_device_code" => {
            "The device code is unknown. Request a new code if the user still wants to connect."
        }
        "invalid_flow" => {
            "The authorization page expired or its ticket is invalid. Restart the connection from the MCP client."
        }
        "invalid_genres" => "genres must be comma-separated lowercase genre slugs.",
        "invalid_grant" => {
            "The authorization code or refresh token is invalid, expired, or already used. Reconnect through the MCP client."
        }
        "invalid_json" => "The request body is not valid JSON.",
        "invalid_media_type" => {
            "media_type must be movie, show, movies, shows, or all where the tool allows it."
        }
        "invalid_mode" => "mode must be all or recent.",
        "invalid_oauth_request" => {
            "The authorization service rejected this request. Restart the connection from the MCP client."
        }
        "invalid_operation_schema" => {
            "The bundled catalog entry for this operation is invalid. Choose another operation or report the problem."
        }
        "invalid_pagination" => {
            "page must be a positive integer and limit must be between 1 and 100."
        }
        "invalid_parameters" => {
            "The arguments do not match this tool's input schema. Correct them once using the published schema."
        }
        "invalid_public_base_url" => {
            "The deployment's PUBLIC_BASE_URL setting is invalid. The operator must fix the configuration."
        }
        "invalid_query" => "query must be non-empty and at most 500 characters.",
        "invalid_redirect_uri" => {
            "Redirect URIs must be HTTPS, or HTTP on a loopback address, without credentials or fragments."
        }
        "invalid_scope" => {
            "Request trakt:read, optionally with trakt:write. A refresh cannot add scopes the connection was not granted."
        }
        "invalid_target" => {
            "The resource parameter must name this server's MCP endpoint, such as the /mcp URL from its protected resource metadata."
        }
        "invalid_token" => {
            "The access token is missing, expired, or revoked. Reconnect through the MCP client's sign-in flow."
        }
        "invalid_trakt_pagination" => {
            "Trakt returned inconsistent pagination, so coverage is unknown. Report partial results and retry later."
        }
        "invalid_trakt_response" => {
            "Trakt returned a response this service could not read. Retry a read later; inspect state before repeating a write."
        }
        "invalid_trakt_token" => {
            "Trakt rejected the stored authorization. Reconnect the Trakt account."
        }
        "invalid_upstream_configuration" => {
            "The deployment's Trakt configuration is invalid. The operator must fix the configuration."
        }
        "invalid_years" => {
            "years must be a four-digit year or an inclusive YYYY-YYYY range between 1800 and 2200."
        }
        "legacy_manifest_not_configured" => {
            "This deployment does not publish the legacy plugin manifest."
        }
        "list_not_owned" => {
            "The connected account does not own this list, so it cannot be changed."
        }
        "list_ownership_unverified" => {
            "List ownership could not be verified, so no change was made. Read the list and try once more."
        }
        "login_already_pending" => {
            "A Trakt login is already waiting for approval. Reuse its code until it expires."
        }
        "login_not_started" => "Start the Trakt login before polling for its result.",
        "method_not_allowed" => "This endpoint does not accept that HTTP method.",
        "missing_id" => "An internal identifier was missing. Retry the request.",
        "missing_public_base_url" => {
            "The deployment has no PUBLIC_BASE_URL setting. The operator must fix the configuration."
        }
        "missing_session" => {
            "The connection record was not found. Reconnect through the MCP client."
        }
        "missing_sse_session" => {
            "The SSE session_id is missing or unknown. Open a new /sse stream."
        }
        "not_found" => "No endpoint exists at this path.",
        "operation_requires_read_tool" => "This operation is a read. Use trakt_api_read.",
        "operation_requires_write_tool" => {
            "This operation changes data. Use trakt_api_write after the user requests the change."
        }
        "operation_unavailable" => {
            "This catalog operation is restricted or unsupported on this service."
        }
        "origin_not_allowed" => "Requests from this browser origin are not allowed.",
        "pkce_required" => "Public clients must send a PKCE code_challenge.",
        "query_required" => "Provide a non-empty query.",
        "rate_limited" => "Too many requests. Wait for retry_after before trying again.",
        "reauthorization_required" => {
            "This connection must be authorized again through the MCP client."
        }
        "request_too_large" => "The request body exceeds 64 KiB.",
        "slow_down" => {
            "Trakt asked for slower polling. Wait for retry_after before confirming again."
        }
        "slow_sse_consumer" => {
            "The SSE client is not reading events fast enough. Reconnect the stream."
        }
        "sse_response_too_large" => {
            "The response is too large for one SSE event. Request a smaller page."
        }
        "sse_session_expired" => "The SSE session expired. Open a new /sse stream.",
        "storage_or_runtime_error" => {
            "The service hit a temporary storage or runtime problem. Retry a read later; inspect state before repeating a write."
        }
        "stream_error" => "The event stream failed. Reconnect the stream.",
        "tool_result_too_large" => {
            "The result exceeds the 60,000 characters AI clients accept. Request a smaller limit, use detail=compact, or narrow the filters, then continue with the next page."
        }
        "too_many_streams" => {
            "This connection has too many open streams. Close one before opening another."
        }
        "trakt_conflict" => {
            "Trakt reported a conflict with existing data. Read the current state before deciding on another change."
        }
        "trakt_forbidden" => {
            "Trakt refused this request for the connected account. Do not retry it."
        }
        "trakt_login_required" => {
            "The Trakt account is not connected. Reconnect only if the user asks."
        }
        "trakt_not_found" => "Trakt has no item matching these identifiers.",
        "trakt_rate_limited" => {
            "Trakt rate limited this account. Wait for retry_after before one retry of a read."
        }
        "trakt_response_too_large" => {
            "Trakt's response exceeded the size limit. Request a smaller page or the compact detail."
        }
        "trakt_subscription_required" => {
            "This operation needs a Trakt subscription on the connected account."
        }
        "trakt_unavailable" => {
            "Trakt is temporarily unavailable. Retry a read later; inspect state before repeating a write."
        }
        "trakt_upstream_error" => {
            "Trakt returned an unexpected error. Retry a read later; inspect state before repeating a write."
        }
        "trakt_validation_failed" => {
            "Trakt rejected the submitted values. Correct them using the operation schema."
        }
        "unauthorized" => "Send a bearer access token from the MCP client's sign-in flow.",
        "unknown_operation" => {
            "No catalog operation has this operation_id. Find one with trakt_list_operations."
        }
        "unknown_tool" => "No tool has this name. Use a name from tools/list.",
        "unsupported_grant_type" => "grant_type must be authorization_code or refresh_token.",
        "unsupported_protocol_version" => {
            "The MCP-Protocol-Version header names a revision this server does not support. Use 2025-06-18, 2025-03-26, or 2024-11-05."
        }
        "unsupported_media_type" => {
            "Send the request as application/json or application/x-www-form-urlencoded."
        }
        "write_authorization_required" => {
            "This connection is read-only. Reauthorize through the MCP client with trakt:read trakt:write."
        }
        _ => "The request failed. Use the error code to decide the next step.",
    }
}
