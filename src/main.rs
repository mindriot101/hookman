use askama::Template;
use eyre::{Result, WrapErr};
use log::{debug, info, trace};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use uuid::Uuid;

#[derive(Template)]
#[template(path = "hook.sh", escape = "none")]
struct HookTemplate {
    hooks: Vec<HookContext>,
}

impl<'a> From<Vec<HookContext>> for HookTemplate {
    fn from(hooks: Vec<HookContext>) -> HookTemplate {
        HookTemplate { hooks }
    }
}

#[derive(Debug, StructOpt)]
enum Commands {
    /// Installs git hooks
    Install {
        /// Path of the configuration file.
        #[structopt(short, long, default_value = ".hookman.toml")]
        config: String,
        #[structopt(short = "n", long)]
        dry_run: bool,
    },
}

#[derive(Deserialize, PartialEq, Eq, Debug, Hash, Clone, Copy)]
enum Stage {
    #[serde(rename = "pre-push")]
    PrePush,
    #[serde(rename = "post-commit")]
    PostCommit,
    #[serde(rename = "pre-commit")]
    PreCommit,
}

impl Default for Stage {
    fn default() -> Stage {
        Stage::PreCommit
    }
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Stage::PrePush => f.write_str("pre-push"),
            Stage::PostCommit => f.write_str("post-commit"),
            Stage::PreCommit => f.write_str("pre-commit"),
        }
    }
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct Hook {
    name: Option<String>,
    command: String,
    #[serde(default)]
    stage: Stage,
    #[serde(default)]
    background: bool,
    #[serde(default)]
    pass_git_files: bool,
}

fn sanitise_name(n: &str) -> String {
    n.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

fn random_name() -> String {
    Uuid::new_v4().to_string()
}

impl Hook {
    fn context(&self) -> HookContext {
        let name = match &self.name {
            Some(n) => sanitise_name(n),
            None => random_name(),
        };

        HookContext {
            name,
            command: self.command.clone(),
        }
    }
}

struct HookContext {
    name: String,
    command: String,
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct Config {
    hooks: Vec<Hook>,
}

impl Config {
    fn from_path<'a>(p: &'a Path) -> Result<Self> {
        info!("reading configuration from {:?}", p);
        let s = std::fs::read_to_string(p).wrap_err("reading config file")?;
        trace!("config: {}", s);

        let config = toml::from_str(&s).wrap_err("parsing config file")?;
        Ok(config)
    }
}

#[derive(PartialEq, Eq, Debug)]
struct ConfigLocation {
    config: Config,
    path: PathBuf,
}

impl ConfigLocation {
    fn from_path(p: impl AsRef<Path>) -> Result<Self> {
        let p = p.as_ref();
        let config = Config::from_path(p)?;
        Ok(Self {
            config,
            path: p.into(),
        })
    }
}

struct Generator {
    config: ConfigLocation,
}

impl Generator {
    fn new(config: ConfigLocation) -> Result<Self> {
        // can we make this a lazy static?
        Ok(Self { config })
    }

    fn install(&self, dry_run: bool) -> Result<()> {
        info!("installing hooks");
        let hooks_per_stage = self.hooks_per_stage();
        debug!("hooks per stage: {:?}", hooks_per_stage);
        for (stage, hooks) in hooks_per_stage {
            self.generate_hook(stage, hooks, dry_run)
                .wrap_err_with(|| format!("generating hook for stage {:?}", stage))?;
        }
        Ok(())
    }

    fn hooks_per_stage<'a>(&'a self) -> HashMap<Stage, Vec<&'a Hook>> {
        let mut out = HashMap::new();
        for hook in &self.config.config.hooks {
            let entry = out.entry(hook.stage).or_insert_with(|| Vec::new());
            entry.push(hook);
        }
        out
    }

    fn generate_hook<'a>(
        &'a self,
        stage: Stage,
        hooks: Vec<&'a Hook>,
        dry_run: bool,
    ) -> Result<()> {
        let contents = self.generate_hook_contents(stage, hooks)?;
        debug!("{:?} hook: {}", stage, contents);

        if dry_run {
            eprintln!("would install {} script:", stage);
            eprintln!("{}", contents);
        } else {
            let hook_path = self.compute_hook_path(stage);
        }
        todo!()
    }

    fn generate_hook_contents(&self, stage: Stage, hooks: Vec<&'_ Hook>) -> Result<String> {
        let hook_contexts = hooks.iter().map(|h| h.context()).collect::<Vec<_>>();
        let template = HookTemplate::from(hook_contexts);
        template.render().wrap_err("generating template")
    }

    fn compute_hook_path(&self, stage: Stage) -> PathBuf {
        todo!("compute_hook_path")
        // match stage {
        //     Stage::PrePush => PathBuf,
        // }
    }
}

#[derive(Serialize)]
struct Context {}

fn main() -> Result<()> {
    env_logger::init();
    let opts = Commands::from_args();
    debug!("options: {:?}", opts);
    match opts {
        Commands::Install {
            config: config_path,
            dry_run: dry_run,
        } => {
            let config = ConfigLocation::from_path(config_path)?;
            let generator = Generator::new(config).unwrap();
            generator
                .install(dry_run)
                .wrap_err("generating configuration")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_logger() {
        let _ = env_logger::try_init();
    }

    #[test]
    fn parse_config() {
        init_logger();
        let path = "share/hookman.toml";
        let config = ConfigLocation::from_path(path).unwrap();

        assert_eq!(
            config.config.hooks,
            vec![
                Hook {
                    name: Some("Test".to_string()),
                    command: "pytest".to_string(),
                    stage: Stage::PrePush,
                    background: false,
                    pass_git_files: false,
                },
                Hook {
                    name: Some("Generate hooks".to_string()),
                    command: "ctags --tag-relative-yes -Rf.git/tags.$$".to_string(),
                    background: true,
                    stage: Stage::PostCommit,
                    pass_git_files: true,
                },
                Hook {
                    name: Some("Lint".to_string()),
                    command: "pylint".to_string(),
                    background: false,
                    stage: Stage::PreCommit,
                    pass_git_files: false,
                }
            ],
        );
    }
}
