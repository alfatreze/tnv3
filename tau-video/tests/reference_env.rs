//! Guard against silent skipping: many reference tests return early when their tools are not configured.
//! Without the variables below this test fails, with the exact list; set them (`make rust-conformance` does,
//! for all three packages) or accept skipping explicitly with `TAU_ALLOW_SKIP=1` (`make rust-test`).
const REQUIRED: &[&str] = &["TAU_TVC_VECTOR_DIR", "TAU_VIDEO_VECTOR_DIR", "TAU_TVC_DECODER", "TAU_TNV3_TOOL", "TAU_TEST_TNV3", "TAU_TEST_PYTHON", "TAU_TEST_FFMPEG", "TAU_TEST_FFPROBE", "TAU_TEST_TVC", "TAU_TEST_C_SCRIPT", "TAU_TEST_TVC_ENC", "TAU_TEST_TAC", "TAU_TEST_FONT"];

#[test]
fn reference_tools_are_configured_or_skipping_is_explicit() {
    if std::env::var("TAU_ALLOW_SKIP").is_ok_and(|v| v == "1") {
        eprintln!("TAU_ALLOW_SKIP=1: reference tests without their tools return early and are NOT checked.");
        return;
    }
    let missing: Vec<&str> = REQUIRED.iter().copied().filter(|n| std::env::var_os(n).is_none_or(|v| !std::path::Path::new(&v).exists() && !v.to_string_lossy().ends_with("python3") && v != "python3")).collect();
    assert!(
        missing.is_empty(),
        "Reference tests would be skipped silently because these variables are unset or point nowhere: {missing:?}. \
         Run `make rust-conformance`, or set TAU_ALLOW_SKIP=1 to skip on purpose."
    );
}
