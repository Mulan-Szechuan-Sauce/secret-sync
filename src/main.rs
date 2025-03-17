use clap::Parser;

use kube::CustomResourceExt;
use secret_sync::*;
use secret_sync::crds::*;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
enum Args {
    Run {
        #[arg(long, default_value_t = tracing::Level::INFO)]
        tracing_level: tracing::Level,
    },
    Crds,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args {
        Args::Crds => {
            println!("{}", serde_yaml::to_string(&SyncSecret::crd()).unwrap());
        }
        Args::Run { tracing_level } => {
            tracing_subscriber::fmt()
                .with_max_level(tracing_level)
                .init();

            run().await?;
        }
    };

    Ok(())
}
