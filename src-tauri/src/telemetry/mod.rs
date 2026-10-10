pub use tau_observability::telemetry::{trace_context, CommandSpan};

pub mod ingest;

pub struct Telemetry(std::sync::Arc<tau_observability::Telemetry>);

impl Telemetry {
    pub fn init() -> Self {
        Self(
            tau_observability::Telemetry::init_with_environment(
                if crate::profile::ENVIRONMENT_NAME == "development" {
                    tau_observability::telemetry::Environment::Development
                } else {
                    tau_observability::telemetry::Environment::Production
                },
            )
            .expect("Tau profile initialized before telemetry"),
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test(dir: std::path::PathBuf) -> Self {
        Self(std::sync::Arc::new(tau_observability::Telemetry::for_test(
            dir,
        )))
    }
}

impl std::ops::Deref for Telemetry {
    type Target = tau_observability::Telemetry;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
