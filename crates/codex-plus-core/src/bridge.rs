use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, anyhow};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

pub const BRIDGE_BINDING_NAME: &str = "codexSessionDeleteV2";

pub type BridgeHandler = Arc<
    dyn Fn(String, Value) -> Pin<Box<dyn Future<Output = anyhow::Result<Value>> + Send>>
        + Send
        + Sync,
>;

static NEXT_MESSAGE_ID: AtomicU64 = AtomicU64::new(100);

pub fn build_bridge_script(binding_name: &str) -> String {
    format!(
        r#"
(() => {{
  window.__codexSessionDeleteCallbacks = new Map();
  window.__codexSessionDeleteSeq = 0;
  window.__codexSessionDeleteResolve = (id, result) => {{
    const callback = window.__codexSessionDeleteCallbacks.get(id);
    if (!callback) return;
    window.__codexSessionDeleteCallbacks.delete(id);
    callback.resolve(result);
  }};
  window.__codexSessionDeleteReject = (id, message) => {{
    const callback = window.__codexSessionDeleteCallbacks.get(id);
    if (!callback) return;
    window.__codexSessionDeleteCallbacks.delete(id);
    callback.resolve({{ status: "failed", message }});
  }};
  window.__codexSessionDeleteBridge = (path, payload) => new Promise((resolve) => {{
    const id = String(++window.__codexSessionDeleteSeq);
    window.__codexSessionDeleteCallbacks.set(id, {{ resolve }});
    window.{binding_name}(JSON.stringify({{ id, path, payload }}));
  }});
}})();
"#
    )
}

pub async fn evaluate_script(websocket_url: &str, script: &str) -> anyhow::Result<Value> {
    let (mut socket, _) = connect_async(websocket_url)
        .await
        .context("failed to connect CDP websocket")?;
    send_command(
        &mut socket,
        1,
        "Runtime.evaluate",
        json!({
            "expression": script,
            "awaitPromise": false,
            "allowUnsafeEvalBlockedByCSP": true,
        }),
    )
    .await
}

pub async fn add_script_to_new_documents(
    websocket_url: &str,
    script: &str,
) -> anyhow::Result<Value> {
    let (mut socket, _) = connect_async(websocket_url)
        .await
        .context("failed to connect CDP websocket")?;
    send_command(
        &mut socket,
        1,
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": script }),
    )
    .await
}

pub async fn install_bridge(
    websocket_url: &str,
    binding_name: &str,
    handler: BridgeHandler,
    new_document_scripts: &[String],
) -> anyhow::Result<()> {
    let (mut socket, _) = connect_async(websocket_url)
        .await
        .context("failed to connect CDP websocket")?;

    send_command(&mut socket, 1, "Runtime.enable", json!({})).await?;
    send_command(
        &mut socket,
        2,
        "Runtime.removeBinding",
        json!({ "name": binding_name }),
    )
    .await?;
    send_command(
        &mut socket,
        3,
        "Runtime.addBinding",
        json!({ "name": binding_name }),
    )
    .await?;

    let bridge_script = build_bridge_script(binding_name);
    send_command(
        &mut socket,
        4,
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": bridge_script }),
    )
    .await?;
    send_command(
        &mut socket,
        5,
        "Runtime.evaluate",
        runtime_evaluate_params(&bridge_script),
    )
    .await?;

    for script in new_document_scripts {
        let message_id = next_message_id();
        send_command(
            &mut socket,
            message_id,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": script }),
        )
        .await?;
    }

    while let Some(message) = socket.next().await {
        let message = message.context("failed to read CDP websocket message")?;
        let Message::Text(text) = message else {
            continue;
        };
        let value: Value = serde_json::from_str(&text).context("failed to parse CDP message")?;
        if value.get("method").and_then(Value::as_str) != Some("Runtime.bindingCalled") {
            continue;
        }
        route_binding_call(&mut socket, &handler, value).await?;
    }

    Ok(())
}

pub fn runtime_evaluate_params(script: &str) -> Value {
    json!({
        "expression": script,
        "awaitPromise": false,
        "allowUnsafeEvalBlockedByCSP": true,
    })
}

pub fn resolve_bridge_expression(request_id: &str, result: &Value) -> anyhow::Result<String> {
    Ok(format!(
        "window.__codexSessionDeleteResolve({}, {})",
        serde_json::to_string(request_id)?,
        serde_json::to_string(result)?,
    ))
}

pub fn reject_bridge_expression(request_id: &str, message: &str) -> anyhow::Result<String> {
    Ok(format!(
        "window.__codexSessionDeleteReject({}, {})",
        serde_json::to_string(request_id)?,
        serde_json::to_string(message)?,
    ))
}

async fn route_binding_call<S>(
    socket: &mut S,
    handler: &BridgeHandler,
    message: Value,
) -> anyhow::Result<()>
where
    S: SinkExt<Message>
        + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    let payload = message
        .get("params")
        .and_then(|params| params.get("payload"))
        .and_then(Value::as_str)
        .unwrap_or("{}");
    let parsed: Value = serde_json::from_str(payload).context("failed to parse bridge payload")?;
    let request_id = parsed
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("bridge payload missing id"))?;
    let path = parsed
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let payload = parsed.get("payload").cloned().unwrap_or_else(|| json!({}));

    match handler(path, payload).await {
        Ok(result) => {
            let expression = resolve_bridge_expression(request_id, &result)?;
            send_command(
                socket,
                next_message_id(),
                "Runtime.evaluate",
                runtime_evaluate_params(&expression),
            )
            .await?;
        }
        Err(error) => {
            let expression = reject_bridge_expression(request_id, &error.to_string())?;
            send_command(
                socket,
                next_message_id(),
                "Runtime.evaluate",
                runtime_evaluate_params(&expression),
            )
            .await?;
        }
    }

    Ok(())
}

async fn send_command<S>(
    socket: &mut S,
    message_id: u64,
    method: &str,
    params: Value,
) -> anyhow::Result<Value>
where
    S: SinkExt<Message>
        + StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    socket
        .send(Message::Text(
            json!({
                "id": message_id,
                "method": method,
                "params": params,
            })
            .to_string()
            .into(),
        ))
        .await
        .context("failed to send CDP command")?;

    wait_for_id(socket, message_id).await
}

async fn wait_for_id<S>(socket: &mut S, message_id: u64) -> anyhow::Result<Value>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(message) = socket.next().await {
        let message = message.context("failed to read CDP websocket message")?;
        let Message::Text(text) = message else {
            continue;
        };
        let value: Value = serde_json::from_str(&text).context("failed to parse CDP response")?;
        if value.get("id").and_then(Value::as_u64) != Some(message_id) {
            continue;
        }
        if let Some(error) = value.get("error") {
            return Err(anyhow!("CDP command failed: {error}"));
        }
        return Ok(value);
    }

    Err(anyhow!("CDP websocket closed before response {message_id}"))
}

fn next_message_id() -> u64 {
    NEXT_MESSAGE_ID.fetch_add(1, Ordering::Relaxed) + 1
}
