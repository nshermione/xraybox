use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use tauri::AppHandle;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::{connect, Message};

use crate::engine::error::EngineError;

use super::ClientCredentials;

static HOOKS: OnceLock<SocketHooks> = OnceLock::new();
static STOP: OnceLock<Mutex<Option<std::sync::mpsc::Sender<()>>>> = OnceLock::new();

#[derive(Clone, Copy)]
pub struct SocketHooks {
    pub load: fn(&AppHandle) -> Option<ClientCredentials>,
    pub refresh: fn(&AppHandle) -> Result<ClientCredentials, EngineError>,
    pub on_message: fn(&AppHandle, &str),
}

pub fn configure(hooks: SocketHooks) {
    let _ = HOOKS.set(hooks);
}

pub fn restart(app: &AppHandle) {
    stop();
    let Some(hooks) = HOOKS.get().copied() else {
        return;
    };
    let Some(api) = (hooks.load)(app) else {
        return;
    };
    if api.socket_url.is_empty() || api.token.is_empty() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    *STOP
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|err| err.into_inner()) = Some(tx);
    let handle = app.clone();
    thread::spawn(move || run_loop(handle, api, rx, hooks));
}

pub fn spawn_on_ready(app: &AppHandle) {
    restart(app);
}

pub fn stop() {
    if let Some(slot) = STOP.get() {
        if let Ok(mut guard) = slot.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(());
            }
        }
    }
}

fn run_loop(
    app: AppHandle,
    mut api: ClientCredentials,
    stop: std::sync::mpsc::Receiver<()>,
    hooks: SocketHooks,
) {
    let mut failures = 0;
    let mut refreshed = false;
    loop {
        if stop.try_recv().is_ok() {
            return;
        }
        match connect_socket(&api) {
            Ok(mut socket) => {
                failures = 0;
                loop {
                    if stop.try_recv().is_ok() {
                        let _ = socket.close(None);
                        return;
                    }
                    match socket.read() {
                        Ok(Message::Text(text)) => (hooks.on_message)(&app, &text),
                        Ok(Message::Ping(payload)) => {
                            let _ = socket.send(Message::Pong(payload));
                        }
                        Ok(Message::Close(frame)) => {
                            if frame
                                .as_ref()
                                .is_some_and(|close| close.code == CloseCode::from(4401))
                                && !refreshed
                            {
                                refreshed = true;
                                if let Ok(next) = (hooks.refresh)(&app) {
                                    api = next;
                                    break;
                                }
                            }
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            Err(_) => {
                failures += 1;
                if failures > 3 {
                    return;
                }
                if stop
                    .recv_timeout(Duration::from_secs(2u64.saturating_pow(failures)))
                    .is_ok()
                {
                    return;
                }
            }
        }
    }
}

fn connect_socket(
    api: &ClientCredentials,
) -> Result<
    tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    EngineError,
> {
    let mut request = api
        .socket_url
        .as_str()
        .into_client_request()
        .map_err(|err| EngineError::Config(err.to_string()))?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", api.token)
            .parse()
            .map_err(|err| EngineError::Config(format!("{err}")))?,
    );
    let (socket, _) = connect(request).map_err(|err| EngineError::Config(err.to_string()))?;
    Ok(socket)
}
