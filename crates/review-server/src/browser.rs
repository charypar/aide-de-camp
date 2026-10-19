use crate::app::Shared;
use review_core::{Review, ReviewStatus, ServerEvent};

/// Open once useful content is ready, or earlier when the reviewer must act.
pub(crate) struct BrowserLauncher;

impl BrowserLauncher {
    fn ready(review: &Review) -> bool {
        (!review.generating && !review.guide_html.is_empty())
            || !review.requests.is_empty()
            || review.error.is_some()
            || review.status == ReviewStatus::Completed
    }

    pub fn spawn(app: Shared, url: String) {
        let mut events = app.events.subscribe();
        tokio::spawn(async move {
            loop {
                if Self::ready(&app.store.lock().await.review) {
                    if let Err(error) = webbrowser::open(&url) {
                        eprintln!("Open {url} in your browser ({error})");
                    }
                    return;
                }
                match events.recv().await {
                    Ok(ServerEvent::Snapshot { .. })
                    | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Ok(_) => continue,
                    Err(_) => return,
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use review_core::{AgentRequest, Workspace};
    #[test]
    fn opens_for_ready_guides_or_required_interaction_only() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(
            temp.path(),
            Workspace {
                fingerprint: "test".into(),
                vcs: "git".into(),
                files: vec![],
                diff: String::new(),
            },
        )
        .unwrap();
        assert!(!BrowserLauncher::ready(&store.review));
        store.review.generating = true;
        store.review.guide = "# Draft".into();
        assert!(!BrowserLauncher::ready(&store.review));
        store.review.requests.push(AgentRequest {
            id: serde_json::json!(1),
            method: "approval".into(),
            params: serde_json::json!({}),
        });
        assert!(BrowserLauncher::ready(&store.review));
        store.review.requests.clear();
        store.review.error = Some("Preparation failed".into());
        assert!(BrowserLauncher::ready(&store.review));
        store.review.error = None;
        store.review.generating = false;
        store.review.guide_html = "<h1>Ready</h1>".into();
        assert!(BrowserLauncher::ready(&store.review));
    }
}
