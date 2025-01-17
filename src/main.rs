use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use clap::Parser;
use config::Config;
use config::File;
use i3ipc_jl::reply::Output;
use i3ipc_jl::I3Connection;
use log::debug;
use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;
use tempfile::NamedTempFile;

#[derive(Parser)]
struct Cli {
    /// Configuration file
    #[arg(short, long)]
    config: String,
}

fn get_outputs() -> Result<Vec<Output>, String> {
    let mut connection =
        I3Connection::connect().map_err(|e| format!("Failed to connect to I3/Sway IPC: {e}"))?;
    let outputs = connection
        .get_outputs()
        .map_err(|e| format!("Could not get display outputs: {e}"))?;

    Ok(outputs.outputs)
}

fn run_grim(output: &Output, temp: &NamedTempFile) {
    let x_min = output.rect.0;
    let y_min = output.rect.1;
    let x_max = output.rect.2;
    let y_max = output.rect.3;

    let start = Instant::now();
    let grim_output = Command::new("grim")
        .arg("-l")
        .arg("0")
        .arg("-g")
        .arg(format!("{},{} {}x{}", x_min, y_min, x_max, y_max))
        .arg(temp.path())
        .output()
        .expect("Spawning grim failed");
    let end = Instant::now();
    debug!("Grim took: {:#?}", end - start);

    if !grim_output.status.success() {
        panic!(
            "Error running grim: {}",
            String::from_utf8(grim_output.stderr).expect("Couldnt read grim output")
        );
    }
}
fn run_modifier(settings: &Config, temp: &NamedTempFile) {
    let mut modify_program: String = settings
        .get("external_command")
        .expect("Couldnt read 'external_command' setting from the config file");

    modify_program = modify_program
        .replace("$IN", temp.path().to_str().unwrap())
        .replace("$OUT", temp.path().to_str().unwrap());

    let mut modified = modify_program.split_whitespace();

    let start = Instant::now();
    let mod_output = Command::new(modified.next().expect("Expected program"))
        .args(modified)
        .output()
        .expect("Couldnt run modifier program");
    let end = Instant::now();

    debug!("Modifier took: {:#?}", end - start);

    if !mod_output.status.success() {
        panic!(
            "Couldnt run modifier program: {}",
            String::from_utf8_lossy(&mod_output.stderr)
        )
    }
}

fn main() {
    pretty_env_logger::init();
    let cli = Cli::parse();
    let settings = Config::builder()
        .add_source(File::with_name(&cli.config))
        .build()
        .expect("Couldnt parse configuration file");

    let idle_images = Arc::new(Mutex::new(Vec::new()));
    let temp_files: Arc<Mutex<Vec<NamedTempFile>>> = Arc::new(Mutex::new(Vec::new()));
    let outputs = get_outputs().expect("Failed to get outputs");

    outputs.par_iter().for_each(|output| {
        let temp = NamedTempFile::new().unwrap();

        run_grim(output, &temp);

        run_modifier(&settings, &temp);

        idle_images.lock().unwrap().push(format!(
            "{}:{}",
            output.name,
            temp.path().to_str().unwrap()
        ));
        temp_files.lock().unwrap().push(temp);
    });

    let args = settings.get::<String>("lock_args").unwrap();
    let arg_parts = args.split_whitespace();

    let mut command = Command::new("swaylock");
    command.args(arg_parts);

    for image in idle_images.lock().unwrap().iter() {
        command.arg("-i").arg(image);
    }

    debug!("Running: {:?}", command);

    let output = command.output().expect("Couldnt run swaylock");
    if !output.status.success() {
        panic!(
            "swaylock error: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    }
}
