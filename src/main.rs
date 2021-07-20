use askama::Template;
use eyre::{Result, WrapErr};
use log::{debug, info, trace, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::str::FromStr;
use structopt::StructOpt;

struct NameGen {
    i: usize,
}

impl NameGen {
    const fn new() -> Self {
        Self { i: 0 }
    }

    fn generate(&mut self) -> String {
        let new_name = format!("hook_{}", self.i);
        self.i += 1;
        new_name
    }
}

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
        #[structopt(short, long)]
        force: bool,
        #[structopt(long = "no-remove")]
        no_remove: bool,
    },
    /// Generate an example configuration file to the console
    Example,
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

#[derive(Deserialize, PartialEq, Eq, Debug, Clone)]
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

impl Hook {
    fn context(&self, name_gen: &mut NameGen) -> HookContext {
        let name = match &self.name {
            Some(n) => sanitise_name(n),
            None => name_gen.generate(),
        };

        let command = if self.pass_git_files {
            format!("{} $(git ls-files)", self.command.clone())
        } else {
            self.command.clone()
        };

        HookContext {
            name,
            original_name: self.name.clone(),
            command,
            background: self.background,
        }
    }
}

struct HookContext {
    name: String,
    // This field is read in the template
    #[allow(dead_code)]
    original_name: Option<String>,
    // This field is read in the template
    #[allow(dead_code)]
    command: String,
    background: bool,
}

#[derive(Deserialize, PartialEq, Eq, Debug, Clone)]
struct Config {
    hooks: Vec<Hook>,
}

impl Config {
    fn from_path(p: &'_ Path) -> Result<Self> {
        info!("reading configuration from {:?}", p);
        let s = std::fs::read_to_string(p).wrap_err("reading config file")?;
        trace!("config: {}", s);

        let config = toml::from_str(&s).wrap_err("parsing config file")?;
        Ok(config)
    }
}

#[derive(PartialEq, Eq, Debug, Clone)]
struct ConfigLocation {
    config: Config,
    path: PathBuf,
}

impl ConfigLocation {
    fn from_path(p: impl AsRef<Path>) -> Result<Self> {
        let p = p.as_ref();
        debug!("trying to load configuration from {:?}", p);
        let config = Config::from_path(p)?;
        Ok(Self {
            config,
            path: p.into(),
        })
    }

    fn global() -> Result<Self> {
        let config_path = match dirs::config_dir() {
            Some(d) => Ok(d.join("hookman").join("hookman.toml")),
            None => Err(eyre::eyre!("cannot find global config file")),
        }?;

        Self::from_path(config_path)
    }

    fn merge(&self, other: &ConfigLocation) -> Self {
        let mut out = self.clone();
        // XXX(srw) why does `extend` not work?
        for hook in &other.config.hooks {
            out.config.hooks.push(hook.clone());
        }
        out
    }
}

struct Generator {
    config: ConfigLocation,
    name_gen: NameGen,
}

impl Generator {
    fn new(config: ConfigLocation) -> Result<Self> {
        let name_gen = NameGen::new();
        Ok(Self { config, name_gen })
    }

    fn install(&mut self, dry_run: bool, force: bool, no_remove: bool) -> Result<()> {
        let hook_root_path = self
            .compute_root_hook_path()
            .wrap_err("calculating root hook path")?;

        if !no_remove {
            self.clear_hooks(&hook_root_path)
                .wrap_err("clearing existing hooks")?;
        } else {
            info!("not removing existing hooks because --no-remove was passed");
        }

        info!("installing hooks");
        let hooks_per_stage = self.hooks_per_stage();
        debug!("hooks per stage: {:?}", hooks_per_stage);
        for (stage, hooks) in hooks_per_stage {
            self.generate_hook(stage, hooks, &hook_root_path, dry_run, force)
                .wrap_err_with(|| format!("generating hook for stage {:?}", stage))?;
        }
        Ok(())
    }

    fn clear_hooks(&self, hook_root_path: &Path) -> Result<()> {
        info!("clearing out previous hooks");
        let mut remove_candidates = Vec::new();
        match hook_root_path.read_dir() {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.wrap_err("error reading directory entry")?;
                    if entry.file_type()?.is_file() {
                        remove_candidates.push(hook_root_path.join(entry.file_name()));
                    }
                }
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => {
                    info!("no hook directory found");
                    return Ok(());
                }
                _ => {
                    warn!("error reading hook directory: {:?}", e);
                    return Ok(());
                }
            },
        }

        for candidate in &remove_candidates {
            std::fs::remove_file(candidate)
                .wrap_err_with(|| format!("removing file {:?}", candidate))?;
        }

        Ok(())
    }

    fn compute_root_hook_path(&self) -> Result<PathBuf> {
        // Use git to find the root directory
        let output = process::Command::new("git")
            .args(&["rev-parse", "--git-dir"])
            .output()
            .wrap_err("spawning git command")?;

        if !output.status.success() {
            let stderr =
                std::str::from_utf8(&output.stderr).expect("reading command output as utf-8");
            return Err(eyre::eyre!("error running git: {}", stderr));
        }

        let git_dir = std::str::from_utf8(&output.stdout)
            .expect("reading command output as utf-8")
            .trim();

        if git_dir.is_empty() {
            return Err(eyre::eyre!("no git directory found"));
        }

        let hook_dir = PathBuf::from_str(git_dir)
            .expect("cannot fail")
            .join("hooks");

        Ok(hook_dir)
    }

    fn hooks_per_stage(&self) -> HashMap<Stage, Vec<Hook>> {
        let mut out = HashMap::new();
        for hook in &self.config.config.hooks {
            let entry = out.entry(hook.stage).or_insert_with(Vec::new);
            entry.push(hook.clone());
        }
        out
    }

    fn generate_hook(
        &mut self,
        stage: Stage,
        hooks: Vec<Hook>,
        hook_root_path: &Path,
        dry_run: bool,
        force: bool,
    ) -> Result<()> {
        let contents = self.generate_hook_contents(hooks)?;
        debug!("{:?} hook: {}", stage, contents);

        if dry_run {
            println!("would install {} script:", stage);
            println!("{}", contents);
        } else {
            let hook_path = self.compute_hook_path(hook_root_path, stage)?;
            debug!("writing hook to {:?}", hook_path);
            if hook_path.exists() && !force {
                return Err(eyre::eyre!(
                    "file {:?} exists and -f/--force not given",
                    &hook_path
                ));
            }
            self.write_file(&hook_path, &contents)
                .wrap_err("writing to file")?;
        }
        Ok(())
    }

    fn write_file(&self, path: &Path, contents: &str) -> Result<()> {
        // TODO(srw): how to do this on not(unix)?
        use std::os::unix::fs::OpenOptionsExt;

        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o770)
            .open(path)
            .wrap_err_with(|| format!("creating file {:?}", path))?;
        write!(&mut out, "{}", contents).wrap_err("writing file contents")?;

        Ok(())
    }

    fn generate_hook_contents(&mut self, hooks: Vec<Hook>) -> Result<String> {
        let hook_contexts = hooks
            .iter()
            .map(|h| h.context(&mut self.name_gen))
            .collect::<Vec<_>>();
        let template = HookTemplate::from(hook_contexts);
        template.render().wrap_err("generating template")
    }

    fn compute_hook_path(&self, hook_root_path: &Path, stage: Stage) -> Result<PathBuf> {
        let stub = match stage {
            Stage::PrePush => "pre-push",
            Stage::PostCommit => "post-commit",
            Stage::PreCommit => "pre-commit",
        };

        if !hook_root_path.is_dir() {
            debug!("hook dir does not exist, creating");
            std::fs::create_dir_all(&hook_root_path).wrap_err("creating hook directory")?;
        }

        Ok(hook_root_path.join(stub))
    }
}

#[derive(Serialize)]
struct Context {}

fn main() -> Result<()> {
    env_logger::init();
    color_eyre::install()?;

    let opts = Commands::from_args();
    debug!("options: {:?}", opts);
    match opts {
        Commands::Install {
            config: config_path,
            dry_run,
            force,
            no_remove,
        } => {
            let config = ConfigLocation::from_path(config_path)?;
            let config = match ConfigLocation::global() {
                Ok(global_config) => {
                    debug!("found global configuration");
                    global_config.merge(&config)
                }
                Err(_) => {
                    debug!("no global configuration found");
                    config
                }
            };

            let mut generator = Generator::new(config).unwrap();
            // XXX should removal be handled elsewhere?
            generator
                .install(dry_run, force, no_remove)
                .wrap_err("generating configuration")?;
        }
        Commands::Example => {
            let text = include_str!("../share/hookman.toml");
            println!("{}", text);
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
                    command: "ctags --tag-relative-yes -Rf.git/tags.$$ $(git ls-files)".to_string(),
                    background: true,
                    stage: Stage::PostCommit,
                    pass_git_files: false,
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

    #[test]
    fn compute_hook_path() {
        init_logger();
        let generator = empty_generator();

        let examples = &[
            (
                Stage::PreCommit,
                PathBuf::from_str(".git/hooks/pre-commit").unwrap(),
            ),
            (
                Stage::PrePush,
                PathBuf::from_str(".git/hooks/pre-push").unwrap(),
            ),
            (
                Stage::PostCommit,
                PathBuf::from_str(".git/hooks/post-commit").unwrap(),
            ),
        ];

        let hook_root_path = PathBuf::from_str(".git/hooks").unwrap();

        for (stage, expected) in examples {
            assert_eq!(
                generator
                    .compute_hook_path(&hook_root_path, *stage)
                    .unwrap(),
                *expected
            );
        }
    }

    #[test]
    fn name_gen() {
        init_logger();
        let mut n = NameGen::new();
        for _ in 0..10 {
            let _ = n.generate();
        }

        let res = n.generate();

        assert_eq!(res, "hook_10");
    }

    #[cfg(unix)]
    #[test]
    fn creating_file() {
        use std::os::unix::fs::MetadataExt;

        init_logger();
        let generator = empty_generator();

        let tdir = tempfile::tempdir().unwrap();
        let file_path = tdir.path().join("example.txt");

        generator.write_file(&file_path, "abc").unwrap();

        // check the file mode
        let mode = file_path.metadata().unwrap().mode();
        assert!(mode & 0o100 > 0, "checking 100, mode: {:o}", mode);
        assert!(mode & 0o010 > 0, "checking 010, mode: {:o}", mode);
    }

    fn empty_generator() -> Generator {
        let config = ConfigLocation {
            config: Config { hooks: Vec::new() },
            path: PathBuf::from_str("").unwrap(),
        };
        Generator::new(config).unwrap()
    }
}
