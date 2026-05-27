mod agent;
mod cli;
mod hooks;
mod implementer;
mod llm;
mod mediator;
mod memory;
mod queue;
mod session;
mod spec;

use std::env;

fn print_usage() {
    println!("spec — version control and coordination system for AI agents");
    println!();
    println!("USAGE:");
    println!("  spec <command> [arguments]");
    println!();
    println!("COMMANDS:");
    println!("  init                       Create .spec folder, begin tracking .spec files");
    println!("  propose <file> \"<intent>\" [--knowledge \"<basis>\"]  Agent proposes a spec change based on the current source file");
    println!("  respond <file>             Agent responds to a proposal; may add context before taking a stance");
    println!("  concede <file>             Agent updates or withdraws their position");
    println!("  agree <file> [--solo]      Agent signs off; --solo locks without requiring a second agent");
    println!("  clarify <file>             Mediator surfaces contradictions between competing proposals");
    println!("  reframe <file>             Mediator finds common ground to help agents resolve disagreement");
    println!("  build <file>               Implementer writes code from agreed spec");
    println!("  test <file>                Run tests against the build");
    println!("  release <file> <env>       Promote build to an environment");
    println!("  status                     Observe current project state");
    println!("  queue add <file> \"<intent>\" [--after <id>...]  Add a task to the work queue");
    println!("  queue list                 Show all tasks and their status");
    println!("  queue done <id>            Mark a task complete manually");
    println!("  next                       Claim the next unblocked task (agents use this)");
    println!("  reset <file>               Clear a session so a fresh proposal can be made");
    println!("  state <file>               Machine-readable session status for polling (no LLM)");
    println!("  wait [<file>] <status>      Block until any (or a specific) session reaches a target status, then exit");
    println!("  watch <file>               Block and emit STATUS lines when session changes (use in mediator/implementer terminals)");
    println!("  log <file>                 Full session message history for a spec");
    println!("  lessons                    View the lesson graph");
    println!("  install-skills [--target <dir>]  Install skills (defaults to ~/.codex/skills/ or ~/.claude/skills/)");
    println!("  run <role> [name] --with <claude|codex>  Launch an AI tool as a specific spec role");
    println!();
    println!("ENVIRONMENT VARIABLES:");
    println!("  SPEC_PROVIDER              LLM provider: anthropic, openai, ollama, claudecode, codex (overrides config)");
    println!("  SPEC_API_KEY               API key for the provider (not required for claudecode or codex)");
    println!("  SPEC_MODEL                 Model to use (overrides config, e.g. claude-opus-4-7)");
    println!("  SPEC_AGENT_ID              Stable agent identity required for propose/respond/concede/agree");
    println!("  SPEC_ROLE                  Role enforcement: agent, proposer, mediator, implementer (set automatically by spec run)");
    println!();
    println!("EXAMPLES:");
    println!("  spec init");
    println!("  spec propose src/auth.spec \"add JWT token validation\"");
    println!("  SPEC_AGENT_ID=agent-2 spec respond src/auth.spec");
    println!("  spec agree src/auth.spec");
    println!("  spec build src/auth.spec");
}

fn check_role_allowed(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    let role = std::env::var("SPEC_ROLE").unwrap_or_default();
    if role.is_empty() {
        return Ok(());
    }
    let allowed: &[&str] = match role.as_str() {
        "agent" | "proposer" => &["propose", "respond", "concede", "agree"],
        "mediator"           => &["clarify", "reframe"],
        "implementer"        => &["build", "test", "release", "status"],
        "orchestrator"       => &["queue", "next", "status"],
        _ => return Err(format!(
            "Unknown SPEC_ROLE '{}'. Valid roles: agent, proposer, mediator, implementer, orchestrator", role
        ).into()),
    };
    if !allowed.contains(&command) {
        return Err(format!(
            "SPEC_ROLE={} cannot run 'spec {}'. Allowed commands for this role: {}",
            role, command, allowed.join(", ")
        ).into());
    }
    Ok(())
}

fn require_spec_initialized() -> Result<(), Box<dyn std::error::Error>> {
    if !std::path::Path::new(".spec").exists() {
        return Err(
            "Not a spec project. Run 'spec init' first to initialize.".into()
        );
    }
    Ok(())
}

fn load_provider() -> Result<Box<dyn llm::LlmProvider>, Box<dyn std::error::Error>> {
    let config = spec::load_config()?;
    llm::build_provider(&config)
}

fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::Path;

    let spec_dir = Path::new(".spec");
    if spec_dir.exists() {
        println!("Spec project already initialized.");
        return Ok(());
    }

    // Create directory structure
    std::fs::create_dir_all(".spec/sessions")?;
    std::fs::create_dir_all(".spec/lessons")?;

    // Write default config
    let config = spec::Config::default();
    spec::save_config(&config)?;

    // Ensure global lesson store exists at ~/.spec/lessons.json
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let global_spec_dir = std::path::Path::new(&home).join(".spec");
    std::fs::create_dir_all(&global_spec_dir)?;
    let lessons_file = global_spec_dir.join("lessons.json");
    if !lessons_file.exists() {
        let graph = memory::LessonGraph::default();
        memory::save_lessons(&graph)?;
    }

    // Create hooks directory with example scripts
    hooks::init_hooks()?;

    println!("Initialized spec project");
    println!("  .spec/config.json     — LLM provider configuration");
    println!("  .spec/sessions/       — session logs per spec file");
    println!("  .spec/hooks/          — lifecycle hook scripts (*.example to get started)");
    println!("  ~/.spec/lessons.json  — global lesson graph (shared across all projects and branches)");
    println!();
    cli::try_install_skills();

    println!();
    println!("Next steps:");
    println!("  Create a spec file:  spec propose <file> \"<your intent>\"");
    println!("  Set your provider:   export SPEC_PROVIDER=anthropic  (or claudecode / codex for subscription-based use)");
    println!("  Set your API key:    export SPEC_API_KEY=<your-key>  (not needed for claudecode or codex)");

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let command = args[1].as_str();

    let result = match command {
        "init" => cmd_init(),

        "propose" => {
            if args.len() < 4 {
                eprintln!("Usage: spec propose <file> \"<intent>\" [--knowledge \"<knowledge>\"]");
                std::process::exit(1);
            }
            let file = &args[2];
            let rest = &args[3..];
            let knowledge_pos = rest.iter().position(|a| a == "--knowledge");
            let knowledge: Option<String> = knowledge_pos.and_then(|i| rest.get(i + 1)).map(|s| s.clone());
            let intent_parts: Vec<&str> = rest.iter().enumerate()
                .filter(|(i, _)| Some(*i) != knowledge_pos && knowledge_pos.map(|k| *i != k + 1).unwrap_or(true))
                .map(|(_, s)| s.as_str())
                .collect();
            let intent = intent_parts.join(" ");
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("propose")?;
                require_spec_initialized()?;
                let provider = load_provider()?;
                agent::propose(file, &intent, knowledge.as_deref(), provider.as_ref())
            })()
        }

        "respond" => {
            if args.len() < 3 {
                eprintln!("Usage: spec respond <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("respond")?;
                require_spec_initialized()?;
                let provider = load_provider()?;
                agent::respond(file, provider.as_ref())
            })()
        }

        "concede" => {
            if args.len() < 3 {
                eprintln!("Usage: spec concede <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("concede")?;
                require_spec_initialized()?;
                let provider = load_provider()?;
                agent::concede(file, provider.as_ref())
            })()
        }

        "agree" => {
            if args.len() < 3 {
                eprintln!("Usage: spec agree <file> [--solo]");
                std::process::exit(1);
            }
            let file = &args[2];
            let solo = args.iter().any(|a| a == "--solo");
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("agree")?;
                require_spec_initialized()?;
                let provider = load_provider()?;
                agent::agree(file, solo, provider.as_ref())
            })()
        }

        "clarify" => {
            if args.len() < 3 {
                eprintln!("Usage: spec clarify <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("clarify")?;
                require_spec_initialized()?;
                let provider = load_provider()?;
                mediator::clarify(file, provider.as_ref())
            })()
        }

        "reframe" => {
            if args.len() < 3 {
                eprintln!("Usage: spec reframe <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("reframe")?;
                require_spec_initialized()?;
                let provider = load_provider()?;
                mediator::reframe(file, provider.as_ref())
            })()
        }

        "build" => {
            if args.len() < 3 {
                eprintln!("Usage: spec build <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("build")?;
                require_spec_initialized()?;
                let provider = load_provider()?;
                implementer::build(file, provider.as_ref())
            })()
        }

        "test" => {
            if args.len() < 3 {
                eprintln!("Usage: spec test <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("test")?;
                require_spec_initialized()?;
                implementer::test(file)
            })()
        }

        "release" => {
            if args.len() < 4 {
                eprintln!("Usage: spec release <file> <env>");
                std::process::exit(1);
            }
            let file = &args[2];
            let env_target = &args[3];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                check_role_allowed("release")?;
                require_spec_initialized()?;
                implementer::release(file, env_target)
            })()
        }

        "status" => {
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                cli::status()
            })()
        }

        "state" => {
            if args.len() < 3 {
                eprintln!("Usage: spec state <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                cli::state(file)
            })()
        }

        "queue" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "add" => {
                    if args.len() < 5 {
                        eprintln!("Usage: spec queue add <file> \"<intent>\" [--after <id>...]");
                        std::process::exit(1);
                    }
                    let file = &args[3];
                    let intent = &args[4];
                    let depends_on: Vec<String> = {
                        let mut out = Vec::new();
                        let mut i = 5;
                        while i < args.len() {
                            if args[i] == "--after" {
                                i += 1;
                                if i < args.len() { out.push(args[i].clone()); }
                            }
                            i += 1;
                        }
                        out
                    };
                    (|| -> Result<(), Box<dyn std::error::Error>> {
                        require_spec_initialized()?;
                        cli::queue_add(file, intent, depends_on)
                    })()
                }
                "list" => {
                    (|| -> Result<(), Box<dyn std::error::Error>> {
                        require_spec_initialized()?;
                        cli::queue_list()
                    })()
                }
                "done" => {
                    if args.len() < 4 {
                        eprintln!("Usage: spec queue done <task-id>");
                        std::process::exit(1);
                    }
                    (|| -> Result<(), Box<dyn std::error::Error>> {
                        require_spec_initialized()?;
                        cli::queue_done(&args[3])
                    })()
                }
                _ => {
                    eprintln!("Unknown queue subcommand '{}'. Use: add, list, done", sub);
                    std::process::exit(1);
                }
            }
        }

        "next" => {
            let agent_id = std::env::var("SPEC_AGENT_ID").unwrap_or_else(|_| "agent".to_string());
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                cli::next(&agent_id)
            })()
        }

        "reset" => {
            if args.len() < 3 {
                eprintln!("Usage: spec reset <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                cli::reset(file)
            })()
        }

        "wait" => {
            // spec wait <status> [--timeout <secs>]
            // spec wait <file> <status> [--timeout <secs>]
            if args.len() < 3 {
                eprintln!("Usage: spec wait [<file>] <status> [--timeout <secs>]");
                eprintln!("  status: STUCK, LOCKED, WAITING_FOR_REPLY, WAITING_FOR_AGREE, NO_SESSION");
                std::process::exit(1);
            }
            let timeout: u64 = args.iter()
                .position(|a| a == "--timeout")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok())
                .unwrap_or(30);
            let positional: Vec<&str> = {
                let mut skip_next = false;
                let mut out = Vec::new();
                for a in &args[2..] {
                    if skip_next { skip_next = false; continue; }
                    if a == "--timeout" { skip_next = true; continue; }
                    out.push(a.as_str());
                }
                out
            };
            let (file, target) = if positional.len() >= 2 {
                (Some(positional[0]), positional[1])
            } else if positional.len() == 1 {
                (None, positional[0])
            } else {
                eprintln!("Usage: spec wait [<file>] <status> [--timeout <secs>]");
                std::process::exit(1);
            };
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                cli::wait(file, target, timeout)
            })()
        }

        "watch" => {
            if args.len() < 3 {
                eprintln!("Usage: spec watch <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                cli::watch(file)
            })()
        }

        "log" => {
            if args.len() < 3 {
                eprintln!("Usage: spec log <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                cli::log(file)
            })()
        }

        "lessons" => {
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                cli::lessons()
            })()
        }

        "install-skills" => {
            let target = args.iter().position(|a| a == "--target")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());
            cli::install_skills(target)
        }

        "run" => {
            if args.len() < 3 {
                eprintln!("Usage: spec run <role> [name] --with <claude|codex>");
                std::process::exit(1);
            }
            let role = args[2].as_str();
            let with_pos = args.iter().position(|a| a == "--with");
            let runner = with_pos
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("claude");
            // name is any arg before --with that isn't the role
            let name = args.get(3)
                .filter(|a| *a != "--with" && with_pos.map(|i| 3 < i).unwrap_or(true))
                .map(|s| s.as_str());
            cli::run(role, name, runner)
        }

        "--help" | "-h" | "help" => {
            print_usage();
            Ok(())
        }

        unknown => {
            eprintln!("Unknown command: {}", unknown);
            eprintln!("Run 'spec --help' for usage.");
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
