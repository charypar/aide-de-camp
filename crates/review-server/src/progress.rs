use crate::app::Shared;
use std::{
    io::{IsTerminal, Write},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) struct TerminalProgress;

impl TerminalProgress {
    pub fn spawn(app: Shared, url: String) {
        tokio::spawn(async move {
            let terminal = std::io::stdout().is_terminal();
            let mut previous_phase = None;
            let mut last_print = Instant::now();
            let mut ticks = tokio::time::interval(Duration::from_secs(1));

            loop {
                ticks.tick().await;
                let store = app.store.lock().await;
                let review = &store.review;
                let elapsed = review.preparation_elapsed(now_ms()).unwrap_or_default();
                let phase = if review.generating {
                    Some(if review.requests.is_empty() {
                        "Preparing the walkthrough"
                    } else {
                        "Waiting for your response"
                    })
                } else {
                    None
                };
                let stopped = review.error.is_some() || review.guide_html.is_empty();
                drop(store);

                if let Some(phase) = phase {
                    if terminal
                        || previous_phase != Some(phase)
                        || last_print.elapsed() >= Duration::from_secs(15)
                    {
                        let message = format!("{phase} · {elapsed} elapsed · {url}");
                        if terminal {
                            print!("\r\x1b[2K{message}");
                            let _ = std::io::stdout().flush();
                        } else {
                            println!("{message}");
                        }
                        last_print = Instant::now();
                    }
                } else if previous_phase.is_some() {
                    let outcome = if stopped {
                        "Preparation stopped"
                    } else {
                        "Walkthrough ready"
                    };
                    let prefix = if terminal { "\r\x1b[2K" } else { "" };
                    println!("{prefix}{outcome} · {elapsed} elapsed · {url}");
                }
                previous_phase = phase;
            }
        });
    }
}
