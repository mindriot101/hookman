use eyre::{Result, WrapErr};
use log::{debug, info, trace};
use serde::Deserialize;
use std::path::Path;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
enum Commands {
    /// Installs git hooks
    Install {
        /// Path of the configuration file.
        #[structopt(short, long, default_value = ".hookman.toml")]
        config: String,
    },
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
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

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct Hook {
    name: Option<String>,
    command: String,
    #[serde(default)]
    stage: Stage,
    #[serde(default)]
    background: bool,
}

#[derive(Deserialize, PartialEq, Eq, Debug)]
struct Config {
    hooks: Vec<Hook>,
}

impl Config {
    fn from_path(p: impl AsRef<Path>) -> Result<Self> {
        let p = p.as_ref();
        info!("reading configuration from {:?}", p);
        let s = std::fs::read_to_string(p).wrap_err("reading config file")?;
        trace!("config: {}", s);

        let config = toml::from_str(&s).wrap_err("parsing config file")?;
        Ok(config)
    }
}

fn main() {
    env_logger::init();
    let opts = Commands::from_args();
    debug!("options: {:?}", opts);
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
        let config = Config::from_path(path).unwrap();

        assert_eq!(config.hooks.len(), 3);

        assert_eq!(
            config.hooks,
            vec![
                Hook {
                    name: Some("Test".to_string()),
                    command: "pytest".to_string(),
                    stage: Stage::PrePush,
                    background: false,
                },
                Hook {
                    name: Some("Generate hooks".to_string()),
                    command: "ctags --tag-relative-yes -Rf.git/tags.$$ $(git ls-files)".to_string(),
                    background: true,
                    stage: Stage::PostCommit,
                },
                Hook {
                    name: Some("Lint".to_string()),
                    command: "pylint".to_string(),
                    background: false,
                    stage: Stage::PreCommit,
                }
            ],
        );
    }
}
