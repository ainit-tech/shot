//! ONNX Runtime, which runs the program's models. It allows one environment in a process, and a
//! second one fails even after the first has gone, so the models share one that stays.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use ort::environment::Environment;

static ENVIRONMENT: Mutex<Option<Environment>> = Mutex::new(None);

/// The environment of ONNX Runtime, from its library the first time.
pub fn environment(library: &Path) -> Result<Environment, String> {
    let mut environment = ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(environment) = environment.as_ref() {
        return Ok(environment.clone());
    }
    let built = ort::init_from(library)
        .map_err(|error| error.to_string())?
        .with_telemetry(false)
        .build()
        .map_err(|error| error.to_string())?;
    *environment = Some(built.clone());
    Ok(built)
}
