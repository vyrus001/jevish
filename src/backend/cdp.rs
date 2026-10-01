use super::BrowserBackend;
use crate::contracts::{
    ActionPlan, CONTRACT_VERSION, DocumentIdentity, ExecutionResult, Operation, RawNode,
    RawSnapshot,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};
use tungstenite::{Message, WebSocket, connect, stream::MaybeTlsStream};
use url::Url;

pub struct CdpBackend {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl CdpBackend {
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
        loop {
            let message = self.socket.read()?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(&text)?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                bail!("CDP {method} failed: {error}")
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
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
        let result = match plan.operation {
            Operation::Open => {
                self.call(
                    "Page.navigate",
                    json!({"url": plan.value.as_deref().context("open requires URL")?}),
                )?;
                None
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
                let value = plan.value.clone().map(Value::String).into_iter().collect();
                let output = match operation {
                    Operation::Click => self.call_on(&object, "function(){this.click();return true}", vec![])?,
                    Operation::Focus => self.call_on(&object, "function(){this.focus();return true}", vec![])?,
                    Operation::Fill => self.call_on(&object, "function(v){this.focus();this.value=v;this.dispatchEvent(new Event('input',{bubbles:true}));this.dispatchEvent(new Event('change',{bubbles:true}));return this.value}", value)?,
                    Operation::Type => self.call_on(&object, "function(v){this.focus();this.value=(this.value||'')+v;this.dispatchEvent(new Event('input',{bubbles:true}));return this.value}", value)?,
                    Operation::Check => self.call_on(&object, "function(){if(!this.checked)this.click();return !!this.checked}", vec![])?,
                    Operation::Uncheck => self.call_on(&object, "function(){if(this.checked)this.click();return !!this.checked}", vec![])?,
                    Operation::Select => self.call_on(&object, "function(v){this.value=v;this.dispatchEvent(new Event('input',{bubbles:true}));this.dispatchEvent(new Event('change',{bubbles:true}));return this.value}", value)?,
                    Operation::GetText => self.call_on(&object, "function(){return this.innerText||this.textContent||this.value||''}", vec![])?,
                    _ => unreachable!(),
                };
                Some(output)
            }
        };
        let document = self.document()?;
        Ok(ExecutionResult {
            contract: CONTRACT_VERSION.into(),
            status: "executed".into(),
            operation: plan.operation,
            target_id: plan.target.as_ref().map(|target| target.element_id.clone()),
            result,
            document,
        })
    }
}
