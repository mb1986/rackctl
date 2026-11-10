//! `rackctl trace`: follows the cables from an endpoint.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Args;
use rackctl_core::wiring::endpoint_name;

use crate::paths::Locations;
use crate::{CONFIG_ERROR, load_setup};

#[derive(Debug, Args)]
pub struct TraceArgs {
    /// The endpoint, such as `srv01:mgmt`, `pdu:8` or `patch-32:15`
    endpoint: String,
}

/// Runs `trace`. `config` is the configuration named with `-c`, if any.
pub fn run(
    args: &TraceArgs,
    config: Option<PathBuf>,
    locations: &Locations,
) -> io::Result<ExitCode> {
    let mut err = io::stderr();
    let Some(setup) = load_setup(config, locations, &mut err)? else {
        return Ok(ExitCode::from(CONFIG_ERROR));
    };
    let layout = &setup.config.layout;
    let Some(cabling) = &setup.config.cabling else {
        writeln!(err, "rackctl: there is no wiring: the configuration has no `wiring` node")?;
        return Ok(ExitCode::from(CONFIG_ERROR));
    };
    let socket = match cabling.find(&args.endpoint, layout, &setup.catalog) {
        Ok(socket) => socket,
        Err(problem) => {
            let help = problem.help().map_or_else(String::new, |help| format!("; {help}"));
            writeln!(err, "rackctl: {}{help}", problem.message())?;
            return Ok(ExitCode::FAILURE);
        }
    };

    let name = |socket| endpoint_name(cabling.endpoint(socket), layout, &setup.catalog);
    let path: Vec<String> = cabling.path(socket).into_iter().map(name).collect();
    let mut out = io::stdout();
    if let [alone] = path.as_slice() {
        writeln!(out, "{alone} is not connected")?;
    } else {
        writeln!(out, "{}", path.join(" -> "))?;
    }
    Ok(ExitCode::SUCCESS)
}
