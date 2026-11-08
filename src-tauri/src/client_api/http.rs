use serde::de::DeserializeOwned;
use serde::Serialize;

use super::ClientCredentials;

const REQUEST_TIMEOUT_SECS: u64 = 15;

#[derive(Debug, PartialEq, Eq)]
pub enum ApiError {
    Expired,
    Unavailable(String),
}

pub fn join_api_path(api_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        api_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

pub fn get_json<T: DeserializeOwned>(creds: &ClientCredentials, path: &str) -> Result<T, ApiError> {
    let url = join_api_path(&creds.api_url, path);
    let response = agent()
        .get(&url)
        .set("Authorization", &bearer(&creds.token))
        .call();
    read_json(response)
}

#[allow(dead_code)]
pub fn post_json<B: Serialize, T: DeserializeOwned>(
    creds: &ClientCredentials,
    path: &str,
    body: &B,
) -> Result<T, ApiError> {
    let url = join_api_path(&creds.api_url, path);
    let payload = serde_json::to_string(body)
        .map_err(|err| ApiError::Unavailable(format!("api encode failed: {err}")))?;
    let response = agent()
        .post(&url)
        .set("Authorization", &bearer(&creds.token))
        .set("Content-Type", "application/json")
        .send_string(&payload);
    read_json(response)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn read_json<T: DeserializeOwned>(
    response: Result<ureq::Response, ureq::Error>,
) -> Result<T, ApiError> {
    match response {
        Ok(body) => {
            let text = body
                .into_string()
                .map_err(|err| ApiError::Unavailable(format!("api read failed: {err}")))?;
            serde_json::from_str(&text)
                .map_err(|err| ApiError::Unavailable(format!("api parse failed: {err}")))
        }
        Err(ureq::Error::Status(401, _)) => Err(ApiError::Expired),
        Err(error) => Err(ApiError::Unavailable(format!("api unavailable: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_api_paths_without_double_slashes() {
        assert_eq!(
            join_api_path("https://a1.example/api/", "/usage"),
            "https://a1.example/api/usage"
        );
    }
}
