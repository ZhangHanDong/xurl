use std::process::ExitCode;

use clap::Parser;
use xurl_core::{
    ProviderRoots, ThreadUri, render_subagent_view_markdown, render_thread_head_markdown,
    render_thread_markdown, resolve_subagent_view, resolve_thread,
};

#[derive(Debug, Parser)]
#[command(name = "xurl", version, about = "Resolve and read code-agent threads")]
struct Cli {
    /// Thread URI like agents://codex/<session_id>, agents://claude/<session_id>, agents://pi/<session_id>/<entry_id>, or legacy forms like codex://<session_id>
    uri: String,

    /// Output frontmatter only (header mode)
    #[arg(short = 'I', long)]
    head: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> xurl_core::Result<()> {
    let roots = ProviderRoots::from_env_or_home()?;
    let uri = ThreadUri::parse(&cli.uri)?;

    if cli.head {
        let head = render_thread_head_markdown(&uri, &roots)?;
        print!("{head}");
        return Ok(());
    }

    let supports_subagent = matches!(
        uri.provider,
        xurl_core::ProviderKind::Codex | xurl_core::ProviderKind::Claude
    );

    if supports_subagent && uri.agent_id.is_some() {
        let head = render_thread_head_markdown(&uri, &roots)?;
        let view = resolve_subagent_view(&uri, &roots, false)?;
        let body = render_subagent_view_markdown(&view);
        let markdown = format!("{head}\n{body}");
        print!("{markdown}");
        return Ok(());
    }

    let head = render_thread_head_markdown(&uri, &roots)?;
    let resolved = resolve_thread(&uri, &roots)?;
    let body = render_thread_markdown(&uri, &resolved)?;
    let markdown = format!("{head}\n{body}");
    print!("{markdown}");

    Ok(())
}
