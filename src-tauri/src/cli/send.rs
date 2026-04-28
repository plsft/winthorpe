//! `winthorpe send` — send a prompt to an agent, stream output.
//! Also hosts `winthorpe models list`.

use std::io::{Read, Write};

use anyhow::{Context, Result};

use crate::agents::AgentStreamEvent;
use crate::pipeline::types::{ExtendedMessagePart, MessagePart};
use crate::service;

use super::args::{Cli, ModelsAction, SendArgs};
use super::output;

pub fn send(args: &SendArgs, cli: &Cli) -> Result<()> {
    let prompt = read_prompt(&args.prompt).context("Failed to read prompt")?;
    let permission_mode = if args.plan {
        Some("plan".to_string())
    } else {
        args.permission_mode.clone()
    };

    let params = service::SendMessageParams {
        workspace_ref: args.workspace.clone(),
        session_id: args.session.clone(),
        prompt,
        model: args.model.clone(),
        permission_mode,
        linked_directories: args.linked_dirs.clone(),
    };

    let mut stdout = std::io::stdout().lock();
    let json_mode = cli.json;
    let quiet = cli.quiet;

    let result = service::send_message(params, &mut |event| {
        if json_mode {
            if let Ok(line) = serde_json::to_string(event) {
                let _ = writeln!(stdout, "{line}");
                let _ = stdout.flush();
            }
            return;
        }

        match event {
            AgentStreamEvent::StreamingPartial { message } => {
                for part in &message.content {
                    if let ExtendedMessagePart::Basic(MessagePart::Text { text, .. }) = part {
                        let _ = write!(stdout, "{text}");
                        let _ = stdout.flush();
                    }
                }
            }
            AgentStreamEvent::Done {
                provider,
                resolved_model,
                session_id,
                ..
            } if !quiet => {
                let _ = writeln!(stdout);
                let _ = writeln!(stdout, "---");
                let _ = writeln!(
                    stdout,
                    "Done ({provider}/{resolved_model}) session={}",
                    session_id.as_deref().unwrap_or("-")
                );
            }
            AgentStreamEvent::Aborted { reason, .. } => {
                let _ = writeln!(stdout);
                let _ = writeln!(stdout, "---");
                let _ = writeln!(stdout, "Aborted: {reason}");
            }
            AgentStreamEvent::Error { message, .. } => {
                eprintln!("Error: {message}");
            }
            _ => {}
        }
    })?;

    if !json_mode && !quiet {
        eprintln!(
            "[session: {} | model: {}/{}]",
            result.session_id, result.provider, result.model
        );
    }

    Ok(())
}

fn read_prompt(raw: &str) -> Result<String> {
    if raw == "-" {
        let mut buffer = String::new();
        std::io::stdin().read_to_string(&mut buffer)?;
        Ok(buffer)
    } else {
        Ok(raw.to_string())
    }
}

// ---------------------------------------------------------------------------
// models
// ---------------------------------------------------------------------------

pub fn dispatch_models(action: &ModelsAction, cli: &Cli) -> Result<()> {
    match action {
        ModelsAction::List => list_models(cli),
    }
}

fn list_models(cli: &Cli) -> Result<()> {
    let sections = service::fetch_model_sections();
    output::print(cli, &sections, |all| {
        let mut lines = Vec::new();
        for section in all {
            lines.push(format!("{} ({:?})", section.label, section.status,));
            for option in &section.options {
                lines.push(format!("  {}\t{}", option.id, option.label));
            }
        }
        if lines.is_empty() {
            "No models available.".to_string()
        } else {
            lines.join("\n")
        }
    })
}
