//! `winthorpe data` — data directory / database info.

use anyhow::Result;

use crate::service;

use super::args::{Cli, DataAction};
use super::output;

pub fn dispatch(action: &DataAction, cli: &Cli) -> Result<()> {
    match action {
        DataAction::Info => info(cli),
    }
}

fn info(cli: &Cli) -> Result<()> {
    let info = service::get_data_info()?;
    output::print(cli, &info, |info| {
        format!(
            "Mode:     {}\nData dir: {}\nDatabase: {}",
            info.data_mode, info.data_dir, info.db_path
        )
    })
}
