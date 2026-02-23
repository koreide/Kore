use std::time::Duration;

pub const PORT_FORWARD_BUFFER_SIZE: usize = 4096;
pub const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);
pub const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);
pub const MAX_KUBECTL_CONNECT_RETRIES: u32 = 50;
pub const KUBECTL_RETRY_INTERVAL: Duration = Duration::from_millis(100);
pub const DEFAULT_LOG_TAIL_LINES: i64 = 200;
