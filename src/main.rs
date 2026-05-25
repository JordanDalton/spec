mod agent;
mod cli;
mod hooks;
mod implementer;
mod llm;
mod mediator;
mod memory;
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
    println!("  propose <file> \"<intent>\"  Agent proposes a spec change");
    println!("  respond <file>             Agent responds to an existing proposal");
    println!("  concede <file>             Agent updates or withdraws their position");
    println!("  agree <file>               Agent signs off on the current spec state");
    println!("  clarify <file>             Mediator surfaces a contradiction");
    println!("  reframe <file>             Mediator reframes the disagreement");
    println!("  build <file>               Implementer writes code from agreed spec");
    println!("  test <file>                Run tests against the build");
    println!("  release <file> <env>       Promote build to an environment");
    println!("  status                     Observe current project state");
    println!("  log <file>                 Full session message history for a spec");
    println!("  lessons                    View the lesson graph");
    println!();
    println!("ENVIRONMENT VARIABLES:");
    println!("  SPEC_PROVIDER              LLM provider: anthropic, openai, ollama (overrides config)");
    println!("  SPEC_API_KEY               API key for the provider (overrides provider-specific keys)");
    println!("  SPEC_MODEL                 Model to use (overrides config, e.g. claude-opus-4-7)");
    println!("  SPEC_AGENT_ID              Override agent identity (default: hostname:pid)");
    println!();
    println!("EXAMPLES:");
    println!("  spec init");
    println!("  spec propose src/auth.spec \"add JWT token validation\"");
    println!("  SPEC_AGENT_ID=agent-2 spec respond src/auth.spec");
    println!("  spec agree src/auth.spec");
    println!("  spec build src/auth.spec");
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
    println!("Next steps:");
    println!("  Create a spec file:  spec propose <file> \"<your intent>\"");
    println!("  Set your provider:   export SPEC_PROVIDER=anthropic");
    println!("  Set your API key:    export SPEC_API_KEY=<your-key>");

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
                eprintln!("Usage: spec propose <file> \"<intent>\"");
                std::process::exit(1);
            }
            let file = &args[2];
            let intent = args[3..].join(" ");
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                let provider = load_provider()?;
                agent::propose(file, &intent, provider.as_ref())
            })()
        }

        "respond" => {
            if args.len() < 3 {
                eprintln!("Usage: spec respond <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
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
                require_spec_initialized()?;
                let provider = load_provider()?;
                agent::concede(file, provider.as_ref())
            })()
        }

        "agree" => {
            if args.len() < 3 {
                eprintln!("Usage: spec agree <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
                require_spec_initialized()?;
                let provider = load_provider()?;
                agent::agree(file, provider.as_ref())
            })()
        }

        "clarify" => {
            if args.len() < 3 {
                eprintln!("Usage: spec clarify <file>");
                std::process::exit(1);
            }
            let file = &args[2];
            (|| -> Result<(), Box<dyn std::error::Error>> {
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
