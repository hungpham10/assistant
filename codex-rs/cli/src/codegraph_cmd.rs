//! `codex codegraph index` — host-driven indexing for the codegraph data
//! layer. This is the only indexing entry point; the agent tool
//! (`codegraph.call`) never indexes.

use std::path::PathBuf;

use clap::Args;
use clap::Subcommand;
use clap::ValueEnum;
use codex_core::CodegraphService;
use codex_core::IndexKind;

#[derive(Debug, Args)]
pub struct CodegraphCli {
    #[command(subcommand)]
    pub subcommand: CodegraphSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CodegraphSubcommand {
    /// Build/update the codegraph index for a workspace.
    Index(CodegraphIndexArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
pub enum IndexKindArg {
    /// Index source code + documents (default).
    All,
    /// Index source code only.
    Source,
    /// Index documents only (`.codegraph/config.toml [docgraph] paths`).
    Docs,
}

impl IndexKindArg {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Source => "source",
            Self::Docs => "docs",
        }
    }
}

#[derive(Debug, Args)]
pub struct CodegraphIndexArgs {
    /// Workspace root to index (default: current directory).
    #[arg(long, value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// What to index: `all` (source + docs), `source` (code only), or
    /// `docs` (documents only).
    #[arg(long, value_enum, default_value_t = IndexKindArg::All)]
    pub kind: IndexKindArg,
}

pub async fn run_codegraph_command(cli: CodegraphCli) -> anyhow::Result<()> {
    match cli.subcommand {
        CodegraphSubcommand::Index(args) => run_index(args).await,
    }
}

async fn run_index(args: CodegraphIndexArgs) -> anyhow::Result<()> {
    let path = args
        .path
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let path_str = path.to_string_lossy().into_owned();
    let summary = CodegraphService::host_index(&path_str, args.kind.as_str()).await?;
    let kind = IndexKind::parse(args.kind.as_str());
    println!("codegraph index ({}) done for {}", kind.as_str(), summary.root);
    if kind.indexes_source() {
        println!(
            "  source: {} files, {} symbols, {} chains, {} calls (skipped {})",
            summary.files, summary.symbols, summary.chains, summary.calls, summary.skipped
        );
    }
    if kind.indexes_docs() {
        if summary.docs_configured {
            println!(
                "  docs: {} documents, {} nodes (failed {})",
                summary.docs, summary.doc_nodes, summary.docs_failed
            );
        } else {
            println!("  docs: no [docgraph] paths configured in .codegraph/config.toml — skipped");
        }
    }
    Ok(())
}
