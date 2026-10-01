use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use jevish::backend::{BrowserBackend, cdp::CdpBackend, process::ProcessBackend};
use jevish::contracts::*;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "jevish",
    version,
    about = "Model-agnostic browser decision pipeline"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, ValueEnum)]
enum BackendKind {
    Cdp,
    Process,
}

#[derive(Subcommand)]
enum Command {
    Snapshot {
        #[arg(long, value_enum, default_value = "cdp")]
        backend: BackendKind,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        adapter: Option<String>,
        #[arg(last = true)]
        adapter_args: Vec<String>,
    },
    Extract {
        #[arg(default_value = "-")]
        input: PathBuf,
    },
    Candidates {
        #[arg(default_value = "-")]
        input: PathBuf,
        #[arg(long)]
        operation: Operation,
    },
    Bind {
        instruction: String,
        #[arg(long)]
        operation: Option<Operation>,
    },
    Questions {
        #[arg(long)]
        binding: PathBuf,
        #[arg(long)]
        candidates: Option<PathBuf>,
    },
    Decide {
        #[arg(default_value = "-")]
        questions: PathBuf,
    },
    Gate {
        #[arg(long)]
        questions: PathBuf,
        #[arg(long)]
        decisions: PathBuf,
        #[arg(long)]
        candidates: PathBuf,
        #[arg(long)]
        binding: PathBuf,
        #[arg(long, default_value_t = 0.8)]
        probability: f64,
        #[arg(long, default_value_t = 0.2)]
        margin: f64,
    },
    Execute {
        #[arg(default_value = "-")]
        plan: PathBuf,
        #[arg(long, value_enum, default_value = "cdp")]
        backend: BackendKind,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        adapter: Option<String>,
        #[arg(last = true)]
        adapter_args: Vec<String>,
    },
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let mut data = String::new();
    if path == Path::new("-") {
        io::stdin().read_to_string(&mut data)?;
    } else {
        data = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
    }
    serde_json::from_str(&data).with_context(|| format!("invalid JSON in {}", path.display()))
}

fn output<T: serde::Serialize>(value: &T) -> Result<()> {
    serde_json::to_writer_pretty(io::stdout(), value)?;
    println!();
    Ok(())
}

fn backend(
    kind: BackendKind,
    endpoint: Option<String>,
    adapter: Option<String>,
    args: Vec<String>,
) -> Result<Box<dyn BrowserBackend>> {
    match kind {
        BackendKind::Cdp => Ok(Box::new(CdpBackend::connect(
            endpoint
                .as_deref()
                .context("--endpoint is required for CDP")?,
        )?)),
        BackendKind::Process => Ok(Box::new(ProcessBackend::new(
            adapter.context("--adapter is required for process backend")?,
            args,
        ))),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Snapshot {
            backend: kind,
            endpoint,
            adapter,
            adapter_args,
        } => output(&backend(kind, endpoint, adapter, adapter_args)?.snapshot()?),
        Command::Extract { input } => output(&jevish::extraction::extract(&read(&input)?)),
        Command::Candidates { input, operation } => {
            output(&jevish::candidates::candidates(&read(&input)?, operation))
        }
        Command::Bind {
            instruction,
            operation,
        } => output(&jevish::binding::bind(&instruction, operation)),
        Command::Questions {
            binding,
            candidates,
        } => {
            let binding: Binding = read(&binding)?;
            let candidates: Option<CandidateSet> =
                candidates.map(|path| read(&path)).transpose()?;
            output(&jevish::questions::build(&binding, candidates.as_ref()))
        }
        Command::Decide { questions } => output(&jevish::decision::heuristic(&read(&questions)?)),
        Command::Gate {
            questions,
            decisions,
            candidates,
            binding,
            probability,
            margin,
        } => output(&jevish::gate::build_plan(
            &read(&questions)?,
            &read(&decisions)?,
            &read(&candidates)?,
            &read(&binding)?,
            probability,
            margin,
        )?),
        Command::Execute {
            plan,
            backend: kind,
            endpoint,
            adapter,
            adapter_args,
        } => output(&backend(kind, endpoint, adapter, adapter_args)?.execute(&read(&plan)?)?),
    }
}
