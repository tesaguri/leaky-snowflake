mod api;
mod run;
mod twurlrc;
mod util;

use std::env;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::ops::ControlFlow;
use std::process::ExitCode;

use anyhow::Context;
use getopts::Options;

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    tracing_subscriber::fmt::init();
    let args = match process_args(env::args_os())? {
        ControlFlow::Continue(args) => args,
        ControlFlow::Break(code) => return Ok(code),
    };
    run::run(args).await.and(Ok(ExitCode::SUCCESS))
}

fn process_args(mut args: env::ArgsOs) -> anyhow::Result<ControlFlow<ExitCode, run::Args>> {
    let program = args.next().unwrap();

    let mut opts = Options::new();
    opts.optopt(
        "",
        "credentials",
        "path to API credentials file (default: reads from `$HOME/.twurlrc` if any)",
        "FILE",
    );
    opts.optopt(
        "k",
        "",
        "assume the `k` value to be MILLIS ms (default: 1000)",
        "MILLIS",
    );
    opts.optflag("h", "help", "print this help");

    let matches = opts.parse(args)?;

    if matches.opt_present("h") {
        let program = program.to_string_lossy();
        print_usage(&program, &opts);
        return Ok(ControlFlow::Break(ExitCode::SUCCESS));
    }

    let list_id = if let [ref s] = *matches.free {
        s.parse()?
    } else {
        let program = program.to_string_lossy();
        println!("{}: missing LIST_ID argument", program);
        print_usage(&program, &opts);
        return Ok(ControlFlow::Break(ExitCode::FAILURE));
    };

    let k_ms = matches.opt_get_default("k", 1000)?;

    let bearer_token = if let Some(credentials) = matches.opt_str("credentials") {
        #[derive(serde::Deserialize)]
        struct Credentials {
            bearer_token: String,
        }
        let Credentials { bearer_token } =
            serde_json::from_reader(BufReader::new(File::open(credentials)?))?;
        bearer_token
    } else if let Some(f) = dirs::home_dir()
        .and_then(|mut home| {
            home.push(".twurlrc");
            match File::open(&home) {
                Ok(f) => Some(Ok(f)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => Some(Err(e)),
            }
        })
        .transpose()?
    {
        let profile: twurlrc::DefaultProfile = serde_yaml::from_reader(f)?;
        tracing::info!(
            profile.username,
            "Using default credentials from `.twurlrc`"
        );
        profile.bearer_token
    } else {
        let program = program.to_string_lossy();
        println!("{}: missing `--credential` option and `.twurlrc`", program);
        print_usage(&program, &opts);
        return Ok(ControlFlow::Break(ExitCode::FAILURE));
    };

    let bearer = format!("Bearer {}", bearer_token)
        .try_into()
        .context("Invalid bearer value")?;

    Ok(ControlFlow::Continue(run::Args {
        list_id,
        k_ms,
        bearer,
    }))
}

fn print_usage(program: &str, opts: &Options) {
    let brief = format!("Usage: {} [OPTIONS..] LIST_ID", program);
    print!("{}", opts.usage(&brief));
}
