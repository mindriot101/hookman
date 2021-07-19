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

fn main() {
    let opts = Commands::from_args();
    dbg!(opts);
}
