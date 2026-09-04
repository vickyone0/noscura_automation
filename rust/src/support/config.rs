use playwright_rs::api::launch_options::LaunchOptions;
use std::env;

pub fn login_url() -> String {
    env::var("LOGIN_URL").unwrap_or_else(|_| "https://admin-test.noscura.net/login".to_string())
}

pub fn new_patient_url() -> String {
    env::var("NEW_PATIENT_URL")
        .unwrap_or_else(|_| "https://admin-test.noscura.net/newHomeAdmin?selectedOption=1".to_string())
}

pub fn login_email() -> Option<String> {
    env::var("LOGIN_EMAIL").ok()
}

pub fn login_password() -> Option<String> {
    env::var("LOGIN_PASSWORD").ok()
}

pub fn run_login_smoke() -> bool {
    env::var("RUN_NOSCURA_LOGIN_SMOKE").as_deref() == Ok("1")
}

// Set HEADED=1 to watch a run live in an actual visible Chromium window instead of the default
// headless mode. SLOW_MO_MS (default 150) pads every mouse/keyboard action so it's legible
// rather than a blur; set SLOW_MO_MS=0 to disable padding while still running headed.
pub fn launch_options() -> LaunchOptions {
    let headed = env::var("HEADED").as_deref() == Ok("1");
    let slow_mo = env::var("SLOW_MO_MS").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(if headed { 150.0 } else { 0.0 });
    let mut options = LaunchOptions::new().headless(!headed);
    if slow_mo > 0.0 {
        options = options.slow_mo(slow_mo);
    }
    options
}
