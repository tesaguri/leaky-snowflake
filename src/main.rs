mod api;
mod run;
mod util;

use std::env;
use std::fs::File;
use std::io::BufReader;
use std::ops::ControlFlow;
use std::process::ExitCode;

use getopts::Options;

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
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
        "path to API credentials file (default: `credentials.json`)",
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

    let owned;
    let credentials = if let Some(s) = matches.opt_str("credentials") {
        owned = s;
        &owned
    } else {
        "credentials.json"
    };

    let k_ms = matches.opt_get_default("k", 1000)?;

    #[derive(serde::Deserialize)]
    struct Credentials {
        consumer_key: String,
        consumer_secret: String,
        access_token: String,
        access_token_secret: String,
    }
    let Credentials {
        consumer_key,
        consumer_secret,
        access_token,
        access_token_secret,
    } = serde_json::from_reader(BufReader::new(File::open(credentials)?))?;
    let token = oauth::Token::from_parts(
        consumer_key,
        consumer_secret,
        access_token,
        access_token_secret,
    );

    Ok(ControlFlow::Continue(run::Args {
        list_id,
        k_ms,
        token,
    }))
}

fn print_usage(program: &str, opts: &Options) {
    let brief = format!("Usage: {} [OPTIONS..] LIST_ID", program);
    print!("{}", opts.usage(&brief));
}
