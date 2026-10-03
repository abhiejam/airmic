mod ipc_client;

use ipc_client::{Event, IpcClient, IpcError};
use serde_json::{Value, json};
use tauri::{Emitter, Manager};

/// Forwards one daemon request from the web UI, e.g. `invoke("ipc_call", { method: "status" })`.
#[tauri::command]
async fn ipc_call(
    client: tauri::State<'_, IpcClient>,
    method: String,
    params: Option<Value>,
) -> Result<Value, IpcError> {
    client.call(&method, params.unwrap_or(Value::Null)).await
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let socket = ipc_client::default_socket_path()?;
            let handle = app.handle().clone();
            let client = tauri::async_runtime::block_on(async move {
                IpcClient::spawn(socket, move |event| {
                    let _ = match event {
                        Event::Connected => {
                            handle.emit("daemon-connection", json!({ "connected": true }))
                        }
                        Event::Disconnected => {
                            handle.emit("daemon-connection", json!({ "connected": false }))
                        }
                        Event::Notification { method, params } => handle.emit(
                            "daemon-notification",
                            json!({ "method": method, "params": params }),
                        ),
                    };
                })
            });
            app.manage(client);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ipc_call])
        .run(tauri::generate_context!())
        .expect("error while running the AirMic app");
}
