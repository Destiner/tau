//! The explicit trace-context argument Tauri commands accept once later
//! stages wire IPC propagation. Concurrent sessions and controllers pass this
//! value explicitly on each call; nothing here is stored as ambient global
//! state, so context cannot leak between them.
use serde::{Deserialize, Serialize};

/// A decoded W3C `traceparent` (<https://www.w3.org/TR/trace-context/>).
/// `tracestate` is out of scope: Tau does not need vendor-specific state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceContext {
    /// 32 lowercase hex characters, not all zero.
    pub trace_id: String,
    /// 16 lowercase hex characters, not all zero.
    pub span_id: String,
    pub sampled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceContextError {
    Format,
    Version,
    TraceId,
    SpanId,
}

const TRACE_ID_LEN: usize = 32;
pub(super) const SPAN_ID_LEN: usize = 16;

impl TraceContext {
    /// Parses a `version-traceId-spanId-flags` traceparent header value.
    pub fn parse(traceparent: &str) -> Result<Self, TraceContextError> {
        let mut parts = traceparent.split('-');
        let version = parts.next().ok_or(TraceContextError::Format)?;
        let trace_id = parts.next().ok_or(TraceContextError::Format)?;
        let span_id = parts.next().ok_or(TraceContextError::Format)?;
        let flags = parts.next().ok_or(TraceContextError::Format)?;
        if parts.next().is_some() {
            return Err(TraceContextError::Format);
        }
        if version != "00" {
            return Err(TraceContextError::Version);
        }
        if !is_non_zero_lower_hex(trace_id, TRACE_ID_LEN) {
            return Err(TraceContextError::TraceId);
        }
        if !is_non_zero_lower_hex(span_id, SPAN_ID_LEN) {
            return Err(TraceContextError::SpanId);
        }
        if flags.len() != 2 || !is_lower_hex(flags) {
            return Err(TraceContextError::Format);
        }
        let flags = u8::from_str_radix(flags, 16).map_err(|_| TraceContextError::Format)?;
        Ok(TraceContext {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            sampled: flags & 0x01 != 0,
        })
    }

    /// Formats this context back into a `traceparent` header value.
    pub fn to_traceparent(&self) -> String {
        let flags = if self.sampled { "01" } else { "00" };
        format!("00-{}-{}-{flags}", self.trace_id, self.span_id)
    }
}

fn is_lower_hex(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_digit() || byte.is_ascii_lowercase() && byte.is_ascii_hexdigit()
        })
}

pub(super) fn is_non_zero_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && is_lower_hex(value) && value.bytes().any(|byte| byte != b'0')
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    #[test]
    fn parses_a_valid_traceparent() {
        let context = TraceContext::parse(VALID).expect("valid traceparent");
        assert_eq!(context.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(context.span_id, "00f067aa0ba902b7");
        assert!(context.sampled);
    }

    #[test]
    fn round_trips_through_formatting() {
        let context = TraceContext::parse(VALID).expect("valid traceparent");
        assert_eq!(context.to_traceparent(), VALID);
    }

    #[test]
    fn unsampled_flag_round_trips() {
        let unsampled = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00";
        let context = TraceContext::parse(unsampled).expect("valid traceparent");
        assert!(!context.sampled);
        assert_eq!(context.to_traceparent(), unsampled);
    }

    #[test]
    fn rejects_an_all_zero_trace_id() {
        let value = "00-00000000000000000000000000000000-00f067aa0ba902b7-01";
        assert_eq!(TraceContext::parse(value), Err(TraceContextError::TraceId));
    }

    #[test]
    fn rejects_an_all_zero_span_id() {
        let value = "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01";
        assert_eq!(TraceContext::parse(value), Err(TraceContextError::SpanId));
    }

    #[test]
    fn rejects_the_wrong_number_of_segments() {
        assert_eq!(
            TraceContext::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7"),
            Err(TraceContextError::Format)
        );
        assert_eq!(
            TraceContext::parse(&format!("{VALID}-extra")),
            Err(TraceContextError::Format)
        );
    }

    #[test]
    fn rejects_an_unsupported_version() {
        assert_eq!(
            TraceContext::parse("ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
            Err(TraceContextError::Version)
        );
    }

    #[test]
    fn rejects_uppercase_hex() {
        assert_eq!(
            TraceContext::parse("00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01"),
            Err(TraceContextError::TraceId)
        );
    }

    #[test]
    fn rejects_wrong_length_identifiers() {
        assert_eq!(
            TraceContext::parse("00-4bf92f3577b34da6a3ce929d0e0e47-00f067aa0ba902b7-01"),
            Err(TraceContextError::TraceId)
        );
    }
}
