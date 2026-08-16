//! The `tau.*`/`pi.*` and OpenTelemetry resource attribute catalog. Every
//! record family later stages emit must draw its attributes from here, so
//! the allowlist lives in one authoritative place instead of being repeated
//! (and drifting) at each call site.
use opentelemetry::Value;
use opentelemetry_semantic_conventions::resource::{
    DEPLOYMENT_ENVIRONMENT_NAME, HOST_ARCH, OS_TYPE, SERVICE_INSTANCE_ID, SERVICE_NAME,
    SERVICE_VERSION,
};

use super::privacy::DEFAULT_MAX_ATTRIBUTE_LEN;

/// The OpenTelemetry value shapes this catalog allows. Kept narrower than
/// `opentelemetry::Value` on purpose: nothing in the initial catalog needs a
/// float or an array, and adding one should be a deliberate catalog change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeKind {
    Str,
    I64,
}

#[derive(Debug, Clone, Copy)]
pub struct AttributeSpec {
    pub key: &'static str,
    pub kind: AttributeKind,
    /// Maximum encoded length for string values. Always `Some` for
    /// `AttributeKind::Str`; always `None` otherwise, which
    /// `catalog_specs_are_well_formed` below enforces.
    pub max_len: Option<usize>,
    /// Whether this key may appear on a metric's dimensions. `false` marks
    /// session, project, controller, runtime, request, and trace
    /// identifiers, which the design principles forbid on metrics because
    /// they would create unbounded cardinality.
    pub metric_safe: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeError {
    /// The key is not part of this family's allowlist (or the shared context
    /// attributes), so it cannot be recorded at all.
    UnknownAttribute,
    /// The value's runtime type does not match the spec's declared kind.
    WrongType,
    /// A string value exceeds the spec's `max_len`.
    TooLong,
}

/// Resource attributes attached once per app run (Stage 1 generates the
/// actual `Resource`). Not family-scoped: every record family carries these.
pub const RESOURCE_ATTRIBUTES: &[AttributeSpec] = &[
    AttributeSpec {
        key: SERVICE_NAME,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: SERVICE_VERSION,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: SERVICE_INSTANCE_ID,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: DEPLOYMENT_ENVIRONMENT_NAME,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: OS_TYPE,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
    AttributeSpec {
        key: HOST_ARCH,
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    },
];

/// Tau-specific context attributes. Attached to spans and logs when
/// meaningful; never to metric dimensions (all `metric_safe: false`), per the
/// design principles' cardinality rule.
pub const CONTEXT_ATTRIBUTES: &[AttributeSpec] = &[
    AttributeSpec {
        key: "tau.project.id",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    },
    AttributeSpec {
        key: "tau.session.id",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    },
    AttributeSpec {
        key: "tau.controller.id",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    },
    AttributeSpec {
        key: "tau.runtime.id",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: false,
    },
    AttributeSpec {
        key: "pi.generation",
        kind: AttributeKind::I64,
        max_len: None,
        metric_safe: false,
    },
];

/// One named event/span family and the attributes it may carry, beyond the
/// shared context attributes above.
pub struct RecordFamily {
    pub name: &'static str,
    pub attributes: &'static [AttributeSpec],
}

pub const UI_ACTION: RecordFamily = RecordFamily {
    name: "ui.action",
    attributes: &[AttributeSpec {
        key: "tau.action.name",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    }],
};

pub const TAURI_INVOKE: RecordFamily = RecordFamily {
    name: "tauri.invoke",
    attributes: &[AttributeSpec {
        key: "tau.invoke.command",
        kind: AttributeKind::Str,
        max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
        metric_safe: true,
    }],
};

pub const PI_RPC: RecordFamily = RecordFamily {
    name: "pi.rpc",
    attributes: &[
        AttributeSpec {
            key: "pi.rpc.method",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "pi.rpc.request_id",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: false,
        },
        AttributeSpec {
            key: "pi.rpc.outcome",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
    ],
};

pub const CONTROLLER_LIFECYCLE: RecordFamily = RecordFamily {
    name: "controller.lifecycle",
    attributes: &[
        AttributeSpec {
            key: "tau.controller.state.before",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.controller.state.after",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.controller.transition.cause",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
    ],
};

pub const PI_PROCESS_LIFECYCLE: RecordFamily = RecordFamily {
    name: "pi.process.lifecycle",
    attributes: &[
        AttributeSpec {
            key: "tau.process.stop_reason",
            kind: AttributeKind::Str,
            max_len: Some(DEFAULT_MAX_ATTRIBUTE_LEN),
            metric_safe: true,
        },
        AttributeSpec {
            key: "tau.process.exit_code",
            kind: AttributeKind::I64,
            max_len: None,
            metric_safe: true,
        },
    ],
};

/// `app.started`/`app.exited` lifecycle logs. They carry only the resource
/// and, where applicable, context attributes, so this family adds none.
pub const APP_LIFECYCLE: RecordFamily = RecordFamily {
    name: "app.lifecycle",
    attributes: &[],
};

pub const FAMILIES: &[&RecordFamily] = &[
    &UI_ACTION,
    &TAURI_INVOKE,
    &PI_RPC,
    &CONTROLLER_LIFECYCLE,
    &PI_PROCESS_LIFECYCLE,
    &APP_LIFECYCLE,
];

pub fn find_family(name: &str) -> Option<&'static RecordFamily> {
    FAMILIES.iter().copied().find(|family| family.name == name)
}

/// Resolves `key` against `family`'s own attributes and the shared context
/// attributes, in that order.
pub fn allowed_attribute(family: &str, key: &str) -> Option<&'static AttributeSpec> {
    let family = find_family(family)?;
    family
        .attributes
        .iter()
        .chain(CONTEXT_ATTRIBUTES)
        .find(|spec| spec.key == key)
}

/// Whether `key` is safe to use as a metric dimension. Unknown keys are not
/// safe: only cataloged, explicitly-reviewed attributes may reach a metric.
pub fn is_metric_safe(key: &str) -> bool {
    RESOURCE_ATTRIBUTES
        .iter()
        .chain(CONTEXT_ATTRIBUTES)
        .chain(FAMILIES.iter().flat_map(|family| family.attributes))
        .find(|spec| spec.key == key)
        .is_some_and(|spec| spec.metric_safe)
}

/// Validates `value` for `key` within `family`: the key must be allowlisted,
/// its runtime type must match the spec, and string values must fit the
/// spec's maximum length. This is the enforcement point that keeps telemetry
/// callers from recording arbitrary attributes.
pub fn validate(family: &str, key: &str, value: &Value) -> Result<(), AttributeError> {
    let spec = allowed_attribute(family, key).ok_or(AttributeError::UnknownAttribute)?;
    match (spec.kind, value) {
        (AttributeKind::Str, Value::String(string)) => {
            let max_len = spec.max_len.expect("string specs declare a max length");
            if string.as_str().len() > max_len {
                return Err(AttributeError::TooLong);
            }
        }
        (AttributeKind::I64, Value::I64(_)) => {}
        _ => return Err(AttributeError::WrongType),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_specs() -> impl Iterator<Item = &'static AttributeSpec> {
        RESOURCE_ATTRIBUTES
            .iter()
            .chain(CONTEXT_ATTRIBUTES)
            .chain(FAMILIES.iter().flat_map(|family| family.attributes))
    }

    #[test]
    fn catalog_specs_are_well_formed() {
        for spec in all_specs() {
            match spec.kind {
                AttributeKind::Str => assert!(
                    spec.max_len.is_some(),
                    "{} is a string attribute without a max length",
                    spec.key
                ),
                AttributeKind::I64 => assert!(
                    spec.max_len.is_none(),
                    "{} is a numeric attribute with a max length",
                    spec.key
                ),
            }
        }
    }

    #[test]
    fn context_identifiers_are_never_metric_safe() {
        for spec in CONTEXT_ATTRIBUTES {
            assert!(
                !spec.metric_safe,
                "{} must stay off metric dimensions",
                spec.key
            );
        }
    }

    #[test]
    fn family_attributes_are_reachable_by_name() {
        let spec = allowed_attribute("pi.rpc", "pi.rpc.method").expect("known attribute");
        assert_eq!(spec.kind, AttributeKind::Str);
    }

    #[test]
    fn context_attributes_are_reachable_from_every_family() {
        let spec = allowed_attribute("ui.action", "tau.session.id").expect("context attribute");
        assert!(!spec.metric_safe);
    }

    #[test]
    fn unknown_family_or_key_is_rejected() {
        assert!(allowed_attribute("does.not.exist", "tau.session.id").is_none());
        assert!(allowed_attribute("ui.action", "tau.does.not.exist").is_none());
    }

    #[test]
    fn validate_accepts_a_well_formed_attribute() {
        assert_eq!(
            validate(
                "pi.rpc",
                "pi.rpc.method",
                &Value::String("get_state".into())
            ),
            Ok(())
        );
        assert_eq!(
            validate(
                "pi.process.lifecycle",
                "tau.process.exit_code",
                &Value::I64(0)
            ),
            Ok(())
        );
    }

    #[test]
    fn validate_rejects_an_unknown_attribute() {
        assert_eq!(
            validate(
                "pi.rpc",
                "pi.rpc.transcript",
                &Value::String("hello".into())
            ),
            Err(AttributeError::UnknownAttribute)
        );
    }

    #[test]
    fn validate_rejects_a_type_mismatch() {
        assert_eq!(
            validate("pi.rpc", "pi.rpc.method", &Value::I64(1)),
            Err(AttributeError::WrongType)
        );
    }

    #[test]
    fn validate_rejects_an_oversized_string() {
        let oversized = "x".repeat(DEFAULT_MAX_ATTRIBUTE_LEN + 1);
        assert_eq!(
            validate("pi.rpc", "pi.rpc.method", &Value::String(oversized.into())),
            Err(AttributeError::TooLong)
        );
    }

    #[test]
    fn metric_safety_matches_the_catalog() {
        assert!(is_metric_safe("tau.action.name"));
        assert!(!is_metric_safe("tau.session.id"));
        assert!(!is_metric_safe("unknown.key"));
    }
}
