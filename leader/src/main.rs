use std::env;
use std::{fs::File, path::PathBuf};

use anyhow::Result;
use clap::Parser;
use cli::Command;
use common::prover_state::TableLoadStrategy;
use dotenvy::dotenv;
use ops::register;
use paladin::runtime::Runtime;
use proof_gen::types::PlonkyProofIntern;

use crate::utils::get_package_version;

mod cli;
mod http;
mod init;
mod jerigon;
mod stdio;
mod utils;

fn get_previous_proof(path: Option<PathBuf>) -> Result<Option<PlonkyProofIntern>> {
    if path.is_none() {
        return Ok(None);
    }

    let path = path.unwrap();
    let file = File::open(path)?;
    let des = &mut serde_json::Deserializer::from_reader(&file);
    let proof: PlonkyProofIntern = serde_path_to_error::deserialize(des)?;
    Ok(Some(proof))
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();
    init::tracing();

    if env::var("EVM_ARITHMETIZATION_PKG_VER").is_err() {
        let pkg_ver = get_package_version("evm_arithmetization")?;
        // Extract the major and minor version parts and append 'x' as the patch version
        if let Some((major_minor, _)) = pkg_ver.as_ref().and_then(|s| s.rsplit_once('.')) {
            let circuits_version = format!("{}.x", major_minor);
            // Set the environment variable for the evm_arithmetization package version
            env::set_var("EVM_ARITHMETIZATION_PKG_VER", circuits_version);
        } else {
            // Set to "NA" if version extraction fails
            env::set_var("EVM_ARITHMETIZATION_PKG_VER", "NA");
        }
    }

    let args = cli::Cli::parse();
    if let paladin::config::Runtime::InMemory = args.paladin.runtime {
        // If running in emulation mode, we'll need to initialize the prover
        // state here.
        args.prover_state_config
            .into_prover_state_manager()
            // Use the monolithic load strategy for the prover state when running in
            // emulation mode.
            .with_load_strategy(TableLoadStrategy::Monolithic)
            .initialize()?;
    }

    let runtime = Runtime::from_config(&args.paladin, register()).await?;

    match args.command {
        Command::Stdio {
            previous_proof,
            save_inputs_on_error,
        } => {
            let previous_proof = get_previous_proof(previous_proof)?;
            stdio::stdio_main(runtime, previous_proof, save_inputs_on_error).await?;
        }
        Command::Http {
            port,
            output_dir,
            save_inputs_on_error,
        } => {
            // check if output_dir exists, is a directory, and is writable
            let output_dir_metadata = std::fs::metadata(&output_dir);
            if output_dir_metadata.is_err() {
                // Create output directory
                std::fs::create_dir(&output_dir)?;
            } else if !output_dir.is_dir() || output_dir_metadata?.permissions().readonly() {
                panic!("output-dir is not a writable directory");
            }

            http::http_main(runtime, port, output_dir, save_inputs_on_error).await?;
        }
        Command::Jerigon {
            rpc_url,
            block_number,
            checkpoint_block_number,
            previous_proof,
            proof_output_path,
            save_inputs_on_error,
        } => {
            let previous_proof = get_previous_proof(previous_proof)?;

            jerigon::jerigon_main(
                runtime,
                &rpc_url,
                block_number,
                checkpoint_block_number,
                previous_proof,
                proof_output_path,
                save_inputs_on_error,
            )
            .await?;
        }
    }

    Ok(())
}
