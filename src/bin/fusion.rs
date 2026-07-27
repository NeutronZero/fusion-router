use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use fusion_router::devex::commands;
use fusion_router::release::bootstrap;
use fusion_router::release::gate::{GateContext, GateId};

#[derive(Parser)]
#[command(name = "fusion", about = "FusionRouter release governance tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand)]
    Gates(GatesCmd),
    #[command(subcommand)]
    Features(FeaturesCmd),
}

#[derive(Subcommand)]
enum GatesCmd {
    List,
    Check {
        #[arg(long)]
        gate: Option<String>,
        #[arg(long, default_value_t, value_enum)]
        format: OutputFormat,
    },
    Explain {
        id: String,
    },
}

#[derive(Subcommand)]
enum FeaturesCmd {
    List {
        #[arg(long, default_value_t, value_enum)]
        format: OutputFormat,
    },
}

#[derive(ValueEnum, Clone, Default)]
enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (runner, registry) = bootstrap::bootstrap(workspace_root.clone(), "HEAD");

    match cli.command {
        Commands::Gates(cmd) => match cmd {
            GatesCmd::List => {
                let output = commands::gates::list_gates(&runner);
                println!("{output}");
            }
            GatesCmd::Check { gate: _, format: _ } => {
                let context = GateContext {
                    workspace_root,
                    baseline_version: None,
                };
                let output = commands::gates::check_gates(&runner, &context).await;
                println!("{output}");
            }
            GatesCmd::Explain { id } => {
                let gate_id = GateId::from_str(&id).unwrap_or_else(|| {
                    panic!(
                        "Invalid gate ID: {id}. Valid IDs: SDK-1, RPL-1, UPG-1, DET-1"
                    )
                });
                let output = commands::gates::explain_gate(&runner, gate_id);
                println!("{output}");
            }
        },
        Commands::Features(cmd) => match cmd {
            FeaturesCmd::List { format: _ } => {
                let output = commands::features::list_features(&registry);
                println!("{output}");
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_help_contains_gates() {
        let mut cmd = Cli::command();
        let help = cmd.render_help().to_string();
        assert!(
            help.contains("gates"),
            "Help should mention 'gates' subcommand"
        );
        assert!(
            help.contains("features"),
            "Help should mention 'features' subcommand"
        );
    }

    #[test]
    fn test_parse_gates_list() {
        let cli = Cli::try_parse_from(&["fusion", "gates", "list"]).unwrap();
        assert!(matches!(cli.command, Commands::Gates(GatesCmd::List)));
    }

    #[test]
    fn test_parse_gates_check() {
        let cli = Cli::try_parse_from(&["fusion", "gates", "check"]).unwrap();
        assert!(matches!(cli.command, Commands::Gates(GatesCmd::Check { .. })));
    }

    #[test]
    fn test_parse_gates_check_with_gate() {
        let cli =
            Cli::try_parse_from(&["fusion", "gates", "check", "--gate", "SDK-1"]).unwrap();
        assert!(matches!(cli.command, Commands::Gates(GatesCmd::Check { .. })));
    }

    #[test]
    fn test_parse_gates_explain() {
        let cli = Cli::try_parse_from(&["fusion", "gates", "explain", "SDK-1"]).unwrap();
        assert!(matches!(cli.command, Commands::Gates(GatesCmd::Explain { .. })));
    }

    #[test]
    fn test_parse_features_list() {
        let cli = Cli::try_parse_from(&["fusion", "features", "list"]).unwrap();
        assert!(
            matches!(cli.command, Commands::Features(FeaturesCmd::List { .. }))
        );
    }
}
