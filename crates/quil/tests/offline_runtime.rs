#![cfg(target_os = "macos")]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn run(command: &mut Command) -> Output {
    command.output().unwrap_or_else(|error| {
        panic!("failed to run {command:?}: {error}");
    })
}

#[test]
fn runtime_never_attempts_an_outbound_connection() {
    let root = repository_root();
    let work = root.join("target/quil-network-test");
    fs::create_dir_all(&work).expect("create test work directory");
    let library = work.join("libquil_network_probe.dylib");
    let client = work.join("network_probe_client");
    let log = work.join("connections.log");
    let _ = fs::remove_file(&log);

    let compile_library = run(Command::new("cc")
        .arg("-dynamiclib")
        .arg("-fPIC")
        .arg(root.join("tests/network_probe.c"))
        .arg("-o")
        .arg(&library));
    assert!(
        compile_library.status.success(),
        "probe compilation failed: {}",
        String::from_utf8_lossy(&compile_library.stderr)
    );
    let compile_client = run(Command::new("cc")
        .arg(root.join("tests/network_probe_client.c"))
        .arg("-o")
        .arg(&client));
    assert!(
        compile_client.status.success(),
        "probe client compilation failed: {}",
        String::from_utf8_lossy(&compile_client.stderr)
    );

    let sandbox = root.join("tests/deny-network.sb");
    let run_probed = |executable: &Path, arguments: &[&str], log_path: &Path| {
        let mut command = Command::new(executable);
        command
            .args(arguments)
            .env("DYLD_INSERT_LIBRARIES", &library)
            .env("DYLD_FORCE_FLAT_NAMESPACE", "1")
            .env("QUIL_NETWORK_PROBE_PATH", log_path);
        run(&mut command)
    };

    // Prove the interposer catches and blocks a deliberate connect before using
    // it as the runtime assertion, so the test cannot pass due to missing DYLD
    // instrumentation.
    let self_test = run_probed(&client, &[], &log);
    assert!(
        self_test.status.success(),
        "probe self-test failed: {}",
        String::from_utf8_lossy(&self_test.stderr)
    );
    assert_eq!(
        fs::read_to_string(&log).expect("probe self-test log"),
        "connect\n",
        "probe did not intercept the deliberate connection"
    );
    fs::remove_file(&log).expect("clear probe log");

    let quil = Path::new(env!("CARGO_BIN_EXE_quil"));
    let output = run_probed(quil, &["models"], &log);
    assert!(
        output.status.success(),
        "quil failed under the outbound-connection probe: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !log.exists(),
        "quil attempted an outbound connection: {}",
        fs::read_to_string(&log).unwrap_or_default()
    );

    let sandboxed = run(Command::new("sandbox-exec")
        .arg("-p")
        .arg(include_str!("../../../tests/deny-network.sb"))
        .arg(quil)
        .arg("models"));
    assert!(
        sandboxed.status.success(),
        "quil failed under deny-network sandbox {}: {}",
        sandbox.display(),
        String::from_utf8_lossy(&sandboxed.stderr)
    );

    let model_dir = std::env::var("QUIL_TEST_MODEL_DIR").ok();
    let wav = std::env::var("QUIL_TEST_WAV").ok();
    if model_dir.is_some() != wav.is_some() {
        panic!("QUIL_TEST_MODEL_DIR and QUIL_TEST_WAV must be set together");
    }
    if let (Some(model_dir), Some(wav)) = (model_dir, wav) {
        let model_dir = fs::canonicalize(root.join(model_dir)).expect("canonical model directory");
        let model_dir = model_dir.to_str().expect("UTF-8 model directory");
        let wav = fs::canonicalize(root.join(wav)).expect("canonical benchmark WAV");
        let wav = wav.to_str().expect("UTF-8 benchmark WAV");
        let arguments = [
            "--model-dir",
            model_dir,
            "--execution",
            "cpu",
            "transcribe",
            "--model",
            "moonshine-tiny",
            "--input",
            wav,
        ];
        let transcription = run_probed(quil, &arguments, &log);
        assert!(
            transcription.status.success(),
            "local ONNX transcription failed under the network probe: {}",
            String::from_utf8_lossy(&transcription.stderr)
        );
        assert!(
            !log.exists(),
            "local ONNX transcription attempted an outbound connection: {}",
            fs::read_to_string(&log).unwrap_or_default()
        );

        let sandboxed_transcription = run(Command::new("sandbox-exec")
            .arg("-p")
            .arg(include_str!("../../../tests/deny-network.sb"))
            .arg(quil)
            .args(arguments));
        assert!(
            sandboxed_transcription.status.success(),
            "local ONNX transcription failed under deny-network sandbox: {}",
            String::from_utf8_lossy(&sandboxed_transcription.stderr)
        );
    }
}
