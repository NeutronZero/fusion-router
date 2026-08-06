use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use fusion_router::devex::commands;
use fusion_router::events::consumers::{PersistentEventStoreProjection, TimelineProjection};
use fusion_router::events::projection::EventProjection;
use fusion_router::release::archive::{ArchiveBackend, FilesystemArchiveBackend};
use fusion_router::release::assessment::ReleaseAssessment;
use fusion_router::release::attestation::{AttestationBuilder, ReleaseAttestation};
use fusion_router::release::bootstrap;
use fusion_router::release::envelope::AttestationEnvelope;
#[allow(unused_imports)]
use fusion_router::release::evaluator::{EvaluationContext, PolicyEvaluation, PolicyEvaluator, ReleaseDecision};
use fusion_router::release::gate::{GateContext, GateId};
use fusion_router::release::policy::{load_policy_from_yaml, PolicyDefinition, ReleaseEnvironment};
use fusion_router::release::signing::{HmacSha256Signer, Signer};
use fusion_router::release::verifier::AttestationVerifier;
use fusion_router::release::waiver::{load_waivers_from_yaml, WaiverSet};

#[derive(Parser)]
#[command(name = "fusion", about = "FusionRouter release governance & runtime trace tool")]
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
    #[command(subcommand)]
    Trace(TraceCmd),
    #[command(subcommand)]
    Capability(CapabilityCmd),
}

#[derive(Subcommand)]
enum CapabilityCmd {
    New {
        name: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    Build {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value = "output")]
        output_dir: PathBuf,
    },
    Test {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    Publish {
        pkg_path: PathBuf,
        #[arg(long)]
        registry: String,
        #[arg(long)]
        key: Option<String>,
    },
    Dev {
        #[arg(long, default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value_t = 3030)]
        port: u16,
    },
    Inspect,
    Info,
    Logs,
    Config,
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
    Evaluate {
        #[arg(long, default_value = "production")]
        env: String,
        #[arg(long)]
        policy: Option<PathBuf>,
        #[arg(long)]
        waivers: Option<PathBuf>,
    },
    Attest {
        #[arg(long, default_value = "production")]
        env: String,
        #[arg(long)]
        output_dir: Option<PathBuf>,
    },
    VerifyAttestation {
        target: String,
    },
}

#[derive(Subcommand)]
enum FeaturesCmd {
    List {
        #[arg(long, default_value_t, value_enum)]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
enum TraceCmd {
    Timeline {
        execution_id: String,
        #[arg(long, default_value_t, value_enum)]
        format: OutputFormat,
    },
    Events {
        execution_id: String,
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
                        "Invalid gate ID: {id}. Valid IDs: SDK-1, RPL-1, UPG-1, DET-1, PLG-1, STR-1, PRV-1, CON-1"
                    )
                });
                let output = commands::gates::explain_gate(&runner, gate_id);
                println!("{output}");
            }
            GatesCmd::Evaluate { env, policy, waivers } => {
                let context = GateContext {
                    workspace_root,
                    baseline_version: None,
                };
                let policy_def = match policy {
                    Some(p) => load_policy_from_yaml(&p).unwrap_or_else(|e| panic!("{e}")),
                    None => PolicyDefinition::default_policy(),
                };
                let waiver_set = match waivers {
                    Some(w) => load_waivers_from_yaml(&w).unwrap_or_else(|e| panic!("{e}")),
                    None => WaiverSet::default(),
                };

                let gate_results = runner.run_all(&context).await;
                let rel_env = ReleaseEnvironment::from_str(&env);
                let eval_ctx = EvaluationContext::new(rel_env, policy_def, waiver_set);
                let evaluation = PolicyEvaluator::evaluate(&eval_ctx, &gate_results);

                let output = render_policy_evaluation(&evaluation);
                println!("{output}");
            }
            GatesCmd::Attest { env, output_dir } => {
                let context = GateContext {
                    workspace_root: workspace_root.clone(),
                    baseline_version: None,
                };
                let gate_results = runner.run_all(&context).await;
                let rel_env = ReleaseEnvironment::from_str(&env);
                let eval_ctx = EvaluationContext::new(rel_env.clone(), PolicyDefinition::default_policy(), WaiverSet::default());
                let evaluation = PolicyEvaluator::evaluate(&eval_ctx, &gate_results);

                let assessment = ReleaseAssessment::new(rel_env, evaluation, vec![]);
                let attestation = ReleaseAttestation::new(assessment);
                let canonical_bytes = AttestationBuilder::to_canonical_bytes(&attestation).unwrap();

                let signer = HmacSha256Signer::new("fusion-cli", &resolve_signing_key());
                let sig = signer.sign(&canonical_bytes).unwrap();
                let signed = fusion_router::release::signing::SignedAttestation { attestation, signature: sig };
                let envelope = AttestationEnvelope::new(signed);

                let archive_path = output_dir.unwrap_or_else(|| workspace_root.join(".fusion/attestations"));
                let archive = FilesystemArchiveBackend::new(archive_path);
                let stored_path = archive.store(&envelope).unwrap_or_else(|e| panic!("{e}"));

                println!("Signed Release Attestation Created");
                println!("Assessment ID: {}", envelope.signed_attestation.attestation.assessment.assessment_id);
                println!("Environment: {}", envelope.signed_attestation.attestation.assessment.environment);
                println!("Decision: {:?}", envelope.signed_attestation.attestation.assessment.policy_evaluation.decision);
                println!("Saved to: {}", stored_path.display());
            }
            GatesCmd::VerifyAttestation { target } => {
                let archive = FilesystemArchiveBackend::new(workspace_root.join(".fusion/attestations"));
                let envelope = if PathBuf::from(&target).exists() {
                    let content = std::fs::read_to_string(&target).unwrap();
                    serde_json::from_str::<AttestationEnvelope>(&content).unwrap()
                } else {
                    archive.load(&target).unwrap_or_else(|e| panic!("{e}"))
                };

                let signer = HmacSha256Signer::new("fusion-cli", &resolve_signing_key());
                let report = AttestationVerifier::verify(&envelope, &signer).unwrap_or_else(|e| panic!("{e}"));

                println!("Attestation Verification Report");
                println!("Schema Valid: {}", report.schema_valid);
                println!("Canonical Valid: {}", report.canonical_valid);
                println!("Signature Valid: {}", report.signature_valid);
                println!("Semantic Valid: {}", report.semantic_valid);
                println!("Summary: {}", report.summary);
            }
        },
        Commands::Features(cmd) => match cmd {
            FeaturesCmd::List { format: _ } => {
                let output = commands::features::list_features(&registry);
                println!("{output}");
            }
        },
        Commands::Trace(cmd) => match cmd {
            TraceCmd::Timeline { execution_id, format } => {
                let store = PersistentEventStoreProjection::new(workspace_root.join(".fusion/events"));
                let events = store.load_events(&execution_id).await.unwrap_or_default();

                let mut proj = TimelineProjection::new(execution_id);
                for env in &events {
                    let _ = proj.handle_event(env).await;
                }

                match format {
                    OutputFormat::Text => println!("{}", proj.model.render_ascii()),
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&proj.model).unwrap()),
                }
            }
            TraceCmd::Events { execution_id, format } => {
                let store = PersistentEventStoreProjection::new(workspace_root.join(".fusion/events"));
                let events = store.load_events(&execution_id).await.unwrap_or_default();

                match format {
                    OutputFormat::Text => {
                        println!("Events for execution: {execution_id} (count: {})", events.len());
                        for env in &events {
                            println!("[seq: {}] [schema: {}] {:?}", env.sequence_number, env.schema_version, env.payload);
                        }
                    }
                    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&events).unwrap()),
                }
            }
        },
        Commands::Capability(cmd) => match cmd {
            CapabilityCmd::New { name, path } => {
                if let Err(e) = commands::new::execute_new(&name, &path) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            CapabilityCmd::Build { path, output_dir } => {
                if let Err(e) = commands::build::execute_build(&path, &output_dir) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            CapabilityCmd::Test { path } => {
                if let Err(e) = commands::test::execute_test(&path) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            CapabilityCmd::Publish { pkg_path, registry, key } => {
                let rt = tokio::runtime::Runtime::new().unwrap();
                if let Err(e) = rt.block_on(
                    commands::publish::execute_publish(&pkg_path, &registry, key.as_deref())
                ) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            CapabilityCmd::Dev { path, port } => {
                if let Err(e) = commands::dev::execute_dev(&path, port) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
            CapabilityCmd::Inspect => commands::inspect::execute_inspect(),
            CapabilityCmd::Info => commands::info::execute_info(),
            CapabilityCmd::Logs => commands::logs::execute_logs(),
            CapabilityCmd::Config => commands::config_cmd::execute_config(),
        },
    }
}

/// Attestation signing is keyed HMAC-SHA256; the key comes from the
/// `FUSION_SIGNING_KEY` environment variable. Refuses to run without it
/// rather than silently falling back to a fabricated key.
fn resolve_signing_key() -> Vec<u8> {
    std::env::var("FUSION_SIGNING_KEY")
        .map(|k| k.into_bytes())
        .unwrap_or_else(|_| {
            eprintln!("error: FUSION_SIGNING_KEY must be set to sign or verify attestations");
            std::process::exit(1);
        })
}

pub fn render_policy_evaluation(eval: &PolicyEvaluation) -> String {
    let mut out = String::new();
    out.push_str("Release Policy Evaluation Report\n");
    out.push_str(&format!("Environment: {}\n", eval.environment));
    out.push_str(&format!("Decision: {:?}\n\n", eval.decision));

    out.push_str(&format!(
        "Summary: {} total gates, {} passed, {} required failed, {} waived, {} advisory failed.\n\n",
        eval.summary.total_gates,
        eval.summary.passed,
        eval.summary.required_failed,
        eval.summary.waived,
        eval.summary.advisory_failed
    ));

    out.push_str(&format!("Required Gates Passed: {:?}\n", eval.passed_gates));
    if !eval.waived_failures.is_empty() {
        out.push_str("Waived Failures:\n");
        for w in &eval.waived_failures {
            out.push_str(&format!("  [{}] {} (approved by: {})\n", w.waiver.id, w.gate, w.waiver.approved_by));
        }
    } else {
        out.push_str("Waived Failures: None\n");
    }

    if !eval.advisory_failures.is_empty() {
        out.push_str(&format!("Advisory Failures: {:?}\n", eval.advisory_failures));
    } else {
        out.push_str("Advisory Failures: None\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_help_contains_gates() {
        let mut cmd = Cli::command();
        let help = cmd.render_help().to_string();
        assert!(help.contains("gates"));
        assert!(help.contains("trace"));
    }

    #[test]
    fn test_parse_trace_timeline() {
        let cli = Cli::try_parse_from(["fusion", "trace", "timeline", "exec-123"]).unwrap();
        if let Commands::Trace(TraceCmd::Timeline { execution_id, .. }) = cli.command {
            assert_eq!(execution_id, "exec-123");
        } else {
            panic!("expected TraceCmd::Timeline");
        }
    }

    #[test]
    fn test_parse_trace_events() {
        let cli = Cli::try_parse_from(["fusion", "trace", "events", "exec-123"]).unwrap();
        if let Commands::Trace(TraceCmd::Events { execution_id, .. }) = cli.command {
            assert_eq!(execution_id, "exec-123");
        } else {
            panic!("expected TraceCmd::Events");
        }
    }

    #[test]
    fn test_cli_help_contains_capability() {
        let mut cmd = Cli::command();
        let help = cmd.render_help().to_string();
        assert!(help.contains("capability"));
    }

    #[test]
    fn test_parse_capability_new() {
        let cli = Cli::try_parse_from(["fusion", "capability", "new", "my-cap"]).unwrap();
        assert!(matches!(cli.command, Commands::Capability(CapabilityCmd::New { .. })));
    }

    #[test]
    fn test_parse_capability_build() {
        let cli = Cli::try_parse_from(["fusion", "capability", "build"]).unwrap();
        assert!(matches!(cli.command, Commands::Capability(CapabilityCmd::Build { .. })));
    }

    #[test]
    fn test_parse_capability_test() {
        let cli = Cli::try_parse_from(["fusion", "capability", "test"]).unwrap();
        assert!(matches!(cli.command, Commands::Capability(CapabilityCmd::Test { .. })));
    }

    #[test]
    fn test_parse_capability_publish() {
        let cli = Cli::try_parse_from([
            "fusion", "capability", "publish", "pkg.fusionpkg",
            "--registry", "http://localhost",
        ]).unwrap();
        assert!(matches!(cli.command, Commands::Capability(CapabilityCmd::Publish { .. })));
    }

    #[test]
    fn test_parse_capability_dev() {
        let cli = Cli::try_parse_from(["fusion", "capability", "dev"]).unwrap();
        assert!(matches!(cli.command, Commands::Capability(CapabilityCmd::Dev { .. })));
    }

    #[test]
    fn test_parse_capability_inspect() {
        let cli = Cli::try_parse_from(["fusion", "capability", "inspect"]).unwrap();
        assert!(matches!(cli.command, Commands::Capability(CapabilityCmd::Inspect)));
    }
}
