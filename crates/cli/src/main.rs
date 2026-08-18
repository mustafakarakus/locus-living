use clap::{Parser, Subcommand, ValueEnum};
use homeai_common::{Paths, Scope, TokenStore};

#[derive(Parser)]
#[command(name = "homeai", about = "Home AI admin CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    Admin {
        #[command(subcommand)]
        cmd: AdminCommand,
    },
}

#[derive(Subcommand)]
enum AdminCommand {
    /// Create, list, rotate, or revoke scoped bearer tokens (UC-103).
    Token {
        #[command(subcommand)]
        cmd: TokenCommand,
    },
}

#[derive(Subcommand)]
enum TokenCommand {
    Create {
        #[arg(long)]
        id: String,
        #[arg(long = "scope", value_enum)]
        scopes: Vec<ScopeArg>,
    },
    List,
    Revoke {
        #[arg(long)]
        id: String,
    },
    Rotate {
        #[arg(long)]
        id: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ScopeArg {
    Read,
    Control,
    Admin,
}

impl From<ScopeArg> for Scope {
    fn from(s: ScopeArg) -> Self {
        match s {
            ScopeArg::Read => Scope::Read,
            ScopeArg::Control => Scope::Control,
            ScopeArg::Admin => Scope::Admin,
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("homeai: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let paths = Paths::from_env();
    paths.ensure_runtime_dirs()?;
    match cli.cmd {
        Command::Admin {
            cmd: AdminCommand::Token { cmd },
        } => token_cmd(&paths, cmd),
    }
}

fn token_cmd(paths: &Paths, cmd: TokenCommand) -> anyhow::Result<()> {
    let dir = paths.tokens_dir();
    let mut store = TokenStore::load(&dir)?;
    match cmd {
        TokenCommand::Create { id, scopes } => {
            let scopes: Vec<Scope> = if scopes.is_empty() {
                vec![Scope::Read]
            } else {
                scopes.into_iter().map(Scope::from).collect()
            };
            let rec = store.create(&id, scopes)?;
            println!("id={}", rec.id);
            println!("secret={}", rec.secret);
            println!(
                "scopes={}",
                rec.scopes
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            );
            eprintln!("store this secret; it is not shown again");
        }
        TokenCommand::List => {
            for rec in store.list() {
                let scopes = rec
                    .scopes
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                println!("{}\t{scopes}", rec.id);
            }
        }
        TokenCommand::Revoke { id } => {
            store.revoke(&id)?;
            println!("revoked {id}");
        }
        TokenCommand::Rotate { id } => {
            let rec = store.rotate(&id)?;
            println!("id={}", rec.id);
            println!("secret={}", rec.secret);
            eprintln!("previous secret is no longer valid");
        }
    }
    Ok(())
}
