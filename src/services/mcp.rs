//! Local MCP transport. App state stays on the GPUI thread.
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
};
use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::*,
    service::RequestContext,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::Arc,
    time::Duration,
};

pub type Reply = async_channel::Sender<Result<Value, String>>;
pub enum Action {
    List,
    Pull {
        workspace_id: String,
        expected_branch: String,
    },
}
pub struct RequestMessage {
    pub action: Action,
    pub reply: Reply,
}

pub struct Server {
    stop: Box<dyn Fn() + Send>,
}
impl Drop for Server {
    fn drop(&mut self) {
        (self.stop)();
    }
}

#[derive(Clone)]
struct Handler {
    requests: async_channel::Sender<RequestMessage>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PullArgs {
    workspace_id: String,
    expected_branch: String,
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info = Implementation::new("artifex", env!("CARGO_PKG_VERSION"));
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.instructions = Some("Discover open workspaces, then pull the expected branch after pushing upstream. Never retry a timed-out pull without checking repository state.".into());
        info
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut result = ListToolsResult::default();
        result.tools = serde_json::from_value(json!([
            {"name":"list_workspaces","description":"List open Artifex workspaces and their current cached Git state.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}},
            {"name":"pull_workspace","description":"Fetch and fast-forward an open workspace's configured upstream. Requires a saved editor and no active Git operation. Does not push or switch branches.","inputSchema":{"type":"object","properties":{"workspace_id":{"type":"string","minLength":1},"expected_branch":{"type":"string","minLength":1}},"required":["workspace_id","expected_branch"],"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":true}}
        ])).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(result)
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        let action = match request.name.as_ref() {
            "list_workspaces" if args.as_object().is_some_and(|a| a.is_empty()) => Action::List,
            "pull_workspace" => {
                let a: PullArgs = serde_json::from_value(args)
                    .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
                if a.workspace_id.is_empty() || a.expected_branch.is_empty() {
                    return Err(ErrorData::invalid_params(
                        "Workspace and branch must not be empty",
                        None,
                    ));
                }
                Action::Pull {
                    workspace_id: a.workspace_id,
                    expected_branch: a.expected_branch,
                }
            }
            _ => {
                return Err(ErrorData::invalid_params(
                    "Unknown tool or invalid arguments",
                    None,
                ));
            }
        };
        let (reply, receive) = async_channel::bounded(1);
        let result = if self
            .requests
            .try_send(RequestMessage { action, reply })
            .is_err()
        {
            Err("App unavailable or request queue full".into())
        } else {
            match tokio::time::timeout(Duration::from_secs(45), receive.recv()).await {
                Ok(Ok(result)) => result,
                Ok(Err(_)) => Err("Workspace or app closed; operation outcome unknown".into()),
                Err(_) => Err("Timed out; operation may still finish. Check repository state before retrying.".into()),
            }
        };
        Ok(match result {
            Ok(value) => CallToolResult::structured(value),
            Err(message) => CallToolResult::structured_error(json!({"error":message})),
        }
        .into())
    }
}

async fn authorize(
    State(token): State<Arc<String>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let valid = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|candidate| token_matches(candidate, &token));
    if !valid {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(request).await)
}

fn token_matches(candidate: &str, token: &str) -> bool {
    candidate.len() == token.len()
        && candidate
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

pub fn load_token(path: &Path) -> Result<String, String> {
    let mut file = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(mut file) => {
            let mut random = [0u8; 32];
            fs::File::open("/dev/urandom")
                .and_then(|mut source| source.read_exact(&mut random))
                .map_err(|e| e.to_string())?;
            let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
            file.write_all(token.as_bytes())
                .map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            return Ok(token);
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| e.to_string())?,
        Err(e) => return Err(e.to_string()),
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 || metadata.len() > 128 {
        return Err("MCP token must be a private regular file (chmod 600)".into());
    }
    let mut token = String::new();
    file.read_to_string(&mut token).map_err(|e| e.to_string())?;
    let token = token.trim().to_owned();
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("MCP token must contain 64 hexadecimal characters".into());
    }
    Ok(token)
}

pub fn start() -> Result<(Server, async_channel::Receiver<RequestMessage>), String> {
    let port = std::env::var("ARTIFEX_MCP_PORT")
        .unwrap_or_else(|_| "47831".into())
        .parse::<u16>()
        .map_err(|_| "Invalid ARTIFEX_MCP_PORT")?;
    if port == 0 {
        return Err("ARTIFEX_MCP_PORT must be a fixed nonzero port".into());
    }
    let path = super::session::state_path()
        .and_then(|p| p.parent().map(|p| p.join("mcp-token")))
        .ok_or("Cannot locate app support directory")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let token = Arc::new(load_token(&path)?);
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).map_err(|e| {
        format!("MCP port {port}: {e}; close the other instance or set ARTIFEX_MCP_PORT")
    })?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let (requests, receive) = async_channel::bounded(16);
    let mut config = StreamableHttpServerConfig::default();
    config.legacy_session_mode = false;
    config.json_response = true;
    config.allowed_hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    config.allowed_origins = vec![format!("http://127.0.0.1:{port}"), format!("http://localhost:{port}")];
    let cancel = config.cancellation_token.clone();
    let shutdown = cancel.clone();
    let close_requests = requests.clone();
    std::thread::Builder::new()
        .name("artifex-mcp".into())
        .spawn(move || {
            runtime.block_on(async move {
                let service = StreamableHttpService::new(
                    move || {
                        Ok(Handler {
                            requests: requests.clone(),
                        })
                    },
                    Arc::new(LocalSessionManager::default()),
                    config,
                );
                let router = axum::Router::new()
                    .nest_service("/mcp", service)
                    .layer(middleware::from_fn_with_state(token, authorize));
                match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => {
                        if let Err(e) = axum::serve(listener, router)
                            .with_graceful_shutdown(shutdown.cancelled_owned())
                            .await
                        {
                            eprintln!("artifex MCP: {e}");
                        }
                    }
                    Err(e) => eprintln!("artifex MCP: {e}"),
                }
            });
        })
        .map_err(|e| e.to_string())?;
    Ok((
        Server {
            stop: Box::new(move || {
                close_requests.close();
                cancel.cancel();
            }),
        },
        receive,
    ))
}
