//! Binary entry point for the Greengrass component.
//!
//! On-device (built with `--features greengrass`) this connects to the
//! Nucleus IPC socket, subscribes to the local sensor topic and the cloud
//! response topic, and runs the orchestrator. Without the feature it
//! prints an explanatory error — host builds exist for tests/benchmarks.

use edge_ai_classifier::config::Config;
use std::path::PathBuf;

struct Args {
    model_path: PathBuf,
    config_json: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut model_path = None;
    let mut config_json = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--model-path" => {
                model_path = Some(PathBuf::from(
                    args.next().ok_or("--model-path requires a value")?,
                ));
            }
            "--config-json" => {
                config_json = Some(args.next().ok_or("--config-json requires a value")?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Args {
        model_path: model_path.ok_or("--model-path is required")?,
        config_json,
    })
}

fn load_config(args: &Args) -> Result<Config, String> {
    // Configuration comes from --config-json (the recipe interpolates the
    // merged component configuration into the run command) or from the
    // GG_CONFIG environment variable as a fallback.
    let json = match &args.config_json {
        Some(json) => json.clone(),
        None => std::env::var("GG_CONFIG")
            .map_err(|_| "no configuration: pass --config-json or set GG_CONFIG".to_string())?,
    };
    Config::from_json(&json).map_err(|e| format!("invalid configuration: {e}"))
}

#[cfg(feature = "greengrass")]
fn main() {
    use edge_ai_classifier::communication::mqtt::greengrass::GreengrassTransport;
    use edge_ai_classifier::inference::classifier::Classifier;
    use edge_ai_classifier::inference::model_loader::load_verified_model;
    use edge_ai_classifier::ingestion::ipc_subscriber::greengrass::subscribe_sensors;
    use edge_ai_classifier::orchestrator::Orchestrator;
    use std::sync::Arc;

    tracing_subscriber::fmt().with_target(false).init();

    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });
    let config = load_config(&args).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });

    // FR-8: verify model integrity before anything else.
    let session = load_verified_model(&args.model_path, &config.model_sha256).unwrap_or_else(|e| {
        eprintln!("fatal: {e}");
        std::process::exit(1);
    });
    let classifier = Classifier::new(session);

    let thing_name = std::env::var("AWS_IOT_THING_NAME").unwrap_or_else(|_| {
        eprintln!("fatal: AWS_IOT_THING_NAME not set (not running under Greengrass?)");
        std::process::exit(1);
    });

    let transport = GreengrassTransport::connect().unwrap_or_else(|e| {
        eprintln!("fatal: {e}");
        std::process::exit(1);
    });

    let sensors =
        subscribe_sensors(transport.sdk(), &config.sensor_ipc_topic).unwrap_or_else(|e| {
            eprintln!("fatal: {e}");
            std::process::exit(1);
        });
    let responses = transport
        .subscribe_responses(&config.mqtt_response_topic)
        .unwrap_or_else(|e| {
            eprintln!("fatal: {e}");
            std::process::exit(1);
        });

    tracing::info!(%thing_name, "edge-ai-classifier ready");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async move {
        Orchestrator::new(config, thing_name, classifier, Arc::new(transport))
            .run(sensors, responses)
            .await;
    });
}

#[cfg(not(feature = "greengrass"))]
fn main() {
    // Host builds still validate args, config, and the model (useful as a
    // pre-deployment smoke check: `edge-ai-classifier --model-path m.onnx
    // --config-json '{...}'` verifies the hash without a Nucleus).
    if let Ok(args) = parse_args() {
        if let Ok(config) = load_config(&args) {
            match edge_ai_classifier::inference::model_loader::load_verified_model(
                &args.model_path,
                &config.model_sha256,
            ) {
                Ok(_) => eprintln!("model and configuration OK"),
                Err(e) => {
                    eprintln!("fatal: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
    eprintln!(
        "edge-ai-classifier was built without the `greengrass` feature.\n\
         This binary only runs as a Greengrass component on Linux; build with\n\
         `cargo build --release --features greengrass --target aarch64-unknown-linux-gnu`\n\
         (see edge-component/Dockerfile)."
    );
    std::process::exit(1);
}
