use crate::app::{Shared, publish};
use anyhow::Result;
use review_core::*;
use uuid::Uuid;

pub(crate) async fn fixture_turn(app: &Shared, prompt: Option<String>) -> Result<()> {
    let id = app.store.lock().await.review.id.clone();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let mut s = app.store.lock().await;
    if s.review.id != id || !s.review.busy {
        return Ok(());
    }
    if let Some(text) = prompt {
        if text.starts_with("fixture:approval")
            || text.starts_with("fixture:question")
            || text.starts_with("fixture:form")
        {
            let (method, params) = if text.starts_with("fixture:approval") {
                (
                    "mcpServer/elicitation/request",
                    serde_json::json!({"message":"Allow the fixture tool to inspect this review?","_meta":{"codex_approval_kind":"mcp_tool_call"},"requestedSchema":{"type":"object","properties":{}}}),
                )
            } else if text.starts_with("fixture:question") {
                (
                    "item/tool/requestUserInput",
                    serde_json::json!({"questions":[{"id":"scope","question":"Which behavior should I explain?","options":[{"label":"Arithmetic"},{"label":"Errors"}]}]}),
                )
            } else {
                (
                    "mcpServer/elicitation/request",
                    serde_json::json!({"message":"Provide a review label.","requestedSchema":{"type":"object","properties":{"label":{"type":"string","title":"Review label"}}}}),
                )
            };
            s.review.requests.push(AgentRequest {
                id: serde_json::json!(Uuid::new_v4().to_string()),
                method: method.into(),
                params,
            });
            s.save("fixture_request", None)?;
            publish(app, &s);
            return Ok(());
        }
        if text.starts_with("fixture:slow") {
            s.review.messages.push(review_core::Message {
                id: Uuid::new_v4().to_string(),
                role: MessageRole::Assistant,
                text: "Partial response before interruption".into(),
                reference: None,
                status: MessageStatus::Streaming,
            });
            s.save("fixture_partial", None)?;
            publish(app, &s);
            return Ok(());
        }
        s.review.messages.push(review_core::Message {
            id: Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            text: format!("Fixture response to: {text}"),
            reference: None,
            status: MessageStatus::Complete,
        });
    } else {
        let mut guide="# Workspace walkthrough\n\nThis change updates the **workspace** implementation. Inspect the following changes and discuss the tradeoffs before recording feedback.\n\n".to_string();
        for file in &s.review.workspace.files {
            if let Some(source) = &file.new {
                guide.push_str(&format!("## {}\n\nReview how this file contributes to the change.\n\n```text {{\"file\":{},\"start\":1}}\n{}```\n\n",file.path,serde_json::to_string(&file.path)?,if source.ends_with('\n'){source.clone()}else{format!("{source}\n")}));
            }
        }
        s.review.guide = guide;
        let (h, w) = document::render(&s.review.guide, &s.review.workspace);
        s.review.guide_html = h;
        s.review.guide_warnings = w;
    }
    s.review.busy = false;
    s.review.generating = false;
    s.save("fixture", None)?;
    publish(app, &s);
    Ok(())
}
