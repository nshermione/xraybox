use super::http::ApiError;
use super::ClientCredentials;

pub fn with_token_refresh<T, E>(
    load: impl FnOnce() -> Result<Option<ClientCredentials>, E>,
    refresh: impl FnOnce() -> Result<ClientCredentials, E>,
    request: impl Fn(&ClientCredentials) -> Result<T, ApiError>,
    on_refreshed: impl FnOnce(),
    map_request_error: impl FnOnce(ApiError) -> E,
) -> Result<Option<T>, E> {
    let Some(api) = load()? else {
        return Ok(None);
    };
    match request(&api) {
        Ok(value) => Ok(Some(value)),
        Err(ApiError::Expired) => match refresh() {
            Ok(api) => {
                on_refreshed();
                request(&api).map(Some).map_err(map_request_error)
            }
            Err(_) => Ok(None),
        },
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn sample() -> ClientCredentials {
        ClientCredentials {
            api_url: "https://a1.example/api".into(),
            token: "tok".into(),
            socket_url: "wss://a1.example/api/ws".into(),
        }
    }

    #[test]
    fn retries_once_after_expired() {
        let creds = sample();
        let calls = Cell::new(0);
        let refreshed = Cell::new(false);
        let restarted = Cell::new(false);
        let result = with_token_refresh(
            || Ok(Some(creds.clone())),
            || {
                refreshed.set(true);
                Ok(creds.clone())
            },
            |_| {
                let n = calls.get();
                calls.set(n + 1);
                if n == 0 {
                    Err(ApiError::Expired)
                } else {
                    Ok("ok")
                }
            },
            || restarted.set(true),
            |error| error,
        );
        assert_eq!(result, Ok(Some("ok")));
        assert!(refreshed.get());
        assert!(restarted.get());
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn other_errors_become_none() {
        let result = with_token_refresh::<&str, ApiError>(
            || Ok(Some(sample())),
            || panic!("should not refresh"),
            |_| Err(ApiError::Unavailable("down".into())),
            || panic!("should not restart"),
            |error| error,
        );
        assert_eq!(result, Ok(None));
    }

    #[test]
    fn missing_credentials_become_none() {
        let result = with_token_refresh::<&str, ApiError>(
            || Ok(None),
            || panic!("should not refresh"),
            |_| panic!("should not request"),
            || panic!("should not restart"),
            |error| error,
        );
        assert_eq!(result, Ok(None));
    }

    #[test]
    fn failed_refresh_becomes_none() {
        let result = with_token_refresh::<&str, ApiError>(
            || Ok(Some(sample())),
            || Err(ApiError::Unavailable("no subscription".into())),
            |_| Err(ApiError::Expired),
            || panic!("should not restart"),
            |error| error,
        );
        assert_eq!(result, Ok(None));
    }
}
