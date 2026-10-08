use std::time::Duration;

const USER_AGENT: &str = concat!("bdo-discord-rpc/", env!("CARGO_PKG_VERSION"));

pub fn get(url: &str, what: &str, timeout: Duration) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(timeout))
        .timeout_recv_response(Some(timeout))
        .timeout_recv_body(Some(timeout))
        .user_agent(USER_AGENT)
        .build()
        .into();
    agent
        .get(url)
        .call()
        .map_err(|e| format!("{what}: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("{what}, reading the Response: {e}"))
}
