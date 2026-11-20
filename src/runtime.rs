//! ONNX Runtime, which runs the program's models. It allows one environment in a process, and a
//! second one fails even after the first has gone, so the models share one that stays.
//!
//! A model runs on the graphics card or on the processor. The card is reached through WebGPU, a
//! provider of ONNX Runtime in a library of its own that the program downloads beside it: WebGPU
//! works over DirectX 12 on any card that has it. A model the card can't run runs on the processor.

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use ort::environment::Environment;
use ort::logging::LogLevel;
use ort::session::Session;
use ort::session::builder::SessionBuilder;

static ENVIRONMENT: Mutex<Option<Environment>> = Mutex::new(None);
/// Whether the provider of WebGPU is in the environment, or why it isn't: tried once a process.
static CARD: Mutex<Option<Result<(), String>>> = Mutex::new(None);

/// The name under which ONNX Runtime knows the devices of the provider of WebGPU.
const WEBGPU: &str = "WebGpuExecutionProvider";

/// Where a model runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Card,
    Processor,
    /// The graphics card was wanted and couldn't run the model: the processor runs it.
    CardFailed,
}

/// The environment of ONNX Runtime, from its library the first time.
pub fn environment(library: &Path) -> Result<Environment, String> {
    let mut environment = ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(environment) = environment.as_ref() {
        return Ok(environment.clone());
    }
    let mut builder = ort::init_from(library).map_err(|error| error.to_string())?.with_telemetry(false);
    // ONNX Runtime's own log, for finding out what went wrong: `ORT_LOG=1` gives its warnings and
    // errors, `ORT_LOG=verbose` everything.
    if let Some(wanted) = std::env::var_os("ORT_LOG") {
        let all = wanted == "verbose";
        builder = builder.with_logger(std::sync::Arc::new(move |level: LogLevel, category: &str, _: &str, place: &str, message: &str| {
            if all || matches!(level, LogLevel::Warning | LogLevel::Error | LogLevel::Fatal) {
                eprintln!("ONNX Runtime {level:?} [{category}] {place}: {message}");
            }
        }));
    }
    let built = builder.build().map_err(|error| error.to_string())?;
    *environment = Some(built.clone());
    Ok(built)
}

/// A session of a graph with the options `options` gives it: on the graphics card through the
/// provider of WebGPU in `card`, if it is given and the card can run the graph, and on the
/// processor otherwise. Which of them runs it comes with it.
pub fn session(
    environment: &Environment,
    graph: &Path,
    card: Option<&Path>,
    options: impl Fn(SessionBuilder) -> ort::Result<SessionBuilder>,
) -> Result<(Session, Device), String> {
    if let Some(provider) = card {
        match on_card(environment, graph, provider, &options) {
            Ok(session) => return Ok((session, Device::Card)),
            Err(problem) => eprintln!("{} doesn't run on the graphics card ({problem}): the processor runs it", graph.display()),
        }
    }
    let session = Session::builder(environment)
        .and_then(&options)
        .and_then(|mut builder| builder.commit_from_file(graph))
        .map_err(|error| error.to_string())?;
    Ok((session, if card.is_some() { Device::CardFailed } else { Device::Processor }))
}

/// A session of a graph on the graphics card, through the provider of WebGPU.
fn on_card(
    environment: &Environment,
    graph: &Path,
    provider: &Path,
    options: impl Fn(SessionBuilder) -> ort::Result<SessionBuilder>,
) -> Result<Session, String> {
    let registered = CARD
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(|| environment.register_ep_library("WebGPU", provider).map_err(|error| error.to_string()))
        .clone();
    registered?;
    let devices: Vec<_> = environment.devices().filter(|device| device.ep().is_ok_and(|ep| ep == WEBGPU)).collect();
    if devices.is_empty() {
        return Err("WebGPU found no graphics card".into());
    }
    Session::builder(environment)
        .and_then(options)
        .and_then(|builder| builder.with_devices(devices, None).map_err(Into::into))
        .and_then(|mut builder| builder.commit_from_file(graph))
        .map_err(|error| error.to_string())
}
