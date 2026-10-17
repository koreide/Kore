use std::time::Duration;

pub const PORT_FORWARD_BUFFER_SIZE: usize = 4096;
pub const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);
pub const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);
pub const MAX_KUBECTL_CONNECT_RETRIES: u32 = 30;
pub const KUBECTL_RETRY_INTERVAL: Duration = Duration::from_millis(100);
pub const DEFAULT_LOG_TAIL_LINES: i64 = 200;

// Resource listing limits.
// `list_resources` pages through the API with `limit` + `continue` tokens
// (A4). Each page asks for `LIST_PAGE_SIZE` objects and gets its own timeout and
// retries (a request on a stale pooled connection can otherwise hang). The
// whole list stops at `MAX_LIST_TOTAL` objects as a memory safety cap; the
// result is then marked `truncated` (see `ResourceList`) and a warning logged.
pub const LIST_PAGE_SIZE: u32 = 500;
pub const MAX_LIST_TOTAL: usize = 50_000;
pub const LIST_PAGE_TIMEOUT: Duration = Duration::from_secs(8);
pub const LIST_PAGE_ATTEMPTS: u32 = 3;
pub const MAX_SEARCH_RESULTS: usize = 200;
pub const MAX_DASHBOARD_RESOURCES: u32 = 1000;

// AI response streaming limit
pub const MAX_AI_RESPONSE_BYTES: usize = 2 * 1024 * 1024; // 2MB

// YAML input size limit
pub const MAX_YAML_INPUT_BYTES: usize = 5 * 1024 * 1024; // 5MB

// Multi-cluster: per-context timeout so one unreachable context can't stall the call
pub const MULTI_CLUSTER_CONTEXT_TIMEOUT: Duration = Duration::from_secs(5);
