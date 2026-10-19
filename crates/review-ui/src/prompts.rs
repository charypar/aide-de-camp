use crate::context::Context;
use leptos::prelude::*;
use review_core::*;

#[component]
fn PromptField(
    cx: Context,
    field_key: String,
    label: String,
    options: Vec<String>,
    input_type: String,
) -> impl IntoView {
    let key = StoredValue::new(field_key);
    view! {<div class="request-field"><label for=key.get_value()>{label}</label>
        <div class="request-options">{options.into_iter().map(move|option|{let text=option.clone();view!{<button on:click=move |_|cx.change(|u|{u.request_drafts.insert(key.get_value(),text.clone());})>{option}</button>}}).collect_view()}</div>
        <input id=key.get_value() type=input_type prop:value=move||cx.ui.get().request_drafts.get(&key.get_value()).cloned().unwrap_or_default()
          on:input=move|e|cx.change(|u|{u.request_drafts.insert(key.get_value(),event_target_value(&e));})/>
    </div>}
}
#[component]
pub(crate) fn AgentPrompt(cx: Context, request: AgentRequest) -> impl IntoView {
    let mcp = request.method.contains("elicitation");
    let tool_approval = mcp && request.params["_meta"]["codex_approval_kind"] == "mcp_tool_call";
    let approval = request.method.ends_with("requestApproval") || tool_approval;
    let user_input = request.method.ends_with("requestUserInput");
    let known = approval || user_input || mcp;
    let data = StoredValue::new(request.clone());
    let title = if approval {
        "Codex needs your approval"
    } else {
        "Codex needs your input"
    };
    let description = request.params["message"]
        .as_str()
        .or(request.params["reason"].as_str())
        .unwrap_or("")
        .to_owned();
    let command = request.params["command"].as_str().unwrap_or("").to_owned();
    let mut fields: Vec<(String, String, Vec<String>, String)> = vec![];
    if user_input {
        for q in request.params["questions"].as_array().into_iter().flatten() {
            fields.push((
                q["id"].as_str().unwrap_or("").into(),
                q["question"].as_str().unwrap_or("Your answer").into(),
                q["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|o| o["label"].as_str().map(str::to_owned))
                    .collect(),
                if q["isSecret"] == true {
                    "password".into()
                } else {
                    "text".into()
                },
            ));
        }
    }
    if mcp && !approval {
        for (name, schema) in request.params["requestedSchema"]["properties"]
            .as_object()
            .into_iter()
            .flatten()
        {
            fields.push((
                name.clone(),
                schema["title"].as_str().unwrap_or(name).into(),
                schema["enum"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|o| o.as_str().map(str::to_owned))
                    .collect(),
                if schema["type"] == "integer" || schema["type"] == "number" {
                    "number".into()
                } else {
                    "text".into()
                },
            ));
        }
    }
    let field_ids = StoredValue::new(fields.iter().map(|f| f.0.clone()).collect::<Vec<_>>());
    let url = request.params["url"]
        .as_str()
        .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
        .map(str::to_owned);
    let controller = PromptController {
        cx,
        data,
        field_ids,
        mcp,
        user_input,
    };
    let approve = move |_| controller.decision(true);
    let decline = move |_| controller.decision(false);
    let submit = move |_| controller.submit();
    let cancel = move |_| controller.cancel();
    let custom_draft = move |event| {
        cx.change(|ui| {
            ui.request_drafts
                .insert(data.get_value().id.to_string(), event_target_value(&event));
        })
    };
    view! {<section class="agent-request"><strong>{title}</strong><p class="request-description">{description}</p>
        {(!command.is_empty()).then(||view!{<pre class="request-command">{command}</pre>})}
        {url.map(|url|view!{<a href=url target="_blank" rel="noopener noreferrer">"Open requested authorization page"</a>})}
        <div hidden=!approval><button on:click=approve>"Approve once"</button><button on:click=decline>"Decline"</button></div>
        <div hidden=approval>
            {fields.into_iter().map(move|(name,label,options,input_type)|view!{<PromptField cx=cx field_key=format!("{}:{}",data.get_value().id,name) label=label options=options input_type=input_type/>}).collect_view()}
            <div hidden=known><p>"This harness request uses a custom response. Enter its JSON below."</p><textarea aria-label="Native response JSON" prop:value=move||cx.ui.get().request_drafts.get(&data.get_value().id.to_string()).cloned().unwrap_or_default() on:input=custom_draft></textarea></div>
            <button on:click=submit>"Send response"</button>
            <button on:click=cancel>"Cancel"</button>
        </div>
        <details><summary>"Request details"</summary><pre>{serde_json::to_string_pretty(&request.params).unwrap()}</pre></details>
    </section>}
}

#[derive(Clone, Copy)]
struct PromptController {
    cx: Context,
    data: StoredValue<AgentRequest>,
    field_ids: StoredValue<Vec<String>>,
    mcp: bool,
    user_input: bool,
}

impl PromptController {
    fn decision(self, accept: bool) {
        let response = if self.mcp {
            serde_json::json!({"action": if accept {"accept"} else {"decline"}, "content": {}})
        } else {
            serde_json::json!({"decision": if accept {"accept"} else {"decline"}})
        };
        self.cx.send(Command::Respond {
            request_id: self.data.get_value().id,
            response,
        });
    }

    fn cancel(self) {
        if self.mcp {
            self.cx.send(Command::Respond {
                request_id: self.data.get_value().id,
                response: serde_json::json!({"action":"cancel"}),
            });
        } else {
            self.cx.send(Command::Interrupt);
        }
    }

    fn submit(self) {
        let r = self.data.get_value();
        let drafts = self.cx.ui.get_untracked().request_drafts;
        let mut fields = serde_json::Map::new();
        for name in self.field_ids.get_value() {
            let text = drafts
                .get(&format!("{}:{}", r.id, name))
                .cloned()
                .unwrap_or_default();
            let value = if self.user_input {
                serde_json::json!({"answers":[text]})
            } else {
                match r.params["requestedSchema"]["properties"][&name]["type"].as_str() {
                    Some("integer" | "number" | "boolean" | "object" | "array") => {
                        match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => {
                                self.cx.error.set(format!("Enter a valid value for {name}"));
                                return;
                            }
                        }
                    }
                    _ => serde_json::Value::String(text),
                }
            };
            fields.insert(name, value);
        }
        let response = if self.user_input {
            serde_json::json!({"answers":fields})
        } else if self.mcp {
            serde_json::json!({"action":"accept","content":fields})
        } else {
            match serde_json::from_str(
                drafts
                    .get(&r.id.to_string())
                    .map(String::as_str)
                    .unwrap_or(""),
            ) {
                Ok(v) => v,
                Err(e) => {
                    self.cx.error.set(e.to_string());
                    return;
                }
            }
        };
        self.cx.send(Command::Respond {
            request_id: r.id,
            response,
        });
    }
}
