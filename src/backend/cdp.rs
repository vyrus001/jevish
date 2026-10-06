use super::BrowserBackend;
use crate::contracts::{
    ActionPlan, CONTRACT_VERSION, DocumentIdentity, ExecutionResult, Operation, RawNode,
    RawSnapshot,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::{Message, WebSocket, connect, stream::MaybeTlsStream};
use url::Url;

const ACTION_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const CDP_COMMAND_RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const ACCESSIBILITY_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(120);
const NAVIGATION_PROBE_TIMEOUT: Duration = Duration::from_millis(250);
const NAVIGATION_SETTLE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, PartialEq)]
enum NavigationCall {
    Completed { value: Value, navigated: bool },
    Indeterminate,
}

const PREPARE_FILL: &str = r#"function(){
this.focus();
if(typeof this.select==='function')this.select();
const prototype=this instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:this instanceof HTMLInputElement?HTMLInputElement.prototype:Object.getPrototypeOf(this);
const descriptor=Object.getOwnPropertyDescriptor(prototype,'value');
if(!descriptor||!descriptor.set)throw new Error('target has no native value setter');
descriptor.set.call(this,'');
this.dispatchEvent(new InputEvent('input',{bubbles:true,composed:true,inputType:'deleteContentBackward',data:null}));
return this.value;
}"#;

const PREPARE_TYPE: &str = r#"function(){
this.focus();
if(typeof this.setSelectionRange==='function'){const end=(this.value||'').length;this.setSelectionRange(end,end);}
return this.value||'';
}"#;

const SET_TEXT_VALUE: &str = r#"function(v,inputType,data){
this.focus();
const prototype=this instanceof HTMLTextAreaElement?HTMLTextAreaElement.prototype:this instanceof HTMLInputElement?HTMLInputElement.prototype:Object.getPrototypeOf(this);
const descriptor=Object.getOwnPropertyDescriptor(prototype,'value');
if(!descriptor||!descriptor.set)throw new Error('target has no native value setter');
descriptor.set.call(this,v);
this.dispatchEvent(new InputEvent('input',{bubbles:true,composed:true,inputType:inputType,data:data}));
return this.value;
}"#;

const COMMIT_TEXT: &str = r#"async function(expected){
this.dispatchEvent(new Event('change',{bubbles:true}));
this.blur();
let stable=true;
for(const delay of [0,16,50,100,250]){
  await new Promise(resolve=>setTimeout(resolve,delay));
  if(this.value!==expected)stable=false;
}
return {value:this.value,stable:stable};
}"#;

const SET_SELECT_VALUE: &str = r#"async function(v){
this.focus();
const descriptor=Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,'value');
descriptor.set.call(this,v);
this.dispatchEvent(new Event('input',{bubbles:true,composed:true}));
this.dispatchEvent(new Event('change',{bubbles:true}));
this.blur();
let stable=true;
for(const delay of [0,16,50,100,250]){
  await new Promise(resolve=>setTimeout(resolve,delay));
  if(this.value!==v)stable=false;
}
return {value:this.value,stable:stable};
}"#;

fn verified_value(operation: Operation, expected: &str, output: Value) -> Result<Value> {
    let actual = output["value"]
        .as_str()
        .context("browser action returned no field value")?;
    let stable = output["stable"]
        .as_bool()
        .context("browser action returned no stability result")?;
    if !stable || actual != expected {
        bail!("ACTION_NOT_APPLIED: {operation:?} field value did not persist after stabilization")
    }
    Ok(json!({"verified": true}))
}

pub struct CdpBackend {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl CdpBackend {
    fn response_timeout(method: &str) -> Duration {
        if method == "Accessibility.getFullAXTree" {
            ACCESSIBILITY_SNAPSHOT_TIMEOUT
        } else {
            CDP_COMMAND_RESPONSE_TIMEOUT
        }
    }

    pub fn connect(endpoint: &str) -> Result<Self> {
        let url = Url::parse(endpoint).context("CDP endpoint must be a ws:// or wss:// URL")?;
        let (socket, _) = connect(url.as_str()).context("failed to connect to CDP endpoint")?;
        let mut backend = Self { socket, next_id: 1 };
        backend.call("Accessibility.enable", json!({}))?;
        backend.call("Page.enable", json!({}))?;
        backend.call("Runtime.enable", json!({}))?;
        Ok(backend)
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.socket.send(Message::Text(
            json!({"id": id, "method": method, "params": params})
                .to_string()
                .into(),
        ))?;
        let deadline = Instant::now() + Self::response_timeout(method);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.set_read_timeout(None)?;
                bail!("CDP_RESPONSE_TIMEOUT: {method} did not receive a response")
            }
            self.set_read_timeout(Some(remaining))?;
            let message = match self.socket.read() {
                Ok(message) => message,
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    self.set_read_timeout(None)?;
                    bail!("CDP_RESPONSE_TIMEOUT: {method} did not receive a response")
                }
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    bail!("CDP_CONNECTION_CLOSED: {method} lost its page target")
                }
                Err(error) => return Err(error.into()),
            };
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(&text)?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                self.set_read_timeout(None)?;
                bail!("CDP {method} failed: {error}")
            }
            self.set_read_timeout(None)?;
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> Result<()> {
        let result = match self.socket.get_mut() {
            MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
            MaybeTlsStream::NativeTls(stream) => stream.get_ref().set_read_timeout(timeout),
            _ => Ok(()),
        };
        result.context("failed to configure CDP response timeout")
    }

    fn navigation_event(value: &Value, expected_frame_id: &str) -> Option<bool> {
        let method = value.get("method").and_then(Value::as_str)?;
        let frame_id = match method {
            "Page.frameNavigated" => value.pointer("/params/frame/id").and_then(Value::as_str),
            "Page.frameStartedLoading" | "Page.frameStoppedLoading" => {
                value.pointer("/params/frameId").and_then(Value::as_str)
            }
            _ => return None,
        };
        (frame_id == Some(expected_frame_id)).then_some(method == "Page.frameStoppedLoading")
    }

    fn call_navigation_aware(
        &mut self,
        method: &str,
        params: Value,
        expected_frame_id: &str,
    ) -> Result<NavigationCall> {
        let id = self.next_id;
        self.next_id += 1;
        self.socket.send(Message::Text(
            json!({"id": id, "method": method, "params": params})
                .to_string()
                .into(),
        ))?;

        let mut response = None;
        let mut navigation_started = false;
        let mut deadline = Instant::now() + ACTION_RESPONSE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.set_read_timeout(None)?;
                return Ok(if let Some(value) = response {
                    NavigationCall::Completed {
                        value,
                        navigated: navigation_started,
                    }
                } else {
                    NavigationCall::Indeterminate
                });
            }
            self.set_read_timeout(Some(remaining))?;
            let message = match self.socket.read() {
                Ok(message) => message,
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    self.set_read_timeout(None)?;
                    return Ok(if let Some(value) = response {
                        NavigationCall::Completed {
                            value,
                            navigated: navigation_started,
                        }
                    } else {
                        NavigationCall::Indeterminate
                    });
                }
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return Ok(NavigationCall::Indeterminate);
                }
                Err(error) => return Err(error.into()),
            };
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(&text)?;
            if let Some(completed) = Self::navigation_event(&value, expected_frame_id) {
                navigation_started = true;
                if completed {
                    self.set_read_timeout(None)?;
                    return Ok(NavigationCall::Completed {
                        value: response.unwrap_or(Value::Bool(true)),
                        navigated: true,
                    });
                }
                deadline = Instant::now() + NAVIGATION_SETTLE_TIMEOUT;
                continue;
            }
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                self.set_read_timeout(None)?;
                bail!("CDP {method} failed: {error}")
            }
            response = Some(value.get("result").cloned().unwrap_or(Value::Null));
            if !navigation_started {
                deadline = Instant::now() + NAVIGATION_PROBE_TIMEOUT;
            }
        }
    }

    fn scalar(value: Option<&Value>) -> String {
        value
            .and_then(|item| item.get("value"))
            .and_then(|item| match item {
                Value::String(text) => Some(text.clone()),
                Value::Bool(flag) => Some(flag.to_string()),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn document(&mut self) -> Result<DocumentIdentity> {
        let tree = self.call("Page.getFrameTree", json!({}))?;
        let frame = &tree["frameTree"]["frame"];
        let url = frame["url"].as_str().unwrap_or_default().to_owned();
        let context_id = frame["id"].as_str().unwrap_or_default().to_owned();
        let revision = frame
            .get("loaderId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let title = self.call(
            "Runtime.evaluate",
            json!({"expression":"document.title","returnByValue":true}),
        )?["result"]["value"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        Ok(DocumentIdentity {
            url,
            title,
            context_id,
            revision,
        })
    }

    fn resolve_object(&mut self, backend_node_id: i64) -> Result<String> {
        let result = self.call("DOM.resolveNode", json!({"backendNodeId": backend_node_id}))?;
        result["object"]["objectId"]
            .as_str()
            .map(str::to_owned)
            .context("target has no runtime object")
    }

    fn call_on(&mut self, object_id: &str, function: &str, args: Vec<Value>) -> Result<Value> {
        let arguments: Vec<Value> = args
            .into_iter()
            .map(|value| json!({"value": value}))
            .collect();
        let result = self.call("Runtime.callFunctionOn", json!({"objectId": object_id, "functionDeclaration": function, "arguments": arguments, "returnByValue": true, "awaitPromise": true}))?;
        if let Some(exception) = result.get("exceptionDetails") {
            bail!("browser action failed: {exception}")
        }
        Ok(result["result"]["value"].clone())
    }

    fn call_on_navigation_aware(
        &mut self,
        object_id: &str,
        function: &str,
        expected_frame_id: &str,
    ) -> Result<NavigationCall> {
        let outcome = self.call_navigation_aware(
            "Runtime.callFunctionOn",
            json!({"objectId": object_id, "functionDeclaration": function, "arguments": [], "returnByValue": true, "awaitPromise": true}),
            expected_frame_id,
        )?;
        if let NavigationCall::Completed { value, navigated } = outcome {
            if let Some(exception) = value.get("exceptionDetails") {
                bail!("browser action failed: {exception}")
            }
            return Ok(NavigationCall::Completed {
                value: value["result"]["value"].clone(),
                navigated,
            });
        }
        Ok(outcome)
    }

    fn fill(&mut self, object: &str, value: &str) -> Result<Value> {
        self.call_on(object, PREPARE_FILL, vec![])?;
        self.call_on(
            object,
            SET_TEXT_VALUE,
            vec![json!(value), json!("insertText"), json!(value)],
        )?;
        let output = self.call_on(object, COMMIT_TEXT, vec![json!(value)])?;
        verified_value(Operation::Fill, value, output)
    }

    fn type_text(&mut self, object: &str, value: &str) -> Result<Value> {
        let initial = self.call_on(object, PREPARE_TYPE, vec![])?;
        let initial = initial
            .as_str()
            .context("browser action returned no initial field value")?;
        let expected = format!("{initial}{value}");
        self.call_on(
            object,
            SET_TEXT_VALUE,
            vec![json!(expected), json!("insertText"), json!(value)],
        )?;
        let output = self.call_on(object, COMMIT_TEXT, vec![json!(expected)])?;
        verified_value(Operation::Type, &expected, output)
    }

    fn select(&mut self, object: &str, value: &str) -> Result<Value> {
        let output = self.call_on(object, SET_SELECT_VALUE, vec![json!(value)])?;
        verified_value(Operation::Select, value, output)
    }
}

impl BrowserBackend for CdpBackend {
    fn snapshot(&mut self) -> Result<RawSnapshot> {
        let document = self.document()?;
        let result = self.call("Accessibility.getFullAXTree", json!({}))?;
        let nodes = result["nodes"]
            .as_array()
            .context("CDP returned no accessibility nodes")?
            .iter()
            .map(|node| {
                let properties = node["properties"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| {
                                item["name"]
                                    .as_str()
                                    .map(|name| (name.to_owned(), item["value"].clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_else(BTreeMap::new);
                RawNode {
                    node_id: node["nodeId"].as_str().unwrap_or_default().into(),
                    parent_id: node
                        .get("parentId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    backend_node_id: node.get("backendDOMNodeId").and_then(Value::as_i64),
                    ignored: node["ignored"].as_bool().unwrap_or(false),
                    role: Self::scalar(node.get("role")),
                    name: Self::scalar(node.get("name")),
                    description: Self::scalar(node.get("description")),
                    value: Self::scalar(node.get("value")),
                    properties,
                }
            })
            .collect::<Vec<_>>();
        let captured_at_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let mut digest = Sha256::new();
        digest.update(serde_json::to_vec(&(&document, &nodes))?);
        let snapshot_id = format!("{:x}", digest.finalize())[..16].to_owned();
        Ok(RawSnapshot {
            contract: CONTRACT_VERSION.into(),
            backend: "cdp".into(),
            snapshot_id,
            document,
            captured_at_ms,
            nodes,
        })
    }

    fn execute(&mut self, plan: &ActionPlan) -> Result<ExecutionResult> {
        let current = self.document()?;
        if current.context_id != plan.document.context_id
            || current.revision != plan.document.revision
            || current.url != plan.document.url
        {
            bail!("STALE_DOCUMENT: page identity changed after planning")
        }
        let mut status = "executed";
        let mut navigation_dispatched = false;
        let mut result = match plan.operation {
            Operation::Open => {
                navigation_dispatched = true;
                match self.call_navigation_aware(
                    "Page.navigate",
                    json!({"url": plan.value.as_deref().context("open requires URL")?}),
                    &plan.document.context_id,
                )? {
                    NavigationCall::Completed { navigated, .. } => {
                        if navigated {
                            status = "navigation_completed";
                        }
                        Some(json!({
                            "dispatch": "established",
                            "navigation": if navigated { "completed" } else { "not_observed" }
                        }))
                    }
                    NavigationCall::Indeterminate => {
                        status = "navigation_indeterminate";
                        Some(json!({"dispatch": "established", "navigation": "indeterminate"}))
                    }
                }
            }
            Operation::Press => {
                let key = plan.value.as_deref().context("press requires key")?;
                self.call(
                    "Input.dispatchKeyEvent",
                    json!({"type":"keyDown","key":key}),
                )?;
                self.call("Input.dispatchKeyEvent", json!({"type":"keyUp","key":key}))?;
                None
            }
            Operation::Scroll => {
                let direction = plan.value.as_deref().context("scroll requires direction")?;
                let expression = match direction {
                    "up" => "scrollBy(0,-window.innerHeight*.8)",
                    "down" => "scrollBy(0,window.innerHeight*.8)",
                    "left" => "scrollBy(-window.innerWidth*.8,0)",
                    "right" => "scrollBy(window.innerWidth*.8,0)",
                    "top" => "scrollTo(0,0)",
                    "bottom" => "scrollTo(0,document.body.scrollHeight)",
                    _ => bail!("unsupported scroll direction"),
                };
                self.call("Runtime.evaluate", json!({"expression":expression}))?;
                None
            }
            operation => {
                let target = plan.target.as_ref().context("operation requires target")?;
                let backend_node_id = target
                    .backend_node_id
                    .context("target is not backed by a DOM node")?;
                let object = self.resolve_object(backend_node_id)?;
                let identity = self.call_on(&object, "function(){return {role:this.getAttribute('role')||this.tagName.toLowerCase(),name:this.getAttribute('aria-label')||this.innerText||this.value||'',visible:!!(this.offsetWidth||this.offsetHeight||this.getClientRects().length),enabled:!this.disabled}}", vec![])?;
                if identity["visible"] != true || identity["enabled"] != true {
                    bail!("STALE_TARGET: target is hidden or disabled")
                }
                let observed_name = identity["name"].as_str().unwrap_or_default().trim();
                if !observed_name.is_empty() && !target.fingerprint.is_empty() {
                    // Fingerprint comparison happens through a fresh accessibility snapshot below.
                    let fresh = crate::extraction::extract(&self.snapshot()?);
                    let Some(element) = fresh
                        .elements
                        .iter()
                        .find(|element| element.backend_node_id == Some(backend_node_id))
                    else {
                        bail!("STALE_TARGET: target disappeared")
                    };
                    if element.fingerprint != target.fingerprint {
                        bail!("STALE_TARGET: target identity changed")
                    }
                }
                let value = plan.value.as_deref();
                let output = match operation {
                    Operation::Click => {
                        navigation_dispatched = true;
                        match self.call_on_navigation_aware(
                            &object,
                            "function(){this.click();return true}",
                            &plan.document.context_id,
                        )? {
                            NavigationCall::Completed { value, navigated } => {
                                if navigated {
                                    status = "navigation_completed";
                                    json!({"dispatch": "established", "navigation": "completed"})
                                } else {
                                    value
                                }
                            }
                            NavigationCall::Indeterminate => {
                                status = "navigation_indeterminate";
                                json!({"dispatch": "established", "navigation": "indeterminate"})
                            }
                        }
                    }
                    Operation::Focus => {
                        self.call_on(&object, "function(){this.focus();return true}", vec![])?
                    }
                    Operation::Fill => self.fill(&object, value.context("fill requires value")?)?,
                    Operation::Type => {
                        self.type_text(&object, value.context("type requires value")?)?
                    }
                    Operation::Check => self.call_on(
                        &object,
                        "function(){if(!this.checked)this.click();return !!this.checked}",
                        vec![],
                    )?,
                    Operation::Uncheck => self.call_on(
                        &object,
                        "function(){if(this.checked)this.click();return !!this.checked}",
                        vec![],
                    )?,
                    Operation::Select => {
                        self.select(&object, value.context("select requires value")?)?
                    }
                    Operation::GetText => self.call_on(
                        &object,
                        "function(){return this.innerText||this.textContent||this.value||''}",
                        vec![],
                    )?,
                    _ => unreachable!(),
                };
                Some(output)
            }
        };
        let document = if status == "navigation_indeterminate" {
            plan.document.clone()
        } else {
            match self.document() {
                Ok(document) => document,
                Err(_) if navigation_dispatched => {
                    status = "navigation_indeterminate";
                    result =
                        Some(json!({"dispatch": "established", "navigation": "indeterminate"}));
                    plan.document.clone()
                }
                Err(error) => return Err(error),
            }
        };
        Ok(ExecutionResult {
            contract: CONTRACT_VERSION.into(),
            status: status.into(),
            operation: plan.operation,
            target_id: plan.target.as_ref().map(|target| target.element_id.clone()),
            result,
            document,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn navigation_server(close_after_dispatch: bool) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let request = socket.read().unwrap();
            assert!(matches!(request, Message::Text(_)));
            if close_after_dispatch {
                socket.close(None).unwrap();
                return;
            }
            socket
                .send(Message::Text(
                    json!({"method":"Page.frameStartedLoading","params":{"frameId":"frame"}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            socket
                .send(Message::Text(
                    json!({"method":"Page.frameStoppedLoading","params":{"frameId":"frame"}})
                        .to_string()
                        .into(),
                ))
                .unwrap();

            let request = socket.read().unwrap();
            let Message::Text(request) = request else {
                panic!("expected follow-up CDP command")
            };
            let request: Value = serde_json::from_str(&request).unwrap();
            socket
                .send(Message::Text(
                    json!({"id":request["id"],"result":{"active":true}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        });
        (format!("ws://{address}"), handle)
    }

    fn test_backend(endpoint: &str) -> CdpBackend {
        let (socket, _) = connect(endpoint).unwrap();
        CdpBackend { socket, next_id: 1 }
    }

    #[test]
    fn react_text_write_uses_native_setter_and_stabilized_events() {
        assert!(PREPARE_FILL.contains("Object.getOwnPropertyDescriptor(prototype,'value')"));
        assert!(PREPARE_FILL.contains("deleteContentBackward"));
        assert!(SET_TEXT_VALUE.contains("Object.getOwnPropertyDescriptor(prototype,'value')"));
        assert!(SET_TEXT_VALUE.contains("new InputEvent('input'"));
        assert!(COMMIT_TEXT.contains("change"));
        assert!(COMMIT_TEXT.contains("this.blur()"));
        assert!(COMMIT_TEXT.contains("[0,16,50,100,250]"));
    }

    #[test]
    fn timeout_policy_keeps_navigation_bounded_and_allows_slow_snapshots() {
        assert_eq!(
            CdpBackend::response_timeout("Accessibility.getFullAXTree"),
            Duration::from_secs(120)
        );
        assert_eq!(
            CdpBackend::response_timeout("Runtime.evaluate"),
            Duration::from_secs(30)
        );
        assert!(ACTION_RESPONSE_TIMEOUT < CDP_COMMAND_RESPONSE_TIMEOUT);
        assert!(NAVIGATION_SETTLE_TIMEOUT < ACCESSIBILITY_SNAPSHOT_TIMEOUT);
    }

    #[test]
    fn async_controlled_input_reset_fails_without_exposing_value() {
        let secret_test_value = "iam-user";
        let delayed_reset = json!({
            "value": "",
            "stable": false,
            "samples": [secret_test_value, ""]
        });
        let error = verified_value(Operation::Fill, secret_test_value, delayed_reset)
            .expect_err("controlled input reset after a tick must fail");
        assert!(error.to_string().contains("ACTION_NOT_APPLIED"));
        assert!(!error.to_string().contains(secret_test_value));
        assert_eq!(
            verified_value(
                Operation::Fill,
                secret_test_value,
                json!({"value": secret_test_value, "stable": true})
            )
            .unwrap(),
            json!({"verified": true})
        );
    }

    #[test]
    fn navigation_response_loss_does_not_block_follow_up_command() {
        let (endpoint, server) = navigation_server(false);
        let mut backend = test_backend(&endpoint);
        let outcome = backend
            .call_navigation_aware("Runtime.callFunctionOn", json!({}), "frame")
            .unwrap();
        assert_eq!(
            outcome,
            NavigationCall::Completed {
                value: json!(true),
                navigated: true,
            }
        );
        assert_eq!(
            backend.call("Runtime.evaluate", json!({})).unwrap(),
            json!({"active":true})
        );
        server.join().unwrap();
    }

    #[test]
    fn closed_target_after_dispatch_returns_navigation_indeterminate() {
        let (endpoint, server) = navigation_server(true);
        let mut backend = test_backend(&endpoint);
        let outcome = backend
            .call_navigation_aware("Runtime.callFunctionOn", json!({}), "frame")
            .unwrap();
        assert_eq!(outcome, NavigationCall::Indeterminate);
        server.join().unwrap();
    }
}
