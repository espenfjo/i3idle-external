use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;

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
    #[clap(flatten)]
    wm: WM,
    /// Configuration file
    #[arg(short, long)]
    config: String,
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
pub struct WM {
    /// Uses i3lock and i3 features
    #[clap(short, long)]
    i3: bool,
    /// Uses swaylock and sway features
    #[clap(short, long)]
    sway: bool,
}

fn get_outputs() -> Result<Vec<Output>, String> {
    let mut connection =
        I3Connection::connect().map_err(|e| format!("Failed to connect to I3/Sway IPC: {e}"))?;
    let outputs = connection
        .get_outputs()
        .map_err(|e| format!("Could not get display outputs: {e}"))?;

    Ok(outputs.outputs)
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
        let x_min = output.rect.0;
        let y_min = output.rect.1;
        let x_max = output.rect.2;
        let y_max = output.rect.3;
        let temp = NamedTempFile::new().unwrap();
        debug!("{}: {},{} {}x{}", output.name, x_min, y_min, x_max, y_max);
        let grim_output = Command::new("grim")
            .arg("-g")
            .arg(format!("{},{} {}x{}", x_min, y_min, x_max, y_max))
            .arg(temp.path())
            .output()
            .expect("Spawning grim failed");
        if !grim_output.status.success() {
            panic!(
                "Error running grim: {}",
                String::from_utf8(grim_output.stderr).expect("Couldnt read grim output")
            );
        }
        let mut modify_program: String = settings
            .get("external_command")
            .expect("Couldnt read 'external_command' setting from the config file");

        modify_program = modify_program
            .replace("$IN", temp.path().to_str().unwrap())
            .replace("$OUT", temp.path().to_str().unwrap());

        let mut modified = modify_program.split_whitespace();
        let mod_output = Command::new(modified.next().expect("Expected program"))
            .args(modified)
            .output()
            .expect("Couldnt run modifier program");
        if !mod_output.status.success() {
            panic!(
                "Couldnt run modifier program: {}",
                String::from_utf8_lossy(&mod_output.stderr)
            )
        }
        idle_images.lock().unwrap().push(format!(
            "{}:{}",
            output.name,
            temp.path().to_str().unwrap()
        ));
        temp_files.lock().unwrap().push(temp);
    });

    let args = settings.get::<String>("lock_args").unwrap();
    let arg_parts = args.split_whitespace();

    let mut command = if cli.wm.sway {
        let mut command = Command::new("swaylock");
        command.args(arg_parts);
        for image in idle_images.lock().unwrap().iter() {
            command.arg("-i").arg(image);
        }
        command
    } else {
        let mut command = Command::new("i3lock");
        command.args(arg_parts);
        for image in idle_images.lock().unwrap().iter() {
            command.arg("-i").arg(image);
        }
        command
    };
    debug!("Running: {:?}", command);
    let output = command.output().expect("Couldnt run lock program");
    if !output.status.success() {
        panic!(
            "Lock program error: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    }
}
